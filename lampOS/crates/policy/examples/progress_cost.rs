//! What the progress question costs the policy itself on this machine
//! (`docs/policy.md`, "Progress news"), with a stand-in that answers at
//! once in place of a decision model: the state and the arithmetic, and the
//! loop from a status on the bus to its decision on the bus. No model is
//! asked; a real one adds its own 0.5 to 1 s.
//!
//! `cargo run --release -p lamp-policy --example progress_cost -- --samples 30`
use lamp_bus::Bus;
use lamp_msgs::{AgentState, Decision, Sign, TaskProgress, TaskStatus, Voice, VoiceState};
use lamp_policy::{
    attention::Room,
    decide::{Answer, Asked, Decider, Failed, Question},
    policy::{self, Options},
    progress::{self, Context},
};
use serde_json::{Value, json};
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

const STORE: &str = "0123456789abcdef0123456789abcdef";

/// Answers every question at once: worth little, so the answer is "wait"
/// and no note holds the next change back.
struct AtOnce;
impl Decider for AtOnce {
    fn ask(
        &mut self,
        _: &str,
        questions: &[(String, Question)],
        _: Duration,
    ) -> Result<Asked, Failed> {
        Ok(Asked {
            answers: questions
                .iter()
                .map(|(name, _)| (name.clone(), Answer::Noul(0.1)))
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

fn summary(mut values: Vec<f64>) -> Value {
    let first = values[0];
    values.sort_by(f64::total_cmp);
    json!({"first":first,"median":values[values.len() / 2],"max":values[values.len() - 1],"samples":values.len()})
}

fn context() -> Context {
    Context {
        store_id: STORE.into(),
        task: 3,
        change: 4,
        run: 1,
        engine: "claude".into(),
        state: AgentState::Working,
        working_s: Some(754),
        idle_s: Some(12),
        changed_now: true,
        new_share: 0.5,
        new_signs: vec![Sign::TestsPass],
        told_signs: vec![],
        notes_told: 0,
        since_note_s: None,
        someone_here: Some(true),
        quiet_s: Some(340),
        room: Room::Quiet,
        spoke_up_last_hour: 0,
        hour: Some(14),
    }
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
        _ => return Err(io::Error::other("usage: progress_cost [--samples N]")),
    };
    // The state and the arithmetic, alone.
    let answers = progress::questions()
        .into_iter()
        .map(|(name, _)| (name, Answer::Noul(0.7)))
        .collect();
    let mut pure = Vec::new();
    for _ in 0..1_000 {
        let began = Instant::now();
        std::hint::black_box(progress::state_text(&context()));
        std::hint::black_box(progress::combine(&answers).map_err(io::Error::other)?);
        pure.push((began.elapsed().as_secs_f64() * 1e7).round() / 10.0);
    }
    // The loop: a status published, its decision received.
    let root = PathBuf::from(format!("/tmp/lpc-{}", std::process::id()));
    let log_dir = std::env::temp_dir().join(format!("lamp-policy-cost-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;
    let bus = Bus::new(&root)?.rescan_every(Duration::from_millis(20));
    let mut status =
        bus.publisher_declared(TaskStatus::TOPIC, &TaskStatus::declaration(), "tasks")?;
    let mut voice =
        bus.publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")?;
    let mut decisions = bus.subscribe_typed(Decision::TOPIC, "cost", Decision::TYPE)?;
    let stop = Arc::new(AtomicBool::new(false));
    let running = {
        let (bus, stop) = (bus.clone(), Arc::clone(&stop));
        let options = Options {
            log_dir: log_dir.clone(),
            deadline: Duration::from_secs(1),
            pause: Duration::from_millis(300),
            act_attention: false,
            report_every: policy::REPORT_EVERY,
        };
        let progress = progress::Options::new(
            Duration::from_secs(1),
            Some(std::sync::Arc::new(progress::LocalTime::Offset(420))),
        );
        thread::spawn(move || {
            policy::run_with_progress(&bus, AtOnce, &options, &stop, &mut |_| {}, AtOnce, progress)
        })
    };
    let started = Instant::now();
    while !bus
        .subscribers(TaskStatus::TOPIC)?
        .iter()
        .any(|listener| listener.socket.starts_with("policy.") && listener.listening)
    {
        if started.elapsed() > Duration::from_secs(5) {
            return Err(io::Error::other("the policy did not subscribe"));
        }
        thread::sleep(Duration::from_millis(5));
    }
    voice.publish_json(
        0,
        &VoiceState {
            state: Voice::Resting,
        },
    )?;
    let work = |changes: u32| TaskStatus {
        store_id: STORE.into(),
        run: 1,
        unread: vec![],
        settled: Vec::new(),
        tasks: vec![TaskProgress {
            task: 3,
            engine: "claude".into(),
            state: AgentState::Working,
            working_s: Some(600),
            last_active_unix_ms: None,
            read_unix_ms: 0,
            changes,
            changed: changes > 0,
            new_share: 0.5,
            signs: vec![],
            signs_seen: vec![],
        }],
    };
    status.publish_json(0, &work(0))?;
    thread::sleep(Duration::from_millis(200));
    let mut loop_ms = Vec::new();
    for change in 1..=samples as u32 {
        let began = Instant::now();
        status.publish_json(0, &work(change))?;
        loop {
            let message = decisions
                .recv(Duration::from_secs(5))?
                .ok_or_else(|| io::Error::other("no decision"))?;
            let decision: Decision = message.parse()?;
            if decision.task.as_deref() == Some(&*format!("{STORE}:3:{change}@1")) {
                break;
            }
        }
        loop_ms.push((began.elapsed().as_secs_f64() * 1e4).round() / 10.0);
        thread::sleep(Duration::from_millis(50));
    }
    stop.store(true, Ordering::Relaxed);
    running
        .join()
        .map_err(|_| io::Error::other("the policy panicked"))??;
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&log_dir);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "platform":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},
            "state_and_combine_us":summary(pure),
            "status_to_decision_ms":summary(loop_ms),
            "boundaries":"state_and_combine: progress::state_text and progress::combine; status_to_decision: a TaskStatus published on a bus in /tmp to its Decision received there, through the policy's loop (20 ms tick), its asking thread and a stand-in that answers at once, and the decision's line written to a log in the temporary directory",
        }))?
    );
    Ok(())
}
