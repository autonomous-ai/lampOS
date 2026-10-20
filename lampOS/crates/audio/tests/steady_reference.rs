//! A rig with no lamp in it, for the echo canceller as `lamp_audio` wraps
//! it (docs/steady-reference.md).
//!
//! - **The lamp's voice**: speech-like sound made here (a voice source
//!   through moving formants, consonant noise, syllables and pauses) at
//!   24 kHz, as the speaker is given it.
//! - **The speaker** plays it on a clock 98 ppm fast, as measured on
//!   lamp-4ace, and accepts it in 240-frame packets with a fixed queue.
//! - **The path** to the microphone: a direct sound and four reflections,
//!   and a delay that slides and steps as the lamp's second microphone was
//!   recorded doing on 2026-10-14 (docs/duplex-listening.md): 85.6 ms, then
//!   eight steps of 5.4 ms to 128.8 ms between 10 and 37 s, then one step
//!   back and forth every 3.5 s.
//! - **The microphone** samples at 16 kHz on a clock 200 ppm slow, with a
//!   noise floor at -63 dBFS. The lamp's echo reaches it at -14 dBFS.
//! - **A person** speaks over the lamp six times, 1.5 s each, at a known
//!   level against the echo.
//! - **The voice's side** is the capture-clocked reference as the lamp runs
//!   it (`--reference-clock capture`): accepted packets into
//!   `lamp_audio::drift::Resampler`, one render block read for each capture
//!   block at the rate `drift::Controller` asks, as
//!   `ReferenceAssembly::render_for_capture` does in `crates/voice`.
//!
//! Measured: how much of the echo the canceller takes off while only the
//! lamp speaks, how much of the person it leaves while both speak, and
//! where the echo sits as the canceller sees it, from the rig's own ground
//! truth. The measures are printed by the ignored test
//! `steady_reference_measures` (`--release --include-ignored --nocapture`);
//! the tests that run by default hold the findings that must not regress.
use lamp_audio::{
    CAPTURE_SAMPLES, CaptureBlock, EchoProcessor, RENDER_SAMPLES, Reference, RenderBlock,
    drift::{Controller, Resampler, TARGET_FRAMES},
    steady::{HOLD_FRAMES, SteadyReference},
};
use std::f64::consts::PI;

const RENDER_RATE: f64 = 24_000.0;
const CAPTURE_RATE: f64 = 16_000.0;
const SPEAKER_PPM: f64 = 98.0;
const MICROPHONE_PPM: f64 = -200.0;
/// Frames the speaker holds accepted and unplayed: two packets.
const QUEUE_FRAMES: f64 = 480.0;
/// Where the person speaks over the lamp, seconds.
const OVERLAPS: [f64; 6] = [12.0, 20.5, 27.0, 34.0, 45.0, 53.0];
const OVERLAP_SECONDS: f64 = 1.5;
/// Seconds before anything is measured: the canceller's first learning.
const WARM_UP: f64 = 6.0;

/// xorshift64*: the same sounds on every machine.
struct Random(u64);
impl Random {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }
    fn between(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.next()
    }
}

/// A two-pole resonance at `frequency` Hz with bandwidth `width` Hz.
#[derive(Default, Clone, Copy)]
struct Resonance {
    a1: f64,
    a2: f64,
    gain: f64,
    y1: f64,
    y2: f64,
}
impl Resonance {
    fn tune(&mut self, frequency: f64, width: f64, rate: f64) {
        let radius = (-PI * width / rate).exp();
        self.a1 = 2.0 * radius * (2.0 * PI * frequency / rate).cos();
        self.a2 = -radius * radius;
        self.gain = 1.0 - radius;
    }
    fn step(&mut self, x: f64) -> f64 {
        let y = self.gain * x + self.a1 * self.y1 + self.a2 * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Vowel formants, Hz.
const VOWELS: [[f64; 3]; 6] = [
    [730.0, 1_090.0, 2_440.0],
    [270.0, 2_290.0, 3_010.0],
    [300.0, 870.0, 2_240.0],
    [530.0, 1_840.0, 2_480.0],
    [640.0, 1_190.0, 2_390.0],
    [440.0, 1_020.0, 2_240.0],
];

/// Speech-like sound at `rate` for `seconds`, its voice at `pitch` Hz, made
/// from `seed`, sounding only where `talking` says. Unit peak level.
fn speech(
    rate: f64,
    seconds: f64,
    pitch: f64,
    seed: u64,
    talking: &dyn Fn(f64) -> bool,
) -> Vec<f64> {
    let length = (seconds * rate) as usize;
    let mut out = vec![0.0; length];
    let mut random = Random(seed);
    let (mut formants, mut glottis) = ([Resonance::default(); 3], [Resonance::default(); 2]);
    glottis[0].tune(0.0, 700.0, rate);
    glottis[1].tune(0.0, 700.0, rate);
    let mut index = 0usize;
    let mut phase = 0.0;
    let mut previous_flow = 0.0;
    let mut syllables_left = 0;
    while index < length {
        let t = index as f64 / rate;
        if !talking(t) {
            index += (0.02 * rate) as usize;
            continue;
        }
        if syllables_left == 0 {
            syllables_left = 3 + (random.next() * 4.0) as usize;
            index += (random.between(0.12, 0.25) * rate) as usize;
            continue;
        }
        syllables_left -= 1;
        let vowel = VOWELS[(random.next() * VOWELS.len() as f64) as usize % VOWELS.len()];
        let next = VOWELS[(random.next() * VOWELS.len() as f64) as usize % VOWELS.len()];
        let consonant = random.between(0.0, 0.06) * rate;
        let duration = random.between(0.12, 0.26) * rate;
        let level = random.between(0.5, 1.0);
        let contour = random.between(-0.15, 0.15);
        let total = (consonant + duration) as usize;
        for step in 0..total {
            let Some(slot) = out.get_mut(index + step) else {
                break;
            };
            let along = step as f64 / total as f64;
            let envelope = (PI * along).sin().powf(0.6) * level;
            let mut sample = 0.0;
            if (step as f64) < consonant {
                // A fricative: noise, sharpened towards the high end.
                sample += (random.next() - 0.5) * 0.25;
            } else {
                let f0 = pitch * (1.0 + contour * (along - 0.5) + 0.01 * (random.next() - 0.5));
                phase += f0 / rate;
                let pulse = if phase >= 1.0 {
                    phase -= 1.0;
                    1.0
                } else {
                    0.0
                };
                // The glottal flow, then its change: what radiates from the
                // lips has no steady part.
                let shaped = glottis[0].step(pulse);
                let flow = glottis[1].step(shaped);
                let source = flow - previous_flow;
                previous_flow = flow;
                for (formant, (from, to)) in formants.iter_mut().zip(vowel.iter().zip(next)) {
                    formant.tune(from + (to - from) * along, 90.0, rate);
                }
                sample += formants
                    .iter_mut()
                    .map(|formant| formant.step(source * 400.0))
                    .sum::<f64>();
            }
            *slot = sample * envelope;
        }
        index += total + (random.between(0.0, 0.05) * rate) as usize;
    }
    let peak = out.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-9);
    out.iter_mut().for_each(|v| *v /= peak);
    out
}

/// Band-limited reading of a 24 kHz signal at a fractional frame, cut at
/// 7 kHz as the microphone's own filter would before 16 kHz.
///
/// Each tap's distance from the frame is the frame's fraction less a whole
/// number of taps, so the sine of its sinc and the cosine of its window come
/// by angle addition from the fraction's and a table's: four sines and
/// cosines a reading rather than sixty-four. Building a 45 s scene took about
/// 2.3 s of the two moving-echo tests on ARM64 before (#1806).
/// `the_tabled_reader_reads_as_the_direct_one` holds it to the direct sum.
struct Reader {
    taps: i64,
    cut: f64,
    /// For each tap from `1 - taps` to `taps`: the sine and cosine of
    /// `PI * cut * tap`, and of `PI * tap / taps`.
    sinc_turns: Vec<(f64, f64)>,
    window_turns: Vec<(f64, f64)>,
}
impl Reader {
    fn new(taps: i64, cut: f64) -> Self {
        let turns = |scale: f64| {
            (-taps + 1..=taps)
                .map(|tap| (scale * tap as f64).sin_cos())
                .collect()
        };
        Self {
            taps,
            cut,
            sinc_turns: turns(PI * cut),
            window_turns: turns(PI / taps as f64),
        }
    }

    fn at(&self, signal: &[f64], position: f64) -> f64 {
        let whole = position.floor() as i64;
        let fraction = position - whole as f64;
        let (sin_a, cos_a) = (PI * self.cut * fraction).sin_cos();
        let (sin_w, cos_w) = (PI * fraction / self.taps as f64).sin_cos();
        let mut sum = 0.0;
        for (slot, tap) in (-self.taps + 1..=self.taps).enumerate() {
            let index = whole + tap;
            if index < 0 || index as usize >= signal.len() {
                continue;
            }
            let x = position - index as f64;
            // sin(a - b) and cos(w - v), with b and v the tap's.
            let (sin_b, cos_b) = self.sinc_turns[slot];
            let sinc = if x.abs() < 1e-9 {
                1.0
            } else {
                (sin_a * cos_b - cos_a * sin_b) / (PI * self.cut * x)
            };
            let (sin_v, cos_v) = self.window_turns[slot];
            let window = 0.5 + 0.5 * (cos_w * cos_v + sin_w * sin_v);
            sum += signal[index as usize] * self.cut * sinc * window;
        }
        sum
    }

    /// The same sum, each tap's sine and cosine taken directly.
    fn at_directly(&self, signal: &[f64], position: f64) -> f64 {
        let whole = position.floor() as i64;
        let mut sum = 0.0;
        for tap in -self.taps + 1..=self.taps {
            let index = whole + tap;
            if index < 0 || index as usize >= signal.len() {
                continue;
            }
            let x = position - index as f64;
            let sinc = if x.abs() < 1e-9 {
                1.0
            } else {
                (PI * self.cut * x).sin() / (PI * self.cut * x)
            };
            let window = 0.5 + 0.5 * (PI * x / self.taps as f64).cos();
            sum += signal[index as usize] * self.cut * sinc * window;
        }
        sum
    }
}

#[test]
fn the_tabled_reader_reads_as_the_direct_one() {
    let reader = Reader::new(16, 7_000.0 / 12_000.0);
    let signal = speech(RENDER_RATE, 2.0, 205.0, 0x1a4b, &|_| true);
    let mut worst: f64 = 0.0;
    // Whole frames, fractions, both ends and a step past them.
    for step in 0..20_000 {
        let position = -20.0 + step as f64 * 2.4037;
        let tabled = reader.at(&signal, position);
        let direct = reader.at_directly(&signal, position);
        worst = worst.max((tabled - direct).abs());
    }
    for position in [0.0, 1.0, 100.0, 100.5, signal.len() as f64 - 1.0] {
        worst =
            worst.max((reader.at(&signal, position) - reader.at_directly(&signal, position)).abs());
    }
    assert!(worst < 1e-12, "{worst}");
}

/// The echo's delay after the speaker plays a frame, in seconds, at `t`.
fn moving_delay(t: f64) -> f64 {
    const STEP: f64 = 0.0054;
    let steps = [10.0, 13.9, 17.7, 21.6, 25.4, 29.3, 33.1, 37.0]
        .iter()
        .filter(|&&at| t >= at)
        .count() as f64;
    let back = t >= 40.0 && (((t - 40.0) / 3.5) as u64).is_multiple_of(2);
    0.0856 + STEP * steps - if back { STEP } else { 0.0 }
}

fn still_delay(_t: f64) -> f64 {
    0.0856
}

#[derive(Clone, Copy)]
struct Scene {
    seconds: f64,
    /// The person against the lamp's echo at the microphone, dB.
    person_db: f64,
    delay: fn(f64) -> f64,
    reference: Reference,
    /// Part of the echo that is not a linear copy, 0 to 1.
    distortion: f64,
}

#[derive(Debug)]
struct Measures {
    /// Median over 1 s windows of only the lamp speaking, dB taken off.
    echo_removed_db: f64,
    /// The person's part of the output against the person, while both
    /// speak, dB (0 is all of the person kept).
    person_kept_db: f64,
    /// The same while the lamp is silent: the canceller's cost to a person
    /// it has no reason to touch.
    person_alone_kept_db: f64,
    /// Where the echo sat as the canceller saw it, ms after the reference:
    /// lowest, highest and the share of blocks within 1 ms of the median.
    seen_delay_ms: (f64, f64, f64),
    /// The canceller's own estimate of the delay, lowest and highest, ms.
    internal_ms: (i32, i32),
}

fn overlapping(t: f64) -> bool {
    OVERLAPS
        .iter()
        .any(|&at| t >= at && t < at + OVERLAP_SECONDS)
}

const SPEAKER: f64 = RENDER_RATE * (1.0 + SPEAKER_PPM * 1e-6);
const MICROPHONE: f64 = CAPTURE_RATE * (1.0 + MICROPHONE_PPM * 1e-6);

/// One scene's sound: what the speaker is given and what the microphone
/// hears.
struct Sound {
    lamp_pcm: Vec<i16>,
    microphone_pcm: Vec<i16>,
    /// The person's part of the microphone, as heard.
    near: Vec<f64>,
    seconds: f64,
}

impl Sound {
    /// The lamp is silent from 57.5 s, and in the last two seconds of any
    /// scene, so that a person can be heard alone.
    fn lamp_talks(&self, t: f64) -> bool {
        (0.5..57.5).contains(&t) && t < self.seconds - 2.0
    }

    fn person_talks(&self, t: f64) -> bool {
        overlapping(t) || (t >= self.seconds - 1.5 && t < self.seconds)
    }

    fn new(scene: &Scene) -> Self {
        let seconds = scene.seconds;
        let lamp_talks = move |t: f64| (0.5..57.5).contains(&t) && t < seconds - 2.0;
        let person_talks = move |t: f64| overlapping(t) || (t >= seconds - 1.5 && t < seconds);
        let lamp = speech(RENDER_RATE, seconds + 1.0, 205.0, 0x1a4b, &lamp_talks);
        let person = speech(CAPTURE_RATE, seconds + 1.0, 118.0, 0x77e1, &person_talks);
        let reader = Reader::new(16, 7_000.0 / 12_000.0);
        let reflections = [
            (0.0, 1.0),
            (0.0012, 0.5),
            (0.003, -0.3),
            (0.007, 0.2),
            (0.015, 0.1),
        ];
        // The echo at the microphone, then scaled so that it sits at
        // -14 dBFS while the lamp speaks.
        let samples = (seconds * CAPTURE_RATE) as usize;
        let mut echo = vec![0.0; samples];
        for (index, slot) in echo.iter_mut().enumerate() {
            let t = index as f64 / MICROPHONE;
            let mut sum = 0.0;
            for &(after, gain) in &reflections {
                let played = t - (scene.delay)(t) - after;
                if played > 0.0 {
                    sum += gain * reader.at(&lamp, played * SPEAKER);
                }
            }
            *slot = sum + scene.distortion * (3.0 * sum).tanh() / 3.0 - scene.distortion * sum;
        }
        let level = |signal: &[f64], keep: &dyn Fn(f64) -> bool| {
            let (mut energy, mut count) = (0.0, 0usize);
            for (index, value) in signal.iter().enumerate() {
                if keep(index as f64 / CAPTURE_RATE) {
                    energy += value * value;
                    count += 1;
                }
            }
            (energy / count.max(1) as f64).sqrt()
        };
        let echo_scale =
            32_768.0 * 10f64.powf(-14.0 / 20.0) / level(&echo, &|t| lamp_talks(t) && t > 1.0);
        echo.iter_mut().for_each(|v| *v *= echo_scale);
        let person_rms = level(&person, &|t| person_talks(t));
        let person_scale = 32_768.0 * 10f64.powf((-14.0 + scene.person_db) / 20.0) / person_rms;
        let mut noise = Random(0x5eed);
        let floor = 32_768.0 * 10f64.powf(-63.0 / 20.0) * 3f64.sqrt();
        let near: Vec<f64> = person
            .iter()
            .take(samples)
            .map(|v| v * person_scale)
            .collect();
        let microphone_pcm = (0..samples)
            .map(|index| {
                let value = echo[index] + near[index] + floor * (2.0 * noise.next() - 1.0);
                value.round().clamp(-32_768.0, 32_767.0) as i16
            })
            .collect();
        Self {
            lamp_pcm: lamp.iter().map(|v| (v * 20_000.0).round() as i16).collect(),
            microphone_pcm,
            near,
            seconds,
        }
    }

    fn blocks(&self) -> usize {
        self.microphone_pcm.len() / CAPTURE_SAMPLES
    }

    fn capture(&self, block: usize) -> CaptureBlock {
        self.microphone_pcm[block * CAPTURE_SAMPLES..(block + 1) * CAPTURE_SAMPLES]
            .try_into()
            .unwrap()
    }
}

/// The voice's side, capture-clocked, as `ReferenceAssembly` drives it.
struct VoiceSide {
    ring: Resampler,
    controller: Controller,
    accepted: usize,
}

impl VoiceSide {
    fn new() -> Self {
        let mut ring = Resampler::new();
        // The ring starts at its target, as the voice primes it.
        ring.push(&vec![0; TARGET_FRAMES]).unwrap();
        Self {
            ring,
            controller: Controller::default(),
            accepted: 0,
        }
    }

    /// The render block handed to the canceller with capture block `block`,
    /// and the true time at that block's end.
    fn next(&mut self, lamp_pcm: &[i16], block: usize) -> (RenderBlock, f64) {
        let end = (block + 1) as f64 * CAPTURE_SAMPLES as f64 / MICROPHONE;
        // Whole packets the speaker has accepted by now.
        let exact = end * SPEAKER + QUEUE_FRAMES;
        let through = (exact / RENDER_SAMPLES as f64).floor() as usize * RENDER_SAMPLES;
        while self.accepted < through {
            self.ring
                .push(&lamp_pcm[self.accepted..self.accepted + RENDER_SAMPLES])
                .unwrap();
            self.accepted += RENDER_SAMPLES;
        }
        let mut render = [0; RENDER_SAMPLES];
        self.ring.take(&mut render).expect("reference ready");
        let owed = exact - through as f64;
        let ppm = self.controller.observe(self.ring.unread() as f64 + owed);
        self.ring.set_ppm(ppm);
        (render, end)
    }

    /// Where the echo sits after the newest frame handed, as the canceller
    /// sees it, in ms: positive is later. `delayed_ms` is what a steady
    /// reference adds.
    fn seen_ms(&self, scene: &Scene, end: f64, delayed_ms: f64) -> f64 {
        let handed = self.ring.consumed() as f64 - TARGET_FRAMES as f64 - delayed_ms * 24.0;
        let heard = (end - (scene.delay)(end)) * SPEAKER;
        (handed - heard) / 24.0
    }
}

fn run(scene: Scene) -> Measures {
    let sound = Sound::new(&scene);
    let seconds = sound.seconds;
    // The person alone through a canceller that hears no lamp: what the
    // canceller's own filtering leaves of a person, to measure against.
    let alone: Vec<f64> = {
        let mut processor = EchoProcessor::new(false);
        let mut out = Vec::with_capacity(sound.near.len());
        for block in sound.near.chunks_exact(CAPTURE_SAMPLES) {
            processor.render(&[0; RENDER_SAMPLES]).unwrap();
            let pcm: CaptureBlock =
                std::array::from_fn(|i| block[i].round().clamp(-32_768.0, 32_767.0) as i16);
            out.extend(
                processor
                    .capture(&pcm, 5)
                    .unwrap()
                    .iter()
                    .map(|&v| f64::from(v)),
            );
        }
        out
    };
    let mut echo_processor = EchoProcessor::with_reference(false, scene.reference);
    let mut voice = VoiceSide::new();
    let mut output = Vec::with_capacity(sound.microphone_pcm.len());
    let (mut seen, mut internal) = (Vec::new(), Vec::new());
    for block in 0..sound.blocks() {
        let (render, end) = voice.next(&sound.lamp_pcm, block);
        echo_processor.render(&render).unwrap();
        let clean = echo_processor.capture(&sound.capture(block), 5).unwrap();
        output.extend_from_slice(&clean);
        if end > WARM_UP && sound.lamp_talks(end) {
            let delayed = echo_processor.steadiness().map_or(0.0, |s| s.delay_ms);
            seen.push(voice.seen_ms(&scene, end, delayed));
            if let Some(alignment) = echo_processor.internal_alignment_ms() {
                internal.push(alignment);
            }
        }
    }

    // Echo removed: 1 s windows of only the lamp speaking.
    let mut removed = Vec::new();
    let mut second = WARM_UP;
    while second + 1.0 <= seconds {
        let quiet_person = !(0..10).any(|i| sound.person_talks(second + i as f64 * 0.1));
        if quiet_person && sound.lamp_talks(second) && sound.lamp_talks(second + 0.99) {
            let range = (second * CAPTURE_RATE) as usize..((second + 1.0) * CAPTURE_RATE) as usize;
            let before: f64 = sound.microphone_pcm[range.clone()]
                .iter()
                .map(|&v| f64::from(v).powi(2))
                .sum();
            let after: f64 = output[range].iter().map(|&v| f64::from(v).powi(2)).sum();
            removed.push(10.0 * (before / after.max(1.0)).log10());
        }
        second += 1.0;
    }
    removed.sort_by(f64::total_cmp);
    let kept = |keep: &dyn Fn(f64) -> bool| {
        let (mut product, mut energy) = (0.0, 0.0);
        for (index, &value) in output.iter().enumerate() {
            if keep(index as f64 / CAPTURE_RATE) {
                product += f64::from(value) * alone[index];
                energy += alone[index] * alone[index];
            }
        }
        20.0 * (product / energy).abs().max(1e-6).log10()
    };
    let (low, high, within) = spread(&seen, None);
    Measures {
        echo_removed_db: removed[removed.len() / 2],
        person_kept_db: kept(&|t| overlapping(t) && t > WARM_UP),
        person_alone_kept_db: kept(&|t| t >= seconds - 1.5),
        seen_delay_ms: (low, high, within),
        internal_ms: (
            internal.iter().copied().min().unwrap_or(0),
            internal.iter().copied().max().unwrap_or(0),
        ),
    }
}

/// Lowest, highest, and the share within 1 ms of `centre` (the median when
/// none is given).
fn spread(seen: &[f64], centre: Option<f64>) -> (f64, f64, f64) {
    let mut sorted = seen.to_vec();
    sorted.sort_by(f64::total_cmp);
    let centre = centre.unwrap_or(sorted[sorted.len() / 2]);
    let within =
        seen.iter().filter(|&&d| (d - centre).abs() <= 1.0).count() as f64 / seen.len() as f64;
    (sorted[0], sorted[sorted.len() - 1], within)
}

/// Where the echo sits as the canceller would see it, block by block after
/// the warm-up, with the reference as given or through a steady reference
/// alone: no canceller, so it runs quickly in a debug build.
fn follow_only(scene: Scene) -> Vec<f64> {
    let sound = Sound::new(&scene);
    let mut steady = (scene.reference == Reference::Steady).then(SteadyReference::new);
    let mut voice = VoiceSide::new();
    let mut seen = Vec::new();
    for block in 0..sound.blocks() {
        let (render, end) = voice.next(&sound.lamp_pcm, block);
        let delayed = match steady.as_mut() {
            Some(steady) => {
                steady.render(&render);
                steady.capture(&sound.capture(block));
                steady.steadiness().delay_ms
            }
            None => 0.0,
        };
        if end > WARM_UP && sound.lamp_talks(end) {
            seen.push(voice.seen_ms(&scene, end, delayed));
        }
    }
    seen
}

fn scene(delay: fn(f64) -> f64, reference: Reference) -> Scene {
    Scene {
        seconds: 60.0,
        person_db: 0.0,
        delay,
        reference,
        distortion: 0.0,
    }
}

/// The rig reproduces what was recorded: as given, the echo moves over the
/// whole of the measured range and more, the capture-clocked ring's own
/// wander added to the steps.
#[test]
fn as_given_the_echo_moves_as_it_did_on_the_lamp() {
    let seen = follow_only(Scene {
        seconds: 45.0,
        ..scene(moving_delay, Reference::AsGiven)
    });
    let (low, high, within) = spread(&seen, None);
    assert!(high - low > 40.0, "{low} to {high}");
    assert!(within < 0.2, "{within}");
}

/// The steady reference holds the same moving echo where the canceller first
/// found it: 20 ms after the reference, within a millisecond for most of the
/// time and never long more than one step away.
#[test]
fn the_steady_reference_holds_a_moving_echo_in_one_place() {
    let seen = follow_only(Scene {
        seconds: 45.0,
        ..scene(moving_delay, Reference::Steady)
    });
    let hold = HOLD_FRAMES / 24.0;
    let (_, _, within) = spread(&seen, Some(hold));
    let near = seen.iter().filter(|&&d| (d - hold).abs() <= 6.0).count() as f64 / seen.len() as f64;
    eprintln!(
        "within 1 ms: {:.0}%, within one step: {:.0}%",
        within * 100.0,
        near * 100.0
    );
    assert!(within > 0.7, "{within}");
    assert!(near > 0.97, "{near}");
}

/// What the steady reference costs per 10 ms block on this machine: its
/// render and capture calls together, over a moving scene.
fn cost_per_block_us(scene: Scene) -> (f64, f64, f64) {
    let sound = Sound::new(&scene);
    let mut steady = SteadyReference::new();
    let mut voice = VoiceSide::new();
    let mut times = Vec::with_capacity(sound.blocks());
    for block in 0..sound.blocks() {
        let (render, _) = voice.next(&sound.lamp_pcm, block);
        let capture = sound.capture(block);
        let started = std::time::Instant::now();
        steady.render(&render);
        steady.capture(&capture);
        times.push(started.elapsed().as_secs_f64() * 1e6);
    }
    times.sort_by(f64::total_cmp);
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    (at(0.5), at(0.99), at(1.0))
}

#[test]
#[ignore = "about a minute of simulated sound per scene; run with --release"]
fn steady_reference_measures() {
    let (median, p99, worst) = cost_per_block_us(scene(moving_delay, Reference::Steady));
    println!(
        "steady reference, render and capture calls per 10 ms block: median {median:.0} us, 99th percentile {p99:.0} us, worst {worst:.0} us"
    );
    println!(
        "| Echo | Reference | Person against echo | Echo removed | Person kept, over the lamp | Person kept, alone | Echo as the canceller sees it (ms) | Within 1 ms | Canceller's estimate (ms) |"
    );
    println!("|---|---|---|---|---|---|---|---|---|");
    for (distortion, delay_name, delay) in [
        (0.0, "still", still_delay as fn(f64) -> f64),
        (0.0, "moving", moving_delay),
        (0.5, "moving, half of it not linear", moving_delay),
    ] {
        for reference in [Reference::AsGiven, Reference::Steady] {
            for person_db in [0.0, -15.0] {
                let measures = run(Scene {
                    person_db,
                    distortion,
                    ..scene(delay, reference)
                });
                println!(
                    "| {delay_name} | {reference:?} | {person_db} dB | {:.1} dB | {:.1} dB | {:.1} dB | {:.1} to {:.1} | {:.0}% | {} to {} |",
                    measures.echo_removed_db,
                    measures.person_kept_db,
                    measures.person_alone_kept_db,
                    measures.seen_delay_ms.0,
                    measures.seen_delay_ms.1,
                    measures.seen_delay_ms.2 * 100.0,
                    measures.internal_ms.0,
                    measures.internal_ms.1,
                );
            }
        }
    }
}
