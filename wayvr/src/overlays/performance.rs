//! Request-driven PC telemetry. The worker sleeps when the panel is not rendered.
use crate::{gui::panel::GuiPanel, state::AppState};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};
use wgui::{
    event::EventListenerKind,
    parser::Fetchable,
    widget::{EventResult, label::WidgetLabel},
};

#[derive(Default)]
struct Metrics {
    cpu: Option<f64>,
    ram_used: u64,
    ram_total: u64,
    cpu_temp: Option<f64>,
    gpus: Vec<Gpu>,
}
struct Gpu {
    name: String,
    usage: Option<u64>,
    used: Option<u64>,
    total: Option<u64>,
    temp: Option<f64>,
}
struct Sample {
    metrics: Metrics,
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
                    let cpu = previous
                        .filter(|t| t.elapsed() < Duration::from_secs(3))
                        .map(|_| f64::from(system.global_cpu_usage()));
                    previous = Some(Instant::now());
                    let metrics = Metrics {
                        cpu,
                        ram_used: system.used_memory(),
                        ram_total: system.total_memory(),
                        cpu_temp: cpu_temp(Path::new("/sys/class/hwmon")),
                        gpus: gpu_metrics(Path::new("/sys/class/drm"), nvidia_output().as_deref()),
                    };
                    if results
                        .send(Sample {
                            metrics,
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
fn cpu_temp(root: &Path) -> Option<f64> {
    for hwmon in directories(root) {
        let name = std::fs::read_to_string(hwmon.join("name")).unwrap_or_default();
        if matches!(name.trim(), "coretemp" | "k10temp" | "zenpower")
            && let Some(t) = temperature(hwmon.join("temp1_input"))
        {
            return Some(t);
        }
    }
    None
}
fn gpu_metrics(root: &Path, nvidia: Option<&str>) -> Vec<Gpu> {
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
            .filter(|v| *v <= 100);
        let (used, total) = (
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
        );
        let temp = directories(&device.join("hwmon"))
            .iter()
            .find_map(|h| temperature(h.join("temp1_input")))
            .or_else(|| {
                nv.as_ref()
                    .and_then(|v| v[3])
                    .filter(|v| *v <= 150)
                    .map(|v| v as f64)
            });
        lines.push(Gpu {
            name: format!("{vendor} {name}"),
            usage,
            used,
            total,
            temp,
        });
        if lines.len() == 3 {
            break;
        }
    }
    lines
}
#[cfg(test)]
fn gpu_readings(root: &Path, nvidia: Option<&str>) -> String {
    gpu_metrics(root, nvidia)
        .iter()
        .map(|g| {
            format!(
                "{}: {}    Temperature: {}\nVRAM: {}",
                g.name,
                g.usage
                    .map_or_else(|| "unavailable".into(), |v| format!("{v}%")),
                g.temp
                    .map_or_else(|| "unavailable".into(), |v| format!("{v:.0} °C")),
                match (g.used, g.total) {
                    (Some(u), Some(t)) if t > 0 => format!("{:.1} / {:.1} GiB", gib(u), gib(t)),
                    _ => "unavailable".into(),
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    metrics: Option<Metrics>,
    selected: Option<String>,
    next_gpu: std::rc::Rc<std::cell::Cell<bool>>,
}
#[derive(Clone, Copy)]
struct Gauge {
    label: wgui::layout::WidgetID,
    fill: Option<wgui::layout::WidgetID>,
}
fn ratio(value: Option<f64>, total: f64) -> f32 {
    value
        .filter(|v| v.is_finite())
        .filter(|_| total.is_finite() && total > 0.)
        .map_or(0., |v| (v / total).clamp(0., 1.) as f32)
}
fn paint(common: &mut wgui::event::CallbackDataCommon, gauge: Gauge, amount: f32, text: &str) {
    if let Some(mut label) = common.state.widgets.get_as::<WidgetLabel>(gauge.label) {
        label.set_text(common, wgui::i18n::Translation::from_raw_text(text));
    }
    if let Some(fill) = gauge.fill {
        common.alterables.set_style(
            fill,
            wgui::event::StyleSetRequest::Width(wgui::taffy::prelude::percent(amount)),
        );
    }
}
fn percent_text(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |v| format!("{v:.0}%"))
}
fn memory_text(used: Option<u64>, total: Option<u64>) -> String {
    match (used, total) {
        (Some(u), Some(t)) if t > 0 => format!("{:.1}/{:.0} GiB", gib(u), gib(t)),
        _ => "—".into(),
    }
}
/// Attach live telemetry to the watch canvas instead of creating another hand overlay.
pub fn attach<S: 'static>(panel: &mut GuiPanel<S>) -> anyhow::Result<()> {
    let Ok(surface) = panel.parser_state.get_widget_id("shelf_performance") else {
        return Ok(());
    };
    let state = Rc::new(RefCell::new(State {
        sampler: Sampler::new()?,
        metrics: None,
        selected: None,
        next_gpu: Default::default(),
    }));
    let mut gauges = Vec::new();
    for id in ["frame", "gpu", "vram", "gpu_temp", "cpu", "ram", "cpu_temp"] {
        gauges.push(Gauge {
            label: panel.parser_state.get_widget_id(&format!("{id}_value"))?,
            fill: panel.parser_state.get_widget_id(&format!("{id}_fill")).ok(),
        });
    }
    let gpu_name = panel.parser_state.get_widget_id("gpu_name")?;
    let status = panel.parser_state.get_widget_id("status")?;
    let next = state.borrow().next_gpu.clone();
    panel
        .parser_state
        .fetch_component_as::<wgui::components::button::ComponentButton>("next_gpu")?
        .on_click(std::rc::Rc::new(move |_, _| {
            next.set(true);
            Ok(())
        }));
    panel.layout.add_event_listener::<AppState, S>(
        panel.layout.tree_root_widget,
        EventListenerKind::InternalStateChange,
        Box::new(move |common, _, app, _| {
            let node = common.state.nodes.get(surface).copied();
            if node.is_none_or(|node| !wgui::widget::is_node_visible(&common.state.tree, node)) {
                return Ok(EventResult::Pass);
            }
            let mut state = state.borrow_mut();
            while let Ok(sample) = state.sampler.result.try_recv() {
                state.sampler.pending = false;
                state.metrics =
                    (sample.taken.elapsed() < Duration::from_secs(3)).then_some(sample.metrics);
            }
            if !state.sampler.pending && state.sampler.last.elapsed() >= Duration::from_secs(1) {
                state.sampler.pending = state.sampler.request.try_send(()).is_ok();
                state.sampler.last = Instant::now();
            }
            if state.sampler.pending && state.sampler.last.elapsed() > Duration::from_secs(3) {
                state.metrics = None;
            }
            let frame = f64::from(app.delta_time);
            let frame_valid = frame.is_finite() && frame > 0.;
            let ms = frame_valid.then_some(frame * 1000.);
            paint(
                common,
                gauges[0],
                ratio(ms, 33.333),
                &if frame_valid {
                    format!("{:.0} FPS  ·  {:.1} ms", frame.recip(), frame * 1000.)
                } else {
                    "—".into()
                },
            );
            let state = &mut *state;
            let metrics = state.metrics.as_ref();
            let gpu = metrics.and_then(|m| {
                if m.gpus.is_empty() {
                    return None;
                }
                let selected = state
                    .selected
                    .as_ref()
                    .and_then(|name| m.gpus.iter().position(|g| &g.name == name));
                let mut index = selected.unwrap_or_else(|| {
                    m.gpus
                        .iter()
                        .enumerate()
                        .max_by_key(|(_, g)| (g.name.starts_with("NVIDIA"), g.total.unwrap_or(0)))
                        .map_or(0, |(i, _)| i)
                });
                if state.next_gpu.replace(false) {
                    index = (index + 1) % m.gpus.len();
                }
                state.selected = Some(m.gpus[index].name.clone());
                Some(&m.gpus[index])
            });
            let gpu_usage = gpu.and_then(|g| g.usage).map(|v| v as f64);
            paint(
                common,
                gauges[1],
                ratio(gpu_usage, 100.),
                &percent_text(gpu_usage),
            );
            let (used, total) = (gpu.and_then(|g| g.used), gpu.and_then(|g| g.total));
            paint(
                common,
                gauges[2],
                ratio(used.map(|v| v as f64), total.unwrap_or(0) as f64),
                &memory_text(used, total),
            );
            let gpu_temp = gpu.and_then(|g| g.temp);
            paint(
                common,
                gauges[3],
                ratio(gpu_temp, 100.),
                &gpu_temp.map_or_else(|| "—".into(), |v| format!("{v:.0} °C")),
            );
            let cpu = metrics.and_then(|m| m.cpu);
            paint(common, gauges[4], ratio(cpu, 100.), &percent_text(cpu));
            let (used, total) = (metrics.map(|m| m.ram_used), metrics.map(|m| m.ram_total));
            paint(
                common,
                gauges[5],
                ratio(used.map(|v| v as f64), total.unwrap_or(0) as f64),
                &memory_text(used, total),
            );
            let cpu_temp = metrics.and_then(|m| m.cpu_temp);
            paint(
                common,
                gauges[6],
                ratio(cpu_temp, 100.),
                &cpu_temp.map_or_else(|| "—".into(), |v| format!("{v:.0} °C")),
            );
            for (id, text) in [
                (gpu_name, gpu.map_or("GPU unavailable", |g| g.name.as_str())),
                (status, "WayVR loop timing · — unavailable / warming up"),
            ] {
                if let Some(mut label) = common.state.widgets.get_as::<WidgetLabel>(id) {
                    label.set_text(common, wgui::i18n::Translation::from_raw_text(text));
                }
            }
            Ok(EventResult::Pass)
        }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dashboard_bar_ranges_reject_invalid_values() {
        assert_eq!(ratio(None, 100.), 0.);
        assert_eq!(ratio(Some(f64::NAN), 100.), 0.);
        assert_eq!(ratio(Some(50.), 0.), 0.);
        assert_eq!(ratio(Some(-20.), 100.), 0.);
        assert_eq!(ratio(Some(250.), 100.), 1.);
        assert_eq!(ratio(Some(50.), 100.), 0.5);
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
        assert!(cpu_temp(&root).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
