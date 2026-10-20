//! The first five minutes (`docs/onboarding.md`, #841): the lamp walks its
//! new person through where to sit, their name and face, and a few
//! preferences, one step at a time.
//!
//! The runner here owns the order and the timing; every line is the
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
pub(crate) mod near;
pub(crate) mod privacy;
pub(crate) mod storage;

use crate::wire::PrivacyReceipt;
use lamp_gemini::{Tool, ToolCall};
use lamp_turn_owner::BootId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub const ENVIRONMENT: &str = "LAMP_ONBOARDING";
pub const FILE_ENVIRONMENT: &str = "LAMP_ONBOARDING_FILE";
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
const TIMED_OUT_QUIET_US: u64 = 20_000_000;
/// Nobody in front of the lamp this long: the step being led is dropped and
/// asked again when someone is back.
const GONE_US: u64 = 30_000_000;
/// The mailbox is looked at this often.
const READ_EVERY_US: u64 = 500_000;
/// The near face, as the hello counts it (`speak_up`), and how far from the
/// middle of the picture its centre may be (-1 to 1 across and down).
const NEAR: f32 = 0.14;
const MIDDLE: f32 = 0.25;
/// A look on the bus older than this says nothing of now.
const FRESH_MS: u64 = 700;
/// "Touch": no touch on the head this long after the ask has failed.
const TOUCH_GIVE_UP_US: u64 = 60_000_000;
/// The name the walk reads Controls' touches under.
const SUBSCRIBER: &str = "voice_onboarding";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    WhereYouSit,
    WhoYouAre,
    HowYouSound,
    Touch,
    Privacy,
    GivingWay,
    Preferences,
}

impl Step {
    pub const ALL: [Step; 7] = [
        Step::WhereYouSit,
        Step::WhoYouAre,
        Step::HowYouSound,
        Step::Touch,
        Step::Privacy,
        Step::GivingWay,
        Step::Preferences,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Step::WhereYouSit => "where_you_sit",
            Step::WhoYouAre => "who_you_are",
            Step::HowYouSound => "how_you_sound",
            Step::Touch => "touch",
            Step::Privacy => "privacy",
            Step::GivingWay => "giving_way",
            Step::Preferences => "preferences",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|step| step.name() == name)
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

    /// What the model is given to begin the step, `again` when it is
    /// offered after the walk.
    fn note(self, again: bool) -> &'static str {
        match self {
            Step::WhereYouSit => SIT_NOTE,
            Step::WhoYouAre => WHO_NOTE,
            Step::Touch if again => TOUCH_OFFER,
            Step::Touch => TOUCH_NOTE,
            Step::Preferences => PREFERENCES_NOTE,
            // PLACEHOLDER (#841): each owner words its own step's note,
            // opening with "one more thing I can do now" for the re-offer.
            _ => ONE_MORE_NOTE,
        }
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

pub const ONE_MORE_NOTE: &str = "(Nobody has spoken. Tell them in a few words that there is one more thing you can do now that you could not when you first met, and show them.)";
/// A step not available at the first walk is offered again at most this
/// often, until it has an outcome.
const OFFER_EVERY_S: u64 = 86_400;

pub const SIT_NOTE: &str = "(Nobody has spoken. You are meeting the person you will live with for the first time. Say hello in a few warm words, then ask them to sit where they usually sit and look at you, so you learn where they are. Nothing more.)";
pub const SIT_NO_FACE: &str = "(You cannot see their face. Ask them in a few words to sit where they usually sit and look at you.)";
pub const SIT_CLOSER: &str = "(You can see them, but they are far from you. Ask them in a few words to bring you a bit closer if they like, and that where they are is fine too.)";
pub const SIT_TURN: &str = "(You can see them, but at the edge of what you see. Ask them in a few words to turn you a little toward them if they like, and that where they are is fine too.)";
pub const SIT_DONE: &str =
    "(Now you can see them well where they sit. Tell them so in a few words.)";
pub const SIT_FAILED: &str = "(You still cannot see them well where they sit. Tell them in a few light words that it is fine: they can turn you toward their seat later, or ask you to redo the setup.)";
pub const WHO_NOTE: &str = "(Nobody has spoken. Ask them, in a few words, what you should call them. When they have told you, ask whether you may remember their face, and only after a clear yes call remember_my_face with their name. Then call setup_step with step \"who_you_are\": done, skipped if they would rather not, or failed if it could not be done, and then tell them in a few words what they can try later.)";
pub const TOUCH_NOTE: &str = "(Nobody has spoken. Ask them, in a few words, to touch the top of your head gently, then stay quiet.)";
pub const TOUCH_OFFER: &str = "(Nobody has spoken. Tell them in a few words that there is one more thing you can do now: feel a touch on the top of your head. Ask them to touch it gently, then stay quiet.)";
pub const TOUCH_REMIND: &str = "(You have not felt a touch yet. Ask them once more, in a few words, to touch the top of your head gently, then stay quiet.)";
pub const PREFERENCES_NOTE: &str = "(Nobody has spoken. Ask them two short things, one at a time: whether there are hours when you should keep quiet, and whether you may say hello when they sit down. Keep each answer as a rule if you can. As soon as you have heard the second answer, call setup_step with step \"preferences\" before you say anything else, then tell them in a few words that is everything, and that they can ask you to redo the setup any time.)";

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
        if self.steps.len() > Step::ALL.len()
            || self.privacy_invitation_tries > 2
            || self.giving_way_tries > giving_way::MOST_TRIES
        {
            return Err(invalid());
        }
        for (name, kept) in &self.steps {
            if Step::named(name).is_none() {
                return Err(invalid());
            }
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

struct Led {
    step: Step,
    asked_us: Option<u64>,
    held_since: Option<u64>,
    guided_us: Option<u64>,
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

// Only the voice's own teardown is verified; the eyes close the camera on
// their own permit timeout, which this step does not observe (#1802).
const PRIVACY_DONE: &str = "(Their physical switch closed the microphones, and they opened it again. Briefly confirm that the switch works. Nothing more.)";
const PRIVACY_FAILED: &str = "(The privacy-switch check was not completed. Briefly say they can try it again later by redoing setup. Do not claim the switch was tested.)";

pub struct Onboarding {
    file: PathBuf,
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
    owed: Option<&'static str>,
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
    /// Controls' touches (`controls/touch`), with the bus on: without them
    /// "touch" is not available.
    touches: Option<lamp_bus::Subscriber>,
    privacy_flow: PrivacyFlow,
    near: near::NearFlow,
    giving: giving_way::GivingWayFlow,
    /// Unix milliseconds, for evidence dated by the bus (tests set their own).
    unix_clock: fn() -> u64,
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

pub fn mailbox_of(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".told");
    PathBuf::from(name)
}

impl Onboarding {
    /// The service owns the sole I/O executor across all voice cycles.
    pub(crate) fn from_storage(client: storage::Client) -> Option<Self> {
        let file = switched_on(
            std::env::var(ENVIRONMENT).ok().as_deref(),
            std::env::var(FILE_ENVIRONMENT).ok().as_deref(),
        )?;
        let mut walk = Self::with_storage(file, client);
        walk.eyes = lamp_bus::Bus::from_environment().ok();
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
        Some(walk)
    }

    pub(crate) fn with_storage(file: PathBuf, storage: storage::Client) -> Self {
        Self {
            file,
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
            looked: None,
            felt: None,
            goals: None,
            body_heard: false,
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
    fn drain_storage_for_test(&mut self, now_us: u64) {
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
        self.privacy_open_receipt(now_us)?;
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
        Some(format!(
            "(Say this short invitation completely before they act: the physical privacy switch {location} closes both microphones and camera. Invite them to close it, then open it again. Explain before they close it that you cannot hear or see them while it is closed. Do not claim success or ask a tool to mark privacy done.)"
        ))
    }

    pub(crate) fn bind_privacy_invitation(
        &mut self,
        token: u64,
        voice_boot: BootId,
        turn: u64,
    ) -> bool {
        let Some(offer) = self.privacy_flow.offer.as_mut() else {
            return false;
        };
        if offer.token != token || !offer.claimed || offer.bound.is_some() || turn == 0 {
            return false;
        }
        offer.bound = Some((voice_boot, turn));
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

    /// Called only on the existing attributable whole-turn playback result.
    /// Admission, first audio and model text never arm a physical attempt.
    pub(crate) fn privacy_invitation_finished(
        &mut self,
        voice_boot: BootId,
        turn: u64,
        delivered_whole: bool,
        now_us: u64,
    ) {
        if self.privacy_invitation_binding() != Some((voice_boot, turn)) {
            return;
        }
        if !delivered_whole {
            self.retry_privacy(now_us);
            return;
        }
        let armed = self.privacy_flow.context.as_ref().and_then(|context| {
            self.privacy_open_receipt(now_us).map(|baseline| {
                privacy::Attempt::arm(
                    context.epoch.clone(),
                    privacy::Invitation {
                        voice_boot,
                        turn,
                        delivered_at_us: now_us,
                    },
                    baseline,
                    now_us,
                )
            })
        });
        match armed {
            Some(Ok(attempt)) => {
                self.privacy_flow.attempt = Some(attempt);
                self.privacy_flow.offer = None;
                self.records
                    .push(json!({"kind":"onboarding_privacy_armed","turn":turn,"at_us":now_us}));
            }
            _ => self.retry_privacy(now_us),
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
            // A slide read just before the invitation's end was processed is
            // the person acting early, not a failed check: offer it again.
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
                        self.privacy_flow.pending_result = Some((Outcome::Done, Some(attempt.id())))
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
            self.owed = Some(if outcome == Outcome::Done {
                PRIVACY_DONE
            } else {
                PRIVACY_FAILED
            });
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
        delivered_whole: bool,
        now_us: u64,
    ) {
        self.near_invitation_finished(voice_boot, turn, delivered_whole, now_us);
        self.giving_invitation_finished(voice_boot, turn, delivered_whole, now_us);
        self.privacy_invitation_finished(voice_boot, turn, delivered_whole, now_us);
    }

    pub fn kept(&self) -> &Progress {
        &self.committed
    }

    /// Whether the lamp is in the middle of leading a step.
    pub fn leading(&self) -> bool {
        self.owed.is_some() || self.led.as_ref().is_some_and(|led| led.asked_us.is_some())
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
    ) -> Option<&'static str> {
        self.poll_storage(now_us);
        // Read every tick, so what came before the ask is never taken.
        let touched = self.head_touched();
        if face.is_some() {
            self.seen_us = Some(now_us);
        }
        if paused {
            self.invalidate_privacy(privacy::Invalidation::FenceFailure);
            self.invalidate_near();
            // Giving way under the pause is not available: offered later.
            self.giving_way_paused(now_us);
            // The keeper's pause stops the walk; a step being led is asked
            // again afterwards.
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
        if let Some(note) = self.owed
            && can_speak
        {
            self.owed = None;
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
                asked_us: None,
                held_since: None,
                guided_us: None,
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
            if can_speak && face.is_some() && quiet_us >= START_QUIET_US {
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
                    self.owed = Some(step.note(true));
                    return None;
                }
                return Some(step.note(false));
            }
            return None;
        };
        if gone {
            self.led = None;
            self.records.push(
                json!({"kind":"onboarding_stopped","why":"nobody_there","step":step.name(),"at_us":now_us}),
            );
            return None;
        }
        if step == Step::Touch {
            // Done only by a touch Controls felt; one reminder, then failed.
            if touched {
                self.finish(step, Outcome::Done, None, now_us);
            } else if now_us.saturating_sub(asked_us) >= TOUCH_GIVE_UP_US {
                self.finish(step, Outcome::Failed, None, now_us);
            } else if can_speak && quiet_us >= GUIDE_EVERY_US && led.guided_us == Some(asked_us) {
                led.guided_us = Some(now_us);
                self.records
                    .push(json!({"kind":"onboarding_guide","step":step.name(),"at_us":now_us}));
                return Some(TOUCH_REMIND);
            }
            return None;
        }
        if step != Step::WhereYouSit {
            if now_us.saturating_sub(asked_us) >= STEP_GIVE_UP_US && quiet_us >= TIMED_OUT_QUIET_US
            {
                self.finish(step, Outcome::Failed, None, now_us);
            }
            return None;
        }
        // Where you sit: what the eyes see ends it, and only while the head
        // is still and no hand is on the lamp: a lamp turning toward them,
        // or being turned, can pass through a good framing mid-move. Any
        // motion starts the three seconds again. After the one guide, any
        // steady face is taken where it is: a person beside the lamp would
        // just say "there's fine" (row 10 on lamp-52e6).
        let settled = led.guided_us.is_some_and(|at| at > asked_us);
        match face.filter(|face| still && (face.good() || settled)) {
            Some(framing) => {
                let since = *led.held_since.get_or_insert(now_us);
                if now_us.saturating_sub(since) >= HOLD_US {
                    self.finish(step, Outcome::Done, Some(framing), now_us);
                    self.owed = Some(SIT_DONE);
                }
                return None;
            }
            None => led.held_since = None,
        }
        if now_us.saturating_sub(asked_us) >= SIT_GIVE_UP_US {
            self.finish(step, Outcome::Failed, face, now_us);
            self.owed = Some(SIT_FAILED);
            return None;
        }
        if can_speak && quiet_us >= GUIDE_EVERY_US && led.guided_us == Some(asked_us) {
            led.guided_us = Some(now_us);
            let note = match face {
                None => SIT_NO_FACE,
                Some(face) if face.size < NEAR => SIT_CLOSER,
                Some(_) => SIT_TURN,
            };
            self.records.push(
                json!({"kind":"onboarding_guide","step":step.name(),"face":face,"at_us":now_us}),
            );
            return Some(note);
        }
        None
    }

    /// The first step with no outcome; once every step has one, a step that
    /// was not available then and is now, offered at most once a day and
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
        Step::ALL
            .into_iter()
            .find(|step| !self.desired.steps.contains_key(step.name()))
            .or_else(|| {
                // One being offered again now is led to its end.
                self.led
                    .as_ref()
                    .filter(|led| led.asked_us.is_some())
                    .map(|led| led.step)
            })
            .or_else(|| {
                Step::ALL.into_iter().find(|step| {
                    self.step_available(*step)
                        && self.desired.steps.get(step.name()).is_some_and(|kept| {
                            kept.outcome == Outcome::NotAvailable
                                && kept
                                    .offered_at_unix_s
                                    .is_none_or(|at| unix_s.saturating_sub(at) >= OFFER_EVERY_S)
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
            if led && step.reportable(outcome) {
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
            } else {
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
    let names: Vec<&str> = Step::ALL.iter().map(|step| step.name()).collect();
    vec![
        Tool {
            name: STEP.into(),
            description: "Call this during your first meeting with someone, when a step you were asked \
                          to lead has ended: done, skipped because they asked to skip it, or failed. If \
                          they want to skip the rest, call it skipped for the step you are on. Then go on \
                          talking as the step asked. Only for the step you are leading now; \
                          where_you_sit, how_you_sound, touch, privacy and giving_way can only be \
                          skipped; the runner requires physical evidence: seeing them, hearing their \
                          call, feeling their touch, the switch's receipt, the body giving way."
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
/// for the runner, and say what to tell the model.
pub fn run(file: &Path, call: &ToolCall) -> (Value, bool) {
    let line = match call.name.as_str() {
        REDO => json!({"redo": true}),
        STEP => {
            let step = call.args.get("step").and_then(Value::as_str);
            let outcome = call.args.get("outcome").and_then(Value::as_str);
            match (step.and_then(Step::named), outcome.and_then(Outcome::named)) {
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
    let written = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(mailbox_of(file))
        .and_then(|mut mailbox| {
            let line = format!("{line}\n");
            if mailbox.metadata()?.len().saturating_add(line.len() as u64) > 16 * 1024 {
                return Err(io::Error::other("onboarding mailbox is full"));
            }
            mailbox.write_all(line.as_bytes())
        });
    match written {
        Ok(()) => (json!({"ok": true}), true),
        Err(_) => (
            json!({"error": "I could not note that just now. Go on all the same."}),
            false,
        ),
    }
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
        assert_eq!(run(&walk.file, &call), (json!({"ok": true}), true));
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
        fn wait(&mut self, for_us: u64, face: Option<Framing>) -> Vec<&'static str> {
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

        fn outcome(&self, step: Step) -> Option<Outcome> {
            self.walk
                .kept()
                .steps
                .get(step.name())
                .map(|kept| kept.outcome)
        }
    }

    const NONE: [&str; 0] = [];
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
        assert_eq!(room.walk.kept().steps.len(), 7);
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

    #[test]
    fn a_step_is_skipped_and_the_walk_redone() {
        let mut room = Room::new("skip");
        assert_eq!(room.wait(4 * S, Some(SIDE)), [SIT_NOTE]);
        report(&room.walk, "where_you_sit", "skipped");
        assert_eq!(room.wait(4 * S, Some(SIDE)), [WHO_NOTE]);
        assert_eq!(room.outcome(Step::WhereYouSit), Some(Outcome::Skipped));
        report(&room.walk, "who_you_are", "skipped");
        assert_eq!(room.wait(4 * S, Some(SIDE)), [PREFERENCES_NOTE]);
        report(&room.walk, "preferences", "skipped");
        assert_eq!(room.wait(4 * S, Some(SIDE)), NONE);
        assert_eq!(room.walk.kept().steps.len(), 7);
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
        assert_eq!(room.walk.kept().steps.len(), 7);
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
        assert_eq!(notes.first(), Some(&SIT_NOTE));
        assert!(!notes.contains(&SIT_DONE));
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
        // Never reported: failed after ninety seconds and a quiet room.
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
        let root = PathBuf::from(format!("/tmp/lvo-{name}-{}", std::process::id()));
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
        // Only the outcome is kept.
        assert_eq!(room.walk.kept().steps["touch"].framing, None);
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
        // Skipped is theirs to ask.
        report(&room.walk, "touch", "skipped");
        room.wait(S, Some(NEAR_MIDDLE));
        assert_eq!(room.outcome(Step::Touch), Some(Outcome::Skipped));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn no_touch_is_reminded_once_then_failed() {
        let (mut room, _says, root) = at_touch("untouched");
        assert_eq!(
            room.wait(61 * S, Some(NEAR_MIDDLE)),
            [TOUCH_REMIND, PREFERENCES_NOTE]
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
