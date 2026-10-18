use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, LazyLock, atomic::AtomicU64},
    time::Instant,
};

use glam::{Affine2, DVec2, Vec2, vec2};
use wlx_capture::{
    WlxCapture,
    frame::{MouseMeta, Transform},
};
use wlx_common::screen_region::ScreenRegion;

use crate::{
    XrBackend,
    backend::input::{HoverResult, PointerHit, PointerMode},
    overlays::screen::capture::MyFirstDmaExporter,
    state::AppState,
    subsystem::{
        hid::{MOUSE_LEFT, MOUSE_MIDDLE, MOUSE_RIGHT, WheelDelta},
        input::InputFocus,
    },
    windowing::backend::{
        FrameMeta, OverlayBackend, OverlayEventData, RenderResources, ShouldRender, ui_transform,
    },
};
use wlx_common::{
    config::CaptureMethod,
    overlays::{BackendAttrib, BackendAttribValue, MouseTransform, StereoMode},
};

use super::capture::{ScreenPipeline, WlxCaptureIn, WlxCaptureOut, receive_callback};

static START: LazyLock<Instant> = LazyLock::new(Instant::now);
static NEXT_MOVE: AtomicU64 = AtomicU64::new(0);

fn can_move() -> bool {
    START.elapsed().as_millis() as u64 > NEXT_MOVE.load(std::sync::atomic::Ordering::Relaxed)
}

fn set_next_move(millis_from_now: u64) {
    NEXT_MOVE.store(
        START.elapsed().as_millis() as u64 + millis_from_now,
        std::sync::atomic::Ordering::Relaxed,
    );
}

pub(super) enum CaptureType {
    PipeWire,
    ScreenCopy,
    Xshm,
}

struct SharedCapture {
    capture_type: CaptureType,
    capture: Box<dyn WlxCapture<WlxCaptureIn, WlxCaptureOut>>,
    frame: Option<WlxCaptureOut>,
    generation: u64,
    last_poll: Option<usize>,
    last_request: Option<usize>,
    active: usize,
}

pub struct ScreenBackend {
    name: Arc<str>,
    shared: Rc<RefCell<SharedCapture>>,
    region: ScreenRegion,
    generation: u64,
    active: bool,
    display_width: u32,
    pipeline: Option<ScreenPipeline>,
    cur_frame: Option<WlxCaptureOut>,
    meta: Option<FrameMeta>,
    mouse_transform: Affine2,
    interaction_transform: Option<Affine2>,
    stereo: Option<StereoMode>,
    stereo_full_frame: bool,
    stereo_adjust_mouse: bool,
    pub(super) logical_pos: Vec2,
    pub(super) logical_size: Vec2,
    mouse_transform_override: MouseTransform,
    just_resumed: bool,
}

impl ScreenBackend {
    pub(super) fn new_raw(
        name: Arc<str>,
        xr_backend: XrBackend,
        capture_type: CaptureType,
        capture: Box<dyn WlxCapture<WlxCaptureIn, WlxCaptureOut>>,
    ) -> Self {
        Self {
            name,
            shared: Rc::new(RefCell::new(SharedCapture {
                capture_type,
                capture,
                frame: None,
                generation: 0,
                last_poll: None,
                last_request: None,
                active: 0,
            })),
            region: ScreenRegion::FULL,
            generation: 0,
            active: false,
            display_width: 1,
            pipeline: None,
            cur_frame: None,
            meta: None,
            mouse_transform: Affine2::ZERO,
            interaction_transform: None,
            stereo: xr_backend.is_open_xr().then_some(StereoMode::None),
            stereo_full_frame: false,
            stereo_adjust_mouse: false,
            logical_pos: Vec2::ZERO,
            logical_size: Vec2::ZERO,
            mouse_transform_override: MouseTransform::Default,
            just_resumed: false,
        }
    }

    pub(super) fn apply_mouse_transform_with_override(&mut self, transform: Transform) {
        let mut size = self.logical_size;
        let pos = self.logical_pos;

        if self.stereo_adjust_mouse
            && let Some(stereo) = self.stereo.as_ref()
        {
            match stereo {
                StereoMode::LeftRight | StereoMode::RightLeft => size.x *= 0.5,
                StereoMode::TopBottom | StereoMode::BottomTop => size.y *= 0.5,
                _ => {}
            }
        }

        self.mouse_transform = match transform {
            Transform::Normal | Transform::Undefined => {
                Affine2::from_cols(vec2(size.x, 0.), vec2(0., size.y), pos)
            }
            Transform::Rotated90 => Affine2::from_cols(
                vec2(0., size.y),
                vec2(-size.x, 0.),
                vec2(pos.x + size.x, pos.y),
            ),
            Transform::Rotated180 => Affine2::from_cols(
                vec2(-size.x, 0.),
                vec2(0., -size.y),
                vec2(pos.x + size.x, pos.y + size.y),
            ),
            Transform::Rotated270 => Affine2::from_cols(
                vec2(0., -size.y),
                vec2(size.x, 0.),
                vec2(pos.x, pos.y + size.y),
            ),
            Transform::Flipped => Affine2::from_cols(
                vec2(-size.x, 0.),
                vec2(0., size.y),
                vec2(pos.x + size.x, pos.y),
            ),
            Transform::Flipped90 => {
                Affine2::from_cols(vec2(0., size.y), vec2(size.x, 0.), vec2(pos.x, pos.y))
            }
            Transform::Flipped180 => Affine2::from_cols(
                vec2(size.x, 0.),
                vec2(0., -size.y),
                vec2(pos.x, pos.y + size.y),
            ),
            Transform::Flipped270 => Affine2::from_cols(
                vec2(0., -size.y),
                vec2(-size.x, 0.),
                vec2(pos.x + size.x, pos.y + size.y),
            ),
        };
    }
}

impl Drop for ScreenBackend {
    fn drop(&mut self) {
        self.set_capture_active(false);
    }
}

impl ScreenBackend {
    fn set_capture_active(&mut self, active: bool) {
        if self.active == active {
            return;
        }
        let mut shared = self.shared.borrow_mut();
        self.active = active;
        if active {
            if shared.active == 0 {
                shared.capture.resume();
            }
            shared.active += 1;
        } else {
            shared.active -= 1;
            if shared.active == 0 {
                shared.capture.pause();
            }
        }
    }

    fn desktop_uv(&self, uv: Vec2) -> Vec2 {
        self.region
            .desktop_uv(self.display_width, uv.to_array())
            .into()
    }
}

impl OverlayBackend for ScreenBackend {
    fn screen_source(&self) -> Option<Arc<str>> {
        Some(self.name.clone())
    }

    fn screen_region(&self, region: ScreenRegion) -> Option<Box<dyn OverlayBackend>> {
        // Stereo source packing and desktop splitting are separate modes.
        if self.stereo.is_some_and(|s| s != StereoMode::None) {
            return None;
        }
        Some(Box::new(Self {
            name: self.name.clone(),
            shared: self.shared.clone(),
            region,
            generation: 0,
            active: false,
            display_width: self.display_width,
            pipeline: None,
            cur_frame: None,
            meta: None,
            mouse_transform: self.mouse_transform,
            interaction_transform: None,
            stereo: self.stereo.map(|_| StereoMode::None),
            stereo_full_frame: false,
            stereo_adjust_mouse: false,
            logical_pos: self.logical_pos,
            logical_size: self.logical_size,
            mouse_transform_override: self.mouse_transform_override,
            just_resumed: true,
        }))
    }

    fn init(&mut self, app: &mut AppState) -> anyhow::Result<()> {
        if self.region == ScreenRegion::FULL {
            if let Some(split) = app
                .session
                .config
                .screen_splits
                .iter()
                .find(|s| s.source == self.name)
            {
                app.tasks.enqueue(crate::backend::task::TaskType::Overlay(
                    crate::backend::task::OverlayTask::ScreenLayout(
                        crate::windowing::OverlaySelector::Name(self.name.clone()),
                        split.layout,
                        true,
                    ),
                ));
            }
        }
        Ok(())
    }
    fn should_render(&mut self, app: &mut AppState) -> anyhow::Result<ShouldRender> {
        self.set_capture_active(true);
        let shared_rc = self.shared.clone();
        let mut shared = shared_rc.borrow_mut();
        if !shared.capture.is_ready() {
            let supports_dmabuf =
                app.gfx.capabilities().external_memory_dma_buf && shared.capture.supports_dmbuf();

            let capture_method = app.session.config.capture_method;

            let allow_dmabuf = !matches!(
                capture_method,
                CaptureMethod::PipeWireCpu | CaptureMethod::ScreenCopyCpu
            );

            let (dmabuf_formats, dma_exporter) = if !supports_dmabuf {
                log::info!("Capture method does not support DMA-buf");
                if !app.gfx.has_capture_queue() {
                    log::warn!(
                        "Current GPU exposes only one graphics queue. Software capture will take place on the main thread. Expect degraded performance."
                    );
                }
                ([].as_slice(), None)
            } else if !allow_dmabuf {
                log::info!(
                    "Not using DMA-buf capture due to {}",
                    capture_method.as_ref()
                );
                if !app.gfx.has_capture_queue() {
                    log::warn!(
                        "Current GPU exposes only one graphics queue. Software capture will take place on the main thread. Expect degraded performance."
                    );
                }
                ([].as_slice(), None)
            } else {
                log::warn!(
                    "Using GPU capture. If you're having issues with screens, go to the Dashboard's Settings tab and switch 'Wayland capture method' to a CPU option!"
                );

                let dma_exporter = if matches!(shared.capture_type, CaptureType::ScreenCopy) {
                    Some(MyFirstDmaExporter::new(
                        app.gfx.clone(),
                        app.gfx_extras.drm_formats.clone(),
                    ))
                } else {
                    None
                };

                (&*app.gfx_extras.drm_formats, dma_exporter)
            };

            let user_data = WlxCaptureIn::new(self.name.clone(), app, dma_exporter);
            shared
                .capture
                .init(dmabuf_formats, user_data, receive_callback);
            shared.capture.request_new_frame();
            return Ok(ShouldRender::Unable);
        }

        let frame_number = crate::FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
        if shared.last_poll != Some(frame_number) {
            shared.last_poll = Some(frame_number);
            if let Some(frame) = shared.capture.receive() {
                shared.frame = Some(frame);
                shared.generation += 1;
            }
        }
        let generation = shared.generation;
        let frame = if generation != self.generation || self.just_resumed {
            shared.frame.clone()
        } else {
            None
        };
        drop(shared);

        if let Some(frame) = frame {
            self.generation = generation;
            self.just_resumed = false;
            let stereo = self.stereo.unwrap_or(StereoMode::None);
            let mut meta = frame.get_frame_meta(&app.session.config, stereo);

            if let Some(stereo) = self.stereo {
                // Apply stereo full frame logic
                if self.stereo_full_frame {
                    match stereo {
                        StereoMode::LeftRight | StereoMode::RightLeft => {
                            meta.extent[0] /= 2;
                        }
                        StereoMode::TopBottom | StereoMode::BottomTop => {
                            meta.extent[1] /= 2;
                        }
                        _ => {}
                    }
                }
            }

            let full_extent = meta.extent;
            self.display_width = full_extent[0].max(1);
            let [start, end] = self.region.pixels(self.display_width);
            meta.extent[0] = (end - start).max(1);
            let [u0, u1] = self.region.uv(self.display_width);
            // Crop in oriented display coordinates, then map to backing-image coordinates.
            let source_uv = match frame.format.transform {
                Transform::Rotated90 => [0.0, 1.0 - u1, 1.0, 1.0 - u0],
                Transform::Rotated180 | Transform::Flipped => [1.0 - u1, 0.0, 1.0 - u0, 1.0],
                Transform::Rotated270 | Transform::Flipped90 => [0.0, u0, 1.0, u1],
                Transform::Flipped270 => [0.0, 1.0 - u1, 1.0, 1.0 - u0],
                _ => [u0, 0.0, u1, 1.0],
            };

            if let Some(pipeline) = self.pipeline.as_mut() {
                if self.meta.is_some_and(|old| old.extent != meta.extent)
                    || frame.format.transform != pipeline.transform()
                {
                    pipeline.set_layout(
                        app,
                        [meta.extent[0] as _, meta.extent[1] as _],
                        [0., 0.],
                        frame.format.transform,
                    )?;
                    self.interaction_transform = Some(ui_transform(meta.extent));
                }
            } else {
                let pipeline =
                    ScreenPipeline::new(&meta, app, stereo, [0., 0.], frame.format.transform)?;
                self.pipeline = Some(pipeline);
                self.interaction_transform = Some(ui_transform(meta.extent));
            }

            self.pipeline.as_mut().unwrap().set_source_uv(source_uv);
            self.meta = Some(meta);
            self.cur_frame = Some(frame);

            Ok(ShouldRender::Should)
        } else if self.cur_frame.is_some() {
            if self.just_resumed {
                self.just_resumed = false;
                Ok(ShouldRender::Should)
            } else {
                Ok(ShouldRender::Can)
            }
        } else {
            log::trace!("{}: backend ready, but no image received.", self.name);
            Ok(ShouldRender::Unable)
        }
    }
    fn render(&mut self, app: &mut AppState, rdr: &mut RenderResources) -> anyhow::Result<()> {
        // want panic; must be some if should_render was not Unable
        let capture = self.cur_frame.as_ref().unwrap();
        let image = capture.image.clone();

        // want panic; must be Some if cur_frame is also Some
        let mouse = capture.mouse.as_ref().and_then(|mouse| {
            // Metadata cursor coordinates are normalized in the backing image.
            let (x, y) = (mouse.x, mouse.y);
            let uv = match capture.format.transform {
                Transform::Rotated90 => [1.0 - y, x],
                Transform::Rotated180 => [1.0 - x, 1.0 - y],
                Transform::Rotated270 => [y, 1.0 - x],
                Transform::Flipped => [1.0 - x, y],
                Transform::Flipped90 => [y, x],
                Transform::Flipped180 => [x, 1.0 - y],
                Transform::Flipped270 => [1.0 - y, 1.0 - x],
                _ => [x, y],
            };
            self.region
                .local_uv(self.display_width, uv)
                .map(|[x, y]| MouseMeta { x, y })
        });
        self.pipeline
            .as_mut()
            .unwrap()
            .render(image, mouse.as_ref(), app, rdr)?;
        let frame_number = crate::FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
        let mut shared = self.shared.borrow_mut();
        if shared.last_request != Some(frame_number) {
            shared.last_request = Some(frame_number);
            shared.capture.request_new_frame();
        }
        Ok(())
    }
    fn pause(&mut self, _app: &mut AppState) -> anyhow::Result<()> {
        self.set_capture_active(false);
        Ok(())
    }
    fn resume(&mut self, _app: &mut AppState) -> anyhow::Result<()> {
        self.just_resumed = true;
        self.set_capture_active(true);
        Ok(())
    }
    fn frame_meta(&mut self) -> Option<FrameMeta> {
        self.meta
    }

    fn notify(&mut self, _app: &mut AppState, event_data: OverlayEventData) -> anyhow::Result<()> {
        if matches!(event_data, OverlayEventData::SettingsChanged) {
            self.just_resumed = true;
        }
        Ok(())
    }

    fn on_hover(&mut self, app: &mut AppState, hit: &PointerHit) -> HoverResult {
        #[cfg(debug_assertions)]
        log::trace!("Hover: {:?}", hit.uv);

        let pick_now = app.input_state.picking_focus.is_picking();
        if pick_now {
            app.input_state.stop_picking();
            app.hid_provider
                .set_input_focus(app.wvr_server.as_mut(), InputFocus::PhysicalScreen);
        }

        if can_move()
            && (!app.session.config.focus_follows_mouse_mode
                || app.input_state.pointers[hit.pointer].now.move_mouse)
        {
            let pos = self
                .mouse_transform
                .transform_point2(self.desktop_uv(hit.uv));
            app.hid_provider
                .inner
                .mouse_move(DVec2::new(pos.x as _, pos.y as _));
            set_next_move(app.session.config.mouse_move_interval_ms as _);
        }
        HoverResult::consume()
    }
    fn on_pointer(&mut self, app: &mut AppState, hit: &PointerHit, pressed: bool) {
        let mut btn = match hit.mode {
            PointerMode::Right => MOUSE_RIGHT,
            PointerMode::Middle => MOUSE_MIDDLE,
            _ => MOUSE_LEFT,
        };

        // Swap left and right buttons if left-handed mode is enabled
        if app.session.config.left_handed_mouse {
            btn = match btn {
                MOUSE_LEFT => MOUSE_RIGHT,
                MOUSE_RIGHT => MOUSE_LEFT,
                other => other,
            };
        }

        if pressed {
            set_next_move(app.session.config.click_freeze_time_ms as _);
        }

        // Position first: the hover throttle may still leave the OS cursor on
        // another panel when a trigger press arrives.
        if pressed {
            let pos = self
                .mouse_transform
                .transform_point2(self.desktop_uv(hit.uv));
            app.hid_provider
                .inner
                .mouse_move(DVec2::new(pos.x as _, pos.y as _));
        }
        app.hid_provider.inner.send_button(btn, pressed);
    }

    fn on_scroll(&mut self, app: &mut AppState, hit: &PointerHit, mut delta: WheelDelta) {
        // Hover motion can be throttled or frozen after a click. Position the
        // cursor in this region before scrolling, including with two pointers.
        let pos = self
            .mouse_transform
            .transform_point2(self.desktop_uv(hit.uv));
        app.hid_provider
            .inner
            .mouse_move(DVec2::new(pos.x as _, pos.y as _));
        // convert to v120 value; maximum deflect scrolls at half speed
        delta.x *= 60.0;
        delta.y *= 60.0;
        app.hid_provider.inner.wheel(delta);
    }

    fn on_left(&mut self, _app: &mut AppState, _hand: usize) {}

    fn get_interaction_transform(&mut self) -> Option<Affine2> {
        self.interaction_transform
    }
    fn get_attrib(&self, attrib: BackendAttrib) -> Option<BackendAttribValue> {
        match attrib {
            BackendAttrib::Stereo if self.region == ScreenRegion::FULL => {
                self.stereo.map(BackendAttribValue::Stereo)
            }
            BackendAttrib::MouseTransform => Some(BackendAttribValue::MouseTransform(
                self.mouse_transform_override,
            )),
            BackendAttrib::StereoFullFrame => {
                Some(BackendAttribValue::StereoFullFrame(self.stereo_full_frame))
            }
            BackendAttrib::StereoAdjustMouse => Some(BackendAttribValue::StereoAdjustMouse(
                self.stereo_adjust_mouse,
            )),
            _ => None,
        }
    }
    fn set_attrib(&mut self, _app: &mut AppState, value: BackendAttribValue) -> bool {
        match value {
            BackendAttribValue::Stereo(new) if self.region == ScreenRegion::FULL => {
                if let Some(stereo) = self.stereo.as_mut() {
                    log::debug!("{}: stereo: {stereo:?} → {new:?}", self.name);
                    *stereo = new;
                    self.just_resumed = true;
                    if let Some(pipeline) = self.pipeline.as_mut() {
                        pipeline.ensure_stereo(new);
                    }
                    true
                } else {
                    false
                }
            }
            BackendAttribValue::MouseTransform(new) => {
                self.mouse_transform_override = new;
                let frame_transform = mouse_transform_to_transform(new);
                self.apply_mouse_transform_with_override(frame_transform);
                true
            }
            BackendAttribValue::StereoFullFrame(new) => {
                self.stereo_full_frame = new;
                self.just_resumed = true;
                true
            }
            BackendAttribValue::StereoAdjustMouse(new) => {
                self.stereo_adjust_mouse = new;
                let frame_transform = mouse_transform_to_transform(self.mouse_transform_override);
                self.apply_mouse_transform_with_override(frame_transform);
                true
            }
            _ => false,
        }
    }
}

const fn mouse_transform_to_transform(mouse_transform: MouseTransform) -> Transform {
    match mouse_transform {
        MouseTransform::Default => Transform::Undefined,
        MouseTransform::Normal => Transform::Normal,
        MouseTransform::Rotated90 => Transform::Rotated90,
        MouseTransform::Rotated180 => Transform::Rotated180,
        MouseTransform::Rotated270 => Transform::Rotated270,
        MouseTransform::Flipped => Transform::Flipped,
        MouseTransform::Flipped90 => Transform::Flipped90,
        MouseTransform::Flipped180 => Transform::Flipped180,
        MouseTransform::Flipped270 => Transform::Flipped270,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct MockCapture {
        pauses: Rc<Cell<usize>>,
        resumes: Rc<Cell<usize>>,
    }

    impl WlxCapture<WlxCaptureIn, WlxCaptureOut> for MockCapture {
        fn init(
            &mut self,
            _: &[wlx_capture::DrmFormat],
            _: WlxCaptureIn,
            _: fn(&WlxCaptureIn, wlx_capture::frame::WlxFrame) -> Option<WlxCaptureOut>,
        ) {
        }
        fn is_ready(&self) -> bool {
            true
        }
        fn supports_dmbuf(&self) -> bool {
            false
        }
        fn receive(&mut self) -> Option<WlxCaptureOut> {
            None
        }
        fn request_new_frame(&mut self) {}
        fn pause(&mut self) {
            self.pauses.set(self.pauses.get() + 1);
        }
        fn resume(&mut self) {
            self.resumes.set(self.resumes.get() + 1);
        }
    }

    #[test]
    fn region_input_respects_desktop_scaling_and_negative_monitor_origin() {
        let mut source = ScreenBackend::new_raw(
            "DP-1".into(),
            XrBackend::OpenXR,
            CaptureType::PipeWire,
            Box::new(MockCapture {
                pauses: Rc::new(Cell::new(0)),
                resumes: Rc::new(Cell::new(0)),
            }),
        );
        // 5120 physical pixels at 200% desktop scaling, to the left of
        // the primary monitor and with a vertical desktop offset.
        source.display_width = 5120;
        source.logical_pos = vec2(-2560.0, 120.0);
        source.logical_size = vec2(2560.0, 720.0);
        source.apply_mouse_transform_with_override(Transform::Undefined);
        let panel = source
            .screen_region(ScreenRegion {
                start: 1,
                end: 2,
                total: 2,
            })
            .unwrap();
        let panel = (panel as Box<dyn std::any::Any>)
            .downcast::<ScreenBackend>()
            .ok()
            .unwrap();
        let desktop = panel
            .mouse_transform
            .transform_point2(panel.desktop_uv(vec2(0.5, 0.5)));
        assert_eq!(desktop, vec2(-640.0, 480.0));
    }

    #[test]
    fn hiding_or_dropping_one_panel_keeps_sibling_capture_alive() {
        let pauses = Rc::new(Cell::new(0));
        let resumes = Rc::new(Cell::new(0));
        let mut source = ScreenBackend::new_raw(
            "DP-1".into(),
            XrBackend::OpenXR,
            CaptureType::PipeWire,
            Box::new(MockCapture {
                pauses: pauses.clone(),
                resumes: resumes.clone(),
            }),
        );
        let panel = source
            .screen_region(ScreenRegion {
                start: 1,
                end: 2,
                total: 2,
            })
            .unwrap();
        let mut panel = (panel as Box<dyn std::any::Any>)
            .downcast::<ScreenBackend>()
            .ok()
            .unwrap();
        assert!(Rc::ptr_eq(&source.shared, &panel.shared));
        source.set_capture_active(true);
        panel.set_capture_active(true);
        assert_eq!(resumes.get(), 1);
        source.set_capture_active(false);
        assert_eq!(pauses.get(), 0);
        drop(source);
        assert_eq!(pauses.get(), 0);
        panel.set_capture_active(false);
        assert_eq!(pauses.get(), 1);
        panel.set_capture_active(true);
        assert_eq!(resumes.get(), 2);
        drop(panel);
        assert_eq!(pauses.get(), 2);
    }
}
