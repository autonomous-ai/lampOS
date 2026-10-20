//! The echo evidence against scripted scenes run through the real canceller.
//!
//! Every signal is a function of time, sampled at 24 kHz for the speaker and
//! at 16 kHz for the microphone, so no resampler stands between a scene and
//! what is measured. The echo path is a delayed, inverted, attenuated copy
//! with one reflection: a simplification of a speaker beside a microphone,
//! not a room. Nothing here is acoustic evidence.
use lamp_audio::{
    BLOCK_MS, CAPTURE_RATE, CAPTURE_SAMPLES, CaptureBlock, EchoProcessor, RENDER_RATE,
    RENDER_SAMPLES, RenderBlock,
    evidence::{ACTIVITY_BLOCKS, EchoEvidence, EchoObserver, Level, MATCH_PERCENT},
    float_to_pcm,
};

const DELAY: f64 = 0.1135;
const DIRECT: f32 = -0.5;
const REFLECTED: f32 = 0.12;
const REFLECTION: f64 = 0.004;
/// Echo amplitude at the microphone per unit of speaker amplitude. The two
/// paths are 4 ms apart and the source has no memory that long, so their
/// energies add.
const ECHO_GAIN: f32 = 0.514_198;

fn noise(seed: u64, index: u64) -> f32 {
    let mut x = index
        .wrapping_add(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15))
        .wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((x ^ (x >> 31)) >> 40) as f32 / 8_388_608.0 - 1.0
}
/// Band-limited noise with a syllable-rate envelope. Two seeds are two
/// unrelated talkers.
fn voice(seed: u64, seconds: f64) -> f32 {
    if seconds < 0.0 {
        return 0.0;
    }
    let position = seconds * 8_000.0;
    let (index, part) = (position.floor() as u64, position.fract() as f32);
    let carrier = noise(seed, index) * (1.0 - part) + noise(seed, index + 1) * part;
    let syllable = (seconds * 3.1 * std::f64::consts::TAU + seed as f64)
        .sin()
        .abs() as f32;
    carrier * (0.3 + 0.7 * syllable)
}

/// The two-path echo: delay in seconds and gain.
const SIMPLE_PATH: &[(f64, f32)] = &[(DELAY, DIRECT), (DELAY + REFLECTION, REFLECTED)];

/// The path from the speaker to the microphone beside it as the first device
/// recording had it: least-squares coefficients of the capture on the
/// accepted render at the analysis rate, 250 us apart from 103.5 ms, scaled
/// by a quarter so the scene does not clip. A property of one lamp, not
/// audio.
const MEASURED_PATH: &[(f64, f32)] = &{
    const TAPS: [f32; 21] = [
        0.298, 0.491, 0.237, -1.177, -0.761, 0.0, 0.279, 0.521, 0.449, 0.520, 0.0, -0.551, -0.412,
        0.0, 0.297, 0.0, 0.0, 0.0, 0.0, -0.345, -0.278,
    ];
    let mut path = [(0.0, 0.0); 21];
    let mut tap = 0;
    while tap < 21 {
        path[tap] = (0.1035 + tap as f64 * 0.000_25, TAPS[tap] * 0.25);
        tap += 1;
    }
    path
};

/// A voiced sound: nine harmonics of a pitch that glides around 195 Hz, under
/// the syllable envelope of `voice`, with a little breath. It repeats every
/// five milliseconds, which is what makes an alignment one pitch period from
/// the true one look right for as long as the pitch holds still.
fn pitched(seconds: f64) -> f32 {
    if seconds < 0.0 {
        return 0.0;
    }
    use std::f64::consts::TAU;
    let glide = 0.08 / (TAU * 0.7) * (TAU * 0.7 * seconds).cos();
    let phase = TAU * 195.0 * (seconds - glide);
    let harmonics = [1.0, 0.8, 0.9, 0.6, 0.4, 0.5, 0.3, 0.2, 0.15]
        .iter()
        .enumerate()
        .map(|(index, amplitude)| amplitude * (phase * (index + 1) as f64).sin())
        .sum::<f64>()
        / 4.85;
    let syllable = (seconds * 3.1 * TAU).sin().abs();
    let breath = 0.05 * noise(5, (seconds * 8_000.0) as u64);
    (harmonics * (0.3 + 0.7 * syllable)) as f32 + breath
}

struct Scene {
    /// What Lamp says, as a function of time.
    source: fn(f64) -> f32,
    /// How it reaches the microphone.
    path: &'static [(f64, f32)],
    /// Speaker amplitude at a time, zero when Lamp is not speaking.
    speaker: Box<dyn Fn(f64) -> f32>,
    /// A person's amplitude at the microphone at a time.
    person: Box<dyn Fn(f64) -> f32>,
    /// Microphone clock relative to the speaker clock, minus one.
    drift: f64,
    /// Vary how far render acceptance runs ahead of capture, as ticks do.
    jitter: bool,
}
impl Scene {
    fn playback(from: f64, until: f64) -> Self {
        Self {
            source: |t| voice(1, t),
            path: SIMPLE_PATH,
            speaker: Box::new(move |t| if (from..until).contains(&t) { 0.3 } else { 0.0 }),
            person: Box::new(|_| 0.0),
            drift: 0.0,
            jitter: true,
        }
    }
    fn lamp(&self, seconds: f64) -> f32 {
        (self.speaker)(seconds) * (self.source)(seconds)
    }
    fn render(&self, block: usize) -> RenderBlock {
        std::array::from_fn(|offset| {
            float_to_pcm(
                self.lamp((block * RENDER_SAMPLES + offset) as f64 / f64::from(RENDER_RATE)),
            )
        })
    }
    fn capture(&self, block: usize) -> CaptureBlock {
        std::array::from_fn(|offset| {
            let index = (block * CAPTURE_SAMPLES + offset) as u64;
            let seconds = index as f64 / f64::from(CAPTURE_RATE) * (1.0 + self.drift);
            let echo: f32 = self
                .path
                .iter()
                .map(|(delay, gain)| gain * self.lamp(seconds - delay))
                .sum();
            float_to_pcm(
                echo + (self.person)(seconds) * voice(2, seconds) + 0.0003 * noise(3, index),
            )
        })
    }
    /// Run `seconds` of the scene through a new canceller.
    fn run(&self, seconds: f64) -> Vec<EchoEvidence> {
        let mut processor = EchoProcessor::new(true);
        let mut rendered = 0;
        (0..(seconds * 1000.0) as usize / BLOCK_MS as usize)
            .map(|block| {
                let lead = if self.jitter {
                    1 + (noise(4, block as u64 / 7).abs() * 5.0) as usize
                } else {
                    2
                };
                while rendered < block + lead {
                    processor.render(&self.render(rendered)).unwrap();
                    rendered += 1;
                }
                processor.capture(&self.capture(block), 20).unwrap();
                processor.evidence().unwrap()
            })
            .collect()
    }
}

fn block_at(seconds: f64) -> usize {
    (seconds * 1000.0) as usize / BLOCK_MS as usize
}
fn median(mut values: Vec<i32>) -> i32 {
    assert!(!values.is_empty());
    values.sort_unstable();
    values[values.len() / 2]
}
/// Median of one matched measurement over a span of seconds.
fn matched(
    evidence: &[EchoEvidence],
    from: f64,
    until: f64,
    field: impl Fn(&EchoEvidence) -> Option<i32>,
) -> i32 {
    median(
        evidence[block_at(from)..block_at(until)]
            .iter()
            .filter_map(field)
            .collect(),
    )
}
fn capture_explained(evidence: &EchoEvidence) -> Option<i32> {
    evidence.echo.map(|echo| i32::from(echo.capture_explained))
}
fn residual_explained(evidence: &EchoEvidence) -> Option<i32> {
    evidence.echo.map(|echo| i32::from(echo.residual_explained))
}

#[test]
fn playback_alone_is_matched_and_explains_what_the_microphone_hears() {
    let evidence = Scene::playback(0.5, 6.0).run(7.0);
    // Nothing is claimed before Lamp speaks.
    assert!(
        evidence[..block_at(0.5)]
            .iter()
            .all(|block| block.echo.is_none() && block.render_active_blocks == 0)
    );
    // The echo reaches the microphone 113.5 ms after the speaker took it.
    let first = evidence
        .iter()
        .position(|block| block.echo.is_some())
        .expect("playback is matched");
    let found_after_ms = first as f64 * 10.0 - (0.5 + DELAY) * 1000.0;
    let explained = matched(&evidence, 1.0, 6.0, capture_explained);
    let unmatched = evidence[block_at(1.0)..block_at(6.0)]
        .iter()
        .filter(|block| {
            block
                .echo
                .is_none_or(|echo| echo.capture_explained < MATCH_PERCENT)
        })
        .count();
    // How long this canceller takes on this scene to remove 10 dB.
    let cancelling_after_ms = evidence[first..]
        .iter()
        .position(|block| {
            block.residual_level.tenths_of_db() <= block.capture_level.tenths_of_db() - 100
        })
        .map(|blocks| blocks * 10);
    println!(
        "MEASURED echo_evidence playback_only matched_ms_after_first_echo={found_after_ms:.0} capture_explained_median={explained} blocks_below_match={unmatched} of 500 ten_db_cancellation_ms_after_match={cancelling_after_ms:?} (synthetic echo path through the real canceller, host)"
    );
    assert!((0.0..=400.0).contains(&found_after_ms), "{found_after_ms}");
    assert!(explained >= 85, "{explained}");
    assert!(unmatched <= 10, "{unmatched}");
    // After the last echo has passed, nothing is claimed again.
    let quiet = &evidence[block_at(6.0 + DELAY + 0.2)..];
    assert!(quiet.iter().all(|block| block.echo.is_none()));
    assert_eq!(evidence.last().unwrap().render_active_blocks, 0);
    assert!(evidence[block_at(3.0)].render_active_blocks == ACTIVITY_BLOCKS);
}

/// What a canceller leaves, scripted: `leak` of the echo's amplitude and all
/// of the person. Run through the observer alone, so the share it reports for
/// the cancelled signal can be checked against a known mixture.
/// Medians of `residual_explained` and `residual_playback_floor`.
fn residual_share(leak: f32, person: f32) -> (i32, i32) {
    let scene = Scene {
        person: Box::new(move |t| if t >= 0.2 { person } else { 0.0 }),
        ..Scene::playback(0.2, 5.0)
    };
    let quiet = Scene::playback(0.2, 5.0);
    let mut observer = EchoObserver::new();
    let shares = (0..block_at(5.0))
        .filter_map(|block| {
            observer.render(&scene.render(block));
            let (raw, echo) = (scene.capture(block), quiet.capture(block));
            // The echo alone, scaled, plus whatever of the raw block is not echo.
            let clean: CaptureBlock = std::array::from_fn(|index| {
                let rest = f32::from(raw[index]) - f32::from(echo[index]);
                (leak * f32::from(echo[index]) + rest).round() as i16
            });
            observer.capture(&raw, &clean).echo.map(|echo| {
                (
                    i32::from(echo.residual_explained),
                    i32::from(echo.residual_playback_floor),
                )
            })
        })
        .collect::<Vec<_>>();
    let late = &shares[shares.len() / 2..];
    (
        median(late.iter().map(|(explained, _)| *explained).collect()),
        median(late.iter().map(|(_, floor)| *floor).collect()),
    )
}

#[test]
fn leaked_playback_in_the_cancelled_signal_is_measured_as_playback() {
    // The recorded false interruption: 0.85 s into a reply the cancelled
    // signal still carried enough of Lamp's voice to pass a speech detector.
    // A canceller that has only halved the echo, nobody speaking:
    let (leak_only, leak_only_floor) = residual_share(0.5, 0.0);
    // The same leak under a person as loud as the leak itself, and one who
    // is four times its energy:
    let level = 0.3 * ECHO_GAIN * 0.5;
    let ((equal, equal_floor), (louder, louder_floor)) =
        (residual_share(0.5, level), residual_share(0.5, level * 2.0));
    // No leak at all, only the person:
    let (person_only, person_only_floor) = residual_share(0.0, level);
    println!(
        "MEASURED echo_evidence residual leak_only={leak_only} leak_with_equal_person={equal} leak_with_person_plus_6db={louder} person_only={person_only} floors={leak_only_floor},{equal_floor},{louder_floor},{person_only_floor} (scripted residual through the observer alone, host)"
    );
    assert!(leak_only >= 85, "{leak_only}");
    assert!((equal - leak_only / 2).abs() <= 8, "{equal}");
    assert!((louder - leak_only / 5).abs() <= 8, "{louder}");
    // Sixteen copies of an unrelated voice account for a few percent of any
    // signal by chance.
    assert!(person_only <= 8, "{person_only}");
    // The floor never claims more playback than there is: all of it, half,
    // a fifth and none. It is short of the truth by what the fit misses.
    assert!((70..=100).contains(&leak_only_floor), "{leak_only_floor}");
    assert!((30..=50).contains(&equal_floor), "{equal_floor}");
    assert!((8..=20).contains(&louder_floor), "{louder_floor}");
    assert_eq!(person_only_floor, 0);
}

#[test]
fn a_person_alone_is_never_attributed_to_the_speaker() {
    let scene = Scene {
        speaker: Box::new(|_| 0.0),
        person: Box::new(|t| if t >= 0.5 { 0.2 } else { 0.0 }),
        ..Scene::playback(0.0, 0.0)
    };
    let evidence = scene.run(4.0);
    assert!(
        evidence
            .iter()
            .all(|block| block.echo.is_none() && block.render_active_blocks == 0)
    );
    let level = |from, until, field: fn(&EchoEvidence) -> Level| {
        median(
            evidence[block_at(from)..block_at(until)]
                .iter()
                .map(|block| i32::from(field(block).tenths_of_db()))
                .collect(),
        )
    };
    let (capture, residual) = (
        level(1.0, 4.0, |block| block.capture_level),
        level(1.0, 4.0, |block| block.residual_level),
    );
    println!(
        "MEASURED echo_evidence person_only capture_level_db={:.1} residual_level_db={:.1} (synthetic, host)",
        f64::from(capture) / 10.0,
        f64::from(residual) / 10.0
    );
    // The person is in both signals at much the same level: the canceller
    // has no render to subtract.
    assert!((capture - residual).abs() <= 60, "{capture} {residual}");
    assert!(capture > -400, "{capture}");
}

/// Playback throughout; a person joins at three seconds, `ratio` times the
/// echo's energy at the microphone. Returns the shares before and after.
fn overlap(ratio: f32) -> (i32, i32, i32) {
    // Both talkers have the same statistics, so amplitudes compare directly.
    let echo = 0.3 * ECHO_GAIN;
    let scene = Scene {
        person: Box::new(move |t| if t >= 3.0 { echo * ratio.sqrt() } else { 0.0 }),
        ..Scene::playback(0.2, 7.0)
    };
    let evidence = scene.run(7.0);
    assert!(
        evidence[block_at(1.0)..]
            .iter()
            .all(|block| block.echo.is_some()),
        "a person speaking does not unseat the alignment"
    );
    (
        matched(&evidence, 1.5, 3.0, capture_explained),
        matched(&evidence, 3.3, 7.0, capture_explained),
        matched(&evidence, 3.3, 7.0, residual_explained),
    )
}

#[test]
fn a_person_over_playback_lowers_the_explained_share_in_proportion() {
    let mut shares = Vec::new();
    for (ratio, label) in [(0.25, "-6"), (1.0, "0"), (4.0, "+6")] {
        let (alone, together, residual) = overlap(ratio);
        // With an unrelated talker of `ratio` times the echo's energy, the
        // render can explain at most 1 / (1 + ratio) of the total.
        let predicted = (alone as f32 / (1.0 + ratio)).round() as i32;
        println!(
            "MEASURED echo_evidence overlap person_to_echo_db={label} capture_explained_alone={alone} together={together} predicted={predicted} residual_explained_together={residual} (synthetic, host)"
        );
        assert!((together - predicted).abs() <= 6, "{together} {predicted}");
        shares.push((together, residual));
    }
    // Louder person, smaller share, before and after cancellation.
    assert!(shares[0].0 > shares[1].0 && shares[1].0 > shares[2].0);
    // Once the canceller has converged what remains is the person.
    assert!(shares[1].1 <= 25 && shares[2].1 <= 25, "{shares:?}");
}

#[test]
fn clock_drift_between_speaker_and_microphone_is_followed() {
    // 300 parts per million for twenty seconds moves the alignment by 6 ms,
    // twenty-four analysis samples.
    let scene = Scene {
        drift: 300e-6,
        ..Scene::playback(0.2, 20.0)
    };
    let evidence = scene.run(20.0);
    let late = &evidence[block_at(15.0)..block_at(19.5)];
    let explained = median(late.iter().filter_map(capture_explained).collect());
    let stale = late
        .iter()
        .filter(|block| block.echo.is_none_or(|echo| echo.alignment_age_blocks > 20))
        .count();
    println!(
        "MEASURED echo_evidence drift_300ppm capture_explained_median_15_to_19.5s={explained} blocks_unmatched_or_stale={stale} of 450 (synthetic, host)"
    );
    assert!(explained >= 88, "{explained}");
    assert!(stale <= 9, "{stale}");
}

#[test]
fn acceptance_order_jitter_does_not_change_what_is_measured() {
    let steady = Scene {
        jitter: false,
        ..Scene::playback(0.2, 5.0)
    };
    let jittered = Scene::playback(0.2, 5.0);
    let (steady, jittered) = (
        matched(&steady.run(5.0), 1.0, 5.0, capture_explained),
        matched(&jittered.run(5.0), 1.0, 5.0, capture_explained),
    );
    assert!((steady - jittered).abs() <= 2, "{steady} {jittered}");
}

#[test]
fn reset_forgets_the_alignment_and_counts_from_one() {
    let scene = Scene::playback(0.0, 10.0);
    let mut processor = EchoProcessor::new(true);
    assert_eq!(processor.evidence(), None);
    for block in 0..200 {
        processor.render(&scene.render(block)).unwrap();
        processor.capture(&scene.capture(block), 20).unwrap();
    }
    let before = processor.evidence().unwrap();
    assert_eq!(before.blocks, 200);
    assert!(before.echo.is_some());
    processor.reset();
    assert_eq!(processor.evidence(), None);
    processor.render(&scene.render(200)).unwrap();
    processor.capture(&scene.capture(200), 20).unwrap();
    let after = processor.evidence().unwrap();
    assert_eq!(after.blocks, 1);
    assert_eq!(after.echo, None);
    assert_eq!(after.render_active_blocks, 1);
}

#[test]
fn one_block_of_evidence_is_bounded_work() {
    let scene = Scene::playback(0.0, 30.0);
    let blocks: Vec<_> = (0..2_000)
        .map(|block| (scene.render(block), scene.capture(block)))
        .collect();
    let mut observer = EchoObserver::new();
    let started = std::time::Instant::now();
    for (render, capture) in &blocks {
        observer.render(render);
        std::hint::black_box(observer.capture(capture, capture));
    }
    let each = started.elapsed() / blocks.len() as u32;
    println!(
        "MEASURED echo_evidence cost per_block_us={:.1} blocks={} profile={} (observer alone, one thread, host)",
        each.as_secs_f64() * 1e6,
        blocks.len(),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    // A 10 ms block must cost a small part of 10 ms even unoptimized.
    assert!(each.as_micros() < 2_000, "{each:?}");
}

#[test]
fn a_voiced_reply_over_the_path_a_device_recorded_is_matched_and_explained() {
    // The first device recording was all playback, and the observer of that
    // day reported nothing: its search wanted one delayed copy to explain
    // half the capture, and on this path one copy explains less.
    let scene = Scene {
        source: pitched,
        path: MEASURED_PATH,
        ..Scene::playback(0.5, 6.0)
    };
    let evidence = scene.run(7.0);
    let first = evidence
        .iter()
        .position(|block| block.echo.is_some())
        .expect("playback is matched");
    let found_after_ms = first as f64 * 10.0 - (0.5 + 0.1035) * 1000.0;
    let shares: Vec<i32> = evidence[block_at(1.0)..block_at(6.0)]
        .iter()
        .map(|block| capture_explained(block).unwrap_or(0))
        .collect();
    let below = shares
        .iter()
        .filter(|share| **share < i32::from(MATCH_PERCENT))
        .count();
    let mut sorted = shares.clone();
    sorted.sort_unstable();
    println!(
        "MEASURED echo_evidence measured_path matched_ms_after_first_echo={found_after_ms:.0} capture_explained_p10={} median={} blocks_below_match={below} of 500 (pitched source over the device's measured path through the real canceller, host)",
        sorted[50], sorted[250]
    );
    assert!((0.0..=400.0).contains(&found_after_ms), "{found_after_ms}");
    assert!(sorted[250] >= 85, "{}", sorted[250]);
    assert!(sorted[50] >= 70, "{}", sorted[50]);
    assert!(below <= 10, "{below}");
}

/// #828: a pause in a reply. Once the window the echo would come from is
/// silent, the evidence says so (`silent_at_echo`, with a fresh alignment),
/// apart from having no alignment at all, as before the reply. While the
/// lamp is heard and matched it is absent.
#[test]
fn a_pause_in_a_reply_is_said_to_be_silent_at_the_echo() {
    let scene = Scene {
        speaker: Box::new(|t| {
            if (0.5..3.0).contains(&t) || (3.3..5.0).contains(&t) {
                0.3
            } else {
                0.0
            }
        }),
        ..Scene::playback(0.0, 0.0)
    };
    let evidence = scene.run(5.5);
    // Before the reply nothing is matched, and nothing is silent at it.
    assert!(
        evidence[..block_at(0.5)]
            .iter()
            .all(|block| block.echo.is_none() && block.silent_at_echo.is_none())
    );
    // In the pause, past the window its last words fill, at the microphone.
    let pause = &evidence[block_at(3.0 + DELAY + 0.13)..block_at(3.3 + DELAY)];
    assert!(!pause.is_empty());
    assert!(
        pause
            .iter()
            .all(|block| block.echo.is_none() && block.silent_at_echo.is_some_and(|age| age <= 40)),
        "{:?}",
        pause
            .iter()
            .map(|block| block.silent_at_echo)
            .collect::<Vec<_>>()
    );
    // While the lamp is heard and matched, it is absent.
    let speaking = &evidence[block_at(1.5)..block_at(2.5)];
    assert!(speaking.iter().all(|block| block.silent_at_echo.is_none()));
    let matched_blocks = speaking.iter().filter(|block| block.echo.is_some()).count();
    assert!(matched_blocks > 90, "{matched_blocks}");
}

/// What a capture block costs while the speaker is silent, with the search
/// and with it skipped: in a room loud enough to search (a fan, an office:
/// -26 dBFS) before the lamp has spoken, and, after it has spoken and an
/// alignment is held, in that room and in a quiet one, where no search runs
/// but the held alignment is still fitted every block: `cargo test --release
/// -p lamp-audio --test it evidence::measure_a_silent_render_block -- --ignored
/// --nocapture`.
#[test]
#[ignore]
fn measure_a_silent_render_block() {
    const BLOCKS: u64 = 3_000;
    const SPOKEN: u64 = 100;
    let silent = [0i16; RENDER_SAMPLES];
    // The lamp speaking, and the microphone hearing it 100 ms later.
    let speaking = |block: u64| -> (RenderBlock, CaptureBlock) {
        let render = std::array::from_fn(|index| {
            let sample = block * RENDER_SAMPLES as u64 + index as u64;
            float_to_pcm(0.5 * voice(1, sample as f64 / RENDER_RATE as f64))
        });
        let capture = std::array::from_fn(|index| {
            let sample = block * CAPTURE_SAMPLES as u64 + index as u64;
            float_to_pcm(-0.25 * voice(1, sample as f64 / CAPTURE_RATE as f64 - 0.1))
        });
        (render, capture)
    };
    let room = |level: f32, block: u64| -> CaptureBlock {
        std::array::from_fn(|index| {
            float_to_pcm(level * noise(7, block * CAPTURE_SAMPLES as u64 + index as u64))
        })
    };
    for (name, level, spoken) in [
        ("-26 dBFS, nothing held", 0.05, false),
        ("-26 dBFS, an alignment held", 0.05, true),
        ("quiet, an alignment held", 0.0, true),
    ] {
        for skip in [false, true] {
            let mut observer = EchoObserver::new();
            observer.skip_while_render_silent(skip);
            let mut processor = EchoProcessor::new(false);
            processor.skip_evidence_while_render_silent(skip);
            if spoken {
                for block in 0..SPOKEN {
                    let (render, capture) = speaking(block);
                    observer.render(&render);
                    observer.capture(&capture, &capture);
                    processor.render(&render).unwrap();
                    processor.capture(&capture, 0).unwrap();
                }
                assert!(observer.echo_delay().is_some(), "{name}: nothing held");
            }
            let started = std::time::Instant::now();
            for block in SPOKEN..SPOKEN + BLOCKS {
                observer.render(&silent);
                let raw = room(level, block);
                observer.capture(&raw, &raw);
            }
            let observing = started.elapsed() / BLOCKS as u32;
            let started = std::time::Instant::now();
            for block in SPOKEN..SPOKEN + BLOCKS {
                processor.render(&silent).unwrap();
                processor.capture(&room(level, block), 0).unwrap();
            }
            let whole = started.elapsed() / BLOCKS as u32;
            println!(
                "{name}, skipped {skip}: a 10 ms block takes the evidence {observing:?}, \
                 the canceller and the evidence {whole:?}"
            );
        }
    }
}
