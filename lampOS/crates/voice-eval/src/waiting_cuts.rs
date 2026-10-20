//! `waiting-cuts` (#1866): waiting answers held, cut and kept, on both voice
//! routes, from `journalctl -u lamp-voice -o short-iso` and, read only, the
//! session records of the same runs
//! (`<sessions>/<invocation>/metadata/events-*.jsonl`).
//!
//! The journal alone cannot give these counts: the wait's decisions
//! (`answer_waits`, `input_let_go_answer_waits`, `wait_words`), whether what
//! cut an answer had words (the input `transcript`), and a turn's route
//! (`voice_route_switched`) are records, not journal lines. The journal gives
//! each run's start, `second only`, `wait words` and the second route's own
//! lines. Sessions are paired with the journal's runs by when each session's
//! folder was made.
//!
//! Only counts, fixed category names and numbers are ever output: never a
//! transcript, a name or any other free text from the journal or records.
//! A voice line near these (wait, stop, request, route, cut, interrupt) that
//! no pattern knows is counted as `unclassified`, unprinted.

use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

pub const SESSIONS: &str = "/var/lib/lampOS/voice/sessions";
const ROUTES: [&str; 2] = ["live", "second"];
/// A follow-up is short under this many speech blocks
/// (`lamp_voice::not_speech::SHORT_BLOCKS`).
const SHORT_BLOCKS: u64 = 80;
/// A session's folder is made a moment after its run's start is journaled.
const PAIR_SLACK_S: i64 = 30;
const STOP_REASONS: [&str; 3] = ["hand_stop", "head_stop", "spoken_stop"];
const NEAR: [&str; 6] = ["wait", "stop", "request", "route", "cut", "interrupt"];

/// A journal line counted: its category, its route (None: the run's), the
/// line with `{n}` for each number, whether more may follow, and which
/// number is a time in ms.
struct Pattern {
    name: &'static str,
    route: Option<&'static str>,
    text: &'static str,
    prefix: bool,
    ms: Option<usize>,
}
const fn p(
    name: &'static str,
    route: Option<&'static str>,
    text: &'static str,
    prefix: bool,
    ms: Option<usize>,
) -> Pattern {
    Pattern {
        name,
        route,
        text,
        prefix,
        ms,
    }
}
const SECOND: Option<&str> = Some("second");
const JOURNAL: &[Pattern] = &[
    p(
        "route_second_only",
        None,
        "lamp-voice: the voice route is second only (LAMP_VOICE_ROUTE=second)",
        true,
        None,
    ),
    p(
        "second_route_worker_exited",
        None,
        "lamp-voice: the second voice route's worker exited",
        true,
        None,
    ),
    p(
        "wait_words_ready",
        None,
        "lamp-voice: wait words: a sound while an answer waits is judged by its words",
        true,
        None,
    ),
    p(
        "wait_words_unavailable",
        None,
        "lamp-voice: LAMP_WAIT_KEEPS_ANSWER=words has no speech-to-text",
        true,
        None,
    ),
    p(
        "wait_words_unavailable",
        None,
        "lamp-voice: wait words are not heard",
        true,
        None,
    ),
    p(
        "wait_words_stream_ready",
        None,
        "lamp-voice: wait words: a stream is ready, connected in {n} ms",
        true,
        Some(0),
    ),
    p(
        "stop_words_ready",
        None,
        "lamp-voice: stop words: heard",
        true,
        None,
    ),
    p(
        "stop_phrase_acts",
        None,
        "lamp-voice: stop words: a held stop phrase acts after {n} ms held",
        true,
        Some(0),
    ),
    p(
        "stop_phrase_was_lamps_own",
        None,
        "lamp-voice: stop words: a stop phrase heard was the lamp's own, let go after {n} ms held",
        true,
        Some(0),
    ),
    p(
        "stop_phrase_over_voice",
        None,
        "lamp-voice: stop words: a stop phrase was heard over the lamp's voice",
        false,
        None,
    ),
    p(
        "yield_words_stop",
        None,
        "lamp-voice: a yield's words came: {n} word, a stop phrase",
        false,
        None,
    ),
    p(
        "yield_words_stop",
        None,
        "lamp-voice: a yield's words came: {n} words, a stop phrase",
        false,
        None,
    ),
    p(
        "yield_words_not_stop",
        None,
        "lamp-voice: a yield's words came: {n} word, not a stop phrase",
        false,
        None,
    ),
    p(
        "yield_words_not_stop",
        None,
        "lamp-voice: a yield's words came: {n} words, not a stop phrase",
        false,
        None,
    ),
    p(
        "yield_no_words",
        None,
        "lamp-voice: a yield ended before its words came",
        false,
        None,
    ),
    p(
        "short_sound_after_stop",
        None,
        "lamp-voice: a short sound just after the lamp was stopped is not taken as a request",
        false,
        None,
    ),
    // Special-cased: its second number is a word count, its third a time.
    p(
        "second_heard",
        SECOND,
        "lamp-voice second: request {n} heard {n} words, the last {n} ms after the line ended",
        false,
        None,
    ),
    p(
        "second_goes_on",
        SECOND,
        "lamp-voice second: request {n} goes on from the request given up: its {n} words begin it",
        false,
        None,
    ),
    p(
        "second_first_audio",
        SECOND,
        "lamp-voice second: request {n} first answer audio {n} ms after the words were heard",
        false,
        Some(1),
    ),
    p(
        "second_answer_held_back",
        SECOND,
        "lamp-voice second: request {n}: its answer is held back",
        true,
        None,
    ),
    p(
        "second_answer_played",
        SECOND,
        "lamp-voice second: request {n}: its answer is played",
        true,
        None,
    ),
];

/// The numbers in `message` where `pattern` has `{n}`, if it matches.
fn matches(pattern: &Pattern, message: &str) -> Option<Vec<u64>> {
    let mut rest = message;
    let mut numbers = Vec::new();
    let mut parts = pattern.text.split("{n}").peekable();
    while let Some(literal) = parts.next() {
        rest = rest.strip_prefix(literal)?;
        if parts.peek().is_some() {
            let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            numbers.push(rest[..digits].parse().ok()?);
            rest = &rest[digits..];
        }
    }
    (pattern.prefix || rest.is_empty()).then_some(numbers)
}

/// The first of [`NEAR`] that begins a word in `message`.
fn near(message: &str) -> Option<&'static str> {
    let lower = message.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    (0..bytes.len())
        .filter(|&at| at == 0 || !(bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_'))
        .find_map(|at| {
            NEAR.iter()
                .copied()
                .find(|word| lower[at..].starts_with(word))
        })
}

/// A short-iso stamp (`2026-10-19T20:01:02+0700`, fractions allowed): unix
/// seconds, and its wall clock as `YYYY-MM-DD HH:MM:SS`.
pub fn parse_time(stamp: &str) -> Option<(i64, String)> {
    let field = |from: usize, to: usize| stamp.get(from..to)?.parse::<i64>().ok();
    let (year, month, day) = (field(0, 4)?, field(5, 7)?, field(8, 10)?);
    let (hour, minute, second) = (field(11, 13)?, field(14, 16)?, field(17, 19)?);
    if stamp.get(10..11)? != "T" {
        return None;
    }
    let zone = stamp[19..].trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let sign = match zone.get(..1)? {
        "+" => 1,
        "-" => -1,
        _ => return None,
    };
    let zone = zone[1..].replace(':', "");
    let offset =
        zone.get(..2)?.parse::<i64>().ok()? * 3600 + zone.get(2..4)?.parse::<i64>().ok()? * 60;
    let days = crate::export::days_from_civil(year, month, day);
    let unix = days * 86_400 + hour * 3600 + minute * 60 + second - sign * offset;
    Some((unix, format!("{} {}", &stamp[..10], &stamp[11..19])))
}

/// `--since`/`--until` as a wall clock to compare: `YYYY-MM-DD HH:MM[:SS]`
/// (or with `T`), in the journal's own zone.
pub fn wall_bound(text: &str) -> Option<String> {
    let text = text.replacen('T', " ", 1);
    let text = if text.len() == 16 {
        format!("{text}:00")
    } else {
        text
    };
    let shape = "0000-00-00 00:00:00";
    (text.len() == shape.len()
        && text.bytes().zip(shape.bytes()).all(|(c, s)| {
            if s == b'0' {
                c.is_ascii_digit()
            } else {
                c == s
            }
        }))
    .then_some(text)
}

#[derive(Default)]
pub struct Run {
    started: Option<i64>,
    started_wall: Option<String>,
    route: &'static str,
    mode: Option<&'static str>,
    sessions: u64,
    unreadable: u64,
    journal: BTreeMap<&'static str, u64>,
    counts: [BTreeMap<String, u64>; 2],
    times: [BTreeMap<String, Vec<f64>>; 2],
    unclassified: BTreeMap<String, u64>,
}
fn index(route: &str) -> usize {
    usize::from(route == "second")
}
impl Run {
    fn new(started: Option<(i64, String)>) -> Self {
        let (started, started_wall) = started.unzip();
        Self {
            started,
            started_wall,
            route: "live",
            ..Self::default()
        }
    }
    fn add(&mut self, route: &str, name: &str) {
        *self.counts[index(route)]
            .entry(name.to_owned())
            .or_default() += 1;
    }
    fn time(&mut self, route: &str, name: &str, ms: Option<f64>) {
        if let Some(ms) = ms.filter(|ms| *ms >= 0.0) {
            self.times[index(route)]
                .entry(name.to_owned())
                .or_default()
                .push(ms);
        }
    }
    fn miss(&mut self, name: &str) {
        *self.unclassified.entry(name.to_owned()).or_default() += 1;
    }
}

/// The journal's window: its first and last stamps, in unix seconds.
type Window = Option<(i64, i64)>;

fn read_journal(text: &str, since: Option<&str>, until: Option<&str>) -> (Vec<Run>, Window) {
    let mut runs: Vec<Run> = Vec::new();
    let mut window: Window = None;
    for line in text.lines() {
        let mut fields = line.splitn(3, ' ');
        let (Some(stamp), Some(_host), Some(rest)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let Some((ident, message)) = rest.split_once(": ") else {
            continue;
        };
        let ident = ident.split('[').next().unwrap_or(ident);
        let time = parse_time(stamp);
        if since.is_some() || until.is_some() {
            let Some((_, wall)) = &time else { continue };
            if since.is_some_and(|since| wall.as_str() < since)
                || until.is_some_and(|until| wall.as_str() > until)
            {
                continue;
            }
        }
        if let Some((at, _)) = &time {
            window = Some(window.map_or((*at, *at), |(first, _)| (first, *at)));
        }
        if ident == "systemd"
            && (message.starts_with("Started lamp-voice.service")
                // systemd before 249 names only the Description.
                || message.starts_with("Started lampOS voice (lamp-voice serve)"))
        {
            runs.push(Run::new(time));
            continue;
        }
        if !message.starts_with("lamp-voice") {
            continue;
        }
        if runs.is_empty() {
            runs.push(Run::new(None));
        }
        let Some(run) = runs.last_mut() else { continue };
        let found = JOURNAL
            .iter()
            .find_map(|pattern| matches(pattern, message).map(|numbers| (pattern, numbers)));
        let Some((pattern, numbers)) = found else {
            if let Some(word) = near(message) {
                run.miss(&format!("journal:{word}"));
            }
            continue;
        };
        if pattern.name == "route_second_only" {
            run.route = "second";
        }
        let ms = pattern
            .ms
            .and_then(|at| numbers.get(at))
            .map(|ms| *ms as f64);
        match pattern.route {
            Some(route) if pattern.name == "second_heard" => {
                let words = numbers.get(1).copied().unwrap_or(0);
                run.add(
                    route,
                    if words > 0 {
                        "second_heard_words"
                    } else {
                        "second_heard_no_words"
                    },
                );
                run.time(
                    route,
                    "second_words_after_line_ms",
                    numbers.get(2).map(|ms| *ms as f64),
                );
            }
            Some(route) => {
                run.add(route, pattern.name);
                run.time(route, &format!("{}_ms", pattern.name), ms);
            }
            None => {
                *run.journal.entry(pattern.name).or_default() += 1;
                let route = run.route;
                run.time(route, &format!("{}_ms", pattern.name), ms);
            }
        }
    }
    (runs, window)
}

/// A session folder: where, when it was made, when its records were last
/// written (unix seconds).
pub type Session = (PathBuf, i64, i64);

fn modified(path: &Path) -> Option<i64> {
    let at = fs::metadata(path).ok()?.modified().ok()?;
    Some(at.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64)
}

/// The session folders under `root` (32 lowercase hex, with `metadata/`).
pub fn find_sessions(root: &Path) -> Vec<Session> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let folder = entry.path();
        let meta = folder.join("metadata");
        if name.len() != 32
            || !name
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !meta.is_dir()
        {
            continue;
        }
        let end = fs::read_dir(&meta)
            .map(|files| {
                files
                    .flatten()
                    .filter(|file| file.file_name().to_string_lossy().ends_with(".jsonl"))
                    .filter_map(|file| modified(&file.path()))
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        if let Some(start) = modified(&folder) {
            found.push((folder, start, end));
        }
    }
    found
}

fn records(folder: &Path) -> std::io::Result<Vec<Value>> {
    let meta = folder.join("metadata");
    let mut names: Vec<PathBuf> = fs::read_dir(&meta)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    names.sort();
    let mut events = Vec::new();
    for path in names {
        let bytes = fs::read(path)?;
        for line in String::from_utf8_lossy(&bytes).lines() {
            if let Ok(event) = serde_json::from_str::<Value>(line)
                && event["service_sequence"].is_u64()
            {
                events.push(event);
            }
        }
    }
    events.sort_by_key(|event| event["service_sequence"].as_u64());
    Ok(events)
}

/// Counted, never kept: the tokens with a letter or digit in them.
fn words_in(text: &str) -> u64 {
    text.split_whitespace()
        .filter(|token| token.chars().any(char::is_alphanumeric))
        .count() as u64
}

fn ms(later: Option<u64>, earlier: Option<u64>) -> Option<f64> {
    Some((later? as f64 - earlier? as f64) / 1000.0)
}

struct Cut {
    turn: Option<u64>,
    at: Option<u64>,
    route: &'static str,
    waited: bool,
    successor: Option<u64>,
}

fn read_session(run: &mut Run, events: &[Value]) {
    let mut route = run.route;
    let mut admitted_last = None;
    let mut words: BTreeMap<u64, u64> = BTreeMap::new();
    let mut blocks: BTreeMap<u64, u64> = BTreeMap::new();
    let mut endpoint: BTreeMap<u64, Option<u64>> = BTreeMap::new();
    // candidate -> (at_us, route) of a sound held
    let mut waits: BTreeMap<u64, (u64, &'static str)> = BTreeMap::new();
    let mut turns_waited = BTreeSet::new();
    let mut cuts: Vec<Cut> = Vec::new();
    let mut pending_cut = false;
    let mut stop_said_at = None;
    let mut mode = None;
    for event in events {
        let kind = event["kind"].as_str().unwrap_or("");
        let at = event["at_us"].as_u64();
        let turn = event["turn"].as_u64();
        match kind {
            "wait_keeps_answer" => {
                mode = Some(if event["words"] == json!(true) {
                    "words"
                } else {
                    "on"
                });
            }
            "voice_route_switched" => match event["to"].as_str() {
                Some("live") => route = "live",
                Some("second") => route = "second",
                _ => run.miss(kind),
            },
            "input_admitted" => {
                admitted_last = turn;
                if event["admission_basis"] == json!("continued_after_early_end") {
                    run.add(route, "went_on_after_early_end");
                }
                if std::mem::take(&mut pending_cut)
                    && let Some(cut) = cuts.last_mut()
                {
                    cut.successor = turn;
                }
                if let Some(candidate) = event["candidate"].as_u64()
                    && let Some((held_at, where_)) = waits.remove(&candidate)
                {
                    run.add(where_, "wait_took_turn");
                    run.time(where_, "wait_hold_ms", ms(at, Some(held_at)));
                }
            }
            "transcript" => {
                if event["turn"].is_null()
                    && let (Some(last), Some(text)) = (admitted_last, event["text"].as_str())
                {
                    *words.entry(last).or_default() += words_in(text);
                }
            }
            "local_endpoint" => {
                if let Some(turn) = turn {
                    endpoint.insert(turn, at);
                    let count = event["speech_level"]["blocks"]
                        .as_u64()
                        .or_else(|| event["not_speech"]["speech_blocks"].as_u64());
                    if let Some(count) = count {
                        blocks.insert(turn, count);
                    }
                }
            }
            "answer_waits" => {
                let (Some(candidate), Some(at)) = (event["candidate"].as_u64(), at) else {
                    run.miss(kind);
                    continue;
                };
                waits.insert(candidate, (at, route));
                turns_waited.insert(turn);
                run.add(route, "wait_held_a_sound");
            }
            "input_let_go_answer_waits" => {
                let held = event["candidate"]
                    .as_u64()
                    .and_then(|candidate| waits.remove(&candidate));
                let where_ = held.map_or(route, |(_, where_)| where_);
                match event["why"].as_str() {
                    Some("quiet") => run.add(where_, "wait_let_go_quiet"),
                    Some("ended") => run.add(where_, "wait_let_go_ended"),
                    Some("stop_said") => {
                        run.add(where_, "wait_stop_said");
                        stop_said_at = at;
                    }
                    _ => {
                        run.miss(kind);
                        continue;
                    }
                }
                if let Some((held_at, _)) = held {
                    run.time(where_, "wait_hold_ms", ms(at, Some(held_at)));
                }
            }
            "wait_words" => {
                match &event["words"] {
                    Value::Null => run.add(route, "wait_words_none_in_time"),
                    told if told.as_u64() == Some(0) => run.add(route, "wait_words_asking_nothing"),
                    told if told.is_u64() => run.add(route, "wait_words_asking"),
                    _ => {
                        run.miss(kind);
                        continue;
                    }
                }
                run.time(
                    route,
                    "wait_words_ms",
                    ms(at, event["asked_at_us"].as_u64()),
                );
            }
            "hand_control_cancel" => {
                let Some(reason) = event["reason"]
                    .as_str()
                    .filter(|r| STOP_REASONS.contains(r))
                else {
                    continue;
                };
                run.add(route, &format!("stop_{}", reason.trim_end_matches("_stop")));
                let dispatch = event["observation_to_speaker_dispatch_us"].as_u64();
                run.time(
                    route,
                    "stop_dispatch_ms",
                    dispatch.map(|us| us as f64 / 1000.0),
                );
                if reason == "spoken_stop" && stop_said_at.is_some() {
                    run.time(route, "wait_stop_to_cancel_ms", ms(at, stop_said_at.take()));
                }
            }
            "turn_finished" if event["outcome"] == json!("user_interrupted") => {
                match event["provider_audio_seen"].as_bool() {
                    Some(false) => {
                        cuts.push(Cut {
                            turn,
                            at,
                            route,
                            waited: turns_waited.contains(&turn),
                            successor: None,
                        });
                        pending_cut = true;
                    }
                    Some(true) => run.add(route, "cut_while_answer_sounds"),
                    None => run.miss(kind),
                }
            }
            _ => {}
        }
    }
    for cut in cuts {
        let heard = cut
            .successor
            .and_then(|turn| words.get(&turn))
            .copied()
            .unwrap_or(0);
        let name = if heard > 0 {
            "cut_waiting_by_words"
        } else {
            "cut_waiting_by_no_words"
        };
        run.add(cut.route, name);
        if cut.waited {
            run.add(cut.route, &format!("{name}_after_a_wait"));
        }
        let short = cut
            .successor
            .and_then(|turn| blocks.get(&turn))
            .is_some_and(|blocks| *blocks < SHORT_BLOCKS);
        if heard > 0 && short {
            run.add(cut.route, "short_follow_up_kept");
        }
        let ended = cut
            .turn
            .and_then(|turn| endpoint.get(&turn).copied().flatten());
        run.time(cut.route, "awaited_before_cut_ms", ms(cut.at, ended));
    }
    if mode.is_some() || run.mode.is_none() {
        run.mode = Some(mode.unwrap_or("off"));
    }
}

/// The runs in `journal` (within `since`..=`until`, wall clocks from
/// [`wall_bound`]), each with the sessions paired with it.
pub fn count(
    journal: &str,
    sessions: &[Session],
    since: Option<&str>,
    until: Option<&str>,
) -> Vec<Run> {
    let (mut runs, window) = read_journal(journal, since, until);
    if runs.is_empty() {
        runs.push(Run::new(None));
    }
    let mut sessions = sessions.to_vec();
    sessions.sort_by_key(|session| session.1);
    for (folder, start, end) in sessions {
        // Only a session within the journal's window, given to the run
        // started last before it; one begun before the window to its first.
        let Some((first, last)) = window else { break };
        if end < first - PAIR_SLACK_S || start > last + PAIR_SLACK_S {
            continue;
        }
        let owner = runs
            .iter()
            .rposition(|run| run.started.is_some_and(|at| at <= start + PAIR_SLACK_S))
            .unwrap_or(0);
        let run = &mut runs[owner];
        run.sessions += 1;
        match records(&folder) {
            Ok(events) => read_session(run, &events),
            Err(_) => run.unreadable += 1,
        }
    }
    runs
}

fn round(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn summary(values: &[f64]) -> Value {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let n = values.len();
    let median = if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    };
    let p90 = values[(9 * n).div_ceil(10).saturating_sub(1)];
    json!({"n": n, "median": round(median), "p90": round(p90), "max": round(values[n - 1])})
}

/// The runs as JSON: every key and string in it is fixed by this module or a
/// time it formatted, never text from the journal or records.
pub fn report(runs: &[Run], sessions_root: &str) -> Value {
    let runs: Vec<Value> = runs
        .iter()
        .enumerate()
        .map(|(at, run)| {
            let per_route = |f: &dyn Fn(usize) -> Value| {
                json!({"live": f(0), "second": f(1)})
            };
            json!({
                "run": at + 1,
                "started": run.started_wall,
                "route_at_start": run.route,
                "wait_mode": run.mode,
                "sessions": run.sessions,
                "sessions_unreadable": run.unreadable,
                "journal": run.journal,
                "counts": per_route(&|route| json!(run.counts[route])),
                "times_ms": per_route(&|route| {
                    Value::Object(run.times[route].iter().map(|(name, values)| (name.clone(), summary(values))).collect())
                }),
                "unclassified": run.unclassified,
            })
        })
        .collect();
    json!({"sessions_root": sessions_root, "runs": runs})
}

/// [`report`] as tables.
pub fn table(report: &Value) -> String {
    let mut out = Vec::new();
    let empty = serde_json::Map::new();
    let object = |value: &Value| value.as_object().cloned().unwrap_or_else(|| empty.clone());
    for run in report["runs"].as_array().into_iter().flatten() {
        let note = if run["sessions_unreadable"].as_u64() > Some(0) {
            format!(
                "  UNREADABLE {} (run with sudo)",
                run["sessions_unreadable"]
            )
        } else if run["sessions"] == json!(0) {
            "  (no session records: the cuts and routes need them; run with sudo or pass --sessions)".into()
        } else {
            String::new()
        };
        out.push(format!(
            "run {}  started {}  route at start {}  wait {}  sessions {}{note}",
            run["run"],
            run["started"].as_str().unwrap_or("(before the window)"),
            run["route_at_start"].as_str().unwrap_or(""),
            run["wait_mode"].as_str().unwrap_or("(no records)"),
            run["sessions"],
        ));
        let (live, second) = (
            object(&run["counts"]["live"]),
            object(&run["counts"]["second"]),
        );
        out.push(format!("  {:<38} {:>6} {:>6}", "count", "live", "second"));
        let names: BTreeSet<&String> = live.keys().chain(second.keys()).collect();
        for name in names {
            let n = |map: &serde_json::Map<String, Value>| {
                map.get(name).and_then(Value::as_u64).unwrap_or(0)
            };
            out.push(format!("  {name:<38} {:>6} {:>6}", n(&live), n(&second)));
        }
        out.push(format!(
            "  {:<6} {:<31} {:>5} {:>8} {:>8} {:>8}",
            "route", "time (ms)", "n", "median", "p90", "max"
        ));
        for route in ROUTES {
            for (name, s) in object(&run["times_ms"][route]) {
                let f = |key: &str| s[key].as_f64().unwrap_or(0.0);
                out.push(format!(
                    "  {route:<6} {name:<31} {:>5} {:>8.1} {:>8.1} {:>8.1}",
                    s["n"].as_u64().unwrap_or(0),
                    f("median"),
                    f("p90"),
                    f("max")
                ));
            }
        }
        for (name, n) in object(&run["journal"]) {
            out.push(format!(
                "  journal {name:<30} {:>6}",
                n.as_u64().unwrap_or(0)
            ));
        }
        for (name, n) in object(&run["unclassified"]) {
            out.push(format!(
                "  unclassified {name:<25} {:>6}",
                n.as_u64().unwrap_or(0)
            ));
        }
        out.push(String::new());
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_takes_its_numbers_and_ends_where_it_says() {
        let heard = JOURNAL.iter().find(|p| p.name == "second_heard").unwrap();
        assert_eq!(
            matches(
                heard,
                "lamp-voice second: request 6 heard 3 words, the last 210 ms after the line ended"
            ),
            Some(vec![6, 3, 210])
        );
        assert_eq!(
            matches(
                heard,
                "lamp-voice second: request 6 heard 3 words, the last 210 ms after the line ended!"
            ),
            None
        );
        assert_eq!(matches(heard, "lamp-voice second: request x heard"), None);
    }

    #[test]
    fn a_stamp_reads_as_unix_seconds_and_its_wall_clock() {
        assert_eq!(
            parse_time("1970-01-01T07:00:01+0700"),
            Some((1, "1970-01-01 07:00:01".into()))
        );
        assert_eq!(
            parse_time("1970-01-01T00:00:01.250-01:00").map(|t| t.0),
            Some(3601)
        );
        assert_eq!(
            wall_bound("2026-10-19 20:30"),
            Some("2026-10-19 20:30:00".into())
        );
        assert_eq!(wall_bound("20:30"), None);
    }

    #[test]
    fn a_near_word_begins_a_word() {
        assert_eq!(near("lamp-voice: the request failed"), Some("request"));
        assert_eq!(near("lamp-voice: nonstop"), None);
    }
}
