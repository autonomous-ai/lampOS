//! Where a turn with a function call spends its time, measured against the
//! model itself and nothing else (`lamp-voice provider-check CONFIG
//! --tool-turns ROUNDS`, #799).
//!
//! On lamp-4ace on 2026-10-15 a plain answer began 0.87 to 1.45 s after the
//! lamp decided the person had finished, and a turn with a function call 1.8
//! to 2.9 s after, though the tasks part answered each call in 38 to 52 ms.
//! The voice's own handling of a call takes a few milliseconds (it is read
//! on a 2 ms tick and written at once), so the rest is the model's: it
//! makes the call in one generation and speaks in another, after the
//! answer.
//!
//! This asks the model a fixed set of written requests, some that need a
//! function and some that do not, each on a connection of its own, and
//! times each step from the request: the call, our answer sent, the first
//! audio, and the first audio after the answer. Every call is answered at
//! once by a stand-in shaped like the real answer, so nothing reaches the
//! tasks part, the executive or anything on the lamp; nothing is captured
//! or played. A written request is not speech: these times leave out the
//! lamp's end of speech, its playout start and the speaker, and measure
//! only what the model adds.
//!
//! Sessions are opened one after another with a pause between them, longer
//! after each that could not be opened: on lamp-4ace on 2026-10-15 at 16:20
//! the company relay refused new connections that came too fast on one key
//! (`ConnectTimeout` after three sessions) while the lamp's own stayed up.
//! A session that fails is a line saying where, and the rest go on.
use serde_json::{Value, json};
use std::time::Duration;

/// Between one session and the next.
pub const PAUSE: Duration = Duration::from_secs(3);
/// The longest pause, after failures in a row.
pub const MOST_PAUSE: Duration = Duration::from_secs(30);

/// The pause before the next session: [`PAUSE`] after one that was opened,
/// twice the last after one that failed, at most [`MOST_PAUSE`].
pub fn next_pause(last: Duration, failed: bool) -> Duration {
    if failed {
        (last * 2).clamp(PAUSE, MOST_PAUSE)
    } else {
        PAUSE
    }
}

/// The line for a request whose session failed at `stage` (`connect`,
/// `write`), with the error's own words, which name no person.
pub fn failed(scripted: &Scripted, round: usize, stage: &str, error: &str) -> Value {
    json!({"label": scripted.label, "round": round, "failed": stage, "error": error})
}

/// One scripted request: what it is called, the function it should make
/// the model call, if any, and what is written to it. With more than one
/// line, the earlier ones are said first in the same session and only the
/// last is timed.
#[derive(Clone, Copy, Debug)]
pub struct Scripted {
    pub label: &'static str,
    pub expects: Option<&'static str>,
    pub lines: &'static [&'static str],
}

const HAND_OVER: &str =
    "Can you ask Claude Code on my office computer to write a short poem about a desk lamp?";

/// The requests, plain ones first. Words a person says at the lamp, as
/// written; nothing in them is private.
pub const SCRIPT: &[Scripted] = &[
    Scripted {
        label: "plain_fact",
        expects: None,
        lines: &["What's the capital of Australia?"],
    },
    Scripted {
        label: "plain_chat",
        expects: None,
        lines: &["Tell me one fun thing about octopuses."],
    },
    Scripted {
        label: "light",
        expects: Some("light_colour"),
        lines: &["Turn your light blue, please."],
    },
    Scripted {
        label: "hand_over",
        expects: Some(crate::agent_tasks::START),
        lines: &[HAND_OVER],
    },
    Scripted {
        label: "yes",
        expects: Some(crate::agent_tasks::CONFIRM),
        lines: &[HAND_OVER, "Yes, go ahead."],
    },
    Scripted {
        label: "check",
        expects: Some(crate::agent_tasks::CHECK),
        lines: &["How is the work you gave Claude Code going?"],
    },
];

const STORY_ASKED: &str = "Hey Lamp, can you ask Claude Code to write a short story about a lamp who learns to sing, in five chapters, and to read each chapter over before starting the next one?";

/// The hand-overs of #854, as written requests, for `provider-check CONFIG
/// --hand-overs ROUNDS`, run once with `LAMP_TASKS_ASKED=on` and once
/// without: how often the model hands each over to Claude Code, and how
/// often it does the work itself. On lamp-4ace on 2026-10-15 the first two
/// were told as stories by the lamp, and a story already under way was
/// carried on when asked again (`mid_story`, `after_own_story`). The two
/// named poems are as the service heard them on lamp-4ace on 2026-10-16,
/// when two heads of #903 lost them. Those whose label begins `own_` are
/// asked of the lamp itself, which should tell them: on 2026-10-16 a long
/// one was offered to Claude Code instead, and a session that began
/// offering went on offering every story (`own_story_long`,
/// `own_story_after_hand_over`, `own_stories_in_a_row`; only the last line
/// of each is recorded, so the last shows the third story after two in one
/// session, not all three). The words are the ones played there; the model
/// reads them here, where on the lamp it heard them.
pub const HAND_OVERS: &[Scripted] = &[
    Scripted {
        label: "story_asked",
        expects: Some(crate::agent_tasks::START),
        lines: &[STORY_ASKED],
    },
    Scripted {
        label: "story_comma",
        expects: Some(crate::agent_tasks::START),
        lines: &["Hey Lamp, ask Claude Code, write a short story about a lamp who learns to sing."],
    },
    Scripted {
        label: "story_imperative",
        expects: Some(crate::agent_tasks::START),
        lines: &[
            "Have Claude Code write a short story about a lamp who learns to sing, in five chapters.",
        ],
    },
    Scripted {
        label: "poem_get",
        expects: Some(crate::agent_tasks::START),
        lines: &["Get Claude Code to write a poem about the rain."],
    },
    Scripted {
        label: "mid_story",
        expects: Some(crate::agent_tasks::START),
        lines: &[
            "Tell me a short story about a fox, in three chapters. Just chapter one for now.",
            "Now ask Claude Code to write chapters two and three.",
        ],
    },
    // Asked of Claude Code after the lamp told the story itself, as on
    // lamp-4ace, where it went on with the story. (Asked again after a
    // hand-over, as this first was, it is a yes, and `confirm_agent_task`
    // is right: on lamp-4ace at 21:11 to 21:27 it counted 0 of 9 for that.)
    Scripted {
        label: "after_own_story",
        expects: Some(crate::agent_tasks::START),
        lines: &[
            "Tell me a very short story about a lamp who learns to sing.",
            "Actually, ask Claude Code to write that story instead, a longer one.",
        ],
    },
    Scripted {
        label: "named_poem_sea",
        expects: Some(crate::agent_tasks::START),
        lines: &["Hey Lamp, can you ask Claude Co to write a short poem about the sea?"],
    },
    Scripted {
        label: "named_poem_rain",
        expects: Some(crate::agent_tasks::START),
        lines: &["Hey Lamp, ask Claude to write a haiku poem about the rain."],
    },
    // Work on the office computer with nobody named: handed over as well
    // (the reviewer on #903).
    Scripted {
        label: "computer_unnamed",
        expects: Some(crate::agent_tasks::START),
        lines: &[
            "Create notes.txt on my office computer with this shopping list: milk, eggs and bread.",
        ],
    },
    Scripted {
        label: "own_story",
        expects: None,
        lines: &["Tell me a very short story about a lamp who learns to sing."],
    },
    Scripted {
        label: "own_story_long",
        expects: None,
        lines: &[OWN_LONG],
    },
    Scripted {
        label: "own_story_after_hand_over",
        expects: None,
        lines: &[HAND_OVER, "Yes, go ahead.", OWN_LONG],
    },
    Scripted {
        label: "own_stories_in_a_row",
        expects: None,
        lines: &[
            "Hey Lamp, tell me a story about a clever fox, at least five sentences long.",
            "Tell me another one, about a wise old owl, at least eight sentences long.",
            "And one more, about a brave little mouse, at least eight sentences long.",
        ],
    },
];

/// The request that was offered to Claude Code on lamp-4ace on 2026-10-16
/// at 02:05, 3 times of 3 (#854).
const OWN_LONG: &str =
    "Hey Lamp, tell me a story about a clever fox, at least eight sentences long.";

/// The answer the voice would give to `name`, made up, of the same shape
/// as the real one and with the same `next` (with the lead-in or without,
/// [`crate::agent_tasks::LEAD_IN_FLAG`]), so that what the model says after
/// it costs what it would on the lamp.
pub fn stand_in(name: &str, args: &Value, lead_in: bool) -> Value {
    match name {
        crate::tools::NOT_FOR_ME => crate::tools::declined(),
        crate::agent_tasks::START => json!({
            "prepared": true,
            "task": args.get("task").cloned().unwrap_or(Value::Null),
            "where": "Claude Code on the office computer",
            "next": crate::agent_tasks::prepared_next(lead_in, false, "Claude Code")
        }),
        crate::agent_tasks::CONFIRM => json!({
            "handing_over": true,
            "who": "Claude Code",
            "next": crate::agent_tasks::handing_over_next(lead_in, "Claude Code")
        }),
        crate::agent_tasks::CHECK => json!({
            "who": "Claude Code",
            "task": "Write a short poem about a desk lamp",
            "state": "working",
            "next": "Tell them in a few words that Claude Code is still working on it and that you will tell them yourself when it is done. Do not check again unless they ask again."
        }),
        _ => json!({"done": true}),
    }
}

/// What came back for one timed request, each time in microseconds from
/// the request being written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Timeline {
    pub called: Option<String>,
    pub calls: usize,
    pub call_us: Option<u64>,
    pub answered_us: Option<u64>,
    pub first_audio_us: Option<u64>,
    /// The first audio after the first answer was sent.
    pub audio_after_answer_us: Option<u64>,
    /// Audio before the first call: a lead-in said before calling.
    pub samples_before_call: usize,
    pub samples: usize,
    pub said: String,
    pub ended: Option<(&'static str, u64)>,
}

impl Timeline {
    pub fn call(&mut self, name: &str, at_us: u64) {
        self.calls += 1;
        if self.called.is_none() {
            self.called = Some(name.to_owned());
            self.call_us = Some(at_us);
        }
    }

    pub fn answered(&mut self, at_us: u64) {
        self.answered_us.get_or_insert(at_us);
    }

    pub fn audio(&mut self, samples: usize, at_us: u64) {
        self.first_audio_us.get_or_insert(at_us);
        if self.call_us.is_none() {
            self.samples_before_call += samples;
        }
        if self.answered_us.is_some() {
            self.audio_after_answer_us.get_or_insert(at_us);
        }
        self.samples += samples;
    }

    pub fn said(&mut self, text: &str) {
        self.said.push_str(text);
    }

    pub fn ended(&mut self, how: &'static str, at_us: u64) {
        self.ended.get_or_insert((how, at_us));
    }

    /// One line of the result, in milliseconds.
    /// A request that did not end with the model's turn complete (it timed
    /// out, the connection ended, or the service left it unanswered) is
    /// marked failed with how it ended, and its times are kept for reading
    /// but left out of every median (#810 review, 17:23).
    pub fn result(&self, scripted: &Scripted, round: usize) -> Value {
        let mut row = self.times(scripted, round);
        match self.ended {
            Some(("turn_complete", _)) => {}
            Some((how, _)) => row["failed"] = json!(how),
            None => row["failed"] = json!("no_end"),
        }
        row
    }

    fn times(&self, scripted: &Scripted, round: usize) -> Value {
        let ms = |us: Option<u64>| us.map(|us| us as f64 / 1_000.0);
        let after_answer = self
            .audio_after_answer_us
            .zip(self.answered_us)
            .map(|(audio, answered)| audio.saturating_sub(answered));
        let answer_took = self
            .answered_us
            .zip(self.call_us)
            .map(|(answered, call)| answered.saturating_sub(call));
        json!({
            "label": scripted.label,
            "round": round,
            "expected": scripted.expects,
            "called": self.called,
            "calls": self.calls,
            "call_ms": ms(self.call_us),
            "answer_sent_after_call_ms": ms(answer_took),
            "first_audio_ms": ms(self.first_audio_us),
            "audio_after_answer_ms": ms(after_answer),
            "lead_in_ms": audio_ms(self.samples_before_call),
            "audio_ms": audio_ms(self.samples),
            "ended": self.ended.map(|(how, _)| how),
            "ended_ms": ms(self.ended.map(|(_, at)| at)),
            "said": self.said,
        })
    }
}

/// Milliseconds of the model's audio, whole.
fn audio_ms(samples: usize) -> f64 {
    (samples as f64 * 1_000.0 / f64::from(lamp_gemini::OUTPUT_RATE)).round()
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len() % 2 == 1 {
        values[middle]
    } else {
        (values[middle - 1] + values[middle]) / 2.0
    })
}

/// Medians over the rounds, for each request and for plain and function
/// turns, of the times in `results` (as [`Timeline::result`] makes them).
/// A function turn counts only when the function expected was called.
pub fn summary(all: &[Value]) -> Value {
    summary_of(all, SCRIPT)
}

/// The same for the requests of `script` ([`SCRIPT`] or [`HAND_OVERS`]).
pub fn summary_of(all: &[Value], script: &[Scripted]) -> Value {
    let results: Vec<Value> = all
        .iter()
        .filter(|row| row.get("failed").is_none())
        .cloned()
        .collect();
    let mut failures = serde_json::Map::new();
    for row in all.iter().filter_map(|row| row["failed"].as_str()) {
        let count = failures.entry(row.to_owned()).or_insert(json!(0));
        *count = json!(count.as_u64().unwrap_or(0) + 1);
    }
    let of = |rows: &[&Value], field: &str| {
        median(rows.iter().filter_map(|row| row[field].as_f64()).collect())
    };
    let fields = [
        "first_audio_ms",
        "call_ms",
        "answer_sent_after_call_ms",
        "audio_after_answer_ms",
        "lead_in_ms",
    ];
    let medians = |rows: &[&Value]| {
        let mut out = json!({"n": rows.len()});
        for field in fields {
            out[field] = json!(of(rows, field));
        }
        out
    };
    let mut each = serde_json::Map::new();
    for scripted in script {
        let rows: Vec<&Value> = results
            .iter()
            .filter(|row| row["label"] == scripted.label)
            .collect();
        let as_expected = rows
            .iter()
            .filter(|row| row["called"].as_str() == scripted.expects)
            .count();
        // What the person heard: the work offered to, or said to go to,
        // Claude Code (#854: "Claude Code can write that story").
        let offered = rows
            .iter()
            .filter(|row| {
                row["said"]
                    .as_str()
                    .is_some_and(|said| said.contains("Claude Code"))
            })
            .count();
        // Of those that called the function expected, how many spoke
        // before the call (a lead-in, #799): on lamp-4ace on 2026-10-16 a
        // lead-in asked for before the call came without the call at all.
        let lead_ins = rows
            .iter()
            .filter(|row| {
                scripted.expects.is_some()
                    && row["called"].as_str() == scripted.expects
                    && row["lead_in_ms"].as_f64().is_some_and(|ms| ms > 0.0)
            })
            .count();
        let mut out = medians(&rows);
        out["as_expected"] = json!(as_expected);
        out["lead_ins"] = json!(lead_ins);
        out["offered"] = json!(offered);
        each.insert(scripted.label.to_owned(), out);
    }
    let plain: Vec<&Value> = results
        .iter()
        .filter(|row| row["expected"].is_null() && row["called"].is_null())
        .collect();
    let function: Vec<&Value> = results
        .iter()
        .filter(|row| !row["expected"].is_null() && row["called"] == row["expected"])
        .collect();
    json!({
        "status": "tool_turns_summary",
        "asked": all.len(),
        "failed": all.len() - results.len(),
        "failed_at": failures,
        "each": each,
        "plain": medians(&plain),
        "function": medians(&function),
        "boundary": "from the written request to the model's events on this host; leaves out the lamp's end of speech, playout start and speaker",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_function_turn_is_timed_at_each_step_and_a_lead_in_is_counted() {
        let mut timeline = Timeline::default();
        // A lead-in of 0.2 s, then the call, our answer, and the answer's audio.
        timeline.audio(4_800, 900_000);
        timeline.call(crate::agent_tasks::CONFIRM, 1_100_000);
        timeline.answered(1_104_000);
        timeline.audio(2_400, 2_050_000);
        timeline.audio(2_400, 2_150_000);
        timeline.said("Okay, handing it over. ");
        timeline.ended("turn_complete", 3_000_000);
        let row = timeline.result(&SCRIPT[4], 2);
        assert_eq!(row["label"], "yes");
        assert_eq!(row["called"], "confirm_agent_task");
        assert_eq!(row["call_ms"], 1_100.0);
        assert_eq!(row["answer_sent_after_call_ms"], 4.0);
        assert_eq!(row["first_audio_ms"], 900.0);
        assert_eq!(row["audio_after_answer_ms"], 946.0);
        assert_eq!(row["lead_in_ms"], 200.0);
        assert_eq!(row["audio_ms"], 400.0);
        assert_eq!(row["ended"], "turn_complete");
        // A plain one: audio only.
        let mut plain = Timeline::default();
        plain.audio(2_400, 700_000);
        let row = plain.result(&SCRIPT[0], 0);
        assert_eq!(row["first_audio_ms"], 700.0);
        assert!(row["call_ms"].is_null() && row["audio_after_answer_ms"].is_null());
        // A second call is counted, not timed again.
        timeline.call("light_colour", 2_500_000);
        assert_eq!(
            (timeline.calls, timeline.called.as_deref()),
            (2, Some("confirm_agent_task"))
        );
    }

    #[test]
    fn the_summary_takes_medians_and_counts_only_turns_that_called_what_was_expected() {
        let row = |label: &str, expected: Option<&str>, called: Option<&str>, first: f64| {
            json!({"label":label,"expected":expected,"called":called,"first_audio_ms":first,
                "call_ms":called.map(|_| first - 500.0),"audio_after_answer_ms":called.map(|_| 900.0)})
        };
        let results = vec![
            row("plain_fact", None, None, 700.0),
            row("plain_fact", None, None, 900.0),
            row("plain_chat", None, None, 800.0),
            row("light", Some("light_colour"), Some("light_colour"), 1_800.0),
            row("light", Some("light_colour"), None, 600.0),
            row(
                "yes",
                Some("confirm_agent_task"),
                Some("confirm_agent_task"),
                2_000.0,
            ),
        ];
        let summary = summary(&results);
        assert_eq!(summary["plain"]["n"], 3);
        assert_eq!(summary["plain"]["first_audio_ms"], 800.0);
        assert_eq!(summary["function"]["n"], 2);
        assert_eq!(summary["function"]["first_audio_ms"], 1_900.0);
        assert_eq!(summary["function"]["audio_after_answer_ms"], 900.0);
        assert_eq!(summary["each"]["light"]["n"], 2);
        assert_eq!(summary["each"]["light"]["as_expected"], 1);
        assert_eq!(summary["each"]["check"]["n"], 0);
        assert!(summary["each"]["check"]["first_audio_ms"].is_null());
    }

    /// The reviewer on #810 (17:23): a silent session that timed out after
    /// 14 s, or one whose connection ended, was counted as answered. Only
    /// a turn the model completed is.
    #[test]
    fn a_session_that_times_out_or_whose_connection_ends_is_counted_as_failed() {
        for how in ["timeout", "connection_ended", "unanswered"] {
            let mut silent = Timeline::default();
            silent.ended(how, 14_000_000);
            let row = silent.result(&SCRIPT[0], 0);
            assert_eq!(row["failed"], how);
            let summary = summary(&[row]);
            assert_eq!(
                (summary["failed"].clone(), summary["plain"]["n"].clone()),
                (json!(1), json!(0))
            );
            assert_eq!(summary["failed_at"][how], 1);
            assert_eq!(summary["each"]["plain_fact"]["as_expected"], 0);
        }
        let mut never = Timeline::default();
        never.audio(2_400, 700_000);
        assert_eq!(never.result(&SCRIPT[0], 0)["failed"], "no_end");
        let mut answered = Timeline::default();
        answered.audio(2_400, 700_000);
        answered.ended("turn_complete", 2_000_000);
        assert!(answered.result(&SCRIPT[0], 0).get("failed").is_none());
    }

    /// After the relay refused new connections at 16:20: a failed session
    /// is a line of its own and counted, never a median, and the pause
    /// grows after each failure in a row and is short again after a success.
    #[test]
    fn a_session_that_fails_is_counted_apart_and_the_pause_after_it_grows() {
        let results = vec![
            json!({"label":"plain_fact","expected":null,"called":null,"first_audio_ms":800.0}),
            failed(&SCRIPT[0], 0, "connect", "Gemini Live ConnectTimeout"),
            failed(&SCRIPT[2], 0, "connect", "Gemini Live ConnectTimeout"),
            failed(&SCRIPT[3], 1, "write", "closed"),
        ];
        let summary = summary(&results);
        assert_eq!(
            (summary["asked"].clone(), summary["failed"].clone()),
            (json!(4), json!(3))
        );
        assert_eq!(summary["failed_at"], json!({"connect": 2, "write": 1}));
        assert_eq!(summary["plain"]["n"], 1);
        assert_eq!(summary["plain"]["first_audio_ms"], 800.0);
        assert_eq!(summary["each"]["plain_fact"]["n"], 1);
        assert_eq!(summary["each"]["light"]["n"], 0);
        assert_eq!(failed(&SCRIPT[0], 2, "connect", "x")["label"], "plain_fact");
        let mut pause = PAUSE;
        for expected in [6, 12, 24, 30, 30] {
            pause = next_pause(pause, true);
            assert_eq!(pause, Duration::from_secs(expected));
        }
        assert_eq!(next_pause(pause, false), PAUSE);
    }

    #[test]
    fn every_stand_in_answer_has_the_shape_of_the_real_one_and_reaches_nothing() {
        let start = stand_in(
            crate::agent_tasks::START,
            &json!({"task":"Write a poem"}),
            false,
        );
        assert_eq!(
            (start["prepared"].clone(), start["task"].clone()),
            (json!(true), json!("Write a poem"))
        );
        assert!(
            start["next"]
                .as_str()
                .unwrap()
                .starts_with("Say the task back")
        );
        assert_eq!(
            stand_in(crate::agent_tasks::CONFIRM, &json!({}), false)["handing_over"],
            true
        );
        assert_eq!(
            stand_in(crate::agent_tasks::CHECK, &json!({}), false)["state"],
            "working"
        );
        assert_eq!(
            stand_in(crate::tools::NOT_FOR_ME, &json!({}), false),
            crate::tools::declined()
        );
        assert_eq!(
            stand_in("light_colour", &json!({}), false),
            json!({"done": true})
        );
        // With the lead-in, the `next` the voice itself gives.
        assert_eq!(
            stand_in(crate::agent_tasks::CONFIRM, &json!({}), true)["next"],
            crate::agent_tasks::handing_over_next(true, "Claude Code")
        );
        assert!(
            stand_in(crate::agent_tasks::START, &json!({}), true)["next"]
                .as_str()
                .unwrap()
                .starts_with("If you already said the task back, naming each name it has")
        );
        // Every function the script expects is one the voice declares.
        let place = crate::agent_tasks::Place {
            directory: "/nonexistent".into(),
            target: crate::agent_tasks::DEFAULT_TARGET.into(),
            who: false,
            lead_in: false,
            asked: false,
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: false,
            make_skill: false,
            named_only: false,
            correct: false,
            questions: false,
            sign_in: false,
        };
        let declared: Vec<String> = crate::agent_tasks::tools(&place)
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        for scripted in SCRIPT {
            if let Some(name) = scripted.expects
                && name != "light_colour"
            {
                assert!(declared.iter().any(|tool| tool == name), "{name}");
            }
        }
        assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(vec![]), None);
    }

    /// #854's requests: each has a label of its own, all but the lamp's own
    /// story are hand-overs, and each is counted by whether the model handed
    /// it over or did the work itself.
    #[test]
    fn the_hand_overs_are_counted_by_whether_each_was_handed_over() {
        let mut labels: Vec<&str> = HAND_OVERS.iter().map(|scripted| scripted.label).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), HAND_OVERS.len());
        for scripted in HAND_OVERS {
            let lamps_own = scripted.label.starts_with("own_");
            assert_eq!(
                scripted.expects,
                (!lamps_own).then_some(crate::agent_tasks::START),
                "{}",
                scripted.label
            );
            let last = scripted.lines.last().unwrap();
            assert_eq!(
                last.contains("Claude") || last.contains("office computer"),
                !lamps_own,
                "{}",
                scripted.label
            );
        }
        let row = |scripted: &Scripted, called: Option<&str>, said: &str| {
            let mut timeline = Timeline::default();
            // A read-back said before its call, as the lead-in asks (#799).
            if said.starts_with("Claude Code can write that story about a lamp") {
                timeline.audio(9_600, 700_000);
            }
            if let Some(name) = called {
                timeline.call(name, 900_000);
                timeline.answered(905_000);
            }
            timeline.audio(4_800, 1_800_000);
            timeline.said(said);
            timeline.ended("turn_complete", 3_000_000);
            timeline.result(scripted, 0)
        };
        let story = &HAND_OVERS[0];
        let own = HAND_OVERS
            .iter()
            .find(|scripted| scripted.label == "own_story")
            .unwrap();
        let long = HAND_OVERS
            .iter()
            .find(|scripted| scripted.label == "own_story_long")
            .unwrap();
        let rows = [
            row(
                story,
                Some(crate::agent_tasks::START),
                "Here is chapter one. ",
            ),
            row(story, None, "Here is chapter one. "),
            row(
                story,
                Some(crate::agent_tasks::START),
                "Claude Code can write that story about a lamp who learns to sing. Shall I start it?",
            ),
            row(own, None, "Here is chapter one. "),
            // The regression of 2026-10-16, as the person heard it.
            row(
                long,
                None,
                "Claude Code can write that story about the clever fox. Should I start it?",
            ),
            row(
                long,
                None,
                "Once upon a time, a clever fox lived by the river. ",
            ),
        ];
        let summary = summary_of(&rows, HAND_OVERS);
        assert_eq!(summary["each"]["story_asked"]["n"], 3);
        assert_eq!(summary["each"]["story_asked"]["as_expected"], 2);
        // One of the two that called said the read-back first, 0.4 s of it.
        assert_eq!(summary["each"]["story_asked"]["lead_ins"], 1);
        assert_eq!(rows[2]["lead_in_ms"], 400.0);
        assert_eq!(summary["each"]["own_story"]["lead_ins"], 0);
        assert_eq!(summary["each"]["own_story"]["as_expected"], 1);
        assert_eq!(
            (
                &summary["each"]["own_story_long"]["n"],
                &summary["each"]["own_story_long"]["offered"]
            ),
            (&json!(2), &json!(1))
        );
        assert_eq!(summary["each"]["own_story"]["offered"], 0);
        assert_eq!(summary["each"]["mid_story"]["n"], 0);
        // The timing script is untouched.
        assert!(
            summary_of(&rows, SCRIPT)["each"]
                .get("story_asked")
                .is_none()
        );
    }
}
