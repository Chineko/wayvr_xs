//! Bounded texture reuse across live SDR/HDR or size changes.
//! Keep the previously published texture alive until overlays explicitly clear it.
use anyhow::{Result, ensure};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceKey {
    pub size: [u32; 2],
    pub format: DXGI_FORMAT,
}

pub struct SurfaceCache<T> {
    active: Option<(SurfaceKey, T)>,
    spare: Option<(SurfaceKey, T)>,
    clear_required: bool,
}

impl<T> Default for SurfaceCache<T> {
    fn default() -> Self {
        Self {
            active: None,
            spare: None,
            clear_required: false,
        }
    }
}

impl<T> SurfaceCache<T> {
    pub fn key(&self) -> Option<SurfaceKey> {
        self.active.as_ref().map(|(k, _)| *k)
    }
    pub fn texture(&self) -> Option<&T> {
        self.active.as_ref().map(|(_, t)| t)
    }
    pub fn clear_required(&self) -> bool {
        self.clear_required
    }

    /// Only call after all old overlay references have been cleared (or when no overlays exist).
    pub fn overlays_cleared(&mut self) {
        self.clear_required = false;
    }

    pub fn activate(
        &mut self,
        key: SurfaceKey,
        create: impl FnOnce() -> Result<T>,
    ) -> Result<bool> {
        ensure!(
            !self.clear_required,
            "Clear old overlay references before acquiring another desktop frame"
        );
        if self.key() == Some(key) {
            return Ok(false);
        }
        // Allocate before changing ownership. On failure the published texture stays alive.
        let next = if self.spare.as_ref().is_some_and(|(k, _)| *k == key) {
            self.spare.take().unwrap()
        } else {
            (key, create()?)
        };
        self.spare = self.active.replace(next);
        self.clear_required = self.spare.is_some();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};
    use windows::Win32::Graphics::Dxgi::Common::*;

    fn key(format: DXGI_FORMAT, width: u32) -> SurfaceKey {
        SurfaceKey {
            size: [width, 1440],
            format,
        }
    }

    #[test]
    fn alternating_sdr_hdr_frames_reuse_two_textures_without_reconnecting() {
        let mut cache = SurfaceCache::default();
        let allocations = Cell::new(0);
        for i in 0..100 {
            let k = key(
                if i % 2 == 0 {
                    DXGI_FORMAT_B8G8R8A8_UNORM
                } else {
                    DXGI_FORMAT_R16G16B16A16_FLOAT
                },
                5120,
            );
            assert!(
                cache
                    .activate(k, || {
                        allocations.set(allocations.get() + 1);
                        Ok(k)
                    })
                    .unwrap()
            );
            assert_eq!(cache.texture(), Some(&k));
            assert_eq!(cache.clear_required(), i != 0);
            cache.overlays_cleared();
        }
        assert_eq!(allocations.get(), 2);
    }

    #[test]
    fn old_texture_lives_until_clear_and_resize_is_not_a_capture_error() {
        struct Texture(Rc<Cell<u32>>);
        impl Drop for Texture {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let drops = Rc::new(Cell::new(0));
        let mut cache = SurfaceCache::default();
        cache
            .activate(key(DXGI_FORMAT_B8G8R8A8_UNORM, 5120), || {
                Ok(Texture(drops.clone()))
            })
            .unwrap();
        cache
            .activate(key(DXGI_FORMAT_R16G16B16A16_FLOAT, 5120), || {
                Ok(Texture(drops.clone()))
            })
            .unwrap();
        assert_eq!(drops.get(), 0);
        assert!(
            cache
                .activate(key(DXGI_FORMAT_R16G16B16A16_FLOAT, 2560), || unreachable!())
                .is_err()
        );
        cache.overlays_cleared();
        cache
            .activate(key(DXGI_FORMAT_R16G16B16A16_FLOAT, 2560), || {
                Ok(Texture(drops.clone()))
            })
            .unwrap();
        // Only the already-cleared spare was evicted; the last published texture remains alive.
        assert_eq!(drops.get(), 1);
        cache.overlays_cleared();
        drop(cache);
        assert_eq!(drops.get(), 3);
    }

    #[test]
    fn allocation_failure_keeps_the_published_surface() {
        let mut cache = SurfaceCache::default();
        let original = key(DXGI_FORMAT_B8G8R8A8_UNORM, 5120);
        cache.activate(original, || Ok(123)).unwrap();
        assert!(
            cache
                .activate(key(DXGI_FORMAT_R16G16B16A16_FLOAT, 5120), || anyhow::bail!(
                    "GPU allocation failed"
                ))
                .is_err()
        );
        assert_eq!(cache.key(), Some(original));
        assert_eq!(cache.texture(), Some(&123));
        assert!(!cache.clear_required());
    }
}
