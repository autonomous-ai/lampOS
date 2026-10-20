//! Explicit host arithmetic measurement; never an acoustic or lamp result.
use lamp_policy::{
    attention,
    decide::Decider,
    policy::local::{Cues, Local, RoomCue, Weights},
};
use serde_json::json;
use std::{
    hint::black_box,
    path::Path,
    time::{Duration, Instant},
};

#[test]
#[ignore = "host measurement: run alone with --release --ignored --nocapture"]
fn measure_local_first_and_repeated() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models/policy/local-weights.json");
    let began = Instant::now();
    let weights = Weights::load(&path).unwrap();
    let weights_load_ns = began.elapsed().as_nanos();
    let mut local = Local::new(weights);
    let questions = attention::questions();
    let state = serde_json::to_string(&Cues {
        name_score: Some(0.98),
        conversation: Some(false),
        room: Some(RoomCue::Busy),
        facing: Some(true),
        face_age_ms: Some(70),
        word_count: Some(6),
        text_called: Some(true),
        ..Cues::default()
    })
    .unwrap();
    let mut samples = Vec::with_capacity(1001);
    for _ in 0..1001 {
        let started = Instant::now();
        black_box(
            local
                .ask(black_box(&state), &questions, Duration::from_secs(1))
                .unwrap(),
        );
        samples.push(started.elapsed().as_nanos());
    }
    let first_ns = samples.remove(0);
    samples.sort_unstable();
    println!(
        "LOCAL_TIMING {}",
        json!({
        "schema":1,"model":local.weights().name(),"profile":if cfg!(debug_assertions) { "debug" } else { "release" }, "os":std::env::consts::OS,
            "arch":std::env::consts::ARCH,"samples_repeated":samples.len(),"weights_load_ns":weights_load_ns,
            "first_ns":first_ns,"median_ns":samples[499],"p95_ns":samples[949],"p99_ns":samples[989],
            "worst_ns":samples[999], "boundary":"Decider::ask including JSON parsing and seven typed answer allocation; excludes weights loading, cue gathering, final combining, logging and bus scheduling",
            "cache":"OS file cache not cleared; fresh test process, no previous Local::ask",
        })
    );
    assert!(
        samples[949] <= 20_000_000 && samples[989] <= 50_000_000,
        "declared host arithmetic latency target"
    );
}
