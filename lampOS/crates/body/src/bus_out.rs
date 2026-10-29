//! What the body says on the lamp's bus (`docs/bus.md`): where every joint
//! is and where it was last asked to be (`body/joint_states`, the type
//! `lamp_msgs::JointState`), and once a second how the body is
//! (`diagnostics`, `lamp_msgs::Diagnostic`; let go for its pose, once per
//! change, [`WAITING_RESEND`]), whether a hand pushes it
//! (`body/hand`), and each gesture that began, ended or was dropped
//! (`body/gesture`, `lamp_msgs::BodyGesture`), as the journal says it.
//!
//! The body's loop reads and writes the servos every 20 ms and must never
//! wait for anything else. So nothing is published from it: it offers a
//! step to a thread of this module's own, without waiting, and a step that
//! thread has no room for is let go.
use crate::{
    body::{Glimpse, Stop, Told},
    gesture::Mood,
};
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{BodyGesture, Diagnostic, GestureState, Hand, JointState, KeyValue, Level};
use std::{
    f64::consts::TAU,
    io,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// Where the body reads whether to publish. `on` only.
pub const ENVIRONMENT: &str = "LAMP_BUS";
/// The name the body publishes under.
pub const SOURCE: &str = crate::ask::SOURCE;
/// The joints, in the order of their servos' numbers, 1 to 5. Every list
/// of a `JointState` from here is in this order.
pub const JOINTS: [&str; 5] = ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"];
/// One step in this many is offered: a step is 20 ms, so 12.5 messages a
/// second, against the ten the topic promises.
pub const EVERY_STEPS: u64 = 4;
/// A step of the body, in seconds.
const STEP_S: f64 = 0.02;
/// The body's report goes with one offered step in this many: every
/// 0.96 s, inside the second the topic promises.
const REPORT_EVERY: u32 = 12;
/// While let go for its pose (#2602) every step is offered (one each
/// `crate::body::WAIT_READ`, 100 ms, or `WAIT_READ_LATE` once it stopped
/// asking), and its report goes once per change, not with each read
/// (#2938, the Reviewer-lead's P3 on #2940): when what it says changes,
/// and the same again only this long after it last went. `lamp-health`
/// takes a part as stale after 5 s without a report (`--stale-ms`'s
/// default, the product's), so 2 s keeps it fresh with room for one lost.
pub const WAITING_RESEND: Duration = Duration::from_secs(2);

/// Whether `glimpse` is passed on: one step in [`EVERY_STEPS`], and every
/// step of a body let go for its pose, which steps only ten times a second.
fn offered(glimpse: &Glimpse) -> bool {
    glimpse.waiting.is_some() || glimpse.step.is_multiple_of(EVERY_STEPS)
}

/// Whether a let-go body's `report` goes out at `now`, `last` being the
/// one that went last and when: it changed, or `WAITING_RESEND` passed.
fn waiting_due(report: &Diagnostic, last: Option<&(Diagnostic, Instant)>, now: Instant) -> bool {
    last.is_none_or(|(sent, at)| {
        sent != report || now.saturating_duration_since(*at) >= WAITING_RESEND
    })
}

/// How long a message of where the joints are means anything. A body that
/// has stopped must not go on being read as where the lamp is.
pub const LASTS: Duration = Duration::from_secs(1);
/// A servo's turn is 4,096 counts, and its middle is the angle nothing.
const COUNTS_A_TURN: f64 = 4096.0;
const MIDDLE: f64 = 2048.0;
/// What a servo reports as its load is in thousandths of all it can exert.
const FULL_LOAD: f64 = 1000.0;
/// A joint this warm is said to be warm. The body ends its run at 62 on a
/// joint it commands and sends the arm down at the same reading; this is
/// far enough under to be told before that.
pub const WARM: u8 = 55;
/// Resistance met is said for this long after it happened.
const RESISTANCE_SAID_FOR: Duration = Duration::from_secs(60);

fn radians(counts: u16) -> f64 {
    (f64::from(counts) - MIDDLE) * TAU / COUNTS_A_TURN
}

/// The five joints of a glimpse in the order of [`JOINTS`]: each one's
/// position and its goal, in counts.
fn in_order(glimpse: &Glimpse) -> ([u16; 5], [u16; 5]) {
    // A glimpse has pitch, roll, yaw, and then shoulder, elbow.
    let [pitch, roll, yaw] = glimpse.positions;
    let [shoulder, elbow] = glimpse.arm_positions;
    let [pitch_goal, roll_goal, yaw_goal] = glimpse.goals;
    let [shoulder_goal, elbow_goal] = glimpse.arm_goals;
    (
        [yaw, shoulder, elbow, roll, pitch],
        [yaw_goal, shoulder_goal, elbow_goal, roll_goal, pitch_goal],
    )
}

/// Where the joints are at this step, as the message.
///
/// Angles are in radians from the middle of each servo's turn, the way
/// its own counts grow. `effort` is not ROS's newton metres: it is the
/// share of its full strength each servo reports, from 0 to 1, without a
/// direction. `velocity` is worked out from where the joints were at
/// `before` (a step's number and its positions in counts), and is left
/// empty when there is no such step.
pub fn joint_state(glimpse: &Glimpse, before: Option<(u64, [u16; 5])>) -> JointState {
    let (positions, goals) = in_order(glimpse);
    let [pitch_load, roll_load, yaw_load] = glimpse.loads;
    let [shoulder_load, elbow_load] = glimpse.arm_loads;
    let loads = [yaw_load, shoulder_load, elbow_load, roll_load, pitch_load];
    let velocity = match before {
        Some((step, was)) if glimpse.step > step => {
            let seconds = (glimpse.step - step) as f64 * STEP_S;
            positions
                .iter()
                .zip(was)
                .map(|(now, was)| (radians(*now) - radians(was)) / seconds)
                .collect()
        }
        _ => Vec::new(),
    };
    JointState {
        name: JOINTS.map(str::to_owned).to_vec(),
        position: positions.map(radians).to_vec(),
        velocity,
        effort: loads.map(|load| f64::from(load) / FULL_LOAD).to_vec(),
        commanded: goals.map(radians).to_vec(),
    }
}

fn mood_word(mood: Mood) -> &'static str {
    match mood {
        Mood::Resting => "resting",
        Mood::Listening => "listening",
        Mood::Thinking => "thinking",
        Mood::Speaking => "speaking",
        Mood::Reading => "reading",
    }
}

fn fact(key: &str, value: impl ToString) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: value.to_string(),
    }
}

/// How the body is at this step, as its report. `resisted` is how many
/// times a joint has met resistance within the last minute.
///
/// It is well unless a joint is warm, or one lately met something and
/// stopped pushing. Neither stops the body: both are what a person would
/// want to know before it does.
pub fn health(glimpse: &Glimpse, resisted: u32) -> Diagnostic {
    // Pitch, roll, yaw, and then shoulder, elbow, as a glimpse has them.
    let warmth = [
        ("head_pitch", glimpse.temperatures[0]),
        ("head_roll", glimpse.temperatures[1]),
        ("base_yaw", glimpse.temperatures[2]),
        ("shoulder", glimpse.arm_temperatures[0]),
        ("elbow", glimpse.arm_temperatures[1]),
    ];
    let (warmest, reading) = warmth
        .into_iter()
        .max_by_key(|(_, reading)| *reading)
        .unwrap_or(("head_pitch", 0));
    // Let go for a pose a hand can set straight (#2602): what it asks is
    // the whole of its word, and the start checks are not behind it.
    if let Some(say) = glimpse.waiting {
        return Diagnostic {
            level: Level::Error,
            name: SOURCE.to_owned(),
            message: say.to_owned(),
            values: vec![
                fact("mood", mood_word(glimpse.mood)),
                fact(
                    lamp_keeper::STATE,
                    lamp_keeper::PartState::Starting.as_str(),
                ),
            ],
        };
    }
    let (level, message) = if reading >= WARM {
        (
            Level::Warn,
            format!("{warmest} is warm: it reads {reading}, and the body stops at 62"),
        )
    } else if let Some(joint) = glimpse.cannot_lift {
        (
            Level::Warn,
            format!(
                "its {} pushed as hard as it may and did not move, in two long rests' tries: the lamp stays folded until it is started again, and the arm may need looking at",
                match joint {
                    "base_pitch" => "shoulder",
                    "elbow_pitch" => "elbow",
                    other => other,
                }
            ),
        )
    } else if glimpse.resting_s > 0 {
        (
            Level::Warn,
            format!(
                "its arm sat down for a strain three times in half an hour: it rests folded and may rise again in {} min",
                glimpse.resting_s.div_ceil(60)
            ),
        )
    } else if resisted > 0 {
        (
            Level::Warn,
            format!(
                "a joint met resistance and stopped pushing, {resisted} time{} in the last minute",
                if resisted == 1 { "" } else { "s" }
            ),
        )
    } else {
        (Level::Ok, String::new())
    };
    Diagnostic {
        level,
        name: SOURCE.to_owned(),
        message,
        values: vec![
            fact("mood", mood_word(glimpse.mood)),
            fact("standing", format!("{:.2}", glimpse.stand)),
            fact("warmest_joint", warmest),
            fact("warmest_reading", reading),
            fact("resisted_last_minute", resisted),
            fact(
                "resisted_since_start",
                glimpse.eased.iter().map(|n| u32::from(*n)).sum::<u32>(),
            ),
            // A step has run: the start checks are behind it.
            fact(lamp_keeper::STATE, lamp_keeper::PartState::Ready.as_str()),
        ],
    }
}

/// The state said in `report` becomes `paused`.
fn said_paused(report: &mut Diagnostic) {
    for value in &mut report.values {
        if value.key == lamp_keeper::STATE {
            value.value = lamp_keeper::PartState::Paused.as_str().to_owned();
        }
    }
}

/// A joint by its servo's number, for a person.
fn joint_word(id: u8) -> &'static str {
    usize::from(id)
        .checked_sub(1)
        .and_then(|index| JOINTS.get(index))
        .copied()
        .unwrap_or("a joint")
}

/// Why the body stopped, as its last report. Only being asked to is not a
/// fault, and even then the lamp no longer moves: a warning.
pub fn stopped(stop: &Stop) -> Diagnostic {
    let (level, message) = match stop {
        Stop::Cancelled => (Level::Warn, "stopped, as it was asked to".to_owned()),
        Stop::Refused { why } => (Level::Error, format!("would not start: {why}")),
        Stop::Load { joint, load } => (
            Level::Error,
            format!(
                "stopped: {} was pushed against (load {load})",
                joint_word(*joint)
            ),
        ),
        Stop::Tracking {
            joint,
            goal,
            position,
        } => (
            Level::Error,
            format!(
                "stopped: {} did not follow (asked {goal}, at {position})",
                joint_word(*joint)
            ),
        ),
        Stop::Status { joint, found, now } => (
            Level::Error,
            format!(
                "stopped: the servo of {} reports a fault (status {found:#04x} became {now:#04x})",
                joint_word(*joint)
            ),
        ),
        Stop::Drift { joint, found, now } => (
            Level::Error,
            format!(
                "stopped: {} was moved by something else (from {found} to {now})",
                joint_word(*joint)
            ),
        ),
        Stop::Temperature { joint, temperature } => (
            Level::Error,
            format!(
                "stopped: {} is too warm (it reads {temperature})",
                joint_word(*joint)
            ),
        ),
        Stop::Bus { .. } => (
            Level::Error,
            "stopped: the servos did not answer as expected".to_owned(),
        ),
        Stop::Window {
            joint,
            goal,
            window: (low, high),
            from,
        } => (
            Level::Error,
            format!(
                "stopped: {} was not asked for {goal}, outside {low}..={high} or too far from {from}",
                joint_word(*joint)
            ),
        ),
        Stop::BadRead {
            joint,
            position,
            from,
        } => (
            Level::Error,
            format!(
                "stopped: {} read {position} three times in a row, not where it can be (last good {from})",
                joint_word(*joint)
            ),
        ),
        Stop::BadTemperature {
            joint,
            temperature,
            from,
        } => (
            Level::Error,
            format!(
                "stopped: {} temperature unreadable (it reads {temperature}, last good {from})",
                joint_word(*joint)
            ),
        ),
        Stop::Overload {
            joint,
            load,
            detail,
        } => (
            Level::Error,
            format!(
                "stopped with its torque off: {} was overloaded (load {load}); {detail}",
                joint_word(*joint)
            ),
        ),
    };
    Diagnostic {
        level,
        name: SOURCE.to_owned(),
        message,
        values: vec![fact(
            lamp_keeper::STATE,
            lamp_keeper::PartState::Stopping.as_str(),
        )],
    }
}

/// What the thread is given.
// A step is what is sent most, every fourth step of the body's loop:
// boxing it to make the rare variants' slots smaller would allocate on that
// loop instead, which must never wait for anything.
#[allow(clippy::large_enum_variant)]
enum Out {
    /// A step, whether the lamp is paused, and when the body saw it.
    Step(Glimpse, bool, Instant),
    Stopped(Diagnostic),
    Gesture(BodyGesture),
}

/// The one drop of a clip already playing (`body.rs`, a joint that could
/// not keep up): it began, so it is said `over`, not `dropped`.
const LET_GO: &str = "dropped: a joint could not keep up with it";

/// One piece of gesture news: the gesture's name, what became of it, and
/// for a drop, why.
pub type GestureNews = (&'static str, GestureState, Option<&'static str>);

/// The same with whom it is told of (#940): whose a gesture dropped or
/// ended was, and who holds the body instead.
pub type ToldNews = (GestureNews, Told);

/// A line when a fold ends, for the journal (the first builder on #705,
/// 2026-10-18 14:05): at rest the step lines come every thirty seconds, so
/// on lamp-52e6 they caught 1 fold in 49. From the first step the arm
/// comes down from standing to the step it is folded: how long, where the
/// shoulder and the elbow were told to go against where they are, the
/// head and base the same, and whether it sat down for a strain since it
/// was last folded: on the way up, standing or on the way down. The body
/// counts a sit-down after the step that shows it, and that fold's first
/// step down already has it, so the count is taken from the last folded
/// step, not from the fold's first (the reviewer on #1518).
#[derive(Default)]
pub struct FoldSaid {
    /// Where the last step had it, 0 folded to 1 standing.
    stand: f32,
    /// The step a fold began on.
    began: Option<u64>,
    /// The retreats counted when it was last folded (none at the start).
    folded_retreats: u16,
}

impl FoldSaid {
    pub fn step(&mut self, glimpse: &Glimpse) -> Option<String> {
        let (was, now) = (self.stand, glimpse.stand);
        self.stand = now;
        if !glimpse.stands {
            return None;
        }
        if now > was {
            // Rising again before it was down: that was no fold.
            self.began = None;
            return None;
        }
        if now < was && self.began.is_none() {
            self.began = Some(glimpse.step.saturating_sub(1));
        }
        if now > 0.0 {
            return None;
        }
        let retreats = glimpse.retreats.saturating_sub(self.folded_retreats);
        self.folded_retreats = glimpse.retreats;
        let began = self.began.take()?;
        Some(format!(
            "lamp-body: step {} folded in {:.1} s; arm goals {:?} ended at {:?}; head and base goals {:?} at {:?}; {}",
            glimpse.step,
            (glimpse.step - began) as f32 * crate::body::TICK.as_secs_f32(),
            glimpse.arm_goals,
            glimpse.arm_positions,
            glimpse.goals,
            glimpse.positions,
            match retreats {
                0 => "nothing retreated since it was last folded".to_owned(),
                1 => "it sat down for a strain once since it was last folded".to_owned(),
                sat => format!("it sat down for a strain {sat} times since it was last folded"),
            }
        ))
    }
}

/// The reads not used so far, for the journal every five minutes (the
/// first builder on #705, 2026-10-18 14:05), not only in the exit report.
#[derive(Default)]
pub struct ReadsSaid {
    said_at: Option<u64>,
}

impl ReadsSaid {
    /// Five minutes of 20 ms steps.
    pub const EVERY_STEPS: u64 = 15_000;

    pub fn step(&mut self, glimpse: &Glimpse) -> Option<String> {
        if self
            .said_at
            .is_some_and(|at| glimpse.step.saturating_sub(at) < Self::EVERY_STEPS)
        {
            return None;
        }
        self.said_at = Some(glimpse.step);
        let reads = crate::Joint::ALL
            .iter()
            .zip(glimpse.bad_reads)
            .map(|(joint, bad)| format!("{} {bad}", joint.name()))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!(
            "lamp-body: step {} bad reads so far: {reads}; retreats so far {}",
            glimpse.step, glimpse.retreats
        ))
    }
}

/// What became of the gestures, step by step, as lamp-body's journal says
/// it and `body/gesture` publishes it, so that the two never differ: each
/// gesture dropped (`glimpse.gesture_fates` saying `dropped: ...`), and the
/// gesture made changing (`glimpse.gesture`): `over` for the one before,
/// if any, then `began` for the new one. At the run's end, `over` for the
/// one it was making ([`GestureSaid::stopped`]).
#[derive(Default)]
pub struct GestureSaid {
    making: Option<&'static str>,
}

impl GestureSaid {
    /// This step's news. Drops first, as the journal says them.
    pub fn step(&mut self, glimpse: &Glimpse) -> Vec<GestureNews> {
        self.step_told(glimpse)
            .into_iter()
            .map(|(news, _)| news)
            .collect()
    }

    /// The same, each with whom it is told of.
    pub fn step_told(&mut self, glimpse: &Glimpse) -> Vec<ToldNews> {
        // The gesture it was making, let go in this step as it played (a
        // joint that could not keep up): it began, so it is `over`, not
        // `dropped`, which says one never began.
        let let_go = self.making.filter(|_| glimpse.gesture != self.making);
        // A gesture ended for a higher ask (#1239, `ended: by WHO`): it did
        // not play out, and says `dropped`, not `over`, so that a skill
        // waiting on it sends nothing more.
        let ended = |name: &str| {
            glimpse
                .gesture_fates
                .iter()
                .any(|(ended, why)| ended == name && why.starts_with("ended"))
        };
        let mut said: Vec<ToldNews> = glimpse
            .gesture_fates
            .iter_told()
            .filter(|((name, why), _)| {
                (why.starts_with("dropped") && !(Some(*name) == let_go && *why == LET_GO))
                    || why.starts_with("ended")
            })
            .map(|((name, why), told)| ((name, GestureState::Dropped, Some(why)), told))
            .collect();
        if glimpse.gesture != self.making {
            if let Some(name) = self.making
                && !ended(name)
            {
                said.push(((name, GestureState::Over, None), Told::default()));
            }
            if let Some(name) = glimpse.gesture {
                said.push(((name, GestureState::Began, None), Told::default()));
            }
            self.making = glimpse.gesture;
        }
        said
    }

    /// The run stopped (a goal refused, a surprise, asked to): `over` for
    /// the gesture it was making, which no later step will end. A gesture
    /// still waiting to begin is not in the glimpse, so nothing is said of
    /// it.
    pub fn stopped(&mut self) -> Vec<GestureNews> {
        self.making
            .take()
            .map(|name| (name, GestureState::Over, None))
            .into_iter()
            .collect()
    }
}

/// Says `news` at `step`: `began` and `over` to `journal` as lamp-body's
/// lines (`gesture NAME`, `gesture over`; a drop's line is the fate's), and
/// every piece on `body/gesture` through `bus`.
pub fn tell(
    news: &[GestureNews],
    step: u64,
    bus: Option<&mut BusOut>,
    journal: impl FnMut(String),
) {
    let news: Vec<ToldNews> = news.iter().map(|news| (*news, Told::default())).collect();
    tell_told(&news, step, bus, false, journal);
}

/// The same, and with `telling` (`lamp-body --arbitration on`, #940), a
/// drop or an end on `body/gesture` also says whose it was, why, and who
/// holds the body; without it, the message is as it always was.
pub fn tell_told(
    news: &[ToldNews],
    step: u64,
    mut bus: Option<&mut BusOut>,
    telling: bool,
    mut journal: impl FnMut(String),
) {
    for ((name, state, why), told) in news {
        match state {
            GestureState::Began => journal(format!("lamp-body: step {step} gesture {name}")),
            GestureState::Over => journal(format!("lamp-body: step {step} gesture over")),
            GestureState::Dropped => {}
        }
        if let Some(bus) = bus.as_deref_mut() {
            if telling {
                bus.gesture_told(name, *state, *why, *told);
            } else {
                bus.gesture(name, *state);
            }
        }
    }
}

/// The way to the thread that publishes.
pub struct BusOut {
    steps: mpsc::SyncSender<Out>,
    thread: Option<thread::JoinHandle<()>>,
    /// Steps the thread had no room for.
    pub let_go: u64,
    /// `body/gesture` news the thread had no room for.
    pub gestures_let_go: u64,
    /// The lamp is paused (`lamp/pause`): the state it says is `paused`.
    pub paused: bool,
}

/// What the publishing thread holds between two steps.
struct Publishing {
    joints: Publisher,
    health: Publisher,
    /// `body/gesture`.
    gesture: Publisher,
    /// `body/hand`, said with every step passed on (every 80 ms), and when
    /// the push it says began.
    hand: Publisher,
    pushed_since: Option<u64>,
    started: Instant,
    before: Option<(u64, [u16; 5])>,
    /// Steps offered since the body's report was last sent, and each time
    /// a joint met resistance, with how many such times there had been in
    /// all.
    since_report: Option<u32>,
    /// While let go for its pose, the report that went last and when.
    waiting_sent: Option<(Diagnostic, Instant)>,
    resisted: Vec<Instant>,
    resisted_in_all: u32,
}

impl Publishing {
    fn on(bus: &Bus) -> io::Result<Self> {
        let mut joints =
            bus.publisher_declared(JointState::TOPIC, &JointState::declaration(), SOURCE)?;
        joints.expires_after(Some(LASTS));
        let health =
            bus.publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), SOURCE)?;
        let mut hand = bus.publisher_declared(Hand::TOPIC, &Hand::declaration(), SOURCE)?;
        hand.expires_after(Some(LASTS));
        let gesture =
            bus.publisher_declared(BodyGesture::TOPIC, &BodyGesture::declaration(), SOURCE)?;
        Ok(Self {
            joints,
            health,
            gesture,
            hand,
            pushed_since: None,
            started: Instant::now(),
            before: None,
            since_report: None,
            waiting_sent: None,
            resisted: Vec::new(),
            resisted_in_all: 0,
        })
    }

    fn at_us(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    fn step(&mut self, glimpse: &Glimpse, paused: bool, seen: Instant) {
        let now = Instant::now();
        let state = joint_state(glimpse, self.before);
        self.before = Some((glimpse.step, in_order(glimpse).0));
        let at_us = self.at_us();
        let _ = self.joints.publish_json(at_us, &state);
        // The hand is said as of when the body saw it, and lasts a second
        // from then: a step that waited in the queue is not fresh evidence,
        // and one that waited a second or more is not said at all.
        let seen_us = u64::try_from(seen.saturating_duration_since(self.started).as_micros())
            .unwrap_or(u64::MAX);
        self.pushed_since = glimpse.hand.then(|| self.pushed_since.unwrap_or(seen_us));
        // Measured after the joints are said: that write is file I/O and
        // may itself have taken time the hand has aged by.
        let age = Instant::now().saturating_duration_since(seen);
        if let Some(left) = LASTS.checked_sub(age).filter(|left| !left.is_zero()) {
            let hand = Hand {
                pushed: glimpse.hand,
                since_us: self.pushed_since.unwrap_or(0),
            };
            self.hand.expires_after(Some(left));
            let _ = self.hand.publish_json(seen_us, &hand);
        }
        let in_all: u32 = glimpse.eased.iter().map(|n| u32::from(*n)).sum();
        for _ in self.resisted_in_all..in_all {
            self.resisted.push(now);
        }
        self.resisted_in_all = in_all;
        self.resisted
            .retain(|at| now.saturating_duration_since(*at) < RESISTANCE_SAID_FOR);
        let report = |publishing: &Self| {
            let mut report = health(
                glimpse,
                u32::try_from(publishing.resisted.len()).unwrap_or(u32::MAX),
            );
            if paused {
                said_paused(&mut report);
            }
            report
        };
        if glimpse.waiting.is_some() {
            // Let go: once per change (#2938). The next running step
            // reports at once.
            self.since_report = None;
            let report = report(self);
            if waiting_due(&report, self.waiting_sent.as_ref(), now) {
                let _ = self.health.publish_json(self.at_us(), &report);
                self.waiting_sent = Some((report, now));
            }
            return;
        }
        self.waiting_sent = None;
        let since = self.since_report.map_or(REPORT_EVERY, |steps| steps + 1);
        self.since_report = Some(since);
        if since >= REPORT_EVERY {
            self.since_report = Some(0);
            let report = report(self);
            let _ = self.health.publish_json(self.at_us(), &report);
        }
    }
}

impl BusOut {
    /// The publisher, if the environment says `on` and the bus can be used.
    /// A bus that cannot is said once by `say` and the body goes on
    /// without.
    pub fn from_environment(say: impl FnOnce(&str)) -> Option<Self> {
        if std::env::var(ENVIRONMENT).ok().as_deref() != Some("on") {
            return None;
        }
        match Bus::from_environment().and_then(|bus| Self::on(&bus)) {
            Ok(out) => Some(out),
            Err(error) => {
                say(&format!("nothing is published on the bus: {error}"));
                None
            }
        }
    }

    pub fn on(bus: &Bus) -> io::Result<Self> {
        let mut publishing = Publishing::on(bus)?;
        // Room for a little over a second of offered steps.
        let (steps, offered) = mpsc::sync_channel::<Out>(16);
        let thread = thread::Builder::new().name("bus".into()).spawn(move || {
            for out in offered {
                match out {
                    Out::Step(glimpse, paused, seen) => publishing.step(&glimpse, paused, seen),
                    Out::Stopped(report) => {
                        let at_us = publishing.at_us();
                        let _ = publishing.health.publish_json(at_us, &report);
                    }
                    Out::Gesture(gesture) => {
                        let at_us = publishing.at_us();
                        let _ = publishing.gesture.publish_json(at_us, &gesture);
                    }
                }
            }
        })?;
        Ok(Self {
            steps,
            thread: Some(thread),
            let_go: 0,
            gestures_let_go: 0,
            paused: false,
        })
    }

    /// What became of a gesture, for `body/gesture`: passed on whatever the
    /// step, and never waited for. One the thread has no room for (the bus
    /// stalled for a second) is let go and counted in `gestures_let_go`.
    pub fn gesture(&mut self, name: &str, state: GestureState) {
        self.gesture_told(name, state, None, Told::default());
    }

    /// The same, saying whose it was, why and who holds the body, each
    /// when known (#940).
    pub fn gesture_told(&mut self, name: &str, state: GestureState, why: Option<&str>, told: Told) {
        let gesture = BodyGesture {
            name: name.to_owned(),
            state,
            source: told.source.map(|source| source.as_str().to_owned()),
            why: why.map(str::to_owned),
            by: told.by.map(|by| by.as_str().to_owned()),
        };
        if self.steps.try_send(Out::Gesture(gesture)).is_err() {
            self.gestures_let_go += 1;
        }
    }

    /// A step of the body. One in [`EVERY_STEPS`] is passed on, and never
    /// waited for.
    pub fn offer(&mut self, glimpse: &Glimpse) {
        if !offered(glimpse) {
            return;
        }
        if self
            .steps
            .try_send(Out::Step(*glimpse, self.paused, Instant::now()))
            .is_err()
        {
            self.let_go += 1;
        }
    }

    /// The body has stopped: say why, and wait for that to be said.
    pub fn stopped(mut self, stop: &Stop) {
        let _ = self.steps.send(Out::Stopped(stopped(stop)));
        self.end();
    }

    fn end(&mut self) {
        // The thread ends when nothing more can be offered.
        let (closed, _) = mpsc::sync_channel(1);
        drop(std::mem::replace(&mut self.steps, closed));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for BusOut {
    fn drop(&mut self) {
        self.end();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::Level;

    fn glimpse(step: u64) -> Glimpse {
        Glimpse {
            unplug_safe: false,
            unplug_unsafe: None,
            unplug_gap: false,
            step,
            mood: Mood::Listening,
            // Pitch, roll, yaw.
            goals: [1_968, 2_069, 2_015],
            positions: [1_950, 2_043, 2_015],
            loads: [120, 0, 40],
            temperatures: [38, 36, 41],
            eased: [0, 0, 0],
            kept_where_put: [0, 0, 0],
            stalls: [0; 5],
            stands: true,
            stand: 1.0,
            // Shoulder, elbow.
            arm_goals: [2_657, 2_602],
            arm_positions: [2_650, 2_602],
            arm_loads: [310, 250],
            arm_temperatures: [44, 43],
            look: [0.0, 0.0],
            look_aimed: None,
            face: None,
            gesture: None,
            released: 0,
            limp: false,
            hand: false,
            tipping: Default::default(),
            gesture_fates: Default::default(),
            looks_dropped: None,
            toward_refused: None,
            bad_reads: [0; 5],
            retreats: 0,
            resting_s: 0,
            cannot_lift: None,
            waiting: None,
        }
    }

    /// #2602: a body let go for its pose says what it asks of a person as
    /// its whole word, an error, and that it is still starting.
    #[test]
    fn a_body_let_go_for_its_pose_says_what_it_asks() {
        let say = "My head is tipped too far. Please move it gently back toward the middle, and I will start by myself.";
        let report = health(
            &Glimpse {
                waiting: Some(say),
                ..glimpse(0)
            },
            0,
        );
        assert_eq!((report.level, report.message.as_str()), (Level::Error, say));
        assert!(
            report
                .values
                .iter()
                .any(|value| value.key == lamp_keeper::STATE
                    && value.value == lamp_keeper::PartState::Starting.as_str())
        );
    }

    /// #2938 (the Reviewer-lead's P3 on #2940): let go, every read is
    /// offered but the report goes once per change, and the same again
    /// only `WAITING_RESEND` later, inside lamp-health's 5 s stale time.
    #[test]
    fn a_body_let_go_reports_once_per_change_and_slowly_again() {
        let start = Instant::now();
        let ask = |say: &'static str| {
            health(
                &Glimpse {
                    waiting: Some(say),
                    ..glimpse(1)
                },
                0,
            )
        };
        let mut last: Option<(Diagnostic, Instant)> = None;
        let mut sent = Vec::new();
        // 60 reads, 100 ms apart; the ask changes at the 30th.
        for read in 0..60u32 {
            let now = start + crate::body::WAIT_READ * read;
            let report = ask(if read < 30 {
                "My base is turned too far."
            } else {
                crate::body::STOPPED_ASKING
            });
            if waiting_due(&report, last.as_ref(), now) {
                sent.push(read);
                last = Some((report, now));
            }
        }
        // At once, then 2 s later, and at once again on the change.
        assert_eq!(sent, [0, 20, 30, 50]);
        assert!(WAITING_RESEND < Duration::from_secs(5));
        // Every step of a let-go body is offered; a running one is as
        // before, one step in four.
        assert!(offered(&Glimpse {
            waiting: Some("x"),
            ..glimpse(1)
        }));
        assert!(!offered(&glimpse(1)) && offered(&glimpse(EVERY_STEPS)));
    }

    /// #2938: the voice says only a let-go body's ask, never another
    /// part's error, a body's stop, or a well body.
    #[test]
    fn only_a_let_go_bodys_ask_is_an_ask() {
        use crate::ask::asks;
        let say = "My head is tipped too far.";
        let mut let_go = health(
            &Glimpse {
                waiting: Some(say),
                ..glimpse(1)
            },
            0,
        );
        assert_eq!(asks(&let_go), Some(say));
        said_paused(&mut let_go);
        assert_eq!(asks(&let_go), Some(say));
        let mut other = let_go.clone();
        other.name = "eyes".into();
        assert_eq!(asks(&other), None);
        assert_eq!(asks(&health(&glimpse(1), 0)), None);
        let refused = stopped(&Stop::Refused {
            why: "out of range".into(),
        });
        assert_eq!(asks(&refused), None);
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// Steps of a stand going from `from` to `to` evenly, from step `at`.
    fn stands(at: u64, from: f32, to: f32, steps: u64) -> Vec<Glimpse> {
        (0..=steps)
            .map(|k| Glimpse {
                stand: from + (to - from) * k as f32 / steps as f32,
                ..glimpse(at + k)
            })
            .collect()
    }

    /// #705, 14:05: one line when a fold ends, how long it took, where the
    /// joints were told to go against where they are, and any retreat.
    #[test]
    fn a_fold_is_said_once_when_it_ends_with_its_time_and_the_joints() {
        let mut said = FoldSaid::default();
        let mut lines = Vec::new();
        // Standing a while, then 6 s down (300 steps of 20 ms), then folded.
        for g in stands(0, 1.0, 1.0, 50)
            .into_iter()
            .chain(stands(51, 1.0, 0.0, 300))
            .chain(stands(352, 0.0, 0.0, 50))
        {
            lines.extend(said.step(&g));
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].starts_with("lamp-body: step 351 folded in 6.0 s; arm goals [2657, 2602] ended at [2650, 2602]; head and base goals [1968, 2069, 2015] at [1950, 2043, 2015]; nothing retreated since it was last folded"),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn a_fold_that_rises_again_is_not_said_and_a_retreat_on_the_way_is() {
        let mut said = FoldSaid::default();
        let mut lines = Vec::new();
        // Down halfway, up again: no fold.
        for g in stands(0, 1.0, 0.5, 100)
            .into_iter()
            .chain(stands(101, 0.5, 1.0, 100))
        {
            lines.extend(said.step(&g));
        }
        assert!(lines.is_empty(), "{lines:?}");
        // Down all the way with a sit-down for a strain counted halfway.
        for g in stands(300, 1.0, 0.0, 200) {
            let g = Glimpse {
                retreats: u16::from(g.step >= 400),
                ..g
            };
            lines.extend(said.step(&g));
        }
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("folded in 4.0 s"), "{}", lines[0]);
        assert!(
            lines[0].ends_with("it sat down for a strain once since it was last folded"),
            "{}",
            lines[0]
        );
        // A body that never stands says no fold.
        let mut said = FoldSaid::default();
        for g in stands(0, 1.0, 0.0, 10) {
            assert_eq!(said.step(&Glimpse { stands: false, ..g }), None);
        }
    }

    #[test]
    fn the_bad_reads_are_said_at_the_first_step_and_every_five_minutes() {
        let mut said = ReadsSaid::default();
        let at = |step: u64| Glimpse {
            bad_reads: [0, 0, 0, 3, 1],
            retreats: 2,
            ..glimpse(step)
        };
        assert_eq!(
            said.step(&at(10)).as_deref(),
            Some(
                "lamp-body: step 10 bad reads so far: base_yaw 0, base_pitch 0, elbow_pitch 0, wrist_roll 3, wrist_pitch 1; retreats so far 2"
            )
        );
        assert_eq!(said.step(&at(10 + ReadsSaid::EVERY_STEPS - 1)), None);
        assert!(said.step(&at(10 + ReadsSaid::EVERY_STEPS)).is_some());
    }

    #[test]
    fn the_joints_are_named_in_servo_order_in_radians_from_the_middle_of_the_turn() {
        let state = joint_state(&glimpse(8), None);
        assert_eq!(state.check(), Ok(()));
        assert_eq!(
            state.name,
            ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
        );
        let count = TAU / 4096.0;
        let expected = [
            (2_015.0 - 2_048.0) * count,
            (2_650.0 - 2_048.0) * count,
            (2_602.0 - 2_048.0) * count,
            (2_043.0 - 2_048.0) * count,
            (1_950.0 - 2_048.0) * count,
        ];
        for (is, should) in state.position.iter().zip(expected) {
            assert!(near(*is, should), "{is} {should}");
        }
        // The middle of the turn is nothing, and 1,024 counts a quarter turn.
        assert!(near(radians(2_048), 0.0));
        assert!(near(radians(3_072), TAU / 4.0));
        assert!(near(radians(1_024), -TAU / 4.0));
        // Where each was asked to be, in the same order.
        assert!(near(state.commanded[0], (2_015.0 - 2_048.0) * count));
        assert!(near(state.commanded[1], (2_657.0 - 2_048.0) * count));
        assert!(near(state.commanded[3], (2_069.0 - 2_048.0) * count));
        assert!(near(state.commanded[4], (1_968.0 - 2_048.0) * count));
        // The load as a share of all the servo can exert.
        assert_eq!(state.effort, [0.04, 0.31, 0.25, 0.0, 0.12]);
        // No step before this one: no speed is made up.
        assert!(state.velocity.is_empty());
    }

    #[test]
    fn how_fast_each_joint_turns_is_worked_out_from_the_step_before() {
        let was = glimpse(8);
        let mut now = glimpse(12);
        // In four steps, 80 ms: the head lifted 10 counts and the base
        // turned back 4.
        now.positions = [1_960, 2_043, 2_011];
        let state = joint_state(&now, Some((was.step, in_order(&was).0)));
        assert_eq!(state.check(), Ok(()));
        let count = TAU / 4096.0;
        assert!(near(state.velocity[4], 10.0 * count / 0.08));
        assert!(near(state.velocity[0], -4.0 * count / 0.08));
        assert!(near(state.velocity[1], 0.0));
        // A step that is not later than the one before has no speed.
        assert!(
            joint_state(&was, Some((12, in_order(&now).0)))
                .velocity
                .is_empty()
        );
    }

    #[test]
    fn a_paused_body_says_paused_and_nothing_else_changes() {
        let well = health(&glimpse(50), 0);
        let mut paused = well.clone();
        said_paused(&mut paused);
        let state = |report: &Diagnostic| {
            report
                .values
                .iter()
                .find(|value| value.key == lamp_keeper::STATE)
                .map(|value| value.value.clone())
        };
        assert_eq!(state(&well).as_deref(), Some("ready"));
        assert_eq!(state(&paused).as_deref(), Some("paused"));
        assert_eq!((paused.level, &paused.message), (well.level, &well.message));
    }

    #[test]
    fn the_body_is_well_unless_a_joint_is_warm_or_one_lately_met_resistance() {
        let well = health(&glimpse(50), 0);
        assert_eq!((well.level, well.message.as_str()), (Level::Ok, ""));
        assert_eq!(well.name, "body");
        let facts: Vec<(&str, &str)> = well
            .values
            .iter()
            .map(|fact| (fact.key.as_str(), fact.value.as_str()))
            .collect();
        assert_eq!(
            facts,
            [
                ("mood", "listening"),
                ("standing", "1.00"),
                ("warmest_joint", "shoulder"),
                ("warmest_reading", "44"),
                ("resisted_last_minute", "0"),
                ("resisted_since_start", "0"),
                ("state", "ready"),
            ]
        );
        // One under warm is well, warm is not.
        let mut warm = glimpse(50);
        warm.arm_temperatures = [WARM - 1, 40];
        assert_eq!(health(&warm, 0).level, Level::Ok);
        warm.arm_temperatures = [WARM, 40];
        let report = health(&warm, 0);
        assert_eq!(report.level, Level::Warn);
        assert_eq!(
            report.message,
            "shoulder is warm: it reads 55, and the body stops at 62"
        );
        // Resting folded after three strains in half an hour is said, as
        // a warning, until it may rise again (lamp-4ace folded unseen from
        // 20 to 24 Oct). Warmth is said first.
        let mut resting = glimpse(50);
        resting.resting_s = 1_741;
        let report = health(&resting, 0);
        assert_eq!(report.level, Level::Warn);
        assert_eq!(
            report.message,
            "its arm sat down for a strain three times in half an hour: it rests folded and may rise again in 30 min"
        );
        resting.arm_temperatures = [WARM, 40];
        assert!(health(&resting, 0).message.starts_with("shoulder is warm"));
        // A joint the body commands, too.
        let mut head = glimpse(50);
        head.temperatures = [58, 36, 41];
        assert!(health(&head, 0).message.starts_with("head_pitch is warm"));
        // Resistance, once and more than once. Warmth is said first.
        let mut pushed = glimpse(50);
        pushed.eased = [2, 0, 1];
        let report = health(&pushed, 1);
        assert_eq!(report.level, Level::Warn);
        assert_eq!(
            report.message,
            "a joint met resistance and stopped pushing, 1 time in the last minute"
        );
        assert!(
            health(&pushed, 3)
                .message
                .ends_with("3 times in the last minute")
        );
        assert_eq!(report.values[5].value, "3");
        assert!(health(&head, 3).message.starts_with("head_pitch is warm"));
    }

    #[test]
    fn why_the_body_stopped_is_said_in_words_and_only_being_asked_is_not_a_fault() {
        let asked = stopped(&Stop::Cancelled);
        assert_eq!(
            (asked.level, asked.message.as_str()),
            (Level::Warn, "stopped, as it was asked to")
        );
        for (stop, says) in [
            (
                Stop::Load {
                    joint: 5,
                    load: 260,
                },
                "stopped: head_pitch was pushed against (load 260)",
            ),
            (
                Stop::Tracking {
                    joint: 1,
                    goal: 2_050,
                    position: 2_015,
                },
                "stopped: base_yaw did not follow (asked 2050, at 2015)",
            ),
            (
                Stop::Status {
                    joint: 3,
                    found: 0,
                    now: 0x20,
                },
                "stopped: the servo of elbow reports a fault (status 0x00 became 0x20)",
            ),
            (
                Stop::Drift {
                    joint: 2,
                    found: 2_048,
                    now: 2_060,
                },
                "stopped: shoulder was moved by something else (from 2048 to 2060)",
            ),
            (
                Stop::Temperature {
                    joint: 4,
                    temperature: 63,
                },
                "stopped: head_roll is too warm (it reads 63)",
            ),
            (
                Stop::Bus {
                    detail: "41 bytes of a late reply".into(),
                },
                "stopped: the servos did not answer as expected",
            ),
            (
                Stop::Window {
                    joint: 3,
                    goal: 4_095,
                    window: (1_627, 3_022),
                    from: 2_047,
                },
                "stopped: elbow was not asked for 4095, outside 1627..=3022 or too far from 2047",
            ),
            (
                Stop::BadRead {
                    joint: 3,
                    position: 37_639,
                    from: 2_049,
                },
                "stopped: elbow read 37639 three times in a row, not where it can be (last good 2049)",
            ),
            (
                Stop::Refused {
                    why: "head pitch is at 2095".into(),
                },
                "would not start: head pitch is at 2095",
            ),
            (
                Stop::Load { joint: 9, load: 1 },
                "stopped: a joint was pushed against (load 1)",
            ),
        ] {
            let report = stopped(&stop);
            assert_eq!(report.level, Level::Error, "{stop:?}");
            assert_eq!(report.message, says);
            assert_eq!(report.name, "body");
        }
    }

    #[test]
    fn steps_offered_reach_the_bus_and_the_last_word_is_why_it_stopped() {
        use lamp_msgs::JointState;
        let root = std::path::PathBuf::from(format!("/tmp/lb-bus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut joints = bus
            .subscribe_typed(JointState::TOPIC, "test", JointState::TYPE)
            .unwrap();
        let mut reports = bus
            .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
            .unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        // Twelve steps: three of them are passed on.
        for step in 0..12 {
            let mut seen = glimpse(step);
            seen.positions[0] = 1_950 + step as u16;
            out.offer(&seen);
        }
        let wait = Duration::from_secs(5);
        let states: Vec<JointState> = (0..3)
            .map(|_| {
                joints
                    .recv(wait)
                    .unwrap()
                    .expect("a state")
                    .parse()
                    .unwrap()
            })
            .collect();
        assert!(states[0].velocity.is_empty());
        assert_eq!(states[1].velocity.len(), 5);
        let count = TAU / 4096.0;
        assert!(near(states[2].position[4], (1_958.0 - 2_048.0) * count));
        assert!(near(states[2].velocity[4], 4.0 * count / 0.08));
        // One report of how it is, with the first step.
        let report: Diagnostic = reports
            .recv(wait)
            .unwrap()
            .expect("a report")
            .parse()
            .unwrap();
        assert_eq!((report.level, report.name.as_str()), (Level::Ok, "body"));
        assert_eq!(
            (
                report.values[6].key.as_str(),
                report.values[6].value.as_str()
            ),
            ("state", "ready")
        );
        // What is kept for whoever comes later is the newest. A message
        // reaches its subscribers a moment before it is the kept one.
        let since = Instant::now();
        loop {
            let kept: JointState = bus
                .latest(JointState::TOPIC)
                .unwrap()
                .expect("kept")
                .parse()
                .unwrap();
            if kept == states[2] {
                break;
            }
            assert!(since.elapsed() < wait, "{kept:?}");
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(out.let_go, 0);
        out.stopped(&Stop::Temperature {
            joint: 2,
            temperature: 62,
        });
        let last: Diagnostic = reports
            .recv(wait)
            .unwrap()
            .expect("the last")
            .parse()
            .unwrap();
        assert_eq!(last.level, Level::Error);
        assert_eq!(last.message, "stopped: shoulder is too warm (it reads 62)");
        assert_eq!(last.values[0].value, "stopping");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_gesture_that_replaces_another_says_the_first_over_and_a_stop_ends_the_last() {
        let mut said = GestureSaid::default();
        let mut seen = glimpse(1);
        seen.gesture = Some("idle");
        assert_eq!(said.step(&seen), [("idle", GestureState::Began, None)]);
        // The idle clip replaced by a nod in one step: over, then began.
        seen.gesture = Some("nod");
        assert_eq!(
            said.step(&seen),
            [
                ("idle", GestureState::Over, None),
                ("nod", GestureState::Began, None)
            ]
        );
        // A nod let go as it played (a joint could not keep up): over, not
        // dropped, since it began.
        let mut let_go = glimpse(2);
        let_go
            .gesture_fates
            .push(("nod", "dropped: a joint could not keep up with it"));
        let mut playing = GestureSaid::default();
        seen.gesture_fates = Default::default();
        playing.step(&seen);
        assert_eq!(playing.step(&let_go), [("nod", GestureState::Over, None)]);
        // Another ask of the same clip, dropped in the step the first ends
        // (the reviewer's probe, 12:46 on #1230): the second says
        // `dropped`, the first `over`.
        let mut ended = glimpse(3);
        ended
            .gesture_fates
            .push(("nod", "dropped: another gesture plays"));
        let mut same = GestureSaid::default();
        same.step(&seen);
        assert_eq!(
            same.step(&ended),
            [
                (
                    "nod",
                    GestureState::Dropped,
                    Some("dropped: another gesture plays")
                ),
                ("nod", GestureState::Over, None),
            ]
        );
        // Two fates in a step: only the drop is news, and first.
        seen.gesture_fates
            .push(("shy", "dropped: another was asked after it"));
        seen.gesture_fates.push(("nod", "taken: it plays"));
        assert_eq!(
            said.step(&seen),
            [(
                "shy",
                GestureState::Dropped,
                Some("dropped: another was asked after it")
            )]
        );
        // The run stops mid-nod: over for it, once.
        assert_eq!(said.stopped(), [("nod", GestureState::Over, None)]);
        assert!(said.stopped().is_empty());
    }

    /// lamp-body's step and its stop say the news through [`tell`]: its
    /// journal lines, and every piece on `body/gesture` (Fifth on #1230:
    /// without the bus call, this fails).
    #[test]
    fn tell_says_the_journal_lines_and_publishes_every_piece() {
        let root = std::path::PathBuf::from(format!("/tmp/lb-tell-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut gestures = bus
            .subscribe_typed(BodyGesture::TOPIC, "test", BodyGesture::TYPE)
            .unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        let mut lines = Vec::new();
        tell(
            &[
                ("shy", GestureState::Dropped, Some("dropped: paused")),
                ("nod", GestureState::Began, None),
            ],
            7,
            Some(&mut out),
            |line| lines.push(line),
        );
        tell(
            &[("nod", GestureState::Over, None)],
            9,
            Some(&mut out),
            |line| lines.push(line),
        );
        assert_eq!(
            lines,
            [
                "lamp-body: step 7 gesture nod",
                "lamp-body: step 9 gesture over"
            ]
        );
        let got: Vec<(String, GestureState)> = (0..3)
            .map(|_| {
                let gesture: BodyGesture = gestures
                    .recv(Duration::from_secs(5))
                    .unwrap()
                    .expect("a gesture")
                    .parse()
                    .unwrap();
                (gesture.name, gesture.state)
            })
            .collect();
        assert_eq!(
            got,
            [
                ("shy".into(), GestureState::Dropped),
                ("nod".into(), GestureState::Began),
                ("nod".into(), GestureState::Over),
            ]
        );
        assert_eq!((out.let_go, out.gestures_let_go), (0, 0));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// #940, the fifth's sixth condition: with `telling` a drop says whose
    /// it was, why and who holds the body; without it, the message is byte
    /// for byte as before, whatever the news holds.
    #[test]
    fn a_drop_says_whose_it_was_why_and_who_holds_the_body_only_when_telling() {
        let root = std::path::PathBuf::from(format!("/tmp/lb-told-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut gestures = bus
            .subscribe_typed(BodyGesture::TOPIC, "test", BodyGesture::TYPE)
            .unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        let told = Told {
            source: crate::moves::Source::new("skill:dance"),
            by: crate::moves::Source::new("skill:stretch-break"),
        };
        let news = [
            (
                ("nod", GestureState::Dropped, Some("dropped: busy, skill")),
                told,
            ),
            (("shy", GestureState::Began, None), Told::default()),
        ];
        tell_told(&news, 7, Some(&mut out), true, |_| {});
        tell_told(&news, 8, Some(&mut out), false, |_| {});
        let got: Vec<String> = (0..4)
            .map(|_| {
                let message = gestures
                    .recv(Duration::from_secs(5))
                    .unwrap()
                    .expect("a gesture");
                String::from_utf8(message.payload).unwrap()
            })
            .collect();
        assert_eq!(
            got,
            [
                "{\"name\":\"nod\",\"state\":\"dropped\",\"source\":\"skill:dance\",\
                 \"why\":\"dropped: busy, skill\",\"by\":\"skill:stretch-break\"}",
                "{\"name\":\"shy\",\"state\":\"began\"}",
                "{\"name\":\"nod\",\"state\":\"dropped\"}",
                "{\"name\":\"shy\",\"state\":\"began\"}",
            ]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn what_became_of_each_gesture_is_said_on_body_gesture_whatever_the_step() {
        let root = std::path::PathBuf::from(format!("/tmp/lb-gest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut gestures = bus
            .subscribe_typed(BodyGesture::TOPIC, "test", BodyGesture::TYPE)
            .unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        let mut said = GestureSaid::default();
        // A nod from step 1 to step 6, and a shy dropped at step 3: on
        // steps that are not passed on as joint states (one in four).
        for step in 0..8 {
            let mut seen = glimpse(step);
            seen.gesture = (1..6).contains(&step).then_some("nod");
            if step == 3 {
                seen.gesture_fates
                    .push(("shy", "dropped: another gesture plays"));
            }
            for (name, state, _) in said.step(&seen) {
                out.gesture(name, state);
            }
        }
        let got: Vec<BodyGesture> = (0..3)
            .map(|_| {
                gestures
                    .recv(Duration::from_secs(5))
                    .unwrap()
                    .expect("a gesture")
                    .parse()
                    .unwrap()
            })
            .collect();
        let named = |name: &str, state| BodyGesture {
            name: name.into(),
            state,
            source: None,
            why: None,
            by: None,
        };
        assert_eq!(
            got,
            [
                named("nod", GestureState::Began),
                named("shy", GestureState::Dropped),
                named("nod", GestureState::Over),
            ]
        );
        assert_eq!(out.let_go, 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_hand_is_said_with_each_step_passed_on_and_keeps_when_the_push_began() {
        let root = std::path::PathBuf::from(format!("/tmp/lb-hand-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut hands = bus
            .subscribe_typed(Hand::TOPIC, "test", Hand::TYPE)
            .unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        // Passed on: steps 0, 4, 8, 12, 16. Pushed on 4 and 8 (and 12, 16 not).
        for step in 0..20 {
            let mut seen = glimpse(step);
            seen.hand = (4..12).contains(&step);
            out.offer(&seen);
        }
        let said: Vec<Hand> = (0..5)
            .map(|_| {
                hands
                    .recv(Duration::from_secs(5))
                    .unwrap()
                    .expect("a hand")
                    .parse()
                    .unwrap()
            })
            .collect();
        assert_eq!(
            said.iter().map(|h| h.pushed).collect::<Vec<_>>(),
            [false, true, true, false, false]
        );
        assert_eq!(said[0].since_us, 0);
        assert!(said[1].since_us > 0);
        assert_eq!(said[2].since_us, said[1].since_us, "the push began once");
        assert_eq!((said[3].since_us, said[4].since_us), (0, 0));
        drop(out);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A hand that waited in the queue is said as of when the body saw it:
    /// a second or more old it is not said at all, and younger it lasts
    /// only what is left of its second. The reviewer's case: a push seen
    /// 1.2 s before the thread got to it, then the body stopped.
    #[test]
    fn a_hand_that_waited_in_the_queue_is_not_said_as_fresh() {
        let root = std::path::PathBuf::from(format!("/tmp/lb-handq-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut publishing = Publishing::on(&bus).unwrap();
        let mut seen = glimpse(0);
        seen.hand = true;
        let now = Instant::now();
        publishing.step(&seen, false, now - Duration::from_millis(1_200));
        assert!(
            bus.latest(Hand::TOPIC).unwrap().is_none(),
            "a hand seen 1.2 s ago is not said"
        );
        publishing.step(&seen, false, Instant::now() - Duration::from_millis(600));
        let said = bus
            .latest(Hand::TOPIC)
            .unwrap()
            .expect("a hand seen 0.6 s ago");
        let lasts = said.header.expires_ms.unwrap();
        assert!(
            lasts <= 400,
            "it lasts only what is left of its second: {lasts} ms"
        );
        let hand: Hand = said.parse().unwrap();
        assert!(hand.pushed);
        // A live step: said at once, for its whole second less only what
        // the step itself took (a loaded runner took over 10 ms).
        let live = Instant::now();
        publishing.step(&seen, false, live);
        let took = live.elapsed().as_millis() as u64;
        let said = bus.latest(Hand::TOPIC).unwrap().unwrap();
        let lasts = said.header.expires_ms.unwrap();
        assert!(
            lasts + took + 1 >= 1_000,
            "a live hand lasts its second less the step's {took} ms: {lasts} ms"
        );
        drop(publishing);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A hand is aged by what came before it on the same step too: the
    /// joints' write is file I/O, and here it is held up (the reviewer's
    /// case, a FIFO where the joints' next value is written), so a push is
    /// a second and more old by the time it would be said, and is not said
    /// as fresh. Seen 900 ms before the step and held up 250 ms, not seen as
    /// the step began and held up 1.2 s (#1806): without the write's time
    /// it would be under the second and said.
    #[test]
    fn a_hand_after_a_slow_joints_write_is_aged_by_it() {
        let root = std::path::PathBuf::from(format!("/tmp/lb-handio-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut publishing = Publishing::on(&bus).unwrap();
        publishing.step(&glimpse(0), false, Instant::now());
        let boot = bus.latest(JointState::TOPIC).unwrap().unwrap().header.boot;
        let fifo = root
            .join(JointState::TOPIC)
            .join(format!(".latest.{boot}.new"));
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        let reader = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            let _ = std::fs::read(&fifo);
        });
        let mut pushed = glimpse(EVERY_STEPS);
        pushed.hand = true;
        publishing.step(&pushed, false, Instant::now() - Duration::from_millis(900));
        reader.join().unwrap();
        let said: Hand = bus.latest(Hand::TOPIC).unwrap().unwrap().parse().unwrap();
        assert!(!said.pushed, "a push 1.15 s old is not said as fresh");
        drop(publishing);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_thread_with_no_room_costs_the_loop_a_step_and_never_a_wait() {
        let root = std::path::PathBuf::from(format!("/tmp/lb-full-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut out = BusOut::on(&bus).unwrap();
        // Far more than the thread can hold, as fast as they can be made.
        let started = Instant::now();
        for step in 0..40_000u64 {
            out.offer(&glimpse(step * EVERY_STEPS));
        }
        let each = started.elapsed() / 40_000;
        // A step is 20 ms; offering one must be nothing beside that.
        assert!(each < Duration::from_millis(1), "{each:?}");
        drop(out);
        std::fs::remove_dir_all(root).unwrap();
    }
}
