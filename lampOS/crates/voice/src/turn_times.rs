//! Where each of the lamp's turns spent its time, from what the voice
//! already writes down (`lamp-voice turn-times SESSIONS [FIRST_OUTPUT]`,
//! #799): for every request, from the moment the lamp decided the person had
//! finished, when the model's first output came and what it was, when a
//! function was called and our answer sent, when the model's first audio
//! came, and when the first sound was handed to the speaker. Then medians
//! for turns with a function call and for plain ones.
//!
//! It reads the session records (`<session>/metadata/events-*.jsonl`) and,
//! if given, the provider worker's first-output files
//! (`first-output-*.jsonl`, `LAMP_FIRST_OUTPUT`). Every time in both is the
//! lamp's monotonic clock in microseconds. A request number is the
//! coordinator's turn; turns are counted afresh each time the voice starts,
//! so a session folder is one run, and a first-output line is matched to a
//! turn of the same number whose end of speech was decided at most a minute
//! before it. What it prints is numbers and function names: no words that
//! were heard or said.
//!
//! Every request admitted is a turn, also one cut before its end of speech
//! (`cut`). Each also says how long the lamp sat idle before it (from the
//! end of the request before, or from the run's `listening_ready` for its
//! first), and the longest capture gap (`capture_gap`) it held. The
//! summary keeps a run's first request and a turn after a long idle apart
//! from the rest: on lamp-4ace on 18 Oct, two runs' first request after
//! about six idle minutes stalled the microphone 135 and 172 ms and lost
//! the second route (#1608), which only a journal line said.
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Calls after which the model says nothing more: they tell whom it heard,
/// not something to do, and do not make a turn one with a function call.
const SIGNALS: &[&str] = &[crate::tools::NOT_FOR_ME, crate::attention::HEARD_MY_NAME];
/// A first-output line belongs to a turn whose end of speech was decided at
/// most this long before it.
const MATCH_WITHIN_US: u64 = 60_000_000;
/// A turn after at least this long with nothing going on is counted apart
/// from the rest (#1608: the stalls came after about six idle minutes).
pub const LONG_IDLE_US: u64 = 300_000_000;

/// Every JSON line of the files under `root`, at any depth, whose names
/// begin with `prefix` and end `.jsonl`, each with the folder it was in.
/// A line that is not JSON is passed over.
pub fn lines_under(root: &Path, prefix: &str) -> std::io::Result<Vec<(PathBuf, Value)>> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                folders.push(path);
            } else if kind.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(prefix) && name.ends_with(".jsonl"))
            {
                let text = std::fs::read_to_string(&path)?;
                found.extend(
                    text.lines()
                        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                        .map(|value| (folder.clone(), value)),
                );
            }
        }
    }
    Ok(found)
}

/// One function call and our answer, as the first-output file says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub name: String,
    pub called_us: u64,
    pub sent_us: u64,
}

/// One request, its times on the lamp's monotonic clock.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Turn {
    pub run: PathBuf,
    pub turn: u64,
    pub endpoint_us: u64,
    pub first_output: Option<(String, u64)>,
    pub calls: Vec<Call>,
    pub first_audio_us: Option<u64>,
    pub first_sound_us: Option<u64>,
    pub outcome: Option<String>,
    /// When its input was admitted (`input_admitted`), and when it ended
    /// (`turn_finished`).
    pub admitted_us: Option<u64>,
    pub finished_us: Option<u64>,
    /// A request admitted but ended before its end of speech was decided
    /// (no `local_endpoint`): it has no times after an end of speech, and
    /// `endpoint_us` is its admission.
    pub cut: bool,
    /// The run's first request admitted.
    pub first_of_run: bool,
    /// How long nothing went on before it: from the end of the request
    /// before (0 if that had not ended yet), or from the run's
    /// `listening_ready` for its first.
    pub idle_us: Option<u64>,
    /// The longest capture gap that ended while it was the latest request
    /// admitted and not yet finished.
    pub stalled_us: Option<u64>,
}

impl Turn {
    /// A turn whose model called something to be done, not only a signal.
    pub fn with_function(&self) -> bool {
        self.calls
            .iter()
            .any(|call| !SIGNALS.contains(&call.name.as_str()))
            || self
                .first_output
                .as_ref()
                .is_some_and(|(trigger, _)| trigger == "tool_call")
    }

    fn after(&self, at_us: Option<u64>) -> Option<f64> {
        at_us
            .filter(|at| !self.cut && *at >= self.endpoint_us)
            .map(|at| (at - self.endpoint_us) as f64 / 1_000.0)
    }

    /// The row printed for it, every time in milliseconds after the end of
    /// speech was decided.
    pub fn row(&self) -> Value {
        let call = self
            .calls
            .iter()
            .find(|call| !SIGNALS.contains(&call.name.as_str()));
        // Sound before the call: a lead-in, said before the function was
        // asked for. The first audio is then not what came after our
        // answer, and nothing in the records says when that came.
        let lead_in = call
            .zip(self.first_audio_us)
            .is_some_and(|(call, audio)| audio < call.called_us);
        // Sound before our answer was sent, whether before the call or
        // between the call and the answer: the first audio is then not what
        // came after the answer either (#810 review, 18:33).
        let audio_before_answer = call
            .zip(self.first_audio_us)
            .is_some_and(|(call, audio)| audio < call.sent_us);
        json!({
            "turn": self.turn,
            "kind": if self.with_function() { "function" } else { "plain" },
            "functions": self.calls.iter().map(|call| call.name.as_str()).collect::<Vec<_>>(),
            "first_output": self.first_output.as_ref().map(|(trigger, _)| trigger.as_str()),
            "first_output_ms": self.after(self.first_output.as_ref().map(|(_, at)| *at)),
            "call_ms": self.after(call.map(|call| call.called_us)),
            "answer_sent_ms": self.after(call.map(|call| call.sent_us)),
            "first_audio_ms": self.after(self.first_audio_us),
            "lead_in": lead_in,
            "audio_before_answer": audio_before_answer,
            "first_sound_ms": self.after(self.first_sound_us),
            "outcome": self.outcome,
            "cut": self.cut,
            "first_of_run": self.first_of_run,
            "idle_ms": self.idle_us.map(|us| us as f64 / 1_000.0),
            "stalled_ms": self.stalled_us.map(|us| us as f64 / 1_000.0),
        })
    }
}

/// The turns in the session records `events`, with the first-output lines
/// `first` matched to them.
pub fn turns(events: &[(PathBuf, Value)], first: &[(PathBuf, Value)]) -> Vec<Turn> {
    let mut by_turn: BTreeMap<(PathBuf, u64), Turn> = BTreeMap::new();
    let number = |value: &Value, field: &str| value[field].as_u64();
    // Two passes, so that the order the files are read in does not matter:
    // the service writes its records into numbered files in turn, and a
    // later file can be read before an earlier one (#810 review, 17:23).
    // The turns from their ends of speech first, then what came of them,
    // the earliest of each.
    for (folder, event) in events {
        if event["kind"] == "local_endpoint"
            && let (Some(turn), Some(at)) = (number(event, "turn"), number(event, "at_us"))
        {
            let entry = by_turn
                .entry((folder.clone(), turn))
                .or_insert_with(|| Turn {
                    run: folder.clone(),
                    turn,
                    endpoint_us: at,
                    ..Turn::default()
                });
            entry.endpoint_us = entry.endpoint_us.min(at);
        }
    }
    // A request admitted and ended before any end of speech (#1615 review,
    // 21:56): still the run's first if it came first, and its stall counts.
    for (folder, event) in events {
        if event["kind"] == "input_admitted"
            && let (Some(turn), Some(at)) = (number(event, "turn"), number(event, "at_us"))
        {
            let entry = by_turn
                .entry((folder.clone(), turn))
                .or_insert_with(|| Turn {
                    run: folder.clone(),
                    turn,
                    endpoint_us: at,
                    cut: true,
                    ..Turn::default()
                });
            if entry.cut {
                entry.endpoint_us = entry.endpoint_us.min(at);
            }
        }
    }
    let earliest = |kept: &mut Option<u64>, at: u64| {
        *kept = Some(kept.map_or(at, |kept| kept.min(at)));
    };
    for (folder, event) in events {
        let (Some(turn), Some(at)) = (number(event, "turn"), number(event, "at_us")) else {
            continue;
        };
        let Some(entry) = by_turn.get_mut(&(folder.clone(), turn)) else {
            continue;
        };
        match event["kind"].as_str() {
            Some("provider_first_audio") => earliest(&mut entry.first_audio_us, at),
            Some("speaker_first_write") => earliest(&mut entry.first_sound_us, at),
            Some("turn_finished") => {
                entry.outcome = event["outcome"].as_str().map(str::to_owned);
                earliest(&mut entry.finished_us, at);
            }
            Some("input_admitted") => earliest(&mut entry.admitted_us, at),
            _ => {}
        }
    }
    let mut turns: Vec<Turn> = by_turn.into_values().collect();
    idle_and_gaps(events, &mut turns);
    // Each first-output line goes to the turn of its number decided most
    // recently before it, within a minute.
    for (_, line) in first {
        let Some(request) = number(line, "request") else {
            continue;
        };
        let (at, call) = match line["event"].as_str() {
            Some("voice_first_output") => (number(line, "sampled_start_us"), None),
            Some("voice_tool_call") => {
                let (Some(called_us), Some(sent_us), Some(name)) = (
                    number(line, "called_us"),
                    number(line, "sent_us"),
                    line["name"].as_str(),
                ) else {
                    continue;
                };
                (
                    Some(called_us),
                    Some(Call {
                        name: name.to_owned(),
                        called_us,
                        sent_us,
                    }),
                )
            }
            _ => continue,
        };
        let Some(at) = at else { continue };
        let Some(turn) = turns
            .iter_mut()
            .filter(|turn| {
                !turn.cut
                    && turn.turn == request
                    && turn.endpoint_us <= at
                    && at - turn.endpoint_us <= MATCH_WITHIN_US
            })
            .max_by_key(|turn| turn.endpoint_us)
        else {
            continue;
        };
        match call {
            Some(call) => turn.calls.push(call),
            None => {
                if let Some(trigger) = line["trigger"].as_str() {
                    turn.first_output.get_or_insert((trigger.to_owned(), at));
                }
            }
        }
    }
    turns
}

/// Each turn's idle time before it, whether it is its run's first, and its
/// longest capture gap (#1608). Turns are in run and turn order already
/// (the map's key).
fn idle_and_gaps(events: &[(PathBuf, Value)], turns: &mut [Turn]) {
    let mut ready: BTreeMap<&PathBuf, u64> = BTreeMap::new();
    let mut gaps: BTreeMap<&PathBuf, Vec<(u64, u64)>> = BTreeMap::new();
    for (folder, event) in events {
        let Some(at) = event["at_us"].as_u64() else {
            continue;
        };
        match event["kind"].as_str() {
            Some("listening_ready") => {
                let kept = ready.entry(folder).or_insert(at);
                *kept = (*kept).min(at);
            }
            Some("capture_gap") => {
                if let Some(stalled) = event["stalled_us"].as_u64() {
                    gaps.entry(folder).or_default().push((at, stalled));
                }
            }
            _ => {}
        }
    }
    // Runs are contiguous and in turn order (the map's key).
    let mut from = 0;
    while from < turns.len() {
        let to = from
            + turns[from..]
                .iter()
                .take_while(|turn| turn.run == turns[from].run)
                .count();
        let run = &turns[from].run;
        let (ready, gaps) = (
            ready.get(run).copied(),
            gaps.get(run).map_or(&[][..], Vec::as_slice),
        );
        idle_and_gaps_of_run(&mut turns[from..to], ready, gaps);
        from = to;
    }
}

/// One run's turns: the first is its first request; each idle time runs
/// from the end of the one before (finished, else first sound, else end of
/// speech). A gap is stamped when it ended and goes to one turn only, the
/// latest admitted whose window holds it; a window runs from admission to
/// `turn_finished`, or to the next admission if it has none.
fn idle_and_gaps_of_run(turns: &mut [Turn], ready: Option<u64>, gaps: &[(u64, u64)]) {
    let begun: Vec<u64> = turns
        .iter()
        .map(|turn| turn.admitted_us.unwrap_or(turn.endpoint_us))
        .collect();
    let window_end: Vec<u64> = (0..turns.len())
        .map(|i| {
            turns[i]
                .finished_us
                .or_else(|| begun.get(i + 1).copied())
                .unwrap_or(u64::MAX)
                .max(begun[i])
        })
        .collect();
    let mut previous = ready;
    for (i, turn) in turns.iter_mut().enumerate() {
        turn.first_of_run = i == 0;
        turn.idle_us = previous.map(|end| begun[i].saturating_sub(end));
        previous = Some(
            turn.finished_us
                .or(turn.first_sound_us)
                .unwrap_or(turn.endpoint_us)
                .max(begun[i]),
        );
    }
    for (at, stalled) in gaps {
        let Some(owner) = (0..turns.len())
            .rev()
            .find(|&i| begun[i] <= *at && *at <= window_end[i])
        else {
            continue;
        };
        let kept = &mut turns[owner].stalled_us;
        *kept = Some(kept.map_or(*stalled, |kept| kept.max(*stalled)));
    }
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

/// The turns apart by what came before them (#1608): a run's first
/// request, one after at least [`LONG_IDLE_US`] with nothing going on, and
/// the rest. For each: how many, how many reached the speaker and the
/// median time to it, and how many had a capture gap and the longest.
fn by_idle(rows: &[Value]) -> Value {
    let long_ms = LONG_IDLE_US as f64 / 1_000.0;
    let group = |keep: &dyn Fn(&Value) -> bool| {
        let mine: Vec<&Value> = rows.iter().filter(|row| keep(row)).collect();
        let stalled: Vec<f64> = mine
            .iter()
            .filter_map(|row| row["stalled_ms"].as_f64())
            .collect();
        json!({
            "turns": mine.len(),
            "cut": mine.iter().filter(|row| row["cut"] == true).count(),
            "answered": mine.iter().filter(|row| !row["first_sound_ms"].is_null()).count(),
            "first_sound_ms": median(mine.iter().filter_map(|row| row["first_sound_ms"].as_f64()).collect()),
            "stalled": stalled.len(),
            "longest_stall_ms": stalled.iter().copied().reduce(f64::max),
        })
    };
    let long = |row: &Value| row["idle_ms"].as_f64().is_some_and(|idle| idle >= long_ms);
    json!({
        "long_idle_ms": long_ms,
        "first_of_run": group(&|row| row["first_of_run"] == true),
        "after_long_idle": group(&|row| row["first_of_run"] != true && long(row)),
        "rest": group(&|row| row["first_of_run"] != true && !long(row)),
    })
}

/// Medians for turns with a function call and for plain ones, of the turns
/// that reached the speaker; each step of a function turn, too.
pub fn summary(turns: &[Turn]) -> Value {
    let rows: Vec<Value> = turns.iter().map(Turn::row).collect();
    let of = |kind: &str, field: &str| {
        median(
            rows.iter()
                .filter(|row| row["kind"] == kind && !row["first_sound_ms"].is_null())
                .filter_map(|row| row[field].as_f64())
                .collect(),
        )
    };
    let between = |from: &str, to: &str| {
        median(
            rows.iter()
                .filter(|row| row["kind"] == "function" && !row["first_sound_ms"].is_null())
                .filter_map(|row| Some(row[to].as_f64()? - row[from].as_f64()?))
                .collect(),
        )
    };
    let count = |kind: &str| {
        rows.iter()
            .filter(|row| row["kind"] == kind && !row["first_sound_ms"].is_null())
            .count()
    };
    json!({
        "status": "turn_times_summary",
        "turns": rows.iter().filter(|row| row["cut"] != true).count(),
        "cut": rows.iter().filter(|row| row["cut"] == true).count(),
        "plain": {
            "n": count("plain"),
            "first_output_ms": of("plain", "first_output_ms"),
            "first_audio_ms": of("plain", "first_audio_ms"),
            "first_sound_ms": of("plain", "first_sound_ms"),
        },
        "function": {
            "n": count("function"),
            "call_ms": of("function", "call_ms"),
            "answer_sent_after_call_ms": between("call_ms", "answer_sent_ms"),
            // Only for turns whose first audio came after our answer was
            // sent: before it, a lead-in or audio while the call was out,
            // it is not the answer's delay (#810 review, 17:23 and 18:33).
            "first_audio_after_answer_ms": median(
                rows.iter()
                    .filter(|row| {
                        row["kind"] == "function"
                            && !row["first_sound_ms"].is_null()
                            && row["audio_before_answer"] != true
                    })
                    .filter_map(|row| Some(row["first_audio_ms"].as_f64()? - row["answer_sent_ms"].as_f64()?))
                    .collect(),
            ),
            "with_lead_in": rows.iter().filter(|row| row["lead_in"] == true).count(),
            "audio_before_answer": rows.iter().filter(|row| row["audio_before_answer"] == true).count(),
            "first_audio_ms": of("function", "first_audio_ms"),
            "first_sound_ms": of("function", "first_sound_ms"),
        },
        "by_idle": by_idle(&rows),
        "boundary": "from the voice's end-of-speech decision (local_endpoint) to the first write the speaker accepted (speaker_first_write); acoustic onset unmeasured; calls only where a first-output file has voice_tool_call lines",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str, turn: u64, at_us: u64) -> (PathBuf, Value) {
        (
            PathBuf::from("/s/one/metadata"),
            json!({"kind": kind, "turn": turn, "at_us": at_us}),
        )
    }
    fn first(request: u64, trigger: &str, at_us: u64) -> (PathBuf, Value) {
        (
            PathBuf::from("/f"),
            json!({"event":"voice_first_output","request":request,"trigger":trigger,"sampled_start_us":at_us}),
        )
    }
    fn call(request: u64, name: &str, called_us: u64, sent_us: u64) -> (PathBuf, Value) {
        (
            PathBuf::from("/f"),
            json!({"event":"voice_tool_call","request":request,"name":name,"called_us":called_us,"sent_us":sent_us}),
        )
    }

    #[test]
    fn each_turn_is_timed_from_its_end_of_speech_and_a_call_makes_it_a_function_turn() {
        let events = vec![
            // A plain answer: 0.9 s to the model's audio, 1.25 s to the speaker.
            event("local_endpoint", 4, 10_000_000),
            event("provider_first_audio", 4, 10_900_000),
            event("speaker_first_write", 4, 11_250_000),
            event("turn_finished", 4, 13_000_000),
            // A turn with a call: the call at 1.0 s, the answer 45 ms after,
            // the audio 0.95 s after the answer, the sound 0.35 s after.
            event("local_endpoint", 5, 20_000_000),
            event("provider_first_audio", 5, 21_995_000),
            event("speaker_first_write", 5, 22_345_000),
            // A turn the model said was not for it: a signal, not a function.
            event("local_endpoint", 6, 30_000_000),
        ];
        let lines = vec![
            first(4, "audio", 10_850_000),
            first(5, "tool_call", 21_000_000),
            call(5, "confirm_agent_task", 21_000_000, 21_045_000),
            call(6, "not_for_me", 30_800_000, 30_800_100),
            // The same number from a run long after: not this turn's.
            first(4, "tool_call", 500_000_000),
        ];
        let turns = turns(&events, &lines);
        assert_eq!(turns.len(), 3);
        let plain = turns[0].row();
        assert_eq!(
            (plain["kind"].clone(), plain["first_output"].clone()),
            (json!("plain"), json!("audio"))
        );
        assert_eq!(plain["first_audio_ms"], 900.0);
        assert_eq!(plain["first_sound_ms"], 1_250.0);
        let function = turns[1].row();
        assert_eq!(function["kind"], "function");
        assert_eq!(function["functions"], json!(["confirm_agent_task"]));
        assert_eq!(
            (
                function["call_ms"].clone(),
                function["answer_sent_ms"].clone()
            ),
            (json!(1_000.0), json!(1_045.0))
        );
        assert_eq!(function["first_sound_ms"], 2_345.0);
        assert_eq!(turns[2].row()["kind"], "plain");
        let summary = summary(&turns);
        assert_eq!(summary["plain"]["n"], 1);
        assert_eq!(summary["plain"]["first_sound_ms"], 1_250.0);
        assert_eq!(summary["function"]["n"], 1);
        assert_eq!(summary["function"]["answer_sent_after_call_ms"], 45.0);
        assert_eq!(summary["function"]["first_audio_after_answer_ms"], 950.0);
        assert_eq!(summary["function"]["first_sound_ms"], 2_345.0);
        // Numbers and names only: no words.
        let text = summary.to_string();
        assert!(!text.contains("said") && !text.contains("text"));
    }

    /// The reviewer on #810 (17:23): the service writes its records into
    /// numbered files in turn, so a later file can be read first; read that
    /// way, a turn lost its audio, its sound and how it ended. And a lead-in
    /// said before the call was taken for the audio after our answer, at
    /// -204 ms.
    #[test]
    fn the_order_records_are_read_in_does_not_matter_and_a_lead_in_is_not_the_answers_delay() {
        let finished = |turn: u64, at: u64| {
            (
                PathBuf::from("/s/one/metadata"),
                json!({"kind":"turn_finished","turn":turn,"at_us":at,"outcome":"completed"}),
            )
        };
        // The later file first: what came of turn 4, then its end of speech.
        let rotated = vec![
            event("speaker_first_write", 4, 11_200_000),
            event("provider_first_audio", 4, 11_000_000),
            finished(4, 13_000_000),
            event("local_endpoint", 4, 10_000_000),
            // A lead-in at 0.9 s, the call at 1.1 s, our answer 4 ms after.
            event("provider_first_audio", 5, 20_900_000),
            event("speaker_first_write", 5, 21_250_000),
            event("local_endpoint", 5, 20_000_000),
        ];
        let lines = vec![call(5, "confirm_agent_task", 21_100_000, 21_104_000)];
        let turns = turns(&rotated, &lines);
        let plain = turns[0].row();
        assert_eq!(
            (
                plain["first_audio_ms"].clone(),
                plain["first_sound_ms"].clone(),
                plain["outcome"].clone()
            ),
            (json!(1_000.0), json!(1_200.0), json!("completed"))
        );
        let lead_in = turns[1].row();
        assert_eq!(
            (lead_in["kind"].clone(), lead_in["lead_in"].clone()),
            (json!("function"), json!(true))
        );
        let summary = summary(&turns);
        assert_eq!(summary["plain"]["n"], 1);
        assert!(
            summary["function"]["first_audio_after_answer_ms"].is_null(),
            "{summary}"
        );
        assert_eq!(summary["function"]["with_lead_in"], 1);
        assert_eq!(summary["function"]["first_sound_ms"], 1_250.0);
        // Read in any order, the same.
        let mut reversed = rotated.clone();
        reversed.reverse();
        assert_eq!(super::turns(&reversed, &lines), turns);
    }

    /// The reviewer on #810 (18:33): audio between the call and our answer
    /// is not a lead-in, and it is not the audio after our answer either;
    /// taken for it, it read -2 ms. The turn is left out of that median,
    /// and counted.
    #[test]
    fn audio_between_the_call_and_the_answer_is_not_the_answers_delay() {
        let events = vec![
            // The call at 1.1 s, audio at 1.102 s, our answer sent at 1.104 s.
            event("local_endpoint", 5, 20_000_000),
            event("provider_first_audio", 5, 21_102_000),
            event("speaker_first_write", 5, 21_400_000),
            // A turn measured as before: the answer at 1.045 s, audio 0.95 s after.
            event("local_endpoint", 6, 30_000_000),
            event("provider_first_audio", 6, 31_995_000),
            event("speaker_first_write", 6, 32_345_000),
        ];
        let lines = vec![
            call(5, "confirm_agent_task", 21_100_000, 21_104_000),
            call(6, "confirm_agent_task", 31_000_000, 31_045_000),
        ];
        let turns = turns(&events, &lines);
        let between = turns[0].row();
        assert_eq!(
            (
                between["lead_in"].clone(),
                between["audio_before_answer"].clone()
            ),
            (json!(false), json!(true))
        );
        assert_eq!(turns[1].row()["audio_before_answer"], false);
        let summary = summary(&turns);
        assert_eq!(summary["function"]["n"], 2);
        assert_eq!(summary["function"]["first_audio_after_answer_ms"], 950.0);
        assert_eq!(summary["function"]["audio_before_answer"], 1);
        assert_eq!(summary["function"]["with_lead_in"], 0);
    }

    #[test]
    fn records_are_read_from_every_folder_under_the_root_and_broken_lines_are_passed_over() {
        let root = std::env::temp_dir().join(format!("turn-times-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let metadata = root.join("run-1/metadata");
        std::fs::create_dir_all(&metadata).unwrap();
        std::fs::write(
            metadata.join("events-0.jsonl"),
            "{\"kind\":\"local_endpoint\",\"turn\":1,\"at_us\":5}\nnot json\n",
        )
        .unwrap();
        std::fs::write(metadata.join("health.json"), "{}").unwrap();
        std::fs::write(
            root.join("first-output-1.jsonl"),
            "{\"event\":\"capture_start\"}\n",
        )
        .unwrap();
        let events = lines_under(&root, "events-").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, metadata);
        assert_eq!(lines_under(&root, "first-output-").unwrap().len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// #1608: a run's first request after six idle minutes, its capture gap
    /// counted to it and not the start's, a quick follow-up, and a turn after
    /// another long idle, each kept apart in the summary; a run with no
    /// `listening_ready` has a first request with no idle time.
    #[test]
    fn a_runs_first_request_and_one_after_a_long_idle_are_kept_apart_with_their_stalls() {
        const S: u64 = 1_000_000;
        let at = |folder: &str, value: Value| (PathBuf::from(folder), value);
        let one = "/s/one/metadata";
        let events = vec![
            at(one, json!({"kind":"listening_ready","at_us":10 * S})),
            // The start's own stall, before any request: no turn's.
            at(
                one,
                json!({"kind":"capture_gap","stalled_us":160_000,"dropped_frames":0,"at_us":10 * S + S / 2}),
            ),
            // Request 1, six minutes later, with its stall 0.6 s in.
            at(
                one,
                json!({"kind":"input_admitted","turn":1,"at_us":370 * S}),
            ),
            at(
                one,
                json!({"kind":"capture_gap","stalled_us":135_000,"dropped_frames":2,"at_us":370 * S + 600_000}),
            ),
            at(
                one,
                json!({"kind":"local_endpoint","turn":1,"at_us":372 * S}),
            ),
            at(
                one,
                json!({"kind":"speaker_first_write","turn":1,"at_us":373 * S}),
            ),
            at(
                one,
                json!({"kind":"turn_finished","turn":1,"at_us":375 * S,"outcome":"completed"}),
            ),
            // Request 2, five seconds after it.
            at(
                one,
                json!({"kind":"input_admitted","turn":2,"at_us":380 * S}),
            ),
            at(
                one,
                json!({"kind":"local_endpoint","turn":2,"at_us":381 * S}),
            ),
            at(
                one,
                json!({"kind":"speaker_first_write","turn":2,"at_us":382 * S}),
            ),
            at(
                one,
                json!({"kind":"turn_finished","turn":2,"at_us":384 * S,"outcome":"completed"}),
            ),
            // Request 3, after another six minutes, not answered.
            at(
                one,
                json!({"kind":"input_admitted","turn":3,"at_us":744 * S}),
            ),
            at(
                one,
                json!({"kind":"local_endpoint","turn":3,"at_us":745 * S}),
            ),
            at(
                one,
                json!({"kind":"turn_finished","turn":3,"at_us":746 * S,"outcome":"no_audio_answer"}),
            ),
            // Another run, with no listening_ready in its records.
            at(
                "/s/two/metadata",
                json!({"kind":"local_endpoint","turn":1,"at_us":5 * S}),
            ),
            at(
                "/s/two/metadata",
                json!({"kind":"speaker_first_write","turn":1,"at_us":6 * S}),
            ),
        ];
        let turns = turns(&events, &[]);
        let rows: Vec<Value> = turns.iter().map(Turn::row).collect();
        assert_eq!(rows.len(), 4);
        let (first, second, third, other) = (&rows[0], &rows[1], &rows[2], &rows[3]);
        assert_eq!(
            (
                first["first_of_run"].clone(),
                first["idle_ms"].clone(),
                first["stalled_ms"].clone()
            ),
            (json!(true), json!(360_000.0), json!(135.0))
        );
        assert_eq!(
            (
                second["first_of_run"].clone(),
                second["idle_ms"].clone(),
                second["stalled_ms"].clone()
            ),
            (json!(false), json!(5_000.0), Value::Null)
        );
        assert_eq!(third["idle_ms"], json!(360_000.0));
        assert_eq!(
            (other["first_of_run"].clone(), other["idle_ms"].clone()),
            (json!(true), Value::Null)
        );
        let by = &summary(&turns)["by_idle"];
        assert_eq!(by["first_of_run"]["turns"], 2);
        assert_eq!(by["first_of_run"]["stalled"], 1);
        assert_eq!(by["first_of_run"]["longest_stall_ms"], json!(135.0));
        assert_eq!(by["first_of_run"]["first_sound_ms"], json!(1_000.0));
        assert_eq!(by["after_long_idle"]["turns"], 1);
        assert_eq!(by["after_long_idle"]["answered"], 0);
        assert_eq!(by["rest"]["turns"], 1);
        assert_eq!(by["rest"]["stalled"], 0);
        // Numbers only: no words.
        assert!(!summary(&turns).to_string().contains("heard"));
    }

    /// #1615 review, 21:56: a request cut before its end of speech, its
    /// stall inside it, is still the run's first and its stall counted; the
    /// request after it is not the first, and its idle runs from the cut
    /// one's end.
    #[test]
    fn a_first_request_cut_before_its_end_of_speech_is_the_runs_first_with_its_stall() {
        const S: u64 = 1_000_000;
        let one = PathBuf::from("/s/one/metadata");
        let at = |value: Value| (one.clone(), value);
        let events = vec![
            at(json!({"kind":"listening_ready","at_us":10 * S})),
            at(json!({"kind":"input_admitted","turn":1,"at_us":370 * S})),
            at(
                json!({"kind":"capture_gap","stalled_us":135_000,"dropped_frames":2,"at_us":370 * S + 600_000}),
            ),
            at(json!({"kind":"turn_finished","turn":1,"at_us":371 * S,"outcome":"capture_gap"})),
            at(json!({"kind":"input_admitted","turn":2,"at_us":375 * S})),
            at(json!({"kind":"local_endpoint","turn":2,"at_us":376 * S})),
            at(json!({"kind":"speaker_first_write","turn":2,"at_us":377 * S})),
            at(json!({"kind":"turn_finished","turn":2,"at_us":379 * S,"outcome":"completed"})),
        ];
        // A first-output line for request 1 goes to no cut request.
        let firsts = vec![first(1, "audio", 370 * S + 1)];
        let turns = turns(&events, &firsts);
        let rows: Vec<Value> = turns.iter().map(Turn::row).collect();
        assert_eq!(rows.len(), 2);
        let (cut, next) = (&rows[0], &rows[1]);
        assert_eq!(
            (
                cut["cut"].clone(),
                cut["first_of_run"].clone(),
                cut["idle_ms"].clone(),
                cut["stalled_ms"].clone()
            ),
            (json!(true), json!(true), json!(360_000.0), json!(135.0))
        );
        assert_eq!(
            (cut["outcome"].clone(), cut["first_output"].clone()),
            (json!("capture_gap"), Value::Null)
        );
        assert_eq!(
            (
                next["cut"].clone(),
                next["first_of_run"].clone(),
                next["idle_ms"].clone(),
                next["first_sound_ms"].clone()
            ),
            (json!(false), json!(false), json!(4_000.0), json!(1_000.0))
        );
        let summary = summary(&turns);
        assert_eq!(
            (summary["turns"].clone(), summary["cut"].clone()),
            (json!(1), json!(1))
        );
        let by = &summary["by_idle"];
        assert_eq!(
            (
                by["first_of_run"]["turns"].clone(),
                by["first_of_run"]["cut"].clone(),
                by["first_of_run"]["longest_stall_ms"].clone()
            ),
            (json!(1), json!(1), json!(135.0))
        );
        assert_eq!(by["rest"]["turns"], 1);
    }

    /// #1615 review, 21:56: a gap goes to one turn only, the latest admitted
    /// (the one being heard), and a turn with no `turn_finished` holds a gap
    /// after its end of speech; an overlapping turn's idle time is 0.
    #[test]
    fn a_gap_goes_to_one_turn_and_an_unfinished_turn_holds_one_after_its_end_of_speech() {
        const S: u64 = 1_000_000;
        let one = PathBuf::from("/s/one/metadata");
        let at = |value: Value| (one.clone(), value);
        let events = vec![
            at(json!({"kind":"listening_ready","at_us":0})),
            at(json!({"kind":"input_admitted","turn":1,"at_us":10 * S})),
            at(json!({"kind":"local_endpoint","turn":1,"at_us":11 * S})),
            at(json!({"kind":"speaker_first_write","turn":1,"at_us":12 * S})),
            // Request 2 is admitted while request 1 still plays.
            at(json!({"kind":"input_admitted","turn":2,"at_us":13 * S})),
            at(
                json!({"kind":"capture_gap","stalled_us":180_000,"dropped_frames":0,"at_us":13 * S + S / 2}),
            ),
            at(json!({"kind":"turn_finished","turn":1,"at_us":14 * S,"outcome":"completed"})),
            at(json!({"kind":"local_endpoint","turn":2,"at_us":15 * S})),
            // The run ends mid-turn: no sound, no turn_finished.
            at(
                json!({"kind":"capture_gap","stalled_us":250_000,"dropped_frames":0,"at_us":15 * S + S / 2}),
            ),
        ];
        let rows: Vec<Value> = turns(&events, &[]).iter().map(Turn::row).collect();
        assert_eq!(rows[0]["stalled_ms"], Value::Null);
        assert_eq!(
            (rows[1]["idle_ms"].clone(), rows[1]["stalled_ms"].clone()),
            (json!(0.0), json!(250.0))
        );
    }
}
