//! Two causal Keras LSTM layers followed by a ReLU and a three-class softmax.
use super::features::STATS;
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

const PACKED_BYTES: u64 = 630_928;
const PACKED_SHA256: &str = "fa1c39e2da45113313648a2501767002c33624e88a4dc906e7ba025468453bd0";

pub(super) struct Weights {
    mean: Vec<f32>,
    stddev: Vec<f32>,
    projection: Vec<f32>,
    projection_bias: Vec<f32>,
    pub threshold: f32,
    pub mel_mean: Vec<f32>,
    pub mel_stddev: Vec<f32>,
    layers: [Layer; 2],
    hidden_weights: Vec<f32>,
    hidden_bias: Vec<f32>,
    classifier: Vec<f32>,
    class_bias: Vec<f32>,
}

struct Layer {
    input: Vec<f32>,
    recurrent: Vec<f32>,
    bias: Vec<f32>,
}

pub(super) struct State {
    hidden: [f32; 64],
    cell: [f32; 64],
}

impl Default for State {
    fn default() -> Self {
        Self {
            hidden: [0.0; 64],
            cell: [0.0; 64],
        }
    }
}

/// Row-major kernel: add input times each contiguous row to the output.
fn multiply(input: &[f32], matrix: &[f32], output: &mut [f32]) {
    for (&x, row) in input.iter().zip(matrix.chunks_exact(output.len())) {
        for (y, w) in output.iter_mut().zip(row) {
            *y += x * w;
        }
    }
}

impl Layer {
    fn advance(&self, input: &[f32], state: &mut State) {
        let mut gates = [0.0; 256];
        gates.copy_from_slice(&self.bias);
        multiply(input, &self.input, &mut gates);
        multiply(&state.hidden, &self.recurrent, &mut gates);
        let sigmoid = |x: f32| 1.0 / (1.0 + (-x).exp());
        for i in 0..64 {
            state.cell[i] =
                sigmoid(gates[i + 64]) * state.cell[i] + sigmoid(gates[i]) * gates[i + 128].tanh();
            state.hidden[i] = sigmoid(gates[i + 192]) * state.cell[i].tanh();
        }
    }
}

impl Weights {
    pub(super) fn load(dir: &Path) -> Result<Self, String> {
        let file = File::open(dir.join("caller-frame.f32")).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(PACKED_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 != PACKED_BYTES
            || format!("{:x}", Sha256::digest(&bytes)) != PACKED_SHA256
        {
            return Err("unexpected caller-frame model size or SHA-256".into());
        }
        let mut words = bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()));
        let mut take = |n: usize| words.by_ref().take(n).collect::<Vec<_>>();
        let mean = take(STATS);
        let stddev = take(STATS);
        let projection = take(STATS * 256);
        let projection_bias = take(256);
        let threshold = take(1)[0];
        let mel_mean = take(40);
        let mel_stddev = take(40);
        let layers = [41, 64].map(|n| Layer {
            input: take(n * 256),
            recurrent: take(64 * 256),
            bias: take(256),
        });
        let weights = Self {
            mean,
            stddev,
            projection,
            projection_bias,
            threshold,
            mel_mean,
            mel_stddev,
            layers,
            hidden_weights: take(64 * 64),
            hidden_bias: take(64),
            classifier: take(64 * 3),
            class_bias: take(3),
        };
        debug_assert_eq!(words.len(), 0);
        Ok(weights)
    }

    pub(super) fn project(&self, mut stats: [f32; STATS]) -> [f32; 256] {
        for ((x, mu), sigma) in stats.iter_mut().zip(&self.mean).zip(&self.stddev) {
            *x = (*x - mu) / sigma.max(1e-5);
        }
        let mut out = [0.0; 256];
        out.copy_from_slice(&self.projection_bias);
        multiply(&stats, &self.projection, &mut out);
        let norm = out.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
        out.map(|x| x / norm)
    }

    pub(super) fn predict(
        &self,
        mel: &[f32; 120],
        cosine: f32,
        state: &mut [State; 2],
    ) -> [f32; 3] {
        let mut input = [0.0; 41];
        for i in 0..40 {
            input[i] = (mel[i] - self.mel_mean[i]) / self.mel_stddev[i].max(1e-5);
        }
        input[40] = cosine;
        let [first, second] = state;
        self.layers[0].advance(&input, first);
        self.layers[1].advance(&first.hidden, second);
        let mut hidden = [0.0; 64];
        hidden.copy_from_slice(&self.hidden_bias);
        multiply(&second.hidden, &self.hidden_weights, &mut hidden);
        let mut logits = [0.0; 3];
        logits.copy_from_slice(&self.class_bias);
        multiply(&hidden.map(|x| x.max(0.0)), &self.classifier, &mut logits);
        let max = logits.into_iter().fold(f32::NEG_INFINITY, f32::max);
        let exp = logits.map(|x| (x - max).exp());
        let total: f32 = exp.iter().sum();
        exp.map(|x| x / total)
    }
}
