//! `lamp-addons serve` (#2530 part 1): reads `addon/proposal` and what the
//! lamp is doing, judges each proposal by [`crate::rules`], and asks the ring
//! for a kept light ([`crate::carry`]). Every proposal's verdict goes to the
//! journal, one line each, with why. A kept mood is said to `lamp-mood` as a
//! nudge on `mood/nudge`, which it holds inside its own bounds. A say past
//! the rules is put to the policy on `addon/ask` (#2530 part 2) and kept or
//! dropped by its answer on `policy/decisions`, `by` `jev`. A kept move is
//! asked of the body as one gesture at idle on its mood socket.
use crate::carry;
use crate::rules::{ASLEEP_FRESH_US, Drop, Judge, Verdict};
use crate::situation::{TOPICS, Watcher};
use lamp_bus::{Bus, Message, Publisher, Subscriber};
use lamp_msgs::{
    AddonAsk, AddonDecision, AddonProposal, Decision, ExpressionRestState, Light, LightRequest,
    MicrophonePrivacy, MoodNudge, MoodState, OnboardingPending, Pause, RoomState, VoiceMute,
    VoiceState,
};
use std::{
    io,
    os::unix::net::UnixDatagram,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// The product's switch, as `lamp-skills`' (`lamp_skills::ADDONS_ENVIRONMENT`).
pub const ENVIRONMENT: &str = "LAMP_ADDONS";

/// The body's mood socket, where a gesture is asked (`lamp-skills`' own
/// `--mood-socket` default).
pub const BODY_SOCKET: &str = "/run/lampos-body/mood.sock";

/// How long one pass waits for a proposal before reading the lamp again.
const PASS: Duration = Duration::from_millis(100);

/// How it runs.
#[derive(Clone, Debug)]
pub struct Config {
    pub bus_root: Option<PathBuf>,
    /// The claims' clock: the host's boot, or a test's.
    pub clock_id: Option<String>,
    pub run_for: Option<Duration>,
    /// A shorter quiet before a say, for tests only (`--say-quiet-ms`).
    pub say_quiet: Option<Duration>,
    /// A shorter quiet before a move, for tests only (`--move-quiet-ms`).
    pub move_quiet: Option<Duration>,
    /// The body's mood socket, [`BODY_SOCKET`] if unset.
    pub body_socket: Option<PathBuf>,
    /// Wait for a mute never said after startup or a voice restart. Tests
    /// may shorten it; a mute that was said keeps its own 3 s freshness.
    pub mute_grace_us: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bus_root: None,
            clock_id: None,
            run_for: None,
            say_quiet: None,
            move_quiet: None,
            body_socket: None,
            mute_grace_us: ASLEEP_FRESH_US,
        }
    }
}

/// What it did, said when it stops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub kept: u64,
    pub dropped: u64,
    pub lights_asked: u64,
    pub moods_nudged: u64,
    pub moves_asked: u64,
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
    let body = config
        .body_socket
        .clone()
        .unwrap_or_else(|| PathBuf::from(BODY_SOCKET));
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
    let mut asks = bus.publisher_declared(AddonAsk::TOPIC, &AddonAsk::declaration(), "addons")?;
    let mut answers = bus.subscribe(Decision::TOPIC, "addons")?;
    // The say put to the policy, while it waits for an answer.
    let mut asked: Option<AddonProposal> = None;
    let mut watcher =
        Watcher::new(lamp_ipc::monotonic_us(), config.mute_grace_us).on_clock(&clock_id);
    // What was there before it started: the kept values of each topic.
    for topic in TOPICS {
        if let Ok(Some(message)) = bus.latest(topic) {
            take(&mut watcher, &message, lamp_ipc::monotonic_us());
        }
    }
    let mut judge = config.say_quiet.map_or_else(Judge::default, |quiet| {
        Judge::with_say_quiet(quiet.as_micros() as u64)
    });
    if let Some(quiet) = config.move_quiet {
        judge = judge.with_move_quiet(quiet.as_micros() as u64);
    }
    let mut report = Report::default();
    eprintln!(
        "lamp-addons: judging addon/proposal; a kept light is asked of the ring at Idle, a kept move of the body"
    );
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
        while let Some(message) = answers.recv(Duration::ZERO)? {
            let Some(pending) = &asked else { continue };
            // Only the policy writes decisions; only one about this say counts.
            let Ok(answer) = message.parse::<Decision>() else {
                continue;
            };
            if message.header.source != "policy"
                || answer.about != AddonAsk::ABOUT
                || answer.proposal != Some(pending.proposal)
            {
                continue;
            }
            let now_us = lamp_ipc::monotonic_us();
            if let Some(verdict) = judge.answered(
                pending,
                answer.choice == "say_now",
                answer.shadow,
                &watcher.situation(now_us),
                now_us,
            ) {
                let why = match verdict {
                    Verdict::Kept => {
                        report.kept += 1;
                        "the policy said to say it now; the voice does not take an add-on's say yet (#2530)".to_owned()
                    }
                    Verdict::Dropped(drop) => {
                        report.dropped += 1;
                        drop.why().to_owned()
                    }
                    Verdict::Ask => continue,
                };
                // A deadline passed is the rules' drop, whatever was answered.
                let by = match verdict {
                    Verdict::Dropped(Drop::Late | Drop::PolicySilent) => "rule",
                    _ => "jev",
                };
                write(&mut decisions, pending, verdict, by, &why, now_us);
                asked = None;
            }
        }
        let now_us = lamp_ipc::monotonic_us();
        if let Some((proposal, drop)) = judge.lapsed(now_us)
            && let Some(pending) = asked.take_if(|pending| pending.proposal == proposal)
        {
            report.dropped += 1;
            write(
                &mut decisions,
                &pending,
                Verdict::Dropped(drop),
                "rule",
                drop.why(),
                now_us,
            );
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
        // Only lamp-skills and the own-computer service write proposals,
        // each stamping who made them.
        if !writer_may(&message.header.source, &proposal.addon) || proposal.check().is_err() {
            eprintln!(
                "lamp-addons: dropped a proposal from {:?}: not a proposal writer's, or not well formed",
                message.header.source
            );
            report.dropped += 1;
            continue;
        }
        let age_us = now_us.saturating_sub(message.header.at_us);
        let situation = watcher.situation(now_us);
        let verdict = judge.judge(&proposal, age_us, &situation, now_us);
        if verdict == Verdict::Ask {
            let ask = AddonAsk {
                proposal: proposal.proposal,
                addon: proposal.addon.clone(),
                why: proposal.why.clone(),
                sure: proposal.sure,
            };
            match asks.publish_json(now_us, &ask) {
                Ok(_) => {
                    eprintln!(
                        "lamp-addons: asked the policy about {} say #{}",
                        proposal.addon, proposal.proposal
                    );
                    asked = Some(proposal);
                }
                Err(error) => {
                    eprintln!(
                        "lamp-addons: the ask about #{} not written: {error}",
                        proposal.proposal
                    );
                    judge.lapsed(u64::MAX);
                    report.dropped += 1;
                    let verdict = Verdict::Dropped(Drop::PolicySilent);
                    write(
                        &mut decisions,
                        &proposal,
                        verdict,
                        "rule",
                        Drop::PolicySilent.why(),
                        now_us,
                    );
                }
            }
            continue;
        }
        let why = decide(
            &proposal,
            verdict,
            &clock_id,
            now_us,
            Outputs {
                lights: &mut lights,
                nudges: &mut nudges,
                body: &body,
                watcher: &mut watcher,
                over_persons: situation.persons_light,
            },
            &mut report,
        );
        write(&mut decisions, &proposal, verdict, "rule", &why, now_us);
    }
    Ok(report)
}

/// Whether `source` may write a proposal named `addon`: `lamp-skills` for
/// an add-on on the lamp, never under a computer's stamp, and the
/// own-computer service ([`crate::machine`]) only under its `own-` stamp
/// (`docs/addons.md`, "The judge takes a third writer, never as `skills`").
pub fn writer_may(source: &str, addon: &str) -> bool {
    let computer = addon.starts_with(crate::machine::OWN) || addon.starts_with("remote-");
    match source {
        "skills" => !computer,
        crate::machine::SOURCE => addon.starts_with(crate::machine::OWN),
        _ => false,
    }
}

/// One verdict said on `addon/decision`, `by` the rules or the policy.
fn write(
    decisions: &mut Publisher,
    proposal: &AddonProposal,
    verdict: Verdict,
    by: &str,
    why: &str,
    now_us: u64,
) {
    if let Verdict::Dropped(_) = verdict
        && by == "jev"
    {
        eprintln!(
            "lamp-addons: dropped {} say #{}: {why}",
            proposal.addon, proposal.proposal
        );
    } else if by == "jev" {
        eprintln!(
            "lamp-addons: kept {} say #{}: {why}",
            proposal.addon, proposal.proposal
        );
    }
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
        by: by.into(),
        why: why.chars().take(200).collect(),
    };
    if let Err(error) = decisions.publish_json(now_us, &decision) {
        eprintln!(
            "lamp-addons: the decision on #{} not written: {error}",
            proposal.proposal
        );
    }
}

/// Where a kept act goes: the ring's requests, Mood's nudges and the body's
/// mood socket; and whether a light lies over the person's own, which
/// rests, so is brief.
struct Outputs<'a> {
    lights: &'a mut Publisher,
    nudges: &'a mut Publisher,
    body: &'a Path,
    watcher: &'a mut Watcher,
    over_persons: bool,
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
        // Put to the policy before this is reached.
        Verdict::Ask => "asked of the policy".into(),
        Verdict::Dropped(drop) => {
            report.dropped += 1;
            eprintln!("lamp-addons: dropped {named}: {}", drop.why());
            drop.why().into()
        }
        Verdict::Kept => {
            report.kept += 1;
            if let Some(ask) = carry::gesture_ask(proposal) {
                let gesture = proposal.gesture.as_deref().unwrap_or_default();
                return match UnixDatagram::unbound()
                    .and_then(|socket| socket.send_to(ask.as_bytes(), out.body))
                {
                    Ok(_) => {
                        report.moves_asked += 1;
                        let why = format!("asked of the body at idle: {gesture}");
                        eprintln!("lamp-addons: kept {named}: {why}");
                        why
                    }
                    Err(error) => {
                        eprintln!("lamp-addons: kept {named}, not asked: {error}");
                        "kept, but the body was not reached".into()
                    }
                };
            }
            match carry::light_request(proposal, clock_id, now_us, out.over_persons) {
                Some(request) => match out.lights.publish_json(now_us, &request) {
                    Ok(_) => {
                        report.lights_asked += 1;
                        out.watcher.glowing_until(request.claim.until_us);
                        let why = format!(
                            "asked of the ring at idle for {} ms{}",
                            (request.claim.until_us - now_us) / 1_000,
                            if out.over_persons {
                                ", over the person's own light, which rests"
                            } else {
                                ""
                            }
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
        "light/command" => message.parse::<Light>().map(|value| {
            watcher.light_command(&value, &message.header.source, message.header.at_us)
        }),
        MoodState::TOPIC => message.parse().map(|value| watcher.mood(&value)),
        "light/state" => message.parse().map(|value| watcher.light_state(&value)),
        _ => Ok(()),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_writer_writes_only_under_its_own_names() {
        assert!(writer_may("skills", "evening-glow"));
        assert!(!writer_may("skills", "own-0123456789ab"));
        assert!(!writer_may("skills", "remote-0123456789ab"));
        assert!(writer_may("machine_addons", "own-0123456789ab"));
        assert!(!writer_may("machine_addons", "evening-glow"));
        assert!(!writer_may("machine_addons", "remote-0123456789ab"));
        assert!(!writer_may("voice", "evening-glow"));
        assert!(!writer_may("voice", "own-0123456789ab"));
    }

    #[test]
    fn the_product_waits_three_seconds_for_a_mute_never_said() {
        let config = Config::default();
        assert_eq!(config.mute_grace_us, 3_000_000);
        let watcher = Watcher::new(1_000_000, config.mute_grace_us);
        assert_eq!(watcher.situation(3_999_999).asleep, None);
        assert!(!watcher.situation(4_000_000).asleep.unwrap().value);
        assert_eq!(Watcher::default().situation(2_999_999).asleep, None);
        assert!(
            !Watcher::default()
                .situation(3_000_000)
                .asleep
                .unwrap()
                .value
        );
    }
}
