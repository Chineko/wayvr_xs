//! Physical-pixel mapping for a DPI-aware Windows desktop backend.
use crate::screen_region::ScreenRegion;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DesktopRect {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

impl DesktopRect {
    /// Map panel-local UV into the captured monitor's physical desktop pixels.
    /// Endpoints stay inside this region, including UV=1 on the right/bottom edge.
    pub fn panel_pointer(self, region: ScreenRegion, uv: [f32; 2]) -> Option<[i32; 2]> {
        if self.width == 0
            || self.height == 0
            || region.total == 0
            || region.start >= region.end
            || region.end > region.total
            || uv
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return None;
        }
        let [start, end] = region.pixels(self.width);
        if end <= start {
            return None;
        }
        let x = start + ((f64::from(uv[0]) * f64::from(end - start)) as u32).min(end - start - 1);
        let y = ((f64::from(uv[1]) * f64::from(self.height)) as u32).min(self.height - 1);
        Some([
            i32::try_from(i64::from(self.left) + i64::from(x)).ok()?,
            i32::try_from(i64::from(self.top) + i64::from(y)).ok()?,
        ])
    }

    /// Convert physical pixels to SendInput's 0..65535 virtual-desktop range.
    /// Callers must use MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK.
    pub fn absolute_pointer(self, point: [i32; 2]) -> Option<[i32; 2]> {
        let normalize = |position: i32, origin: i32, extent: u32| {
            let offset = i64::from(position) - i64::from(origin);
            if extent == 0 || offset < 0 || offset >= i64::from(extent) {
                return None;
            }
            Some((offset * 65535 / i64::from(extent.saturating_sub(1).max(1))) as i32)
        };
        Some([
            normalize(point[0], self.left, self.width)?,
            normalize(point[1], self.top, self.height)?,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: DesktopRect = DesktopRect {
        left: -5120,
        top: -200,
        width: 5120,
        height: 1440,
    };

    #[test]
    fn right_panel_maps_into_physical_monitor_with_negative_origin() {
        let right = ScreenRegion {
            start: 1,
            end: 2,
            total: 2,
        };
        assert_eq!(MONITOR.panel_pointer(right, [0.5, 0.5]), Some([-1280, 520]));
        assert_eq!(
            MONITOR.panel_pointer(right, [0.0, 0.0]),
            Some([-2560, -200])
        );
        assert_eq!(MONITOR.panel_pointer(right, [1.0, 1.0]), Some([-1, 1239]));
    }

    #[test]
    fn panel_edges_do_not_click_a_sibling_or_the_next_monitor() {
        let left = ScreenRegion {
            start: 0,
            end: 1,
            total: 2,
        };
        let right = ScreenRegion {
            start: 1,
            end: 2,
            total: 2,
        };
        assert_eq!(MONITOR.panel_pointer(left, [1.0, 0.0]), Some([-2561, -200]));
        assert_eq!(
            MONITOR.panel_pointer(right, [0.0, 0.0]),
            Some([-2560, -200])
        );
    }

    #[test]
    fn absolute_input_uses_the_whole_virtual_desktop_and_its_origin() {
        let desktop = DesktopRect {
            left: -5120,
            top: -200,
            width: 7680,
            height: 1640,
        };
        assert_eq!(desktop.absolute_pointer([-5120, -200]), Some([0, 0]));
        assert_eq!(desktop.absolute_pointer([2559, 1439]), Some([65535, 65535]));
        assert_eq!(desktop.absolute_pointer([2560, 0]), None);
        assert_eq!(desktop.absolute_pointer([-5121, 0]), None);
    }

    #[test]
    fn invalid_or_stale_capture_coordinates_are_rejected() {
        assert_eq!(
            MONITOR.panel_pointer(ScreenRegion::FULL, [f32::NAN, 0.5]),
            None
        );
        assert_eq!(MONITOR.panel_pointer(ScreenRegion::FULL, [-0.1, 0.5]), None);
        assert_eq!(
            MONITOR.panel_pointer(
                ScreenRegion {
                    start: 0,
                    end: 1,
                    total: 0
                },
                [0.5; 2]
            ),
            None
        );
        let empty = DesktopRect {
            width: 0,
            ..MONITOR
        };
        assert_eq!(empty.panel_pointer(ScreenRegion::FULL, [0.5; 2]), None);
    }
}
