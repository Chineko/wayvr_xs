//! Compact wrist preset manager; storage and loading use the existing task path.
use crate::{
    backend::task::{ModifyPanelCommand, OverlayTask, TaskType},
    gui::panel::GuiPanel,
    layout_presets::{Action, Library},
    state::AppState,
    windowing::backend::OverlayEventData,
};
use std::{cell::RefCell, rc::Rc};
use wgui::{
    components::button::ComponentButton,
    event::{CallbackDataCommon, EventListenerKind},
    parser::Fetchable,
    widget::{EventResult, label::WidgetLabel},
};

const PAGE: usize = 3;
struct Ui {
    library: Library,
    selected: usize,
    name: String,
    message: String,
    digits: bool,
    confirm: Option<String>,
    view: &'static str,
    page: usize,
    pending: Rc<RefCell<Option<String>>>,
}
impl Ui {
    fn new(library: Library, message: String) -> Self {
        let name = library
            .presets
            .first()
            .map_or_else(String::new, |p| p.name.clone());
        Self {
            library,
            selected: 0,
            name,
            message,
            digits: false,
            confirm: None,
            view: "main",
            page: 0,
            pending: Default::default(),
        }
    }
    fn command(&mut self, command: &str) -> Option<Action> {
        let selected = self
            .library
            .presets
            .get(self.selected)
            .map(|p| p.id.clone());
        if let Some(slot) = command
            .strip_prefix("choose_")
            .and_then(|s| s.parse::<usize>().ok())
        {
            let index = self.page * PAGE + slot;
            if let Some(p) = self.library.presets.get(index) {
                self.selected = index;
                self.name.clone_from(&p.name);
                self.view = "main";
                self.confirm = None;
            }
            return None;
        }
        let action = match command {
            "picker" => {
                self.view = "list";
                self.page = self.selected / PAGE;
                None
            }
            "edit_name" => {
                self.view = "keys";
                None
            }
            "done" | "back" => {
                self.view = "main";
                None
            }
            "previous" => {
                self.page = self.page.saturating_sub(1);
                None
            }
            "next" => {
                self.page =
                    (self.page + 1).min(self.library.presets.len().saturating_sub(1) / PAGE);
                None
            }
            "digits" => {
                self.digits = !self.digits;
                None
            }
            "clear" => {
                self.name.clear();
                None
            }
            "backspace" => {
                self.name.pop();
                None
            }
            "space" => {
                if self.name.chars().count() < 48 {
                    self.name.push(' ');
                }
                None
            }
            "save" => {
                match self
                    .library
                    .presets
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(self.name.trim()))
                {
                    Some(p) => Some(Action::Replace(p.id.clone())),
                    None => Some(Action::Save(self.name.clone())),
                }
            }
            "rename" => selected.map(|id| Action::Rename(id, self.name.clone())),
            "delete" => selected.map(Action::Delete),
            "startup" => selected.map(|id| {
                Action::Startup(if self.library.startup.as_ref() == Some(&id) {
                    None
                } else {
                    Some(id)
                })
            }),
            "load" => selected.map(Action::Load),
            _ => {
                if let Some(key) = command.strip_prefix("key_")
                    && self.name.chars().count() < 48
                {
                    self.name.push_str(match key {
                        "dash" => "-",
                        "underscore" => "_",
                        "period" => ".",
                        "apostrophe" => "'",
                        _ => key,
                    });
                }
                None
            }
        };
        let sensitive = matches!(action, Some(Action::Replace(_) | Action::Delete(_)));
        if sensitive && self.confirm.as_deref() != Some(command) {
            self.confirm = Some(command.into());
            self.message = format!("Click {command} again to confirm.");
            return None;
        }
        self.confirm = None;
        if action.is_some() {
            self.message = "Working…".into();
        } else if !matches!(command, "previous" | "next") {
            self.message.clear();
        }
        action
    }
}
fn label(common: &mut CallbackDataCommon, parser: &impl Fetchable, id: &str, text: &str) {
    if let Ok(id) = parser.get_widget_id(id)
        && let Some(mut label) = common.state.widgets.get_as::<WidgetLabel>(id)
    {
        label.set_text(common, wgui::i18n::Translation::from_raw_text(text));
    }
}
fn refresh(common: &mut CallbackDataCommon, parser: &impl Fetchable, ui: &Ui) {
    for view in ["main", "list", "keys"] {
        if let Ok(id) = parser.get_widget_id(&format!("layout_{view}")) {
            common.alterables.set_widget_visible(id, ui.view == view);
        }
    }
    for (id, visible) in [
        ("layout_header", ui.view != "keys"),
        ("layout_status", ui.view != "keys"),
        ("layout_letters", !ui.digits),
        ("layout_numbers", ui.digits),
    ] {
        if let Ok(id) = parser.get_widget_id(id) {
            common.alterables.set_widget_visible(id, visible);
        }
    }
    let preset = ui.library.presets.get(ui.selected);
    let chosen = preset.map_or("Choose a layout", |p| p.name.as_str());
    label(
        common,
        parser,
        "layout_selection",
        &format!("{} ▾", display_name(chosen, 24)),
    );
    label(
        common,
        parser,
        "layout_name",
        &display_name(
            if ui.name.is_empty() {
                "Enter a name…"
            } else {
                &ui.name
            },
            24,
        ),
    );
    label(
        common,
        parser,
        "layout_key_name",
        &display_name(&ui.name, 36),
    );
    label(common, parser, "layout_status", &ui.message);
    label(
        common,
        parser,
        "layout_startup_label",
        if preset.is_some_and(|p| ui.library.startup.as_ref() == Some(&p.id)) {
            "Startup ✓"
        } else {
            "Use at startup"
        },
    );
    label(
        common,
        parser,
        "layout_page",
        &format!(
            "{} / {}",
            ui.page + 1,
            ui.library.presets.len().max(1).div_ceil(PAGE)
        ),
    );
    for slot in 0..PAGE {
        let p = ui.library.presets.get(ui.page * PAGE + slot);
        if let Ok(button) =
            parser.fetch_component_as::<ComponentButton>(&format!("layout_choose_{slot}"))
        {
            common
                .alterables
                .set_widget_visible(button.get_rect(), p.is_some());
            button.set_text(
                common,
                wgui::i18n::Translation::from_raw_text(
                    &p.map_or(String::new(), |p| display_name(&p.name, 40)),
                ),
            );
        }
    }
}
fn display_name(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        name.into()
    } else {
        format!("{}…", name.chars().take(max - 1).collect::<String>())
    }
}
fn bind_buttons(parser: &wgui::parser::ParserState, pending: &Rc<RefCell<Option<String>>>) {
    for id in parser
        .data
        .components_by_id
        .keys()
        .filter(|id| id.starts_with("layout_"))
    {
        let Ok(button) = parser.fetch_component_as::<ComponentButton>(id) else {
            continue;
        };
        let command = id.trim_start_matches("layout_").to_string();
        let pending = pending.clone();
        button.on_click(Rc::new(move |_, _| {
            *pending.borrow_mut() = Some(command.clone());
            Ok(())
        }));
    }
}
pub struct WristManager {
    state: Rc<RefCell<Ui>>,
}
impl WristManager {
    pub fn notify(
        &self,
        common: &mut CallbackDataCommon,
        parser: &impl Fetchable,
        event: &OverlayEventData,
    ) {
        if let OverlayEventData::CustomCommand {
            element,
            command: ModifyPanelCommand::SetText(message),
        } = event
            && element == "preset_result"
        {
            let mut ui = self.state.borrow_mut();
            match Library::read(&crate::layout_presets::path()) {
                Ok(library) => {
                    let selected = ui.library.presets.get(ui.selected).map(|p| p.id.clone());
                    ui.selected = if message == "Preset saved" {
                        library.presets.len().saturating_sub(1)
                    } else {
                        selected
                            .and_then(|id| library.presets.iter().position(|p| p.id == id))
                            .unwrap_or(0)
                    };
                    ui.library = library;
                    if matches!(
                        message.as_str(),
                        "Preset saved" | "Preset replaced" | "Preset renamed" | "Preset deleted"
                    ) {
                        ui.name = ui
                            .library
                            .presets
                            .get(ui.selected)
                            .map_or_else(String::new, |p| p.name.clone());
                    }
                    ui.page = ui
                        .page
                        .min(ui.library.presets.len().saturating_sub(1) / PAGE);
                }
                Err(e) => log::warn!("Preset refresh failed: {e:#}"),
            }
            ui.message.clone_from(message);
            refresh(common, parser, &ui);
        }
    }
}
pub fn attach<S: 'static>(panel: &mut GuiPanel<S>) -> WristManager {
    let (library, message) = match Library::read(&crate::layout_presets::path()) {
        Ok(l) => (l, "Loading a layout restarts WayVR.".into()),
        Err(e) => (Library::default(), format!("Preset error: {e:#}")),
    };
    let ui = Rc::new(RefCell::new(Ui::new(library, message)));
    bind_buttons(&panel.parser_state, &ui.borrow().pending);
    let parser = panel.parser_state.data.clone();
    refresh(&mut panel.layout.common(), &parser, &ui.borrow());
    let state = ui.clone();
    panel.layout.add_event_listener::<AppState, S>(
        panel.layout.tree_root_widget,
        EventListenerKind::InternalStateChange,
        Box::new(move |common, _, app, _| {
            let mut ui = state.borrow_mut();
            let command = ui.pending.borrow_mut().take();
            if let Some(command) = command {
                if let Some(action) = ui.command(&command) {
                    app.tasks
                        .enqueue(TaskType::Overlay(OverlayTask::LayoutPreset(action)));
                }
                refresh(common, &parser, &ui);
            }
            Ok(EventResult::Pass)
        }),
    );
    WristManager { state: ui }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgui::{
        components::ComponentTrait,
        event::{DeviceBitmask, Event, MouseButtonEvent, MouseButtonIndex, MouseMotionEvent},
        layout::{Layout, LayoutUpdateParams},
    };
    fn library() -> Library {
        Library {
            presets: (0..7)
                .map(|i| crate::layout_presets::Preset {
                    id: format!("id-{i}"),
                    name: format!("Desk setup {i}"),
                    state: crate::config::AutoState {
                        app_panels: vec![],
                        screen_splits: vec![],
                        sets: vec![],
                        spawn_overlays: vec![],
                        last_set: 0,
                        attribs: Default::default(),
                    },
                })
                .collect(),
            ..Default::default()
        }
    }
    #[test]
    fn selection_naming_confirmation_and_startup_reuse_stable_ids() {
        let mut ui = Ui::new(library(), String::new());
        ui.command("picker");
        ui.command("next");
        ui.command("choose_1");
        assert_eq!(ui.selected, 4);
        assert_eq!(ui.name, "Desk setup 4");
        assert!(matches!(ui.command("load"), Some(Action::Load(id)) if id == "id-4"));
        assert!(ui.command("save").is_none());
        assert!(matches!(ui.command("save"), Some(Action::Replace(id)) if id == "id-4"));
        ui.command("delete");
        ui.command("edit_name");
        assert!(
            ui.command("delete").is_none(),
            "Editing must cancel deletion confirmation"
        );
        ui.command("clear");
        for key in ["key_A", "space", "key_2", "key_dash", "key_B"] {
            ui.command(key);
        }
        assert_eq!(ui.name, "A 2-B");
        assert!(matches!(ui.command("save"), Some(Action::Save(name)) if name == "A 2-B"));
        assert!(
            matches!(ui.command("rename"), Some(Action::Rename(id, name)) if id == "id-4" && name == "A 2-B")
        );
        assert!(matches!(ui.command("startup"), Some(Action::Startup(Some(id))) if id == "id-4"));
        ui.library.startup = Some("id-4".into());
        assert!(matches!(ui.command("startup"), Some(Action::Startup(None))));
        ui.name = "界".repeat(48);
        ui.command("key_A");
        assert_eq!(ui.name.chars().count(), 48);
        assert_eq!(display_name(&ui.name, 24).chars().count(), 24);
    }
    fn update(layout: &mut Layout) {
        layout
            .update(&mut LayoutUpdateParams {
                size: glam::vec2(2048., 2048.),
                timestep_alpha: 1.,
            })
            .unwrap();
    }
    fn center(layout: &Layout, id: wgui::layout::WidgetID) -> glam::Vec2 {
        let node = *layout.state.nodes.get(id).unwrap();
        let mut pos = layout.state.get_node_size(node) * 0.5;
        let mut parent = Some(node);
        while let Some(node) = parent {
            let l = layout.state.tree.layout(node).unwrap();
            pos += glam::vec2(l.location.x, l.location.y);
            parent = layout.state.tree.parent(node);
        }
        pos
    }
    #[test]
    fn wrist_picker_keyboard_and_actions_have_real_hit_targets_without_resizing() {
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
                path: wgui::assets::AssetPathRef::BuiltIn("gui/watch.xml"),
                extra: wgui::parser::ParseDocumentExtra {
                    extra_vars: std::collections::HashMap::from([
                        ("not_kde".into(), "0".into()),
                        ("not_window_capture".into(), "0".into()),
                    ]),
                    ..Default::default()
                },
            },
            Default::default(),
        )
        .unwrap();
        let mut ui = Ui::new(library(), String::new());
        bind_buttons(&parser, &ui.pending);
        layout
            .alterables
            .set_widget_visible(parser.get_widget_id("shelf_layouts").unwrap(), true);
        refresh(&mut layout.common(), &parser, &ui);
        update(&mut layout);
        let base = center(&layout, parser.get_widget_id("watch_base").unwrap());
        for id in [
            "picker",
            "next",
            "choose_1",
            "edit_name",
            "clear",
            "key_A",
            "space",
            "digits",
            "key_2",
            "digits",
            "key_dash",
            "key_B",
            "backspace",
            "done",
            "save",
            "rename",
            "startup",
            "delete",
            "delete",
            "load",
        ] {
            let b = parser
                .fetch_component_as::<ComponentButton>(&format!("layout_{id}"))
                .unwrap();
            let node = *layout.state.nodes.get(b.base().get_id()).unwrap();
            assert!(
                wgui::widget::is_node_visible(&layout.state.tree, node),
                "Hidden {id}"
            );
            let size = layout.state.get_node_size(node);
            assert!(size.x >= 44. && size.y >= 44., "{id}: {size:?}");
            let pos = center(&layout, b.base().get_id());
            assert!(
                pos.x > 32. && pos.x < 552. && pos.y > 32. && pos.y < 352.,
                "{id}: {pos:?}"
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
            assert_eq!(
                ui.pending.borrow_mut().take().as_deref(),
                Some(id),
                "Missed {id}"
            );
            ui.command(id);
            refresh(&mut layout.common(), &parser, &ui);
            update(&mut layout);
            assert_eq!(layout.content_size, glam::vec2(584., 800.));
            assert_eq!(
                center(&layout, parser.get_widget_id("watch_base").unwrap()),
                base
            );
        }
        ui.library.presets.clear();
        ui.view = "list";
        refresh(&mut layout.common(), &parser, &ui);
        update(&mut layout);
        let b = parser
            .fetch_component_as::<ComponentButton>("layout_choose_0")
            .unwrap();
        assert!(!wgui::widget::is_node_visible(
            &layout.state.tree,
            *layout.state.nodes.get(b.get_rect()).unwrap()
        ));
    }
}
