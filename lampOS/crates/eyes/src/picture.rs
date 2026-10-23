//! A camera frame as the detector takes it: decoded, shrunk to fit, in planes.
use zune_jpeg::{
    JpegDecoder,
    zune_core::{colorspace::ColorSpace, options::DecoderOptions},
};

/// The detector's input, a multiple of 32 each way: a 320 by 240 frame with
/// sixteen rows to spare.
pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 256;
/// The largest frame decoded at all, either way.
const MAX_SIDE: usize = 4_096;

/// Blue, green and red planes of `WIDTH` by `HEIGHT`, each value 0 to 255,
/// with the frame in the top left corner and the rest black.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub planes: Vec<f32>,
    /// How much of the planes the frame fills, in pixels across and down.
    pub filled: (usize, usize),
}

impl Picture {
    /// From red, green and blue bytes, row by row. The frame is shrunk to
    /// fit, never enlarged, and keeps its shape.
    pub fn from_rgb(rgb: &[u8], width: usize, height: usize) -> Result<Self, &'static str> {
        if width == 0
            || height == 0
            || width > MAX_SIDE
            || height > MAX_SIDE
            || rgb.len() != width * height * 3
        {
            return Err("a frame of no size, too large, or not three bytes a pixel");
        }
        let scale = fit_scale(width, height);
        let filled = (
            ((width as f32 * scale) as usize).clamp(1, WIDTH),
            ((height as f32 * scale) as usize).clamp(1, HEIGHT),
        );
        let mut planes = vec![0.0f32; 3 * WIDTH * HEIGHT];
        for y in 0..filled.1 {
            let from_y = (((y as f32 + 0.5) / scale) as usize).min(height - 1);
            for x in 0..filled.0 {
                let from_x = (((x as f32 + 0.5) / scale) as usize).min(width - 1);
                let at = (from_y * width + from_x) * 3;
                for plane in 0..3 {
                    planes[plane * WIDTH * HEIGHT + y * WIDTH + x] = f32::from(rgb[at + 2 - plane]);
                }
            }
        }
        Ok(Self { planes, filled })
    }

    /// From one JPEG, as the camera gives them.
    pub fn from_jpeg(bytes: &[u8]) -> Result<Self, String> {
        Self::from_jpeg_full(bytes).map(|(picture, _)| picture)
    }

    /// As [`Picture::from_jpeg`], with the decoded frame kept at its own
    /// size beside it: the decode's own buffer, held for the look, never
    /// copied or written anywhere (`LAMP_FACE_FULL_CROP`, `who::align_full`).
    pub fn from_jpeg_full(bytes: &[u8]) -> Result<(Self, Frame), String> {
        let options = DecoderOptions::default()
            .set_max_width(MAX_SIDE)
            .set_max_height(MAX_SIDE)
            .jpeg_set_out_colorspace(ColorSpace::RGB);
        let mut decoder = JpegDecoder::new_with_options(bytes, options);
        let rgb = decoder
            .decode()
            .map_err(|error| format!("jpeg: {error:?}"))?;
        let (width, height) = decoder.dimensions().ok_or("a jpeg without a size")?;
        let picture = Self::from_rgb(&rgb, width, height)?;
        let frame = Frame {
            rgb,
            width,
            height,
            scale: fit_scale(width, height),
        };
        Ok((picture, frame))
    }
}

/// The decoded frame at its own size: red, green and blue bytes, row by row,
/// and the scale its [`Picture`] was shrunk by.
pub struct Frame {
    pub rgb: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub scale: f32,
}

/// How much a frame this size is shrunk to fit, never enlarged.
fn fit_scale(width: usize, height: usize) -> f32 {
    (WIDTH as f32 / width as f32)
        .min(HEIGHT as f32 / height as f32)
        .min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_camera_frame_keeps_every_pixel_and_the_spare_rows_are_black() {
        // 320 by 240, red rising across and blue rising down.
        let mut rgb = Vec::new();
        for y in 0..240u32 {
            for x in 0..320u32 {
                rgb.extend([(x % 256) as u8, 7, (y % 256) as u8]);
            }
        }
        let picture = Picture::from_rgb(&rgb, 320, 240).unwrap();
        assert_eq!(picture.filled, (320, 240));
        let plane = WIDTH * HEIGHT;
        // Blue first, red last.
        assert_eq!(picture.planes[100 * WIDTH + 30], 100.0);
        assert_eq!(picture.planes[plane + 100 * WIDTH + 30], 7.0);
        assert_eq!(picture.planes[2 * plane + 100 * WIDTH + 30], 30.0);
        assert!(
            picture.planes[240 * WIDTH..plane]
                .iter()
                .all(|value| *value == 0.0)
        );
    }

    #[test]
    fn a_larger_frame_is_shrunk_to_fit_and_keeps_its_shape() {
        let rgb = vec![200u8; 1_280 * 720 * 3];
        let picture = Picture::from_rgb(&rgb, 1_280, 720).unwrap();
        assert_eq!(picture.filled, (320, 180));
        assert_eq!(picture.planes[179 * WIDTH + 319], 200.0);
        assert_eq!(picture.planes[180 * WIDTH], 0.0);
        // A small one is not enlarged.
        let picture = Picture::from_rgb(&[9u8; 16 * 8 * 3], 16, 8).unwrap();
        assert_eq!(picture.filled, (16, 8));
    }

    #[test]
    fn what_is_not_a_frame_is_refused() {
        assert!(Picture::from_rgb(&[0u8; 10], 2, 2).is_err());
        assert!(Picture::from_rgb(&[], 0, 0).is_err());
        assert!(Picture::from_jpeg(b"not a jpeg").is_err());
        assert!(Picture::from_jpeg(&[]).is_err());
    }
}
