//! Whether a session's end of speech worked, from the voice's own session
//! records: the end-of-speech worker's states (`endpoint_worker`) and each
//! end of speech's basis (`local_endpoint`'s `endpoint`). The first builder
//! on #1722, 20 Oct 2026 07:20: an old `lamp-turn-end` refused its new
//! options from 19 Oct, the worker failed, every end of speech fell back to
//! the whole silence (`silence_fallback`, `failed`), and no played test said
//! so. Run `lamp-voice-eval endpoint SESSION` after each arm of a played test:
//! it fails when the worker's newest state isn't `ready`, or when an end of
//! speech fell back because the worker failed. Counts and states only: no
//! words.
//!
//! A long session rotates its records, and the worker's states, written as it
//! starts, can be rotated out (lamp-4ace, the first builder on #1722, 20 Oct
//! 12:13: 52 ends `model_complete`, and "never started" said of a worker that
//! ran). With no state in the records, an end the model decided shows the
//! worker ran: it is taken as ready, and said to be inferred.
use serde_json::Value;
use std::{collections::BTreeMap, fmt};

/// The bases of an end of speech the worker's model decided.
pub const MODEL_BASES: [&str; 2] = ["model_complete", "model_uncertain"];

/// A session's end of speech.
#[derive(Debug, Default, PartialEq)]
pub struct Summary {
    /// The worker's states, oldest first: when (`at_us`), the state, and why
    /// when it gave a reason.
    pub states: Vec<(u64, String, Option<String>)>,
    /// The ends of speech from `since_us` on, by basis (`model_complete`,
    /// `silence_fallback`, ...).
    pub bases: BTreeMap<String, usize>,
    /// Of those that fell back, by the fallback's reason (`short`, `failed`,
    /// ...).
    pub fallbacks: BTreeMap<String, usize>,
}

/// The session's lines (any order, any files), with the ends of speech
/// counted from `since_us` (the voice's monotonic clock) on. The worker's
/// states are all kept: one set before an arm began still holds in it.
pub fn summarize<'a>(lines: impl IntoIterator<Item = &'a str>, since_us: u64) -> Summary {
    let mut summary = Summary::default();
    for line in lines {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let at = record["at_us"].as_u64().unwrap_or(0);
        match record["kind"].as_str() {
            Some("endpoint_worker") => {
                let availability = &record["availability"];
                if let Some(state) = availability["state"].as_str() {
                    let detail = availability["detail"].as_str().map(str::to_owned);
                    summary.states.push((at, state.to_owned(), detail));
                }
            }
            Some("local_endpoint") if at >= since_us => {
                let endpoint = &record["endpoint"];
                let Some(basis) = endpoint["basis"].as_str() else {
                    continue;
                };
                *summary.bases.entry(basis.to_owned()).or_default() += 1;
                if let Some(reason) = endpoint["fallback"].as_str() {
                    *summary.fallbacks.entry(reason.to_owned()).or_default() += 1;
                }
            }
            _ => {}
        }
    }
    summary.states.sort_by_key(|(at, _, _)| *at);
    summary
}

impl Summary {
    /// The worker's newest state, and why.
    pub fn newest(&self) -> Option<(&str, Option<&str>)> {
        self.states
            .last()
            .map(|(_, state, detail)| (state.as_str(), detail.as_deref()))
    }

    /// The ends of speech counted.
    pub fn ends(&self) -> usize {
        self.bases.values().sum()
    }

    /// The ends of speech the worker's model decided: only a worker that
    /// ran decides one.
    pub fn model_ends(&self) -> usize {
        MODEL_BASES
            .iter()
            .filter_map(|basis| self.bases.get(*basis))
            .sum()
    }

    /// No state in the records, but ends the model decided: the worker ran,
    /// and its states were rotated out.
    pub fn inferred_ready(&self) -> bool {
        self.states.is_empty() && self.model_ends() > 0
    }

    /// What is wrong, one line each; none when the end of speech worked.
    pub fn faults(&self) -> Vec<String> {
        let mut faults = Vec::new();
        match self.newest() {
            None if self.inferred_ready() => {}
            None => faults.push(
                "no state of the end-of-speech worker in the session: it never started".to_owned(),
            ),
            Some(("ready", _)) => {}
            Some((state, detail)) => faults.push(format!(
                "the end-of-speech worker is {state}{}: every end of speech waits its whole silence",
                detail.map_or_else(String::new, |detail| format!(" ({detail})"))
            )),
        }
        if let Some(&failed) = self.fallbacks.get("failed") {
            faults.push(format!(
                "{failed} of {} ends of speech fell back because the worker failed",
                self.ends()
            ));
        }
        faults
    }
}

impl fmt::Display for Summary {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let states: Vec<&str> = self
            .states
            .iter()
            .map(|(_, state, _)| state.as_str())
            .collect();
        let worker = if self.inferred_ready() {
            format!(
                "ready, inferred: no state in these records (rotated out), and {} ends decided by the model",
                self.model_ends()
            )
        } else if states.is_empty() {
            "none".to_owned()
        } else {
            states.join(", ")
        };
        writeln!(out, "end-of-speech worker: {worker}")?;
        let bases: Vec<String> = self
            .bases
            .iter()
            .map(|(basis, n)| {
                if basis == "silence_fallback" && !self.fallbacks.is_empty() {
                    let reasons: Vec<String> = self
                        .fallbacks
                        .iter()
                        .map(|(why, n)| format!("{why} {n}"))
                        .collect();
                    format!("{basis} {n} ({})", reasons.join(", "))
                } else {
                    format!("{basis} {n}")
                }
            })
            .collect();
        writeln!(out, "ends of speech: {}: {}", self.ends(), bases.join(", "))?;
        for fault in self.faults() {
            writeln!(out, "FAULT: {fault}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(at: u64, state: &str, detail: Option<&str>) -> String {
        serde_json::json!({"kind":"endpoint_worker","at_us":at,
            "availability":{"state":state,"detail":detail},"report":{"ready_after_us":5}})
        .to_string()
    }

    fn end(at: u64, basis: &str, fallback: Option<&str>) -> String {
        serde_json::json!({"kind":"local_endpoint","turn":1,"at_us":at,
            "endpoint":{"basis":basis,"silence_ms":600,"probability":null,"fallback":fallback}})
        .to_string()
    }

    fn of(lines: &[String], since: u64) -> Summary {
        summarize(lines.iter().map(String::as_str), since)
    }

    #[test]
    fn a_ready_worker_and_model_ends_are_no_fault() {
        let lines = [
            worker(10, "loading", None),
            worker(20, "ready", None),
            end(30, "model_complete", None),
            end(40, "model_uncertain", None),
            end(50, "silence_fallback", Some("short")),
        ];
        let summary = of(&lines, 0);
        assert!(summary.faults().is_empty(), "{:?}", summary.faults());
        assert_eq!(
            summary.to_string(),
            "end-of-speech worker: loading, ready\n\
             ends of speech: 3: model_complete 1, model_uncertain 1, silence_fallback 1 (short 1)\n"
        );
    }

    #[test]
    fn a_failed_worker_is_a_fault_with_why_and_so_is_each_failed_fallback() {
        // 19 Oct on lamp-4ace: the old worker refused its options.
        let lines = [
            worker(10, "loading", None),
            worker(20, "failed", Some("exit status 2")),
            end(30, "silence_fallback", Some("failed")),
            end(40, "silence_fallback", Some("failed")),
        ];
        let faults = of(&lines, 0).faults();
        assert_eq!(
            faults,
            [
                "the end-of-speech worker is failed (exit status 2): every end of speech waits its whole silence",
                "2 of 2 ends of speech fell back because the worker failed",
            ]
        );
        assert!(
            of(&lines, 0)
                .to_string()
                .contains("FAULT: the end-of-speech worker is failed")
        );
    }

    #[test]
    fn the_newest_state_counts_in_time_order_whatever_file_order() {
        // The newest state in an earlier line (another file read first).
        let lines = [worker(30, "ready", None), worker(10, "loading", None)];
        assert_eq!(of(&lines, 0).newest(), Some(("ready", None)));
        let lines = [
            worker(30, "recovering", Some("restart")),
            worker(10, "ready", None),
        ];
        assert_eq!(
            of(&lines, 0).newest(),
            Some(("recovering", Some("restart")))
        );
    }

    #[test]
    fn ends_count_from_the_arms_start_and_a_state_set_before_it_holds() {
        let lines = [
            worker(5, "ready", None),
            end(10, "silence_fallback", Some("failed")),
            end(100, "model_complete", None),
        ];
        let summary = of(&lines, 50);
        assert_eq!(summary.ends(), 1);
        assert!(summary.faults().is_empty(), "{:?}", summary.faults());
    }

    #[test]
    fn with_its_states_rotated_out_a_worker_whose_model_decided_ends_is_ready() {
        // lamp-4ace, 20 Oct: the start records rotated out of a long session.
        let lines = [
            end(10, "model_complete", None),
            end(20, "model_uncertain", None),
            end(30, "silence_fallback", Some("short")),
        ];
        let summary = of(&lines, 0);
        assert!(summary.inferred_ready());
        assert!(summary.faults().is_empty(), "{:?}", summary.faults());
        assert_eq!(
            summary.to_string(),
            "end-of-speech worker: ready, inferred: no state in these records (rotated out), \
             and 2 ends decided by the model\n\
             ends of speech: 3: model_complete 1, model_uncertain 1, silence_fallback 1 (short 1)\n"
        );
        // An end that fell back because the worker failed is still a fault.
        let lines = [
            end(10, "model_complete", None),
            end(20, "silence_fallback", Some("failed")),
        ];
        assert_eq!(
            of(&lines, 0).faults(),
            ["1 of 2 ends of speech fell back because the worker failed"]
        );
        // Only the arm's ends count: one the model decided before it is not
        // the worker running in it.
        let lines = [
            end(10, "model_complete", None),
            end(100, "silence_fallback", Some("short")),
        ];
        assert!(!of(&lines, 50).inferred_ready());
        assert_eq!(of(&lines, 50).faults().len(), 1);
    }

    #[test]
    fn no_worker_state_is_a_fault() {
        let lines = [end(10, "silence", None)];
        assert_eq!(
            of(&lines, 0).faults(),
            ["no state of the end-of-speech worker in the session: it never started"]
        );
    }

    #[test]
    fn other_records_and_broken_lines_are_left_alone() {
        let lines = [
            "not json".to_owned(),
            r#"{"kind":"onboarding_step","at_us":5}"#.to_owned(),
            r#"{"kind":"local_endpoint","at_us":6}"#.to_owned(),
            worker(7, "ready", None),
        ];
        let summary = of(&lines, 0);
        assert_eq!(summary.ends(), 0);
        assert!(summary.faults().is_empty());
    }
}
