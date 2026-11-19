//! What runs a model: a [`Backend`] loads one into a [`Net`], which takes one
//! RGB frame at its own size and gives its raw outputs as [`Tensor`]s. The
//! real backend is the A733's VIPLite ([`crate::vip`]); the tests use
//! [`Fake`]. Nothing here knows what the model is for: a head
//! ([`crate::head`]) reads the outputs.
use std::path::Path;

/// How a tensor's numbers are kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    F32,
    F16,
    U8,
    I8,
    U16,
    I16,
    I32,
    U32,
}

impl Format {
    /// VIPLite's `vip_buffer_format_e`.
    pub fn from_vip(code: i32) -> Option<Self> {
        Some(match code {
            0 => Self::F32,
            1 => Self::F16,
            2 => Self::U8,
            3 | 6 => Self::I8,
            4 => Self::U16,
            5 => Self::I16,
            8 => Self::I32,
            9 => Self::U32,
            _ => return None,
        })
    }

    pub fn bytes(self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::F16 | Self::U16 | Self::I16 => 2,
            Self::F32 | Self::I32 | Self::U32 => 4,
        }
    }
}

/// How a quantized tensor's numbers map back to real ones.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Quant {
    None,
    /// `(q - zero) * scale`.
    Affine {
        scale: f32,
        zero: i32,
    },
    /// `q / 2^pos` (dynamic fixed point).
    Dfp {
        pos: i32,
    },
}

/// One output: its shape, outermost first (`n, c, h, w` for a feature map),
/// its raw bytes, little-endian, and how to read them.
#[derive(Clone, Debug, PartialEq)]
pub struct Tensor {
    pub dims: Vec<usize>,
    pub format: Format,
    pub quant: Quant,
    pub data: Vec<u8>,
}

impl Tensor {
    /// A float tensor, for the tests and for a model with float outputs.
    pub fn from_f32(dims: Vec<usize>, values: &[f32]) -> Self {
        Self {
            dims,
            format: Format::F32,
            quant: Quant::None,
            data: values.iter().flat_map(|v| v.to_le_bytes()).collect(),
        }
    }

    /// How many numbers it holds.
    pub fn len(&self) -> usize {
        self.data.len() / self.format.bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The `i`th number, as a real number; 0 past the end.
    pub fn at(&self, i: usize) -> f32 {
        let size = self.format.bytes();
        let Some(raw) = self.data.get(i * size..(i + 1) * size) else {
            return 0.0;
        };
        let q = match self.format {
            Format::U8 => f32::from(raw[0]),
            Format::I8 => f32::from(raw[0] as i8),
            Format::U16 => f32::from(u16::from_le_bytes([raw[0], raw[1]])),
            Format::I16 => f32::from(i16::from_le_bytes([raw[0], raw[1]])),
            Format::F16 => f16_to_f32(u16::from_le_bytes([raw[0], raw[1]])),
            Format::F32 => f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
            Format::I32 => i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f32,
            Format::U32 => u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f32,
        };
        match self.quant {
            Quant::None => q,
            Quant::Affine { scale, zero } => (q - zero as f32) * scale,
            Quant::Dfp { pos } => q * 2f32.powi(-pos),
        }
    }
}

/// An IEEE half as a float.
fn f16_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    let fraction = f32::from(bits & 0x3ff);
    match exponent {
        0 => sign * fraction * 2f32.powi(-24),
        31 if fraction == 0.0 => sign * f32::INFINITY,
        31 => f32::NAN,
        _ => sign * (1.0 + fraction / 1024.0) * 2f32.powi(exponent - 15),
    }
}

/// How the model takes its frame: unsigned bytes, red, green and blue, at
/// `width` x `height`, interleaved (`HWC`) or in planes (`CHW`). The
/// normalisation is the model's own, baked in when it was converted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub width: usize,
    pub height: usize,
    pub planar: bool,
}

impl Input {
    /// The bytes one frame takes.
    pub fn bytes(self) -> usize {
        self.width * self.height * 3
    }
}

/// How long one run's parts took, in ms.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RunMs {
    /// The frame copied in and flushed.
    pub input: f64,
    /// The NPU's own run.
    pub npu: f64,
}

/// A loaded model.
pub trait Net: Send {
    fn input(&self) -> Input;
    /// One run on `rgb`, [`Input::bytes`] long in the input's layout.
    fn run(&mut self, rgb: &[u8]) -> Result<(Vec<Tensor>, RunMs), String>;
}

/// Why a model did not load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// This lamp has no NPU, or not one this build can open: nothing to
    /// retry.
    Unsupported(String),
    /// The NPU is there and refused this file.
    Failed(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(why) | Self::Failed(why) => f.write_str(why),
        }
    }
}

/// What loads a model file into a [`Net`].
pub trait Backend: Send + Sync {
    fn load(&self, model: &Path) -> Result<Box<dyn Net>, LoadError>;
}

/// A backend for the tests: every model loads as a net of `input` that
/// answers each run with `outputs`, and counts its loads and runs.
#[derive(Clone)]
pub struct Fake {
    pub input: Input,
    pub outputs: Vec<Tensor>,
    pub loads: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub runs: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// `Some`: every load fails so.
    pub refuse: Option<LoadError>,
    /// How long each run takes: a slow or hung NPU.
    pub delay: std::time::Duration,
}

impl Fake {
    pub fn new(input: Input, outputs: Vec<Tensor>) -> Self {
        Self {
            input,
            outputs,
            loads: Default::default(),
            runs: Default::default(),
            refuse: None,
            delay: std::time::Duration::ZERO,
        }
    }
}

struct FakeNet {
    input: Input,
    outputs: Vec<Tensor>,
    runs: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    delay: std::time::Duration,
}

impl Net for FakeNet {
    fn input(&self) -> Input {
        self.input
    }

    fn run(&mut self, rgb: &[u8]) -> Result<(Vec<Tensor>, RunMs), String> {
        if rgb.len() != self.input.bytes() {
            return Err(format!(
                "the frame is {} bytes, the model takes {}",
                rgb.len(),
                self.input.bytes()
            ));
        }
        self.runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::thread::sleep(self.delay);
        Ok((self.outputs.clone(), RunMs::default()))
    }
}

impl Backend for Fake {
    fn load(&self, _model: &Path) -> Result<Box<dyn Net>, LoadError> {
        if let Some(refused) = &self.refuse {
            return Err(refused.clone());
        }
        self.loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Box::new(FakeNet {
            input: self.input,
            outputs: self.outputs.clone(),
            runs: self.runs.clone(),
            delay: self.delay,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tensor_reads_back_through_its_quantisation() {
        let affine = Tensor {
            dims: vec![3],
            format: Format::U8,
            quant: Quant::Affine {
                scale: 0.5,
                zero: 128,
            },
            data: vec![128, 130, 0],
        };
        assert_eq!(
            (affine.at(0), affine.at(1), affine.at(2)),
            (0.0, 1.0, -64.0)
        );
        assert_eq!(affine.at(3), 0.0);
        let dfp = Tensor {
            dims: vec![1],
            format: Format::I8,
            quant: Quant::Dfp { pos: 2 },
            data: vec![(-8i8) as u8],
        };
        assert_eq!(dfp.at(0), -2.0);
        // int16 dynamic fixed point, as the int16 MoveNet's outputs are
        // (fl 15 for its sigmoids, 10 and 8 for its offsets and regression).
        let i16_dfp = |pos: i32, values: &[i16]| Tensor {
            dims: vec![values.len()],
            format: Format::I16,
            quant: Quant::Dfp { pos },
            data: values.iter().flat_map(|v| v.to_le_bytes()).collect(),
        };
        let sigmoid = i16_dfp(15, &[0, 16384, 32767]);
        assert_eq!((sigmoid.at(0), sigmoid.at(1)), (0.0, 0.5));
        assert!((sigmoid.at(2) - 32767.0 / 32768.0).abs() < 1e-7);
        let regress = i16_dfp(8, &[-256, 24_576]);
        assert_eq!((regress.at(0), regress.at(1)), (-1.0, 96.0));
        let dfp8 = Tensor {
            dims: vec![1],
            format: Format::I8,
            quant: Quant::Dfp { pos: 7 },
            data: vec![64],
        };
        assert_eq!(dfp8.at(0), 0.5);
        let float = Tensor::from_f32(vec![2], &[1.5, -3.0]);
        assert_eq!((float.len(), float.at(1)), (2, -3.0));
        // 1.0 and -2.0 as halves.
        let half = Tensor {
            dims: vec![2],
            format: Format::F16,
            quant: Quant::None,
            data: [0x3c00u16, 0xc000]
                .iter()
                .flat_map(|h| h.to_le_bytes())
                .collect(),
        };
        assert_eq!((half.at(0), half.at(1)), (1.0, -2.0));
    }
}
