use std::{
    any::Any,
    cell::RefCell,
    mem::{self, ManuallyDrop},
    rc::Rc,
    time::{Duration, Instant},
};

use glam::{Vec2, vec2};
use slotmap::Key;
use wgui::{
    components::{button::ComponentButton, checkbox::ComponentCheckbox, slider::ComponentSlider},
    event::{CallbackDataCommon, EventCallback, StyleSetRequest},
    i18n::Translation,
    parser::Fetchable,
    taffy::Display,
    widget::EventResult,
};
use wlx_common::overlays::{BackendAttrib, BackendAttribValue, MouseTransform, StereoMode};

use crate::{
    attrib_value,
    backend::{
        input::HoverResult,
        task::{OverlayTask, TaskContainer, TaskType},
    },
    gui::panel::{
        GuiPanel, NewGuiPanelParams, OnCustomAttribFunc,
        button::{BUTTON_EVENT_SUFFIX, BUTTON_EVENTS},
    },
    overlays::edit::{
        lock::InteractLockHandler,
        mouse::new_mouse_tab_handler,
        pos::{PosTabState, new_pos_tab_handler},
        sprite_tab::SpriteTabHandler,
        stereo::{get_stereo_full_frame_translation, new_stereo_tab_handler},
        tab::ButtonPaneTabSwitcher,
    },
    state::AppState,
    subsystem::hid::WheelDelta,
    windowing::{
        OverlayID, OverlaySelector,
        backend::{DummyBackend, OverlayBackend, OverlayEventData, RenderResources, ShouldRender},
        window::{OverlayWindowConfig, save_transform, scalar_scale},
    },
};

mod lock;
mod mouse;
mod pos;
mod sprite_tab;
mod stereo;
pub mod tab;

enum ResizeState {
    None,
    Start {
        overlay_id: OverlayID,
    },
    Active {
        overlay_id: OverlayID,
        pointer: usize,
        start_uv: Vec2,
        last_uv: Vec2,
        last_extent: [u32; 2],
    },
}

struct EditModeState {
    tasks: Rc<RefCell<TaskContainer>>,
    id: Rc<RefCell<OverlayID>>,
    tabs: ButtonPaneTabSwitcher,
    lock: InteractLockHandler,
    pos: SpriteTabHandler<PosTabState>,
    stereo: SpriteTabHandler<StereoMode>,
    mouse: SpriteTabHandler<MouseTransform>,
    resize: ResizeState,
}

impl EditModeState {
    fn resize_end(&mut self, app: &mut AppState) {
        if let ResizeState::Active { overlay_id, .. } =
            std::mem::replace(&mut self.resize, ResizeState::None)
        {
            app.tasks.enqueue(TaskType::Overlay(OverlayTask::Modify(
                OverlaySelector::Id(overlay_id),
                Box::new(|_app, owc| {
                    owc.resizing = false;
                }),
            )));
        }
    }
}

type EditModeWrapPanel = GuiPanel<EditModeState>;

#[derive(Default)]
pub struct EditWrapperManager {
    #[allow(dead_code)]
    edit_mode: bool,
    panel_pool: Vec<EditModeWrapPanel>,
}

impl EditWrapperManager {
    pub fn wrap_edit_mode(
        &mut self,
        id: OverlayID,
        owc: &mut OverlayWindowConfig,
        app: &mut AppState,
    ) -> anyhow::Result<()> {
        if owc.editing {
            return Ok(());
        }

        log::debug!("EditMode wrap on {}", owc.name);
        let mut panel = self.panel_pool.pop();
        if panel.is_none() {
            panel = Some(make_edit_panel(app)?);
        }
        let mut panel = panel.unwrap();
        reset_panel(&mut panel, id, owc)?;

        let inner = mem::replace(&mut owc.backend, Box::new(DummyBackend {}));

        owc.backend = Box::new(EditModeBackendWrapper {
            inner: ManuallyDrop::new(inner),
            panel: ManuallyDrop::new(panel),
            extent: [0, 0],
            last_render: Instant::now(),
            can_render_inner: false,
        });
        owc.editing = true;

        Ok(())
    }

    pub fn unwrap_edit_mode(
        &mut self,
        owc: &mut OverlayWindowConfig,
        app: &mut AppState,
    ) -> anyhow::Result<()> {
        if !owc.editing {
            return Ok(());
        }

        log::debug!("EditMode unwrap on {}", owc.name);
        let wrapper = mem::replace(&mut owc.backend, Box::new(DummyBackend {}));
        let mut wrapper: Box<dyn Any> = wrapper;
        let wrapper = wrapper
            .downcast_mut::<EditModeBackendWrapper>()
            .expect("Wrong type to unwrap");

        let panel = unsafe { ManuallyDrop::take(&mut wrapper.panel) };
        self.panel_pool.push(panel);

        let inner = unsafe { ManuallyDrop::take(&mut wrapper.inner) };
        owc.backend = inner;
        owc.editing = false;

        owc.backend.resume(app)

        // wrapper is destroyed with nothing left inside
    }
}

pub struct EditModeBackendWrapper {
    panel: ManuallyDrop<EditModeWrapPanel>,
    inner: ManuallyDrop<Box<dyn OverlayBackend>>,

    extent: [u32; 2],
    last_render: Instant,
    can_render_inner: bool,
}

impl EditModeBackendWrapper {
    /// Access the content without removing an active edit panel. Portal
    /// completion can replace the content while the pointer is editing it.
    pub(crate) fn content_backend_mut(
        backend: &mut Box<dyn OverlayBackend>,
    ) -> &mut Box<dyn OverlayBackend> {
        if (backend.as_ref() as &dyn Any).is::<Self>() {
            let wrapper = (backend.as_mut() as &mut dyn Any)
                .downcast_mut::<Self>()
                .expect("Edit wrapper type already checked");
            &mut wrapper.inner
        } else {
            backend
        }
    }
}

impl OverlayBackend for EditModeBackendWrapper {
    fn init(&mut self, app: &mut crate::state::AppState) -> anyhow::Result<()> {
        self.inner.init(app)?;
        self.panel.init(app)
    }
    fn pause(&mut self, app: &mut crate::state::AppState) -> anyhow::Result<()> {
        self.inner.pause(app)?;
        self.panel.pause(app)
    }
    fn resume(&mut self, app: &mut crate::state::AppState) -> anyhow::Result<()> {
        self.inner.resume(app)?;
        self.panel.resume(app)
    }
    fn should_render(&mut self, app: &mut crate::state::AppState) -> anyhow::Result<ShouldRender> {
        {
            let mut local_tasks = self.panel.state.tasks.borrow_mut();
            app.tasks.transfer_from(&mut local_tasks);
        }

        let inner = self.inner.should_render(app)?;

        if !matches!(inner, ShouldRender::Unable)
            && let Some(ref frame_meta) = self.inner.frame_meta()
        {
            let (width_px, height_px) = (frame_meta.extent[0], frame_meta.extent[1]);

            let new_size = vec2(width_px as _, height_px as _);
            if !self.panel.max_size.abs_diff_eq(new_size, 0.1) {
                log::debug!("EditWrapperGui size {} → {new_size}", self.panel.max_size);
                self.panel.max_size = new_size;

                let gui_scale = (new_size.x / 750.0).min(new_size.y / 300.0);
                self.panel.gui_scale = (gui_scale * 4.0).round() / 4.0;
                self.panel.update_layout(app)?;
            }

            self.can_render_inner = true;
        } else {
            self.can_render_inner = false;
            if self.last_render.elapsed() < Duration::from_millis(100) {
                return Ok(ShouldRender::Unable);
            }
        }

        let panel = self.panel.should_render(app)?;

        #[allow(clippy::match_same_arms)]
        Ok(match (inner, panel) {
            (ShouldRender::Should, ShouldRender::Should) => ShouldRender::Should,
            (ShouldRender::Should, ShouldRender::Can) => ShouldRender::Should,
            (ShouldRender::Can, ShouldRender::Should) => ShouldRender::Should,
            (ShouldRender::Can, ShouldRender::Can) => ShouldRender::Can,
            // only if last render was more than 100ms ago.
            (ShouldRender::Unable, ShouldRender::Should) => ShouldRender::Should,
            _ => ShouldRender::Unable,
        })
    }
    fn render(
        &mut self,
        app: &mut crate::state::AppState,
        rdr: &mut RenderResources,
    ) -> anyhow::Result<()> {
        if self.can_render_inner {
            self.inner.render(app, rdr)?;
        }

        // during resize, just fade the color
        if matches!(self.panel.state.resize, ResizeState::None) {
            self.panel.render(app, rdr)?;
            // `GuiPanel` is not stereo-aware, so just render the same pass twice
            if rdr.cmd_bufs.len() > 1 {
                rdr.cmd_bufs.reverse();
                self.panel.render(app, rdr)?;
                rdr.cmd_bufs.reverse();
            }
        }

        self.extent = rdr.extent;

        Ok(())
    }
    fn frame_meta(&mut self) -> Option<crate::windowing::backend::FrameMeta> {
        self.inner.frame_meta()
    }
    fn on_hover(
        &mut self,
        app: &mut crate::state::AppState,
        hit: &crate::backend::input::PointerHit,
    ) -> HoverResult {
        // pass through hover events to force pipewire to capture frames for us
        let _ = self.inner.on_hover(app, hit);
        let res = self.panel.on_hover(app, hit);

        match &mut self.panel.state.resize {
            ResizeState::Active {
                overlay_id,
                pointer,
                start_uv,
                last_uv,
                last_extent,
            } if *pointer == hit.pointer => {
                fn lerp_u32(a: [u32; 2], b: [u32; 2], t: f32) -> [u32; 2] {
                    [
                        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u32,
                        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u32,
                    ]
                }

                // freshest extent we can get is from last frame, so use uv from that frame
                let want_extent = resize_centered_from_uv(*start_uv, *last_uv, self.extent);

                app.tasks
                    .enqueue(TaskType::Overlay(OverlayTask::ResizeOverlay(
                        OverlaySelector::Id(*overlay_id),
                        lerp_u32(*last_extent, want_extent, 0.33),
                    )));

                *last_uv = hit.uv;
                *last_extent = self.extent;
            }
            _ => {}
        }

        res
    }
    fn on_left(&mut self, app: &mut crate::state::AppState, pointer: usize) {
        self.panel.state.resize_end(app);

        self.inner.on_left(app, pointer);
        self.panel.on_left(app, pointer);
    }
    fn on_pointer(
        &mut self,
        app: &mut crate::state::AppState,
        hit: &crate::backend::input::PointerHit,
        pressed: bool,
    ) {
        self.panel.on_pointer(app, hit, pressed);

        if let ResizeState::Start { overlay_id } = &mut self.panel.state.resize {
            app.tasks.enqueue(TaskType::Overlay(OverlayTask::Modify(
                OverlaySelector::Id(*overlay_id),
                Box::new(|_app, owc| {
                    owc.resizing = true;
                }),
            )));

            let last_uv = hit.uv;
            self.panel.state.resize = ResizeState::Active {
                overlay_id: *overlay_id,
                pointer: hit.pointer,
                start_uv: last_uv,
                last_uv,
                last_extent: self.extent,
            };
        }
    }
    fn on_scroll(
        &mut self,
        app: &mut crate::state::AppState,
        hit: &crate::backend::input::PointerHit,
        delta: WheelDelta,
    ) {
        self.panel.on_scroll(app, hit, delta);
    }
    fn notify(&mut self, app: &mut AppState, event_data: OverlayEventData) -> anyhow::Result<()> {
        self.panel.notify(app, event_data.clone())?;
        self.inner.notify(app, event_data)
    }
    fn get_interaction_transform(&mut self) -> Option<glam::Affine2> {
        self.inner.get_interaction_transform()
    }
    fn interaction_pose(&self) -> glam::Affine3A {
        self.inner.interaction_pose()
    }
    fn screen_source(&self) -> Option<std::sync::Arc<str>> {
        self.inner.screen_source()
    }
    fn screen_region(
        &self,
        region: wlx_common::screen_region::ScreenRegion,
    ) -> Option<Box<dyn OverlayBackend>> {
        self.inner.screen_region(region)
    }
    fn get_attrib(&self, attrib: BackendAttrib) -> Option<BackendAttribValue> {
        self.inner.get_attrib(attrib)
    }
    fn set_attrib(&mut self, app: &mut AppState, value: BackendAttribValue) -> bool {
        self.inner.set_attrib(app, value)
    }
}

#[allow(clippy::too_many_lines)]
fn make_edit_panel(app: &mut AppState) -> anyhow::Result<EditModeWrapPanel> {
    let state = EditModeState {
        id: Rc::new(RefCell::new(OverlayID::null())),
        tasks: Rc::new(RefCell::new(TaskContainer::new())),
        tabs: ButtonPaneTabSwitcher::default(),
        lock: InteractLockHandler::default(),
        pos: SpriteTabHandler::default(),
        stereo: SpriteTabHandler::default(),
        mouse: SpriteTabHandler::default(),
        resize: ResizeState::None,
    };

    let on_custom_attrib: OnCustomAttribFunc = Box::new(move |layout, parser, attribs, _app| {
        let Ok(button) = parser
            .fetch_component_from_widget_id_as::<ComponentButton>(&layout.state, attribs.widget_id)
        else {
            return;
        };

        for (name, kind, test_button, test_duration) in &BUTTON_EVENTS {
            for suffix in BUTTON_EVENT_SUFFIX {
                let name = &format!("{name}{suffix}");
                let Some(action) = attribs.get_value(name) else {
                    break;
                };

                let mut args = action.split_whitespace();
                let Some(command) = args.next() else {
                    continue;
                };

                let button = button.clone();

                let callback: EventCallback<AppState, EditModeState> = match command {
                    "::EditModeAdjust" => {
                        let action = args.next().unwrap().to_owned();
                        Box::new(move |_common, data, app, state| {
                            if !test_button(data) || !test_duration(&button, app) {
                                return Ok(EventResult::Pass);
                            }
                            let action = action.clone();
                            app.tasks.enqueue(TaskType::Overlay(OverlayTask::Modify(
                                OverlaySelector::Id(*state.id.borrow()),
                                Box::new(move |app, owc| {
                                    apply_quick_action(app, owc, &action);
                                }),
                            )));
                            Ok(EventResult::Consumed)
                        })
                    }
                    "::EditModeScreenLayout" => {
                        let layout = match args.next() {
                            Some("dual") => wlx_common::config::ScreenLayout::Dual,
                            Some("thirds") => wlx_common::config::ScreenLayout::Thirds,
                            Some("center") => wlx_common::config::ScreenLayout::Center,
                            _ => wlx_common::config::ScreenLayout::Full,
                        };
                        Box::new(move |_common, data, app, state| {
                            if !test_button(data) || !test_duration(&button, app) {
                                return Ok(EventResult::Pass);
                            }
                            app.tasks
                                .enqueue(TaskType::Overlay(OverlayTask::ScreenLayout(
                                    OverlaySelector::Id(*state.id.borrow()),
                                    layout,
                                    false,
                                )));
                            Ok(EventResult::Consumed)
                        })
                    }
                    "::EditModeToggleLock" => Box::new(move |common, data, app, state| {
                        if !test_button(data) || !test_duration(&button, app) {
                            return Ok(EventResult::Pass);
                        }

                        let sel = OverlaySelector::Id(*state.id.borrow());
                        let task = state.lock.toggle(common);
                        app.tasks
                            .enqueue(TaskType::Overlay(OverlayTask::Modify(sel, task)));
                        Ok(EventResult::Consumed)
                    }),
                    "::EditModeToggleGrab" => Box::new(move |_common, data, app, state| {
                        if !test_button(data) || !test_duration(&button, app) {
                            return Ok(EventResult::Pass);
                        }

                        let sel = OverlaySelector::Id(*state.id.borrow());
                        app.tasks.enqueue(TaskType::Overlay(OverlayTask::Modify(
                            sel,
                            Box::new(|_app, owc| {
                                let state = owc.active_state.as_mut().unwrap(); //want panic
                                state.grabbable = !state.grabbable;
                            }),
                        )));
                        Ok(EventResult::Consumed)
                    }),
                    "::EditModeTab" => {
                        let tab_name = args.next().unwrap().to_owned();
                        Box::new(move |common, data, app, state| {
                            if !test_button(data) || !test_duration(&button, app) {
                                return Ok(EventResult::Pass);
                            }

                            state.tabs.tab_button_clicked(common, &tab_name);
                            Ok(EventResult::Consumed)
                        })
                    }
                    "::EditModeSetPos" => {
                        let key = args.next().unwrap().to_owned();
                        Box::new(move |common, data, app, state| {
                            if !test_button(data) || !test_duration(&button, app) {
                                return Ok(EventResult::Pass);
                            }

                            let sel = OverlaySelector::Id(*state.id.borrow());
                            let task = state.pos.button_clicked(common, &key);
                            app.tasks
                                .enqueue(TaskType::Overlay(OverlayTask::Modify(sel, task)));
                            Ok(EventResult::Consumed)
                        })
                    }
                    "::EditModeSetStereo" => {
                        let key = args.next().unwrap().to_owned();
                        Box::new(move |common, data, app, state| {
                            if !test_button(data) || !test_duration(&button, app) {
                                return Ok(EventResult::Pass);
                            }

                            let sel = OverlaySelector::Id(*state.id.borrow());
                            let task = state.stereo.button_clicked(common, &key);
                            app.tasks
                                .enqueue(TaskType::Overlay(OverlayTask::Modify(sel, task)));
                            Ok(EventResult::Consumed)
                        })
                    }
                    "::EditModeSetStereoFullFrame" => {
                        let full_frame = args.next().unwrap().parse::<bool>().unwrap();
                        Box::new(move |_common, data, app, state| {
                            if !test_button(data) || !test_duration(&button, app) {
                                return Ok(EventResult::Pass);
                            }

                            let sel = OverlaySelector::Id(*state.id.borrow());
                            app.tasks.enqueue(TaskType::Overlay(OverlayTask::Modify(
                                sel,
                                Box::new(move |app, owc| {
                                    let attrib = BackendAttribValue::StereoFullFrame(full_frame);
                                    owc.backend.set_attrib(app, attrib);
                                }),
                            )));
                            Ok(EventResult::Consumed)
                        })
                    }
                    "::EditModeSetMouse" => {
                        let key = args.next().unwrap().to_owned();
                        Box::new(move |common, data, app, state| {
                            if !test_button(data) || !test_duration(&button, app) {
                                return Ok(EventResult::Pass);
                            }

                            let sel = OverlaySelector::Id(*state.id.borrow());
                            let task = state.mouse.button_clicked(common, &key);
                            app.tasks
                                .enqueue(TaskType::Overlay(OverlayTask::Modify(sel, task)));
                            Ok(EventResult::Consumed)
                        })
                    }
                    "::EditModeDelete" => Box::new(move |_common, data, app, state| {
                        if !test_button(data) || !test_duration(&button, app) {
                            return Ok(EventResult::Pass);
                        }

                        app.tasks.enqueue(TaskType::Overlay(OverlayTask::Modify(
                            OverlaySelector::Id(*state.id.borrow()),
                            Box::new(move |_app, owc| {
                                owc.active_state = None;
                            }),
                        )));
                        Ok(EventResult::Consumed)
                    }),
                    "::EditModeResizeStart" => Box::new(move |_common, data, app, state| {
                        if !test_button(data) || !test_duration(&button, app) {
                            return Ok(EventResult::Pass);
                        }
                        state.resize = ResizeState::Start {
                            overlay_id: *state.id.borrow(),
                        };
                        Ok(EventResult::Consumed)
                    }),
                    "::EditModeResizeStop" => Box::new(move |_common, data, app, state| {
                        if !test_button(data) || !test_duration(&button, app) {
                            return Ok(EventResult::Pass);
                        }
                        state.resize_end(app);
                        Ok(EventResult::Consumed)
                    }),
                    _ => return,
                };

                let id = layout.add_event_listener(attribs.widget_id, *kind, callback);
                log::debug!("Registered {action} on {:?} as {id:?}", attribs.widget_id);
            }
        }
    });

    let mut panel = GuiPanel::new_from_template(
        app,
        "gui/edit.xml",
        state,
        NewGuiPanelParams {
            on_custom_attrib: Some(on_custom_attrib),
            resize_to_parent: true,
            ..Default::default()
        },
    )?;

    panel.state.pos = new_pos_tab_handler(&mut panel)?;
    panel.state.stereo = new_stereo_tab_handler(&mut panel)?;
    panel.state.mouse = new_mouse_tab_handler(&mut panel)?;
    panel.state.lock = InteractLockHandler::new(&mut panel)?;
    panel.state.tabs = ButtonPaneTabSwitcher::new(
        &mut panel,
        &["none", "quick", "pos", "alpha", "stereo", "mouse", "split"],
    )?;

    set_up_checkbox(&mut panel, "additive_box", cb_assign_additive)?;
    set_up_checkbox(&mut panel, "align_box", cb_assign_align)?;
    set_up_checkbox(&mut panel, "global_box", cb_assign_global)?;
    set_up_checkbox(&mut panel, "angle_fade_box", cb_assign_angle_fade)?;
    set_up_checkbox(&mut panel, "block_input_box", cb_assign_block_input)?;
    set_up_checkbox(
        &mut panel,
        "stereo_full_frame_box",
        cb_assign_stereo_full_frame,
    )?;
    set_up_checkbox(
        &mut panel,
        "stereo_adjust_mouse_box",
        cb_assign_stereo_adjust_mouse,
    )?;
    set_up_slider(&mut panel, "lerp_slider", cb_assign_lerp)?;
    set_up_slider(&mut panel, "alpha_slider", cb_assign_alpha)?;
    set_up_slider(&mut panel, "curve_slider", cb_assign_curve)?;

    Ok(panel)
}

fn reset_panel(
    panel: &mut EditModeWrapPanel,
    id: OverlayID,
    owc: &mut OverlayWindowConfig,
) -> anyhow::Result<()> {
    *panel.state.id.borrow_mut() = id;
    let state = owc.active_state.as_mut().unwrap();

    let mut com = panel.layout.common();

    let c = panel
        .parser_state
        .fetch_component_as::<ComponentButton>("top_grab")?;
    c.set_sticky_state(&mut com, !state.grabbable);

    let c = panel
        .parser_state
        .fetch_component_as::<ComponentSlider>("lerp_slider")?;
    c.set_value_primary(&mut com, state.positioning.get_lerp().unwrap_or(1.0));

    let c = panel
        .parser_state
        .fetch_component_as::<ComponentSlider>("alpha_slider")?;
    c.set_value_primary(&mut com, state.alpha);

    let c = panel
        .parser_state
        .fetch_component_as::<ComponentSlider>("curve_slider")?;
    c.set_value_primary(&mut com, state.curvature.unwrap_or(0.0));

    let c = panel
        .parser_state
        .fetch_component_as::<ComponentCheckbox>("additive_box")?;
    c.set_checked(&mut com, state.additive);

    let c = panel
        .parser_state
        .fetch_component_as::<ComponentCheckbox>("align_box")?;
    c.set_checked(&mut com, state.align_to_hmd);

    let c = panel
        .parser_state
        .fetch_component_as::<ComponentCheckbox>("global_box")?;
    c.set_checked(&mut com, owc.global);

    let c = panel
        .parser_state
        .fetch_component_as::<ComponentCheckbox>("angle_fade_box")?;
    c.set_checked(&mut com, state.angle_fade);

    let c = panel
        .parser_state
        .fetch_component_as::<ComponentCheckbox>("block_input_box")?;
    c.set_checked(&mut com, state.block_input);

    panel.state.pos.reset(&mut com, &state.positioning.into());
    panel.state.lock.reset(&mut com, state.interactable);
    panel.state.tabs.reset(&mut com);
    panel.state.tabs.tab_button_clicked(&mut com, "quick");
    panel
        .state
        .tabs
        .set_tab_visible(&mut com, "split", owc.backend.screen_source().is_some());

    if let Some(stereo) = attrib_value!(
        owc.backend.get_attrib(BackendAttrib::Stereo),
        BackendAttribValue::Stereo
    ) {
        panel.state.tabs.set_tab_visible(&mut com, "stereo", true);
        panel.state.stereo.reset(&mut com, &stereo);

        // Set the checkbox label based on stereo mode
        let translation = get_stereo_full_frame_translation(stereo);
        let c = panel
            .parser_state
            .fetch_component_as::<ComponentCheckbox>("stereo_full_frame_box")?;
        c.set_text(&mut com, Translation::from_translation_key(translation));
    } else {
        panel.state.tabs.set_tab_visible(&mut com, "stereo", false);
    }

    if let Some(mouse) = attrib_value!(
        owc.backend.get_attrib(BackendAttrib::MouseTransform),
        BackendAttribValue::MouseTransform
    ) {
        panel.state.tabs.set_tab_visible(&mut com, "mouse", true);
        panel.state.mouse.reset(&mut com, &mouse);
    } else {
        panel.state.tabs.set_tab_visible(&mut com, "mouse", false);
    }

    if let Some(full_frame) = attrib_value!(
        owc.backend.get_attrib(BackendAttrib::StereoFullFrame),
        BackendAttribValue::StereoFullFrame
    ) {
        let c = panel
            .parser_state
            .fetch_component_as::<ComponentCheckbox>("stereo_full_frame_box")?;
        c.set_checked(&mut com, full_frame);
    }

    if let Some(adjust_mouse) = attrib_value!(
        owc.backend.get_attrib(BackendAttrib::StereoAdjustMouse),
        BackendAttribValue::StereoAdjustMouse
    ) {
        let c = panel
            .parser_state
            .fetch_component_as::<ComponentCheckbox>("stereo_adjust_mouse_box")?;
        c.set_checked(&mut com, adjust_mouse);
    }

    let resizable = attrib_value!(
        owc.backend.get_attrib(BackendAttrib::Resizable),
        BackendAttribValue::Resizable
    )
    .unwrap_or(false);
    let c = panel
        .parser_state
        .fetch_component_as::<ComponentButton>("resize_corner")?;
    let common = CallbackDataCommon {
        state: &panel.layout.state,
        alterables: &mut panel.layout.alterables,
    };
    common.alterables.set_style(
        c.get_rect(),
        StyleSetRequest::Display(if resizable {
            Display::Block
        } else {
            Display::None
        }),
    );

    Ok(())
}

const fn cb_assign_lerp(_app: &mut AppState, owc: &mut OverlayWindowConfig, lerp: f32) {
    owc.dirty = true;
    let active_state = owc.active_state.as_mut().unwrap();
    active_state.positioning = active_state.positioning.with_lerp(lerp);
}

const fn cb_assign_alpha(_app: &mut AppState, owc: &mut OverlayWindowConfig, alpha: f32) {
    owc.dirty = true;
    owc.active_state.as_mut().unwrap().alpha = alpha;
}

fn cb_assign_curve(_app: &mut AppState, owc: &mut OverlayWindowConfig, curvature: f32) {
    owc.dirty = true;
    owc.active_state.as_mut().unwrap().curvature = if curvature < 0.005 {
        None
    } else {
        Some(curvature)
    };
}

const fn cb_assign_additive(_app: &mut AppState, owc: &mut OverlayWindowConfig, additive: bool) {
    owc.dirty = true;
    owc.active_state.as_mut().unwrap().additive = additive;
}

const fn cb_assign_align(_app: &mut AppState, owc: &mut OverlayWindowConfig, align: bool) {
    owc.dirty = true;
    let active_state = owc.active_state.as_mut().unwrap();
    active_state.align_to_hmd = align;
}

const fn cb_assign_global(_app: &mut AppState, owc: &mut OverlayWindowConfig, global: bool) {
    owc.global = global;
}

const fn cb_assign_angle_fade(
    _app: &mut AppState,
    owc: &mut OverlayWindowConfig,
    angle_fade: bool,
) {
    owc.active_state.as_mut().unwrap().angle_fade = angle_fade;
}

const fn cb_assign_block_input(
    _app: &mut AppState,
    owc: &mut OverlayWindowConfig,
    block_input: bool,
) {
    owc.active_state.as_mut().unwrap().block_input = block_input;
}

fn cb_assign_stereo_full_frame(
    app: &mut AppState,
    owc: &mut OverlayWindowConfig,
    full_frame: bool,
) {
    owc.dirty = true;
    let attrib = BackendAttribValue::StereoFullFrame(full_frame);
    owc.backend.set_attrib(app, attrib);
}

fn cb_assign_stereo_adjust_mouse(
    app: &mut AppState,
    owc: &mut OverlayWindowConfig,
    adjust_mouse: bool,
) {
    owc.dirty = true;
    let attrib = BackendAttribValue::StereoAdjustMouse(adjust_mouse);
    owc.backend.set_attrib(app, attrib);
}

fn set_up_slider(
    panel: &mut EditModeWrapPanel,
    id: &str,
    callback: fn(&mut AppState, &mut OverlayWindowConfig, f32),
) -> anyhow::Result<()> {
    let slider = panel
        .parser_state
        .fetch_component_as::<ComponentSlider>(id)?;
    let tasks = panel.state.tasks.clone();
    let overlay_id = panel.state.id.clone();
    slider.on_value_changed(Box::new(move |_common, e| {
        let mut tasks = tasks.borrow_mut();
        let e_value = e.value;

        tasks.enqueue(TaskType::Overlay(OverlayTask::Modify(
            OverlaySelector::Id(*overlay_id.borrow()),
            Box::new(move |app, owc| callback(app, owc, e_value)),
        )));
    }));

    Ok(())
}

fn set_up_checkbox(
    panel: &mut EditModeWrapPanel,
    id: &str,
    callback: fn(&mut AppState, &mut OverlayWindowConfig, bool),
) -> anyhow::Result<()> {
    let checkbox = panel
        .parser_state
        .fetch_component_as::<ComponentCheckbox>(id)?;
    let tasks = panel.state.tasks.clone();
    let overlay_id = panel.state.id.clone();
    checkbox.on_toggle(Box::new(move |_common, e| {
        let mut tasks = tasks.borrow_mut();
        let e_checked = e.checked;

        tasks.enqueue(TaskType::Overlay(OverlayTask::Modify(
            OverlaySelector::Id(*overlay_id.borrow()),
            Box::new(move |app, owc| callback(app, owc, e_checked)),
        )));
        Ok(())
    }));

    Ok(())
}

pub fn resize_centered_from_uv(grab_uv: Vec2, cur_uv: Vec2, cur_extent: [u32; 2]) -> [u32; 2] {
    const MIN_SIZE: Vec2 = Vec2::new(160.0, 160.0);
    const CENTER_DEADZONE: f32 = 0.1;

    let cur_extent_v = Vec2::new(cur_extent[0] as f32, cur_extent[1] as f32);
    let grab_from_center = grab_uv - Vec2::splat(0.5);
    let cur_from_center = (cur_uv - Vec2::splat(0.5)) * cur_extent_v;

    let mut out = cur_extent_v;
    if grab_from_center.x.abs() > CENTER_DEADZONE {
        out.x = (cur_from_center.x / grab_from_center.x).abs();
    }

    if grab_from_center.y.abs() > CENTER_DEADZONE {
        out.y = (cur_from_center.y / grab_from_center.y).abs();
    }

    out = out.max(MIN_SIZE);

    [out.x.round() as u32, out.y.round() as u32]
}

/// Shared by edit mode and the interactive panel's compact controls.
pub fn apply_quick_action(app: &mut AppState, owc: &mut OverlayWindowConfig, action: &str) {
    let Some(state) = owc.active_state.as_mut() else {
        return;
    };
    match action {
        "opacity_down" | "opacity_up" => {
            let alpha =
                (state.alpha + if action == "opacity_up" { 0.1 } else { -0.1 }).clamp(0.1, 1.);
            cb_assign_alpha(app, owc, alpha);
        }
        "curve_down" | "curve_up" => {
            let curve = (state.curvature.unwrap_or(0.)
                + if action == "curve_up" { 0.05 } else { -0.05 })
            .clamp(0., 0.5);
            cb_assign_curve(app, owc, curve);
        }
        "pin" => {
            state.grabbable = !state.grabbable;
            owc.dirty = true;
        }
        _ => {
            adjust_panel(&mut state.transform, &app.input_state.hmd, action);
            save_transform(state, app);
            owc.dirty = true;
        }
    }
}

/// Adjust only this panel, retaining its positioning mode and capture region.
fn adjust_panel(transform: &mut glam::Affine3A, hmd: &glam::Affine3A, action: &str) {
    let scale = scalar_scale(transform);
    if !scale.is_finite() || scale <= f32::EPSILON {
        return;
    }
    match action {
        "width_down" | "width_up" => {
            let factor = if action == "width_up" { 1.05 } else { 0.95 };
            transform.matrix3 = transform
                .matrix3
                .mul_scalar((scale * factor).clamp(0.1, 20.0) / scale);
        }
        "nearer" | "farther" | "recenter" => {
            let offset = transform.translation - hmd.translation;
            let distance = offset.length();
            if !distance.is_finite() {
                return;
            }
            let forward = -hmd.z_axis.normalize();
            let direction = if action == "recenter" || distance < 0.001 {
                forward
            } else {
                offset / distance
            };
            let delta = match action {
                "nearer" => -0.1,
                "farther" => 0.1,
                _ => 0.0,
            };
            transform.translation =
                hmd.translation + direction * (distance + delta).clamp(0.25, 20.0);
            if action == "recenter" {
                transform.matrix3 = hmd.matrix3.mul_scalar(scale);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod quick_control_tests {
    use super::adjust_panel;
    use glam::{Affine3A, Quat, Vec3, Vec3A};

    #[test]
    fn width_changes_preserve_position_and_clamp_scale() {
        let mut panel = Affine3A::from_scale_rotation_translation(
            Vec3::splat(19.9),
            Quat::from_rotation_y(0.4),
            Vec3::new(1.0, 2.0, -3.0),
        );
        let original = panel;
        adjust_panel(&mut panel, &Affine3A::IDENTITY, "width_up");
        assert!((panel.x_axis.length() - 20.0).abs() < 0.001);
        assert_eq!(panel.translation, original.translation);
        assert!(
            panel
                .x_axis
                .normalize()
                .abs_diff_eq(original.x_axis.normalize(), 0.001)
        );
        for _ in 0..200 {
            adjust_panel(&mut panel, &Affine3A::IDENTITY, "width_down");
        }
        assert!((panel.x_axis.length() - 0.1).abs() < 0.001);
    }

    #[test]
    fn distance_preserves_direction_and_handles_coincident_panel() {
        let hmd = Affine3A::from_translation(Vec3::new(2.0, 1.6, 1.0));
        let mut panel = hmd;
        adjust_panel(&mut panel, &hmd, "nearer");
        assert!(
            panel
                .translation
                .abs_diff_eq(hmd.translation - Vec3A::Z * 0.25, 0.001)
        );
        let orientation = panel.matrix3;
        adjust_panel(&mut panel, &hmd, "farther");
        assert!((panel.translation.distance(hmd.translation) - 0.35).abs() < 0.001);
        assert_eq!(panel.matrix3, orientation);
    }

    #[test]
    fn recenter_preserves_size_and_distance_with_rotated_head() {
        let hmd = Affine3A::from_rotation_translation(
            Quat::from_rotation_y(0.7),
            Vec3::new(3.0, 1.6, -1.0),
        );
        let mut panel = Affine3A::from_scale_rotation_translation(
            Vec3::splat(2.0),
            Quat::IDENTITY,
            Vec3::new(5.0, 1.6, -1.0),
        );
        adjust_panel(&mut panel, &hmd, "recenter");
        assert!(
            panel
                .translation
                .abs_diff_eq(hmd.translation - hmd.z_axis * 2.0, 0.001)
        );
        assert!(
            panel
                .matrix3
                .abs_diff_eq(hmd.matrix3.mul_scalar(2.0), 0.001)
        );
    }
}
