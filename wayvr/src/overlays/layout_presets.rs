//! Small in-VR preset manager; uses the existing GUI and normal restart path.
use crate::{
    backend::task::{OverlayTask, TaskType},
    gui::{
        panel::{GuiPanel, NewGuiPanelParams},
        timer::GuiTimer,
    },
    layout_presets::{Action, Library},
    state::AppState,
    windowing::{
        backend::OverlayEventData,
        window::{OverlayCategory, OverlayWindowConfig},
    },
};
use glam::{Affine3A, Quat, Vec3, vec3};
use std::{cell::RefCell, rc::Rc, time::Duration};
use wgui::{
    components::button::ComponentButton,
    event::{CallbackDataCommon, EventListenerKind},
    layout::WidgetID,
    parser::Fetchable,
    widget::{EventResult, label::WidgetLabel},
};
use wlx_common::windowing::OverlayWindowState;

struct Ui {
    library: Library,
    selected: usize,
    name: String,
    message: String,
    confirm: Option<String>,
    pending: Rc<RefCell<Option<String>>>,
    labels: [WidgetID; 3],
}
fn refresh(common: &mut CallbackDataCommon, state: &Ui) {
    let selected = state.library.presets.get(state.selected).map_or_else(
        || "No saved presets".into(),
        |p| {
            format!(
                "{} / {}: {}{}",
                state.selected + 1,
                state.library.presets.len(),
                p.name,
                if state.library.startup.as_ref() == Some(&p.id) {
                    " [startup]"
                } else {
                    ""
                }
            )
        },
    );
    for (id, text) in state.labels.into_iter().zip([
        selected,
        format!("Name: {}", state.name),
        state.message.clone(),
    ]) {
        if let Some(mut label) = common.state.widgets.get_as::<WidgetLabel>(id) {
            label.set_text(common, wgui::i18n::Translation::from_raw_text(&text));
        }
    }
}
pub fn create(app: &mut AppState) -> anyhow::Result<OverlayWindowConfig> {
    let (library, message) = match Library::read(&crate::layout_presets::path()) {
        Ok(library) => (
            library,
            "Save the current layout, or load a saved layout with a restart.".into(),
        ),
        Err(error) => (Library::default(), format!("Preset error: {error:#}")),
    };
    let pending = Rc::new(RefCell::new(None));
    let mut panel = GuiPanel::new_from_template(
        app,
        "gui/layout-presets.xml",
        Ui {
            library,
            selected: 0,
            name: String::new(),
            message,
            confirm: None,
            pending: pending.clone(),
            labels: Default::default(),
        },
        NewGuiPanelParams {
            resize_to_parent: true,
            ..Default::default()
        },
    )?;
    panel.max_size = glam::vec2(1100., 900.);
    panel.state.labels =
        ["selection", "name", "status"].map(|id| panel.parser_state.get_widget_id(id).unwrap());
    let ids: Vec<_> = panel.parser_state.data.ids.keys().cloned().collect();
    for id in ids {
        let Ok(button) = panel
            .parser_state
            .fetch_component_as::<ComponentButton>(&id)
        else {
            continue;
        };
        let pending = pending.clone();
        button.on_click(Rc::new(move |_, _| {
            *pending.borrow_mut() = Some(id.to_string());
            Ok(())
        }));
    }
    panel.layout.add_event_listener::<AppState, Ui>(
        panel.layout.tree_root_widget,
        EventListenerKind::InternalStateChange,
        Box::new(|common, _, app, state| {
            let command = state.pending.borrow_mut().take();
            let Some(command) = command else {
                return Ok(EventResult::Pass);
            };
            let selected = state
                .library
                .presets
                .get(state.selected)
                .map(|p| p.id.clone());
            let sensitive = matches!(command.as_str(), "replace" | "delete");
            if sensitive && state.confirm.as_ref() != Some(&command) {
                state.confirm = Some(command.clone());
                state.message = format!("Click {command} again to confirm the selected preset.");
                refresh(common, state);
                return Ok(EventResult::Consumed);
            }
            state.confirm = None;
            let action = match command.as_str() {
                "previous" | "next" => {
                    let count = state.library.presets.len();
                    if count > 0 {
                        state.selected = if command == "next" {
                            (state.selected + 1) % count
                        } else {
                            (state.selected + count - 1) % count
                        };
                        state
                            .name
                            .clone_from(&state.library.presets[state.selected].name);
                    }
                    None
                }
                "space" => {
                    if state.name.chars().count() < 48 {
                        state.name.push(' ');
                    }
                    None
                }
                "backspace" => {
                    state.name.pop();
                    None
                }
                "clear" => {
                    state.name.clear();
                    None
                }
                "save" => Some(Action::Save(state.name.clone())),
                "replace" => selected.map(Action::Replace),
                "rename" => selected.map(|id| Action::Rename(id, state.name.clone())),
                "delete" => selected.map(Action::Delete),
                "startup" => selected.map(|id| Action::Startup(Some(id))),
                "last_session" => Some(Action::Startup(None)),
                "load" => selected.map(Action::Load),
                _ => {
                    if let Some(key) = command.strip_prefix("key_")
                        && state.name.chars().count() < 48
                    {
                        state.name.push_str(key);
                    }
                    None
                }
            };
            state.message = if let Some(action) = action {
                app.tasks
                    .enqueue(TaskType::Overlay(OverlayTask::LayoutPreset(action)));
                "Working…".into()
            } else {
                "Choose a preset, or enter a name to save a new one.".into()
            };
            refresh(common, state);
            Ok(EventResult::Consumed)
        }),
    );
    panel
        .timers
        .push(GuiTimer::new(Duration::from_millis(50), 0));
    panel.on_notify = Some(Box::new(|panel, _, event| {
        if let OverlayEventData::CustomCommand {
            element,
            command: crate::backend::task::ModifyPanelCommand::SetText(message),
        } = event
            && element == "preset_result"
        {
            match Library::read(&crate::layout_presets::path()) {
                Ok(library) => {
                    panel.state.library = library;
                    panel.state.selected = if message == "Preset saved" {
                        panel.state.library.presets.len().saturating_sub(1)
                    } else {
                        panel
                            .state
                            .selected
                            .min(panel.state.library.presets.len().saturating_sub(1))
                    };
                }
                Err(error) => {
                    log::warn!("Preset refresh failed: {error:#}");
                }
            }
            panel.state.message = message;
            refresh(
                &mut CallbackDataCommon {
                    state: &panel.layout.state,
                    alterables: &mut panel.layout.alterables,
                },
                &panel.state,
            );
        }
        Ok(())
    }));
    panel.update_layout(app)?;
    refresh(
        &mut CallbackDataCommon {
            state: &panel.layout.state,
            alterables: &mut panel.layout.alterables,
        },
        &panel.state,
    );
    Ok(OverlayWindowConfig {
        name: "XSLayouts".into(),
        category: OverlayCategory::Panel,
        global: true,
        show_on_spawn: false,
        default_state: OverlayWindowState {
            interactable: true,
            grabbable: true,
            transform: Affine3A::from_scale_rotation_translation(
                Vec3::splat(0.7),
                Quat::IDENTITY,
                vec3(0., 0.15, -0.7),
            ),
            ..Default::default()
        },
        ..OverlayWindowConfig::from_backend(Box::new(panel))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manager_layout_fills_viewport_and_exposes_actions() {
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
                path: wgui::assets::AssetPathRef::BuiltIn("gui/layout-presets.xml"),
                extra: Default::default(),
            },
            wgui::layout::LayoutParams {
                resize_to_parent: true,
                ..Default::default()
            },
        )
        .unwrap();
        layout
            .update(&mut wgui::layout::LayoutUpdateParams {
                size: glam::vec2(1100., 900.),
                timestep_alpha: 1.,
            })
            .unwrap();
        assert_eq!(layout.content_size, glam::vec2(1100., 900.));
        for id in [
            "save",
            "replace",
            "rename",
            "delete",
            "load",
            "startup",
            "last_session",
            "previous",
            "next",
            "key_A",
            "key_9",
            "space",
            "backspace",
            "clear",
        ] {
            use wgui::components::ComponentTrait;
            let button = parser.fetch_component_as::<ComponentButton>(id).unwrap();
            let clicked = Rc::new(std::cell::Cell::new(false));
            let result = clicked.clone();
            button.on_click(Rc::new(move |_, _| {
                result.set(true);
                Ok(())
            }));
            let node = *layout.state.nodes.get(button.base().get_id()).unwrap();
            let mut pos = layout.state.get_node_size(node) * 0.5;
            let mut parent = Some(node);
            while let Some(node) = parent {
                let l = layout.state.tree.layout(node).unwrap();
                pos += glam::vec2(l.location.x, l.location.y);
                parent = layout.state.tree.parent(node);
            }
            assert!(
                pos.x > 0. && pos.x < 1100. && pos.y > 0. && pos.y < 900.,
                "{id}: {pos:?}"
            );
            use wgui::event::{
                DeviceBitmask, Event, MouseButtonEvent, MouseButtonIndex, MouseMotionEvent,
            };
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
            assert!(clicked.get(), "Rendered button center missed {id}");
        }
    }
}
