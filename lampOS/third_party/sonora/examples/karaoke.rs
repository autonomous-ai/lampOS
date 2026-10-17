//! Microphone loopback with echo cancellation, creating a karaoke-like effect.
//!
//! Uses cpal for audio I/O and ring buffers to shuttle samples between the
//! input/output callbacks and a processing thread.
//!
//! ```sh
//! cargo run -p sonora --features examples --example karaoke
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Observer, Producer, Split};

use sonora::config::EchoCanceller;
use sonora::{AudioProcessing, Config, StreamConfig};

#[allow(dead_code, reason = "shared helpers for multi-channel examples")]
mod common;

const SAMPLE_RATE: u32 = 48_000;
const NUM_CHANNELS: u16 = 1;
const FRAME_SIZE: usize = (SAMPLE_RATE / 100) as usize; // 10 ms

fn main() -> Result<()> {
    let running = Arc::new(AtomicBool::new(true));

    ctrlc::set_handler({
        let running = running.clone();
        move || running.store(false, Ordering::SeqCst)
    })?;

    let host = cpal::default_host();
    let input_device = host
        .default_input_device()
        .context("no input device available")?;
    let output_device = host
        .default_output_device()
        .context("no output device available")?;

    println!("Input:  {}", input_device.description()?.name());
    println!("Output: {}", output_device.description()?.name());

    let cpal_config = cpal::StreamConfig {
        channels: NUM_CHANNELS,
        sample_rate: SAMPLE_RATE,
        buffer_size: cpal::BufferSize::Default,
    };

    // Ring buffers: input callback → processing thread → output callback.
    let ring_size = FRAME_SIZE * 8;
    let (mut in_prod, mut in_cons) = HeapRb::<f32>::new(ring_size).split();
    let (mut out_prod, mut out_cons) = HeapRb::<f32>::new(ring_size).split();

    // Input stream: push mic samples into ring buffer.
    let input_stream = input_device.build_input_stream(
        cpal_config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            in_prod.push_slice(data);
        },
        |err| eprintln!("input error: {err}"),
        None,
    )?;

    // Output stream: pull processed samples from ring buffer.
    let output_stream = output_device.build_output_stream(
        cpal_config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let filled = out_cons.pop_slice(data);
            data[filled..].fill(0.0);
        },
        |err| eprintln!("output error: {err}"),
        None,
    )?;

    input_stream.play()?;
    output_stream.play()?;

    // Processing thread: read 10 ms frames, apply AEC, push to output.
    let running_proc = running.clone();
    let proc_thread = thread::spawn(move || {
        let stream_config = StreamConfig::new(SAMPLE_RATE, NUM_CHANNELS);
        let config = Config {
            echo_canceller: Some(EchoCanceller::default()),
            ..Default::default()
        };
        let mut apm = AudioProcessing::builder()
            .config(config)
            .capture_config(stream_config)
            .render_config(stream_config)
            .build();

        let mut input_buf = vec![0.0f32; FRAME_SIZE];
        let mut capture_out = vec![0.0f32; FRAME_SIZE];
        let mut render_out = vec![0.0f32; FRAME_SIZE];

        while running_proc.load(Ordering::SeqCst) {
            if in_cons.occupied_len() < FRAME_SIZE {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            in_cons.pop_slice(&mut input_buf);

            // Process capture (apply AEC to mic input).
            apm.process_capture_f32(&[&input_buf], &mut [&mut capture_out])
                .unwrap();

            // Tell AEC what we are sending to the speakers.
            apm.process_render_f32(&[&capture_out], &mut [&mut render_out])
                .unwrap();

            out_prod.push_slice(&capture_out);
        }
    });

    println!("Looping mic -> AEC -> speakers (Ctrl+C to stop)");

    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));
    }

    drop(input_stream);
    drop(output_stream);
    proc_thread.join().unwrap();

    println!("\nDone.");
    Ok(())
}
