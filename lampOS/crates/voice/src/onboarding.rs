//! The first five minutes (`docs/onboarding.md`, #841): the lamp walks its
//! new person through where to sit, their name and face, and a few
//! preferences, one step at a time.
//!
//! The installed skill owns the order and words; the runner owns timing. Every line is the
//! model's, from a written note per step, begun only when the lamp is at
//! rest and someone is in front of it. A step the model leads ends when the
//! model calls [`STEP`]; the provider worker writes that into a small
//! mailbox file beside the onboarding file, and only the runner writes the
//! onboarding file itself. "Where you sit" is ended by what the eyes
//! measure, and "touch" by Controls' head-pad event on the bus. The steps
//! other builders own are placeholders: recorded as not available, not
//! spoken, and passed over.
//!
//! Off unless `LAMP_ONBOARDING=on` and `LAMP_ONBOARDING_FILE` names an
//! absolute path: then nothing here runs, no function is declared and no
//! file is read or written.
pub(crate) mod giving_way;
pub(crate) mod hardware;
#[cfg(test)]
pub(crate) mod legacy_words;
pub(crate) mod near;
pub(crate) mod privacy;
pub mod skill;
pub(crate) mod storage;
#[cfg(test)]
pub(crate) use legacy_words::*;
#[cfg(test)]
const OFFER_EVERY_S: u64 = 86_400;

use crate::wire::PrivacyReceipt;
use lamp_gemini::{Tool, ToolCall};
use lamp_turn_owner::BootId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

pub const ENVIRONMENT: &str = "LAMP_ONBOARDING";
pub const FILE_ENVIRONMENT: &str = "LAMP_ONBOARDING_FILE";
/// `on`: a step the model ends is nudged to report itself once answered,
/// "who you are" falls back to the name heard, and "where you sit" takes a
/// steady face after its guide while the head moves (row 10 try 4 on
/// lamp-52e6). Off: the walk as before.
/// How long "who you are" waits, after the latest yes following the name
/// (the face question's, as a rule), for the face to be saved before it ends
/// done with the name only, once [`START_QUIET_US`] has been quiet. The model calls
/// `remember_my_face` in its answer to that yes, a few seconds on, and the
/// eyes answer in about 1.7 s (lamp-29e7): well inside it, so the step is
/// never done before a face that is being saved is kept (the lead's merge
/// condition on #1711).
const FACE_SAVE_WAIT_US: u64 = 15_000_000;

pub const NUDGE_ENVIRONMENT: &str = "LAMP_ONBOARDING_NUDGE";
/// The provider worker's flag: the onboarding file, whose mailbox it writes.
pub const FLAG: &str = "--onboarding-file";
pub const STEP: &str = "setup_step";
pub const REDO: &str = "redo_setup";

/// A step begins after this much quiet.
const START_QUIET_US: u64 = 3_000_000;
/// "Where you sit": the framing must hold this long ...
const HOLD_US: u64 = 3_000_000;
/// ... and is given up on after this long, with a word on what to do later.
const SIT_GIVE_UP_US: u64 = 60_000_000;
/// One word on how to move the lamp, after this much quiet.
const GUIDE_EVERY_US: u64 = 8_000_000;
/// A step the model leads and never reports has failed after this long,
/// once the room has been quiet for [`TIMED_OUT_QUIET_US`].
const STEP_GIVE_UP_US: u64 = 90_000_000;
/// "Who you are", with the nudge on: the name heard is asked back no
/// sooner than this after the nudge, so an answer on the face can come.
const ASK_BACK_AFTER_US: u64 = 10_000_000;
const TIMED_OUT_QUIET_US: u64 = 20_000_000;
/// Nobody in front of the lamp this long: the step being led is dropped and
/// asked again when someone is back.
const GONE_US: u64 = 30_000_000;
/// The mailbox is looked at this often.
const READ_EVERY_US: u64 = 500_000;
/// The near face, as the hello counts it (`speak_up`), and how far from the
/// middle of the picture its centre may be (-1 to 1 across and down).
const NEAR: f32 = 0.14;
/// A seated person a lamp looks up at is a little high in its picture: the
/// lead's chair at about 1 m gave y -0.26 (row 10, 21 October). A
/// calibration knob, not a law.
const MIDDLE: f32 = 0.35;
/// "Where you sit": a face off the middle is looked at (the body's gaze)
/// this long before the person is asked to turn the lamp.
const LOOK_FIRST_US: u64 = 5_000_000;
/// "Where you sit" after its guide, with the nudge on: how far a face's
/// centre may wander and still be held.
const STEADY: f32 = 0.1;
/// "Where you sit", with the nudge on: the walk begins only once a face
/// at least [`NEAR`] has stayed this long, so someone passing far behind
/// an empty seat does not start it (row 10 try 5 on lamp-52e6: seated,
/// 0.14 to 0.18).
const LINGER_US: u64 = 2_000_000;
/// A look on the bus older than this says nothing of now.
const FRESH_MS: u64 = 700;
/// "Touch": no touch on the head this long after the ask has failed.
const TOUCH_GIVE_UP_US: u64 = 60_000_000;
/// The name the walk reads Controls' touches under.
const SUBSCRIBER: &str = "voice_onboarding";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    #[default]
    WhereYouSit,
    WhoYouAre,
    HowYouSound,
    Touch,
    Privacy,
    GivingWay,
    RoomSounds,
    FaceExpressions,
    Preferences,
}

impl Step {
    /// Stable mechanism IDs for validation and the tool schema, not walk order.
    pub const IDS: [&'static str; 9] = [
        "where_you_sit",
        "who_you_are",
        "how_you_sound",
        "touch",
        "privacy",
        "giving_way",
        "room_sounds",
        "face_expressions",
        "preferences",
    ];

    pub fn name(self) -> &'static str {
        match self {
            Step::WhereYouSit => "where_you_sit",
            Step::WhoYouAre => "who_you_are",
            Step::HowYouSound => "how_you_sound",
            Step::Touch => "touch",
            Step::Privacy => "privacy",
            Step::GivingWay => "giving_way",
            Step::RoomSounds => ROOM_SOUNDS,
            Step::FaceExpressions => FACE_EXPRESSIONS,
            Step::Preferences => "preferences",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        match name {
            "where_you_sit" => Some(Self::WhereYouSit),
            "who_you_are" => Some(Self::WhoYouAre),
            "how_you_sound" => Some(Self::HowYouSound),
            "touch" => Some(Self::Touch),
            "privacy" => Some(Self::Privacy),
            "giving_way" => Some(Self::GivingWay),
            "room_sounds" => Some(Self::RoomSounds),
            "face_expressions" => Some(Self::FaceExpressions),
            "preferences" => Some(Self::Preferences),
            _ => None,
        }
    }

    /// PLACEHOLDER (#841): the steps whose parts other builders own. Each
    /// is passed over until its part can be measured from here. Privacy,
    /// how they sound (#1803) and giving way (#1800) have their own
    /// availability; touch needs Controls' head pads on the bus as well
    /// ([`Onboarding::step_available`]).
    fn available(self) -> bool {
        matches!(
            self,
            Step::WhereYouSit | Step::WhoYouAre | Step::Touch | Step::Preferences
        )
    }

    /// What the model may report of this step: a measured step only that
    /// it was skipped.
    fn reportable(self, outcome: Outcome) -> bool {
        match self {
            Step::WhereYouSit
            | Step::Touch
            | Step::Privacy
            | Step::HowYouSound
            | Step::GivingWay => outcome == Outcome::Skipped,
            _ => outcome != Outcome::NotAvailable,
        }
    }

    /// A step the model can only skip, other than privacy: its skip is said
    /// by the runner from the record, in the skill's words (#3769).
    fn says_its_skip(self) -> bool {
        matches!(
            self,
            Step::WhereYouSit | Step::Touch | Step::HowYouSound | Step::GivingWay
        )
    }

    /// Only the GivingWay step may request a completion movement.
    /// Other onboarding steps stay physically still (the owner's #841 decision).
    fn gesture(self) -> &'static str {
        match self {
            Step::WhoYouAre => "greeting",
            _ => "nod",
        }
    }
}

/// The room sounds step (#1722 item 1): asked only on a lamp with no
/// privacy slide (`LAMP_PRIVACY_CONTROLS=no-slide`), where music and
/// laughter are off until a yes. A yes (`done`) is kept as [`ROOM_SOUNDS_PATH`]
/// for lamp-ears; no (`skipped`) or silence (`failed`) keeps only the step's
/// outcome, so it is asked once and stays off.
pub const ROOM_SOUNDS: &str = "room_sounds";
/// Outside the voice's 0700 folder, so that lamp-ears (`lampos`) can read
/// it: `deploy/product/lampos.conf` makes the folder 0750 root:lampos.
pub const ROOM_SOUNDS_PATH: &str = "/var/lib/lampOS/consent/room-sounds.json";
/// The group, by name in `/etc/group`, that may read [`ROOM_SOUNDS_PATH`].
pub const ROOM_SOUNDS_GROUP: &str = "lampos";
/// The only value of `LAMP_PRIVACY_CONTROLS` that offers the step.
pub const NO_SLIDE: &str = "no-slide";
/// The face expressions step (#2872): asked only with
/// [`FACE_EXPRESSIONS_ASK_ENVIRONMENT`] on, a lamp whose eyes read how a face
/// seems, and only with a yes for this lamp, covering everyone it sees. Kept as the room sounds yes is: a
/// yes (`done`) as [`FACE_EXPRESSIONS_PATH`] for lamp-eyes, beside
/// [`ROOM_SOUNDS_PATH`] and readable by the same group; no or silence keeps
/// only the step's outcome, so it is asked once and stays off.
pub const FACE_EXPRESSIONS: &str = "face_expressions";
/// The file lamp-eyes reads (`lamp_eyes::seems::FACE_EXPRESSIONS_FILE`).
pub const FACE_EXPRESSIONS_PATH: &str = "/var/lib/lampOS/consent/face-expressions.json";
/// [`FACE_EXPRESSIONS_PATH`] for a voice whose room sounds yes is at
/// `room_sounds`: its sibling, so a test's folder keeps both.
pub fn face_expressions_beside(room_sounds: &Path) -> PathBuf {
    room_sounds.with_file_name("face-expressions.json")
}
/// `on` offers the face expressions step; anything else passes it over.
pub const FACE_EXPRESSIONS_ASK_ENVIRONMENT: &str = "LAMP_FACE_EXPRESSIONS_ASK";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Done,
    Skipped,
    Failed,
    NotAvailable,
}

impl Outcome {
    fn named(name: &str) -> Option<Self> {
        match name {
            "done" => Some(Outcome::Done),
            "skipped" => Some(Outcome::Skipped),
            "failed" => Some(Outcome::Failed),
            _ => None,
        }
    }
}

/// How an invitation's turn ended, as the coordinator saw it (#3754).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// Played whole and heard: only this opens a try.
    Whole,
    /// Some of it played, then cut (someone spoke over it).
    Cut,
    /// Played to its end and complete, with nobody's turn taken over it, but
    /// with a gap in its playback: on lamp-52e6 a yield to the lamp's own
    /// voice that resumed (#4035). Every word was said, though not as one
    /// line: like [`Self::Unsaid`], the person was never asked as the step
    /// asks, so it is no try of theirs.
    Gapped,
    /// Nothing of it played (`no_audio_answer`, a provider that never
    /// began): the person was never asked, so it is no try of theirs.
    Unsaid,
}

impl Delivery {
    /// A bound invitation's turn that ended without settling: cut if any
    /// of it played, else never said.
    pub(crate) fn unsettled(had_audio: bool) -> Self {
        if had_audio { Self::Cut } else { Self::Unsaid }
    }
}

/// A step's invitation the voice never said is offered again this many
/// times at most (#3754); past them the step is `not_available`, offered
/// again later as any step that was, and never failed.
const MOST_UNSAID: u8 = 2;
/// Why a step was left unasked, on its record.
const NOT_SAID: &str = "invitation not said";

/// Where the near face is: its height as a share of the picture's, and its
/// centre from the middle, -1 to 1 across and down.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Framing {
    pub size: f32,
    pub x: f32,
    pub y: f32,
}

impl Framing {
    fn good(&self) -> bool {
        self.size >= NEAR && self.x.abs() <= MIDDLE && self.y.abs() <= MIDDLE
    }

    /// Still where `from` was: centre within [`STEADY`] each way, size
    /// within a fifth.
    fn steady(&self, from: &Framing) -> bool {
        (self.x - from.x).abs() <= STEADY
            && (self.y - from.y).abs() <= STEADY
            && (self.size - from.size).abs() <= from.size * 0.2
    }
}

/// What a step came to, as kept in the onboarding file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    pub outcome: Outcome,
    pub at_unix_s: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framing: Option<Framing>,
    /// When a step that was not available was last offered again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offered_at_unix_s: Option<u64>,
}

/// The onboarding file: each step's outcome, by its name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub steps: BTreeMap<String, Kept>,
    /// Durable deduplication of verified physical-close/reopen completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy_done_attempt: Option<BootId>,
    /// At most two whole invitation attempts per setup/redo, across restarts.
    #[serde(default)]
    pub privacy_invitation_tries: u8,
    /// Redo's durable tombstone precedes deletion of any old checkpoint.
    #[serde(default)]
    pub privacy_discard_pending: bool,
    /// Near-call prompts committed this setup/redo, before each is published
    /// (#1803): at most `lamp_near_call::MOST_PROMPTS`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub near_call_prompts: u8,
    /// The accepted near-call tries so far: numbers only, never sound.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub near_call_tries: Vec<lamp_near_call::Try>,
    /// Prompts for the step's sentence part committed this setup/redo,
    /// before each is published: at most [`near::SENTENCE_PROMPTS`]. Only
    /// the count: nothing of the sentence is kept.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sentence_prompts: u8,
    /// Giving-way invitations committed this setup/redo, before each is
    /// published (#1800): at most two, so a rebuilt walk cannot ask again.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub giving_way_tries: u8,
}

fn is_zero(value: &u8) -> bool {
    *value == 0
}

impl Progress {
    /// The storage executor accepts only the bounded onboarding schema.
    pub(crate) fn check(&self) -> io::Result<()> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid onboarding progress");
        if self.privacy_invitation_tries > 2 || self.giving_way_tries > giving_way::MOST_TRIES {
            return Err(invalid());
        }
        // A step this build doesn't know (a later build's, rolled back) is
        // read past and kept as it is, never a refused file: the walk goes
        // on with the steps it knows (#2110). The file's 16 KiB bound holds
        // how many there may be.
        for kept in self.steps.values() {
            if let Some(frame) = kept.framing
                && (!(0.0..=1.0).contains(&frame.size)
                    || !(-1.0..=1.0).contains(&frame.x)
                    || !(-1.0..=1.0).contains(&frame.y))
            {
                return Err(invalid());
            }
        }
        // Persisted samples are validated on every load and commit (#1803).
        let tries = &self.near_call_tries;
        if usize::from(self.near_call_prompts) > lamp_near_call::MOST_PROMPTS
            || tries.len() > lamp_near_call::TRIES
            || tries.len() > usize::from(self.near_call_prompts)
            || !tries.iter().all(near::valid_try)
            || self.sentence_prompts > near::SENTENCE_PROMPTS
            // The step is done by its sentence part, which asked at least
            // once; the name part's bar is its own file.
            || self
                .steps
                .get(Step::HowYouSound.name())
                .is_some_and(|kept| kept.outcome == Outcome::Done && self.sentence_prompts == 0)
        {
            return Err(invalid());
        }
        if self.privacy_done_attempt.is_some()
            && !self
                .steps
                .get("privacy")
                .is_some_and(|step| step.outcome == Outcome::Done)
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Default)]
struct Led {
    step: Step,
    asked_us: Option<u64>,
    held_since: Option<u64>,
    guided_us: Option<u64>,
    /// "Where you sit" after its guide: the framing the hold began at.
    held_at: Option<Framing>,
    /// Lines with words the person has said since the step was asked.
    answers: u8,
    /// When the one note to report the step was given.
    nudged_us: Option<u64>,
    /// "Who you are": whether the lamp has spoken since the ask, and
    /// whether a line after that has been looked at for a name.
    spoke: bool,
    name_heard: bool,
    /// "Who you are": the first line after the ask, when it could be a
    /// name. Kept here only, never recorded.
    name: Option<String>,
    confirm: Confirm,
    /// "Where you sit": since when a face has been in view, looked at.
    looked_since: Option<u64>,
    /// What of the answers has been heard ([`Onboarding::heard`]).
    heard: Answers,
    /// "Touch": a touch on the head felt since the ask, kept until the step
    /// is looked at (#3756).
    touched: bool,
    /// Asked again after its conversation ended unanswered ([`MOST_UNSAID`]).
    asked_again: u8,
}

/// "Who you are"'s fallback: the name heard, asked back once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Confirm {
    #[default]
    NotAsked,
    /// "Did I get that right?" asked; their answer, once heard.
    Asked(Option<bool>),
    /// Said no: asked once more for the name, and no fallback again.
    Over,
}

/// The answers a step the model leads by talk has heard, never their words.
#[derive(Clone, Copy, Debug, Default)]
struct Answers {
    name: bool,
    /// With a face to keep: when the latest yes after the name was heard
    /// (the face question's, as a rule), so the step ends done at [`FACE_SAVE_WAIT_US`] if no face
    /// is saved by then.
    face_answered_us: Option<u64>,
    quiet_hours: bool,
    hello: bool,
}

struct PrivacyContext {
    epoch: privacy::Epoch,
    receipt: Option<PrivacyReceipt>,
    location: Option<String>,
}

struct PrivacyOffer {
    token: u64,
    attempt: u64,
    ready: bool,
    claimed: bool,
    persisted: bool,
    offered_us: u64,
    /// When its words were taken: the invitation begins, and a close from
    /// then on is the person's answer (the lead's 52e6 run, 21 October).
    claimed_us: u64,
    bound: Option<(BootId, u64)>,
}

#[derive(Default)]
struct PrivacyFlow {
    context: Option<PrivacyContext>,
    loaded: bool,
    maintenance_failed: bool,
    attempt: Option<privacy::Attempt>,
    offer: Option<PrivacyOffer>,
    generation: u64,
    offered: u64,
    failures: u8,
    /// Invitations the voice never said, offered again ([`MOST_UNSAID`]).
    unsaid: u8,
    retry_after_us: u64,
    pending_result: Option<(Outcome, Option<BootId>)>,
    erase_pending: bool,
    last_open: Option<(String, String, u64)>,
    /// Since when privacy has been led with no open, fresh receipt and
    /// nothing offered (Third's P3 on #1792).
    no_receipt_since: Option<u64>,
    /// That wait passed [`SIT_GIVE_UP_US`]: privacy is not available until an
    /// open, fresh receipt is seen again. Memory only.
    waited_out: bool,
    /// The Done just decided also proved the camera closed (#1802).
    camera_proven: bool,
    /// Whether eyes that give close receipts were on the bus at the last
    /// look: read when an attempt is armed and on each tick while it is, so
    /// the mute reads only this (#1948's P3).
    eyes_give_receipts: bool,
    /// The fresh token for the armed attempt's camera close, made when it
    /// is armed, so the mute makes none.
    camera_token: Option<u64>,
}

fn fresh_token() -> io::Result<u64> {
    let bytes = crate::process::new_boot()?.bytes();
    let first = u64::from_le_bytes(bytes[..8].try_into().expect("eight bytes"));
    Ok(if first != 0 {
        first
    } else {
        u64::from_le_bytes(bytes[8..].try_into().expect("eight bytes"))
    })
}

// The voice's own teardown is verified for the microphones. The camera is
// claimed only when the eyes' receipt for this close was read (#1802):
// without eyes that give receipts, the microphones alone are claimed.
/// The eyes' receipt for a close is said within this of the ask, or the
/// camera is not proven: the eyes shut it within half a second when well.
pub(crate) const CAMERA_RECEIPT_MS: u64 = 2_000;
/// The eyes' name on the bus (`lamp_eyes::bus_out::SOURCE`).
const EYES_SOURCE: &str = "eyes";

pub struct Onboarding {
    file: PathBuf,
    definition: Arc<skill::Definition>,
    /// Desired state; only this runner mutates it. Old I/O completions never replace it.
    desired: Progress,
    committed: Progress,
    storage: storage::Client,
    initialized: bool,
    storage_epoch: u64,
    storage_busy: bool,
    storage_failed: bool,
    dirty: bool,
    erase_on_commit: bool,
    flight: Option<storage::Tag>,
    barrier: Option<storage::Tag>,
    mailbox_revision: Option<(u64, u64)>,
    checkpoint_bytes: Option<Vec<u8>>,
    pending_records: Vec<Value>,
    #[cfg(test)]
    fixture_storage: Option<storage::ServiceStorage>,
    led: Option<Led>,
    /// A word owed before the next step: how "where you sit" ended.
    owed: Option<String>,
    seen_us: Option<u64>,
    read_us: Option<u64>,
    finished_recorded: bool,
    records: Vec<Value>,
    eyes: Option<lamp_bus::Bus>,
    looked: Option<(u64, Option<Framing>)>,
    /// When the body was last read, and whether it was still then.
    felt: Option<(u64, bool)>,
    /// Where the head joints were last asked to be.
    goals: Option<[f64; 3]>,
    /// Whether the body has said where its joints are, or whether a hand
    /// is on it, since this voice started.
    body_heard: bool,
    /// Which steps this voice has (`Step::available`; tests set their own).
    available: fn(Step) -> bool,
    /// The move asked of the body for a step just done, until taken.
    gesture: Option<&'static str>,
    /// [`NUDGE_ENVIRONMENT`].
    nudge: bool,
    /// Whether the voice can save a face (`remember_my_face`, the eyes' who
    /// socket set): "who you are" then asks for the face, and is done when it
    /// is saved, not on a name and a yes (#3883's follow-up).
    face_kept: bool,
    /// Whether no hand was on the lamp at the body's last reading.
    released: bool,
    /// Since when a face at least [`NEAR`] has been seen without a break.
    near_since: Option<u64>,
    /// Controls' touches (`controls/touch`), with the bus on: without them
    /// "touch" is not available.
    touches: Option<lamp_bus::Subscriber>,
    privacy_flow: PrivacyFlow,
    near: near::NearFlow,
    giving: giving_way::GivingWayFlow,
    /// Unix milliseconds, for evidence dated by the bus (tests set their own).
    unix_clock: fn() -> u64,
    /// A lamp with no privacy slide (`LAMP_PRIVACY_CONTROLS=no-slide`):
    /// only then is the room sounds step offered.
    no_slide: bool,
    /// [`FACE_EXPRESSIONS_ASK_ENVIRONMENT`] on: only then is the face
    /// expressions step offered.
    face_ask: bool,
}

/// What a read of a body topic found: nothing ever, a message
/// fresh within [`FRESH_MS`] of `now_ms`, or anything else (stale, expired,
/// or a read that failed), which is a body that is not saying so now.
fn heard(
    read: std::io::Result<Option<lamp_bus::Message>>,
    now_ms: u64,
) -> Heard<lamp_bus::Message> {
    match read {
        Ok(None) => Heard::Never,
        Ok(Some(message))
            if !message.expired && now_ms.saturating_sub(message.header.at_unix_ms) <= FRESH_MS =>
        {
            Heard::Fresh(message)
        }
        _ => Heard::Stale,
    }
}

/// What the body last said on a topic.
enum Heard<T> {
    Never,
    Stale,
    Fresh(T),
}

impl Heard<lamp_bus::Message> {
    /// A fresh word that does not parse is not evidence of anything.
    fn parsed<T: serde::de::DeserializeOwned>(self) -> Heard<T> {
        match self {
            Heard::Never => Heard::Never,
            Heard::Stale => Heard::Stale,
            Heard::Fresh(message) => message.parse().map_or(Heard::Stale, Heard::Fresh),
        }
    }
}

/// The onboarding file, when the switch is on and the path is absolute.
pub fn switched_on(switch: Option<&str>, file: Option<&str>) -> Option<PathBuf> {
    (switch == Some("on"))
        .then_some(file?)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.file_name().is_some())
}

/// A lamp with no privacy slide: `LAMP_PRIVACY_CONTROLS` exactly `no-slide`.
pub fn no_slide(privacy_controls: Option<&str>) -> bool {
    privacy_controls == Some(NO_SLIDE)
}

pub fn mailbox_of(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".told");
    PathBuf::from(name)
}

impl Onboarding {
    /// The service owns the sole I/O executor across all voice cycles.
    pub(crate) fn from_storage(
        client: storage::Client,
        definition: Arc<skill::Definition>,
    ) -> Option<Self> {
        let file = switched_on(
            std::env::var(ENVIRONMENT).ok().as_deref(),
            std::env::var(FILE_ENVIRONMENT).ok().as_deref(),
        )?;
        let mut walk = Self::with_definition(file, client, definition);
        walk.eyes = lamp_bus::Bus::from_environment().ok();
        walk.nudge = std::env::var(NUDGE_ENVIRONMENT).as_deref() == Ok("on");
        walk.face_kept =
            std::env::var(crate::known_faces::ENVIRONMENT).is_ok_and(|socket| !socket.is_empty());
        // The head pads' touches, from `--touch` Controls, only with the
        // voice's bus on.
        if std::env::var(crate::bus_out::ENVIRONMENT).ok().as_deref() == Some("on") {
            walk.touches = walk.eyes.as_ref().and_then(|bus| {
                bus.subscribe_typed(
                    lamp_msgs::TouchEvent::TOPIC,
                    SUBSCRIBER,
                    lamp_msgs::TouchEvent::TYPE,
                )
                .ok()
            });
        }
        walk.no_slide = no_slide(
            std::env::var(crate::privacy_bus::ENVIRONMENT)
                .ok()
                .as_deref(),
        );
        walk.face_ask = std::env::var(FACE_EXPRESSIONS_ASK_ENVIRONMENT).as_deref() == Ok("on");
        Some(walk)
    }

    #[cfg(test)]
    pub(crate) fn with_storage(file: PathBuf, storage: storage::Client) -> Self {
        Self::with_definition(file, storage, skill::fixture())
    }

    pub(crate) fn with_definition(
        file: PathBuf,
        storage: storage::Client,
        definition: Arc<skill::Definition>,
    ) -> Self {
        Self {
            file,
            definition,
            desired: Progress::default(),
            committed: Progress::default(),
            storage,
            initialized: false,
            storage_epoch: 0,
            storage_busy: true,
            storage_failed: false,
            dirty: false,
            erase_on_commit: false,
            flight: None,
            barrier: None,
            mailbox_revision: None,
            checkpoint_bytes: None,
            pending_records: Vec::new(),
            #[cfg(test)]
            fixture_storage: None,
            led: None,
            owed: None,
            seen_us: None,
            read_us: None,
            finished_recorded: false,
            records: Vec::new(),
            eyes: None,
            available: Step::available,
            gesture: None,
            privacy_flow: PrivacyFlow::default(),
            near: near::NearFlow::default(),
            giving: giving_way::GivingWayFlow::default(),
            unix_clock: unix_ms,
            no_slide: false,
            face_ask: false,
            looked: None,
            felt: None,
            goals: None,
            body_heard: false,
            nudge: false,
            face_kept: false,
            released: true,
            near_since: None,
            touches: None,
        }
    }

    /// Whether Controls said a touch on the head pads since the last look.
    /// Never waits; what is read is only this, never kept.
    fn head_touched(&mut self) -> bool {
        let Some(touches) = self.touches.as_mut() else {
            return false;
        };
        let mut touched = false;
        for _ in 0..8 {
            let Ok(Some(message)) = touches.recv(std::time::Duration::ZERO) else {
                break;
            };
            touched |= !message.expired
                && message.header.source == "controls"
                && message.parse::<lamp_msgs::TouchEvent>().is_ok_and(|event| {
                    event.check().is_ok()
                        && event.enabled
                        && event.source == lamp_msgs::TouchSource::Head
                });
        }
        touched
    }

    /// Fixture construction waits outside the production input loop.
    #[cfg(test)]
    fn at(file: PathBuf) -> Self {
        let owner = storage::ServiceStorage::start(storage::Paths {
            progress: file.clone(),
            checkpoint: file.parent().unwrap().join("privacy-pending.json"),
            mailbox: mailbox_of(&file),
            near_call: file.parent().unwrap().join("near-call.json"),
            room_sounds: file.parent().unwrap().join("room-sounds.json"),
            room_sounds_group: None,
        })
        .unwrap();
        let mut walk = Self::with_storage(file, owner.client());
        walk.fixture_storage = Some(owner);
        walk.drain_storage_for_test(0);
        walk
    }

    fn storage_idle(&self) -> bool {
        self.initialized
            && !self.storage_failed
            && !self.storage_busy
            && !self.dirty
            && self.flight.is_none()
            && self.barrier.is_none()
    }

    fn storage_failed(&mut self, error: impl std::fmt::Display) {
        if !self.storage_failed {
            self.records
                .push(json!({"kind":"onboarding_storage_failed","error":error.to_string()}));
        }
        self.storage_failed = true;
        self.privacy_flow.offer = None;
        self.privacy_flow.attempt = None;
        self.privacy_flow.pending_result = None;
        self.forget_near();
        self.forget_giving();
        self.owed = None;
        self.gesture = None;
    }

    /// One bounded completion snapshot; all filesystem work belongs to storage.
    pub(crate) fn poll_storage(&mut self, now_us: u64) {
        let snapshot = match self.storage.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
            Err(error) => {
                self.storage_failed(error);
                return;
            }
        };
        if let Some(error) = snapshot.failed.as_ref() {
            self.storage_failed(error);
            self.storage_busy = snapshot.busy;
            if let Some((revision, lines)) = snapshot.mailbox {
                if self.mailbox_revision != Some((snapshot.epoch, revision)) {
                    self.mailbox_revision = Some((snapshot.epoch, revision));
                    self.apply_mailbox(&lines, now_us);
                }
                // A contended acknowledgement retries next poll without
                // applying the already-consumed batch a second time.
                if let Err(error) = self.storage.ack_mailbox(revision)
                    && error.kind() != io::ErrorKind::WouldBlock
                {
                    self.storage_failed(error);
                }
            }
            // Failed storage never retries a write on its own. Its bounded
            // mailbox read remains available so an explicit redo can fence it.
            if self.storage_failed
                && !snapshot.busy
                && self
                    .read_us
                    .is_none_or(|at| now_us.saturating_sub(at) >= READ_EVERY_US)
                && let Ok(storage::Submit::Queued(_)) = self.storage.read_mailbox()
            {
                self.read_us = Some(now_us);
                self.storage_busy = true;
            }
            return;
        }
        self.storage_busy = snapshot.busy;
        if self.initialized && snapshot.epoch != self.storage_epoch {
            // Fence visibility changes immediately, even while its disk barrier
            // waits behind an older write. It cannot leave old proof usable.
            self.storage_epoch = snapshot.epoch;
            self.initialized = false;
            self.flight = None;
            self.barrier = None;
            self.dirty = false;
            self.erase_on_commit = false;
            self.pending_records.clear();
            self.owed = None;
            self.led = None;
            self.privacy_flow.offer = None;
            self.privacy_flow.attempt = None;
            self.privacy_flow.pending_result = None;
            self.privacy_flow.loaded = false;
            self.forget_near();
            self.forget_giving();
            self.checkpoint_bytes = None;
        }
        if !snapshot.ready {
            return;
        }
        if !self.initialized {
            let Some(progress) = snapshot.progress.clone() else {
                return;
            };
            self.desired = progress.clone();
            self.committed = progress;
            self.storage_epoch = snapshot.epoch;
            self.initialized = true;
        }
        let completed_barrier = self
            .barrier
            .is_some_and(|tag| tag.epoch == snapshot.epoch && tag.revision <= snapshot.revision);
        let completed = self
            .flight
            .is_some_and(|tag| tag.epoch == snapshot.epoch && tag.revision <= snapshot.revision)
            || self.barrier.is_some_and(|tag| {
                tag.epoch == snapshot.epoch && tag.revision <= snapshot.revision
            });
        if completed {
            if let Some(progress) = snapshot.progress.clone() {
                self.committed = progress;
            }
            self.flight = None;
            self.barrier = None;
            if completed_barrier {
                self.privacy_flow.erase_pending = false;
                self.privacy_flow.attempt = None;
                self.privacy_flow.loaded = true;
            }
            // The latest desired snapshot wins over any older completed one.
            self.dirty = self.desired != self.committed;
            if !self.dirty {
                self.records.append(&mut self.pending_records);
                self.erase_on_commit = false;
            }
        }
        self.near_stored(snapshot.epoch, snapshot.revision, now_us);
        self.checkpoint_bytes = snapshot.checkpoint;
        if let Some((revision, lines)) = snapshot.mailbox {
            if self.mailbox_revision != Some((snapshot.epoch, revision)) {
                self.mailbox_revision = Some((snapshot.epoch, revision));
                self.apply_mailbox(&lines, now_us);
            }
            if let Err(error) = self.storage.ack_mailbox(revision)
                && error.kind() != io::ErrorKind::WouldBlock
            {
                self.storage_failed(error);
            }
        }
        if self.storage_failed || self.storage_busy || self.barrier.is_some() {
            return;
        }
        if self.dirty && self.flight.is_none() {
            match self
                .storage
                .commit(self.desired.clone(), self.erase_on_commit)
            {
                Ok(storage::Submit::Queued(tag)) => {
                    self.flight = Some(tag);
                    self.storage_busy = true;
                }
                Ok(storage::Submit::Busy) => self.storage_busy = true,
                Ok(storage::Submit::Closed) => self.storage_failed("storage is closed"),
                Err(error) => self.storage_failed(error),
            }
        } else if self.flight.is_none()
            && self
                .read_us
                .is_none_or(|at| now_us.saturating_sub(at) >= READ_EVERY_US)
        {
            match self.storage.read_mailbox() {
                Ok(storage::Submit::Queued(_)) => {
                    self.read_us = Some(now_us);
                    self.storage_busy = true;
                }
                Ok(storage::Submit::Busy) => self.storage_busy = true,
                Ok(storage::Submit::Closed) => self.storage_failed("storage is closed"),
                Err(error) => self.storage_failed(error),
            }
        }
    }

    fn fence_storage(&mut self) {
        if self.storage_failed {
            return;
        }
        match self.storage.fence(Some(self.desired.clone())) {
            Ok(tag) => {
                self.storage_epoch = tag.epoch;
                self.barrier = Some(tag);
                self.flight = None;
                self.storage_busy = true;
                self.dirty = true;
                self.checkpoint_bytes = None;
            }
            Err(error) => self.storage_failed(error),
        }
    }

    #[cfg(test)]
    pub(crate) fn drain_storage_for_test(&mut self, now_us: u64) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            self.poll_storage(now_us);
            assert!(!self.storage_failed, "fixture storage failed");
            if self.storage_idle() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "fixture storage did not settle"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// Memory-only context. The profile's location is trusted input, never
    /// inferred from a model or from an unverified observation. Without a
    /// receipt source (`LAMP_PRIVACY_CONTROLS=on`; the GPIO worker sends no
    /// receipt) no offer can ever be made, so the step is not available.
    pub(crate) fn set_privacy_context(
        &mut self,
        epoch: privacy::Epoch,
        receipt_source: bool,
        receipt: Option<PrivacyReceipt>,
        verified_location: Option<&str>,
    ) {
        let location = verified_location
            .filter(|_| receipt_source)
            .filter(|value| {
                !value.trim().is_empty()
                    && value.chars().count() <= 160
                    && !value.chars().any(char::is_control)
            })
            .map(str::to_owned);
        let source_changed = self.privacy_flow.attempt.as_ref().is_some_and(|attempt| {
            let prior = attempt.last_acquisition();
            receipt.as_ref().is_some_and(|receipt| {
                receipt.controls_session != prior.controls_session
                    || receipt.physical.reader_session_id != prior.physical.reader_session_id
                    || receipt.physical.stamp.clock_id != prior.physical.stamp.clock_id
            })
        });
        if source_changed
            || self
                .privacy_flow
                .context
                .as_ref()
                .is_some_and(|old| old.epoch != epoch)
        {
            self.invalidate_privacy(privacy::Invalidation::SourceRestart);
        }
        self.privacy_flow.context = Some(PrivacyContext {
            epoch,
            receipt,
            location,
        });
    }

    fn privacy_path(&self) -> PathBuf {
        self.file
            .parent()
            .expect("absolute onboarding file")
            .join("privacy-pending.json")
    }

    fn privacy_available(&self) -> bool {
        !self.privacy_flow.waited_out
            && self
                .privacy_flow
                .context
                .as_ref()
                .is_some_and(|context| context.location.is_some())
    }

    /// Whether this voice has the step now: privacy only with a verified
    /// location and receipts, how they sound only with the near-call path,
    /// touch only with Controls' touches to read.
    fn step_available(&self, step: Step) -> bool {
        match step {
            Step::Privacy => self.privacy_available(),
            Step::HowYouSound => self.near_available(),
            Step::RoomSounds => self.no_slide,
            Step::FaceExpressions => self.face_ask,
            Step::GivingWay => self.giving_way_available(),
            Step::Touch => (self.available)(step) && self.touches.is_some(),
            _ => (self.available)(step),
        }
    }

    fn privacy_open_receipt(&self, now_us: u64) -> Option<privacy::Acquisition> {
        let context = self.privacy_flow.context.as_ref()?;
        let receipt = context.receipt.as_ref()?;
        (receipt.physical.muted == Some(false)
            && receipt.physical.fault.is_none()
            && receipt.physical.fresh_at(&context.epoch.clock_id, now_us))
        .then(|| privacy::Acquisition {
            controls_session: receipt.controls_session.clone(),
            physical: receipt.physical.clone(),
        })
    }

    pub(crate) fn privacy_pending(&self) -> lamp_msgs::OnboardingPending {
        let invitations = self
            .privacy_flow
            .offer
            .as_ref()
            .map(|offer| lamp_msgs::OnboardingInvitation {
                invitation: offer.token,
                generation: self.privacy_flow.generation,
                attempt: offer.attempt,
                step: lamp_msgs::OnboardingStep::Privacy,
                ready: offer.ready && offer.persisted && !offer.claimed && self.storage_idle(),
            })
            .into_iter()
            .collect();
        lamp_msgs::OnboardingPending { invitations }
    }

    /// Only the core skill's exact current eligible token obtains words. The
    /// coordinator still checks its normal readiness before calling this.
    pub(crate) fn take_privacy_invitation(&mut self, token: u64, now_us: u64) -> Option<String> {
        if !self.storage_idle() {
            return None;
        }
        let baseline = self.privacy_open_receipt(now_us)?;
        if !privacy::armable(
            &self.privacy_flow.context.as_ref()?.epoch,
            &baseline,
            now_us,
        ) {
            return None;
        }
        if self
            .seen_us
            .is_none_or(|seen| now_us.saturating_sub(seen) > FRESH_MS * 1_000)
        {
            return None;
        }
        let location = self.privacy_flow.context.as_ref()?.location.as_deref()?;
        let offer = self.privacy_flow.offer.as_mut()?;
        if offer.token != token || !offer.ready || !offer.persisted || offer.claimed {
            return None;
        }
        offer.claimed = true;
        offer.ready = false;
        offer.claimed_us = now_us;
        Some(self.definition.privacy_invitation(
            self.desired.steps.contains_key(Step::Privacy.name()),
            location,
        ))
    }

    /// Arms the attempt as the invitation's turn begins, not when it ends:
    /// a close while the lamp is still saying it counts, and the mute stops
    /// the words as any physical mute does (the lead's 52e6 run, 21 October).
    pub(crate) fn bind_privacy_invitation(
        &mut self,
        token: u64,
        voice_boot: BootId,
        turn: u64,
    ) -> bool {
        let Some(offer) = self.privacy_flow.offer.as_ref() else {
            return false;
        };
        if offer.token != token || !offer.claimed || offer.bound.is_some() || turn == 0 {
            return false;
        }
        let began_us = offer.claimed_us;
        let armed = self.privacy_flow.context.as_ref().and_then(|context| {
            self.privacy_open_receipt(began_us).map(|baseline| {
                privacy::Attempt::arm(
                    context.epoch.clone(),
                    privacy::Invitation {
                        voice_boot,
                        turn,
                        delivered_at_us: began_us,
                    },
                    baseline,
                    began_us,
                )
            })
        });
        let Some(Ok(attempt)) = armed else {
            // The words are being said already: spending a try here would
            // leave a close during them with nothing to answer. The same try
            // is offered again once the switch reads open and it is quiet;
            // the offer's give-up still bounds a failure that keeps coming.
            if let Some(offer) = self.privacy_flow.offer.as_mut() {
                offer.claimed = false;
                offer.ready = false;
            }
            self.records
                .push(json!({"kind":"onboarding_privacy_arm_failed","turn":turn,"at_us":began_us}));
            return false;
        };
        self.privacy_flow.attempt = Some(attempt);
        self.privacy_flow.camera_token = fresh_token().ok();
        self.look_for_receipt_eyes();
        if let Some(offer) = self.privacy_flow.offer.as_mut() {
            offer.bound = Some((voice_boot, turn));
        }
        self.records
            .push(json!({"kind":"onboarding_privacy_armed","turn":turn,"at_us":began_us}));
        true
    }

    pub(crate) fn privacy_invitation_binding(&self) -> Option<(BootId, u64)> {
        self.privacy_flow
            .offer
            .as_ref()
            .and_then(|offer| offer.bound)
    }

    pub(crate) fn privacy_invitation_refused(&mut self, token: u64, now_us: u64) {
        if self
            .privacy_flow
            .offer
            .as_ref()
            .is_none_or(|offer| offer.token != token || offer.bound.is_some())
        {
            return;
        }
        self.retry_privacy(now_us);
    }

    fn retry_privacy(&mut self, now_us: u64) {
        self.privacy_flow.offer = None;
        self.privacy_flow.failures = self.privacy_flow.failures.saturating_add(1);
        self.privacy_flow.retry_after_us = now_us.saturating_add(1_000_000);
        if self.privacy_flow.failures >= 2 {
            self.privacy_flow.pending_result = Some((Outcome::Skipped, None));
        }
    }

    /// Called on the existing attributable whole-turn playback result. The
    /// attempt was armed as the turn began; an invitation not heard whole
    /// (cut off, a playback gap, no audio) drops it and is offered again
    /// within the two-try budget. A physical mute while it is said never
    /// gets here: that cycle ends with the close as its candidate.
    pub(crate) fn privacy_invitation_finished(
        &mut self,
        voice_boot: BootId,
        turn: u64,
        delivery: Delivery,
        now_us: u64,
    ) {
        if self.privacy_invitation_binding() != Some((voice_boot, turn)) {
            return;
        }
        let attempt = self.privacy_flow.offer.take().map(|offer| offer.attempt);
        if delivery != Delivery::Whole {
            // Nothing of an armed attempt was persisted: dropping it is enough.
            if self
                .privacy_flow
                .attempt
                .as_ref()
                .is_some_and(|attempt| attempt.phase() == privacy::Phase::Armed)
            {
                self.privacy_flow.attempt = None;
                self.privacy_flow.camera_token = None;
            }
            if delivery == Delivery::Unsaid {
                self.privacy_unsaid(attempt, now_us);
            } else {
                self.retry_privacy(now_us);
            }
        }
    }

    /// Never said, never asked (#3754): offered again without spending a
    /// try, [`MOST_UNSAID`] times at most, then `not_available`.
    fn privacy_unsaid(&mut self, attempt: Option<u64>, now_us: u64) {
        if let Some(attempt) = attempt
            && u64::from(self.desired.privacy_invitation_tries) == attempt
        {
            self.desired.privacy_invitation_tries =
                u8::try_from(attempt.saturating_sub(1)).unwrap_or(0);
            self.save();
        }
        let flow = &mut self.privacy_flow;
        flow.offer = None;
        flow.unsaid = flow.unsaid.saturating_add(1);
        let again = flow.unsaid <= MOST_UNSAID;
        if again {
            flow.retry_after_us = now_us.saturating_add(1_000_000);
        }
        self.records.push(json!({"kind":"onboarding_privacy_try",
            "why":NOT_SAID,"offered_again":again,"at_us":now_us}));
        if !again {
            self.invalidate_privacy(privacy::Invalidation::FenceFailure);
            self.finish(Step::Privacy, Outcome::NotAvailable, None, now_us);
        }
    }

    /// Memory only, before revocation. The service persists this candidate only
    /// after the ordinary physical-mute teardown has verifiably succeeded.
    pub(crate) fn take_close_candidate(
        &mut self,
        receipt: &PrivacyReceipt,
        now_us: u64,
    ) -> Option<privacy::Checkpoint> {
        if self.privacy_flow.erase_pending {
            self.privacy_flow.attempt = None;
            return Some(privacy::Checkpoint::discard(self.privacy_path()));
        }
        let mut attempt = self.privacy_flow.attempt.take()?;
        if attempt.phase() != privacy::Phase::Armed {
            self.privacy_flow.attempt = Some(attempt);
            return None;
        }
        let acquisition = privacy::Acquisition {
            controls_session: receipt.controls_session.clone(),
            physical: receipt.physical.clone(),
        };
        let early = acquisition.physical.stamp.at_us <= attempt.delivered_at_us();
        match attempt.observe_close(acquisition, now_us) {
            Ok(()) => Some(privacy::Checkpoint::new(attempt, self.privacy_path())),
            // A slide read just before the invitation began is the person
            // acting early, not a failed check: offer it again.
            Err(_) if early => {
                self.privacy_flow.erase_pending = true;
                self.retry_privacy(now_us);
                Some(privacy::Checkpoint::discard(self.privacy_path()))
            }
            Err(_) => {
                self.privacy_flow.erase_pending = true;
                self.privacy_flow.pending_result = Some((Outcome::Failed, None));
                self.privacy_flow.attempt = Some(attempt);
                Some(privacy::Checkpoint::discard(self.privacy_path()))
            }
        }
    }

    /// While an attempt is armed, look whether eyes that give receipts are
    /// on the bus and keep the answer for the mute. Bus I/O: never called
    /// on the mute path.
    fn look_for_receipt_eyes(&mut self) {
        if self
            .privacy_flow
            .attempt
            .as_ref()
            .is_some_and(|attempt| attempt.phase() == privacy::Phase::Armed)
        {
            self.privacy_flow.eyes_give_receipts = eyes_give_receipts(self.eyes.as_ref());
        }
    }

    /// [`Onboarding::take_close_candidate`] for a cycle whose eyes have
    /// just been told no: when eyes that give receipts were on the bus at
    /// the last look, the close also waits for theirs, and they are asked
    /// for it (#1802). Memory and the one non-blocking send only: what the
    /// bus said and the token were read when the attempt was armed.
    ///
    /// Also hands back the walk's records, these camera ones among them:
    /// the mute ends the cycle at once, before the loop would take them,
    /// so the caller writes them to the session's trace (memory) itself.
    pub(crate) fn take_close_candidate_telling(
        &mut self,
        receipt: &PrivacyReceipt,
        now_us: u64,
        eyes: Option<&mut crate::body_link::EyesLink>,
    ) -> (Option<privacy::Checkpoint>, Vec<Value>) {
        let checkpoint = self.close_candidate_telling(receipt, now_us, eyes);
        (checkpoint, self.take_records())
    }

    fn close_candidate_telling(
        &mut self,
        receipt: &PrivacyReceipt,
        now_us: u64,
        eyes: Option<&mut crate::body_link::EyesLink>,
    ) -> Option<privacy::Checkpoint> {
        let checkpoint = self.take_close_candidate(receipt, now_us)?;
        let Some(eyes) = eyes else {
            return Some(checkpoint);
        };
        if checkpoint.is_discard() {
            return Some(checkpoint);
        }
        if !self.privacy_flow.eyes_give_receipts {
            self.records.push(
                json!({"kind":"onboarding_privacy_camera","state":"not_available","at_us":now_us}),
            );
            return Some(checkpoint);
        }
        let Some(token) = self.privacy_flow.camera_token.take() else {
            return Some(checkpoint);
        };
        let camera = privacy::CameraClose {
            token,
            asked_unix_ms: unix_ms(),
        };
        checkpoint.expect_camera(camera);
        if checkpoint.camera() == Some(camera) {
            eyes.ask_close_receipt(token);
            self.records
                .push(json!({"kind":"onboarding_privacy_camera","state":"asked","at_us":now_us}));
        }
        Some(checkpoint)
    }

    /// No I/O here: close, software mute and input faults must revoke first.
    pub(crate) fn invalidate_privacy(&mut self, why: privacy::Invalidation) {
        let work = self.privacy_flow.attempt.is_some()
            || self.privacy_flow.offer.is_some()
            || self.privacy_flow.pending_result.is_some()
            || self.checkpoint_bytes.is_some()
            || self.desired.privacy_done_attempt != self.committed.privacy_done_attempt;
        if let Some(attempt) = self.privacy_flow.attempt.as_mut() {
            attempt.invalidate(why);
        }
        self.privacy_flow.offer = None;
        self.privacy_flow.attempt = None;
        self.privacy_flow.pending_result = None;
        self.privacy_flow.last_open = None;
        if !work {
            return;
        }
        // A delayed Done write may still reach disk; fence orders its durable
        // replacement and deletion before any new tutorial offer is possible.
        if self.desired.privacy_done_attempt != self.committed.privacy_done_attempt {
            match self.committed.steps.get("privacy").cloned() {
                Some(kept) => {
                    self.desired.steps.insert("privacy".into(), kept);
                }
                None => {
                    self.desired.steps.remove("privacy");
                }
            }
            self.desired.privacy_done_attempt = self.committed.privacy_done_attempt;
        }
        self.pending_records.retain(|row| row["step"] != "privacy");
        self.owed = None;
        self.privacy_flow.erase_pending = true;
        self.fence_storage();
    }

    /// Polling and evidence qualification never perform filesystem I/O.
    /// A completion says storage finished, not that a physical sample is fresh.
    pub(crate) fn maintain_privacy(&mut self, now_us: u64, normal_ready: bool) -> io::Result<()> {
        self.poll_storage(now_us);
        if !normal_ready || !self.storage_idle() {
            return Ok(());
        }
        let Some(context) = self.privacy_flow.context.as_ref() else {
            return Ok(());
        };
        if !self.privacy_flow.loaded {
            self.privacy_flow.loaded = true;
            if let Some(bytes) = self.checkpoint_bytes.as_ref() {
                match privacy::Attempt::from_bytes(bytes, &context.epoch, now_us) {
                    Ok(attempt) if attempt.phase() == privacy::Phase::Closed => {
                        if self.committed.privacy_done_attempt == Some(attempt.id()) {
                            self.privacy_flow.erase_pending = true;
                        }
                        self.privacy_flow.attempt = Some(attempt);
                    }
                    Ok(attempt) => {
                        self.privacy_flow.attempt = Some(attempt);
                        self.privacy_flow.erase_pending = true;
                    }
                    Err(error) => {
                        self.records.push(json!({"kind":"onboarding_privacy_recovery_refused","error":error.to_string(),"at_us":now_us}));
                        self.privacy_flow.erase_pending = true;
                    }
                }
            }
            // Even a RAM snapshot load is not a physical observation. Recheck
            // the current receipt and ordinary readiness on the next pass.
            return Ok(());
        }
        if self.privacy_flow.erase_pending {
            self.privacy_flow.attempt = None;
            self.privacy_flow.erase_pending = false;
            self.fence_storage();
            return Ok(());
        }
        if let Some(offer) = self
            .privacy_flow
            .offer
            .as_mut()
            .filter(|offer| !offer.persisted)
        {
            if u64::from(self.committed.privacy_invitation_tries) >= offer.attempt {
                offer.persisted = true;
            } else {
                self.desired.privacy_invitation_tries =
                    u8::try_from(offer.attempt).unwrap_or(u8::MAX);
                self.save();
                self.poll_storage(now_us);
                return Ok(());
            }
        }
        if let Some(receipt) = self.privacy_open_receipt(now_us) {
            let key = (
                receipt.controls_session.clone(),
                receipt.physical.reader_session_id.clone(),
                receipt.physical.stamp.sequence,
            );
            if self.privacy_flow.last_open.as_ref() != Some(&key)
                && let Some(attempt) = self.privacy_flow.attempt.as_mut()
                && attempt.phase() == privacy::Phase::Closed
            {
                self.privacy_flow.last_open = Some(key);
                match attempt.observe_open(receipt, now_us, true) {
                    Ok(privacy::OpenOutcome::Done) => {
                        let camera = attempt
                            .camera()
                            .map(|close| camera_receipt(self.eyes.as_ref(), close));
                        if let Some(proof) = camera {
                            self.records.push(json!({"kind":"onboarding_privacy_camera",
                                "state": if proof.is_ok() { "proven" } else { "not_proven" },
                                "why": proof.err(),"at_us":now_us}));
                        }
                        self.privacy_flow.camera_proven = camera == Some(Ok(()));
                        self.privacy_flow.pending_result = Some(match camera {
                            // The camera was to be proven and was not: the
                            // check failed, whatever the microphones did.
                            Some(Err(_)) => (Outcome::Failed, None),
                            _ => (Outcome::Done, Some(attempt.id())),
                        })
                    }
                    Ok(privacy::OpenOutcome::WaitingForReadiness) => {}
                    Ok(privacy::OpenOutcome::Expired) | Err(_) => {
                        self.privacy_flow.pending_result = Some((Outcome::Failed, None))
                    }
                }
            }
        }
        if let Some((outcome, attempt_id)) = self.privacy_flow.pending_result.take() {
            self.desired.steps.insert(
                Step::Privacy.name().into(),
                Kept {
                    outcome,
                    at_unix_s: unix_ms() / 1000,
                    framing: None,
                    offered_at_unix_s: None,
                },
            );
            self.desired.privacy_done_attempt = attempt_id;
            self.privacy_flow.offer = None;
            self.privacy_flow.attempt = None;
            self.led = None;
            self.pending_records.push(
                json!({"kind":"onboarding_step","step":"privacy","outcome":outcome,"at_us":now_us}),
            );
            let camera = std::mem::take(&mut self.privacy_flow.camera_proven);
            self.owed = Some(
                match outcome {
                    Outcome::Done if camera => self.definition.words.privacy_done_camera.as_str(),
                    Outcome::Done => self.definition.words.privacy_done.as_str(),
                    _ => self.definition.words.privacy_failed.as_str(),
                }
                .to_owned(),
            );
            self.erase_on_commit = true;
            self.save();
            self.poll_storage(now_us);
        }
        Ok(())
    }

    fn privacy_tick(&mut self, now_us: u64, quiet_us: u64, can_speak: bool, person_present: bool) {
        if self.privacy_flow.maintenance_failed
            || !self.privacy_flow.loaded
            || self.privacy_flow.pending_result.is_some()
            || self.privacy_flow.attempt.is_some()
        {
            return;
        }
        if self.privacy_flow.generation == 0 {
            let Ok(generation) = fresh_token() else {
                return;
            };
            self.privacy_flow.generation = generation;
        }
        let ready = can_speak
            && person_present
            && quiet_us >= START_QUIET_US
            && self.privacy_open_receipt(now_us).is_some();
        if let Some(offer) = self.privacy_flow.offer.as_mut() {
            offer.ready = ready && !offer.claimed;
            if offer.bound.is_none()
                && now_us.saturating_sub(offer.offered_us) >= STEP_GIVE_UP_US
                && quiet_us >= TIMED_OUT_QUIET_US
            {
                self.retry_privacy(now_us);
            }
            return;
        }
        if self.desired.privacy_invitation_tries >= 2 {
            self.privacy_flow.pending_result = Some((Outcome::Skipped, None));
            return;
        }
        if !ready || now_us < self.privacy_flow.retry_after_us {
            return;
        }
        let Ok(token) = fresh_token() else {
            return;
        };
        self.privacy_flow.offered = u64::from(self.desired.privacy_invitation_tries) + 1;
        self.privacy_flow.offer = Some(PrivacyOffer {
            token,
            attempt: self.privacy_flow.offered,
            ready: true,
            claimed: false,
            persisted: false,
            offered_us: now_us,
            claimed_us: now_us,
            bound: None,
        });
        if let Some(led) = self.led.as_mut() {
            led.asked_us = Some(now_us);
        }
        self.records.push(
            json!({"kind":"onboarding_step","step":"privacy","state":"offered","at_us":now_us}),
        );
    }

    /// The one pending invitation, of whichever measured step is led.
    pub(crate) fn pending(&self) -> lamp_msgs::OnboardingPending {
        match self.near_pending().or_else(|| self.giving_pending()) {
            Some(offer) => lamp_msgs::OnboardingPending {
                invitations: vec![offer],
            },
            None => self.privacy_pending(),
        }
    }

    pub(crate) fn take_invitation(&mut self, token: u64, now_us: u64) -> Option<String> {
        if self.near_pending().is_some() {
            self.take_near_invitation(token, now_us)
        } else if self.giving_pending().is_some() {
            self.take_giving_invitation(token, now_us)
        } else {
            self.take_privacy_invitation(token, now_us)
        }
    }

    pub(crate) fn bind_invitation(&mut self, token: u64, voice_boot: BootId, turn: u64) -> bool {
        self.bind_near_invitation(token, voice_boot, turn)
            || self.bind_giving_invitation(token, voice_boot, turn)
            || self.bind_privacy_invitation(token, voice_boot, turn)
    }

    pub(crate) fn invitation_binding(&self) -> Option<(BootId, u64)> {
        self.near_binding()
            .or_else(|| self.giving_binding())
            .or_else(|| self.privacy_invitation_binding())
    }

    pub(crate) fn invitation_refused(&mut self, token: u64, now_us: u64) {
        self.near_invitation_refused(token, now_us);
        self.giving_invitation_refused(token, now_us);
        self.privacy_invitation_refused(token, now_us);
    }

    pub(crate) fn invitation_finished(
        &mut self,
        voice_boot: BootId,
        turn: u64,
        delivery: Delivery,
        now_us: u64,
    ) {
        self.near_invitation_finished(voice_boot, turn, delivery, now_us);
        self.giving_invitation_finished(voice_boot, turn, delivery, now_us);
        self.privacy_invitation_finished(voice_boot, turn, delivery, now_us);
    }

    pub fn kept(&self) -> &Progress {
        &self.committed
    }

    /// Whether the lamp is in the middle of leading a step.
    pub fn leading(&self) -> bool {
        self.owed.is_some() || self.led.as_ref().is_some_and(|led| led.asked_us.is_some())
    }

    /// Whether the walk is under way (#3747, the lead on #1711: no sleep
    /// from the head then): from its start, waiting for its first ask
    /// included, until every step in the skill's order has its outcome,
    /// the gaps between steps among it, not only while a step is led; and
    /// while a step offered again after the walk is led. Read from the
    /// runner's own progress, so a step asked and then stopped (nobody
    /// there) and a restart mid-walk still count; `None` until that
    /// progress is read, for the caller to keep what it last knew. Not a
    /// walk whose storage failed: it leads nothing.
    pub fn under_way(&self) -> Option<bool> {
        if self.storage_failed {
            return Some(false);
        }
        self.initialized.then(|| {
            self.leading()
                || self
                    .definition
                    .order
                    .iter()
                    .any(|step| !self.desired.steps.contains_key(step.name()))
        })
    }

    /// "Where you sit" is being led: the lamp looks at the person (the
    /// coordinator tells the body it listens, so its gaze turns the head to
    /// the face in small corrections) before it asks to be moved.
    pub fn looking(&self) -> bool {
        self.led
            .as_ref()
            .is_some_and(|led| led.step == Step::WhereYouSit && led.asked_us.is_some())
    }

    /// A finished line the person said (#1722 row 10): the steps the model
    /// leads by talk end on what is heard too, so a model that forgets its
    /// `setup_step` call does not fail them; the call stays a second way
    /// in. "Who you are": a name, then a yes. "Preferences": the quiet
    /// hours and the hello, each answered. The words are not kept. With the
    /// nudge on, "who you are" is [`Self::heard_line`]'s alone: its
    /// asked-back name and yes end the step, keeping the name, so the two
    /// never both end it (the first builder on #2526).
    pub fn heard(&mut self, text: &str, now_us: u64) {
        let nudge = self.nudge;
        let face_kept = self.face_kept;
        let Some(led) = self.led.as_mut().filter(|led| led.asked_us.is_some()) else {
            return;
        };
        let words = words_of(text);
        let done = match led.step {
            Step::WhoYouAre if nudge => false,
            Step::WhoYouAre => {
                // With a face to keep, the yes after a name is most often the
                // yes to "May I remember your face?": the step ends when the
                // face is saved (`face_saved`), or by the model's report, not
                // here. On lamp-29e7 (28 Oct, 8001) a name and a yes ended it
                // at 08:50:29 and no face was kept.
                if led.heard.name && agrees(&words) {
                    // Every yes restarts the wait, so it runs from the
                    // latest: the first is often the name's ("Marie,
                    // right?"), and the face question, its yes, the model's
                    // call and the save can all come after it (the
                    // Reviewer-lead's P2 on #3992).
                    if face_kept {
                        led.heard.face_answered_us = Some(now_us);
                    }
                    !face_kept
                } else if names_someone(text, &words) {
                    if !led.heard.name {
                        eprintln!(
                            "lamp-voice: onboarding: who_you_are heard a name; {}",
                            if face_kept {
                                "the face saved ends it"
                            } else {
                                "a yes ends it"
                            }
                        );
                    }
                    led.heard.name = true;
                    false
                } else if face_kept && led.heard.name && refuses(&words) {
                    // With a face to keep, a no after a name neither ends the
                    // step nor erases the name (the Reviewer-lead's P2 on
                    // #3992): a no to "Marie, right?" ended it before the
                    // face was asked, and a long face no ("No thanks, I'd
                    // rather not.") erased the name and the give-up asked it
                    // again. Like a yes, it restarts the face wait: a
                    // corrected name comes in through `names_someone` above,
                    // and the face's no ends it done, the name only, at
                    // `FACE_SAVE_WAIT_US` after quiet (`tick`).
                    eprintln!(
                        "lamp-voice: onboarding: who_you_are heard a no after the name: the face wait restarts"
                    );
                    led.heard.face_answered_us = Some(now_us);
                    false
                } else {
                    // "No" to the name the lamp checked: not known.
                    if refuses(&words) && led.heard.name {
                        eprintln!(
                            "lamp-voice: onboarding: who_you_are heard a no: the name is not known"
                        );
                        led.heard.name = false;
                    }
                    false
                }
            }
            Step::Preferences => {
                let heard = &mut led.heard;
                // A line about hello is their answer only with a yes or a no in
                // it ("Yes, you can say hello", "Please don't greet me"): a
                // greeting or an opener is someone saying hello, not their
                // answer on whether the lamp may. On lamp-52e6 on 27 Oct
                // "Hello, Lamp." ended the step, and the lamp asked again
                // (reviewer-three's P2 on #3673: "Hi, how are you?" too).
                let about_hello = words
                    .iter()
                    .any(|word| matches!(word.as_str(), "hello" | "hellos" | "hi" | "greet"))
                    && (hello_yes(&words) || refuses(&words));
                // Once the quiet hours are in, a yes or a no answers the hello
                // question: "That's fine." does, "Hi, I'm fine." doesn't, since
                // a line that greets takes only `hello_yes`'s yes.
                let greets = words.iter().any(|word| {
                    matches!(word.as_str(), "hello" | "hellos" | "hi" | "hey" | "greet")
                });
                let yes = if greets {
                    hello_yes(&words)
                } else {
                    agrees(&words)
                };
                if about_hello || (heard.quiet_hours && (yes || refuses(&words))) {
                    heard.hello = true;
                } else if quiet_hours(&words) {
                    heard.quiet_hours = true;
                }
                heard.quiet_hours && heard.hello
            }
            _ => false,
        };
        if done {
            let step = led.step;
            self.records
                .push(json!({"kind":"onboarding_heard","step":step.name(),
                    "name_kept":(step == Step::WhoYouAre).then_some(false),"at_us":now_us}));
            self.finish(step, Outcome::Done, None, now_us);
        }
    }

    /// What to record in the session, since last asked.
    pub fn take_records(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.records)
    }

    /// The near face on the eyes' bus topic now, for [`Self::tick`]: read
    /// at most every [`READ_EVERY_US`] / 2.
    pub fn face_now(&mut self, now_us: u64) -> Option<Framing> {
        match self.looked {
            Some((at, face)) if now_us.saturating_sub(at) < READ_EVERY_US / 2 => face,
            _ => {
                let face = self.look();
                self.looked = Some((now_us, face));
                face
            }
        }
    }

    /// Whether the head is still and no hand is on the lamp, from the
    /// body's topics (`body/joint_states`, `body/hand`), read as often as
    /// the face. No body on the bus: nothing moves the head, still.
    pub fn still_now(&mut self, now_us: u64) -> bool {
        if let Some((at, still)) = self.felt
            && now_us.saturating_sub(at) < READ_EVERY_US / 2
        {
            return still;
        }
        let read = |topic| {
            let read = self.eyes.as_ref().map_or(Ok(None), |bus| bus.latest(topic));
            heard(read, unix_ms())
        };
        let joints = read(lamp_msgs::JointState::TOPIC).parsed::<lamp_msgs::JointState>();
        let hand = read(lamp_msgs::Hand::TOPIC).parsed::<lamp_msgs::Hand>();
        // Any body topic or declaration on the bus is a body, whether or not
        // it has said these two. A bus that cannot be listed is not proof of
        // none.
        let body_on_bus = !self.body_heard
            && self.eyes.as_ref().is_some_and(|bus| {
                bus.topics().map_or(true, |topics| {
                    topics.iter().any(|topic| topic.name.starts_with("body/"))
                })
            });
        let still = self.judge(joints, hand, body_on_bus);
        self.felt = Some((now_us, still));
        still
    }

    /// Still only on positive, fresh evidence. Once there is any sign of a
    /// body (either topic present, fresh, stale, unreadable or not parsing,
    /// or any `body/` topic on the bus), it stays so: a reading that is not
    /// fresh is not still, and a hand not freshly released is still on it.
    /// Only a lamp with no body at all goes without that gate.
    fn judge(
        &mut self,
        joints: Heard<lamp_msgs::JointState>,
        hand: Heard<lamp_msgs::Hand>,
        body_on_bus: bool,
    ) -> bool {
        if body_on_bus || !matches!(joints, Heard::Never) || !matches!(hand, Heard::Never) {
            self.body_heard = true;
        }
        let head = match joints {
            Heard::Fresh(joints) => head_still(&joints, &mut self.goals),
            Heard::Never if !self.body_heard => true,
            _ => {
                self.goals = None;
                false
            }
        };
        let released = match hand {
            Heard::Fresh(hand) => !hand.pushed,
            Heard::Never => !self.body_heard,
            Heard::Stale => false,
        };
        self.released = released;
        head && released
    }

    fn look(&self) -> Option<Framing> {
        let message = self
            .eyes
            .as_ref()?
            .latest(lamp_msgs::Faces::TOPIC)
            .ok()
            .flatten()?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        if message.expired || now_ms.saturating_sub(message.header.at_unix_ms) > FRESH_MS {
            return None;
        }
        nearest(&message.parse::<lamp_msgs::Faces>().ok()?)
    }

    /// A line the person finished saying: with the nudge on, an answer to
    /// a step the model ends (the skill's nudge) being led.
    pub fn heard_line(&mut self, text: &str) {
        let Some(led) = self.led.as_mut().filter(|led| {
            self.nudge && led.asked_us.is_some() && self.definition.nudge(led.step).is_some()
        }) else {
            return;
        };
        if !text.chars().any(char::is_alphanumeric) {
            return;
        }
        led.answers = led.answers.saturating_add(1);
        // Only the first line after the lamp has asked its question.
        if led.step == Step::WhoYouAre && led.spoke && !led.name_heard {
            led.name_heard = true;
            led.name = name_in(text);
        }
        if led.confirm == Confirm::Asked(None) {
            led.confirm = Confirm::Asked(yes_or_no(text));
        }
    }

    /// The lamp's own words, as they come: after a step's ask, its
    /// question has been spoken.
    pub fn lamp_said(&mut self) {
        if let Some(led) = self.led.as_mut().filter(|led| led.asked_us.is_some()) {
            led.spoke = true;
        }
    }

    /// With the nudge on, once a step the model ends is over: whether its
    /// note was answered, nudged and reported, as counts, never words.
    fn nudge_record(&mut self, reported: bool, now_us: u64) {
        if let Some(led) = self.led.as_ref()
            && self.nudge
            && led.asked_us.is_some()
            && self.definition.nudge(led.step).is_some()
        {
            self.records
                .push(json!({"kind":"onboarding_nudge","step":led.step.name(),
                "noted":true,"answers":led.answers,"nudged":led.nudged_us.is_some(),"reported":reported,
                "name_asked_back":led.confirm != Confirm::NotAsked,
                "name_confirmed":led.confirm == Confirm::Asked(Some(true)),"at_us":now_us}));
        }
    }

    /// Call every loop. `quiet_us` is how long nothing has been said to the
    /// lamp or by it; `can_speak` that it may begin a turn of its own now;
    /// `face` the near face; `still` that the head is still and no hand is
    /// on the lamp ([`Self::still_now`]). A note returned is a turn the
    /// caller begins now as a written request.
    pub fn tick(
        &mut self,
        now_us: u64,
        quiet_us: u64,
        can_speak: bool,
        paused: bool,
        face: Option<Framing>,
        still: bool,
    ) -> Option<String> {
        self.poll_storage(now_us);
        self.look_for_receipt_eyes();
        // Read every tick, so what came before the ask is never taken. One
        // felt after "touch" was asked is kept with the step until it is
        // looked at: a tap is one event, read on one tick, and a tick may
        // return before the step is looked at (storage busy, a note owed),
        // which lost every tap on 3e83 and 29e7 (#3756).
        let touched = self.head_touched();
        if touched
            && let Some(led) = self
                .led
                .as_mut()
                .filter(|led| led.step == Step::Touch && led.asked_us.is_some())
        {
            led.touched = true;
        }
        if face.is_some() {
            self.seen_us = Some(now_us);
        }
        if face.is_some_and(|face| face.size >= NEAR) {
            self.near_since.get_or_insert(now_us);
        } else {
            self.near_since = None;
        }
        if paused {
            self.invalidate_privacy(privacy::Invalidation::FenceFailure);
            self.invalidate_near();
            // Giving way under the pause is not available: offered later.
            self.giving_way_paused(now_us);
            // The keeper's pause stops the walk; a step being led is asked
            // again afterwards.
            self.nudge_record(false, now_us);
            if self.led.take().is_some_and(|led| led.asked_us.is_some()) {
                self.records
                    .push(json!({"kind":"onboarding_stopped","why":"paused","at_us":now_us}));
            }
            return None;
        }
        if self.privacy_open_receipt(now_us).is_some() {
            self.privacy_flow.no_receipt_since = None;
            self.privacy_flow.waited_out = false;
        }
        // A try is judged on its own evidence, whatever storage is doing.
        self.near_judge(now_us);
        self.giving_judge(now_us);
        if !self.storage_idle() {
            return None;
        }
        if can_speak && let Some(note) = self.owed.take() {
            return Some(note);
        }
        let Some(step) = self.next_step() else {
            if !self.finished_recorded && !self.desired.steps.is_empty() {
                self.finished_recorded = true;
                self.records
                    .push(json!({"kind":"onboarding_finished","at_us":now_us}));
            }
            return None;
        };
        if !self.step_available(step) {
            self.finish(step, Outcome::NotAvailable, None, now_us);
            return None;
        }
        if self.led.as_ref().is_none_or(|led| led.step != step) {
            self.led = Some(Led {
                step,
                ..Led::default()
            });
        }
        if step == Step::Privacy {
            if self
                .seen_us
                .is_some_and(|seen| now_us.saturating_sub(seen) >= GONE_US)
            {
                self.invalidate_privacy(privacy::Invalidation::FenceFailure);
                self.led = None;
                return None;
            }
            // A receipt that never turns open and fresh (faulted, stale, or a
            // reader that stopped) would hold the walk here with nothing
            // said: past the sitting step's own give-up time the step is not
            // available, and the next open, fresh receipt offers it again.
            let unopened = self.privacy_open_receipt(now_us).is_none();
            let flow = &mut self.privacy_flow;
            if unopened
                && flow.offer.is_none()
                && flow.attempt.is_none()
                && flow.pending_result.is_none()
            {
                let since = *flow.no_receipt_since.get_or_insert(now_us);
                if now_us.saturating_sub(since) >= SIT_GIVE_UP_US {
                    flow.waited_out = true;
                    self.records.push(
                        json!({"kind":"onboarding_stopped","why":"no_open_privacy_receipt","step":"privacy","at_us":now_us}),
                    );
                    self.finish(step, Outcome::NotAvailable, None, now_us);
                    return None;
                }
            }
            self.privacy_tick(now_us, quiet_us, can_speak, face.is_some());
            return None;
        }
        if step == Step::HowYouSound {
            self.near_tick(now_us, quiet_us, can_speak, face.is_some());
            return None;
        }
        if step == Step::GivingWay {
            self.giving_tick(now_us, quiet_us, can_speak, face.is_some());
            return None;
        }
        let gone = self
            .seen_us
            .is_none_or(|seen| now_us.saturating_sub(seen) >= GONE_US);
        let led = self.led.as_mut()?;
        let Some(asked_us) = led.asked_us else {
            // With the nudge on, the walk's first ask waits for someone
            // near and staying ([`LINGER_US`]); before that, nothing is said.
            let there = if self.nudge && step == Step::WhereYouSit {
                self.near_since
                    .is_some_and(|since| now_us.saturating_sub(since) >= LINGER_US)
            } else {
                face.is_some()
            };
            if can_speak && there && quiet_us >= START_QUIET_US {
                led.asked_us = Some(now_us);
                led.guided_us = Some(now_us);
                let again = self.desired.steps.get_mut(step.name()).map(|kept| {
                    kept.offered_at_unix_s = Some(unix_ms() / 1_000);
                });
                self.records.push(
                    json!({"kind":"onboarding_step","step":step.name(),"state":"asked",
                        "offered_again":again.is_some(),"at_us":now_us}),
                );
                if again.is_some() {
                    self.save();
                    self.owed = Some(self.definition.note(step, true).to_owned());
                    return None;
                }
                return Some(self.definition.note(step, false).into());
            }
            return None;
        };
        if gone {
            self.nudge_record(false, now_us);
            self.led = None;
            self.records.push(
                json!({"kind":"onboarding_stopped","why":"nobody_there","step":step.name(),"at_us":now_us}),
            );
            return None;
        }
        if step == Step::Touch {
            // Done only by a touch Controls felt; one reminder, then failed.
            // Either end is said before the next step is asked (#3769).
            if touched || led.touched {
                self.finish(step, Outcome::Done, None, now_us);
                self.owed = self.definition.words.touch_done.clone();
            } else if now_us.saturating_sub(asked_us) >= TOUCH_GIVE_UP_US {
                self.finish(step, Outcome::Failed, None, now_us);
                self.owed = self.definition.words.touch_failed.clone();
            } else if can_speak && quiet_us >= GUIDE_EVERY_US && led.guided_us == Some(asked_us) {
                led.guided_us = Some(now_us);
                self.records
                    .push(json!({"kind":"onboarding_guide","step":step.name(),"at_us":now_us}));
                return Some(self.definition.words.touch_remind.as_str().into());
            }
            return None;
        }
        // A yes to the face with no face saved in [`FACE_SAVE_WAIT_US`] (a
        // refused save, the eyes away, or no call): done, the name only, not
        // left silent until the give-up (the Reviewer-lead's P2 on #3992).
        // Like the give-up it waits for quiet, but the short bar a step
        // starts on ([`START_QUIET_US`]): a lamp mid-sentence, or the save
        // still being talked through, is not cut off.
        if step == Step::WhoYouAre
            && quiet_us >= START_QUIET_US
            && led
                .heard
                .face_answered_us
                .is_some_and(|at| now_us.saturating_sub(at) >= FACE_SAVE_WAIT_US)
        {
            eprintln!(
                "lamp-voice: onboarding: who_you_are: no face saved after the yes: done, the name only"
            );
            self.nudge_record(false, now_us);
            self.records
                .push(json!({"kind":"onboarding_heard","step":step.name(),
                "name_kept":false,"at_us":now_us}));
            self.finish(step, Outcome::Done, None, now_us);
            return None;
        }
        if step != Step::WhereYouSit {
            if now_us.saturating_sub(asked_us) >= STEP_GIVE_UP_US && quiet_us >= TIMED_OUT_QUIET_US
            {
                // A name heard and never taken back is known, call or not.
                // The name's words are not kept: the record says so.
                let named = step == Step::WhoYouAre && led.heard.name;
                let asked_again = led.asked_again;
                self.nudge_record(false, now_us);
                if named {
                    self.records
                        .push(json!({"kind":"onboarding_heard","step":step.name(),
                        "name_kept":false,"at_us":now_us}));
                    self.finish(step, Outcome::Done, None, now_us);
                    return None;
                }
                // Timed out only after `TIMED_OUT_QUIET_US` of quiet, past
                // the lamp's own 20 s of conversation: nobody is in one with
                // it, so it is no answer of the person's (lamp-29e7, 27 Oct
                // 14:22:20, `preferences: failed` 100 s after the last line
                // answered, #3754). Asked again once someone is there, as a
                // dropped invitation is, [`MOST_UNSAID`] times; only then,
                // asked and given its time each time, it fails.
                let again = asked_again < MOST_UNSAID;
                self.records.push(json!({"kind":"onboarding_stopped",
                    "why":"no conversation","step":step.name(),
                    "asked_again":again,"at_us":now_us}));
                if again {
                    self.led = Some(Led {
                        step,
                        asked_again: asked_again + 1,
                        ..Led::default()
                    });
                } else {
                    self.finish(step, Outcome::Failed, None, now_us);
                }
                return None;
            }
            let (nudge, answers) = self.definition.nudge(step).filter(|_| self.nudge)?;
            // "Did I get that right?" answered yes: kept as the model's
            // done would be. No: asked once more, then as before.
            if led.confirm == Confirm::Asked(Some(true)) {
                self.nudge_record(false, now_us);
                self.finish(step, Outcome::Done, None, now_us);
                return None;
            }
            let at_rest = can_speak && quiet_us >= START_QUIET_US;
            if led.confirm == Confirm::Asked(Some(false)) && at_rest {
                led.confirm = Confirm::Over;
                return Some(self.definition.words.who_again.as_str().into());
            }
            // Answered and not reported: one note, at the next quiet moment.
            if led.answers >= answers && led.nudged_us.is_none() && at_rest {
                led.nudged_us = Some(now_us);
                return Some(nudge.into());
            }
            // Nudged and still not reported: the name heard, asked back,
            // and not while an answer on the face may still be coming.
            if led
                .nudged_us
                .is_some_and(|at| now_us.saturating_sub(at) >= ASK_BACK_AFTER_US)
                && led.confirm == Confirm::NotAsked
                && at_rest
                && let Some(name) = led.name.as_ref()
            {
                let note = self.definition.who_confirm(name);
                led.confirm = if note.is_some() {
                    Confirm::Asked(None)
                } else {
                    Confirm::Over
                };
                return note;
            }
            return None;
        }
        // Where you sit: what the eyes see ends it, and only while the head
        // is still and no hand is on the lamp: a lamp turning toward them,
        // or being turned, can pass through a good framing mid-move. Any
        // motion starts the three seconds again. After the one guide, any
        // steady face is taken where it is: a person beside the lamp would
        // just say "there's fine" (row 10 on lamp-52e6). With the nudge on,
        // after a guide, any face that stays where it is ([`Framing::steady`])
        // is taken there while no hand is on the lamp, still head or not: the
        // body's gaze kept nudging the head, so it was never still for 3 s
        // (row 10 try 4 on lamp-52e6).
        let guided = led.guided_us.is_some_and(|at| at > asked_us);
        led.looked_since = face.and(led.looked_since.or(Some(now_us)));
        let settled = self.nudge && guided;
        let released = self.released;
        let held = if settled {
            face.filter(|_| released)
        } else {
            face.filter(|face| still && (face.good() || guided))
        };
        match held {
            Some(framing) => {
                // A face that moved begins the hold again where it is now.
                if settled && led.held_at.is_some_and(|from| !framing.steady(&from)) {
                    led.held_since = None;
                }
                if led.held_since.is_none() {
                    led.held_at = Some(framing);
                }
                let since = *led.held_since.get_or_insert(now_us);
                if now_us.saturating_sub(since) >= HOLD_US {
                    self.finish(step, Outcome::Done, Some(framing), now_us);
                    self.owed = Some(self.definition.words.sit_done.clone());
                    return None;
                }
                // Settled, a face that never stays still fails at 60 s.
                if !settled {
                    return None;
                }
            }
            None => {
                led.held_since = None;
                led.held_at = None;
            }
        }
        if now_us.saturating_sub(asked_us) >= SIT_GIVE_UP_US {
            self.finish(step, Outcome::Failed, face, now_us);
            self.owed = Some(self.definition.words.sit_failed.clone());
            return None;
        }
        // A face off the middle is looked at first, as a creature turns to
        // someone (rule 8): the person is asked to turn the lamp only once
        // its own look has had time and still has them off the middle.
        let looked = led
            .looked_since
            .is_some_and(|since| now_us.saturating_sub(since) >= LOOK_FIRST_US);
        let off_middle = face.is_some_and(|face| face.size >= NEAR);
        if held.is_none()
            && can_speak
            && quiet_us >= GUIDE_EVERY_US
            && led.guided_us == Some(asked_us)
            && (!off_middle || looked)
        {
            led.guided_us = Some(now_us);
            let note = match face {
                None => self.definition.words.sit_no_face.as_str(),
                Some(face) if face.size < NEAR => self.definition.words.sit_closer.as_str(),
                Some(_) => self.definition.words.sit_turn.as_str(),
            };
            self.records.push(
                json!({"kind":"onboarding_guide","step":step.name(),"face":face,"at_us":now_us}),
            );
            return Some(note.into());
        }
        None
    }

    /// The first step with no outcome; once every step has one, a step that
    /// was not available then and is now, offered at the skill's interval
    /// (one day in the shipped data) and
    /// never before the walk is over (First's addition 2).
    fn next_step(&self) -> Option<Step> {
        let unix_s = unix_ms() / 1_000;
        if self.privacy_flow.context.is_some()
            && self.desired.steps.get("privacy").is_none_or(|kept| {
                kept.outcome == Outcome::NotAvailable && self.privacy_available()
            })
        {
            return Some(Step::Privacy);
        }
        self.definition
            .order
            .iter()
            .copied()
            .find(|step| !self.desired.steps.contains_key(step.name()))
            .or_else(|| {
                // One being offered again now is led to its end.
                self.led
                    .as_ref()
                    .filter(|led| led.asked_us.is_some())
                    .map(|led| led.step)
            })
            .or_else(|| {
                self.definition.order.iter().copied().find(|step| {
                    self.step_available(*step)
                        && self.desired.steps.get(step.name()).is_some_and(|kept| {
                            kept.outcome == Outcome::NotAvailable
                                && kept.offered_at_unix_s.is_none_or(|at| {
                                    unix_s.saturating_sub(at) >= self.definition.reoffer_after_s
                                })
                        })
                })
            })
    }

    /// The move to ask of the body for a step just done, once.
    pub fn take_gesture(&mut self) -> Option<&'static str> {
        if self.storage_idle() {
            self.gesture.take()
        } else {
            None
        }
    }

    fn finish(&mut self, step: Step, outcome: Outcome, framing: Option<Framing>, now_us: u64) {
        let at_unix_s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        self.desired.steps.insert(
            step.name().to_owned(),
            Kept {
                outcome,
                at_unix_s,
                framing,
                offered_at_unix_s: None,
            },
        );
        // The owner's #841 decision: giving way is the only onboarding
        // step that may move the body. Other success is spoken, never a nod.
        if outcome == Outcome::Done && step == Step::GivingWay {
            self.gesture = Some(step.gesture());
        }
        if self.led.as_ref().is_some_and(|led| led.step == step) {
            self.led = None;
        }
        self.pending_records.push(
            json!({"kind":"onboarding_step","step":step.name(),"outcome":outcome,
            "framing":framing,"at_us":now_us}),
        );
        self.save();
    }

    fn save(&mut self) {
        self.dirty = self.desired != self.committed || self.erase_on_commit;
    }

    /// What the model reported through the provider worker: a step's end,
    /// or that the walk starts again.
    fn apply_mailbox(&mut self, lines: &str, now_us: u64) {
        for line in lines.lines() {
            let Ok(told) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if told.get("redo").and_then(Value::as_bool) == Some(true) {
                self.storage_failed = false;
                self.invalidate_privacy(privacy::Invalidation::Redo);
                self.forget_near();
                self.near.generation = 0;
                self.redo_giving();
                self.privacy_flow.loaded = true;
                self.privacy_flow.maintenance_failed = false;
                self.privacy_flow.failures = 0;
                self.privacy_flow.unsaid = 0;
                self.privacy_flow.no_receipt_since = None;
                self.privacy_flow.waited_out = false;
                self.privacy_flow.generation = 0;
                self.desired = Progress::default();
                self.led = None;
                self.owed = None;
                self.finished_recorded = false;
                self.pending_records.clear();
                self.pending_records
                    .push(json!({"kind":"onboarding_redo","at_us":now_us}));
                self.save();
                self.fence_storage();
                continue;
            }
            let step = told
                .get("step")
                .and_then(Value::as_str)
                .and_then(Step::named);
            let outcome = told
                .get("outcome")
                .and_then(Value::as_str)
                .and_then(Outcome::named);
            let (Some(step), Some(outcome)) = (step, outcome) else {
                continue;
            };
            // Only the step being led, and a measured step only skipped:
            // "where you sit" is done by what the eyes see and "touch" by
            // what Controls feels, never by a word.
            let led = self
                .led
                .as_ref()
                .is_some_and(|led| led.step == step && led.asked_us.is_some());
            // A face `remember_my_face` saved (#3740) is no report of the
            // model's: its nudge record says so, and the walk's record names
            // the face save as what ended the step (the Reviewer-lead's P3s on
            // #3746).
            let by_face =
                told.get("by").and_then(Value::as_str) == Some(crate::known_faces::REMEMBER);
            if led && step.reportable(outcome) {
                self.nudge_record(!by_face, now_us);
                if by_face {
                    self.records.push(
                        json!({"kind":"onboarding_face_saved","step":step.name(),"at_us":now_us}),
                    );
                }
                if step == Step::Privacy {
                    self.invalidate_privacy(privacy::Invalidation::Redo);
                    self.privacy_flow.pending_result = Some((outcome, None));
                } else if step == Step::HowYouSound {
                    // A skip leaves any earlier bar as it is; late evidence
                    // and a late write of this walk are dropped.
                    self.invalidate_near();
                    self.finish(step, outcome, None, now_us);
                } else if step == Step::GivingWay {
                    // Late evidence of this walk's try is dropped.
                    self.forget_giving();
                    self.finish(step, outcome, None, now_us);
                } else {
                    self.finish(step, outcome, None, now_us);
                }
                // The step said is the step the record skipped (#3769).
                if outcome == Outcome::Skipped
                    && let Some(line) = self.definition.skipped(step)
                {
                    self.owed = Some(line.to_owned());
                }
            } else {
                let leading = self.led.as_ref().map(|led| {
                    let asked = if led.asked_us.is_some() {
                        ""
                    } else {
                        ", not asked yet"
                    };
                    format!("{}{asked}", led.step.name())
                });
                eprintln!(
                    "lamp-voice: onboarding: {} {:?} not taken: the step led is {}",
                    step.name(),
                    outcome,
                    leading.as_deref().unwrap_or("none")
                );
                self.records.push(
                    json!({"kind":"onboarding_report_refused","step":step.name(),
                    "outcome":outcome,"at_us":now_us}),
                );
            }
        }
    }
}

#[cfg(test)]
impl Drop for Onboarding {
    fn drop(&mut self) {
        if let Some(owner) = self.fixture_storage.as_mut() {
            let _ = owner.finish(std::time::Duration::from_secs(2));
        }
    }
}

/// The joints that move the camera, as the eyes' full look has them (#842).
const HEAD: [&str; 3] = ["base_yaw", "head_pitch", "head_roll"];
/// Six of a servo's 4096 counts a turn from where it was asked to be.
const ARRIVED_RADIANS: f64 = 6.0 * std::f64::consts::TAU / 4_096.0;
/// Slower than this is not turning (the full look's line).
const STILL_RADIANS_A_SECOND: f64 = 25.0 * std::f64::consts::TAU / 4_096.0;

const YES: [&str; 11] = [
    "yes", "yeah", "yep", "sure", "ok", "okay", "fine", "right", "correct", "please", "alright",
];
const NO: [&str; 6] = ["no", "nope", "not", "don't", "never", "none"];

/// Common ways of saying yes that hold a no word or none of [`YES`]: "No
/// problem", "I don't mind", "Of course", "Go ahead". On lamp-29e7 on 27 Oct
/// "who you are" took 5 minutes (#3883); a "no problem" there also cleared
/// the name heard.
const YES_PHRASES: [&[&str]; 5] = [
    &["no", "problem"],
    &["don't", "mind"],
    &["do", "not", "mind"],
    &["of", "course"],
    &["go", "ahead"],
];

/// Lines no longer than this are an answer as a whole: "Sure, go ahead."
const SHORT_ANSWER_WORDS: usize = 4;

/// A line's words with its [`YES_PHRASES`] taken out, and whether it had one.
/// A phrase counts only as an answer: leading the line, or in a short one. In
/// a statement ("I'll go ahead and sit down", "There is no problem with the
/// wifi") it is only words (Fourteen's P2 on #3893).
fn without_yes_phrases(words: &[String]) -> (Vec<&str>, bool) {
    let mut rest = Vec::with_capacity(words.len());
    let mut found = false;
    let mut at = 0;
    let short = words.len() <= SHORT_ANSWER_WORDS;
    'words: while at < words.len() {
        for phrase in YES_PHRASES.iter().filter(|_| short || at == 0) {
            let end = at + phrase.len();
            if end <= words.len()
                && words[at..end]
                    .iter()
                    .zip(phrase.iter())
                    .all(|(w, p)| w == p)
            {
                found = true;
                at = end;
                continue 'words;
            }
        }
        rest.push(words[at].as_str());
        at += 1;
    }
    (rest, found)
}

fn agrees(words: &[String]) -> bool {
    let (rest, phrase) = without_yes_phrases(words);
    (phrase || rest.iter().any(|word| YES.contains(word)))
        && !rest.iter().any(|word| NO.contains(word))
}

fn refuses(words: &[String]) -> bool {
    let (rest, _) = without_yes_phrases(words);
    rest.iter().any(|word| NO.contains(word))
}

/// Words that follow "I'm", "it's", "call me" or "my name is" without
/// being a name: "I'm fine", "it's okay", "I'm not sure" (the first
/// builder's P2 on #2526). The lamp's own name too.
/// ponytail: "lamp" stands for the lamp's name; read the configured name if
/// a lamp is ever called something else.
const NOT_A_NAME: [&str; 14] = [
    "fine", "good", "okay", "ok", "not", "sure", "here", "ready", "sorry", "just", "back", "busy",
    "tired", "lamp",
];
/// What a line of only a word or two is when it is not a name.
const NOT_A_NAME_ALONE: [&str; 8] = ["what", "huh", "hmm", "um", "uh", "hey", "hi", "hello"];

fn not_a_name(word: &str) -> bool {
    NOT_A_NAME.contains(&word) || YES.contains(&word) || NO.contains(&word)
}

/// "Call me Marie", "my name is Marie", "I'm Marie", or the name alone,
/// never a question.
/// ponytail: English only, as the notes are; a word list, not a parser.
fn names_someone(text: &str, words: &[String]) -> bool {
    let at = |i: usize| words.get(i).map(String::as_str);
    let after = (0..words.len()).find_map(|i| match (at(i), at(i + 1)) {
        (Some("call"), Some("me")) | (Some("name"), Some("is")) | (Some("i"), Some("am")) => {
            Some(i + 2)
        }
        (Some("i'm" | "it's"), _) => Some(i + 1),
        _ => None,
    });
    match after {
        Some(i) => at(i).is_some_and(|word| !not_a_name(word)),
        None => {
            (1..=2).contains(&words.len())
                && !text.contains('?')
                && !words
                    .iter()
                    .any(|word| not_a_name(word) || NOT_A_NAME_ALONE.contains(&word.as_str()))
        }
    }
}

/// A yes beside "hello" in a line ([`Onboarding::heard`]'s `preferences`):
/// [`YES`] without the words a greeting has too ("Hi, I'm fine", "Hello,
/// right"), and with no no word.
fn hello_yes(words: &[String]) -> bool {
    const HELLO_YES: [&str; 7] = ["yes", "yeah", "yep", "sure", "ok", "okay", "alright"];
    words.iter().any(|word| HELLO_YES.contains(&word.as_str())) && !refuses(words)
}

/// An answer about the hours to keep quiet: a time, a part of the day, or none.
/// A part of the day right after "good" is a greeting, not an hour: "Good
/// evening, Lamp." answers nothing (Fourteen's P2 on #3673).
fn quiet_hours(words: &[String]) -> bool {
    const GREETS: [&str; 4] = ["morning", "afternoon", "evening", "night"];
    const WHEN: [&str; 14] = [
        "quiet",
        "night",
        "morning",
        "evening",
        "afternoon",
        "hour",
        "hours",
        "o'clock",
        "am",
        "pm",
        "noon",
        "midnight",
        "after",
        "before",
    ];
    words.iter().enumerate().any(|(at, word)| {
        let greeting = at > 0 && words[at - 1] == "good" && GREETS.contains(&word.as_str());
        !greeting
            && (WHEN.contains(&word.as_str())
                || NO.contains(&word.as_str())
                || word.chars().any(|c| c.is_ascii_digit()))
    })
}

/// Whether the head is still: every head joint within [`ARRIVED_RADIANS`]
/// of its goal, turning slower than [`STILL_RADIANS_A_SECOND`], and asked
/// to be where it was asked to be at the last reading. `goals` keeps that
/// reading. What the body does not say (velocity, goals) is not waited for.
fn head_still(joints: &lamp_msgs::JointState, goals: &mut Option<[f64; 3]>) -> bool {
    let given = |list: &[f64]| list.len() == joints.name.len();
    let mut now = [0.0; 3];
    for (at, name) in joints.name.iter().enumerate() {
        let Some(which) = HEAD.iter().position(|head| head == name) else {
            continue;
        };
        let off = given(&joints.commanded)
            && given(&joints.position)
            && (joints.commanded[at] - joints.position[at]).abs() > ARRIVED_RADIANS;
        let turning = given(&joints.velocity) && joints.velocity[at].abs() > STILL_RADIANS_A_SECOND;
        if off || turning {
            *goals = None;
            return false;
        }
        if given(&joints.commanded) {
            now[which] = joints.commanded[at];
        }
    }
    let unchanged = goals.is_none_or(|before| before == now);
    *goals = Some(now);
    unchanged
}

/// Words of a line, lower case, without the marks around them.
fn words_of(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// A short line's yes or no; anything else is neither.
fn yes_or_no(text: &str) -> Option<bool> {
    let words = words_of(text);
    if words.is_empty() || words.len() > 4 {
        return None;
    }
    let (rest, phrase) = without_yes_phrases(&words);
    let has = |set: &[&str]| rest.iter().any(|word| set.contains(word));
    if has(&["no", "nope", "nah", "not", "wrong"]) {
        Some(false)
    } else if phrase
        || has(&[
            "yes", "yeah", "yep", "yup", "right", "correct", "exactly", "sure",
        ])
    {
        Some(true)
    } else {
        None
    }
}

/// A line that could be only a name: one or two words, after an "I'm",
/// and no question, yes, no, greeting or thanks. English words only.
fn name_in(text: &str) -> Option<String> {
    let greets = ["hello", "hi", "hey", "okay", "ok", "thanks", "thank"];
    if text.contains('?')
        || yes_or_no(text).is_some()
        || words_of(text)
            .iter()
            .any(|word| greets.contains(&word.as_str()))
    {
        return None;
    }
    let mut words: Vec<&str> = text
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|word| !word.is_empty())
        .collect();
    if words
        .first()
        .is_some_and(|word| ["i'm", "im", "it's", "its"].contains(&word.to_lowercase().as_str()))
    {
        words.remove(0);
    }
    let asks = ["what", "why", "how", "who", "where", "when", "which"];
    if words
        .first()
        .is_some_and(|word| asks.contains(&word.to_lowercase().as_str()))
    {
        return None;
    }
    (1..=2).contains(&words.len()).then(|| words.join(" "))
}

/// Whether eyes that give close receipts are on the bus now: their
/// `eyes/camera_closed` publisher shows itself alive (#1802).
fn eyes_give_receipts(eyes: Option<&lamp_bus::Bus>) -> bool {
    eyes.and_then(|bus| bus.publishers(lamp_msgs::CameraClosed::TOPIC).ok())
        .is_some_and(|sources| sources.iter().any(|source| source.name == EYES_SOURCE))
}

/// The eyes' receipt for this close: theirs, for this close's token, the
/// device released, and said within [`CAMERA_RECEIPT_MS`] of the ask. A
/// receipt for another close, an older one or a late one proves nothing.
fn camera_receipt(
    eyes: Option<&lamp_bus::Bus>,
    close: privacy::CameraClose,
) -> Result<(), &'static str> {
    let message = eyes
        .ok_or("no_bus")?
        .latest(lamp_msgs::CameraClosed::TOPIC)
        .ok()
        .flatten()
        .ok_or("no_receipt")?;
    if message.header.source != EYES_SOURCE {
        return Err("not_the_eyes");
    }
    let receipt = message
        .parse::<lamp_msgs::CameraClosed>()
        .map_err(|_| "unreadable")?;
    if receipt.close != close.token {
        return Err("another_close");
    }
    if !receipt.released {
        return Err("not_released");
    }
    let at = message.header.at_unix_ms;
    if at < close.asked_unix_ms || at - close.asked_unix_ms > CAMERA_RECEIPT_MS {
        return Err("late");
    }
    Ok(())
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// The largest face in a look, as a framing.
fn nearest(faces: &lamp_msgs::Faces) -> Option<Framing> {
    faces
        .faces
        .iter()
        .filter(|face| face.check().is_ok())
        .max_by(|one, other| {
            ((one.xmax - one.xmin) * (one.ymax - one.ymin))
                .total_cmp(&((other.xmax - other.xmin) * (other.ymax - other.ymin)))
        })
        .map(|face| Framing {
            size: face.ymax - face.ymin,
            x: face.xmin + face.xmax - 1.0,
            y: face.ymin + face.ymax - 1.0,
        })
}

/// The two functions the model is told of when the walk is switched on.
pub fn tools() -> Vec<Tool> {
    let names = Step::IDS;
    vec![
        Tool {
            name: STEP.into(),
            description: "Call this during your first meeting with someone, when a step you were asked \
                          to lead has ended: done, skipped because they asked to skip it, or failed. If \
                          they want to skip the rest, call it skipped for the step you are on. Then go on \
                          talking as the step asked. Only for the step you are leading now; \
                          where_you_sit, how_you_sound, touch, privacy and giving_way can only be \
                          skipped; the runner requires physical evidence: seeing them, hearing their \
                          call, feeling their touch, the switch's receipt, the body giving way. \
                          room_sounds and face_expressions are done only after a clear yes."
                .into(),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "step": {"type": "string", "enum": names},
                    "outcome": {"type": "string", "enum": ["done", "skipped", "failed"]}
                },
                "required": ["step", "outcome"]
            })),
            silent: false,
        },
        Tool {
            name: REDO.into(),
            description: "Call this when someone asks you to redo the setup, or to go through your first \
                          introductions again. Then say in a few words that you will start again."
                .into(),
            parameters: None,
            silent: false,
        },
    ]
}

pub fn is_tool(name: &str) -> bool {
    matches!(name, STEP | REDO)
}

/// The provider worker's side: put what the model reported in the mailbox
/// for the runner, and say what to tell the model. Each call's step,
/// outcome and answer go in the journal: on lamp-29e7 on 27 Oct three
/// setup_step calls left "the onboarding answered" and nothing more, so
/// what held "who you are" for 5 minutes could not be read (#3883).
pub fn run(file: &Path, call: &ToolCall) -> (Value, bool) {
    let (told, answered) = answer(file, call);
    let said = |key: &str| call.args.get(key).and_then(Value::as_str).unwrap_or("none");
    eprintln!(
        "lamp-voice: onboarding: {} step {} outcome {}: {}",
        call.name,
        said("step"),
        said("outcome"),
        told.get("error")
            .and_then(Value::as_str)
            .map_or_else(|| "noted".to_owned(), |error| format!("refused ({error})"))
    );
    (told, answered)
}

fn answer(file: &Path, call: &ToolCall) -> (Value, bool) {
    let line = match call.name.as_str() {
        REDO => json!({"redo": true}),
        STEP => {
            let step = call.args.get("step").and_then(Value::as_str);
            let outcome = call.args.get("outcome").and_then(Value::as_str);
            match (step.and_then(Step::named), outcome.and_then(Outcome::named)) {
                (Some(step), Some(Outcome::Skipped)) if step.says_its_skip() => {
                    if post(file, &json!({"step": step.name(), "outcome": outcome})).is_err() {
                        return (
                            json!({"error": "I could not note that just now. Go on all the same."}),
                            false,
                        );
                    }
                    return (
                        json!({"ok": true, "note": "I say that it is skipped myself. Say nothing about it, and nothing more until I do."}),
                        true,
                    );
                }
                (Some(step), Some(told)) if step.reportable(told) => {
                    json!({"step": step.name(), "outcome": outcome})
                }
                (Some(Step::Privacy), Some(_)) => {
                    return (
                        json!({"error":"Privacy is completed only by the physical switch closing the microphones and reopening. You can only skip it if they ask."}),
                        true,
                    );
                }
                (Some(Step::HowYouSound), Some(_)) => {
                    return (
                        json!({"error":"How they call you is learned only from their calls heard from their seat. You can only skip it if they ask."}),
                        true,
                    );
                }
                (Some(Step::GivingWay), Some(_)) => {
                    return (
                        json!({"error":"Giving way is done only when the body feels a hand move its head and let go. You can only skip it if they ask."}),
                        true,
                    );
                }
                (Some(Step::WhereYouSit), Some(_)) => {
                    return (
                        json!({"error": "Where they sit is done when you can see them well. You can only skip it, if they ask."}),
                        true,
                    );
                }
                (Some(Step::Touch), Some(_)) => {
                    return (
                        json!({"error": "The touch is done when you feel it on your head. You can only skip it, if they ask."}),
                        true,
                    );
                }
                _ => {
                    return (
                        json!({"error": "That is not a step and an outcome I know."}),
                        true,
                    );
                }
            }
        }
        _ => return (crate::tools::unavailable(), false),
    };
    match post(file, &line) {
        Ok(()) => (json!({"ok": true}), true),
        Err(_) => (
            json!({"error": "I could not note that just now. Go on all the same."}),
            false,
        ),
    }
}

/// A face `remember_my_face` saved (#3740): "who you are" is done, as the
/// model's own `setup_step` would say. The runner takes it only while it
/// leads that step, so a face saved at any other time changes nothing. On
/// a tester's 7071 lamp on 27 Oct the chat saved the face twice and the
/// step was never recorded.
pub fn face_saved(file: &Path) {
    let line = json!({"step": Step::WhoYouAre.name(), "outcome": "done",
        "by": crate::known_faces::REMEMBER});
    if let Err(error) = post(file, &line) {
        eprintln!("lamp-voice: onboarding: a face saved, not noted for \"who you are\": {error}");
    }
}

/// One line for the runner, appended to the mailbox, at most 16 KiB in all.
fn post(file: &Path, line: &Value) -> io::Result<()> {
    std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(mailbox_of(file))
        .and_then(|mut mailbox| {
            let line = format!("{line}\n");
            if mailbox.metadata()?.len().saturating_add(line.len() as u64) > 16 * 1024 {
                return Err(io::Error::other("onboarding mailbox is full"));
            }
            mailbox.write_all(line.as_bytes())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;
    const NEAR_MIDDLE: Framing = Framing {
        size: 0.2,
        x: 0.05,
        y: -0.1,
    };

    fn file(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "onboarding-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory.join("onboarding.json")
    }

    fn report(walk: &Onboarding, step: &str, outcome: &str) {
        let call = ToolCall {
            id: "c".into(),
            name: STEP.into(),
            args: json!({"step": step, "outcome": outcome}),
        };
        let (answer, ok) = run(&walk.file, &call);
        assert!(ok && answer["ok"] == json!(true), "{answer}");
    }

    /// A room at rest: the lamp's own notes are the only thing said, and
    /// each is taken as said the moment it is begun.
    struct Room {
        walk: Onboarding,
        at: u64,
        said: u64,
        still: bool,
        body_on_bus: bool,
    }

    impl Room {
        fn new(name: &str) -> Self {
            Self {
                walk: Onboarding::at(file(name)),
                at: 0,
                said: 0,
                still: true,
                body_on_bus: false,
            }
        }

        /// Ticks every quarter second for `for_us`, returning the notes begun.
        /// A room with [`NUDGE_ENVIRONMENT`] on.
        fn nudged(name: &str) -> Self {
            let mut room = Self::new(name);
            room.walk.nudge = true;
            room
        }

        fn wait(&mut self, for_us: u64, face: Option<Framing>) -> Vec<String> {
            let mut notes = Vec::new();
            let until = self.at + for_us;
            while self.at < until {
                self.walk.drain_storage_for_test(self.at);
                if let Some(note) =
                    self.walk
                        .tick(self.at, self.at - self.said, true, false, face, self.still)
                {
                    notes.push(note);
                    self.said = self.at;
                }
                self.walk.drain_storage_for_test(self.at);
                self.at += S / 4;
            }
            notes
        }

        /// As [`Self::wait`], the face jumping between `a` and `b` every
        /// half second.
        fn jumping(&mut self, for_us: u64, a: Framing, b: Framing) -> Vec<String> {
            let until = self.at + for_us;
            let mut notes = Vec::new();
            while self.at < until {
                let face = if (self.at / (S / 2)).is_multiple_of(2) {
                    a
                } else {
                    b
                };
                notes.extend(self.wait(S / 2, Some(face)));
            }
            notes
        }

        fn outcome(&self, step: Step) -> Option<Outcome> {
            self.walk
                .kept()
                .steps
                .get(step.name())
                .map(|kept| kept.outcome)
        }
    }

    const NONE: [&str; 0] = [];

    #[test]
    fn installed_order_words_and_nudge_drive_the_existing_runner() {
        let shipped = include_str!("../../../skills/onboarding/skill.toml");
        let changed = shipped
            .replacen(
                "order = [\"where_you_sit\", \"who_you_are\"",
                "order = [\"who_you_are\", \"where_you_sit\"",
                1,
            )
            .replacen(
                "Ask them, in a few words, what you should call them.",
                "Ask for their preferred name.",
                1,
            )
            .replace("after_answers = 1", "after_answers = 3")
            .replace(
                "Otherwise call setup_step with step",
                "Otherwise promptly call setup_step with step",
            );
        let definition = Arc::new(skill::parse(&changed).unwrap());
        let mut room = Room::nudged("installed-order");
        room.walk.definition = definition.clone();
        assert_eq!(
            room.wait(4 * S, Some(NEAR_MIDDLE)),
            [definition.note(Step::WhoYouAre, false)]
        );
        room.walk.heard_line("one");
        room.walk.heard_line("two");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), NONE);
        room.walk.heard_line("three");
        assert_eq!(
            room.wait(S, Some(NEAR_MIDDLE)),
            [definition.nudge(Step::WhoYouAre).unwrap().0]
        );
        // A file's order cannot give model reports physical authority.
        assert!(!Step::Privacy.reportable(Outcome::Done));
        assert!(!Step::GivingWay.reportable(Outcome::Done));
        room.walk.set_privacy_context(
            privacy::Epoch {
                device_boot: "test-boot".into(),
                clock_id: "test-clock".into(),
            },
            true,
            None,
            Some("beside the base"),
        );
        assert_eq!(room.walk.next_step(), Some(Step::Privacy));
    }

    #[test]
    fn installed_reoffer_words_and_interval_drive_the_existing_runner() {
        let mut room = Room::new("installed-reoffer");
        let mut definition = (*skill::fixture()).clone();
        definition.reoffer_after_s = 10;
        definition
            .steps
            .get_mut(&Step::Preferences)
            .unwrap()
            .reoffer = "(Offer the preferences once more.)".into();
        room.walk.definition = Arc::new(definition.checked().unwrap());
        let now = unix_ms() / 1_000;
        for step in room.walk.definition.order.iter().copied() {
            room.walk.desired.steps.insert(
                step.name().into(),
                Kept {
                    outcome: if step == Step::Preferences {
                        Outcome::NotAvailable
                    } else {
                        Outcome::Skipped
                    },
                    at_unix_s: now,
                    framing: None,
                    offered_at_unix_s: Some(now),
                },
            );
        }
        assert_eq!(room.walk.next_step(), None);
        room.walk
            .desired
            .steps
            .get_mut(Step::Preferences.name())
            .unwrap()
            .offered_at_unix_s = Some(now - 11);
        assert_eq!(room.walk.next_step(), Some(Step::Preferences));
        room.walk.save();
        room.walk.drain_storage_for_test(0);
        assert_eq!(
            room.wait(4 * S, Some(NEAR_MIDDLE)),
            ["(Offer the preferences once more.)"]
        );
    }

    const LEFT: Framing = Framing {
        size: 0.2,
        x: -0.7,
        y: -0.1,
    };
    const SIDE: Framing = Framing {
        size: 0.2,
        x: 0.7,
        y: -0.1,
    };

    #[test]
    fn the_switch_is_on_only_with_on_and_an_absolute_file() {
        assert_eq!(switched_on(None, Some("/a")), None);
        assert_eq!(switched_on(Some("off"), Some("/a")), None);
        assert_eq!(switched_on(Some("on"), None), None);
        assert_eq!(switched_on(Some("on"), Some("a")), None);
        assert_eq!(switched_on(Some("on"), Some("/")), None);
        assert_eq!(
            switched_on(Some("on"), Some("/a")),
            Some(PathBuf::from("/a"))
        );
    }

    #[test]
    fn the_steps_are_walked_in_order_and_kept() {
        let mut room = Room::new("order");
        // Nobody there: nothing begins.
        assert_eq!(room.wait(10 * S, None), NONE);
        // Someone far off: asked to sit, then asked to come closer.
        let far = Framing {
            size: 0.08,
            x: 0.0,
            y: 0.0,
        };
        assert_eq!(room.wait(9 * S, Some(far)), [SIT_NOTE, SIT_CLOSER]);
        // Steady for three seconds after the guide: done, said, kept.
        assert_eq!(room.wait(3 * S + S / 2, Some(NEAR_MIDDLE)), [SIT_DONE]);
        let sat = &room.walk.kept().steps["where_you_sit"];
        assert_eq!(sat.outcome, Outcome::Done);
        assert_eq!(sat.framing, Some(NEAR_MIDDLE));
        // Who you are ends when the model says so; the placeholders are
        // passed over unspoken; then the preferences.
        assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        report(&room.walk, "who_you_are", "done");
        assert_eq!(room.wait(5 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        for step in [
            Step::HowYouSound,
            Step::Touch,
            Step::Privacy,
            Step::GivingWay,
        ] {
            assert_eq!(room.outcome(step), Some(Outcome::NotAvailable));
        }
        report(&room.walk, "preferences", "done");
        assert_eq!(room.wait(120 * S, Some(NEAR_MIDDLE)), NONE);
        assert_eq!(room.walk.kept().steps.len(), Step::IDS.len());
        assert_eq!(room.outcome(Step::Preferences), Some(Outcome::Done));
        // Kept across a restart: nothing is asked again.
        let mut again = Room {
            walk: Onboarding::at(room.walk.file.clone()),
            at: 0,
            said: 0,
            still: true,
            body_on_bus: false,
        };
        assert_eq!(again.walk.kept(), room.walk.kept());
        assert_eq!(again.wait(30 * S, Some(NEAR_MIDDLE)), NONE);
    }

    /// #3747 (the lead on #1711): the walk is under way from its start to
    /// its end, waiting for its first ask and the gaps between steps too,
    /// not only while a step is led; over once every step has ended; under
    /// way again when it is redone.
    #[test]
    fn the_walk_is_under_way_from_its_start_to_its_end_gaps_and_all() {
        let mut room = Room::new("under-way");
        // Nobody there yet: nothing asked, and the walk is waiting to begin.
        assert_eq!(room.wait(2 * S, None), NONE);
        assert!(!room.walk.leading());
        assert_eq!(room.walk.under_way(), Some(true));
        assert_eq!(room.wait(4 * S, Some(SIDE)), [SIT_NOTE]);
        assert!(room.walk.leading());
        assert_eq!(room.walk.under_way(), Some(true));
        report(&room.walk, "where_you_sit", "skipped");
        // Between two steps: the first ended (its skip said, #3769), the
        // next not yet asked.
        assert_eq!(room.wait(S, None), [WHERE_YOU_SIT_SKIPPED]);
        assert_eq!(room.outcome(Step::WhereYouSit), Some(Outcome::Skipped));
        assert!(!room.walk.leading());
        assert_eq!(room.walk.under_way(), Some(true), "between steps");
        assert_eq!(room.wait(4 * S, Some(SIDE)), [WHO_NOTE]);
        report(&room.walk, "who_you_are", "skipped");
        assert_eq!(room.wait(4 * S, Some(SIDE)), [PREFERENCES_NOTE]);
        assert_eq!(room.walk.under_way(), Some(true));
        report(&room.walk, "preferences", "skipped");
        assert_eq!(room.wait(4 * S, Some(SIDE)), NONE);
        assert_eq!(room.walk.kept().steps.len(), Step::IDS.len());
        assert_eq!(room.walk.under_way(), Some(false), "every step ended");
        // Kept across a restart: still over. Before a walk has read its
        // progress, it doesn't say.
        let again = Onboarding::at(room.walk.file.clone());
        assert_eq!(again.under_way(), Some(false));
        let unread = Onboarding::with_storage(room.walk.file.clone(), again.storage.clone());
        assert_eq!(unread.under_way(), None);
        let redo = ToolCall {
            id: "c".into(),
            name: REDO.into(),
            args: json!({}),
        };
        assert!(run(&room.walk.file, &redo).1);
        assert_eq!(room.wait(4 * S, Some(SIDE)), [SIT_NOTE]);
        assert_eq!(room.walk.under_way(), Some(true), "redone");
    }

    #[test]
    fn a_step_is_skipped_and_the_walk_redone() {
        let mut room = Room::new("skip");
        assert_eq!(room.wait(4 * S, Some(SIDE)), [SIT_NOTE]);
        report(&room.walk, "where_you_sit", "skipped");
        assert_eq!(
            room.wait(4 * S, Some(SIDE)),
            [WHERE_YOU_SIT_SKIPPED, WHO_NOTE]
        );
        assert_eq!(room.outcome(Step::WhereYouSit), Some(Outcome::Skipped));
        report(&room.walk, "who_you_are", "skipped");
        assert_eq!(room.wait(4 * S, Some(SIDE)), [PREFERENCES_NOTE]);
        report(&room.walk, "preferences", "skipped");
        assert_eq!(room.wait(4 * S, Some(SIDE)), NONE);
        assert_eq!(room.walk.kept().steps.len(), Step::IDS.len());
        // "Lamp, let's redo the setup."
        let redo = ToolCall {
            id: "c".into(),
            name: REDO.into(),
            args: json!({}),
        };
        assert!(run(&room.walk.file, &redo).1);
        assert_eq!(room.wait(4 * S, Some(SIDE)), [SIT_NOTE]);
        assert!(room.walk.kept().steps.is_empty());
        // A report the runner does not know is refused.
        let wrong = ToolCall {
            id: "c".into(),
            name: STEP.into(),
            args: json!({"step": "dance", "outcome": "done"}),
        };
        assert!(run(&room.walk.file, &wrong).0.get("error").is_some());
    }

    /// Ticks every quarter second for `for_us` with what the body says on
    /// each, judged as the coordinator judges it.
    fn felt(
        room: &mut Room,
        for_us: u64,
        joints: impl Fn() -> Heard<lamp_msgs::JointState>,
        hand: impl Fn() -> Heard<lamp_msgs::Hand>,
    ) {
        let until = room.at + for_us;
        while room.at < until {
            room.walk.drain_storage_for_test(room.at);
            let still = room.walk.judge(joints(), hand(), room.body_on_bus);
            if room
                .walk
                .tick(
                    room.at,
                    room.at - room.said,
                    true,
                    false,
                    Some(NEAR_MIDDLE),
                    still,
                )
                .is_some()
            {
                room.said = room.at;
            }
            room.walk.drain_storage_for_test(room.at);
            room.at += S / 4;
        }
    }

    fn body(velocity: f64) -> lamp_msgs::JointState {
        lamp_msgs::JointState {
            name: HEAD.map(String::from).to_vec(),
            position: vec![0.1, 0.2, 0.3],
            velocity: vec![velocity, 0.0, 0.0],
            effort: vec![],
            commanded: vec![0.1, 0.2, 0.3],
        }
    }

    const RELEASED: lamp_msgs::Hand = lamp_msgs::Hand {
        pushed: false,
        since_us: 0,
    };

    #[test]
    fn where_you_sit_holds_only_on_fresh_evidence_from_the_body() {
        // The head last seen moving, then silence: never done.
        let mut room = Room::new("moving-silent");
        felt(
            &mut room,
            S,
            || Heard::Fresh(body(1.0)),
            || Heard::Fresh(RELEASED),
        );
        felt(
            &mut room,
            20 * S,
            || Heard::Never,
            || Heard::Fresh(RELEASED),
        );
        felt(
            &mut room,
            20 * S,
            || Heard::Stale,
            || Heard::Fresh(RELEASED),
        );
        assert_eq!(room.outcome(Step::WhereYouSit), None);
        // A hand last felt on the lamp, then silence: never done.
        let mut room = Room::new("pushed-silent");
        let pushed = || {
            Heard::Fresh(lamp_msgs::Hand {
                pushed: true,
                since_us: 5,
            })
        };
        felt(&mut room, S, || Heard::Fresh(body(0.0)), pushed);
        felt(
            &mut room,
            20 * S,
            || Heard::Fresh(body(0.0)),
            || Heard::Never,
        );
        felt(
            &mut room,
            20 * S,
            || Heard::Fresh(body(0.0)),
            || Heard::Stale,
        );
        assert_eq!(room.outcome(Step::WhereYouSit), None);
        // Fresh, still and released throughout: done.
        felt(
            &mut room,
            4 * S,
            || Heard::Fresh(body(0.0)),
            || Heard::Fresh(RELEASED),
        );
        assert_eq!(room.outcome(Step::WhereYouSit), Some(Outcome::Done));
        // A body that has never said either: no gate.
        let mut bare = Room::new("no-body");
        felt(&mut bare, 8 * S, || Heard::Never, || Heard::Never);
        assert_eq!(bare.outcome(Step::WhereYouSit), Some(Outcome::Done));
        // A topic present on the bus but stale is a body that has spoken.
        let mut stale = Room::new("stale-first");
        felt(&mut stale, 20 * S, || Heard::Stale, || Heard::Never);
        assert_eq!(stale.outcome(Step::WhereYouSit), None);
    }

    /// A walk to its end with every step skipped but the first.
    fn walked(name: &str) -> Room {
        let mut room = Room::new(name);
        assert_eq!(room.wait(8 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.walk.take_gesture(), None);
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        report(&room.walk, "who_you_are", "done");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        assert_eq!(room.walk.take_gesture(), None);
        report(&room.walk, "preferences", "skipped");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), NONE);
        assert_eq!(room.walk.take_gesture(), None);
        room
    }

    #[test]
    fn completed_setup_steps_do_not_move_the_body() {
        let mut room = walked("gesture");
        assert_eq!(room.walk.kept().steps.len(), Step::IDS.len());
        assert_eq!(room.walk.take_gesture(), None);
    }

    #[test]
    fn a_step_that_was_not_available_is_offered_once_a_day_when_it_is() {
        // Every placeholder now has its own measure; preferences stands in
        // for a step a first voice lacked and a later one has.
        let without = |step: Step| step != Step::Preferences && Step::available(step);
        let mut room = Room::new("offer");
        room.walk.available = without;
        assert_eq!(room.wait(8 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        report(&room.walk, "who_you_are", "done");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), NONE);
        assert_eq!(room.outcome(Step::Preferences), Some(Outcome::NotAvailable));
        // The same voice: nothing more is offered.
        let mut same = Room {
            walk: Onboarding::at(room.walk.file.clone()),
            at: 0,
            said: 0,
            still: true,
            body_on_bus: false,
        };
        same.walk.available = without;
        assert_eq!(same.wait(30 * S, Some(NEAR_MIDDLE)), NONE);
        // A later voice that has it: offered, after the walk.
        let mut later = Room {
            walk: Onboarding::at(room.walk.file.clone()),
            at: 0,
            said: 0,
            still: true,
            body_on_bus: false,
        };
        assert_eq!(later.wait(4 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        assert!(later.walk.leading());
        // Led to its end like any step, while it is being led.
        assert_eq!(later.wait(20 * S, Some(NEAR_MIDDLE)), NONE);
        assert!(later.walk.leading());
        // They leave before it ends: not offered again today.
        later.wait(31 * S, None);
        assert!(!later.walk.leading());
        assert_eq!(later.wait(60 * S, Some(NEAR_MIDDLE)), NONE);
        assert_eq!(
            later.outcome(Step::Preferences),
            Some(Outcome::NotAvailable)
        );
        // A day later it is offered again, and done it is kept.
        later
            .walk
            .desired
            .steps
            .get_mut("preferences")
            .unwrap()
            .offered_at_unix_s = Some(unix_ms() / 1_000 - OFFER_EVERY_S);
        assert_eq!(later.wait(4 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        report(&later.walk, "preferences", "done");
        assert_eq!(later.wait(60 * S, Some(NEAR_MIDDLE)), NONE);
        assert_eq!(later.outcome(Step::Preferences), Some(Outcome::Done));
        assert_eq!(later.walk.take_gesture(), None);
    }

    #[test]
    fn any_sign_of_a_body_keeps_the_gate_closed_for_good() {
        // Stale first, then missing: still a body.
        let mut room = Room::new("stale-missing");
        felt(&mut room, S, || Heard::Stale, || Heard::Never);
        felt(&mut room, 20 * S, || Heard::Never, || Heard::Never);
        assert_eq!(room.outcome(Step::WhereYouSit), None);
        // A read that fails is a body not saying so, never no body.
        assert!(matches!(
            heard(Err(std::io::Error::other("read")), 0),
            Heard::Stale
        ));
        assert!(matches!(heard(Ok(None), 0), Heard::Never));
        let mut room = Room::new("error-missing");
        felt(
            &mut room,
            S,
            || Heard::Never,
            || heard(Err(std::io::Error::other("read")), 0).parsed(),
        );
        felt(&mut room, 20 * S, || Heard::Never, || Heard::Never);
        assert_eq!(room.outcome(Step::WhereYouSit), None);
        // Joints heard and a hand never readable: the hand is not exempt.
        let mut room = Room::new("no-hand");
        felt(
            &mut room,
            20 * S,
            || Heard::Fresh(body(0.0)),
            || Heard::Never,
        );
        assert_eq!(room.outcome(Step::WhereYouSit), None);
        // A body topic declared on the bus and neither read: not exempt.
        let mut room = Room::new("declared");
        room.body_on_bus = true;
        felt(&mut room, S, || Heard::Never, || Heard::Never);
        room.body_on_bus = false;
        felt(&mut room, 20 * S, || Heard::Never, || Heard::Never);
        assert_eq!(room.outcome(Step::WhereYouSit), None);
        // The same with fresh, still and released throughout: done.
        felt(
            &mut room,
            4 * S,
            || Heard::Fresh(body(0.0)),
            || Heard::Fresh(RELEASED),
        );
        assert_eq!(room.outcome(Step::WhereYouSit), Some(Outcome::Done));
    }

    #[test]
    fn where_you_sit_holds_only_while_the_head_is_still() {
        // A head that keeps moving never completes the framing.
        let mut room = Room::new("moving");
        room.still = false;
        let notes = room.wait(30 * S, Some(NEAR_MIDDLE));
        assert_eq!(notes.first().map(String::as_str), Some(SIT_NOTE));
        assert!(!notes.iter().any(|note| note == SIT_DONE));
        assert_eq!(room.outcome(Step::WhereYouSit), None);
        // Still now: three seconds complete it.
        room.still = true;
        assert_eq!(room.wait(3 * S + S / 2, Some(NEAR_MIDDLE)), [SIT_DONE]);
        assert_eq!(room.outcome(Step::WhereYouSit), Some(Outcome::Done));
        // Motion mid-hold starts the three seconds again.
        let mut room = Room::new("mid-hold");
        assert_eq!(room.wait(4 * S, Some(SIDE)), [SIT_NOTE]);
        room.wait(2 * S, Some(NEAR_MIDDLE));
        room.still = false;
        room.wait(S / 4, Some(NEAR_MIDDLE));
        room.still = true;
        room.wait(2 * S + S / 2, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhereYouSit), None);
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhereYouSit), Some(Outcome::Done));
    }

    #[test]
    fn with_the_nudge_a_steady_face_after_the_guide_is_a_seat_while_the_gaze_moves_the_head() {
        // Row 10 try 4 on lamp-52e6: the face in 39 of 40 looks, the head
        // never still for 3 s.
        let drifted = Framing {
            size: 0.22,
            x: 0.75,
            y: -0.05,
        };
        let mut room = Room::nudged("gazed");
        let mut notes = Vec::new();
        // The head moving every other second, the face drifting a little.
        for second in 0..16 {
            room.still = second % 2 == 1;
            let face = if second % 2 == 0 { SIDE } else { drifted };
            notes.extend(room.wait(S, Some(face)));
            if room.outcome(Step::WhereYouSit).is_some() {
                break;
            }
        }
        assert_eq!(notes, [SIT_NOTE, SIT_TURN, SIT_DONE]);
        assert_eq!(room.outcome(Step::WhereYouSit), Some(Outcome::Done));
        assert!(room.at <= 15 * S);
        // Off: the same room is never done.
        let mut off = Room::new("gazed-off");
        for second in 0..30 {
            off.still = second % 2 == 1;
            off.wait(S, Some(if second % 2 == 0 { SIDE } else { drifted }));
        }
        assert_eq!(off.outcome(Step::WhereYouSit), None);
        // A face jumping around is not one, and fails at 60 s.
        let mut room = Room::nudged("jumping");
        room.still = false;
        assert_eq!(room.jumping(30 * S, SIDE, LEFT), [SIT_NOTE, SIT_TURN]);
        assert_eq!(room.outcome(Step::WhereYouSit), None);
        assert_eq!(room.jumping(35 * S, SIDE, LEFT), [SIT_FAILED]);
        assert_eq!(room.outcome(Step::WhereYouSit), Some(Outcome::Failed));
    }

    #[test]
    fn with_the_nudge_the_walk_waits_for_a_near_face_that_stays() {
        // Row 10 try 5 on lamp-52e6: someone walking by far behind the empty
        // seat started the walk; seated, the face was 0.14 to 0.18.
        let passing = Framing {
            size: 0.06,
            x: 0.3,
            y: -0.2,
        };
        let mut room = Room::nudged("passing");
        assert_eq!(room.wait(30 * S, Some(passing)), NONE);
        // Near, but gone again within two seconds: still nothing.
        assert_eq!(room.wait(S + S / 2, Some(NEAR_MIDDLE)), NONE);
        assert_eq!(room.wait(S, Some(passing)), NONE);
        // Near and staying two seconds: the walk begins.
        assert_eq!(room.wait(S + S * 3 / 4, Some(NEAR_MIDDLE)), NONE);
        assert_eq!(room.wait(S / 2, Some(NEAR_MIDDLE)), [SIT_NOTE]);
        // Off: the passing face starts it, as before.
        let mut off = Room::new("passing-off");
        assert_eq!(off.wait(4 * S, Some(passing)), [SIT_NOTE]);
    }

    #[test]
    fn with_the_nudge_a_hand_on_the_lamp_still_keeps_the_seat_open() {
        let pushed = || {
            Heard::Fresh(lamp_msgs::Hand {
                pushed: true,
                since_us: 5,
            })
        };
        let mut room = Room::nudged("hand");
        felt(&mut room, 30 * S, || Heard::Fresh(body(1.0)), pushed);
        assert_eq!(room.outcome(Step::WhereYouSit), None);
        // Let go, the head still moving: the steady face is the seat.
        felt(
            &mut room,
            4 * S,
            || Heard::Fresh(body(1.0)),
            || Heard::Fresh(RELEASED),
        );
        assert_eq!(room.outcome(Step::WhereYouSit), Some(Outcome::Done));
    }

    /// A room led to "who you are", with the nudge on or off.
    fn at_who(name: &str, nudge: bool) -> Room {
        let mut room = Room::new(name);
        room.walk.nudge = nudge;
        assert_eq!(room.wait(8 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        room
    }

    fn nudge_records(room: &mut Room) -> Vec<Value> {
        let records = room.walk.take_records();
        assert!(!serde_json::to_string(&records).unwrap().contains("Tovi"));
        records
            .into_iter()
            .filter(|line| line["kind"] == "onboarding_nudge")
            .collect()
    }

    #[test]
    fn a_step_the_model_ends_is_nudged_once_after_it_is_answered() {
        // Row 10 try 4 on lamp-52e6: the name heard, no setup_step called.
        let mut room = at_who("nudged", true);
        room.walk.heard_line("My name is Tovi Ama.");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [WHO_NUDGE]);
        // A second line: no second note, and no name to ask back.
        room.walk.heard_line("Okay.");
        assert_eq!(room.wait(10 * S, Some(NEAR_MIDDLE)), NONE);
        report(&room.walk, "who_you_are", "done");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        let counted = nudge_records(&mut room);
        assert_eq!(counted.len(), 1);
        assert_eq!(counted[0]["answers"], 2);
        assert_eq!(counted[0]["nudged"], true);
        assert_eq!(counted[0]["reported"], true);
        // The preferences: nudged only after both answers.
        room.walk.heard_line("After ten at night.");
        assert_eq!(room.wait(5 * S, Some(NEAR_MIDDLE)), NONE);
        room.walk.heard_line("Yes, please.");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NUDGE]);
        // A line with no words: none.
        let mut room = at_who("wordless", true);
        room.walk.heard_line("...");
        assert_eq!(room.wait(10 * S, Some(NEAR_MIDDLE)), NONE);
        // Reported before the quiet moment: none.
        let mut room = at_who("reported", true);
        room.walk.heard_line("Tovi.");
        report(&room.walk, "who_you_are", "done");
        // The model's own words after its call.
        room.said = room.at;
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        // Off: none.
        let mut room = at_who("off", false);
        room.walk.heard_line("Tovi.");
        assert_eq!(room.wait(10 * S, Some(NEAR_MIDDLE)), NONE);
        assert!(nudge_records(&mut room).is_empty());
    }

    #[test]
    fn a_name_said_alone_is_asked_back_and_kept_on_a_yes() {
        let asked = "(If you asked whether you may remember their face and they have not answered yet, wait for their answer. Otherwise ask them, in a few words: \"Did I get that right, Tovi?\" Nothing more.)";
        let mut room = at_who("yes", true);
        room.walk.lamp_said();
        room.walk.heard_line("I'm Tovi.");
        assert_eq!(room.wait(3 * S, Some(NEAR_MIDDLE)), [WHO_NUDGE]);
        // Not while an answer on the face may still be coming.
        assert_eq!(room.wait(6 * S, Some(NEAR_MIDDLE)), NONE);
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [asked]);
        room.walk.heard_line("Yes, that's right.");
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), Some(Outcome::Done));
        let counted = nudge_records(&mut room);
        assert_eq!(counted[0]["name_asked_back"], true);
        assert_eq!(counted[0]["name_confirmed"], true);
        assert_eq!(counted[0]["reported"], false);
        // No: asked once more, then never asked back again.
        let mut room = at_who("no", true);
        room.walk.lamp_said();
        room.walk.heard_line("Tovi");
        assert_eq!(room.wait(3 * S, Some(NEAR_MIDDLE)), [WHO_NUDGE]);
        assert_eq!(room.wait(10 * S, Some(NEAR_MIDDLE)), [asked]);
        room.walk.heard_line("No.");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [WHO_AGAIN]);
        room.walk.heard_line("Tovina");
        assert_eq!(room.wait(15 * S, Some(NEAR_MIDDLE)), NONE);
        assert_eq!(room.outcome(Step::WhoYouAre), None);
        // A question is not a name.
        let mut room = at_who("question", true);
        room.walk.lamp_said();
        room.walk.heard_line("Why do you ask?");
        assert_eq!(room.wait(3 * S, Some(NEAR_MIDDLE)), [WHO_NUDGE]);
        assert_eq!(room.wait(15 * S, Some(NEAR_MIDDLE)), NONE);
        // Nor a line heard before the lamp asked its question.
        let mut room = at_who("early", true);
        room.walk.heard_line("Tovi");
        assert_eq!(room.wait(3 * S, Some(NEAR_MIDDLE)), [WHO_NUDGE]);
        assert_eq!(room.wait(15 * S, Some(NEAR_MIDDLE)), NONE);
        for line in ["What?", "Sure.", "Hello", "Okay", "Thanks", "Hi there"] {
            assert_eq!(name_in(line), None, "{line}");
        }
        assert_eq!(name_in("Tovi Ama"), Some("Tovi Ama".into()));
    }

    #[test]
    fn the_head_is_still_only_at_its_goal_slow_and_with_the_same_goals() {
        let joints = |off: f64, velocity: f64, goal: f64| lamp_msgs::JointState {
            name: ["base_yaw", "shoulder", "head_pitch", "head_roll"]
                .map(String::from)
                .to_vec(),
            position: vec![goal + off, 1.0, 0.2, 0.3],
            velocity: vec![velocity, 2.0, 0.0, 0.0],
            effort: vec![],
            commanded: vec![goal, 0.0, 0.2, 0.3],
        };
        let count = std::f64::consts::TAU / 4_096.0;
        let mut goals = None;
        // At the goal, slow, a moving shoulder: still.
        assert!(head_still(&joints(5.0 * count, 0.0, 0.1), &mut goals));
        assert!(head_still(&joints(0.0, 20.0 * count, 0.1), &mut goals));
        // Seven counts off, or turning fast: moving.
        assert!(!head_still(&joints(7.0 * count, 0.0, 0.1), &mut goals));
        assert!(head_still(&joints(0.0, 0.0, 0.1), &mut goals));
        assert!(!head_still(&joints(0.0, 30.0 * count, 0.1), &mut goals));
        assert!(head_still(&joints(0.0, 0.0, 0.1), &mut goals));
        // A new goal, even one already reached: moving, once.
        assert!(!head_still(&joints(0.0, 0.0, 0.2), &mut goals));
        assert!(head_still(&joints(0.0, 0.0, 0.2), &mut goals));
        // A body that says only where its joints are: still.
        let mut bare = joints(0.0, 0.0, 0.2);
        bare.velocity.clear();
        bare.commanded.clear();
        assert!(head_still(&bare, &mut None));
    }

    #[test]
    fn where_you_sit_is_done_only_by_what_the_eyes_see() {
        let mut room = Room::new("forged");
        let done = ToolCall {
            id: "c".into(),
            name: STEP.into(),
            args: json!({"step": "where_you_sit", "outcome": "done"}),
        };
        let forged = "{\"step\":\"where_you_sit\",\"outcome\":\"done\"}\n";
        // Refused by the worker with words the model can say ...
        assert!(run(&room.walk.file, &done).0.get("error").is_some());
        // ... and, written into the mailbox all the same, by the runner:
        // with no face and no step being led ...
        std::fs::write(mailbox_of(&room.walk.file), forged).unwrap();
        assert_eq!(room.wait(10 * S, None), NONE);
        assert_eq!(room.outcome(Step::WhereYouSit), None);
        // ... and while the step is being led.
        assert_eq!(room.wait(4 * S, Some(SIDE)), [SIT_NOTE]);
        std::fs::write(mailbox_of(&room.walk.file), forged).unwrap();
        room.wait(S, Some(SIDE));
        assert_eq!(room.outcome(Step::WhereYouSit), None);
        // A step that is not being led is not taken either.
        report(&room.walk, "preferences", "done");
        room.wait(S, Some(SIDE));
        assert_eq!(room.outcome(Step::Preferences), None);
        assert!(
            room.walk
                .take_records()
                .iter()
                .any(|line| line["kind"] == "onboarding_report_refused")
        );
        assert!(room.walk.leading());
    }

    #[test]
    fn a_seat_beside_the_lamp_is_taken_after_one_guide() {
        // Row 10 on lamp-52e6: sitting beside the lamp, the face low in the
        // picture.
        let beside = Framing {
            size: 0.2,
            x: 0.12,
            y: -0.63,
        };
        let mut room = Room::new("beside");
        assert_eq!(room.wait(12 * S, Some(beside)), [SIT_NOTE, SIT_TURN]);
        assert_eq!(room.wait(4 * S, Some(beside)), [SIT_DONE]);
        let sat = &room.walk.kept().steps["where_you_sit"];
        assert_eq!(sat.outcome, Outcome::Done);
        assert_eq!(sat.framing, Some(beside));
    }

    #[test]
    fn the_leads_seat_at_about_a_metre_passes_without_a_guide() {
        // Row 10 on lamp-52e6 (21 October): the lead's chair, a little high.
        let chair = Framing {
            size: 0.15,
            x: 0.05,
            y: -0.26,
        };
        let mut room = Room::new("chair");
        assert_eq!(room.wait(7 * S, Some(chair)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.walk.kept().steps["where_you_sit"].framing, Some(chair));
    }

    #[test]
    fn a_face_at_the_edge_is_looked_at_before_the_lamp_asks_to_be_turned() {
        let edge = Framing {
            size: 0.2,
            x: 0.95,
            y: 0.0,
        };
        let mut room = Room::new("edge");
        assert_eq!(room.wait(4 * S, Some(edge)), [SIT_NOTE]);
        // The lamp looks at the person: its body is told it listens.
        assert!(room.walk.looking());
        // Out of view, then back at the edge after the guide's quiet: the
        // lamp's own look comes first ...
        assert_eq!(room.wait(6 * S, None), NONE);
        assert_eq!(room.wait(4 * S, Some(edge)), NONE);
        assert!(room.walk.looking());
        // ... and only then is the person asked to turn it.
        assert_eq!(room.wait(2 * S, Some(edge)), [SIT_TURN]);
        assert!(room.walk.looking());
    }

    /// With the nudge on, a face saved ends "who you are" too, but its
    /// nudge record is no report of the model's, and the walk's record
    /// names the face save (the Reviewer-lead's P3s on #3746).
    #[test]
    fn a_face_saved_with_the_nudge_on_is_no_report_of_the_models() {
        let mut room = at_who("face-saved-nudge", true);
        room.walk.take_records();
        face_saved(&room.walk.file);
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), Some(Outcome::Done));
        let records = room.walk.take_records();
        let nudges: Vec<&Value> = records
            .iter()
            .filter(|line| line["kind"] == "onboarding_nudge")
            .collect();
        assert!(!nudges.is_empty(), "{records:?}");
        assert!(
            nudges.iter().all(|line| line["reported"] == false),
            "{nudges:?}"
        );
        assert!(
            records.iter().any(
                |line| line["kind"] == "onboarding_face_saved" && line["step"] == "who_you_are"
            ),
            "{records:?}"
        );
    }

    /// A face `remember_my_face` saved while "who you are" is led ends it
    /// (#3740, a tester's 7071 lamp); saved before the step is led, it
    /// changes nothing.
    #[test]
    fn a_face_saved_ends_who_you_are_only_while_it_is_led() {
        let mut room = Room::new("face-saved");
        face_saved(&room.walk.file);
        assert_eq!(room.wait(7 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(
            room.outcome(Step::WhoYouAre),
            None,
            "saved before it was led"
        );
        face_saved(&room.walk.file);
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), Some(Outcome::Done));
    }

    #[test]
    fn a_name_said_and_confirmed_ends_who_you_are_with_no_call() {
        let mut room = Room::new("heard-name");
        assert_eq!(room.wait(7 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        room.walk.heard("You can call me, Marie.", room.at);
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), None);
        room.walk.heard("Yes. That's fine.", room.at);
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), Some(Outcome::Done));
        // The model's call after it changes nothing.
        report(&room.walk, "who_you_are", "failed");
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), Some(Outcome::Done));
        // Without the nudge, `heard` alone ended it, once, the name unkept.
        let records = room.walk.take_records();
        let of_who = |kind: &str| {
            records
                .iter()
                .filter(|line| line["kind"] == kind && line["step"] == "who_you_are")
                .filter(|line| kind != "onboarding_step" || line["outcome"].is_string())
                .count()
        };
        assert_eq!(of_who("onboarding_step"), 1, "{records:?}");
        assert_eq!(of_who("onboarding_heard"), 1, "{records:?}");
    }

    /// #3883: on lamp-29e7 "who you are" took 5 minutes. A name and a yes
    /// said as "no problem", "of course" or "go ahead" end it at once, and
    /// the "no" in "no problem" no longer clears the name heard.
    #[test]
    fn a_name_and_a_yes_phrase_end_who_you_are_in_one_exchange() {
        for yes in [
            "No problem.",
            "Of course!",
            "Sure, go ahead.",
            "I don't mind.",
        ] {
            let mut room = Room::new("yes-phrase");
            assert_eq!(room.wait(7 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
            assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
            room.walk.heard("I'm Marie.", room.at);
            room.wait(S, Some(NEAR_MIDDLE));
            assert_eq!(room.outcome(Step::WhoYouAre), None);
            room.walk.heard(yes, room.at);
            room.wait(S, Some(NEAR_MIDDLE));
            assert_eq!(room.outcome(Step::WhoYouAre), Some(Outcome::Done), "{yes}");
        }
    }

    #[test]
    fn yes_phrases_are_a_yes_and_a_plain_no_stays_a_no() {
        let words = |text: &str| words_of(text);
        for yes in [
            "No problem",
            "of course",
            "go ahead",
            "I don't mind",
            "I do not mind",
        ] {
            assert!(agrees(&words(yes)), "{yes}");
            assert!(!refuses(&words(yes)), "{yes}");
            assert_eq!(yes_or_no(yes), Some(true), "{yes}");
        }
        for statement in [
            "I'll go ahead and sit down.",
            "There is no problem with the wifi.",
            "I don't mind the noise from the street.",
        ] {
            assert!(!agrees(&words(statement)), "{statement}");
        }
        for no in ["No", "Of course not", "No, don't", "Please don't"] {
            assert!(!agrees(&words(no)), "{no}");
            assert!(refuses(&words(no)), "{no}");
        }
        assert_eq!(yes_or_no("of course not"), Some(false));
    }

    /// With a face to keep (the eyes' who socket), a name and a yes leave
    /// "who you are" open: the yes is the face question's, and the face
    /// saved ends it (lamp-29e7, 28 Oct: ended on the yes, no face kept).
    #[test]
    fn with_a_face_to_keep_a_name_and_a_yes_wait_for_the_face() {
        // The ways a person says yes to the face (the lead's merge condition
        // on #1711): never done before the face is kept, the model's save
        // coming a few seconds on.
        for yes in ["Yes.", "No problem.", "Of course!", "Sure, go ahead."] {
            let mut room = Room::new("face-kept");
            room.walk.face_kept = true;
            assert_eq!(room.wait(7 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
            assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
            room.walk.heard("You can call me, Marie.", room.at);
            room.walk.heard(yes, room.at);
            room.wait(5 * S, Some(NEAR_MIDDLE));
            assert_eq!(room.outcome(Step::WhoYouAre), None, "{yes}");
            face_saved(&room.walk.file);
            room.wait(S, Some(NEAR_MIDDLE));
            assert_eq!(room.outcome(Step::WhoYouAre), Some(Outcome::Done), "{yes}");
        }
    }

    /// The Reviewer-lead's P2s on #3992: with a face to keep, a face no of
    /// any length keeps the name and ends it done after `FACE_SAVE_WAIT_US`
    /// plus quiet, never asking the name again; a yes with no face saved
    /// ends it done after `FACE_SAVE_WAIT_US`, not at the give-up.
    #[test]
    fn with_a_face_to_keep_a_no_or_no_save_still_ends_it_with_the_name() {
        for no in ["No, please don't.", "No thanks, I'd rather not."] {
            let mut room = Room::new("face-no");
            room.walk.face_kept = true;
            assert_eq!(room.wait(7 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
            assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
            room.walk.heard("You can call me, Marie.", room.at);
            room.walk.heard("Yes.", room.at);
            room.wait(4 * S, Some(NEAR_MIDDLE));
            room.walk.heard(no, room.at);
            room.wait(FACE_SAVE_WAIT_US - S, Some(NEAR_MIDDLE));
            assert_eq!(room.outcome(Step::WhoYouAre), None, "{no}");
            assert!(
                room.walk.led.as_ref().is_some_and(|led| led.heard.name),
                "{no}"
            );
            room.wait(START_QUIET_US + 2 * S, Some(NEAR_MIDDLE));
            assert_eq!(room.outcome(Step::WhoYouAre), Some(Outcome::Done), "{no}");
            let records = room.walk.take_records();
            assert!(
                !records
                    .iter()
                    .any(|line| line["kind"] == "onboarding_stopped"),
                "{no}: {records:?}"
            );
        }

        let mut room = Room::new("face-unsaved");
        room.walk.face_kept = true;
        assert_eq!(room.wait(7 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        room.walk.heard("You can call me, Marie.", room.at);
        room.walk.heard("Sure.", room.at);
        room.wait(FACE_SAVE_WAIT_US - S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), None);
        room.wait(2 * S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), Some(Outcome::Done));
    }

    /// The Reviewer-lead's P2 on #3992: "No, it's Mary." after a name
    /// corrects the name; it is not the face's no.
    #[test]
    fn with_a_face_to_keep_a_no_with_a_name_corrects_it_and_waits() {
        let mut room = Room::new("face-no-name");
        room.walk.face_kept = true;
        assert_eq!(room.wait(7 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        room.walk.heard("You can call me, Marie.", room.at);
        room.walk.heard("No, it's Mary.", room.at);
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), None);
        assert!(room.walk.led.as_ref().is_some_and(|led| led.heard.name));
        room.walk.heard("Yes.", room.at);
        room.wait(5 * S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), None);
        face_saved(&room.walk.file);
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), Some(Outcome::Done));
    }

    /// The Reviewer-lead's P2 on #3992: with a face to keep, a no to
    /// "Marie, right?" doesn't end the step before the face is asked; the
    /// new name, its yes and the save end it, and only the save.
    #[test]
    fn with_a_face_to_keep_a_no_to_the_name_waits_for_a_new_name_and_the_face() {
        let mut room = Room::new("face-name-no");
        room.walk.face_kept = true;
        assert_eq!(room.wait(7 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        room.walk.heard("You can call me, Marie.", room.at);
        room.walk.heard("No.", room.at);
        room.wait(3 * S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), None);
        room.walk.heard("Call me Mary.", room.at);
        room.wait(2 * S, Some(NEAR_MIDDLE));
        room.walk.heard("Yes.", room.at);
        room.wait(5 * S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), None);
        face_saved(&room.walk.file);
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), Some(Outcome::Done));
    }

    /// The Reviewer-lead's P2 on #3992: the wait runs from the latest yes,
    /// so a face saved 12 s after the face's yes, 18 s after the name's,
    /// still ends the step, and nothing ends it before.
    #[test]
    fn with_a_face_to_keep_the_wait_runs_from_the_latest_yes() {
        let mut room = Room::new("face-two-yes");
        room.walk.face_kept = true;
        assert_eq!(room.wait(7 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        room.walk.heard("You can call me, Marie.", room.at);
        room.walk.heard("Yes.", room.at);
        room.wait(6 * S, Some(NEAR_MIDDLE));
        room.walk.heard("Sure.", room.at);
        room.wait(12 * S - S / 4, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), None);
        face_saved(&room.walk.file);
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), Some(Outcome::Done));
        let records = room.walk.take_records();
        assert!(
            records
                .iter()
                .any(|line| line["kind"] == "onboarding_face_saved"),
            "{records:?}"
        );
    }

    /// The first builder on #2526: with the nudge on, both of the
    /// coordinator's readers hear each line, and "who you are" ends once,
    /// through `heard_line`'s asked-back name and yes, with the name kept;
    /// `heard` leaves it alone.
    #[test]
    fn with_the_nudge_a_heard_name_ends_who_you_are_once_and_is_kept() {
        let asked = "(If you asked whether you may remember their face and they have not answered yet, wait for their answer. Otherwise ask them, in a few words: \"Did I get that right, Marie?\" Nothing more.)";
        let mut room = at_who("one-path", true);
        room.walk.lamp_said();
        let say = |room: &mut Room, line: &str| {
            room.walk.heard_line(line);
            room.walk.heard(line, room.at);
        };
        say(&mut room, "I'm Marie.");
        assert_eq!(room.wait(3 * S, Some(NEAR_MIDDLE)), [WHO_NUDGE]);
        assert_eq!(room.wait(6 * S, Some(NEAR_MIDDLE)), NONE);
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [asked]);
        assert_eq!(room.outcome(Step::WhoYouAre), None);
        say(&mut room, "Yes.");
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::WhoYouAre), Some(Outcome::Done));
        let records = room.walk.take_records();
        let of_who = |kind: &str| -> Vec<&Value> {
            records
                .iter()
                .filter(|line| line["kind"] == kind && line["step"] == "who_you_are")
                .filter(|line| kind != "onboarding_step" || line["outcome"].is_string())
                .collect()
        };
        assert_eq!(of_who("onboarding_step").len(), 1, "{records:?}");
        assert!(of_who("onboarding_heard").is_empty(), "{records:?}");
        assert_eq!(of_who("onboarding_nudge")[0]["name_confirmed"], true);
    }

    // The first builder's P2 on #2526: none of these is a name, so a yes
    // after it, or the give-up, never ends the step as done.
    #[test]
    fn a_line_that_is_not_a_name_never_ends_who_you_are() {
        let mut room = Room::new("not-a-name");
        assert_eq!(room.wait(7 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        for line in [
            "I'm not sure",
            "I'm fine",
            "it's okay",
            "I'm here",
            "What?",
            "Sorry?",
            "Hmm",
            "Hey Lamp",
            "Coffee?",
            "I'm not telling",
        ] {
            room.walk.heard(line, room.at);
            room.walk.heard("Yes, that's fine.", room.at);
            room.wait(S, Some(NEAR_MIDDLE));
            assert_eq!(room.outcome(Step::WhoYouAre), None, "{line}");
        }
        // A name taken back by a no is not known either.
        room.walk.heard("Call me Marie.", room.at);
        room.walk.heard("No.", room.at);
        // Its conversation over, asked twice more (#3754), then failed.
        assert_eq!(room.wait(95 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        assert_eq!(room.wait(95 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        assert_eq!(room.outcome(Step::WhoYouAre), None);
        assert_eq!(room.wait(95 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        assert_eq!(room.outcome(Step::WhoYouAre), Some(Outcome::Failed));
    }

    #[test]
    fn both_preferences_heard_end_the_step_with_no_call() {
        let mut room = Room::new("heard-preferences");
        assert_eq!(room.wait(7 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        report(&room.walk, "who_you_are", "done");
        assert_eq!(room.wait(5 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        // A misheard line answers nothing.
        room.walk.heard("Who's the flight doctor?", room.at);
        room.walk
            .heard("Please stay quiet after ten at night.", room.at);
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::Preferences), None);
        // A greeting or an opener, as on lamp-52e6 on 27 Oct, answers
        // nothing (reviewer-three's lines on #3673).
        for greeting in [
            "Hello, Lamp.",
            "Hi there.",
            "Good morning, Lamp.",
            "Hey.",
            "Hi, how are you?",
            "Hello, my name is Sam.",
            "Hi Lamp, I'm back.",
            "Hello! What can you do?",
            "Hey, good to see you.",
            "Hi, I'm fine.",
        ] {
            room.walk.heard(greeting, room.at);
            room.wait(S, Some(NEAR_MIDDLE));
            assert_eq!(room.outcome(Step::Preferences), None, "{greeting}");
        }
        room.walk.heard("That's fine.", room.at);
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::Preferences), Some(Outcome::Done));
    }

    /// The hello answered before the quiet hours: a line about hello with
    /// a yes or a no is the answer, whichever order they come in.
    #[test]
    fn a_hello_answer_needs_a_yes_or_a_no_in_it() {
        for (n, answer) in [
            "Yes, say hello when I come in.",
            "Please don't greet me.",
            "No hellos please.",
            "Sure, you can say hi.",
        ]
        .into_iter()
        .enumerate()
        {
            let mut room = Room::new(&format!("hello-first-{n}"));
            assert_eq!(room.wait(7 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
            assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
            report(&room.walk, "who_you_are", "done");
            assert_eq!(room.wait(5 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
            room.walk.heard(answer, room.at);
            room.wait(S, Some(NEAR_MIDDLE));
            assert_eq!(
                room.outcome(Step::Preferences),
                None,
                "{answer}: quiet hours still owed"
            );
            room.walk.heard("Quiet after ten at night.", room.at);
            room.wait(S, Some(NEAR_MIDDLE));
            assert_eq!(
                room.outcome(Step::Preferences),
                Some(Outcome::Done),
                "{answer}"
            );
        }
    }

    /// "Good evening, Lamp." before the quiet hours is a greeting: the step
    /// still owes them after a hello answer (Fourteen's probe on #3673).
    #[test]
    fn a_good_evening_is_not_the_quiet_hours() {
        for (n, opener) in [
            "Good evening, Lamp.",
            "Good morning!",
            "Good afternoon.",
            "Good night, Lamp.",
        ]
        .into_iter()
        .enumerate()
        {
            let mut room = Room::new(&format!("good-evening-{n}"));
            assert_eq!(room.wait(7 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
            assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
            report(&room.walk, "who_you_are", "done");
            assert_eq!(room.wait(5 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
            room.walk.heard(opener, room.at);
            room.walk.heard("Yes, you can say hello.", room.at);
            room.wait(S, Some(NEAR_MIDDLE));
            assert_eq!(
                room.outcome(Step::Preferences),
                None,
                "{opener}: quiet hours still owed"
            );
        }
        // A part of the day as an hour still answers.
        for (n, hours) in ["Quiet in the evening.", "After ten at night."]
            .into_iter()
            .enumerate()
        {
            let mut room = Room::new(&format!("evening-hours-{n}"));
            assert_eq!(room.wait(7 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
            assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
            report(&room.walk, "who_you_are", "done");
            assert_eq!(room.wait(5 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
            room.walk.heard("Good evening, Lamp.", room.at);
            room.walk.heard(hours, room.at);
            room.walk.heard("Yes, you can say hello.", room.at);
            room.wait(S, Some(NEAR_MIDDLE));
            assert_eq!(
                room.outcome(Step::Preferences),
                Some(Outcome::Done),
                "{hours}"
            );
        }
    }

    #[test]
    fn a_step_whose_conversation_ended_is_asked_again_not_failed() {
        // lamp-29e7, 27 Oct: `preferences: failed` at 14:22:20, 100 s after
        // the last answered line, the conversation under it over (#3754).
        let mut room = Room::new("conversation-ended");
        assert_eq!(room.wait(7 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.wait(20 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        report(&room.walk, "who_you_are", "done");
        assert_eq!(room.wait(5 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        room.walk
            .heard("Please stay quiet after ten at night.", room.at);
        // Nothing more: the conversation ends, the step's time runs out.
        assert_eq!(room.wait(95 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        assert_eq!(room.outcome(Step::Preferences), None, "not failed");
        assert!(
            room.walk
                .take_records()
                .iter()
                .any(|row| row["why"] == "no conversation" && row["asked_again"] == true)
        );
        // Asked again, it ends on the person's answers.
        room.walk
            .heard("Please stay quiet after ten at night.", room.at);
        room.walk
            .heard("Yes, you can say hello in the morning.", room.at);
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::Preferences), Some(Outcome::Done));
    }

    #[test]
    fn a_turn_gone_before_its_first_audio_is_no_ask_and_a_redo_forgets_the_count() {
        // The route failed, the service stopped or a privacy gap came
        // before any of the invitation played: never said (#3754).
        assert_eq!(Delivery::unsettled(false), Delivery::Unsaid);
        assert_eq!(Delivery::unsettled(true), Delivery::Cut);
        let mut room = Room::new("unsaid-redo");
        room.walk.privacy_flow.unsaid = MOST_UNSAID;
        room.walk.privacy_flow.failures = 1;
        room.walk.apply_mailbox("{\"redo\":true}\n", room.at);
        assert_eq!(room.walk.privacy_flow.unsaid, 0);
        assert_eq!(room.walk.privacy_flow.failures, 0);
    }

    #[test]
    fn where_you_sit_guides_only_once() {
        let mut room = Room::new("once");
        room.still = false;
        assert_eq!(room.wait(55 * S, Some(SIDE)), [SIT_NOTE, SIT_TURN]);
        assert_eq!(room.outcome(Step::WhereYouSit), None);
    }

    #[test]
    fn a_failing_step_says_what_to_do_and_moves_on() {
        let mut room = Room::new("fail");
        let far = Framing {
            size: 0.05,
            x: 0.0,
            y: 0.0,
        };
        // A face that never holds still: one guide, then given up on.
        room.still = false;
        assert_eq!(
            room.wait(64 * S, Some(far)),
            [SIT_NOTE, SIT_CLOSER, SIT_FAILED]
        );
        assert_eq!(room.outcome(Step::WhereYouSit), Some(Outcome::Failed));
        room.still = true;
        assert_eq!(room.walk.kept().steps["where_you_sit"].framing, Some(far));
        assert_eq!(room.wait(4 * S, Some(far)), [WHO_NOTE]);
        // Never reported: asked twice more once its conversation is over
        // (#3754), then failed after ninety seconds and a quiet room.
        assert_eq!(room.wait(95 * S, Some(far)), [WHO_NOTE]);
        assert_eq!(room.wait(95 * S, Some(far)), [WHO_NOTE]);
        assert_eq!(room.wait(95 * S, Some(far)), [PREFERENCES_NOTE]);
        assert_eq!(room.outcome(Step::WhoYouAre), Some(Outcome::Failed));
    }

    #[test]
    fn the_pause_and_an_empty_room_stop_the_step_being_led() {
        let mut room = Room::new("pause");
        assert_eq!(room.wait(4 * S, Some(SIDE)), [SIT_NOTE]);
        assert!(room.walk.leading());
        // Paused: nothing said, the step dropped and asked again after.
        for at in 0..40 {
            assert_eq!(
                room.walk
                    .tick(4 * S + at * S, 60 * S, true, true, Some(SIDE), true),
                None
            );
        }
        assert!(!room.walk.leading());
        assert!(room.walk.kept().steps.is_empty());
        room.at = 44 * S;
        assert_eq!(room.wait(4 * S, Some(SIDE)), [SIT_NOTE]);
        // They leave: after half a minute the step is dropped, and asked
        // again when they are back.
        room.wait(31 * S, None);
        assert!(!room.walk.leading());
        assert_eq!(room.wait(4 * S, Some(SIDE)), [SIT_NOTE]);
        // Not at rest: nothing is begun.
        let mut busy = Onboarding::at(file("busy"));
        for at in 0..20 {
            assert_eq!(
                busy.tick(at * S, 60 * S, false, false, Some(SIDE), true),
                None
            );
        }
    }

    /// A walk led to "touch", with Controls' touches on a bus of its own.
    fn at_touch(name: &str) -> (Room, lamp_bus::Publisher, PathBuf) {
        use lamp_msgs::TouchEvent;
        // Short, for the sockets' path limit.
        // Not the reminders tests' `/tmp/lvo-`: the crate's unit tests are one process.
        let root = PathBuf::from(format!("/tmp/lvon-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut room = Room::new(name);
        room.walk.touches = Some(
            bus.subscribe_typed(TouchEvent::TOPIC, SUBSCRIBER, TouchEvent::TYPE)
                .unwrap(),
        );
        let says = bus
            .publisher_declared(TouchEvent::TOPIC, &TouchEvent::declaration(), "controls")
            .unwrap();
        assert_eq!(room.wait(8 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.walk.take_gesture(), None);
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        report(&room.walk, "who_you_are", "done");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [TOUCH_NOTE]);
        assert_eq!(room.walk.take_gesture(), None);
        (room, says, root)
    }

    /// Ticks at the same moment until "touch" has an outcome, or for up to
    /// two seconds while the bus delivers.
    fn until_touch_ends(room: &mut Room) {
        let since = std::time::Instant::now();
        while room.outcome(Step::Touch).is_none()
            && since.elapsed() < std::time::Duration::from_secs(2)
        {
            room.walk.drain_storage_for_test(room.at);
            room.walk.tick(
                room.at,
                room.at - room.said,
                true,
                false,
                Some(NEAR_MIDDLE),
                true,
            );
            room.walk.drain_storage_for_test(room.at);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    #[test]
    fn a_touch_on_the_head_after_the_ask_is_done() {
        let (mut room, mut says, root) = at_touch("touched");
        let (_, touches) = crate::hand_controls::test_head_touch("boot", lamp_ipc::monotonic_us());
        assert!(!touches.is_empty());
        for (at, touch) in touches.iter().enumerate() {
            says.publish_json(at as u64 + 1, touch).unwrap();
        }
        until_touch_ends(&mut room);
        assert_eq!(room.outcome(Step::Touch), Some(Outcome::Done));
        // Spoken, never a nod (the owner's #841 decision).
        assert_eq!(room.walk.take_gesture(), None);
        // That it worked is said before the next step is asked (#3769).
        assert_eq!(
            room.wait(8 * S, Some(NEAR_MIDDLE)),
            [TOUCH_DONE, PREFERENCES_NOTE]
        );
        // Only the outcome is kept.
        assert_eq!(room.walk.kept().steps["touch"].framing, None);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A tap is one event, read on one tick (#3756: 3e83 and 29e7, where
    /// taps never counted and only a hand left resting did). A tick that
    /// reads it and then returns before the touch step is looked at, here
    /// with a note owed, as it does too while storage is busy or a privacy
    /// receipt is read, still ends the step on it.
    #[test]
    fn a_tap_read_on_a_tick_that_returns_early_still_ends_touch() {
        let (mut room, mut says, root) = at_touch("tap-early");
        let (_, touches) = crate::hand_controls::test_head_touch("boot", lamp_ipc::monotonic_us());
        for (at, touch) in touches.iter().enumerate() {
            says.publish_json(at as u64 + 1, touch).unwrap();
        }
        // The bus delivers, then one tick reads every touch and returns
        // with the note it owed.
        std::thread::sleep(std::time::Duration::from_millis(50));
        room.walk.drain_storage_for_test(room.at);
        room.walk.owed = Some("owed".into());
        assert_eq!(
            room.walk
                .tick(
                    room.at,
                    room.at - room.said,
                    true,
                    false,
                    Some(NEAR_MIDDLE),
                    true
                )
                .as_deref(),
            Some("owed")
        );
        assert_eq!(room.outcome(Step::Touch), None);
        // Nothing more is felt; the next ticks end the step on that tap.
        until_touch_ends(&mut room);
        assert_eq!(room.outcome(Step::Touch), Some(Outcome::Done));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A touch felt before "touch" is asked never ends it (#3756's latch
    /// holds only what came after the ask).
    #[test]
    fn a_touch_before_the_touch_step_is_asked_is_not_kept() {
        let (mut room, mut says, root) = at_touch("tap-before");
        // Back before the ask: the step is led but not yet asked.
        room.walk.led.as_mut().unwrap().asked_us = None;
        let (_, touches) = crate::hand_controls::test_head_touch("boot", lamp_ipc::monotonic_us());
        for (at, touch) in touches.iter().enumerate() {
            says.publish_json(at as u64 + 1, touch).unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        room.walk.drain_storage_for_test(room.at);
        room.walk.owed = Some("owed".into());
        room.walk.tick(
            room.at,
            room.at - room.said,
            true,
            false,
            Some(NEAR_MIDDLE),
            true,
        );
        assert!(!room.walk.led.as_ref().is_some_and(|led| led.touched));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// With the onboarding file an older updater's `install` writes (before
    /// #3769), a touch still ends the step and the walk goes on, as before:
    /// no line of its own (Reviewer-two's P2 on #3781).
    #[test]
    fn with_the_file_from_before_3769_a_touch_still_ends_the_step() {
        let (mut room, mut says, root) = at_touch("touched-before-3769");
        room.walk.definition = Arc::new(skill::parse(skill::tests::BEFORE_3769).unwrap());
        let (_, touches) = crate::hand_controls::test_head_touch("boot", lamp_ipc::monotonic_us());
        for (at, touch) in touches.iter().enumerate() {
            says.publish_json(at as u64 + 1, touch).unwrap();
        }
        until_touch_ends(&mut room);
        assert_eq!(room.outcome(Step::Touch), Some(Outcome::Done));
        let next = room
            .walk
            .definition
            .note(Step::Preferences, false)
            .to_owned();
        assert_eq!(room.wait(8 * S, Some(NEAR_MIDDLE)), [next]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn touch_is_done_only_by_what_controls_feels() {
        let (mut room, _says, root) = at_touch("touch-forged");
        let done = ToolCall {
            id: "c".into(),
            name: STEP.into(),
            args: json!({"step": "touch", "outcome": "done"}),
        };
        assert!(run(&room.walk.file, &done).0.get("error").is_some());
        std::fs::write(
            mailbox_of(&room.walk.file),
            "{\"step\":\"touch\",\"outcome\":\"done\"}\n",
        )
        .unwrap();
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::Touch), None);
        // Skipped is theirs to ask. The walk says which step it skipped,
        // from the record, before the next (#3769); the model is told to
        // say nothing of it.
        let skip = ToolCall {
            id: "c".into(),
            name: STEP.into(),
            args: json!({"step": "touch", "outcome": "skipped"}),
        };
        let (answer, _) = run(&room.walk.file, &skip);
        assert!(
            answer["note"]
                .as_str()
                .unwrap()
                .contains("Say nothing about it")
        );
        assert_eq!(
            room.wait(8 * S, Some(NEAR_MIDDLE)),
            [TOUCH_SKIPPED, PREFERENCES_NOTE]
        );
        assert_eq!(room.outcome(Step::Touch), Some(Outcome::Skipped));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn no_touch_is_reminded_once_then_failed() {
        let (mut room, _says, root) = at_touch("untouched");
        assert_eq!(
            room.wait(61 * S, Some(NEAR_MIDDLE)),
            [TOUCH_REMIND, TOUCH_FAILED, PREFERENCES_NOTE]
        );
        assert_eq!(room.outcome(Step::Touch), Some(Outcome::Failed));
        assert_eq!(room.walk.take_gesture(), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn touch_is_not_available_without_the_bus() {
        let room = walked("touch-no-bus");
        assert_eq!(room.outcome(Step::Touch), Some(Outcome::NotAvailable));
    }

    fn room_sounds_file(room: &Room) -> PathBuf {
        room.walk.file.parent().unwrap().join("room-sounds.json")
    }

    /// A no-slide lamp's walk up to the room sounds question.
    fn asked_room_sounds(name: &str) -> Room {
        let mut room = Room::new(name);
        room.walk.no_slide = true;
        assert_eq!(room.wait(8 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        report(&room.walk, "who_you_are", "done");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [ROOM_SOUNDS_NOTE]);
        room
    }

    #[test]
    fn room_sounds_is_offered_only_on_a_lamp_with_no_slide() {
        assert!(no_slide(Some("no-slide")));
        for value in [None, Some("on"), Some("off"), Some(""), Some("no-slide ")] {
            assert!(!no_slide(value), "{value:?}");
        }
        // A slide lamp, or the switch unset: passed over unspoken.
        let room = walked("room-sounds-slide");
        assert_eq!(room.outcome(Step::RoomSounds), Some(Outcome::NotAvailable));
        assert!(!room_sounds_file(&room).exists());
        assert!(
            ROOM_SOUNDS_NOTE
                .contains("May I notice music and laughter in the room? Nothing is recorded.")
        );
    }

    #[test]
    fn a_yes_to_room_sounds_is_kept_once_and_redo_wipes_it() {
        use std::os::unix::fs::PermissionsExt;
        let mut room = asked_room_sounds("room-sounds-yes");
        report(&room.walk, "room_sounds", "done");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        let path = room_sounds_file(&room);
        let kept: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let at = room.walk.kept().steps["room_sounds"].at_unix_s;
        assert_eq!(
            kept,
            json!({"version":1,"room_sounds":"yes","at_unix_s":at})
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        report(&room.walk, "preferences", "done");
        room.wait(4 * S, Some(NEAR_MIDDLE));
        // A walk rebuilt (a restart, a mute) does not ask again.
        let mut again = Room {
            walk: Onboarding::at(room.walk.file.clone()),
            at: 0,
            said: 0,
            still: true,
            body_on_bus: false,
        };
        again.walk.no_slide = true;
        assert_eq!(again.wait(60 * S, Some(NEAR_MIDDLE)), NONE);
        assert!(path.exists());
        // "Let's redo the setup": the yes goes with every other outcome.
        let redo = ToolCall {
            id: "c".into(),
            name: REDO.into(),
            args: json!({}),
        };
        assert!(run(&again.walk.file, &redo).1);
        assert_eq!(again.wait(4 * S, Some(SIDE)), [SIT_NOTE]);
        assert!(again.walk.kept().steps.is_empty());
        assert!(!path.exists());
    }

    /// Roll-back safety (#2110): a progress file with a step a later build
    /// added loads here; the known steps are intact, the walk does not ask
    /// them again, and the unknown one is kept as it was.
    #[test]
    fn a_step_this_build_does_not_know_is_read_past_and_kept() {
        let mut room = asked_room_sounds("unknown-step");
        report(&room.walk, "room_sounds", "skipped");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        report(&room.walk, "preferences", "done");
        room.wait(4 * S, Some(NEAR_MIDDLE));
        let file = room.walk.file.clone();
        let mut written: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        written["steps"]["a_later_step"] = json!({"outcome":"done","at_unix_s":9});
        std::fs::write(&file, serde_json::to_vec(&written).unwrap()).unwrap();
        let known = room.walk.kept().steps.clone();
        let mut again = Room {
            walk: Onboarding::at(file.clone()),
            at: 0,
            said: 0,
            still: true,
            body_on_bus: false,
        };
        again.walk.no_slide = true;
        assert_eq!(again.wait(60 * S, Some(NEAR_MIDDLE)), NONE);
        let loaded = again.walk.kept().steps.clone();
        for (name, kept) in &known {
            assert_eq!(loaded.get(name), Some(kept), "{name}");
        }
        assert_eq!(loaded["a_later_step"].outcome, Outcome::Done);
        assert_eq!(loaded.len(), known.len() + 1);
    }

    #[test]
    fn no_or_silence_to_room_sounds_leaves_it_off_and_is_not_asked_again() {
        // No: skipped, kept as an outcome only.
        let mut room = asked_room_sounds("room-sounds-no");
        report(&room.walk, "room_sounds", "skipped");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        assert_eq!(room.outcome(Step::RoomSounds), Some(Outcome::Skipped));
        assert!(!room_sounds_file(&room).exists());
        // Silence: failed after ninety seconds and a quiet room.
        let mut room = asked_room_sounds("room-sounds-silence");
        assert_eq!(room.wait(95 * S, Some(NEAR_MIDDLE)), [ROOM_SOUNDS_NOTE]);
        assert_eq!(room.wait(95 * S, Some(NEAR_MIDDLE)), [ROOM_SOUNDS_NOTE]);
        assert_eq!(room.wait(95 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        assert_eq!(room.outcome(Step::RoomSounds), Some(Outcome::Failed));
        assert!(!room_sounds_file(&room).exists());
        report(&room.walk, "preferences", "done");
        room.wait(4 * S, Some(NEAR_MIDDLE));
        let mut again = Room {
            walk: Onboarding::at(room.walk.file.clone()),
            at: 0,
            said: 0,
            still: true,
            body_on_bus: false,
        };
        again.walk.no_slide = true;
        assert_eq!(again.wait(60 * S, Some(NEAR_MIDDLE)), NONE);
        assert!(!room_sounds_file(&again).exists());
    }

    fn face_expressions_file(room: &Room) -> PathBuf {
        face_expressions_beside(&room_sounds_file(room))
    }

    /// A walk on a lamp whose eyes read faces, up to the face question.
    fn asked_face_expressions(name: &str) -> Room {
        let mut room = Room::new(name);
        room.walk.face_ask = true;
        assert_eq!(room.wait(8 * S, Some(NEAR_MIDDLE)), [SIT_NOTE, SIT_DONE]);
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        report(&room.walk, "who_you_are", "done");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [FACE_EXPRESSIONS_NOTE]);
        room
    }

    #[test]
    fn face_expressions_is_offered_only_with_its_switch_and_kept_where_the_eyes_read() {
        assert_eq!(
            face_expressions_beside(Path::new(ROOM_SOUNDS_PATH)),
            Path::new(FACE_EXPRESSIONS_PATH)
        );
        let room = walked("face-expressions-off");
        assert_eq!(
            room.outcome(Step::FaceExpressions),
            Some(Outcome::NotAvailable)
        );
        assert!(!face_expressions_file(&room).exists());
        assert!(FACE_EXPRESSIONS_NOTE.contains(
            "A yes covers everyone at this lamp. I never say it aloud and keep no pictures."
        ));
    }

    #[test]
    fn a_yes_to_face_expressions_is_kept_once_and_redo_wipes_it() {
        let mut room = asked_face_expressions("face-expressions-yes");
        report(&room.walk, "face_expressions", "done");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        let path = face_expressions_file(&room);
        let at = room.walk.kept().steps["face_expressions"].at_unix_s;
        // Exactly what lamp-eyes' `face_expressions_yes` reads as a yes.
        assert_eq!(
            std::fs::read(&path).unwrap(),
            format!(r#"{{"at_unix_s":{at},"face_expressions":"yes","version":1}}"#).as_bytes()
        );
        assert!(!room_sounds_file(&room).exists());
        report(&room.walk, "preferences", "done");
        room.wait(4 * S, Some(NEAR_MIDDLE));
        let mut again = Room {
            walk: Onboarding::at(room.walk.file.clone()),
            at: 0,
            said: 0,
            still: true,
            body_on_bus: false,
        };
        again.walk.face_ask = true;
        assert_eq!(again.wait(60 * S, Some(NEAR_MIDDLE)), NONE);
        assert!(path.exists());
        let redo = ToolCall {
            id: "c".into(),
            name: REDO.into(),
            args: json!({}),
        };
        assert!(run(&again.walk.file, &redo).1);
        assert_eq!(again.wait(4 * S, Some(SIDE)), [SIT_NOTE]);
        assert!(!path.exists());
    }

    #[test]
    fn no_or_silence_to_face_expressions_leaves_it_off_and_is_not_asked_again() {
        let mut room = asked_face_expressions("face-expressions-no");
        report(&room.walk, "face_expressions", "skipped");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        assert_eq!(room.outcome(Step::FaceExpressions), Some(Outcome::Skipped));
        assert!(!face_expressions_file(&room).exists());
        let mut room = asked_face_expressions("face-expressions-silence");
        assert_eq!(
            room.wait(95 * S, Some(NEAR_MIDDLE)),
            [FACE_EXPRESSIONS_NOTE]
        );
        assert_eq!(
            room.wait(95 * S, Some(NEAR_MIDDLE)),
            [FACE_EXPRESSIONS_NOTE]
        );
        assert_eq!(room.wait(95 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        assert_eq!(room.outcome(Step::FaceExpressions), Some(Outcome::Failed));
        assert!(!face_expressions_file(&room).exists());
        report(&room.walk, "preferences", "done");
        room.wait(4 * S, Some(NEAR_MIDDLE));
        let mut again = Room {
            walk: Onboarding::at(room.walk.file.clone()),
            at: 0,
            said: 0,
            still: true,
            body_on_bus: false,
        };
        again.walk.face_ask = true;
        assert_eq!(again.wait(60 * S, Some(NEAR_MIDDLE)), NONE);
        assert!(!face_expressions_file(&again).exists());
    }

    #[test]
    fn the_nearest_face_is_its_framing() {
        let face = |xmin: f32, ymin: f32, side: f32| lamp_msgs::Face {
            speaking: None,
            id: "f".into(),
            xmin,
            ymin,
            xmax: xmin + side,
            ymax: ymin + side,
            confidence: 1.0,
            yaw: None,
            facing: None,
        };
        let faces = lamp_msgs::Faces {
            faces: vec![face(0.0, 0.0, 0.1), face(0.4, 0.4, 0.2)],
        };
        let framing = nearest(&faces).unwrap();
        assert!((framing.size - 0.2).abs() < 1e-6);
        assert!(framing.x.abs() < 1e-6 && framing.y.abs() < 1e-6);
        assert_eq!(nearest(&lamp_msgs::Faces { faces: vec![] }), None);
    }
}

#[cfg(test)]
mod giving_way_flow_tests;
#[cfg(test)]
mod near_flow_tests;
#[cfg(test)]
mod privacy_flow_tests;
