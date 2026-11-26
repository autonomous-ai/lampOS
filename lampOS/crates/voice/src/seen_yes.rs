//! The owner's yes to seen activity, by voice (#6543, `docs/onboarding.md`,
//! "Seen activity"): lamp-seen may look now and then at whether they are
//! eating, drinking or taking a break, and so sends pictures from the camera
//! to the lamp's server to be labelled. Three system functions the model
//! calls, declared only with `LAMP_SEEN_ACTIVITY=on`, lamp-seen's own
//! switch; never a skill.
//!
//! The yes is informed (the Reviewer-lead's P2 on #6912): the model first
//! calls `offer_desk_noticing`, which gives it the disclosure to say word
//! for word ([`SEEN_DISCLOSURE`], or [`SEEN_DISCLOSURE_POSTURE`] with
//! `LAMP_SEEN_POSTURE=on`), naming the server, and keeps that offer
//! pending. The offer counts only once the lamp played the disclosure
//! whole, every word of it uncut ([`lamp_said`], [`lamp_cut`], as a
//! read-back's is checked: `services::read_back_said`); cut, interrupted or
//! barged in, it is no offer; cut short by a newer line that answers its
//! words in its place, it goes with them ([`answered_in_place`]).
//! `look_at_my_desk` keeps the yes only in the very next turn after it
//! ([`turn_began`]), and only for the owner's own spoken line:
//! seen saying it or in their voice (`services::owners_yes_now`); with
//! `LAMP_PRIVACY_ROOM`, where any heard line counts as the owner's, their
//! voice by the judge (`services::owners_voice_now`), so a television's
//! "yes" is none. Without a pending offer it is refused, so an ordinary
//! "look at my desk, where are my keys" never turns it on. The offer goes
//! at the next turn either way.
//!
//! The yes is kept as onboarding keeps room sounds' and face expressions'
//! (`lamp_msgs::consent`): `seen-activity.json` beside them, written whole
//! by the same writer (`onboarding::storage::keep_yes`), 0640
//! `root:lampos`, `"by":"voice"`. Anyone's "stop" turns it off, the safe
//! side, and so does any forget ([`forgotten`]) and setup's reset. A stop
//! and a forget also raise the app's mark beside it to the lamp's now
//! ([`lamp_msgs::consent::SEEN_ACTIVITY_MARK`], Heron's P2 on #7551), so no
//! desk noticing yes the app sent before it lands after it: the mark raised
//! and the yes removed under the mark's lock, which root's helper holds
//! across its own check and write (Twelve's P2 on #7551).
//! lamp-seen only reads the file, and says the lines back itself, on and
//! off, so the model is told to add nothing to them.
//!
//! With `LAMP_NOTICING_BY_DEFAULT=on` (#7767, `lamp_msgs::consent`, off
//! unless set, in no unit) a missing file is on, so a way off keeps a no
//! ([`run_by_default`]): the stop writes `"no"` for seen activity and face
//! expressions both, under the mark's lock with the mark raised, as the
//! removal is today. `look_at_my_desk` ("start looking again") keeps the
//! owner's own spoken yes with no offer and no `APP_FIRST`, a no in either
//! file turned back to a yes; the model judges the phrase, from the two
//! descriptions ([`tools_for`]), and no function is added. A forget keeps a
//! no in place and never turns noticing on ([`forget_takes`]).
use lamp_gemini::{Tool, ToolCall};
use serde_json::{Value, json};
use std::{
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};

/// lamp-seen's switch: `on` for the functions to be declared. Anything
/// else: never.
pub const ENVIRONMENT: &str = "LAMP_SEEN_ACTIVITY";
/// The provider worker's flag for it.
pub const FLAG: &str = "--seen-activity";
/// Posture too (`on` only, with [`ENVIRONMENT`]): the disclosure names it.
pub const POSTURE_ENVIRONMENT: &str = "LAMP_SEEN_POSTURE";
/// The provider worker's flag for it, right after [`FLAG`].
pub const POSTURE_FLAG: &str = "--seen-posture";
/// The voice's own offer (`on` only, with [`ENVIRONMENT`]): only with it is
/// [`OFFER`] declared and routed, so the owner's spoken yes can be asked
/// for. Off unless set, and off in the product (the first builder on
/// #7506): the yes comes from the app's button instead, while
/// [`LOOK`] (refused with no offer just before) and [`STOP`] stay.
pub const OFFER_ENVIRONMENT: &str = "LAMP_SEEN_VOICE_OFFER";
/// The provider worker's flag for it, after [`POSTURE_FLAG`].
pub const OFFER_FLAG: &str = "--seen-voice-offer";
/// The function the model calls first, to say the disclosure.
pub const OFFER: &str = "offer_desk_noticing";
/// The function the model calls on the owner's yes to the offer.
pub const LOOK: &str = "look_at_my_desk";
/// The function the model calls on a "stop looking".
pub const STOP: &str = "stop_looking_at_my_desk";

/// What the lamp says before any yes, the PM's wording on #6543 with the
/// first builder's c6957770745 and the lead's "you or anyone else"
/// (#1711, no stranger check): what it notices (eating, drinking, a
/// break, looking tired, celebrating) and that it reacts to it, that
/// pictures go to Autonomous's server to be labelled and are deleted once
/// checked, that the light shows each send, and that a
/// stop works any time. "Deleted once they're checked" waits on the
/// backend's written retention confirmation before the switch goes on
/// (#6912).
pub const SEEN_DISCLOSURE: &str = "I can notice when you eat, drink, take a break, look tired or celebrate, and I react to what I notice. To do that, while you or anyone else is at the desk I'd send a few pictures from my camera to Autonomous's server to be labelled, and they're deleted once they're checked. My light shows each time I send one, and you can tell me to stop any time. Shall I?";
/// [`SEEN_DISCLOSURE`] with posture on.
pub const SEEN_DISCLOSURE_POSTURE: &str = "I can notice when you eat, drink, take a break, look tired or celebrate, and how you're sitting, and I react to what I notice. To do that, while you or anyone else is at the desk I'd send a few pictures from my camera to Autonomous's server to be labelled, and they're deleted once they're checked. My light shows each time I send one, and you can tell me to stop any time. Shall I?";

/// Told with the disclosure.
pub const OFFER_SAY: &str = "Say exactly the words in \"say\", and nothing else, then wait for their answer. Only a plain yes in their very next words is a yes: then call look_at_my_desk.";
/// Refused: no offer was said just before.
pub const OFFER_FIRST: &str = "Not done: it must be offered first, in the lamp's own words, and only a yes right after counts. If they asked you to notice what they do at the desk, call offer_desk_noticing; if they only asked you to look at something, just look.";
/// Refused with the voice's own offer off ([`OFFER_ENVIRONMENT`] unset, as
/// in the product): [`OFFER`] is not declared, so the model is told the
/// yes is the app's (Heron on #7506), never to call it.
pub const APP_FIRST: &str = "Not done: they turn this on in the app, under Desk noticing; the lamp can't turn it on by voice yet. If they asked you to notice what they do at the desk, say so in a few words; if they only asked you to look at something, just look.";
/// Kept: lamp-seen says what it will look for.
pub const LOOKING: &str = "Done. The lamp says itself, in its own words, what it will look for, so say nothing more about it.";
/// Kept already.
pub const LOOKING_ALREADY: &str =
    "It is on already: say so in a few words, and that they can say stop looking at any time.";
/// Not the owner's own spoken line at the lamp.
pub const OWNER_ONLY: &str = "Not done: only the lamp's owner can turn this on, speaking at the lamp. Say so in a few words.";
/// The file could not be written or removed.
pub const NOT_KEPT: &str = "Not done: it could not be changed just now. Say so in a few words, and that they can ask again.";
/// Taken back: lamp-seen says it has stopped.
pub const STOPPED: &str =
    "Done. The lamp says itself that it has stopped looking, so say nothing more about it.";
/// With `LAMP_NOTICING_BY_DEFAULT` on (#7767): the stop took back face
/// reads alone (seen activity was off already), which lamp-seen does not
/// say back.
pub const STOPPED_FACES: &str = "Done: say in a few words that you have stopped looking at them.";
/// With `LAMP_NOTICING_BY_DEFAULT` on (#7767): "start looking again" turned
/// face reads alone back on (seen activity was on already), which lamp-seen
/// does not say back.
pub const LOOKING_FACES: &str = "Done: say in a few words that you are looking at them again, and that they can say stop looking at any time.";
/// Nothing to take back.
pub const NOT_LOOKING: &str =
    "It was not on: say in a few words that you are not looking at their desk.";

/// The provider worker's flags: [`FLAG`] with the switch `on`, then
/// [`POSTURE_FLAG`] with posture `on` too and [`OFFER_FLAG`] with the
/// voice's offer `on` too.
pub fn arguments(value: Option<&str>, posture: Option<&str>, offer: Option<&str>) -> Vec<String> {
    arguments_or(value, posture, offer, false)
}

/// [`arguments`] with `LAMP_NOTICING_BY_DEFAULT` given (#7767, slice 6):
/// with it on, [`OFFER_FLAG`] is never passed, so the offer is never
/// declared while noticing is on by default.
pub fn arguments_or(
    value: Option<&str>,
    posture: Option<&str>,
    offer: Option<&str>,
    by_default: bool,
) -> Vec<String> {
    let mut flags = Vec::new();
    if value == Some("on") {
        flags.push(FLAG.to_owned());
        if posture == Some("on") {
            flags.push(POSTURE_FLAG.to_owned());
        }
        if offer == Some("on") && !by_default {
            flags.push(OFFER_FLAG.to_owned());
        }
    }
    flags
}

pub fn is_tool(name: &str) -> bool {
    matches!(name, OFFER | LOOK | STOP)
}

/// The three functions, as [`tools_for`] gives them for this process's
/// `LAMP_NOTICING_BY_DEFAULT` (`lamp_msgs::consent::noticing_by_default`).
pub fn tools() -> [Tool; 3] {
    tools_for(lamp_msgs::consent::noticing_by_default())
}

/// `look_at_my_desk` with `LAMP_NOTICING_BY_DEFAULT` on (#7767): "start
/// looking again", with no offer before it.
const LOOK_AGAIN: &str = "Call this when the person asks you to start looking at their desk or at \
                          what they are doing again, after they asked you to stop (\"start looking \
                          again\", \"you can watch me again\"). It turns noticing back on, which \
                          sends pictures from your camera to the lamp's server to be labelled. An \
                          ordinary request to look at something (\"look at my desk, where are my \
                          keys?\") is a look, never this. Then say only what it tells you.";

/// The stop with `LAMP_NOTICING_BY_DEFAULT` on (#7767): face reads too.
const STOP_ALL: &str = "Call this when someone asks you to stop looking at them, at their desk or at \
                        what they are doing, or to stop sending pictures (\"stop looking at my desk\", \
                        \"stop noticing what I'm doing\", \"stop watching me\"). It stops noticing \
                        what they do at the desk and how their face seems; seen_remarks stops only \
                        what you say about it. Then say only what it tells you.";

/// The three functions; with `by_default` (`LAMP_NOTICING_BY_DEFAULT` on,
/// #7767) only the descriptions of [`LOOK`] and [`STOP`] change.
pub fn tools_for(by_default: bool) -> [Tool; 3] {
    let [offer, mut look, mut stop] = tools_as_today();
    if by_default {
        look.description = LOOK_AGAIN.into();
        stop.description = STOP_ALL.into();
    }
    [offer, look, stop]
}

fn tools_as_today() -> [Tool; 3] {
    [
        Tool {
            name: OFFER.into(),
            description: "Call this when the person asks you to notice what they do at the desk now and \
                          then, such as when they eat, drink or take a break (\"keep an eye on whether I \
                          eat\", \"watch what I'm doing\"). That sends pictures from your camera to the \
                          lamp's server to be labelled, so it is always offered first, in the lamp's own \
                          words. An ordinary request to look at something (\"look at my desk, where are \
                          my keys?\") is a look, never this. Then say exactly what it gives you as \"say\", \
                          and nothing else."
                .into(),
            parameters: None,
            silent: false,
        },
        Tool {
            name: LOOK.into(),
            description: "Call this only when you have just said the offer offer_desk_noticing gave you, \
                          and the person's very next words are a plain yes to it. It turns on sending \
                          pictures from your camera to the lamp's server to be labelled. Never infer a yes \
                          from a request to look at something, never on a maybe, and never without that \
                          offer said just before. Then say only what it tells you."
                .into(),
            parameters: None,
            silent: false,
        },
        Tool {
            name: STOP.into(),
            description: "Call this when someone asks you to stop looking at their desk or at what they \
                          are doing, or to stop sending pictures (\"stop looking at my desk\", \"stop \
                          noticing what I'm doing\", \"stop watching me\"). It stops the noticing \
                          itself; seen_remarks stops only what you say about it. Then say only what it \
                          tells you."
                .into(),
            parameters: None,
            silent: false,
        },
    ]
}

/// The four seen reactions' shared stop (`skills/seen-*/skill.toml`,
/// `[stop] tool`): their spoken remarks only.
pub const REMARKS: &str = "seen_remarks";

/// The words that stop something, directly before one of
/// [`LOOKING_WORDS`] ("stop looking", "don't watch me", "quit noticing").
pub const STOP_WORDS: [&str; 6] = ["stop", "don't", "dont", "do not", "quit", "no more"];
/// The lamp's looking, named after one of [`STOP_WORDS`] (#8404).
pub const LOOKING_WORDS: [&str; 6] = ["looking", "look", "watching", "watch", "noticing", "notice"];
/// The yes's own name in the app, a stop of the looking wherever it is
/// heard ("turn off desk noticing").
pub const NOTICING_NAME: &str = "desk noticing";
/// The remarks' words: with any of them, the line is about what the lamp
/// says, and the model's remarks stop stands ("don't comment on what I
/// do", "stop telling me what I'm doing").
pub const REMARK_WORDS: [&str; 14] = [
    "remark",
    "remarks",
    "comment",
    "comments",
    "commenting",
    "say",
    "saying",
    "talk",
    "talking",
    "tell",
    "telling",
    "mention",
    "mentioning",
    "lines",
];

/// `heard`'s words: lowercase, a curly apostrophe as a straight one, split
/// at anything but a letter, a digit or an apostrophe.
fn words(heard: &str) -> Vec<String> {
    heard
        .to_lowercase()
        .replace('\u{2019}', "'")
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether `phrase`'s words come one after another in `words` at `start`.
fn at(words: &[String], start: usize, phrase: &str) -> bool {
    let phrase: Vec<&str> = phrase.split(' ').collect();
    words
        .get(start..start + phrase.len())
        .is_some_and(|run| run.iter().zip(&phrase).all(|(word, said)| word == said))
}

/// Whether `heard` asks the lamp to stop looking, not to stop its remarks:
/// one of [`STOP_WORDS`] directly before one of [`LOOKING_WORDS`], or
/// [`NOTICING_NAME`], and none of [`REMARK_WORDS`]. Whole words, any case,
/// English only for now, as `stop_scope`'s.
pub fn names_the_looking(heard: &str) -> bool {
    let words = words(heard);
    if words
        .iter()
        .any(|word| REMARK_WORDS.contains(&word.as_str()))
    {
        return false;
    }
    let stop_looking = (0..words.len()).any(|start| {
        STOP_WORDS.iter().any(|stop| {
            at(&words, start, stop)
                && LOOKING_WORDS
                    .iter()
                    .any(|looking| at(&words, start + stop.split(' ').count(), looking))
        })
    });
    stop_looking || (0..words.len()).any(|start| at(&words, start, NOTICING_NAME))
}

/// The words that bring the desk noticing's state into a heard line's
/// notes ([`state_note`]): the looking's ([`LOOKING_WORDS`]), their past,
/// seeing, the camera, the eyes and the desk's ("keep an eye on my desk").
/// Widened on lamp-06c1's third re-row of #8674 (26 Nov, Q3): a line a
/// person says of the looking is told the state even with the lamp's name
/// and half its words lost to the speech-to-text ("Now stop watching the").
/// It matches too often rather than too rarely: the note is a few words.
pub const STATE_WORDS: [&str; 15] = [
    "looking", "look", "looked", "watching", "watch", "watched", "noticing", "notice", "noticed",
    "see", "seeing", "camera", "desk", "eye", "eyes",
];

/// Whether `heard` names the lamp's looking or the desk, any one of
/// [`STATE_WORDS`] as a whole word, any case: the lines told the state.
pub fn names_the_noticing(heard: &str) -> bool {
    words(heard)
        .iter()
        .any(|word| STATE_WORDS.contains(&word.as_str()))
}

/// With `LAMP_NOTICING_BY_DEFAULT` on (#7767), the note beside a heard line
/// that names the looking ([`names_the_noticing`]): the desk noticing's
/// state read just then from `path` ([`state_note_at`]). `None` without
/// `path` (the switch off, or seen activity not declared) or for any other
/// line. On lamp-06c1's re-row of #8674 (26 Nov) the model, never told the
/// state, answered from earlier turns: "not watching right now" while bursts
/// ran, and "I've stopped" with no call. The read is the two consent files
/// (two small opens and reads, and a stat of the done mark for a missing
/// one), only on such a line, never cached.
pub fn state_note(path: Option<&Path>, heard: &str) -> Option<String> {
    let path = path.filter(|_| names_the_noticing(heard))?;
    Some(state_note_at(path, true))
}

/// The state at `path` (seen activity's file; face expressions' beside
/// it), by lamp-seen's and lamp-eyes' own rule (`lamp_msgs::consent::read_or`
/// with `by_default`: a kept record, else the default), in words for the
/// model, with the function to call.
pub fn state_note_at(path: &Path, by_default: bool) -> String {
    use lamp_msgs::consent::{FACE_EXPRESSIONS, SEEN_ACTIVITY, allowed, file_in, read_or};
    let seen = read_or(path, SEEN_ACTIVITY, by_default);
    let face = path
        .parent()
        .and_then(|folder| file_in(folder, FACE_EXPRESSIONS))
        .is_some_and(|face| allowed(&face, FACE_EXPRESSIONS, by_default));
    let head = "(For you to know, read just now, not to answer: desk noticing is";
    let stands =
        "This is how it is now, whatever earlier turns said; say nothing of it unless they ask.";
    if seen.yes {
        let how = if seen.by_default {
            "on by default (nobody turned it off)"
        } else {
            "on (they said yes)"
        };
        let what = if face {
            "what they do at the desk and how their face seems"
        } else {
            "what they do at the desk"
        };
        format!(
            "{head} {how}: you are noticing {what}, sending a few pictures from your camera to be \
             labelled. {stands} If they ask whether you are watching or noticing, say it is on. If \
             they ask you to stop looking at them or at their desk, call {STOP}, and never say you \
             stopped without calling it.)"
        )
    } else if face {
        format!(
            "{head} off: you are not noticing what they do at the desk, but you still notice how \
             their face seems. {stands} If they ask you to stop looking at them, call {STOP}, and \
             never say you stopped without calling it. If they ask you to start looking at their \
             desk again, call {LOOK}.)"
        )
    } else {
        format!(
            "{head} off: you are not looking at their desk or their face, as they asked. {stands} \
             If they ask whether you are watching, say you are not. If they ask you to start \
             looking again, call {LOOK}.)"
        )
    }
}

/// The words of a reply that say the lamp has stopped, directly before one
/// of [`LOOKING_VERBS`] ("I've stopped watching"), or alone in a short
/// reply to the person's own stop of the looking ([`claims_stopped`]).
const STOPPED_WORDS: [&str; 8] = [
    "stopped", "stopping", "quit", "quitting", "paused", "pausing", "halted", "ended",
];

/// The words that, directly before "stop" ("I'll stop", "I will stop"),
/// make it a promise, which [`claims_stopped`] takes as a claim as well, as
/// "going to stop"; never "tell me to stop", the way the lamp offers one.
const STOP_PROMISED: [&str; 2] = ["i'll", "will"];

/// The words of a sentence that offers a way off rather than saying it is
/// off ("you can turn it off any time", "If you ask, my camera goes off",
/// "I'll stop watching if you'd like"): with one in an earlier clause of
/// the sentence, or earlier in the claim's own clause, neither "off" nor a
/// stop word is a claim, nor a promised "I'll stop" with one after it in
/// its clause; one after the claim's clause is an offer to undo it ("I've
/// stopped watching, but you can turn it back on any time"), still a claim.
const OFFERING_WORDS: [&str; 6] = ["can", "could", "if", "whenever", "anytime", "any"];

/// The words that, directly before a looking verb ([`LOOKING_VERBS`]) or
/// with one of [`NOT_GAP`] between, say the lamp isn't looking: "I'm not
/// watching", "I won't be looking", "no longer noticing".
const NOT_WORDS: [&str; 7] = ["not", "won't", "wont", "never", "isn't", "aren't", "longer"];

/// The words that may come between one of [`NOT_WORDS`] and the looking
/// verb ("I won't be looking", "I'm not still watching").
const NOT_GAP: [&str; 4] = ["be", "keep", "still", "ever"];

/// The words that, directly before a stop word or with only the lamp
/// ([`LAMP_SUBJECT`]) or one of [`STOP_NOT_GAP`] between, negate it ("I
/// haven't stopped watching", "No, I never stopped", "I have not
/// stopped"): the lamp says it is still looking, no claim. Curly
/// apostrophes are straight ones by then ([`words`]).
const STOP_NOT_WORDS: [&str; 22] = [
    "not", "won't", "wont", "never", "isn't", "aren't", "haven't", "havent", "hasn't", "hasnt",
    "hadn't", "hadnt", "didn't", "didnt", "don't", "dont", "can't", "cant", "cannot", "wasn't",
    "weren't", "nor",
];

/// The auxiliaries that may come between one of [`STOP_NOT_WORDS`] and the
/// stop word ("I have not stopped", "never have I stopped").
const STOP_NOT_GAP: [&str; 9] = [
    "have", "has", "had", "been", "ever", "am", "was", "really", "even",
];

/// The looking's verbs and the camera, for [`STOPPED_WORDS`],
/// [`NOT_WORDS`] and "off": not "see" or "desk", which a reply describing
/// the desk says with a "not" ("I don't see your keys", "that's not your
/// desk").
const LOOKING_VERBS: [&str; 11] = [
    "looking", "look", "watching", "watch", "noticing", "notice", "camera", "peeking", "peek",
    "pictures", "eyes",
];

/// What "off" may say is off, as the lamp's own: its noticing, its camera,
/// its watching or looking; never "eyes", which the lamp says of the
/// person's ("take your eyes off the screen").
const OFF_LOOKING: [&str; 4] = ["noticing", "camera", "watching", "looking"];

/// The lamp as a clause's subject: the claim is the lamp's own looking.
const LAMP_SUBJECT: [&str; 9] = ["i", "i'm", "i've", "i'll", "i'd", "im", "ive", "my", "lamp"];

/// The person as a clause's subject or owner ("take your eyes off", "you
/// can turn it off"): an "off" with one is not the lamp's.
const PERSON_WORDS: [&str; 3] = ["you", "your", "you're"];

/// The words a short reply may say beside a stop word alone and still be
/// only that stop ("Okay, I've stopped.", "Sure, stopping now.").
const BARE_STOP_FILLER: [&str; 17] = [
    "okay", "ok", "sure", "alright", "all", "right", "done", "yes", "yeah", "now", "i", "i've",
    "i'm", "i'll", "have", "am", "just",
];

/// A sentence's first word that makes it a question even with its "?"
/// lost ("Should I turn the camera off"), after [`BARE_STOP_FILLER`]'s
/// "okay"s: never a claim.
const QUESTION_OPENERS: [&str; 13] = [
    "should", "shall", "do", "does", "did", "can", "could", "would", "may", "am", "want", "wanna",
    "how",
];

/// The words that end a clause inside a sentence, for [`claims_stopped`]:
/// a claim's stop and its looking are in one clause ("I've stopped the
/// music, keep watching" is none).
const CLAUSE_BREAKS: [&str; 6] = ["and", "but", "so", "while", "because", "then"];

/// Whether the reply `reply` says the lamp stopped looking, or isn't
/// looking, where `heard` is the person's line. Only the lamp's own looking
/// is a claim (the Reviewer-lead's must-fix 1 on #8674, c7185816145): in a
/// clause of a sentence that is no question (no "?", and no opener such as
/// "should" or "can"),
///
/// - one of [`STOPPED_WORDS`], or "stop" after one of [`STOP_PROMISED`] or
///   "going to", with one of [`LOOKING_VERBS`] directly after it or one
///   word later ("I've stopped watching", "I'll stop noticing", "stopped my
///   camera"), the lamp as subject ([`LAMP_SUBJECT`] before it, or none),
///   no negation before it ([`STOP_NOT_WORDS`]: "I haven't stopped
///   watching") and none of [`OFFERING_WORDS`] before it in the sentence,
///   nor after a promised "stop" in its clause ("I'll stop watching if
///   you'd like");
/// - one of [`NOT_WORDS`] directly before one of [`LOOKING_VERBS`], or with
///   one of [`NOT_GAP`] between, the lamp as subject before it ("I'm not
///   looking at your desk", "I won't watch", "I'm no longer watching");
/// - "off" with one of [`OFF_LOOKING`], said of the lamp ([`LAMP_SUBJECT`],
///   or an "is"/"are"/"'s"), none of [`PERSON_WORDS`] in the clause and
///   none of [`OFFERING_WORDS`] before it in the sentence ("noticing is
///   off", "my camera is off"; never "If you ask, my camera goes off");
/// - a stop word alone ("I've stopped.", "Okay, stopping now."), only to a
///   person's line asking to stop the looking ([`names_the_looking`]).
///
/// A word of the looking in the person's line alone makes none: "Music
/// stopped." to "Let me see, stop the music" is no claim, nor "Try not to
/// look at the screen.". Whole words, any case, English only, as
/// [`names_the_looking`]. Unclear is no claim; the lamp's own clear claim
/// is (lamp-06c1's third re-row of #8674, 26 Nov, Q3: "I've stopped
/// watching your desk" to "Now stop watching the").
pub fn claims_stopped(reply: &str, heard: Option<&str>) -> bool {
    let asked_to_stop = heard.is_some_and(names_the_looking);
    reply
        .split_inclusive(['.', '!', '?', ';', '\n'])
        .filter(|sentence| !sentence.trim_end().ends_with('?'))
        .filter(|sentence| !opens_a_question(&words(sentence)))
        .any(|sentence| {
            // An offer governs its own clause and the ones after it, never
            // a claim before it (Friday's P2-a and P3 on #8780, c7194711313;
            // the Reviewer-lead's c7195457427 and Friday's c7195959981).
            let mut offered = false;
            sentence
                .split([',', ':', '\u{2014}', '\u{2013}'])
                .flat_map(|part| clauses(words(part)))
                .any(|clause| {
                    let claims = clause_claims(&clause, asked_to_stop, offered);
                    offered |= clause
                        .iter()
                        .any(|word| OFFERING_WORDS.contains(&word.as_str()));
                    claims
                })
        })
}

/// Whether a sentence's words open as a question ([`QUESTION_OPENERS`]),
/// past any "okay" or "so" first.
fn opens_a_question(words: &[String]) -> bool {
    words
        .iter()
        .find(|word| !matches!(word.as_str(), "okay" | "ok" | "so" | "um" | "alright"))
        .is_some_and(|word| QUESTION_OPENERS.contains(&word.as_str()))
}

/// `words` cut at each of [`CLAUSE_BREAKS`], none empty.
fn clauses(words: Vec<String>) -> Vec<Vec<String>> {
    let mut all = vec![Vec::new()];
    for word in words {
        if CLAUSE_BREAKS.contains(&word.as_str()) {
            all.push(Vec::new());
        } else if let Some(last) = all.last_mut() {
            last.push(word);
        }
    }
    all.retain(|clause| !clause.is_empty());
    all
}

/// One clause of [`claims_stopped`]'s: whether it claims the lamp stopped
/// looking, `asked_to_stop` where the person's line asked it to,
/// `offered` where an earlier clause of its sentence has one of
/// [`OFFERING_WORDS`].
fn clause_claims(words: &[String], asked_to_stop: bool, offered: bool) -> bool {
    let has = |set: &[&str], word: &str| set.contains(&word);
    // An offer before `at` in the sentence governs the word there.
    let offer_before =
        |at: usize| offered || words[..at].iter().any(|word| has(&OFFERING_WORDS, word));
    let lamp_before = |at: usize| words[..at].iter().any(|word| has(&LAMP_SUBJECT, word));
    let looking_at = |at: usize| words.get(at).is_some_and(|word| has(&LOOKING_VERBS, word));
    let stop_at = |at: usize| {
        let word = words[at].as_str();
        let before = |back: usize| at.checked_sub(back).map(|at| words[at].as_str());
        has(&STOPPED_WORDS, word)
            || (word == "stop"
                && (before(1).is_some_and(|word| STOP_PROMISED.contains(&word))
                    || (before(2) == Some("going") && before(1) == Some("to"))))
    };
    // A negation before the stop, past the lamp or an auxiliary ("I
    // haven't stopped", "No, I never stopped"): the lamp is still looking
    // (Friday's P2-b on #8780).
    let negated = |at: usize| {
        words[..at]
            .iter()
            .rev()
            .find(|word| !has(&LAMP_SUBJECT, word) && !has(&STOP_NOT_GAP, word))
            .is_some_and(|word| has(&STOP_NOT_WORDS, word))
    };
    // A promised stop with an offer after it in its clause ("I'll stop
    // watching if you'd like") is offered, not said.
    let promise_offered = |at: usize| {
        words[at] == "stop"
            && words[at + 1..]
                .iter()
                .any(|word| has(&OFFERING_WORDS, word))
    };
    let stopped = (0..words.len()).any(|at| {
        stop_at(at)
            && !offer_before(at)
            && !promise_offered(at)
            && !negated(at)
            && (looking_at(at + 1) || looking_at(at + 2))
            && (at == 0 || lamp_before(at))
    });
    let not_looking = (0..words.len()).any(|at| {
        let word = words[at].as_str();
        has(&NOT_WORDS, word)
            && (word != "longer" || at > 0 && words[at - 1] == "no")
            && (looking_at(at + 1)
                || (words.get(at + 1).is_some_and(|word| has(&NOT_GAP, word))
                    && looking_at(at + 2)))
            && (lamp_before(at) || (word == "longer" && at == 1))
    });
    let off = words
        .iter()
        .position(|word| word == "off")
        .is_some_and(|at| !offer_before(at))
        && words.iter().any(|word| has(&OFF_LOOKING, word))
        && !words.iter().any(|word| has(&PERSON_WORDS, word))
        && words.iter().any(|word| {
            has(&LAMP_SUBJECT, word) || word == "is" || word == "are" || word.ends_with("'s")
        });
    let bare_stop = asked_to_stop && {
        let rest: Vec<&String> = words
            .iter()
            .filter(|word| !has(&BARE_STOP_FILLER, word))
            .collect();
        rest.len() == 1 && (has(&STOPPED_WORDS, rest[0]) || rest[0] == "stop")
    };
    stopped || not_looking || off || bare_stop
}

/// A reply's claim to have stopped looking, watched over its pieces as they
/// are said ([`ClaimWatch::piece`]): the second route's sentences, Live's
/// output transcript.
#[derive(Debug, Default)]
pub struct ClaimWatch {
    /// The reply said so far, as given; whether its turn ended (all of it
    /// judged, not only its ended sentences); whether its claim was acted
    /// on already.
    said: String,
    ended: bool,
    acted: bool,
}

impl ClaimWatch {
    /// With `path` (seen activity's file: `LAMP_NOTICING_BY_DEFAULT` on and
    /// the functions declared), `piece`, the next of the reply to request
    /// `request` to be said, to the person's spoken line `heard`: once the
    /// reply's ended sentences claim a stop ([`claims_stopped`]) while desk
    /// noticing is still on, the stop is run as [`STOP`] runs it
    /// ([`stop_claimed`]). Once a turn. Whether it ran.
    ///
    /// The Reviewer-lead's must-fixes 2 and 3 on #8674 (c7185816145):
    /// `heard` `None` (a turn nobody spoke: written, a reminder) runs
    /// nothing, as the stop call never runs there; `piece` is added
    /// exactly as given (Live's deltas carry their own spaces, and a word
    /// may be split across two); a sentence not yet ended is judged only at
    /// the turn's end ([`Self::end`]), so "Should I stop watching" waits
    /// for its "?".
    pub fn piece(
        &mut self,
        path: Option<&Path>,
        request: u64,
        piece: &str,
        heard: Option<&str>,
    ) -> bool {
        if self.said.len() < SPOKEN_MOST {
            self.said.push_str(piece);
        }
        self.judge(path, request, heard)
    }

    /// The reply's turn ended: what was said and not yet ended as a
    /// sentence is judged too ([`Self::piece`]). Whether the stop ran.
    pub fn end(&mut self, path: Option<&Path>, request: u64, heard: Option<&str>) -> bool {
        self.ended = true;
        self.judge(path, request, heard)
    }

    fn judge(&mut self, path: Option<&Path>, request: u64, heard: Option<&str>) -> bool {
        let Some(path) = path.filter(|_| !self.acted) else {
            return false;
        };
        let Some(heard) = heard else {
            return false;
        };
        let judged = if self.ended {
            self.said.as_str()
        } else {
            let ended = self
                .said
                .rfind(['.', '!', '?', ';', '\n'])
                .map_or(0, |at| at + 1);
            &self.said[..ended]
        };
        if !claims_stopped(judged, Some(heard)) {
            return false;
        }
        self.acted = true;
        stop_claimed(path, request)
    }
}

/// The reply to `request` claimed a stop with desk noticing on at `path`
/// and no stop called before it: the stop's own kept no
/// ([`run_by_default`], the [`STOP`] call's path, anyone's), one journal
/// line saying so. Nothing when both are off already (the stop was called
/// first, or it was off): the claim is true. Whether it ran.
pub fn stop_claimed(path: &Path, request: u64) -> bool {
    use lamp_msgs::consent::{FACE_EXPRESSIONS, SEEN_ACTIVITY, allowed, file_in};
    let face_on = path
        .parent()
        .and_then(|folder| file_in(folder, FACE_EXPRESSIONS))
        .is_some_and(|face| allowed(&face, FACE_EXPRESSIONS, true));
    if !allowed(path, SEEN_ACTIVITY, true) && !face_on {
        return false;
    }
    eprintln!(
        "lamp-voice: request {request}: the reply says it stopped looking with desk noticing on and no stop called, so the stop is run as {STOP}"
    );
    run_by_default(
        path,
        STOP,
        false,
        now_unix_s(),
        crate::onboarding::consent_group,
    );
    true
}

/// Live's watch ([`arm_live`], [`live_piece`]): seen activity's file when
/// armed, and the reply being said, by request.
static LIVE_WATCH: Mutex<Option<(PathBuf, u64, ClaimWatch)>> = Mutex::new(None);

/// The Live route's start (`provider_worker`): with `on` (seen activity
/// declared and `LAMP_NOTICING_BY_DEFAULT` on), its output transcript is
/// watched for a claimed stop ([`live_piece`]).
pub fn arm_live(on: bool) {
    if let Ok(mut watch) = LIVE_WATCH.lock() {
        *watch = on.then(|| (path(), 0, ClaimWatch::default()));
    }
    if on {
        eprintln!(
            "lamp-voice provider: a reply saying it stopped looking with no stop called runs the stop (LAMP_NOTICING_BY_DEFAULT)"
        );
    }
}

/// A piece `text` of Live's output transcript for `request`, as given:
/// watched as [`ClaimWatch::piece`], to the person's line as the thread
/// keeps it, only for a turn a person spoke (`heard`: not a reminder's,
/// and one with a heard line); with `finished` (the transcript's end), its
/// turn ended ([`ClaimWatch::end`]). Nothing unless [`arm_live`] armed it.
pub fn live_piece(request: u64, text: &str, heard: bool, finished: bool) -> bool {
    let Ok(mut watch) = LIVE_WATCH.lock() else {
        return false;
    };
    let Some((path, at, claim)) = watch.as_mut() else {
        return false;
    };
    if *at != request {
        *at = request;
        *claim = ClaimWatch::default();
    }
    let heard = crate::stop_scope::heard_words(request, heard);
    let path = path.clone();
    let ran = claim.piece(Some(&path), request, text, heard.as_deref());
    ran || (finished && claim.end(Some(&path), request, heard.as_deref()))
}

/// Live's `GenerationComplete` for `request`: its reply ended, so what of
/// it is not yet an ended sentence is judged too ([`ClaimWatch::end`]),
/// for a turn a person spoke (`heard`). Nothing for another request's.
pub fn live_end(request: u64, heard: bool) -> bool {
    let Ok(mut watch) = LIVE_WATCH.lock() else {
        return false;
    };
    let Some((path, _, claim)) = watch.as_mut().filter(|(_, at, _)| *at == request) else {
        return false;
    };
    let heard = crate::stop_scope::heard_words(request, heard);
    let path = path.clone();
    claim.end(Some(&path), request, heard.as_deref())
}

/// The stop of the desk noticing itself ([`STOP`], with the call's id), for
/// `call`, a snooze or off of the seen reactions' remarks ([`REMARKS`]),
/// when the person's own line `heard` names the looking, not the remarks
/// ([`names_the_looking`]); `None` otherwise, the model's call standing.
/// On lamp-06c1 (23 Nov 17:30, #8404) the model sent the remarks' off for
/// "stop noticing what I'm doing" and "stop looking at my desk", 2 of 2,
/// and the desk noticing went on. The stop is the safe side: with the
/// looking stopped, no remark comes either.
pub fn stop_instead(call: &ToolCall, heard: Option<&str>) -> Option<ToolCall> {
    let how = call
        .args
        .get(crate::use_skill::STOP_CHOICE)
        .and_then(Value::as_str);
    (call.name == REMARKS
        && matches!(how, Some("off" | "snooze"))
        && heard.is_some_and(names_the_looking))
    .then(|| ToolCall {
        id: call.id.clone(),
        name: STOP.into(),
        args: json!({}),
    })
}

/// Where the yes is kept: beside room sounds' (`lamp_msgs::consent::FOLDER`).
pub fn path() -> PathBuf {
    crate::onboarding::seen_activity_beside(Path::new(crate::onboarding::ROOM_SOUNDS_PATH))
}

/// The most of the lamp's words kept toward the disclosure: well over
/// either wording.
const SPOKEN_MOST: usize = 1024;

/// The offer given, waiting for its answer: the request it was given in,
/// the next one, the only one whose yes counts, the disclosure to be said,
/// what the lamp played of that request, and whether it was cut.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Offer {
    said_in: Option<u64>,
    answer_in: Option<u64>,
    disclosure: &'static str,
    spoken: String,
    cut: bool,
}

impl Offer {
    /// Given in `request` with `disclosure` to say: any earlier offer is
    /// gone.
    fn said(&mut self, request: u64, disclosure: &'static str) {
        *self = Self {
            said_in: Some(request),
            disclosure,
            ..Self::default()
        };
    }

    /// The lamp played `words` in `request`: kept when it is the offer's.
    fn played(&mut self, request: u64, words: &str) {
        if self.said_in == Some(request) && self.spoken.len() + words.len() <= SPOKEN_MOST {
            self.spoken.push_str(words);
        }
    }

    /// The lamp's answer in `request` was cut: the offer's, no offer.
    fn cut_in(&mut self, request: u64) {
        if self.said_in == Some(request) {
            self.cut = true;
        }
    }

    /// Whether the disclosure was played whole and uncut.
    fn said_whole(&self) -> bool {
        !self.cut && crate::services::read_back_said(self.disclosure, &self.spoken)
    }

    /// `request` began: the first after the offer is its answer's; any
    /// later one ends it.
    fn began(&mut self, request: u64) {
        match (self.said_in, self.answer_in) {
            (Some(said), None) if said == request => {}
            (Some(_), None) => self.answer_in = Some(request),
            (Some(_), Some(answer)) if answer == request => {}
            _ => *self = Self::default(),
        }
    }

    /// Whether `request` is the answer to an offer played whole; the offer
    /// is gone after.
    fn answered_in(&mut self, request: u64) -> bool {
        let next = self.said_in.is_some() && self.answer_in == Some(request);
        let answered = next && self.said_whole();
        if next && !answered {
            eprintln!("lamp-voice: the seen activity offer was not played whole: no offer");
        }
        *self = Self::default();
        answered
    }

    /// The words of `from`, cut short, are answered in `to`'s place: an
    /// offer given in `from` is `to`'s now, as if given there, so the replay
    /// must play the disclosure whole and uncut itself, and the line after
    /// `to` answers it. Any other offer is left as it is.
    fn in_place(&mut self, from: u64, to: u64) {
        if self.said_in == Some(from) {
            self.said(to, self.disclosure);
        }
    }
}

/// The one offer pending, shared by the worker's two routes.
static OFFERED: Mutex<Offer> = Mutex::new(Offer {
    said_in: None,
    answer_in: None,
    disclosure: "",
    spoken: String::new(),
    cut: false,
});

/// The lamp said `words` in `request` (`thread::Thread::lamp_for`, as
/// played): kept toward the disclosure when it is the offer's request.
pub fn lamp_said(request: u64, words: &str) {
    if let Ok(mut offer) = OFFERED.lock() {
        offer.played(request, words);
    }
}

/// The lamp's answer in `request` was cut (`thread::Thread::heard_only`): a
/// disclosure cut, interrupted or barged in is no offer.
pub fn lamp_cut(request: u64) {
    if let Ok(mut offer) = OFFERED.lock() {
        offer.cut_in(request);
    }
}

/// Request `to` answers in its place the words of request `from` it cut
/// short (`second_provider`: a held line's, or one that only declined with
/// `LAMP_CUT_ASK_ANSWERED`), as `services::answered_in_place` keeps a
/// write's read-back: an offer given in `from` goes with them, held to
/// being played whole and uncut in `to`, and answered by the line after it
/// (lamp-06c1, 17 Nov: a 0-word line cut the offer before any word of it,
/// the replay played it whole, and the owner's yes found no offer).
pub fn answered_in_place(from: u64, to: u64) {
    if let Ok(mut offer) = OFFERED.lock() {
        offer.in_place(from, to);
    }
}

/// A request began (its `Start`, either route): an offer said before the
/// one just before it goes.
pub fn turn_began(request: u64) {
    if let Ok(mut offer) = OFFERED.lock() {
        offer.began(request);
    }
}

/// Whether the heard line `request` (its memory scope `lease`) is the
/// owner's own spoken yes: outside the private room, the owner seen saying
/// it or in their voice; in it, where any heard line is the owner's, their
/// voice by the judge only. A line not heard is none.
fn owners_spoken_yes(
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> bool {
    heard
        && if crate::person::private_room() {
            crate::services::owners_voice_now(request)
        } else {
            crate::services::owners_yes_now(request, heard, lease)
        }
}

/// `call` for the line `request` (`heard`: spoken at the lamp; `lease`, its
/// memory scope; `posture`, `LAMP_SEEN_POSTURE`; `offer`,
/// `LAMP_SEEN_VOICE_OFFER`): the offer said, the owner's yes kept, or the
/// yes taken back, and what the model is told.
pub fn run(
    call: &ToolCall,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
    posture: bool,
    offer_on: bool,
) -> Value {
    let Ok(mut offer) = OFFERED.lock() else {
        return json!({"ok": false, "note": NOT_KEPT});
    };
    let by_default = lamp_msgs::consent::noticing_by_default();
    match call.name.as_str() {
        OFFER => offered(&mut offer, request, heard, posture),
        LOOK => {
            let answered = offer.answered_in(request);
            drop(offer);
            if by_default {
                return run_by_default(
                    &path(),
                    LOOK,
                    owners_spoken_yes(request, heard, lease),
                    now_unix_s(),
                    crate::onboarding::consent_group,
                );
            }
            let owners = answered && owners_spoken_yes(request, heard, lease);
            run_at(
                &path(),
                LOOK,
                offer_on,
                answered,
                owners,
                now_unix_s(),
                crate::onboarding::consent_group,
            )
        }
        _ => {
            drop(offer);
            // Once the old nos are done (#7767), a stop with the switch
            // since turned off keeps a no as well, so the next on still
            // reads it as no (Heron's P3 on #7940).
            let done = path()
                .parent()
                .is_some_and(lamp_msgs::consent::old_nos_done);
            if by_default || done {
                return run_by_default(
                    &path(),
                    STOP,
                    false,
                    now_unix_s(),
                    crate::onboarding::consent_group,
                );
            }
            run_at(&path(), STOP, offer_on, false, false, 0, || None)
        }
    }
}

fn now_unix_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The offer for `request`: kept pending only when said aloud at the lamp
/// (`heard`), and the words to say, with posture when `posture`.
fn offered(offer: &mut Offer, request: u64, heard: bool, posture: bool) -> Value {
    let say = if posture {
        SEEN_DISCLOSURE_POSTURE
    } else {
        SEEN_DISCLOSURE
    };
    if heard {
        offer.said(request, say);
    }
    json!({"ok": true, "say": say, "note": OFFER_SAY})
}

/// [`run`] at `path` for `look_at_my_desk` and the stop: `offer_on`, the
/// voice's own offer is declared ([`OFFER_ENVIRONMENT`]); `answered`, the
/// call's line is the very next after the offer, played whole; `owners`, it is the
/// owner's own spoken yes; `at`, now in Unix seconds; `group`, the reader
/// group's id, asked only for a yes to keep.
fn run_at(
    path: &Path,
    name: &str,
    offer_on: bool,
    answered: bool,
    owners: bool,
    at: u64,
    group: impl FnOnce() -> Option<u32>,
) -> Value {
    let key = lamp_msgs::consent::SEEN_ACTIVITY;
    if name == LOOK {
        if !answered {
            eprintln!("lamp-voice: a yes to seen activity with no offer just before: not kept");
            let note = if offer_on { OFFER_FIRST } else { APP_FIRST };
            return json!({"ok": false, "note": note});
        }
        if !owners {
            eprintln!("lamp-voice: a yes to seen activity not the owner's own line: not kept");
            return json!({"ok": false, "note": OWNER_ONLY});
        }
        if lamp_msgs::consent::read(path, key).yes {
            return json!({"ok": true, "note": LOOKING_ALREADY});
        }
        return match crate::onboarding::storage::keep_yes(path, key, at, group()) {
            Ok(()) => {
                eprintln!("lamp-voice: the owner's yes to seen activity is kept");
                json!({"ok": true, "note": LOOKING})
            }
            Err(error) => {
                eprintln!(
                    "lamp-voice: the {key} yes was not kept at {}: {error}",
                    path.display()
                );
                json!({"ok": false, "note": NOT_KEPT})
            }
        };
    }
    let lock = lock_beside(path);
    raise_mark_beside(lock.as_ref(), path, lamp_msgs::consent::now_unix_ms());
    let removed = remove(path);
    drop(lock);
    match removed {
        Ok(true) => {
            eprintln!("lamp-voice: the yes to seen activity is taken back by voice");
            json!({"ok": true, "note": STOPPED})
        }
        Ok(false) => json!({"ok": true, "note": NOT_LOOKING}),
        Err(error) => {
            eprintln!(
                "lamp-voice: the {key} yes could not be removed at {}: {error}",
                path.display()
            );
            json!({"ok": false, "note": NOT_KEPT})
        }
    }
}

/// [`run`] at `path` with `LAMP_NOTICING_BY_DEFAULT` on (#7767), where a
/// missing file is on: `owners`, the call's line is the owner's own spoken
/// yes; `at`, now in Unix seconds; `group`, the reader group's id.
///
/// - `look_at_my_desk`: with both on already, anyone's, told it is on
///   already and nothing written (lamp-06c1's re-row of #8674, Q9: nothing
///   is turned on). Else the owner's line alone, with no offer before it (the
///   model judged it a "start looking again"). A file that is not on, a
///   kept no (or one that can't be read), is made a yes, for seen activity
///   and face expressions both; a missing one is on already and stays
///   missing.
/// - The stop, anyone's: a kept no for both, under the mark's lock and the
///   mark raised to now first, as [`run_at`]'s removal, so no app yes sent
///   before lands after it.
fn run_by_default(
    path: &Path,
    name: &str,
    owners: bool,
    at: u64,
    group: impl FnOnce() -> Option<u32>,
) -> Value {
    use lamp_msgs::consent::{FACE_EXPRESSIONS, SEEN_ACTIVITY, allowed, file_in};
    let face = path
        .parent()
        .and_then(|folder| file_in(folder, FACE_EXPRESSIONS));
    // What was on before, seen activity and face reads apart (Twelve's
    // P3 1 and Heron's on #7928): lamp-seen says back only seen activity's
    // change, so face reads alone get their own line.
    let seen_was = allowed(path, SEEN_ACTIVITY, true);
    let face_was = face
        .as_deref()
        .is_some_and(|face| allowed(face, FACE_EXPRESSIONS, true));
    let both = std::iter::once((path.to_path_buf(), SEEN_ACTIVITY))
        .chain(face.map(|face| (face, FACE_EXPRESSIONS)));
    if name == LOOK {
        // On already, both: nothing is turned on, so anyone's ask is told
        // so, never `OWNER_ONLY`'s "couldn't turn that on" (lamp-06c1's
        // re-row of #8674, 26 Nov, Q9). Only a change to on is the owner's.
        if seen_was && face_was {
            return json!({"ok": true, "note": LOOKING_ALREADY});
        }
        if !owners {
            eprintln!("lamp-voice: a start looking again not the owner's own line: not kept");
            return json!({"ok": false, "note": OWNER_ONLY});
        }
        // The group asked only for a yes to keep, as [`run_at`] asks it.
        let mut group = Some(group);
        let mut id = None;
        let mut kept = Ok(());
        for (file, key) in both {
            if !allowed(&file, key, true) {
                if let Some(group) = group.take() {
                    id = group();
                }
                kept = kept.and(crate::onboarding::storage::keep_yes(&file, key, at, id));
            }
        }
        return match kept {
            Err(error) => {
                eprintln!("lamp-voice: start looking again was not kept: {error}");
                json!({"ok": false, "note": NOT_KEPT})
            }
            Ok(()) => {
                eprintln!("lamp-voice: the owner's start looking again is kept");
                json!({"ok": true, "note": if seen_was { LOOKING_FACES } else { LOOKING }})
            }
        };
    }
    let lock = lock_beside(path);
    raise_mark_beside(lock.as_ref(), path, lamp_msgs::consent::now_unix_ms());
    let group = group();
    let mut kept = Ok(());
    for (file, key) in both {
        kept = kept.and(crate::onboarding::storage::keep_no(&file, key, at, group));
    }
    drop(lock);
    match kept {
        Err(error) => {
            eprintln!("lamp-voice: the stop's no was not kept: {error}");
            json!({"ok": false, "note": NOT_KEPT})
        }
        Ok(()) if seen_was || face_was => {
            eprintln!("lamp-voice: seen activity and face expressions are a kept no by voice");
            json!({"ok": true, "note": if seen_was { STOPPED } else { STOPPED_FACES }})
        }
        Ok(()) => json!({"ok": true, "note": NOT_LOOKING}),
    }
}

/// The mark's lock in the folder of the yes at `path`, held across a "no"'s
/// raise and removal (Twelve's P2 on #7551), so root's helper never keeps an
/// app yes it checked before it. One not taken is said in the journal; the
/// yes is removed all the same, the safe side.
fn lock_beside(path: &Path) -> Option<lamp_msgs::consent::MarkLock> {
    let folder = path.parent()?;
    lamp_msgs::consent::lock_mark(folder)
        .inspect_err(|error| {
            eprintln!(
                "lamp-voice: the app's mark could not be locked in {}: {error}",
                folder.display()
            );
        })
        .ok()
}

/// A "no" at the lamp at `at_ms`: the app's mark beside the yes at `path`
/// raised to it under `lock`, so the helper refuses any desk noticing yes
/// the app sent before. A mark not written (or no lock) is said in the
/// journal; the yes is removed all the same.
fn raise_mark_beside(lock: Option<&lamp_msgs::consent::MarkLock>, path: &Path, at_ms: u64) {
    let Some(lock) = lock else { return };
    if let Err(error) = lock.raise(at_ms) {
        eprintln!(
            "lamp-voice: the app's mark could not be raised beside {}: {error}",
            path.display()
        );
    }
}

/// Remove `path`, its folder synced after: whether it was there.
fn remove(path: &Path) -> io::Result<bool> {
    // A stop or a forget is an explicit "no", so it always renews the
    // revision, even with no yes there and no state yet (the state is made
    // then): an app `consent.set` of seen activity sent before it and
    // retried after is stale (the Reviewer-lead's second P2 on #8704, and
    // the coordinator's on a fresh lamp). A yes is removed holding the consent folder, after the mark's
    // lock, first, then a fresh revision (#5500); a revision that can't be
    // kept never keeps the yes, it is only logged
    // (`lamp_msgs::consent::Held::withdraw_at`). The answer says whether a
    // yes was there.
    let there = std::fs::symlink_metadata(path).is_ok();
    lamp_msgs::consent::take_back_at(path, lamp_msgs::consent::SEEN_ACTIVITY)?;
    Ok(there)
}

/// Any forget, whoever asked (`remember.rs`'s `forget` and
/// `forget_everything`), takes back the yes at `path` (#6543): said in the
/// journal only when there was one, and the app's mark raised to now when
/// there was one or a mark is kept. With neither there, nothing changes.
/// With `LAMP_NOTICING_BY_DEFAULT` on, only a kept yes is removed
/// ([`forget_takes`]).
pub fn forgotten(path: &Path) {
    forgotten_or(path, lamp_msgs::consent::noticing_by_default());
}

/// Whether a forget removes `key`'s file at `path` (#7767, the first
/// builder's answer 1 on #7767): with `by_default` on, or with it off and
/// [`lamp_msgs::consent::OLD_NOS_DONE`] beside `path` (the rule that keeps
/// a no, as `lamp-setup change`'s `keeps_no`), only a kept yes, so a kept
/// no (or a file that can't be read, off) stays and a forget never turns
/// noticing on, nor lets a later switch-on read it as on (Twelve's P3 3 on
/// #8631). With neither, always, as before.
pub fn forget_takes(path: &Path, key: &str, by_default: bool) -> bool {
    let keeps_no = by_default || path.parent().is_some_and(lamp_msgs::consent::old_nos_done);
    !keeps_no || lamp_msgs::consent::read(path, key).yes
}

/// [`forgotten`] with `LAMP_NOTICING_BY_DEFAULT` given.
fn forgotten_or(path: &Path, by_default: bool) {
    // The mark only where the app's switch has been used or a yes is kept:
    // a forget on a lamp without seen activity writes nothing.
    let marked = path
        .parent()
        .is_some_and(|folder| lamp_msgs::consent::mark_in(folder).exists());
    let lock = (marked || path.exists())
        .then(|| lock_beside(path))
        .flatten();
    raise_mark_beside(lock.as_ref(), path, lamp_msgs::consent::now_unix_ms());
    let removed = if forget_takes(path, lamp_msgs::consent::SEEN_ACTIVITY, by_default) {
        remove(path)
    } else {
        // A kept no or the default stays (#7767); its revision is renewed
        // all the same, as `remember.rs` renews the other two (Thirteen's
        // P3 on #8704), best effort: it never changes what the forget does.
        if path.exists()
            && let Err(error) =
                lamp_msgs::consent::renew_holding(path, lamp_msgs::consent::SEEN_ACTIVITY)
        {
            eprintln!(
                "lamp-voice provider: a forget could not renew {}: {error}",
                path.display()
            );
        }
        Ok(false)
    };
    drop(lock);
    match removed {
        Ok(true) => eprintln!("lamp-voice provider: a forget removed {}", path.display()),
        Ok(false) => {}
        Err(error) => eprintln!(
            "lamp-voice provider: a forget could not remove {}: {error}",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    /// A kept file's bytes with its revision taken out (#5500): every held
    /// write adds a fresh one, so the rest is compared byte for byte.
    fn sans_rev(path: &Path) -> Vec<u8> {
        let mut kept: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert!(kept["rev"].as_str().is_some(), "{kept}");
        kept.as_object_mut().unwrap().remove("rev");
        serde_json::to_vec(&kept).unwrap()
    }

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("seen-yes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        // A lamp whose earlier nos are kept (#7767,
        // `lamp_msgs::consent::OLD_NOS_DONE`), where a missing file with
        // the switch on is the default; with it off, read by nobody here.
        std::fs::write(folder.join(lamp_msgs::consent::OLD_NOS_DONE), b"{}").unwrap();
        folder
    }

    fn file_in(folder: &Path) -> PathBuf {
        crate::onboarding::seen_activity_beside(&folder.join("room-sounds.json"))
    }

    fn call(name: &str) -> ToolCall {
        ToolCall {
            id: "seen-1".into(),
            name: name.into(),
            args: json!({}),
        }
    }

    /// #8404, lamp-06c1 23 Nov 17:30: "stop noticing what I'm doing" and
    /// "stop looking at my desk" came as the remarks' off; the person's
    /// words name the looking, so the desk noticing stops. The remarks'
    /// own words still stop only the remarks.
    #[test]
    fn a_stop_looking_said_as_the_remarks_off_stops_the_desk_noticing() {
        let remarks = |how: &str| ToolCall {
            id: "seen-8404".into(),
            name: REMARKS.into(),
            args: json!({"how": how}),
        };
        for heard in [
            "Stop noticing what I'm doing.",
            "Stop looking at my desk.",
            "Lamp, stop watching me.",
            "Please don\u{2019}t watch me.",
            "Can you stop looking at what I do?",
            "Turn off desk noticing.",
        ] {
            for how in ["off", "snooze"] {
                let stop = stop_instead(&remarks(how), Some(heard));
                assert_eq!(
                    stop,
                    Some(ToolCall {
                        id: "seen-8404".into(),
                        name: STOP.into(),
                        args: json!({}),
                    }),
                    "{heard} ({how})"
                );
            }
            // "On" is no stop, a line not heard is the model's, and
            // another function is never one.
            assert_eq!(stop_instead(&remarks("on"), Some(heard)), None, "{heard}");
            assert_eq!(stop_instead(&remarks("off"), None), None);
            let other = ToolCall {
                name: "wellbeing_nudges".into(),
                ..remarks("off")
            };
            assert_eq!(stop_instead(&other, Some(heard)), None, "{heard}");
        }
        for heard in [
            "Stop the remarks.",
            "Don't comment on what I do.",
            "Stop commenting on what you notice.",
            "Stop telling me what I'm doing.",
            "Snooze the remarks till morning.",
            "Stop that.",
            "I'm looking at my desk, stop it.",
        ] {
            assert_eq!(stop_instead(&remarks("off"), Some(heard)), None, "{heard}");
        }
    }

    #[test]
    fn the_stop_names_the_looking_and_the_remarks_name_it() {
        for by_default in [false, true] {
            let stop = &tools_for(by_default)[2].description;
            for said in [
                "stop looking at my desk",
                "stop noticing what I'm doing",
                "stop watching me",
                REMARKS,
            ] {
                assert!(stop.contains(said), "{by_default}: {said}: {stop}");
            }
        }
    }

    #[test]
    fn declared_only_with_the_switch_on_exactly() {
        assert_eq!(arguments(Some("on"), None, None), [FLAG]);
        assert_eq!(
            arguments(Some("on"), Some("on"), None),
            [FLAG, POSTURE_FLAG]
        );
        assert_eq!(
            arguments(Some("on"), Some("on"), Some("on")),
            [FLAG, POSTURE_FLAG, OFFER_FLAG]
        );
        assert_eq!(arguments(Some("on"), None, Some("on")), [FLAG, OFFER_FLAG]);
        assert!(arguments(None, Some("on"), Some("on")).is_empty());
        for value in [None, Some("yes"), Some("ON"), Some("")] {
            assert!(arguments(value, None, None).is_empty(), "{value:?}");
            assert_eq!(arguments(Some("on"), None, value), [FLAG], "{value:?}");
        }
        assert!(is_tool(OFFER) && is_tool(LOOK) && is_tool(STOP) && !is_tool("look"));
        // #7767, slice 6 (the first builder's 13:55 on #7767): with
        // LAMP_NOTICING_BY_DEFAULT on, the offer is never declared, so the
        // lamp never asks to send pictures while it already sends them.
        assert_eq!(
            arguments_or(Some("on"), Some("on"), Some("on"), true),
            [FLAG, POSTURE_FLAG]
        );
        assert_eq!(arguments_or(Some("on"), None, Some("on"), true), [FLAG]);
        assert_eq!(
            arguments_or(Some("on"), None, Some("on"), false),
            arguments(Some("on"), None, Some("on"))
        );
        assert!(arguments_or(None, Some("on"), Some("on"), true).is_empty());
        assert_eq!(
            path(),
            Path::new(lamp_msgs::consent::FOLDER).join("seen-activity.json")
        );
        assert_eq!(
            lamp_msgs::consent::file_in(
                Path::new(lamp_msgs::consent::FOLDER),
                lamp_msgs::consent::SEEN_ACTIVITY
            ),
            Some(path())
        );
        // Every description says the pictures go to the server, and the
        // offer's and the yes's that an ordinary look is no yes.
        let tools = tools();
        assert!(
            tools[..2]
                .iter()
                .all(|tool| tool.description.contains("server"))
        );
        assert!(tools[0].description.contains("where are my keys"));
        assert!(tools[1].description.contains("Never infer a yes"));
    }

    #[test]
    fn the_disclosure_names_the_server_and_posture_only_when_on() {
        for line in [SEEN_DISCLOSURE, SEEN_DISCLOSURE_POSTURE] {
            assert!(
                line.contains("to Autonomous's server to be labelled"),
                "{line}"
            );
            assert!(line.contains("they're deleted once they're checked"));
            assert!(line.contains("My light shows each time I send one"));
            assert!(line.contains("tell me to stop any time") && line.ends_with("Shall I?"));
            // What the yes covers, the first builder's c6957770745: tired
            // and celebrating as well as eating and drinking, and a reaction.
            assert!(line.starts_with(
                "I can notice when you eat, drink, take a break, look tired or celebrate,"
            ));
            assert!(line.contains("and I react to what I notice."));
        }
        assert_eq!(
            SEEN_DISCLOSURE_POSTURE,
            SEEN_DISCLOSURE.replacen("celebrate,", "celebrate, and how you're sitting,", 1)
        );
        assert!(!SEEN_DISCLOSURE.contains("sitting"));
        assert!(SEEN_DISCLOSURE_POSTURE.contains("celebrate, and how you're sitting,"));
        let mut offer = Offer::default();
        assert_eq!(offered(&mut offer, 4, true, false)["say"], SEEN_DISCLOSURE);
        assert_eq!(
            offered(&mut offer, 4, true, true)["say"],
            SEEN_DISCLOSURE_POSTURE
        );
        // Said in a typed line: nothing pending.
        let mut typed = Offer::default();
        offered(&mut typed, 4, false, false);
        assert_eq!(typed, Offer::default());
    }

    #[test]
    fn only_the_very_next_turn_after_the_offer_answers_it() {
        // Offered and played whole in 10, answered in 11: a yes there
        // counts, once.
        let mut offer = Offer::default();
        offer.said(10, SEEN_DISCLOSURE);
        offer.began(10);
        offer.played(10, SEEN_DISCLOSURE);
        offer.began(11);
        offer.began(11);
        assert!(offer.answered_in(11));
        assert!(!offer.answered_in(11), "an offer answers once");
        // A yes in the same turn as the offer: none.
        offer.said(20, SEEN_DISCLOSURE);
        offer.played(20, SEEN_DISCLOSURE);
        assert!(!offer.answered_in(20));
        // Another turn between: the offer is gone.
        offer.said(30, SEEN_DISCLOSURE);
        offer.played(30, SEEN_DISCLOSURE);
        offer.began(31);
        offer.began(32);
        assert!(!offer.answered_in(32));
        // No offer at all.
        assert!(!Offer::default().answered_in(5));
    }

    /// An offer given and answered in the very next turn: whether the
    /// owner's yes there counts, once the lamp played `played` of it in the
    /// offer's turn, and was cut or not.
    fn answered_after(played: &[(u64, &str)], cut: Option<u64>) -> bool {
        let mut offer = Offer::default();
        offer.said(50, SEEN_DISCLOSURE_POSTURE);
        offer.began(50);
        for (request, words) in played {
            offer.played(*request, words);
        }
        if let Some(request) = cut {
            offer.cut_in(request);
        }
        offer.began(51);
        offer.answered_in(51)
    }

    #[test]
    fn the_offer_counts_only_played_whole_and_uncut() {
        let (start, end) = SEEN_DISCLOSURE_POSTURE.split_at(80);
        // Whole, in pieces as played, with a word around it: an offer.
        assert!(answered_after(
            &[(50, "Sure. "), (50, start), (50, end)],
            None
        ));
        // Cut, interrupted or barged in after every word: none.
        assert!(!answered_after(&[(50, SEEN_DISCLOSURE_POSTURE)], Some(50)));
        // Only part of it played: none.
        assert!(!answered_after(&[(50, start)], None));
        // Not said at all, or said in another turn: none.
        assert!(!answered_after(&[], None));
        assert!(!answered_after(&[(49, SEEN_DISCLOSURE_POSTURE)], None));
        // The other wording than the one given: none.
        assert!(!answered_after(&[(50, SEEN_DISCLOSURE)], None));
        // A cut of another turn changes nothing.
        assert!(answered_after(&[(50, SEEN_DISCLOSURE_POSTURE)], Some(49)));
    }

    /// lamp-06c1, 17 Nov 17:59: the offer in request 29 was cut by a 0-word
    /// line, 30, before any word of it played; 30 was held and answered 29's
    /// words in its place, playing the disclosure whole; the yes in 31 found
    /// "no offer just before". `played`: what the lamp played in 30, `cut`:
    /// whether 30's answer was cut, `from`: the request whose words 30
    /// answers.
    fn yes_after_replay(played: &str, cut: bool, from: u64) -> bool {
        let mut offer = Offer::default();
        offer.said(29, SEEN_DISCLOSURE);
        offer.began(29);
        offer.began(30);
        offer.cut_in(29);
        offer.in_place(from, 30);
        offer.began(30);
        offer.played(30, played);
        if cut {
            offer.cut_in(30);
        }
        // A late cut of the request answered in its place changes nothing.
        offer.cut_in(29);
        offer.began(31);
        offer.answered_in(31)
    }

    #[test]
    fn an_offer_answered_in_a_cut_lines_place_counts_played_whole() {
        // Replayed whole in 30: the next yes counts, as the read-back's
        // (`services::answered_in_place`).
        assert!(yes_after_replay(SEEN_DISCLOSURE, false, 29));
        // The replay itself cut, or played only in part: none.
        assert!(!yes_after_replay(SEEN_DISCLOSURE, true, 29));
        assert!(!yes_after_replay(&SEEN_DISCLOSURE[..80], false, 29));
        // Another request's words answered in 30's place: the offer stays
        // 29's, and 31 is no answer to it.
        assert!(!yes_after_replay(SEEN_DISCLOSURE, false, 28));
        // Nothing pending: nothing moves.
        let mut none = Offer::default();
        none.in_place(29, 30);
        assert_eq!(none, Offer::default());
        // An offer already answered in place keeps its own disclosure.
        let mut posture = Offer::default();
        posture.said(29, SEEN_DISCLOSURE_POSTURE);
        posture.in_place(29, 30);
        assert_eq!(posture.said_in, Some(30));
        assert_eq!(posture.disclosure, SEEN_DISCLOSURE_POSTURE);
    }

    #[test]
    fn an_ordinary_look_another_turn_a_guest_or_a_television_keeps_nothing() {
        let folder = folder("informed");
        let path = file_in(&folder);
        // "Look at my desk, where are my keys?": no offer, so no yes,
        // even the owner's.
        assert_eq!(
            run_at(&path, LOOK, true, false, true, 7, || panic!("no write")),
            json!({"ok": false, "note": OFFER_FIRST})
        );
        assert!(!path.exists());
        let mut offer = Offer::default();
        offer.said(40, SEEN_DISCLOSURE);
        offer.played(40, SEEN_DISCLOSURE);
        offer.began(41);
        offer.began(42);
        let answered = offer.answered_in(42);
        assert_eq!(
            run_at(&path, LOOK, true, answered, true, 7, || panic!("no write"))["note"],
            OFFER_FIRST
        );
        // A guest's yes, or a television's (no owner seen, no owner's
        // voice), in the very next turn: not the owner's.
        assert_eq!(
            run_at(&path, LOOK, true, true, false, 7, || panic!("no write")),
            json!({"ok": false, "note": OWNER_ONLY})
        );
        assert!(!path.exists());
        // Through the route, with no offer pending for these requests.
        assert_eq!(
            run(&call(LOOK), 1, true, None, false, true)["note"],
            OFFER_FIRST
        );
        assert_eq!(
            run(&call(OFFER), 2, false, None, false, true)["say"],
            SEEN_DISCLOSURE
        );
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn the_owners_yes_to_the_offer_is_kept_whole_as_room_sounds_is() {
        let folder = folder("yes");
        let path = file_in(&folder);
        // Offered and played whole, then the owner's yes in the very next
        // turn.
        let mut offer = Offer::default();
        offer.said(60, SEEN_DISCLOSURE);
        offer.played(60, SEEN_DISCLOSURE);
        offer.began(61);
        let answered = offer.answered_in(61);
        // The reader group's: 0640 and that group (here the test's own).
        let group = std::fs::metadata(&folder).unwrap().gid();
        assert_eq!(
            run_at(&path, LOOK, true, answered, true, 7, || Some(group)),
            json!({"ok": true, "note": LOOKING})
        );
        assert_eq!(
            sans_rev(&path),
            br#"{"at_unix_s":7,"by":"voice","seen_activity":"yes","version":1}"#
        );
        let kept = std::fs::metadata(&path).unwrap();
        assert_eq!(kept.permissions().mode() & 0o777, 0o640);
        assert_eq!(kept.gid(), group);
        assert!(!path.with_extension("storage-next").exists());
        let read = lamp_msgs::consent::read(&path, lamp_msgs::consent::SEEN_ACTIVITY);
        assert!(read.yes && !read.by_app() && read.at_unix_s == Some(7));
        // Said again, kept as it was: lamp-seen says nothing new.
        assert_eq!(
            run_at(&path, LOOK, true, true, true, 9, || panic!("no write")),
            json!({"ok": true, "note": LOOKING_ALREADY})
        );
        assert_eq!(
            lamp_msgs::consent::read(&path, lamp_msgs::consent::SEEN_ACTIVITY).at_unix_s,
            Some(7)
        );
        // With no group known, root's alone.
        std::fs::remove_file(&path).unwrap();
        run_at(&path, LOOK, true, true, true, 8, || None);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn stop_and_any_forget_take_it_back() {
        let folder = folder("stop");
        let path = file_in(&folder);
        run_at(&path, LOOK, true, true, true, 7, || None);
        assert!(path.exists());
        // Anyone's stop, the owner's or not, with no offer: the app's mark
        // raised to the lamp's now (Heron's P2 on #7551), so the helper
        // refuses any desk noticing yes the app sent before it.
        let mark = lamp_msgs::consent::mark_in(&folder);
        assert_eq!(lamp_msgs::consent::read_mark(&mark), Some(0));
        let before = lamp_msgs::consent::now_unix_ms();
        assert_eq!(
            run_at(&path, STOP, true, false, false, 9, || panic!("no write")),
            json!({"ok": true, "note": STOPPED})
        );
        assert!(!path.exists());
        let stopped = lamp_msgs::consent::read_mark(&mark).unwrap();
        assert!(stopped >= before && stopped <= lamp_msgs::consent::now_unix_ms());
        assert_eq!(
            run_at(&path, STOP, true, false, false, 9, || None),
            json!({"ok": true, "note": NOT_LOOKING})
        );
        // An app yes too (`"by":"app"`), and with none there nothing.
        std::fs::write(
            &path,
            lamp_msgs::consent::bytes(
                lamp_msgs::consent::SEEN_ACTIVITY,
                5,
                lamp_msgs::consent::BY_APP,
            ),
        )
        .unwrap();
        // A forget raises it too; a mark already later stays.
        lamp_msgs::consent::raise_mark(&mark, 1).unwrap();
        let before = lamp_msgs::consent::now_unix_ms();
        forgotten(&path);
        assert!(!path.exists());
        assert!(lamp_msgs::consent::read_mark(&mark).unwrap() >= before.max(stopped));
        forgotten(&path);
        // A removal that fails says not done.
        std::fs::create_dir(&path).unwrap();
        assert_eq!(
            run_at(&path, STOP, true, true, true, 9, || None),
            json!({"ok": false, "note": NOT_KEPT})
        );
        let _ = std::fs::remove_dir_all(folder);
    }

    /// #7767: with `LAMP_NOTICING_BY_DEFAULT` on, where a missing file is
    /// on, the stop keeps a no for seen activity and face expressions both,
    /// in the yes's mode and with the mark raised, and says it stopped only
    /// when something was on; off, the stop removes, as before.
    #[test]
    fn with_the_switch_on_a_stop_keeps_a_no_for_both() {
        use lamp_msgs::consent::{
            BY_VOICE, FACE_EXPRESSIONS, SEEN_ACTIVITY, allowed, mark_in, no_bytes, read_mark,
        };
        let folder = folder("stop-default");
        let path = file_in(&folder);
        let face = crate::onboarding::face_expressions_beside(&folder.join("room-sounds.json"));
        let group = std::fs::metadata(&folder).unwrap().gid();
        let before = lamp_msgs::consent::now_unix_ms();
        // Nothing kept: on by default, so it stops.
        assert_eq!(
            run_by_default(&path, STOP, false, 9, || Some(group)),
            json!({"ok": true, "note": STOPPED})
        );
        assert_eq!(sans_rev(&path), no_bytes(SEEN_ACTIVITY, 9, BY_VOICE));
        assert_eq!(sans_rev(&face), no_bytes(FACE_EXPRESSIONS, 9, BY_VOICE));
        for file in [&path, &face] {
            let kept = std::fs::metadata(file).unwrap();
            assert_eq!(kept.permissions().mode() & 0o777, 0o640);
            assert_eq!(kept.gid(), group);
        }
        assert!(!allowed(&path, SEEN_ACTIVITY, true) && !allowed(&face, FACE_EXPRESSIONS, true));
        assert!(read_mark(&mark_in(&folder)).unwrap() >= before);
        // Again: a no already, so nothing was on.
        assert_eq!(
            run_by_default(&path, STOP, false, 10, || None),
            json!({"ok": true, "note": NOT_LOOKING})
        );
        assert!(!allowed(&path, SEEN_ACTIVITY, true));
        // A kept yes: a no in its place.
        std::fs::write(&path, lamp_msgs::consent::bytes(SEEN_ACTIVITY, 5, BY_VOICE)).unwrap();
        assert_eq!(
            run_by_default(&path, STOP, false, 11, || None)["note"],
            STOPPED
        );
        assert!(path.exists() && !allowed(&path, SEEN_ACTIVITY, true));
        // Off: the stop removes the file, as before, and leaves face's.
        run_at(&path, STOP, false, false, false, 0, || None);
        assert!(!path.exists() && face.exists());
        // A no that can't be written says not done (the held write's
        // temporary, `lamp_msgs::consent::keep_with_group`, blocked).
        std::fs::create_dir(path.with_extension("change-next")).unwrap();
        assert_eq!(
            run_by_default(&path, STOP, false, 12, || None),
            json!({"ok": false, "note": NOT_KEPT})
        );
        let _ = std::fs::remove_dir_all(folder);
    }

    /// Twelve's P3 1 and Heron's on #7928: with `LAMP_NOTICING_BY_DEFAULT`
    /// on, what was on is seen activity and face reads apart. A stop with
    /// only faces on says it stopped (in its own line, since lamp-seen says
    /// back only seen activity), and "start looking again" with only faces
    /// off says it is looking again, never that it was already.
    #[test]
    fn with_the_switch_on_the_answer_counts_face_reads_too() {
        use lamp_msgs::consent::{
            BY_APP, BY_VOICE, FACE_EXPRESSIONS, SEEN_ACTIVITY, allowed, no_bytes,
        };
        let folder = folder("faces-default");
        let path = file_in(&folder);
        let face = crate::onboarding::face_expressions_beside(&folder.join("room-sounds.json"));
        // The app's desk off, faces on by default: the stop stops faces.
        std::fs::write(&path, no_bytes(SEEN_ACTIVITY, 5, BY_APP)).unwrap();
        assert_eq!(
            run_by_default(&path, STOP, false, 9, || None),
            json!({"ok": true, "note": STOPPED_FACES})
        );
        assert!(!allowed(&face, FACE_EXPRESSIONS, true));
        // The mirror: seen activity on by default, faces a kept no.
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&face, no_bytes(FACE_EXPRESSIONS, 5, BY_VOICE)).unwrap();
        assert_eq!(
            run_by_default(&path, LOOK, true, 10, || None),
            json!({"ok": true, "note": LOOKING_FACES})
        );
        assert!(allowed(&face, FACE_EXPRESSIONS, false));
        assert!(!path.exists());
        // Both on now: on already.
        assert_eq!(
            run_by_default(&path, LOOK, true, 11, || panic!("no write")),
            json!({"ok": true, "note": LOOKING_ALREADY})
        );
        let _ = std::fs::remove_dir_all(folder);
    }

    /// #7767: with `LAMP_NOTICING_BY_DEFAULT` on, "start looking again" is
    /// the owner's own line alone, with no offer and no `APP_FIRST`: a kept
    /// no in either file turned back to a yes, a missing one left on by
    /// default, a guest's line refused. Off, the same call with no offer is
    /// refused, as before.
    #[test]
    fn with_the_switch_on_start_looking_again_is_the_owners_yes() {
        use lamp_msgs::consent::{BY_VOICE, FACE_EXPRESSIONS, SEEN_ACTIVITY, allowed, no_bytes};
        let folder = folder("look-default");
        let path = file_in(&folder);
        let face = crate::onboarding::face_expressions_beside(&folder.join("room-sounds.json"));
        // Missing: on already, and nothing written.
        assert_eq!(
            run_by_default(&path, LOOK, true, 7, || panic!("no write")),
            json!({"ok": true, "note": LOOKING_ALREADY})
        );
        assert!(!path.exists() && !face.exists());
        // After a stop: a guest's line keeps nothing; the owner's turns both on.
        std::fs::write(&path, no_bytes(SEEN_ACTIVITY, 5, BY_VOICE)).unwrap();
        std::fs::write(&face, no_bytes(FACE_EXPRESSIONS, 5, BY_VOICE)).unwrap();
        assert_eq!(
            run_by_default(&path, LOOK, false, 7, || panic!("no write")),
            json!({"ok": false, "note": OWNER_ONLY})
        );
        assert!(!allowed(&path, SEEN_ACTIVITY, true));
        assert_eq!(
            run_by_default(&path, LOOK, true, 8, || None),
            json!({"ok": true, "note": LOOKING})
        );
        assert_eq!(
            sans_rev(&path),
            lamp_msgs::consent::bytes(SEEN_ACTIVITY, 8, BY_VOICE)
        );
        assert!(allowed(&face, FACE_EXPRESSIONS, false));
        // Off: with no offer just before, refused as before.
        std::fs::write(&path, no_bytes(SEEN_ACTIVITY, 5, BY_VOICE)).unwrap();
        assert_eq!(
            run_at(&path, LOOK, false, false, true, 9, || panic!("no write")),
            json!({"ok": false, "note": APP_FIRST})
        );
        let _ = std::fs::remove_dir_all(folder);
    }

    /// lamp-06c1's third re-row of #8674 (26 Nov, Q3): the speech-to-text
    /// lost the lamp's name and half the line ("Now stop watching the"),
    /// and a person names the looking with seeing, the camera and the eyes
    /// too: each such line is told the state.
    #[test]
    fn a_line_with_any_word_of_the_looking_is_told_the_state() {
        for heard in [
            "Now stop watching the",
            "Lamp, can you see me?",
            "Are you seeing what I'm doing?",
            "Turn off your camera.",
            "Close your eyes.",
            "Keep your eye off me.",
        ] {
            assert!(names_the_noticing(heard), "{heard}");
        }
        for heard in ["What's the weather in Hanoi?", "Play some music.", "Stop."] {
            assert!(!names_the_noticing(heard), "{heard}");
        }
    }

    /// The replies that claim a stop of the looking, and the ones that
    /// don't: Q3's own ("I've stopped watching your desk") with its heard
    /// line, a bare "I've stopped." to a line naming the looking, the
    /// promises and the "not"s; never the lamp saying it is on, describing
    /// the desk, asking, or offering a way off.
    #[test]
    fn a_reply_claiming_a_stop_is_told_apart() {
        for (reply, heard) in [
            (
                "I've stopped watching your desk and noticing what you're doing.",
                Some("Now stop watching the"),
            ),
            ("I've stopped watching your desk.", None),
            ("I've stopped.", Some("Lamp, stop looking at my gaze.")),
            ("Okay, stopping now.", Some("Lamp, stop watching me.")),
            ("I've stopped looking at your desk.", None),
            ("I won't look at your desk anymore.", None),
            ("Okay. I'm no longer watching.", None),
            ("Sure, I'm not watching your desk now.", None),
            ("I'll stop noticing what you do.", None),
            ("I'm going to stop looking now.", None),
            ("Desk noticing is off now.", None),
            ("Done, my camera is off.", None),
            ("Got it, I'm not watching anymore.", None),
            ("I'm not looking at your desk anymore.", None),
            // The Reviewer-lead's must-fix on #8780 (c7195457427): a claim
            // followed by an offer to undo it is still a claim.
            (
                "I've stopped watching your desk, but you can turn it back on any time.",
                None,
            ),
            (
                "Done, my camera is off \u{2014} you can turn it back on whenever you like.",
                None,
            ),
            (
                "Okay, I've stopped noticing, just ask if you want it back.",
                None,
            ),
            ("Don't worry, I've stopped watching.", None),
            // Friday's re-read on #8780 (c7195959981): an offer governs only
            // its own clause and the ones after it.
            (
                "I've stopped watching your desk, you can ask me to start again anytime.",
                None,
            ),
            (
                "Okay, I've stopped watching, and if you want me back just say so.",
                Some("stop watching my desk"),
            ),
            ("Noticing is off, you can turn it on any time.", None),
        ] {
            assert!(claims_stopped(reply, heard), "{reply} to {heard:?}");
        }
        for (reply, heard) in [
            ("Yes, it's on.", Some("Lamp, are you watching my desk?")),
            (
                "Yes — I'm noticing what goes on at your desk now.",
                Some("Lamp, can you keep an eye on my desk?"),
            ),
            ("I don't see your keys on the desk.", None),
            ("That's not your desk, it's a table.", None),
            ("Do you want me to stop looking?", None),
            (
                "It's on already, and you can tell me to stop looking any time.",
                None,
            ),
            ("You can turn desk noticing off any time.", None),
            ("I stopped the timer.", Some("Stop the timer.")),
            ("Your desk looks tidy.", None),
            (SEEN_DISCLOSURE, None),
            // Friday's P2-a on #8780 (c7194711313): an offer in an earlier
            // clause of the sentence is still an offer.
            ("If you ask, my camera goes off.", None),
            ("Whenever you want, I'll turn my camera off.", None),
            // P2-b: a negated stop says the lamp is still watching.
            (
                "I haven't stopped watching your desk.",
                Some("Lamp, are you still watching my desk?"),
            ),
            ("No, I never stopped watching.", None),
            ("I haven\u{2019}t stopped watching your desk.", None),
            ("I have not stopped watching.", None),
            // P3: a stop on offer is no claim either.
            ("I'll stop watching if you'd like.", None),
        ] {
            assert!(!claims_stopped(reply, heard), "{reply} to {heard:?}");
        }
    }

    /// Q3's case whole: desk noticing on by default, heard "Now stop
    /// watching the", the reply "I've stopped watching your desk" and no
    /// call. The claim runs the stop once, so the kept state is no for both
    /// (`"by":"voice"`, as the [`STOP`] call keeps it); once both are off (a
    /// stop called first), a claim runs nothing; without the switch, nothing
    /// is watched.
    #[test]
    fn a_claimed_stop_with_no_call_runs_the_stop() {
        use lamp_msgs::consent::{FACE_EXPRESSIONS, SEEN_ACTIVITY, allowed};
        let folder = folder("claimed-stop");
        let path = file_in(&folder);
        let face = crate::onboarding::face_expressions_beside(&folder.join("room-sounds.json"));
        assert!(allowed(&path, SEEN_ACTIVITY, true));
        let mut watch = ClaimWatch::default();
        let heard = Some("Now stop watching the");
        assert!(watch.piece(
            Some(&path),
            3,
            "I've stopped watching your desk and noticing what you're doing.",
            heard
        ));
        assert!(!allowed(&path, SEEN_ACTIVITY, true));
        assert!(!allowed(&face, FACE_EXPRESSIONS, true));
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("\"by\":\"voice\"")
        );
        // Once a turn.
        assert!(!watch.piece(Some(&path), 3, "I've stopped.", heard));
        // Off already (the stop called before the words): the claim is true.
        assert!(!stop_claimed(&path, 4));
        // Pieces as said: no claim until the piece that makes one.
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(&face).unwrap();
        let mut watch = ClaimWatch::default();
        assert!(!watch.piece(Some(&path), 5, "Sure.", Some("Lamp, stop looking.")));
        assert!(allowed(&path, SEEN_ACTIVITY, true));
        assert!(watch.piece(Some(&path), 5, "I've stopped.", Some("Lamp, stop looking.")));
        assert!(!allowed(&path, SEEN_ACTIVITY, true));
        // The switch off (no path): nothing watched, nothing written.
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(&face).unwrap();
        let mut watch = ClaimWatch::default();
        assert!(!watch.piece(None, 6, "I've stopped watching your desk.", heard));
        assert!(!path.exists() && !face.exists());
        let _ = std::fs::remove_dir_all(folder);
    }

    /// The Reviewer-lead's must-fix 1 on #8674 (c7185816145): a state word
    /// in the person's line ("see", "look", "eye") made any "stopped",
    /// "paused" or "ended" in the reply a claim, and a "not" or "off" beside
    /// a looking word made one with no lamp in it. None of these is the
    /// lamp saying it stopped looking, so none runs the stop. Q3's claim, and
    /// the lamp's own "I've stopped watching", "I'm not looking at your
    /// desk", "I won't watch", "noticing is off", still are.
    #[test]
    fn a_stop_beside_a_state_word_is_no_claim_unless_the_lamp_stops_looking() {
        for (reply, heard) in [
            ("Music stopped.", Some("Let me see, stop the music")),
            ("Paused.", Some("Look, pause the timer")),
            (
                "It ended at three.",
                Some("Can you see when my meeting ended?"),
            ),
            ("That movie isn't worth watching.", None),
            ("Try not to look at the screen.", None),
            (
                "Take your eyes off the screen and stretch.",
                Some("Lamp, my eyes are tired."),
            ),
            ("Take your eyes off the screen and stretch.", None),
            ("I paused the video you were watching.", None),
            ("Should I stop watching?", None),
            ("Should I turn the camera off", None),
        ] {
            assert!(!claims_stopped(reply, heard), "{reply} to {heard:?}");
        }
        for (reply, heard) in [
            (
                "I've stopped watching your desk.",
                Some("Now stop watching the"),
            ),
            ("I've stopped watching.", None),
            ("I'm not looking at your desk.", None),
            ("I won't watch.", None),
            ("Noticing is off.", None),
        ] {
            assert!(claims_stopped(reply, heard), "{reply} to {heard:?}");
        }
    }

    /// The Reviewer-lead's must-fix 2 on #8674: the stop runs only for a
    /// person's spoken turn, where the [`STOP`] call could run. A turn with
    /// no heard line (Live's reminder, or a written turn's) runs nothing,
    /// whatever its reply says.
    #[test]
    fn a_claim_in_a_turn_nobody_spoke_runs_nothing() {
        use lamp_msgs::consent::{SEEN_ACTIVITY, allowed};
        let folder = folder("claimed-unheard");
        let path = file_in(&folder);
        let mut watch = ClaimWatch::default();
        assert!(!watch.piece(Some(&path), 3, "I've stopped watching your desk.", None));
        assert!(allowed(&path, SEEN_ACTIVITY, true) && !path.exists());
        let _ = std::fs::remove_dir_all(folder);
    }

    /// The Reviewer-lead's must-fix 3 on #8674: a Live sentence is judged
    /// only once it has ended, so a question is never taken for a claim
    /// before its "?" arrives.
    #[test]
    fn live_pieces_are_judged_once_their_sentence_ended() {
        use lamp_msgs::consent::{SEEN_ACTIVITY, allowed};
        let folder = folder("claimed-live");
        let path = file_in(&folder);
        let heard = Some("Lamp, my eyes hurt, should you keep looking?");
        let mut watch = ClaimWatch::default();
        for piece in ["Should I turn", " the camera off", "?", " Just say."] {
            assert!(!watch.piece(Some(&path), 4, piece, heard), "{piece}");
        }
        assert!(allowed(&path, SEEN_ACTIVITY, true) && !path.exists());
        let _ = std::fs::remove_dir_all(folder);
    }

    /// Must-fix 3's other half: a delta split inside a word ("stop" +
    /// "ped") is joined as given, so Q3's claim split that way still runs
    /// the stop.
    #[test]
    fn live_pieces_split_inside_a_word_are_joined_as_given() {
        use lamp_msgs::consent::{SEEN_ACTIVITY, allowed};
        let folder = folder("claimed-live-split");
        let path = file_in(&folder);
        let heard = Some("Now stop watching the");
        let mut watch = ClaimWatch::default();
        assert!(!watch.piece(Some(&path), 5, "Okay. I've stop", heard));
        assert!(watch.piece(Some(&path), 5, "ped watching your desk.", heard));
        assert!(!allowed(&path, SEEN_ACTIVITY, true));
        let _ = std::fs::remove_dir_all(folder);
    }

    /// Must-fix 3's end: a last sentence with no stop of its own is judged
    /// at the turn's end ([`ClaimWatch::end`]), so Q3's claim said with no
    /// "." still runs the stop, and a question left open still runs none.
    #[test]
    fn a_last_sentence_with_no_stop_is_judged_at_the_end() {
        use lamp_msgs::consent::{SEEN_ACTIVITY, allowed};
        let folder = folder("claimed-end");
        let path = file_in(&folder);
        let heard = Some("Now stop watching the");
        let mut watch = ClaimWatch::default();
        assert!(!watch.piece(Some(&path), 6, "Should I stop watching", heard));
        assert!(!watch.end(Some(&path), 6, heard));
        assert!(allowed(&path, SEEN_ACTIVITY, true) && !path.exists());
        let mut watch = ClaimWatch::default();
        assert!(!watch.piece(Some(&path), 7, "I've stopped watching your desk", heard));
        assert!(allowed(&path, SEEN_ACTIVITY, true));
        assert!(watch.end(Some(&path), 7, heard));
        assert!(!allowed(&path, SEEN_ACTIVITY, true));
        let _ = std::fs::remove_dir_all(folder);
    }

    /// lamp-06c1's re-row of #8674 (26 Nov, Q9): with `LAMP_NOTICING_BY_DEFAULT`
    /// on and noticing on already, a guest's (or a television's) "keep an
    /// eye on my desk" turns nothing on, so it is told it is on already, not
    /// `OWNER_ONLY`'s "couldn't turn that on", and nothing is written. With a
    /// kept no, a guest's line is still refused, and anyone's stop still
    /// keeps a no, as before: only the owner changes the consent to on.
    #[test]
    fn with_the_switch_on_a_guests_look_when_on_already_is_on_already() {
        use lamp_msgs::consent::{BY_VOICE, FACE_EXPRESSIONS, SEEN_ACTIVITY, allowed, no_bytes};
        let folder = folder("look-guest-on");
        let path = file_in(&folder);
        let face = crate::onboarding::face_expressions_beside(&folder.join("room-sounds.json"));
        // No file: on by default, both.
        assert_eq!(
            run_by_default(&path, LOOK, false, 7, || panic!("no write")),
            json!({"ok": true, "note": LOOKING_ALREADY})
        );
        assert!(!path.exists() && !face.exists());
        // Faces a kept no: not all on, so a guest's line is the owner's to give.
        std::fs::write(&face, no_bytes(FACE_EXPRESSIONS, 5, BY_VOICE)).unwrap();
        assert_eq!(
            run_by_default(&path, LOOK, false, 8, || panic!("no write")),
            json!({"ok": false, "note": OWNER_ONLY})
        );
        assert!(!allowed(&face, FACE_EXPRESSIONS, true) && !path.exists());
        // A guest's stop still keeps a no for both.
        std::fs::remove_file(&face).unwrap();
        assert_eq!(
            run_by_default(&path, STOP, false, 9, || None),
            json!({"ok": true, "note": STOPPED})
        );
        assert!(!allowed(&path, SEEN_ACTIVITY, true) && !allowed(&face, FACE_EXPRESSIONS, true));
        assert_eq!(
            run_by_default(&path, LOOK, false, 10, || panic!("no write")),
            json!({"ok": false, "note": OWNER_ONLY})
        );
        let _ = std::fs::remove_dir_all(folder);
    }

    /// lamp-06c1's re-row of #8674 (26 Nov, Q2, Q4, Q7b, Q10, Q10b, Q8b):
    /// every state line of the row names the noticing, an ordinary line
    /// doesn't, and the state is read as lamp-seen reads it: no file on by
    /// default, a kept no off, faces alone said apart, a kept yes on.
    #[test]
    fn the_state_note_names_the_state_read_and_what_to_call() {
        use lamp_msgs::consent::{
            BY_APP, BY_VOICE, FACE_EXPRESSIONS, SEEN_ACTIVITY, bytes, no_bytes,
        };
        for heard in [
            "Lamp, can you keep an eye on my desk?",
            "Lamp, are you watching my desk?",
            "Lamp, stop looking at my desk.",
            "Lamp, stop watching me.",
            "Lamp, stop looking.",
            "Lamp, what can you notice about my desk?",
        ] {
            assert!(names_the_noticing(heard), "{heard}");
        }
        assert!(!names_the_noticing("What's the weather in Hanoi?"));
        let folder = folder("state-note");
        let path = file_in(&folder);
        let face = crate::onboarding::face_expressions_beside(&folder.join("room-sounds.json"));
        assert_eq!(state_note(None, "Lamp, are you watching my desk?"), None);
        assert_eq!(state_note(Some(&path), "What's the weather?"), None);
        let on = state_note(Some(&path), "Lamp, are you watching my desk?").unwrap();
        assert!(on.contains("on by default") && on.contains("face"), "{on}");
        assert!(
            on.contains(STOP) && on.contains("never say you stopped"),
            "{on}"
        );
        std::fs::write(&path, no_bytes(SEEN_ACTIVITY, 5, BY_VOICE)).unwrap();
        let faces = state_note_at(&path, true);
        assert!(
            faces.contains("is off") && faces.contains("face seems"),
            "{faces}"
        );
        assert!(faces.contains(STOP) && faces.contains(LOOK), "{faces}");
        std::fs::write(&face, no_bytes(FACE_EXPRESSIONS, 5, BY_VOICE)).unwrap();
        let off = state_note_at(&path, true);
        assert!(
            off.contains("is off") && off.contains(LOOK) && !off.contains(STOP),
            "{off}"
        );
        std::fs::write(&path, bytes(SEEN_ACTIVITY, 5, BY_APP)).unwrap();
        let yes = state_note_at(&path, true);
        assert!(
            yes.contains("on (they said yes)") && !yes.contains("face"),
            "{yes}"
        );
        let _ = std::fs::remove_dir_all(folder);
    }

    /// #7767: only the descriptions of the look and the stop change with
    /// the switch; the names, the offer and the count stay.
    #[test]
    fn the_switch_changes_only_two_descriptions() {
        let today = tools_for(false);
        let on = tools_for(true);
        assert_eq!(today, tools_as_today());
        assert_eq!(on[0], today[0]);
        for (on, today) in on.iter().zip(&today) {
            assert_eq!(on.name, today.name);
            assert_eq!(on.parameters, today.parameters);
            assert_eq!(on.silent, today.silent);
        }
        assert!(on[1].description != today[1].description);
        assert!(on[1].description.contains("start looking"));
        assert!(on[1].description.contains("server"));
        assert!(on[1].description.contains("where are my"));
        assert!(!on[1].description.contains("offer"));
        assert!(on[2].description != today[2].description);
        assert!(on[2].description.contains("face"));
    }

    /// #7767, the first builder's answer 1: with `LAMP_NOTICING_BY_DEFAULT`
    /// on, a forget keeps a no (and a file that can't be read) in place and
    /// removes only a kept yes, so it never turns noticing on; off with no
    /// done mark, it removes whatever is there, as before.
    #[test]
    fn with_the_switch_on_a_forget_keeps_a_no() {
        use lamp_msgs::consent::{BY_APP, SEEN_ACTIVITY, allowed, bytes, no_bytes};
        let folder = folder("forget-default");
        let path = file_in(&folder);
        for text in [no_bytes(SEEN_ACTIVITY, 5, BY_APP), b"not json".to_vec()] {
            std::fs::write(&path, &text).unwrap();
            assert!(!forget_takes(&path, SEEN_ACTIVITY, true));
            forgotten_or(&path, true);
            assert_eq!(std::fs::read(&path).unwrap(), text);
            assert!(!allowed(&path, SEEN_ACTIVITY, true));
        }
        std::fs::write(&path, bytes(SEEN_ACTIVITY, 5, BY_APP)).unwrap();
        assert!(forget_takes(&path, SEEN_ACTIVITY, true));
        forgotten_or(&path, true);
        assert!(!path.exists() && allowed(&path, SEEN_ACTIVITY, true));
        // Off with no done mark: a no is removed, as any file was.
        std::fs::remove_file(folder.join(lamp_msgs::consent::OLD_NOS_DONE)).unwrap();
        for text in [no_bytes(SEEN_ACTIVITY, 5, BY_APP), b"not json".to_vec()] {
            std::fs::write(&path, &text).unwrap();
            assert!(forget_takes(&path, SEEN_ACTIVITY, false));
        }
        std::fs::write(&path, no_bytes(SEEN_ACTIVITY, 5, BY_APP)).unwrap();
        forgotten_or(&path, false);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(folder);
    }

    /// Twelve's P3 3 on #8631 (#7767): with the switch off and the done
    /// mark there, a forget still keeps a no (so on, off, on still reads it
    /// as no) and still removes a kept yes; with the mark gone, a no goes
    /// as before.
    #[test]
    fn with_the_switch_off_and_the_done_mark_a_forget_keeps_a_no() {
        use lamp_msgs::consent::{BY_APP, OLD_NOS_DONE, SEEN_ACTIVITY, allowed, bytes, no_bytes};
        let folder = folder("forget-done-mark");
        let path = file_in(&folder);
        // `folder` writes the done mark.
        assert!(lamp_msgs::consent::old_nos_done(&folder));
        let no = no_bytes(SEEN_ACTIVITY, 5, BY_APP);
        std::fs::write(&path, &no).unwrap();
        assert!(!forget_takes(&path, SEEN_ACTIVITY, false));
        forgotten_or(&path, false);
        assert_eq!(std::fs::read(&path).unwrap(), no);
        assert!(!allowed(&path, SEEN_ACTIVITY, true));
        std::fs::write(&path, bytes(SEEN_ACTIVITY, 5, BY_APP)).unwrap();
        assert!(forget_takes(&path, SEEN_ACTIVITY, false));
        forgotten_or(&path, false);
        assert!(!path.exists());
        std::fs::remove_file(folder.join(OLD_NOS_DONE)).unwrap();
        std::fs::write(&path, &no).unwrap();
        forgotten_or(&path, false);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(folder);
    }

    /// Twelve's P2 on #7551: the stop raises the mark and removes the yes
    /// under the mark's lock, so while root's helper holds it (its check and
    /// write of an app yes) the stop waits, then takes the yes back. A forget
    /// on a lamp with neither a yes nor a mark makes no lock either (only
    /// the fresh revision it renews, #8704).
    #[test]
    fn a_stop_waits_for_the_helpers_lock() {
        let folder = folder("lock");
        std::fs::remove_file(folder.join(lamp_msgs::consent::OLD_NOS_DONE)).unwrap();
        let path = file_in(&folder);
        forgotten(&path);
        let left: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, [lamp_msgs::consent::STATE_FILE]);
        let mark = lamp_msgs::consent::mark_in(&folder);
        let helper = lamp_msgs::consent::lock_mark(&folder).unwrap();
        std::thread::scope(|scope| {
            let stop = scope.spawn(|| run_at(&path, STOP, false, false, false, 9, || None));
            std::thread::sleep(std::time::Duration::from_millis(200));
            assert!(!stop.is_finished());
            // The helper's app yes, checked and kept under its lock.
            helper.raise(10).unwrap();
            std::fs::write(
                &path,
                lamp_msgs::consent::bytes(
                    lamp_msgs::consent::SEEN_ACTIVITY,
                    5,
                    lamp_msgs::consent::BY_APP,
                ),
            )
            .unwrap();
            drop(helper);
            assert_eq!(stop.join().unwrap(), json!({"ok": true, "note": STOPPED}));
        });
        assert!(!path.exists());
        assert!(lamp_msgs::consent::read_mark(&mark).unwrap() > 10);
        let _ = std::fs::remove_dir_all(folder);
    }

    /// The Reviewer-lead's P2 on #8704: a stop removes the yes even when its
    /// revision can't be kept (a folder in the state's temporary's place);
    /// a stop with no yes still renews the revision (their second P2).
    #[test]
    fn a_stop_removes_the_yes_when_its_revision_cannot_be_kept() {
        let folder = folder("blocked-rev");
        std::fs::remove_file(folder.join(lamp_msgs::consent::OLD_NOS_DONE)).unwrap();
        let path = file_in(&folder);
        lamp_msgs::consent::keep_yes_holding(
            &path,
            lamp_msgs::consent::SEEN_ACTIVITY,
            5,
            lamp_msgs::consent::BY_VOICE,
            None,
        )
        .unwrap();
        std::fs::create_dir(folder.join("app-revs.change-next")).unwrap();
        assert_eq!(
            run_at(&path, STOP, false, false, false, 0, || None),
            json!({"ok": true, "note": STOPPED})
        );
        assert!(!path.exists());
        std::fs::remove_dir(folder.join("app-revs.change-next")).unwrap();
        let state = std::fs::read(folder.join(lamp_msgs::consent::STATE_FILE)).unwrap();
        assert_eq!(
            run_at(&path, STOP, false, false, false, 0, || None),
            json!({"ok": true, "note": NOT_LOOKING})
        );
        assert_ne!(
            std::fs::read(folder.join(lamp_msgs::consent::STATE_FILE)).unwrap(),
            state,
            "a stop with no yes renews the revision"
        );
        let _ = std::fs::remove_dir_all(folder);
    }

    /// The revision an app `consent.get` hands out for seen activity now.
    fn app_rev(folder: &Path) -> String {
        lamp_msgs::consent::hold(folder)
            .unwrap()
            .rev(lamp_msgs::consent::SEEN_ACTIVITY)
            .unwrap()
    }

    /// The Reviewer-lead's second P2 on #8704: with no yes there, the app
    /// reads the revision, the person says stop (already off), and the
    /// app's set retried with the revision from before is stale: the stop
    /// renewed it. The answer is still NOT_LOOKING.
    #[test]
    fn a_stop_while_already_off_makes_an_older_app_yes_stale() {
        let folder = folder("stop-off-stale");
        std::fs::remove_file(folder.join(lamp_msgs::consent::OLD_NOS_DONE)).unwrap();
        let path = file_in(&folder);
        let old = app_rev(&folder);
        assert_eq!(
            run_at(&path, STOP, false, false, false, 0, || None),
            json!({"ok": true, "note": NOT_LOOKING})
        );
        assert!(!path.exists());
        assert_ne!(app_rev(&folder), old, "the app's old revision is stale");
        let _ = std::fs::remove_dir_all(folder);
    }

    /// The same on a fresh lamp: no yes and no state. The cloud's
    /// unheld read gives the revision for no file, the person says stop,
    /// and that revision never matches after: the stop made the state with
    /// a fresh revision.
    #[test]
    fn a_stop_on_a_fresh_lamp_makes_an_older_app_yes_stale() {
        let folder = folder("stop-fresh-stale");
        std::fs::remove_file(folder.join(lamp_msgs::consent::OLD_NOS_DONE)).unwrap();
        let path = file_in(&folder);
        let key = lamp_msgs::consent::SEEN_ACTIVITY;
        assert!(!folder.join(lamp_msgs::consent::STATE_FILE).exists());
        let old = lamp_msgs::consent::rev(&folder, key, &lamp_msgs::consent::Revs::read(&folder));
        assert_eq!(
            run_at(&path, STOP, false, false, false, 0, || None),
            json!({"ok": true, "note": NOT_LOOKING})
        );
        assert!(folder.join(lamp_msgs::consent::STATE_FILE).exists());
        assert_ne!(app_rev(&folder), old, "the app's old revision is stale");
        let _ = std::fs::remove_dir_all(folder);
    }

    /// As above for a forget: with no yes there, a forget after the app
    /// read the revision makes the app's set from before stale.
    #[test]
    fn a_forget_while_already_off_makes_an_older_app_yes_stale() {
        let folder = folder("forget-off-stale");
        std::fs::remove_file(folder.join(lamp_msgs::consent::OLD_NOS_DONE)).unwrap();
        let path = file_in(&folder);
        let old = app_rev(&folder);
        forgotten_or(&path, false);
        assert!(!path.exists());
        assert_ne!(app_rev(&folder), old, "the app's old revision is stale");
        let _ = std::fs::remove_dir_all(folder);
    }
}
