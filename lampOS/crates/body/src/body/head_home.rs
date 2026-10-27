//! A head tipped up by hand, brought home under a torque cap before the
//! factory's rest read (#3504): `lamp-body-measure head-home`.
//!
//! The rest read (`poses::home_pitch`) refuses a head pitch more than
//! `HOME_PITCH_TOLERANCE` outside the body's home range, so a hand-moved head
//! never becomes this lamp's home. A head found over the range (tipped up;
//! a falling pitch count tips the head down) by no more than `WAKE_REACH`
//! is taken here, as the hardware team's bench tool
//! `robots/lamp/hardware/test/tools/head-nudge.py` takes it: the head pitch
//! only, RAM registers only (torque limit 48, goal 42, torque enable 40),
//! the torque capped at 50%, 3 counts every 80 ms down to the range's high
//! edge, stopped on load, lag, heat, a timeout, arrival or settling. Every
//! path that wrote anything ends torque off with the limit put back. On
//! lamp-52e6 there is no stop near home and the head stays where it is let
//! go (friction), so the read after it reads where it was brought.
use super::{Bus, force_write, goal_write, torque_write};
use crate::{
    Joint,
    body::{Limits, WAKE_REACH},
    inspection::{PortIo, Runtime},
    poses::HOME_PITCH_TOLERANCE,
    protocol::{ReadOnlyRequest, ReadWindow},
};
use std::time::Duration;

const PITCH: Joint = Joint::WristPitch;
/// The torque limit while it moves, of 1,000 (50%).
pub const CAP: u16 = 500;
/// Counts a step, and a step's time.
pub const STEP: u16 = 3;
pub const PERIOD: Duration = Duration::from_millis(80);
/// Stops at this load, of 1,000: it met something.
pub const STOP_LOAD: u16 = 450;
/// Stops when the head lags its goal this far: it is not following.
pub const MAX_LAG: u16 = 40;
pub const MAX_TEMPERATURE: u8 = 55;
pub const TIMEOUT: Duration = Duration::from_secs(30);
/// Arrived within this of the target; or settled, the goal in and still
/// (within a count) this long, as a servo under a small push holds short.
const ARRIVED: u16 = 4;
const SETTLED: Duration = Duration::from_secs(1);
/// After letting go, how long before the last read.
const LET_GO_WAIT: Duration = Duration::from_millis(1_500);
/// Tries at torque off and the limit put back.
const RESTORE_TRIES: u8 = 5;

/// Where a head found at `pitch` at rest is brought: the home range's high
/// edge, for a head over the range by more than the rest read's tolerance
/// and at most `WAKE_REACH`. Anything else is not moved, with why.
pub fn head_home_target(pitch: u16) -> Result<u16, String> {
    let (low, high) = Limits::FOLDED.pitch_home;
    if pitch < low {
        return Err(format!(
            "head pitch {pitch} is under the body's home range {low}..{high} (tipped down): never moved here; nothing moved"
        ));
    }
    let over = pitch.saturating_sub(high);
    if over <= HOME_PITCH_TOLERANCE {
        return Err(format!(
            "head pitch {pitch} is within {HOME_PITCH_TOLERANCE} of the body's home range {low}..{high}: the rest read takes it as it is; nothing moved"
        ));
    }
    if over > WAKE_REACH {
        return Err(format!(
            "head pitch {pitch} is {over} over the body's home range {low}..{high}, more than {WAKE_REACH}: not a head tipped up by hand; nothing moved"
        ));
    }
    Ok(high)
}

/// One step from `goal` toward `target`, never past it.
fn next_goal(goal: u16, target: u16) -> u16 {
    if goal < target {
        goal.saturating_add(STEP).min(target)
    } else {
        goal.saturating_sub(STEP).max(target)
    }
}

/// The head pitch's position, load (of 1,000) and temperature.
fn pitch_now<P: PortIo, R: Runtime>(bus: &mut Bus<'_, P, R>) -> Result<(u16, u16, u8), String> {
    let request = ReadOnlyRequest::sync_read(&[PITCH], ReadWindow::TELEMETRY)
        .map_err(|error| error.to_string())?;
    let mut now = (0, 0, 0);
    bus.read(&request, |reply| {
        let telemetry = reply.telemetry().map_err(|error| error.to_string())?;
        now = (
            telemetry.position().raw(),
            telemetry.load().magnitude_raw(),
            telemetry.temperature_raw(),
        );
        Ok(())
    })?;
    Ok(now)
}

/// The head pitch's torque enable (`profiles` reads the commanded three,
/// the head pitch first).
fn pitch_torque<P: PortIo, R: Runtime>(bus: &mut Bus<'_, P, R>) -> Result<u8, String> {
    debug_assert_eq!(super::COMMANDED[0], PITCH);
    Ok(bus.profiles()?[0].0)
}

fn percent(load: u16) -> u16 {
    load / 10
}

/// Bring a head found tipped up by hand home: read, decide
/// ([`head_home_target`]), move under the cap, let go, read again. Each
/// step is said. `Ok(true)` when it arrived or settled, `Ok(false)` when it
/// stopped on anything else (the head let go where it is), `Err` when
/// nothing was moved or the head could not be let go.
pub(crate) fn head_home<P: PortIo, R: Runtime>(
    io: &mut P,
    runtime: &mut R,
    say: &mut dyn FnMut(String),
) -> Result<bool, String> {
    let mut bus = Bus::new(io, runtime, None);
    let (pitch, load, temperature) = pitch_now(&mut bus)?;
    let torque = pitch_torque(&mut bus)?;
    let limit = bus.force_limits(&[PITCH])?[0];
    let (min, max) = bus.position_limits(PITCH)?;
    say(format!(
        "head pitch {pitch}, load {}%, {temperature} C, torque {}, torque limit {limit}, the servo's limits {min}-{max}",
        percent(load),
        if torque == 0 { "off" } else { "ON" }
    ));
    let target = head_home_target(pitch)?;
    if torque != 0 {
        return Err("torque is already on: something else holds the head; nothing moved".into());
    }
    if !(min..=max).contains(&target) {
        return Err(format!(
            "{target} is outside the servo's own limits {min}-{max}; nothing moved"
        ));
    }
    say(format!(
        "bringing the head from {pitch} to {target}, {STEP} counts every {} ms under a {}% torque cap; it stops at {}% load, {MAX_LAG} counts of lag, {MAX_TEMPERATURE} C or {} s, then lets go",
        PERIOD.as_millis(),
        percent(CAP),
        percent(STOP_LOAD),
        TIMEOUT.as_secs()
    ));
    let mut peak = 0;
    let moved = step_home(&mut bus, pitch, target, &mut peak, say);
    // Whatever happened: torque off, then the limit put back, tried again
    // a few times on a wire that fails for a moment.
    let mut restored = Err(String::new());
    for _ in 0..RESTORE_TRIES {
        restored = bus
            .write_acknowledged(PITCH, &torque_write(PITCH, 0), true)
            .and_then(|()| bus.write_acknowledged(PITCH, &force_write(PITCH, limit), true));
        if restored.is_ok() {
            break;
        }
        bus.settle(Duration::from_millis(20));
    }
    let why = match &moved {
        Ok((why, _)) => why.clone(),
        Err(error) => format!("error: {error}"),
    };
    say(format!("stopped: {why}; peak load {}%", percent(peak)));
    restored.map_err(|error| {
        format!("the head could not be let go ({error}): it may be held at a {}% cap; take the power off", percent(CAP))
    })?;
    say(format!("torque off, the torque limit back to {limit}"));
    bus.settle(LET_GO_WAIT);
    let (pitch, _, temperature) = pitch_now(&mut bus)?;
    let torque = pitch_torque(&mut bus)?;
    say(format!(
        "after letting go: head pitch {pitch}, torque {}, {temperature} C",
        if torque == 0 { "off" } else { "ON" }
    ));
    moved.map(|(_, home)| home)
}

/// The capped move itself: the cap, a goal where the head is, torque on,
/// then a step at a time. Returns why it stopped and whether it got home.
fn step_home<P: PortIo, R: Runtime>(
    bus: &mut Bus<'_, P, R>,
    from: u16,
    target: u16,
    peak: &mut u16,
    say: &mut dyn FnMut(String),
) -> Result<(String, bool), String> {
    bus.write_checked(PITCH, &force_write(PITCH, CAP))?;
    bus.write_checked(PITCH, &goal_write(PITCH, from))?;
    bus.write_checked(PITCH, &torque_write(PITCH, 1))?;
    let start = bus.runtime.now();
    let (mut goal, mut last, mut still_since) = (from, from, None);
    loop {
        if bus.runtime.cancelled().map_err(|error| error.to_string())? {
            return Ok(("cancelled".into(), false));
        }
        goal = next_goal(goal, target);
        bus.write_checked(PITCH, &goal_write(PITCH, goal))?;
        let due = bus.runtime.now() + PERIOD;
        bus.sleep_until(due);
        let (position, load, temperature) = pitch_now(bus)?;
        *peak = (*peak).max(load);
        let now = bus.runtime.now();
        say(format!(
            "  {:4.1} s: goal {goal}, pitch {position}, load {}%, {temperature} C",
            (now - start).as_secs_f32(),
            percent(load)
        ));
        if load >= STOP_LOAD {
            return Ok((
                format!(
                    "load {}% reached the stop ({}%): it met something",
                    percent(load),
                    percent(STOP_LOAD)
                ),
                false,
            ));
        }
        if goal.abs_diff(position) > MAX_LAG {
            return Ok((
                format!(
                    "{} counts of lag: it is not following",
                    goal.abs_diff(position)
                ),
                false,
            ));
        }
        if temperature >= MAX_TEMPERATURE {
            return Ok((format!("{temperature} C"), false));
        }
        if now - start > TIMEOUT {
            return Ok(("timeout".into(), false));
        }
        if goal == target && position.abs_diff(target) <= ARRIVED {
            return Ok((format!("reached {target}"), true));
        }
        if goal == target && position.abs_diff(last) <= 1 {
            let since = *still_since.get_or_insert(now);
            if now - since >= SETTLED {
                return Ok((
                    format!(
                        "settled at {position}, {} short of {target}",
                        position.abs_diff(target)
                    ),
                    true,
                ));
            }
        } else {
            still_since = None;
        }
        last = position;
    }
}
