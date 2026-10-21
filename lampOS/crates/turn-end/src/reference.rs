//! The `voiced_9s` signal of `tests/fixtures/README.md` and ONNX Runtime's
//! probability for it with the pinned model. The worker's installation
//! self-check classifies it; `tests/it/parity.rs` checks both against the fixture.

/// ONNX Runtime 1.31.0's probability for [`voiced`].
pub const VOICED_PROBABILITY: f32 = 0.968_532_3;
/// Largest accepted difference between tract and ONNX Runtime.
pub const TOLERANCE: f32 = 1e-4;
/// The fast model's probability for [`voiced`]: the pinned model rewritten
/// to 8 bits (`crate::eight_bit`) under ONNX Runtime 1.30.0, as measured on
/// Linux ARM64 and macOS (docs/endpoint-pilot.md, "The fast model").
pub const VOICED_PROBABILITY_FAST: f32 = 0.968_226_6;
/// Largest accepted difference for the fast model: its 8-bit rounding is
/// the same everywhere, but its float stages may take other routes on
/// other processors.
pub const TOLERANCE_FAST: f32 = 1e-3;

/// 9 s of xorshift32 noise (seed 2) with a 3-sample pulse every 100 samples,
/// whose height steps every 0.1 s.
pub fn voiced() -> Vec<i16> {
    let mut state = 2u32;
    (0..144_000)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let pulse = if i % 100 < 3 { 9_000 } else { 0 };
            (pulse * ((i / 1_600) % 10 + 1) / 10 + (state % 801) as i32 - 400) as i16
        })
        .collect()
}
