//! A second asking when the first is slow (#1522; the first builder's yes,
//! 18 Oct 23:08). On lamp-4ace on 18 October the judge answered on a kept
//! connection in 573 ms in the middle and 989 ms nine times in ten, and in a
//! busy room a person waits for it while the lamp holds its reply.
//!
//! With `--hedge-after-ms N`, a question not answered within N ms is asked
//! again on a second connection of its own, and whichever answers first is
//! taken. The other's answer is dropped when it comes, and its connection is
//! kept for the next question. A failure is not asked again: a first asking
//! that fails before N ms is the answer, as without this; once both are
//! asked, one that fails waits for the other. Each second asking is said in
//! the journal once it ends, whichever answered or if neither did, so a
//! day's extra requests can be counted. Off unless set.
use crate::decide::{Asked, Decider, Failed, Question};
use std::{
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

/// What the journal is told, one line at a time.
pub type Say = Arc<dyn Fn(&str) + Send + Sync>;

enum Job {
    Ask {
        number: u64,
        state: String,
        questions: Vec<(String, Question)>,
        deadline: Duration,
    },
    Ready(Duration),
}

/// One connection's judge, on a thread of its own.
struct Lane {
    jobs: mpsc::Sender<Job>,
    /// Asked something whose answer hasn't come back yet.
    busy: bool,
    _thread: thread::JoinHandle<()>,
}

/// Two judges, each with its own connection: the first asked first, the
/// other only once the first has gone `after` without an answer.
pub struct Hedged {
    lanes: [Lane; 2],
    answers: mpsc::Receiver<(usize, u64, Result<Asked, Failed>)>,
    after: Duration,
    number: u64,
    name: String,
    say: Say,
}

impl Hedged {
    pub fn new<D: Decider + Send + 'static>(
        judges: [D; 2],
        after: Duration,
        say: Say,
    ) -> std::io::Result<Self> {
        let name = judges[0].name().to_owned();
        let (answered, answers) = mpsc::channel();
        let [first, second] = judges;
        let lanes = [
            lane(0, first, answered.clone())?,
            lane(1, second, answered)?,
        ];
        Ok(Self {
            lanes,
            answers,
            after,
            number: 0,
            name,
            say,
        })
    }

    /// A lane with nothing outstanding, waiting until `until` for one to
    /// come free; the answers that free them are to questions already
    /// answered, and are dropped.
    fn idle(&mut self, until: Instant) -> Option<usize> {
        loop {
            if let Some(index) = self.lanes.iter().position(|lane| !lane.busy) {
                return Some(index);
            }
            let (lane, _, _) = self
                .answers
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .ok()?;
            self.lanes[lane].busy = false;
        }
    }

    /// Waits for question `number`'s answer from `first`, asking the other
    /// lane too once [`Self::after`] has passed; returns the answer, the lane
    /// it came from, and whether it was asked again.
    fn settle(
        &mut self,
        first: usize,
        number: u64,
        started: Instant,
        until: Instant,
        job: &dyn Fn() -> Job,
    ) -> (Result<Asked, Failed>, Option<usize>, bool) {
        // Askings out for this question; whether its time to ask again has
        // passed, and whether it was asked again (not if the other lane was
        // still out on an earlier question).
        let (mut out, mut tried_again, mut asked_again) = (1, false, false);
        let mut failed = None;
        loop {
            let wait_until = if tried_again {
                until
            } else {
                (started + self.after).min(until)
            };
            match self
                .answers
                .recv_timeout(wait_until.saturating_duration_since(Instant::now()))
            {
                Ok((lane, answered, answer)) => {
                    self.lanes[lane].busy = false;
                    if answered != number {
                        continue;
                    }
                    out -= 1;
                    match answer {
                        Ok(asked) => return (Ok(asked), Some(lane), asked_again),
                        Err(error) if out == 0 && (tried_again || failed.is_some()) => {
                            return (Err(failed.unwrap_or(error)), None, asked_again);
                        }
                        Err(error) if !tried_again => return (Err(error), None, asked_again),
                        Err(error) => failed = failed.or(Some(error)),
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < until && !tried_again => {
                    tried_again = true;
                    let other = 1 - first;
                    if !self.lanes[other].busy && self.send(other, job()) {
                        out += 1;
                        asked_again = true;
                    }
                }
                Err(_) => return (Err(failed.unwrap_or(Failed::TimedOut)), None, asked_again),
            }
        }
    }

    fn send(&mut self, lane: usize, job: Job) -> bool {
        let sent = self.lanes[lane].jobs.send(job).is_ok();
        self.lanes[lane].busy = sent;
        sent
    }
}

fn lane<D: Decider + Send + 'static>(
    index: usize,
    mut judge: D,
    answered: mpsc::Sender<(usize, u64, Result<Asked, Failed>)>,
) -> std::io::Result<Lane> {
    let (jobs, taken) = mpsc::channel::<Job>();
    let thread = thread::Builder::new()
        .name(format!("ask-{index}"))
        .spawn(move || {
            for job in taken {
                match job {
                    Job::Ready(within) => judge.ready(within),
                    Job::Ask {
                        number,
                        state,
                        questions,
                        deadline,
                    } => {
                        let answer = judge.ask(&state, &questions, deadline);
                        if answered.send((index, number, answer)).is_err() {
                            break;
                        }
                    }
                }
            }
        })?;
    Ok(Lane {
        jobs,
        busy: false,
        _thread: thread,
    })
}

impl Decider for Hedged {
    fn ask(
        &mut self,
        state: &str,
        questions: &[(String, Question)],
        deadline: Duration,
    ) -> Result<Asked, Failed> {
        let started = Instant::now();
        let until = started + deadline;
        self.number += 1;
        let number = self.number;
        let job = || Job::Ask {
            number,
            state: state.to_owned(),
            questions: questions.to_vec(),
            deadline: until.saturating_duration_since(Instant::now()),
        };
        let first = self.idle(until).ok_or(Failed::TimedOut)?;
        if !self.send(first, job()) {
            return Err(Failed::TimedOut);
        }
        let (result, answered_by, asked_again) = self.settle(first, number, started, until, &job);
        if asked_again {
            let outcome = match (&result, answered_by) {
                (Ok(_), Some(lane)) if lane == first => "the first asking answered first",
                (Ok(_), Some(_)) => "the second asking answered first",
                _ => "neither answered",
            };
            (self.say)(&format!(
                "decision asked again after {} ms; {outcome}, {} ms after the question",
                self.after.as_millis(),
                started.elapsed().as_millis()
            ));
        }
        result
    }

    fn ready(&mut self, within: Duration) {
        for lane in 0..self.lanes.len() {
            if !self.lanes[lane].busy {
                let _ = self.lanes[lane].jobs.send(Job::Ready(within));
            }
        }
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::Mutex;

    /// A judge that takes each scripted time in turn, then answers as itself
    /// (`None`) or fails as scripted; never past its deadline unless it is
    /// `stubborn`.
    struct Scripted {
        name: &'static str,
        takes: VecDeque<(u64, Option<Failed>)>,
        stubborn: bool,
    }

    impl Decider for Scripted {
        fn ask(
            &mut self,
            _state: &str,
            _questions: &[(String, Question)],
            deadline: Duration,
        ) -> Result<Asked, Failed> {
            let (take, fails) = self
                .takes
                .pop_front()
                .unwrap_or((0, Some(Failed::TimedOut)));
            let take = Duration::from_millis(take);
            thread::sleep(if self.stubborn {
                take
            } else {
                take.min(deadline)
            });
            if let Some(failed) = fails {
                return Err(failed);
            }
            if take > deadline && !self.stubborn {
                return Err(Failed::TimedOut);
            }
            Ok(Asked {
                answers: BTreeMap::new(),
                model: self.name.into(),
                took: take,
                usage: None,
                connection: None,
            })
        }

        fn name(&self) -> &str {
            self.name
        }
    }

    const AFTER: Duration = Duration::from_millis(150);
    const DEADLINE: Duration = Duration::from_secs(3);
    const ANSWERS: Option<Failed> = None;

    fn refused() -> Failed {
        Failed::Unreachable {
            during: "connecting",
            kind: std::io::ErrorKind::ConnectionRefused,
        }
    }

    fn judges(
        first: Vec<(u64, Option<Failed>)>,
        second: Vec<(u64, Option<Failed>)>,
        stubborn: bool,
    ) -> (Hedged, Arc<Mutex<Vec<String>>>) {
        let said = Arc::new(Mutex::new(Vec::new()));
        let journal = said.clone();
        let judges = [
            Scripted {
                name: "first",
                takes: first.into(),
                stubborn,
            },
            Scripted {
                name: "second",
                takes: second.into(),
                stubborn,
            },
        ];
        let say: Say = Arc::new(move |line: &str| journal.lock().unwrap().push(line.to_owned()));
        (Hedged::new(judges, AFTER, say).unwrap(), said)
    }

    fn hedged(
        first: Vec<(u64, Option<Failed>)>,
        second: Vec<(u64, Option<Failed>)>,
    ) -> (Hedged, Arc<Mutex<Vec<String>>>) {
        judges(first, second, false)
    }

    fn answered_by(answer: Result<Asked, Failed>) -> String {
        answer
            .map(|asked| asked.model)
            .unwrap_or_else(|failed| failed.to_string())
    }

    /// Answered within the time: one asking, nothing said.
    #[test]
    fn a_quick_answer_is_asked_once() {
        let (mut judge, said) = hedged(vec![(10, ANSWERS)], vec![(10, ANSWERS)]);
        assert_eq!(answered_by(judge.ask("{}", &[], DEADLINE)), "first");
        assert!(said.lock().unwrap().is_empty());
    }

    /// Slow past the time: asked again, the second answers first, and the
    /// journal says so, timed from the question.
    #[test]
    fn a_slow_answer_is_asked_again_and_the_first_to_answer_is_taken() {
        let (mut judge, said) = hedged(vec![(1_500, ANSWERS)], vec![(10, ANSWERS)]);
        let started = Instant::now();
        assert_eq!(answered_by(judge.ask("{}", &[], DEADLINE)), "second");
        assert!(started.elapsed() < Duration::from_millis(1_000));
        let said = said.lock().unwrap();
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].starts_with(
                "decision asked again after 150 ms; the second asking answered first, "
            ) && said[0].ends_with(" ms after the question"),
            "{said:?}"
        );
    }

    /// Asked again, but the first still answers first: it is taken.
    #[test]
    fn the_first_answer_in_wins_whichever_asking_it_was() {
        let (mut judge, said) = hedged(vec![(300, ANSWERS)], vec![(1_500, ANSWERS)]);
        assert_eq!(answered_by(judge.ask("{}", &[], DEADLINE)), "first");
        assert!(said.lock().unwrap()[0].contains("the first asking answered first"));
    }

    /// A first asking that fails before the time is the answer, as without
    /// the hedge; nothing is asked again.
    #[test]
    fn a_quick_failure_is_not_asked_again() {
        let (mut judge, said) = hedged(vec![(10, Some(refused()))], vec![(10, ANSWERS)]);
        assert_eq!(judge.ask("{}", &[], DEADLINE).unwrap_err(), refused());
        assert!(said.lock().unwrap().is_empty());
    }

    /// Once both are out, one that fails waits for the other; with both
    /// failed, the first failure is the answer, and the journal still says
    /// the second asking (the reviewer on #1629).
    #[test]
    fn with_both_out_a_failure_waits_for_the_other() {
        let (mut judge, _) = hedged(vec![(300, Some(refused()))], vec![(600, ANSWERS)]);
        assert_eq!(answered_by(judge.ask("{}", &[], DEADLINE)), "second");
        let (mut judge, said) = hedged(
            vec![(300, Some(refused()))],
            vec![(400, Some(Failed::TimedOut))],
        );
        assert_eq!(judge.ask("{}", &[], DEADLINE).unwrap_err(), refused());
        let said = said.lock().unwrap();
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("; neither answered, "), "{said:?}");
    }

    /// The late answer to one question is never handed to the next. The
    /// first lane is still out on question 1 when question 2 goes to the
    /// free lane; its late answer comes before question 2's, and is dropped.
    #[test]
    fn a_late_answer_is_dropped_not_given_to_the_next_question() {
        let (mut judge, said) = hedged(vec![(500, ANSWERS)], vec![(10, ANSWERS), (600, ANSWERS)]);
        assert_eq!(answered_by(judge.ask("{}", &[], DEADLINE)), "second");
        assert_eq!(answered_by(judge.ask("{}", &[], DEADLINE)), "second");
        // Question 2 found no free lane to ask again on.
        assert_eq!(said.lock().unwrap().len(), 1);
    }

    /// Neither in time, even from judges that run past their own deadline:
    /// the asking ends at the deadline with its failure, and the second
    /// asking is said (the reviewer on #1629).
    #[test]
    fn neither_in_time_is_the_deadlines_failure_at_the_deadline() {
        let (mut judge, said) = judges(vec![(2_000, ANSWERS)], vec![(2_000, ANSWERS)], true);
        let started = Instant::now();
        let answer = judge.ask("{}", &[], Duration::from_millis(600));
        let took = started.elapsed();
        assert_eq!(answer.unwrap_err(), Failed::TimedOut);
        assert!(
            took >= Duration::from_millis(600) && took < Duration::from_millis(900),
            "{took:?}"
        );
        assert!(said.lock().unwrap()[0].contains("; neither answered, "));
    }
}
