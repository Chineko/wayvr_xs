//! Opt-in KWin window capture with compositor-validated desktop input.
mod worker;

use super::{
    backend::{CaptureType, ScreenBackend, pointer_button},
    capture::new_wlx_capture,
};
use crate::{
    backend::input::{HoverResult, PointerHit},
    config::none_if_0,
    gui::panel::{GuiPanel, NewGuiPanelParams},
    state::AppState,
    subsystem::{hid::WheelDelta, input::InputFocus},
    windowing::{
        backend::{FrameMeta, OverlayBackend, OverlayEventData, RenderResources, ShouldRender},
        window::{OverlayCategory, OverlayWindowConfig},
    },
};
use glam::{Affine2, Affine3A, DVec2, Quat, Vec3, vec3};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use wgui::{event::CallbackDataCommon, parser::Fetchable, widget::label::WidgetLabel};
use wlx_capture::{frame::Transform, pipewire::capture::PipewireCapture};
use wlx_common::{
    overlays::{BackendAttrib, BackendAttribValue},
    windowing::OverlayWindowState,
};
use worker::{Event, Reply, Request, WindowChoice, Worker};

#[derive(Default)]
struct PickerState {
    windows: Vec<WindowChoice>,
    page: usize,
    selected: Option<String>,
    dirty: bool,
}
const PAGE_SIZE: usize = 6;

#[derive(Clone, Copy)]
enum Action {
    Move,
    Press(u16),
    Scroll(f32, f32),
}
struct Pending {
    action: Action,
    pointer: usize,
    sent: Instant,
}

#[derive(Default)]
struct InputState {
    down: [bool; 2],
    held: Option<(usize, u16)>,
}
impl InputState {
    const fn accept_press(&mut self, pointer: usize, button: u16) -> bool {
        if pointer >= 2 || !self.down[pointer] || self.held.is_some() {
            return false;
        }
        self.held = Some((pointer, button));
        true
    }
    fn release(&mut self, pointer: Option<usize>) -> Option<u16> {
        if let Some(pointer) = pointer {
            self.down[pointer] = false;
        } else {
            self.down = [false; 2];
        }
        if self
            .held
            .is_some_and(|(owner, _)| pointer.is_none_or(|p| p == owner))
        {
            self.held.take().map(|(_, button)| button)
        } else {
            None
        }
    }
}

struct InteractiveBackend {
    worker: Worker,
    loading: GuiPanel<Rc<RefCell<PickerState>>>,
    screen: Option<ScreenBackend>,
    name: Arc<str>,
    pending: HashMap<u64, Pending>,
    input: InputState,
    next_seq: u64,
    last_move: Instant,
    paused: bool,
    failed: bool,
}

pub fn new_interactive_window(
    name: Arc<str>,
    app: &mut AppState,
) -> anyhow::Result<OverlayWindowConfig> {
    let picker = Rc::new(RefCell::new(PickerState::default()));
    let mut loading = GuiPanel::new_from_template(
        app,
        "gui/kde-window-picker.xml",
        picker.clone(),
        NewGuiPanelParams::default(),
    )?;
    for index in 0..PAGE_SIZE {
        let state = picker.clone();
        loading
            .parser_state
            .fetch_component_as::<wgui::components::button::ComponentButton>(&format!(
                "choice_{index}"
            ))?
            .on_click(Rc::new(move |_, _| {
                let mut state = state.borrow_mut();
                state.selected = state
                    .windows
                    .get(state.page * PAGE_SIZE + index)
                    .map(|w| w.uuid.clone());
                Ok(())
            }));
    }
    for (id, next) in [("previous", false), ("next", true)] {
        let state = picker.clone();
        loading
            .parser_state
            .fetch_component_as::<wgui::components::button::ComponentButton>(id)?
            .on_click(Rc::new(move |_, _| {
                let mut state = state.borrow_mut();
                let last = state.windows.len().saturating_sub(1) / PAGE_SIZE;
                state.page = if next {
                    (state.page + 1).min(last)
                } else {
                    state.page.saturating_sub(1)
                };
                state.dirty = true;
                Ok(())
            }));
    }
    loading.update_layout(app)?;
    let backend = InteractiveBackend {
        worker: Worker::start(),
        loading,
        screen: None,
        name: name.clone(),
        pending: HashMap::new(),
        input: InputState::default(),
        next_seq: 1,
        last_move: Instant::now(),
        paused: false,
        failed: false,
    };
    Ok(OverlayWindowConfig {
        name,
        category: OverlayCategory::Mirror,
        show_on_spawn: true,
        input_focus: None, // Route the virtual keyboard only after a validated click.
        default_state: OverlayWindowState {
            positioning: app.session.config.default_positioning.into(),
            curvature: none_if_0(app.session.config.default_curvature),
            alpha: app.session.config.default_opacity,
            interactable: true,
            grabbable: true,
            transform: Affine3A::from_scale_rotation_translation(
                Vec3::ONE * app.session.config.default_overlay_scale,
                Quat::IDENTITY,
                vec3(0.0, 0.2, -0.35),
            ),
            ..Default::default()
        },
        ..OverlayWindowConfig::from_backend(Box::new(backend))
    })
}

impl InteractiveBackend {
    fn release(&mut self, app: &mut AppState, pointer: Option<usize>) {
        self.pending
            .retain(|_, request| pointer.is_some_and(|p| request.pointer != p));
        if let Some(button) = self.input.release(pointer) {
            app.hid_provider.inner.send_button(button, false);
            app.hid_provider.inner.commit();
        }
    }
    fn request(&mut self, hit: &PointerHit, action: Action) {
        if self.paused || self.failed || hit.pointer >= 2 || !hit.uv.is_finite() {
            return;
        }
        if self
            .input
            .held
            .is_some_and(|(owner, _)| owner != hit.pointer)
        {
            return;
        }
        let Some(extent) = self
            .screen
            .as_mut()
            .and_then(OverlayBackend::frame_meta)
            .map(|m| m.extent)
        else {
            return;
        };
        let seq = self.next_seq;
        self.next_seq += 1;
        let request = Request {
            seq,
            uv: hit.uv.to_array(),
            extent,
            activate: matches!(action, Action::Press(_) | Action::Scroll(..)),
        };
        if self.pending.len() >= 16 {
            return;
        }
        if self.worker.requests.try_send(request).is_ok() {
            self.pending.insert(
                seq,
                Pending {
                    action,
                    pointer: hit.pointer,
                    sent: Instant::now(),
                },
            );
        }
    }
    fn apply_reply(&mut self, app: &mut AppState, reply: Reply) {
        if !reply.alive || (!reply.active && self.input.held.is_some()) {
            self.release(app, None);
            return;
        }
        if !reply.active {
            self.pending.remove(&reply.seq);
            return;
        }
        let Some(pending) = self.pending.remove(&reply.seq) else {
            return;
        };
        if !reply.valid
            || !reply.x.is_finite()
            || !reply.y.is_finite()
            || pending.sent.elapsed() > Duration::from_millis(150)
            || self.paused
        {
            return;
        }
        if self
            .input
            .held
            .is_some_and(|(owner, _)| owner != pending.pointer)
        {
            return;
        }
        // Each reply is for this selected UUID and a compositor-owned input point.
        // Flush the mapped motion before the action, avoiding another overlay's
        // previously queued pointer position taking precedence in the HID provider.
        app.hid_provider.inner.commit();
        app.hid_provider
            .inner
            .mouse_move(DVec2::new(reply.x, reply.y));
        match pending.action {
            Action::Move => {}
            Action::Press(button) => {
                if self.input.accept_press(pending.pointer, button) {
                    app.hid_provider
                        .set_input_focus(app.wvr_server.as_mut(), InputFocus::PhysicalScreen);
                    app.hid_provider.inner.send_button(button, true);
                }
            }
            Action::Scroll(x, y) => app.hid_provider.inner.wheel(WheelDelta {
                x: x * 60.0,
                y: y * 60.0,
            }),
        }
        app.hid_provider.inner.commit();
    }
    fn fail(&mut self, app: &mut AppState, reason: &str) {
        log::warn!("{}: {reason}", self.name);
        self.release(app, None);
        self.failed = true;
        if let Some(screen) = self.screen.as_mut() {
            let _ = screen.pause(app);
        }
        self.screen = None;
        if let Ok(mut status) = self
            .loading
            .parser_state
            .fetch_widget_as::<WidgetLabel>(&self.loading.layout.state, "status")
        {
            status.set_text(
                &mut CallbackDataCommon {
                    state: &self.loading.layout.state,
                    alterables: &mut self.loading.layout.alterables,
                },
                wgui::i18n::Translation::from_raw_text(reason),
            );
        }
    }
}

impl OverlayBackend for InteractiveBackend {
    fn init(&mut self, app: &mut AppState) -> anyhow::Result<()> {
        self.loading.init(app)
    }
    fn should_render(&mut self, app: &mut AppState) -> anyhow::Result<ShouldRender> {
        loop {
            let event = match self.worker.events.try_recv() {
                Ok(event) => event,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if !self.failed {
                        self.fail(app, "The KDE input bridge disconnected");
                    }
                    break;
                }
            };
            match event {
                Event::Choices(windows) => {
                    let mut state = self.loading.state.borrow_mut();
                    state.windows = windows;
                    state.dirty = true;
                }
                Event::Ready(node) => {
                    let capture =
                        new_wlx_capture!(app, PipewireCapture::new(self.name.clone(), node));
                    let mut screen = ScreenBackend::new_raw(
                        self.name.clone(),
                        app.feats.xr_backend,
                        CaptureType::PipeWire,
                        capture,
                    );
                    screen.apply_mouse_transform_with_override(Transform::Undefined);
                    screen.init(app)?;
                    if self.paused {
                        screen.pause(app)?;
                    }
                    self.screen = Some(screen);
                }
                Event::Reply(reply) => self.apply_reply(app, reply),
                Event::Failed(reason) => self.fail(app, &reason),
            }
        }
        if self.screen.is_none() && !self.failed {
            let (selection, labels) = {
                let mut state = self.loading.state.borrow_mut();
                let selection = state.selected.take();
                let labels = if state.dirty {
                    state.dirty = false;
                    let mut labels = vec![(
                        "status".to_string(),
                        if state.windows.is_empty() {
                            "No app windows found. Open an app, then add a new panel.".to_string()
                        } else {
                            format!(
                                "Choose an app — page {} / {}",
                                state.page + 1,
                                state.windows.len().div_ceil(PAGE_SIZE)
                            )
                        },
                    )];
                    for i in 0..PAGE_SIZE {
                        let text = state.windows.get(state.page * PAGE_SIZE + i).map_or_else(
                            || "—".to_string(),
                            |w| {
                                format!(
                                    "[{}] {}",
                                    w.output,
                                    w.title.chars().take(64).collect::<String>()
                                )
                            },
                        );
                        labels.push((format!("title_{i}"), text));
                    }
                    Some(labels)
                } else {
                    None
                };
                (selection, labels)
            };
            if let Some(uuid) = selection {
                let _ = self.worker.selection.send(uuid);
            }
            if let Some(labels) = labels {
                for (id, text) in labels {
                    if let Ok(mut label) = self
                        .loading
                        .parser_state
                        .fetch_widget_as::<WidgetLabel>(&self.loading.layout.state, &id)
                    {
                        label.set_text(
                            &mut CallbackDataCommon {
                                state: &self.loading.layout.state,
                                alterables: &mut self.loading.layout.alterables,
                            },
                            wgui::i18n::Translation::from_raw_text(&text),
                        );
                    }
                }
                self.loading.update_layout(app)?;
            }
        }
        self.pending
            .retain(|_, p| p.sent.elapsed() <= Duration::from_millis(150));
        if let Some(screen) = self.screen.as_mut() {
            screen.should_render(app)
        } else {
            self.loading.should_render(app)
        }
    }
    fn render(&mut self, app: &mut AppState, rdr: &mut RenderResources) -> anyhow::Result<()> {
        if let Some(screen) = self.screen.as_mut() {
            screen.render(app, rdr)
        } else {
            self.loading.render(app, rdr)
        }
    }
    fn frame_meta(&mut self) -> Option<FrameMeta> {
        self.screen
            .as_mut()
            .map_or_else(|| self.loading.frame_meta(), OverlayBackend::frame_meta)
    }
    fn pause(&mut self, app: &mut AppState) -> anyhow::Result<()> {
        self.worker.set_enabled(false);
        self.paused = true;
        self.release(app, None);
        if let Some(screen) = self.screen.as_mut() {
            screen.pause(app)?;
        }
        self.loading.pause(app)
    }
    fn resume(&mut self, app: &mut AppState) -> anyhow::Result<()> {
        self.worker.set_enabled(true);
        self.paused = false;
        if let Some(screen) = self.screen.as_mut() {
            screen.resume(app)?;
        }
        self.loading.resume(app)
    }
    fn on_hover(&mut self, app: &mut AppState, hit: &PointerHit) -> HoverResult {
        if self.screen.is_none() {
            return self.loading.on_hover(app, hit);
        }
        if hit.pointer < 2
            && (!app.session.config.focus_follows_mouse_mode
                || app.input_state.pointers[hit.pointer].now.move_mouse)
            && self.last_move.elapsed()
                >= Duration::from_millis((app.session.config.mouse_move_interval_ms.max(33)) as u64)
        {
            self.request(hit, Action::Move);
            self.last_move = Instant::now();
        }
        HoverResult::consume()
    }
    fn on_pointer(&mut self, app: &mut AppState, hit: &PointerHit, pressed: bool) {
        if self.screen.is_none() {
            self.loading.on_pointer(app, hit, pressed);
            return;
        }
        if hit.pointer >= 2 {
            return;
        }
        if !pressed {
            self.release(app, Some(hit.pointer));
            return;
        }
        self.input.down[hit.pointer] = true;
        let button = pointer_button(hit.mode, app.session.config.left_handed_mouse);
        self.request(hit, Action::Press(button));
    }
    fn on_left(&mut self, app: &mut AppState, pointer: usize) {
        if self.screen.is_none() {
            self.loading.on_left(app, pointer);
        }
        if pointer < 2 {
            self.release(app, Some(pointer));
        }
    }
    fn on_scroll(&mut self, _app: &mut AppState, hit: &PointerHit, delta: WheelDelta) {
        self.request(hit, Action::Scroll(delta.x, delta.y));
    }
    fn notify(&mut self, app: &mut AppState, event: OverlayEventData) -> anyhow::Result<()> {
        if let Some(screen) = self.screen.as_mut() {
            screen.notify(app, event)
        } else {
            self.loading.notify(app, event)
        }
    }
    fn get_interaction_transform(&mut self) -> Option<Affine2> {
        self.screen.as_mut().map_or_else(
            || self.loading.get_interaction_transform(),
            OverlayBackend::get_interaction_transform,
        )
    }
    // No desktop split, stereo or mouse-rotation options: input uses the exact
    // untransformed window stream validated by KWin.
    fn get_attrib(&self, _: BackendAttrib) -> Option<BackendAttribValue> {
        None
    }
    fn set_attrib(&mut self, _: &mut AppState, _: BackendAttribValue) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::InputState;

    #[test]
    fn window_picker_layout_exposes_clickable_rows() {
        use wgui::{assets::AssetPathRef, components::button::ComponentButton, parser::Fetchable};
        let globals = wgui::globals::WguiGlobals::new(
            Box::new(crate::gui::asset::GuiAsset {}),
            &wlx_common::locale::WayVRLangProvider::default(),
            &wgui::font_config::WguiFontConfig::default(),
            std::path::PathBuf::from("/nonexistent-wayvr-test-assets"),
            wgui::palette::WguiColorPalette::get_builtin("default"),
        )
        .unwrap();
        let (_, parser) = wgui::parser::new_layout_from_assets(
            &wgui::parser::ParseDocumentParams {
                globals,
                path: AssetPathRef::BuiltIn("gui/kde-window-picker.xml"),
                extra: Default::default(),
            },
            Default::default(),
        )
        .unwrap();
        for id in (0..super::PAGE_SIZE)
            .map(|i| format!("choice_{i}"))
            .chain(["previous".into(), "next".into()])
        {
            parser.fetch_component_as::<ComponentButton>(&id).unwrap();
        }
    }

    #[test]
    fn released_trigger_cannot_deliver_a_late_press() {
        let mut state = InputState::default();
        state.down[0] = true;
        assert_eq!(state.release(Some(0)), None);
        assert!(!state.accept_press(0, 0x110));
    }
    #[test]
    fn holds_have_one_owner_and_release_once() {
        let mut state = InputState::default();
        state.down = [true, true];
        assert!(state.accept_press(0, 0x110));
        assert!(!state.accept_press(1, 0x111));
        assert_eq!(state.release(Some(1)), None);
        assert_eq!(state.release(None), Some(0x110));
        assert_eq!(state.release(None), None);
    }
}
