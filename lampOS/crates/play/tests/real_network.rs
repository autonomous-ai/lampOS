//! A play over the real network, by hand: `PLAY_TEST_URL` and
//! `PLAY_TEST_HOSTS` (comma separated) name what to fetch, so no URL is in
//! the tree. Run with `cargo test -p lamp-play --test real_network --
//! --ignored --nocapture`. It prints the time to the first block of sound
//! (lookup, connect, TLS, the first bytes and the first frames) and the
//! outcome, and plays at most `PLAY_TEST_SECONDS` (default 20) of it.
use lamp_play::decode::{Flow, Limits, decode_report};
use lamp_play::fetch::{SyncBody, SystemNet, get};
use lamp_play::hosts::Allowed;
use std::time::Instant;

#[test]
#[ignore = "needs the network and PLAY_TEST_URL"]
fn a_real_https_mp3_plays_and_its_first_sound_is_timed() {
    let url = std::env::var("PLAY_TEST_URL").expect("PLAY_TEST_URL");
    let hosts: Vec<String> = std::env::var("PLAY_TEST_HOSTS")
        .expect("PLAY_TEST_HOSTS")
        .split(',')
        .map(str::to_owned)
        .collect();
    let seconds: u64 = std::env::var("PLAY_TEST_SECONDS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(20);
    let allowed = Allowed::new(&hosts).unwrap();
    let begun = Instant::now();
    let mut net = SystemNet::default();
    let body = get(&allowed, &url, Limits::default().most_bytes, &mut net);
    let body = match body {
        Ok(body) => body,
        Err(error) => panic!("fetch: {}", error.word()),
    };
    println!("headers after {} ms", begun.elapsed().as_millis());
    let mut first = None;
    let mut samples = 0u64;
    let report = decode_report(SyncBody::new(body), Limits::default(), |block| {
        first.get_or_insert_with(|| begun.elapsed());
        samples += block.len() as u64;
        if samples >= seconds * 24_000 {
            Flow::Stop
        } else {
            Flow::Continue
        }
    });
    println!(
        "first block after {:?} ms; outcome {}; {} s of sound; {} bytes read; total {} ms",
        first.map(|time| time.as_millis()),
        report.outcome.word(),
        report.samples / 24_000,
        report.bytes,
        begun.elapsed().as_millis()
    );
}
