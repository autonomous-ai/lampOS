//! A JPEG made into the model's frame: decoded, and a [`Region`] of it
//! scaled to the model's input (bilinear, black past the picture's edges),
//! with the mapping back to the picture's own pixels. The region is the
//! whole picture centred on black to the input's shape, or the head's crop
//! around the person last seen ([`crate::head::PoseHead::next`]).
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

/// The part of a picture the model is shown, normalised to the picture
/// (`0..1` across it, but it may reach past its edges, which are black):
/// MoveNet's crop region, `y0, x0, y1, x1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub y0: f32,
    pub x0: f32,
    pub y1: f32,
    pub x1: f32,
}

impl Region {
    /// The whole picture, centred on black to the input's shape: a
    /// 640x480 picture in a square frame has 80 rows of black above and
    /// below (MoveNet's `init_crop_region` for a square input).
    pub fn whole(width: usize, height: usize, input: Input) -> Self {
        let picture = width as f32 / height as f32;
        let frame = input.width as f32 / input.height as f32;
        if picture >= frame {
            let h = picture / frame;
            Self {
                y0: 0.5 - h / 2.0,
                x0: 0.0,
                y1: 0.5 + h / 2.0,
                x1: 1.0,
            }
        } else {
            let w = frame / picture;
            Self {
                y0: 0.0,
                x0: 0.5 - w / 2.0,
                y1: 1.0,
                x1: 0.5 + w / 2.0,
            }
        }
    }

    /// The picture's pixel for the frame's `(x, y)` pixel, as it is: a
    /// point in the black past the picture's edge stays out there.
    pub fn back(&self, input: Input, width: usize, height: usize, x: f32, y: f32) -> (f32, f32) {
        let u = x / input.width as f32;
        let v = y / input.height as f32;
        (
            (self.x0 + u * (self.x1 - self.x0)) * width as f32,
            (self.y0 + v * (self.y1 - self.y0)) * height as f32,
        )
    }
}

/// `region` of `picture` as one frame for `input`: each frame pixel `(u,
/// v)` is the picture at `(x0 W + u / sx, y0 H + v / sy)`, bilinear, black
/// past the picture, as OpenCV's `warpAffine` makes MoveNet's crop.
pub fn crop(picture: &Picture, region: Region, input: Input) -> Vec<u8> {
    let (w, h) = (input.width, input.height);
    let (pw, ph) = (picture.width as f32, picture.height as f32);
    let step_x = (region.x1 - region.x0) * pw / w as f32;
    let step_y = (region.y1 - region.y0) * ph / h as f32;
    let (left, top) = (region.x0 * pw, region.y0 * ph);
    let mut frame = vec![0u8; input.bytes()];
    let plane = w * h;
    // One axis's two neighbours and their weights; a neighbour off the
    // picture is `None`, and black.
    let taps = |at: f32, size: usize| -> [(Option<usize>, f32); 2] {
        let floor = at.floor();
        let weight = at - floor;
        let inside = |i: f32| (i >= 0.0 && i < size as f32).then_some(i as usize);
        [(inside(floor), 1.0 - weight), (inside(floor + 1.0), weight)]
    };
    let columns: Vec<_> = (0..w)
        .map(|u| taps(left + u as f32 * step_x, picture.width))
        .collect();
    for v in 0..h {
        let rows = taps(top + v as f32 * step_y, picture.height);
        for (u, column) in columns.iter().enumerate() {
            let mut pixel = [0f32; 3];
            for (row, wy) in rows {
                let Some(row) = row else { continue };
                for (col, wx) in column {
                    let Some(col) = col else { continue };
                    let at = (row * picture.width + col) * 3;
                    for (c, value) in pixel.iter_mut().enumerate() {
                        *value += f32::from(picture.rgb[at + c]) * wy * wx;
                    }
                }
            }
            for (c, value) in pixel.iter().enumerate() {
                let byte = value.round().clamp(0.0, 255.0) as u8;
                if input.planar {
                    frame[c * plane + v * w + u] = byte;
                } else {
                    frame[(v * w + u) * 3 + c] = byte;
                }
            }
        }
    }
    frame
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

    const SQUARE: Input = Input {
        width: 192,
        height: 192,
        planar: false,
    };

    #[test]
    fn a_picture_of_the_models_size_goes_in_as_it_is() {
        let p = picture(8, 6);
        let input = Input {
            width: 8,
            height: 6,
            planar: false,
        };
        let whole = Region::whole(8, 6, input);
        assert_eq!(crop(&p, whole, input), p.rgb);
        assert_eq!(whole.back(input, 8, 6, 4.0, 3.0), (4.0, 3.0));
    }

    #[test]
    fn a_wide_picture_is_centred_in_a_square_frame_and_maps_back() {
        let p = picture(640, 480);
        let whole = Region::whole(640, 480, SQUARE);
        assert_eq!((whole.x0, whole.x1), (0.0, 1.0));
        assert!((whole.y0 + 1.0 / 6.0).abs() < 1e-6 && (whole.y1 - 7.0 / 6.0).abs() < 1e-6);
        let frame = crop(&p, whole, SQUARE);
        assert_eq!(frame.len(), 192 * 192 * 3);
        // 640x480 in 192: 144 rows of picture, 24 rows of black above.
        assert!(frame[..192 * 24 * 3].iter().all(|&b| b == 0));
        assert_eq!(frame[192 * 24 * 3 + 1], 7);
        let (x, y) = whole.back(SQUARE, 640, 480, 96.0, 96.0);
        assert!(
            (x - 320.0).abs() < 0.01 && (y - 240.0).abs() < 0.01,
            "{x} {y}"
        );
        // A point in the black stays off the picture.
        assert!(whole.back(SQUARE, 640, 480, 0.0, 0.0).1 < 0.0);
        // A tall one is centred side to side.
        let tall = Region::whole(480, 640, SQUARE);
        assert!((tall.x0 + 1.0 / 6.0).abs() < 1e-6 && tall.y0 == 0.0);
    }

    #[test]
    fn a_planar_frame_puts_each_colour_in_its_plane() {
        let p = picture(4, 4);
        let input = Input {
            width: 4,
            height: 4,
            planar: true,
        };
        let frame = crop(&p, Region::whole(4, 4, input), input);
        assert!(frame[16..32].iter().all(|&b| b == 7));
        assert!(frame[32..48].iter().all(|&b| b == 200));
    }

    #[derive(serde::Deserialize)]
    struct Cropped {
        width: usize,
        height: usize,
        region: [f32; 4],
        size: usize,
        at: Vec<usize>,
        rgb: Vec<u8>,
    }

    /// The reference's own crops (OpenCV's `warpAffine`, by
    /// `tests/fixtures/movenet/make.py`) of a made picture: the whole of a
    /// wide and of a tall one, a crop around a person, one past the edge.
    #[test]
    fn crops_are_the_references() {
        let crops: Vec<Cropped> =
            serde_json::from_str(include_str!("../tests/fixtures/movenet/crop.json")).unwrap();
        assert_eq!(crops.len(), 4);
        for c in crops {
            let (w, h) = (c.width, c.height);
            let rgb = (0..w * h)
                .flat_map(|i| {
                    let (x, y) = (i % w, i / w);
                    [
                        x * 255 / (w - 1),
                        y * 255 / (h - 1),
                        (x + y) * 255 / (w + h - 2),
                    ]
                    .map(|v| v as u8)
                })
                .collect();
            let p = Picture {
                rgb,
                width: w,
                height: h,
            };
            let [y0, x0, y1, x1] = c.region;
            let input = Input {
                width: c.size,
                height: c.size,
                planar: false,
            };
            let frame = crop(&p, Region { y0, x0, y1, x1 }, input);
            for (n, at) in c.at.iter().enumerate() {
                for k in 0..3 {
                    let (ours, theirs) = (frame[at * 3 + k], c.rgb[n * 3 + k]);
                    // OpenCV weighs in 1/32 pixel steps: one apart at most.
                    assert!(
                        ours.abs_diff(theirs) <= 1,
                        "{:?} pixel {at}: {ours} {theirs}",
                        c.region
                    );
                }
            }
        }
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
