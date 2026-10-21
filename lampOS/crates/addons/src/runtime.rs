//! `lamp-addons serve` (#2530 part 1): reads `addon/proposal` and what the
//! lamp is doing, judges each proposal by [`crate::rules`], and asks the ring
//! for a kept light ([`crate::carry`]). Every proposal's verdict goes to the
//! journal, one line each, with why. A kept mood is said to `lamp-mood` as a
//! nudge on `mood/nudge`, which it holds inside its own bounds.
use crate::carry;
use crate::rules::{Judge, Verdict};
use crate::situation::{TOPICS, Watcher};
use lamp_bus::{Bus, Message, Publisher, Subscriber};
use lamp_msgs::{
    AddonDecision, AddonProposal, ExpressionRestState, Light, LightRequest, MicrophonePrivacy,
    MoodNudge, OnboardingPending, Pause, RoomState, VoiceMute, VoiceState,
};
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// The product's switch, as `lamp-skills`' (`lamp_skills::ADDONS_ENVIRONMENT`).
pub const ENVIRONMENT: &str = "LAMP_ADDONS";

/// How long one pass waits for a proposal before reading the lamp again.
const PASS: Duration = Duration::from_millis(100);

/// How it runs.
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub bus_root: Option<PathBuf>,
    /// The claims' clock: the host's boot, or a test's.
    pub clock_id: Option<String>,
    pub run_for: Option<Duration>,
}

/// What it did, said when it stops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub kept: u64,
    pub dropped: u64,
    pub lights_asked: u64,
    pub moods_nudged: u64,
}

/// The boot's identity, which the ring holds a claim's clock to.
fn boot_clock(given: Option<&str>) -> io::Result<String> {
    match given {
        Some(id) => Ok(id.to_owned()),
        None => std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .map(|id| id.trim().to_owned())
            .map_err(|error| {
                io::Error::other(format!("the boot's clock: {error}; give --clock-id"))
            }),
    }
}

pub fn serve(config: Config, stop: Arc<AtomicBool>) -> io::Result<Report> {
    let bus = match &config.bus_root {
        Some(root) => Bus::new(root)?,
        None => Bus::from_environment()?,
    };
    let clock_id = boot_clock(config.clock_id.as_deref())?;
    let mut proposals = bus.subscribe(AddonProposal::TOPIC, "addons")?;
    let mut lamp: Vec<Subscriber> = TOPICS
        .iter()
        .map(|topic| bus.subscribe(topic, "addons"))
        .collect::<io::Result<_>>()?;
    let mut lights =
        bus.publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "addons")?;
    let mut nudges =
        bus.publisher_declared(MoodNudge::TOPIC, &MoodNudge::declaration(), "addons")?;
    let mut decisions = bus.publisher_declared(
        AddonDecision::TOPIC,
        &AddonDecision::declaration(),
        "addons",
    )?;
    let mut watcher = Watcher::new(lamp_ipc::monotonic_us());
    // What was there before it started: the kept values of each topic.
    for topic in TOPICS {
        if let Ok(Some(message)) = bus.latest(topic) {
            take(&mut watcher, &message, lamp_ipc::monotonic_us());
        }
    }
    let mut judge = Judge::default();
    let mut report = Report::default();
    eprintln!("lamp-addons: judging addon/proposal; a kept light is asked of the ring at Idle");
    let began = Instant::now();
    while !stop.load(Ordering::Relaxed)
        && config
            .run_for
            .is_none_or(|run_for| began.elapsed() < run_for)
    {
        for subscriber in &mut lamp {
            while let Some(message) = subscriber.recv(Duration::ZERO)? {
                take(&mut watcher, &message, lamp_ipc::monotonic_us());
            }
        }
        let Some(message) = proposals.recv(PASS)? else {
            continue;
        };
        let now_us = lamp_ipc::monotonic_us();
        let proposal: AddonProposal = match message.parse() {
            Ok(proposal) => proposal,
            Err(error) => {
                eprintln!("lamp-addons: a proposal not read: {error}");
                continue;
            }
        };
        // Only lamp-skills writes proposals, stamping who made them.
        if message.header.source != "skills" || proposal.check().is_err() {
            eprintln!(
                "lamp-addons: dropped a proposal from {:?}: not lamp-skills', or not well formed",
                message.header.source
            );
            report.dropped += 1;
            continue;
        }
        let age_us = now_us.saturating_sub(message.header.at_us);
        let verdict = judge.judge(&proposal, age_us, &watcher.situation(now_us), now_us);
        let why = decide(
            &proposal,
            verdict,
            &clock_id,
            now_us,
            Outputs {
                lights: &mut lights,
                nudges: &mut nudges,
            },
            &mut report,
        );
        let decision = AddonDecision {
            proposal: proposal.proposal,
            addon: proposal.addon.clone(),
            act: proposal.act.clone(),
            choice: if verdict == Verdict::Kept {
                "kept"
            } else {
                "dropped"
            }
            .into(),
            by: "rule".into(),
            why: why.chars().take(200).collect(),
        };
        if let Err(error) = decisions.publish_json(now_us, &decision) {
            eprintln!(
                "lamp-addons: the decision on #{} not written: {error}",
                proposal.proposal
            );
        }
    }
    Ok(report)
}

/// Where a kept act goes: the ring's requests, and Mood's nudges.
struct Outputs<'a> {
    lights: &'a mut Publisher,
    nudges: &'a mut Publisher,
}

/// One verdict journaled, and a kept light asked of the ring or a kept mood
/// said to Mood: why, in words.
fn decide(
    proposal: &AddonProposal,
    verdict: Verdict,
    clock_id: &str,
    now_us: u64,
    out: Outputs,
    report: &mut Report,
) -> String {
    let named = format!("{} {} #{}", proposal.addon, proposal.act, proposal.proposal);
    match verdict {
        Verdict::Dropped(drop) => {
            report.dropped += 1;
            eprintln!("lamp-addons: dropped {named}: {}", drop.why());
            drop.why().into()
        }
        Verdict::Kept => {
            report.kept += 1;
            match carry::light_request(proposal, clock_id, now_us) {
                Some(request) => match out.lights.publish_json(now_us, &request) {
                    Ok(_) => {
                        report.lights_asked += 1;
                        let why = format!(
                            "asked of the ring at idle for {} ms",
                            (request.claim.until_us - now_us) / 1_000
                        );
                        eprintln!("lamp-addons: kept {named}: {why}");
                        why
                    }
                    Err(error) => {
                        eprintln!("lamp-addons: kept {named}, not asked: {error}");
                        "kept, but the ring was not asked".into()
                    }
                },
                None => match carry::mood_nudge(proposal, clock_id, now_us, report.kept) {
                    Some(nudge) => match out.nudges.publish_json(now_us, &nudge) {
                        Ok(_) => {
                            report.moods_nudged += 1;
                            let why = format!(
                                "said to lamp-mood for {} s",
                                (nudge.until_us - now_us) / 1_000_000
                            );
                            eprintln!("lamp-addons: kept {named}: {why}");
                            why
                        }
                        Err(error) => {
                            eprintln!("lamp-addons: kept {named}, not said: {error}");
                            "kept, but lamp-mood was not told".into()
                        }
                    },
                    None => {
                        eprintln!("lamp-addons: kept {named}: nothing to ask");
                        "kept; nothing to ask".into()
                    }
                },
            }
        }
    }
}

/// One message from the lamp, folded into what the judge knows.
fn take(watcher: &mut Watcher, message: &Message, now_us: u64) {
    let topic = message.header.topic.as_str();
    let _ = match topic {
        MicrophonePrivacy::TOPIC => message.parse().map(|value| watcher.privacy(&value, now_us)),
        Pause::TOPIC => message.parse().map(|value| watcher.pause(&value)),
        VoiceMute::TOPIC => message.parse().map(|value| watcher.mute(&value, now_us)),
        ExpressionRestState::TOPIC => message.parse().map(|value| watcher.rest(&value, now_us)),
        OnboardingPending::TOPIC => message.parse().map(|value| watcher.onboarding(&value)),
        VoiceState::TOPIC => message
            .parse()
            .map(|value| watcher.voice(&value, now_us, &message.header.boot)),
        RoomState::TOPIC => message.parse().map(|value| watcher.room(&value, now_us)),
        "light/command" => message
            .parse::<Light>()
            .map(|value| watcher.light_command(&value, &message.header.source)),
        "light/state" => message.parse().map(|value| watcher.light_state(&value)),
        _ => Ok(()),
    };
}
