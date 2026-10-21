//! Display-space horizontal regions. Integer boundaries keep odd-width splits seamless.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRegion {
    pub start: u32,
    pub end: u32,
    pub total: u32,
}

impl ScreenRegion {
    pub const FULL: Self = Self {
        start: 0,
        end: 1,
        total: 1,
    };

    pub fn pixels(self, width: u32) -> [u32; 2] {
        let boundary = |part| (u64::from(width) * u64::from(part) / u64::from(self.total)) as u32;
        [boundary(self.start), boundary(self.end)]
    }

    pub fn uv(self, width: u32) -> [f32; 2] {
        let [start, end] = self.pixels(width.max(1));
        [
            start as f32 / width.max(1) as f32,
            end as f32 / width.max(1) as f32,
        ]
    }

    pub fn desktop_uv(self, width: u32, uv: [f32; 2]) -> [f32; 2] {
        let [start, end] = self.uv(width);
        [start + uv[0] * (end - start), uv[1]]
    }

    pub fn local_uv(self, width: u32, uv: [f32; 2]) -> Option<[f32; 2]> {
        let [start, end] = self.uv(width);
        if end <= start || uv[0] < start || uv[0] >= end || !(0.0..1.0).contains(&uv[1]) {
            return None;
        }
        Some([(uv[0] - start) / (end - start), uv[1]])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dual_5120_is_two_1440p_panels() {
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
        assert_eq!(left.pixels(5120), [0, 2560]);
        assert_eq!(right.pixels(5120), [2560, 5120]);
        assert_eq!(right.desktop_uv(5120, [0.5, 0.25]), [0.75, 0.25]);
        assert_eq!(right.local_uv(5120, [0.75, 0.25]), Some([0.5, 0.25]));
        assert_eq!(left.local_uv(5120, [0.75, 0.25]), None);
        // Exactly one panel owns the cursor on the seam.
        assert_eq!(left.local_uv(5120, [0.5, 0.5]), None);
        assert_eq!(right.local_uv(5120, [0.5, 0.5]), Some([0.0, 0.5]));
    }

    #[test]
    fn thirds_cover_every_pixel_without_overlap() {
        for width in [5120, 3840, 5119, 7] {
            let bounds: Vec<_> = (0..3)
                .map(|i| {
                    ScreenRegion {
                        start: i,
                        end: i + 1,
                        total: 3,
                    }
                    .pixels(width)
                })
                .collect();
            assert_eq!(bounds[0][0], 0);
            assert_eq!(bounds[0][1], bounds[1][0]);
            assert_eq!(bounds[1][1], bounds[2][0]);
            assert_eq!(bounds[2][1], width);
            assert_eq!(bounds.iter().map(|b| b[1] - b[0]).sum::<u32>(), width);
        }
    }

    #[test]
    fn center_layout_preserves_2560_wide_main_panel() {
        let regions = [(0, 1), (1, 3), (3, 4)].map(|(start, end)| ScreenRegion {
            start,
            end,
            total: 4,
        });
        assert_eq!(
            regions.map(|r| r.pixels(5120)),
            [[0, 1280], [1280, 3840], [3840, 5120]]
        );
    }

    #[test]
    fn local_and_desktop_coordinates_round_trip() {
        for width in [5120, 5119] {
            for i in 0..3 {
                let region = ScreenRegion {
                    start: i,
                    end: i + 1,
                    total: 3,
                };
                for x in [0.1, 0.5, 0.9] {
                    let uv = region
                        .local_uv(width, region.desktop_uv(width, [x, 0.3]))
                        .unwrap();
                    assert!((uv[0] - x).abs() < 0.00001);
                    assert_eq!(uv[1], 0.3);
                }
            }
        }
    }
}
