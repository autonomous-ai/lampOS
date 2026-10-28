//! A light the voice asks for, end to end on a real bus, as the person
//! waits for it: `lamp-actions`' executive asks (`light/command`), the
//! ring's light (`RingLight`, ticked every 2 ms as the ring's loop does)
//! takes it and says what it shows (`light/state`), and the executive
//! answers once it reads that. The model speaks only after that answer, so
//! the person hears this wait on every light call (#1806).
use lamp_actions::{
    Executive, Outcome,
    light::BusLight,
    sound::{Mixer, Volume},
};
use lamp_voice::ring_light::RingLight;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// No volume control: only the light is asked for here.
struct NoMixer;
impl Mixer for NoMixer {
    fn get(&mut self) -> std::io::Result<u8> {
        Ok(0)
    }
    fn set(&mut self, _: u8) -> std::io::Result<()> {
        Ok(())
    }
}

/// The ring's loop's pass (`ring_worker`'s `TICK_US`).
const PASS: Duration = Duration::from_millis(2);

#[test]
fn twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them() {
    // No other test's disk flush beside these timed calls (`disk_flush`).
    let _timing = crate::disk_flush::timing();
    // A short root: a Unix socket's address holds 103 bytes.
    let root = PathBuf::from(format!("/tmp/lvl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let ring = {
        let (root, stop) = (root.clone(), stop.clone());
        thread::spawn(move || {
            let (mut ring, _) = RingLight::on(&root).unwrap();
            while !stop.load(Ordering::Acquire) {
                ring.tick(lamp_ipc::monotonic_us());
                thread::sleep(PASS);
            }
        })
    };
    let bus = lamp_bus::Bus::new(&root).unwrap();
    // The ring is there once it has said what it shows.
    let began = Instant::now();
    while bus.latest(lamp_actions::light::STATE).unwrap().is_none() {
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "the ring never said"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let mut executive = Executive {
        sound: None::<Volume<NoMixer>>,
        light: Some(BusLight::new(lamp_bus::Bus::new(&root).unwrap()).unwrap()),
        short: false,
    };
    let mut took = Vec::new();
    for call in 0..20 {
        let colour = if call % 2 == 0 { "red" } else { "blue" };
        let started = Instant::now();
        let outcome = executive.call("light_colour", &json!({ "colour": colour }));
        took.push(started.elapsed());
        assert!(
            matches!(outcome, Outcome::Done(_)),
            "call {call}, {colour}: {outcome:?}"
        );
    }
    stop.store(true, Ordering::Release);
    ring.join().unwrap();
    std::fs::remove_dir_all(&root).unwrap();
    took.sort();
    eprintln!("light calls, fastest to slowest: {took:?}");
    // A few of the ring's passes, not a 50 ms look and a 15 ms read: the
    // middle call took about 55 ms before #1806 on the Intel Mac, and 3 ms
    // after.
    let middle = took[took.len() / 2];
    assert!(middle < Duration::from_millis(25), "{took:?}");
}
