//! Record microphone audio, process it through sonora, and write WAV files.
//!
//! Writes both raw (unprocessed) and processed audio to separate files so you
//! can compare them.
//!
//! ```sh
//! cargo run -p sonora --features examples --example recording -- --duration 5 --ns --agc
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::WavWriter;
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Observer, Producer, Split};

use sonora::config::{EchoCanceller, GainController2, NoiseSuppression};
use sonora::{AudioProcessing, Config, StreamConfig};

#[allow(dead_code, reason = "shared helpers for multi-channel examples")]
mod common;

const SAMPLE_RATE: u32 = 48_000;
const NUM_CHANNELS: u16 = 1;
const FRAME_SIZE: usize = (SAMPLE_RATE / 100) as usize; // 10 ms

#[derive(Parser, Debug)]
#[command(about = "Record and process microphone audio through sonora")]
struct Args {
    /// Recording duration in seconds.
    #[arg(short, long, default_value_t = 5)]
    duration: u64,

    /// Path for the raw (unprocessed) recording.
    #[arg(long, default_value = "raw.wav")]
    raw_output: String,

    /// Path for the processed recording.
    #[arg(long, default_value = "processed.wav")]
    processed_output: String,

    /// Enable echo cancellation.
    #[arg(long)]
    aec: bool,

    /// Enable noise suppression.
    #[arg(long)]
    ns: bool,

    /// Enable automatic gain control.
    #[arg(long)]
    agc: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let running = Arc::new(AtomicBool::new(true));

    ctrlc::set_handler({
        let running = running.clone();
        move || running.store(false, Ordering::SeqCst)
    })?;

    let host = cpal::default_host();
    let input_device = host
        .default_input_device()
        .context("no input device available")?;
    println!("Recording from: {}", input_device.description()?.name());

    let cpal_config = cpal::StreamConfig {
        channels: NUM_CHANNELS,
        sample_rate: SAMPLE_RATE,
        buffer_size: cpal::BufferSize::Default,
    };

    let ring_size = FRAME_SIZE * 8;
    let (mut prod, mut cons) = HeapRb::<f32>::new(ring_size).split();

    let input_stream = input_device.build_input_stream(
        cpal_config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            prod.push_slice(data);
        },
        |err| eprintln!("input error: {err}"),
        None,
    )?;

    input_stream.play()?;

    // Build config from CLI flags.
    let config = Config {
        echo_canceller: if args.aec {
            Some(EchoCanceller::default())
        } else {
            None
        },
        noise_suppression: if args.ns {
            Some(NoiseSuppression::default())
        } else {
            None
        },
        gain_controller2: if args.agc {
            Some(GainController2::default())
        } else {
            None
        },
        ..Default::default()
    };

    let stream_config = StreamConfig::new(SAMPLE_RATE, NUM_CHANNELS);
    let mut apm = AudioProcessing::builder()
        .config(config)
        .capture_config(stream_config)
        .render_config(stream_config)
        .build();

    let spec = hound::WavSpec {
        channels: NUM_CHANNELS,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut raw_writer = WavWriter::create(&args.raw_output, spec)?;
    let mut proc_writer = WavWriter::create(&args.processed_output, spec)?;

    println!(
        "Recording for {} seconds (Ctrl+C to stop early)...",
        args.duration
    );

    let deadline = Instant::now() + Duration::from_secs(args.duration);
    let mut input_buf = vec![0.0f32; FRAME_SIZE];
    let mut output_buf = vec![0.0f32; FRAME_SIZE];

    while running.load(Ordering::SeqCst) && Instant::now() < deadline {
        if cons.occupied_len() < FRAME_SIZE {
            thread::sleep(Duration::from_millis(1));
            continue;
        }

        cons.pop_slice(&mut input_buf);

        // Write raw samples.
        for &s in &input_buf {
            raw_writer.write_sample(s)?;
        }

        // Process through sonora.
        apm.process_capture_f32(&[&input_buf], &mut [&mut output_buf])
            .unwrap();

        // Write processed samples.
        for &s in &output_buf {
            proc_writer.write_sample(s)?;
        }
    }

    raw_writer.finalize()?;
    proc_writer.finalize()?;

    println!("Wrote {} and {}", args.raw_output, args.processed_output);

    Ok(())
}
