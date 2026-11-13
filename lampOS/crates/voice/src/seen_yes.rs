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
//! barged in, it is no offer. `look_at_my_desk` keeps the yes only in the
//! very next turn after it ([`turn_began`]), and only for the owner's own
//! spoken line:
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
//! side, and so does any forget ([`forgotten`]) and setup's reset.
//! lamp-seen only reads the file, and says the lines back itself, on and
//! off, so the model is told to add nothing to them.
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
/// The function the model calls first, to say the disclosure.
pub const OFFER: &str = "offer_desk_noticing";
/// The function the model calls on the owner's yes to the offer.
pub const LOOK: &str = "look_at_my_desk";
/// The function the model calls on a "stop looking".
pub const STOP: &str = "stop_looking_at_my_desk";

/// What the lamp says before any yes, the PM's wording on #6543: what it
/// notices, that pictures go to Autonomous's server to be recognised and
/// are deleted once checked, that the light shows each send, and that a
/// stop works any time. "Deleted once they're checked" waits on the
/// backend's written retention confirmation before the switch goes on
/// (#6912).
pub const SEEN_DISCLOSURE: &str = "I can notice when you eat, drink or take a break, so my reminders fit better. To do that, while you're at the desk I'd send a few pictures from my camera to Autonomous's server to be recognised, and they're deleted once they're checked. My light shows each time I send one, and you can tell me to stop any time. Shall I?";
/// [`SEEN_DISCLOSURE`] with posture on.
pub const SEEN_DISCLOSURE_POSTURE: &str = "I can notice when you eat, drink or take a break, and how you're sitting, so my reminders fit better. To do that, while you're at the desk I'd send a few pictures from my camera to Autonomous's server to be recognised, and they're deleted once they're checked. My light shows each time I send one, and you can tell me to stop any time. Shall I?";

/// Told with the disclosure.
pub const OFFER_SAY: &str = "Say exactly the words in \"say\", and nothing else, then wait for their answer. Only a plain yes in their very next words is a yes: then call look_at_my_desk.";
/// Refused: no offer was said just before.
pub const OFFER_FIRST: &str = "Not done: it must be offered first, in the lamp's own words, and only a yes right after counts. If they asked you to notice what they do at the desk, call offer_desk_noticing; if they only asked you to look at something, just look.";
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
/// Nothing to take back.
pub const NOT_LOOKING: &str =
    "It was not on: say in a few words that you are not looking at their desk.";

/// The provider worker's flags: [`FLAG`] with the switch `on`, and
/// [`POSTURE_FLAG`] after it with posture `on` too.
pub fn arguments(value: Option<&str>, posture: Option<&str>) -> Vec<String> {
    let mut flags = Vec::new();
    if value == Some("on") {
        flags.push(FLAG.to_owned());
        if posture == Some("on") {
            flags.push(POSTURE_FLAG.to_owned());
        }
    }
    flags
}

pub fn is_tool(name: &str) -> bool {
    matches!(name, OFFER | LOOK | STOP)
}

pub fn tools() -> [Tool; 3] {
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
/// memory scope; `posture`, `LAMP_SEEN_POSTURE`): the offer said, the
/// owner's yes kept, or the yes taken back, and what the model is told.
pub fn run(
    call: &ToolCall,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
    posture: bool,
) -> Value {
    let Ok(mut offer) = OFFERED.lock() else {
        return json!({"ok": false, "note": NOT_KEPT});
    };
    match call.name.as_str() {
        OFFER => offered(&mut offer, request, heard, posture),
        LOOK => {
            let answered = offer.answered_in(request);
            drop(offer);
            let owners = answered && owners_spoken_yes(request, heard, lease);
            run_at(
                &path(),
                LOOK,
                answered,
                owners,
                now_unix_s(),
                crate::onboarding::consent_group,
            )
        }
        _ => {
            drop(offer);
            run_at(&path(), STOP, false, false, 0, || None)
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

/// [`run`] at `path` for `look_at_my_desk` and the stop: `answered`, the
/// call's line is the very next after the offer, played whole; `owners`, it is the
/// owner's own spoken yes; `at`, now in Unix seconds; `group`, the reader
/// group's id, asked only for a yes to keep.
fn run_at(
    path: &Path,
    name: &str,
    answered: bool,
    owners: bool,
    at: u64,
    group: impl FnOnce() -> Option<u32>,
) -> Value {
    let key = lamp_msgs::consent::SEEN_ACTIVITY;
    if name == LOOK {
        if !answered {
            eprintln!("lamp-voice: a yes to seen activity with no offer just before: not kept");
            return json!({"ok": false, "note": OFFER_FIRST});
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
    match remove(path) {
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
/// journal only when there was one. With none there, nothing changes.
pub fn forgotten(path: &Path) {
    match remove(path) {
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
        assert_eq!(arguments(Some("on"), None), [FLAG]);
        assert_eq!(arguments(Some("on"), Some("on")), [FLAG, POSTURE_FLAG]);
        assert!(arguments(None, Some("on")).is_empty());
        for value in [None, Some("yes"), Some("ON"), Some("")] {
            assert!(arguments(value, None).is_empty(), "{value:?}");
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
                line.contains("to Autonomous's server to be recognised"),
                "{line}"
            );
            assert!(line.contains("they're deleted once they're checked"));
            assert!(line.contains("My light shows each time I send one"));
            assert!(line.contains("tell me to stop any time") && line.ends_with("Shall I?"));
        }
        assert_eq!(
            SEEN_DISCLOSURE_POSTURE,
            SEEN_DISCLOSURE.replacen("take a break", "take a break, and how you're sitting", 1)
        );
        assert!(!SEEN_DISCLOSURE.contains("sitting"));
        assert!(SEEN_DISCLOSURE_POSTURE.contains("take a break, and how you're sitting,"));
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

    #[test]
    fn an_ordinary_look_another_turn_a_guest_or_a_television_keeps_nothing() {
        let folder = folder("informed");
        let path = file_in(&folder);
        // "Look at my desk, where are my keys?": no offer, so no yes,
        // even the owner's.
        assert_eq!(
            run_at(&path, LOOK, false, true, 7, || panic!("no write")),
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
            run_at(&path, LOOK, answered, true, 7, || panic!("no write"))["note"],
            OFFER_FIRST
        );
        // A guest's yes, or a television's (no owner seen, no owner's
        // voice), in the very next turn: not the owner's.
        assert_eq!(
            run_at(&path, LOOK, true, false, 7, || panic!("no write")),
            json!({"ok": false, "note": OWNER_ONLY})
        );
        assert!(!path.exists());
        // Through the route, with no offer pending for these requests.
        assert_eq!(run(&call(LOOK), 1, true, None, false)["note"], OFFER_FIRST);
        assert_eq!(
            run(&call(OFFER), 2, false, None, false)["say"],
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
            run_at(&path, LOOK, answered, true, 7, || Some(group)),
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
            run_at(&path, LOOK, true, true, 9, || panic!("no write")),
            json!({"ok": true, "note": LOOKING_ALREADY})
        );
        assert_eq!(
            lamp_msgs::consent::read(&path, lamp_msgs::consent::SEEN_ACTIVITY).at_unix_s,
            Some(7)
        );
        // With no group known, root's alone.
        std::fs::remove_file(&path).unwrap();
        run_at(&path, LOOK, true, true, 8, || None);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn stop_and_any_forget_take_it_back() {
        let folder = folder("stop");
        let path = file_in(&folder);
        run_at(&path, LOOK, true, true, 7, || None);
        assert!(path.exists());
        // Anyone's stop, the owner's or not, with no offer.
        assert_eq!(
            run_at(&path, STOP, false, false, 9, || panic!("no write")),
            json!({"ok": true, "note": STOPPED})
        );
        assert!(!path.exists());
        assert_eq!(
            run_at(&path, STOP, false, false, 9, || None),
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
        forgotten(&path);
        assert!(!path.exists());
        forgotten(&path);
        // A removal that fails says not done.
        std::fs::create_dir(&path).unwrap();
        assert_eq!(
            run_at(&path, STOP, true, true, 9, || None),
            json!({"ok": false, "note": NOT_KEPT})
        );
        let _ = std::fs::remove_dir_all(folder);
    }
}
