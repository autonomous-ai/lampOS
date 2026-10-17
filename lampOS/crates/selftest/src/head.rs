//! The head's small move (#812, as the fifth builder agreed it): at the
//! factory and when asked, the head's pitch and roll are each asked a few
//! counts one way and the other through `expression/body`, the way
//! Expression asks, and `body/joint_states` is watched for the body's goal
//! to move, the joint to follow it and to come back. `lamp-body` takes such
//! a move at rest under `--rest-glance` and cuts it to 50 counts of pitch
//! and 25 of roll; it never moves the base or the arm for it, and neither
//! does this check: those three are only read (`listen::joint`).
//!
//! Only from rest, standing, with no hand on the lamp and nothing asked of
//! the body by anyone else in the last minute. Otherwise it says why and
//! moves nothing; it never stands a lamp up. Never at first boot. A move
//! that fails is reported and not tried again in the run.
use crate::{Check, Clock, Host, listen};
use lamp_bus::Bus;
use lamp_msgs::{
    BodyOffsets, Diagnostic, EXPRESSION_LEASE_US, ExpressionBody, ExpressionLease, Hand,
    JointState, Level, Occasion, PartResult, Verdict,
};
use std::{
    f64::consts::{PI, TAU},
    time::{Duration, Instant},
};

/// The part's name in the report.
pub const PART: &str = "head_move";
/// The name the self-test asks under on `expression/body`.
pub const SOURCE: &str = "selftest";
/// How far each move asks, in counts: inside the body's rest reach (50
/// and 25), so the body's own cut leaves it as asked where it can.
pub const PITCH_COUNTS: f64 = 40.0;
pub const ROLL_COUNTS: f64 = 20.0;
/// The body's own take-hold tolerance, in counts; its tracking stop is 30.
pub const NEAR: f64 = 12.0;
/// The goal must be met within this of being asked.
pub const REACH_WITHIN: Duration = Duration::from_secs(2);
/// The joint must be back within this of the move being let go.
pub const BACK_WITHIN: Duration = Duration::from_secs(3);
/// Nothing asked of the body by anyone else for this long before.
pub const QUIET: Duration = Duration::from_secs(60);
/// A body at least this far up is standing (`standing` in its report).
pub const STANDING: f64 = 0.95;
/// The body stops at this temperature reading; a joint this warm is not moved.
pub const WARM: u32 = 62;
/// The body reports how it is once a second.
pub const REPORT_WITHIN: Duration = Duration::from_millis(2_500);
/// Easing out and back, as Expression's moves do.
const EASE: Duration = Duration::from_millis(400);
/// Requests are renewed as often as Expression renews them.
const STEP: Duration = Duration::from_millis(20);
/// A goal unchanged this long has arrived where the body takes it.
const STEADY: Duration = Duration::from_millis(200);
/// A servo's turn is 4,096 counts.
const COUNTS_A_RADIAN: f64 = 4096.0 / TAU;
/// How long a joint may take to hold still before a move reads its start.
const SETTLE_WITHIN: Duration = Duration::from_secs(1);
/// What one move takes at most, with the margin of a renewal or two, and
/// the wait for the joint to hold still first.
const ONE_MOVE: Duration = Duration::from_millis(6_600);

/// Which joint a move asks of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Joint {
    Pitch,
    Roll,
}

impl Joint {
    pub fn name(self) -> &'static str {
        match self {
            Self::Pitch => "head_pitch",
            Self::Roll => "head_roll",
        }
    }

    fn counts(self) -> f64 {
        match self {
            Self::Pitch => PITCH_COUNTS,
            Self::Roll => ROLL_COUNTS,
        }
    }

    fn offsets(self, counts: f64) -> BodyOffsets {
        let radians = counts / COUNTS_A_RADIAN;
        match self {
            Self::Pitch => BodyOffsets {
                head_pitch: radians,
                ..BodyOffsets::ZERO
            },
            Self::Roll => BodyOffsets {
                head_roll: radians,
                ..BodyOffsets::ZERO
            },
        }
    }
}

/// The moves, in order: the head up and down, then tilted each way. A
/// positive pitch dips the head, as in the body's own poses.
pub const MOVES: [(Joint, f64); 4] = [
    (Joint::Pitch, -1.0),
    (Joint::Pitch, 1.0),
    (Joint::Roll, 1.0),
    (Joint::Roll, -1.0),
];

/// What the body said of itself on `diagnostics`.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyReport {
    pub level: Level,
    pub message: String,
    pub mood: String,
    pub standing: f64,
    pub warmest: u32,
}

impl BodyReport {
    /// The body's report, if `diagnostic` is one.
    pub fn of(diagnostic: &Diagnostic) -> Option<Self> {
        if diagnostic.name != "body" {
            return None;
        }
        let value = |key: &str| {
            diagnostic
                .values
                .iter()
                .find(|value| value.key == key)
                .map(|value| value.value.as_str())
        };
        Some(Self {
            level: diagnostic.level,
            message: diagnostic.message.clone(),
            mood: value("mood").unwrap_or("").to_owned(),
            standing: value("standing")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0),
            warmest: value("warmest_reading")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
        })
    }
}

/// What is known before moving: the body's newest report, whether a hand
/// is on the lamp, and how long ago someone else last asked the body for
/// anything.
#[derive(Clone, Debug, PartialEq)]
pub struct Before {
    pub occasion: Occasion,
    pub body: Option<BodyReport>,
    pub pushed: bool,
    pub others_asked: Option<Duration>,
}

/// Whether the head may be moved now, or the part's result if not.
pub fn may_move(before: &Before) -> Result<(), PartResult> {
    let not = |why: &str| Err(PartResult::new(PART, Verdict::NotChecked, why));
    if before.occasion == Occasion::FirstBoot {
        return not("never moved at first boot: only at the factory and when asked");
    }
    let Some(body) = &before.body else {
        return not("lamp-body said nothing on diagnostics: is it running, with LAMP_BUS=on?");
    };
    if body.warmest >= WARM {
        return not(&format!(
            "a joint is warm (it reads {}, and the body stops at {WARM})",
            body.warmest
        ));
    }
    if body.level != Level::Ok {
        return not(&format!("lamp-body is not well: {}", body.message));
    }
    if body.mood != "resting" {
        return not(&format!("the lamp is not at rest: it is {}", body.mood));
    }
    if body.standing < STANDING {
        return not(&format!(
            "the lamp is not standing (standing {:.2}); it is not stood up for a test",
            body.standing
        ));
    }
    if before.pushed {
        return not("a hand is on the lamp");
    }
    if let Some(ago) = before.others_asked
        && ago < QUIET
    {
        return not(&format!(
            "the body was asked to move {:.0} s ago by someone else",
            ago.as_secs_f64()
        ));
    }
    Ok(())
}

/// One reading of a joint, in counts, at a time since the move began.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub at: Duration,
    pub position: f64,
    pub commanded: f64,
}

/// Where a move should bring the joint, in counts from where it began:
/// what was asked, or less if the body cut its goal shorter (lamp-52e6's
/// roll reaches one count). Never the written goal itself: lifting a
/// relaxed head, the body writes its goal 40 counts past the pose it wants
/// (`GLANCE_LIFT_LEAD`), because the head stops about 30 short of it.
pub fn expected(goal_moved: f64, asked: f64) -> f64 {
    goal_moved.min(asked)
}

/// When the move was met: the body's goal moved the asked way and has held
/// still for a moment, and the joint is within [`NEAR`] of where it was
/// asked to go from `start` ([`expected`]). That move in counts beside it.
pub fn reached(
    start: f64,
    goal_was: f64,
    sign: f64,
    asked: f64,
    samples: &[Sample],
) -> Option<(Duration, f64)> {
    for (index, sample) in samples.iter().enumerate() {
        let moved = (sample.commanded - goal_was) * sign;
        if moved < 0.5 {
            continue;
        }
        let wanted = expected(moved, asked);
        if (sample.position - (start + sign * wanted)).abs() > NEAR {
            continue;
        }
        // Unchanged since the last reading at least `STEADY` before.
        let since = sample.at.saturating_sub(STEADY);
        let Some(from) = samples[..index]
            .iter()
            .rposition(|earlier| earlier.at <= since)
        else {
            continue;
        };
        let held = samples[from..=index]
            .iter()
            .all(|earlier| (earlier.commanded - sample.commanded).abs() < 0.5);
        if held {
            return Some((sample.at, wanted));
        }
    }
    None
}

/// How one move went.
#[derive(Clone, Debug, PartialEq)]
pub enum Moved {
    /// The body's goal never moved: the move was not taken.
    NotTaken,
    /// Met, and back within [`NEAR`] of where the joint began.
    Done {
        reached: Duration,
        goal: f64,
        back: Duration,
    },
    /// The goal moved and the joint did not get where it was asked in time.
    NotReached { goal: f64, gap: f64 },
    /// Met, and not back where it began in time.
    NotBack { goal: f64, away: f64 },
}

/// One move and how it went.
#[derive(Clone, Debug, PartialEq)]
pub struct Tried {
    pub joint: Joint,
    pub sign: f64,
    pub moved: Moved,
}

fn way(joint: Joint, sign: f64) -> &'static str {
    match (joint, sign > 0.0) {
        (Joint::Pitch, true) => "down",
        (Joint::Pitch, false) => "up",
        (Joint::Roll, true) => "one way",
        (Joint::Roll, false) => "the other way",
    }
}

/// The part's result from the moves tried and, if it was, what ended the
/// check early.
pub fn judge(tried: &[Tried], interrupted: Option<&str>) -> PartResult {
    let mut values = Vec::new();
    for t in tried {
        let key = format!(
            "{}_{}",
            t.joint.name(),
            if t.sign > 0.0 { "plus" } else { "minus" }
        );
        if let Moved::Done {
            reached,
            goal,
            back,
        } = &t.moved
        {
            values.push((format!("{key}_goal_counts"), format!("{goal:.0}")));
            values.push((
                format!("{key}_ms"),
                format!(
                    "{:.0}/{:.0}",
                    reached.as_secs_f64() * 1e3,
                    back.as_secs_f64() * 1e3
                ),
            ));
        }
    }
    let with_values = |mut result: PartResult| {
        for (key, value) in &values {
            result = result.with(key, value);
        }
        result
    };
    for t in tried {
        let what = format!("{} {}", t.joint.name(), way(t.joint, t.sign));
        let failed = match &t.moved {
            Moved::NotReached { goal, gap } => Some(format!(
                "{what}: the goal moved {goal:.0} counts and the joint stayed {gap:.0} away after {} s",
                REACH_WITHIN.as_secs()
            )),
            Moved::NotBack { goal, away } => Some(format!(
                "{what}: moved {goal:.0} counts and was still {away:.0} from where it began after {} s",
                BACK_WITHIN.as_secs()
            )),
            _ => None,
        };
        if let Some(why) = failed {
            return with_values(PartResult::new(PART, Verdict::Fail, &why));
        }
    }
    if let Some(why) = interrupted {
        return with_values(PartResult::new(
            PART,
            Verdict::NotChecked,
            &format!("stopped: {why}"),
        ));
    }
    // Every asked move is judged: each must have been taken and done.
    let done = |joint: Joint, sign: f64| {
        tried
            .iter()
            .any(|t| t.joint == joint && t.sign == sign && matches!(t.moved, Moved::Done { .. }))
    };
    let missing: Vec<String> = MOVES
        .iter()
        .filter(|(joint, sign)| !done(*joint, *sign))
        .map(|(joint, sign)| format!("{} {}", joint.name(), way(*joint, *sign)))
        .collect();
    if missing.len() == MOVES.len() {
        return PartResult::new(
            PART,
            Verdict::NotChecked,
            "the body took no move: it runs without LAMP_EXPRESSION_BODY=on, or with --rest-glance off",
        );
    }
    if !missing.is_empty() {
        return with_values(PartResult::new(
            PART,
            Verdict::NotChecked,
            &format!("the body did not move the goal for {}", missing.join(", ")),
        ));
    }
    with_values(PartResult::new(PART, Verdict::Pass, ""))
}

/// A joint's position and goal once its position has held within half a
/// count for [`STEADY`], as [`reached`] asks of a goal; `Err` if it does not
/// within [`SETTLE_WITHIN`], or something ends the check meanwhile.
fn settled(
    bus: &Bus,
    watch: &mut Watch,
    joint: Joint,
    clock: &mut impl Clock,
) -> Result<(f64, f64), String> {
    let began = clock.now();
    let mut still: Option<(Instant, f64)> = None;
    let mut last_seq = None;
    loop {
        if let Some(why) = watch.interruption(bus) {
            return Err(why);
        }
        let Some((seq, position, goal)) = read(bus, joint) else {
            return Err(format!(
                "{} is not read on {}",
                joint.name(),
                JointState::TOPIC
            ));
        };
        if last_seq != Some(seq) {
            last_seq = Some(seq);
            match still {
                Some((held_from, at)) if (position - at).abs() < 0.5 => {
                    if since(clock, held_from) >= STEADY {
                        return Ok((position, goal));
                    }
                }
                _ => still = Some((clock.now(), position)),
            }
        }
        if since(clock, began) >= SETTLE_WITHIN {
            // A body that stopped saying its joints is named as such, as
            // before a move (the reviewer on #1340).
            if let Some(age) = newest_age(bus).filter(|age| *age > listen::FAST_FRESH) {
                return Err(format!(
                    "lamp-body is not publishing {} (newest {:.1} s old)",
                    JointState::TOPIC,
                    age.as_secs_f64()
                ));
            }
            return Err(format!(
                "{} did not hold still within {} s before its move",
                joint.name(),
                SETTLE_WITHIN.as_secs()
            ));
        }
        clock.pause(STEP);
    }
}

/// A joint's position and goal, in counts, from the body's newest reading.
fn read(bus: &Bus, joint: Joint) -> Option<(u64, f64, f64)> {
    let message = bus.latest(JointState::TOPIC).ok()??;
    let joints = message.parse::<JointState>().ok()?;
    let at = joints.name.iter().position(|n| n == joint.name())?;
    Some((
        message.header.seq,
        *joints.position.get(at)? * COUNTS_A_RADIAN,
        *joints.commanded.get(at)? * COUNTS_A_RADIAN,
    ))
}

/// How old the body's newest `body/joint_states` is, by its wall clock.
fn newest_age(bus: &Bus) -> Option<Duration> {
    let message = bus.latest(JointState::TOPIC).ok()??;
    Some(Duration::from_millis(
        unix_ms().saturating_sub(message.header.at_unix_ms),
    ))
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// How long ago someone other than the self-test last asked on
/// `expression/body`, by its kept value: before the check, for the quiet
/// minute. While it runs, [`Watch`] hears every request.
fn others_asked(bus: &Bus) -> Option<Duration> {
    let message = bus.latest(ExpressionBody::TOPIC).ok()??;
    (message.header.source != SOURCE)
        .then(|| Duration::from_millis(unix_ms().saturating_sub(message.header.at_unix_ms)))
}

fn pushed(bus: &Bus) -> bool {
    matches!(
        listen::newest::<Hand>(bus, Hand::TOPIC),
        Ok(Some(seen)) if seen.value.pushed && seen.age <= listen::FAST_FRESH
    )
}

/// The body's newest report: the first to come within `within`, or with
/// no time given, the newest of those already come.
fn body_report(diagnostics: &mut lamp_bus::Subscriber, within: Duration) -> Option<BodyReport> {
    let until = Instant::now() + within;
    let mut newest = None;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        match diagnostics.recv(left) {
            Ok(Some(message)) => {
                if let Some(report) = message
                    .parse::<Diagnostic>()
                    .ok()
                    .as_ref()
                    .and_then(BodyReport::of)
                {
                    newest = Some(report);
                    if !within.is_zero() {
                        return newest;
                    }
                }
            }
            Ok(None) | Err(_) => return newest,
        }
    }
}

/// Asks on `expression/body`, the way Expression does.
struct Asker {
    out: lamp_bus::Publisher,
    clock_id: String,
    session_id: String,
    sequence: u64,
}

impl Asker {
    fn new(bus: &Bus) -> Result<Self, String> {
        let out = bus
            .publisher_declared(
                ExpressionBody::TOPIC,
                &ExpressionBody::declaration(),
                SOURCE,
            )
            .map_err(|e| e.to_string())?;
        let clock_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .map_err(|e| format!("this machine's boot cannot be named: {e}"))?
            .trim()
            .to_owned();
        let session_id = format!(
            "selftest-{}-{}",
            std::process::id(),
            lamp_ipc::monotonic_us()
        );
        Ok(Self {
            out,
            clock_id,
            session_id,
            sequence: 0,
        })
    }

    fn ask(&mut self, offsets: BodyOffsets) -> Result<(), String> {
        let now = lamp_ipc::monotonic_us();
        self.sequence += 1;
        let request = ExpressionBody {
            lease: ExpressionLease {
                clock_id: self.clock_id.clone(),
                session_id: self.session_id.clone(),
                voice_session_id: SOURCE.into(),
                sequence: self.sequence,
                turn: 1,
                request: 1,
                issued_us: now,
                apply_us: now,
                valid_until_us: now + EXPRESSION_LEASE_US,
            },
            offsets,
            hold: false,
            gaze: None,
        };
        request.check().map_err(|why| format!("{why:?}"))?;
        self.out
            .publish_json(now, &request)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// How long ago `at` was, by `clock`: what `at.elapsed()` is on the lamp.
fn since(clock: &impl Clock, at: Instant) -> Duration {
    clock.now().saturating_duration_since(at)
}

fn ease(share: f64) -> f64 {
    0.5 - 0.5 * (PI * share.clamp(0.0, 1.0)).cos()
}

/// What is listened to for the whole check: the body's reports, and every
/// request on `expression/body`, so that another asker is seen whenever it
/// asks, not only if its request is the newest kept.
struct Watch {
    diagnostics: lamp_bus::Subscriber,
    asked: lamp_bus::Subscriber,
}

impl Watch {
    fn on(bus: &Bus) -> Result<Self, String> {
        let open = |topic: &str, type_name: &str| {
            bus.subscribe_typed(topic, SOURCE, type_name)
                .map_err(|error| format!("{topic} cannot be read: {error}"))
        };
        Ok(Self {
            diagnostics: open(Diagnostic::TOPIC, Diagnostic::TYPE)?,
            asked: open(ExpressionBody::TOPIC, ExpressionBody::TYPE)?,
        })
    }

    /// What ends the check while it moves: a hand on the lamp, anyone else
    /// asking the body, the lamp leaving rest, or its starting to fold.
    fn interruption(&mut self, bus: &Bus) -> Option<String> {
        if pushed(bus) {
            return Some("a hand was put on the lamp".into());
        }
        let mut other = None;
        while let Ok(Some(message)) = self.asked.recv(Duration::ZERO) {
            if message.header.source != SOURCE {
                other = Some(message.header.source);
            }
        }
        if let Some(source) = other {
            return Some(format!("{source} asked the body to move"));
        }
        if let Some(body) = body_report(&mut self.diagnostics, Duration::ZERO) {
            if body.mood != "resting" {
                return Some(format!("the lamp left rest: it is {}", body.mood));
            }
            if body.standing < STANDING {
                return Some(format!(
                    "the lamp began to fold (standing {:.2})",
                    body.standing
                ));
            }
        }
        None
    }
}

/// One move: eased out and held until met or [`REACH_WITHIN`], eased back,
/// and watched until back or [`BACK_WITHIN`].
fn one_move(
    bus: &Bus,
    asker: &mut Asker,
    watch: &mut Watch,
    joint: Joint,
    sign: f64,
    clock: &mut impl Clock,
) -> Result<Moved, String> {
    // Where it begins, from a reading the body made just now: a kept one
    // from a body that has stopped would make a false start (the eighth
    // on #983).
    let age = newest_age(bus);
    if age.is_some_and(|age| age > listen::FAST_FRESH) {
        return Err(format!(
            "lamp-body is not publishing {} (newest {:.1} s old)",
            JointState::TOPIC,
            age.unwrap_or_default().as_secs_f64()
        ));
    }
    // From where the joint holds still, not from where the last move's
    // return was counted while it was still settling: the next return is
    // judged against this start (Tests on #1329: the merged test passed by
    // 0.1 count of `NEAR`, and a head settling a little slower fails).
    let (start, goal_was) = settled(bus, watch, joint, clock)?;
    let target = joint.counts() * sign;
    let began = clock.now();
    let mut samples: Vec<Sample> = Vec::new();
    let mut last_seq = None;
    let mut met = None;
    while since(clock, began) < REACH_WITHIN {
        let share = ease(since(clock, began).as_secs_f64() / EASE.as_secs_f64());
        asker.ask(joint.offsets(target * share))?;
        if let Some((seq, position, commanded)) = read(bus, joint)
            && last_seq != Some(seq)
        {
            last_seq = Some(seq);
            samples.push(Sample {
                at: since(clock, began),
                position,
                commanded,
            });
            met = reached(start, goal_was, sign, joint.counts(), &samples);
            if met.is_some() {
                break;
            }
        }
        if let Some(why) = watch.interruption(bus) {
            return Err(why);
        }
        clock.pause(STEP);
    }
    // Let go the way it was asked: eased back. Each request is renewed
    // only if nothing has ended the check.
    let letting_go = clock.now();
    while since(clock, letting_go) < EASE {
        if let Some(why) = watch.interruption(bus) {
            return Err(why);
        }
        let share = 1.0 - ease(since(clock, letting_go).as_secs_f64() / EASE.as_secs_f64());
        asker.ask(joint.offsets(target * share))?;
        clock.pause(STEP);
    }
    let Some((reached_at, goal)) = met else {
        let moved = samples
            .iter()
            .map(|s| (s.commanded - goal_was) * sign)
            .fold(0.0f64, f64::max);
        if moved < 0.5 {
            return Ok(Moved::NotTaken);
        }
        let wanted = expected(moved, joint.counts());
        let gap = samples
            .last()
            .map_or(f64::NAN, |s| (s.position - (start + sign * wanted)).abs());
        return Ok(Moved::NotReached { goal: wanted, gap });
    };
    let mut away = f64::NAN;
    while since(clock, letting_go) < BACK_WITHIN {
        // Looked at before a return is taken as done, so nothing that came
        // during it is missed.
        if let Some(why) = watch.interruption(bus) {
            return Err(why);
        }
        if let Some((_, position, _)) = read(bus, joint) {
            away = (position - start).abs();
            if away <= NEAR {
                return Ok(Moved::Done {
                    reached: reached_at,
                    goal,
                    back: since(clock, letting_go),
                });
            }
        }
        clock.pause(STEP);
    }
    Ok(Moved::NotBack { goal, away })
}

pub struct HeadMove<'a> {
    pub bus: &'a Bus,
    pub occasion: Occasion,
}

impl HeadMove<'_> {
    fn moves(&mut self, deadline: Instant, clock: &mut impl Clock) -> PartResult {
        let mut watch = match Watch::on(self.bus) {
            Ok(watch) => watch,
            Err(why) => return PartResult::new(PART, Verdict::Fail, &why),
        };
        let before = Before {
            occasion: self.occasion,
            body: body_report(&mut watch.diagnostics, REPORT_WITHIN),
            pushed: pushed(self.bus),
            others_asked: others_asked(self.bus),
        };
        if let Err(result) = may_move(&before) {
            return result;
        }
        let mut asker = match Asker::new(self.bus) {
            Ok(asker) => asker,
            Err(why) => return PartResult::new(PART, Verdict::Fail, &why),
        };
        let mut tried = Vec::new();
        for (joint, sign) in MOVES {
            if clock.now() + ONE_MOVE > deadline {
                return judge(&tried, Some("out of time"));
            }
            match one_move(self.bus, &mut asker, &mut watch, joint, sign, clock) {
                Ok(moved) => {
                    let failed = matches!(moved, Moved::NotReached { .. } | Moved::NotBack { .. });
                    tried.push(Tried { joint, sign, moved });
                    // A fail is reported, never tried again or after.
                    if failed {
                        break;
                    }
                }
                Err(why) => return judge(&tried, Some(&why)),
            }
        }
        judge(&tried, None)
    }
}

impl Check for HeadMove<'_> {
    fn part(&self) -> &str {
        PART
    }

    fn run(&mut self, deadline: Instant) -> PartResult {
        self.moves(deadline, &mut Host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::KeyValue;

    fn report(mood: &str, standing: f64, warmest: u32) -> Diagnostic {
        Diagnostic {
            level: Level::Ok,
            name: "body".into(),
            message: String::new(),
            values: [
                ("mood", mood.to_owned()),
                ("standing", format!("{standing:.2}")),
                ("warmest_joint", "head_pitch".into()),
                ("warmest_reading", warmest.to_string()),
            ]
            .into_iter()
            .map(|(key, value)| KeyValue {
                key: key.into(),
                value,
            })
            .collect(),
        }
    }

    fn before(body: Option<Diagnostic>) -> Before {
        Before {
            occasion: Occasion::Factory,
            body: body.as_ref().and_then(BodyReport::of),
            pushed: false,
            others_asked: None,
        }
    }

    fn why(result: Result<(), PartResult>) -> String {
        let result = result.unwrap_err();
        assert_eq!(result.verdict, Verdict::NotChecked);
        result.reason
    }

    #[test]
    fn a_standing_lamp_at_rest_may_be_moved() {
        assert_eq!(may_move(&before(Some(report("resting", 1.0, 40)))), Ok(()));
        let mut long_ago = before(Some(report("resting", 0.98, 40)));
        long_ago.others_asked = Some(Duration::from_secs(61));
        assert_eq!(may_move(&long_ago), Ok(()));
    }

    #[test]
    fn nothing_moves_unless_at_rest_standing_untouched_and_quiet() {
        let mut first_boot = before(Some(report("resting", 1.0, 40)));
        first_boot.occasion = Occasion::FirstBoot;
        assert!(why(may_move(&first_boot)).contains("first boot"));
        assert!(why(may_move(&before(None))).contains("lamp-body said nothing"));
        assert!(why(may_move(&before(Some(report("speaking", 1.0, 40))))).contains("speaking"));
        assert!(why(may_move(&before(Some(report("resting", 0.0, 40))))).contains("not standing"));
        assert!(why(may_move(&before(Some(report("resting", 1.0, 62))))).contains("warm"));
        let mut touched = before(Some(report("resting", 1.0, 40)));
        touched.pushed = true;
        assert!(why(may_move(&touched)).contains("hand"));
        let mut surprised = before(Some(report("resting", 1.0, 40)));
        surprised.others_asked = Some(Duration::from_secs(20));
        assert!(why(may_move(&surprised)).contains("20 s ago"));
        let mut unwell = report("resting", 1.0, 40);
        unwell.level = Level::Warn;
        unwell.message = "a joint met resistance".into();
        assert!(why(may_move(&before(Some(unwell)))).contains("resistance"));
        let mut other = report("resting", 1.0, 40);
        other.name = "ears".into();
        assert!(BodyReport::of(&other).is_none());
    }

    fn at(ms: u64, position: f64, commanded: f64) -> Sample {
        Sample {
            at: Duration::from_millis(ms),
            position,
            commanded,
        }
    }

    #[test]
    fn a_goal_is_met_once_it_holds_and_the_joint_is_near() {
        // The goal eases 40 counts down, holds, and the joint follows.
        let samples = [
            at(0, 1700.0, 1700.0),
            at(100, 1702.0, 1710.0),
            at(200, 1712.0, 1725.0),
            at(300, 1726.0, 1738.0),
            at(400, 1735.0, 1740.0),
            at(500, 1738.0, 1740.0),
            at(600, 1739.0, 1740.0),
        ];
        assert_eq!(
            reached(1700.0, 1700.0, 1.0, 40.0, &samples),
            Some((Duration::from_millis(600), 40.0))
        );
        // The other way it was never met.
        assert_eq!(reached(1700.0, 1700.0, -1.0, 40.0, &samples), None);
        // Lifting a relaxed head, the body writes its goal 40 past the pose
        // it wants and the head stops about 30 short of that goal: met
        // where it was asked to go, and the move is the asked 40.
        let lifted = [
            at(0, 1700.0, 1700.0),
            at(300, 1680.0, 1650.0),
            at(600, 1652.0, 1620.0),
            at(700, 1650.0, 1620.0),
            at(800, 1650.0, 1620.0),
            at(900, 1650.0, 1620.0),
        ];
        assert_eq!(
            reached(1700.0, 1700.0, -1.0, 40.0, &lifted),
            Some((Duration::from_millis(800), 40.0))
        );
        // A relaxed head sits a few counts off its goal: judged from where
        // the joint began.
        let off = [
            at(0, 1705.0, 1700.0),
            at(300, 1745.0, 1740.0),
            at(600, 1745.0, 1740.0),
        ];
        assert_eq!(
            reached(1705.0, 1700.0, 1.0, 40.0, &off),
            Some((Duration::from_millis(600), 40.0))
        );
        // A goal cut to one count, as lamp-52e6's roll is, is met.
        let roll = [
            at(0, 2035.0, 2035.0),
            at(100, 2035.0, 2036.0),
            at(400, 2036.0, 2036.0),
        ];
        assert_eq!(
            reached(2035.0, 2035.0, 1.0, 20.0, &roll),
            Some((Duration::from_millis(400), 1.0))
        );
        // A joint left behind its goal is not.
        let stuck = [
            at(0, 1700.0, 1700.0),
            at(300, 1700.0, 1740.0),
            at(600, 1701.0, 1740.0),
        ];
        assert_eq!(reached(1700.0, 1700.0, 1.0, 40.0, &stuck), None);
    }

    fn done(joint: Joint, sign: f64) -> Tried {
        Tried {
            joint,
            sign,
            moved: Moved::Done {
                reached: Duration::from_millis(900),
                goal: 40.0,
                back: Duration::from_millis(1_100),
            },
        }
    }

    #[test]
    fn all_four_moves_met_and_back_pass_with_their_times() {
        let tried: Vec<_> = MOVES
            .iter()
            .map(|(joint, sign)| done(*joint, *sign))
            .collect();
        let result = judge(&tried, None);
        assert_eq!(result.verdict, Verdict::Pass);
        assert!(
            result
                .values
                .iter()
                .any(|v| v.key == "head_pitch_minus_ms" && v.value == "900/1100")
        );
    }

    #[test]
    fn a_move_not_followed_or_not_back_fails_and_names_it() {
        let mut tried = vec![done(Joint::Pitch, -1.0)];
        tried.push(Tried {
            joint: Joint::Pitch,
            sign: 1.0,
            moved: Moved::NotReached {
                goal: 40.0,
                gap: 38.0,
            },
        });
        let result = judge(&tried, None);
        assert_eq!(result.verdict, Verdict::Fail);
        assert!(
            result
                .reason
                .starts_with("head_pitch down: the goal moved 40"),
            "{}",
            result.reason
        );
        let back = [Tried {
            joint: Joint::Roll,
            sign: 1.0,
            moved: Moved::NotBack {
                goal: 20.0,
                away: 19.0,
            },
        }];
        assert!(
            judge(&back, None)
                .reason
                .contains("still 19 from where it began")
        );
    }

    #[test]
    fn a_move_never_taken_or_cut_short_is_not_checked() {
        let none: Vec<_> = MOVES
            .iter()
            .map(|(joint, sign)| Tried {
                joint: *joint,
                sign: *sign,
                moved: Moved::NotTaken,
            })
            .collect();
        let result = judge(&none, None);
        assert_eq!(result.verdict, Verdict::NotChecked);
        assert!(result.reason.contains("LAMP_EXPRESSION_BODY"));
        let pitch_only = [done(Joint::Pitch, -1.0), done(Joint::Pitch, 1.0)];
        assert!(judge(&pitch_only, None).reason.contains("head_roll"));
        // Each asked move is judged: one never taken is not a pass.
        let not_taken = |joint, sign| Tried {
            joint,
            sign,
            moved: Moved::NotTaken,
        };
        let one_down_missing = [
            done(Joint::Pitch, -1.0),
            not_taken(Joint::Pitch, 1.0),
            done(Joint::Roll, 1.0),
            done(Joint::Roll, -1.0),
        ];
        let result = judge(&one_down_missing, None);
        assert_eq!(result.verdict, Verdict::NotChecked);
        assert_eq!(
            result.reason,
            "the body did not move the goal for head_pitch down"
        );
        let one_sided_roll = [
            done(Joint::Pitch, -1.0),
            done(Joint::Pitch, 1.0),
            done(Joint::Roll, 1.0),
            not_taken(Joint::Roll, -1.0),
        ];
        let result = judge(&one_sided_roll, None);
        assert_eq!(result.verdict, Verdict::NotChecked);
        assert!(
            result.reason.ends_with("head_roll the other way"),
            "{}",
            result.reason
        );
        let cut = judge(&pitch_only, Some("a hand was put on the lamp"));
        assert_eq!(cut.verdict, Verdict::NotChecked);
        assert_eq!(cut.reason, "stopped: a hand was put on the lamp");
    }

    /// How a fake body treats what it is asked.
    #[cfg(target_os = "linux")]
    #[derive(Clone, Copy)]
    enum Fake {
        Follows,
        PitchStuck,
        TakesNothing,
        /// Starts to fold a little over a second in.
        Folds,
        /// Expression asks too, a little over a second in.
        OtherAsks,
        /// Starts to fold as the last move, the roll the other way, is
        /// being eased back.
        FoldsLettingGoLast,
        /// Says its joints once and then never again, as a body stopped.
        Silent,
    }

    /// A body on `bus` as lamp-52e6's: it takes requests on
    /// `expression/body`, moves its goals at 120 counts a second, writes a
    /// lifting head's goal 40 counts ahead, where the head stops 30 short
    /// (`GLANCE_LIFT_LEAD`), cuts the roll to one count, lets the joints
    /// follow, and says it rests, standing, until stopped.
    ///
    /// It runs on the host's clock until the check first pauses, so its
    /// report and its first readings come as on a lamp. From then on it
    /// takes one step for each of the check's pauses ([`Stepped`]).
    #[cfg(target_os = "linux")]
    fn body(
        root: std::path::PathBuf,
        fake: Fake,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        steps: Steps,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let bus = Bus::new(&root).unwrap();
            let mut joints = bus
                .publisher_declared(JointState::TOPIC, &JointState::declaration(), "body")
                .unwrap();
            let mut diagnostics = bus
                .publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), "body")
                .unwrap();
            let mut asked = bus
                .subscribe_typed(ExpressionBody::TOPIC, "body", ExpressionBody::TYPE)
                .unwrap();
            let mut expression = Asker::new(&bus).unwrap();
            expression.out = bus
                .publisher_declared(
                    ExpressionBody::TOPIC,
                    &ExpressionBody::declaration(),
                    "expression",
                )
                .unwrap();
            let rest = [1771.0, 2035.0];
            let (mut goal, mut position) = (rest, rest);
            let mut offsets = None::<(BodyOffsets, u64)>;
            let (mut deepest_roll, mut folding) = (0.0f64, false);
            let mut step = 0u64;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let stepped = steps.granted.load(std::sync::atomic::Ordering::SeqCst) > 0;
                if stepped
                    && steps.taken.load(std::sync::atomic::Ordering::SeqCst)
                        >= steps.granted.load(std::sync::atomic::Ordering::SeqCst)
                {
                    std::thread::sleep(Duration::from_micros(50));
                    continue;
                }
                while let Ok(Some(message)) = asked.recv(Duration::ZERO) {
                    if let Ok(request) = message.parse::<ExpressionBody>() {
                        let roll = request.offsets.head_roll;
                        deepest_roll = deepest_roll.min(roll);
                        if matches!(fake, Fake::FoldsLettingGoLast)
                            && !folding
                            && deepest_roll < 0.0
                            && roll > deepest_roll * 0.9
                        {
                            folding = true;
                            diagnostics
                                .publish_json(lamp_ipc::monotonic_us(), &report("resting", 0.6, 40))
                                .unwrap();
                        }
                        // A lease lasts its length in the body's own steps,
                        // so it lapses on the stepped clock as it does on the
                        // host's (the reviewer on #1373).
                        let lasts = request
                            .lease
                            .valid_until_us
                            .saturating_sub(request.lease.issued_us)
                            / STEP.as_micros() as u64;
                        offsets = Some((request.offsets, step + lasts));
                    }
                }
                let now = lamp_ipc::monotonic_us();
                let wanted = match offsets {
                    Some((o, until)) if until > step && !matches!(fake, Fake::TakesNothing) => [
                        rest[0] + (o.head_pitch * COUNTS_A_RADIAN).clamp(-50.0, 50.0),
                        rest[1] + (o.head_roll * COUNTS_A_RADIAN).clamp(-1.0, 1.0),
                    ],
                    _ => rest,
                };
                let lead = if wanted[0] < rest[0] - 0.5 { 40.0 } else { 0.0 };
                let goal_wanted = [wanted[0] - lead, wanted[1]];
                for joint in 0..2 {
                    goal[joint] += (goal_wanted[joint] - goal[joint]).clamp(-2.4, 2.4);
                    let short = if joint == 0 { lead * 0.75 } else { 0.0 };
                    if joint == 1 || !matches!(fake, Fake::PitchStuck) {
                        position[joint] += (goal[joint] + short - position[joint]) * 0.5;
                    }
                }
                if matches!(fake, Fake::OtherAsks) && step == 60 {
                    expression.ask(BodyOffsets::ZERO).unwrap();
                }
                let standing = if (matches!(fake, Fake::Folds) && step >= 60) || folding {
                    0.6
                } else {
                    1.0
                };
                let radians = |counts: f64| counts / COUNTS_A_RADIAN;
                let state = JointState {
                    name: ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
                        .map(str::to_owned)
                        .to_vec(),
                    position: [2035.0, 2657.0, 2602.0, position[1], position[0]]
                        .map(radians)
                        .to_vec(),
                    velocity: Vec::new(),
                    effort: Vec::new(),
                    commanded: [2035.0, 2657.0, 2602.0, goal[1], goal[0]]
                        .map(radians)
                        .to_vec(),
                };
                if step == 0 || !matches!(fake, Fake::Silent) {
                    joints.publish_json(now, &state).unwrap();
                }
                if step.is_multiple_of(10) {
                    diagnostics
                        .publish_json(now, &report("resting", standing, 40))
                        .unwrap();
                }
                step += 1;
                if stepped {
                    steps
                        .taken
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                } else {
                    std::thread::sleep(STEP);
                }
            }
        })
    }

    /// The check's pauses granted to the fake body, and the steps it took.
    #[cfg(target_os = "linux")]
    #[derive(Clone, Default)]
    struct Steps {
        granted: std::sync::Arc<std::sync::atomic::AtomicU64>,
        taken: std::sync::Arc<std::sync::atomic::AtomicU64>,
    }

    /// The check's clock in a test: it stands still until the check pauses.
    /// Each pause is one step of the fake body, and the clock moves on only
    /// once the body has taken it. A step is the body's 20 ms (`STEP`), as
    /// on the host's clock, without waiting for it.
    #[cfg(target_os = "linux")]
    struct Stepped {
        at: Instant,
        steps: Steps,
    }

    #[cfg(target_os = "linux")]
    impl Clock for Stepped {
        fn now(&self) -> Instant {
            self.at
        }
        fn pause(&mut self, pause: Duration) {
            let granted = self
                .steps
                .granted
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                + 1;
            let until = Instant::now() + Duration::from_secs(5);
            while self.steps.taken.load(std::sync::atomic::Ordering::SeqCst) < granted {
                assert!(
                    Instant::now() < until,
                    "the fake body never took step {granted}"
                );
                std::thread::sleep(Duration::from_micros(50));
            }
            self.at += pause;
        }
    }

    #[cfg(target_os = "linux")]
    fn run_against(fake: Fake, name: &str) -> PartResult {
        let root = std::path::PathBuf::from(format!("/tmp/hm-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let steps = Steps::default();
        let fake_body = body(root.clone(), fake, stop.clone(), steps.clone());
        // A silent body's one reading is let grow old first.
        let settle = if matches!(fake, Fake::Silent) {
            1_500
        } else {
            300
        };
        std::thread::sleep(Duration::from_millis(settle));
        let bus = Bus::new(&root).unwrap();
        let mut check = HeadMove {
            bus: &bus,
            occasion: Occasion::OnDemand,
        };
        let mut clock = Stepped {
            at: Instant::now(),
            steps,
        };
        let deadline = clock.at + Duration::from_secs(60);
        let result = check.moves(deadline, &mut clock);
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        fake_body.join().unwrap();
        let _ = std::fs::remove_dir_all(&root);
        result
    }

    // The requests' clock is named by this boot's id, which only Linux has.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_body_that_follows_passes_and_one_that_does_not_fails_or_is_not_checked() {
        let follows = run_against(Fake::Follows, "follows");
        assert_eq!(follows.verdict, Verdict::Pass, "{follows:?}");
        let value = |key: &str| {
            follows
                .values
                .iter()
                .find(|v| v.key == key)
                .map(|v| v.value.clone())
        };
        // Lifted as asked although the goal was written 40 ahead; the roll
        // as far as the body cut it.
        assert_eq!(value("head_pitch_minus_goal_counts").as_deref(), Some("40"));
        assert_eq!(value("head_roll_plus_goal_counts").as_deref(), Some("1"));
        let stuck = run_against(Fake::PitchStuck, "stuck");
        assert_eq!(stuck.verdict, Verdict::Fail, "{stuck:?}");
        assert!(
            stuck.reason.starts_with("head_pitch up"),
            "{}",
            stuck.reason
        );
        let deaf = run_against(Fake::TakesNothing, "deaf");
        assert_eq!(deaf.verdict, Verdict::NotChecked, "{deaf:?}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn another_asker_or_folding_during_the_moves_ends_them_not_checked() {
        let other = run_against(Fake::OtherAsks, "other");
        assert_eq!(other.verdict, Verdict::NotChecked, "{other:?}");
        assert_eq!(other.reason, "stopped: expression asked the body to move");
        let folds = run_against(Fake::Folds, "folds");
        assert_eq!(folds.verdict, Verdict::NotChecked, "{folds:?}");
        assert!(folds.reason.contains("began to fold"), "{}", folds.reason);
        // While the last move is let go, too: never a pass.
        let late = run_against(Fake::FoldsLettingGoLast, "late");
        assert_eq!(late.verdict, Verdict::NotChecked, "{late:?}");
        assert!(late.reason.contains("began to fold"), "{}", late.reason);
    }

    /// Tests on #1329: a move's start is read where the joint holds still,
    /// not where it still moves after the last return. A head pitch
    /// settling from 1768 toward 1771 gives 1771; one that drifts a count
    /// every 100 ms never holds still, and gives why within its second.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_moves_start_is_read_once_the_joint_holds_still() {
        let root = std::path::PathBuf::from(format!("/tmp/hm-settle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // A head pitch going to 1771 by a third of the way each 20 ms, or
        // drifting a count every 100 ms for ever.
        let head = |root: std::path::PathBuf,
                    shakes: bool,
                    stop: std::sync::Arc<std::sync::atomic::AtomicBool>| {
            std::thread::spawn(move || {
                let bus = Bus::new(&root).unwrap();
                let mut joints = bus
                    .publisher_declared(JointState::TOPIC, &JointState::declaration(), "body")
                    .unwrap();
                let (mut pitch, began) = (1768.0f64, std::time::Instant::now());
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    // Drifting a count every 100 ms by the clock can never hold
                    // still for 200 ms, however the readings are seen (the reviewer
                    // on #1340).
                    pitch = if shakes {
                        1771.0 + (began.elapsed().as_millis() / 100) as f64
                    } else {
                        pitch + (1771.0 - pitch) * 0.3
                    };
                    let radians = |counts: f64| counts / COUNTS_A_RADIAN;
                    let state = JointState {
                        name: ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"]
                            .map(str::to_owned)
                            .to_vec(),
                        position: [2035.0, 2657.0, 2602.0, 2035.0, pitch]
                            .map(radians)
                            .to_vec(),
                        velocity: Vec::new(),
                        effort: Vec::new(),
                        commanded: [2035.0, 2657.0, 2602.0, 2035.0, 1771.0]
                            .map(radians)
                            .to_vec(),
                    };
                    joints
                        .publish_json(lamp_ipc::monotonic_us(), &state)
                        .unwrap();
                    std::thread::sleep(STEP);
                }
            })
        };
        let bus = Bus::new(&root).unwrap();
        let mut watch = Watch::on(&bus).unwrap();
        for shakes in [false, true] {
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let moving = head(root.clone(), shakes, stop.clone());
            std::thread::sleep(Duration::from_millis(30));
            let got = settled(&bus, &mut watch, Joint::Pitch, &mut Host);
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            moving.join().unwrap();
            if shakes {
                assert!(got.unwrap_err().contains("did not hold still"));
            } else {
                let (start, _) = got.unwrap();
                assert!((start - 1771.0).abs() < 0.5, "{start}");
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A body that said its joints once and stopped is named, not taken for
    /// a head that never moved.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_body_that_stopped_saying_its_joints_is_named_not_checked() {
        let silent = run_against(Fake::Silent, "silent");
        assert_eq!(silent.verdict, Verdict::NotChecked, "{silent:?}");
        assert!(
            silent
                .reason
                .starts_with("stopped: lamp-body is not publishing body/joint_states (newest"),
            "{}",
            silent.reason
        );
    }
}
