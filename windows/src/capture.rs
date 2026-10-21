use anyhow::{Context, Result, bail, ensure};
use std::time::{Duration, Instant};
use windows::{
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0},
            Direct3D11::*,
            Dxgi::{Common::*, *},
        },
    },
    core::{BOOL, Interface},
};

pub struct Monitor {
    pub adapter: IDXGIAdapter1,
    pub output: IDXGIOutput1,
    pub adapter_index: u32,
    pub name: String,
    pub rect: windows::Win32::Foundation::RECT,
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
    pub texture: Option<ID3D11Texture2D>,
    pub size: [u32; 2],
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
                texture: None,
                size: [0; 2],
            })
        }
    }

    pub fn pause(&mut self) {
        self.duplicate = None;
        self.texture = None;
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
            ensure!(
                desc.Format == DXGI_FORMAT_B8G8R8A8_UNORM,
                "Unexpected desktop texture format: {:?}",
                desc.Format
            );
            ensure!(
                self.texture.is_none() || self.size == [desc.Width, desc.Height],
                "Desktop size changed; recreate capture before releasing its published texture"
            );
            if self.texture.is_none() {
                desc.Usage = D3D11_USAGE_DEFAULT;
                desc.BindFlags = D3D11_BIND_SHADER_RESOURCE.0 as u32;
                desc.CPUAccessFlags = 0;
                desc.MiscFlags = D3D11_RESOURCE_MISC_SHARED.0 as u32;
                self.device
                    .CreateTexture2D(&desc, None, Some(&mut self.texture))?;
                self.size = [desc.Width, desc.Height];
            }
            self.context
                .CopyResource(self.texture.as_ref().unwrap(), &source);
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
