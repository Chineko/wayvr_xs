use glam::{Affine2, Affine3A, Quat, Vec2, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PanelGeometry {
    pub content: [u32; 2],
    pub full: [u32; 2],
}
impl PanelGeometry {
    pub fn new(content: Option<[u32; 2]>, expanded: bool) -> Self {
        let content = content.unwrap_or([1280, 0]);
        let width = content[0].max(1);
        let controls = if expanded { 792 } else { 72 };
        let extra = ((u64::from(width) * controls).div_ceil(1280)) as u32;
        Self {
            content,
            full: [width, content[1].saturating_add(extra)],
        }
    }
    /// Normalize to a fixed physical width and put the controls below the
    /// content center, so expanding the chooser does not move the app itself.
    pub fn view_transform(self) -> Affine3A {
        let width = self.full[0] as f32;
        let major = (self.full[1] as f32 / width).max(1.);
        let offset = if self.content[1] == 0 {
            0.
        } else {
            -((self.full[1] - self.content[1]) as f32) / (2. * width)
        };
        Affine3A::from_scale_rotation_translation(
            Vec3::splat(major),
            Quat::IDENTITY,
            Vec3::new(0., offset, 0.),
        )
    }
    pub fn content_uv(self, uv: Vec2) -> Option<Vec2> {
        if !uv.is_finite() || uv.min_element() < 0. || uv.max_element() > 1. || self.content[1] == 0
        {
            return None;
        }
        if uv.y >= self.content[1] as f32 / self.full[1] as f32 {
            return None;
        }
        let y = uv.y * self.full[1] as f32 / self.content[1] as f32;
        (y < 1.).then_some(Vec2::new(uv.x, y))
    }
    pub fn interaction_pose(self) -> Affine3A {
        // Ray casting uses width units, whereas presentation normalizes to the
        // longest image edge. Keep the cylinder radius tied to the app width.
        Affine3A::from_translation(self.view_transform().translation.into())
    }
    pub fn interaction_transform(self) -> Affine2 {
        Affine2::from_scale_angle_translation(
            Vec2::new(1., -(self.full[0] as f32 / self.full[1] as f32)),
            0.,
            Vec2::splat(0.5),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controller_rays_match_presented_pixels_and_controls() {
        use crate::windowing::raycast_overlay;
        for content in [[1920, 1080], [400, 1000]] {
            for expanded in [false, true] {
                let g = PanelGeometry::new(Some(content), expanded);
                for curvature in [None, Some(0.2)] {
                    for uv in [Vec2::new(0.1, 0.1), Vec2::splat(0.5), Vec2::new(0.8, 0.95)] {
                        let x = uv.x - 0.5;
                        let y = (0.5 - uv.y) * g.full[1] as f32 / g.full[0] as f32;
                        let (x, z) = curvature.map_or((x, 0.), |c| {
                            let radius = 1. / (std::f32::consts::TAU * c);
                            let angle = x / radius;
                            (radius * angle.sin(), radius * (1. - angle.cos()))
                        });
                        let surface = g.interaction_pose().transform_point3(Vec3::new(x, y, z));
                        let ray = Affine3A::from_translation(surface + Vec3::Z);
                        let (_, local) =
                            raycast_overlay(&ray, &g.interaction_pose(), curvature).unwrap();
                        let actual = g.interaction_transform().transform_point2(local);
                        assert!(
                            actual.abs_diff_eq(uv, 0.0001),
                            "{g:?} {curvature:?}: {actual:?} != {uv:?}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn expanding_preserves_app_width_and_center_for_both_aspects() {
        for content in [[1920, 1080], [400, 1000]] {
            for expanded in [false, true] {
                let g = PanelGeometry::new(Some(content), expanded);
                let t = g.view_transform();
                let aspect = g.full[0] as f32 / g.full[1] as f32;
                let physical_width = t.matrix3.x_axis.length() * aspect.min(1.);
                assert!((physical_width - 1.).abs() < 0.0001);
                let content_center =
                    t.translation.y + (g.full[1] - content[1]) as f32 / (2. * content[0] as f32);
                assert!(content_center.abs() < 0.0001);
            }
        }
    }
    #[test]
    fn controls_never_map_to_desktop_input() {
        for expanded in [false, true] {
            let g = PanelGeometry::new(Some([1920, 1080]), expanded);
            let center = Vec2::new(0.5, 540. / g.full[1] as f32);
            assert!(
                g.content_uv(center)
                    .unwrap()
                    .abs_diff_eq(Vec2::splat(0.5), 0.0001)
            );
            assert!(g.content_uv(Vec2::new(0.5, 1.)).is_none());
            assert!(
                g.content_uv(Vec2::new(0.5, 1080. / g.full[1] as f32))
                    .is_none()
            );
            assert!(g.content_uv(Vec2::NAN).is_none());
        }
        assert!(
            PanelGeometry::new(None, true)
                .content_uv(Vec2::splat(0.5))
                .is_none()
        );
    }
}
