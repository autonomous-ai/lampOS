//! The picture a camera that has turned sees: the facing picture warped by the
//! homography `H = K M K^-1` (exact for a camera that only rotates and a scene
//! far away), black where the turned view reaches beyond the picture, and faded
//! to black as the turn passes the lens's field of view. Pure; pixels in, pixels out.
use crate::pose::Mat3;

/// The lens's horizontal field of view, degrees: measured on lamp-4ace (the gaze
/// table, docs/body.md: about 128), not the datasheet's 78 (docs/hardware.md).
/// A lens this wide is not a pinhole: the warp is only good near the middle.
pub const FIELD_OF_VIEW: f64 = 128.0;
/// The turn at which the picture starts to fade, degrees, and where it is black.
pub const FADE_FROM: f64 = 60.0;
pub const BLACK_AT: f64 = 90.0;

/// An RGB picture, 3 bytes a pixel.
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
}

/// How much of the picture is left after the camera turned by `degrees` off the
/// person: all of it up to [`FADE_FROM`], none from [`BLACK_AT`].
pub fn fade(degrees: f64) -> f64 {
    ((BLACK_AT - degrees) / (BLACK_AT - FADE_FROM)).clamp(0.0, 1.0)
}

/// The angle in degrees between the optical axes, from the relative rotation `m`.
pub fn turn_of(m: &Mat3) -> f64 {
    m[2][2].clamp(-1.0, 1.0).acos().to_degrees()
}

/// Where the pixel `(x, y)` of the turned picture comes from in the facing one, for a
/// picture `width` by `height`; none if it comes from behind the camera.
pub fn source_of(m: &Mat3, x: f64, y: f64, width: usize, height: usize) -> Option<(f64, f64)> {
    let f = (width as f64 / 2.0) / (FIELD_OF_VIEW.to_radians() / 2.0).tan();
    let (cx, cy) = (width as f64 / 2.0, height as f64 / 2.0);
    let d = [(x - cx) / f, (y - cy) / f, 1.0];
    let r = [
        m[0][0] * d[0] + m[0][1] * d[1] + m[0][2] * d[2],
        m[1][0] * d[0] + m[1][1] * d[1] + m[1][2] * d[2],
        m[2][0] * d[0] + m[2][1] * d[1] + m[2][2] * d[2],
    ];
    (r[2] > 1e-6).then(|| (r[0] / r[2] * f + cx, r[1] / r[2] * f + cy))
}

/// `facing` seen after the camera turned by `m` (see `pose::relative`).
pub fn turned(facing: &Picture, m: &Mat3) -> Picture {
    let (w, h) = (facing.width, facing.height);
    let f = (w as f64 / 2.0) / (FIELD_OF_VIEW.to_radians() / 2.0).tan();
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let keep = fade(turn_of(m));
    let mut rgb = vec![0u8; w * h * 3];
    if keep > 0.0 {
        for y in 0..h {
            for x in 0..w {
                // The ray of this output pixel, in the camera now, carried into the facing camera.
                let d = [(x as f64 + 0.5 - cx) / f, (y as f64 + 0.5 - cy) / f, 1.0];
                let r = [
                    m[0][0] * d[0] + m[0][1] * d[1] + m[0][2] * d[2],
                    m[1][0] * d[0] + m[1][1] * d[1] + m[1][2] * d[2],
                    m[2][0] * d[0] + m[2][1] * d[1] + m[2][2] * d[2],
                ];
                if r[2] <= 1e-6 {
                    continue;
                }
                let (sx, sy) = (r[0] / r[2] * f + cx - 0.5, r[1] / r[2] * f + cy - 0.5);
                if sx < 0.0 || sy < 0.0 || sx > (w - 1) as f64 || sy > (h - 1) as f64 {
                    continue;
                }
                let (x0, y0) = (sx.floor() as usize, sy.floor() as usize);
                let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
                let (tx, ty) = (sx - x0 as f64, sy - y0 as f64);
                for c in 0..3 {
                    let at = |px: usize, py: usize| f64::from(facing.rgb[(py * w + px) * 3 + c]);
                    let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
                    let bottom = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
                    rgb[(y * w + x) * 3 + c] =
                        ((top * (1.0 - ty) + bottom * ty) * keep).round() as u8;
                }
            }
        }
    }
    Picture {
        width: w,
        height: h,
        rgb,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient() -> Picture {
        let (width, height) = (64, 36);
        let mut rgb = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgb.extend([(x * 4) as u8, (y * 7) as u8, 200]);
            }
        }
        Picture { width, height, rgb }
    }

    fn about_y(degrees: f64) -> Mat3 {
        let (s, c) = (degrees.to_radians().sin(), degrees.to_radians().cos());
        [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]]
    }

    #[test]
    fn facing_is_the_same_picture() {
        let p = gradient();
        let same = turned(&p, &about_y(0.0));
        let worst = p
            .rgb
            .iter()
            .zip(&same.rgb)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(worst <= 1, "{worst}");
    }

    #[test]
    fn thirty_degrees_is_the_picture_moved_across_and_still_there() {
        let p = gradient();
        let t = turned(&p, &about_y(30.0));
        let lit = t.rgb.chunks(3).filter(|px| px[2] > 100).count();
        let share = lit as f64 / (t.width * t.height) as f64;
        assert!((0.15..0.9).contains(&share), "{share}");
        assert_ne!(t.rgb, p.rgb);
    }

    #[test]
    fn well_past_the_field_of_view_it_is_black() {
        let p = gradient();
        for degrees in [95.0, 120.0, 175.0] {
            let t = turned(&p, &about_y(degrees));
            assert!(t.rgb.iter().all(|b| *b == 0), "{degrees}");
        }
    }

    #[test]
    fn it_fades_between_sixty_and_ninety_degrees() {
        assert_eq!(fade(10.0), 1.0);
        assert_eq!(fade(60.0), 1.0);
        assert!((fade(75.0) - 0.5).abs() < 1e-9);
        assert_eq!(fade(90.0), 0.0);
        assert_eq!(fade(120.0), 0.0);
    }
}
