//! Presentation-only controls around regular screens. Capture and desktop mapping stay native.
use super::geometry::PanelGeometry;
use crate::{
    backend::{
        input::{HoverResult, PointerHit},
        task::{OverlayTask, TaskType, ToggleMode},
    },
    gui::panel::{GuiPanel, NewGuiPanelParams},
    state::AppState,
    subsystem::hid::WheelDelta,
    windowing::{
        OverlayID, OverlaySelector,
        backend::{
            FrameMeta, OverlayBackend, OverlayEventData, OverlayLifetimeUpdate, RenderResources,
            ShouldRender,
        },
    },
};
use glam::{Affine2, Affine3A, Vec3, vec2};
use std::{cell::RefCell, rc::Rc, sync::Arc};
use wgui::{
    components::button::ComponentButton, event::StyleSetRequest, parser::Fetchable,
    widget::label::WidgetLabel,
};
use wlx_common::{
    overlays::{BackendAttrib, BackendAttribValue, StereoMode},
    screen_region::ScreenRegion,
};

fn width_factor(content: [u32; 2]) -> f32 {
    (content[0] as f32 / content[1].max(1) as f32).min(1.)
}
fn bind_buttons(parser: &wgui::parser::ParserState, pending: &Rc<RefCell<Option<String>>>) {
    for id in parser.data.components_by_id.keys() {
        let Some(action) = id.strip_prefix("quick_") else {
            continue;
        };
        let Ok(button) = parser.fetch_component_as::<ComponentButton>(id) else {
            continue;
        };
        let action = action.to_owned();
        let pending = pending.clone();
        button.on_click(Rc::new(move |_, _| {
            *pending.borrow_mut() = Some(action.clone());
            Ok(())
        }));
    }
}
pub(super) struct ScreenControls {
    inner: Box<dyn OverlayBackend>,
    gui: Option<GuiPanel<()>>,
    pending: Rc<RefCell<Option<String>>>,
    target: Option<OverlayID>,
    geometry: PanelGeometry,
    expanded: bool,
    enabled: bool,
    held: Vec<PointerHit>,
}
impl ScreenControls {
    pub fn new(inner: Box<dyn OverlayBackend>) -> Self {
        Self {
            inner,
            gui: None,
            pending: Default::default(),
            target: None,
            geometry: PanelGeometry::new(None, false, false),
            expanded: false,
            enabled: false,
            held: vec![],
        }
    }
    fn take_releases(&mut self, pointer: Option<usize>) -> Vec<PointerHit> {
        let mut released = Vec::new();
        let mut i = 0;
        while i < self.held.len() {
            if pointer.is_none_or(|p| self.held[i].pointer == p) {
                released.push(self.held.remove(i));
            } else {
                i += 1;
            }
        }
        released
    }
    fn release(&mut self, app: &mut AppState, pointer: Option<usize>) {
        for hit in self.take_releases(pointer) {
            self.inner.on_pointer(app, &hit, false);
        }
    }
    fn content_hit(&self, hit: &PointerHit) -> Option<PointerHit> {
        self.geometry
            .content_uv(hit.uv)
            .map(|uv| PointerHit { uv, ..*hit })
    }
}
impl OverlayBackend for ScreenControls {
    fn screen_source(&self) -> Option<Arc<str>> {
        self.inner.screen_source()
    }
    fn screen_content_extent(&mut self) -> Option<[u32; 2]> {
        self.inner.screen_content_extent()
    }
    fn screen_region(&self, region: ScreenRegion) -> Option<Box<dyn OverlayBackend>> {
        self.inner
            .screen_region(region)
            .map(|inner| Box::new(Self::new(inner)) as Box<dyn OverlayBackend>)
    }
    fn init(&mut self, app: &mut AppState) -> anyhow::Result<()> {
        self.inner.init(app)?;
        let mut gui = GuiPanel::new_from_template(
            app,
            "gui/screen-controls.xml",
            (),
            NewGuiPanelParams {
                resize_to_parent: true,
                ..Default::default()
            },
        )?;
        bind_buttons(&gui.parser_state, &self.pending);
        let name = self
            .inner
            .screen_source()
            .unwrap_or_else(|| "Screen".into());
        let caption = if name.chars().count() > 40 {
            format!("{}…", name.chars().take(39).collect::<String>())
        } else {
            name.to_string()
        };
        let id = gui.parser_state.get_widget_id("screen_name")?;
        let mut common = gui.layout.common();
        if let Some(mut label) = common.state.widgets.get_as::<WidgetLabel>(id) {
            label.set_text(
                &mut common,
                wgui::i18n::Translation::from_raw_text(&caption),
            );
        }
        gui.init(app)?;
        self.gui = Some(gui);
        Ok(())
    }
    fn pause(&mut self, app: &mut AppState) -> anyhow::Result<()> {
        self.release(app, None);
        self.inner.pause(app)?;
        if let Some(gui) = &mut self.gui {
            gui.pause(app)?;
        }
        Ok(())
    }
    fn resume(&mut self, app: &mut AppState) -> anyhow::Result<()> {
        self.inner.resume(app)?;
        if let Some(gui) = &mut self.gui {
            gui.resume(app)?;
        }
        Ok(())
    }
    fn on_lifetime_update(&mut self, update: OverlayLifetimeUpdate) {
        self.inner.on_lifetime_update(update);
        if let Some(gui) = &mut self.gui {
            gui.on_lifetime_update(update);
        }
    }
    fn should_render(&mut self, app: &mut AppState) -> anyhow::Result<ShouldRender> {
        let command = self.pending.borrow_mut().take();
        if let Some(action) = command {
            self.release(app, None);
            if action == "toggle" {
                self.expanded = !self.expanded;
            } else if let Some(id) = self.target {
                let target = OverlaySelector::Id(id);
                if action == "hide" {
                    app.tasks
                        .enqueue(TaskType::Overlay(OverlayTask::ToggleOverlay(
                            target,
                            ToggleMode::EnsureOff,
                        )));
                } else {
                    app.tasks.enqueue(TaskType::Overlay(OverlayTask::Modify(
                        target,
                        Box::new(move |app, panel| {
                            crate::overlays::edit::apply_quick_action(app, panel, &action);
                        }),
                    )));
                }
            }
        }
        let inner = self.inner.should_render(app)?;
        if matches!(inner, ShouldRender::Unable) {
            return Ok(inner);
        }
        let Some(meta) = self.inner.frame_meta() else {
            return Ok(ShouldRender::Unable);
        };
        let enabled = meta.stereo == StereoMode::None && meta.extent.iter().all(|n| *n > 0);
        if self.enabled != enabled {
            self.release(app, None);
        }
        self.enabled = enabled;
        // Preserve stereo presentation and input without inserting a mono footer into either eye.
        if !enabled {
            return Ok(inner);
        }
        let geometry = PanelGeometry::new(Some(meta.extent), false, self.expanded);
        let changed = geometry != self.geometry;
        self.geometry = geometry;
        let Some(gui) = self.gui.as_mut() else {
            return Ok(inner);
        };
        if changed {
            let scale = geometry.full[0] as f32 / 1280.;
            gui.gui_scale = scale;
            gui.max_size = vec2(geometry.full[0] as f32, geometry.full[1] as f32);
            let space = gui.parser_state.get_widget_id("content_space")?;
            let controls = gui.parser_state.get_widget_id("quick_controls")?;
            gui.layout.alterables.set_style(
                space,
                StyleSetRequest::Height(wgui::taffy::prelude::length(
                    geometry.content[1] as f32 / scale,
                )),
            );
            gui.layout
                .alterables
                .set_widget_visible(controls, self.expanded);
            gui.update_layout(app)?;
        }
        let ui = gui.should_render(app)?;
        Ok(
            if changed
                || matches!(inner, ShouldRender::Should)
                || matches!(ui, ShouldRender::Should)
            {
                ShouldRender::Should
            } else {
                ShouldRender::Can
            },
        )
    }
    fn render(&mut self, app: &mut AppState, rdr: &mut RenderResources) -> anyhow::Result<()> {
        self.inner.render(app, rdr)?;
        if self.enabled
            && let Some(gui) = &mut self.gui
        {
            gui.render(app, rdr)?;
        }
        Ok(())
    }
    fn frame_meta(&mut self) -> Option<FrameMeta> {
        let mut meta = self.inner.frame_meta()?;
        if self.enabled {
            meta.extent = self.geometry.full;
            meta.transform = Affine3A::from_scale(Vec3::splat(width_factor(self.geometry.content)))
                * self.geometry.view_transform();
            meta.clear = wgui::gfx::cmd::WGfxClearMode::Clear([0., 0., 0., 0.]);
        }
        Some(meta)
    }
    fn notify(&mut self, app: &mut AppState, event: OverlayEventData) -> anyhow::Result<()> {
        if let OverlayEventData::IdAssigned(id) = &event {
            self.target = Some(*id);
        }
        self.inner.notify(app, event.clone())?;
        if let Some(gui) = &mut self.gui {
            gui.notify(app, event)?;
        }
        Ok(())
    }
    fn on_hover(&mut self, app: &mut AppState, hit: &PointerHit) -> HoverResult {
        if !self.enabled {
            return self.inner.on_hover(app, hit);
        }
        if let Some(content) = self.content_hit(hit) {
            if let Some(gui) = &mut self.gui {
                gui.on_left(app, hit.pointer);
            }
            self.inner.on_hover(app, &content)
        } else {
            self.release(app, Some(hit.pointer));
            self.inner.on_left(app, hit.pointer);
            self.gui
                .as_mut()
                .map_or_else(HoverResult::default, |gui| gui.on_hover(app, hit))
        }
    }
    fn on_left(&mut self, app: &mut AppState, pointer: usize) {
        // The input router sends release to the originally clicked overlay.
        // Keep desktop drags alive while moving between monitor/split panels.
        self.inner.on_left(app, pointer);
        if let Some(gui) = &mut self.gui {
            gui.on_left(app, pointer);
        }
    }
    fn on_pointer(&mut self, app: &mut AppState, hit: &PointerHit, pressed: bool) {
        if !self.enabled {
            if pressed {
                self.held.push(*hit);
                self.inner.on_pointer(app, hit, true);
            } else {
                self.release(app, Some(hit.pointer));
            }
            return;
        }
        if !pressed {
            // Release using the original button even if controller orientation changed.
            self.release(app, Some(hit.pointer));
            if let Some(gui) = &mut self.gui {
                gui.on_pointer(app, hit, false);
            }
        } else if let Some(content) = self.content_hit(hit) {
            if !self
                .held
                .iter()
                .any(|h| h.pointer == hit.pointer && h.mode == hit.mode)
            {
                self.held.push(content);
                self.inner.on_pointer(app, &content, true);
            }
        } else if let Some(gui) = &mut self.gui {
            gui.on_pointer(app, hit, true);
        }
    }
    fn on_scroll(&mut self, app: &mut AppState, hit: &PointerHit, delta: WheelDelta) {
        if !self.enabled {
            self.inner.on_scroll(app, hit, delta);
        } else if let Some(content) = self.content_hit(hit) {
            self.inner.on_scroll(app, &content, delta);
        } else if let Some(gui) = &mut self.gui {
            gui.on_scroll(app, hit, delta);
        }
    }
    fn get_interaction_transform(&mut self) -> Option<Affine2> {
        if self.enabled {
            Some(self.geometry.interaction_transform())
        } else {
            self.inner.get_interaction_transform()
        }
    }
    fn interaction_pose(&self) -> Affine3A {
        if self.enabled {
            Affine3A::from_scale(Vec3::splat(width_factor(self.geometry.content)))
                * self.geometry.interaction_pose()
        } else {
            self.inner.interaction_pose()
        }
    }
    fn get_attrib(&self, attrib: BackendAttrib) -> Option<BackendAttribValue> {
        self.inner.get_attrib(attrib)
    }
    fn set_attrib(&mut self, app: &mut AppState, value: BackendAttribValue) -> bool {
        self.release(app, None);
        self.inner.set_attrib(app, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgui::{
        components::ComponentTrait,
        event::{DeviceBitmask, Event, MouseButtonEvent, MouseButtonIndex, MouseMotionEvent},
        layout::LayoutUpdateParams,
    };
    #[test]
    fn releases_keep_original_buttons_and_do_not_release_the_other_pointer() {
        use crate::backend::input::PointerMode;
        let mut panel = ScreenControls::new(Box::new(crate::windowing::backend::DummyBackend {}));
        panel.held = vec![
            PointerHit {
                pointer: 0,
                mode: PointerMode::Left,
                uv: vec2(0.2, 0.3),
                ..Default::default()
            },
            PointerHit {
                pointer: 1,
                mode: PointerMode::Right,
                ..Default::default()
            },
            PointerHit {
                pointer: 0,
                mode: PointerMode::Middle,
                ..Default::default()
            },
        ];
        let released = panel.take_releases(Some(0));
        assert_eq!(released.len(), 2);
        assert_eq!(released[0].mode, PointerMode::Left);
        assert_eq!(released[0].uv, vec2(0.2, 0.3));
        assert_eq!(released[1].mode, PointerMode::Middle);
        assert_eq!(panel.held.len(), 1);
        assert_eq!(panel.held[0].pointer, 1);
        assert!(panel.take_releases(Some(0)).is_empty());
        assert_eq!(panel.take_releases(None)[0].mode, PointerMode::Right);
        assert!(panel.held.is_empty());
    }

    #[test]
    fn screen_geometry_keeps_native_size_center_and_curved_pointer_mapping() {
        for content in [[5120, 1440], [2560, 1440], [1440, 2560]] {
            for expanded in [false, true] {
                let g = PanelGeometry::new(Some(content), false, expanded);
                let factor = width_factor(content);
                let pose = Affine3A::from_scale(Vec3::splat(factor)) * g.interaction_pose();
                let view = Affine3A::from_scale(Vec3::splat(factor)) * g.view_transform();
                let full_major = g.full[0].max(g.full[1]) as f32;
                let center = view.transform_point3(Vec3::new(
                    0.,
                    (g.full[1] - content[1]) as f32 / (2. * full_major),
                    0.,
                ));
                assert!(center.abs_diff_eq(Vec3::ZERO, 0.00001));
                assert!(
                    (view.matrix3.x_axis.length() * g.full[0] as f32 / full_major - factor).abs()
                        < 0.00001
                );
                for curve in [None, Some(0.2)] {
                    for uv in [vec2(0.1, 0.1), vec2(0.8, 0.95)] {
                        let x = uv.x - 0.5;
                        let y = (0.5 - uv.y) * g.full[1] as f32 / g.full[0] as f32;
                        let (x, z) = curve.map_or((x, 0.), |c| {
                            let radius = 1. / (std::f32::consts::TAU * c);
                            let angle = x / radius;
                            (radius * angle.sin(), radius * (1. - angle.cos()))
                        });
                        let surface = pose.transform_point3(Vec3::new(x, y, z));
                        let (_, local) = crate::windowing::raycast_overlay(
                            &Affine3A::from_translation(surface + Vec3::Z),
                            &pose,
                            curve,
                        )
                        .unwrap();
                        assert!(
                            g.interaction_transform()
                                .transform_point2(local)
                                .abs_diff_eq(uv, 0.0001)
                        );
                    }
                }
                let midpoint = vec2(0.5, content[1] as f32 * 0.5 / g.full[1] as f32);
                assert!(
                    g.content_uv(midpoint)
                        .unwrap()
                        .abs_diff_eq(vec2(0.5, 0.5), 0.00001)
                );
                assert!(
                    g.content_uv(vec2(0.5, content[1] as f32 / g.full[1] as f32))
                        .is_none()
                );
                assert!(g.content_uv(vec2(0.5, 1.)).is_none());
            }
        }
    }
    #[test]
    fn native_screen_footer_exposes_shared_actions_at_rendered_hit_positions() {
        for content in [[5120, 1440], [2560, 1440], [1440, 2560]] {
            let globals = wgui::globals::WguiGlobals::new(
                Box::new(crate::gui::asset::GuiAsset {}),
                &wlx_common::locale::WayVRLangProvider::default(),
                &wgui::font_config::WguiFontConfig::default(),
                "/nonexistent-test-assets".into(),
                wgui::palette::WguiColorPalette::get_builtin("default"),
            )
            .unwrap();
            let (mut layout, parser) = wgui::parser::new_layout_from_assets(
                &wgui::parser::ParseDocumentParams {
                    globals,
                    path: wgui::assets::AssetPathRef::BuiltIn("gui/screen-controls.xml"),
                    extra: Default::default(),
                },
                wgui::layout::LayoutParams {
                    resize_to_parent: true,
                    ..Default::default()
                },
            )
            .unwrap();
            let pending = Rc::new(RefCell::new(None));
            bind_buttons(&parser, &pending);
            for expanded in [false, true] {
                let g = PanelGeometry::new(Some(content), false, expanded);
                let scale = content[0] as f32 / 1280.;
                layout.alterables.set_style(
                    parser.get_widget_id("content_space").unwrap(),
                    StyleSetRequest::Height(wgui::taffy::prelude::length(
                        content[1] as f32 / scale,
                    )),
                );
                layout
                    .alterables
                    .set_widget_visible(parser.get_widget_id("quick_controls").unwrap(), expanded);
                let size = vec2(g.full[0] as f32, g.full[1] as f32) / scale;
                layout
                    .update(&mut LayoutUpdateParams {
                        size,
                        timestep_alpha: 1.,
                    })
                    .unwrap();
                assert!(layout.content_size.abs_diff_eq(size, 1.));
                let actions: &[&str] = if expanded {
                    &[
                        "toggle",
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
                    ]
                } else {
                    &["toggle"]
                };
                for action in actions {
                    let b = parser
                        .fetch_component_as::<ComponentButton>(&format!("quick_{action}"))
                        .unwrap();
                    let node = *layout.state.nodes.get(b.base().get_id()).unwrap();
                    let dim = layout.state.get_node_size(node);
                    let mut pos = dim * 0.5;
                    let mut parent = Some(node);
                    while let Some(node) = parent {
                        let l = layout.state.tree.layout(node).unwrap();
                        pos += vec2(l.location.x, l.location.y);
                        parent = layout.state.tree.parent(node);
                    }
                    assert!(
                        pos.x > 0.
                            && pos.x < size.x
                            && pos.y * scale >= content[1] as f32
                            && pos.y < size.y,
                        "{action}: {pos:?}"
                    );
                    assert!(
                        g.content_uv(pos / size).is_none(),
                        "Control must never generate desktop input"
                    );
                    let device = DeviceBitmask::from_index(0);
                    layout
                        .push_event(
                            &Event::MouseMotion(MouseMotionEvent { pos, device }),
                            &mut (),
                            &mut (),
                        )
                        .unwrap();
                    let e = MouseButtonEvent {
                        pos,
                        device,
                        index: MouseButtonIndex::Left,
                    };
                    layout
                        .push_event(&Event::MouseDown(e), &mut (), &mut ())
                        .unwrap();
                    layout
                        .push_event(&Event::MouseUp(e), &mut (), &mut ())
                        .unwrap();
                    assert_eq!(pending.borrow_mut().take().as_deref(), Some(*action));
                }
            }
        }
    }
}
