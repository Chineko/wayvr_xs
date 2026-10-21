use crate::surface::{SurfaceCache, SurfaceKey};
use anyhow::{Context, Result, bail, ensure};
use std::time::{Duration, Instant};
use windows::{
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0},
            Direct3D11::*,
            Dxgi::{Common::*, *},
            Gdi::{DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW},
        },
    },
    core::{BOOL, Interface, PCWSTR},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureColor {
    Gamma,
    Linear,
}

#[derive(Debug)]
struct UnsupportedFormat(DXGI_FORMAT);
impl std::fmt::Display for UnsupportedFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Unsupported desktop texture format {:?}; this is not a temporary device loss",
            self.0
        )
    }
}
impl std::error::Error for UnsupportedFormat {}

fn capture_color(format: DXGI_FORMAT) -> Result<CaptureColor> {
    match format {
        DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_R8G8B8A8_UNORM => Ok(CaptureColor::Gamma),
        // Windows HDR desktop duplication can return scRGB, even with DuplicateOutput.
        // Keep the FP16 texture and tell OpenVR it is linear; never byte-copy to BGRA8.
        DXGI_FORMAT_R16G16B16A16_FLOAT => Ok(CaptureColor::Linear),
        _ => Err(UnsupportedFormat(format).into()),
    }
}

/// Retry actual device/desktop interruptions, not frame format changes.
pub fn recoverable(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<windows::core::Error>()
        .is_some_and(|e| {
            matches!(
                e.code(),
                DXGI_ERROR_ACCESS_LOST
                    | DXGI_ERROR_DEVICE_REMOVED
                    | DXGI_ERROR_DEVICE_RESET
                    | DXGI_ERROR_SESSION_DISCONNECTED
                    | DXGI_ERROR_NOT_CURRENTLY_AVAILABLE
                    | windows::Win32::Foundation::E_ACCESSDENIED
            )
        })
}

pub struct Monitor {
    pub adapter: IDXGIAdapter1,
    pub output: IDXGIOutput1,
    pub adapter_index: u32,
    pub name: String,
    pub rect: windows::Win32::Foundation::RECT,
    pub active_mode: Option<[u32; 2]>,
}

pub fn monitors() -> Result<Vec<Monitor>> {
    // SAFETY: COM interfaces own their reference counts; DXGI enumeration needs no COM apartment.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
        let mut monitors = Vec::new();
        let mut ai = 0;
        loop {
            let adapter = match factory.EnumAdapters1(ai) {
                Ok(a) => a,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(e) => return Err(e.into()),
            };
            let mut oi = 0;
            loop {
                let output = match adapter.EnumOutputs(oi) {
                    Ok(o) => o,
                    Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                    Err(e) => return Err(e.into()),
                };
                let desc = output.GetDesc()?;
                if desc.AttachedToDesktop.as_bool() {
                    let end = desc
                        .DeviceName
                        .iter()
                        .position(|c| *c == 0)
                        .unwrap_or(desc.DeviceName.len());
                    monitors.push(Monitor {
                        adapter: adapter.clone(),
                        output: output.cast()?,
                        adapter_index: ai,
                        name: String::from_utf16_lossy(&desc.DeviceName[..end]),
                        rect: desc.DesktopCoordinates,
                        active_mode: {
                            let mut mode = DEVMODEW {
                                dmSize: size_of::<DEVMODEW>() as u16,
                                ..Default::default()
                            };
                            EnumDisplaySettingsW(
                                PCWSTR(desc.DeviceName.as_ptr()),
                                ENUM_CURRENT_SETTINGS,
                                &mut mode,
                            )
                            .as_bool()
                            .then_some([mode.dmPelsWidth, mode.dmPelsHeight])
                        },
                    });
                }
                oi += 1;
            }
            ai += 1;
        }
        Ok(monitors)
    }
}

pub struct Capture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplicate: Option<IDXGIOutputDuplication>,
    query: ID3D11Query,
    surfaces: SurfaceCache<ID3D11Texture2D>,
    last_surface_log: Option<Instant>,
    pub size: [u32; 2],
    pub color: CaptureColor,
}

struct Frame(IDXGIOutputDuplication);
impl Drop for Frame {
    fn drop(&mut self) {
        // SAFETY: constructed only after a successful AcquireNextFrame, released exactly once.
        unsafe {
            let _ = self.0.ReleaseFrame();
        }
    }
}

impl Capture {
    pub fn new(monitor: &Monitor) -> Result<Self> {
        unsafe {
            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(
                &monitor.adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .context("Create D3D11 device on the selected monitor's GPU")?;
            let device = device.context("D3D11 returned no device")?;
            let context = context.context("D3D11 returned no immediate context")?;
            let duplicate = monitor.output.DuplicateOutput(&device)
                .context("Start desktop duplication (unlock the desktop and close other capture tools if needed)")?;
            let desc = duplicate.GetDesc();
            ensure!(
                desc.Rotation == DXGI_MODE_ROTATION_IDENTITY,
                "Rotated monitors are not supported by this first Windows build"
            );
            let mut query = None;
            device.CreateQuery(
                &D3D11_QUERY_DESC {
                    Query: D3D11_QUERY_EVENT,
                    MiscFlags: 0,
                },
                Some(&mut query),
            )?;
            Ok(Self {
                device,
                context,
                duplicate: Some(duplicate),
                query: query.context("Missing GPU query")?,
                surfaces: SurfaceCache::default(),
                last_surface_log: None,
                size: [0; 2],
                color: CaptureColor::Gamma,
            })
        }
    }

    pub fn pause(&mut self) {
        self.duplicate = None;
        self.surfaces = SurfaceCache::default();
    }

    pub fn texture(&self) -> Option<&ID3D11Texture2D> {
        self.surfaces.texture()
    }
    pub fn clear_required(&self) -> bool {
        self.surfaces.clear_required()
    }
    /// The caller must first clear every overlay's old texture (no overlays in capture-test mode).
    pub fn overlays_cleared(&mut self) {
        self.surfaces.overlays_cleared();
    }

    /// Own the image before releasing DXGI's frame. Every overlay uses this same GPU texture.
    pub fn update(&mut self, timeout_ms: u32) -> Result<bool> {
        unsafe {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource = None;
            let duplicate = self.duplicate.as_ref().context("Capture is paused")?;
            match duplicate.AcquireNextFrame(timeout_ms, &mut info, &mut resource) {
                Ok(()) => (),
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(false),
                Err(e) => return Err(e).context("Desktop duplication interrupted"),
            }
            let _frame = Frame(duplicate.clone());
            let source: ID3D11Texture2D = resource
                .context("DXGI returned no frame resource")?
                .cast()?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            source.GetDesc(&mut desc);
            ensure!(desc.Width > 0 && desc.Height > 0, "Empty desktop texture");
            let color = capture_color(desc.Format)?;
            let key = SurfaceKey {
                size: [desc.Width, desc.Height],
                format: desc.Format,
            };
            let previous = self.surfaces.key();
            let device = &self.device;
            let changed = self.surfaces.activate(key, || {
                desc.Usage = D3D11_USAGE_DEFAULT;
                desc.BindFlags = D3D11_BIND_SHADER_RESOURCE.0 as u32;
                desc.CPUAccessFlags = 0;
                desc.MiscFlags = D3D11_RESOURCE_MISC_SHARED.0 as u32;
                let mut next = None;
                device.CreateTexture2D(&desc, None, Some(&mut next))?;
                next.context("D3D11 returned no capture texture")
            })?;
            if changed {
                self.size = key.size;
                self.color = color;
                // Some drivers alternate SDR/HDR surfaces; keep this diagnostic bounded.
                if self
                    .last_surface_log
                    .is_none_or(|t| t.elapsed() >= Duration::from_secs(5))
                {
                    println!(
                        "Capture surface: {}x{}, {:?}, {}{}",
                        self.size[0],
                        self.size[1],
                        key.format,
                        if color == CaptureColor::Linear {
                            "linear scRGB / HDR source"
                        } else {
                            "SDR gamma"
                        },
                        if previous.is_some() {
                            " (texture updated in place; capture stays connected)"
                        } else {
                            ""
                        }
                    );
                    self.last_surface_log = Some(Instant::now());
                }
            }
            self.context
                .CopyResource(self.surfaces.texture().unwrap(), &source);
            self.context.End(&self.query);
            self.context.Flush();
            // Complete the copy before releasing DXGI's borrowed resource or giving it to OpenVR.
            let started = Instant::now();
            loop {
                let mut done = BOOL(0);
                self.context.GetData(
                    &self.query,
                    Some((&mut done as *mut BOOL).cast()),
                    size_of::<BOOL>() as u32,
                    0,
                )?;
                if done.as_bool() {
                    break;
                }
                if started.elapsed() > Duration::from_secs(2) {
                    bail!("GPU copy timed out");
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdr_float_surface_uses_linear_submission_and_sdr_stays_gamma() {
        // The user's crash/recovery log reports DXGI_FORMAT(10).
        assert_eq!(
            capture_color(DXGI_FORMAT(10)).unwrap(),
            CaptureColor::Linear
        );
        assert_eq!(
            capture_color(DXGI_FORMAT_B8G8R8A8_UNORM).unwrap(),
            CaptureColor::Gamma
        );
        assert_eq!(
            capture_color(DXGI_FORMAT_R8G8B8A8_UNORM).unwrap(),
            CaptureColor::Gamma
        );
    }

    #[test]
    fn permanent_format_errors_do_not_enter_reconnect_loop() {
        let error = capture_color(DXGI_FORMAT_UNKNOWN)
            .unwrap_err()
            .context("Frame capture");
        assert!(!recoverable(&error));
        assert!(!recoverable(&anyhow::anyhow!("GPU copy timed out")));
    }

    #[test]
    fn desktop_and_device_changes_remain_recoverable_through_context() {
        for code in [
            DXGI_ERROR_ACCESS_LOST,
            DXGI_ERROR_DEVICE_REMOVED,
            DXGI_ERROR_DEVICE_RESET,
            windows::Win32::Foundation::E_ACCESSDENIED,
        ] {
            let error = anyhow::Error::new(windows::core::Error::from_hresult(code))
                .context("Frame capture");
            assert!(recoverable(&error));
        }
        assert!(!recoverable(
            &windows::core::Error::from_hresult(windows::Win32::Foundation::E_INVALIDARG).into()
        ));
    }
}
