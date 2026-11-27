use super::*;

struct Bench {
    trial: Trial,
    jobs: queue::Receiver<Pair>,
    results: queue::Sender<ResultRow>,
    rows: mpsc::Receiver<ResultRow>,
    _generation: watch::Receiver<u64>,
    warming: watch::Receiver<Option<u64>>,
}

impl Bench {
    fn new() -> Self {
        let (jobs, requests) = queue::channel(1);
        let (ready, results) = queue::channel(1);
        let (generation, resets) = watch::channel(0);
        let (prewarm, warming) = watch::channel(None);
        let (journal, rows) = mpsc::sync_channel(8);
        Self {
            trial: Trial::new(jobs, results, generation, prewarm, journal),
            jobs: requests,
            results: ready,
            rows,
            _generation: resets,
            warming,
        }
    }

    fn line(&mut self, request: u64, start: u64, words: &str, kind: LineKind) {
        self.trial.begin(request, true, start);
        self.trial
            .audio(request, &[request as i16; 320], true, start + 20_000);
        self.trial.pcm_ended(request, start, start + 21_000);
        self.trial.words(request, Some(words), None);
        self.trial.judged(request, kind, start, start + 30_000);
    }
}

#[test]
fn only_the_two_wake_words_can_supply_samples() {
    let pcm = [1, 2, 3, 4, 5, 6];
    assert_eq!(wake_audio(&pcm, "Hey, Lamp!", None), Some(0..6));
    assert_eq!(wake_audio(&pcm, "Hey Lamp, tell me something.", None), None);
    assert_eq!(
        wake_audio(&pcm, "Hey Lamp, tell me something.", Some(1..4)),
        Some(1..4)
    );
    assert_eq!(
        wake_audio(&pcm, "Hey Lamp, tell me something.", Some(4..7)),
        None
    );
    assert_eq!(wake_audio(&pcm, "Hey Lynn", Some(1..4)), None);
    assert_eq!(
        wake_audio(&[0; REFERENCE_SAMPLES + 1], "Hey Lamp.", None),
        None
    );
    assert_eq!(wake_audio(&[], "Hey Lamp", None), None);

    let data = r#"{"channel":{"alternatives":[{"words":[{"word":"hey","start":0.125,"end":0.25},{"word":"Lamp","start":0.25,"end":0.5},{"word":"please","start":0.5,"end":0.75}]}]}}"#;
    assert_eq!(wake_range(data), Some(2000..8000));
    assert_eq!(wake_range(&data.replace("0.5", "14.0")), None);
    assert_eq!(wake_range(&data.replace("0.125", "0.75")), None);
    assert_eq!(wake_range(&data.replace("Lamp", "man")), None);
    assert_eq!(wake_range("{}"), None);
}

#[test]
fn compound_wake_uses_only_aligned_samples_and_other_calls_replace_it() {
    let mut b = Bench::new();
    b.trial.begin(1, true, 1_000_000);
    b.trial.audio(1, &[1, 1, 2, 2, 9, 9], true, 1_010_000);
    b.trial.pcm_ended(1, 1_000_000, 1_011_000);
    b.trial
        .words(1, Some("Hey Lamp what time is it?"), Some(0..4));
    b.trial
        .judged(1, LineKind::SpokenCall, 1_000_000, 1_020_000);
    assert_eq!(
        *b.warming.borrow_and_update(),
        Some(*b.trial.generation.borrow())
    );
    assert!(b.jobs.try_recv().is_err(), "enrollment sends no samples");
    b.trial.answered(1, 2_000_000, 1_050_000);
    b.line(2, 3_000_000, "And times two?", LineKind::FollowUp);
    let job = b.jobs.try_recv().unwrap();
    assert_eq!(
        (job.observation.caller, job.observation.request),
        (Some(1), 2)
    );
    assert_eq!(&*job.reference, &[1, 1, 2, 2]);
    assert_eq!(*job.line, vec![2; 320]);
    let result = ResultRow {
        observation: job.observation,
        outcome: "different",
        score: Some(0.27),
        ready_us: Some(4_000_000),
        verdict_us: None,
    };
    b.line(3, 3_500_000, "Hey Lamp", LineKind::OtherCall); // a tap or a computer
    assert!(!b.warming.has_changed().unwrap());
    let provisional = b.jobs.try_recv().unwrap();
    assert_ne!(
        provisional.observation.generation,
        *b.trial.generation.borrow()
    );
    b.results.try_send(result).unwrap();
    b.trial.tick(4_000_000);
    assert!(
        b.rows.try_recv().is_err(),
        "a result for the previous caller is retired"
    );
    b.line(4, 5_000_000, "And times two?", LineKind::FollowUp);
    assert!(b.jobs.try_recv().is_err());
    b.line(5, 6_000_000, "Hey Lamp play music", LineKind::SpokenCall);
    assert!(
        b.trial.reference.is_none(),
        "a compound call without alignment clears the old reference"
    );
    b.line(6, 6_100_000, "Hey Lamp", LineKind::Other);
    assert!(
        b.trial.reference.is_none(),
        "a transcript alone cannot enroll"
    );
}

#[test]
fn mixed_stale_oversize_and_revoked_audio_never_reaches_the_model() {
    let mut b = Bench::new();
    b.line(1, 100_000, "Hey Lamp", LineKind::SpokenCall);
    b.trial.answered(1, 150_000, 150_000);
    for (request, clean, fresh, size) in [
        (2, false, true, 100),
        (3, true, false, 100),
        (4, true, true, LINE_SAMPLES + 1),
    ] {
        b.trial.begin(request, clean, 200_000);
        b.trial.audio(request, &vec![1; size], fresh, 210_000);
        b.trial.pcm_ended(request, 200_000, 215_000);
        b.trial.words(request, Some("Yes"), None);
        b.trial
            .judged(request, LineKind::FollowUp, 200_000, 250_000);
        assert!(b.jobs.try_recv().is_err(), "request {request}");
        assert_eq!(b.rows.try_recv().unwrap().outcome, "unknown");
    }
    b.trial.begin(5, true, 300_000);
    b.trial.audio(5, &[1; 320], true, 320_000);
    forgotten();
    b.trial.pcm_ended(5, 300_000, 310_000);
    b.trial.words(5, Some("Hey Lamp"), None);
    b.trial.judged(5, LineKind::SpokenCall, 300_000, 330_000);
    assert!(b.trial.reference.is_none());
    b.line(6, 500_000, "Hey Lamp", LineKind::SpokenCall);
    b.trial.clear(); // mute or stop
    b.line(7, 700_000, "Yes", LineKind::FollowUp);
    assert!(b.jobs.try_recv().is_err());
}

#[test]
fn missing_or_impossible_mic_timing_cannot_enroll_a_wake() {
    for (began, end, queued) in [
        (0, 220_000, 230_000),
        (250_000, 220_000, 260_000),
        (200_000, 260_000, 230_000),
    ] {
        let mut b = Bench::new();
        b.trial.begin(1, true, 200_000);
        b.trial.audio(1, &[1; 320], true, end);
        b.trial.pcm_ended(1, began, queued);
        b.trial.words(1, Some("Hey Lamp"), None);
        b.trial.judged(1, LineKind::SpokenCall, began, 270_000);
        assert_eq!(*b.warming.borrow(), None);
        b.trial.answered(1, 300_000, 300_000);
        b.line(2, 500_000, "And times two?", LineKind::FollowUp);
        assert!(b.trial.reference.is_none());
        assert!(b.jobs.try_recv().is_err());
        let row = b.rows.try_recv().unwrap();
        assert_eq!((row.outcome, row.ready_us), ("unknown", None));
    }
}

#[test]
fn window_uses_line_start_and_only_real_playback_can_extend_the_reference() {
    let mut b = Bench::new();
    b.line(1, 1_000_000, "Hey Lamp", LineKind::SpokenCall);
    b.trial.answered(1, 2_000_000, 1_100_000);
    b.trial.begin(2, true, 21_500_000);
    b.trial.audio(2, &[2; 320], true, 21_520_000);
    b.trial.tick(22_500_000);
    b.trial.pcm_ended(2, 21_500_000, 22_520_000);
    b.trial.words(2, Some("And how many?"), None);
    b.trial
        .judged(2, LineKind::FollowUp, 21_500_000, 22_600_000);
    let job = b.jobs.try_recv().unwrap();
    assert_eq!(job.observation.caller, Some(1));
    let result = ResultRow {
        observation: job.observation,
        outcome: "same",
        score: Some(0.96),
        ready_us: Some(22_610_000),
        verdict_us: None,
    };
    b.results.try_send(result).unwrap();
    b.trial.tick(22_620_000);
    assert_eq!(b.rows.try_recv().unwrap().outcome, "same");
    assert_eq!(
        b.trial.reference.as_ref().unwrap().until_us,
        22_000_000,
        "a match does not renew"
    );
    b.trial.answered(99, 90_000_000, 22_700_000);
    assert_eq!(b.trial.reference.as_ref().unwrap().until_us, 22_000_000);
    b.trial.answered(2, 23_000_000, 22_810_000);
    assert_eq!(b.trial.reference.as_ref().unwrap().until_us, 43_000_000);
    b.trial.tick(43_000_001);
    assert!(b.trial.reference.is_none());
    b.trial.answered(2, 45_000_000, 44_000_000);
    assert!(
        b.trial.reference.is_none(),
        "an expired voice cannot be revived"
    );
}

#[test]
fn configuration_requires_the_explicit_switch_and_local_model_directory() {
    for value in [None, Some("off"), Some("true"), Some("ON")] {
        assert!(!enabled(value, Path::new("/")));
    }
    assert!(!enabled(Some("on"), Path::new("relative")));
    assert!(!enabled(
        Some("on"),
        Path::new("/no-such-caller-frame-model-8849")
    ));
    let dir = std::env::temp_dir().join(format!(
        "caller-frame-model-check-{}-{}",
        std::process::id(),
        lamp_ipc::monotonic_us()
    ));
    std::fs::create_dir(&dir).unwrap();
    assert!(!enabled(Some("on"), &dir));
    let asset = dir.join("caller-frame.f32");
    std::fs::File::create_new(&asset).unwrap();
    assert!(enabled(Some("on"), &dir));
    std::fs::remove_file(asset).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

/// A wake can load weights without PCM. Reset one stalled handshake, queue a
/// pair during the next load, and reset that stalled pair. Only the current
/// caller's bytes may produce a score, including on a cold fallback start.
#[cfg(unix)]
#[tokio::test]
async fn prewarm_and_inflight_pair_are_reaped_and_the_next_caller_has_its_own_bytes() {
    use rustix::process::{Pid, test_kill_process};
    use std::os::unix::fs::PermissionsExt;
    use tokio::time::timeout;

    async fn marker(path: &Path) -> String {
        timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(value) = std::fs::read_to_string(path)
                    && !value.trim().is_empty()
                {
                    return value;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }

    let dir = std::env::temp_dir().join(format!(
        "caller-frame-pipe-{}-{}",
        std::process::id(),
        lamp_ipc::monotonic_us()
    ));
    std::fs::create_dir(&dir).unwrap();
    let first_gate = dir.join("first-gate");
    let second_gate = dir.join("second-gate");
    let pair_gate = dir.join("pair-gate");
    assert!(
        std::process::Command::new("mkfifo")
            .args([&first_gate, &second_gate, &pair_gate])
            .status()
            .unwrap()
            .success()
    );
    let program = dir.join("child");
    std::fs::write(
        &program,
        r#"#!/bin/sh
if [ "$1" != frame-serve ]; then exit 1; fi
if [ ! -f "$2/first-pid" ]; then
    printf '%s\n' "$$" > "$2/first-pid"
    read -r go < "$2/first-gate" || exit 3
elif [ ! -f "$2/second-pid" ]; then
    printf '%s\n' "$$" > "$2/second-pid"
    read -r go < "$2/second-gate" || exit 4
fi
printf 'ready\n'
while read -r tag caller request refs samples; do
    if [ "$tag" != pair ]; then exit 2; fi
    if [ "$request" = 2 ]; then
        printf '%s\n' "$$" > "$2/started"
        # Keep the reply pipe open until cancellation. GNU dd with of=/dev/null
        # replaces stdout and would tell the parent EOF while still running.
        IFS= read -r ignored < "$2/pair-gate"
        exit 5
    fi
    got=$(dd bs=1 count=$(((refs + samples) * 2)) 2>/dev/null | od -An -tx1 | tr -d ' \n')
    if [ "$caller" = 7 ] && [ "$request" = 8 ] && [ "$got" = 0100feff0300 ]; then
        printf 'same 0.91\n'
    else
        printf 'unknown\n'
    fi
done
"#,
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (jobs, rx) = queue::channel(1);
    let (tx, mut results) = queue::channel(1);
    let (generation, invalidated) = watch::channel(0);
    let (prewarm, warming) = watch::channel(None);
    let task = tokio::spawn(child::run(
        program.clone(),
        dir.clone(),
        rx,
        tx,
        invalidated,
        warming,
    ));
    let send = |caller, request, generation| Pair {
        observation: Observation {
            generation,
            caller: Some(caller),
            request,
            capture_end_us: Some(10),
            queued_us: Some(11),
        },
        reference: Arc::from([1, -2]),
        line: Arc::new(vec![3]),
    };
    prewarm.send_replace(Some(0));
    let pid = |text: String| Pid::from_raw(text.trim().parse().unwrap()).unwrap();
    let first = pid(marker(&dir.join("first-pid")).await);
    assert!(results.try_recv().is_err());
    generation.send_replace(1);
    prewarm.send_replace(Some(1));
    let second = pid(marker(&dir.join("second-pid")).await);
    assert!(test_kill_process(first).is_err(), "old warm process reaped");
    jobs.try_send(send(1, 2, 1)).unwrap();
    let started = dir.join("started");
    assert!(!started.exists(), "a pair must wait for its model's ready");
    assert!(results.try_recv().is_err());
    let gate = second_gate.clone();
    timeout(
        Duration::from_secs(3),
        tokio::task::spawn_blocking(move || std::fs::write(gate, b"go\n")),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    assert_eq!(
        pid(marker(&started).await),
        second,
        "reuse the prewarmed child"
    );
    assert!(results.try_recv().is_err(), "the stalled pair has no reply");
    generation.send_replace(2);
    jobs.send(send(7, 8, 2)).await.unwrap();
    let row = timeout(Duration::from_secs(3), results.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (row.observation.caller, row.observation.request, row.outcome),
        (Some(7), 8, "same")
    );
    assert_eq!(row.score, Some(0.91));
    assert!(test_kill_process(second).is_err(), "stalled pair reaped");
    drop(jobs);
    timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    std::fs::remove_file(started).unwrap();
    for name in [
        "first-pid",
        "second-pid",
        "first-gate",
        "second-gate",
        "pair-gate",
    ] {
        std::fs::remove_file(dir.join(name)).unwrap();
    }
    std::fs::remove_file(program).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
#[ignore = "host timing; run alone with --ignored --nocapture"]
fn measure_caller_frame_capture_and_submission() {
    use std::time::Instant;
    let mut b = Bench::new();
    b.line(1, 10, "Hey Lamp", LineKind::SpokenCall);
    b.trial.answered(1, 100, 100);
    let mut capture = Vec::new();
    let mut submit = Vec::new();
    for n in 0..=2_000 {
        let request = 2 + n;
        b.trial.begin(request, true, 1000);
        let start = Instant::now();
        b.trial
            .audio(request, std::hint::black_box(&[1; 160]), true, 11_000);
        capture.push(start.elapsed().as_nanos());
        let start = Instant::now();
        b.trial.pcm_ended(request, 1000, 12_000);
        submit.push(start.elapsed().as_nanos());
        b.trial.words(request, Some("And again"), None);
        b.trial.judged(request, LineKind::FollowUp, 1000, 12_900);
        let job = b.jobs.try_recv().unwrap();
        let row = ResultRow {
            observation: job.observation,
            outcome: "different",
            score: Some(0.1),
            ready_us: Some(13_000),
            verdict_us: None,
        };
        b.results.try_send(row).unwrap();
        b.trial.tick(13_000);
        b.rows.try_recv().unwrap();
    }
    // Opt in to preserving every measured repetition without touching either
    // timed section. Repeated runs never overwrite a previous recording.
    if let Some(path) = std::env::var_os("LAMP_CALLER_FRAME_TIMING_OUT") {
        use std::io::{BufWriter, Write};
        let mut file = BufWriter::new(std::fs::File::create_new(path).unwrap());
        writeln!(
            file,
            "trial\tappend_160_pcm_samples_ns\tsubmit_before_asr_finishes_ns"
        )
        .unwrap();
        for (trial, (pcm, queued)) in capture.iter().zip(&submit).enumerate() {
            writeln!(file, "{trial}\t{pcm}\t{queued}").unwrap();
        }
        file.flush().unwrap();
    }
    for (name, mut durations) in [
        ("append_160_pcm_samples", capture),
        ("submit_before_asr_finishes", submit),
    ] {
        let first = durations.remove(0);
        durations.sort_unstable();
        eprintln!(
            "{name}: first_ns={first} warm_n={} p95_ns={} max_ns={}",
            durations.len(),
            durations[1899],
            durations.last().unwrap()
        );
    }
}

#[test]
fn pcm_end_precedes_asr_and_only_an_ordinary_verdict_releases_the_original_readiness() {
    for ready_before_asr in [true, false] {
        for kind in [
            LineKind::FollowUp,
            LineKind::SpokenCall,
            LineKind::PlaybackOnly,
            LineKind::Other,
        ] {
            let mut b = Bench::new();
            b.line(1, 1_000_000, "Hey Lamp", LineKind::SpokenCall);
            b.trial.answered(1, 2_000_000, 2_000_000);
            b.trial.begin(2, true, 3_000_000);
            b.trial.audio(2, &[50; 320], true, 3_020_000);
            b.trial.pcm_ended(2, 3_000_000, 3_020_500);
            let pair = b
                .jobs
                .try_recv()
                .expect("request submitted before ASR is back");
            let verdict_us = 3_110_000;
            let ready_us = if ready_before_asr {
                3_075_000
            } else {
                3_150_000
            };
            let row = || ResultRow {
                observation: pair.observation,
                outcome: "different",
                score: Some(0.25),
                ready_us: Some(ready_us),
                verdict_us: None,
            };
            let mut stale = row();
            stale.observation.generation = stale.observation.generation.wrapping_sub(1);
            b.results.try_send(stale).unwrap();
            b.trial.tick(3_067_000);
            assert!(b.trial.pending.as_ref().unwrap().ready.is_none());
            if ready_before_asr {
                b.results.try_send(row()).unwrap();
                b.trial.tick(ready_us);
                assert!(
                    b.rows.try_recv().is_err(),
                    "unconfirmed classification escaped before ASR"
                );
            }
            b.trial.words(2, Some("And times two?"), None);
            b.trial.judged(2, kind, 3_000_000, verdict_us);
            if !ready_before_asr {
                assert!(b.rows.try_recv().is_err());
                b.results.try_send(row()).unwrap();
                b.trial.tick(ready_us);
            }
            if matches!(kind, LineKind::FollowUp) {
                let row = b.rows.try_recv().unwrap();
                assert_eq!(
                    (row.observation.queued_us, row.ready_us, row.verdict_us),
                    (Some(3_020_500), Some(ready_us), Some(verdict_us))
                );
                assert_eq!(b.trial.reference.as_ref().unwrap().until_us, 22_000_000);
            } else {
                assert!(
                    b.rows.try_recv().is_err(),
                    "new call, confirmation or held line published a provisional result"
                );
            }
        }
    }
}

#[test]
fn a_split_call_and_a_confirmation_keep_only_the_base_playback_window() {
    let mut b = Bench::new();
    b.line(1, 1_000_000, "Hey Lamp", LineKind::SpokenCall);
    b.line(2, 2_000_000, "How many?", LineKind::PlaybackOnly);
    assert!(
        b.jobs.try_recv().is_err(),
        "the original call is still being made"
    );
    b.trial.answered(2, 3_000_000, 3_000_000);
    b.line(3, 4_000_000, "Yes", LineKind::PlaybackOnly);
    let provisional = b.jobs.try_recv().unwrap();
    assert_ne!(
        provisional.observation.generation,
        *b.trial.generation.borrow()
    );
    assert!(b.rows.try_recv().is_err());
    b.trial.answered(3, 6_000_000, 6_000_000);
    b.line(4, 25_000_000, "And again?", LineKind::FollowUp);
    let pair = b.jobs.try_recv().unwrap();
    assert_eq!(
        (pair.observation.caller, pair.observation.request),
        (Some(1), 4)
    );
}
