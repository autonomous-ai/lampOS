//! An opt-in conversational vote for the second route (#8849).
//! The opening call supplies the only reference. All times use the audio clock.

use super::{DIFFERENT_BELOW, SAME_FROM, cosine};

pub(crate) const SWITCH: &str = "LAMP_FOLLOW_UP_CALLER_MATCH";
const TIMELY_US: u64 = 100_000;
use std::sync::atomic::{AtomicU64, Ordering};

// The second worker's existing memory/forgot subscriber advances this even
// when its other optional consumers are disabled.
static FORGET: AtomicU64 = AtomicU64::new(0);
pub(crate) fn forgotten() {
    FORGET.fetch_add(1, Ordering::AcqRel);
}

#[cfg(test)]
pub(super) use crate::scenario_clock::monotonic_us;
#[cfg(not(test))]
pub(super) use lamp_ipc::monotonic_us;

struct Print {
    value: Vec<f32>,
    ready_us: u64,
}

struct Caller {
    request: u64,
    print: Option<Print>,
    answered_until_us: Option<u64>,
    opened: std::time::Instant,
    opened_us: u64,
}

struct Heard {
    request: u64,
    clean: bool,
    ended: bool,
    touched_us: u64,
    submitted_us: Option<u64>,
    print: Option<Print>,
    trial: Option<Trial>,
}

struct Trial {
    ended_us: u64,
    decision_us: u64,
    reported_score: bool,
    timely_match: bool,
    first_admission_us: Option<u64>,
    renewed: Option<bool>,
}

#[derive(Default)]
pub(crate) struct CallerMatch {
    caller: Option<Caller>,
    line: Option<Heard>,
    rows: Option<std::sync::mpsc::SyncSender<Row>>,
    generation: u64,
}

impl CallerMatch {
    pub(super) fn journaled() -> Option<Self> {
        let (rows, stream) = std::sync::mpsc::sync_channel::<Row>(64);
        let worker_epoch = format!("{}-{}", std::process::id(), monotonic_us());
        std::thread::Builder::new()
            .name("caller-match-rows".into())
            .spawn(move || {
                for row in stream {
                    row.write(&worker_epoch);
                }
            })
            .ok()?;
        Some(Self {
            rows: Some(rows),
            ..Self::default()
        })
    }

    pub(super) fn maintain(&mut self, now: u64) {
        let generation = FORGET.load(Ordering::Acquire);
        if self.generation != generation {
            self.reset();
            self.generation = generation;
        }
        if self.caller.as_ref().is_some_and(|c| {
            now.saturating_sub(c.answered_until_us.unwrap_or(c.opened_us))
                > crate::follows_on::WITHIN_US
        }) {
            self.caller = None;
        }
        if self
            .line
            .as_ref()
            .is_some_and(|l| now.saturating_sub(l.touched_us) > crate::follows_on::WITHIN_US)
        {
            self.line = None;
        }
    }

    pub(crate) fn reset(&mut self) {
        self.line = None;
        self.caller = None;
    }

    pub(crate) fn check_scope(
        &mut self,
        changed: bool,
        closed: Option<std::time::Instant>,
        has_words: bool,
    ) {
        if self.caller.as_ref().is_some_and(|c| {
            changed
                || closed.is_some_and(|at| at >= c.opened)
                || (c.answered_until_us.is_some() && !has_words)
        }) {
            self.caller = None;
        }
    }

    pub(crate) fn declined(&mut self, request: u64) {
        if self.caller.as_ref().is_some_and(|c| c.request == request) {
            self.caller = None;
        }
        if self.line.as_ref().is_some_and(|l| l.request == request) {
            self.line = None;
        }
    }

    /// A new recording retires every pending vote from the previous line.
    pub(super) fn began(&mut self, request: u64, clean: bool, now_us: u64) {
        self.maintain(now_us);
        self.line = Some(Heard {
            request,
            clean,
            ended: false,
            touched_us: now_us,
            submitted_us: None,
            print: None,
            trial: None,
        });
    }

    pub(super) fn submitted(&mut self, request: u64, now: u64) {
        if let Some(line) = self.line.as_mut().filter(|l| l.request == request) {
            line.submitted_us = Some(now);
        }
    }

    pub(super) fn ended(&mut self, request: u64, now: u64) {
        if let Some(line) = self.line.as_mut().filter(|l| l.request == request) {
            line.ended = true;
            line.touched_us = now;
        }
    }

    pub(super) fn received(&mut self, request: u64, value: &[f32], ready_us: u64) {
        let slot = if let Some(caller) = self.caller.as_mut().filter(|c| c.request == request) {
            &mut caller.print
        } else if let Some(line) = self
            .line
            .as_mut()
            .filter(|l| l.request == request && l.clean)
        {
            &mut line.print
        } else {
            return;
        };
        slot.get_or_insert_with(|| Print {
            value: value.to_vec(),
            ready_us,
        });
        // A held line is still a measurement opportunity. Report a result
        // first seen after its decision without re-running its admission.
        if let (Some(caller), Some(line)) = (&self.caller, &self.line)
            && let Some(trial) = line.trial.as_ref().filter(|t| !t.reported_score)
            && let Some(score) = self.score(line.request)
        {
            self.report(
                caller.request,
                line.request,
                "available",
                trial.ended_us,
                Some(score),
                false,
            );
            self.line
                .as_mut()
                .unwrap()
                .trial
                .as_mut()
                .unwrap()
                .reported_score = true;
        }
    }

    /// Called only for a newly admitted spoken call. Even an unprintable
    /// new call retires the previous reference. A follow-up never opens one.
    pub(crate) fn opened(&mut self, request: u64) {
        if self.caller.as_ref().is_some_and(|c| c.request == request) {
            return;
        }
        self.caller = self
            .line
            .as_mut()
            .filter(|l| l.request == request && l.clean && l.ended)
            .map(|line| Caller {
                request,
                print: line.print.take(),
                answered_until_us: None,
                opened: std::time::Instant::now(),
                opened_us: line.touched_us,
            });
    }

    fn score(&self, request: u64) -> Option<(f32, u64)> {
        let caller = self.caller.as_ref()?.print.as_ref()?;
        let line = self
            .line
            .as_ref()
            .filter(|l| l.request == request && l.clean && l.ended)?;
        let other = line.print.as_ref()?;
        Some((
            cosine(&caller.value, &other.value)?,
            caller.ready_us.max(other.ready_us),
        ))
    }

    /// Freeze the first admission decision. Later polls can only supply a
    /// score for renewal after an already-admitted line actually gets audio.
    pub(crate) fn admission(&mut self, request: u64, said: (u64, u64)) -> bool {
        let Some(caller) = &self.caller else {
            return false;
        };
        let Some(last_answer) = caller.answered_until_us else {
            return false;
        };
        let Some(line) = self
            .line
            .as_ref()
            .filter(|l| l.request == request && l.ended)
        else {
            return false;
        };
        if caller.request == request
            || said.1 < said.0
            || said.0.saturating_sub(last_answer) > crate::follows_on::WITHIN_US
        {
            return false;
        }
        if let Some(trial) = &line.trial {
            return trial.timely_match;
        }
        if said.0 < last_answer {
            let line = self.line.as_mut().unwrap();
            line.clean = false;
            line.print = None;
        }
        let score = self.score(request);
        let timely = score.is_some_and(|(_, at)| at <= said.1.saturating_add(TIMELY_US));
        let positive = timely && score.is_some_and(|(s, _)| s >= SAME_FROM);
        self.line.as_mut().unwrap().trial = Some(Trial {
            ended_us: said.1,
            decision_us: monotonic_us(),
            reported_score: score.is_some(),
            timely_match: positive,
            first_admission_us: None,
            renewed: None,
        });
        self.report(
            caller.request,
            request,
            "admission",
            said.1,
            score,
            positive,
        );
        positive
    }

    pub(crate) fn admitted(&mut self, request: u64, admitted: bool) {
        let Some(caller) = self.caller.as_ref().map(|c| c.request) else {
            return;
        };
        let Some(trial) = self
            .line
            .as_mut()
            .filter(|l| l.request == request)
            .and_then(|l| l.trial.as_mut())
        else {
            return;
        };
        if admitted && trial.first_admission_us.is_none() {
            trial.first_admission_us = Some(monotonic_us());
            let ended_us = trial.ended_us;
            self.report(
                caller,
                request,
                "admitted",
                ended_us,
                self.score(request),
                false,
            );
        }
    }

    /// Called at the existing playback receipt. A matched ordinary answer
    /// may renew if its only restriction was that its words had no tie to
    /// the conversation. Other renewal restrictions remain with the worker.
    pub(crate) fn answered(
        &mut self,
        request: u64,
        until_us: u64,
        now_us: u64,
        renews: bool,
        untied_only: bool,
    ) -> bool {
        if let Some(line) = self
            .line
            .as_mut()
            .filter(|l| l.request != request && !l.ended)
        {
            line.clean = false;
            line.print = None;
        }
        let Some(caller) = &mut self.caller else {
            return false;
        };
        if caller.request == request {
            if self.line.as_ref().is_some_and(|l| l.request == request) {
                caller.answered_until_us = caller.answered_until_us.max(Some(until_us));
            }
            return false;
        }
        let Some(trial) = self
            .line
            .as_ref()
            .filter(|l| l.request == request)
            .and_then(|l| l.trial.as_ref())
            .filter(|t| t.first_admission_us.is_some())
        else {
            return false;
        };
        let Some(end) = caller.answered_until_us else {
            return false;
        };
        if until_us < end || now_us.saturating_sub(end) > crate::follows_on::WITHIN_US {
            return false;
        }
        if until_us < trial.ended_us {
            return false;
        }
        if let Some(renewed) = trial.renewed {
            if renewed {
                caller.answered_until_us = Some(until_us);
            }
            return false;
        }
        let caller_id = caller.request;
        let ended_us = trial.ended_us;
        let score = self.score(request);
        let helps = untied_only && score.is_some_and(|(s, _)| s >= SAME_FROM);
        self.report(caller_id, request, "renewal", ended_us, score, helps);
        self.line.as_mut().unwrap().trial.as_mut().unwrap().renewed = Some(renews || helps);
        if renews || helps {
            self.caller.as_mut().unwrap().answered_until_us = Some(until_us);
        }
        helps
    }

    fn report(
        &self,
        caller: u64,
        request: u64,
        phase: &'static str,
        end: u64,
        score: Option<(f32, u64)>,
        vote: bool,
    ) {
        if let Some(rows) = &self.rows {
            // Logging cannot delay capture, admission or playback. An absent
            // prediction counts as a miss in the independently labelled set.
            let Some(line) = &self.line else { return };
            let Some(trial) = &line.trial else { return };
            let _ = rows.try_send(Row {
                submitted_us: line.submitted_us,
                decision_us: trial.decision_us,
                first_admission_us: trial.first_admission_us,
                caller,
                request,
                phase,
                end,
                score,
                vote,
            });
        }
    }
}

struct Row {
    first_admission_us: Option<u64>,
    submitted_us: Option<u64>,
    decision_us: u64,
    caller: u64,
    request: u64,
    phase: &'static str,
    end: u64,
    score: Option<(f32, u64)>,
    vote: bool,
}

impl Row {
    fn write(&self, worker_epoch: &str) {
        let class = match self.score {
            Some((s, _)) if s >= SAME_FROM => "same",
            Some((s, _)) if s < DIFFERENT_BELOW => "different",
            _ => "unknown",
        };
        // Serialization and the journal write run outside the voice worker's
        // input path. The kernel boot ID identifies the capture clock on Linux.
        let data = serde_json::json!({
            "clock_id": lamp_bus::kernel_boot_id(),
            "worker_epoch": worker_epoch,
            "caller_request_id": self.caller.to_string(), "request_id": self.request.to_string(), "phase": self.phase,
            "output": class, "cosine": self.score.map(|(s,_)| s),
            "line_end_us": self.end, "input_end_us": self.submitted_us,
            "first_admission_us": self.first_admission_us,
            "ready_us": self.score.map(|(_,at)| at), "decision_us": self.decision_us,
            "positive_vote": self.vote
        });
        eprintln!("lamp-voice caller-match: {data}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const END: u64 = 1_000_000;
    const LINE: (u64, u64) = (2_000_000, 4_000_000);

    fn with_call() -> CallerMatch {
        let mut sensor = CallerMatch::default();
        sensor.began(1, true, 0);
        sensor.ended(1, END);
        sensor.received(1, &[1., 0.], END);
        sensor.opened(1);
        sensor.answered(1, END, END, true, false);
        sensor
    }

    fn start(sensor: &mut CallerMatch, id: u64, clean: bool) {
        sensor.began(id, clean, LINE.0);
        sensor.ended(id, LINE.1);
    }

    #[test]
    fn caller_match_uses_only_the_opening_print_and_reports_the_measured_score() {
        let mut sensor = with_call();
        let (rows, recorded) = std::sync::mpsc::sync_channel(2);
        sensor.rows = Some(rows);
        for (id, score) in [
            (2, 0.26_f32),
            (3, 0.29),
            (4, 0.30),
            (5, 0.69),
            (6, 0.70),
            (7, 1.0),
        ] {
            start(&mut sensor, id, true);
            sensor.received(
                id,
                &[score, (1. - score * score).sqrt()],
                LINE.1 + TIMELY_US,
            );
            assert_eq!(sensor.admission(id, LINE), score >= SAME_FROM);
            let row = recorded.try_recv().unwrap();
            assert_eq!(row.caller, 1);
            assert_eq!(row.phase, "admission");
            assert_eq!(row.score, Some((score, LINE.1 + TIMELY_US)));
            sensor.admitted(id, true);
            assert!(recorded.try_recv().unwrap().first_admission_us.is_some());
            assert!(!sensor.answered(id, LINE.1, LINE.1, true, false));
            assert_eq!(recorded.try_recv().unwrap().phase, "renewal");
            // The next case starts inside this answer's window.
            sensor.caller.as_mut().unwrap().answered_until_us = Some(END);
        }
    }

    #[test]
    fn caller_match_after_admission_can_only_renew_an_ordinary_spoken_answer_once() {
        for (before_decision, delay, admitted, untied, expected) in [
            (false, 0, true, true, true),
            (true, TIMELY_US + 1, true, true, true),
            (false, 0, false, true, false),
            (false, 0, true, false, false),
        ] {
            let mut sensor = with_call();
            start(&mut sensor, 2, true);
            if before_decision {
                sensor.received(2, &[1., 0.], LINE.1 + delay);
            }
            assert!(!sensor.admission(2, LINE));
            sensor.admitted(2, admitted);
            if !before_decision {
                sensor.received(2, &[1., 0.], LINE.1 + delay);
            }
            assert!(!sensor.admission(2, LINE));
            let at = LINE.1 + TIMELY_US + 2;
            assert_eq!(sensor.answered(2, at, at, false, untied), expected);
            assert!(!sensor.answered(2, at + 1, at + 1, false, untied));
            assert_eq!(
                sensor.caller.unwrap().answered_until_us,
                Some(if expected { at + 1 } else { END })
            );
        }
    }

    #[test]
    fn caller_match_drops_crossed_lines_unavailable_prints_and_revoked_windows() {
        let mut sensor = with_call();
        start(&mut sensor, 2, false);
        sensor.received(2, &[1., 0.], LINE.1);
        assert!(!sensor.admission(2, LINE), "overlapping playback");
        start(&mut sensor, 3, true);
        sensor.received(2, &[1., 0.], LINE.1);
        assert!(!sensor.admission(3, LINE), "no current print");
        sensor.admitted(3, true);
        start(&mut sensor, 4, true);
        sensor.received(3, &[1., 0.], LINE.1);
        assert!(
            !sensor.answered(3, LINE.1, LINE.1, false, true),
            "newer input"
        );
        sensor.opened(4);
        sensor.answered(4, END, END, true, false);
        start(&mut sensor, 5, true);
        sensor.received(5, &[1., 0.], LINE.1);
        assert!(!sensor.admission(5, LINE), "new caller has no reference");
        for clear in [
            "stop or privacy",
            "forgotten",
            "closed",
            "session changed",
            "declined",
            "expired",
        ] {
            let mut sensor = with_call();
            match clear {
                "stop or privacy" => sensor.reset(),
                "forgotten" => sensor.check_scope(false, None, false),
                "closed" => sensor.check_scope(false, Some(std::time::Instant::now()), true),
                "session changed" => sensor.check_scope(true, None, true),
                "declined" => sensor.declined(1),
                _ => sensor.began(2, true, END + crate::follows_on::WITHIN_US + 1),
            }
            start(&mut sensor, 3, true);
            sensor.received(1, &[1., 0.], LINE.1);
            sensor.received(3, &[1., 0.], LINE.1);
            assert!(!sensor.admission(3, LINE), "{clear}");
        }
    }

    /// Time only the additional vote with the two cached 256-value test
    /// vectors or a printer that has not answered. The journal cannot drain.
    #[test]
    #[ignore = "host timing evidence for #8849"]
    fn measure_caller_match_admission() {
        use std::hint::black_box;
        use std::time::Instant;
        for available in [true, false] {
            let mut sensor = with_call();
            let (sender, _stalled_reader) = std::sync::mpsc::sync_channel(1);
            sensor.rows = Some(sender);
            let print = vec![1.0; 256];
            sensor
                .caller
                .as_mut()
                .unwrap()
                .print
                .as_mut()
                .unwrap()
                .value = print.clone();
            let mut elapsed = Vec::new();
            for id in 2..2003 {
                start(&mut sensor, id, true);
                if available {
                    sensor.received(id, &print, LINE.1);
                }
                let before = Instant::now();
                let vote = black_box(sensor.admission(black_box(id), LINE));
                elapsed.push(before.elapsed().as_nanos());
                assert_eq!(vote, available);
            }
            let first = elapsed.remove(0);
            elapsed.sort_unstable();
            eprintln!(
                "caller-match available={available} first_ns={first} warm_n={} p50_ns={} p95_ns={}",
                elapsed.len(),
                elapsed[999],
                elapsed[1899]
            );
        }
    }
}
