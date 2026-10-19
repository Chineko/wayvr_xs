use std::{
    collections::VecDeque,
    fs,
    os::fd::AsRawFd,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use dbus::{
    Message, MessageType,
    arg::PropMap,
    blocking::Connection as Bus,
    channel::{MatchingReceiver, Sender},
    message::MatchRule,
};
use serde::{Deserialize, Serialize};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};

#[allow(
    dead_code,
    unused_imports,
    non_camel_case_types,
    non_upper_case_globals,
    non_snake_case,
    clippy::all
)]
mod protocol {
    use wayland_client;
    use wayland_client::protocol::wl_output;
    pub mod __interfaces {
        use wayland_client::backend as wayland_backend;
        use wayland_client::protocol::__interfaces::{WL_OUTPUT_INTERFACE, wl_output_interface};
        wayland_scanner::generate_interfaces!("protocols/zkde-screencast-unstable-v1.xml");
    }
    use self::__interfaces::{
        ZKDE_SCREENCAST_STREAM_UNSTABLE_V1_INTERFACE, ZKDE_SCREENCAST_UNSTABLE_V1_INTERFACE,
    };
    wayland_scanner::generate_client_code!("protocols/zkde-screencast-unstable-v1.xml");
}
use protocol::{
    zkde_screencast_stream_unstable_v1::{self, ZkdeScreencastStreamUnstableV1},
    zkde_screencast_unstable_v1::{self, ZkdeScreencastUnstableV1},
};

#[derive(Clone, Serialize)]
pub struct Request {
    pub seq: u64,
    pub uv: [f32; 2],
    pub extent: [u32; 2],
    pub activate: bool,
}

#[derive(Debug, Deserialize)]
pub struct Reply {
    pub seq: u64,
    pub alive: bool,
    pub active: bool,
    pub valid: bool,
    pub x: f64,
    pub y: f64,
}

pub enum Event {
    Ready(u32),
    Reply(Reply),
    Failed(String),
}

pub struct Worker {
    pub requests: mpsc::SyncSender<Request>,
    pub events: mpsc::Receiver<Event>,
    stop: Arc<AtomicBool>,
    enabled: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
    finished: mpsc::Receiver<()>,
}
impl Worker {
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn start() -> Self {
        Self::start_inner(None)
    }

    #[cfg(test)]
    fn start_for_window(uuid: String) -> Self {
        Self::start_inner(Some(uuid))
    }

    fn start_inner(uuid: Option<String>) -> Self {
        let (requests, rx) = mpsc::sync_channel(16);
        let (tx, events) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let enabled = Arc::new(AtomicBool::new(true));
        let thread_enabled = enabled.clone();
        let thread_stop = stop.clone();
        let (finished_tx, finished) = mpsc::channel();
        let join = std::thread::spawn(move || {
            if let Err(error) = run(&rx, &tx, &thread_stop, &thread_enabled, uuid) {
                let _ = tx.send(Event::Failed(format!("{error:#}")));
            }
            let _ = finished_tx.send(());
        });
        Self {
            requests,
            events,
            stop,
            enabled,
            join: Some(join),
            finished,
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if self
            .finished
            .recv_timeout(Duration::from_millis(250))
            .is_ok()
            && let Some(join) = self.join.take()
        {
            let _ = join.join();
        }
    }
}

struct WaylandState {
    node: Option<u32>,
    failed: Option<String>,
}
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for WaylandState {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<ZkdeScreencastUnstableV1, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZkdeScreencastUnstableV1,
        _: zkde_screencast_unstable_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<ZkdeScreencastStreamUnstableV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _: &ZkdeScreencastStreamUnstableV1,
        event: zkde_screencast_stream_unstable_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zkde_screencast_stream_unstable_v1::Event;
        match event {
            Event::Created { node } => state.node = Some(node),
            Event::Closed => state.failed = Some("The selected app window closed".into()),
            Event::Failed { error } => state.failed = Some(error),
            _ => {}
        }
    }
}

struct ScriptGuard {
    bus: Bus,
    plugin: String,
    path: std::path::PathBuf,
}
impl Drop for ScriptGuard {
    fn drop(&mut self) {
        let _: Result<(bool,), _> = self
            .bus
            .with_proxy("org.kde.KWin", "/Scripting", Duration::from_millis(500))
            .method_call("org.kde.kwin.Scripting", "unloadScript", (&self.plugin,));
        let _ = fs::remove_file(&self.path);
    }
}

fn select_window(bus: &Bus, stop: &AtomicBool) -> anyhow::Result<String> {
    // The native desktop crosshair identifies the exact window; Escape cancels.
    let msg = Message::new_method_call("org.kde.KWin", "/KWin", "org.kde.KWin", "queryWindowInfo")
        .unwrap();
    let serial = bus
        .send(msg)
        .map_err(|()| anyhow::anyhow!("Could not contact KWin"))?;
    let selected = Arc::new(Mutex::new(None));
    let mut rules = Vec::new();
    for message_type in [MessageType::MethodReturn, MessageType::Error] {
        let selected = selected.clone();
        let mut rule = MatchRule::new();
        rule.msg_type = Some(message_type);
        rules.push(bus.start_receive(
            rule,
            Box::new(move |message, _| {
                if message.get_reply_serial() != Some(serial) {
                    return true;
                }
                let result = if message.msg_type() == MessageType::Error {
                    Err("Window selection cancelled or unavailable".to_owned())
                } else {
                    message
                        .read1::<PropMap>()
                        .ok()
                        .and_then(|properties| {
                            properties
                                .get("uuid")
                                .and_then(|v| v.0.as_str())
                                .map(str::to_owned)
                        })
                        .ok_or_else(|| "KWin did not return a window ID".to_owned())
                };
                *selected.lock().unwrap() = Some(result);
                true
            }),
        ));
    }
    let started = Instant::now();
    let result = loop {
        if stop.load(Ordering::Relaxed) {
            break Err(anyhow::anyhow!(
                "Window selection stopped; press Escape to close the desktop picker"
            ));
        }
        if started.elapsed() > Duration::from_secs(120) {
            break Err(anyhow::anyhow!(
                "Window selection timed out; press Escape to close the desktop picker"
            ));
        }
        bus.process(Duration::from_millis(10))?;
        let selection = { selected.lock().unwrap().take() };
        if let Some(result) = selection {
            break result.map_err(anyhow::Error::msg);
        }
    };
    for rule in rules {
        bus.stop_receive(rule);
    }
    result
}

fn run(
    rx: &mpsc::Receiver<Request>,
    tx: &mpsc::Sender<Event>,
    stop: &AtomicBool,
    enabled: &AtomicBool,
    selected_uuid: Option<String>,
) -> anyhow::Result<()> {
    let bus = Bus::new_session()?;
    let uuid = match selected_uuid {
        Some(uuid) => uuid,
        None => select_window(&bus, stop)?,
    };
    uuid::Uuid::parse_str(uuid.trim_matches(['{', '}'])).context("Invalid KWin window ID")?;
    let wl = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<WaylandState>(&wl)?;
    let manager: ZkdeScreencastUnstableV1 = globals
        .bind(&queue.handle(), 1..=5, ())
        .context("KDE window capture is unavailable; use the view-only portal option")?;
    let stream = manager.stream_window(uuid.clone(), 2, &queue.handle(), ());
    wl.flush()?;
    let mut state = WaylandState {
        node: None,
        failed: None,
    };

    let plugin = format!("wayvr-xs-{}", uuid::Uuid::new_v4());
    let directories = xdg::BaseDirectories::with_prefix("wayvr");
    let runtime = directories.get_runtime_directory()?;
    let path = runtime.join(format!("{plugin}.js"));
    let script = include_str!("bridge.js")
        .replace("__UUID__", &serde_json::to_string(&uuid)?)
        .replace(
            "__BUS__",
            &serde_json::to_string(&bus.unique_name().to_string())?,
        );
    // The runtime directory is private to this user; scripts are session-scoped.
    fs::write(&path, script)?;
    let cleanup_bus = Bus::new_session()?;
    let guard = ScriptGuard {
        bus: cleanup_bus,
        plugin,
        path,
    };
    let scripting = bus.with_proxy("org.kde.KWin", "/Scripting", Duration::from_secs(2));
    let (script_id,): (i32,) = scripting.method_call(
        "org.kde.kwin.Scripting",
        "loadScript",
        (guard.path.to_string_lossy().as_ref(), &guard.plugin),
    )?;
    if script_id < 0 {
        bail!("KWin could not load the temporary input bridge");
    }
    let (owner,): (String,) = bus
        .with_proxy(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            Duration::from_secs(1),
        )
        .method_call("org.freedesktop.DBus", "GetNameOwner", ("org.kde.KWin",))?;
    let pending = Arc::new(Mutex::new(None::<Message>));
    let pending_callback = pending.clone();
    let tx_callback = tx.clone();
    let mut rule = MatchRule::new_method_call();
    rule.path = Some("/org/wayvr/XS".into());
    rule.interface = Some("org.wayvr.XS.WindowInput".into());
    rule.member = Some("Next".into());
    let mut last_state = None;
    bus.start_receive(
        rule,
        Box::new(move |message, _| {
            if message
                .sender()
                .is_none_or(|sender| sender.as_ref() != owner)
            {
                return true;
            }
            if let Ok((json,)) = message.read1::<String>().map(|v| (v,))
                && let Ok(reply) = serde_json::from_str::<Reply>(&json)
            {
                let state = (reply.alive, reply.active);
                if reply.seq != 0 || last_state != Some(state) {
                    last_state = Some(state);
                    let _ = tx_callback.send(Event::Reply(reply));
                }
            }
            *pending_callback.lock().unwrap() = Some(message);
            true
        }),
    );
    // Send asynchronously: KWin calls back into this connection from the script.
    let run = Message::new_method_call(
        "org.kde.KWin",
        format!("/Scripting/Script{script_id}"),
        "org.kde.kwin.Script",
        "run",
    )
    .unwrap();
    bus.send(run)
        .map_err(|()| anyhow::anyhow!("Could not run KWin input bridge"))?;
    let mut announced = false;
    let mut commands = VecDeque::new();
    let mut heartbeat = Instant::now();
    let mut last_bridge_call = Instant::now();
    let start = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        bus.process(Duration::from_millis(5))?;
        queue.dispatch_pending(&mut state)?;
        wl.flush()?;
        if let Some(read) = wl.prepare_read() {
            let mut fd = libc::pollfd {
                fd: read.connection_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&raw mut fd, 1, 0) } > 0 {
                read.read()?;
            }
        }
        if let Some(error) = state.failed.take() {
            bail!("{error}");
        }
        if !announced && let Some(node) = state.node {
            tx.send(Event::Ready(node))?;
            announced = true;
        }
        for command in rx.try_iter() {
            if enabled.load(Ordering::Relaxed) {
                commands.push_back(command);
            }
        }
        if !enabled.load(Ordering::Relaxed) {
            commands.clear();
        }
        let next_call = { pending.lock().unwrap().take() };
        if let Some(message) = next_call {
            last_bridge_call = Instant::now();
            if let Some(command) = commands.pop_front() {
                let reply = message
                    .method_return()
                    .append1(serde_json::to_string(&command)?);
                bus.send(reply)
                    .map_err(|()| anyhow::anyhow!("KWin bridge disconnected"))?;
                heartbeat = Instant::now();
            } else if heartbeat.elapsed() > Duration::from_millis(50) {
                bus.send(message.method_return().append1("{\"seq\":0}"))
                    .map_err(|()| anyhow::anyhow!("KWin bridge disconnected"))?;
                heartbeat = Instant::now();
            } else {
                *pending.lock().unwrap() = Some(message);
            }
        }
        if last_bridge_call.elapsed() > Duration::from_secs(2)
            || (!announced && start.elapsed() > Duration::from_secs(10))
        {
            bail!("KDE capture/input bridge did not respond");
        }
    }
    let final_call = { pending.lock().unwrap().take() };
    if let Some(message) = final_call {
        let _ = bus.send(message.method_return().append1("{\"stop\":true}"));
    }
    stream.close();
    manager.destroy();
    let _ = wl.flush();
    drop(guard);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Requires a disposable foreground KDE test window; never injects input.
    #[test]
    #[ignore = "requires explicit WAYVR_XS_TEST_WINDOW_UUID and EXTENT for a disposable KDE window"]
    fn native_kde_capture_and_validated_mapping() {
        let uuid = std::env::var("WAYVR_XS_TEST_WINDOW_UUID").unwrap();
        let extent: [u32; 2] =
            serde_json::from_str(&std::env::var("WAYVR_XS_TEST_WINDOW_EXTENT").unwrap()).unwrap();
        let worker = Worker::start_for_window(uuid);
        let start = Instant::now();
        let mut ready = false;
        let mut valid = false;
        let mut rejected_stale = false;
        let mut rejected_outside = false;
        while start.elapsed() < Duration::from_secs(8) {
            let _ = worker.requests.try_send(Request {
                seq: 1,
                uv: [0.5, 0.5],
                extent,
                activate: false,
            });
            let _ = worker.requests.try_send(Request {
                seq: 2,
                uv: [0.5, 0.5],
                extent: [extent[0] + 30, extent[1]],
                activate: false,
            });
            let _ = worker.requests.try_send(Request {
                seq: 3,
                uv: [1.1, 0.5],
                extent,
                activate: false,
            });
            if let Ok(event) = worker.events.recv_timeout(Duration::from_millis(100)) {
                match event {
                    Event::Ready(node) => {
                        assert!(node > 0);
                        ready = true;
                    }
                    Event::Reply(reply) if reply.seq == 1 => {
                        if reply.valid {
                            assert!(reply.active && reply.alive);
                            assert!(reply.x.is_finite() && reply.y.is_finite());
                            valid = true;
                        }
                    }
                    Event::Reply(reply) if reply.seq == 2 => {
                        assert!(!reply.valid);
                        rejected_stale = true;
                    }
                    Event::Reply(reply) if reply.seq == 3 => {
                        assert!(!reply.valid);
                        rejected_outside = true;
                    }
                    Event::Failed(error) => panic!("{error}"),
                    _ => {}
                }
            }
            if ready && valid && rejected_stale && rejected_outside {
                break;
            }
        }
        assert!(ready, "KWin did not create the capture stream");
        assert!(rejected_stale && rejected_outside);
        assert!(
            valid,
            "KWin did not validate the test window's input mapping"
        );
    }
}
