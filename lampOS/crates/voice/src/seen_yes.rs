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
    let mut flags = Vec::new();
    if value == Some("on") {
        flags.push(FLAG.to_owned());
        if posture == Some("on") {
            flags.push(POSTURE_FLAG.to_owned());
        }
        if offer == Some("on") {
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
                        what they are doing, or to stop sending pictures (\"stop looking\", \"don't \
                        watch me\"). It stops noticing what they do at the desk and how their face \
                        seems. Then say only what it tells you.";

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
                          are doing, or to stop sending pictures (\"stop looking\", \"don't watch me\"). \
                          Then say only what it tells you."
                .into(),
            parameters: None,
            silent: false,
        },
    ]
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
/// - `look_at_my_desk`: the owner's line alone, with no offer before it (the
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
            Ok(()) if seen_was && face_was => json!({"ok": true, "note": LOOKING_ALREADY}),
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
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    }
    if let Some(folder) = path.parent() {
        std::fs::File::open(folder)?.sync_all()?;
    }
    Ok(true)
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
/// builder's answer 1 on #7767): always with `by_default` off, as before;
/// with it on only a kept yes, which leaves noticing on by default, so a
/// kept no (or a file that can't be read, off) stays and a forget never
/// turns noticing on.
pub fn forget_takes(path: &Path, key: &str, by_default: bool) -> bool {
    !by_default || lamp_msgs::consent::read(path, key).yes
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
            std::fs::read(&path).unwrap(),
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
        assert_eq!(
            std::fs::read(&path).unwrap(),
            no_bytes(SEEN_ACTIVITY, 9, BY_VOICE)
        );
        assert_eq!(
            std::fs::read(&face).unwrap(),
            no_bytes(FACE_EXPRESSIONS, 9, BY_VOICE)
        );
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
        // A no that can't be written says not done.
        std::fs::create_dir(path.with_extension("storage-next")).unwrap();
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
            std::fs::read(&path).unwrap(),
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
    /// removes only a kept yes, so it never turns noticing on; off, it
    /// removes whatever is there, as before.
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
            assert!(forget_takes(&path, SEEN_ACTIVITY, false));
        }
        std::fs::write(&path, bytes(SEEN_ACTIVITY, 5, BY_APP)).unwrap();
        assert!(forget_takes(&path, SEEN_ACTIVITY, true));
        forgotten_or(&path, true);
        assert!(!path.exists() && allowed(&path, SEEN_ACTIVITY, true));
        // Off: a no is removed, as any file was.
        std::fs::write(&path, no_bytes(SEEN_ACTIVITY, 5, BY_APP)).unwrap();
        forgotten_or(&path, false);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(folder);
    }

    /// Twelve's P2 on #7551: the stop raises the mark and removes the yes
    /// under the mark's lock, so while root's helper holds it (its check and
    /// write of an app yes) the stop waits, then takes the yes back. A forget
    /// on a lamp with neither a yes nor a mark makes no lock either.
    #[test]
    fn a_stop_waits_for_the_helpers_lock() {
        let folder = folder("lock");
        std::fs::remove_file(folder.join(lamp_msgs::consent::OLD_NOS_DONE)).unwrap();
        let path = file_in(&folder);
        forgotten(&path);
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
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
}
