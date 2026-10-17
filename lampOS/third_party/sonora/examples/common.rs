//! Helpers shared by multiple examples.

/// Deinterleave an interleaved multi-channel buffer into per-channel vectors.
///
/// ```text
/// Interleaved (stereo, 3 samples):
/// [L0, R0, L1, R1, L2, R2]
///
/// Deinterleaved:
/// [[L0, L1, L2],
///  [R0, R1, R2]]
/// ```
pub(crate) fn deinterleave(src: &[f32], dst: &mut [Vec<f32>]) {
    let num_channels = dst.len();
    let num_samples = src.len() / num_channels;
    assert_eq!(src.len(), num_channels * num_samples);
    for ch in dst.iter_mut() {
        ch.resize(num_samples, 0.0);
    }
    for (i, &sample) in src.iter().enumerate() {
        dst[i % num_channels][i / num_channels] = sample;
    }
}

/// Interleave per-channel vectors into a flat interleaved buffer.
pub(crate) fn interleave(src: &[Vec<f32>], dst: &mut [f32]) {
    let num_channels = src.len();
    let num_samples = src[0].len();
    assert_eq!(dst.len(), num_channels * num_samples);
    for ch in 0..num_channels {
        for s in 0..num_samples {
            dst[s * num_channels + ch] = src[ch][s];
        }
    }
}
