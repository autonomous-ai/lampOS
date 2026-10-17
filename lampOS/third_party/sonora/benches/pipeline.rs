//! Benchmarks for the Sonora audio processing pipeline and components.

use std::slice;

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use sonora::config::{EchoCanceller, GainController2, NoiseSuppression};
use sonora::{AudioProcessing, Config, StreamConfig};
use sonora_common_audio::sinc_resampler::{SincResampler, SincResamplerCallback};
use sonora_fft::pffft::{FftType, Pffft};
use sonora_ns::config::{NS_FRAME_SIZE, SuppressionLevel};
use sonora_ns::noise_suppressor::NoiseSuppressor;

// ---------------------------------------------------------------------------
// Full pipeline benchmarks
// ---------------------------------------------------------------------------

fn make_apm(sample_rate: u32, channels: u16) -> (AudioProcessing, StreamConfig) {
    let config = Config {
        echo_canceller: Some(EchoCanceller::default()),
        noise_suppression: Some(NoiseSuppression::default()),
        gain_controller2: Some(GainController2::default()),
        ..Default::default()
    };
    let mut apm = AudioProcessing::builder().config(config).build();
    let stream = StreamConfig::new(sample_rate, channels);

    // Warm up the internal state with a few frames so we bench steady-state.
    let samples = stream.num_frames();
    let nch = channels as usize;
    let src_ch: Vec<f32> = (0..samples)
        .map(|i| (i as f32 * 0.01).sin() * 0.1)
        .collect();
    let src: Vec<&[f32]> = (0..nch).map(|_| src_ch.as_slice()).collect();
    let mut dst_ch = vec![0.0f32; samples];
    let mut dst: Vec<&mut [f32]> = (0..nch)
        .map(|_| {
            // SAFETY: each slice is independent; we just need the borrow checker to cooperate
            unsafe { slice::from_raw_parts_mut(dst_ch.as_mut_ptr(), samples) }
        })
        .collect();

    for _ in 0..20 {
        let _ = apm.process_capture_f32_with_config(&src, &stream, &stream, &mut dst);
    }
    (apm, stream)
}

fn bench_process_stream(c: &mut Criterion) {
    let mut group = c.benchmark_group("process_stream");

    // 16kHz mono
    {
        let (mut apm, stream) = make_apm(16000, 1);
        let samples = stream.num_frames();
        let src_data: Vec<f32> = (0..samples)
            .map(|i| (i as f32 * 0.01).sin() * 0.1)
            .collect();
        let src = [src_data.as_slice()];
        let mut dst_data = vec![0.0f32; samples];

        group.bench_function("16k_mono", |b| {
            b.iter(|| {
                let mut dst = [dst_data.as_mut_slice()];
                apm.process_capture_f32_with_config(black_box(&src), &stream, &stream, &mut dst)
                    .unwrap();
            });
        });
    }

    // 48kHz mono
    {
        let (mut apm, stream) = make_apm(48000, 1);
        let samples = stream.num_frames();
        let src_data: Vec<f32> = (0..samples)
            .map(|i| (i as f32 * 0.01).sin() * 0.1)
            .collect();
        let src = [src_data.as_slice()];
        let mut dst_data = vec![0.0f32; samples];

        group.bench_function("48k_mono", |b| {
            b.iter(|| {
                let mut dst = [dst_data.as_mut_slice()];
                apm.process_capture_f32_with_config(black_box(&src), &stream, &stream, &mut dst)
                    .unwrap();
            });
        });
    }

    // 48kHz stereo
    {
        let (mut apm, stream) = make_apm(48000, 2);
        let samples = stream.num_frames();
        let src_data: Vec<f32> = (0..samples)
            .map(|i| (i as f32 * 0.01).sin() * 0.1)
            .collect();
        let src = [src_data.as_slice(), src_data.as_slice()];
        let mut dst_data_l = vec![0.0f32; samples];
        let mut dst_data_r = vec![0.0f32; samples];

        group.bench_function("48k_stereo", |b| {
            b.iter(|| {
                let mut dst = [dst_data_l.as_mut_slice(), dst_data_r.as_mut_slice()];
                apm.process_capture_f32_with_config(black_box(&src), &stream, &stream, &mut dst)
                    .unwrap();
            });
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Component benchmarks
// ---------------------------------------------------------------------------

fn bench_noise_suppressor(c: &mut Criterion) {
    let mut group = c.benchmark_group("noise_suppressor");
    let mut ns = NoiseSuppressor::with_level(SuppressionLevel::K12dB);

    let mut frame = [0.0f32; NS_FRAME_SIZE];
    for (i, s) in frame.iter_mut().enumerate() {
        *s = (i as f32 * 0.05).sin() * 0.1;
    }

    // Warm up
    for _ in 0..50 {
        ns.analyze(&frame);
        ns.process(&mut frame);
    }

    group.bench_function("analyze_and_process", |b| {
        b.iter(|| {
            ns.analyze(black_box(&frame));
            ns.process(black_box(&mut frame));
        });
    });

    group.finish();
}

fn bench_pffft(c: &mut Criterion) {
    let mut group = c.benchmark_group("pffft");

    for &size in &[128, 256, 512] {
        let mut fft = Pffft::new(size, FftType::Real);
        let mut input = fft.create_buffer();
        let mut output = fft.create_buffer();
        for (i, s) in input.as_mut_slice().iter_mut().enumerate() {
            *s = (i as f32 * 0.01).sin();
        }

        group.bench_function(format!("forward_{size}"), |b| {
            b.iter(|| {
                fft.forward(black_box(&input), &mut output, true);
            });
        });

        group.bench_function(format!("roundtrip_{size}"), |b| {
            b.iter(|| {
                fft.forward(black_box(&input), &mut output, true);
                fft.backward(&output, &mut input, true);
            });
        });
    }

    group.finish();
}

struct ZeroCallback;
impl SincResamplerCallback for ZeroCallback {
    fn run(&mut self, frames: usize, destination: &mut [f32]) {
        for s in &mut destination[..frames] {
            *s = 0.001;
        }
    }
}

fn bench_sinc_resampler(c: &mut Criterion) {
    let mut group = c.benchmark_group("sinc_resampler");

    // 48kHz -> 16kHz (ratio = 48/16 = 3.0, but SincResampler uses io ratio = output/input)
    let ratio = 16000.0 / 48000.0;
    let request_frames = 160; // 10ms at 16kHz
    let mut resampler = SincResampler::new(ratio, request_frames);
    let mut output = vec![0.0f32; request_frames];
    let mut cb = ZeroCallback;

    // Warm up
    for _ in 0..20 {
        resampler.resample(request_frames, &mut output, &mut cb);
    }

    group.bench_function("48k_to_16k", |b| {
        b.iter(|| {
            resampler.resample(black_box(request_frames), &mut output, &mut cb);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_process_stream,
    bench_noise_suppressor,
    bench_pffft,
    bench_sinc_resampler,
);
criterion_main!(benches);
