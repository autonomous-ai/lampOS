//! The Policy's logged attention decisions asked again of other decision
//! models (#871, `lamp-policy-replay`): the same question as on the day,
//! rebuilt from what was known ([`attention::state_text`] of the logged
//! context, the text that was sent), asked of each model in turn and made a
//! choice by the same weights ([`attention::combine`]), so that the models
//! can be laid side by side, and beside what should have happened where
//! someone has said.
//!
//! It runs where the logs are, since they hold what was heard. What it
//! gives back holds no words: for each decision its kinds (how many words,
//! whether the lamp was named in them, whether another language was heard),
//! and each model's choice, probability and time; then counts. The weights
//! were fitted to `typesafe/jev-1.13`'s answers: another model's answers go
//! through them as they are, so a model that answers on another scale is
//! judged by weights that were not fitted to it.
use crate::{
    attention::{self, Context, Room},
    decide::{Connection, Decider, Failed, Jev},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, BufRead, BufReader},
    path::Path,
    time::Duration,
};

/// One attention decision read back from the log.
#[derive(Clone, Debug, PartialEq)]
pub struct Logged {
    /// `2026-10-15#139`: the day's file and the line's number in it, from 1.
    pub id: String,
    pub at_unix_ms: u64,
    pub context: Context,
    /// What was chosen on the day, by whom.
    pub choice: String,
    pub probability: f64,
    pub by: String,
    /// The model's `follows_lamp` answer, if it gave one: the voice lets a
    /// `stay_quiet` from it at [`FOLLOWS_FROM`] or more run the lamp's own
    /// calls ([`agreement`]).
    pub follows_lamp: Option<f32>,
}

/// The voice's `policy_holds::FOLLOWS_FROM` (`crates/voice`, which this
/// crate does not link): from this `follows_lamp` up, the voice takes a
/// `stay_quiet` as `StayQuietFollowing` and runs the lamp's own calls.
/// Held equal to it by `lamp-voice-eval`'s `tests/pinned_copies.rs`.
pub const FOLLOWS_FROM: f32 = 0.6;

/// The attention decisions in `dir`'s files for `days` (`2026-10-15`), in
/// order, and how many lines could not be read as one.
pub fn read(dir: &Path, days: &[String]) -> io::Result<(Vec<Logged>, usize)> {
    let mut read = Vec::new();
    let mut unread = 0;
    for day in days {
        let path = dir.join(format!("decisions-{day}.jsonl"));
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        // Split on bytes: a line cut inside a character (a write the power
        // took) is not text, and is counted unread, not the end of the run.
        for (index, line) in BufReader::new(file).split(b'\n').enumerate() {
            let line = line?;
            let Ok(value) = serde_json::from_slice::<Value>(&line) else {
                unread += 1;
                continue;
            };
            if value["about"] != attention::ABOUT {
                continue;
            }
            match logged(&value, format!("{day}#{}", index + 1)) {
                Some(logged) => read.push(logged),
                None => unread += 1,
            }
        }
    }
    Ok((read, unread))
}

fn logged(value: &Value, id: String) -> Option<Logged> {
    Some(Logged {
        id,
        at_unix_ms: value["at_unix_ms"].as_u64()?,
        context: serde_json::from_value(value["context"].clone()).ok()?,
        choice: value["judgement"]["choice"].as_str()?.to_owned(),
        probability: value["judgement"]["probability"].as_f64()?,
        by: value["by"].as_str().unwrap_or("rule").to_owned(),
        follows_lamp: value["answers"]["follows_lamp"].as_f64().map(|v| v as f32),
    })
}

/// What should have happened, said by someone who knows: answered (or
/// looked up at), or left alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Should {
    Answer,
    StayQuiet,
}

impl Should {
    fn as_str(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::StayQuiet => "stay_quiet",
        }
    }

    /// Whether `choice` does what should have been done: `answer` and
    /// `look_up` both take the person up.
    fn met_by(self, choice: &str) -> bool {
        (choice == "stay_quiet") == (self == Self::StayQuiet)
    }
}

/// Labels, one JSON line each: `{"at_unix_ms": N, "should": "answer"}` or
/// `"stay_quiet"`, the moment as the replay's lines give it.
pub fn labels(text: &str) -> Result<BTreeMap<u64, Should>, String> {
    let mut labels = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line)
            .map_err(|_| format!("label line {} is not JSON", index + 1))?;
        let at = value["at_unix_ms"]
            .as_u64()
            .ok_or_else(|| format!("label line {} has no at_unix_ms", index + 1))?;
        let should = match value["should"].as_str() {
            Some("answer") => Should::Answer,
            Some("stay_quiet") => Should::StayQuiet,
            _ => {
                return Err(format!(
                    "label line {}: should is answer or stay_quiet",
                    index + 1
                ));
            }
        };
        // The same label twice at one moment is one label; two that
        // disagree at one moment are refused, whichever comes first (the
        // reviewer on #873): neither may become ground truth by its place.
        if let Some(earlier) = labels.insert(at, should)
            && earlier != should
        {
            return Err(format!(
                "label line {} says {} at {at}, where an earlier line says {}",
                index + 1,
                should.as_str(),
                earlier.as_str()
            ));
        }
    }
    Ok(labels)
}

/// What was heard, as kinds only: never its words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kinds {
    pub words: usize,
    /// The lamp was named in the words ([`attention::names_the_lamp`]).
    pub names_lamp: bool,
    /// Likely another language than English: a letter outside English's,
    /// or `¿` or `¡`, beyond the Vietnamese call "Lamp ơi" itself; or three
    /// words or more with none of [`ENGLISH`] among them ("Voglio mangiare
    /// sushi."). Rough, for counting apart; it decides nothing.
    pub other_language: bool,
}

/// Words nearly every English sentence said to a lamp has one of.
const ENGLISH: &[&str] = &[
    "a", "about", "all", "am", "an", "and", "are", "as", "at", "be", "but", "by", "can", "could",
    "did", "do", "does", "for", "from", "get", "go", "good", "had", "has", "have", "he", "her",
    "here", "hey", "hi", "him", "his", "how", "i", "i'm", "if", "in", "is", "it", "it's", "just",
    "know", "lamp", "let", "like", "me", "my", "no", "not", "now", "of", "ok", "okay", "on", "one",
    "or", "please", "say", "she", "so", "some", "stop", "tell", "that", "the", "their", "them",
    "then", "there", "they", "this", "time", "to", "too", "up", "us", "was", "we", "what", "when",
    "where", "which", "who", "why", "will", "with", "would", "yeah", "yes", "you", "your",
];

pub fn kinds(heard: &str) -> Kinds {
    let lowered = heard.to_lowercase().replace("lamp ơi", "lamp");
    let words: Vec<&str> = lowered
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .collect();
    let letters = lowered
        .chars()
        .any(|c| matches!(c, '¿' | '¡') || (c.is_alphabetic() && !c.is_ascii()));
    let no_english = words.len() >= 3 && !words.iter().any(|word| ENGLISH.contains(word));
    Kinds {
        words: heard.split_whitespace().count(),
        names_lamp: attention::names_the_lamp(heard),
        other_language: letters || no_english,
    }
}

/// One model's answer to one decision.
#[derive(Clone, Debug, PartialEq)]
pub enum Replayed {
    Chose {
        /// The model as its service named it.
        by: String,
        choice: String,
        probability: f64,
        took_ms: u64,
        /// Its seven answers, by the question's name: numbers only, for
        /// fitting the weights to this model if they do not fit it.
        answers: BTreeMap<String, f64>,
    },
    Failed(&'static str),
}

/// Ask `logged`'s question of `judge`, as it was asked on the day.
pub fn ask(judge: &mut dyn Decider, logged: &Logged, deadline: Duration) -> Replayed {
    let context = &logged.context;
    let asked = judge.ask(
        &attention::state_text(context),
        &attention::questions(),
        deadline,
    );
    match asked {
        Ok(asked) => match attention::Answers::of(&asked.answers) {
            Ok(answers) => {
                let judgement = attention::combine(&answers, context);
                Replayed::Chose {
                    by: asked.model,
                    choice: judgement.choice.as_str().to_owned(),
                    probability: f64::from(judgement.probability),
                    took_ms: u64::try_from(asked.took.as_millis()).unwrap_or(u64::MAX),
                    answers: answers
                        .by_name()
                        .into_iter()
                        .map(|(name, yes)| (name, round(f64::from(yes))))
                        .collect(),
                }
            }
            Err(_) => Replayed::Failed("missing"),
        },
        Err(failed) => Replayed::Failed(Failed::kind(&failed)),
    }
}

/// A judge whose connection a probe can steer (#1522).
pub trait Connections: Decider {
    /// Let go of the kept connection: the next asking makes its own.
    fn forget(&mut self);
    /// Keep a connection that has answered for `keep`.
    fn keep_answered_for(&mut self, keep: Duration);
}

impl Connections for Jev {
    fn forget(&mut self) {
        Jev::forget(self);
    }
    fn keep_answered_for(&mut self, keep: Duration) {
        Jev::keep_answered_for(self, keep);
    }
}

/// One asking of `--probe-connections`: how long nothing is asked first,
/// whether it goes on a connection of its own, and whether that one is made
/// ahead of the question, as lamp-policy does when someone speaks.
pub struct Probe {
    pub name: &'static str,
    pub after: Duration,
    pub fresh: bool,
    pub ahead: bool,
}

/// Nothing asked for this long before an asking "after quiet".
pub const QUIET: Duration = Duration::from_secs(300);
/// A connection made ahead is asked on this long after, about the length
/// of something said.
pub const AHEAD_BY: Duration = Duration::from_secs(2);
/// How long the probe keeps a connection that has answered, so that the
/// last kept asking finds out whether the other end kept it 300 s.
pub const PROBE_KEEPS: Duration = Duration::from_secs(600);

/// The first builder's question on #1522 (18 Oct 23:08): on lamp-4ace an
/// asking on a connection made `ahead` took 1,031 ms in the middle where one
/// on a kept connection took 573 ms. Made ahead always means after 2.5 quiet
/// minutes too, so these askings take the two apart: a new connection with
/// and without a quiet spell before it, a kept one at once and after 140 s
/// and 300 s (past lamp-policy's 150 s), and one made ahead with and
/// without a quiet spell.
pub const PROBES: [Probe; 7] = [
    Probe {
        name: "fresh_after_quiet",
        after: QUIET,
        fresh: true,
        ahead: false,
    },
    Probe {
        name: "kept_at_once",
        after: Duration::ZERO,
        fresh: false,
        ahead: false,
    },
    Probe {
        name: "fresh_at_once",
        after: Duration::ZERO,
        fresh: true,
        ahead: false,
    },
    Probe {
        name: "ahead_at_once",
        after: Duration::ZERO,
        fresh: true,
        ahead: true,
    },
    Probe {
        name: "kept_after_140s",
        after: Duration::from_secs(140),
        fresh: false,
        ahead: false,
    },
    Probe {
        name: "kept_after_300s",
        after: Duration::from_secs(300),
        fresh: false,
        ahead: false,
    },
    Probe {
        name: "ahead_after_quiet",
        after: QUIET,
        fresh: true,
        ahead: true,
    },
];

/// The newest decision a model made, whose words went to a judge on the
/// day: never one the rule made alone (`--rule-decides`), whose words stayed
/// on the lamp (the reviewer on #1630).
pub fn newest_asked(logged: &[Logged]) -> Option<&Logged> {
    logged.iter().rev().find(|line| line.by != "rule")
}

/// Asks `logged`'s question of `judge` as [`PROBES`] say, `rounds` times,
/// waiting with `wait`, and gives `each` one line for each asking as it is
/// made: its name, what it was sent on, how long it took, and whether it
/// was answered. No words.
pub fn probe_connections(
    judge: &mut dyn Connections,
    logged: &Logged,
    deadline: Duration,
    rounds: u32,
    wait: &mut dyn FnMut(Duration),
    each: &mut dyn FnMut(&Value),
) {
    judge.keep_answered_for(PROBE_KEEPS);
    let (state, questions) = (
        attention::state_text(&logged.context),
        attention::questions(),
    );
    for round in 1..=rounds {
        for probe in &PROBES {
            wait(probe.after);
            if probe.fresh {
                judge.forget();
            }
            if probe.ahead {
                judge.ready(AHEAD_BY);
                wait(AHEAD_BY);
            }
            let line = match judge.ask(&state, &questions, deadline) {
                Ok(asked) => json!({
                    "round": round,
                    "probe": probe.name,
                    "on": match asked.connection {
                        Some(Connection::New) => "new",
                        Some(Connection::Ahead) => "ahead",
                        Some(Connection::Kept) => "kept",
                        Some(Connection::Replaced) => "replaced",
                        None => "none",
                    },
                    "took_ms": u64::try_from(asked.took.as_millis()).unwrap_or(u64::MAX),
                }),
                Err(failed) => json!({
                    "round": round,
                    "probe": probe.name,
                    "failed": Failed::kind(&failed),
                }),
            };
            each(&line);
        }
    }
}

/// The line for one decision: its kinds, what was chosen on the day, what
/// should have happened if known, and each model's answer, in the order the
/// models were given. No words.
pub fn line(logged: &Logged, should: Option<Should>, replayed: &[(String, Replayed)]) -> Value {
    let kinds = kinds(&logged.context.heard);
    let judges: Vec<Value> = replayed
        .iter()
        .map(|(asked, replayed)| match replayed {
            Replayed::Chose {
                by,
                choice,
                probability,
                took_ms,
                answers,
            } => json!({"asked": asked, "by": by, "choice": choice,
                        "probability": round(*probability), "took_ms": took_ms,
                        "answers": answers}),
            Replayed::Failed(kind) => json!({"asked": asked, "failed": kind}),
        })
        .collect();
    json!({
        "id": logged.id,
        "at_unix_ms": logged.at_unix_ms,
        "room": logged.context.room,
        "in_conversation": logged.context.in_conversation(),
        "name_heard": logged.context.name_heard.map(|_| logged.context.was_called()),
        "words": kinds.words,
        "names_lamp": kinds.names_lamp,
        "other_language": kinds.other_language,
        "logged": {"by": logged.by, "choice": logged.choice, "probability": round(logged.probability)},
        "should": should.map(Should::as_str),
        "judges": judges,
    })
}

fn round(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

/// An earlier run's lines (`lamp-policy-replay` output, its summary line
/// left out) with what should have happened set again from `labels`, and
/// the models in them, in their order: so a run can be labelled after it is
/// seen, and counted again without asking anything.
pub fn relabelled(
    text: &str,
    labels: &BTreeMap<u64, Should>,
) -> Result<(Vec<Value>, Vec<String>), String> {
    let mut lines = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let mut value: Value =
            serde_json::from_str(line).map_err(|_| format!("line {} is not JSON", index + 1))?;
        if value["status"] == "replay_summary" {
            continue;
        }
        let at = value["at_unix_ms"]
            .as_u64()
            .ok_or_else(|| format!("line {} is not a replayed decision", index + 1))?;
        value["should"] = json!(labels.get(&at).map(|should| should.as_str()));
        lines.push(value);
    }
    let models = lines
        .first()
        .and_then(|line| line["judges"].as_array())
        .map(|judges| {
            judges
                .iter()
                .filter_map(|judge| judge["asked"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok((lines, models))
}

/// The counts over every line: for each model asked, how often it answered
/// and how fast (p50 and p95 of `took_ms`), what it chose, and, where what
/// should have happened is known, how often it did that: over all, for the
/// named requests, in a busy room and for another language. Then how often
/// each two models chose alike.
pub fn summary(lines: &[Value], models: &[String]) -> Value {
    let mut each = serde_json::Map::new();
    for (index, model) in models.iter().enumerate() {
        let of = |line: &Value| line["judges"].get(index).cloned().unwrap_or(Value::Null);
        let mut took: Vec<u64> = lines
            .iter()
            .filter_map(|line| of(line)["took_ms"].as_u64())
            .collect();
        took.sort_unstable();
        let mut failed = BTreeMap::new();
        let mut chose = BTreeMap::new();
        for line in lines {
            let answer = of(line);
            if let Some(kind) = answer["failed"].as_str() {
                *failed.entry(kind.to_owned()).or_insert(0_u64) += 1;
            }
            if let Some(choice) = answer["choice"].as_str() {
                *chose.entry(choice.to_owned()).or_insert(0_u64) += 1;
            }
        }
        let right = |keep: &dyn Fn(&Value) -> bool| {
            let labelled: Vec<&Value> = lines
                .iter()
                .filter(|line| keep(line) && line["should"].is_string())
                .filter(|line| of(line)["choice"].is_string())
                .collect();
            let met = labelled
                .iter()
                .filter(|line| {
                    let should = if line["should"] == "stay_quiet" {
                        Should::StayQuiet
                    } else {
                        Should::Answer
                    };
                    should.met_by(of(line)["choice"].as_str().unwrap_or_default())
                })
                .count();
            json!({"labelled": labelled.len(), "right": met})
        };
        let same_as_logged = lines
            .iter()
            .filter(|line| line["logged"]["by"] != "rule")
            .filter(|line| of(line)["choice"].is_string())
            .fold((0, 0), |(n, same), line| {
                (
                    n + 1,
                    same + usize::from(of(line)["choice"] == line["logged"]["choice"]),
                )
            });
        each.insert(
            model.clone(),
            json!({
                "asked": lines.len(),
                "answered": took.len(),
                "failed": failed,
                "took_ms": {"p50": percentile(&took, 50), "p95": percentile(&took, 95), "max": took.last()},
                "chose": chose,
                "right": right(&|_| true),
                "right_named": right(&|line| line["names_lamp"] == true),
                "right_busy": right(&|line| line["room"] == json!(Room::Busy)),
                "right_other_language": right(&|line| line["other_language"] == true),
                "same_as_logged": {"n": same_as_logged.0, "same": same_as_logged.1},
            }),
        );
    }
    let mut pairs = Vec::new();
    for first in 0..models.len() {
        for second in first + 1..models.len() {
            let both: Vec<(&Value, &Value)> = lines
                .iter()
                .map(|line| {
                    (
                        &line["judges"][first]["choice"],
                        &line["judges"][second]["choice"],
                    )
                })
                .filter(|(a, b)| a.is_string() && b.is_string())
                .collect();
            let same = both.iter().filter(|(a, b)| a == b).count();
            let same_side = both
                .iter()
                .filter(|(a, b)| (*a == "stay_quiet") == (*b == "stay_quiet"))
                .count();
            pairs.push(
                json!({"models": [models[first], models[second]], "both_answered": both.len(),
                              "same_choice": same, "same_side": same_side}),
            );
        }
    }
    json!({
        "status": "replay_summary",
        "decisions": lines.len(),
        "models": Value::Object(each),
        "pairs": pairs,
        "words": "none: kinds and counts only",
        "weights": "fitted to typesafe/jev-1.13's answers on 2026-10-13; every model's answers go through them",
    })
}

/// The lines of `pictures-YYYY-MM-DD.jsonl` for `days` (`lamp-policy
/// --images`, #871), in order.
pub fn read_pictures(dir: &Path, days: &[String]) -> io::Result<Vec<Value>> {
    let mut read = Vec::new();
    for day in days {
        let bytes = match fs::read(dir.join(format!("pictures-{day}.jsonl"))) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        // Split on bytes, as `read` does: a line cut inside a character is
        // passed over, not the end of the run (the reviewer on #1510).
        read.extend(
            bytes
                .split(|byte| *byte == b'\n')
                .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
                .filter(|line| line["about"] == "attention_picture"),
        );
    }
    Ok(read)
}

/// How far a label's moment may be from a picture line's: the line is
/// written at the moment the question was sent, and a label is written by a
/// scene's clock.
pub const LABEL_WITHIN_MS: u64 = 3_000;

/// Each label given to the one line nearest it within [`LABEL_WITHIN_MS`],
/// whatever that line is, a picture sent or not (the reviewer on #873): a
/// label is never given to more than one line, so a decision nobody labelled
/// is never taken for one somebody did. A label equally near two lines is
/// given to neither; two labels given to one line count only if they agree.
/// The labels per line, in order, and how they were matched.
fn assign(lines: &[Value], labels: &BTreeMap<u64, Should>) -> (Vec<Option<Should>>, Value) {
    let moments: Vec<Option<u64>> = lines
        .iter()
        .map(|line| line["at_unix_ms"].as_u64())
        .collect();
    let mut given: Vec<Vec<Should>> = vec![Vec::new(); lines.len()];
    let (mut unmatched, mut ambiguous) = (0_usize, 0_usize);
    for (at, should) in labels {
        let near: Vec<(usize, u64)> = moments
            .iter()
            .enumerate()
            .filter_map(|(index, moment)| Some((index, (*moment)?.abs_diff(*at))))
            .filter(|(_, distance)| *distance <= LABEL_WITHIN_MS)
            .collect();
        let Some(nearest) = near.iter().map(|(_, distance)| *distance).min() else {
            unmatched += 1;
            continue;
        };
        let at_nearest: Vec<usize> = near
            .iter()
            .filter(|(_, distance)| *distance == nearest)
            .map(|(index, _)| *index)
            .collect();
        match at_nearest.as_slice() {
            [one] => given[*one].push(*should),
            _ => ambiguous += 1,
        }
    }
    let mut conflicting = 0;
    let assigned: Vec<Option<Should>> = given
        .iter()
        .map(|labels| match labels.split_first() {
            Some((first, rest)) if rest.iter().all(|other| other == first) => Some(*first),
            Some(_) => {
                conflicting += labels.len();
                None
            }
            None => None,
        })
        .collect();
    let matched = given.iter().map(Vec::len).sum::<usize>() - conflicting;
    (
        assigned,
        json!({"given": labels.len(), "matched": matched, "unmatched": unmatched,
               "ambiguous": ambiguous, "conflicting": conflicting}),
    )
}

/// The counts over the picture lines, with no words: how often a frame was
/// sent, whether the service answered a request with it or that is not
/// known, how often the asking was withdrawn after one went, and why none
/// was sent; for the asking without the picture and with it, how often each
/// answered and failed, how fast, and how often it did what should have
/// happened; and for each picture question, and for the eyes' own count of
/// faces turned to the lamp, how often its yes would have been right. The
/// labels are matched by [`assign`].
///
/// Both askings are counted right on the same lines: a frame sent and both
/// chose. A line where either made no choice is in neither's right; why is
/// in that asking's `failed` (the reviewer on #873): one side is never counted on
/// lines the other is not.
pub fn pictures_summary(lines: &[Value], labels: &BTreeMap<u64, Should>) -> Value {
    let (assigned, matching) = assign(lines, labels);
    let mut why_not = BTreeMap::new();
    for line in lines {
        if let Some(why) = line["picture"]["why_not"].as_str() {
            *why_not.entry(why.to_owned()).or_insert(0_u64) += 1;
        }
    }
    let sent: Vec<(&Value, Option<Should>)> = lines
        .iter()
        .zip(assigned.iter().copied())
        .filter(|(line, _)| line["picture"]["sent"] == true)
        .collect();
    let mut delivery = BTreeMap::new();
    for (line, _) in &sent {
        if let Some(how) = line["picture"]["delivery"].as_str() {
            *delivery.entry(how.to_owned()).or_insert(0_u64) += 1;
        }
    }
    let withdrawn_after_sending = sent
        .iter()
        .filter(|(line, _)| line["picture"]["withdrawn"] == true)
        .count();
    // Labelled lines where both chose: what each is counted right on.
    let compared: Vec<(Should, &Value)> = sent
        .iter()
        .filter_map(|(line, should)| Some(((*should)?, *line)))
        .filter(|(_, line)| {
            line["without"]["choice"].is_string() && line["with"]["choice"].is_string()
        })
        .collect();
    let asking = |which: &str| {
        let mut took: Vec<u64> = sent
            .iter()
            .filter_map(|(line, _)| line[which]["took_ms"].as_u64())
            .collect();
        took.sort_unstable();
        let mut failed = BTreeMap::new();
        for (line, _) in &sent {
            if let Some(kind) = line[which]["failed"].as_str() {
                *failed.entry(kind.to_owned()).or_insert(0_u64) += 1;
            }
        }
        let right = compared
            .iter()
            .filter(|(should, line)| {
                should.met_by(line[which]["choice"].as_str().unwrap_or_default())
            })
            .count();
        json!({
            "answered": took.len(),
            "failed": failed,
            "took_ms": {"p50": percentile(&took, 50), "p95": percentile(&took, 95), "max": took.last()},
            "right": {"labelled": compared.len(), "right": right},
        })
    };
    // A sign is right when its yes goes with `yes_means`.
    let sign = |yes: &dyn Fn(&Value) -> Option<bool>, yes_means: Should| {
        let judged: Vec<(Should, bool)> = sent
            .iter()
            .filter_map(|(line, should)| Some(((*should)?, yes(line)?)))
            .collect();
        let right = judged
            .iter()
            .filter(|(should, yes)| *yes == (*should == yes_means))
            .count();
        json!({"labelled": judged.len(), "right": right,
               "yes": judged.iter().filter(|(_, yes)| *yes).count()})
    };
    let answered =
        |line: &Value, name: &str| line["with"]["answers"][name].as_f64().map(|yes| yes >= 0.5);
    // The questions that take a person for granted, and what a yes to
    // each means.
    const OF_A_PERSON: [(&str, Should); 4] = [
        ("picture_faces_lamp", Should::Answer),
        ("picture_asks_with_a_look", Should::Answer),
        ("picture_absorbed", Should::StayQuiet),
        ("picture_talking_to_another", Should::StayQuiet),
    ];
    let mut signs = serde_json::Map::new();
    for (name, means) in OF_A_PERSON
        .into_iter()
        .chain([("picture_away", Should::StayQuiet)])
    {
        let yes = |line: &Value| answered(line, name);
        signs.insert(name.to_owned(), sign(&yes, means));
    }
    let facing = |line: &Value| line["seen"]["facing"].as_u64().map(|facing| facing > 0);
    signs.insert("eyes_facing".to_owned(), sign(&facing, Should::Answer));
    // The same, only where the picture was said to have a person in it
    // (`picture_person`, the first builder on #871): on an empty desk they
    // are answered near chance.
    let mut given_a_person = serde_json::Map::new();
    for (name, means) in OF_A_PERSON {
        let yes = |line: &Value| {
            answered(line, "picture_person")
                .filter(|person| *person)
                .and_then(|_| answered(line, name))
        };
        given_a_person.insert(name.to_owned(), sign(&yes, means));
    }
    // Whether the picture's person and the eyes' faces agree, line by line.
    let both: Vec<(bool, bool)> = sent
        .iter()
        .filter_map(|(line, _)| {
            Some((
                answered(line, "picture_person")?,
                line["seen"]["faces"].as_u64()? > 0,
            ))
        })
        .collect();
    let person = json!({
        "lines": both.len(),
        "yes": both.iter().filter(|(person, _)| *person).count(),
        "eyes_saw_a_face": both.iter().filter(|(_, face)| *face).count(),
        "agree": both.iter().filter(|(person, face)| person == face).count(),
    });
    json!({
        "status": "pictures_summary",
        "questions": lines.len(),
        "picture_sent": sent.len(),
        "delivery": delivery,
        "withdrawn_after_sending": withdrawn_after_sending,
        "why_not": why_not,
        "without": asking("without"),
        "with": asking("with"),
        "signs": signs,
        "signs_given_a_person": given_a_person,
        "picture_person": person,
        "label_within_ms": LABEL_WITHIN_MS,
        "labels": matching,
        "words": "none: kinds and counts only",
    })
}

/// The `percent`th percentile of sorted `values`, by the nearest rank.
fn percentile(values: &[u64], percent: usize) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let rank = (percent * values.len()).div_ceil(100).max(1);
    values.get(rank - 1).copied()
}

pub mod refit;

/// Keep only the decisions made from `from` and before `until` (ms since
/// 1970): a night held out from the days its kinds were picked on.
pub fn within(logged: &mut Vec<Logged>, from: Option<u64>, until: Option<u64>) {
    logged.retain(|line| {
        from.is_none_or(|from| line.at_unix_ms >= from)
            && until.is_none_or(|until| line.at_unix_ms < until)
    });
}

/// `2026-10-18T11:00:00Z` as milliseconds since 1970, UTC only: a
/// window's edge for `lamp-policy-replay --agreement --from/--until`.
pub fn utc_ms(text: &str) -> Option<u64> {
    let text = text.strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;
    let number = |part: &str, digits: usize| {
        (part.len() == digits && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<i64>().ok())
            .flatten()
    };
    let mut date = date.split('-');
    let (year, month, day) = (
        number(date.next()?, 4)?,
        number(date.next()?, 2)?,
        number(date.next()?, 2)?,
    );
    let mut time = time.split(':');
    let (hour, minute, second) = (
        number(time.next()?, 2)?,
        number(time.next()?, 2)?,
        number(time.next()?, 2)?,
    );
    // The month's own length, leap years included: a day past it is a typo,
    // not the next month (the reviewer on #1539).
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if date.next().is_some()
        || time.next().is_some()
        || !(1..=12).contains(&month)
        || !(1..=days_in_month).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    // Days from 1970 by the civil calendar (Howard Hinnant's days_from_civil).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let of_era = y - era * 400;
    let of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let of_era_days = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    let days = era * 146_097 + of_era_days - 719_468;
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second;
    u64::try_from(seconds).ok().map(|seconds| seconds * 1_000)
}

/// One kind of attention decision, for [`agreement`]: what the rule
/// ([`attention::fallback`]) reads, and how plainly the name was heard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Kind {
    pub room: &'static str,
    pub in_conversation: bool,
    pub lamp_is_speaking: bool,
    /// The name's score in the lamp's own microphone: `none` (not heard,
    /// or nothing listening), `<0.5`, `0.5-0.9`, `0.9-0.99` or `>=0.99`.
    pub name: &'static str,
}

impl std::fmt::Display for Kind {
    /// As `--agreement` prints it and `lamp-policy --rule-decides` takes it:
    /// `busy false false >=0.99`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.room, self.in_conversation, self.lamp_is_speaking, self.name
        )
    }
}

impl Kind {
    /// A kind as printed ([`Kind`]'s `Display`), or `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let [room, conversation, speaking, name] = words.as_slice() else {
            return None;
        };
        let flag = |word: &str| match word {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        };
        Some(Self {
            room: ["quiet", "busy", "unknown"]
                .into_iter()
                .find(|r| r == room)?,
            in_conversation: flag(conversation)?,
            lamp_is_speaking: flag(speaking)?,
            name: ["none", "<0.5", "0.5-0.9", "0.9-0.99", ">=0.99"]
                .into_iter()
                .find(|n| n == name)?,
        })
    }

    pub fn of(context: &Context) -> Self {
        Self {
            room: match context.room {
                Room::Quiet => "quiet",
                Room::Busy => "busy",
                Room::Unknown => "unknown",
            },
            in_conversation: context.in_conversation(),
            lamp_is_speaking: context.lamp_is_speaking,
            name: match context.name_heard {
                None => "none",
                Some(score) if score < 0.5 => "<0.5",
                Some(score) if score < 0.9 => "0.5-0.9",
                Some(score) if score < 0.99 => "0.9-0.99",
                Some(_) => ">=0.99",
            },
        }
    }
}

/// How often the rule and the model agreed on one kind of decision, about
/// whether the lamp takes the turn (an answer or a look up) or stays quiet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Agreement {
    pub decisions: u32,
    pub agreed: u32,
    /// The rule would have taken the turn, the model stayed quiet.
    pub rule_took_model_quiet: u32,
    /// The rule would have stayed quiet, the model took the turn.
    pub rule_quiet_model_took: u32,
    /// Of the model's turns taken, how many were a `stay_quiet` the voice
    /// takes as following the lamp ([`FOLLOWS_FROM`]): it runs the lamp's
    /// own calls on them, where the rule's plain `stay_quiet` would hold
    /// them (Fourth and the reviewer on #1510).
    pub model_quiet_following: u32,
}

/// The decisions a model made (not the rule's own), each set beside what
/// the rule would have said of the same context, counted by [`Kind`]: where
/// the two never disagreed over enough decisions, the rule could decide
/// without asking the model (#705, Ten's step 2 on the Policy's wait, with
/// Fourth's conditions: the certain kinds come from the record, and run in
/// shadow first). Nothing of what was heard is in it.
pub fn agreement(logged: &[Logged]) -> BTreeMap<Kind, Agreement> {
    let mut kinds: BTreeMap<Kind, Agreement> = BTreeMap::new();
    for line in logged.iter().filter(|line| line.by != "rule") {
        let rule_takes = attention::fallback(&line.context).choice != attention::Choice::StayQuiet;
        // A following `stay_quiet` is the lamp's turn for its calls, as the
        // voice takes it.
        let following = line.choice == "stay_quiet"
            && line
                .follows_lamp
                .is_some_and(|follows| follows >= FOLLOWS_FROM);
        let model_takes = line.choice != "stay_quiet" || following;
        let counted = kinds.entry(Kind::of(&line.context)).or_default();
        counted.decisions += 1;
        counted.model_quiet_following += u32::from(following);
        match (rule_takes, model_takes) {
            (true, false) => counted.rule_took_model_quiet += 1,
            (false, true) => counted.rule_quiet_model_took += 1,
            _ => counted.agreed += 1,
        }
    }
    kinds
}

/// [`agreement`] as printed lines: one per kind, most decisions first, and
/// then the kinds with at least `least` decisions and no disagreement.
pub fn agreement_summary(kinds: &BTreeMap<Kind, Agreement>, least: u32) -> String {
    let mut rows: Vec<(&Kind, &Agreement)> = kinds.iter().collect();
    rows.sort_by(|a, b| b.1.decisions.cmp(&a.1.decisions).then(a.0.cmp(b.0)));
    let mut out = String::from(
        "room conversation speaking name | decisions agreed rule_took_model_quiet rule_quiet_model_took model_quiet_following\n",
    );
    for (kind, counted) in &rows {
        out.push_str(&format!(
            "{} {} {} {} | {} {} {} {} {}\n",
            kind.room,
            kind.in_conversation,
            kind.lamp_is_speaking,
            kind.name,
            counted.decisions,
            counted.agreed,
            counted.rule_took_model_quiet,
            counted.rule_quiet_model_took,
            counted.model_quiet_following
        ));
    }
    let total: u32 = rows.iter().map(|(_, counted)| counted.decisions).sum();
    let certain: Vec<_> = rows
        .iter()
        .filter(|(_, counted)| counted.decisions >= least && counted.agreed == counted.decisions)
        .collect();
    // With n agreeing and none against, a true disagreement rate up to
    // 1 - 0.05^(1/n) is still likely at 95 %: a list for the shadow night,
    // not proof (the reviewer on #1510).
    let bound = 1.0 - 0.05_f64.powf(1.0 / f64::from(least.max(1)));
    out.push_str(&format!(
        "\nkinds with at least {least} decisions and no disagreement: {} of {} kinds, {} of {total} decisions (with {least} of {least} agreeing, a disagreement rate up to {:.0} % is still likely at 95 %: for the shadow night, not proof)\n",
        certain.len(),
        rows.len(),
        certain.iter().map(|(_, counted)| counted.decisions).sum::<u32>(),
        bound * 100.0
    ));
    for (kind, counted) in certain {
        out.push_str(&format!(
            "  {} {} {} {}: {}\n",
            kind.room, kind.in_conversation, kind.lamp_is_speaking, kind.name, counted.decisions
        ));
    }
    out
}

#[cfg(test)]
mod tests;
