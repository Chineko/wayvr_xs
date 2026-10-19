#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(
    clippy::cargo_common_metadata,
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::match_wildcard_for_single_variants,
    clippy::multiple_crate_versions,
    clippy::needless_pass_by_ref_mut,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else,
    clippy::struct_excessive_bools,
    clippy::suboptimal_flops,
    clippy::too_many_lines,
    clippy::use_self
)]
mod app_misc;
mod backend;
mod config;
mod graphics;
mod gui;
mod ipc;
mod layout_presets;
mod overlays;
mod shaders;
mod state;
mod subsystem;
mod test;
mod windowing;

use std::{
    ffi::{OsStr, OsString},
    os::unix::{ffi::OsStrExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant},
};

use clap::Parser;
use libc::{SIGINT, SIGTERM, SIGUSR1};
use signal_hook::iterator::Signals;
use sysinfo::Pid;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use wgui::{font_config::WguiFontConfig, globals::WguiGlobals};

use crate::{config::load_general_config, subsystem::dbus::DbusConnector};
use wlx_common::{
    XrBackend,
    config::GeneralConfig,
    config_io::{self, get_config_file_path},
    locale::WayVRLangProvider,
    palette::load_palette,
};

pub static FRAME_COUNTER: AtomicUsize = AtomicUsize::new(0);
pub static RUNNING: AtomicBool = AtomicBool::new(true);
pub static RESTART: AtomicBool = AtomicBool::new(false);
pub static KEYMAP_CHANGE: AtomicBool = AtomicBool::new(false);

/// The lightweight desktop overlay for OpenVR and OpenXR
#[derive(Default, Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[cfg(feature = "openvr")]
    /// Force OpenVR backend
    #[arg(long)]
    openvr: bool,

    #[cfg(feature = "openxr")]
    /// Force OpenXR backend
    #[arg(long)]
    openxr: bool,

    /// Show the working set of overlay on startup. Also skips tutorial.
    #[arg(long)]
    show: bool,

    /// Wait for a runtime to be available instead of exiting
    #[arg(long)]
    wait: bool,

    /// Uninstall SteamVR manifest and exit
    #[arg(long)]
    uninstall: bool,

    /// Install SteamVR manifest (not recommended; WayVR may have issues when auto-started by SteamVR!)
    #[arg(long)]
    install: bool,

    /// Replace running WayVR instance
    #[arg(long)]
    replace: bool,

    /// Allow multiple running instances of WayVR (things may break!)
    #[arg(long)]
    multi: bool,

    /// Disable desktop access altogether.
    #[arg(long)]
    headless: bool,

    /// Do not auto-start registered apps
    #[arg(long)]
    no_autostart: bool,

    /// Path to write logs to
    #[arg(short, long, value_name = "FILE_PATH")]
    log_to: Option<String>,

    /// Disable audio output
    #[arg(long)]
    no_audio: bool,
}

fn load_config() -> GeneralConfig {
    let config_root_path = config_io::ConfigRoot::Generic.ensure_dir();
    log::info!("Config root path: {}", config_root_path.display());
    let mut config = load_general_config();
    if let Err(error) = layout_presets::apply_on_startup(&mut config) {
        log::error!("Preset not applied: {error:#}");
    }
    config
}

fn init_run_params(no_audio: bool) -> backend::RunParams {
    let assets = Box::new(gui::asset::GuiAsset {});
    let config = load_config();
    let lang_provider = WayVRLangProvider::from_config(&config);
    let theme_path = config.theme_path.clone();

    let wgui_globals = WguiGlobals::new(
        assets,
        &lang_provider,
        &WguiFontConfig::default(),
        get_config_file_path(&theme_path),
        load_palette(&config.color_palette),
    )
    .expect("Failed to init Globals, can't proceed");

    backend::RunParams {
        wgui_globals,
        config,
        no_audio,
    }
}

#[allow(clippy::unnecessary_wraps)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = if std::env::args().skip(1).any(|a| !a.is_empty()) {
        Args::parse()
    } else {
        Args::default()
    };

    if !args.multi && !ensure_single_instance(args.replace) {
        println!("Looks like WayVR is already running.");
        println!("Use --replace and I will terminate it for you.");
        return Ok(());
    }

    logging_init(&mut args);

    log::info!(
        "Welcome to {} version {}!{}",
        env!("CARGO_PKG_NAME"),
        env!("WLX_BUILD"),
        if std::env::var("APPDIR").is_ok() {
            " (AppImage)"
        } else {
            ""
        }
    );
    log::info!("It is {}.", chrono::Local::now().format("%c"));

    #[cfg(feature = "openvr")]
    if args.uninstall {
        crate::backend::openvr::openvr_uninstall();
        return Ok(());
    }

    setup_signal_hooks()?;

    let mut used_backend = None;
    auto_run(args, &mut used_backend);

    if RESTART.load(Ordering::Relaxed) {
        log::warn!("Restarting...");
        let exe = restart_program(
            &std::env::current_exe()?,
            std::env::var_os("APPIMAGE").as_deref(),
        );
        let args = restart_arguments(std::env::args_os().skip(1), used_backend);
        let mut command = Command::new(exe);
        command.args(args);
        let error = restart_exec(command);
        log::error!("Could not restart WayVR: {error}");
        return Err(error.into());
    }

    Ok(())
}

// Re-exec keeps the desktop launcher's process scope and avoids racing our
// own PID file. Prefer the current installed binary after a package update.
fn restart_program(current: &Path, appimage: Option<&OsStr>) -> PathBuf {
    if let Some(image) = appimage.filter(|path| Path::new(path).is_file()) {
        return PathBuf::from(image);
    }
    if let Some(path) = current.as_os_str().as_bytes().strip_suffix(b" (deleted)") {
        let path = Path::new(OsStr::from_bytes(path));
        if path.is_file() {
            return path.to_path_buf();
        }
    }
    current.to_path_buf()
}
fn restart_arguments(
    arguments: impl IntoIterator<Item = OsString>,
    backend: Option<XrBackend>,
) -> Vec<OsString> {
    let mut args: Vec<_> = arguments
        .into_iter()
        .filter(|arg| arg != "--replace")
        .collect();
    if !args.iter().any(|arg| arg == "--show") {
        args.push("--show".into());
    }
    if !args.iter().any(|arg| arg == "--wait") {
        args.push("--wait".into());
    }
    if !args
        .iter()
        .any(|arg| arg == "--openxr" || arg == "--openvr")
    {
        match backend {
            Some(XrBackend::OpenXR) => args.push("--openxr".into()),
            Some(XrBackend::OpenVR) => args.push("--openvr".into()),
            _ => {}
        }
    }
    args
}
fn restart_exec(mut command: Command) -> std::io::Error {
    command.exec()
}

#[allow(unused_mut, clippy::similar_names)]
fn auto_run(args: Args, used_backend: &mut Option<XrBackend>) {
    let mut tried_xr = false;
    let mut tried_vr = false;

    loop {
        #[cfg(feature = "openxr")]
        if !args_get_openvr(&args) {
            use crate::backend::{BackendError, openxr::openxr_run};
            tried_xr = true;
            match openxr_run(&args, init_run_params(args.no_audio)) {
                Ok(()) => {
                    used_backend.replace(XrBackend::OpenXR);
                    return;
                }
                Err(BackendError::NotSupported) => (),
                Err(e) => {
                    used_backend.replace(XrBackend::OpenXR);
                    log::error!("{e:?}");
                    return;
                }
            }
        }

        #[cfg(feature = "openvr")]
        if !args_get_openxr(&args) {
            use crate::backend::{BackendError, openvr::openvr_run};
            tried_vr = true;
            match openvr_run(&args, init_run_params(args.no_audio)) {
                Ok(()) => {
                    used_backend.replace(XrBackend::OpenVR);
                    return;
                }
                Err(BackendError::NotSupported) => (),
                Err(e) => {
                    used_backend.replace(XrBackend::OpenVR);
                    log::error!("{e:?}");
                    return;
                }
            }
        }

        if args.wait {
            // wait 5s unless we get signaled
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if !RUNNING.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        } else {
            break;
        }
    }

    log::error!("No more backends to try");

    let instructions_key = match (tried_xr, tried_vr) {
        (true, true) => "NOTIFICATION.NOT_RUNNING_XR_VR",
        (false, true) => "NOTIFICATION.NOT_RUNNING_VR",
        (true, false) => "NOTIFICATION.NOT_RUNNING_XR",
        _ => "NOTIFICATION.INVALID_ARGS",
    };

    let run_params = init_run_params(args.no_audio);

    let instructions_str = run_params.wgui_globals.i18n().translate(instructions_key);

    let instructions_title = run_params
        .wgui_globals
        .i18n()
        .translate("NOTIFICATION.COUD_NOT_CONNECT_TO_VR_RUNTIME");

    let instructions = format!("{instructions_title}\n{instructions_str}");

    let _ = DbusConnector::notify_send("WayVR", &instructions, 1, 30000, 0, false);

    #[cfg(not(any(feature = "openvr", feature = "openxr")))]
    compile_error!("No VR support! Enable either openvr or openxr features!");
}

#[allow(dead_code, unused_variables)]
const fn args_get_openvr(args: &Args) -> bool {
    #[cfg(feature = "openvr")]
    let ret = args.openvr;

    #[cfg(not(feature = "openvr"))]
    let ret = false;

    ret
}

#[allow(dead_code, unused_variables)]
const fn args_get_openxr(args: &Args) -> bool {
    #[cfg(feature = "openxr")]
    let ret = args.openxr;

    #[cfg(not(feature = "openxr"))]
    let ret = false;

    ret
}

#[allow(clippy::single_match_else)]
fn setup_signal_hooks() -> anyhow::Result<()> {
    let mut signals = Signals::new([SIGINT, SIGTERM, SIGUSR1])?;

    std::thread::spawn(move || {
        for signal in signals.forever() {
            match signal {
                SIGUSR1 => {
                    log::info!("SIGUSR1 received (keymap changed)");
                    KEYMAP_CHANGE.store(true, Ordering::Relaxed);
                }
                _ => {
                    RUNNING.store(false, Ordering::Relaxed);
                    break;
                }
            }
        }
    });
    Ok(())
}

fn logging_init(args: &mut Args) {
    let log_file_path = args
        .log_to
        .take()
        .or_else(|| std::env::var("WAYVR_LOGFILE").ok())
        .unwrap_or_else(|| String::from("/tmp/wayvr.log"));

    let file_writer = match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_file_path)
    {
        Ok(file) => {
            println!("Logging to {log_file_path}");
            Some(file)
        }
        Err(e) => {
            println!("Failed to open log file (path: {e:?}): {log_file_path}");
            None
        }
    };

    let registry = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .pretty()
                .with_writer(std::io::stderr),
        )
        .with(
            /* read RUST_LOG env var */
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy()
                .add_directive("symphonia_core::probe=warn".parse().unwrap())
                .add_directive("symphonia_bundle_mp3=warn".parse().unwrap())
                .add_directive("zbus=warn".parse().unwrap())
                .add_directive("usvg=error".parse().unwrap())
                .add_directive("resvg=error".parse().unwrap())
                .add_directive("cosmic_text=warn".parse().unwrap())
                .add_directive("wlx_capture::wayland=info".parse().unwrap())
                .add_directive("smithay=debug".parse().unwrap()), /* GLES render spam */
        );

    if let Some(writer) = file_writer {
        registry
            .with(
                tracing_subscriber::fmt::layer()
                    .with_file(true)
                    .with_line_number(true)
                    .with_writer(writer)
                    .with_ansi(false),
            )
            .init();
    } else {
        registry.init();
    }

    log_panics::init();
}

fn ensure_single_instance(replace: bool) -> bool {
    let mut path =
        std::env::var("XDG_RUNTIME_DIR").map_or_else(|_| PathBuf::from("/tmp"), PathBuf::from);
    path.push("wayvr.pid");
    ensure_single_instance_at(&path, replace)
}
fn ensure_single_instance_at(path: &Path, replace: bool) -> bool {
    if path.exists() {
        // load contents
        if let Ok(pid_str) = std::fs::read_to_string(path)
            && let Ok(pid) = pid_str.trim().parse::<u32>()
            && pid != std::process::id()
        {
            let mut system = sysinfo::System::new();
            system.refresh_processes(
                sysinfo::ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
                false,
            );
            if let Some(proc) = system.process(sysinfo::Pid::from_u32(pid)) {
                if replace {
                    proc.kill_with(sysinfo::Signal::Term);
                    proc.wait();
                } else {
                    return false;
                }
            }
        }
    }

    let pid = std::process::id().to_string();
    std::fs::write(path, pid).unwrap();

    true
}

#[cfg(test)]
mod restart_tests {
    use super::*;
    #[test]
    fn restart_preserves_options_and_selected_backend() {
        let args = restart_arguments(
            [
                "--replace",
                "--wait",
                "--no-audio",
                "--headless",
                "--log-to",
                "/tmp/my log",
            ]
            .map(OsString::from),
            Some(XrBackend::OpenXR),
        );
        assert_eq!(
            args,
            [
                "--wait",
                "--no-audio",
                "--headless",
                "--log-to",
                "/tmp/my log",
                "--show",
                "--openxr"
            ]
            .map(OsString::from)
        );
        let args = restart_arguments(
            ["--show", "--openvr"].map(OsString::from),
            Some(XrBackend::OpenVR),
        );
        assert_eq!(args, ["--show", "--openvr", "--wait"].map(OsString::from));
    }
    #[test]
    fn updated_executable_and_appimage_resolve_to_live_files() {
        let root = std::env::temp_dir().join(format!("xs-restart-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let binary = root.join("wayvr-xs");
        let image = root.join("WayVR.AppImage");
        std::fs::write(&binary, b"installed").unwrap();
        std::fs::write(&image, b"image").unwrap();
        let mut deleted = binary.as_os_str().to_os_string();
        deleted.push(" (deleted)");
        assert_eq!(restart_program(Path::new(&deleted), None), binary);
        assert_eq!(restart_program(&binary, Some(image.as_os_str())), image);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn own_pid_file_is_accepted_without_replacing_another_process() {
        let path = std::env::temp_dir().join(format!("xs-pid-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, std::process::id().to_string()).unwrap();
        assert!(ensure_single_instance_at(&path, false));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn restart_exec_preserves_pid() {
        const STAGE: &str = "WAYVR_XS_REEXEC_PROBE";
        const PID: &str = "WAYVR_XS_REEXEC_PROBE_PID";
        let stage = std::env::var(STAGE).unwrap_or_default();
        if stage == "after" {
            assert_eq!(std::process::id().to_string(), std::env::var(PID).unwrap());
            return;
        }
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "restart_tests::restart_exec_preserves_pid",
            "--nocapture",
        ]);
        if stage == "before" {
            command
                .env(STAGE, "after")
                .env(PID, std::process::id().to_string());
            panic!("exec failed: {}", restart_exec(command));
        }
        let output = command.env(STAGE, "before").output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
}
