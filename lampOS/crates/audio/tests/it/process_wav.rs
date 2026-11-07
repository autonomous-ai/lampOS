//! `lamp-audio process-wav`: a WAV through the mic2 capture chain (#5540).
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);
impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lamp-audio-process-wav-{}-{}",
            std::process::id(),
            DIRECTORY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

/// A 1 kHz tone at `dbfs` (RMS), `samples` long, at `rate` and `channels`.
fn tone(path: &Path, rate: u32, channels: u16, samples: usize, dbfs: f64) {
    let spec = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let amplitude = 10f64.powf(dbfs / 20.0) * 2f64.sqrt() * 32768.0;
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for n in 0..samples {
        let value =
            amplitude * (2.0 * std::f64::consts::PI * 1000.0 * n as f64 / f64::from(rate)).sin();
        for _ in 0..channels {
            writer.write_sample(value.round() as i16).unwrap();
        }
    }
    writer.finalize().unwrap();
}

fn rms_dbfs(samples: &[i16]) -> f64 {
    let power = samples
        .iter()
        .map(|&s| (f64::from(s) / 32768.0).powi(2))
        .sum::<f64>()
        / samples.len() as f64;
    10.0 * power.log10()
}

fn run(args: &[&Path], flags: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lamp-audio"))
        .arg("process-wav")
        .args(args)
        .args(flags)
        .output()
        .unwrap()
}

#[test]
fn a_tone_without_noise_suppression_keeps_its_length_and_level() {
    let directory = TestDirectory::new();
    let input = directory.0.join("tone.wav");
    let output = directory.0.join("out.wav");
    // 2.005 s: not a whole number of 10 ms blocks, so the tail is padded.
    let length = 32_080;
    tone(&input, 16_000, 1, length, -30.0);
    let result = run(&[&input, &output], &["--ns", "off"]);
    assert!(result.status.success(), "{result:?}");
    let printed = String::from_utf8_lossy(&result.stdout);
    assert!(printed.contains("blocks 201"), "{printed}");
    let mut reader = hound::WavReader::open(&output).unwrap();
    let spec = reader.spec();
    assert_eq!(
        (spec.sample_rate, spec.channels, spec.bits_per_sample),
        (16_000, 1, 16)
    );
    let samples: Vec<i16> = reader.samples::<i16>().map(Result::unwrap).collect();
    assert_eq!(samples.len(), length);
    // The steady part, past the canceller's start and the tail.
    let level = rms_dbfs(&samples[8_000..30_000]);
    assert!((level + 30.0).abs() < 3.0, "output at {level:.1} dBFS");
}

#[test]
fn the_linear_output_of_a_tone_keeps_its_length_and_level() {
    let directory = TestDirectory::new();
    let input = directory.0.join("tone.wav");
    let output = directory.0.join("linear.wav");
    tone(&input, 16_000, 1, 32_000, -30.0);
    let result = run(&[&input, &output], &["--ns", "on", "--linear"]);
    assert!(result.status.success(), "{result:?}");
    let samples: Vec<i16> = hound::WavReader::open(&output)
        .unwrap()
        .samples::<i16>()
        .map(Result::unwrap)
        .collect();
    assert_eq!(samples.len(), 32_000);
    let level = rms_dbfs(&samples[8_000..30_000]);
    assert!(
        (level + 30.0).abs() < 3.0,
        "linear output at {level:.1} dBFS"
    );
}

#[test]
fn anything_but_16_khz_mono_s16_is_refused_and_nothing_is_written() {
    let directory = TestDirectory::new();
    let output = directory.0.join("out.wav");
    for (name, rate, channels) in [("24k.wav", 24_000, 1), ("stereo.wav", 16_000, 2)] {
        let input = directory.0.join(name);
        tone(&input, rate, channels, 4_800, -30.0);
        let result = run(&[&input, &output], &["--ns", "off"]);
        assert!(!result.status.success(), "{name}: {result:?}");
        let said = String::from_utf8_lossy(&result.stderr);
        assert!(said.contains("16 kHz mono"), "{name}: {said}");
        assert!(!output.exists(), "{name}");
    }
}

#[test]
fn the_noise_suppression_flag_is_required() {
    let directory = TestDirectory::new();
    let input = directory.0.join("tone.wav");
    let output = directory.0.join("out.wav");
    tone(&input, 16_000, 1, 1_600, -30.0);
    for flags in [&[][..], &["--ns", "maybe"][..], &["--linear"][..]] {
        let result = run(&[&input, &output], flags);
        assert!(!result.status.success(), "{flags:?}: {result:?}");
        assert!(!output.exists(), "{flags:?}");
    }
}
