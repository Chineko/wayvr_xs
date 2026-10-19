//! Request-driven PC telemetry. The worker sleeps when the panel is not rendered.
use crate::{
    gui::{
        panel::{GuiPanel, NewGuiPanelParams},
        timer::GuiTimer,
    },
    state::AppState,
    windowing::{
        Z_ORDER_WATCH,
        window::{OverlayCategory, OverlayWindowConfig},
    },
};
use glam::{Affine3A, Vec3};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};
use wgui::{
    event::EventListenerKind,
    parser::Fetchable,
    widget::{EventResult, label::WidgetLabel},
};
use wlx_common::{
    common::LeftRight,
    windowing::{OverlayWindowState, Positioning},
};

const NAME: &str = "XSPerformance";
struct Sample {
    text: String,
    taken: Instant,
}
struct Sampler {
    request: SyncSender<()>,
    result: Receiver<Sample>,
    last: Instant,
    pending: bool,
}
impl Sampler {
    fn new() -> anyhow::Result<Self> {
        let (request, commands) = mpsc::sync_channel(1);
        let (results, result) = mpsc::channel();
        std::thread::Builder::new()
            .name("xs-performance".into())
            .spawn(move || {
                let mut system = sysinfo::System::new();
                let mut previous: Option<Instant> = None;
                while commands.recv().is_ok() {
                    system.refresh_cpu_usage();
                    system.refresh_memory();
                    let cpu = if previous.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) {
                        format!("{:.0}%", system.global_cpu_usage())
                    } else {
                        "warming up".into()
                    };
                    previous = Some(Instant::now());
                    let text = format!(
                        "CPU: {cpu}    RAM: {:.1} / {:.1} GiB\n{}\n{}",
                        gib(system.used_memory()),
                        gib(system.total_memory()),
                        cpu_temperature(Path::new("/sys/class/hwmon")),
                        gpu_readings(Path::new("/sys/class/drm"), nvidia_output().as_deref())
                    );
                    if results
                        .send(Sample {
                            text,
                            taken: Instant::now(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            request,
            result,
            last: Instant::now()
                .checked_sub(Duration::from_secs(2))
                .unwrap_or_else(Instant::now),
            pending: false,
        })
    }
}
fn gib(bytes: u64) -> f64 {
    bytes as f64 / 1_073_741_824.
}
fn number(path: impl AsRef<Path>) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}
fn temperature(path: impl AsRef<Path>) -> Option<f64> {
    let value: f64 = std::fs::read_to_string(path).ok()?.trim().parse().ok()?;
    let c = value / 1000.;
    (c.is_finite() && (-50. ..=150.).contains(&c)).then_some(c)
}
fn directories(path: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    paths.sort();
    paths
}
fn cpu_temperature(root: &Path) -> String {
    for hwmon in directories(root) {
        let name = std::fs::read_to_string(hwmon.join("name")).unwrap_or_default();
        if matches!(name.trim(), "coretemp" | "k10temp" | "zenpower")
            && let Some(t) = temperature(hwmon.join("temp1_input"))
        {
            return format!("CPU temperature: {t:.0} °C");
        }
    }
    "CPU temperature: unavailable".into()
}
fn gpu_readings(root: &Path, nvidia: Option<&str>) -> String {
    let mut lines = Vec::new();
    for card in directories(root) {
        let name = card.file_name().unwrap_or_default().to_string_lossy();
        if !name
            .strip_prefix("card")
            .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        {
            continue;
        }
        let device = card.join("device");
        if !device.exists() {
            continue;
        }
        let vendor = std::fs::read_to_string(device.join("vendor")).unwrap_or_default();
        let vendor = match vendor.trim() {
            "0x1002" => "AMD",
            "0x8086" => "Intel",
            "0x10de" => "NVIDIA",
            _ => "GPU",
        };
        let pci = device
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        let nv = pci
            .as_deref()
            .and_then(|pci| nvidia.and_then(|text| nvidia_values(text, pci)));
        let usage = number(device.join("gpu_busy_percent"))
            .or_else(|| nv.as_ref().and_then(|v| v[0]))
            .filter(|v| *v <= 100)
            .map_or_else(|| "unavailable".into(), |v| format!("{v}%"));
        let vram = match (
            number(device.join("mem_info_vram_used")).or_else(|| {
                nv.as_ref()
                    .and_then(|v| v[1])
                    .and_then(|v| v.checked_mul(1_048_576))
            }),
            number(device.join("mem_info_vram_total")).or_else(|| {
                nv.as_ref()
                    .and_then(|v| v[2])
                    .and_then(|v| v.checked_mul(1_048_576))
            }),
        ) {
            (Some(used), Some(total)) if total > 0 => {
                format!("{:.1} / {:.1} GiB", gib(used), gib(total))
            }
            _ => "unavailable".into(),
        };
        let temp = directories(&device.join("hwmon"))
            .iter()
            .find_map(|h| temperature(h.join("temp1_input")))
            .or_else(|| {
                nv.as_ref()
                    .and_then(|v| v[3])
                    .filter(|v| *v <= 150)
                    .map(|v| v as f64)
            })
            .map_or_else(|| "unavailable".into(), |t| format!("{t:.0} °C"));
        lines.push(format!(
            "{vendor} {name}: {usage}    Temperature: {temp}\nVRAM: {vram}"
        ));
        if lines.len() == 3 {
            break;
        }
    }
    if lines.is_empty() {
        "GPU / VRAM / temperature: unavailable".into()
    } else {
        lines.join("\n")
    }
}
// NVIDIA usually exposes these counters through its management tool rather
// than DRM sysfs. Query only on this worker, with a strict deadline.
fn nvidia_output() -> Option<String> {
    use std::process::{Command, Stdio};
    if !directories(Path::new("/sys/class/drm")).iter().any(|p| {
        std::fs::read_to_string(p.join("device/vendor")).is_ok_and(|v| v.trim() == "0x10de")
    }) {
        return None;
    }
    let mut child = Command::new("nvidia-smi")
        .args([
            "--query-gpu=pci.bus_id,utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return status
                    .success()
                    .then(|| child.wait_with_output().ok())
                    .flatten()
                    .and_then(|output| String::from_utf8(output.stdout).ok());
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}
fn nvidia_values(text: &str, pci: &str) -> Option<[Option<u64>; 4]> {
    for line in text.lines() {
        let fields: Vec<_> = line.split(',').map(str::trim).collect();
        if fields.len() == 5
            && fields[0]
                .trim_start_matches('0')
                .eq_ignore_ascii_case(pci.trim_start_matches('0'))
        {
            return Some(std::array::from_fn(|i| fields[i + 1].parse().ok()));
        }
    }
    None
}
struct State {
    sampler: Sampler,
    text: String,
}
pub fn create(app: &mut AppState) -> anyhow::Result<OverlayWindowConfig> {
    let mut panel = GuiPanel::new_from_template(
        app,
        "gui/performance.xml",
        State {
            sampler: Sampler::new()?,
            text: "Reading PC sensors…".into(),
        },
        NewGuiPanelParams {
            resize_to_parent: true,
            ..Default::default()
        },
    )?;
    panel.max_size = glam::vec2(1200., 700.);
    let readings = panel.parser_state.get_widget_id("readings")?;
    panel.layout.add_event_listener::<AppState, State>(panel.layout.tree_root_widget, EventListenerKind::InternalStateChange, Box::new(move |common, _, app, state| {
        while let Ok(sample) = state.sampler.result.try_recv() {
            state.sampler.pending = false;
            state.text = if sample.taken.elapsed() < Duration::from_secs(3) { sample.text } else { "Refreshing PC sensors…".into() };
        }
        if !state.sampler.pending && state.sampler.last.elapsed() >= Duration::from_secs(1) {
            if state.sampler.request.try_send(()).is_ok() { state.sampler.pending = true; } else { state.text = "PC telemetry unavailable".into(); }
            state.sampler.last = Instant::now();
        }
        let frame = if app.delta_time.is_finite() && app.delta_time > 0. { format!("WayVR frame interval: {:.1} ms (~{:.0} Hz)", app.delta_time * 1000., app.delta_time.recip()) } else { "WayVR frame interval: unavailable".into() };
        let text = format!("{}\n{frame}\nCompositor GPU timing: unavailable\nRefresh: 1 s • unsupported sensors show unavailable", state.text);
        if let Some(mut label) = common.state.widgets.get_as::<WidgetLabel>(readings) { label.set_text(common, wgui::i18n::Translation::from_raw_text(&text)); }
        Ok(EventResult::Pass)
    }));
    panel
        .timers
        .push(GuiTimer::new(Duration::from_millis(250), 0));
    panel.update_layout(app)?;
    Ok(OverlayWindowConfig {
        name: NAME.into(),
        category: OverlayCategory::Panel,
        global: true,
        show_on_spawn: false,
        z_order: Z_ORDER_WATCH,
        default_state: OverlayWindowState {
            grabbable: false,
            interactable: true,
            angle_fade: true,
            positioning: Positioning::FollowHand {
                hand: LeftRight::Left,
                lerp: 1.,
            },
            transform: Affine3A::from_scale_rotation_translation(
                Vec3::splat(0.22),
                super::watch::WATCH_ROT,
                super::watch::WATCH_POS + super::watch::WATCH_ROT * Vec3::new(0., -0.12, 0.),
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
    fn monitor_layout_and_close_button_match_rendered_bounds() {
        use std::rc::Rc;
        use wgui::components::{ComponentTrait, button::ComponentButton};
        use wgui::event::{
            DeviceBitmask, Event, MouseButtonEvent, MouseButtonIndex, MouseMotionEvent,
        };
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
                path: wgui::assets::AssetPathRef::BuiltIn("gui/performance.xml"),
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
                size: glam::vec2(1200., 700.),
                timestep_alpha: 1.,
            })
            .unwrap();
        assert_eq!(layout.content_size, glam::vec2(1200., 700.));
        let button = parser
            .fetch_component_as::<ComponentButton>("close")
            .unwrap();
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
        assert!(pos.x < 1200. && pos.y < 700.);
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
        assert!(clicked.get());
    }
    #[test]
    fn gpu_sensor_values_and_missing_data_are_honest() {
        assert_eq!(
            nvidia_values("00000000:01:00.0, 42, 2048, 16384, 68", "0000:01:00.0"),
            Some([Some(42), Some(2048), Some(16384), Some(68)])
        );
        assert!(nvidia_values("00000000:01:00.0, 42, 2048, 16384, 68", "0000:02:00.0").is_none());
        assert_eq!(
            nvidia_values("00000000:01:00.0, N/A, 2048, N/A, 68", "0000:01:00.0"),
            Some([None, Some(2048), None, Some(68)])
        );
        let root = std::env::temp_dir().join(format!("xs-sensors-{}", uuid::Uuid::new_v4()));
        let device = root.join("card1/device");
        std::fs::create_dir_all(device.join("hwmon/hwmon1")).unwrap();
        for (file, value) in [
            ("vendor", "0x1002"),
            ("gpu_busy_percent", "34"),
            ("mem_info_vram_used", "1073741824"),
            ("mem_info_vram_total", "8589934592"),
            ("hwmon/hwmon1/temp1_input", "65000"),
        ] {
            std::fs::write(device.join(file), value).unwrap();
        }
        std::fs::create_dir_all(root.join("card1-DP-1/device")).unwrap();
        let text = gpu_readings(&root, None);
        assert!(
            text.contains("AMD card1: 34%")
                && text.contains("65 °C")
                && text.contains("1.0 / 8.0 GiB")
        );
        assert!(!text.contains("DP-1"));
        std::fs::write(device.join("gpu_busy_percent"), "200").unwrap();
        std::fs::write(device.join("hwmon/hwmon1/temp1_input"), "bad").unwrap();
        assert!(gpu_readings(&root, None).contains("card1: unavailable"));
        assert!(cpu_temperature(&root).contains("unavailable"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
