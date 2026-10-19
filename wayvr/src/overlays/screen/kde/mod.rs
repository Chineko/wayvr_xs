//! Opt-in KWin window capture with compositor-validated desktop input.
mod geometry;
mod worker;
use geometry::PanelGeometry;

use super::{
    backend::{CaptureType, ScreenBackend, pointer_button},
    capture::new_wlx_capture,
};
use crate::{
    backend::{
        input::{HoverResult, PointerHit},
        task::{OverlayTask, TaskType, ToggleMode},
    },
    config::none_if_0,
    gui::panel::{GuiPanel, NewGuiPanelParams},
    state::AppState,
    subsystem::{hid::WheelDelta, input::InputFocus},
    windowing::{
        OverlaySelector,
        backend::{FrameMeta, OverlayBackend, OverlayEventData, RenderResources, ShouldRender},
        window::{OverlayCategory, OverlayWindowConfig},
    },
};
use glam::{Affine2, Affine3A, DVec2, Quat, Vec3, vec2, vec3};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use wgui::{
    event::{CallbackDataCommon, StyleSetRequest},
    parser::Fetchable,
    taffy::{Display, prelude::length},
    widget::label::WidgetLabel,
};
use wlx_capture::{frame::Transform, pipewire::capture::PipewireCapture};
use wlx_common::{
    app_panel::{AppIdentity, SavedAppPanel, reconnect_index},
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
    command: Option<PickerCommand>,
    message: Option<String>,
}
const PAGE_SIZE: usize = 6;
#[derive(Clone, Copy)]
enum PickerCommand {
    Toggle,
    Refresh,
    Reconnect,
    Quick,
    Adjust(&'static str),
}

pub fn new_panel_name() -> Arc<str> {
    format!("XSApp-{}", uuid::Uuid::new_v4()).into()
}

impl WindowChoice {
    fn identity(&self) -> AppIdentity {
        AppIdentity {
            app_id: self.app_id.clone(),
            title: self.title.clone(),
        }
    }
}

fn drain_events(worker: &Worker) -> Vec<Event> {
    let mut events = Vec::new();
    loop {
        match worker.events.try_recv() {
            Ok(event) => events.push(event),
            Err(std::sync::mpsc::TryRecvError::Empty) => break,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if !events.iter().any(|event| matches!(event, Event::Failed(_))) {
                    events.push(Event::Failed("The KDE input bridge disconnected".into()));
                }
                break;
            }
        }
    }
    events
}

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
    picker_worker: Option<Worker>,
    next_app: Option<WindowChoice>,
    restore_app: Option<AppIdentity>,
    current_app: Option<AppIdentity>,
    expanded: bool,
    quick: bool,
    geometry: PanelGeometry,
    gui_down: [bool; 2],
    loading: GuiPanel<Rc<RefCell<PickerState>>>,
    screen: Option<ScreenBackend>,
    name: Arc<str>,
    pending: HashMap<u64, Pending>,
    input: InputState,
    next_seq: u64,
    last_move: Instant,
    paused: bool,
    failed: bool,
    reconnect_notice: Option<String>,
}

pub fn new_interactive_window(
    name: Arc<str>,
    app: &mut AppState,
) -> anyhow::Result<OverlayWindowConfig> {
    let restore_app = app
        .session
        .config
        .app_panels
        .iter()
        .find(|p| p.name == name)
        .and_then(|p| p.app.clone());
    let picker = Rc::new(RefCell::new(PickerState::default()));
    let mut loading = GuiPanel::new_from_template(
        app,
        "gui/kde-window-picker.xml",
        picker.clone(),
        NewGuiPanelParams {
            // The capture fixes the render viewport. Shrink-wrapping the GUI
            // makes GuiPanel map pointer UVs against a different width.
            resize_to_parent: true,
            ..Default::default()
        },
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
    for (id, command) in [
        ("change_app", PickerCommand::Toggle),
        ("refresh", PickerCommand::Refresh),
        ("reconnect", PickerCommand::Reconnect),
        ("quick_toggle", PickerCommand::Quick),
        ("close_panel", PickerCommand::Adjust("close")),
    ] {
        let state = picker.clone();
        loading
            .parser_state
            .fetch_component_as::<wgui::components::button::ComponentButton>(id)?
            .on_click(Rc::new(move |_, _| {
                state.borrow_mut().command = Some(command);
                Ok(())
            }));
    }
    for action in [
        "width_down",
        "width_up",
        "nearer",
        "farther",
        "opacity_down",
        "opacity_up",
        "curve_down",
        "curve_up",
        "pin",
        "recenter",
        "hide",
    ] {
        let state = picker.clone();
        loading
            .parser_state
            .fetch_component_as::<wgui::components::button::ComponentButton>(&format!(
                "quick_{action}"
            ))?
            .on_click(Rc::new(move |_, _| {
                state.borrow_mut().command = Some(PickerCommand::Adjust(action));
                Ok(())
            }));
    }
    loading.max_size = vec2(1280., 792.);
    loading.update_layout(app)?;
    let backend = InteractiveBackend {
        worker: Worker::start(),
        picker_worker: None,
        next_app: None,
        current_app: restore_app.clone(),
        restore_app,
        expanded: true,
        quick: false,
        geometry: PanelGeometry::new(None, true, false),
        gui_down: [false; 2],
        loading,
        screen: None,
        name: name.clone(),
        pending: HashMap::new(),
        input: InputState::default(),
        next_seq: 1,
        last_move: Instant::now(),
        paused: false,
        failed: false,
        reconnect_notice: None,
    };
    if !app.session.config.app_panels.iter().any(|p| p.name == name) {
        app.session.config.app_panels.push(SavedAppPanel {
            name: name.clone(),
            app: None,
        });
    }
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
        if self.paused || self.failed || self.expanded || hit.pointer >= 2 || !hit.uv.is_finite() {
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
        if !reply.alive {
            self.fail(app, "The selected app window closed");
            return;
        }
        if !reply.active && self.input.held.is_some() {
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
    fn set_label(&mut self, id: &str, text: &str) {
        if let Ok(mut label) = self
            .loading
            .parser_state
            .fetch_widget_as::<WidgetLabel>(&self.loading.layout.state, id)
        {
            label.set_text(
                &mut CallbackDataCommon {
                    state: &self.loading.layout.state,
                    alterables: &mut self.loading.layout.alterables,
                },
                wgui::i18n::Translation::from_raw_text(text),
            );
        }
    }
    fn open_picker(&mut self, app: &mut AppState) {
        self.release(app, None);
        self.worker.set_enabled(false);
        self.expanded = true;
        self.restore_app = None; // Manual changes never auto-select a saved match.
        self.next_app = None;
        if self.screen.is_some() {
            self.picker_worker = Some(Worker::start());
        } else {
            self.picker_worker = None;
            self.worker = Worker::start();
            self.failed = false;
        }
        let mut state = self.loading.state.borrow_mut();
        state.windows.clear();
        state.selected = None;
        state.page = 0;
        state.message = Some("Loading app windows…".into());
        state.dirty = true;
    }
    fn choices(&mut self, windows: Vec<WindowChoice>) {
        let mut state = self.loading.state.borrow_mut();
        if let Some(saved) = self.restore_app.take() {
            let identities: Vec<_> = windows.iter().map(WindowChoice::identity).collect();
            if let Some(index) = reconnect_index(&saved, &identities) {
                state.selected = Some(windows[index].uuid.clone());
            } else {
                let matches = identities
                    .iter()
                    .filter(|w| !saved.app_id.is_empty() && w.app_id == saved.app_id)
                    .count();
                state.message = Some(if matches > 1 {
                    "Several matching windows — choose one explicitly; the panel will not guess."
                        .into()
                } else {
                    "Saved app is unavailable. Open it and use Reconnect saved app, or choose a replacement.".into()
                });
            }
        } else {
            state.message.clone_from(&self.reconnect_notice);
        }
        state.windows = windows;
        state.dirty = true;
    }
    fn ready(&mut self, app: &mut AppState, node: u32) -> anyhow::Result<()> {
        self.release(app, None);
        let capture = new_wlx_capture!(app, PipewireCapture::new(self.name.clone(), node));
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
        if let Some(old) = self.screen.as_mut() {
            old.pause(app)?;
        }
        if let Some(worker) = self.picker_worker.take() {
            self.worker = worker;
        }
        self.screen = Some(screen);
        self.failed = false;
        self.reconnect_notice = None;
        self.expanded = false;
        self.worker.set_enabled(!self.paused);
        if let Some(choice) = self.next_app.take() {
            self.current_app = Some(choice.identity());
            if let Some(saved) = app
                .session
                .config
                .app_panels
                .iter_mut()
                .find(|p| p.name == self.name)
            {
                saved.app.clone_from(&self.current_app);
            }
            // Save the app choice now. The existing layout lifecycle saves
            // poses on leaving edit mode and on normal exit, without pausing
            // other panels or interrupting an in-progress grab here.
            if let Err(error) = crate::config::save_state(&app.session.config) {
                log::warn!("Could not save app panel choice: {error}");
            }
        }
        Ok(())
    }
    fn update_gui(&mut self, app: &mut AppState) -> anyhow::Result<bool> {
        let content = self
            .screen
            .as_mut()
            .and_then(OverlayBackend::frame_meta)
            .map(|m| m.extent)
            .or_else(|| {
                (self.current_app.is_some() && self.geometry.content[1] > 0)
                    .then_some(self.geometry.content)
            });
        let geometry = PanelGeometry::new(content, self.expanded || content.is_none(), self.quick);
        let changed = self.geometry != geometry;
        self.geometry = geometry;
        if changed {
            let scale = geometry.full[0] as f32 / 1280.;
            self.loading.gui_scale = scale;
            self.loading.max_size = vec2(geometry.full[0] as f32, geometry.full[1] as f32);
            let space = self.loading.parser_state.get_widget_id("content_space")?;
            let picker = self.loading.parser_state.get_widget_id("picker")?;
            let quick = self.loading.parser_state.get_widget_id("quick_controls")?;
            self.loading.layout.alterables.set_style(
                quick,
                StyleSetRequest::Display(if self.quick {
                    Display::Flex
                } else {
                    Display::None
                }),
            );
            self.loading.layout.alterables.set_style(
                space,
                StyleSetRequest::Height(length(geometry.content[1] as f32 / scale)),
            );
            self.loading.layout.alterables.set_style(
                picker,
                StyleSetRequest::Display(if self.expanded || content.is_none() {
                    Display::Flex
                } else {
                    Display::None
                }),
            );
            self.loading.update_layout(app)?;
        }
        let caption = self.current_app.as_ref().map_or_else(
            || "Change app ▾".to_owned(),
            |a| {
                format!(
                    "{}{} ▾",
                    if self.screen.is_none() {
                        "Reconnect: "
                    } else {
                        ""
                    },
                    a.title.chars().take(64).collect::<String>()
                )
            },
        );
        self.set_label("current_app", &caption);
        self.set_label(
            "connection_state",
            if self.screen.is_none() && self.current_app.is_some() {
                "App disconnected — choose a replacement below"
            } else {
                ""
            },
        );
        self.loading.update_layout(app)?;
        Ok(changed)
    }
    fn fail(&mut self, app: &mut AppState, reason: &str) {
        if self.failed {
            return;
        }
        log::warn!("{}: {reason}", self.name);
        self.release(app, None);
        self.worker.set_enabled(false);
        self.failed = true;
        if let Some(screen) = self.screen.as_mut() {
            let _ = screen.pause(app);
        }
        self.screen = None;
        self.expanded = true;
        self.reconnect_notice = Some(format!(
            "Disconnected: {reason}. Choose a replacement or reconnect the saved app."
        ));
        self.picker_worker = Some(Worker::start());
        let mut state = self.loading.state.borrow_mut();
        state.windows.clear();
        state.selected = None;
        state.page = 0;
        state.message.clone_from(&self.reconnect_notice);
        state.dirty = true;
    }
}

impl OverlayBackend for InteractiveBackend {
    fn init(&mut self, app: &mut AppState) -> anyhow::Result<()> {
        self.loading.init(app)
    }
    fn should_render(&mut self, app: &mut AppState) -> anyhow::Result<ShouldRender> {
        // Drain each session separately. Replies from a candidate picker never
        // reach HID, and promotion drops the old session and its pending input.
        let events = if self.failed {
            Vec::new()
        } else {
            drain_events(&self.worker)
        };
        for event in events {
            match event {
                Event::Choices(windows) => self.choices(windows),
                Event::Selected(choice) => self.next_app = Some(choice),
                Event::Ready(node) => self.ready(app, node)?,
                Event::Reply(reply) => {
                    if !self.expanded {
                        self.apply_reply(app, reply);
                    }
                }
                Event::Failed(reason) => {
                    self.fail(app, &reason);
                }
            }
        }
        let events: Vec<_> = self
            .picker_worker
            .as_ref()
            .map_or_else(Vec::new, drain_events);
        for event in events {
            match event {
                Event::Choices(windows) => self.choices(windows),
                Event::Selected(choice) => self.next_app = Some(choice),
                Event::Ready(node) => self.ready(app, node)?,
                Event::Failed(reason) => {
                    self.picker_worker = None;
                    let mut state = self.loading.state.borrow_mut();
                    state.windows.clear();
                    state.message = Some(reason);
                    state.dirty = true;
                }
                Event::Reply(_) => {} // Candidate has not been assigned desktop input.
            }
        }
        let command = self.loading.state.borrow_mut().command.take();
        if let Some(command) = command {
            if matches!(command, PickerCommand::Reconnect) {
                self.open_picker(app);
                self.restore_app.clone_from(&self.current_app);
            } else if matches!(command, PickerCommand::Quick) {
                self.release(app, None);
                self.quick = !self.quick;
            } else if let PickerCommand::Adjust(action) = command {
                self.release(app, None);
                let selector = OverlaySelector::Name(self.name.clone());
                if action == "hide" {
                    app.tasks
                        .enqueue(TaskType::Overlay(OverlayTask::ToggleOverlay(
                            selector,
                            ToggleMode::EnsureOff,
                        )));
                } else if action == "close" {
                    app.tasks
                        .enqueue(TaskType::Overlay(OverlayTask::Drop(selector)));
                } else {
                    app.tasks.enqueue(TaskType::Overlay(OverlayTask::Modify(
                        selector,
                        Box::new(move |app, owc| {
                            crate::overlays::edit::apply_quick_action(app, owc, action);
                        }),
                    )));
                }
            } else if matches!(command, PickerCommand::Toggle)
                && self.expanded
                && self.screen.is_some()
            {
                self.picker_worker = None;
                self.next_app = None;
                self.expanded = false;
                self.worker.set_enabled(!self.paused);
            } else {
                self.open_picker(app);
            }
        }
        let selection = self.loading.state.borrow_mut().selected.take();
        if let Some(uuid) = selection {
            self.release(app, None);
            let worker = self.picker_worker.as_ref().unwrap_or(&self.worker);
            let _ = worker.selection.send(uuid);
            self.loading.state.borrow_mut().message = Some("Opening app…".into());
            self.loading.state.borrow_mut().dirty = true;
        }
        let labels = {
            let mut state = self.loading.state.borrow_mut();
            if state.dirty {
                state.dirty = false;
                let status = state.message.clone().unwrap_or_else(|| {
                    if state.windows.is_empty() {
                        "No apps found. Open an app and press Refresh.".into()
                    } else {
                        format!(
                            "Choose an app — page {} / {}",
                            state.page + 1,
                            state.windows.len().div_ceil(PAGE_SIZE)
                        )
                    }
                });
                let mut labels = vec![("status".to_string(), status)];
                for i in 0..PAGE_SIZE {
                    labels.push((
                        format!("title_{i}"),
                        state.windows.get(state.page * PAGE_SIZE + i).map_or_else(
                            || "—".into(),
                            |w| {
                                let duplicate = state
                                    .windows
                                    .iter()
                                    .filter(|other| {
                                        other.app_id == w.app_id && other.title == w.title
                                    })
                                    .count()
                                    > 1;
                                format!(
                                    "[{}] {}{}",
                                    w.output,
                                    w.title.chars().take(64).collect::<String>(),
                                    if duplicate {
                                        format!(" · {}", w.uuid.chars().take(8).collect::<String>())
                                    } else {
                                        String::new()
                                    }
                                )
                            },
                        ),
                    ));
                }
                labels
            } else {
                Vec::new()
            }
        };
        for (id, text) in labels {
            self.set_label(&id, &text);
        }
        self.pending
            .retain(|_, p| p.sent.elapsed() <= Duration::from_millis(150));
        let inner = if let Some(screen) = self.screen.as_mut() {
            screen.should_render(app)?
        } else {
            ShouldRender::Unable
        };
        let geometry_changed = self.update_gui(app)?;
        let gui = self.loading.should_render(app)?;
        // Recreate and redraw the swapchain in the same frame as its new pose.
        // Reusing the old image with the new aspect stretches a static app.
        if geometry_changed {
            return Ok(ShouldRender::Should);
        }
        Ok(match (inner, gui) {
            (ShouldRender::Should, _) | (_, ShouldRender::Should) => ShouldRender::Should,
            (_, ShouldRender::Can) => ShouldRender::Can,
            _ => ShouldRender::Unable,
        })
    }
    fn render(&mut self, app: &mut AppState, rdr: &mut RenderResources) -> anyhow::Result<()> {
        if let Some(screen) = self.screen.as_mut()
            && screen.frame_meta().is_some()
        {
            screen.render(app, rdr)?;
        }
        self.loading.render(app, rdr)
    }
    fn frame_meta(&mut self) -> Option<FrameMeta> {
        let mut meta = self
            .screen
            .as_mut()
            .and_then(OverlayBackend::frame_meta)
            .unwrap_or_default();
        meta.extent = self.geometry.full;
        meta.transform = self.geometry.view_transform();
        meta.clear = wgui::gfx::cmd::WGfxClearMode::Clear([0., 0., 0., 0.]);
        Some(meta)
    }
    fn pause(&mut self, app: &mut AppState) -> anyhow::Result<()> {
        self.worker.set_enabled(false);
        if let Some(worker) = self.picker_worker.as_ref() {
            worker.set_enabled(false);
        }
        self.paused = true;
        self.release(app, None);
        if let Some(screen) = self.screen.as_mut() {
            screen.pause(app)?;
        }
        self.loading.pause(app)
    }
    fn resume(&mut self, app: &mut AppState) -> anyhow::Result<()> {
        self.worker.set_enabled(!self.expanded);
        if let Some(worker) = self.picker_worker.as_ref() {
            worker.set_enabled(true);
        }
        self.paused = false;
        if let Some(screen) = self.screen.as_mut() {
            screen.resume(app)?;
        }
        self.loading.resume(app)
    }
    fn on_hover(&mut self, app: &mut AppState, hit: &PointerHit) -> HoverResult {
        if let Some(uv) = self.geometry.content_uv(hit.uv)
            && !self.expanded
            && !self.failed
        {
            self.loading.on_left(app, hit.pointer);
            if hit.pointer < 2
                && (!app.session.config.focus_follows_mouse_mode
                    || app.input_state.pointers[hit.pointer].now.move_mouse)
                && self.last_move.elapsed()
                    >= Duration::from_millis(
                        app.session.config.mouse_move_interval_ms.max(33) as u64
                    )
            {
                self.request(&PointerHit { uv, ..*hit }, Action::Move);
                self.last_move = Instant::now();
            }
            HoverResult::consume()
        } else {
            if hit.pointer < 2 {
                self.release(app, Some(hit.pointer));
            }
            self.loading.on_hover(app, hit)
        }
    }
    fn on_pointer(&mut self, app: &mut AppState, hit: &PointerHit, pressed: bool) {
        if hit.pointer >= 2 {
            return;
        }
        if !pressed {
            self.release(app, Some(hit.pointer));
            if self.gui_down[hit.pointer] {
                self.loading.on_pointer(app, hit, false);
                self.gui_down[hit.pointer] = false;
            }
            return;
        }
        if let Some(uv) = self.geometry.content_uv(hit.uv)
            && !self.expanded
            && !self.failed
        {
            self.input.down[hit.pointer] = true;
            self.request(
                &PointerHit { uv, ..*hit },
                Action::Press(pointer_button(
                    hit.mode,
                    app.session.config.left_handed_mouse,
                )),
            );
        } else {
            self.release(app, Some(hit.pointer));
            self.gui_down[hit.pointer] = true;
            self.loading.on_pointer(app, hit, true);
        }
    }
    fn on_left(&mut self, app: &mut AppState, pointer: usize) {
        self.loading.on_left(app, pointer);
        if pointer < 2 {
            self.release(app, Some(pointer));
            self.gui_down[pointer] = false;
        }
    }
    fn on_scroll(&mut self, app: &mut AppState, hit: &PointerHit, delta: WheelDelta) {
        if let Some(uv) = self.geometry.content_uv(hit.uv)
            && !self.expanded
            && !self.failed
        {
            self.request(&PointerHit { uv, ..*hit }, Action::Scroll(delta.x, delta.y));
        } else {
            self.loading.on_scroll(app, hit, delta);
        }
    }
    fn notify(&mut self, app: &mut AppState, event: OverlayEventData) -> anyhow::Result<()> {
        if let Some(screen) = self.screen.as_mut() {
            screen.notify(app, event.clone())?;
        }
        self.loading.notify(app, event)
    }
    fn get_interaction_transform(&mut self) -> Option<Affine2> {
        Some(self.geometry.interaction_transform())
    }
    fn interaction_pose(&self) -> Affine3A {
        self.geometry.interaction_pose()
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
        let (mut layout, parser) = wgui::parser::new_layout_from_assets(
            &wgui::parser::ParseDocumentParams {
                globals,
                path: AssetPathRef::BuiltIn("gui/kde-window-picker.xml"),
                extra: Default::default(),
            },
            wgui::layout::LayoutParams {
                resize_to_parent: true,
                ..Default::default()
            },
        )
        .unwrap();
        for id in (0..super::PAGE_SIZE).map(|i| format!("choice_{i}")).chain([
            "previous".into(),
            "next".into(),
            "change_app".into(),
            "refresh".into(),
            "reconnect".into(),
        ]) {
            parser.fetch_component_as::<ComponentButton>(&id).unwrap();
        }
        use wgui::{
            event::StyleSetRequest,
            taffy::{Display, prelude::length},
        };
        let space = parser.get_widget_id("content_space").unwrap();
        let picker = parser.get_widget_id("picker").unwrap();
        let quick = parser.get_widget_id("quick_controls").unwrap();
        layout
            .alterables
            .set_style(quick, StyleSetRequest::Display(Display::None));
        layout
            .alterables
            .set_style(space, StyleSetRequest::Height(length(720_f32)));
        layout
            .update(&mut wgui::layout::LayoutUpdateParams {
                size: glam::vec2(1280., 1512.),
                timestep_alpha: 1.,
            })
            .unwrap();
        let footer = parser.get_widget_id("footer").unwrap();
        let node = *layout.state.nodes.get(footer).unwrap();
        assert!((layout.state.tree.layout(node).unwrap().location.y - 720.).abs() < 0.1);
        assert!((layout.content_size.y - 1512.).abs() < 0.1);
        layout
            .alterables
            .set_style(picker, StyleSetRequest::Display(Display::None));
        layout
            .update(&mut wgui::layout::LayoutUpdateParams {
                size: glam::vec2(1280., 792.),
                timestep_alpha: 1.,
            })
            .unwrap();
        assert!((layout.content_size.y - 792.).abs() < 0.1);
        for (content, quick_open) in [[1920, 1080], [1001, 777], [400, 1000], [2560, 1440]]
            .into_iter()
            .flat_map(|c| [(c, false), (c, true)])
        {
            layout.alterables.set_style(
                quick,
                StyleSetRequest::Display(if quick_open {
                    Display::Flex
                } else {
                    Display::None
                }),
            );
            let g = super::PanelGeometry::new(Some(content), true, quick_open);
            let scale = g.full[0] as f32 / 1280.;
            let viewport = glam::vec2(g.full[0] as f32, g.full[1] as f32) / scale;
            layout.alterables.set_style(
                space,
                StyleSetRequest::Height(length(content[1] as f32 / scale)),
            );
            layout
                .alterables
                .set_style(picker, StyleSetRequest::Display(Display::Flex));
            layout
                .update(&mut wgui::layout::LayoutUpdateParams {
                    size: viewport,
                    timestep_alpha: 1.,
                })
                .unwrap();
            assert!(
                layout.content_size.abs_diff_eq(viewport, 1.),
                "{content:?}: {:?} vs {viewport:?}",
                layout.content_size
            );
            for id in [
                "previous",
                "refresh",
                "next",
                "reconnect",
                "quick_toggle",
                "close_panel",
            ]
            .into_iter()
            .chain(if quick_open {
                vec![
                    "quick_width_down",
                    "quick_width_up",
                    "quick_nearer",
                    "quick_farther",
                    "quick_opacity_down",
                    "quick_opacity_up",
                    "quick_curve_down",
                    "quick_curve_up",
                    "quick_pin",
                    "quick_recenter",
                    "quick_hide",
                ]
            } else {
                vec![]
            }) {
                let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
                let result = clicked.clone();
                parser
                    .fetch_component_as::<ComponentButton>(id)
                    .unwrap()
                    .on_click(std::rc::Rc::new(move |_, _| {
                        result.set(true);
                        Ok(())
                    }));
                use wgui::components::ComponentTrait;
                let widget = parser
                    .fetch_component_as::<ComponentButton>(id)
                    .unwrap()
                    .base()
                    .get_id();
                let node = *layout.state.nodes.get(widget).unwrap();
                let mut pos = layout.state.get_node_size(node) * 0.5;
                let mut ancestor = Some(node);
                while let Some(node) = ancestor {
                    let l = layout.state.tree.layout(node).unwrap();
                    pos += glam::vec2(l.location.x, l.location.y);
                    ancestor = layout.state.tree.parent(node);
                }
                // Rendered pixels -> normalized overlay hit -> GuiPanel event.
                let uv = pos * scale / glam::vec2(g.full[0] as f32, g.full[1] as f32);
                let event = wgui::event::MouseButtonEvent {
                    pos: uv * layout.content_size,
                    index: wgui::event::MouseButtonIndex::Left,
                    device: wgui::event::DeviceBitmask::from_index(0),
                };
                layout
                    .push_event(
                        &wgui::event::Event::MouseMotion(wgui::event::MouseMotionEvent {
                            pos: event.pos,
                            device: event.device,
                        }),
                        &mut (),
                        &mut (),
                    )
                    .unwrap();
                layout
                    .push_event(&wgui::event::Event::MouseDown(event), &mut (), &mut ())
                    .unwrap();
                layout
                    .push_event(&wgui::event::Event::MouseUp(event), &mut (), &mut ())
                    .unwrap();
                assert!(clicked.get(), "{content:?}: rendered center missed {id}");
            }
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
