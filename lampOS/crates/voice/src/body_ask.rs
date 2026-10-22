//! The body's ask, said aloud (#2938). A body that refuses to start for
//! its pose lets go (or holds a head tipped up) and says on `diagnostics`
//! what it needs a person to do ([`lamp_body::ask::asks`]). The voice
//! says that line once when the body first lets go, and again only when the
//! lamp's name is heard (`name/heard`, at the called note's score), at most
//! once in [`AGAIN_AFTER_US`]: never for each report, and never for another
//! part's diagnostic. "Stopped asking" (`lamp_body::ask::STOPPED_ASKING`)
//! is never said; called by name after it, the last ask is.
//!
//! - **Said** through the say service ([`crate::say::OwnAsks`]): the same
//!   queue and gates as a skill's `text`, at priority 8, not begun after a
//!   minute. Nothing of what is said is logged, the outcome only.
//! - **Never on the voice's loop**: the bus is read on a thread of its own,
//!   and an ask is handed over without waiting.
//! - **An episode** is the body's asks without a break: another body
//!   report (well, or stopped) ends it, and so do [`GONE_AFTER_US`] without
//!   one (the body resends every 2 s, `lamp_body::bus_out::WAITING_RESEND`),
//!   so a body started again and let go again is said again.
//!
//! Off unless [`ENVIRONMENT`] is `on`, with `LAMP_SAY=on` and `LAMP_BUS=on`.
use crate::say::{Answer, OwnAsks};
use lamp_bus::Bus;
use lamp_msgs::{Diagnostic, NameHeard};
use std::{thread, time::Duration};

/// The switch, in the voice's environment. `on` only.
pub const ENVIRONMENT: &str = "LAMP_BODY_ASK";
/// The ask's skill name, for the say service's journal.
const SKILL: &str = "body";
/// A body that needs a hand comes before an ordinary say: 1 s of quiet.
const PRIORITY: u8 = crate::say::URGENT_FROM;
/// Not begun in a minute (the room busy), it is not said until called.
const EXPIRES_MS: u64 = 60_000;
/// Called by name, the ask is said again at most this often.
pub const AGAIN_AFTER_US: u64 = 10_000_000;
/// No ask read for this long: the episode is over.
pub const GONE_AFTER_US: u64 = 10_000_000;
/// The bus's name for this reader.
const SUBSCRIBER: &str = "voice_body_ask";
/// The longest a read of `diagnostics` waits before `name/heard` is read.
const READ_WITHIN: Duration = Duration::from_millis(100);

/// One episode of the body asking.
#[derive(Debug)]
struct Episode {
    seen_us: u64,
    /// The last ask a person can act on; none if only "stopped asking" was
    /// read.
    line: Option<String>,
    said_us: Option<u64>,
}

/// What the voice knows of the body's ask, and when it said it.
#[derive(Debug, Default)]
pub struct BodyAsk {
    episode: Option<Episode>,
}

impl BodyAsk {
    /// A diagnostic read at `now_us`: the line to say now, if any. Only
    /// the first ask of an episode is said.
    pub fn read(&mut self, report: &Diagnostic, now_us: u64) -> Option<String> {
        if report.name != lamp_body::ask::SOURCE {
            return None;
        }
        let Some(ask) = lamp_body::ask::asks(report) else {
            self.episode = None;
            return None;
        };
        let line = (ask != lamp_body::ask::STOPPED_ASKING).then(|| ask.to_owned());
        if let Some(episode) = self.episode.as_mut() {
            episode.seen_us = now_us;
            if line.is_some() {
                episode.line = line;
            }
            return None;
        }
        self.episode = Some(Episode {
            seen_us: now_us,
            said_us: line.as_ref().map(|_| now_us),
            line: line.clone(),
        });
        line
    }

    /// The lamp's name heard at `now_us`: the ask to say again, if the body
    /// is asking and it was not said in the last [`AGAIN_AFTER_US`].
    pub fn called(&mut self, now_us: u64) -> Option<String> {
        let episode = self.episode.as_mut()?;
        let line = episode.line.clone()?;
        if episode
            .said_us
            .is_some_and(|at| now_us.saturating_sub(at) < AGAIN_AFTER_US)
        {
            return None;
        }
        episode.said_us = Some(now_us);
        Some(line)
    }

    /// Time passes: no ask read for [`GONE_AFTER_US`] ends the episode.
    pub fn tick(&mut self, now_us: u64) {
        if self
            .episode
            .as_ref()
            .is_some_and(|episode| now_us.saturating_sub(episode.seen_us) >= GONE_AFTER_US)
        {
            self.episode = None;
        }
    }
}

/// Start the reader on a thread of its own, if [`ENVIRONMENT`] is `on`
/// and the bus is; one that cannot start is said once, and the voice goes
/// on without.
pub fn start_from_environment(asks: OwnAsks) {
    if std::env::var(ENVIRONMENT).ok().as_deref() != Some("on") {
        return;
    }
    if std::env::var(crate::bus_out::ENVIRONMENT).ok().as_deref() != Some("on") {
        eprintln!("lamp-voice: the body's ask is not said: the bus is off");
        return;
    }
    let started = Bus::from_environment().and_then(|bus| {
        let reports = bus.subscribe_typed(Diagnostic::TOPIC, SUBSCRIBER, Diagnostic::TYPE)?;
        let names = bus.subscribe_typed(NameHeard::TOPIC, SUBSCRIBER, NameHeard::TYPE)?;
        thread::Builder::new()
            .name("lamp-voice-body-ask".into())
            .spawn(move || run(reports, names, &asks))
    });
    if let Err(error) = started {
        eprintln!("lamp-voice: the body's ask is not said: {error}");
    }
}

fn run(mut reports: lamp_bus::Subscriber, mut names: lamp_bus::Subscriber, asks: &OwnAsks) {
    let mut ask = BodyAsk::default();
    loop {
        let read = reports.recv(READ_WITHIN);
        let now_us = lamp_ipc::monotonic_us();
        let mut line = match read {
            Ok(message) => message
                .and_then(|message| message.parse::<Diagnostic>().ok())
                .and_then(|report| ask.read(&report, now_us)),
            Err(_) => {
                // Not a tight loop on a bus gone wrong.
                thread::sleep(READ_WITHIN);
                None
            }
        };
        while let Ok(Some(message)) = names.recv(Duration::ZERO) {
            let plain = message
                .parse::<NameHeard>()
                .is_ok_and(|heard| heard.score >= crate::called::LEAST_SCORE);
            if plain && line.is_none() {
                line = ask.called(now_us);
            }
        }
        ask.tick(now_us);
        if let Some(line) = line {
            say(asks, &line);
        }
    }
}

/// Hand the ask over without waiting; its outcome is journalled from a
/// thread of its own, without the words.
fn say(asks: &OwnAsks, line: &str) {
    let answered = asks.text(SKILL, line, PRIORITY, EXPIRES_MS);
    let _ = thread::Builder::new()
        .name("lamp-voice-body-ask-outcome".into())
        .spawn(move || {
            let outcome = loop {
                match answered.recv() {
                    Ok(Answer::Done(outcome)) => break outcome.line(),
                    Ok(Answer::Begun(_)) => {}
                    Err(_) => break "the voice has gone".into(),
                }
            };
            eprintln!("lamp-voice: the body's ask: {outcome}");
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_body::ask::STOPPED_ASKING;
    use lamp_msgs::{KeyValue, Level};

    const ASK: &str = "My base is turned too far. Please turn it gently back toward the middle, and I will start by myself.";

    fn body(level: Level, message: &str, state: &str) -> Diagnostic {
        Diagnostic {
            level,
            name: "body".into(),
            message: message.into(),
            values: vec![KeyValue {
                key: "state".into(),
                value: state.into(),
            }],
        }
    }

    fn asking(message: &str) -> Diagnostic {
        body(Level::Error, message, "starting")
    }

    #[test]
    fn the_ask_is_said_once_per_let_go_not_for_each_identical_report() {
        let mut ask = BodyAsk::default();
        assert_eq!(ask.read(&asking(ASK), 0).as_deref(), Some(ASK));
        // Resent every 2 s, and paused: never said again by itself.
        for at in 1..20u64 {
            assert_eq!(ask.read(&asking(ASK), at * 2_000_000), None);
            ask.tick(at * 2_000_000);
        }
        assert_eq!(
            ask.read(&body(Level::Error, ASK, "paused"), 41_000_000),
            None
        );
        // Stopped asking is never said.
        assert_eq!(ask.read(&asking(STOPPED_ASKING), 42_000_000), None);
        // Set straight: the body is well, the episode is over, and the next
        // let-go is said again.
        assert_eq!(ask.read(&body(Level::Ok, "", "ready"), 43_000_000), None);
        assert_eq!(ask.read(&asking(ASK), 44_000_000).as_deref(), Some(ASK));
    }

    #[test]
    fn called_by_name_it_is_said_again_never_twice_in_ten_seconds() {
        let mut ask = BodyAsk::default();
        assert_eq!(ask.called(0), None, "nothing asked, nothing said");
        assert!(ask.read(&asking(ASK), 1_000_000).is_some());
        // Called a second later: just said.
        assert_eq!(ask.called(2_000_000), None);
        assert_eq!(ask.called(12_000_000).as_deref(), Some(ASK));
        assert_eq!(ask.called(12_500_000), None);
        // After it stopped asking, called by name says the last ask.
        assert_eq!(ask.read(&asking(STOPPED_ASKING), 13_000_000), None);
        assert_eq!(ask.called(30_000_000).as_deref(), Some(ASK));
    }

    #[test]
    fn other_diagnostics_are_never_said() {
        let mut ask = BodyAsk::default();
        let mut eyes = asking("the camera is not answering");
        eyes.name = "eyes".into();
        assert_eq!(ask.read(&eyes, 0), None);
        // The body's stop and its warnings are not asks.
        assert_eq!(
            ask.read(&body(Level::Error, "would not start: x", "stopping"), 0),
            None
        );
        assert_eq!(
            ask.read(&body(Level::Warn, "elbow is warm", "ready"), 0),
            None
        );
        assert_eq!(ask.called(20_000_000), None);
    }

    #[test]
    fn a_body_gone_quiet_ends_the_episode_and_its_next_ask_is_said() {
        let mut ask = BodyAsk::default();
        assert!(ask.read(&asking(ASK), 0).is_some());
        ask.tick(GONE_AFTER_US - 1);
        assert_eq!(ask.read(&asking(ASK), GONE_AFTER_US - 1), None);
        ask.tick(2 * GONE_AFTER_US);
        assert_eq!(ask.called(2 * GONE_AFTER_US), None);
        assert_eq!(
            ask.read(&asking(ASK), 2 * GONE_AFTER_US).as_deref(),
            Some(ASK)
        );
    }

    #[test]
    fn the_ask_reaches_the_say_service_as_written_text() {
        let mut says = crate::say::Says::idle_for_test();
        let _answered = says.own_asks().text(SKILL, ASK, PRIORITY, EXPIRES_MS);
        let now = lamp_ipc::monotonic_us();
        says.poll(now);
        let (turn, priority, ticket, quoted) = says.take(now).unwrap();
        assert_eq!(turn, format!("{} {ASK}", crate::say::AS_WRITTEN));
        assert_eq!((priority, ticket.skill(), quoted), (8, "body", false));
    }
}
