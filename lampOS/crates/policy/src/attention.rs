//! The decision "is this said to the lamp?".
//!
//! A lamp on a desk in a shared office hears much that is not meant for it.
//! Before it answers, it has to judge whether what it just heard was said
//! to it. This is that judgement, and nothing else: what a decision model
//! is shown ([`state`]), what it is asked ([`questions`]), how its seven
//! answers become one probability and one choice ([`combine`]), and what
//! is chosen when no model answers ([`fallback`]).
//!
//! Everything here is arithmetic on what it is given. It reads no clock,
//! no file and no socket, so that the same context always gives the same
//! state and the same answers the same judgement: what the log holds can
//! be judged again later, with other numbers.
use crate::decide::{Answer, Question};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// What this decision is called on the bus and in the log.
pub const ABOUT: &str = "attention";

/// From this probability up, the lamp answers.
pub const ANSWER_FROM: f32 = 0.8;
/// From this one up to [`ANSWER_FROM`], it turns to whoever spoke and
/// waits, without speaking. Below it, it stays as it is.
pub const LOOK_UP_FROM: f32 = 0.4;

/// The lamp is in conversation for this many seconds after it last said
/// something.
pub const IN_CONVERSATION_SECONDS: f32 = 20.0;
/// No more than this of what the lamp last said is shown to a model: the
/// end of it, which is what a person answers.
pub const MOST_SAID_BYTES: usize = 200;

/// What the rule alone gives when it says yes, and when it says no: how
/// often it was right on the examples the weights below were fitted on.
pub const RULE_YES: f32 = 0.74;
pub const RULE_NO: f32 = 0.26;

const WHAT_IT_IS: &str =
    "A small desk lamp robot that can talk. It sits on a desk in a shared office.";
const NAME: &str = "Lamp";
const CALLED_WITH: [&str; 3] = ["Lamp", "Hey Lamp", "Lamp ơi"];
/// What the recogniser has been seen to write for the lamp's name.
const WRITTEN_AS: [&str; 12] = [
    "Milan", "Weiland", "Wei Lan", "Weimar", "Helen", "Liam", "Lam", "Lan", "lamb", "plan", "Man",
    "Hey man",
];

/// How every question begins.
const OPENING: &str = "state.heard is what a speech recogniser wrote for something just said near the lamp; it can mis-write names and words. ";

/// The seven questions: each one's name, the rest of its instruction after
/// [`OPENING`], and the weight of its answer in [`combine`].
///
/// The weights, and the three numbers after them, were fitted on
/// 2026-10-13 on 346 labelled examples from the lamp's own records: a
/// logistic regression on the model's seven answers, on whether the room
/// was quiet and on whether the lamp was in conversation. They are the
/// first fit and not the last: they are to be fitted again as the
/// decisions log grows, and they belong to this wording of the questions
/// and this shape of the state. Change either and they no longer hold.
const ASKED: [(&str, &str, f64); 7] = [
    (
        "calls_lamp",
        "Does state.heard call the lamp: its name, one of state.lamp.people_call_it_with, or one of the mis-writings in state.lamp.the_recogniser_often_writes_its_name_as, at the start or the end of what was said?",
        0.722,
    ),
    (
        "names_someone_else",
        "Does state.heard address some other person or device by name (for example 'Tom,', 'Samantha,', 'Hey Siri'), a name that is not the lamp's and is not in state.lamp.the_recogniser_often_writes_its_name_as?",
        -0.249,
    ),
    (
        "others_business",
        "Is state.heard part of people's own work talk with each other: reports, invoices, clients, meetings, calls, printers, lunch plans, who did or sent or told what, or a reply to something another person said?",
        -2.042,
    ),
    (
        "lamp_could_answer",
        "Could a friendly talking desk lamp sensibly answer state.heard: a general-knowledge question, the time or the weather, a joke or a story, small talk about how it is, a greeting or a good night, or a request about its own light, voice or volume?",
        1.132,
    ),
    (
        "follows_lamp",
        "state.lamp_last_said is what the lamp itself said state.seconds_since_lamp_spoke seconds ago, or null if it has said nothing lately. Is state.heard said back to the lamp about that: a reply, a follow-up question on the same subject, or an instruction such as stop, wait, go on or something else instead? If state.lamp_last_said is null the answer is no.",
        -0.136,
    ),
    (
        "listening_noise",
        "Is state.heard only a listening noise or a scrap: mm-hm, uh-huh, yeah, okay, ha, a cough, one meaningless word, with no question and no request in it?",
        -0.645,
    ),
    (
        "asks_to_stop",
        "Does state.heard ask whoever is speaking to stop, wait, hold on or be quiet?",
        0.031,
    ),
];
/// What the sum starts from.
const BIAS: f64 = 0.263;
/// Added when the room is quiet.
const QUIET: f64 = 1.771;
/// Added when the lamp is in conversation.
const IN_CONVERSATION: f64 = -0.206;

/// The same sum when something on the lamp listens for its name
/// (`name/heard`, `docs/name.md`), which is one more thing known: the
/// weight of each of the seven answers in the order they are asked in,
/// where the sum starts, and what is added for a quiet room, for being
/// in conversation, and for the name having been heard.
///
/// Fitted on the same 182 examples as the weights above, with the sign
/// simulated from what was really played at the rates `lamp-name` showed
/// on lamp-4ace's own microphone on 2026-10-14: heard for 85 in 100 of
/// the calls, for 60 in 100 of the lines the model scores as naming
/// someone else, and for nothing else. They are the mean of 200 such
/// fits, each with its own draw. On the 164 examples kept back that was
/// right 150.8 times against 142 without the sign, and answered what was
/// not for the lamp 7.5 times of 44 against 17. No example in the fit
/// had a real hearing in it.
const WITH_NAME: ([f64; 7], f64, f64, f64, f64) = (
    [0.540, -0.634, -1.704, 0.439, 0.104, -0.588, 0.038],
    -0.328,
    1.683,
    0.081,
    2.346,
);
/// A hearing counts from this score, which is what the rates above were
/// measured at.
pub const NAME_HEARD_FROM: f32 = 0.9;

/// What is added, in the sum without the name and in the one with it, for
/// each unit a line's voice is more like the conversation's asker's than
/// [`LIKE_ASKER_FROM`] ([`Context::like_asker`], #4306). Inside a
/// conversation a follow-up and desk talk look alike to the seven questions
/// (on lamp-4ace's played rounds of 30 Oct, `follows_lamp` 0.61 against
/// 0.42, `others_business` 0.57 against 0.56); whose voice it is tells them
/// apart. 0 until fitted on a lamp's run with the sign, so every decision is
/// the one it was without it.
const LIKE_ASKER: f64 = 0.0;
const WITH_NAME_LIKE_ASKER: f64 = 0.0;
/// Where the term is 0: the voice's own bar for two prints being one
/// person's (`lamp-voice`'s `same_voice::SAME_FROM`). Above it the term
/// counts for the line, below it against, by the fitted weight.
pub const LIKE_ASKER_FROM: f32 = 0.70;

/// What the room sounds like, as far as anyone has judged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Room {
    /// Nobody else has been talking.
    Quiet,
    /// People are talking to each other.
    Busy,
    Unknown,
}

impl Room {
    /// The room as a model is told it.
    fn told(self) -> &'static str {
        match self {
            Self::Quiet => "quiet: nobody else has been talking",
            Self::Busy => "busy: people are talking to each other",
            Self::Unknown => "unknown",
        }
    }
}

/// What was known when something was heard. Read back from the decisions
/// log by [`crate::replay`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Context {
    /// What the recogniser wrote for what was said.
    pub heard: String,
    pub room: Room,
    /// The lamp was speaking when this began to be heard.
    pub lamp_is_speaking: bool,
    /// The end of what the lamp had last said by then ([`last_words`]), if
    /// it had said anything.
    pub lamp_last_said: Option<String>,
    /// How long before this began to be heard the lamp had last spoken, in
    /// seconds.
    pub seconds_since_lamp_spoke: Option<f32>,
    /// The highest score with which the lamp's name was heard in its own
    /// microphone's sound while this was being said, or nothing if it
    /// was not heard. Not there at all when nothing on the lamp is
    /// listening for the name: not heard and not known are two things.
    /// The model is not shown it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_heard: Option<f32>,
    /// How alike the voice of what was said is to the voice of the person
    /// the lamp is in conversation with, who said the line that began it:
    /// the cosine of the two voice prints, from -1 to 1, as the voice said
    /// it for the request on `attention/addressed`. Nothing when the voice
    /// did not say it: no same-voice check, no conversation going on, a
    /// print not known, or no request to join it by. The model is not shown
    /// it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub like_asker: Option<f32>,
}

impl Context {
    /// Something on the lamp was listening for its name, and heard it.
    pub fn was_called(&self) -> bool {
        self.name_heard
            .is_some_and(|score| score >= NAME_HEARD_FROM)
    }

    /// The lamp is speaking, or said something with a letter or a digit in
    /// it within the last [`IN_CONVERSATION_SECONDS`]. An answer that was
    /// only "..." is the lamp keeping quiet aloud, and starts nothing.
    pub fn in_conversation(&self) -> bool {
        let said_words = self
            .lamp_last_said
            .as_deref()
            .is_some_and(|said| said.chars().any(char::is_alphanumeric));
        let lately = self
            .seconds_since_lamp_spoke
            .is_some_and(|seconds| seconds <= IN_CONVERSATION_SECONDS);
        self.lamp_is_speaking || (said_words && lately)
    }
}

/// Whether `heard` names the lamp in words: one of the ways people call it
/// or the recogniser has been seen to write its name, as a whole word or
/// words, in any case. For counting named requests apart (#871); it decides
/// nothing.
pub fn names_the_lamp(heard: &str) -> bool {
    let words: Vec<String> = heard
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    CALLED_WITH.iter().chain(WRITTEN_AS.iter()).any(|form| {
        let form: Vec<String> = form.split_whitespace().map(str::to_lowercase).collect();
        words
            .windows(form.len())
            .any(|window| window == form.as_slice())
    })
}

/// The end of `said`: at most the last [`MOST_SAID_BYTES`] of it, begun at
/// a whole character, or nothing if there is nothing in it.
pub fn last_words(said: &str) -> Option<String> {
    let said = said.trim();
    let mut from = said.len().saturating_sub(MOST_SAID_BYTES);
    while !said.is_char_boundary(from) {
        from += 1;
    }
    let end = said[from..].trim_start();
    (!end.is_empty()).then(|| end.to_owned())
}

/// The three things in a state that depend on whether the lamp is in
/// conversation: outside one, what it said earlier is not shown.
fn lamp_now(context: &Context) -> (bool, Value, Value) {
    if !context.in_conversation() {
        return (false, Value::Null, Value::Null);
    }
    let said = context.lamp_last_said.as_deref().and_then(last_words);
    // To a tenth of a second, which is more than the moment is known to.
    let seconds = context
        .seconds_since_lamp_spoke
        .map(|seconds| (f64::from(seconds.max(0.0)) * 10.0).round() / 10.0);
    (context.lamp_is_speaking, json!(said), json!(seconds))
}

/// What a decision model is shown: the state as a value, for a log or a
/// test. What is sent is [`state_text`], which says the same.
pub fn state(context: &Context) -> Value {
    let (speaking, said, seconds) = lamp_now(context);
    json!({
        "heard": context.heard,
        "lamp": {
            "what_it_is": WHAT_IT_IS,
            "name": NAME,
            "people_call_it_with": CALLED_WITH,
            "the_recogniser_often_writes_its_name_as": WRITTEN_AS,
        },
        "room": context.room.told(),
        "lamp_is_speaking_now": speaking,
        "lamp_last_said": said,
        "seconds_since_lamp_spoke": seconds,
    })
}

/// [`state`] as the text that is sent, its fields in the order the state
/// was given in when the model was measured. A JSON value has no order of
/// its own and the library writes one in the order of the alphabet, so the
/// text is put together here: the weights were fitted to the answers a
/// model gave to one text, and it may read another order differently.
pub fn state_text(context: &Context) -> String {
    let (speaking, said, seconds) = lamp_now(context);
    format!(
        "{{\"heard\":{},\"lamp\":{{\"what_it_is\":{},\"name\":{},\"people_call_it_with\":{},\
         \"the_recogniser_often_writes_its_name_as\":{}}},\"room\":{},\
         \"lamp_is_speaking_now\":{speaking},\"lamp_last_said\":{said},\
         \"seconds_since_lamp_spoke\":{seconds}}}",
        json!(context.heard),
        json!(WHAT_IT_IS),
        json!(NAME),
        json!(CALLED_WITH),
        json!(WRITTEN_AS),
        json!(context.room.told()),
    )
}

/// [`state_text`] with a picture (#871): the same text, with `picture`
/// last, holding `key`, the string the service replaces with the picture.
pub fn state_text_with_picture(context: &Context, key: &str) -> String {
    let mut text = state_text(context);
    text.pop();
    text.push_str(",\"picture\":");
    text.push_str(&json!(key).to_string());
    text.push('}');
    text
}

/// What is asked about a picture (#871), each its own simple question,
/// beside the seven: signs for the policy, none of which decides alone.
/// Whether anyone is in it comes first: on lamp-4ace on 2026-10-16 at 02:28,
/// with nobody at the desk, "two people talking" was answered 0.62 to 0.69,
/// so the questions that take a person for granted are read only given one
/// (the first builder on #871).
const PICTURE_ASKED: [(&str, &str); 6] = [
    (
        "picture_person",
        "Is there at least one person in state.picture?",
    ),
    (
        "picture_faces_lamp",
        "Is a person in state.picture looking toward the camera?",
    ),
    (
        "picture_absorbed",
        "Is the person nearest the camera in state.picture absorbed in something else: typing, reading, on a phone call, or wearing headphones?",
    ),
    (
        "picture_talking_to_another",
        "Are two or more people in state.picture, facing each other, or one talking to another?",
    ),
    (
        "picture_asks_with_a_look",
        "Is a person in state.picture waving at the camera, or looking at it as if asking something?",
    ),
    (
        "picture_away",
        "Is nobody at the desk in state.picture, or is the person there asleep?",
    ),
];

/// How each picture question begins.
const PICTURE_OPENING: &str =
    "state.picture is what the lamp's own camera sees now, from the desk. ";

/// The seven questions and the picture's six (#871), in that order.
pub fn questions_with_picture() -> Vec<(String, Question)> {
    let mut asked = questions();
    asked.extend(PICTURE_ASKED.iter().map(|(name, rest)| {
        (
            (*name).to_owned(),
            Question::Noul {
                instructions: format!("{PICTURE_OPENING}{rest}"),
            },
        )
    }));
    asked
}

/// The names of the picture's questions.
pub fn picture_questions() -> impl Iterator<Item = &'static str> {
    PICTURE_ASKED.iter().map(|(name, _)| *name)
}

/// The seven questions, in the order they are asked in.
pub fn questions() -> Vec<(String, Question)> {
    ASKED
        .iter()
        .map(|(name, rest, _)| {
            let instructions = format!("{OPENING}{rest}");
            ((*name).to_owned(), Question::Noul { instructions })
        })
        .collect()
}

/// A decision model's answers to the seven questions, each the probability
/// of yes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Answers([f32; 7]);

impl Answers {
    /// The seven from what a model answered, or the name of the first
    /// question that has no yes-or-no answer there.
    pub fn of(answers: &BTreeMap<String, Answer>) -> Result<Self, &'static str> {
        let mut seven = [0.0; 7];
        for (to, (name, _, _)) in seven.iter_mut().zip(ASKED) {
            match answers.get(name) {
                Some(Answer::Noul(yes)) if (0.0..=1.0).contains(yes) => *to = *yes,
                _ => return Err(name),
            }
        }
        Ok(Self(seven))
    }

    /// The answer to the question of this name; 0 for a name not asked.
    pub fn of_name(&self, name: &str) -> f32 {
        ASKED
            .iter()
            .position(|(asked, _, _)| *asked == name)
            .map_or(0.0, |index| self.0[index])
    }

    /// Each answer by its question's name.
    pub fn by_name(&self) -> BTreeMap<String, f32> {
        ASKED
            .iter()
            .zip(self.0)
            .map(|((name, _, _), yes)| ((*name).to_owned(), yes))
            .collect()
    }
}

/// What the lamp would do about what it heard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// Answer it.
    Answer,
    /// Turn to whoever spoke and wait, without speaking.
    LookUp,
    /// Go on as it was.
    StayQuiet,
}

impl Choice {
    /// The choice a probability comes to.
    pub fn at(probability: f32) -> Self {
        if probability >= ANSWER_FROM {
            Self::Answer
        } else if probability >= LOOK_UP_FROM {
            Self::LookUp
        } else {
            Self::StayQuiet
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::LookUp => "look_up",
            Self::StayQuiet => "stay_quiet",
        }
    }
}

/// The judgement: how likely it is that this was said to the lamp, and the
/// choice that comes to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Judgement {
    pub probability: f32,
    pub choice: Choice,
    /// No model's answers are behind it: the rule decided ([`fallback`]).
    pub from_rule: bool,
}

/// One probability from the seven answers, the room and the conversation:
/// a weighted sum put through the logistic curve.
pub fn combine(answers: &Answers, context: &Context) -> Judgement {
    judged(sum(answers, context))
}

/// From this `follows_lamp` answer up, [`combine_next`] adds
/// [`NEXT_FOLLOWS_WEIGHT`] for each unit above it, in conversation.
pub const NEXT_FOLLOWS_FROM: f32 = 0.55;
pub const NEXT_FOLLOWS_WEIGHT: f64 = 3.0;

/// The second judgement proposed on #1040, in shadow (`lamp-policy
/// --attention-next`): [`combine`]'s sum and, in conversation only,
/// [`NEXT_FOLLOWS_WEIGHT`] times how far `follows_lamp` is above
/// [`NEXT_FOLLOWS_FROM`]. One-sided, so it is never below [`combine`]'s,
/// and out of conversation it is the same: the fitted weights count a reply
/// to the lamp against answering (`follows_lamp` −0.136, in conversation
/// −0.206), though the answer tells such replies apart. From the first
/// builder's 79 hand-read `stay_quiet` turns in conversation on lamp-4ace on
/// 2026-10-16: a logistic fit of all 79 labels on `follows_lamp` alone,
/// with a ridge (L2) penalty of 0.5 on the summed log loss and the bias not
/// penalised, gave +3.02 centred at 0.55; scored with each row left out in
/// turn, 14 of the 33 said to the lamp and 1 of the 46 not reach 0.6. A
/// plain fit gives +5.02 centred at 0.51 (17 and 7): the penalty is what
/// keeps the term this cautious. With 3 and 0.55, 13 of the 33 said to the
/// lamp leave `stay_quiet` and none of the 46 not (the reviewer on #1106).
pub fn combine_next(answers: &Answers, context: &Context) -> Judgement {
    let mut sum = sum(answers, context);
    if context.in_conversation() {
        let above = (answers.of_name("follows_lamp") - NEXT_FOLLOWS_FROM).max(0.0);
        sum += NEXT_FOLLOWS_WEIGHT * f64::from(above);
    }
    judged(sum)
}

/// The probability a sum comes to, and its choice.
fn judged(sum: f64) -> Judgement {
    let probability = (1.0 / (1.0 + (-sum).exp())) as f32;
    Judgement {
        probability,
        choice: Choice::at(probability),
        from_rule: false,
    }
}

/// [`combine`]'s weighted sum, before the logistic curve.
fn sum(answers: &Answers, context: &Context) -> f64 {
    // With something listening for the name, the weights that were
    // fitted with that sign; with nothing listening, the ones without.
    let listening = context.name_heard.is_some();
    let (bias, quiet, in_conversation) = if listening {
        (WITH_NAME.1, WITH_NAME.2, WITH_NAME.3)
    } else {
        (BIAS, QUIET, IN_CONVERSATION)
    };
    let mut sum = bias;
    for (index, ((_, _, weight), yes)) in ASKED.iter().zip(answers.0).enumerate() {
        let weight = if listening {
            WITH_NAME.0[index]
        } else {
            *weight
        };
        sum += weight * f64::from(yes);
    }
    if context.room == Room::Quiet {
        sum += quiet;
    }
    if context.in_conversation() {
        sum += in_conversation;
    }
    if context.was_called() {
        sum += WITH_NAME.4;
    }
    // Only when the voice said it, and only a cosine: at a weight of 0 the
    // term adds nothing, and the sum is the one before it (#4306).
    if let Some(like) = context
        .like_asker
        .filter(|like| like.is_finite() && (-1.0..=1.0).contains(like))
    {
        let weight = if listening {
            WITH_NAME_LIKE_ASKER
        } else {
            LIKE_ASKER
        };
        sum += weight * f64::from(like - LIKE_ASKER_FROM);
    }
    sum
}

/// The judgement when no model answers: the rule the fitted weights were
/// measured against. It answers when the room is quiet or the lamp is in
/// conversation and stays quiet otherwise, and was right 74 times in 100
/// on the same examples, which is the probability it gives. A lamp that
/// heard its own name answers too.
pub fn fallback(context: &Context) -> Judgement {
    let yes = context.room == Room::Quiet || context.in_conversation() || context.was_called();
    Judgement {
        probability: if yes { RULE_YES } else { RULE_NO },
        choice: if yes {
            Choice::Answer
        } else {
            Choice::StayQuiet
        },
        from_rule: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(text: &str) -> Context {
        Context {
            heard: text.into(),
            room: Room::Quiet,
            lamp_is_speaking: false,
            lamp_last_said: None,
            seconds_since_lamp_spoke: None,
            name_heard: None,
            like_asker: None,
        }
    }

    fn answers(seven: [f32; 7]) -> Answers {
        let named = ASKED
            .iter()
            .zip(seven)
            .map(|((name, _, _), yes)| ((*name).to_owned(), Answer::Noul(yes)))
            .collect();
        Answers::of(&named).unwrap()
    }

    const LAMP: &str = "\"lamp\":{\"what_it_is\":\"A small desk lamp robot that can talk. It sits on a desk in a shared office.\",\"name\":\"Lamp\",\"people_call_it_with\":[\"Lamp\",\"Hey Lamp\",\"Lamp ơi\"],\"the_recogniser_often_writes_its_name_as\":[\"Milan\",\"Weiland\",\"Wei Lan\",\"Weimar\",\"Helen\",\"Liam\",\"Lam\",\"Lan\",\"lamb\",\"plan\",\"Man\",\"Hey man\"]}";

    #[test]
    fn outside_a_conversation_the_state_is_exactly_the_one_the_model_was_measured_with() {
        let context = heard("...");
        // The text of the brief the weights came with, byte for byte.
        let measured = "{\"heard\":\"...\",\"lamp\":{\"what_it_is\":\"A small desk lamp robot that can talk. It sits on a desk in a shared office.\",\"name\":\"Lamp\",\"people_call_it_with\":[\"Lamp\",\"Hey Lamp\",\"Lamp ơi\"],\"the_recogniser_often_writes_its_name_as\":[\"Milan\",\"Weiland\",\"Wei Lan\",\"Weimar\",\"Helen\",\"Liam\",\"Lam\",\"Lan\",\"lamb\",\"plan\",\"Man\",\"Hey man\"]},\"room\":\"quiet: nobody else has been talking\",\"lamp_is_speaking_now\":false,\"lamp_last_said\":null,\"seconds_since_lamp_spoke\":null}";
        assert_eq!(state_text(&context), measured);
        assert_eq!(
            state(&context),
            serde_json::from_str::<Value>(measured).unwrap()
        );
        // What the lamp said a while ago is known and not shown.
        let earlier = Context {
            heard: "Did the invoice go out, Tom?".into(),
            room: Room::Busy,
            lamp_last_said: Some("It is ten past four.".into()),
            seconds_since_lamp_spoke: Some(20.5),
            ..context.clone()
        };
        assert!(!earlier.in_conversation());
        assert_eq!(
            state_text(&earlier),
            format!(
                "{{\"heard\":\"Did the invoice go out, Tom?\",{LAMP},\"room\":\"busy: people are talking to each other\",\"lamp_is_speaking_now\":false,\"lamp_last_said\":null,\"seconds_since_lamp_spoke\":null}}"
            )
        );
        let unknown = Context {
            room: Room::Unknown,
            ..context
        };
        assert_eq!(state(&unknown)["room"], "unknown");
        assert!(state_text(&unknown).contains(",\"room\":\"unknown\","));
    }

    #[test]
    fn in_a_conversation_the_state_says_what_the_lamp_said_and_how_long_ago() {
        let replying = Context {
            heard: "And \"tomorrow\"?\nGo on.".into(),
            room: Room::Unknown,
            lamp_is_speaking: false,
            lamp_last_said: Some("It will rain this afternoon.".into()),
            seconds_since_lamp_spoke: Some(3.44),
            name_heard: None,
            like_asker: None,
        };
        assert!(replying.in_conversation());
        let text = state_text(&replying);
        assert_eq!(
            text,
            format!(
                "{{\"heard\":\"And \\\"tomorrow\\\"?\\nGo on.\",{LAMP},\"room\":\"unknown\",\"lamp_is_speaking_now\":false,\"lamp_last_said\":\"It will rain this afternoon.\",\"seconds_since_lamp_spoke\":3.4}}"
            )
        );
        // The value says the same as the text, whatever is in it.
        let speaking = Context {
            lamp_is_speaking: true,
            lamp_last_said: None,
            seconds_since_lamp_spoke: Some(0.0),
            ..replying.clone()
        };
        let over = Context {
            lamp_is_speaking: true,
            seconds_since_lamp_spoke: Some(95.0),
            ..replying.clone()
        };
        for context in [&replying, &speaking, &over, &heard("Lamp ơi, mấy giờ rồi?")] {
            let value: Value = serde_json::from_str(&state_text(context)).unwrap();
            assert_eq!(value, state(context));
        }
        assert_eq!(state(&speaking)["lamp_is_speaking_now"], true);
        assert_eq!(state(&speaking)["lamp_last_said"], Value::Null);
        assert_eq!(state(&speaking)["seconds_since_lamp_spoke"], 0.0);
        assert_eq!(state(&over)["seconds_since_lamp_spoke"], 95.0);
    }

    #[test]
    fn a_conversation_is_the_lamp_speaking_or_words_from_it_in_the_last_twenty_seconds() {
        let said = |said: &str, seconds: f32| Context {
            lamp_last_said: Some(said.into()),
            seconds_since_lamp_spoke: Some(seconds),
            ..heard("okay")
        };
        assert!(said("Black.", 0.0).in_conversation());
        assert!(said("Black.", 20.0).in_conversation());
        assert!(!said("Black.", 20.1).in_conversation());
        assert!(said("7", 5.0).in_conversation());
        // Keeping quiet aloud is not a turn in a conversation.
        assert!(!said("...", 1.0).in_conversation());
        assert!(!said("", 1.0).in_conversation());
        assert!(!heard("okay").in_conversation());
        // Words without a time, or a time without words, are not one.
        let timeless = Context {
            seconds_since_lamp_spoke: None,
            ..said("Black.", 0.0)
        };
        assert!(!timeless.in_conversation());
        let wordless = Context {
            lamp_last_said: None,
            ..said("Black.", 2.0)
        };
        assert!(!wordless.in_conversation());
        // While it speaks it is in one, whatever else is known.
        let speaking = Context {
            lamp_is_speaking: true,
            ..heard("stop")
        };
        assert!(speaking.in_conversation());
    }

    #[test]
    fn the_seven_questions_are_asked_by_name_in_order_each_after_the_same_opening() {
        let questions = questions();
        let names: Vec<&str> = questions.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "calls_lamp",
                "names_someone_else",
                "others_business",
                "lamp_could_answer",
                "follows_lamp",
                "listening_noise",
                "asks_to_stop",
            ]
        );
        // Word for word what the model was measured with. The weights
        // belong to this wording: a change here is a change to refit for.
        let measured = [
            "Does state.heard call the lamp: its name, one of state.lamp.people_call_it_with, or one of the mis-writings in state.lamp.the_recogniser_often_writes_its_name_as, at the start or the end of what was said?",
            "Does state.heard address some other person or device by name (for example 'Tom,', 'Samantha,', 'Hey Siri'), a name that is not the lamp's and is not in state.lamp.the_recogniser_often_writes_its_name_as?",
            "Is state.heard part of people's own work talk with each other: reports, invoices, clients, meetings, calls, printers, lunch plans, who did or sent or told what, or a reply to something another person said?",
            "Could a friendly talking desk lamp sensibly answer state.heard: a general-knowledge question, the time or the weather, a joke or a story, small talk about how it is, a greeting or a good night, or a request about its own light, voice or volume?",
            "state.lamp_last_said is what the lamp itself said state.seconds_since_lamp_spoke seconds ago, or null if it has said nothing lately. Is state.heard said back to the lamp about that: a reply, a follow-up question on the same subject, or an instruction such as stop, wait, go on or something else instead? If state.lamp_last_said is null the answer is no.",
            "Is state.heard only a listening noise or a scrap: mm-hm, uh-huh, yeah, okay, ha, a cough, one meaningless word, with no question and no request in it?",
            "Does state.heard ask whoever is speaking to stop, wait, hold on or be quiet?",
        ];
        for ((name, question), rest) in questions.iter().zip(measured) {
            let whole = format!(
                "state.heard is what a speech recogniser wrote for something just said near the lamp; it can mis-write names and words. {rest}"
            );
            assert_eq!(
                *question,
                Question::Noul {
                    instructions: whole
                },
                "{name}"
            );
        }
    }

    #[test]
    fn the_four_measured_examples_come_to_the_measured_probabilities() {
        let busy = Context {
            room: Room::Busy,
            ..heard("x")
        };
        let quiet = heard("x");
        assert!(!busy.in_conversation() && !quiet.in_conversation());
        for (seven, context, expected, choice) in [
            (
                [0.07, 0.06, 0.28, 0.73, 0.06, 0.05, 0.09],
                &quiet,
                0.9078,
                Choice::Answer,
            ),
            (
                [0.10, 0.09, 0.33, 0.91, 0.07, 0.03, 0.06],
                &busy,
                0.6552,
                Choice::LookUp,
            ),
            (
                [0.04, 0.03, 0.10, 0.85, 0.06, 0.04, 0.02],
                &quiet,
                0.9416,
                Choice::Answer,
            ),
            (
                [0.07, 0.11, 0.77, 0.66, 0.07, 0.05, 0.10],
                &busy,
                0.3594,
                Choice::StayQuiet,
            ),
        ] {
            let judged = combine(&answers(seven), context);
            assert!(
                (judged.probability - expected).abs() <= 0.002,
                "{seven:?}: {} and not {expected}",
                judged.probability
            );
            assert_eq!(judged.choice, choice, "{seven:?}");
            assert!(!judged.from_rule);
        }
    }

    #[test]
    fn a_quiet_room_adds_to_the_sum_and_a_conversation_takes_a_little_away() {
        let seven = answers([0.5; 7]);
        let logit = |judged: Judgement| {
            let p = f64::from(judged.probability);
            (p / (1.0 - p)).ln()
        };
        let quiet = heard("x");
        let busy = Context {
            room: Room::Busy,
            ..quiet.clone()
        };
        let unknown = Context {
            room: Room::Unknown,
            ..quiet.clone()
        };
        // Half of each weight, and the bias: -0.3305.
        let plain = logit(combine(&seven, &busy));
        assert!((plain - (0.263 - 0.5935)).abs() < 1e-4, "{plain}");
        assert!((logit(combine(&seven, &quiet)) - plain - 1.771).abs() < 1e-4);
        // A room nobody has judged is not taken for a quiet one.
        assert_eq!(combine(&seven, &unknown), combine(&seven, &busy));
        let talking = Context {
            lamp_is_speaking: true,
            ..busy.clone()
        };
        assert!((logit(combine(&seven, &talking)) - plain + 0.206).abs() < 1e-4);
    }

    /// #1040, in shadow: in conversation, `follows_lamp` above 0.55 adds
    /// three for each unit above it; below it, and out of conversation,
    /// the judgement is today's, so it is never lower.
    #[test]
    fn the_next_judgement_raises_a_reply_to_the_lamp_in_conversation_only() {
        let logit = |judged: Judgement| {
            let p = f64::from(judged.probability);
            (p / (1.0 - p)).ln()
        };
        let names: Vec<&str> = ASKED.iter().map(|(name, _, _)| *name).collect();
        let follows = names
            .iter()
            .position(|name| *name == "follows_lamp")
            .unwrap();
        let with = |yes: f32| {
            let mut seven = [0.07, 0.06, 0.28, 0.73, 0.06, 0.05, 0.09];
            seven[follows] = yes;
            answers(seven)
        };
        assert_eq!(with(0.82).of_name("follows_lamp"), 0.82);
        assert_eq!(with(0.82).of_name("nothing_asked"), 0.0);
        let mut talking = heard("And tomorrow?");
        talking.room = Room::Busy;
        talking.lamp_last_said = Some("It will rain tomorrow.".into());
        talking.seconds_since_lamp_spoke = Some(2.0);
        assert!(talking.in_conversation());
        let raised =
            logit(combine_next(&with(0.82), &talking)) - logit(combine(&with(0.82), &talking));
        assert!((raised - 3.0 * 0.27).abs() < 1e-4, "{raised}");
        // At or below 0.55, and out of conversation, nothing changes.
        for yes in [0.55, 0.3, 0.0] {
            assert_eq!(
                combine_next(&with(yes), &talking),
                combine(&with(yes), &talking)
            );
        }
        let mut alone = talking.clone();
        alone.lamp_last_said = None;
        alone.seconds_since_lamp_spoke = None;
        assert!(!alone.in_conversation());
        assert_eq!(
            combine_next(&with(0.95), &alone),
            combine(&with(0.95), &alone)
        );
    }

    #[test]
    fn with_something_listening_for_the_name_the_sum_is_the_one_fitted_with_that_sign() {
        let seven = answers([0.5; 7]);
        let logit = |judged: Judgement| {
            let p = f64::from(judged.probability);
            (p / (1.0 - p)).ln()
        };
        let busy = Context {
            room: Room::Busy,
            ..heard("x")
        };
        // Nothing listens for the name: exactly as before.
        let without = logit(combine(&seven, &busy));
        assert!((without - (0.263 - 0.5935)).abs() < 1e-4, "{without}");
        // Something listens and did not hear it: the other weights, with
        // nothing added. Half of each of the seven, and where it starts.
        let listening = |score: f32| Context {
            name_heard: Some(score),
            ..busy.clone()
        };
        let not_heard = logit(combine(&seven, &listening(0.0)));
        assert!((not_heard - (-0.328 - 0.9025)).abs() < 1e-4, "{not_heard}");
        // Heard: 2.346 more, from a score of 0.9 and not from one under it.
        let heard_it = logit(combine(&seven, &listening(0.93)));
        assert!((heard_it - not_heard - 2.346).abs() < 1e-4, "{heard_it}");
        assert_eq!(
            combine(&seven, &listening(0.89)),
            combine(&seven, &listening(0.0))
        );
        assert_eq!(
            combine(&seven, &listening(0.9)),
            combine(&seven, &listening(1.0))
        );
        // A quiet room and a conversation add what they were fitted to add.
        let quiet = Context {
            room: Room::Quiet,
            ..listening(0.0)
        };
        assert!((logit(combine(&seven, &quiet)) - not_heard - 1.683).abs() < 1e-4);
        let talking = Context {
            lamp_is_speaking: true,
            ..listening(0.0)
        };
        assert!((logit(combine(&seven, &talking)) - not_heard - 0.081).abs() < 1e-4);
        // What it comes to: other people's business in a busy room is
        // left alone though the name seemed to be heard, and a question a
        // lamp could answer, with the name, is answered there.
        let theirs = answers([0.05, 0.9, 0.95, 0.1, 0.05, 0.05, 0.02]);
        let for_it = answers([0.6, 0.05, 0.05, 0.9, 0.05, 0.02, 0.02]);
        assert_ne!(combine(&theirs, &listening(0.95)).choice, Choice::Answer);
        assert_eq!(combine(&for_it, &listening(0.95)).choice, Choice::Answer);
        assert_ne!(combine(&for_it, &listening(0.0)).choice, Choice::Answer);
        // The model is not shown the sign: its weights belong to the
        // state it was measured with.
        assert_eq!(state_text(&listening(0.95)), state_text(&busy));
        assert_eq!(state(&listening(0.95)), state(&busy));
        // With no model, a lamp that heard its name answers.
        assert_eq!(fallback(&listening(0.95)).choice, Choice::Answer);
        assert_eq!(fallback(&listening(0.5)).choice, Choice::StayQuiet);
        // Written down with the decision only when something listened.
        assert!(
            serde_json::to_value(&busy)
                .unwrap()
                .get("name_heard")
                .is_none()
        );
        assert_eq!(
            serde_json::to_value(listening(0.5)).unwrap()["name_heard"],
            0.5
        );
    }

    /// #4306: how alike a line's voice is to the conversation's asker's is
    /// known beside the rest, written down, never shown to the model, and at
    /// its weight of 0 changes no decision, to the bit: with the name or
    /// without, in either room, in conversation or not.
    #[test]
    fn like_asker_is_written_down_not_shown_and_at_weight_0_changes_nothing() {
        assert_eq!((LIKE_ASKER, WITH_NAME_LIKE_ASKER), (0.0, 0.0));
        assert_eq!(LIKE_ASKER_FROM, 0.70);
        let sevens = [
            answers([0.5; 7]),
            answers([0.05, 0.9, 0.95, 0.1, 0.05, 0.05, 0.02]),
            answers([0.6, 0.05, 0.05, 0.9, 0.05, 0.02, 0.02]),
            answers([0.2, 0.1, 0.57, 0.4, 0.61, 0.1, 0.0]),
            answers([0.0; 7]),
            answers([1.0; 7]),
        ];
        let mut contexts = Vec::new();
        for room in [Room::Quiet, Room::Busy, Room::Unknown] {
            for name_heard in [None, Some(0.0), Some(0.95)] {
                for (speaking, said, ago) in [
                    (false, None, None),
                    (false, Some("It will rain."), Some(4.0)),
                    (true, None, Some(0.0)),
                ] {
                    contexts.push(Context {
                        room,
                        name_heard,
                        lamp_is_speaking: speaking,
                        lamp_last_said: said.map(str::to_owned),
                        seconds_since_lamp_spoke: ago,
                        ..heard("And tomorrow?")
                    });
                }
            }
        }
        for context in &contexts {
            for like in [-1.0, -0.3, 0.0, 0.29, 0.7, 0.95, 1.0] {
                let alike = Context {
                    like_asker: Some(like),
                    ..context.clone()
                };
                for seven in &sevens {
                    let (with, without) = (combine(seven, &alike), combine(seven, context));
                    assert_eq!(with.probability.to_bits(), without.probability.to_bits());
                    assert_eq!(with.choice, without.choice);
                    let (with, without) =
                        (combine_next(seven, &alike), combine_next(seven, context));
                    assert_eq!(with.probability.to_bits(), without.probability.to_bits());
                }
                assert_eq!(fallback(&alike), fallback(context));
                // The model is shown the state it was measured with.
                assert_eq!(state_text(&alike), state_text(context));
            }
        }
        // Written down only when the voice said it, and read back from an
        // older line without it as nothing.
        let alike = Context {
            like_asker: Some(0.86),
            ..heard("And tomorrow?")
        };
        assert_eq!(
            serde_json::to_value(&alike).unwrap()["like_asker"],
            0.86_f32
        );
        let unsaid = serde_json::to_value(heard("And tomorrow?")).unwrap();
        assert!(unsaid.get("like_asker").is_none());
        let back: Context = serde_json::from_value(unsaid).unwrap();
        assert_eq!(back.like_asker, None);
        let back: Context = serde_json::from_value(serde_json::to_value(&alike).unwrap()).unwrap();
        assert_eq!(back, alike);
    }

    #[test]
    fn the_choice_changes_at_the_two_thresholds_and_nowhere_else() {
        assert_eq!((ANSWER_FROM, LOOK_UP_FROM), (0.8, 0.4));
        assert_eq!(Choice::at(1.0), Choice::Answer);
        assert_eq!(Choice::at(0.8), Choice::Answer);
        assert_eq!(Choice::at(0.8_f32.next_down()), Choice::LookUp);
        assert_eq!(Choice::at(0.4), Choice::LookUp);
        assert_eq!(Choice::at(0.4_f32.next_down()), Choice::StayQuiet);
        assert_eq!(Choice::at(0.0), Choice::StayQuiet);
        assert_eq!(
            [Choice::Answer, Choice::LookUp, Choice::StayQuiet].map(Choice::as_str),
            ["answer", "look_up", "stay_quiet"]
        );
        // The names a log is written with are the same ones.
        assert_eq!(
            serde_json::to_string(&[Choice::Answer, Choice::LookUp, Choice::StayQuiet]).unwrap(),
            "[\"answer\",\"look_up\",\"stay_quiet\"]"
        );
    }

    #[test]
    fn with_no_model_the_rule_answers_in_a_quiet_room_or_a_conversation() {
        let quiet = heard("x");
        let busy = Context {
            room: Room::Busy,
            ..quiet.clone()
        };
        let unknown = Context {
            room: Room::Unknown,
            ..quiet.clone()
        };
        let yes = Judgement {
            probability: 0.74,
            choice: Choice::Answer,
            from_rule: true,
        };
        let no = Judgement {
            probability: 0.26,
            choice: Choice::StayQuiet,
            from_rule: true,
        };
        assert_eq!(fallback(&quiet), yes);
        assert_eq!(fallback(&busy), no);
        assert_eq!(fallback(&unknown), no);
        for room in [Room::Busy, Room::Unknown] {
            let replying = Context {
                room,
                lamp_last_said: Some("Shall I go on?".into()),
                seconds_since_lamp_spoke: Some(4.0),
                ..quiet.clone()
            };
            assert_eq!(fallback(&replying), yes);
            let speaking = Context {
                room,
                lamp_is_speaking: true,
                ..quiet.clone()
            };
            assert_eq!(fallback(&speaking), yes);
        }
    }

    #[test]
    fn what_the_lamp_last_said_is_cut_to_its_last_200_bytes_at_a_whole_character() {
        assert_eq!(last_words("  Black.  \n").as_deref(), Some("Black."));
        assert_eq!(last_words(" \n "), None);
        assert_eq!(last_words(""), None);
        let exactly = "a".repeat(200);
        assert_eq!(last_words(&exactly).as_deref(), Some(exactly.as_str()));
        let longer = format!("b{exactly}");
        assert_eq!(last_words(&longer).as_deref(), Some(exactly.as_str()));
        // Vietnamese: most letters with a mark are two or three bytes, so
        // 200 bytes back from the end falls inside one.
        let said = "Hôm nay trời đẹp quá, chúng ta đi dạo một vòng quanh hồ nhé. ".repeat(6);
        let end = last_words(&said).unwrap();
        assert!(end.len() <= 200, "{}", end.len());
        assert!(end.len() > 190, "{}", end.len());
        assert!(said.trim().ends_with(&end));
        assert!(end.ends_with("quanh hồ nhé."));
        // Every place the cut can fall in a letter of two bytes and in one
        // of three: it moves on to the next whole letter, never back.
        for (letter, lengths) in [("ơ", [200, 199, 200]), ("ệ", [198, 199, 200])] {
            let letters = letter.repeat(300 / letter.len());
            for (more, length) in lengths.into_iter().enumerate() {
                let text = format!("{letters}{}", "x".repeat(more));
                let end = last_words(&text).unwrap();
                assert_eq!(end.len(), length, "{letter} and {more}");
                assert!(text.ends_with(&end));
            }
        }
        // The state never shows more, whatever the context was given.
        let long = Context {
            lamp_last_said: Some(said.clone()),
            seconds_since_lamp_spoke: Some(1.0),
            ..heard("vâng")
        };
        assert_eq!(state(&long)["lamp_last_said"], end.as_str());
        assert!(state_text(&long).contains(&format!("\"lamp_last_said\":\"{end}\"")));
    }

    #[test]
    fn answers_are_the_seven_yes_or_no_ones_or_the_name_of_the_one_missing() {
        let seven = [0.07, 0.06, 0.28, 0.73, 0.06, 0.05, 0.09];
        let held = answers(seven);
        let named = held.by_name();
        assert_eq!(named.len(), 7);
        assert_eq!(named["calls_lamp"], 0.07);
        assert_eq!(named["asks_to_stop"], 0.09);
        let mut given: BTreeMap<String, Answer> = named
            .iter()
            .map(|(name, yes)| (name.clone(), Answer::Noul(*yes)))
            .collect();
        // One more than was asked is passed over.
        given.insert("extra".into(), Answer::Noul(0.5));
        assert_eq!(Answers::of(&given), Ok(held));
        given.remove("follows_lamp");
        assert_eq!(Answers::of(&given), Err("follows_lamp"));
        given.insert("follows_lamp".into(), Answer::Noul(1.5));
        assert_eq!(Answers::of(&given), Err("follows_lamp"));
        given.insert(
            "follows_lamp".into(),
            Answer::Choice {
                choice: "yes".into(),
                probabilities: BTreeMap::new(),
            },
        );
        assert_eq!(Answers::of(&given), Err("follows_lamp"));
    }
}
