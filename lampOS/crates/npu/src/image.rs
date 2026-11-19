//! A JPEG made into the model's frame: decoded, scaled to fit the model's
//! input without changing its shape (bilinear), centred on black, and the
//! mapping back to the picture's own pixels kept. With the picture's shape
//! the model's (640x480 for 640x480), nothing is padded.
use crate::engine::Input;
use zune_jpeg::{
    JpegDecoder,
    zune_core::{colorspace::ColorSpace, options::DecoderOptions},
};

/// The longest side of a picture decoded.
pub const MOST_SIDE: usize = 4096;

/// A decoded picture: red, green and blue bytes, row by row.
pub struct Picture {
    pub rgb: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

/// Decodes `jpeg`.
pub fn decode(jpeg: &[u8]) -> Result<Picture, String> {
    let options = DecoderOptions::default()
        .set_max_width(MOST_SIDE)
        .set_max_height(MOST_SIDE)
        .jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(jpeg, options);
    let rgb = decoder
        .decode()
        .map_err(|error| format!("not a JPEG this lamp reads: {error:?}"))?;
    let (width, height) = decoder.dimensions().ok_or("a JPEG without a size")?;
    if width == 0 || height == 0 || rgb.len() != width * height * 3 {
        return Err("a JPEG of no picture".to_owned());
    }
    Ok(Picture { rgb, width, height })
}

/// How a point in the model's frame maps back to the picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub scale: f32,
    pub pad_x: f32,
    pub pad_y: f32,
    pub width: usize,
    pub height: usize,
}

impl Fit {
    /// The picture's pixel for the frame's `(x, y)`, kept inside it.
    pub fn back(&self, x: f32, y: f32) -> (f32, f32) {
        let x = ((x - self.pad_x) / self.scale).clamp(0.0, self.width as f32);
        let y = ((y - self.pad_y) / self.scale).clamp(0.0, self.height as f32);
        (x, y)
    }
}

/// `picture` made into one frame for `input`, and how to map back.
pub fn fit(picture: &Picture, input: Input) -> (Vec<u8>, Fit) {
    let (w, h) = (input.width, input.height);
    let scale = (w as f32 / picture.width as f32).min(h as f32 / picture.height as f32);
    let new_w = ((picture.width as f32 * scale).round() as usize).clamp(1, w);
    let new_h = ((picture.height as f32 * scale).round() as usize).clamp(1, h);
    let pad_x = (w - new_w) / 2;
    let pad_y = (h - new_h) / 2;
    let mut frame = vec![0u8; input.bytes()];
    let plane = w * h;
    let same = new_w == picture.width && new_h == picture.height;
    // The source pixel's centre for a frame pixel, as OpenCV's resize.
    let sx = picture.width as f32 / new_w as f32;
    let sy = picture.height as f32 / new_h as f32;
    for y in 0..new_h {
        let fy = ((y as f32 + 0.5) * sy - 0.5).max(0.0);
        let y0 = (fy as usize).min(picture.height - 1);
        let y1 = (y0 + 1).min(picture.height - 1);
        let wy = fy - y0 as f32;
        for x in 0..new_w {
            let mut pixel = [0u8; 3];
            if same {
                let at = (y * picture.width + x) * 3;
                pixel.copy_from_slice(&picture.rgb[at..at + 3]);
            } else {
                let fx = ((x as f32 + 0.5) * sx - 0.5).max(0.0);
                let x0 = (fx as usize).min(picture.width - 1);
                let x1 = (x0 + 1).min(picture.width - 1);
                let wx = fx - x0 as f32;
                for (c, value) in pixel.iter_mut().enumerate() {
                    let at = |xx: usize, yy: usize| {
                        f32::from(picture.rgb[(yy * picture.width + xx) * 3 + c])
                    };
                    let top = at(x0, y0) * (1.0 - wx) + at(x1, y0) * wx;
                    let bottom = at(x0, y1) * (1.0 - wx) + at(x1, y1) * wx;
                    *value = (top * (1.0 - wy) + bottom * wy).round().clamp(0.0, 255.0) as u8;
                }
            }
            let (fx, fy) = (x + pad_x, y + pad_y);
            if input.planar {
                for (c, value) in pixel.iter().enumerate() {
                    frame[c * plane + fy * w + fx] = *value;
                }
            } else {
                let at = (fy * w + fx) * 3;
                frame[at..at + 3].copy_from_slice(&pixel);
            }
        }
    }
    (
        frame,
        Fit {
            scale: new_w as f32 / picture.width as f32,
            pad_x: pad_x as f32,
            pad_y: pad_y as f32,
            width: picture.width,
            height: picture.height,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(width: usize, height: usize) -> Picture {
        let rgb = (0..width * height)
            .flat_map(|i| [(i % 256) as u8, 7, 200])
            .collect();
        Picture { rgb, width, height }
    }

    #[test]
    fn a_picture_of_the_models_size_goes_in_as_it_is() {
        let p = picture(8, 6);
        let input = Input {
            width: 8,
            height: 6,
            planar: false,
        };
        let (frame, fit) = fit(&p, input);
        assert_eq!(frame, p.rgb);
        assert_eq!(fit.back(4.0, 3.0), (4.0, 3.0));
    }

    #[test]
    fn a_wide_picture_is_centred_in_a_square_frame_and_maps_back() {
        let p = picture(640, 480);
        let input = Input {
            width: 192,
            height: 192,
            planar: false,
        };
        let (frame, fit) = fit(&p, input);
        assert_eq!(frame.len(), 192 * 192 * 3);
        // 640x480 at 0.3 is 192x144: 24 rows of black above and below.
        assert_eq!((fit.pad_x, fit.pad_y), (0.0, 24.0));
        assert!(frame[..192 * 24 * 3].iter().all(|&b| b == 0));
        assert_eq!(frame[192 * 24 * 3 + 1], 7);
        let (x, y) = fit.back(96.0, 96.0);
        assert!(
            (x - 320.0).abs() < 0.01 && (y - 240.0).abs() < 0.01,
            "{x} {y}"
        );
        // A point in the padding is kept on the picture.
        assert_eq!(fit.back(0.0, 0.0), (0.0, 0.0));
    }

    #[test]
    fn a_planar_frame_puts_each_colour_in_its_plane() {
        let p = picture(4, 4);
        let input = Input {
            width: 4,
            height: 4,
            planar: true,
        };
        let (frame, _) = fit(&p, input);
        assert!(frame[16..32].iter().all(|&b| b == 7));
        assert!(frame[32..48].iter().all(|&b| b == 200));
    }

    #[test]
    fn a_jpeg_decodes_and_anything_else_is_refused() {
        let mut jpeg = Vec::new();
        let rgb = vec![120u8; 32 * 24 * 3];
        jpeg_encoder::Encoder::new(&mut jpeg, 90)
            .encode(&rgb, 32, 24, jpeg_encoder::ColorType::Rgb)
            .unwrap();
        let p = decode(&jpeg).unwrap();
        assert_eq!((p.width, p.height), (32, 24));
        assert!(decode(b"not a jpeg").is_err());
    }
}
