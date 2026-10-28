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
        own: false,
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

/// #2562: a knock is AudioSet's Knock (353), not Door (348). The door score
/// is the highest of Door, Sliding door (351), Slam (352) and Knock, and
/// Tap (354) and Doorbell (349) raise nothing.
#[test]
fn a_knock_a_slam_or_a_sliding_door_is_the_door_and_a_tap_is_not() {
    let quiet = vec![-10.0_f32; CLASSES];
    assert!(door(&quiet) < 0.001);
    for index in [348, 351, 352, 353] {
        let mut logits = quiet.clone();
        logits[index] = 2.0;
        assert_eq!(door(&logits), sigmoid(2.0), "index {index}");
    }
    for index in [349, 350, 354, FOOTSTEPS] {
        let mut logits = quiet.clone();
        logits[index] = 2.0;
        assert!(door(&logits) < 0.001, "index {index}");
    }
    let mut knock = quiet.clone();
    knock[353] = 1.0;
    knock[348] = -1.0;
    assert_eq!(door(&knock), sigmoid(1.0), "the highest of the group");
}

#[test]
fn capture_handoff_is_bounded_and_never_waits_for_a_slow_classifier() {
    let (handoff, intake) = handoff();
    let gate = gate(1);
    let samples = [1; BLOCK];
    let mut times = Vec::new();
    for sequence in 0..1000 {
        let started = Instant::now();
        assert!(handoff.offer(&samples, &gate, stamp(1, sequence, 1), false, 2));
        times.push(started.elapsed().as_nanos());
        assert!(intake.receiver.try_recv().is_ok());
    }
    // A stalled worker: the queue fills, then offers drop and mark overload.
    let mut stalled = Vec::new();
    for sequence in 0..(QUEUE as u64 + 100) {
        let started = Instant::now();
        let kept = handoff.offer(&samples, &gate, stamp(1, sequence, 1), false, 2);
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
    assert!(!handoff.offer(&[1; BLOCK], &gate, stamp(2, 0, 1), false, 2));
    assert!(!handoff.offer(&[1; BLOCK], &gate, stamp(1, 0, 3), false, 2));
    gate.fence();
    assert!(!handoff.offer(&[1; BLOCK], &gate, stamp(1, 0, 1), false, 2));
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
                        door: 0.0,
                        footsteps: 0.0,
                    })
                }),
                |classified| sent.send(classified).unwrap(),
                || true,
            )
        })
    };
    let signal = tone(74);
    for index in 0..74 {
        let at = lamp_ipc::monotonic_us();
        let samples: &[i16; BLOCK] = signal[index * BLOCK..(index + 1) * BLOCK]
            .try_into()
            .unwrap();
        assert!(handoff.offer(samples, &gate, stamp(1, index as u64, at), false, at));
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

/// A window the lamp spoke into says its own sound is there, and the next
/// window, after the lamp stopped, says unknown, not clear: a stale voice
/// state reads as not speaking (#1757's read: Laugh glance's `own_voice`
/// guard had nothing to act on).
#[test]
fn a_window_the_lamp_spoke_into_is_its_own_and_the_next_is_unknown() {
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
                        door: 0.0,
                        footsteps: 0.0,
                    })
                }),
                |classified| sent.send(classified).unwrap(),
                || true,
            )
        })
    };
    // The first patch is blocks 0 to 48 and the second begins at block 24:
    // the lamp speaks in blocks 0 to 20 only.
    let signal = tone(74);
    for index in 0..74 {
        let at = lamp_ipc::monotonic_us();
        let samples: &[i16; BLOCK] = signal[index * BLOCK..(index + 1) * BLOCK]
            .try_into()
            .unwrap();
        assert!(handoff.offer(samples, &gate, stamp(1, index as u64, at), index <= 20, at));
        thread::sleep(Duration::from_millis(1));
    }
    let mut own = Vec::new();
    while own.len() < 2 {
        let classified = received.recv_timeout(Duration::from_secs(5)).unwrap();
        if let SoundClasses::Scored { own_voice, .. } = classified.classes {
            own.push(own_voice);
        }
    }
    assert_eq!(own, [OwnVoiceEvidence::Present, OwnVoiceEvidence::Unknown]);
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
                || true,
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

/// The classifier starts only with its switch exactly `on`: on a lamp with
/// a slide (`LAMP_PRIVACY_CONTROLS=on`) as built, and on a lamp with no
/// slide only with the person's stored yes (the lead's decision, 07:33),
/// which is asked for nowhere else. Any other route never runs it, and that
/// is said; with the switch off there is nothing to say.
#[test]
fn the_classifier_starts_with_a_slide_or_on_no_slide_with_a_yes() {
    let count = std::cell::Cell::new(0);
    let asked = &count;
    let yes = |answer: bool| {
        move || {
            asked.set(asked.get() + 1);
            answer
        }
    };
    assert_eq!(start(Some("on"), Some("on"), yes(false)), Start::Start);
    assert_eq!(start(Some("on"), Some("no-slide"), yes(true)), Start::Start);
    assert_eq!(
        start(Some("on"), Some("no-slide"), yes(false)),
        Start::NoYes
    );
    assert_eq!(asked.get(), 2, "the yes is read only on no-slide");
    for route in [None, Some(""), Some("off"), Some("ON"), Some("NO-SLIDE")] {
        assert_eq!(
            start(Some("on"), route, yes(true)),
            Start::NotOnThisRoute,
            "{route:?}"
        );
    }
    for classes in [None, Some(""), Some("off"), Some("ON")] {
        for route in [Some("on"), Some("no-slide"), None] {
            assert_eq!(
                start(classes, route, yes(true)),
                Start::Off,
                "{classes:?} {route:?}"
            );
        }
    }
    assert_eq!(asked.get(), 2, "nor with the switch off");
}

/// Seventh's read rule (#1722, 07:41): only a JSON object whose
/// `room_sounds` is exactly `"yes"` is a yes; anything else is no.
#[test]
fn only_a_stored_room_sounds_yes_is_a_yes() {
    let folder = std::env::temp_dir().join(format!("room-sounds-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("room-sounds.json");
    assert!(!room_sounds_yes(&file), "missing");
    let read = |text: &str| {
        std::fs::write(&file, text).unwrap();
        room_sounds_yes(&file)
    };
    assert!(read(
        r#"{"version":1,"room_sounds":"yes","at_unix_s":1792500000}"#
    ));
    assert!(
        read(r#"{"version":2,"room_sounds":"yes","new":true}"#),
        "one version ahead"
    );
    let padded = format!(r#"{{"room_sounds":"yes","pad":"{}"}}"#, "x".repeat(4096));
    for text in [
        r#"{"room_sounds":"no"}"#,
        r#"{"room_sounds":"Yes"}"#,
        r#"{"room_sounds":true}"#,
        r#"{"room_sounds":"yes""#,
        r#"["yes"]"#,
        r#""yes""#,
        "yes",
        "",
        &padded,
    ] {
        assert!(!read(text), "{text:.40}");
    }
    std::fs::remove_dir_all(&folder).unwrap();
    std::fs::create_dir_all(&file).unwrap();
    assert!(!room_sounds_yes(&file), "a folder, unreadable as a file");
    std::fs::remove_dir_all(&folder).unwrap();
}

/// Third's P2 on #2109: a yes taken back while the classifier runs (a forget
/// or a redo of setup removes the file) stops it within `YES_EVERY_US`, with
/// no restart: it says `privacy` once and its thread ends.
#[test]
fn a_yes_taken_back_stops_the_classifier_within_its_bound() {
    let (_handoff, intake) = handoff();
    let gate = Arc::new(PublicationGate::default());
    let stop = Arc::new(AtomicBool::new(false));
    let yes = Arc::new(AtomicBool::new(true));
    let (sent, received) = mpsc::channel();
    let started = Instant::now();
    let worker = {
        let (gate, stop, yes) = (gate.clone(), stop.clone(), yes.clone());
        thread::spawn(move || {
            serve(
                intake,
                &gate,
                &stop,
                lamp_ipc::monotonic_us,
                Some(|_: &[f32]| Err::<SoundScores, _>("never scored".to_owned())),
                |classified| sent.send(classified).unwrap(),
                move || yes.load(Ordering::Acquire),
            )
        })
    };
    thread::sleep(Duration::from_millis(300));
    yes.store(false, Ordering::Release);
    let taken_back = Instant::now();
    worker.join().unwrap();
    assert!(
        taken_back.elapsed() <= Duration::from_micros(YES_EVERY_US) + Duration::from_millis(500),
        "ended {:?} after the yes went",
        taken_back.elapsed()
    );
    assert!(
        started.elapsed() >= Duration::from_micros(YES_EVERY_US),
        "asked only every bound"
    );
    assert!(!stop.load(Ordering::Acquire), "no stop: the yes ended it");
    let last = received
        .try_iter()
        .last()
        .map(|classified| classified.classes);
    assert!(
        matches!(
            last,
            Some(SoundClasses::Unavailable {
                reason: SoundUnavailable::Privacy
            })
        ),
        "{last:?}"
    );
}
