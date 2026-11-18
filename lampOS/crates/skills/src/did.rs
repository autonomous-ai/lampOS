//! lamp-skills' side of `lamp/did` (#7757, `LAMP_DID_PUBLISH`, off): each run
//! that does an outward thing (a say, a light, a motion, a task) is told with
//! the facts it fired on, as one [`lamp_msgs::LampDid`] for each kind of thing
//! it did. The reason is built here from what the lamp itself measured
//! ("room/activity has held for 2820 s") and never from a message's words: a
//! trigger held on `voice/heard`, a list item's name or a model's call
//! arguments gives a plain generic reason, so no word a person said is ever
//! on `lamp/did`.
use crate::Call;
use lamp_msgs::{DidOutcome, DidWhat, LampDid};
use std::collections::HashMap;

/// `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_DID_PUBLISH";

/// Whether [`ENVIRONMENT`] is `on`.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// A bus topic as the skills name it: lower case, digits, `_` and `/`.
fn topic(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 48
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '/')
}

/// A whole number of at most 9 digits.
fn number(text: &str) -> bool {
    !text.is_empty() && text.len() <= 9 && text.chars().all(|c| c.is_ascii_digit())
}

/// The reason for a run whose trigger held as `held` says (the engine's own
/// words for it): only the two forms built from a topic and a number are
/// quoted; anything else, which may carry an item's name or the model's
/// arguments, becomes a generic reason with no data in it.
pub fn why(held: Option<&str>) -> String {
    let Some(held) = held else {
        return "it ran without a trigger's facts".into();
    };
    // "{topic} has held for {n} s"
    if let Some((topic_text, rest)) = held.split_once(" has held for ")
        && topic(topic_text)
        && let Some(seconds) = rest.strip_suffix(" s")
        && number(seconds)
    {
        return held.to_owned();
    }
    // "it appeared on {topic} after {n} s without"
    if let Some(rest) = held.strip_prefix("it appeared on ")
        && let Some((topic_text, tail)) = rest.split_once(" after ")
        && topic(topic_text)
        && let Some(seconds) = tail.strip_suffix(" s without")
        && number(seconds)
    {
        return held.to_owned();
    }
    if held.starts_with("the model called ") {
        return "called for in a conversation".into();
    }
    if held.ends_with("came to hold") {
        return "an item of its list came to hold".into();
    }
    "its trigger held".into()
}

/// What `calls` do outwardly, each kind once, in the order they first come.
pub fn kinds(calls: &[Call]) -> Vec<DidWhat> {
    let mut kinds = Vec::new();
    for call in calls {
        let kind = match call {
            // Said aloud, or a sound the lamp makes: a say, a read told, an audio
            // cue or a clip.
            Call::Say(_)
            | Call::Words(_)
            | Call::SayFrom(..)
            | Call::Text(_)
            | Call::ServiceRead(_)
            | Call::Sound { .. }
            | Call::Cue
            | Call::RequestedAudio(_) => DidWhat::Spoke,
            Call::Light { .. }
            | Call::Relative { .. }
            | Call::Show { .. }
            | Call::Dim { .. }
            | Call::Out { .. } => DidWhat::Lit,
            Call::Gesture(_)
            | Call::Move(_)
            | Call::Mood { .. }
            | Call::Stand(_)
            | Call::Gaze { .. }
            | Call::PrepareUnplug
            | Call::Rest(_) => DidWhat::Moved,
            // A task handed over, or a write to a service the lamp makes.
            Call::Task(_) | Call::ServiceWrite(_) => DidWhat::StartedTask,
            _ => continue,
        };
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    kinds
}

/// Who decided: the skill's id, which is its folder's name. A skill made on
/// this lamp from a person's own words may be named from them, so it is told
/// only as "a made skill" (#7766: no person's text on `lamp/did`).
fn who(skill: &str, made: bool) -> String {
    if made {
        "lamp-skills:a made skill".into()
    } else {
        format!("lamp-skills:{skill}")
            .chars()
            .take(LampDid::MAX_WHO)
            .collect()
    }
}

/// How the voice answered a say, from the engine's words on the run, when it
/// did not play it: "say: refused (...)" is Refused; one expired, dropped, let
/// go unsaid, or whose answer was lost ("the voice ended without an outcome",
/// "the voice's say service was not reached") is Cut. A say that was spoken,
/// or no say word at all, is None (the forms `answer_outcome` reads).
fn say_unplayed(last: &str) -> Option<DidOutcome> {
    let mut found = None;
    for part in last.split("; ") {
        let Some(said) = part.strip_prefix("say: ") else {
            continue;
        };
        if said.starts_with("refused") {
            return Some(DidOutcome::Refused);
        }
        if [
            "expired",
            "dropped",
            "unsaid",
            "the voice ended",
            "the voice's say service",
        ]
        .iter()
        .any(|unplayed| said.starts_with(unplayed))
        {
            found = Some(DidOutcome::Cut);
        }
    }
    found
}

/// How a run's `what` ended, from the engine's last words on the run: every
/// kind Refused when they lead with "not made" (a step refused inside a made
/// run is not that); a spoken kind Refused or Cut when the voice did not play
/// the say, so a muted lamp is never told as having spoken; else Done.
pub fn outcome_of(what: DidWhat, last: &str) -> DidOutcome {
    if last.starts_with("not made") {
        return DidOutcome::Refused;
    }
    if what == DidWhat::Spoke
        && let Some(unplayed) = say_unplayed(last)
    {
        return unplayed;
    }
    DidOutcome::Done
}

/// The dids of one run: one for each kind of outward thing in `kinds`, each
/// with the outcome `outcome` gives it.
fn dids(
    who: &str,
    why: &str,
    kinds: &[DidWhat],
    outcome: impl Fn(DidWhat) -> DidOutcome,
    at_us: u64,
) -> Vec<LampDid> {
    kinds
        .iter()
        .map(|what| LampDid {
            who: who.to_owned(),
            what: *what,
            why: why.to_owned(),
            at_us,
            outcome: outcome(*what),
        })
        .filter(|did| did.check().is_ok())
        .collect()
}

/// What lamp-skills has to tell of its runs: the runs under way, told when
/// they end and the outcome is known, and the ones refused, told at once. The
/// reason a run's trigger held on travels with the run (its `held`), so
/// queued runs of one skill never swap reasons. It makes the dids and never
/// publishes them.
#[derive(Debug, Default)]
pub struct Teller {
    under_way: HashMap<String, (String, Vec<DidWhat>, String)>,
}

impl Teller {
    /// A run of `skill` with `calls` and `held` is refused now (its last is
    /// still under way, or its tasks would not be handed over, or too many
    /// wait). The run under way is not touched.
    pub fn refused(
        &self,
        skill: &str,
        made: bool,
        calls: &[Call],
        held: Option<&str>,
        at_us: u64,
    ) -> Vec<LampDid> {
        dids(
            &who(skill, made),
            &why(held),
            &kinds(calls),
            |_| DidOutcome::Refused,
            at_us,
        )
    }

    /// A run of `skill` with `calls` and `held` is under way: told when it ends.
    pub fn begin(&mut self, skill: &str, made: bool, calls: &[Call], held: Option<&str>) {
        let kinds = kinds(calls);
        if kinds.is_empty() {
            return;
        }
        self.under_way
            .insert(skill.to_owned(), (who(skill, made), kinds, why(held)));
    }

    /// `skill`'s run ended with `last` as the engine's words on it: told with
    /// how it went.
    pub fn ended(&mut self, skill: &str, last: &str, at_us: u64) -> Vec<LampDid> {
        let Some((who, kinds, why)) = self.under_way.remove(skill) else {
            return Vec::new();
        };
        dids(&who, &why, &kinds, |what| outcome_of(what, last), at_us)
    }
}

#[cfg(test)]
mod tests;
