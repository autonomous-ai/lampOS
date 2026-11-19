//! The timeline (#5883's second piece): each turn as V1's Flow Monitor
//! showed it, heard, understood, did, said, joined by the request number
//! `voice/heard`, `voice/turn` and `voice/said` share. What it did is the
//! light asked for (`light/command`, when it changes) and the skills'
//! stages (`skills/events`) that come inside the turn. The tools the voice
//! called come with its decision (below).
//!
//! Words are kept only when the timeline is made with them, which only
//! monitor mode does: then the finished words heard and said, cut to
//! [`WORD_CHARS`], in memory only, the newest [`KEPT`] turns. Never a
//! skill's `why`. The page escapes everything it shows of it.
//!
//! #7267 adds what the lamp decided (`voice/decision`: the tools, or the
//! hand-over to an engine's task), what that task's agent did
//! (`tasks/step`), joined by the task's number, and which tasks settled
//! (`tasks/status`). A step's arguments and result are words: kept only
//! with words on. Tool names, numbers and codes are not words.
use crate::watch::plain;
use lamp_bus::Message;
use lamp_msgs::{
    AgentState, DecisionKind, Light, SkillEvent, SkillStage, SkillTask, SkillTaskStage, Speech,
    StepKind, TaskStatus, TaskStep, TurnVerdict, VoiceDecision, VoiceTurn, cut_whole_chars,
};
use std::collections::{BTreeMap, VecDeque};

/// The most turns kept.
pub const KEPT: usize = 30;
/// The most characters of words kept of a line heard or said.
pub const WORD_CHARS: usize = 300;
/// How long after its first step a turn takes what the lamp did, and a
/// step of the same request number: a number seen again later is another
/// turn (the voice counts again from its start).
pub const TURN_LASTS_MS: u64 = 60_000;
/// The topics only the timeline reads: not on `/bus`. One nobody
/// publishes (an older lamp) only leaves its part of a card empty.
pub const TOPICS: [&str; 6] = [
    Light::TOPICS[0],
    SkillEvent::TOPIC,
    VoiceDecision::TOPIC,
    TaskStep::TOPIC,
    TaskStatus::TOPIC,
    SkillTask::TOPIC,
];
/// The voice's tools that run a skill now, whose `do.task` the turn then
/// began (#7873): `make_skill`'s run of a made skill, `use_skill`'s.
pub const RUNS_A_SKILL: [&str; 2] = ["make_skill", "use_skill"];
/// The most steps kept of one task; later ones are only counted.
pub const MOST_STEPS: usize = 50;
/// The most bytes kept of a tool's or a code's name.
pub const NAME_BYTES: usize = 40;
/// The most bytes kept of a step's arguments or result, cut on a whole
/// character.
pub const STEP_WORD_BYTES: usize = 120;
/// The most tools kept of one decision.
pub const MOST_TOOLS: usize = 8;

/// What a task's agent did: its first [`MOST_STEPS`] steps by number; past
/// them, how many calls came and its newest reply or error (`end`), kept
/// so the card's status and ⏱ stay right; how the tasks part found it
/// settled, and when; and when the tasks part last listed it as going.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Agent {
    pub steps: Vec<TaskStep>,
    pub more: u32,
    pub end: Option<TaskStep>,
    /// The settled state (`idle`, `needs_input`, `stopped`, `failed`) and
    /// when, by the tasks part's clock.
    pub settled: Option<(AgentState, u64)>,
    /// When `tasks/status` last listed it working or unread, by the bus.
    pub seen_unix_ms: u64,
}

impl Agent {
    /// Its steps and, past them, its kept end.
    fn all(&self) -> impl Iterator<Item = &TaskStep> {
        self.steps.iter().chain(&self.end)
    }

    /// Whether the agent is done: it replied, failed, or settled.
    pub fn ended(&self) -> bool {
        self.settled.is_some() || self.all().any(|step| step.kind != StepKind::Call)
    }

    /// Whether it failed: an error step, or settled `failed`.
    pub fn failed(&self) -> bool {
        matches!(self.settled, Some((AgentState::Failed, _)))
            || self.all().any(|step| step.kind == StepKind::Error)
    }

    /// When it ended: its last reply or error, or its settling.
    pub fn ended_at(&self) -> Option<u64> {
        self.all()
            .filter(|step| step.kind != StepKind::Call)
            .map(|step| step.at_unix_ms)
            .chain(self.settled.map(|(_, at)| at))
            .max()
    }

    /// Its newest sign of life: a step, its settling, or a listing.
    pub fn last_heard(&self) -> u64 {
        self.all()
            .map(|step| step.at_unix_ms)
            .chain(self.settled.map(|(_, at)| at))
            .fold(self.seen_unix_ms, u64::max)
    }
}

/// A skill's `do.task` run, or a skill's answer at its gate, as
/// `skills/tasks` told it (#7873): lamp-tasks' run of it, which
/// `tasks/status` never lists, and lamp-skills' gate.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkillRun {
    pub skill: String,
    /// lamp-tasks' id of the run; none for an answer with no run kept (a
    /// read's).
    pub run: Option<String>,
    /// The engine it began on, as `tasks/status` names engines.
    pub engine: Option<String>,
    /// When its first news came, by the bus.
    pub at_unix_ms: u64,
    /// How the run ended, `ran` or `failed`, and when.
    pub ended: Option<(SkillTaskStage, u64)>,
    /// Why its answer waits for its gate: the newest reason.
    pub held: Option<String>,
    /// Its answer told or dropped, and when.
    pub answer: Option<(SkillTaskStage, u64)>,
}

/// One step of a turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// The words heard, finished; the words only with words on.
    Heard(Option<String>),
    /// The voice's verdict: answered, or held back.
    Understood(TurnVerdict),
    /// What the voice decided (#7267).
    Decided(VoiceDecision),
    /// What the lamp did: `light blue at 3`, `skill wave done`.
    Did(String),
    /// The words said, finished; the words only with words on.
    Said(Option<String>),
}

/// One turn, as far as the bus has told it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub request: u64,
    /// Each step with its wall clock time, as it came.
    pub steps: Vec<(u64, Step)>,
    /// When it ended: said, or held back.
    pub ended_at: Option<u64>,
    /// What the agent of the task it handed over did, as
    /// [`Timeline::newest`] joins it by the task's number.
    pub agent: Option<Agent>,
    /// The skill's `do.task` run it began (#7873), as [`Timeline::newest`]
    /// joins it ([`RUNS_A_SKILL`]).
    pub skill_run: Option<SkillRun>,
}

impl Entry {
    /// Its decisions, oldest first.
    fn decisions(&self) -> impl DoubleEndedIterator<Item = &VoiceDecision> {
        let mut decisions: Vec<(u64, &VoiceDecision)> = self
            .steps
            .iter()
            .filter_map(|(at, step)| match step {
                Step::Decided(decision) => Some((*at, decision)),
                _ => None,
            })
            .collect();
        decisions.sort_by_key(|(at, _)| *at);
        decisions.into_iter().map(|(_, decision)| decision)
    }

    /// Its newest decision: the latest of a request wins.
    pub fn decision(&self) -> Option<&VoiceDecision> {
        self.decisions().next_back()
    }

    /// ⚡: the newest decision's first reply, words heard to the answer's
    /// first audio. It may come in a later decision than the path's.
    pub fn first_reply_ms(&self) -> Option<u64> {
        self.decisions()
            .rev()
            .find_map(|decision| decision.first_reply_ms)
            .map(u64::from)
    }

    /// Its first step's time.
    pub fn start(&self) -> u64 {
        self.steps
            .iter()
            .map(|(at, _)| *at)
            .min()
            .unwrap_or_default()
    }

    /// When the speech ended: the words heard, finished; or, for a turn the
    /// lamp began itself, its first step.
    pub fn speech_ended(&self) -> u64 {
        self.steps
            .iter()
            .find_map(|(at, step)| matches!(step, Step::Heard(_)).then_some(*at))
            .unwrap_or_else(|| self.start())
    }

    /// Whether `at` falls inside the turn.
    fn holds(&self, at: u64) -> bool {
        let start = self.start();
        at >= start && at - start < TURN_LASTS_MS && self.ended_at.is_none_or(|ended| at <= ended)
    }
}

/// The newest turns, and the light last asked for.
#[derive(Debug, Default)]
pub struct Timeline {
    words: bool,
    /// Oldest first.
    entries: VecDeque<Entry>,
    light: Option<Light>,
    /// The tasks' agents, by task number: the newest [`KEPT`].
    tasks: BTreeMap<u64, Agent>,
    /// The skills' runs and answers (#7873), oldest first: the newest
    /// [`KEPT`].
    skill_runs: VecDeque<SkillRun>,
}

impl Timeline {
    /// A timeline that keeps the words heard and said only when `words`.
    pub fn new(words: bool) -> Self {
        Self {
            words,
            ..Self::default()
        }
    }

    pub fn words(&self) -> bool {
        self.words
    }

    /// The turns kept, newest first, each with its task's agent.
    pub fn newest(&self) -> Vec<Entry> {
        self.entries
            .iter()
            .rev()
            .map(|entry| {
                let mut entry = entry.clone();
                entry.agent = entry
                    .decision()
                    .and_then(|decision| decision.task)
                    .and_then(|task| self.tasks.get(&task))
                    .cloned();
                entry.skill_run = self.run_begun_by(&entry);
                entry
            })
            .collect()
    }

    /// The skill's run `entry` began (#7873): a turn whose decision called
    /// one of [`RUNS_A_SKILL`] begins the newest run lamp-tasks began
    /// within its minute ([`TURN_LASTS_MS`] from its first step). No
    /// number joins them: lamp-skills runs the skill and lamp-tasks its
    /// task, neither knowing the turn.
    // ponytail: joined by time, so a trigger's run begun in that minute
    // would be taken for the turn's; carry the request in `skills/tasks`
    // if that is ever seen.
    fn run_begun_by(&self, entry: &Entry) -> Option<SkillRun> {
        let called = entry.decision().is_some_and(|decision| {
            decision.kind == DecisionKind::Answered
                && decision
                    .tools
                    .iter()
                    .any(|tool| RUNS_A_SKILL.contains(&tool.as_str()))
        });
        if !called {
            return None;
        }
        let start = entry.start();
        self.skill_runs
            .iter()
            .rev()
            .find(|run| {
                run.run.is_some()
                    && run.at_unix_ms >= start
                    && run.at_unix_ms - start < TURN_LASTS_MS
            })
            .cloned()
    }

    /// The skills' runs and answers kept, newest first (#7873).
    pub fn skill_runs(&self) -> Vec<SkillRun> {
        self.skill_runs.iter().rev().cloned().collect()
    }

    /// What `skills/tasks` said at `at` (#7873), when it checks: a run
    /// begun is a new one; its end goes to the run of its id; a hold, told
    /// or dropped to the skill's newest whose answer has not ended. One
    /// found nowhere is a new one: a read's answer has no run.
    pub fn skill_task(&mut self, news: SkillTask, at: u64) {
        if news.check().is_err() {
            return;
        }
        let found = match news.stage {
            SkillTaskStage::Started => None,
            SkillTaskStage::Ran | SkillTaskStage::Failed => self
                .skill_runs
                .iter()
                .rposition(|kept| kept.run.is_some() && kept.run == news.run),
            SkillTaskStage::Held | SkillTaskStage::Told | SkillTaskStage::Dropped => self
                .skill_runs
                .iter()
                .rposition(|kept| kept.skill == news.skill && kept.answer.is_none()),
        };
        let index = match found {
            Some(index) => index,
            None => {
                self.skill_runs.push_back(SkillRun {
                    skill: news.skill.clone(),
                    run: news.run.clone(),
                    at_unix_ms: at,
                    ..SkillRun::default()
                });
                while self.skill_runs.len() > KEPT {
                    self.skill_runs.pop_front();
                }
                self.skill_runs.len() - 1
            }
        };
        let kept = &mut self.skill_runs[index];
        match news.stage {
            SkillTaskStage::Started => kept.engine = news.engine,
            SkillTaskStage::Ran | SkillTaskStage::Failed => kept.ended = Some((news.stage, at)),
            SkillTaskStage::Held => kept.held = news.gate,
            SkillTaskStage::Told | SkillTaskStage::Dropped => kept.answer = Some((news.stage, at)),
        }
    }

    /// The task's agent, within [`KEPT`] tasks: the lowest numbers go.
    fn agent(&mut self, task: u64) -> &mut Agent {
        if !self.tasks.contains_key(&task) {
            while self.tasks.len() >= KEPT {
                self.tasks.pop_first();
            }
        }
        self.tasks.entry(task).or_default()
    }

    /// A step of a task's agent: its words only with words on, each part
    /// cut; one of the same number replaces it. Past [`MOST_STEPS`] a call
    /// is only counted, and a reply or error is kept as its end (the
    /// newest), so the card still knows how it ended.
    pub fn step(&mut self, mut step: TaskStep) {
        step.tool = cut_whole_chars(&step.tool, NAME_BYTES).to_owned();
        step.engine = plain(&step.engine);
        if self.words {
            step.args = cut_whole_chars(&step.args, STEP_WORD_BYTES).to_owned();
            step.result = cut_whole_chars(&step.result, STEP_WORD_BYTES).to_owned();
        } else {
            step.args.clear();
            step.result.clear();
        }
        let agent = self.agent(step.task);
        match agent.steps.binary_search_by_key(&step.n, |kept| kept.n) {
            Ok(at) => agent.steps[at] = step,
            Err(at) if agent.steps.len() < MOST_STEPS => agent.steps.insert(at, step),
            Err(_) if step.kind == StepKind::Call => agent.more += 1,
            Err(_) => {
                if agent.end.as_ref().is_none_or(|end| end.n <= step.n) {
                    agent.end = Some(step);
                }
            }
        }
    }

    /// Take what `message` says of a turn, if anything.
    pub fn take(&mut self, message: &Message) {
        let at = message.header.at_unix_ms;
        let topic = message.header.topic.as_str();
        if Speech::TOPICS.contains(&topic) {
            let Ok(speech) = message.parse::<Speech>() else {
                return;
            };
            let (true, Some(request)) = (speech.finished, speech.request) else {
                return;
            };
            let words = self.words.then(|| cut(&speech.text));
            let step = if topic == Speech::TOPICS[0] {
                Step::Heard(words)
            } else {
                Step::Said(words)
            };
            self.add(request, at, step);
        } else if topic == VoiceDecision::TOPIC {
            if let Ok(mut decision) = message.parse::<VoiceDecision>() {
                decision.tools.truncate(MOST_TOOLS);
                for tool in &mut decision.tools {
                    *tool = cut_whole_chars(tool, NAME_BYTES).to_owned();
                }
                decision.engine = decision.engine.as_deref().map(plain);
                decision.error = decision.error.as_deref().map(plain);
                self.add(decision.request, at, Step::Decided(decision));
            }
        } else if topic == TaskStep::TOPIC {
            if let Ok(step) = message.parse::<TaskStep>() {
                self.step(step);
            }
        } else if topic == TaskStatus::TOPIC {
            // Published once a second while the tasks part runs: a task it
            // lists is going, one it lets go has settled.
            if let Ok(status) = message.parse::<TaskStatus>() {
                let going = status.tasks.iter().map(|progress| progress.task);
                for task in going.chain(status.unread.iter().copied()) {
                    self.agent(task).seen_unix_ms = at;
                }
                for settled in status.settled {
                    self.agent(settled.task).settled = Some((settled.state, settled.at_unix_ms));
                }
            }
        } else if topic == VoiceTurn::TOPIC {
            if let Ok(turn) = message.parse::<VoiceTurn>() {
                self.add(turn.request, at, Step::Understood(turn.verdict));
            }
        } else if topic == Light::TOPICS[0] {
            // Asked for again each second: only a change is a thing done.
            let Ok(light) = message.parse::<Light>() else {
                return;
            };
            if self.light.as_ref() == Some(&light) {
                return;
            }
            let did = if light.on {
                format!("light {} at {}", plain(&light.colour), light.level)
            } else {
                "light off".to_owned()
            };
            self.light = Some(light);
            self.did(at, did);
        } else if topic == SkillTask::TOPIC {
            if let Ok(news) = message.parse::<SkillTask>() {
                self.skill_task(news, at);
            }
        } else if topic == SkillEvent::TOPIC
            && let Ok(event) = message.parse::<SkillEvent>()
        {
            let stage = match (event.stage, event.allowed) {
                (SkillStage::Triggered, _) => "triggered",
                (SkillStage::Asked, _) => "asked",
                (SkillStage::Answered, Some(false)) => "not allowed",
                (SkillStage::Answered, _) => "allowed",
                (SkillStage::Done, _) => "done",
            };
            self.did(at, format!("skill {} {stage}", plain(&event.skill)));
        }
    }

    /// A step of `request` at `at`: of its turn, or of a new one.
    pub fn add(&mut self, request: u64, at: u64, step: Step) {
        let ends = matches!(
            step,
            Step::Said(_) | Step::Understood(TurnVerdict::HeldBack)
        );
        let found =
            self.entries.iter_mut().rev().find(|entry| {
                entry.request == request && at.abs_diff(entry.start()) < TURN_LASTS_MS
            });
        let entry = match found {
            Some(entry) => entry,
            None => {
                self.entries.push_back(Entry {
                    request,
                    steps: Vec::new(),
                    ended_at: None,
                    agent: None,
                    skill_run: None,
                });
                while self.entries.len() > KEPT {
                    self.entries.pop_front();
                }
                self.entries.back_mut().expect("just pushed")
            }
        };
        entry.steps.push((at, step));
        if ends {
            entry.ended_at = Some(entry.ended_at.map_or(at, |ended| ended.max(at)));
        }
    }

    /// What the lamp did at `at`, in the newest turn that holds it.
    pub fn did(&mut self, at: u64, what: String) {
        if let Some(entry) = self.entries.iter_mut().rev().find(|entry| entry.holds(at)) {
            entry.steps.push((at, Step::Did(what)));
        }
    }
}

/// `text` cut to [`WORD_CHARS`], with a mark when it was.
fn cut(text: &str) -> String {
    let mut chars = text.chars();
    let mut cut: String = chars.by_ref().take(WORD_CHARS).collect();
    if chars.next().is_some() {
        cut.push('…');
    }
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_thirty_turns_are_kept() {
        let mut timeline = Timeline::new(false);
        for request in 1..=40 {
            timeline.add(request, request * 10, Step::Heard(None));
        }
        let newest = timeline.newest();
        assert_eq!(newest.len(), KEPT);
        assert_eq!(newest[0].request, 40);
        assert_eq!(newest[KEPT - 1].request, 11);
    }

    #[test]
    fn what_the_lamp_did_goes_to_the_turn_that_holds_it() {
        let mut timeline = Timeline::new(false);
        timeline.did(500, "light red at 2".into());
        timeline.add(4, 1_000, Step::Heard(None));
        timeline.did(1_200, "light blue at 3".into());
        timeline.add(4, 1_500, Step::Said(None));
        timeline.did(2_000, "light off".into());
        let newest = timeline.newest();
        assert_eq!(
            newest[0].steps,
            vec![
                (1_000, Step::Heard(None)),
                (1_200, Step::Did("light blue at 3".into())),
                (1_500, Step::Said(None)),
            ]
        );
        // A turn held back ends there too.
        timeline.add(5, 3_000, Step::Understood(TurnVerdict::HeldBack));
        timeline.did(3_100, "skill wave done".into());
        assert_eq!(timeline.newest()[0].steps.len(), 1);
    }

    #[test]
    fn a_number_seen_again_much_later_is_another_turn() {
        let mut timeline = Timeline::new(false);
        timeline.add(1, 1_000, Step::Heard(None));
        timeline.add(1, 1_000 + TURN_LASTS_MS, Step::Heard(None));
        assert_eq!(timeline.newest().len(), 2);
    }

    #[test]
    fn words_are_cut() {
        let long = "a".repeat(WORD_CHARS + 5);
        let cut = cut(&long);
        assert_eq!(cut.chars().count(), WORD_CHARS + 1);
        assert!(cut.ends_with('…'));
    }
}
