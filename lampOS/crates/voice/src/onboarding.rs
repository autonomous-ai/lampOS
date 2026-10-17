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
//! measure. The steps other builders own are placeholders: recorded as not
//! available, not spoken, and passed over.
//!
//! Off unless `LAMP_ONBOARDING=on` and `LAMP_ONBOARDING_FILE` names an
//! absolute path: then nothing here runs, no function is declared and no
//! file is read or written.
use lamp_gemini::{Tool, ToolCall};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Write,
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
/// A word on how to move the lamp at most this often, after this much quiet.
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
    /// is passed over until its part can be measured from here: hearing
    /// (fourth, #753), head touch (third, #711), the privacy switch (third,
    /// #795), and whether the body gives way (fifth, #780).
    fn available(self) -> bool {
        matches!(
            self,
            Step::WhereYouSit | Step::WhoYouAre | Step::Preferences
        )
    }

    /// What the model may report of this step: a measured step only that
    /// it was skipped.
    fn reportable(self, outcome: Outcome) -> bool {
        match self {
            Step::WhereYouSit => outcome == Outcome::Skipped,
            _ => outcome != Outcome::NotAvailable,
        }
    }

    /// What the model is given to begin the step.
    fn note(self) -> &'static str {
        match self {
            Step::WhereYouSit => SIT_NOTE,
            Step::WhoYouAre => WHO_NOTE,
            Step::Preferences => PREFERENCES_NOTE,
            // PLACEHOLDER (#841): each owner words its own step's note,
            // opening with "one more thing I can do now" for the re-offer.
            _ => ONE_MORE_NOTE,
        }
    }

    /// The one move the body is asked for when the step is done (#841,
    /// First's addition 1): a nod when it sees them well, a greeting when
    /// it knows them, a nod for the rest. The body has no lean yet.
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
pub const SIT_CLOSER: &str = "(You can see them, but they are far from you. Ask them in a few words to bring you a bit closer.)";
pub const SIT_TURN: &str = "(You can see them, but at the edge of what you see. Ask them in a few words to turn you a little toward them.)";
pub const SIT_DONE: &str =
    "(Now you can see them well where they sit. Tell them so in a few words.)";
pub const SIT_FAILED: &str = "(You still cannot see them well where they sit. Tell them in a few light words that it is fine: they can turn you toward their seat later, or ask you to redo the setup.)";
pub const WHO_NOTE: &str = "(Nobody has spoken. Ask them, in a few words, what you should call them. When they have told you, ask whether you may remember their face, and only after a clear yes call remember_my_face with their name. Then call setup_step with step \"who_you_are\": done, skipped if they would rather not, or failed if it could not be done, and then tell them in a few words what they can try later.)";
pub const PREFERENCES_NOTE: &str = "(Nobody has spoken. Ask them two short things, one at a time: whether there are hours when you should keep quiet, and whether you may say hello when they sit down. Keep each answer as a rule if you can. Then call setup_step with step \"preferences\", and tell them in a few words that is everything, and that they can ask you to redo the setup any time.)";

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
}

struct Led {
    step: Step,
    asked_us: Option<u64>,
    held_since: Option<u64>,
    guided_us: Option<u64>,
}

pub struct Onboarding {
    file: PathBuf,
    kept: Progress,
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
        .filter(|path| path.is_absolute())
}

pub fn mailbox_of(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".told");
    PathBuf::from(name)
}

impl Onboarding {
    pub fn from_environment() -> Option<Self> {
        let file = switched_on(
            std::env::var(ENVIRONMENT).ok().as_deref(),
            std::env::var(FILE_ENVIRONMENT).ok().as_deref(),
        )?;
        let mut walk = Self::at(file);
        walk.eyes = lamp_bus::Bus::from_environment().ok();
        Some(walk)
    }

    /// The runner keeping its results in `file`. Unreadable: started over.
    pub fn at(file: PathBuf) -> Self {
        let kept = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            file,
            kept,
            led: None,
            owed: None,
            seen_us: None,
            read_us: None,
            finished_recorded: false,
            records: Vec::new(),
            eyes: None,
            available: Step::available,
            gesture: None,
            looked: None,
            felt: None,
            goals: None,
            body_heard: false,
        }
    }

    pub fn kept(&self) -> &Progress {
        &self.kept
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
        if self
            .read_us
            .is_none_or(|at| now_us.saturating_sub(at) >= READ_EVERY_US)
        {
            self.read_us = Some(now_us);
            self.read_mailbox(now_us);
        }
        if face.is_some() {
            self.seen_us = Some(now_us);
        }
        if paused {
            // The keeper's pause stops the walk; a step being led is asked
            // again afterwards.
            if self.led.take().is_some_and(|led| led.asked_us.is_some()) {
                self.records
                    .push(json!({"kind":"onboarding_stopped","why":"paused","at_us":now_us}));
            }
            return None;
        }
        if let Some(note) = self.owed
            && can_speak
        {
            self.owed = None;
            return Some(note);
        }
        let step = loop {
            let Some(step) = self.next_step() else {
                if !self.finished_recorded && !self.kept.steps.is_empty() {
                    self.finished_recorded = true;
                    self.records
                        .push(json!({"kind":"onboarding_finished","at_us":now_us}));
                }
                return None;
            };
            if (self.available)(step) {
                break step;
            }
            self.finish(step, Outcome::NotAvailable, None, now_us);
        };
        if self.led.as_ref().is_none_or(|led| led.step != step) {
            self.led = Some(Led {
                step,
                asked_us: None,
                held_since: None,
                guided_us: None,
            });
        }
        let gone = self
            .seen_us
            .is_none_or(|seen| now_us.saturating_sub(seen) >= GONE_US);
        let led = self.led.as_mut()?;
        let Some(asked_us) = led.asked_us else {
            if can_speak && face.is_some() && quiet_us >= START_QUIET_US {
                led.asked_us = Some(now_us);
                led.guided_us = Some(now_us);
                let again = self.kept.steps.get_mut(step.name()).map(|kept| {
                    kept.offered_at_unix_s = Some(unix_ms() / 1_000);
                });
                self.records.push(
                    json!({"kind":"onboarding_step","step":step.name(),"state":"asked",
                        "offered_again":again.is_some(),"at_us":now_us}),
                );
                if again.is_some() {
                    self.save();
                }
                return Some(step.note());
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
        // motion starts the three seconds again.
        match face.filter(|face| still && face.good()) {
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
        if can_speak
            && quiet_us >= GUIDE_EVERY_US
            && led
                .guided_us
                .is_none_or(|at| now_us.saturating_sub(at) >= GUIDE_EVERY_US)
        {
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
        Step::ALL
            .into_iter()
            .find(|step| !self.kept.steps.contains_key(step.name()))
            .or_else(|| {
                // One being offered again now is led to its end.
                self.led
                    .as_ref()
                    .filter(|led| led.asked_us.is_some())
                    .map(|led| led.step)
            })
            .or_else(|| {
                Step::ALL.into_iter().find(|step| {
                    (self.available)(*step)
                        && self.kept.steps.get(step.name()).is_some_and(|kept| {
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
        self.gesture.take()
    }

    fn finish(&mut self, step: Step, outcome: Outcome, framing: Option<Framing>, now_us: u64) {
        let at_unix_s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        self.kept.steps.insert(
            step.name().to_owned(),
            Kept {
                outcome,
                at_unix_s,
                framing,
                offered_at_unix_s: None,
            },
        );
        if outcome == Outcome::Done {
            self.gesture = Some(step.gesture());
        }
        if self.led.as_ref().is_some_and(|led| led.step == step) {
            self.led = None;
        }
        self.records.push(
            json!({"kind":"onboarding_step","step":step.name(),"outcome":outcome,
            "framing":framing,"at_us":now_us}),
        );
        self.save();
    }

    fn save(&mut self) {
        let saved = serde_json::to_vec(&self.kept)
            .map_err(std::io::Error::other)
            .and_then(|bytes| write_private(&self.file, &bytes));
        if let Err(error) = saved {
            eprintln!(
                "lamp-voice: the onboarding file {} could not be written: {error}",
                self.file.display()
            );
        }
    }

    /// What the model reported through the provider worker: a step's end,
    /// or that the walk starts again.
    fn read_mailbox(&mut self, now_us: u64) {
        let mailbox = mailbox_of(&self.file);
        let mut taking = mailbox.clone().into_os_string();
        taking.push(".taking");
        let taking = PathBuf::from(taking);
        if std::fs::rename(&mailbox, &taking).is_err() {
            return;
        }
        let lines = std::fs::read_to_string(&taking).unwrap_or_default();
        let _ = std::fs::remove_file(&taking);
        for line in lines.lines() {
            let Ok(told) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if told.get("redo").and_then(Value::as_bool) == Some(true) {
                self.kept = Progress::default();
                self.led = None;
                self.owed = None;
                self.finished_recorded = false;
                self.records
                    .push(json!({"kind":"onboarding_redo","at_us":now_us}));
                self.save();
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
            // "where you sit" is done by what the eyes see, never by a word.
            let led = self
                .led
                .as_ref()
                .is_some_and(|led| led.step == step && led.asked_us.is_some());
            if led && step.reportable(outcome) {
                self.finish(step, outcome, None, now_us);
            } else {
                self.records.push(
                    json!({"kind":"onboarding_report_refused","step":step.name(),
                    "outcome":outcome,"at_us":now_us}),
                );
            }
        }
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut next = path.as_os_str().to_owned();
    next.push(".next");
    let next = PathBuf::from(next);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&next)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&next, path)
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
                          where_you_sit can only be skipped, since you finish it by seeing them."
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
                (Some(Step::WhereYouSit), Some(_)) => {
                    return (
                        json!({"error": "Where they sit is done when you can see them well. You can only skip it, if they ask."}),
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
        .and_then(|mut mailbox| writeln!(mailbox, "{line}"));
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
                if let Some(note) =
                    self.walk
                        .tick(self.at, self.at - self.said, true, false, face, self.still)
                {
                    notes.push(note);
                    self.said = self.at;
                }
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
        assert_eq!(room.wait(9 * S, Some(SIDE)), [SIT_TURN]);
        // Near and in the middle for three seconds: done, said, kept.
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
        assert_eq!(room.walk.take_gesture(), Some("nod"));
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [WHO_NOTE]);
        report(&room.walk, "who_you_are", "done");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), [PREFERENCES_NOTE]);
        assert_eq!(room.walk.take_gesture(), Some("greeting"));
        report(&room.walk, "preferences", "skipped");
        assert_eq!(room.wait(4 * S, Some(NEAR_MIDDLE)), NONE);
        assert_eq!(room.walk.take_gesture(), None);
        room
    }

    #[test]
    fn a_step_done_asks_the_body_for_one_move_and_a_skip_for_none() {
        let mut room = walked("gesture");
        assert_eq!(room.walk.kept().steps.len(), 7);
        assert_eq!(room.walk.take_gesture(), None);
    }

    #[test]
    fn a_step_that_was_not_available_is_offered_once_a_day_when_it_is() {
        let room = walked("offer");
        // The same voice: nothing more is offered.
        let mut same = Room {
            walk: Onboarding::at(room.walk.file.clone()),
            at: 0,
            said: 0,
            still: true,
            body_on_bus: false,
        };
        assert_eq!(same.wait(30 * S, Some(NEAR_MIDDLE)), NONE);
        // A later voice that has hearing: offered, after the walk.
        let mut later = Room {
            walk: Onboarding::at(room.walk.file.clone()),
            at: 0,
            said: 0,
            still: true,
            body_on_bus: false,
        };
        later.walk.available =
            |step| step != Step::Touch && step != Step::Privacy && step != Step::GivingWay;
        assert_eq!(later.wait(4 * S, Some(NEAR_MIDDLE)), [ONE_MORE_NOTE]);
        assert!(later.walk.leading());
        // Led to its end like any step, while it is being led.
        assert_eq!(later.wait(20 * S, Some(NEAR_MIDDLE)), NONE);
        assert!(later.walk.leading());
        // They leave before it ends: not offered again today.
        later.wait(31 * S, None);
        assert!(!later.walk.leading());
        assert_eq!(later.wait(60 * S, Some(NEAR_MIDDLE)), NONE);
        assert_eq!(
            later.outcome(Step::HowYouSound),
            Some(Outcome::NotAvailable)
        );
        // A day later it is offered again, and done it is kept.
        later
            .walk
            .kept
            .steps
            .get_mut("how_you_sound")
            .unwrap()
            .offered_at_unix_s = Some(unix_ms() / 1_000 - OFFER_EVERY_S);
        assert_eq!(later.wait(4 * S, Some(NEAR_MIDDLE)), [ONE_MORE_NOTE]);
        report(&later.walk, "how_you_sound", "done");
        assert_eq!(later.wait(60 * S, Some(NEAR_MIDDLE)), NONE);
        assert_eq!(later.outcome(Step::HowYouSound), Some(Outcome::Done));
        assert_eq!(later.walk.take_gesture(), Some("nod"));
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
    fn a_failing_step_says_what_to_do_and_moves_on() {
        let mut room = Room::new("fail");
        let far = Framing {
            size: 0.05,
            x: 0.0,
            y: 0.0,
        };
        let notes = room.wait(64 * S, Some(far));
        assert_eq!(notes.first(), Some(&SIT_NOTE));
        assert_eq!(notes.last(), Some(&SIT_FAILED));
        assert!(
            notes[1..notes.len() - 1]
                .iter()
                .all(|note| *note == SIT_CLOSER)
        );
        assert_eq!(room.outcome(Step::WhereYouSit), Some(Outcome::Failed));
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
