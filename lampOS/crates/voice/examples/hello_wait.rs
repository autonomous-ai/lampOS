//! How long a due hello waits for the policy on this machine (#752): the
//! voice's `Arrival` says it on `voice/arrival`, the policy's own loop
//! decides with a stand-in judge that answers at once, and the voice takes
//! the answer, called every 2 ms as its loop calls it. No model, no camera,
//! no sound: a real judge adds its own time (Jev, 0.5 to 1 s; at most
//! 1.5 s, after which the policy's rule answers).
//!
//! `cargo run --release -p lamp-voice --example hello_wait -- --samples 30`
use lamp_bus::Bus;
use lamp_msgs::VoiceArrival;
use lamp_policy::{
    decide::{Answer, Asked, Decider, Failed, Question},
    hello::{self, Hello as PolicyHello},
    policy::{self, Options},
    progress::LocalTime,
};
use lamp_voice::speak_up::{Arrival, Hello};
use serde_json::json;
use std::{
    fs, io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const S: u64 = 1_000_000;

/// Says yes to every hello, at once.
struct Yes;
impl Decider for Yes {
    fn ask(
        &mut self,
        _: &str,
        questions: &[(String, Question)],
        _: Duration,
    ) -> Result<Asked, Failed> {
        Ok(Asked {
            answers: questions
                .iter()
                .map(|(name, _)| {
                    let yes = if name == "intrusive" { 0.0 } else { 1.0 };
                    (name.clone(), Answer::Noul(yes))
                })
                .collect(),
            model: "at-once".into(),
            took: Duration::ZERO,
            usage: None,
            connection: None,
        })
    }
    fn name(&self) -> &str {
        "at-once"
    }
}

fn summary(mut values: Vec<f64>) -> serde_json::Value {
    let first = values[0];
    values.sort_by(f64::total_cmp);
    json!({"first":first,"median":values[values.len() / 2],"max":values[values.len() - 1],"samples":values.len()})
}

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let samples = match args.as_slice() {
        [] => 30,
        [flag, n] if flag == "--samples" => n
            .parse::<usize>()
            .ok()
            .filter(|n| (2..=1_000).contains(n))
            .ok_or_else(|| io::Error::other("--samples is 2..1000"))?,
        _ => return Err(io::Error::other("usage: hello_wait [--samples N]")),
    };
    let root = PathBuf::from(format!("/tmp/lvhw-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;
    let bus = Bus::new(root.join("b"))?.rescan_every(Duration::from_millis(20));
    let stop = Arc::new(AtomicBool::new(false));
    let running = {
        let (bus, stop) = (bus.clone(), stop.clone());
        let options = Options {
            log_dir: root.join("log"),
            deadline: Duration::from_secs(1),
            pause: Duration::from_millis(300),
            act_attention: false,
            report_every: policy::REPORT_EVERY,
        };
        // Acting, and with no twenty minutes between hellos: each sample
        // is asked, not held by the rule.
        let hello = hello::Options {
            act: true,
            at_most_every: Duration::ZERO,
            ..hello::Options::new(
                Duration::from_millis(1_500),
                Some(Arc::new(LocalTime::Offset(420))),
            )
        };
        thread::spawn(move || {
            let hello = PolicyHello::start(Yes, hello)?;
            policy::run_with_questions(&bus, Yes, &options, &stop, &mut |_| {}, None, Some(hello))
        })
    };
    let started = Instant::now();
    while !bus
        .subscribers(VoiceArrival::TOPIC)?
        .iter()
        .any(|listener| listener.socket.starts_with("policy.") && listener.listening)
    {
        if started.elapsed() > Duration::from_secs(5) {
            return Err(io::Error::other("the policy did not subscribe"));
        }
        thread::sleep(Duration::from_millis(5));
    }
    let mut waits = Vec::new();
    let mut rule = Vec::new();
    for sample in 0..samples as u64 {
        // A new person each time: the voice's own limit never holds.
        let start = 1_000 * S + sample * 3_600 * S;
        for asking in [true, false] {
            let mut arrival = Arrival::watching(None, true, start);
            if asking {
                arrival = arrival.asking(&bus)?;
            }
            let mut now = start + 360 * S;
            arrival.seen(true, now);
            now += 3 * S;
            arrival.seen(true, now);
            let began = Instant::now();
            loop {
                let now_us = now + u64::try_from(began.elapsed().as_micros()).unwrap_or(0);
                match arrival.hello(true, 60 * S, now_us) {
                    Hello::NotYet => thread::sleep(Duration::from_millis(2)),
                    Hello::SayNow | Hello::ByRule => break,
                    Hello::Wait => return Err(io::Error::other("the policy said wait")),
                }
                if began.elapsed() > Duration::from_secs(5) {
                    return Err(io::Error::other("no answer"));
                }
            }
            let ms = (began.elapsed().as_secs_f64() * 1e4).round() / 10.0;
            if asking { &mut waits } else { &mut rule }.push(ms);
        }
        thread::sleep(Duration::from_millis(30));
    }
    stop.store(true, Ordering::Relaxed);
    running
        .join()
        .map_err(|_| io::Error::other("the policy panicked"))??;
    let _ = fs::remove_dir_all(&root);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "platform":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},
            "hello_waits_for_the_policy_ms":summary(waits),
            "hello_by_the_rule_alone_ms":summary(rule),
            "boundaries":"from the first Arrival::hello call of a due, ready hello (which says it on voice/arrival) to the call that returns the answer, called every 2 ms; through a bus in /tmp, the policy's loop and its asking thread with a judge that answers at once, and the decision's log line; without asking the rule's answer is the first call's",
        }))?
    );
    Ok(())
}
