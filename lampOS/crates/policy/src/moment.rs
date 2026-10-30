//! The decision "is this a good moment for the lamp to start this itself?"
//! (`docs/policy.md`, "Asked by a skill: the gate"), for a skill's ask whose
//! act is `news` (news of work) or `unprompted` (anything else the lamp
//! would start: a stretch break, stuffy air), #939. A `reminder` the person
//! set is decided here too, by the hard rule alone, with no model asked
//! ([`REMINDER`]). A hello is the hello question's ([`crate::hello`]).
//!
//! One hard rule sits before the decider: never while anyone is talking to
//! the lamp (the voice listening, thinking or speaking, or anything heard
//! or said in the last 15 s, as the hello question knows it). While it
//! holds the answer is `wait`, by the rule, and no model is asked. With
//! `lamp-policy --rules` a second holds as it does: a rule the people at the
//! lamp kept about it speaking first ([`crate::rules`], #813), `news` by its
//! `news` limits and anything else by the limits of either kind. Then one
//! question, `good_moment`, from the progress question's own, put to the
//! decision model on a connection of its own; with no answer in time, a
//! rule. The answer is published for the skill to act on (`shadow`
//! false), naming the ask.
//!
//! With `--addons` (#2530 part 2) an add-on's say past the add-ons' judge
//! (`addon/ask`) is put to the same question as something unprompted, shown
//! only the add-on's name and why, never its words; its decision has `about`
//! `addon` and names the `proposal`, and is shadow unless `--act addon`.
use crate::{
    attention::Room,
    decide::{Answer, Asked, Connection, Decider, Failed, Question, Usage},
    hello::Hold,
    progress::{Choice, LocalTime},
    rules::{Kept, Kind},
};
use lamp_msgs::{AddonAsk, Decision, Rules, SkillAsk};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    io,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

/// What this decision is called on the bus and in the log.
pub const ABOUT: &str = "moment";
/// From this probability up, the lamp may.
pub const SAY_FROM: f32 = 0.5;
/// The most one asking may take: the asker waits about 2 s.
pub const MOST_ASKING: Duration = Duration::from_millis(1_500);
/// An ask not decided within this long is let go undecided: its asker has
/// stopped waiting.
pub const ASKER_WAITS: Duration = Duration::from_secs(2);
/// No more asks than this wait to be decided.
const MOST_WAITING: usize = 4;
/// What the rule gives when no model answers. Authored, not measured.
pub const RULE_YES: f32 = 0.6;
pub const RULE_NO: f32 = 0.1;
/// The act of a reminder the person set, now due (the Reminders skill,
/// #939): not the lamp's to time. Only someone talking to the lamp holds
/// it; no model is asked, and neither the hour, the room, how often the
/// lamp spoke up nor a rule kept about speaking first count, since the
/// person asked for it. One waiting is decided at once, even while a
/// question is out.
pub const REMINDER: &str = "reminder";

/// How the question is asked (`lamp-policy --gate`).
#[derive(Clone, Debug)]
pub struct Options {
    /// How long one asking may take: at most [`MOST_ASKING`].
    pub deadline: Duration,
    /// The lamp's local time, for the hour of the day: a fixed offset
    /// (`--utc-offset-minutes`) or a zone (`--zone`, #820), as the hello
    /// question's.
    pub local: Option<Arc<LocalTime>>,
    /// An add-on's say is answered for real (`--act addon`); else shadow.
    pub act_addon: bool,
}

impl Options {
    pub fn new(deadline: Duration, local: Option<Arc<LocalTime>>) -> Self {
        Self {
            deadline: deadline.min(MOST_ASKING),
            local,
            act_addon: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Judgement {
    pub probability: f32,
    pub choice: Choice,
    /// No model's answers are behind it: a rule decided.
    pub from_rule: bool,
}

/// Everything known when the question was put: the ask, and the room as the
/// policy knows it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Context {
    pub ask: u64,
    pub skill: String,
    pub act: String,
    /// Whether the eyes see someone near; not known without a list of faces
    /// younger than 2 s.
    pub someone_here: Option<bool>,
    /// Seconds since anything was said to or by the lamp.
    pub quiet_s: u32,
    pub room: Room,
    pub spoke_up_last_hour: u32,
    pub hour: Option<u8>,
    /// For an add-on's say: why it suggests it ([`lamp_msgs::ADDON_WHYS`]).
    /// Its words are never here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub addon_why: Option<String>,
}

const LAMP: &str = "A small desk lamp robot on the desk of the person it works for: it keeps quiet unless there is a good reason, and when it starts something itself it is brief.";

fn room_told(room: Room) -> &'static str {
    match room {
        Room::Quiet => "quiet: nobody else has been talking",
        Room::Busy => "busy: people are talking to each other",
        Room::Unknown => "unknown",
    }
}

fn what_told(context: &Context) -> String {
    if let Some(why) = &context.addon_why {
        return format!("say one short line an add-on on the lamp suggests, about: {why}");
    }
    match context.act.as_str() {
        "news" => "tell one short piece of news about work it handed to a coding agent",
        REMINDER => "say a reminder the person set, now due",
        _ => "start something small itself, such as inviting a stretch or noting the air",
    }
    .to_owned()
}

/// The state as the text that is sent, its fields in this order. The
/// skill's name is a folder's name, not anything a person said.
pub fn state_text(context: &Context) -> String {
    format!(
        "{{\"lamp\":{},\"what\":{{\"skill\":{},\"the_lamp_would\":{}}},\"room\":{{\
         \"someone_at_the_lamp\":{},\"seconds_since_anything_was_said\":{},\"room\":{},\
         \"times_the_lamp_spoke_up_unasked_in_the_last_hour\":{},\"hour_of_day\":{}}}}}",
        json!(LAMP),
        json!(context.skill),
        json!(what_told(context)),
        json!(context.someone_here),
        context.quiet_s,
        json!(room_told(context.room)),
        context.spoke_up_last_hour,
        json!(context.hour),
    )
}

/// [`state_text`] as a value, for the log.
pub fn state(context: &Context) -> Value {
    serde_json::from_str(&state_text(context)).expect("the state is JSON")
}

const OPENING: &str = "state.what is something the lamp would start itself, unasked; state.room is how things are around the lamp now. ";

/// The one question: the progress question's `good_moment`, for anything
/// the lamp would start. Authored, not fitted.
const ASKED: (&str, &str) = (
    "good_moment",
    "Is this a good moment for the lamp to speak up unasked, briefly: someone is at the lamp, nobody has spoken for a little while, the room is not busy with other people's talk, the lamp has not spoken up unasked often in the last hour, and it is not late at night or early in the morning?",
);

/// The question, as it is asked.
pub fn questions() -> Vec<(String, Question)> {
    vec![(
        ASKED.0.to_owned(),
        Question::Noul {
            instructions: format!("{OPENING}{}", ASKED.1),
        },
    )]
}

/// The answer to `good_moment` is the probability; or the question's name,
/// if it is missing.
pub fn combine(
    answers: &BTreeMap<String, Answer>,
) -> Result<(Judgement, BTreeMap<String, f32>), &'static str> {
    let yes = match answers.get(ASKED.0) {
        Some(Answer::Noul(yes)) if (0.0..=1.0).contains(yes) => *yes,
        _ => return Err(ASKED.0),
    };
    Ok((
        Judgement {
            probability: yes,
            choice: if yes >= SAY_FROM {
                Choice::SayNow
            } else {
                Choice::Wait
            },
            from_rule: false,
        },
        BTreeMap::from([(ASKED.0.to_owned(), yes)]),
    ))
}

/// The judgement when no model answers: yes unless nobody is known to be
/// there, the room is known to be busy, the lamp spoke up unasked twice or
/// more in the hour, or it is before eight in the morning or after ten at
/// night (when the hour is known). For a reminder the person set: yes.
pub fn fallback(context: &Context) -> Judgement {
    // A reminder the person set: yes, whatever the room or the hour.
    let yes = context.act == REMINDER
        || (context.someone_here != Some(false)
            && context.room != Room::Busy
            && context.spoke_up_last_hour < 2
            && context.hour.is_none_or(|hour| (8..22).contains(&hour)));
    Judgement {
        probability: if yes { RULE_YES } else { RULE_NO },
        choice: if yes { Choice::SayNow } else { Choice::Wait },
        from_rule: true,
    }
}

/// One line of the decisions log for this decision.
#[derive(Clone, Debug, Serialize)]
pub struct Line {
    pub at_unix_ms: u64,
    pub about: &'static str,
    pub shadow: bool,
    pub context: Context,
    pub state: Value,
    pub asked: String,
    pub answers: BTreeMap<String, f32>,
    pub judgement: Judgement,
    pub by: String,
    pub took_ms: u32,
    /// A hard rule that answered `wait`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guard: Option<Hold>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<Connection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_why: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// A decision made: what goes on the bus, what is written down.
#[derive(Clone, Debug)]
pub struct Decided {
    pub decision: Decision,
    pub line: Line,
}

/// What was counted from start to stop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub decided: u64,
    pub by_rule: u64,
    pub said: u64,
    pub held_talking: u64,
    /// With `--rules`.
    pub held_rule: u64,
    /// Asks let go undecided: their askers had stopped waiting.
    pub late: u64,
}

type Answered = (Result<Asked, Failed>, Duration);

/// What the room is like now, as the policy and the hello question know it,
/// for a turn of the question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Now {
    pub room: Room,
    pub someone_here: Option<bool>,
    pub quiet_s: u32,
    pub talking: bool,
    pub spoke_up_last_hour: u32,
}

/// The question as it runs beside the policy's loop, with its own thread
/// and its own decision model.
pub struct Moment {
    options: Options,
    judge_name: String,
    states: mpsc::SyncSender<(String, Duration)>,
    answers: mpsc::Receiver<Answered>,
    asker_gone: bool,
    /// The question out: what it is about, when it was sent, and when its
    /// ask was received, which its 2 s count from.
    asking: Option<(Context, Instant, Instant)>,
    waiting: VecDeque<(SkillAsk, Instant)>,
    /// The asks waiting or out that are add-ons' says, by number, with why.
    addons: BTreeMap<u64, String>,
    /// The newest rules about speaking first, given with `--rules` only.
    rules: Kept,
    totals: Totals,
}

impl Moment {
    /// Start the thread that asks `decider`.
    pub fn start<D: Decider + Send + 'static>(
        mut decider: D,
        options: Options,
    ) -> io::Result<Self> {
        let judge_name = decider.name().to_owned();
        let (states, asked) = mpsc::sync_channel::<(String, Duration)>(1);
        let (answered, answers) = mpsc::channel();
        thread::Builder::new()
            .name("ask-moment".into())
            .spawn(move || {
                let questions = questions();
                // Each state with what is left of its asker's wait.
                for (state, deadline) in asked {
                    let started = Instant::now();
                    let answer = decider.ask(&state, &questions, deadline);
                    if answered.send((answer, started.elapsed())).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            options,
            judge_name,
            states,
            answers,
            asker_gone: false,
            asking: None,
            waiting: VecDeque::new(),
            addons: BTreeMap::new(),
            rules: Kept::default(),
            totals: Totals::default(),
        })
    }

    pub fn totals(&self) -> Totals {
        self.totals
    }

    /// Whether a question is out or an ask waits: its asker waits, so the
    /// loop looks again sooner.
    pub fn busy(&self) -> bool {
        self.asking.is_some() || !self.waiting.is_empty()
    }

    /// What the people at the lamp asked it about speaking first, newest
    /// (`--rules`): the hello question passes it on.
    pub fn rules(&mut self, rules: Rules) {
        self.rules.set(rules);
    }

    /// The hard rules, by how things are now: someone talking to the lamp,
    /// then a rule kept for what the ask would do. A reminder the person set
    /// is held by the first only: no kept rule about speaking first is
    /// about what the person asked the lamp to say (the reviewer on #1003).
    fn hold(&self, act: &str, room_now: Now, unix_ms: u64) -> Option<Hold> {
        let kind = if act == "news" {
            Kind::News
        } else {
            Kind::Unprompted
        };
        if room_now.talking {
            Some(Hold::Talking)
        } else if act == REMINDER {
            None
        } else if self.rules.hold(
            kind,
            unix_ms,
            // The zone's offset at this moment, as the progress question
            // takes it (#820).
            self.options
                .local
                .as_ref()
                .map(|local| local.offset_minutes_at(unix_ms)),
            room_now.room == Room::Quiet,
        ) {
            Some(Hold::KeptRule)
        } else {
            None
        }
    }

    /// A skill asks whether the lamp may start `news`, something
    /// `unprompted` or a `reminder` now. Any other ask is passed over. Past [`MOST_WAITING`]
    /// the oldest is let go.
    pub fn asked(&mut self, ask: &SkillAsk, now: Instant) {
        if ask.check().is_err() || !["news", REMINDER, "unprompted"].contains(&ask.act.as_str()) {
            return;
        }
        if self.waiting.len() == MOST_WAITING {
            self.waiting.pop_front();
            self.totals.late += 1;
        }
        self.waiting.push_back((ask.clone(), now));
    }

    /// An add-on's say past its judge (`addon/ask`, `--addons`): put to
    /// the question as something unprompted, its decision `about` `addon`.
    /// One that fails its check is passed over.
    pub fn asked_addon(&mut self, ask: &AddonAsk, now: Instant) {
        if ask.check().is_err() {
            return;
        }
        let unprompted = SkillAsk {
            ask: ask.proposal,
            skill: ask.addon.clone(),
            act: "unprompted".into(),
            person: None,
            away_min: None,
            there_s: None,
        };
        if self.waiting.len() == MOST_WAITING {
            self.waiting.pop_front();
            self.totals.late += 1;
        }
        self.waiting.push_back((unprompted, now));
        self.addons.insert(ask.proposal, ask.why.clone());
        // Only those still waiting or out are kept.
        let out = self.asking.as_ref().map(|(context, ..)| context.ask);
        let waiting: Vec<u64> = self.waiting.iter().map(|(ask, _)| ask.ask).collect();
        self.addons
            .retain(|number, _| Some(*number) == out || waiting.contains(number));
    }

    /// What is known of `ask` now.
    fn context(&self, ask: SkillAsk, room_now: Now, unix_ms: u64) -> Context {
        Context {
            ask: ask.ask,
            skill: ask.skill,
            act: ask.act,
            someone_here: room_now.someone_here,
            quiet_s: room_now.quiet_s,
            room: room_now.room,
            spoke_up_last_hour: room_now.spoke_up_last_hour,
            hour: self
                .options
                .local
                .as_ref()
                .map(|local| local.hour_of_day(unix_ms)),
            addon_why: self.addons.get(&ask.ask).cloned(),
        }
    }

    /// Take up an answer if one has come; else decide the next ask, by the
    /// hard rule or by putting the question. At most one decision a turn.
    pub fn turn(&mut self, now: Instant, unix_ms: u64, room_now: Now) -> Option<Decided> {
        // A reminder needs no model: one waiting is decided at once, by the
        // hard rule, even while a question is out or other asks wait before
        // it, so it is never let go late behind them (the reviewer on #1099).
        if let Some(index) = self.waiting.iter().position(|(ask, at)| {
            ask.act == REMINDER && now.saturating_duration_since(*at) < ASKER_WAITS
        }) {
            let (ask, _) = self.waiting.remove(index).expect("found above");
            let context = self.context(ask, room_now, unix_ms);
            let held = self.hold(&context.act, room_now, unix_ms);
            return Some(self.decide(context, None, held, Duration::ZERO, unix_ms));
        }
        if self.asking.is_some() {
            let (answer, took) = match self.answers.try_recv() {
                Ok((answer, took)) => Some((Some(answer), took)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.asker_gone = true;
                    Some((None, Duration::ZERO))
                }
            }?;
            let (context, sent, received) = self.asking.take().expect("a question is out");
            // An answer after its asker stopped waiting is let go, and
            // counted late, never published as decided (the reviewer on
            // #958, 12:19).
            if now.saturating_duration_since(received) >= ASKER_WAITS {
                self.totals.late += 1;
                return None;
            }
            let took = match answer {
                Some(_) => took,
                None => now.saturating_duration_since(sent),
            };
            // Someone began to talk while the model was asked, or while its
            // asker went away, or a rule was kept: the hard rules hold by
            // how things are now, never the fallback's `say_now` (Fourth on
            // #958).
            let held = self.hold(&context.act, room_now, unix_ms);
            return Some(self.decide(context, answer, held, took, unix_ms));
        }
        while self
            .waiting
            .front()
            .is_some_and(|(_, at)| now.saturating_duration_since(*at) >= ASKER_WAITS)
        {
            self.waiting.pop_front();
            self.totals.late += 1;
        }
        let (ask, received) = self.waiting.pop_front()?;
        let context = self.context(ask, room_now, unix_ms);
        // Never a reminder here: one within its 2 s was decided at once,
        // above, and a later one was dropped as late just now (the seventh
        // on #1143).
        debug_assert_ne!(context.act, REMINDER, "a reminder is decided at once");
        if let Some(hold) = self.hold(&context.act, room_now, unix_ms) {
            return Some(self.decide(context, None, Some(hold), Duration::ZERO, unix_ms));
        }
        if self.asker_gone {
            return Some(self.decide(context, None, None, Duration::ZERO, unix_ms));
        }
        // The model is given only what is left of the 2 s; with nothing
        // left, the rule answers now.
        let deadline = crate::hello::time_left(
            ASKER_WAITS,
            received,
            now,
            self.options.deadline.min(MOST_ASKING),
        );
        if deadline.is_zero() {
            let failed = Some(Err(Failed::TimedOut));
            return Some(self.decide(context, failed, None, Duration::ZERO, unix_ms));
        }
        match self.states.try_send((state_text(&context), deadline)) {
            Ok(()) => {
                self.asking = Some((context, now, received));
                None
            }
            Err(_) => {
                self.asker_gone = true;
                Some(self.decide(context, None, None, Duration::ZERO, unix_ms))
            }
        }
    }

    /// Decide from the model's answers, by the hard rule (`held`), or by the
    /// rule when there are no answers to use.
    fn decide(
        &mut self,
        context: Context,
        answer: Option<Result<Asked, Failed>>,
        held: Option<Hold>,
        took: Duration,
        unix_ms: u64,
    ) -> Decided {
        let connection = answer
            .as_ref()
            .and_then(|answer| answer.as_ref().ok())
            .and_then(|asked| asked.connection);
        let rule = || crate::policy::RULE.to_owned();
        let (mut judgement, answers, by, failed, usage) = match (held, answer) {
            (Some(_), _) => (
                Judgement {
                    probability: RULE_NO,
                    choice: Choice::Wait,
                    from_rule: true,
                },
                BTreeMap::new(),
                rule(),
                None,
                None,
            ),
            (None, Some(Ok(asked))) => match combine(&asked.answers) {
                Ok((judgement, answers)) => (judgement, answers, asked.model, None, asked.usage),
                Err(question) => (
                    fallback(&context),
                    BTreeMap::new(),
                    rule(),
                    Some(Failed::Missing(question.to_owned())),
                    asked.usage,
                ),
            },
            (None, Some(Err(failed))) => (
                fallback(&context),
                BTreeMap::new(),
                rule(),
                Some(failed),
                None,
            ),
            (None, None) => (
                fallback(&context),
                BTreeMap::new(),
                rule(),
                // A reminder is never asked about: nothing failed.
                (context.act != REMINDER).then_some(Failed::Unreachable {
                    during: "asking",
                    kind: io::ErrorKind::BrokenPipe,
                }),
                None,
            ),
        };
        match held {
            Some(Hold::Talking) => self.totals.held_talking += 1,
            Some(Hold::KeptRule) => self.totals.held_rule += 1,
            Some(Hold::GreetedRecently | Hold::GreetedPerson) | None => {}
        }
        if held.is_some() {
            judgement.choice = Choice::Wait;
        }
        if judgement.choice == Choice::SayNow {
            self.totals.said += 1;
        }
        self.totals.decided += 1;
        self.totals.by_rule += u64::from(judgement.from_rule);
        let took_ms = u32::try_from(took.as_millis()).unwrap_or(u32::MAX);
        let addon = context.addon_why.is_some();
        if addon {
            self.addons.remove(&context.ask);
        }
        let about = if addon { AddonAsk::ABOUT } else { ABOUT };
        let shadow = addon && !self.options.act_addon;
        let decision = Decision {
            about: about.to_owned(),
            choice: judgement.choice.as_str().to_owned(),
            probability: judgement.probability,
            answers: answers.clone(),
            by: by.clone(),
            took_ms,
            shadow,
            request: None,
            task: None,
            arrival: None,
            ask: (!addon).then_some(context.ask),
            proposal: addon.then_some(context.ask),
        };
        let line = Line {
            at_unix_ms: unix_ms,
            about,
            shadow,
            state: state(&context),
            context,
            asked: self.judge_name.clone(),
            answers,
            judgement,
            by,
            took_ms,
            guard: held,
            connection,
            failed: failed.as_ref().map(Failed::kind),
            failed_why: failed.as_ref().map(Failed::to_string),
            usage,
        };
        Decided { decision, line }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(act: &str) -> Context {
        Context {
            ask: 1_792_115_940_123_456,
            skill: "stretch-break".into(),
            act: act.into(),
            someone_here: Some(true),
            quiet_s: 340,
            room: Room::Quiet,
            spoke_up_last_hour: 0,
            hour: Some(14),
            addon_why: None,
        }
    }

    #[test]
    fn the_state_is_this_text_to_the_byte_and_holds_no_words_of_anyone() {
        assert_eq!(
            state_text(&context("unprompted")),
            "{\"lamp\":\"A small desk lamp robot on the desk of the person it works for: it keeps quiet unless there is a good reason, and when it starts something itself it is brief.\",\
             \"what\":{\"skill\":\"stretch-break\",\"the_lamp_would\":\"start something small itself, such as inviting a stretch or noting the air\"},\
             \"room\":{\"someone_at_the_lamp\":true,\"seconds_since_anything_was_said\":340,\"room\":\"quiet: nobody else has been talking\",\
             \"times_the_lamp_spoke_up_unasked_in_the_last_hour\":0,\"hour_of_day\":14}}"
        );
        assert!(state_text(&context("news")).contains("one short piece of news"));
        assert_eq!(questions().len(), 1);
    }

    #[test]
    fn good_moment_is_the_probability_and_the_rule_keeps_quiet_when_it_may_not_be() {
        let answers = BTreeMap::from([("good_moment".to_owned(), Answer::Noul(0.71))]);
        let (judgement, by_name) = combine(&answers).unwrap();
        assert_eq!(
            (
                judgement.choice,
                judgement.probability,
                by_name["good_moment"]
            ),
            (Choice::SayNow, 0.71, 0.71)
        );
        assert_eq!(combine(&BTreeMap::new()).unwrap_err(), "good_moment");
        assert_eq!(fallback(&context("news")).choice, Choice::SayNow);
        for not_now in [
            Context {
                someone_here: Some(false),
                ..context("news")
            },
            Context {
                room: Room::Busy,
                ..context("news")
            },
            Context {
                spoke_up_last_hour: 2,
                ..context("news")
            },
            Context {
                hour: Some(23),
                ..context("news")
            },
        ] {
            assert_eq!(fallback(&not_now).choice, Choice::Wait, "{not_now:?}");
        }
        // Not known is not no.
        assert_eq!(
            fallback(&Context {
                someone_here: None,
                hour: None,
                ..context("news")
            })
            .choice,
            Choice::SayNow
        );
    }

    /// Holds each asking until let go, and keeps the time each was given.
    struct Held {
        go: mpsc::Receiver<()>,
        given: std::sync::Arc<std::sync::Mutex<Vec<Duration>>>,
    }
    impl Decider for Held {
        fn ask(
            &mut self,
            _: &str,
            _: &[(String, Question)],
            deadline: Duration,
        ) -> Result<Asked, Failed> {
            self.given.lock().unwrap().push(deadline);
            self.go.recv().map_err(|_| Failed::TimedOut)?;
            Ok(Asked {
                answers: BTreeMap::from([("good_moment".to_owned(), Answer::Noul(0.9))]),
                model: "held".into(),
                took: Duration::ZERO,
                usage: None,
                connection: None,
            })
        }
        fn name(&self) -> &str {
            "held"
        }
    }

    /// The reviewer on #958 (12:19): an ask's 2 s count from when it was
    /// received. A, asked at once, is given 1.5 s and answered in time; B,
    /// received 0.1 s after A and taken up when A is decided at 1.5 s, is
    /// given what is left (0.4 s), and its answer at 2.2 s is let go and
    /// counted late, never published. One with too little left is answered
    /// by the rule at once.
    #[test]
    fn an_asks_two_seconds_count_from_its_receipt_and_a_late_answer_is_let_go() {
        let ms = Duration::from_millis;
        let (go, gate) = mpsc::channel();
        let given = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let decider = Held {
            go: gate,
            given: given.clone(),
        };
        let mut moment =
            Moment::start(decider, Options::new(Duration::from_secs(3), None)).unwrap();
        let ask = |number: u64| SkillAsk {
            ask: number,
            skill: "stretch-break".into(),
            act: "unprompted".into(),
            person: None,
            away_min: None,
            there_s: None,
        };
        let room_now = Now {
            room: Room::Quiet,
            someone_here: Some(true),
            quiet_s: 300,
            talking: false,
            spoke_up_last_hour: 0,
        };
        // What the asker was given, once it has taken up `count` askings.
        let given_now = |count: usize| {
            for _ in 0..500 {
                if given.lock().unwrap().len() >= count {
                    break;
                }
                std::thread::sleep(ms(2));
            }
            given.lock().unwrap().clone()
        };
        let t = Instant::now();
        moment.asked(&ask(1), t);
        moment.asked(&ask(2), t + ms(100));
        assert!(moment.turn(t, 1_000_000, room_now).is_none(), "A is out");
        go.send(()).unwrap();
        let a = (0..500)
            .find_map(|_| {
                std::thread::sleep(ms(2));
                moment.turn(t + ms(1_500), 1_000_000, room_now)
            })
            .expect("A decided");
        assert_eq!(
            (a.decision.ask, a.decision.choice.as_str()),
            (Some(1), "say_now")
        );
        assert!(
            moment.turn(t + ms(1_500), 1_000_000, room_now).is_none(),
            "B is out"
        );
        assert_eq!(given_now(2), [MOST_ASKING, ms(400)]);
        go.send(()).unwrap();
        for _ in 0..500 {
            if moment.totals().late == 1 {
                break;
            }
            let late = moment.turn(t + ms(2_200), 1_000_000, room_now);
            assert!(late.is_none(), "B published late");
            std::thread::sleep(ms(2));
        }
        assert_eq!((moment.totals().late, moment.totals().decided), (1, 1));
        // Received at 3 s and taken up at 4.85 s: the rule, at once.
        moment.asked(&ask(3), t + ms(3_000));
        let ruled = moment
            .turn(t + ms(4_850), 1_000_000, room_now)
            .expect("the rule at once");
        assert_eq!(
            (ruled.decision.ask, ruled.decision.by.as_str()),
            (Some(3), crate::policy::RULE)
        );
        assert_eq!(ruled.line.failed, Some("timed_out"));
        assert_eq!(given_now(2).len(), 2);
    }
    /// Its thread ends at the first asking, as a model's client that failed
    /// hard would.
    struct Gone;
    impl Decider for Gone {
        fn ask(&mut self, _: &str, _: &[(String, Question)], _: Duration) -> Result<Asked, Failed> {
            panic!("this test's asker is gone, on purpose");
        }
        fn name(&self) -> &str {
            "gone"
        }
    }

    /// Fourth on #958: an asker gone while someone talks is held by the hard
    /// rule, as an answer is; the fallback's `say_now` is never taken then.
    #[test]
    fn an_asker_gone_while_someone_talks_is_held_by_the_hard_rule() {
        let mut moment = Moment::start(Gone, Options::new(Duration::from_secs(3), None)).unwrap();
        let quiet = Now {
            room: Room::Quiet,
            someone_here: Some(true),
            quiet_s: 300,
            talking: false,
            spoke_up_last_hour: 0,
        };
        // With nobody talking, the fallback would say now.
        assert_eq!(fallback(&context("unprompted")).choice, Choice::SayNow);
        let t = Instant::now();
        assert!(!moment.busy());
        moment.asked(
            &SkillAsk {
                ask: 1,
                skill: "stretch-break".into(),
                act: "unprompted".into(),
                person: None,
                away_min: None,
                there_s: None,
            },
            t,
        );
        assert!(moment.busy());
        assert!(moment.turn(t, 1_000_000, quiet).is_none(), "out");
        let talking = Now {
            talking: true,
            ..quiet
        };
        let held = (0..500)
            .find_map(|_| {
                std::thread::sleep(Duration::from_millis(2));
                moment.turn(t + Duration::from_millis(100), 1_000_000, talking)
            })
            .expect("decided once the asker went");
        assert_eq!(
            (
                held.decision.choice.as_str(),
                held.decision.by.as_str(),
                held.line.guard
            ),
            ("wait", crate::policy::RULE, Some(Hold::Talking))
        );
        assert_eq!(moment.totals().held_talking, 1);
        assert!(!moment.busy());
    }

    /// #939: a reminder the person set is the hard rule's alone: `say_now`
    /// at once with nobody there, a busy room and the lamp having spoken up
    /// three times, no model asked and nothing failed; `wait` only while
    /// someone talks to the lamp. Its rule holds at any hour.
    #[test]
    fn a_reminder_is_said_by_the_rule_alone_and_held_only_while_someone_talks() {
        let (_go, gate) = mpsc::channel();
        let given = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let decider = Held {
            go: gate,
            given: given.clone(),
        };
        let mut moment =
            Moment::start(decider, Options::new(Duration::from_secs(3), None)).unwrap();
        let ask = |number: u64| SkillAsk {
            ask: number,
            skill: "reminders".into(),
            act: REMINDER.into(),
            person: None,
            away_min: None,
            there_s: None,
        };
        let anything_but_talk = Now {
            room: Room::Busy,
            someone_here: Some(false),
            quiet_s: 0,
            talking: false,
            spoke_up_last_hour: 3,
        };
        let t = Instant::now();
        moment.asked(&ask(1), t);
        let said = moment
            .turn(t, 1_000_000, anything_but_talk)
            .expect("decided at once");
        assert_eq!(
            (
                said.decision.ask,
                said.decision.choice.as_str(),
                said.decision.by.as_str(),
                said.line.failed
            ),
            (Some(1), "say_now", crate::policy::RULE, None)
        );
        moment.asked(&ask(2), t);
        let talking = Now {
            talking: true,
            ..anything_but_talk
        };
        let held = moment.turn(t, 1_000_000, talking).expect("decided at once");
        assert_eq!(
            (held.decision.choice.as_str(), held.line.guard),
            ("wait", Some(Hold::Talking))
        );
        assert!(given.lock().unwrap().is_empty(), "no model asked");
        let totals = moment.totals();
        assert_eq!(
            (totals.decided, totals.said, totals.held_talking),
            (2, 1, 1)
        );
        let late = Context {
            act: REMINDER.into(),
            hour: Some(23),
            ..context("news")
        };
        assert_eq!(fallback(&late).choice, Choice::SayNow);
    }

    /// Says yes, and counts what it was asked.
    struct Counted(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl Decider for Counted {
        fn ask(&mut self, _: &str, _: &[(String, Question)], _: Duration) -> Result<Asked, Failed> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Asked {
                answers: BTreeMap::from([("good_moment".to_owned(), Answer::Noul(0.9))]),
                model: "counted".into(),
                took: Duration::ZERO,
                usage: None,
                connection: None,
            })
        }
        fn name(&self) -> &str {
            "counted"
        }
    }

    /// #813 with #939: a rule kept about speaking first holds a skill's
    /// `news` by its `news` limits and anything else unprompted by the
    /// limits of either kind, by the rule, with no model asked.
    #[test]
    fn a_kept_rule_holds_news_by_its_own_limits_and_anything_else_by_either() {
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let options = Options::new(Duration::from_secs(1), None);
        let mut moment = Moment::start(Counted(asked.clone()), options).unwrap();
        let room_now = Now {
            room: Room::Quiet,
            someone_here: Some(true),
            quiet_s: 300,
            talking: false,
            spoke_up_last_hour: 0,
        };
        let t = Instant::now();
        let decide = |moment: &mut Moment, ask: u64, act: &str| {
            moment.asked(
                &SkillAsk {
                    ask,
                    skill: "stretch-break".into(),
                    act: act.into(),
                    person: None,
                    away_min: None,
                    there_s: None,
                },
                t,
            );
            let decided = (0..500)
                .find_map(|_| {
                    let decided = moment.turn(t, 1_000_000, room_now);
                    if decided.is_none() {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    decided
                })
                .expect("a decision");
            (
                decided.decision.choice,
                decided.decision.by,
                decided.line.guard,
            )
        };
        let rule = || (crate::policy::RULE.to_owned(), Some(Hold::KeptRule));
        let rules = |hello: lamp_msgs::Unprompted, news: lamp_msgs::Unprompted| Rules {
            person: "everyone".into(),
            quiet: false,
            quiet_mornings: false,
            rules: 1,
            hello,
            news,
            keep_light_on: false,
        };
        // No news before nine: with the lamp's time not known it holds all
        // day, news and anything else alike.
        moment.rules(rules(
            lamp_msgs::Unprompted::default(),
            lamp_msgs::Unprompted {
                not_before_minute: Some(9 * 60),
                ..lamp_msgs::Unprompted::default()
            },
        ));
        for (ask, act) in [(1, "news"), (2, "unprompted")] {
            let (choice, by, guard) = decide(&mut moment, ask, act);
            assert_eq!((choice.as_str(), (by, guard)), ("wait", rule()), "{act}");
        }
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 0);
        // No hello ever: news is asked about and told; anything else waits.
        moment.rules(rules(
            lamp_msgs::Unprompted {
                never: true,
                ..lamp_msgs::Unprompted::default()
            },
            lamp_msgs::Unprompted::default(),
        ));
        assert_eq!(
            decide(&mut moment, 3, "news"),
            ("say_now".to_owned(), "counted".to_owned(), None)
        );
        let (choice, by, guard) = decide(&mut moment, 4, "unprompted");
        assert_eq!((choice.as_str(), (by, guard)), ("wait", rule()));
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 1);
        let totals = moment.totals();
        assert_eq!((totals.held_rule, totals.said, totals.decided), (3, 1, 4));
    }

    /// Every rule a person could keep: no hello and no news ever, quiet,
    /// quiet mornings.
    fn every_rule() -> Rules {
        Rules {
            person: "everyone".into(),
            quiet: true,
            quiet_mornings: true,
            rules: 1,
            hello: lamp_msgs::Unprompted {
                never: true,
                ..lamp_msgs::Unprompted::default()
            },
            news: lamp_msgs::Unprompted {
                never: true,
                ..lamp_msgs::Unprompted::default()
            },
            keep_light_on: false,
        }
    }

    /// The reviewer on #1003: no rule kept about speaking first holds a
    /// reminder the person set; only someone talking does. An unprompted
    /// ask is held by the same rules, by the rule, no model asked.
    #[test]
    fn a_kept_rule_never_holds_a_reminder_and_still_holds_the_rest() {
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut moment = Moment::start(
            Counted(asked.clone()),
            Options::new(Duration::from_secs(1), None),
        )
        .unwrap();
        moment.rules(every_rule());
        let quiet = Now {
            room: Room::Quiet,
            someone_here: Some(true),
            quiet_s: 300,
            talking: false,
            spoke_up_last_hour: 0,
        };
        let ask = |number: u64, act: &str| SkillAsk {
            ask: number,
            skill: "reminders".into(),
            act: act.into(),
            person: None,
            away_min: None,
            there_s: None,
        };
        let t = Instant::now();
        let mut decide = |number: u64, act: &str, room_now: Now| {
            moment.asked(&ask(number, act), t);
            let decided = moment.turn(t, 1_000_000, room_now).expect("at once");
            (
                decided.decision.choice,
                decided.decision.by,
                decided.line.guard,
            )
        };
        let rule = crate::policy::RULE.to_owned();
        assert_eq!(
            decide(1, REMINDER, quiet),
            ("say_now".to_owned(), rule.clone(), None)
        );
        assert_eq!(
            decide(
                2,
                REMINDER,
                Now {
                    talking: true,
                    ..quiet
                }
            ),
            ("wait".to_owned(), rule.clone(), Some(Hold::Talking))
        );
        assert_eq!(
            decide(3, "unprompted", quiet),
            ("wait".to_owned(), rule, Some(Hold::KeptRule))
        );
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// The reviewer on #1003, the kept-rule twin of #1082's: a rule kept
    /// while the question is out, its asker then gone, holds the ask by the
    /// rule; the fallback's `say_now` is never taken.
    #[test]
    fn a_rule_kept_while_the_question_is_out_holds_it_when_the_asker_goes() {
        let mut moment = Moment::start(Gone, Options::new(Duration::from_secs(3), None)).unwrap();
        let quiet = Now {
            room: Room::Quiet,
            someone_here: Some(true),
            quiet_s: 300,
            talking: false,
            spoke_up_last_hour: 0,
        };
        let t = Instant::now();
        moment.asked(
            &SkillAsk {
                ask: 1,
                skill: "stretch-break".into(),
                act: "unprompted".into(),
                person: None,
                away_min: None,
                there_s: None,
            },
            t,
        );
        assert!(moment.turn(t, 1_000_000, quiet).is_none(), "out");
        moment.rules(every_rule());
        let held = (0..500)
            .find_map(|_| {
                std::thread::sleep(Duration::from_millis(2));
                moment.turn(t + Duration::from_millis(100), 1_000_000, quiet)
            })
            .expect("decided once the asker went");
        assert_eq!(
            (held.decision.choice.as_str(), held.line.guard),
            ("wait", Some(Hold::KeptRule))
        );
    }

    /// The reviewer on #1099: a reminder needs no model, so it is decided at
    /// once though news is out with the model and an unprompted ask waits
    /// before it; the question out and the ask waiting are as they were.
    #[test]
    fn a_reminder_is_decided_at_once_behind_a_question_out_and_another_ask() {
        let (_go, gate) = mpsc::channel();
        let given = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let decider = Held {
            go: gate,
            given: given.clone(),
        };
        let mut moment =
            Moment::start(decider, Options::new(Duration::from_secs(3), None)).unwrap();
        let quiet = Now {
            room: Room::Quiet,
            someone_here: Some(true),
            quiet_s: 300,
            talking: false,
            spoke_up_last_hour: 0,
        };
        let ask = |number: u64, act: &str| SkillAsk {
            ask: number,
            skill: "reminders".into(),
            act: act.into(),
            person: None,
            away_min: None,
            there_s: None,
        };
        let t = Instant::now();
        moment.asked(&ask(1, "news"), t);
        assert!(moment.turn(t, 1_000_000, quiet).is_none(), "news is out");
        moment.asked(&ask(2, "unprompted"), t);
        moment.asked(&ask(3, REMINDER), t);
        let said = moment
            .turn(t + Duration::from_millis(10), 1_000_000, quiet)
            .expect("the reminder at once");
        assert_eq!(
            (
                said.decision.ask,
                said.decision.choice.as_str(),
                said.decision.by.as_str()
            ),
            (Some(3), "say_now", crate::policy::RULE)
        );
        // The news still out, and the unprompted ask still waiting, not
        // dropped by the reminder taken before it (the reviewer on #1143).
        assert!(moment.asking.is_some(), "the news out");
        assert_eq!(moment.waiting.len(), 1, "the unprompted ask waiting");
        assert!(
            moment
                .turn(t + Duration::from_millis(20), 1_000_000, quiet)
                .is_none(),
            "nothing else decided"
        );
    }

    /// Says yes, and keeps the state it was shown.
    struct Shown(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
    impl Decider for Shown {
        fn ask(
            &mut self,
            state: &str,
            _: &[(String, Question)],
            _: Duration,
        ) -> Result<Asked, Failed> {
            self.0.lock().unwrap().push(state.to_owned());
            Ok(Asked {
                answers: BTreeMap::from([("good_moment".to_owned(), Answer::Noul(0.9))]),
                model: "shown".into(),
                took: Duration::ZERO,
                usage: None,
                connection: None,
            })
        }
        fn name(&self) -> &str {
            "shown"
        }
    }

    /// One add-on's say through the moment question, acting or not.
    fn addon_said(act_addon: bool) -> (Decided, Vec<String>) {
        let shown = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let options = Options {
            act_addon,
            ..Options::new(Duration::from_secs(1), None)
        };
        let mut moment = Moment::start(Shown(shown.clone()), options).unwrap();
        let room_now = Now {
            room: Room::Quiet,
            someone_here: Some(true),
            quiet_s: 300,
            talking: false,
            spoke_up_last_hour: 0,
        };
        let t = Instant::now();
        moment.asked_addon(
            &AddonAsk {
                proposal: 77,
                addon: "happy-with-music".into(),
                why: "music".into(),
                sure: 0.7,
            },
            t,
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        let decided = loop {
            if let Some(decided) = moment.turn(Instant::now(), 1_000_000, room_now) {
                break decided;
            }
            assert!(Instant::now() < deadline, "not decided");
            std::thread::sleep(Duration::from_millis(5));
        };
        let shown = shown.lock().unwrap().clone();
        (decided, shown)
    }

    /// #2530 part 2: an add-on's say is the moment question's, shown only
    /// the add-on and why; its decision is `about` `addon`, names the
    /// proposal and no skill's ask, and is shadow unless `--act addon`.
    #[test]
    fn an_add_ons_say_is_decided_about_addon_naming_its_proposal() {
        let (decided, shown) = addon_said(true);
        let decision = &decided.decision;
        assert_eq!(
            (
                decision.about.as_str(),
                decision.choice.as_str(),
                decision.proposal,
                decision.ask,
                decision.shadow
            ),
            ("addon", "say_now", Some(77), None, false)
        );
        let [state] = shown.as_slice() else {
            panic!("{shown:?}")
        };
        assert!(
            state.contains("an add-on on the lamp suggests, about: music"),
            "{state}"
        );
        assert!(state.contains("happy-with-music"), "{state}");
        let (shadowed, _) = addon_said(false);
        assert!(shadowed.decision.shadow);
        assert_eq!(shadowed.decision.about, "addon");
    }

    /// The reviewer on #2667: a skill's ask and an add-on's say, queued
    /// together, are each decided as their own: the skill's `about`
    /// `moment` naming its `ask` and acting, the add-on's `about` `addon`
    /// naming its `proposal`, shadow without `--act addon`.
    #[test]
    fn a_skills_ask_still_names_its_ask_beside_an_add_ons() {
        let shown = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let options = Options::new(Duration::from_secs(1), None);
        let mut moment = Moment::start(Shown(shown.clone()), options).unwrap();
        let room_now = Now {
            room: Room::Quiet,
            someone_here: Some(true),
            quiet_s: 300,
            talking: false,
            spoke_up_last_hour: 0,
        };
        let t = Instant::now();
        moment.asked(
            &SkillAsk {
                ask: 11,
                skill: "stretch-break".into(),
                act: "unprompted".into(),
                person: None,
                away_min: None,
                there_s: None,
            },
            t,
        );
        moment.asked_addon(
            &AddonAsk {
                proposal: 22,
                addon: "happy-with-music".into(),
                why: "music".into(),
                sure: 0.7,
            },
            t,
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut decided = Vec::new();
        while decided.len() < 2 {
            if let Some(one) = moment.turn(Instant::now(), 1_000_000, room_now) {
                decided.push(one.decision);
            }
            assert!(Instant::now() < deadline, "decided {decided:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
        let skill = decided
            .iter()
            .find(|d| d.ask == Some(11))
            .expect("the skill's");
        assert_eq!(
            (
                skill.about.as_str(),
                skill.proposal,
                skill.shadow,
                skill.choice.as_str()
            ),
            (ABOUT, None, false, "say_now")
        );
        let addon = decided
            .iter()
            .find(|d| d.proposal == Some(22))
            .expect("the add-on's");
        assert_eq!(
            (addon.about.as_str(), addon.ask, addon.shadow),
            ("addon", None, true)
        );
        let states = shown.lock().unwrap();
        assert_eq!(states.len(), 2);
        assert_eq!(
            states
                .iter()
                .filter(|state| state.contains("add-on"))
                .count(),
            1,
            "only the add-on's state names an add-on"
        );
    }
}
