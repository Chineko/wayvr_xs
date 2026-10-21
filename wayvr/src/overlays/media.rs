//! Request-driven MPRIS controls. No bus calls or artwork downloads on the VR thread.
use crate::{gui::panel::GuiPanel, state::AppState};
use dbus::{
    arg::{PropMap, RefArg},
    blocking::{Connection, stdintf::org_freedesktop_dbus::Properties},
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    io::{Cursor, Read},
    os::unix::ffi::OsStringExt,
    rc::Rc,
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};
use wgui::{
    components::button::ComponentButton,
    event::EventListenerKind,
    parser::{Fetchable, ParserState},
    renderer_vk::text::custom_glyph::CustomGlyphData,
    widget::{EventResult, image::WidgetImage, label::WidgetLabel, sprite::WidgetSprite},
};
const IFACE: &str = "org.mpris.MediaPlayer2.Player";
const PATH: &str = "/org/mpris/MediaPlayer2";
const LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Default)]
struct Player {
    bus: String,
    owner: String,
    name: String,
    title: String,
    artist: String,
    art: String,
    position: Option<i64>,
    length: Option<i64>,
    observed: Option<Instant>,
    rate: f64,
    playing: bool,
    control: bool,
    play: bool,
    pause: bool,
    previous: bool,
    next: bool,
}
impl Player {
    fn allows(&self, verb: &str) -> bool {
        self.control
            && match verb {
                "Previous" => self.previous,
                "Next" => self.next,
                "PlayPause" => {
                    if self.playing {
                        self.pause
                    } else {
                        self.play
                    }
                }
                _ => false,
            }
    }
}
fn text(props: &PropMap, key: &str) -> String {
    props
        .get(key)
        .and_then(|v| v.0.as_str())
        .unwrap_or_default()
        .to_owned()
}
fn flag(props: &PropMap, key: &str) -> bool {
    props
        .get(key)
        .and_then(|v| v.0.as_i64())
        .is_some_and(|v| v != 0)
}
fn unvariant(mut value: &dyn RefArg) -> &dyn RefArg {
    for _ in 0..8 {
        if value.arg_type() != dbus::arg::ArgType::Variant {
            break;
        }
        let Some(next) = value.as_iter().and_then(|mut values| values.next()) else {
            break;
        };
        value = next;
    }
    value
}
// Spotify uses uint64 for mpris:length; other players use the specified int64.
// Read dynamically and reject overflow instead of losing otherwise valid timing.
fn microseconds(value: &dyn RefArg) -> Option<i64> {
    let value = unvariant(value);
    match value.arg_type() {
        dbus::arg::ArgType::UInt64
        | dbus::arg::ArgType::UInt32
        | dbus::arg::ArgType::UInt16
        | dbus::arg::ArgType::Byte => value.as_u64().and_then(|v| i64::try_from(v).ok()),
        dbus::arg::ArgType::Int64 | dbus::arg::ArgType::Int32 | dbus::arg::ArgType::Int16 => {
            value.as_i64()
        }
        _ => None,
    }
}
fn metadata_values(props: &PropMap) -> HashMap<String, &dyn RefArg> {
    let mut values = HashMap::new();
    if let Some(mut entries) = props.get("Metadata").and_then(|v| v.0.as_iter()) {
        while let (Some(key), Some(value)) = (entries.next(), entries.next()) {
            if let Some(key) = key.as_str() {
                values.insert(key.to_owned(), value);
            }
        }
    }
    values
}
fn metadata(props: &PropMap) -> (String, String, String) {
    let values = metadata_values(props);
    let get = |key: &str| {
        values
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned()
    };
    let artist = values
        .get("xesam:artist")
        .and_then(|v| unvariant(*v).as_iter())
        .map(|a| {
            a.filter_map(|v| v.as_str())
                .take(4)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    (get("xesam:title"), artist, get("mpris:artUrl"))
}
fn players(connection: &Connection) -> anyhow::Result<Vec<Player>> {
    let bus = connection.with_proxy(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        Duration::from_millis(100),
    );
    let (mut names,): (Vec<String>,) = bus.method_call("org.freedesktop.DBus", "ListNames", ())?;
    names.retain(|n| n.starts_with("org.mpris.MediaPlayer2."));
    names.sort();
    let mut players = Vec::new();
    for name in names.into_iter().take(8) {
        let Ok((owner,)): Result<(String,), _> =
            bus.method_call("org.freedesktop.DBus", "GetNameOwner", (name.clone(),))
        else {
            continue;
        };
        // Target the unique connection owner so a restarted player never inherits queued commands.
        let proxy = connection.with_proxy(owner.clone(), PATH, Duration::from_millis(100));
        let Ok(props) = proxy.get_all(IFACE) else {
            continue;
        };
        let (title, artist, art) = metadata(&props);
        // Position need not be included in GetAll (some players omit it).
        let position = proxy
            .get::<Box<dyn RefArg>>(IFACE, "Position")
            .ok()
            .and_then(|v| microseconds(v.as_ref()))
            .or_else(|| {
                props
                    .get("Position")
                    .and_then(|v| microseconds(v.0.as_ref()))
            })
            .filter(|v| *v >= 0);
        let observed = Some(Instant::now());
        let length = metadata_values(&props)
            .get("mpris:length")
            .and_then(|v| microseconds(*v))
            .filter(|v| *v > 0);
        let rate = props
            .get("Rate")
            .and_then(|v| v.0.as_f64())
            .filter(|v| v.is_finite() && *v != 0.0)
            .unwrap_or(1.0);
        let identity: String = proxy
            .get("org.mpris.MediaPlayer2", "Identity")
            .unwrap_or_else(|_| {
                name.trim_start_matches("org.mpris.MediaPlayer2.")
                    .to_owned()
            });
        players.push(Player {
            bus: name,
            owner,
            name: identity,
            title,
            artist,
            art,
            position,
            length,
            observed,
            rate,
            playing: text(&props, "PlaybackStatus") == "Playing",
            control: flag(&props, "CanControl"),
            play: flag(&props, "CanPlay"),
            pause: flag(&props, "CanPause"),
            previous: flag(&props, "CanGoPrevious"),
            next: flag(&props, "CanGoNext"),
        });
    }
    Ok(players)
}
fn select(players: &[Player], preferred: Option<&str>, current: Option<&str>) -> Option<usize> {
    preferred
        .and_then(|id| players.iter().position(|p| p.bus == id))
        .or_else(|| current.and_then(|id| players.iter().position(|p| p.bus == id && p.playing)))
        .or_else(|| players.iter().position(|p| p.playing))
        .or_else(|| current.and_then(|id| players.iter().position(|p| p.bus == id)))
        .or_else(|| (!players.is_empty()).then_some(0))
}
fn file_path(url: &str) -> Option<std::path::PathBuf> {
    let raw = url.strip_prefix("file://")?;
    let raw = raw.strip_prefix("localhost").unwrap_or(raw);
    if !raw.starts_with('/') {
        return None;
    }
    let mut out = Vec::new();
    let mut bytes = raw.as_bytes().iter();
    while let Some(&byte) = bytes.next() {
        if byte == b'%' {
            let a = (*bytes.next()? as char).to_digit(16)?;
            let b = (*bytes.next()? as char).to_digit(16)?;
            out.push((a * 16 + b) as u8);
        } else {
            out.push(byte);
        }
    }
    Some(std::ffi::OsString::from_vec(out).into())
}
fn artwork(url: &str, agent: &ureq::Agent) -> Option<Vec<u8>> {
    let data = if let Some(path) = file_path(url) {
        let mut data = Vec::new();
        std::fs::File::open(path)
            .ok()?
            .take(LIMIT + 1)
            .read_to_end(&mut data)
            .ok()?;
        data
    } else if url.starts_with("https://") {
        agent
            .get(url)
            .call()
            .ok()?
            .body_mut()
            .with_config()
            .limit(LIMIT)
            .read_to_vec()
            .ok()?
    } else {
        return None;
    };
    if data.len() as u64 > LIMIT {
        return None;
    }
    let mut reader = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.thumbnail(128, 128);
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, image::ImageFormat::Png).ok()?;
    Some(output.into_inner())
}
fn invoke(connection: &Connection, owner: &str, verb: &str) {
    let proxy = connection.with_proxy(owner, PATH, Duration::from_millis(150));
    if let Ok(props) = proxy.get_all(IFACE) {
        let player = Player {
            control: flag(&props, "CanControl"),
            play: flag(&props, "CanPlay"),
            pause: flag(&props, "CanPause"),
            previous: flag(&props, "CanGoPrevious"),
            next: flag(&props, "CanGoNext"),
            playing: text(&props, "PlaybackStatus") == "Playing",
            ..Default::default()
        };
        if player.allows(verb) {
            let _: Result<(), _> = proxy.method_call(IFACE, verb, ());
        }
    }
}
#[derive(Debug, Default, PartialEq)]
struct Progress {
    text: String,
    width: u16,
}
fn time_text(seconds: Option<u64>) -> String {
    seconds.map_or_else(
        || "—".into(),
        |s| {
            if s >= 3600 {
                format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
            } else {
                format!("{}:{:02}", s / 60, s % 60)
            }
        },
    )
}
fn progress(player: Option<&Player>, now: Instant) -> Progress {
    let length = player
        .and_then(|p| p.length)
        .filter(|v| *v > 0)
        .map(|v| v as u64);
    let position = player.and_then(|p| {
        p.position.filter(|v| *v >= 0).map(|pos| {
            let delta = if p.playing {
                p.observed
                    .map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f64())
                    * if p.rate.is_finite() && p.rate != 0.0 {
                        p.rate
                    } else {
                        1.0
                    }
                    * 1_000_000.0
            } else {
                0.0
            };
            (pos as f64 + delta).clamp(0.0, length.unwrap_or(i64::MAX as u64) as f64) as u64
        })
    });
    // Quantize to seconds, avoiding redraws on every wrist timer tick.
    let width = match (position, length) {
        (Some(pos), Some(total)) if pos >= total => 278,
        (Some(pos), Some(total)) => {
            ((pos / 1_000_000) as f64 * 1_000_000.0 / total as f64 * 278.0).floor() as u16
        }
        _ => 0,
    };
    Progress {
        text: format!(
            "{} / {}",
            time_text(position.map(|v| v / 1_000_000)),
            time_text(length.map(|v| v / 1_000_000))
        ),
        width,
    }
}
struct Sample {
    generation: u64,
    players: Vec<Player>,
    selected: Option<usize>,
    art: Option<Vec<u8>>,
    taken: Instant,
    error: Option<String>,
}
enum Request {
    Poll {
        generation: u64,
        preferred: Option<String>,
    },
    Action {
        owner: String,
        verb: &'static str,
    },
}
struct Worker {
    request: SyncSender<Request>,
    result: Receiver<Sample>,
}
impl Worker {
    fn new() -> anyhow::Result<Self> {
        let (request, commands) = mpsc::sync_channel(8);
        let (results, result) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("xs-media".into())
            .spawn(move || {
                let agent = ureq::Agent::config_builder()
                    .timeout_global(Some(Duration::from_secs(2)))
                    .max_redirects(2)
                    .build()
                    .new_agent();
                let mut connection = None;
                let mut current: Option<String> = None;
                let mut art_url = String::new();
                let mut art = None;
                while let Ok(request) = commands.recv() {
                    if connection.is_none() {
                        connection = Connection::new_session().ok();
                    }
                    match request {
                        Request::Action { owner, verb } => {
                            if let Some(c) = &connection {
                                invoke(c, &owner, verb);
                            }
                        }
                        Request::Poll {
                            generation,
                            preferred,
                        } => {
                            let taken = Instant::now();
                            let result = connection
                                .as_ref()
                                .ok_or_else(|| anyhow::anyhow!("Session media service unavailable"))
                                .and_then(players);
                            let error = result
                                .as_ref()
                                .err()
                                .map(|_| "Media service unavailable".to_owned());
                            if error.is_some() {
                                connection = None;
                            }
                            let players = result.unwrap_or_default();
                            let selected =
                                select(&players, preferred.as_deref(), current.as_deref());
                            current = selected.map(|i| players[i].bus.clone());
                            let url = selected.map_or("", |i| players[i].art.as_str());
                            if url != art_url {
                                url.clone_into(&mut art_url);
                                art = artwork(url, &agent);
                            }
                            if results
                                .send(Sample {
                                    generation,
                                    players,
                                    selected,
                                    art: art.clone(),
                                    taken,
                                    error,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            })?;
        Ok(Self { request, result })
    }
}
fn due_now() -> Instant {
    let now = Instant::now();
    now.checked_sub(Duration::from_secs(2)).unwrap_or(now)
}
struct Ui {
    worker: Worker,
    sample: Option<Sample>,
    preferred: Option<String>,
    generation: u64,
    pending: Option<u64>,
    last: Instant,
    painted_art: Option<Vec<u8>>,
    painted_key: Option<(String, String, bool, bool, bool, bool)>,
    painted_progress: Option<Progress>,
}
/// Independent of action drawers; hiding media keeps the clock and current drawer stationary.
pub(super) fn bind_toggle(parser: &ParserState) -> anyhow::Result<()> {
    let Ok(strip) = parser.get_widget_id("media_strip") else {
        return Ok(());
    };
    let button = parser.fetch_component_as::<ComponentButton>("btn_media")?;
    let weak = Rc::downgrade(&button);
    let shown = Cell::new(false);
    let icon = parser.get_widget_id("btn_media_icon")?;
    button.on_click(Rc::new(move |common, _| {
        let visible = !shown.get();
        shown.set(visible);
        common.alterables.set_widget_visible(strip, visible);
        if let Some(button) = weak.upgrade() {
            button.set_sticky_state(common, visible);
        }
        if let Some(mut sprite) = common.state.widgets.get_as::<WidgetSprite>(icon) {
            sprite.set_color(
                common,
                wgui::drawing::Color::from_hex(if visible { "#19232B" } else { "#E9EEF2" })
                    .unwrap()
                    .into(),
            );
        }
        Ok(())
    }));
    Ok(())
}
fn clipped(value: &str, max: usize) -> String {
    let value = value.replace(['\n', '\r', '\t'], " ");
    if value.chars().count() <= max {
        value
    } else {
        value.chars().take(max - 1).chain(['…']).collect()
    }
}
pub fn attach<S: 'static>(panel: &mut GuiPanel<S>) -> anyhow::Result<()> {
    let Ok(strip) = panel.parser_state.get_widget_id("media_strip") else {
        return Ok(());
    };
    let state = Rc::new(RefCell::new(Ui {
        worker: Worker::new()?,
        sample: None,
        preferred: None,
        generation: 0,
        pending: None,
        last: due_now(),
        painted_art: None,
        painted_key: None,
        painted_progress: None,
    }));
    for (id, verb) in [
        ("media_previous", "Previous"),
        ("media_play", "PlayPause"),
        ("media_next", "Next"),
    ] {
        let state = state.clone();
        panel
            .parser_state
            .fetch_component_as::<ComponentButton>(id)?
            .on_click(Rc::new(move |_, _| {
                let mut state = state.borrow_mut();
                if let Some(player) = state
                    .sample
                    .as_ref()
                    .filter(|s| s.taken.elapsed() < Duration::from_secs(5))
                    .and_then(|s| s.selected.map(|i| &s.players[i]))
                    && player.allows(verb)
                {
                    let _ = state.worker.request.try_send(Request::Action {
                        owner: player.owner.clone(),
                        verb,
                    });
                    state.last = due_now();
                }
                Ok(())
            }));
    }
    let cycle = state.clone();
    panel
        .parser_state
        .fetch_component_as::<ComponentButton>("media_player")?
        .on_click(Rc::new(move |_, _| {
            let mut state = cycle.borrow_mut();
            if let Some(sample) = &state.sample
                && sample.players.len() > 1
            {
                state.preferred = Some(
                    sample.players[(sample.selected.unwrap_or(0) + 1) % sample.players.len()]
                        .bus
                        .clone(),
                );
                state.generation += 1;
                state.sample = None;
                state.last = due_now();
            }
            Ok(())
        }));
    let ids = [
        "media_title",
        "media_artist",
        "media_art",
        "media_art_fallback",
        "media_play_icon",
        "media_pause_icon",
        "media_previous_icon",
        "media_next_icon",
        "media_progress_fill",
        "media_time",
    ]
    .map(|id| panel.parser_state.get_widget_id(id))
    .into_iter()
    .collect::<anyhow::Result<Vec<_>>>()?;
    let globals = panel.layout.state.globals.clone();
    panel.layout.add_event_listener::<AppState, S>(
        panel.layout.tree_root_widget,
        EventListenerKind::InternalStateChange,
        Box::new(move |common, _, _, _| {
            let visible = common
                .state
                .nodes
                .get(strip)
                .is_some_and(|node| wgui::widget::is_node_visible(&common.state.tree, *node));
            let mut state = state.borrow_mut();
            while let Ok(sample) = state.worker.result.try_recv() {
                if state.pending == Some(sample.generation) {
                    state.pending = None;
                }
                if sample.generation == state.generation {
                    state.sample = Some(sample);
                }
            }
            if !visible {
                return Ok(EventResult::Pass);
            }
            if state.pending.is_none() && state.last.elapsed() >= Duration::from_secs(1) {
                let generation = state.generation;
                if state
                    .worker
                    .request
                    .try_send(Request::Poll {
                        generation,
                        preferred: state.preferred.clone(),
                    })
                    .is_ok()
                {
                    state.pending = Some(generation);
                    state.last = Instant::now();
                }
            }
            let sample = state
                .sample
                .as_ref()
                .filter(|s| s.taken.elapsed() < Duration::from_secs(5));
            let player = sample.and_then(|s| s.selected.map(|i| &s.players[i]));
            let title = player.map_or("No media player", |p| {
                if p.title.is_empty() {
                    &p.name
                } else {
                    &p.title
                }
            });
            let artist = player.map_or_else(
                || {
                    sample
                        .and_then(|s| s.error.as_deref())
                        .unwrap_or("Start Spotify or another player")
                        .to_owned()
                },
                |p| {
                    if p.artist.is_empty() {
                        p.name.clone()
                    } else {
                        format!("{} · {}", p.artist, p.name)
                    }
                },
            );
            let playing = player.is_some_and(|p| p.playing);
            let key = (
                clipped(title, 27),
                clipped(&artist, 22),
                playing,
                player.is_some_and(|p| p.allows("PlayPause")),
                player.is_some_and(|p| p.allows("Previous")),
                player.is_some_and(|p| p.allows("Next")),
            );
            let art = sample.and_then(|s| s.art.as_ref());
            let progress = progress(player, Instant::now());
            if state.painted_key.as_ref() == Some(&key)
                && art == state.painted_art.as_ref()
                && state.painted_progress.as_ref() == Some(&progress)
            {
                return Ok(EventResult::Pass);
            }
            for (id, text) in [(ids[0], &key.0), (ids[1], &key.1)] {
                if let Some(mut label) = common.state.widgets.get_as::<WidgetLabel>(id) {
                    label.set_text(common, wgui::i18n::Translation::from_raw_text(text));
                }
            }
            common.alterables.set_widget_visible(ids[4], !playing);
            common.alterables.set_widget_visible(ids[5], playing);
            for (id, verb) in [
                (ids[4], "PlayPause"),
                (ids[5], "PlayPause"),
                (ids[6], "Previous"),
                (ids[7], "Next"),
            ] {
                if let Some(mut sprite) = common.state.widgets.get_as::<WidgetSprite>(id) {
                    sprite.set_color(
                        common,
                        wgui::drawing::Color::from_hex(if player.is_some_and(|p| p.allows(verb)) {
                            "#E9EEF2"
                        } else {
                            "#65717B"
                        })
                        .unwrap()
                        .into(),
                    );
                }
            }
            if art != state.painted_art.as_ref() {
                let content = art.and_then(|bytes| {
                    CustomGlyphData::from_bytes_raster(&globals, "xs-media-thumbnail", bytes).ok()
                });
                common
                    .alterables
                    .set_widget_visible(ids[3], content.is_none());
                if let Some(mut image) = common.state.widgets.get_as::<WidgetImage>(ids[2]) {
                    image.set_content(common.alterables, content);
                }
                state.painted_art = art.cloned();
            }
            if state.painted_progress.as_ref() != Some(&progress) {
                if let Some(mut label) = common.state.widgets.get_as::<WidgetLabel>(ids[9]) {
                    label.set_text(
                        common,
                        wgui::i18n::Translation::from_raw_text(&progress.text),
                    );
                }
                common.alterables.set_style(
                    ids[8],
                    wgui::event::StyleSetRequest::Width(wgui::taffy::prelude::length(f32::from(
                        progress.width,
                    ))),
                );
                state.painted_progress = Some(progress);
            }
            state.painted_key = Some(key);
            Ok(EventResult::Pass)
        }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbus::arg::Variant;
    fn props(playing: bool) -> PropMap {
        let meta: PropMap = HashMap::from([
            (
                "mpris:length".into(),
                Variant(Box::new(120_000_000_u64) as Box<dyn RefArg>),
            ),
            (
                "xesam:title".into(),
                Variant(Box::new("Track ✓".to_owned()) as Box<dyn RefArg>),
            ),
            (
                "xesam:artist".into(),
                Variant(Box::new(vec!["First".to_owned(), "Second".to_owned()]) as Box<dyn RefArg>),
            ),
            (
                "mpris:artUrl".into(),
                Variant(Box::new("file:///tmp/cover.png".to_owned()) as Box<dyn RefArg>),
            ),
        ]);
        HashMap::from([
            (
                "Metadata".into(),
                Variant(Box::new(meta) as Box<dyn RefArg>),
            ),
            (
                "PlaybackStatus".into(),
                Variant(
                    Box::new(if playing { "Playing" } else { "Paused" }.to_owned())
                        as Box<dyn RefArg>,
                ),
            ),
            (
                "CanControl".into(),
                Variant(Box::new(true) as Box<dyn RefArg>),
            ),
            ("CanPlay".into(), Variant(Box::new(true) as Box<dyn RefArg>)),
            (
                "CanPause".into(),
                Variant(Box::new(true) as Box<dyn RefArg>),
            ),
            (
                "CanGoNext".into(),
                Variant(Box::new(true) as Box<dyn RefArg>),
            ),
            (
                "CanGoPrevious".into(),
                Variant(Box::new(false) as Box<dyn RefArg>),
            ),
        ])
    }
    #[test]
    fn metadata_survives_real_dbus_variant_encoding() {
        let message = dbus::Message::new_signal(PATH, IFACE, "Fixture")
            .unwrap()
            .append1(props(false));
        let decoded: PropMap = message.read1().unwrap();
        assert_eq!(
            metadata(&decoded),
            (
                "Track ✓".into(),
                "First, Second".into(),
                "file:///tmp/cover.png".into()
            )
        );
        assert!(flag(&decoded, "CanPlay"));
        assert!(!flag(&decoded, "CanGoPrevious"));
        assert_eq!(
            metadata(&PropMap::new()),
            (String::new(), String::new(), String::new())
        );
    }
    #[test]
    fn spotify_unsigned_and_standard_signed_timing_survive_dbus_round_trip() {
        for (length, position) in [
            (
                Box::new(164_583_000_u64) as Box<dyn RefArg>,
                Box::new(73_282_000_i64) as Box<dyn RefArg>,
            ),
            (
                Box::new(164_583_000_i64) as Box<dyn RefArg>,
                Box::new(73_282_000_u64) as Box<dyn RefArg>,
            ),
        ] {
            let metadata: PropMap = HashMap::from([("mpris:length".into(), Variant(length))]);
            let properties: PropMap = HashMap::from([
                (
                    "Metadata".into(),
                    Variant(Box::new(metadata) as Box<dyn RefArg>),
                ),
                ("Position".into(), Variant(position)),
            ]);
            let message = dbus::Message::new_signal(PATH, IFACE, "Fixture")
                .unwrap()
                .append1(properties);
            let decoded: PropMap = message.read1().unwrap();
            let p = Player {
                length: metadata_values(&decoded)
                    .get("mpris:length")
                    .and_then(|v| microseconds(*v)),
                position: decoded
                    .get("Position")
                    .and_then(|v| microseconds(v.0.as_ref())),
                ..Default::default()
            };
            let progress = progress(Some(&p), Instant::now());
            assert_eq!(progress.text, "1:13 / 2:44");
            assert!(progress.width > 0 && progress.width < 278);
        }
        assert_eq!(microseconds(&u64::MAX), None);
        assert_eq!(microseconds(&"164583000".to_owned()), None);
        assert_eq!(microseconds(&f64::NAN), None);
    }
    #[test]
    fn selection_prefers_playing_but_respects_explicit_choice_and_closed_players() {
        let mut ps = vec![
            Player {
                bus: "browser".into(),
                ..Default::default()
            },
            Player {
                bus: "spotify".into(),
                playing: true,
                ..Default::default()
            },
        ];
        assert_eq!(select(&ps, None, None), Some(1));
        assert_eq!(select(&ps, Some("browser"), None), Some(0));
        assert_eq!(select(&ps, Some("closed"), Some("spotify")), Some(1));
        ps[0].playing = true;
        assert_eq!(select(&ps, None, Some("spotify")), Some(1));
        assert_eq!(select(&[], None, None), None);
        assert_eq!(clipped("a\nb\tc", 20), "a b c");
        assert_eq!(clipped("é漢字text", 4), "é漢字…");
    }
    #[test]
    fn artwork_is_bounded_and_file_urls_preserve_spaces() {
        assert_eq!(
            file_path("file://localhost/tmp/album%20art.png").unwrap(),
            std::path::PathBuf::from("/tmp/album art.png")
        );
        assert!(file_path("file://remotehost/tmp/cover").is_none());
        assert!(file_path("file:///tmp/bad%ZZ").is_none());
        let dir = std::env::temp_dir().join(format!("xs-media-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let agent = ureq::Agent::new_with_defaults();
        let file = dir.join("cover.png");
        image::RgbaImage::new(256, 256).save(&file).unwrap();
        let bytes = artwork(&format!("file://{}", file.display()), &agent).unwrap();
        let image = image::load_from_memory(&bytes).unwrap();
        assert_eq!((image.width(), image.height()), (128, 128));
        image::RgbaImage::new(3000, 1).save(&file).unwrap();
        assert!(artwork(&format!("file://{}", file.display()), &agent).is_none());
        assert!(artwork("data:image/png;base64,AAAA", &agent).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn progress_handles_pause_play_seek_rate_and_missing_duration() {
        let now = Instant::now();
        let mut p = Player {
            position: Some(30_000_000),
            length: Some(120_000_000),
            observed: Some(now),
            rate: 1.0,
            ..Default::default()
        };
        assert_eq!(
            progress(Some(&p), now + Duration::from_secs(5)).text,
            "0:30 / 2:00"
        );
        p.playing = true;
        assert_eq!(
            progress(Some(&p), now + Duration::from_secs(5)).text,
            "0:35 / 2:00"
        );
        assert_eq!(
            progress(Some(&p), now + Duration::from_millis(5100)),
            progress(Some(&p), now + Duration::from_millis(5900))
        );
        p.rate = 2.0;
        assert_eq!(
            progress(Some(&p), now + Duration::from_secs(5)).text,
            "0:40 / 2:00"
        );
        p.position = Some(5_000_000);
        p.observed = Some(now);
        p.rate = 1.0;
        assert_eq!(progress(Some(&p), now).text, "0:05 / 2:00");
        assert_eq!(
            progress(Some(&p), now + Duration::from_secs(200)).width,
            278
        );
        p.length = None;
        let live = progress(Some(&p), now);
        assert_eq!((live.text.as_str(), live.width), ("0:05 / —", 0));
        p.position = None;
        p.length = Some(120_000_000);
        assert_eq!(progress(Some(&p), now).text, "— / 2:00");
        p.position = Some(-5);
        p.length = Some(-1);
        assert_eq!(progress(Some(&p), now).text, "— / —");
        assert_eq!(progress(None, now).width, 0);
        assert_eq!(time_text(Some(3661)), "1:01:01");
    }
    #[test]
    fn private_bus_controls_target_unique_owner_and_honor_capabilities() {
        use dbus::{channel::MatchingReceiver, message::MatchRule};
        use std::{
            io::{BufRead, BufReader},
            process::{Command, Stdio},
            sync::{
                Arc, Mutex,
                atomic::{AtomicBool, Ordering},
            },
        };
        struct Bus(std::process::Child);
        impl Drop for Bus {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut bus = Bus(Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap());
        let mut address = String::new();
        BufReader::new(bus.0.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let address = address.trim();
        let service = Connection::new_address(address).unwrap();
        service
            .request_name("org.mpris.MediaPlayer2.xs_fixture", false, true, false)
            .unwrap();
        let owner = service.unique_name().to_string();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let received = calls.clone();
        let thread = std::thread::spawn(move || {
            let mut playing = false;
            service.start_receive(
                MatchRule::new_method_call(),
                Box::new(move |msg, conn| {
                    let member = msg.member().unwrap().to_string();
                    let reply = match member.as_str() {
                        "GetAll" => msg.method_return().append1(props(playing)),
                        "Get" => {
                            let (_, property): (String, String) = msg.read2().unwrap();
                            if property == "Position" {
                                msg.method_return().append1(Variant(30_000_000_i64))
                            } else {
                                msg.method_return().append1(Variant("Fixture".to_owned()))
                            }
                        }
                        "PlayPause" | "Next" | "Previous" => {
                            received.lock().unwrap().push(member.clone());
                            if member == "PlayPause" {
                                playing = !playing;
                            }
                            msg.method_return()
                        }
                        _ => msg.method_return(),
                    };
                    let _ = conn.channel().send(reply);
                    true
                }),
            );
            while !stop.load(Ordering::Relaxed) {
                service.process(Duration::from_millis(10)).unwrap();
            }
        });
        let client = Connection::new_address(address).unwrap();
        let ps = players(&client).unwrap();
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].owner, owner);
        assert_eq!(ps[0].artist, "First, Second");
        assert!(!ps[0].playing);
        assert_eq!(ps[0].position, Some(30_000_000));
        assert_eq!(ps[0].length, Some(120_000_000));
        assert_eq!(progress(Some(&ps[0]), Instant::now()).text, "0:30 / 2:00");
        invoke(&client, &owner, "PlayPause");
        invoke(&client, &owner, "Previous");
        invoke(&client, &owner, "Next");
        invoke(&client, &owner, "Quit");
        assert!(players(&client).unwrap()[0].playing);
        assert_eq!(*calls.lock().unwrap(), vec!["PlayPause", "Next"]);
        stopped.store(true, Ordering::Relaxed);
        thread.join().unwrap();
        // Queued actions to the departed unique owner cannot reach a replacement.
        invoke(&client, &owner, "Next");
        assert_eq!(calls.lock().unwrap().len(), 2);
    }
}
