use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::{CString, c_char, c_void},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};
use windows::{Win32::Graphics::Direct3D11::ID3D11Texture2D, core::Interface};

unsafe extern "C" {
    fn xs_vr_open(path: *const u16) -> i32;
    fn xs_vr_close();
    fn xs_vr_adapter() -> i32;
    fn xs_vr_head(matrix: *mut f32) -> bool;
    fn xs_vr_quit() -> bool;
    fn xs_vr_create(key: *const c_char, title: *const c_char, handle: *mut u64) -> i32;
    fn xs_vr_destroy(handle: u64);
    fn xs_vr_show(handle: u64, visible: bool) -> i32;
    fn xs_vr_clear(handle: u64);
    fn xs_vr_place(handle: u64, width: f32, matrix: *const f32) -> i32;
    fn xs_vr_submit(handle: u64, texture: *mut c_void, u_min: f32, u_max: f32) -> i32;
}

fn check(code: i32, action: &str) -> Result<()> {
    ensure!(code == 0, "{action}: OpenVR error {code}");
    Ok(())
}

pub fn find_dll(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return path.canonicalize().context("Find --openvr-dll");
    }
    let local = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is missing")?;
    let paths = PathBuf::from(local).join("openvr/openvrpaths.vrpath");
    let data: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&paths).with_context(|| {
            format!(
                "Read {}. Install and launch SteamVR first, or pass --openvr-dll",
                paths.display()
            )
        })?)?;
    if let Some(runtimes) = data["runtime"].as_array() {
        for runtime in runtimes.iter().filter_map(|v| v.as_str()) {
            let dll = Path::new(runtime).join("bin/win64/openvr_api.dll");
            if dll.is_file() {
                return dll.canonicalize().context("Resolve SteamVR DLL");
            }
        }
    }
    bail!(
        "No SteamVR win64 runtime in {}. Launch SteamVR or pass --openvr-dll",
        paths.display()
    )
}

pub struct Runtime {
    handles: Vec<u64>,
    _single_thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl Runtime {
    pub fn open(dll: &Path) -> Result<Self> {
        let path: Vec<_> = dll.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: the absolute, NUL-terminated path is valid for this synchronous call.
        let code = unsafe { xs_vr_open(path.as_ptr()) };
        ensure!(
            code == 0,
            "Initialize SteamVR overlay: error {code}. Start/update SteamVR and connect the headset. Loader errors: -1 DLL load, -2 exports, -3 interfaces, -4 already initialized."
        );
        Ok(Self {
            handles: Vec::new(),
            _single_thread: std::marker::PhantomData,
        })
    }

    pub fn adapter(&self) -> i32 {
        unsafe { xs_vr_adapter() }
    }
    pub fn quitting(&self) -> bool {
        unsafe { xs_vr_quit() }
    }
    pub fn head(&self) -> Option<[f32; 12]> {
        let mut head = [0.0; 12];
        if unsafe { xs_vr_head(head.as_mut_ptr()) } && head.iter().all(|v| v.is_finite()) {
            Some(head)
        } else {
            None
        }
    }

    pub fn rebuild(&mut self, count: usize) -> Result<()> {
        self.destroy();
        for i in 0..count {
            let key = CString::new(format!("wayvr.xs.windows.panel.{i}"))?;
            let title = CString::new(format!("WayVR XS Windows {}", i + 1))?;
            let mut handle = 0;
            check(
                unsafe { xs_vr_create(key.as_ptr(), title.as_ptr(), &mut handle) },
                "Create overlay (close another Windows XS instance if already running)",
            )?;
            self.handles.push(handle);
        }
        Ok(())
    }
    pub fn clear(&self) {
        for &h in &self.handles {
            unsafe {
                xs_vr_clear(h);
            }
        }
    }
    pub fn show(&self, visible: &[bool]) -> Result<()> {
        ensure!(
            visible.len() == self.handles.len(),
            "Overlay/visibility count mismatch"
        );
        for (&h, &visible) in self.handles.iter().zip(visible) {
            check(unsafe { xs_vr_show(h, visible) }, "Show/hide overlay")?;
        }
        Ok(())
    }
    pub fn place(&self, index: usize, width: f32, matrix: &[f32; 12]) -> Result<()> {
        check(
            unsafe { xs_vr_place(self.handles[index], width, matrix.as_ptr()) },
            "Position overlay",
        )
    }
    pub fn submit(&self, texture: &ID3D11Texture2D, bounds: &[[f32; 2]]) -> Result<()> {
        ensure!(
            bounds.len() == self.handles.len(),
            "Overlay/region count mismatch"
        );
        for (&h, uv) in self.handles.iter().zip(bounds) {
            // Texture is app-owned, copy-complete, and retained through all calls and until clear/destroy.
            check(
                unsafe { xs_vr_submit(h, texture.as_raw(), uv[0], uv[1]) },
                "Submit D3D11 desktop texture",
            )?;
        }
        Ok(())
    }
    fn destroy(&mut self) {
        for h in self.handles.drain(..) {
            unsafe {
                xs_vr_destroy(h);
            }
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.destroy();
        unsafe {
            xs_vr_close();
        }
    }
}
