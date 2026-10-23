use std::{
    env,
    io::{BufRead, BufReader, Write},
    marker::PhantomData,
    os::unix::net::UnixStream,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use chrono::{Local, Timelike};
use serde_json::{Value, json};
use wgui::{
    assets::AssetPathRef,
    components::{ComponentTrait, button::ComponentButton},
    event::CallbackDataCommon,
    i18n::Translation,
    layout::{Widget, WidgetID},
    parser::{Fetchable, ParseDocumentParams, ParserState},
    widget::label::WidgetLabel,
};

use crate::{
    frontend::Frontend,
    tab::{Tab, TabType},
};

#[derive(Clone, Debug, PartialEq)]
struct NightshiftState {
    connected: bool,
    sleep_phase: String,
    brightness: f32,
    sleep_target: f32,
    heart_rate: Option<u64>,
    osc_enabled: bool,
    vrchat_connected: bool,
    vrchat_status: String,
    display_name: Option<String>,
    invite_profile: String,
}

impl Default for NightshiftState {
    fn default() -> Self {
        Self {
            connected: false,
            sleep_phase: "awake".into(),
            brightness: 1.0,
            sleep_target: 0.06,
            heart_rate: None,
            osc_enabled: false,
            vrchat_connected: false,
            vrchat_status: "offline".into(),
            display_name: None,
            invite_profile: "disabled".into(),
        }
    }
}

#[derive(Clone, Copy)]
enum NightshiftCommand {
    EnterSleep,
    Wake,
    SetBrightness(f32),
    Stop,
}

struct NightshiftBridge {
    state: Arc<Mutex<NightshiftState>>,
    commands: mpsc::Sender<NightshiftCommand>,
    alive: Arc<AtomicBool>,
}

impl NightshiftBridge {
    fn spawn() -> Self {
        let state = Arc::new(Mutex::new(NightshiftState::default()));
        let alive = Arc::new(AtomicBool::new(true));
        let writer_slot = Arc::new(Mutex::new(None::<UnixStream>));
        let (commands, command_rx) = mpsc::channel();

        {
            let state = state.clone();
            let alive = alive.clone();
            let writer_slot = writer_slot.clone();
            thread::spawn(move || connection_loop(state, alive, writer_slot));
        }

        {
            let writer_slot = writer_slot.clone();
            thread::spawn(move || {
                while let Ok(command) = command_rx.recv() {
                    if matches!(command, NightshiftCommand::Stop) {
                        break;
                    }
                    let payload = match command {
                        NightshiftCommand::EnterSleep => json!({
                            "type": "command",
                            "command": { "type": "enter_sleep" }
                        }),
                        NightshiftCommand::Wake => json!({
                            "type": "command",
                            "command": { "type": "wake" }
                        }),
                        NightshiftCommand::SetBrightness(value) => json!({
                            "type": "command",
                            "command": { "type": "set_brightness", "value": value }
                        }),
                        NightshiftCommand::Stop => break,
                    };

                    let Ok(mut slot) = writer_slot.lock() else {
                        continue;
                    };
                    let Some(stream) = slot.as_mut() else {
                        continue;
                    };
                    let mut line = payload.to_string();
                    line.push('\n');
                    if stream.write_all(line.as_bytes()).is_err() {
                        *slot = None;
                    }
                }
            });
        }

        Self {
            state,
            commands,
            alive,
        }
    }

    fn snapshot(&self) -> NightshiftState {
        self.state.lock().map(|state| state.clone()).unwrap_or_default()
    }

    fn send(&self, command: NightshiftCommand) {
        let _ = self.commands.send(command);
    }
}

impl Drop for NightshiftBridge {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = self.commands.send(NightshiftCommand::Stop);
    }
}

fn socket_path() -> Option<PathBuf> {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|path| path.join("nightshift").join("nightshift.sock"))
}

fn connection_loop(
    state: Arc<Mutex<NightshiftState>>,
    alive: Arc<AtomicBool>,
    writer_slot: Arc<Mutex<Option<UnixStream>>>,
) {
    while alive.load(Ordering::Relaxed) {
        let Some(path) = socket_path() else {
            thread::sleep(Duration::from_secs(1));
            continue;
        };

        let stream = match UnixStream::connect(path) {
            Ok(stream) => stream,
            Err(_) => {
                set_connected(&state, false);
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
        let Ok(mut writer) = stream.try_clone() else {
            thread::sleep(Duration::from_secs(1));
            continue;
        };

        let hello = json!({
            "type": "hello",
            "protocol": 1,
            "client": "wayvr",
            "client_version": env!("CARGO_PKG_VERSION"),
        });
        let mut hello_line = hello.to_string();
        hello_line.push('\n');
        if writer.write_all(hello_line.as_bytes()).is_err() {
            thread::sleep(Duration::from_secs(1));
            continue;
        }

        if let Ok(mut slot) = writer_slot.lock() {
            *slot = Some(writer);
        }
        set_connected(&state, true);

        let mut reader = BufReader::new(stream);
        while alive.load(Ordering::Relaxed) {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if let Ok(message) = serde_json::from_str::<Value>(line.trim_end()) {
                        if let Ok(mut state) = state.lock() {
                            apply_message(&mut state, &message);
                        }
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => break,
            }
        }

        if let Ok(mut slot) = writer_slot.lock() {
            *slot = None;
        }
        set_connected(&state, false);
    }
}

fn set_connected(state: &Arc<Mutex<NightshiftState>>, connected: bool) {
    if let Ok(mut state) = state.lock() {
        state.connected = connected;
    }
}

fn apply_message(state: &mut NightshiftState, message: &Value) {
    match message.get("type").and_then(Value::as_str) {
        Some("full_state") => apply_full_state(state, &message["state"]),
        Some("event") => apply_event(state, &message["event"]),
        _ => {}
    }
}

fn apply_full_state(state: &mut NightshiftState, value: &Value) {
    apply_sleep(state, &value["sleep"]);
    apply_brightness(state, &value["brightness"]);
    apply_heart_rate(state, &value["heart_rate"]);
    apply_osc(state, &value["osc"]);
    apply_vrchat(state, &value["vrchat"]);
    apply_invites(state, &value["invites"]);
}

fn apply_event(state: &mut NightshiftState, event: &Value) {
    let section = &event["state"];
    match event.get("type").and_then(Value::as_str) {
        Some("sleep_changed") => apply_sleep(state, section),
        Some("brightness_changed") => apply_brightness(state, section),
        Some("heart_rate_changed") => apply_heart_rate(state, section),
        Some("osc_changed") => apply_osc(state, section),
        Some("vrchat_changed") => apply_vrchat(state, section),
        Some("invites_changed") => apply_invites(state, section),
        _ => {}
    }
}

fn apply_sleep(state: &mut NightshiftState, value: &Value) {
    if let Some(phase) = value.get("phase").and_then(Value::as_str) {
        state.sleep_phase = phase.to_string();
    }
}

fn apply_brightness(state: &mut NightshiftState, value: &Value) {
    if let Some(current) = value.get("current").and_then(Value::as_f64) {
        state.brightness = current as f32;
    }
    if let Some(target) = value.get("sleep_target").and_then(Value::as_f64) {
        state.sleep_target = target as f32;
    }
}

fn apply_heart_rate(state: &mut NightshiftState, value: &Value) {
    state.heart_rate = value.get("bpm").and_then(Value::as_u64);
}

fn apply_osc(state: &mut NightshiftState, value: &Value) {
    if let Some(enabled) = value.get("enabled").and_then(Value::as_bool) {
        state.osc_enabled = enabled;
    }
}

fn apply_vrchat(state: &mut NightshiftState, value: &Value) {
    if let Some(connected) = value.get("connected").and_then(Value::as_bool) {
        state.vrchat_connected = connected;
    }
    if let Some(status) = value.get("status").and_then(Value::as_str) {
        state.vrchat_status = status.to_string();
    }
    state.display_name = value
        .get("display_name")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
}

fn apply_invites(state: &mut NightshiftState, value: &Value) {
    if let Some(profile) = value.get("profile").and_then(Value::as_str) {
        state.invite_profile = profile.to_string();
    }
}

struct Widgets {
    greeting: Widget,
    connection: Widget,
    sleep_state: Widget,
    brightness: Widget,
    sleep_target: Widget,
    heart_rate: Widget,
    osc: Widget,
    vrchat: Widget,
    invites: Widget,
}

pub struct TabNightshift<T> {
    #[allow(dead_code)]
    state: ParserState,
    bridge: NightshiftBridge,
    widgets: Widgets,
    sleep_button: Rc<ComponentButton>,
    last_state: NightshiftState,
    marker: PhantomData<T>,
}

impl<T> Tab<T> for TabNightshift<T> {
    fn get_type(&self) -> TabType {
        TabType::Nightshift
    }

    fn update(&mut self, frontend: &mut Frontend<T>, _time_ms: u32, _user_data: &mut T) -> anyhow::Result<()> {
        let snapshot = self.bridge.snapshot();
        if snapshot == self.last_state {
            return Ok(());
        }
        self.refresh(frontend, &snapshot)?;
        self.last_state = snapshot;
        Ok(())
    }
}

impl<T> TabNightshift<T> {
    pub fn new(frontend: &mut Frontend<T>, parent_id: WidgetID) -> anyhow::Result<Self> {
        let state = wgui::parser::parse_from_assets(
            &ParseDocumentParams {
                globals: frontend.layout.state.globals.clone(),
                path: AssetPathRef::BuiltIn("gui/tab/nightshift.xml"),
                extra: Default::default(),
            },
            &mut frontend.layout,
            parent_id,
        )?;

        let bridge = NightshiftBridge::spawn();
        let sleep_button = state.fetch_component_as::<ComponentButton>("btn_sleep_toggle")?;
        let brightness_25 = state.fetch_component_as::<ComponentButton>("btn_brightness_25")?;
        let brightness_50 = state.fetch_component_as::<ComponentButton>("btn_brightness_50")?;
        let brightness_100 = state.fetch_component_as::<ComponentButton>("btn_brightness_100")?;

        {
            let commands = bridge.commands.clone();
            let shared_state = bridge.state.clone();
            sleep_button.on_click(Rc::new(move |_, _| {
                let sleeping = shared_state
                    .lock()
                    .map(|state| matches!(state.sleep_phase.as_str(), "sleeping" | "entering_sleep"))
                    .unwrap_or(false);
                let _ = commands.send(if sleeping {
                    NightshiftCommand::Wake
                } else {
                    NightshiftCommand::EnterSleep
                });
                Ok(())
            }));
        }

        register_brightness_button(&brightness_25, bridge.commands.clone(), 0.25);
        register_brightness_button(&brightness_50, bridge.commands.clone(), 0.50);
        register_brightness_button(&brightness_100, bridge.commands.clone(), 1.0);

        let widgets = Widgets {
            greeting: state.fetch_widget(&frontend.layout.state, "label_greeting")?.widget,
            connection: state.fetch_widget(&frontend.layout.state, "label_connection")?.widget,
            sleep_state: state.fetch_widget(&frontend.layout.state, "label_sleep_state")?.widget,
            brightness: state.fetch_widget(&frontend.layout.state, "label_brightness")?.widget,
            sleep_target: state.fetch_widget(&frontend.layout.state, "label_sleep_target")?.widget,
            heart_rate: state.fetch_widget(&frontend.layout.state, "label_heart_rate")?.widget,
            osc: state.fetch_widget(&frontend.layout.state, "label_osc")?.widget,
            vrchat: state.fetch_widget(&frontend.layout.state, "label_vrchat")?.widget,
            invites: state.fetch_widget(&frontend.layout.state, "label_invites")?.widget,
        };

        let mut tab = Self {
            state,
            bridge,
            widgets,
            sleep_button,
            last_state: NightshiftState::default(),
            marker: PhantomData,
        };
        let snapshot = tab.bridge.snapshot();
        tab.refresh(frontend, &snapshot)?;
        tab.last_state = snapshot;
        Ok(tab)
    }

    fn refresh(&mut self, frontend: &mut Frontend<T>, state: &NightshiftState) -> anyhow::Result<()> {
        let mut common = frontend.layout.common();

        set_label(&mut common, self.widgets.greeting, &greeting(state));
        set_label(
            &mut common,
            self.widgets.connection,
            if state.connected { "● Nightshift connected" } else { "○ Nightshift disconnected" },
        );

        let sleeping = matches!(state.sleep_phase.as_str(), "sleeping" | "entering_sleep");
        let sleep_text = match state.sleep_phase.as_str() {
            "entering_sleep" => "Entering sleep mode…",
            "sleeping" => "Sleep mode active",
            "waking" => "Waking up…",
            _ => "Awake",
        };
        set_label(&mut common, self.widgets.sleep_state, sleep_text);
        self.sleep_button.set_text(
            &mut common,
            Translation::from_raw_text(if sleeping { "Wake up" } else { "Enter sleep mode" }),
        );

        set_label(
            &mut common,
            self.widgets.brightness,
            &format!("Current brightness  {}%", (state.brightness * 100.0).round() as u32),
        );
        set_label(
            &mut common,
            self.widgets.sleep_target,
            &format!("Sleep target  {}%", (state.sleep_target * 100.0).round() as u32),
        );
        set_label(
            &mut common,
            self.widgets.heart_rate,
            &state
                .heart_rate
                .map(|bpm| format!("Heart rate  {bpm} BPM"))
                .unwrap_or_else(|| "Heart rate  —".into()),
        );
        set_label(
            &mut common,
            self.widgets.osc,
            if state.osc_enabled { "OSC  Running" } else { "OSC  Off" },
        );
        set_label(
            &mut common,
            self.widgets.vrchat,
            &format!(
                "VRChat  {}",
                if state.vrchat_connected {
                    pretty_status(&state.vrchat_status)
                } else {
                    "Disconnected".into()
                }
            ),
        );
        set_label(
            &mut common,
            self.widgets.invites,
            &format!("Invite rules  {}", pretty_status(&state.invite_profile)),
        );

        Ok(())
    }
}

fn register_brightness_button(
    button: &Rc<ComponentButton>,
    commands: mpsc::Sender<NightshiftCommand>,
    value: f32,
) {
    button.on_click(Rc::new(move |_, _| {
        let _ = commands.send(NightshiftCommand::SetBrightness(value));
        Ok(())
    }));
}

fn set_label(common: &mut CallbackDataCommon, widget: Widget, text: &str) {
    if let Some(mut label) = widget.get_as::<WidgetLabel>() {
        label.set_text(common, Translation::from_raw_text(text));
    }
}

fn greeting(state: &NightshiftState) -> String {
    let phrase = match Local::now().hour() {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    };

    match state.display_name.as_deref().filter(|_| state.vrchat_connected) {
        Some(name) => format!("{phrase}, {name}."),
        None => format!("{phrase}."),
    }
}

fn pretty_status(value: &str) -> String {
    value
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
