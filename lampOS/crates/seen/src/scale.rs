//! The frame as it is sent (#6543): the camera's JPEG at [`SENT`], V1's
//! size on the lamp's link, whatever size the camera serves.
//!
//! A frame exactly [`SENT`] goes as it came. A larger one is decoded,
//! scaled down by area (each sent pixel the mean of the camera pixels it
//! covers) until one side meets [`SENT`]'s and the other is no smaller,
//! so a 4:3 frame becomes 320×240 exactly, and encoded again at
//! [`QUALITY`]. The server resizes anyway (its person detector letterboxes
//! the frame, the action model takes a 224×224 crop), so more pixels would
//! only cost bytes and time. A frame smaller than [`SENT`] is not sent.
//!
//! The decoded pixels and the scaled pixels are wiped before this returns;
//! the result is the in-flight copy, wiped by its holder. The decoder's
//! and encoder's own working rows are inside those crates and are not
//! reachable to wipe; they are freed when this returns.
use zeroize::{Zeroize, Zeroizing};
use zune_jpeg::{
    JpegDecoder,
    zune_core::{colorspace::ColorSpace, options::DecoderOptions},
};

/// The size sent: V1's lamp link size, and the least taken.
pub const SENT: (u32, u32) = (320, 240);
/// The quality a scaled frame is encoded at.
pub const QUALITY: u8 = 80;
/// The largest side decoded: a frame claiming more is refused unread.
pub const MOST_SIDE: u32 = 4096;

/// Why a frame was not made ready to send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Smaller than [`SENT`] on either side.
    TooSmall,
    /// Larger than [`MOST_SIDE`] on either side.
    TooLarge,
    /// Not a JPEG the decoder reads, or not the size its meta said.
    NotAJpeg,
    /// The scaled pixels could not be encoded.
    NotEncoded,
}

/// The size a `width`×`height` frame is sent at: [`SENT`] for 4:3, else
/// the side that reaches [`SENT`]'s first is [`SENT`]'s and the other is
/// in proportion (1280×720 is sent 427×240).
pub fn sent_size(width: u32, height: u32) -> (u32, u32) {
    let (w, h) = (u64::from(width), u64::from(height));
    let (sw, sh) = (u64::from(SENT.0), u64::from(SENT.1));
    // Wider than 4:3: the height is SENT's; else the width is.
    let size = if w * sh >= h * sw {
        (((w * sh + h / 2) / h).max(sw), sh)
    } else {
        (sw, ((h * sw + w / 2) / w).max(sh))
    };
    (size.0 as u32, size.1 as u32)
}

/// The camera's `jpeg`, said by its meta to be `width`×`height`, as it is
/// sent: as it came at [`SENT`], else scaled down and encoded again.
pub fn for_sending(jpeg: &[u8], width: u32, height: u32) -> Result<Zeroizing<Vec<u8>>, Refused> {
    if width < SENT.0 || height < SENT.1 {
        return Err(Refused::TooSmall);
    }
    if width > MOST_SIDE || height > MOST_SIDE {
        return Err(Refused::TooLarge);
    }
    if (width, height) == SENT {
        return Ok(Zeroizing::new(jpeg.to_vec()));
    }
    let options = DecoderOptions::default()
        .set_max_width(MOST_SIDE as usize)
        .set_max_height(MOST_SIDE as usize)
        .jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(jpeg, options);
    decoder.decode_headers().map_err(|_| Refused::NotAJpeg)?;
    if decoder.dimensions() != Some((width as usize, height as usize))
        || decoder.get_output_colorspace() != Some(ColorSpace::RGB)
    {
        return Err(Refused::NotAJpeg);
    }
    let length = decoder.output_buffer_size().ok_or(Refused::NotAJpeg)?;
    if length != width as usize * height as usize * 3 {
        return Err(Refused::NotAJpeg);
    }
    // Ours from the start, so it is wiped however this returns.
    let mut decoded = Zeroizing::new(vec![0u8; length]);
    decoder
        .decode_into(&mut decoded)
        .map_err(|_| Refused::NotAJpeg)?;
    drop(decoder);
    let (across, down) = sent_size(width, height);
    let scaled = Zeroizing::new(area_scaled(&decoded, (width, height), (across, down)));
    drop(decoded);
    // Room for any JPEG of these pixels at once, so the output never grows
    // into a new allocation and leaves an unwiped one behind.
    let mut out = Vec::with_capacity(scaled.len() + 4096);
    let encoded = jpeg_encoder::Encoder::new(&mut out, QUALITY).encode(
        &scaled,
        across as u16,
        down as u16,
        jpeg_encoder::ColorType::Rgb,
    );
    match encoded {
        Ok(()) => Ok(Zeroizing::new(out)),
        Err(_) => {
            out.zeroize();
            Err(Refused::NotEncoded)
        }
    }
}

/// `rgb` (`from`, three bytes a pixel) scaled down to `to` by area: each
/// pixel the rounded mean of the source pixels in its box.
fn area_scaled(rgb: &[u8], from: (u32, u32), to: (u32, u32)) -> Vec<u8> {
    let (fw, fh) = (from.0 as usize, from.1 as usize);
    let (tw, th) = (to.0 as usize, to.1 as usize);
    // Each sent column's first and past-last source column.
    let columns: Vec<(usize, usize)> = (0..tw)
        .map(|x| {
            let start = x * fw / tw;
            (start, ((x + 1) * fw / tw).max(start + 1))
        })
        .collect();
    let mut sums = vec![0u32; tw * 3];
    let mut out = Vec::with_capacity(tw * th * 3);
    for y in 0..th {
        let top = y * fh / th;
        let bottom = ((y + 1) * fh / th).max(top + 1);
        sums.fill(0);
        for row in rgb[top * fw * 3..bottom * fw * 3].chunks_exact(fw * 3) {
            for (sum, &(start, end)) in sums.chunks_exact_mut(3).zip(&columns) {
                for pixel in row[start * 3..end * 3].chunks_exact(3) {
                    sum[0] += u32::from(pixel[0]);
                    sum[1] += u32::from(pixel[1]);
                    sum[2] += u32::from(pixel[2]);
                }
            }
        }
        for (sum, &(start, end)) in sums.chunks_exact(3).zip(&columns) {
            let count = ((end - start) * (bottom - top)) as u32;
            out.extend(sum.iter().map(|&s| ((s + count / 2) / count) as u8));
        }
    }
    sums.zeroize();
    out
}

#[cfg(test)]
pub(crate) mod tests;
