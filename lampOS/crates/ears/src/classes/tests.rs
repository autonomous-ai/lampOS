use super::*;
use std::{sync::mpsc::TryRecvError, thread, time::Instant};

const FAR: u64 = u64::MAX / 2;

fn gate(epoch: u64) -> PublicationGate {
    let gate = PublicationGate::default();
    gate.update(
        Some(Permit {
            epoch,
            until_us: FAR,
        }),
        0,
    );
    gate
}

fn stamp(epoch: u64, sequence: u64, captured_at_us: u64) -> CaptureStamp {
    CaptureStamp {
        permit: Permit {
            epoch,
            until_us: FAR,
        },
        fence: 0,
        sequence,
        captured_at_us,
    }
}

/// A 440 Hz tone with a slow swell: a WAV-like 16 kHz mono PCM16 signal.
fn tone(blocks: usize) -> Vec<i16> {
    (0..blocks * BLOCK)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let swell = 0.2 + 0.1 * (2.0 * std::f32::consts::PI * 0.5 * t).sin();
            (swell * (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 32767.0) as i16
        })
        .collect()
}

fn block(signal: &[i16], index: usize, stamp: CaptureStamp) -> Block {
    Block {
        samples: signal[index * BLOCK..(index + 1) * BLOCK]
            .try_into()
            .unwrap(),
        stamp,
    }
}

#[test]
fn the_switch_is_exact_on_only() {
    for value in [None, Some(""), Some("off"), Some("ON"), Some("yes")] {
        assert!(!enabled(value));
    }
    assert!(enabled(Some("on")));
}

#[test]
fn bounds_include_future_fft_samples_and_never_pad_an_incomplete_tail() {
    assert_eq!(
        bounds(0),
        Bounds {
            first_frame: 0,
            start_sample: 0,
            end_sample: 15_456
        }
    );
    assert_eq!(
        bounds(50),
        Bounds {
            first_frame: 50,
            start_sample: 7_744,
            end_sample: 23_456
        }
    );
    let frontend = Frontend::new();
    assert!(frontend.features(&[0.0; 15_455], 0, bounds(0)).is_err());
    // A later patch never reflects at a buffer's trimmed start.
    assert!(frontend.features(&[0.0; 16_000], 7_744, bounds(0)).is_err());
}

#[test]
fn silence_is_log_offset_and_tone_is_finite_and_not_silence() {
    let frontend = Frontend::new();
    let silence = frontend
        .features(&vec![0.0; RATE as usize], 0, bounds(0))
        .unwrap();
    assert_eq!(silence.len(), FRAMES * MELS);
    assert!(silence.iter().all(|x| (*x - 0.001_f32.ln()).abs() < 1e-6));
    let tone: Vec<_> = (0..RATE as usize)
        .map(|i| 0.25 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / RATE as f32).sin())
        .collect();
    let features = frontend.features(&tone, 0, bounds(0)).unwrap();
    assert!(features.iter().all(|x| x.is_finite()));
    // 440 Hz falls in the low mel bands, not at either edge.
    let middle = &features[40 * MELS..41 * MELS];
    let peak = middle
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap()
        .0;
    assert!((5..=15).contains(&peak), "peak band {peak}");
}

#[test]
fn stable_sigmoid_is_multilabel_and_bounded() {
    assert_eq!(sigmoid(0.0), 0.5);
    assert_eq!(sigmoid(1000.0), 1.0);
    assert_eq!(sigmoid(-1000.0), 0.0);
    assert!((sigmoid(2.0) + sigmoid(-2.0) - 1.0).abs() < 1e-6);
}

#[test]
fn capture_handoff_is_bounded_and_never_waits_for_a_slow_classifier() {
    let (handoff, intake) = handoff();
    let gate = gate(1);
    let samples = [1; BLOCK];
    let mut times = Vec::new();
    for sequence in 0..1000 {
        let started = Instant::now();
        assert!(handoff.offer(&samples, &gate, stamp(1, sequence, 1), 2));
        times.push(started.elapsed().as_nanos());
        assert!(intake.receiver.try_recv().is_ok());
    }
    // A stalled worker: the queue fills, then offers drop and mark overload.
    let mut stalled = Vec::new();
    for sequence in 0..(QUEUE as u64 + 100) {
        let started = Instant::now();
        let kept = handoff.offer(&samples, &gate, stamp(1, sequence, 1), 2);
        stalled.push(started.elapsed().as_nanos());
        assert_eq!(kept, sequence < QUEUE as u64);
    }
    assert!(intake.overflowed.load(Ordering::Acquire));
    times.sort_unstable();
    stalled.sort_unstable();
    eprintln!(
        "ears classes host handoff per 20 ms block: p50={}ns p95={}ns max={}ns (1000 blocks); with the worker stalled: p50={}ns max={}ns; excludes ALSA and target hardware",
        times[499],
        times[949],
        times[999],
        stalled[stalled.len() / 2],
        stalled[stalled.len() - 1]
    );
    // Far under the 1 ms target; a median keeps a busy CI host from flaking.
    assert!(times[499] < 1_000_000 && stalled[stalled.len() / 2] < 1_000_000);
}

#[test]
fn unauthorized_or_future_blocks_are_never_copied() {
    let (handoff, intake) = handoff();
    let gate = gate(1);
    assert!(!handoff.offer(&[1; BLOCK], &gate, stamp(2, 0, 1), 2));
    assert!(!handoff.offer(&[1; BLOCK], &gate, stamp(1, 0, 3), 2));
    gate.fence();
    assert!(!handoff.offer(&[1; BLOCK], &gate, stamp(1, 0, 1), 2));
    assert!(matches!(
        intake.receiver.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn a_streamed_window_scores_every_500_ms_with_the_offline_features_and_stays_bounded() {
    let signal = tone(200);
    let all: Vec<f32> = signal.iter().map(|&x| f32::from(x) / 32768.0).collect();
    let frontend = Frontend::new();
    let gate = gate(1);
    let mut window = Window::default();
    let mut patches = Vec::new();
    for index in 0..200 {
        let at = 1_000_000 + 20_000 * (index as u64 + 1);
        assert_eq!(
            window.push(
                &block(&signal, index, stamp(1, index as u64, at)),
                &gate,
                at
            ),
            None
        );
        assert!(window.held() <= RETAIN);
        if let Some(patch) = window.ready() {
            let streamed = window.features(&frontend, &patch).unwrap();
            assert_eq!(streamed, frontend.features(&all, 0, patch.bounds).unwrap());
            patches.push((index, patch));
            window.advance();
        }
    }
    // 15456 samples is 48.3 blocks: the first patch with block 49 (index 48),
    // then one every 25 blocks (500 ms).
    let at: Vec<usize> = patches.iter().map(|(index, _)| *index).collect();
    assert_eq!(at, vec![48, 73, 98, 123, 148, 173, 198]);
    let (_, first) = patches[0];
    assert_eq!(
        (first.start_us, first.end_us, first.capture_epoch),
        (1_000_000, 1_000_000 + 20_000 * 49, 1)
    );
    let (_, second) = patches[1];
    // Sample 7744 is in block 24, which began at 1.48 s; 23455 in block 73.
    assert_eq!(
        (second.start_us, second.end_us),
        (1_000_000 + 20_000 * 24, 1_000_000 + 20_000 * 74)
    );
    assert!(second.end_us - second.start_us < SOUND_RAW_AGE_US);
}

#[test]
fn a_gap_mute_epoch_or_age_purges_and_a_later_block_starts_a_new_era() {
    let signal = tone(60);
    let push_run = |window: &mut Window, gate: &PublicationGate, epoch, from: u64, count| {
        for sequence in from..from + count {
            let at = 1_000_000 + 20_000 * sequence;
            let block = block(
                &signal,
                (sequence % 60) as usize,
                stamp(epoch, sequence, at),
            );
            window.push(&block, gate, at);
        }
    };
    let gate1 = gate(1);
    let mut window = Window::default();
    push_run(&mut window, &gate1, 1, 0, 40);
    // A dropped block: the old samples go and cannot complete a patch.
    let at = 1_000_000 + 20_000 * 41;
    assert_eq!(
        window.push(&block(&signal, 41, stamp(1, 41, at)), &gate1, at),
        Some(SoundUnavailable::DeviceGap)
    );
    assert!(window.warming());
    assert_eq!(window.held(), BLOCK);
    push_run(&mut window, &gate1, 1, 42, 47);
    assert!(window.ready().is_none());
    push_run(&mut window, &gate1, 1, 89, 1);
    assert!(window.ready().is_some());

    // Mute: the gate closes; maintain purges without waiting for a block.
    let mut window = Window::default();
    push_run(&mut window, &gate1, 1, 0, 40);
    gate1.update(None, 0);
    assert_eq!(
        window.maintain(&gate1, 1_900_000),
        Some(SoundUnavailable::Privacy)
    );
    assert_eq!(window.held(), 0);

    // A new epoch (renewed after a gap) cannot extend the old window.
    let gate2 = gate(2);
    let mut window = Window::default();
    push_run(&mut window, &gate2, 1, 0, 40);
    assert_eq!(window.held(), 0);
    push_run(&mut window, &gate2, 2, 40, 40);
    assert_eq!(window.held(), 40 * BLOCK);
    assert!(window.ready().is_none());

    // A fence revokes the era even with the same epoch.
    let fence = gate2.fence();
    gate2.update(
        Some(Permit {
            epoch: 2,
            until_us: FAR,
        }),
        fence,
    );
    assert_eq!(
        window.maintain(&gate2, 1_900_000),
        Some(SoundUnavailable::Privacy)
    );

    // A stalled worker: samples older than two seconds are purged.
    let gate3 = gate(3);
    let mut window = Window::default();
    push_run(&mut window, &gate3, 3, 0, 10);
    // The first block's samples began at 0.98 s.
    assert_eq!(window.maintain(&gate3, 980_000 + 2_000_000), None);
    assert_eq!(
        window.maintain(&gate3, 980_000 + 2_000_001),
        Some(SoundUnavailable::Overloaded)
    );
    assert_eq!(window.held(), 0);
}

#[test]
fn the_worker_publishes_scores_then_unavailable_and_the_pump_rechecks_authority() {
    let (handoff, intake) = handoff();
    let gate = Arc::new(gate(1));
    let stop = Arc::new(AtomicBool::new(false));
    let (sent, received) = mpsc::channel();
    let worker = {
        let (gate, stop) = (gate.clone(), stop.clone());
        thread::spawn(move || {
            serve(
                intake,
                &gate,
                &stop,
                lamp_ipc::monotonic_us,
                Some(|_: &[f32]| {
                    Ok(SoundScores {
                        speech: 0.1,
                        music: 0.9,
                        laughter: 0.0,
                    })
                }),
                |classified| sent.send(classified).unwrap(),
            )
        })
    };
    let signal = tone(74);
    for index in 0..74 {
        let at = lamp_ipc::monotonic_us();
        let samples: &[i16; BLOCK] = signal[index * BLOCK..(index + 1) * BLOCK]
            .try_into()
            .unwrap();
        assert!(handoff.offer(samples, &gate, stamp(1, index as u64, at), at));
        thread::sleep(Duration::from_millis(1));
    }
    let mut scored = Vec::new();
    while scored.len() < 2 {
        let classified = received.recv_timeout(Duration::from_secs(5)).unwrap();
        if let SoundClasses::Scored { window, .. } = &classified.classes {
            assert_eq!(window.capture_epoch, 1);
            assert!(classified.publishable(&gate, lamp_ipc::monotonic_us()));
            scored.push(classified);
        }
    }
    // Mute: a held result may no longer leave, and the worker says privacy.
    gate.update(None, 0);
    assert!(!scored[1].publishable(&gate, lamp_ipc::monotonic_us()));
    let reason = loop {
        if let SoundClasses::Unavailable { reason } = received
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .classes
        {
            break reason;
        }
    };
    assert_eq!(reason, SoundUnavailable::Privacy);
    stop.store(true, Ordering::Release);
    worker.join().unwrap();
}

#[test]
fn a_model_that_fails_its_pins_is_refused_before_parsing() {
    let dir = std::env::temp_dir().join(format!("ears-classes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let graph = dir.join("yamnet.onnx");
    std::fs::write(&graph, vec![0u8; GRAPH_BYTES as usize]).unwrap();
    let error = Model::load(&graph).err().unwrap();
    assert!(error.contains("pinned SHA256"), "{error}");
    std::fs::write(&graph, b"short").unwrap();
    assert!(Model::load(&graph).is_err());
    assert!(model_from(Some("relative/yamnet.onnx")).is_err());
    assert!(model_from(None).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

/// Runs only with the pinned files present:
/// `LAMP_EARS_CLASSES_TEST_MODEL=/abs/yamnet.onnx cargo test -p lamp-ears -- --nocapture`.
#[test]
fn the_pinned_model_scores_a_patch_in_time() {
    let Some(path) = std::env::var_os("LAMP_EARS_CLASSES_TEST_MODEL") else {
        eprintln!("skipped: LAMP_EARS_CLASSES_TEST_MODEL is unset");
        return;
    };
    if !Path::new(&path).exists() {
        eprintln!("skipped: {} is absent", Path::new(&path).display());
        return;
    }
    let started = Instant::now();
    let model = Model::load(Path::new(&path)).unwrap();
    let load = started.elapsed();
    let frontend = Frontend::new();
    let signal: Vec<f32> = tone(200).iter().map(|&x| f32::from(x) / 32768.0).collect();
    let mut times = Vec::new();
    for first_frame in (0..).step_by(PATCH_HOP).take(20) {
        let bounds = bounds(first_frame % 300);
        let started = Instant::now();
        let features = frontend.features(&signal, 0, bounds).unwrap();
        let scores = model.score(&features).unwrap();
        times.push(started.elapsed().as_micros());
        for score in [scores.speech, scores.music, scores.laughter] {
            assert!(score.is_finite() && (0.0..=1.0).contains(&score));
        }
    }
    let silence = model.score(&vec![0.001_f32.ln(); FRAMES * MELS]).unwrap();
    times.sort_unstable();
    eprintln!(
        "ears classes model load {:?}; per patch (frontend + inference) p50={}us max={}us over 20; silence scores {silence:?}; host only",
        load, times[10], times[19]
    );
    // A patch every 500 ms; scoring must keep well inside it.
    assert!(times[10] < 250_000);
}

/// Third on #1923 (P3): a lamp muted from the start has no lease and sends
/// no blocks; after 100 ms of none the worker says `privacy`, not
/// `warming_up` or `device_gap`.
#[test]
fn a_lamp_muted_from_the_start_says_privacy() {
    let (_handoff, intake) = handoff();
    let gate = Arc::new(PublicationGate::default());
    let stop = Arc::new(AtomicBool::new(false));
    let (sent, received) = mpsc::channel();
    let worker = {
        let (gate, stop) = (gate.clone(), stop.clone());
        thread::spawn(move || {
            serve(
                intake,
                &gate,
                &stop,
                lamp_ipc::monotonic_us,
                Some(|_: &[f32]| Err::<SoundScores, _>("never scored".to_owned())),
                |classified| sent.send(classified).unwrap(),
            )
        })
    };
    // Its first word, at start, is still `warming_up`; within a second it
    // is `privacy`.
    let deadline = Instant::now() + Duration::from_secs(1);
    let said = std::iter::from_fn(|| {
        received
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .ok()
    })
    .find_map(|classified| match classified.classes {
        SoundClasses::Unavailable { reason } if reason != SoundUnavailable::WarmingUp => {
            Some(reason)
        }
        _ => None,
    });
    assert_eq!(said, Some(SoundUnavailable::Privacy));
    stop.store(true, Ordering::Release);
    worker.join().unwrap();
}
