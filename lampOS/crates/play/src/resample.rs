//! Mono f32 at any rate in, mono S16 at the speaker's 24 kHz out: a
//! windowed-sinc resampler (Blackman window), streaming, with its own
//! history. Down from 44.1 or 48 kHz the sinc is scaled so everything above
//! the output's Nyquist is cut, not folded back. At 24 kHz it only converts.

/// The speaker's rate, as `lamp-control` and the voice take it.
pub const OUT_RATE: u32 = 24_000;
/// Taps each side of the centre at the output's own rate.
const HALF: f64 = 12.0;

/// The converter for one stream of one input rate.
#[derive(Debug)]
pub struct Resampler {
    /// The input rate.
    rate: u64,
    /// Lower than 1 when going down: the share of the input's band kept.
    cutoff: f64,
    /// Taps each side of the centre, in input samples.
    half: usize,
    /// The input kept: the last `2 * half` samples before the next centre,
    /// then what has come since.
    buffer: Vec<f32>,
    /// How many outputs have been made.
    made: u64,
    /// How many input samples have been dropped from the buffer's start.
    dropped: u64,
    /// Whether the rate is the output's and nothing is filtered.
    same: bool,
}

impl Resampler {
    /// A converter for `rate` Hz input; `None` for a rate that cannot be
    /// played (zero, or over 192 kHz).
    pub fn new(rate: u32) -> Option<Self> {
        if rate == 0 || rate > 192_000 {
            return None;
        }
        let cutoff = (f64::from(OUT_RATE) / f64::from(rate)).min(1.0);
        let half = (HALF / cutoff).ceil() as usize;
        Some(Self {
            rate: u64::from(rate),
            cutoff,
            half,
            // The first outputs see silence before the start.
            buffer: vec![0.0; half],
            made: 0,
            dropped: 0,
            same: rate == OUT_RATE,
        })
    }

    /// Converts `input`, appending the output samples to `out`.
    pub fn push(&mut self, input: &[f32], out: &mut Vec<i16>) {
        if self.same {
            out.extend(input.iter().map(|&sample| to_i16(sample)));
            return;
        }
        self.buffer.extend_from_slice(input);
        let half = self.half as f64;
        // An output needs `half` input samples after its centre.
        loop {
            let at = self.centre();
            if at + half >= self.buffer.len() as f64 {
                break;
            }
            let first = (at - half).ceil() as usize;
            let last = (at + half).floor() as usize;
            let mut sum = 0.0;
            for index in first..=last {
                let distance = index as f64 - at;
                sum += f64::from(self.buffer[index]) * self.kernel(distance, half);
            }
            out.push(to_i16(sum as f32));
            self.made += 1;
        }
        // Keep only what a later output can still reach.
        let drop = ((self.centre() - half).floor().max(0.0)) as usize;
        if drop > 0 {
            self.buffer.drain(..drop);
            self.dropped += drop as u64;
        }
    }

    /// The next output's centre, in input samples from the buffer's start,
    /// from whole numbers only so no block size can move it: output `n` is
    /// `n * rate / 24000` input samples in, and the buffer starts `half`
    /// samples before the stream's first.
    fn centre(&self) -> f64 {
        let numerator = self.made * self.rate;
        let whole = numerator / u64::from(OUT_RATE);
        let part = numerator % u64::from(OUT_RATE);
        let absolute = whole as f64 + part as f64 / f64::from(OUT_RATE);
        absolute + self.half as f64 - self.dropped as f64
    }

    /// The end of the stream: the last outputs, which wait on input that
    /// will not come, are made from silence after it.
    pub fn finish(&mut self, out: &mut Vec<i16>) {
        if !self.same {
            let silence = vec![0.0; self.half];
            self.push(&silence, out);
        }
    }

    /// The filter's weight for an input sample `distance` from the centre.
    fn kernel(&self, distance: f64, half: f64) -> f64 {
        if distance.abs() >= half {
            return 0.0;
        }
        let x = distance * self.cutoff;
        let sinc = if x.abs() < 1e-9 {
            1.0
        } else {
            (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
        };
        let turn = std::f64::consts::PI * distance / half;
        let window = 0.42 + 0.5 * turn.cos() + 0.08 * (2.0 * turn).cos();
        self.cutoff * sinc * window
    }
}

fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16
}

#[cfg(test)]
mod tests;
