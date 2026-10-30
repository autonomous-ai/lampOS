//! The decision "say hello now?" (`docs/policy.md`, "Hello on arrival").
//!
//! The Welcome back skill says hello to someone who comes back and sits
//! down, and asks this decision first (`skills/ask`, put as an arrival,
//! #939). The voice's own hello, which asked on `voice/arrival` and held its
//! hello up to 2 s for this decision, was removed (#2825): nothing publishes
//! `voice/arrival` now, and the reading of it here is left unfed.
//!
//! Two hard rules sit around the decider and are not left to it: never
//! while anyone is talking to the lamp (the voice listening, thinking or
//! speaking, or anything heard or said in the last 15 s), and no hello
//! within twenty minutes of the last, the voice's own limit. While either
//! holds the answer is `wait`, by the rule, and no model is asked: the voice
//! is waiting, and a question not answered would leave it to its rule.
//! A skill's ask has a third beside them: no hello to the same person within
//! an hour ([`ONCE_PER_PERSON`]), by the ask's `person`, every ask with none
//! sharing one ([`SOMEONE`]) (the first builder, 30 Oct, on the launch
//! testers' findings).
//! With `lamp-policy --rules` a third holds as they do: a rule the people
//! at the lamp kept about it speaking first, by its `hello` limits
//! ([`crate::rules`], #813).
//!
//! Shadow unless `lamp-policy --act hello`: decided, published and written
//! down, and the voice says hello by its own rule.
//!
//! By the hard rules alone with `lamp-policy --hello-by-rule` (the first
//! builder, 30 Oct, on #4625, after #4305 took the judge out of what the lamp
//! does): when none holds the answer is `say_now` at once, by the rule, with
//! no wait for a model. The judge is still asked, in shadow and off the
//! answer's path, and its answer is written on a line of its own
//! ([`JUDGE_ABOUT`]) and counted, never acted on: data for the next tuning
//! round.
use crate::{
    attention::Room,
    decide::{Answer, Asked, Connection, Decider, Failed, Question, Usage},
    progress::{Choice, LocalTime},
    rules::{Kept, Kind},
};
use lamp_msgs::{Decision, Rules, TurnVerdict, Voice, VoiceArrival};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    io,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

/// What this decision is called on the bus and in the log.
pub const ABOUT: &str = "hello";
/// The judge's answer asked in shadow with `--hello-by-rule`, as its log
/// line is called: never published, never acted on.
pub const JUDGE_ABOUT: &str = "hello_judge";
/// From this probability up, the lamp says hello now.
pub const SAY_FROM: f32 = 0.5;
/// No hello within this long of the last: the voice's own limit
/// (`speak_up`, twenty minutes).
pub const AT_MOST_EVERY: Duration = Duration::from_secs(20 * 60);
/// No hello to the same person within this long of their last (the first
/// builder, 30 Oct, on the launch testers' findings: a hello every time a
/// face came back after five minutes away, 15:13 and 15:41 on one lamp).
/// By a skill's ask's `person`; asks without one share [`SOMEONE`].
pub const ONCE_PER_PERSON: Duration = Duration::from_secs(60 * 60);
/// The one key every ask without a `person` (a face not known) shares.
pub const SOMEONE: &str = "someone";
/// Nobody is talking to the lamp: the same quiet as the progress question.
pub const QUIET_FOR: Duration = Duration::from_secs(15);
/// A turn of the lamp's that began with nobody listened to in this long
/// before it is one it began itself, unasked (as the progress question).
pub const UNASKED_AFTER: Duration = Duration::from_secs(10);
/// The voice holds a due hello this long for the decision.
pub const VOICE_WAITS: Duration = Duration::from_secs(2);
/// In shadow, a hello chosen and a turn the lamp began itself (its first
/// `thinking` or `speaking`) this close together are one greeting: the voice
/// said its own rule's hello for the same arrival, after holding it for the
/// decision (the reviewer on #883). The turn is followed from its beginning
/// to its end, however late its first sound comes (the model may take up to
/// 30 s to answer): spoken, it is counted once, as a turn the lamp began;
/// ended without a sound, the hello chosen is counted once, as chosen.
pub const ONE_GREETING_WITHIN: Duration = Duration::from_secs(5);

/// A turn the lamp began itself, unasked, from its beginning to its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OwnTurn {
    /// The shadow hello it is the greeting of, taken from `notes`.
    claimed: Option<Instant>,
    /// Its sound began: it is counted in `unasked`.
    spoke: bool,
}
/// The most one asking may take, whatever `--deadline-ms` says: a model late
/// past it leaves the rule time to answer before the voice stops waiting.
pub const MOST_ASKING: Duration = Duration::from_millis(1_500);
const HOUR: Duration = Duration::from_secs(3_600);
/// No more arrivals than this wait to be decided; there is one voice.
const MOST_WAITING: usize = 4;
/// Kept back of what is left of the asker's wait when the model is asked,
/// for the answer to be taken up and published in it.
pub const ANSWER_MARGIN: Duration = Duration::from_millis(200);

/// How long the model may take for one received at `received` and asked at
/// `now`: what is left of `waits`, less [`ANSWER_MARGIN`], and at most
/// `most`. Zero when nothing is left: the rule answers at once (the
/// reviewer on #958, 12:19).
pub(crate) fn time_left(
    waits: Duration,
    received: Instant,
    now: Instant,
    most: Duration,
) -> Duration {
    waits
        .saturating_sub(now.saturating_duration_since(received))
        .saturating_sub(ANSWER_MARGIN)
        .min(most)
}

/// An arrival waiting to be decided, as it came: from the voice, or a
/// skill's ask put as one (#939), and when.
#[derive(Clone, Debug)]
struct Waiting {
    arrival: VoiceArrival,
    received: Instant,
    skill_ask: bool,
    /// Whom a skill's ask is for, by [`ONCE_PER_PERSON`]'s key; none for
    /// the voice's arrivals.
    person: Option<String>,
}

/// The question out, and when the one it is about was received.
#[derive(Clone, Debug)]
struct Asking {
    context: Context,
    sent: Instant,
    received: Instant,
    person: Option<String>,
}
/// What the rule gives when no model answers. Authored, not measured.
pub const RULE_YES: f32 = 0.6;
pub const RULE_NO: f32 = 0.1;

/// How the hello question is asked (`lamp-policy --hello`).
#[derive(Clone, Debug)]
pub struct Options {
    /// How long one asking may take: at most [`MOST_ASKING`].
    pub deadline: Duration,
    /// The lamp's local time, for the hour of the day: a fixed offset
    /// (`--utc-offset-minutes`) or a zone (`--zone`, #820).
    pub local: Option<Arc<LocalTime>>,
    /// The hard rules ([`QUIET_FOR`], [`AT_MOST_EVERY`]); shorter only in a
    /// test.
    pub quiet_for: Duration,
    pub at_most_every: Duration,
    /// [`ONCE_PER_PERSON`]; shorter only in a test.
    pub once_per_person: Duration,
    /// The decisions act (`--act hello`): published with `shadow: false`,
    /// and the voice says hello or not as they say. Off unless set.
    pub act: bool,
    /// It also answers a skill's ask to say hello (`skills/ask`, `act`
    /// `hello`, `lamp-policy --gate`, #939), as it answers the voice's
    /// arrivals. Off unless set.
    pub gate: bool,
    /// With the gate, it also answers an add-on's say (`addon/ask`,
    /// `lamp-policy --addons`, #2530 part 2), by the moment question. Off
    /// unless set.
    pub addons: bool,
    /// It obeys what the people at the lamp asked about speaking first
    /// (`memory/rules`, `lamp-policy --rules`, [`crate::rules`], #813), and
    /// so does the moment question beside it. Off unless set.
    pub rules: bool,
    /// Only talk to the lamp or by it counts toward [`QUIET_FOR`]
    /// (`lamp-policy --hello-quiet to-the-lamp`, #1155): a turn whose answer
    /// the voice held back as not said to the lamp (`voice/turn`) is taken
    /// back from the quiet. The hold while the voice listens, thinks or
    /// speaks is unchanged, so a hello never comes over someone talking.
    /// Off unless set: all talk counts, as before.
    pub quiet_to_the_lamp: bool,
    /// The hard rules alone decide (`lamp-policy --hello-by-rule`, the first
    /// builder on #4625): a pass is `say_now` at once, by the rule, and the
    /// judge is asked only in shadow, its answer logged and never acted on.
    /// Off unless set.
    pub by_rule: bool,
}

impl Options {
    pub fn new(deadline: Duration, local: Option<Arc<LocalTime>>) -> Self {
        Self {
            deadline: deadline.min(MOST_ASKING),
            local,
            quiet_for: QUIET_FOR,
            at_most_every: AT_MOST_EVERY,
            once_per_person: ONCE_PER_PERSON,
            act: false,
            gate: false,
            addons: false,
            rules: false,
            quiet_to_the_lamp: false,
            by_rule: false,
        }
    }
}

/// A hard rule that answered `wait` without a model, or turned a yes to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Hold {
    /// Someone is talking to the lamp, or was a moment ago.
    Talking,
    /// The lamp said hello less than twenty minutes ago.
    GreetedRecently,
    /// The lamp said hello to the same person less than an hour ago
    /// ([`ONCE_PER_PERSON`]).
    GreetedPerson,
    /// A rule the people at the lamp kept holds it (`--rules`).
    KeptRule,
}

impl Hold {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Talking => "talking",
            Self::GreetedRecently => "greeted_recently",
            Self::GreetedPerson => "greeted_person",
            Self::KeptRule => "kept_rule",
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

/// Everything known when the question was put: the arrival, from
/// `voice/arrival`, and the room, from the lamp's own topics.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Context {
    pub arrival: u64,
    pub away_min: u32,
    pub there_s: u32,
    /// Seconds since anything was said to or by the lamp, as the voice
    /// says it.
    pub quiet_s: u32,
    pub since_hello_min: Option<u32>,
    /// Whether the eyes see someone near, and whether such a face is turned
    /// to the lamp; not known without a list of faces younger than 2 s.
    pub someone_here: Option<bool>,
    pub facing: Option<bool>,
    pub room: Room,
    pub spoke_up_last_hour: u32,
    pub hour: Option<u8>,
    /// A skill's ask put as an arrival (#939): `arrival` is the ask's
    /// number, and the decision names it as the ask. Carried with the
    /// question it is about, from waiting to decided, so that no other ask
    /// can take its place (the reviewer on #958, 12:19). Written only when
    /// true.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub skill_ask: bool,
}

const LAMP: &str = "A small desk lamp robot on the desk of the person it works for, like a friend at the next desk: when someone comes back and sits down it may greet them with a few warm words, and otherwise it keeps quiet.";

fn room_told(room: Room) -> &'static str {
    match room {
        Room::Quiet => "quiet: nobody else has been talking",
        Room::Busy => "busy: people are talking to each other",
        Room::Unknown => "unknown",
    }
}

/// The state as the text that is sent, its fields in this order.
pub fn state_text(context: &Context) -> String {
    format!(
        "{{\"lamp\":{},\"arrival\":{{\"minutes_away\":{},\"seconds_there\":{},\
         \"minutes_since_the_lamp_last_said_hello\":{}}},\"room\":{{\"someone_at_the_lamp\":{},\
         \"facing_the_lamp\":{},\"seconds_since_anything_was_said\":{},\"room\":{},\
         \"times_the_lamp_spoke_up_unasked_in_the_last_hour\":{},\"hour_of_day\":{}}}}}",
        json!(LAMP),
        context.away_min,
        context.there_s,
        json!(context.since_hello_min),
        json!(context.someone_here),
        json!(context.facing),
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

const OPENING: &str = "state.arrival says someone has just come back to the lamp's desk and sat down in front of it; state.room is how things are around the lamp now. ";

/// The two questions: each one's name and the rest of its instruction after
/// [`OPENING`]. Authored, not fitted.
const ASKED: [(&str, &str); 2] = [
    (
        "welcome",
        "Would a friend at the next desk greet them now with a few warm words, given how long they were away, whether they are turned to the lamp, and the hour?",
    ),
    (
        "intrusive",
        "Would a hello now more likely interrupt or annoy them than please them: the room is busy with other people's talk, it is late at night or early in the morning, they were away only a short while, or the lamp has spoken up unasked often in the last hour?",
    ),
];

/// The two questions, in the order they are asked in.
pub fn questions() -> Vec<(String, Question)> {
    ASKED
        .iter()
        .map(|(name, rest)| {
            (
                (*name).to_owned(),
                Question::Noul {
                    instructions: format!("{OPENING}{rest}"),
                },
            )
        })
        .collect()
}

/// Welcome, and not intrusive, as if the two were independent; each answer
/// by its name, or the name of the first that is missing.
pub fn combine(
    answers: &BTreeMap<String, Answer>,
) -> Result<(Judgement, BTreeMap<String, f32>), &'static str> {
    let mut two = [0.0f32; 2];
    for (to, (name, _)) in two.iter_mut().zip(ASKED) {
        match answers.get(name) {
            Some(Answer::Noul(yes)) if (0.0..=1.0).contains(yes) => *to = *yes,
            _ => return Err(name),
        }
    }
    let [welcome, intrusive] = two;
    let probability = (welcome * (1.0 - intrusive)).clamp(0.0, 1.0);
    let by_name = ASKED
        .iter()
        .zip(two)
        .map(|((name, _), yes)| ((*name).to_owned(), yes))
        .collect();
    Ok((
        Judgement {
            probability,
            choice: if probability >= SAY_FROM {
                Choice::SayNow
            } else {
                Choice::Wait
            },
            from_rule: false,
        },
        by_name,
    ))
}

/// The judgement when no model answers: a hello unless the room is known to
/// be busy, the lamp spoke up unasked twice or more in the hour, or it is
/// before eight in the morning or after ten at night (when the hour is
/// known). The voice's own rule has held everything else already.
pub fn fallback(context: &Context) -> Judgement {
    let yes = context.room != Room::Busy
        && context.spoke_up_last_hour < 2
        && context.hour.is_none_or(|hour| (8..22).contains(&hour));
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
    /// A hard rule that answered `wait`, or turned a yes to it.
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
    /// With `--hello-by-rule`, on a pass: `asked_in_shadow` when the judge
    /// was asked beside it (its answer comes on a [`JudgeLine`]), or
    /// `not_asked` when its asker was still busy or gone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judge: Option<&'static str>,
}

/// The judge's answer for a hello the hard rules passed with
/// `--hello-by-rule`: written on a line of its own once it comes, never
/// published, never acted on.
#[derive(Clone, Debug, Serialize)]
pub struct JudgeLine {
    /// When the hello was decided and the judge asked.
    pub at_unix_ms: u64,
    pub about: &'static str,
    pub shadow: bool,
    pub context: Context,
    pub state: Value,
    pub asked: String,
    /// What the rule decided, and acted on.
    pub decided: Choice,
    pub answers: BTreeMap<String, f32>,
    /// What the judge would have decided; none when it gave no usable
    /// answer (`failed` says why).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judgement: Option<Judgement>,
    /// The judge would have decided otherwise than the rule.
    pub differs: bool,
    pub by: String,
    pub took_ms: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<Connection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_why: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// A hello the hard rules passed, whose judge is asked in shadow.
#[derive(Clone, Debug)]
struct Judging {
    context: Context,
    decided: Choice,
    unix_ms: u64,
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
    /// Arrivals answered `wait` by each hard rule.
    pub held_talking: u64,
    pub held_recent: u64,
    pub held_person: u64,
    /// With `--rules`.
    pub held_rule: u64,
    /// Arrivals not decided before the voice stopped waiting for them.
    pub late: u64,
    /// With `--hello-by-rule`: the judge's answers taken in shadow, and
    /// those that would have decided otherwise than the rule.
    pub judged: u64,
    pub judge_differs: u64,
}

type Answered = (Result<Asked, Failed>, Duration);

/// The hello question as it runs beside the policy's loop, with its own
/// thread and its own decision model.
pub struct Hello {
    options: Options,
    judge_name: String,
    states: mpsc::SyncSender<(String, Duration)>,
    answers: mpsc::Receiver<Answered>,
    asker_gone: bool,
    asking: Option<Asking>,
    /// With [`Options::by_rule`]: the hellos whose judge is asked in
    /// shadow, in the order asked.
    judging: VecDeque<Judging>,
    waiting: VecDeque<Waiting>,
    voice: Option<Voice>,
    /// The newest talk, by anyone: what the model is shown, and what the
    /// moment question counts.
    last_talk: Option<Instant>,
    /// The newest talk the hello's own quiet counts: the same, or with
    /// [`Options::quiet_to_the_lamp`] only talk to the lamp or by it (#1155).
    lamp_talk: Option<Instant>,
    last_listening: Option<Instant>,
    unasked: VecDeque<Instant>,
    /// Hellos chosen, which in shadow nobody hears, and the last one that
    /// acted.
    notes: VecDeque<Instant>,
    /// The turn the lamp began itself, while it lasts.
    own_turn: Option<OwnTurn>,
    greeted: Option<Instant>,
    /// The last hello acted on for each person a skill asked for, by
    /// [`ONCE_PER_PERSON`]'s key; kept for that long.
    greeted_people: HashMap<String, Instant>,
    /// When it started: the quiet a skill's ask is shown is counted from
    /// here if nothing has been said since.
    started: Instant,
    /// The moment question beside it (`--gate`, #939), which takes the
    /// skills' asks of news and of anything else unprompted.
    moment: Option<crate::moment::Moment>,
    /// The newest rules about speaking first, with `--rules` only.
    rules: Kept,
    /// With [`Options::quiet_to_the_lamp`]: the talk before the turn under
    /// way (or the last) began, and whether the voice held its answer back
    /// as not said to the lamp. Such a turn's talk is taken back from the
    /// quiet (#1155). None once the lamp has spoken in it: then it is talk
    /// by the lamp, and no verdict takes it back.
    turn_floor: Option<Option<Instant>>,
    turn_held: bool,
    /// The newest request heard in the turn under way, once known: a
    /// verdict for another request, or one said before any of the turn's
    /// words were heard, is not this turn's.
    turn_request: Option<u64>,
    /// The turn under way was begun by its words, heard before the voice
    /// was seen listening: the voice's state and its words come on two
    /// topics, and the loop may take a word first. Its listening, when
    /// seen, is the same turn.
    turn_by_words: bool,
    /// A verdict said before its request's words reached the policy, kept
    /// for them: the voice's words and its verdicts come on two topics, and
    /// the second route says a verdict before its words go out (Second on
    /// #1782). Let go when words of a later request come instead.
    pending_verdict: Option<(u64, TurnVerdict)>,
    totals: Totals,
}

impl Hello {
    /// Start the thread that asks `decider`.
    pub fn start<D: Decider + Send + 'static>(
        mut decider: D,
        options: Options,
    ) -> io::Result<Self> {
        let judge_name = decider.name().to_owned();
        let (states, asked) = mpsc::sync_channel::<(String, Duration)>(1);
        let (answered, answers) = mpsc::channel();
        thread::Builder::new()
            .name("ask-hello".into())
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
            judging: VecDeque::new(),
            waiting: VecDeque::new(),
            voice: None,
            last_talk: None,
            lamp_talk: None,
            last_listening: None,
            unasked: VecDeque::new(),
            notes: VecDeque::new(),
            own_turn: None,
            greeted: None,
            greeted_people: HashMap::new(),
            started: Instant::now(),
            moment: None,
            rules: Kept::default(),
            turn_floor: None,
            turn_held: false,
            turn_request: None,
            turn_by_words: false,
            pending_verdict: None,
            totals: Totals::default(),
        })
    }

    pub fn totals(&self) -> Totals {
        self.totals
    }

    /// Whether its decisions act ([`Options::act`]).
    pub fn acts(&self) -> bool {
        self.options.act
    }

    /// Whether the hard rules alone decide ([`Options::by_rule`]).
    pub fn by_rule(&self) -> bool {
        self.options.by_rule
    }

    /// Whether it obeys the people's rules ([`Options::rules`]).
    pub fn obeys_rules(&self) -> bool {
        self.options.rules
    }

    /// Whether only talk to the lamp or by it counts toward its quiet
    /// ([`Options::quiet_to_the_lamp`]): then it reads `voice/turn`.
    pub fn quiet_to_the_lamp(&self) -> bool {
        self.options.quiet_to_the_lamp
    }

    /// Whether the talk now is a turn the voice held back, which with
    /// [`Options::quiet_to_the_lamp`] does not count toward the quiet.
    fn held_back(&self) -> bool {
        self.options.quiet_to_the_lamp && self.turn_held
    }

    /// What the people at the lamp asked it about speaking first, newest:
    /// kept here and by the moment question beside it.
    pub fn rules(&mut self, rules: Rules) {
        // The moment question has no switch of its own: it obeys them only
        // with `--rules` (the reviewer on #1003).
        if self.options.rules
            && let Some(moment) = self.moment.as_mut()
        {
            moment.rules(rules.clone());
        }
        self.rules.set(rules);
    }

    /// Whether it answers add-ons' says ([`Options::addons`]).
    pub fn answers_addons(&self) -> bool {
        self.options.gate && self.options.addons
    }

    /// Whether it answers skills' asks ([`Options::gate`]).
    pub fn answers_skills(&self) -> bool {
        self.options.gate
    }

    /// With the moment question beside it (`--gate`, #939): skills' asks
    /// of news, of anything else unprompted and of a reminder are put to
    /// it, with what
    /// this question knows of who is talking and how often the lamp spoke
    /// up unasked.
    pub fn with_moment(mut self, moment: crate::moment::Moment) -> Self {
        self.moment = Some(moment);
        self
    }

    /// The moment question's counts, if it runs.
    pub fn moment_totals(&self) -> Option<crate::moment::Totals> {
        self.moment.as_ref().map(crate::moment::Moment::totals)
    }

    /// An add-on's say (`addon/ask`, `--addons`, #2530 part 2), put to the
    /// moment question. False when there is none to take it.
    pub fn asked_addon(&mut self, ask: &lamp_msgs::AddonAsk, now: Instant) -> bool {
        match self.moment.as_mut() {
            Some(moment) => {
                moment.asked_addon(ask, now);
                true
            }
            None => false,
        }
    }

    /// A skill's ask of news, of anything else unprompted or of a reminder,
    /// put to the moment question. False when there is none to take it.
    pub fn asked_moment(&mut self, ask: &lamp_msgs::SkillAsk, now: Instant) -> bool {
        match self.moment.as_mut() {
            Some(moment) => {
                moment.asked(ask, now);
                true
            }
            None => false,
        }
    }

    /// Take up the moment question's answer, or decide its next ask, with
    /// how things are now.
    pub fn moment_turn(
        &mut self,
        now: Instant,
        unix_ms: u64,
        room: Room,
        someone_here: Option<bool>,
    ) -> Option<crate::moment::Decided> {
        let quiet = now.saturating_duration_since(self.last_talk.unwrap_or(self.started));
        let room_now = crate::moment::Now {
            room,
            someone_here,
            quiet_s: u32::try_from(quiet.as_secs()).unwrap_or(u32::MAX),
            talking: self.talking_at_all(now),
            // Acting, the lamp's own turns are counted from `voice/state`.
            spoke_up_last_hour: u32::try_from(self.unasked.len()).unwrap_or(u32::MAX),
        };
        self.moment.as_mut()?.turn(now, unix_ms, room_now)
    }

    /// Someone is talking to the lamp: the voice listening, thinking or
    /// speaking, or anything heard or said in the last [`Options::quiet_for`].
    /// With [`Options::quiet_to_the_lamp`], only talk to the lamp or by it
    /// counts toward that quiet.
    fn talking(&self, now: Instant) -> bool {
        self.voice_busy() || self.within_quiet(self.lamp_talk, now)
    }

    /// As [`Self::talking`], all talk counting: for the moment question.
    fn talking_at_all(&self, now: Instant) -> bool {
        self.voice_busy() || self.within_quiet(self.last_talk, now)
    }

    fn voice_busy(&self) -> bool {
        matches!(
            self.voice,
            Some(Voice::Listening | Voice::Thinking | Voice::Speaking)
        )
    }

    fn within_quiet(&self, talk: Option<Instant>, now: Instant) -> bool {
        talk.is_some_and(|at| now.saturating_duration_since(at) < self.options.quiet_for)
    }

    /// Talk at `now`: counted toward the hello's quiet unless it is a turn
    /// the voice held back ([`Self::held_back`]).
    fn talked(&mut self, now: Instant) {
        self.last_talk = Some(now);
        if !self.held_back() {
            self.lamp_talk = Some(now);
        }
    }

    /// A skill asks to say hello (`skills/ask`, #939): put as an arrival,
    /// decided as the voice's are, by the same hard rules, the same kept
    /// rules and the same question, and its decision names the ask. The
    /// quiet is what the policy knows itself (since anything was last said,
    /// or since it started), and the last hello its own. One that fails its
    /// check, or is not a hello, is passed over.
    pub fn asked(&mut self, ask: &lamp_msgs::SkillAsk, now: Instant) {
        if ask.check().is_err() || ask.act != ABOUT {
            return;
        }
        let quiet = now.saturating_duration_since(self.last_talk.unwrap_or(self.started));
        let arrival = VoiceArrival {
            arrival: ask.ask,
            away_min: ask.away_min.unwrap_or_default(),
            there_s: ask.there_s.unwrap_or_default(),
            quiet_s: u32::try_from(quiet.as_secs()).unwrap_or(u32::MAX),
            since_hello_min: self.greeted.map(|at| {
                u32::try_from(now.saturating_duration_since(at).as_secs() / 60).unwrap_or(u32::MAX)
            }),
        };
        let person = ask.person.clone().unwrap_or_else(|| SOMEONE.to_owned());
        self.wait(arrival, now, Some(person));
    }

    /// Whether a question is out or an arrival waits, here or in the moment
    /// question: the voice or a skill is waiting, so the loop looks again
    /// sooner (Fourth on #958).
    pub fn busy(&self) -> bool {
        self.asking.is_some()
            || !self.judging.is_empty()
            || !self.waiting.is_empty()
            || self
                .moment
                .as_ref()
                .is_some_and(crate::moment::Moment::busy)
    }

    /// A hello is due: it waits to be decided. One that fails its check is
    /// passed over; past [`MOST_WAITING`] the oldest is let go.
    pub fn arrival(&mut self, arrival: VoiceArrival, now: Instant) {
        self.wait(arrival, now, None);
    }

    /// `person`: a skill's ask, for whom; none, a voice's arrival.
    fn wait(&mut self, arrival: VoiceArrival, now: Instant, person: Option<String>) {
        if arrival.check().is_err() {
            return;
        }
        if self.waiting.len() == MOST_WAITING {
            self.waiting.pop_front();
            self.totals.late += 1;
        }
        self.waiting.push_back(Waiting {
            arrival,
            received: now,
            skill_ask: person.is_some(),
            person,
        });
    }

    /// What the voice is doing, as the progress question takes it. Whether
    /// a turn was asked for is known at its beginning and kept to its end:
    /// an answer whose first sound is slow (the model may take 30 s) is
    /// still an answer, never counted as speaking up unasked (the reviewer
    /// on #883, 05:22).
    pub fn voice(&mut self, state: Voice, now: Instant) {
        let unasked = self
            .last_listening
            .is_none_or(|at| now.saturating_duration_since(at) > UNASKED_AFTER);
        // A turn the lamp begins itself: its first `thinking` or `speaking`
        // with nobody listened to lately. In shadow, the hello chosen just
        // before is this very greeting, said by the voice's own rule after
        // holding it for the decision: it is taken from the notes now, and
        // given back if the turn ends without a sound ([`ONE_GREETING_WITHIN`]).
        if matches!(state, Voice::Thinking | Voice::Speaking)
            && !matches!(
                self.voice,
                Some(Voice::Listening | Voice::Thinking | Voice::Speaking)
            )
            && unasked
            && self.own_turn.is_none()
        {
            let claimed = (!self.options.act)
                .then(|| {
                    self.notes.iter().rposition(|chosen| {
                        now.saturating_duration_since(*chosen) <= ONE_GREETING_WITHIN
                    })
                })
                .flatten()
                .and_then(|at| self.notes.remove(at));
            self.own_turn = Some(OwnTurn {
                claimed,
                spoke: false,
            });
        }
        match state {
            Voice::Listening => {
                self.end_own_turn();
                // A turn begins: listened to after quiet, or over the lamp.
                // Listened to again while thinking is the same turn.
                // One its words began already is the same.
                if self.turn_by_words {
                    self.turn_by_words = false;
                } else if !matches!(self.voice, Some(Voice::Listening | Voice::Thinking)) {
                    self.begin_turn();
                }
                self.last_listening = Some(now);
                self.talked(now);
            }
            Voice::Speaking => {
                // A turn the lamp began itself, at its first sound only:
                // one turn may speak, think and speak again (a later
                // generation for the same owner, the coordinator's
                // continued generation), and is one turn the lamp began
                // (the reviewer on #883, 04:55). Any other turn was asked
                // for, however long ago the person was listened to.
                if let Some(turn) = self.own_turn.as_mut().filter(|turn| !turn.spoke) {
                    turn.spoke = true;
                    self.unasked.push_back(now);
                }
                // The lamp speaking is talk by it, whatever the turn was,
                // and no verdict said after takes it back.
                self.turn_floor = None;
                self.turn_held = false;
                self.turn_by_words = false;
                self.talked(now);
            }
            Voice::Thinking => self.talked(now),
            // The quiet begins when the voice stops, not at its last
            // heartbeat before: an active state is said again once a
            // second, so that one can be most of a second old (the
            // reviewer's finding on #751's progress question, the same).
            Voice::Resting => {
                self.end_own_turn();
                if matches!(
                    self.voice,
                    Some(Voice::Listening | Voice::Thinking | Voice::Speaking)
                ) {
                    self.talked(now);
                }
            }
        }
        self.voice = Some(state);
    }

    /// The turn the lamp began itself is over. One that never made a sound
    /// was not heard: the hello it had taken is a hello chosen again, in its
    /// place among the notes.
    fn end_own_turn(&mut self) {
        if let Some(OwnTurn {
            claimed: Some(at),
            spoke: false,
        }) = self.own_turn.take()
        {
            let place = self.notes.partition_point(|chosen| *chosen <= at);
            self.notes.insert(place, at);
        }
    }

    pub fn voice_gone(&mut self) {
        self.end_own_turn();
        self.voice = None;
        self.turn_floor = None;
        self.turn_held = false;
        self.turn_request = None;
        self.turn_by_words = false;
        self.pending_verdict = None;
    }

    /// A turn begins, counted from the talk before it.
    fn begin_turn(&mut self) {
        self.turn_floor = Some(self.lamp_talk);
        self.turn_held = false;
        self.turn_request = None;
    }

    /// Something was said by the lamp.
    pub fn talk(&mut self, now: Instant) {
        self.last_talk = Some(now);
        self.lamp_talk = Some(now);
    }

    /// Something a person said was heard, in `request` when the voice
    /// numbered it: with [`Options::quiet_to_the_lamp`], not counted in a
    /// turn the voice held back. The newest request heard is the turn's:
    /// talk that goes on, listened to again before the voice rests, is
    /// judged by its newest request.
    pub fn heard(&mut self, now: Instant, request: Option<u64>) {
        // The verdict said already for these words, if one was.
        let judged = match (request, self.pending_verdict) {
            (Some(heard), Some((said, verdict))) if heard == said => {
                self.pending_verdict = None;
                Some(verdict)
            }
            (Some(heard), Some((said, _))) if heard > said => {
                self.pending_verdict = None;
                None
            }
            _ => None,
        };
        // Words of a new request while the voice is not seen listening or
        // thinking begin their turn; a late piece of the turn's own request
        // does not, nor words whose turn was judged before they came.
        if request.is_some()
            && request != self.turn_request
            && judged.is_none()
            && !matches!(self.voice, Some(Voice::Listening | Voice::Thinking))
        {
            self.begin_turn();
            self.turn_by_words = true;
        }
        // A new request in the turn is not judged yet: one held back before
        // it no longer holds back what comes now (Second on #1782: the voice
        // can take a new request while a held answer waits, thinking to
        // listening with no rest between).
        if self.turn_floor.is_some() && request.is_some() {
            if request != self.turn_request {
                self.turn_held = false;
            }
            self.turn_request = request;
            if let Some(verdict) = judged {
                self.judged(verdict);
            }
        }
        self.talked(now);
    }

    /// The voice's verdict on the turn under way, or the last
    /// (`voice/turn`, #1155). With [`Options::quiet_to_the_lamp`], a turn
    /// held back as not said to the lamp is taken back from the quiet,
    /// which counts again from the talk before it; the room's talk is
    /// still told to the model as the room it hears. It may come while the
    /// voice thinks or once it rests. A verdict for a turn the lamp has
    /// spoken in changes nothing.
    pub fn verdict(&mut self, request: u64, verdict: TurnVerdict) {
        if !self.options.quiet_to_the_lamp {
            return;
        }
        match self.turn_request {
            Some(turn) if turn == request => self.judged(verdict),
            // For an earlier request than the turn's: not this turn's.
            Some(turn) if request < turn => {}
            // Its words have not come yet: kept for them.
            _ => self.pending_verdict = Some((request, verdict)),
        }
    }

    /// The turn under way is judged: held back, its talk is taken back.
    fn judged(&mut self, verdict: TurnVerdict) {
        if verdict == TurnVerdict::HeldBack
            && let Some(floor) = self.turn_floor
        {
            self.turn_held = true;
            self.lamp_talk = floor;
        }
    }

    fn hold(&self, arrival: &VoiceArrival, now: Instant, unix_ms: u64, room: Room) -> Option<Hold> {
        let talking = self.talking(now);
        let every_min =
            u32::try_from(self.options.at_most_every.as_secs() / 60).unwrap_or(u32::MAX);
        let recent = arrival
            .since_hello_min
            .is_some_and(|minutes| minutes < every_min)
            || self
                .greeted
                .is_some_and(|at| now.saturating_duration_since(at) < self.options.at_most_every);
        if talking {
            Some(Hold::Talking)
        } else if recent {
            Some(Hold::GreetedRecently)
        } else if self.options.rules
            && self.rules.hold(
                Kind::Hello,
                unix_ms,
                // The zone's offset at this moment, as the progress question
                // takes it (#820).
                self.options
                    .local
                    .as_ref()
                    .map(|local| local.offset_minutes_at(unix_ms)),
                room == Room::Quiet,
            )
        {
            Some(Hold::KeptRule)
        } else {
            None
        }
    }

    /// [`Self::hold`], and for a skill's ask for `person` the hello to the
    /// same person within [`Options::once_per_person`], after the talk and
    /// the twenty minutes and before a kept rule.
    fn hold_for(
        &self,
        arrival: &VoiceArrival,
        person: Option<&str>,
        (now, unix_ms, room): (Instant, u64, Room),
    ) -> Option<Hold> {
        let held = self.hold(arrival, now, unix_ms, room);
        if matches!(held, Some(Hold::Talking | Hold::GreetedRecently)) {
            return held;
        }
        let greeted = person
            .and_then(|person| self.greeted_people.get(person))
            .is_some_and(|at| now.saturating_duration_since(*at) < self.options.once_per_person);
        if greeted {
            Some(Hold::GreetedPerson)
        } else {
            held
        }
    }

    /// Take up an answer if one has come; else decide the next arrival, by
    /// a hard rule or by putting the question. At most one decision a turn.
    pub fn turn(
        &mut self,
        now: Instant,
        unix_ms: u64,
        room: Room,
        (someone_here, facing): (Option<bool>, Option<bool>),
    ) -> Option<Decided> {
        for queue in [&mut self.unasked, &mut self.notes] {
            while queue
                .front()
                .is_some_and(|at| now.saturating_duration_since(*at) > HOUR)
            {
                queue.pop_front();
            }
        }
        let once = self.options.once_per_person;
        self.greeted_people
            .retain(|_, at| now.saturating_duration_since(*at) < once);
        if self.asking.is_some() {
            let answer = match self.answers.try_recv() {
                Ok((answer, took)) => Some((Some(answer), took)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.asker_gone = true;
                    Some((None, Duration::ZERO))
                }
            }?;
            let asking = self.asking.take().expect("a question is out");
            // An answer after its asker stopped waiting is let go, as one
            // never asked about is: nobody takes it up, and it changes
            // nothing kept (the reviewer on #958, 12:19).
            if now.saturating_duration_since(asking.received) >= VOICE_WAITS {
                self.totals.late += 1;
                return None;
            }
            let took = match answer.0 {
                Some(_) => answer.1,
                None => now.saturating_duration_since(asking.sent),
            };
            return Some(self.decide(
                (asking.context, asking.person),
                answer.0,
                None,
                took,
                (now, unix_ms, room),
            ));
        }
        // One the voice has stopped waiting for is let go undecided.
        while self
            .waiting
            .front()
            .is_some_and(|waiting| now.saturating_duration_since(waiting.received) >= VOICE_WAITS)
        {
            self.waiting.pop_front();
            self.totals.late += 1;
        }
        let Waiting {
            arrival,
            received,
            skill_ask,
            person,
        } = self.waiting.pop_front()?;
        let context = Context {
            arrival: arrival.arrival,
            away_min: arrival.away_min,
            there_s: arrival.there_s,
            quiet_s: arrival.quiet_s,
            since_hello_min: arrival.since_hello_min,
            someone_here,
            facing,
            room,
            // In shadow the hellos chosen are counted here, but for one the
            // voice also said by its own rule, which is counted once, as a
            // turn the lamp began ([`ONE_GREETING_WITHIN`]); acting, the
            // voice says them and `voice/state` counts them.
            spoke_up_last_hour: u32::try_from(
                self.unasked.len()
                    + if self.options.act {
                        0
                    } else {
                        self.notes.len()
                    },
            )
            .unwrap_or(u32::MAX),
            hour: self
                .options
                .local
                .as_ref()
                .map(|local| local.hour_of_day(unix_ms)),
            skill_ask,
        };
        if let Some(hold) = self.hold_for(&arrival, person.as_deref(), (now, unix_ms, room)) {
            match hold {
                Hold::Talking => self.totals.held_talking += 1,
                Hold::GreetedRecently => self.totals.held_recent += 1,
                Hold::GreetedPerson => self.totals.held_person += 1,
                Hold::KeptRule => self.totals.held_rule += 1,
            }
            return Some(self.decide(
                (context, person),
                None,
                Some(hold),
                Duration::ZERO,
                (now, unix_ms, room),
            ));
        }
        // By the hard rules alone: a pass says hello now, and the judge is
        // only asked beside it, in shadow (the first builder on #4625).
        if self.options.by_rule {
            let shown = context.clone();
            let mut decided = self.decide(
                (context, person),
                None,
                None,
                Duration::ZERO,
                (now, unix_ms, room),
            );
            decided.line.judge =
                Some(self.ask_in_shadow(shown, decided.line.judgement.choice, unix_ms));
            return Some(decided);
        }
        if self.asker_gone {
            return Some(self.decide(
                (context, person),
                None,
                None,
                Duration::ZERO,
                (now, unix_ms, room),
            ));
        }
        // The model is given only what is left of the wait; with nothing
        // left, the rule answers now.
        let deadline = time_left(
            VOICE_WAITS,
            received,
            now,
            self.options.deadline.min(MOST_ASKING),
        );
        if deadline.is_zero() {
            let failed = Some(Err(Failed::TimedOut));
            return Some(self.decide(
                (context, person),
                failed,
                None,
                Duration::ZERO,
                (now, unix_ms, room),
            ));
        }
        match self.states.try_send((state_text(&context), deadline)) {
            Ok(()) => {
                self.asking = Some(Asking {
                    context,
                    sent: now,
                    received,
                    person,
                });
                None
            }
            Err(_) => {
                self.asker_gone = true;
                Some(self.decide(
                    (context, person),
                    None,
                    None,
                    Duration::ZERO,
                    (now, unix_ms, room),
                ))
            }
        }
    }

    /// With [`Options::by_rule`]: put the question to the judge for a hello
    /// already decided, without waiting for it. Whether it was asked.
    fn ask_in_shadow(&mut self, context: Context, decided: Choice, unix_ms: u64) -> &'static str {
        if self.asker_gone {
            return "not_asked";
        }
        let deadline = self.options.deadline.min(MOST_ASKING);
        match self.states.try_send((state_text(&context), deadline)) {
            Ok(()) => {
                self.judging.push_back(Judging {
                    context,
                    decided,
                    unix_ms,
                });
                "asked_in_shadow"
            }
            Err(mpsc::TrySendError::Full(_)) => "not_asked",
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.asker_gone = true;
                "not_asked"
            }
        }
    }

    /// With [`Options::by_rule`]: take up the judge's answer asked in shadow
    /// if one has come, as a line to write. It decides nothing.
    pub fn judge_turn(&mut self) -> Option<JudgeLine> {
        if self.judging.is_empty() {
            return None;
        }
        let (answer, took) = match self.answers.try_recv() {
            Ok(answered) => answered,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.asker_gone = true;
                self.judging.clear();
                return None;
            }
        };
        let Judging {
            context,
            decided,
            unix_ms,
        } = self.judging.pop_front()?;
        let connection = answer.as_ref().ok().and_then(|asked| asked.connection);
        let (judgement, answers, by, failed, usage) = match answer {
            Ok(asked) => match combine(&asked.answers) {
                Ok((judgement, answers)) => {
                    (Some(judgement), answers, asked.model, None, asked.usage)
                }
                Err(question) => (
                    None,
                    BTreeMap::new(),
                    asked.model,
                    Some(Failed::Missing(question.to_owned())),
                    asked.usage,
                ),
            },
            Err(failed) => (
                None,
                BTreeMap::new(),
                self.judge_name.clone(),
                Some(failed),
                None,
            ),
        };
        let differs = judgement.is_some_and(|judgement| judgement.choice != decided);
        if judgement.is_some() {
            self.totals.judged += 1;
            self.totals.judge_differs += u64::from(differs);
        }
        Some(JudgeLine {
            at_unix_ms: unix_ms,
            about: JUDGE_ABOUT,
            shadow: true,
            state: state(&context),
            context,
            asked: self.judge_name.clone(),
            decided,
            answers,
            judgement,
            differs,
            by,
            took_ms: u32::try_from(took.as_millis()).unwrap_or(u32::MAX),
            connection,
            failed: failed.as_ref().map(Failed::kind),
            failed_why: failed.as_ref().map(Failed::to_string),
            usage,
        })
    }

    /// Decide from the model's answers, by a hard rule (`held`), or by the
    /// rule when there are no answers to use. `person`: whom a skill's ask
    /// is for ([`ONCE_PER_PERSON`]).
    fn decide(
        &mut self,
        (context, person): (Context, Option<String>),
        answer: Option<Result<Asked, Failed>>,
        held: Option<Hold>,
        took: Duration,
        (now, unix_ms, room): (Instant, u64, Room),
    ) -> Decided {
        let connection = answer
            .as_ref()
            .and_then(|a| a.as_ref().ok())
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
            // By the hard rules alone, none holding: a hello now.
            (None, None) if self.options.by_rule => (
                Judgement {
                    probability: RULE_YES,
                    choice: Choice::SayNow,
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
                Some(Failed::Unreachable {
                    during: "asking",
                    kind: io::ErrorKind::BrokenPipe,
                }),
                None,
            ),
        };
        // The hard rules again, as the answer comes: someone began to talk
        // while the model was asked, or a rule was kept meanwhile. The room
        // is as it is now, as the moment and progress questions take it, and
        // a yes held then is counted as one held before the model is (the
        // reviewer on #1003).
        let late = (held.is_none() && judgement.choice == Choice::SayNow)
            .then(|| {
                let arrival = VoiceArrival {
                    arrival: context.arrival,
                    away_min: context.away_min,
                    there_s: context.there_s,
                    quiet_s: context.quiet_s,
                    since_hello_min: context.since_hello_min,
                };
                self.hold_for(&arrival, person.as_deref(), (now, unix_ms, room))
            })
            .flatten();
        match late {
            Some(Hold::Talking) => self.totals.held_talking += 1,
            Some(Hold::GreetedRecently) => self.totals.held_recent += 1,
            Some(Hold::GreetedPerson) => self.totals.held_person += 1,
            Some(Hold::KeptRule) => self.totals.held_rule += 1,
            None => {}
        }
        let guard = held.or(late);
        if guard.is_some() {
            judgement.choice = Choice::Wait;
        }
        if judgement.choice == Choice::SayNow {
            self.notes.push_back(now);
            self.totals.said += 1;
            if self.options.act {
                self.greeted = Some(now);
                if let Some(person) = person {
                    self.greeted_people.insert(person, now);
                }
            }
        }
        self.totals.decided += 1;
        self.totals.by_rule += u64::from(judgement.from_rule);
        let took_ms = u32::try_from(took.as_millis()).unwrap_or(u32::MAX);
        // A skill's ask is named as the ask, a voice's arrival as an arrival.
        let skill_ask = context.skill_ask.then_some(context.arrival);
        let decision = Decision {
            about: ABOUT.to_owned(),
            choice: judgement.choice.as_str().to_owned(),
            probability: judgement.probability,
            answers: answers.clone(),
            by: by.clone(),
            took_ms,
            shadow: !self.options.act,
            request: None,
            task: None,
            arrival: skill_ask.is_none().then_some(context.arrival),
            ask: skill_ask,
            proposal: None,
        };
        let line = Line {
            at_unix_ms: unix_ms,
            about: ABOUT,
            shadow: !self.options.act,
            state: state(&context),
            context,
            asked: self.judge_name.clone(),
            answers,
            judgement,
            by,
            took_ms,
            guard,
            connection,
            failed: failed.as_ref().map(Failed::kind),
            failed_why: failed.as_ref().map(Failed::to_string),
            usage,
            judge: None,
        };
        Decided { decision, line }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            arrival: 98_123_456,
            away_min: 42,
            there_s: 3,
            quiet_s: 310,
            since_hello_min: Some(95),
            someone_here: Some(true),
            facing: Some(true),
            room: Room::Quiet,
            spoke_up_last_hour: 0,
            hour: Some(9),
            skill_ask: false,
        }
    }

    #[test]
    fn the_state_is_this_text_to_the_byte_and_holds_no_picture_or_name() {
        assert_eq!(
            state_text(&context()),
            "{\"lamp\":\"A small desk lamp robot on the desk of the person it works for, like a friend at the next desk: when someone comes back and sits down it may greet them with a few warm words, and otherwise it keeps quiet.\",\
             \"arrival\":{\"minutes_away\":42,\"seconds_there\":3,\"minutes_since_the_lamp_last_said_hello\":95},\
             \"room\":{\"someone_at_the_lamp\":true,\"facing_the_lamp\":true,\"seconds_since_anything_was_said\":310,\
             \"room\":\"quiet: nobody else has been talking\",\"times_the_lamp_spoke_up_unasked_in_the_last_hour\":0,\"hour_of_day\":9}}"
        );
        let unknown = Context {
            since_hello_min: None,
            someone_here: None,
            facing: None,
            hour: None,
            ..context()
        };
        let value = state(&unknown);
        for (part, field) in [
            ("arrival", "minutes_since_the_lamp_last_said_hello"),
            ("room", "someone_at_the_lamp"),
            ("room", "facing_the_lamp"),
            ("room", "hour_of_day"),
        ] {
            assert!(value[part][field].is_null(), "{field}");
        }
    }

    fn answers(two: [f32; 2]) -> BTreeMap<String, Answer> {
        questions()
            .into_iter()
            .zip(two)
            .map(|((name, _), yes)| (name, Answer::Noul(yes)))
            .collect()
    }

    struct Never;
    impl Decider for Never {
        fn ask(&mut self, _: &str, _: &[(String, Question)], _: Duration) -> Result<Asked, Failed> {
            Err(Failed::TimedOut)
        }
        fn name(&self) -> &str {
            "never"
        }
    }

    /// Fourth on #958: a skill's ask out with the moment question alone
    /// keeps the loop looking as often as an arrival does.
    #[test]
    fn a_skills_ask_alone_keeps_the_loop_looking_often() {
        let moment = crate::moment::Moment::start(
            Never,
            crate::moment::Options::new(Duration::from_secs(1), None),
        )
        .unwrap();
        let mut hello = Hello::start(Never, Options::new(Duration::from_secs(1), None))
            .unwrap()
            .with_moment(moment);
        assert!(!hello.busy());
        let ask = lamp_msgs::SkillAsk {
            ask: 1,
            skill: "stretch-break".into(),
            act: "unprompted".into(),
            person: None,
            away_min: None,
            there_s: None,
        };
        assert!(hello.asked_moment(&ask, Instant::now()));
        assert!(hello.busy());
    }

    /// As for the progress question (#751 review, 15:26): `resting` after
    /// speech starts the quiet then, not at the last heartbeat before it;
    /// a `resting` said again moves nothing.
    #[test]
    fn the_quiet_before_a_hello_begins_when_the_voice_stops() {
        let mut hello = Hello::start(Never, Options::new(Duration::from_secs(1), None)).unwrap();
        let arrival = VoiceArrival {
            arrival: 1,
            away_min: 60,
            there_s: 3,
            quiet_s: 300,
            since_hello_min: None,
        };
        let t = Instant::now();
        hello.voice(Voice::Speaking, t);
        let stopped = t + Duration::from_millis(900);
        hello.voice(Voice::Resting, stopped);
        let quiet = hello.options.quiet_for;
        assert_eq!(
            hello.hold(&arrival, t + quiet, 0, Room::Quiet),
            Some(Hold::Talking)
        );
        hello.voice(Voice::Resting, stopped + Duration::from_secs(5));
        assert_eq!(
            hello.hold(
                &arrival,
                stopped + quiet - Duration::from_millis(1),
                0,
                Room::Quiet
            ),
            Some(Hold::Talking)
        );
        assert_eq!(hello.hold(&arrival, stopped + quiet, 0, Room::Quiet), None);
    }

    /// A hello with its quiet counting only talk to the lamp or by it, and
    /// the instant it started.
    fn to_the_lamp(on: bool) -> (Hello, Instant) {
        let options = Options {
            quiet_to_the_lamp: on,
            ..Options::new(Duration::from_secs(1), None)
        };
        let hello = Hello::start(Never, options).unwrap();
        let started = hello.started;
        (hello, started)
    }

    fn arrived() -> VoiceArrival {
        VoiceArrival {
            arrival: 1,
            away_min: 60,
            there_s: 3,
            quiet_s: 300,
            since_hello_min: None,
        }
    }

    /// A turn of the room's talk: listened to, `request` heard, thought
    /// about, and resting again `long` after it began; with its verdict
    /// said while it was thought about, when there is one.
    fn turn(
        hello: &mut Hello,
        at: Instant,
        request: u64,
        verdict: Option<TurnVerdict>,
        long: Duration,
    ) {
        hello.voice(Voice::Listening, at);
        hello.heard(at + long / 4, Some(request));
        hello.voice(Voice::Thinking, at + long / 2);
        if let Some(verdict) = verdict {
            hello.verdict(request, verdict);
        }
        hello.voice(Voice::Resting, at + long);
    }

    /// #1155: an office talks every few seconds, not to the lamp. With the
    /// quiet counting only talk to the lamp, a turn the voice held back is
    /// taken back from it: the hello is not held by the rule once the voice
    /// rests, and the model judges it, the room's talk told as the room. It
    /// is still held while the voice listens or thinks: never over someone
    /// talking.
    #[test]
    fn with_the_quiet_to_the_lamp_a_turn_held_back_is_taken_back_from_the_quiet() {
        let (mut hello, started) = to_the_lamp(true);
        let quiet = hello.options.quiet_for;
        let second = Duration::from_secs(1);
        // Quiet since it started, then the room talks.
        let begun = started + quiet + second;
        hello.voice(Voice::Listening, begun);
        hello.heard(begun + second, Some(5));
        assert_eq!(
            hello.hold(&arrived(), begun + second, 0, Room::Busy),
            Some(Hold::Talking)
        );
        hello.voice(Voice::Thinking, begun + 2 * second);
        hello.verdict(5, TurnVerdict::HeldBack);
        assert_eq!(
            hello.hold(&arrived(), begun + 2 * second, 0, Room::Busy),
            Some(Hold::Talking),
            "never while the voice thinks"
        );
        // Words still coming in the turn held back do not count either.
        hello.heard(begun + 2 * second, Some(5));
        hello.voice(Voice::Resting, begun + 3 * second);
        assert_eq!(
            hello.hold(&arrived(), begun + 3 * second, 0, Room::Busy),
            None
        );
        // And the room again, every few seconds: each held back.
        let mut at = begun + 8 * second;
        for request in 6..10 {
            turn(
                &mut hello,
                at,
                request,
                Some(TurnVerdict::HeldBack),
                3 * second,
            );
            assert_eq!(hello.hold(&arrived(), at + 3 * second, 0, Room::Busy), None);
            at += 6 * second;
        }
        // A verdict may come once the voice rests: the turn is taken back
        // then.
        turn(&mut hello, at, 10, None, 3 * second);
        assert_eq!(
            hello.hold(&arrived(), at + 3 * second, 0, Room::Busy),
            Some(Hold::Talking)
        );
        hello.verdict(10, TurnVerdict::HeldBack);
        assert_eq!(hello.hold(&arrived(), at + 3 * second, 0, Room::Busy), None);
    }

    /// #1155: the switch is the hello's own quiet only. A turn held back
    /// is still talk for the moment question beside it (`--gate`), as for
    /// what the model is shown: a reminder asked at the lull the hello may
    /// take is held by the moment's rule while anyone talks.
    #[test]
    fn with_the_quiet_to_the_lamp_the_moment_question_still_counts_all_talk() {
        let moment = crate::moment::Moment::start(
            Never,
            crate::moment::Options::new(Duration::from_secs(1), None),
        )
        .unwrap();
        let (hello, started) = to_the_lamp(true);
        let mut hello = hello.with_moment(moment);
        let quiet = hello.options.quiet_for;
        let second = Duration::from_secs(1);
        let begun = started + quiet + second;
        turn(
            &mut hello,
            begun,
            5,
            Some(TurnVerdict::HeldBack),
            3 * second,
        );
        let lull = begun + 3 * second;
        assert_eq!(hello.hold(&arrived(), lull, 0, Room::Busy), None);
        let ask = lamp_msgs::SkillAsk {
            ask: 1,
            skill: "stretch-break".into(),
            act: crate::moment::REMINDER.into(),
            person: None,
            away_min: None,
            there_s: None,
        };
        assert!(hello.asked_moment(&ask, lull));
        let decided = hello.moment_turn(lull, 0, Room::Busy, Some(true));
        assert!(decided.is_some(), "a reminder is decided at once");
        assert_eq!(hello.moment_totals().unwrap().held_talking, 1);
    }

    /// #1155: a turn the voice answered is talk to the lamp, and holds the
    /// whole quiet as before; so does the lamp speaking.
    #[test]
    fn with_the_quiet_to_the_lamp_a_turn_answered_holds_the_whole_quiet() {
        let (mut hello, started) = to_the_lamp(true);
        let quiet = hello.options.quiet_for;
        let second = Duration::from_secs(1);
        let begun = started + quiet + second;
        hello.voice(Voice::Listening, begun);
        hello.heard(begun + second, Some(5));
        hello.voice(Voice::Thinking, begun + 2 * second);
        hello.verdict(5, TurnVerdict::Answered);
        hello.voice(Voice::Speaking, begun + 3 * second);
        let stopped = begun + 5 * second;
        hello.voice(Voice::Resting, stopped);
        assert_eq!(
            hello.hold(
                &arrived(),
                stopped + quiet - Duration::from_millis(1),
                0,
                Room::Quiet
            ),
            Some(Hold::Talking)
        );
        assert_eq!(
            hello.hold(&arrived(), stopped + quiet, 0, Room::Quiet),
            None
        );
        // A turn with no verdict at all counts as before, too.
        let next = stopped + quiet + second;
        turn(&mut hello, next, 6, None, 3 * second);
        assert_eq!(
            hello.hold(
                &arrived(),
                next + 3 * second + quiet - Duration::from_millis(1),
                0,
                Room::Quiet
            ),
            Some(Hold::Talking)
        );
    }

    /// #1155: the room's talk goes on, the voice listening again before it
    /// rests: the turn is judged by its newest request, not by a late
    /// verdict for an earlier one, and once held back nothing more in it
    /// counts, its listening again included.
    #[test]
    fn a_turn_that_goes_on_is_judged_by_its_newest_request() {
        let (mut hello, started) = to_the_lamp(true);
        let quiet = hello.options.quiet_for;
        let second = Duration::from_secs(1);
        let begun = started + quiet + second;
        hello.voice(Voice::Listening, begun);
        hello.heard(begun + second / 2, Some(5));
        hello.voice(Voice::Thinking, begun + second);
        hello.voice(Voice::Listening, begun + 2 * second);
        hello.heard(begun + 2 * second + second / 2, Some(6));
        hello.voice(Voice::Thinking, begun + 3 * second);
        hello.verdict(6, TurnVerdict::HeldBack);
        // The same people go on talking in the turn held back.
        hello.voice(Voice::Listening, begun + 3 * second + second / 2);
        assert_eq!(
            hello.hold(&arrived(), begun + 3 * second + second / 2, 0, Room::Busy),
            Some(Hold::Talking),
            "never while the voice listens"
        );
        hello.heard(begun + 3 * second + second / 2, Some(6));
        hello.voice(Voice::Thinking, begun + 4 * second);
        hello.voice(Voice::Resting, begun + 5 * second);
        assert_eq!(
            hello.hold(&arrived(), begun + 5 * second, 0, Room::Busy),
            None
        );
    }

    /// #1155: the voice's state and its words come on two topics, and the
    /// policy may take a turn's first words before it sees the voice
    /// listening. They begin the turn, and the listening seen after them is
    /// the same turn, judged by their request. A late piece of the last
    /// turn's own words, heard as the voice rests, begins none.
    #[test]
    fn words_heard_before_the_voice_is_seen_listening_begin_their_turn() {
        let (mut hello, started) = to_the_lamp(true);
        let quiet = hello.options.quiet_for;
        let second = Duration::from_secs(1);
        let begun = started + quiet + second;
        turn(
            &mut hello,
            begun,
            5,
            Some(TurnVerdict::HeldBack),
            3 * second,
        );
        let next = begun + 4 * second;
        hello.heard(next, Some(5));
        assert_eq!(hello.hold(&arrived(), next, 0, Room::Busy), None);
        hello.heard(next, Some(6));
        hello.voice(Voice::Listening, next);
        hello.voice(Voice::Thinking, next + second);
        hello.verdict(6, TurnVerdict::HeldBack);
        hello.voice(Voice::Resting, next + 2 * second);
        assert_eq!(
            hello.hold(&arrived(), next + 2 * second, 0, Room::Busy),
            None
        );
    }

    /// Second on #1782: a turn held back, and then a new request in it, the
    /// voice listening again with no rest between, as when it takes a new
    /// request while a held answer waits. The new request's talk counts
    /// until it is judged: answered, or with no verdict at all, it holds
    /// the whole quiet; held back too, the turn is taken back again.
    #[test]
    fn a_new_request_in_a_turn_held_back_counts_until_it_is_judged() {
        for second_verdict in [
            None,
            Some(TurnVerdict::Answered),
            Some(TurnVerdict::HeldBack),
        ] {
            let (mut hello, started) = to_the_lamp(true);
            let quiet = hello.options.quiet_for;
            let second = Duration::from_secs(1);
            let begun = started + quiet + second;
            hello.voice(Voice::Listening, begun);
            hello.heard(begun + second / 2, Some(5));
            hello.voice(Voice::Thinking, begun + second);
            hello.verdict(5, TurnVerdict::HeldBack);
            hello.voice(Voice::Listening, begun + 2 * second);
            hello.heard(begun + 2 * second + second / 2, Some(6));
            hello.voice(Voice::Thinking, begun + 3 * second);
            if let Some(verdict) = second_verdict {
                hello.verdict(6, verdict);
            }
            let rested = begun + 4 * second;
            hello.voice(Voice::Resting, rested);
            let held = hello.hold(&arrived(), rested, 0, Room::Busy);
            match second_verdict {
                Some(TurnVerdict::HeldBack) => assert_eq!(held, None),
                _ => {
                    assert_eq!(held, Some(Hold::Talking), "{second_verdict:?}");
                    assert_eq!(
                        hello.hold(&arrived(), rested + quiet, 0, Room::Busy),
                        None,
                        "{second_verdict:?}"
                    );
                }
            }
        }
    }

    /// The first builder on #1817: a line the rule let out and the model
    /// then declined (`not_for_me`) is told answered, then held back. The
    /// held back takes the turn back from the quiet.
    #[test]
    fn a_turn_the_model_declines_after_the_rule_let_it_out_is_taken_back() {
        let (mut hello, started) = to_the_lamp(true);
        let quiet = hello.options.quiet_for;
        let second = Duration::from_secs(1);
        let begun = started + quiet + second;
        hello.voice(Voice::Listening, begun);
        hello.heard(begun + second / 2, Some(5));
        hello.voice(Voice::Thinking, begun + second);
        hello.verdict(5, TurnVerdict::Answered);
        hello.verdict(5, TurnVerdict::HeldBack);
        let rested = begun + 2 * second;
        hello.voice(Voice::Resting, rested);
        assert_eq!(hello.hold(&arrived(), rested, 0, Room::Busy), None);
    }

    /// Second on #1782: the second route says its verdict before the words
    /// it judged go out, so the policy can take the verdict first. It is
    /// kept for its words, whether they come while the voice still thinks
    /// or once it rests, and they are the judged turn's, not a new one.
    #[test]
    fn a_verdict_said_before_its_words_is_kept_for_them() {
        for words_after_rest in [false, true] {
            let (mut hello, started) = to_the_lamp(true);
            let quiet = hello.options.quiet_for;
            let second = Duration::from_secs(1);
            let begun = started + quiet + second;
            hello.voice(Voice::Listening, begun);
            hello.voice(Voice::Thinking, begun + second);
            hello.verdict(1, TurnVerdict::HeldBack);
            let rested = begun + 2 * second;
            if words_after_rest {
                hello.voice(Voice::Resting, rested);
                hello.heard(rested, Some(1));
            } else {
                hello.heard(begun + second, Some(1));
                hello.voice(Voice::Resting, rested);
            }
            assert_eq!(
                hello.hold(&arrived(), rested, 0, Room::Busy),
                None,
                "words after the rest: {words_after_rest}"
            );
        }
    }

    /// A verdict kept for words that never come does not take back a later
    /// request's talk, and one for an earlier request than the turn's is
    /// still not this turn's.
    #[test]
    fn a_kept_verdict_is_only_for_its_own_words() {
        let (mut hello, started) = to_the_lamp(true);
        let quiet = hello.options.quiet_for;
        let second = Duration::from_secs(1);
        let begun = started + quiet + second;
        hello.voice(Voice::Listening, begun);
        hello.voice(Voice::Thinking, begun + second);
        hello.verdict(1, TurnVerdict::HeldBack);
        // Its words never come; the next request's do.
        hello.voice(Voice::Listening, begun + 2 * second);
        hello.heard(begun + 2 * second, Some(2));
        hello.voice(Voice::Thinking, begun + 3 * second);
        let rested = begun + 4 * second;
        hello.voice(Voice::Resting, rested);
        assert_eq!(
            hello.hold(&arrived(), rested, 0, Room::Busy),
            Some(Hold::Talking)
        );
        // A late verdict for request 1 now is for an earlier request.
        hello.verdict(1, TurnVerdict::HeldBack);
        hello.heard(rested, Some(1));
        assert_eq!(
            hello.hold(&arrived(), rested, 0, Room::Busy),
            Some(Hold::Talking)
        );
    }

    /// #1155: a turn held back that the lamp then speaks in, as when it
    /// asks again what was unclear, is talk by the lamp: it counts until
    /// the lamp stops, and a verdict said after changes nothing.
    #[test]
    fn a_turn_held_back_that_the_lamp_speaks_in_counts_until_it_stops() {
        let (mut hello, started) = to_the_lamp(true);
        let quiet = hello.options.quiet_for;
        let second = Duration::from_secs(1);
        let begun = started + quiet + second;
        hello.voice(Voice::Listening, begun);
        hello.heard(begun + second / 2, Some(5));
        hello.voice(Voice::Thinking, begun + second);
        hello.verdict(5, TurnVerdict::HeldBack);
        hello.voice(Voice::Speaking, begun + 2 * second);
        let stopped = begun + 4 * second;
        hello.voice(Voice::Resting, stopped);
        hello.verdict(5, TurnVerdict::HeldBack);
        assert_eq!(
            hello.hold(
                &arrived(),
                stopped + quiet - Duration::from_millis(1),
                0,
                Room::Quiet
            ),
            Some(Hold::Talking)
        );
        assert_eq!(
            hello.hold(&arrived(), stopped + quiet, 0, Room::Quiet),
            None
        );
    }

    /// #1155: without `--hello-quiet to-the-lamp`, all talk counts, held
    /// back or not, as before.
    #[test]
    fn without_the_quiet_to_the_lamp_a_turn_held_back_still_holds_the_quiet() {
        let (mut hello, started) = to_the_lamp(false);
        let quiet = hello.options.quiet_for;
        let second = Duration::from_secs(1);
        let begun = started + quiet + second;
        turn(
            &mut hello,
            begun,
            5,
            Some(TurnVerdict::HeldBack),
            3 * second,
        );
        let stopped = begun + 3 * second;
        assert_eq!(
            hello.hold(
                &arrived(),
                stopped + quiet - Duration::from_millis(1),
                0,
                Room::Busy
            ),
            Some(Hold::Talking)
        );
        assert_eq!(hello.hold(&arrived(), stopped + quiet, 0, Room::Busy), None);
        // A verdict said once the voice rests changes nothing either.
        let next = stopped + quiet + second;
        turn(&mut hello, next, 6, None, 3 * second);
        hello.verdict(6, TurnVerdict::HeldBack);
        assert_eq!(
            hello.hold(&arrived(), next + 3 * second, 0, Room::Busy),
            Some(Hold::Talking)
        );
    }

    /// #1155: a verdict for another turn than the one under way changes
    /// nothing: one for the last turn, said late, does not take back the
    /// next one's talk.
    #[test]
    fn a_verdict_for_another_turn_is_not_this_turns() {
        let (mut hello, started) = to_the_lamp(true);
        let quiet = hello.options.quiet_for;
        let second = Duration::from_secs(1);
        let begun = started + quiet + second;
        hello.voice(Voice::Listening, begun);
        hello.heard(begun + second, Some(8));
        hello.voice(Voice::Thinking, begun + 2 * second);
        hello.verdict(7, TurnVerdict::HeldBack);
        let stopped = begun + 3 * second;
        hello.voice(Voice::Resting, stopped);
        assert_eq!(
            hello.hold(
                &arrived(),
                stopped + quiet - Duration::from_millis(1),
                0,
                Room::Quiet
            ),
            Some(Hold::Talking)
        );
    }

    #[test]
    fn welcome_and_not_intrusive_is_a_hello() {
        let (yes, named) = combine(&answers([0.9, 0.2])).unwrap();
        assert!((yes.probability - 0.72).abs() < 1e-6);
        assert_eq!((yes.choice, yes.from_rule), (Choice::SayNow, false));
        assert_eq!(named.len(), 2);
        assert_eq!(
            combine(&answers([0.9, 0.5])).unwrap().0.choice,
            Choice::Wait
        );
        assert_eq!(
            combine(&answers([0.4, 0.0])).unwrap().0.choice,
            Choice::Wait
        );
        let mut missing = answers([1.0, 0.0]);
        missing.remove("intrusive");
        assert_eq!(combine(&missing).unwrap_err(), "intrusive");
        let asked: Vec<String> = questions().into_iter().map(|(name, _)| name).collect();
        assert_eq!(asked, ["welcome", "intrusive"]);
    }

    #[test]
    fn the_rule_says_hello_unless_busy_late_or_after_two_turns_unasked() {
        assert_eq!(fallback(&context()).choice, Choice::SayNow);
        for changed in [
            Context {
                room: Room::Busy,
                ..context()
            },
            Context {
                spoke_up_last_hour: 2,
                ..context()
            },
            Context {
                hour: Some(23),
                ..context()
            },
            Context {
                hour: Some(6),
                ..context()
            },
        ] {
            let judged = fallback(&changed);
            assert_eq!((judged.choice, judged.probability), (Choice::Wait, RULE_NO));
        }
        // Asking never takes longer than the voice can wait.
        assert_eq!(
            Options::new(Duration::from_secs(3), None).deadline,
            MOST_ASKING
        );
        assert!(MOST_ASKING < VOICE_WAITS);
    }

    /// The decision for `arrival` made at `now`, by the rule (the model
    /// never answers), waited for on the clock the test gives.
    /// The first builder, 30 Oct, on the launch testers' findings: at most
    /// one hello an hour to the same person. Acted on, a hello to Alpha
    /// holds the next ask for Alpha within the hour by the rule, without a
    /// model, while one for Beta is decided; asks without a person share
    /// one key; past the hour Alpha is decided again.
    #[test]
    fn one_hello_an_hour_to_the_same_person_and_asks_without_one_share_a_key() {
        const ALPHA: &str = "p-0123456789abcdef";
        const BETA: &str = "p-fedcba9876543210";
        let options = Options {
            act: true,
            gate: true,
            // The twenty minutes out of the way: the person's hour alone.
            at_most_every: Duration::from_secs(60),
            ..Options::new(Duration::from_millis(50), None)
        };
        let mut hello = Hello::start(Never, options).unwrap();
        let ask = |number: u64, person: Option<&str>| lamp_msgs::SkillAsk {
            ask: number,
            skill: "welcome-back".into(),
            act: "hello".into(),
            person: person.map(str::to_owned),
            away_min: Some(42),
            there_s: Some(3),
        };
        let decide = |hello: &mut Hello, number: u64, person: Option<&str>, at: Instant| {
            hello.asked(&ask(number, person), at);
            for _ in 0..500 {
                if let Some(decided) = hello.turn(at, 1_000_000, Room::Quiet, (Some(true), None)) {
                    return decided;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            panic!("no decision");
        };
        let minutes = |n: u64| Duration::from_secs(n * 60);
        let t = Instant::now();
        let first = decide(&mut hello, 1, Some(ALPHA), t);
        assert_eq!(
            (first.decision.choice.as_str(), first.line.guard),
            ("say_now", None)
        );
        // Alpha again, 40 minutes on: the twenty minutes are past, the
        // hour is not.
        let again = decide(&mut hello, 2, Some(ALPHA), t + minutes(40));
        assert_eq!(
            (
                again.decision.choice.as_str(),
                again.decision.by.as_str(),
                again.line.guard
            ),
            ("wait", crate::policy::RULE, Some(Hold::GreetedPerson))
        );
        assert_eq!(Hold::GreetedPerson.as_str(), "greeted_person");
        // Beta, 42 minutes on: decided.
        let beta = decide(&mut hello, 3, Some(BETA), t + minutes(42));
        assert_eq!(
            (beta.decision.choice.as_str(), beta.line.guard),
            ("say_now", None)
        );
        // A face not known, then another: one key.
        let someone = decide(&mut hello, 4, None, t + minutes(44));
        assert_eq!(someone.decision.choice, "say_now");
        let another = decide(&mut hello, 5, None, t + minutes(46));
        assert_eq!(another.line.guard, Some(Hold::GreetedPerson));
        // Alpha past the hour: decided again.
        let later = decide(&mut hello, 6, Some(ALPHA), t + minutes(61));
        assert_eq!(
            (later.decision.choice.as_str(), later.line.guard),
            ("say_now", None)
        );
        assert_eq!(hello.totals().held_person, 2);
    }

    /// In shadow nothing was said: the person's hour starts only from a
    /// hello acted on, as the twenty minutes do.
    #[test]
    fn a_hello_in_shadow_starts_no_persons_hour() {
        let options = Options {
            gate: true,
            at_most_every: Duration::from_secs(60),
            ..Options::new(Duration::from_millis(50), None)
        };
        let mut hello = Hello::start(Never, options).unwrap();
        let t = Instant::now();
        for (number, at) in [(1, t), (2, t + Duration::from_secs(300))] {
            hello.asked(
                &lamp_msgs::SkillAsk {
                    ask: number,
                    skill: "welcome-back".into(),
                    act: "hello".into(),
                    person: Some("p-0123456789abcdef".into()),
                    away_min: Some(42),
                    there_s: Some(3),
                },
                at,
            );
            let decided = (0..500)
                .find_map(|_| {
                    let decided = hello.turn(at, 1_000_000, Room::Quiet, (Some(true), None));
                    if decided.is_none() {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    decided
                })
                .expect("decided");
            assert_eq!(
                (decided.decision.choice.as_str(), decided.line.guard),
                ("say_now", None)
            );
        }
    }

    fn decided(hello: &mut Hello, arrival: u64, now: Instant) -> Decided {
        hello.arrival(
            VoiceArrival {
                arrival,
                away_min: 60,
                there_s: 3,
                quiet_s: 300,
                since_hello_min: None,
            },
            now,
        );
        for _ in 0..500 {
            if let Some(decided) = hello.turn(now, 1_000_000, Room::Quiet, (Some(true), Some(true)))
            {
                return decided;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("no decision");
    }

    /// #820: with a zone, the hour shown is the zone's at the decision.
    #[test]
    fn with_a_zone_the_hello_is_shown_the_zones_hour() {
        let york = Arc::new(LocalTime::zone("America/New_York").unwrap());
        let options = Options::new(Duration::from_millis(50), Some(york));
        let mut hello = Hello::start(Never, options).unwrap();
        // `decided` decides at 00:16 UTC on 1 January 1970: 19:16 the
        // evening before in New York.
        let first = decided(&mut hello, 1, Instant::now() + Duration::from_millis(500));
        assert_eq!(first.line.context.hour, Some(19));
    }

    /// The reviewer on #883: in shadow, with the voice saying its own rule's
    /// hello, one greeting was counted twice, as the hello chosen and as the
    /// turn the lamp began, and the next arrival's rule answered `wait`. Now
    /// it is counted once, whichever came first; acting, and in shadow with
    /// nothing said, it is counted once as before.
    #[test]
    fn one_greeting_is_counted_once_in_shadow_or_acting() {
        let spoke = |act: bool, voice_says: Option<bool>| {
            let options = Options {
                act,
                ..Options::new(Duration::from_millis(50), None)
            };
            let mut hello = Hello::start(Never, options).unwrap();
            let t = Instant::now();
            // `Some(true)`: the voice says its own rule's hello after the
            // decision; `None`: nothing is said. (Said before the decision
            // came, the lamp is talking and the hard rule answers `wait`:
            // nothing is chosen to count twice.)
            let first = decided(&mut hello, 1, t + Duration::from_millis(500));
            assert_eq!(first.line.judgement.choice, Choice::SayNow);
            if voice_says == Some(true) {
                hello.voice(Voice::Speaking, t + Duration::from_millis(1_500));
                hello.voice(Voice::Resting, t + Duration::from_secs(3));
            }
            let later = t + AT_MOST_EVERY + Duration::from_secs(1);
            let second = decided(&mut hello, 2, later);
            (
                second.line.context.spoke_up_last_hour,
                second.line.judgement.choice,
            )
        };
        assert_eq!(spoke(false, Some(true)), (1, Choice::SayNow));
        assert_eq!(spoke(false, None), (1, Choice::SayNow));
        assert_eq!(spoke(true, Some(true)), (1, Choice::SayNow));
    }

    /// The reviewer on #883 (01:33): the voice's greeting begins with
    /// `thinking` and may make its first sound much later (the model may
    /// take up to 30 s). Followed from its beginning, it is still one
    /// greeting; a turn that ends without a sound leaves the hello chosen
    /// counted once, as chosen.
    #[test]
    fn a_greeting_whose_first_sound_comes_late_is_still_counted_once() {
        let run = |speaks_at: Option<Duration>| {
            let options = Options::new(Duration::from_millis(50), None);
            let mut hello = Hello::start(Never, options).unwrap();
            let t = Instant::now();
            let first = decided(&mut hello, 1, t + Duration::from_millis(500));
            assert_eq!(first.line.judgement.choice, Choice::SayNow);
            // Waiting for the model is said as thinking, once a second.
            hello.voice(Voice::Thinking, t + Duration::from_millis(600));
            hello.voice(Voice::Thinking, t + Duration::from_millis(1_600));
            match speaks_at {
                Some(at) => {
                    hello.voice(Voice::Speaking, t + at);
                    hello.voice(Voice::Resting, t + at + Duration::from_secs(2));
                }
                None => hello.voice(Voice::Resting, t + Duration::from_secs(3)),
            }
            let later = t + AT_MOST_EVERY + Duration::from_secs(1);
            let second = decided(&mut hello, 2, later);
            (
                second.line.context.spoke_up_last_hour,
                second.line.judgement.choice,
            )
        };
        assert_eq!(run(Some(Duration::from_secs(6))), (1, Choice::SayNow));
        assert_eq!(run(Some(Duration::from_secs(30))), (1, Choice::SayNow));
        // Cancelled before its first sound: the hello chosen, once.
        assert_eq!(run(None), (1, Choice::SayNow));
    }

    /// The reviewer on #883 (04:55): one turn of the lamp's own may speak,
    /// think and speak again before it ends (a later generation for the
    /// same owner). It is one turn, counted once; two turns are two.
    #[test]
    fn a_turn_that_speaks_twice_is_one_turn_and_two_turns_are_two() {
        let count = |turns: &[&[Voice]]| {
            let options = Options::new(Duration::from_millis(50), None);
            let mut hello = Hello::start(Never, options).unwrap();
            let t = Instant::now();
            let mut at = t;
            for turn in turns {
                for state in *turn {
                    at += Duration::from_millis(700);
                    hello.voice(*state, at);
                }
                // A while apart, none of it listened to.
                at += Duration::from_secs(60);
            }
            decided(&mut hello, 9, t + Duration::from_secs(600))
                .line
                .context
                .spoke_up_last_hour
        };
        use Voice::{Resting, Speaking, Thinking};
        let twice_in_one: &[Voice] = &[Thinking, Speaking, Thinking, Speaking, Resting];
        assert_eq!(count(&[twice_in_one]), 1);
        let once: &[Voice] = &[Thinking, Speaking, Resting];
        assert_eq!(count(&[once, once]), 2);
        assert_eq!(count(&[twice_in_one, once]), 2);
    }

    /// The reviewer on #883 (05:22): an answer is an answer however slow
    /// its first sound. Two turns asked for, two minutes apart, each
    /// `listening, thinking, speaking, resting`; then a hello. With the
    /// first sound 1 s or 30 s after the person was listened to, nothing
    /// was said unasked, and the rule says hello both times. A turn the
    /// lamp began itself is still counted, also when its sound is late.
    #[test]
    fn a_slow_answer_is_never_counted_as_speaking_up_unasked() {
        let run = |first_sound: Duration, own: bool| {
            let options = Options::new(Duration::from_millis(50), None);
            let mut hello = Hello::start(Never, options).unwrap();
            let t = Instant::now();
            let mut at = t;
            for _ in 0..2 {
                if !own {
                    hello.voice(Voice::Listening, at);
                }
                hello.voice(Voice::Thinking, at + Duration::from_millis(400));
                // Thinking is said again once a second while it waits.
                let mut waited = Duration::from_secs(1);
                while waited < first_sound {
                    hello.voice(Voice::Thinking, at + waited);
                    waited += Duration::from_secs(1);
                }
                hello.voice(Voice::Speaking, at + first_sound);
                hello.voice(Voice::Resting, at + first_sound + Duration::from_secs(3));
                at += Duration::from_secs(120);
            }
            let decision = decided(&mut hello, 9, t + Duration::from_secs(20 * 60));
            (
                decision.line.context.spoke_up_last_hour,
                decision.line.judgement.choice,
            )
        };
        for first_sound in [Duration::from_secs(1), Duration::from_secs(30)] {
            assert_eq!(
                run(first_sound, false),
                (0, Choice::SayNow),
                "{first_sound:?}"
            );
            assert_eq!(run(first_sound, true), (2, Choice::Wait), "{first_sound:?}");
        }
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
                answers: answers([0.9, 0.0]),
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

    #[test]
    fn the_model_is_given_what_is_left_of_the_two_seconds_less_a_margin() {
        let t = Instant::now();
        let ms = Duration::from_millis;
        assert_eq!(time_left(VOICE_WAITS, t, t, MOST_ASKING), MOST_ASKING);
        assert_eq!(
            time_left(VOICE_WAITS, t, t + ms(800), MOST_ASKING),
            ms(1_000)
        );
        assert_eq!(
            time_left(VOICE_WAITS, t, t + ms(1_800), MOST_ASKING),
            Duration::ZERO
        );
        assert_eq!(
            time_left(VOICE_WAITS, t, t + ms(5_000), MOST_ASKING),
            Duration::ZERO
        );
    }

    /// The reviewer on #958 (12:19): each ask carries who it is from to its
    /// decision, and its 2 s count from when it was received. With A out
    /// to the model and four more asks waiting, A's decision still names A
    /// as the ask. B, waiting 0.8 s, is given 1 s, and its answer after
    /// its 2 s is let go and counted late, never published. One with too
    /// little left is answered by the rule at once.
    #[test]
    fn each_ask_keeps_its_own_name_and_its_two_seconds_from_receipt() {
        let ms = Duration::from_millis;
        let (go, gate) = mpsc::channel();
        let given = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let options = Options {
            gate: true,
            ..Options::new(Duration::from_secs(3), None)
        };
        let decider = Held {
            go: gate,
            given: given.clone(),
        };
        let mut hello = Hello::start(decider, options).unwrap();
        let ask = |number: u64| lamp_msgs::SkillAsk {
            ask: number,
            skill: "welcome-back".into(),
            act: "hello".into(),
            person: None,
            away_min: Some(42),
            there_s: Some(3),
        };
        let turn = |hello: &mut Hello, at: Instant| {
            hello.turn(at, 1_000_000, Room::Quiet, (Some(true), None))
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
        hello.asked(&ask(1), t);
        assert!(turn(&mut hello, t).is_none(), "A is out");
        for number in 2..=5 {
            hello.asked(&ask(number), t + ms(100));
        }
        go.send(()).unwrap();
        let a = (0..500)
            .find_map(|_| {
                std::thread::sleep(ms(2));
                turn(&mut hello, t + ms(900))
            })
            .expect("A decided");
        assert_eq!((a.decision.ask, a.decision.arrival), (Some(1), None));
        assert_eq!(a.decision.choice, "say_now");
        // B: received at 0.1 s, asked at 0.9 s.
        assert!(turn(&mut hello, t + ms(900)).is_none(), "B is out");
        assert_eq!(given_now(2), [MOST_ASKING, ms(1_000)]);
        go.send(()).unwrap();
        for _ in 0..500 {
            if hello.totals().late == 1 {
                break;
            }
            assert!(
                turn(&mut hello, t + ms(2_100)).is_none(),
                "B published late"
            );
            std::thread::sleep(ms(2));
        }
        assert_eq!(hello.totals().late, 1);
        // C, D and E, received at 0.1 s, are let go too.
        assert!(turn(&mut hello, t + ms(2_100)).is_none());
        assert_eq!((hello.totals().late, hello.totals().decided), (4, 1));
        // Received at 3 s and taken up at 4.85 s: the rule, at once.
        hello.asked(&ask(6), t + ms(3_000));
        let ruled = turn(&mut hello, t + ms(4_850)).expect("the rule at once");
        assert_eq!(
            (ruled.decision.ask, ruled.decision.by.as_str()),
            (Some(6), crate::policy::RULE)
        );
        assert_eq!(ruled.line.failed, Some("timed_out"));
        assert_eq!(given_now(2).len(), 2);
        // A voice's arrival is still named as an arrival.
        hello.arrival(
            VoiceArrival {
                arrival: 7,
                away_min: 60,
                there_s: 3,
                quiet_s: 300,
                since_hello_min: None,
            },
            t + ms(6_000),
        );
        assert!(turn(&mut hello, t + ms(6_000)).is_none());
        go.send(()).unwrap();
        let arrival = (0..500)
            .find_map(|_| {
                std::thread::sleep(ms(2));
                turn(&mut hello, t + ms(6_500))
            })
            .expect("the arrival decided");
        assert_eq!(
            (arrival.decision.ask, arrival.decision.arrival),
            (None, Some(7))
        );
        assert!(!arrival.line.context.skill_ask);
    }

    /// Answers yes once let go, and counts what it was asked.
    struct Gated {
        go: mpsc::Receiver<()>,
        asked: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    impl Decider for Gated {
        fn ask(&mut self, _: &str, _: &[(String, Question)], _: Duration) -> Result<Asked, Failed> {
            self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.go.recv().map_err(|_| Failed::TimedOut)?;
            Ok(Asked {
                answers: answers([0.9, 0.0]),
                model: "gated".into(),
                took: Duration::ZERO,
                usage: None,
                connection: None,
            })
        }
        fn name(&self) -> &str {
            "gated"
        }
    }

    /// #813: with `--rules`, a rule the people at the lamp kept about
    /// speaking first holds the hello as the other hard rules do: `wait`,
    /// by the rule, with no model asked. A yes that comes back after a rule
    /// was kept is a `wait` too. Without the switch a rule is not obeyed
    /// here.
    #[test]
    fn with_rules_a_kept_rule_holds_the_hello_and_no_model_is_asked() {
        let quiet = Rules {
            person: "everyone".into(),
            quiet: true,
            quiet_mornings: false,
            rules: 1,
            hello: lamp_msgs::Unprompted::default(),
            news: lamp_msgs::Unprompted::default(),
            keep_light_on: false,
        };
        let forgotten = Rules {
            quiet: false,
            ..quiet.clone()
        };
        let (go, gate) = mpsc::channel();
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let options = Options {
            rules: true,
            ..Options::new(Duration::from_secs(1), None)
        };
        let decider = Gated {
            go: gate,
            asked: asked.clone(),
        };
        let mut hello = Hello::start(decider, options).unwrap();
        assert!(hello.obeys_rules());
        let t = Instant::now();
        hello.rules(quiet.clone());
        let held = decided(&mut hello, 1, t);
        assert_eq!(held.line.guard, Some(Hold::KeptRule));
        assert_eq!(held.decision.choice, "wait");
        assert_eq!(held.decision.by, crate::policy::RULE);
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(hello.totals().held_rule, 1);

        // The rule forgotten, the question is put; kept again while the
        // model is asked, the yes that comes back is a wait.
        hello.rules(forgotten);
        hello.arrival(
            VoiceArrival {
                arrival: 2,
                away_min: 60,
                there_s: 3,
                quiet_s: 300,
                since_hello_min: None,
            },
            t,
        );
        let turn = |hello: &mut Hello| hello.turn(t, 1_000_000, Room::Quiet, (Some(true), None));
        assert!(turn(&mut hello).is_none());
        hello.rules(quiet.clone());
        go.send(()).unwrap();
        let answered = (0..500)
            .find_map(|_| {
                std::thread::sleep(Duration::from_millis(2));
                turn(&mut hello)
            })
            .expect("an answer");
        assert_eq!(answered.line.guard, Some(Hold::KeptRule));
        assert_eq!(answered.decision.choice, "wait");
        assert_eq!(answered.decision.by, "gated");
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(hello.totals().said, 0);
        // The late hold is counted under its kind (the reviewer on #1003).
        assert_eq!(hello.totals().held_rule, 2);

        // Without `--rules`, the same rule holds nothing here.
        let mut deaf = Hello::start(Never, Options::new(Duration::from_millis(50), None)).unwrap();
        assert!(!deaf.obeys_rules());
        deaf.rules(quiet);
        let said = decided(&mut deaf, 3, t);
        assert_eq!(
            (said.line.guard, said.decision.choice.as_str()),
            (None, "say_now")
        );
    }

    /// Answers `wait` (welcome 0.1, intrusive 0.8) once let go, and counts
    /// what it was asked.
    struct SaysWait {
        go: mpsc::Receiver<()>,
        asked: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    impl Decider for SaysWait {
        fn ask(&mut self, _: &str, _: &[(String, Question)], _: Duration) -> Result<Asked, Failed> {
            self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.go.recv().map_err(|_| Failed::TimedOut)?;
            Ok(Asked {
                answers: answers([0.1, 0.8]),
                model: "says-wait".into(),
                took: Duration::ZERO,
                usage: None,
                connection: None,
            })
        }
        fn name(&self) -> &str {
            "says-wait"
        }
    }

    fn welcome_back(number: u64, person: &str) -> lamp_msgs::SkillAsk {
        lamp_msgs::SkillAsk {
            ask: number,
            skill: "welcome-back".into(),
            act: "hello".into(),
            person: Some(person.into()),
            away_min: Some(42),
            there_s: Some(3),
        }
    }

    /// The first builder on #4625: with `--hello-by-rule` the hard rules
    /// alone decide. No rule holding, the hello is `say_now` at once, by
    /// the rule, before the judge has answered; the judge, asked in shadow,
    /// says `wait`, and its answer is only written on a `hello_judge` line
    /// and counted: the hello said stands.
    #[test]
    fn a_rule_pass_says_hello_while_the_judge_says_wait() {
        let (go, gate) = mpsc::channel();
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let options = Options {
            act: true,
            gate: true,
            by_rule: true,
            ..Options::new(Duration::from_secs(1), None)
        };
        let decider = SaysWait {
            go: gate,
            asked: asked.clone(),
        };
        let mut hello = Hello::start(decider, options).unwrap();
        assert!(hello.by_rule());
        let t = Instant::now();
        hello.asked(&welcome_back(1, "p-0123456789abcdef"), t);
        // Decided on the first turn: no wait for the model.
        let said = hello
            .turn(t, 1_000_000, Room::Quiet, (Some(true), None))
            .expect("decided at once, by the rule");
        assert_eq!(
            (
                said.decision.choice.as_str(),
                said.decision.by.as_str(),
                said.decision.probability,
                said.decision.took_ms,
                said.decision.shadow,
                said.decision.ask,
            ),
            ("say_now", crate::policy::RULE, RULE_YES, 0, false, Some(1))
        );
        assert!(said.line.judgement.from_rule);
        assert_eq!(said.line.guard, None);
        assert_eq!(said.line.judge, Some("asked_in_shadow"));
        let written = serde_json::to_value(&said.line).unwrap();
        assert_eq!(written["by"], crate::policy::RULE);
        assert_eq!(written["judge"], "asked_in_shadow");
        // The judge is asked beside it and has not answered yet.
        for _ in 0..500 {
            if asked.load(std::sync::atomic::Ordering::SeqCst) == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(hello.busy());
        assert!(hello.judge_turn().is_none());
        go.send(()).unwrap();
        let judged = (0..500)
            .find_map(|_| {
                std::thread::sleep(Duration::from_millis(2));
                hello.judge_turn()
            })
            .expect("the judge's answer");
        let judgement = judged.judgement.expect("a usable answer");
        assert_eq!(judgement.choice, Choice::Wait);
        assert!(judgement.probability < SAY_FROM && !judgement.from_rule);
        assert_eq!(
            (judged.about, judged.shadow, judged.decided, judged.differs),
            (JUDGE_ABOUT, true, Choice::SayNow, true)
        );
        assert_eq!(judged.by, "says-wait");
        assert_eq!(judged.context.arrival, 1);
        let written = serde_json::to_value(&judged).unwrap();
        assert_eq!(
            (
                &written["about"],
                &written["judgement"]["choice"],
                &written["decided"]
            ),
            (&json!("hello_judge"), &json!("wait"), &json!("say_now"))
        );
        // Nothing of it acts: the hello said is counted, and the next turn
        // decides nothing more.
        assert!(!hello.busy());
        assert!(
            hello
                .turn(t, 1_000_000, Room::Quiet, (Some(true), None))
                .is_none()
        );
        let totals = hello.totals();
        assert_eq!(
            (
                totals.decided,
                totals.said,
                totals.by_rule,
                totals.judged,
                totals.judge_differs
            ),
            (1, 1, 1, 1, 1)
        );
    }

    /// By the rule, a hello acted on starts the person's hour and the twenty
    /// minutes as a judged one does: a second ask for the same person within
    /// the hour is `wait`, by the hard rule, with no judge asked, while one
    /// for another person past the twenty minutes is said.
    #[test]
    fn by_rule_the_persons_hour_still_holds_a_second_hello() {
        const ALPHA: &str = "p-0123456789abcdef";
        let (_go, gate) = mpsc::channel();
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let options = Options {
            act: true,
            gate: true,
            by_rule: true,
            ..Options::new(Duration::from_secs(1), None)
        };
        let decider = SaysWait {
            go: gate,
            asked: asked.clone(),
        };
        let mut hello = Hello::start(decider, options).unwrap();
        let minutes = |n: u64| Duration::from_secs(n * 60);
        let t = Instant::now();
        let mut decide = |number: u64, person: &str, at: Instant| {
            hello.asked(&welcome_back(number, person), at);
            hello
                .turn(at, 1_000_000, Room::Quiet, (Some(true), None))
                .expect("decided at once")
        };
        let first = decide(1, ALPHA, t);
        assert_eq!(first.decision.choice, "say_now");
        // Within the twenty minutes: held by them.
        let soon = decide(2, "p-fedcba9876543210", t + minutes(5));
        assert_eq!(
            (
                soon.decision.choice.as_str(),
                soon.line.guard,
                soon.line.judge
            ),
            ("wait", Some(Hold::GreetedRecently), None)
        );
        // Alpha again 40 minutes on: the twenty minutes are past, the hour
        // is not.
        let again = decide(3, ALPHA, t + minutes(40));
        assert_eq!(
            (
                again.decision.choice.as_str(),
                again.decision.by.as_str(),
                again.line.guard,
                again.line.judge
            ),
            ("wait", crate::policy::RULE, Some(Hold::GreetedPerson), None)
        );
        // Beta 42 minutes on: said, by the rule.
        let beta = decide(4, "p-fedcba9876543210", t + minutes(42));
        assert_eq!(
            (beta.decision.choice.as_str(), beta.decision.by.as_str()),
            ("say_now", crate::policy::RULE)
        );
        let totals = hello.totals();
        assert_eq!(
            (totals.said, totals.held_recent, totals.held_person),
            (2, 1, 1)
        );
        // Only the two hellos said were put to the judge, in shadow; the
        // first is still out, so the second waits behind it.
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
