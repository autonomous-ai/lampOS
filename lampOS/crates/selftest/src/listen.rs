//! The checks that only listen: each part's newest reading on the bus, and
//! how old it is. Nothing here asks a part to do anything.
//!
//! Each check is a judge, a plain function from what was heard to a
//! [`PartResult`], and [`Listen`], which reads the bus for it.
use crate::Check;
use lamp_bus::Bus;
use lamp_msgs::{
    AirState, BoardState, ControlsState, Faces, Health, JointState, Level, PartResult, RoomSound,
    TouchState, Verdict,
};
use serde::de::DeserializeOwned;
use std::{
    marker::PhantomData,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// A reading and how long ago it was published.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen<T> {
    pub age: Duration,
    pub value: T,
}

/// What the bus gave for a topic: its newest reading, nothing ever, or why
/// it could not be read.
pub type Heard<T> = Result<Option<Seen<T>>, String>;

/// The joints `lamp-body` reports, base to head.
pub const JOINTS: [&str; 5] = ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"];

/// Air is read every 5 s; three periods missed is a sensor that stopped.
pub const AIR_FRESH: Duration = Duration::from_secs(15);
/// The board is read every 5 s by `lamp-health`.
pub const BOARD_FRESH: Duration = Duration::from_secs(15);
/// The room level, the joints and the pads come ten times a second.
pub const FAST_FRESH: Duration = Duration::from_secs(1);

/// The eyes publish each look, a few times a second, while the camera is
/// open; a list older than this was not from a picture taken now.
pub const SIGHT_FRESH: Duration = Duration::from_secs(2);

/// A room microphone at or under this level gives nothing: even a quiet
/// room is well above it.
pub const SILENT_DB: f32 = -90.0;
/// A pad held this long while nobody is asked to touch it is stuck.
pub const STUCK: Duration = Duration::from_secs(10);
/// The hottest a board may be during the test. Under the SoC's own
/// throttling point; to be confirmed against lamp-4ace's trip points.
pub const HOT_C: f32 = 85.0;
/// Less of the disk free than this fails, as `lamp-health` calls it an error.
pub const DISK_FREE_LEAST: f32 = 0.03;

/// What a missing, stale or unreadable reading means, or the reading.
fn fresh<'a, T>(
    part: &str,
    heard: &'a Heard<T>,
    topic: &str,
    owner: &str,
    within: Duration,
) -> Result<&'a T, PartResult> {
    match heard {
        Err(why) => Err(PartResult::new(
            part,
            Verdict::Fail,
            &format!("{topic} cannot be read: {why}"),
        )),
        Ok(None) => Err(PartResult::new(
            part,
            Verdict::NotChecked,
            &format!("nothing was ever said on {topic}: is {owner} running?"),
        )),
        Ok(Some(seen)) if seen.age > within => Err(PartResult::new(
            part,
            Verdict::Fail,
            &format!(
                "the newest {topic} is {:.1} s old: {owner} stopped giving it",
                seen.age.as_secs_f64()
            ),
        )
        .with("age_s", format!("{:.1}", seen.age.as_secs_f64()))),
        Ok(Some(seen)) => Ok(&seen.value),
    }
}

/// The air sensor gives readings, in range.
pub fn air(heard: &Heard<AirState>) -> PartResult {
    let air = match fresh("air", heard, AirState::TOPIC, "lamp-air", AIR_FRESH) {
        Ok(air) => air,
        Err(result) => return result,
    };
    if let Err(why) = air.check() {
        return PartResult::new("air", Verdict::Fail, &format!("out of range: {why}"));
    }
    let values = [
        air.pm2_5.is_some(),
        air.humidity.is_some(),
        air.temperature.is_some(),
        air.co2_ppm.is_some(),
    ];
    if !values.contains(&true) {
        return PartResult::new("air", Verdict::Fail, "the sensor gives no values");
    }
    let mut result = PartResult::new("air", Verdict::Pass, "");
    if let Some(co2) = air.co2_ppm {
        result = result.with("co2_ppm", co2);
    }
    if let Some(t) = air.temperature {
        result = result.with("temperature_c", format!("{t:.1}"));
    }
    result
}

/// What `lamp-air` says when the sensor never gave a reading this boot
/// (the bus lives under `/run`, so `air/state` starts empty at each boot),
/// in `lamp-air`'s own words (`lamp_air::watch`). A sensor that never
/// answered since `lamp-air` started counts absent after three looks,
/// [`lamp_air::watch::ABSENT`]: then it is `not_fitted`, as on
/// lamp-06c1 and lamp-cb94, where the sensor is optional and not fitted
/// (the first builder, 17 Oct 13:57 and 14:50). Before that, "the sensor
/// does not answer", it is still being looked for: `not_checked`. Any other
/// warning fails, as [`owner_says`] does: one that answered and gives no
/// reading ([`lamp_air::watch::NO_READING`], lamp-4ace after a reboot, the
/// Seventh and the reviewer on #1324), or a bus that cannot be opened. A
/// sensor that gave a reading this boot is judged by its reading, so one
/// that answered and then fails (lamp-4ace) is a `fail`.
pub fn air_silent(health: &Heard<Health>) -> Option<PartResult> {
    let Ok(Some(seen)) = health else {
        return None;
    };
    let item = seen.value.items.iter().find(|item| item.name == "air")?;
    if item.message.starts_with(lamp_air::watch::NOT_FITTED)
        || item.message.starts_with(lamp_air::watch::ABSENT)
    {
        return Some(PartResult::new(
            "air",
            Verdict::NotFitted,
            &format!(
                "no air sensor has answered since lamp-air started (lamp-air: {})",
                item.message
            ),
        ));
    }
    if item.message.starts_with(lamp_air::watch::LOOKING) {
        return Some(PartResult::new(
            "air",
            Verdict::NotChecked,
            &format!("lamp-air is still looking for the sensor: {}", item.message),
        ));
    }
    owner_says("air", "air", health)
}

/// The room microphone hears something, however quiet.
pub fn room_mic(heard: &Heard<RoomSound>) -> PartResult {
    let part = "room_mic";
    let sound = match fresh(part, heard, RoomSound::TOPIC, "lamp-ears", FAST_FRESH) {
        Ok(sound) => sound,
        Err(result) => return result,
    };
    if let Err(why) = sound.check() {
        return PartResult::new(part, Verdict::Fail, &format!("out of range: {why}"));
    }
    if sound.level_db <= SILENT_DB {
        return PartResult::new(
            part,
            Verdict::Fail,
            &format!(
                "silent at {:.0} dBFS: the microphone gives nothing",
                sound.level_db
            ),
        );
    }
    PartResult::new(part, Verdict::Pass, "").with("level_db", format!("{:.1}", sound.level_db))
}

/// One joint answers: `lamp-body` reports where it is.
pub fn joint(name: &str, heard: &Heard<JointState>) -> PartResult {
    let joints = match fresh(name, heard, JointState::TOPIC, "lamp-body", FAST_FRESH) {
        Ok(joints) => joints,
        Err(result) => return result,
    };
    let Some(at) = joints.name.iter().position(|n| n == name) else {
        return PartResult::new(name, Verdict::Fail, "lamp-body does not report this joint");
    };
    match joints.position.get(at) {
        Some(p) if p.is_finite() => {
            PartResult::new(name, Verdict::Pass, "").with("position_rad", format!("{p:.3}"))
        }
        Some(p) => PartResult::new(name, Verdict::Fail, &format!("its position reads {p}")),
        None => PartResult::new(name, Verdict::Fail, "lamp-body gives no position for it"),
    }
}

/// Every touch source is ready, without a fault, and not held down.
pub fn touch(heard: &Heard<TouchState>) -> PartResult {
    let part = "touch";
    let state = match fresh(part, heard, TouchState::TOPIC, "lamp-controls", FAST_FRESH) {
        Ok(state) => state,
        Err(result) => return result,
    };
    if !state.enabled {
        return PartResult::new(part, Verdict::NotChecked, "touch reading is switched off");
    }
    if state.sources.is_empty() {
        return PartResult::new(part, Verdict::Fail, "no touch source reports");
    }
    let mut touched = Vec::new();
    for source in &state.sources {
        if let Some(fault) = &source.fault {
            return PartResult::new(part, Verdict::Fail, &format!("{}: {fault:?}", source.name));
        }
        if !source.ready {
            return PartResult::new(
                part,
                Verdict::Fail,
                &format!("{} is not ready", source.name),
            );
        }
        if source.active == Some(true) {
            let held = Duration::from_micros(source.held_us.unwrap_or(0));
            if held >= STUCK {
                return PartResult::new(
                    part,
                    Verdict::Fail,
                    &format!(
                        "{} stuck: held {:.0} s ({})",
                        source.name,
                        held.as_secs_f64(),
                        source.active_names.join(", ")
                    ),
                );
            }
            touched.push(source.name.as_str());
        }
    }
    if !touched.is_empty() {
        return PartResult::new(
            part,
            Verdict::NotChecked,
            &format!("touched during the check: {}", touched.join(", ")),
        );
    }
    PartResult::new(part, Verdict::Pass, "idle").with("sources", state.sources.len())
}

/// The board is cool enough and has room on its disk.
pub fn board(heard: &Heard<BoardState>) -> PartResult {
    let part = "board";
    let state = match fresh(part, heard, BoardState::TOPIC, "lamp-health", BOARD_FRESH) {
        Ok(state) => state,
        Err(result) => return result,
    };
    let hottest = state.temperature_c.iter().max_by(|a, b| a.1.total_cmp(b.1));
    if let Some((sensor, &c)) = hottest
        && c >= HOT_C
    {
        return PartResult::new(part, Verdict::Fail, &format!("{sensor} is at {c:.0} °C"));
    }
    if state.disk_free < DISK_FREE_LEAST {
        return PartResult::new(
            part,
            Verdict::Fail,
            &format!(
                "the disk is all but full: {:.1}% free",
                state.disk_free * 100.0
            ),
        );
    }
    let mut result = PartResult::new(part, Verdict::Pass, "")
        .with("disk_free", format!("{:.3}", state.disk_free));
    if let Some((sensor, c)) = hottest {
        result = result.with("hottest", format!("{sensor} {c:.0} °C"));
    }
    result
}

/// The camera gives pictures: the eyes published a look a moment ago. The
/// self-test never opens the camera itself; it is open only while seeing
/// is permitted (the privacy switch, `lamp-voice`), so a closed camera is
/// not checked, and a camera the eyes say will not open fails.
pub fn camera(sight: &Heard<Faces>, health: &Heard<Health>) -> PartResult {
    let part = "camera";
    if let Ok(Some(seen)) = sight
        && seen.age <= SIGHT_FRESH
    {
        return PartResult::new(part, Verdict::Pass, "")
            .with("age_s", format!("{:.1}", seen.age.as_secs_f64()));
    }
    let eyes = match health {
        Err(why) => {
            return PartResult::new(
                part,
                Verdict::Fail,
                &format!("{} cannot be read: {why}", Health::TOPIC),
            );
        }
        Ok(None) => {
            return PartResult::new(
                part,
                Verdict::NotChecked,
                &format!(
                    "nothing was ever said on {}: is lamp-health running?",
                    Health::TOPIC
                ),
            );
        }
        Ok(Some(seen)) => seen.value.items.iter().find(|item| item.name == "eyes"),
    };
    match eyes {
        None => PartResult::new(
            part,
            Verdict::NotChecked,
            "lamp-eyes does not report its health: is it running, with LAMP_BUS=on?",
        ),
        Some(item) if item.level == Level::Ok => PartResult::new(
            part,
            Verdict::NotChecked,
            "the camera is closed: seeing is not permitted now (privacy switch, or the voice is not listening)",
        ),
        Some(item) => PartResult::new(
            part,
            Verdict::Fail,
            &format!("lamp-eyes: {} ({})", item.message, item.level.as_str()),
        ),
    }
}

/// The camera's check reads two topics: the eyes' looks and the lamp's
/// health.
pub struct Sight<'a> {
    pub bus: &'a Bus,
}

impl Check for Sight<'_> {
    fn part(&self) -> &str {
        "camera"
    }

    fn run(&mut self, _deadline: Instant) -> PartResult {
        camera(
            &newest(self.bus, Faces::TOPIC),
            &newest(self.bus, Health::TOPIC),
        )
    }
}

/// What a part that never gave a reading says of itself, by the name it
/// reports under in `lamp/health`: its owner's own warning fails the part;
/// an owner that is well and has given nothing yet is not checked. `None`
/// when the owner is not in the lamp's health either.
pub fn owner_says(part: &str, owner: &str, health: &Heard<Health>) -> Option<PartResult> {
    let Ok(Some(seen)) = health else {
        return None;
    };
    let item = seen.value.items.iter().find(|item| item.name == owner)?;
    Some(if item.level == Level::Ok {
        PartResult::new(
            part,
            Verdict::NotChecked,
            &format!("{owner} runs and is well, but has given no reading yet"),
        )
    } else {
        let message = if item.message.is_empty() {
            "no reason given"
        } else {
            item.message.as_str()
        };
        PartResult::new(
            part,
            Verdict::Fail,
            &format!("{owner}: {message} ({})", item.level.as_str()),
        )
    })
}

/// The touch pads when `controls/touch_state` was never given but Controls
/// runs: not a missing part, and not known to be switched off either (the
/// touch runtime publishes `controls/state` too).
pub fn touch_unread(controls: &Heard<ControlsState>) -> Option<PartResult> {
    let Ok(Some(seen)) = controls else {
        return None;
    };
    (seen.age <= FAST_FRESH).then(|| {
        PartResult::new(
            "touch",
            Verdict::NotChecked,
            "no touch reading on controls/touch_state, though lamp-controls runs; whether it reads the pads is not known",
        )
    })
}

/// The newest reading on `topic`, and its age by the publisher's wall clock.
pub fn newest<T: DeserializeOwned>(bus: &Bus, topic: &str) -> Heard<T> {
    let Some(message) = bus.latest(topic).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let age = Duration::from_millis(now_ms.saturating_sub(message.header.at_unix_ms));
    let value = message.parse::<T>().map_err(|e| e.to_string())?;
    Ok(Some(Seen { age, value }))
}

/// How a reading on one topic becomes a part's result.
type Judge<'a, T> = Box<dyn Fn(&Heard<T>) -> PartResult + 'a>;

/// A check that reads one topic and judges it.
pub struct Listen<'a, T> {
    part: String,
    bus: &'a Bus,
    topic: &'static str,
    judge: Judge<'a, T>,
    /// What to say when the topic was never given a reading, if the bus
    /// knows better than "nothing was ever said".
    when_silent: Option<Silent<'a>>,
    kind: PhantomData<T>,
}

/// Another look at the bus for a part whose topic is silent.
type Silent<'a> = Box<dyn Fn(&Bus) -> Option<PartResult> + 'a>;

impl<'a, T: DeserializeOwned> Listen<'a, T> {
    pub fn new(
        part: &str,
        bus: &'a Bus,
        topic: &'static str,
        judge: impl Fn(&Heard<T>) -> PartResult + 'a,
    ) -> Self {
        Self {
            part: part.to_owned(),
            bus,
            topic,
            judge: Box::new(judge),
            when_silent: None,
            kind: PhantomData,
        }
    }

    /// Asks the part's owner, by its name in `lamp/health`, when the topic
    /// has never been given a reading.
    pub fn owner(mut self, owner: &'static str) -> Self {
        let part = self.part.clone();
        self.when_silent = Some(Box::new(move |bus: &Bus| {
            owner_says(&part, owner, &newest(bus, Health::TOPIC))
        }));
        self
    }

    /// Looks elsewhere on the bus when the topic has never been given a
    /// reading.
    pub fn when_silent(mut self, look: impl Fn(&Bus) -> Option<PartResult> + 'a) -> Self {
        self.when_silent = Some(Box::new(look));
        self
    }
}

impl<T: DeserializeOwned> Check for Listen<'_, T> {
    fn part(&self) -> &str {
        &self.part
    }

    fn run(&mut self, _deadline: Instant) -> PartResult {
        let heard = newest(self.bus, self.topic);
        if matches!(heard, Ok(None))
            && let Some(look) = &self.when_silent
            && let Some(result) = look(self.bus)
        {
            return result;
        }
        (self.judge)(&heard)
    }
}

/// Every check that only listens, in the order they run.
pub fn checks(bus: &Bus) -> Vec<Box<dyn Check + '_>> {
    let mut checks: Vec<Box<dyn Check + '_>> = vec![
        Box::new(Listen::new("room_mic", bus, RoomSound::TOPIC, room_mic).owner("ears")),
        Box::new(
            Listen::new("air", bus, AirState::TOPIC, air)
                .when_silent(|bus: &Bus| air_silent(&newest(bus, Health::TOPIC))),
        ),
        Box::new(
            Listen::new("touch", bus, TouchState::TOPIC, touch)
                .when_silent(|bus| touch_unread(&newest(bus, ControlsState::TOPIC))),
        ),
        Box::new(Listen::new("board", bus, BoardState::TOPIC, board)),
        Box::new(Sight { bus }),
    ];
    for name in JOINTS {
        checks.push(Box::new(
            Listen::new(
                name,
                bus,
                JointState::TOPIC,
                move |heard: &Heard<JointState>| joint(name, heard),
            )
            .owner("body"),
        ));
    }
    checks
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::AirFeel;
    use std::collections::BTreeMap;

    fn seen<T>(age_ms: u64, value: T) -> Heard<T> {
        Ok(Some(Seen {
            age: Duration::from_millis(age_ms),
            value,
        }))
    }

    fn good_air() -> AirState {
        AirState {
            pm1_0: Some(3.0),
            pm2_5: Some(4.0),
            pm4_0: Some(4.0),
            pm10: Some(4.0),
            humidity: Some(55.0),
            temperature: Some(31.0),
            co2_ppm: Some(650),
            feel: AirFeel::Fresh,
        }
    }

    #[test]
    fn air_in_range_and_fresh_passes() {
        let result = air(&seen(2_000, good_air()));
        assert_eq!(result.verdict, Verdict::Pass, "{result:?}");
        assert!(
            result
                .values
                .iter()
                .any(|v| v.key == "co2_ppm" && v.value == "650")
        );
    }

    #[test]
    fn a_reading_never_given_is_not_checked_and_an_old_one_fails() {
        let never = air(&Ok(None));
        assert_eq!(never.verdict, Verdict::NotChecked);
        assert!(never.reason.contains("lamp-air"), "{}", never.reason);
        let old = air(&seen(40_000, good_air()));
        assert_eq!(old.verdict, Verdict::Fail);
        assert!(old.reason.contains("40.0 s old"), "{}", old.reason);
        let unreadable = air(&Err("not JSON".into()));
        assert_eq!(unreadable.verdict, Verdict::Fail);
    }

    #[test]
    fn air_with_no_values_or_out_of_range_fails() {
        let empty = AirState {
            pm1_0: None,
            pm2_5: None,
            pm4_0: None,
            pm10: None,
            humidity: None,
            temperature: None,
            co2_ppm: None,
            feel: AirFeel::Unknown,
        };
        assert!(air(&seen(1, empty)).reason.contains("no values"));
        let wet = AirState {
            humidity: Some(140.0),
            ..good_air()
        };
        assert_eq!(air(&seen(1, wet)).verdict, Verdict::Fail);
    }

    fn sound(level_db: f32) -> RoomSound {
        RoomSound {
            level_db,
            floor_db: None,
            own_voice: false,
        }
    }

    #[test]
    fn a_quiet_room_passes_and_digital_silence_fails() {
        assert_eq!(room_mic(&seen(50, sound(-62.0))).verdict, Verdict::Pass);
        let dead = room_mic(&seen(50, sound(-120.0)));
        assert_eq!(dead.verdict, Verdict::Fail);
        assert!(dead.reason.contains("silent"), "{}", dead.reason);
        assert_eq!(room_mic(&seen(3_000, sound(-62.0))).verdict, Verdict::Fail);
    }

    fn joints() -> JointState {
        JointState {
            name: JOINTS.iter().map(|n| n.to_string()).collect(),
            position: vec![0.0, 0.4, -1.2, 0.01, f64::NAN],
            velocity: vec![],
            effort: vec![],
            commanded: vec![],
        }
    }

    #[test]
    fn each_joint_answers_for_itself() {
        let heard = seen(80, joints());
        assert_eq!(joint("shoulder", &heard).verdict, Verdict::Pass);
        assert_eq!(joint("head_pitch", &heard).verdict, Verdict::Fail);
        let gone = joint("wrist", &heard);
        assert!(gone.reason.contains("does not report"), "{}", gone.reason);
    }

    fn board_state(hottest: f32, disk_free: f32) -> BoardState {
        BoardState {
            temperature_c: BTreeMap::from([("cpu".into(), hottest), ("gpu".into(), 40.0)]),
            cpu_load: 0.3,
            memory_used: 0.4,
            disk_free,
            uptime_s: 600,
            wifi_dbm: None,
            clock_synced: None,
        }
    }

    #[test]
    fn a_hot_board_or_a_full_disk_fails() {
        let fine = board(&seen(1_000, board_state(55.0, 0.5)));
        assert_eq!(fine.verdict, Verdict::Pass, "{fine:?}");
        assert!(fine.values.iter().any(|v| v.value == "cpu 55 °C"));
        let hot = board(&seen(1_000, board_state(91.0, 0.5)));
        assert!(hot.reason.contains("cpu is at 91"), "{}", hot.reason);
        let full = board(&seen(1_000, board_state(55.0, 0.01)));
        assert!(full.reason.contains("all but full"), "{}", full.reason);
    }

    fn health_with(eyes: Option<(Level, &str)>) -> Health {
        let items: Vec<_> = eyes
            .map(|(level, message)| lamp_msgs::HealthItem {
                name: "eyes".into(),
                level,
                message: message.into(),
                age_ms: 300,
            })
            .into_iter()
            .collect();
        Health {
            level: items.first().map_or(Level::Ok, |i| i.level),
            message: String::new(),
            items,
            missing: vec![],
            quiet_ms: 0,
        }
    }

    #[test]
    fn a_camera_giving_looks_passes_and_one_that_will_not_open_fails() {
        let looks = seen(400, Faces { faces: vec![] });
        let health = seen(1_000, health_with(Some((Level::Ok, ""))));
        assert_eq!(camera(&looks, &health).verdict, Verdict::Pass);
        let stale = seen(30_000, Faces { faces: vec![] });
        let closed = camera(&stale, &health);
        assert_eq!(closed.verdict, Verdict::NotChecked);
        assert!(closed.reason.contains("not permitted"), "{}", closed.reason);
        let broken = seen(
            1_000,
            health_with(Some((Level::Warn, "the camera does not open"))),
        );
        let failed = camera(&Ok(None), &broken);
        assert_eq!(failed.verdict, Verdict::Fail);
        assert!(failed.reason.contains("does not open"), "{}", failed.reason);
        let absent = camera(&Ok(None), &seen(1_000, health_with(None)));
        assert!(absent.reason.contains("LAMP_BUS"), "{}", absent.reason);
        assert_eq!(camera(&Ok(None), &Ok(None)).verdict, Verdict::NotChecked);
    }

    fn health_of(name: &str, level: Level, message: &str) -> Health {
        Health {
            level,
            message: String::new(),
            items: vec![lamp_msgs::HealthItem {
                name: name.into(),
                level,
                message: message.into(),
                age_ms: 400,
            }],
            missing: vec![],
            quiet_ms: 0,
        }
    }

    #[test]
    fn a_silent_part_says_what_its_owner_says_of_itself() {
        let warned = seen(
            1_000,
            health_of("air", Level::Warn, "the sensor does not answer"),
        );
        let failed = owner_says("air", "air", &warned).unwrap();
        assert_eq!(failed.verdict, Verdict::Fail);
        assert!(
            failed.reason.contains("does not answer"),
            "{}",
            failed.reason
        );
        let well = seen(1_000, health_of("air", Level::Ok, ""));
        let waiting = owner_says("air", "air", &well).unwrap();
        assert_eq!(waiting.verdict, Verdict::NotChecked);
        assert!(
            waiting.reason.contains("no reading yet"),
            "{}",
            waiting.reason
        );
        assert!(owner_says("air", "air", &seen(1_000, health_of("body", Level::Ok, ""))).is_none());
        assert!(owner_says("air", "air", &Ok(None)).is_none());
    }

    /// The first builder, 17 Oct 14:50: no sensor found since boot is
    /// `not_fitted` (lamp-06c1, lamp-cb94); one that read and then failed is
    /// a `fail` (lamp-4ace); a bus that cannot be opened still fails.
    #[test]
    fn an_air_sensor_never_found_since_boot_is_not_fitted_and_one_that_failed_is_a_fail() {
        let absent = seen(
            1_000,
            health_of(
                "air",
                Level::Warn,
                "no air sensor answers on I2C bus 0 (command 0xd014: Invalid argument (os error 22)); looked for again every 10 min",
            ),
        );
        let fitted = air_silent(&absent).unwrap();
        assert_eq!(fitted.verdict, Verdict::NotFitted, "{fitted:?}");
        assert!(
            fitted.reason.starts_with(
                "no air sensor has answered since lamp-air started (lamp-air: no air sensor answers on I2C bus 0"
            ),
            "{}",
            fitted.reason
        );
        // lamp-air's own report now says it at `ok` (the first builder, 15:36).
        let ok_absent = air_silent(&seen(
            1_000,
            health_of("air", Level::Ok, lamp_air::watch::NOT_FITTED),
        ))
        .unwrap();
        assert_eq!(ok_absent.verdict, Verdict::NotFitted, "{ok_absent:?}");
        let silent = air_silent(&seen(
            1_000,
            health_of(
                "air",
                Level::Warn,
                &format!(
                    "{} on I2C bus 0 (word 0 fails its CRC)",
                    lamp_air::watch::NO_READING
                ),
            ),
        ))
        .unwrap();
        assert_eq!(silent.verdict, Verdict::Fail, "{silent:?}");
        let looking = air_silent(&seen(
            1_000,
            health_of(
                "air",
                Level::Warn,
                "the sensor does not answer: command 0xd014: Invalid argument (os error 22)",
            ),
        ))
        .unwrap();
        assert_eq!(looking.verdict, Verdict::NotChecked);
        let no_bus = air_silent(&seen(
            1_000,
            health_of(
                "air",
                Level::Warn,
                "I2C bus 0 cannot be opened (No such file or directory); looked at again every 5 s",
            ),
        ))
        .unwrap();
        assert_eq!(no_bus.verdict, Verdict::Fail);
        assert_eq!(
            air_silent(&seen(1_000, health_of("air", Level::Ok, "")))
                .unwrap()
                .verdict,
            Verdict::NotChecked
        );
        assert!(air_silent(&Ok(None)).is_none());
        // lamp-4ace: it read this boot, and its newest reading is stale.
        let stale = air(&seen(40_000, good_air()));
        assert_eq!(stale.verdict, Verdict::Fail, "{stale:?}");
    }

    /// The reviewer's P2 on #1324: the words are `lamp-air`'s, so its own
    /// loop is run here against a sensor that never answers and one that
    /// answers its name and never reads, and each report it says is judged.
    #[test]
    fn lamp_airs_own_reports_judge_a_missing_sensor_not_fitted_and_a_silent_one_failed() {
        use lamp_air::watch::{Clock, Missing, Say, Sensor, run};
        struct Bench {
            answers: bool,
            garbled: bool,
        }
        impl Sensor for Bench {
            fn find(&mut self) -> Result<String, Missing> {
                if self.answers {
                    Ok("SEN63C".into())
                } else if self.garbled {
                    Err(Missing::NoAnswer("word 0 fails its CRC".into()))
                } else {
                    Err(Missing::NoAnswer(
                        "command 0xd014: Invalid argument (os error 22)".into(),
                    ))
                }
            }
            fn ready(&mut self) -> Result<bool, String> {
                Err("word 0 fails its CRC".into())
            }
            fn read(&mut self) -> Result<AirState, String> {
                Err("never".into())
            }
            fn start(&mut self) -> Result<(), String> {
                Ok(())
            }
            fn lose(&mut self) {}
        }
        struct Steps(Duration);
        impl Clock for Steps {
            fn now(&mut self) -> Duration {
                self.0
            }
            fn sleep(&mut self, duration: Duration) {
                self.0 += duration;
            }
        }
        #[derive(Default)]
        struct Heard(Vec<(Duration, lamp_msgs::Diagnostic)>, Duration);
        impl Say for Heard {
            fn air(&mut self, _: &AirState) {}
            fn health(&mut self, report: &lamp_msgs::Diagnostic) {
                self.0.push((self.1, report.clone()));
            }
            fn log(&mut self, _: &str) {}
        }
        let judged = |answers: bool, garbled: bool| -> Vec<Verdict> {
            let (mut clock, mut said) = (Steps(Duration::ZERO), Heard::default());
            // `Heard` cannot see the clock; the reports come in order, which
            // is enough.
            run(
                &mut Bench { answers, garbled },
                &mut clock,
                &mut said,
                0,
                Duration::from_secs(60),
            );
            said.0
                .iter()
                .map(|(_, report)| {
                    let item = lamp_msgs::HealthItem {
                        name: "air".into(),
                        level: report.level,
                        message: report.message.clone(),
                        age_ms: 0,
                    };
                    let health = Health {
                        level: report.level,
                        message: String::new(),
                        items: vec![item],
                        missing: Vec::new(),
                        quiet_ms: 0,
                    };
                    air_silent(&seen(100, health)).unwrap().verdict
                })
                .collect()
        };
        let missing = judged(false, false);
        assert_eq!(missing.first(), Some(&Verdict::NotChecked), "{missing:?}");
        assert_eq!(missing.last(), Some(&Verdict::NotFitted), "{missing:?}");
        assert!(!missing.contains(&Verdict::Fail), "{missing:?}");
        let stuck = judged(true, false);
        assert_eq!(stuck.last(), Some(&Verdict::Fail), "{stuck:?}");
        assert!(!stuck.contains(&Verdict::NotFitted), "{stuck:?}");
        // Every look comes back failing its CRC: something answered, so a
        // fail, never not fitted (the reviewer's P2 on #1324).
        let garbled = judged(false, true);
        assert_eq!(garbled.last(), Some(&Verdict::Fail), "{garbled:?}");
        assert!(!garbled.contains(&Verdict::NotFitted), "{garbled:?}");
    }

    #[test]
    fn a_published_air_reading_is_read_from_what_the_bus_keeps_and_silence_asks_the_owner() {
        let root = std::path::PathBuf::from(format!("/tmp/ls-silent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        // lamp-air as on lamp-4ace after a power cut: alive, no reading.
        let mut sensor = bus
            .publisher_declared(AirState::TOPIC, &AirState::declaration(), "air")
            .unwrap();
        sensor.alive().unwrap();
        let mut health = bus
            .publisher_declared(Health::TOPIC, &Health::declaration(), "health")
            .unwrap();
        health
            .publish_json(
                1,
                &health_of("air", Level::Warn, "the sensor does not answer"),
            )
            .unwrap();
        // The air check as the self-test runs it (the reviewer's P3 on
        // #1324): its wiring in `checks` is what is tested.
        let mut check = checks(&bus)
            .into_iter()
            .find(|check| check.part() == "air")
            .unwrap();
        // Still within lamp-air's first looks: not checked yet.
        let silent = check.run(Instant::now());
        assert_eq!(silent.verdict, Verdict::NotChecked, "{silent:?}");
        assert!(
            silent.reason.starts_with("lamp-air is still looking"),
            "{}",
            silent.reason
        );
        // One reading later, the reading is judged; the owner is not asked.
        sensor.publish_json(2, &good_air()).unwrap();
        assert_eq!(check.run(Instant::now()).verdict, Verdict::Pass);
        drop((sensor, health));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn touch_with_no_reading_while_controls_runs_is_not_checked_and_not_called_missing() {
        let controls: ControlsState = serde_json::from_value(serde_json::json!({
            "stamp": {"clock_id": "controls", "at_us": 1, "sequence": 1},
            "session_id": "s1",
            "enabled": true,
            "ready": true,
            "buttons": [],
            "head_active": false,
            "base_ready": false,
            "base_tier": "none",
            "event_sequence": 0,
            "rejected_inputs": 0
        }))
        .unwrap();
        let result = touch_unread(&seen(50, controls.clone())).unwrap();
        assert_eq!(result.verdict, Verdict::NotChecked);
        assert!(result.reason.contains("not known"), "{}", result.reason);
        assert!(touch_unread(&seen(5_000, controls)).is_none());
        assert!(touch_unread(&Ok(None)).is_none());
    }

    #[test]
    fn every_part_that_listens_is_checked_once() {
        let root = std::path::PathBuf::from(format!("/tmp/ls-bus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut checks = checks(&bus);
        let names: Vec<_> = checks.iter().map(|c| c.part().to_owned()).collect();
        assert_eq!(
            names,
            [
                "room_mic",
                "air",
                "touch",
                "board",
                "camera",
                "base_yaw",
                "shoulder",
                "elbow",
                "head_roll",
                "head_pitch"
            ]
        );
        let test = crate::run(lamp_msgs::Occasion::OnDemand, &mut checks, crate::BUDGET);
        assert!(
            test.parts.iter().all(|p| p.verdict == Verdict::NotChecked),
            "{test:?}"
        );
        assert_eq!(test.check(), Ok(()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_reading_on_the_bus_is_heard_with_its_age() {
        let root = std::path::PathBuf::from(format!("/tmp/ls-air-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut sensor = bus
            .publisher_declared(AirState::TOPIC, &AirState::declaration(), "air")
            .unwrap();
        sensor.publish_json(1, &good_air()).unwrap();
        let heard = newest::<AirState>(&bus, AirState::TOPIC).unwrap().unwrap();
        assert_eq!(heard.value, good_air());
        assert!(heard.age < Duration::from_secs(5), "{:?}", heard.age);
        let mut check = Listen::new("air", &bus, AirState::TOPIC, air);
        assert_eq!(check.run(Instant::now()).verdict, Verdict::Pass);
        drop(sensor);
        std::fs::remove_dir_all(root).unwrap();
    }
}
