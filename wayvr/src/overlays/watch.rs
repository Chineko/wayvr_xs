use crate::{
    gui::{
        panel::{
            GuiPanel, NewGuiPanelParams, apply_custom_command, device_list::DeviceList,
            overlay_list::OverlayList, set_list::SetList,
        },
        timer::GuiTimer,
    },
    state::AppState,
    windowing::{Z_ORDER_WATCH, backend::OverlayEventData, window::OverlayWindowConfig},
};
use glam::{Affine3A, Quat, Vec3, vec3};
use std::{cell::Cell, collections::HashMap, rc::Rc, time::Duration};
use wgui::{
    assets::AssetPathRef,
    components::button::ComponentButton,
    parser::{Fetchable, ParseDocumentParams},
};
use wlx_common::{
    common::LeftRight,
    windowing::{OverlayWindowState, Positioning},
};

pub const WATCH_NAME: &str = "watch";
pub const WATCH_POS: Vec3 = vec3(-0.03, -0.01, 0.125);
pub const WATCH_ROT: Quat = Quat::from_xyzw(-0.707_106_6, 0.000_796_361_8, 0.707_106_6, 0.0);

#[derive(Default)]
struct WatchState {
    device_list: DeviceList,
    overlay_list: OverlayList,
    set_list: SetList,
    clock_12h: bool,
}

/// Use the same callbacks in production and pointer-hit regression tests.
/// Older user-supplied watch layouts can omit the shelf entirely.
fn bind_shelf_buttons(parser: &wgui::parser::ParserState) -> anyhow::Result<()> {
    if parser.get_widget_id("shelf_slot").is_err() {
        return Ok(());
    }
    let actions = parser.get_widget_id("shelf_actions")?;
    let panels = parser.get_widget_id("shelf_panels")?;
    let buttons = ["btn_shelf_actions", "btn_shelf_panels"]
        .map(|id| parser.fetch_component_as::<ComponentButton>(id))
        .into_iter()
        .collect::<anyhow::Result<Vec<_>>>()?;
    let selected = Rc::new(Cell::new(None));
    for (index, button) in buttons.iter().enumerate() {
        let selected = selected.clone();
        let buttons: Vec<_> = buttons.iter().map(Rc::downgrade).collect();
        button.on_click(Rc::new(move |common, _| {
            let next = if selected.get() == Some(index) {
                None
            } else {
                Some(index)
            };
            selected.set(next);
            for (i, widget) in [actions, panels].into_iter().enumerate() {
                common
                    .alterables
                    .set_widget_visible(widget, next == Some(i));
                if let Some(button) = buttons[i].upgrade() {
                    button.set_sticky_state(common, next == Some(i));
                }
            }
            Ok(())
        }));
    }
    Ok(())
}

pub fn create_watch(app: &mut AppState) -> anyhow::Result<OverlayWindowConfig> {
    let state = WatchState {
        clock_12h: app.session.config.clock_12h,
        ..Default::default()
    };
    let watch_xml = "gui/watch.xml";
    #[cfg(feature = "wayland")]
    let window_capture_available =
        app.feats.desktop_backend.is_wayland() && app.screencast_manager.is_some();
    #[cfg(not(feature = "wayland"))]
    let window_capture_available = false;

    let mut panel = GuiPanel::new_from_template(
        app,
        watch_xml,
        state,
        NewGuiPanelParams {
            extra_vars: HashMap::from([
                (
                    "not_window_capture".into(),
                    if window_capture_available {
                        "0".into()
                    } else {
                        "1".into()
                    },
                ),
                (
                    "not_kde".into(),
                    if window_capture_available
                        && std::env::var("XDG_CURRENT_DESKTOP")
                            .unwrap_or_default()
                            .split(':')
                            .any(|name| name.eq_ignore_ascii_case("KDE"))
                    {
                        "0".into()
                    } else {
                        "1".into()
                    },
                ),
            ]),
            ..Default::default()
        },
    )?;

    bind_shelf_buttons(&panel.parser_state)?;
    if panel.parser_state.get_widget_id("shelf_slot").is_ok() {
        panel.hit_regions = [
            "watch_base",
            "shelf_actions",
            "shelf_panels",
            "battery_strip",
        ]
        .map(|id| panel.parser_state.get_widget_id(id))
        .into_iter()
        .collect::<anyhow::Result<Vec<_>>>()?;
    }
    sets_or_overlays(&mut panel, app);

    let doc_params = ParseDocumentParams {
        globals: panel.layout.state.globals.clone(),
        path: AssetPathRef::FileOrBuiltIn(watch_xml),
        extra: panel.doc_extra.take().unwrap_or_default(),
    };

    panel.on_notify = Some(Box::new({
        let name = WATCH_NAME;
        move |panel, app, event_data| {
            let mut elems_changed = panel.state.overlay_list.on_notify(
                &mut panel.layout,
                &mut panel.parser_state,
                &event_data,
                &doc_params,
            )?;

            elems_changed |= panel.state.set_list.on_notify(
                &mut panel.layout,
                &mut panel.parser_state,
                &event_data,
                &doc_params,
            )?;

            elems_changed |= panel.state.device_list.on_notify(
                app,
                &mut panel.layout,
                &mut panel.parser_state,
                &event_data,
                &doc_params,
            )?;

            match event_data {
                OverlayEventData::EditModeChanged(edit_mode) => {
                    if let Ok(btn_edit_mode) = panel
                        .parser_state
                        .fetch_component_as::<ComponentButton>("btn_edit_mode")
                    {
                        btn_edit_mode.set_sticky_state(&mut panel.layout.common(), edit_mode);
                    }
                }
                OverlayEventData::SettingsChanged => {
                    panel.layout.mark_redraw();
                    sets_or_overlays(panel, app);

                    if app.session.config.clock_12h != panel.state.clock_12h {
                        panel.state.clock_12h = app.session.config.clock_12h;

                        let clock_root = panel.parser_state.get_widget_id("clock_root")?;
                        panel.layout.remove_children(clock_root);

                        panel.parser_state.instantiate_template(
                            &doc_params,
                            "Clock",
                            &mut panel.layout,
                            clock_root,
                            Default::default(),
                        )?;
                        if let Ok(root) = panel.parser_state.get_widget_id("timezones_root") {
                            panel.layout.remove_children(root);
                            panel.parser_state.instantiate_template(
                                &doc_params,
                                "Timezones",
                                &mut panel.layout,
                                root,
                                Default::default(),
                            )?;
                        }

                        elems_changed = true;
                    }
                }
                OverlayEventData::CustomCommand { element, command } => {
                    if let Err(e) = apply_custom_command(panel, app, &element, &command) {
                        log::warn!("Could not apply {command:?} on {name}/{element}: {e:?}");
                    } else {
                        elems_changed = true;
                    }
                }
                _ => {}
            }

            if elems_changed {
                panel.process_custom_elems(app);
            }

            Ok(())
        }
    }));

    panel
        .timers
        .push(GuiTimer::new(Duration::from_millis(100), 0));

    let positioning = Positioning::FollowHand {
        hand: LeftRight::Left,
        lerp: 1.0,
    };

    panel.update_layout(app)?;

    Ok(OverlayWindowConfig {
        name: WATCH_NAME.into(),
        z_order: Z_ORDER_WATCH,
        default_state: OverlayWindowState {
            grabbable: false,
            interactable: true,
            positioning,
            transform: Affine3A::from_scale_rotation_translation(
                Vec3::ONE * 0.115,
                WATCH_ROT,
                WATCH_POS,
            ),
            angle_fade: true,
            ..OverlayWindowState::default()
        },
        show_on_spawn: app.session.config.enable_watch,
        global: true,
        ..OverlayWindowConfig::from_backend(Box::new(panel))
    })
}

fn sets_or_overlays(panel: &mut GuiPanel<WatchState>, app: &mut AppState) {
    let visible = if app.session.config.sets_on_watch {
        [false, true]
    } else {
        [true, false]
    };

    let widget = [
        panel
            .parser_state
            .get_widget_id("panels_root")
            .unwrap_or_default(),
        panel
            .parser_state
            .get_widget_id("sets_root")
            .unwrap_or_default(),
    ];

    for i in 0..2 {
        panel
            .layout
            .alterables
            .set_widget_visible(widget[i], visible[i]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgui::{
        components::ComponentTrait,
        event::{DeviceBitmask, Event, MouseButtonEvent, MouseButtonIndex, MouseMotionEvent},
        layout::{Layout, LayoutUpdateParams},
        parser::{ParseDocumentExtra, ParserState, TemplateParams},
    };

    fn update(layout: &mut Layout) {
        layout
            .update(&mut LayoutUpdateParams {
                size: glam::vec2(2048., 2048.),
                timestep_alpha: 1.,
            })
            .unwrap();
    }

    fn bounds(layout: &Layout, id: wgui::layout::WidgetID) -> (glam::Vec2, glam::Vec2) {
        let node = *layout.state.nodes.get(id).unwrap();
        let size = layout.state.get_node_size(node);
        let mut pos = glam::Vec2::ZERO;
        let mut parent = Some(node);
        while let Some(node) = parent {
            let l = layout.state.tree.layout(node).unwrap();
            pos += glam::vec2(l.location.x, l.location.y);
            parent = layout.state.tree.parent(node);
        }
        (pos, size)
    }

    fn click(layout: &mut Layout, parser: &ParserState, id: &str) {
        let b = parser.fetch_component_as::<ComponentButton>(id).unwrap();
        let (pos, size) = bounds(layout, b.base().get_id());
        assert!(size.x >= 44. && size.y >= 44., "{id}: {size:?}");
        let pos = pos + size * 0.5;
        let device = DeviceBitmask::from_index(0);
        layout
            .push_event(
                &Event::MouseMotion(MouseMotionEvent { pos, device }),
                &mut (),
                &mut (),
            )
            .unwrap();
        let event = MouseButtonEvent {
            pos,
            device,
            index: MouseButtonIndex::Left,
        };
        layout
            .push_event(&Event::MouseDown(event), &mut (), &mut ())
            .unwrap();
        layout
            .push_event(&Event::MouseUp(event), &mut (), &mut ())
            .unwrap();
        update(layout);
    }

    #[test]
    fn shelf_pointer_hits_keep_base_stationary_and_preserve_actions() {
        for supported in ["0", "1"] {
            let globals = wgui::globals::WguiGlobals::new(
                Box::new(crate::gui::asset::GuiAsset {}),
                &wlx_common::locale::WayVRLangProvider::default(),
                &wgui::font_config::WguiFontConfig::default(),
                "/nonexistent-test-assets".into(),
                wgui::palette::WguiColorPalette::get_builtin("default"),
            )
            .unwrap();
            let params = ParseDocumentParams {
                globals,
                path: AssetPathRef::BuiltIn("gui/watch.xml"),
                extra: ParseDocumentExtra {
                    extra_vars: HashMap::from([
                        ("not_window_capture".into(), supported.into()),
                        ("not_kde".into(), supported.into()),
                    ]),
                    ..Default::default()
                },
            };
            let (mut layout, mut parser) =
                wgui::parser::new_layout_from_assets(&params, Default::default()).unwrap();
            bind_shelf_buttons(&parser).unwrap();
            update(&mut layout);
            let extent = layout.content_size;
            let base = bounds(&layout, parser.get_widget_id("watch_base").unwrap());
            let regions = [
                "watch_base",
                "shelf_actions",
                "shelf_panels",
                "battery_strip",
            ]
            .map(|id| parser.get_widget_id(id).unwrap());
            assert_eq!(extent, glam::vec2(584., 720.));
            // Reserved transparent space must not intercept another overlay's pointer.
            for pos in [glam::vec2(280., 100.), glam::vec2(280., 600.)] {
                layout
                    .push_event(
                        &Event::MouseMotion(MouseMotionEvent {
                            pos,
                            device: DeviceBitmask::from_index(0),
                        }),
                        &mut (),
                        &mut (),
                    )
                    .unwrap();
                assert!(!crate::gui::panel::pointer_in_regions(
                    &layout, &regions, pos
                ));
            }
            for (button, shown, hidden) in [
                ("btn_shelf_actions", "shelf_actions", "shelf_panels"),
                ("btn_shelf_panels", "shelf_panels", "shelf_actions"),
            ] {
                click(&mut layout, &parser, button);
                let (pos, size) = bounds(&layout, parser.get_widget_id(shown).unwrap());
                // Pointer motion populates the same absolute boundaries used by GuiPanel.
                let pos = pos + size * 0.5;
                layout
                    .push_event(
                        &Event::MouseMotion(MouseMotionEvent {
                            pos,
                            device: DeviceBitmask::from_index(0),
                        }),
                        &mut (),
                        &mut (),
                    )
                    .unwrap();
                assert!(crate::gui::panel::pointer_in_regions(
                    &layout, &regions, pos
                ));
                assert!(bounds(&layout, parser.get_widget_id(shown).unwrap()).1.y > 200.);
                assert_eq!(
                    bounds(&layout, parser.get_widget_id(hidden).unwrap()).1,
                    glam::Vec2::ZERO
                );
                assert_eq!(layout.content_size, extent);
                assert_eq!(
                    bounds(&layout, parser.get_widget_id("watch_base").unwrap()),
                    base
                );
            }
            click(&mut layout, &parser, "btn_shelf_panels");
            assert_eq!(
                bounds(&layout, parser.get_widget_id("shelf_panels").unwrap()).1,
                glam::Vec2::ZERO
            );
            click(&mut layout, &parser, "btn_shelf_actions");
            for id in [
                "btn_keyboard",
                "btn_edit_mode",
                "btn_recenter",
                "btn_floor",
                "btn_dashboard",
                "btn_layouts",
                "btn_performance",
            ] {
                let b = parser.fetch_component_as::<ComponentButton>(id).unwrap();
                let hit = Rc::new(Cell::new(false));
                let result = hit.clone();
                b.on_click(Rc::new(move |_, _| {
                    result.set(true);
                    Ok(())
                }));
                click(&mut layout, &parser, id);
                assert!(hit.get(), "Missed {id}");
            }
            assert_eq!(
                parser
                    .fetch_component_as::<ComponentButton>("btn_add_interactive")
                    .is_ok(),
                supported == "0"
            );
            if supported == "0" {
                for id in ["btn_add_capture", "btn_add_interactive"] {
                    let button = parser.fetch_component_as::<ComponentButton>(id).unwrap();
                    let hit = Rc::new(Cell::new(false));
                    let result = hit.clone();
                    button.on_click(Rc::new(move |_, _| {
                        result.set(true);
                        Ok(())
                    }));
                    click(&mut layout, &parser, id);
                    assert!(hit.get(), "Missed {id}");
                }
            }
            // Three rows of controller / tracker icons fit without moving the watch.
            let root = parser.get_widget_id("devices_root").unwrap();
            for i in 0..18 {
                let mut vars = TemplateParams::new();
                vars.insert_rc("idx", i.to_string().into());
                parser
                    .instantiate_template(
                        &params,
                        if i < 2 { "LeftHand" } else { "Tracker" },
                        &mut layout,
                        root,
                        vars,
                    )
                    .unwrap();
            }
            layout
                .alterables
                .set_widget_visible(parser.get_widget_id("battery_strip").unwrap(), true);
            update(&mut layout);
            let (pos, size) = bounds(&layout, root);
            assert!(
                pos.x >= 32. && pos.x + size.x <= 552.,
                "Battery width: {pos:?} {size:?}"
            );
            assert!(pos.y + size.y <= 720., "Battery height: {pos:?} {size:?}");
            assert_eq!(
                bounds(&layout, parser.get_widget_id("watch_base").unwrap()),
                base
            );
            assert_eq!(layout.content_size, extent);
        }
    }
}
