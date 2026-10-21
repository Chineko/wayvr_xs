mod capture;
mod steamvr;

use anyhow::{Context, Result, ensure};
use clap::{Parser, ValueEnum};
use std::{
    io::{self, BufRead},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use windows::Win32::{
    System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, SetConsoleCtrlHandler},
    UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
};
use windows::core::BOOL;
use xs_core::screen_region::ScreenRegion;

static RUNNING: AtomicBool = AtomicBool::new(true);
unsafe extern "system" fn console_event(event: u32) -> BOOL {
    if matches!(event, CTRL_C_EVENT | CTRL_CLOSE_EVENT | CTRL_BREAK_EVENT) {
        RUNNING.store(false, Ordering::Relaxed);
        BOOL(1)
    } else {
        BOOL(0)
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Layout {
    Full,
    Dual,
    Thirds,
    Center,
}
impl Layout {
    fn regions(self) -> Vec<ScreenRegion> {
        let (parts, total): (&[(u32, u32)], u32) = match self {
            Self::Full => (&[(0, 1)], 1),
            Self::Dual => (&[(0, 1), (1, 2)], 2),
            Self::Thirds => (&[(0, 1), (1, 2), (2, 3)], 3),
            Self::Center => (&[(0, 1), (1, 3), (3, 4)], 4),
        };
        parts
            .iter()
            .map(|&(start, end)| ScreenRegion { start, end, total })
            .collect()
    }
}
fn positive(s: &str) -> std::result::Result<f32, String> {
    let n: f32 = s.parse().map_err(|_| "Expected a number")?;
    if n.is_finite() && (0.1..=10.0).contains(&n) {
        Ok(n)
    } else {
        Err("Expected 0.1 to 10 meters".into())
    }
}

#[derive(Clone, Copy, Debug)]
struct ExpectedSize([u32; 2]);
impl std::str::FromStr for ExpectedSize {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let (w, h) = s
            .split_once(['x', 'X'])
            .ok_or("Use WIDTHxHEIGHT, for example 5120x1440")?;
        let width = w.parse::<u32>().map_err(|_| "Invalid width")?;
        let height = h.parse::<u32>().map_err(|_| "Invalid height")?;
        if width == 0 || height == 0 {
            return Err("Width and height must be positive".into());
        }
        Ok(Self([width, height]))
    }
}

fn check_size(actual: [u32; 2], expected: Option<ExpectedSize>) -> Result<()> {
    if let Some(ExpectedSize(expected)) = expected {
        ensure!(
            actual == expected,
            "Windows is providing {}x{}, expected {}x{}. Restore the desired resolution in Windows Display settings while your VR streaming session is connected, and disable any automatic resolution adjustment in the streaming software. This app cannot capture pixels outside the active Windows desktop.",
            actual[0],
            actual[1],
            expected[0],
            expected[1]
        );
    }
    Ok(())
}
#[derive(Parser)]
#[command(
    version,
    about = "Experimental native Windows SteamVR desktop panels (view-only)"
)]
struct Args {
    #[arg(long)]
    list_monitors: bool,
    #[arg(long, default_value_t = 0)]
    monitor: usize,
    #[arg(long, value_enum, default_value_t = Layout::Dual)]
    layout: Layout,
    #[arg(long, default_value = "0.7", value_parser = positive)]
    height: f32,
    #[arg(long, default_value = "1.5", value_parser = positive)]
    distance: f32,
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u32).range(1..=120))]
    fps: u32,
    /// Exercise GPU capture for this many seconds without starting SteamVR.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=60))]
    capture_test: Option<u32>,
    #[arg(long)]
    openvr_dll: Option<PathBuf>,
    /// Stop with a useful message if streaming software changes the desktop resolution.
    #[arg(long, value_name = "WIDTHxHEIGHT")]
    expect_size: Option<ExpectedSize>,
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("WayVR XS: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
            .context("Enable physical-pixel DPI awareness")?;
        SetConsoleCtrlHandler(Some(console_event), true)?;
    }
    let monitors = capture::monitors()?;
    for (i, m) in monitors.iter().enumerate() {
        println!(
            "{i}: {} — {}x{} at ({},{}) GPU {}",
            m.name,
            m.rect.right - m.rect.left,
            m.rect.bottom - m.rect.top,
            m.rect.left,
            m.rect.top,
            m.adapter_index
        );
        if let Some([w, h]) = m.active_mode {
            println!("   Windows active pixel mode: {w}x{h}");
        }
    }
    if args.list_monitors {
        return Ok(());
    }
    let selected = monitors
        .get(args.monitor)
        .context("Monitor index is unavailable; use --list-monitors")?;
    let name = selected.name.clone();
    if let Some(mode) = selected.active_mode {
        check_size(mode, args.expect_size)?;
    }
    let mut capture = capture::Capture::new(selected)?;
    if let Some(seconds) = args.capture_test {
        let start = Instant::now();
        let mut frames = 0;
        while RUNNING.load(Ordering::Relaxed) && start.elapsed().as_secs() < u64::from(seconds) {
            if capture.update(50)? {
                check_size(capture.size, args.expect_size)?;
                frames += 1;
            }
        }
        ensure!(frames > 0, "No desktop frames arrived");
        println!(
            "GPU capture passed: {frames} updates, {}x{}, no CPU pixel readback",
            capture.size[0], capture.size[1]
        );
        return Ok(());
    }
    let dll = steamvr::find_dll(args.openvr_dll.as_deref())?;
    println!("Using SteamVR loader: {}", dll.display());
    let mut vr = steamvr::Runtime::open(&dll)?;
    ensure!(
        vr.adapter() == selected.adapter_index as i32,
        "Selected monitor uses GPU {}, but SteamVR uses GPU {}. Connect the monitor and headset to the same GPU for this build.",
        selected.adapter_index,
        vr.adapter()
    );
    let mut layout = args.layout;
    let mut regions = layout.regions();
    vr.rebuild(regions.len())?;
    let mut head = None;
    let mut offsets = vec![[0.0; 3]; regions.len()];
    let mut height = args.height;
    let mut distance = args.distance;
    let mut visible = vec![true; regions.len()];
    let mut dirty = true;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in io::stdin()
            .lock()
            .lines()
            .map_while(std::result::Result::ok)
        {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    println!(
        "Waiting for headset tracking. Panels are view-only. Commands: recenter, layout dual|thirds|center|full, height 0.7, distance 1.5, move 1 x y z, hide, show, quit"
    );
    let frame_time = Duration::from_secs_f64(1.0 / f64::from(args.fps));
    let mut last_recovery = Instant::now() - Duration::from_secs(5);
    let mut recovering = false;
    while RUNNING.load(Ordering::Relaxed) && !vr.quitting() {
        let tick = Instant::now();
        for line in rx.try_iter() {
            let words: Vec<_> = line.split_whitespace().collect();
            match words.as_slice() {
                ["quit"] => RUNNING.store(false, Ordering::Relaxed),
                ["recenter"] => {
                    head = vr.head();
                    dirty = true;
                }
                ["hide"] => {
                    visible.fill(false);
                    vr.show(&visible)?;
                }
                ["show"] => {
                    visible.fill(true);
                    dirty = true;
                }
                [command @ ("hide" | "show"), index] => match index.parse::<usize>() {
                    Ok(i) if i > 0 && i <= visible.len() => {
                        visible[i - 1] = *command == "show";
                        dirty = true;
                    }
                    _ => eprintln!("Expected a panel number from 1 to {}", visible.len()),
                },
                ["layout", value] => match Layout::from_str(value, true) {
                    Ok(next) => {
                        layout = next;
                        regions = layout.regions();
                        vr.rebuild(regions.len())?;
                        offsets = vec![[0.0; 3]; regions.len()];
                        visible = vec![true; regions.len()];
                        dirty = true;
                    }
                    Err(e) => eprintln!("{e}"),
                },
                ["height", value] | ["distance", value] => match positive(value) {
                    Ok(n) => {
                        if words[0] == "height" {
                            height = n;
                        } else {
                            distance = n;
                        }
                        dirty = true;
                    }
                    Err(e) => eprintln!("{e}"),
                },
                ["move", index, x, y, z] => {
                    let parsed = (
                        index.parse::<usize>(),
                        x.parse::<f32>(),
                        y.parse::<f32>(),
                        z.parse::<f32>(),
                    );
                    match parsed {
                        (Ok(i), Ok(x), Ok(y), Ok(z))
                            if i > 0
                                && i <= offsets.len()
                                && [x, y, z].iter().all(|n| n.is_finite() && n.abs() <= 10.0) =>
                        {
                            offsets[i - 1] = [x, y, z];
                            dirty = true;
                        }
                        _ => eprintln!(
                            "Use move <panel number starting at 1> <right meters> <up meters> <away meters>, within ±10m"
                        ),
                    }
                }
                [] => (),
                _ => eprintln!(
                    "Commands: recenter, layout dual|thirds|center|full, height <meters>, distance <meters>, move <panel> <x> <y> <z>, hide, show, quit"
                ),
            }
        }
        if recovering {
            if last_recovery.elapsed() >= Duration::from_secs(1) {
                last_recovery = Instant::now();
                let retry = (|| -> Result<capture::Capture> {
                    let outputs = capture::monitors()?;
                    let monitor = outputs
                        .iter()
                        .find(|m| m.name == name)
                        .context("Selected monitor is disconnected")?;
                    ensure!(
                        monitor.adapter_index as i32 == vr.adapter(),
                        "Monitor moved to another GPU; restart after selecting a compatible monitor"
                    );
                    capture::Capture::new(monitor)
                })();
                match retry {
                    Ok(next) => {
                        capture = next;
                        recovering = false;
                        dirty = true;
                        println!("Desktop capture reconnected");
                    }
                    Err(e) => {
                        eprintln!("Waiting for desktop: {e:#}");
                    }
                }
            }
        } else {
            match capture.update(5) {
                Ok(updated) => {
                    if updated {
                        check_size(capture.size, args.expect_size)?;
                    }
                    if head.is_none() {
                        head = vr.head();
                        dirty = true;
                    }
                    if (updated || dirty)
                        && let (Some(texture), Some(origin)) = (&capture.texture, head)
                    {
                        let widths = panel_widths(&regions, capture.size, height);
                        let total: f32 =
                            widths.iter().sum::<f32>() + (widths.len() - 1) as f32 * 0.03;
                        let mut left = -total / 2.0;
                        for (i, width) in widths.iter().enumerate() {
                            let [x, y, z] = offsets[i];
                            let matrix = placement(origin, left + width / 2.0 + x, y, distance + z);
                            vr.place(i, *width, &matrix)?;
                            left += width + 0.03;
                        }
                        let bounds: Vec<_> =
                            regions.iter().map(|r| r.uv(capture.size[0])).collect();
                        vr.submit(texture, &bounds, capture.color)?;
                        vr.show(&visible)?;
                        dirty = false;
                    }
                }
                Err(e) => {
                    if !capture::recoverable(&e) {
                        return Err(e).context("Capture stopped; this error cannot be fixed by repeatedly reconnecting");
                    }
                    eprintln!("Capture paused: {e:#}. Retrying after desktop/device recovery.");
                    vr.show(&vec![false; regions.len()])?;
                    vr.clear();
                    capture.pause();
                    recovering = true;
                    last_recovery = Instant::now();
                }
            }
        }
        if let Some(rest) = frame_time.checked_sub(tick.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    // Clear runtime references while the capture texture/device still exist.
    drop(vr);
    Ok(())
}

fn panel_widths(regions: &[ScreenRegion], size: [u32; 2], height: f32) -> Vec<f32> {
    regions
        .iter()
        .map(|r| {
            let [a, b] = r.pixels(size[0]);
            (b - a) as f32 / size[1].max(1) as f32 * height
        })
        .collect()
}

/// World-locked, level placement from a snapshot of headset yaw (OpenVR looks down -Z).
fn placement(head: [f32; 12], x: f32, y: f32, distance: f32) -> [f32; 12] {
    let length = (head[2] * head[2] + head[10] * head[10]).sqrt();
    let (sin, cos) = if length > 0.001 {
        (head[2] / length, head[10] / length)
    } else {
        (0.0, 1.0)
    };
    [
        cos,
        0.0,
        sin,
        head[3] + cos * x - sin * distance,
        0.0,
        1.0,
        0.0,
        head[7] + y,
        -sin,
        0.0,
        cos,
        head[11] - sin * x - cos * distance,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolution_guard_detects_streaming_mode_change_before_capture() {
        let expected: ExpectedSize = "5120x1440".parse().unwrap();
        assert!(check_size([5120, 1440], Some(expected)).is_ok());
        let error = check_size([2560, 1440], Some(expected)).unwrap_err();
        assert!(error.to_string().contains("Windows is providing 2560x1440"));
        assert!(check_size([2560, 1440], None).is_ok());
        for invalid in ["0x1440", "5120x0", "5120", "-1x1440", "5120x1440x2"] {
            assert!(invalid.parse::<ExpectedSize>().is_err());
        }
        let args = Args::try_parse_from(["xs", "--expect-size", "5120x1440"]).unwrap();
        assert_eq!(args.expect_size.unwrap().0, [5120, 1440]);
    }
    #[test]
    fn every_layout_covers_ultrawide_once_and_preserves_physical_aspect() {
        for layout in [Layout::Full, Layout::Dual, Layout::Thirds, Layout::Center] {
            let regions = layout.regions();
            for width in [5120, 5119] {
                let mut end = 0;
                for r in &regions {
                    let [a, b] = r.pixels(width);
                    assert_eq!(a, end);
                    end = b;
                }
                assert_eq!(end, width);
                let total: f32 = panel_widths(&regions, [width, 1440], 0.7).iter().sum();
                assert!((total - width as f32 / 1440.0 * 0.7).abs() < 0.00001);
            }
        }
    }
    #[test]
    fn head_yaw_positions_panels_in_front_without_head_pitch() {
        let head = [1.0, 0.0, 0.0, 2.0, 0.0, 1.0, 0.0, 1.6, 0.0, 0.0, 1.0, 3.0];
        let m = placement(head, 0.5, 0.0, 1.5);
        assert_eq!([m[3], m[7], m[11]], [2.5, 1.6, 1.5]);
        let yaw = [0.0, 0.0, 1.0, 2.0, 0.0, 1.0, 0.0, 1.6, -1.0, 0.0, 0.0, 3.0];
        let m = placement(yaw, 0.5, 0.0, 1.5);
        assert_eq!([m[3], m[7], m[11]], [0.5, 1.6, 2.5]);
    }
}
