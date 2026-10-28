//! The body against five simulated servos: what it writes, where it stops.
use super::*;
use std::{cell::Cell, collections::VecDeque, rc::Rc};

#[derive(Clone, Copy)]
struct Servo {
    position: f32,
    goal: u16,
    torque: u8,
    acceleration: u8,
    speed: u16,
    status: u8,
    load: u16,
    temperature: u8,
    answers: bool,
    /// Its force limit (register 48), and whether a write of it is kept.
    force: u16,
    keeps_force: bool,
    /// Its own lower and upper position limits (registers 9 and 11).
    range: (u16, u16),
}

/// Something that happens to one servo at a given time.
#[derive(Clone, Copy)]
enum Event {
    Flag(u8),
    Push(f32),
    /// Moved this far at 1,500 counts a second, a fall under `READ_JUMP`
    /// (300 counts a step) so it is not a bad read. The real speed is not
    /// measured: the 16:45 journal has only before and after.
    Fall(f32),
    Heat(u8),
    /// One reading of this temperature, then back to what it was.
    HeatOnce(u8),
}

/// Five servos on one wire, moved by simulated time.
struct Lamp {
    time: Rc<Cell<Duration>>,
    servos: [Servo; 5],
    incoming: VecDeque<u8>,
    accepted: Vec<u8>,
    /// Every sync write of goals: (servo id, goal) as sent.
    goals: Vec<Vec<(u8, u16)>>,
    /// Every single-servo register write: (servo id, address, bytes).
    registers: Vec<(u8, u8, Vec<u8>)>,
    /// When each of `goals` and of `registers` was written.
    goal_times: Vec<Duration>,
    register_times: Vec<Duration>,
    /// One servo cannot pass this position and presses on it instead.
    wall: Option<(usize, f32)>,
    /// One servo stops this many counts short of any goal, as a real one
    /// does against its own friction.
    short: Option<(usize, f32)>,
    /// One servo lifting a drooped head: while above this position and
    /// asked to go lower it stops this many counts short of its goal, and
    /// reads this load while it does, as lamp-52e6's head pitch did after a
    /// power cut. Servo, above, counts, load.
    heavy: Option<(usize, f32, f32, u16)>,
    /// One servo lifts (toward fewer counts) no faster than this many
    /// counts a second, as a head rising slowly against its weight.
    climb: Option<(usize, f32)>,
    /// One servo lifts only in bursts: for this long, then still for this
    /// long, over and over, as lamp-52e6's head creeps back up.
    creep: Option<(usize, Duration, Duration)>,
    silent_from: Option<Duration>,
    silent_until: Option<Duration>,
    event: Option<(Duration, usize, Event)>,
    /// Loads one servo reads for a while whatever it is doing: from, until,
    /// servo, load. Afterwards it reads what it read before.
    strains: Vec<(Duration, Duration, usize, u16)>,
    unstrained: Option<[u16; 5]>,
    /// A temperature to put back once it has been read once.
    blip: Option<(usize, u8)>,
    /// Temperatures one servo reads from a given time: when, servo, degrees.
    heats: Vec<(Duration, usize, u8)>,
    /// From this time, one servo's temperature reads, read by read: a
    /// glitch, or `None` for what it is. Past the end, what it is. From,
    /// servo, readings, reads so far.
    flicker: Option<(Duration, usize, Vec<Option<u8>>, usize)>,
    simulated_to: Duration,
    /// The shoulder and the elbow follow their goals like the other three.
    /// Off, they stay where they are whatever is written to them.
    arm_moves: bool,
    /// What takes a latched overload off a servo: 0 nothing, 1 a write of
    /// torque on, 2 only torque off and then on, 3 being told to stop
    /// pushing (a goal that is where it is), a few steps later.
    overload_clears: u8,
    /// Torque and a goal by itself may be written to any joint, as a body
    /// told to take hold does. Off, only to the shoulder or the elbow.
    may_take_hold: bool,
    /// One servo acknowledges the next goal written to it by itself and
    /// keeps the goal it had.
    drops_goal: Option<usize>,
    /// One servo neither takes nor answers this many register writes, as
    /// one just given power does.
    deaf: Option<(usize, u8)>,
    /// A hand moves one servo toward a position at 100 counts a second,
    /// whatever its goal: from, until, servo, position. Its load is a
    /// strain of the same time.
    hand: Option<(Duration, Duration, usize, f32)>,
    /// More hands, one after another, each a quick turn of 1,000 counts a
    /// second: from, until, servo, position.
    hands: Vec<(Duration, Duration, usize, f32)>,
    /// Positions read that are not where a servo is: from this time, its
    /// next reads give this, this many times. From, servo, reading, reads.
    misreads: Vec<(Duration, usize, u16, u8)>,
    /// When each read of the positions was answered.
    read_times: Vec<Duration>,
    /// Each servo id asked for its position limits (registers 9..=12).
    limit_reads: Vec<u8>,
}

struct Clock {
    time: Rc<Cell<Duration>>,
    cancel_at: Duration,
    inhibit_at: Option<Duration>,
}
impl Runtime for Clock {
    fn now(&self) -> Duration {
        self.time.get()
    }
    fn cancelled(&mut self) -> io::Result<bool> {
        Ok(self.time.get() >= self.cancel_at)
    }
    fn recovery_inhibited(&self) -> bool {
        self.inhibit_at.is_some_and(|at| self.time.get() >= at)
    }
}

/// The lamp as it should be after its head is backed off the base: folded,
/// torque on, the head on the slow profile, the elbow's old flag latched.
fn folded(cancel_at: Duration) -> (Lamp, Clock) {
    let servo = |position: f32, acceleration, speed, status, load| Servo {
        position,
        goal: position as u16,
        torque: 1,
        acceleration,
        speed,
        status,
        load,
        temperature: 45,
        answers: true,
        force: 1_000,
        keeps_force: true,
        range: (0, 4_095),
    };
    let time = Rc::new(Cell::new(Duration::ZERO));
    (
        Lamp {
            time: time.clone(),
            servos: [
                servo(2_015.0, 0, 0, 0, 0),
                servo(2_050.0, 0, 0, 0, 20),
                servo(2_047.0, 0, 0, 0x20, 30),
                servo(2_044.0, 0, 0, 0, 32),
                servo(2_000.0, 50, 150, 0, 25),
            ],
            incoming: VecDeque::new(),
            accepted: Vec::new(),
            goals: Vec::new(),
            registers: Vec::new(),
            goal_times: Vec::new(),
            register_times: Vec::new(),
            wall: None,
            short: None,
            heavy: None,
            climb: None,
            creep: None,
            silent_from: None,
            silent_until: None,
            event: None,
            strains: Vec::new(),
            unstrained: None,
            blip: None,
            heats: Vec::new(),
            flicker: None,
            simulated_to: Duration::ZERO,
            arm_moves: false,
            overload_clears: 0,
            may_take_hold: false,
            drops_goal: None,
            deaf: None,
            hand: None,
            hands: Vec::new(),
            misreads: Vec::new(),
            read_times: Vec::new(),
            limit_reads: Vec::new(),
        },
        Clock {
            time,
            cancel_at,
            inhibit_at: None,
        },
    )
}

const PITCH: usize = 4;
const ROLL: usize = 3;
const YAW: usize = 0;
const HOME: [u16; 3] = [2_000, 2_044, 2_015];

impl Lamp {
    fn simulate(&mut self) {
        let now = self.time.get();
        let step = (now - self.simulated_to).as_secs_f32();
        self.simulated_to = now;
        if let Some((at, index, event)) = self.event
            && now >= at
        {
            self.event = None;
            match event {
                Event::Flag(status) => self.servos[index].status = status,
                Event::Push(by) => self.servos[index].position += by,
                Event::Fall(left) => {
                    let by = left.clamp(-1_500.0 * step, 1_500.0 * step);
                    self.servos[index].position += by;
                    if (left - by).abs() >= 1.0 {
                        self.event = Some((at, index, Event::Fall(left - by)));
                    }
                }
                Event::Heat(to) => self.servos[index].temperature = to,
                Event::HeatOnce(to) => {
                    self.blip = Some((index, self.servos[index].temperature));
                    self.servos[index].temperature = to;
                }
            }
        }
        for (at, index, to) in &self.heats {
            if now >= *at {
                self.servos[*index].temperature = *to;
            }
        }
        self.heats.retain(|(at, _, _)| now < *at);
        if !self.strains.is_empty() {
            let before = *self
                .unstrained
                .get_or_insert_with(|| self.servos.map(|servo| servo.load));
            for (index, servo) in self.servos.iter_mut().enumerate() {
                servo.load = self
                    .strains
                    .iter()
                    .find(|(from, until, strained, _)| {
                        *strained == index && (*from..*until).contains(&now)
                    })
                    .map_or(before[index], |strain| strain.3);
            }
        }
        for (index, servo) in self.servos.iter_mut().enumerate() {
            // An overload that goes once the servo is no longer pushing.
            if self.overload_clears == 3
                && servo.torque == 1
                && (f32::from(servo.goal) - servo.position).abs() < 1.0
            {
                servo.status &= !0x20;
            }
            if let Some((from, until, handled, to)) = self.hand
                && handled == index
                && (from..until).contains(&now)
            {
                servo.position += (to - servo.position).clamp(-100.0 * step, 100.0 * step);
                continue;
            }
            if let Some(&(_, _, _, to)) = self.hands.iter().find(|(from, until, handled, _)| {
                *handled == index && (*from..*until).contains(&now)
            }) {
                servo.position += (to - servo.position).clamp(-1_000.0 * step, 1_000.0 * step);
                continue;
            }
            if servo.torque != 1 || ((index == 1 || index == 2) && !self.arm_moves) {
                continue;
            }
            // The register's unit is not known; here it is counts a second.
            let fastest = if servo.speed == 0 {
                3_000.0
            } else {
                f32::from(servo.speed)
            };
            let mut wanted = f32::from(servo.goal);
            if let Some((sticky, by)) = self.short
                && sticky == index
            {
                let gap = wanted - servo.position;
                wanted = if gap.abs() <= by {
                    servo.position
                } else {
                    wanted - by * gap.signum()
                };
            }
            if let Some((lifted, above, by, load)) = self.heavy
                && lifted == index
            {
                let lifting = wanted < servo.position && servo.position > above;
                // At rest it reads what the folded head reads here.
                servo.load = if lifting { load } else { 25 };
                if lifting {
                    wanted = (wanted + by).min(servo.position);
                }
            }
            let rising = match self.climb {
                Some((slow, most)) if slow == index => most.min(fastest),
                _ => fastest,
            };
            let rising = match self.creep {
                Some((slow, moving, still))
                    if slow == index
                        && now.as_micros() % (moving + still).as_micros() >= moving.as_micros() =>
                {
                    0.0
                }
                _ => rising,
            };
            let moved = (wanted - servo.position).clamp(-rising * step, fastest * step);
            servo.position += moved;
            if let Some((walled, at)) = self.wall
                && walled == index
                && servo.position > at
            {
                servo.position = at;
                servo.load = 300;
            }
        }
    }

    fn registers_of(&self, servo: &Servo, address: u8, count: u8) -> Vec<u8> {
        let position = servo.position.round() as u16;
        (address..address + count)
            .map(|register| match register {
                9 => servo.range.0.to_le_bytes()[0],
                10 => servo.range.0.to_le_bytes()[1],
                11 => servo.range.1.to_le_bytes()[0],
                12 => servo.range.1.to_le_bytes()[1],
                40 => servo.torque,
                41 => servo.acceleration,
                42 => servo.goal.to_le_bytes()[0],
                43 => servo.goal.to_le_bytes()[1],
                46 => servo.speed.to_le_bytes()[0],
                47 => servo.speed.to_le_bytes()[1],
                48 => servo.force.to_le_bytes()[0],
                49 => servo.force.to_le_bytes()[1],
                56 => position.to_le_bytes()[0],
                57 => position.to_le_bytes()[1],
                60 => servo.load.to_le_bytes()[0],
                61 => servo.load.to_le_bytes()[1],
                62 => 122,
                63 => servo.temperature,
                65 => servo.status,
                _ => 0,
            })
            .collect()
    }

    fn answer(&mut self, id: u8, data: &[u8]) {
        let servo = self.servos[usize::from(id) - 1];
        let silent = self.silent_from.is_some_and(|from| self.time.get() >= from)
            && self
                .silent_until
                .is_none_or(|until| self.time.get() < until);
        if !servo.answers || silent {
            return;
        }
        let mut body = vec![id, data.len() as u8 + 2, servo.status];
        body.extend(data);
        let sum = checksum(&body);
        self.incoming.extend([0xff, 0xff]);
        self.incoming.extend(body);
        self.incoming.push_back(sum);
    }

    fn receive(&mut self) {
        while self.accepted.len() >= 6 {
            let total = usize::from(self.accepted[3]) + 4;
            if self.accepted.len() < total {
                return;
            }
            let packet: Vec<u8> = self.accepted.drain(..total).collect();
            assert_eq!(&packet[..2], &[0xff, 0xff]);
            assert_eq!(checksum(&packet[2..total - 1]), packet[total - 1]);
            let (id, instruction, parameters) = (packet[2], packet[4], &packet[5..total - 1]);
            match instruction {
                0x82 => {
                    let (address, count) = (parameters[0], parameters[1]);
                    let now = self.time.get();
                    if address == 56 {
                        self.read_times.push(now);
                    }
                    if address == 9 {
                        self.limit_reads.extend(parameters[2..].iter().copied());
                    }
                    for asked in parameters[2..].iter().copied() {
                        let index = usize::from(asked) - 1;
                        let mut servo = self.servos[index];
                        if address == 56
                            && let Some(misread) = self.misreads.iter_mut().find(|misread| {
                                misread.1 == index && now >= misread.0 && misread.3 > 0
                            })
                        {
                            servo.position = f32::from(misread.2);
                            misread.3 -= 1;
                        }
                        if address == 56
                            && let Some((from, flickering, readings, reads)) = self.flicker.as_mut()
                            && *flickering == index
                            && now >= *from
                        {
                            if let Some(Some(glitch)) = readings.get(*reads) {
                                servo.temperature = *glitch;
                            }
                            *reads += 1;
                        }
                        let data = self.registers_of(&servo, address, count);
                        self.answer(asked, &data);
                    }
                    if address == 56
                        && let Some((index, before)) = self.blip.take()
                    {
                        self.servos[index].temperature = before;
                    }
                }
                0x83 => {
                    assert_eq!(id, 0xfe);
                    assert_eq!((parameters[0], parameters[1]), (42, 2));
                    let mut written = Vec::new();
                    for entry in parameters[2..].chunks(3) {
                        let goal = u16::from_le_bytes([entry[1], entry[2]]);
                        self.servos[usize::from(entry[0]) - 1].goal = goal;
                        written.push((entry[0], goal));
                    }
                    self.goals.push(written);
                    self.goal_times.push(self.time.get());
                }
                0x03 => {
                    if let Some((index, left)) = self.deaf
                        && usize::from(id) - 1 == index
                        && left > 0
                    {
                        self.deaf = Some((index, left - 1));
                        continue;
                    }
                    let (address, data) = (parameters[0], parameters[1..].to_vec());
                    let clears = self.overload_clears;
                    let arm_or_held = id == 2 || id == 3 || self.may_take_hold;
                    let dropped = address == 42 && self.drops_goal == Some(usize::from(id) - 1);
                    if dropped {
                        self.drops_goal = None;
                    }
                    let servo = &mut self.servos[usize::from(id) - 1];
                    match (address, data.as_slice()) {
                        (41, [value]) => servo.acceleration = *value,
                        (46, [low, high]) => servo.speed = u16::from_le_bytes([*low, *high]),
                        // A goal by itself: only ever to the shoulder or the
                        // elbow, or to a joint the body takes hold of.
                        (42, [low, high]) if arm_or_held => {
                            if !dropped {
                                servo.goal = u16::from_le_bytes([*low, *high]);
                            }
                        }
                        // Torque: the same, except off, which lets the
                        // whole lamp go at the end of a run on a surprise.
                        (40, [value]) if arm_or_held || *value == 0 => {
                            let was = servo.torque;
                            servo.torque = *value;
                            if *value == 1 && (clears == 1 || (clears == 2 && was == 0)) {
                                servo.status &= !0x20;
                            }
                        }
                        // The force limit: only ever to the three joints
                        // that carry the lamp.
                        (48, [low, high]) if (1..=3).contains(&id) => {
                            if servo.keeps_force {
                                servo.force = u16::from_le_bytes([*low, *high]);
                            }
                        }
                        other => panic!("a write this body must never send: {other:?}"),
                    }
                    self.registers.push((id, address, data));
                    self.register_times.push(self.time.get());
                    self.answer(id, &[]);
                }
                other => panic!("an instruction this body must never send: {other:#04x}"),
            }
        }
    }

    fn pass(&mut self, by: Duration) {
        self.time.set(self.time.get() + by);
        self.simulate();
    }

    fn nothing_was_written(&self) -> bool {
        self.goals.is_empty() && self.registers.is_empty()
    }
}

impl PortIo for Lamp {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.pass(Duration::from_micros(40));
        if self.incoming.is_empty() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let count = bytes.len().min(self.incoming.len());
        for slot in &mut bytes[..count] {
            *slot = self.incoming.pop_front().unwrap();
        }
        Ok(count)
    }
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.pass(Duration::from_micros(40));
        self.accepted.extend(bytes);
        self.receive();
        Ok(bytes.len())
    }
    fn wait(&mut self, interest: Interest, maximum: Duration) -> io::Result<()> {
        if matches!(interest, Interest::Read) && !self.incoming.is_empty() {
            self.pass(Duration::from_micros(40));
        } else {
            self.pass(maximum);
        }
        Ok(())
    }
}

/// A mood for each moment, as the voice service would send it.
fn script(time: &Rc<Cell<Duration>>) -> impl FnMut() -> Option<(Mood, u8)> + use<> {
    let time = time.clone();
    move || {
        let at = time.get().as_millis() as u64;
        Some(match at {
            0..1_000 => (Mood::Resting, 0),
            1_000..3_000 => (Mood::Listening, 120),
            3_000..4_500 => (Mood::Thinking, 0),
            // Syllables: loud for 120 ms of every 300.
            4_500..7_000 => (Mood::Speaking, if at % 300 < 120 { 230 } else { 30 }),
            _ => (Mood::Resting, 0),
        })
    }
}

/// The last five register writes are torque off to each of the five, and
/// every servo has it off: the whole lamp let go, as V1.
fn let_go_of_the_whole_lamp(lamp: &Lamp) -> bool {
    let off: Vec<_> = (1..=5).map(|id| (id, 40, vec![0])).collect();
    lamp.registers.ends_with(&off) && lamp.servos.iter().all(|servo| servo.torque == 0)
}

fn goal_of(write: &[(u8, u16)], joint: Joint) -> u16 {
    write.iter().find(|(id, _)| *id == joint.id()).unwrap().1
}

#[test]
fn the_three_writes_are_exactly_these_bytes() {
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(
        hex(&goals_write(HOME)),
        "fffffe0d832a0205d00704fc0701df077b"
    );
    assert_eq!(
        hex(&speed_write(Joint::WristRoll, 150)),
        "ffff0405032e96002f"
    );
    assert_eq!(
        hex(&acceleration_write(Joint::BaseYaw, 50)),
        "ffff01040329329c"
    );
}

#[test]
fn a_folded_lamp_listens_thinks_speaks_and_comes_home() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.moved);
    assert_eq!(report.home, Some(HOME));
    assert_eq!(report.missed_reads, 0);

    // The servo's own speed bound: written to the two joints that lacked
    // it, not to the head, which had it, and to nothing else.
    assert_eq!(report.profile_writes, 4);
    let mut registers = lamp.registers.clone();
    registers.sort();
    assert_eq!(
        registers,
        vec![
            (1, 41, vec![50]),
            (1, 46, vec![150, 0]),
            (4, 41, vec![50]),
            (4, 46, vec![150, 0])
        ]
    );

    // Every goal write names the three commanded joints and no other.
    assert!(lamp.goals.len() > 100);
    for write in &lamp.goals {
        let ids: Vec<u8> = write.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, vec![5, 4, 1]);
    }
    // Within reach of where it was found, and the head short of the base.
    let limits = Limits::FOLDED;
    for write in &lamp.goals {
        let pitch = f32::from(goal_of(write, Joint::WristPitch));
        let roll = f32::from(goal_of(write, Joint::WristRoll));
        let yaw = f32::from(goal_of(write, Joint::BaseYaw));
        assert!(pitch >= 2_000.0 - limits.pitch_up && pitch <= 2_000.0 + limits.pitch_down);
        assert!(pitch <= f32::from(limits.pitch_window.1));
        assert!((roll - 2_044.0).abs() <= limits.roll);
        assert!((yaw - 2_015.0).abs() <= limits.yaw);
    }
    // Slowly: no joint's goal moves more than two counts in one write.
    let mut previous = HOME.map(i32::from);
    for write in &lamp.goals {
        for (index, joint) in COMMANDED.iter().enumerate() {
            let goal = i32::from(goal_of(write, *joint));
            assert!(
                (goal - previous[index]).abs() <= 2,
                "{joint} {previous:?} {write:?}"
            );
            previous[index] = goal;
        }
    }
    // It did tilt, look away and nod.
    let reach = |joint: Joint, home: u16| {
        lamp.goals
            .iter()
            .map(|write| goal_of(write, joint).abs_diff(home))
            .max()
            .unwrap()
    };
    assert!(reach(Joint::WristRoll, 2_044) >= 20);
    assert!(reach(Joint::BaseYaw, 2_015) >= 25);
    assert!(reach(Joint::WristPitch, 2_000) >= 20);
    // And it ends where it was found, with the shoulder and elbow untouched.
    assert_eq!(lamp.servos[PITCH].goal, 2_000);
    assert_eq!(lamp.servos[ROLL].goal, 2_044);
    assert_eq!(lamp.servos[YAW].goal, 2_015);
    assert!((lamp.servos[PITCH].position - 2_000.0).abs() <= 2.0);
    assert_eq!((lamp.servos[1].goal, lamp.servos[2].goal), (2_050, 2_047));
    assert_eq!(report.held_at, None);
    serde_json::to_string(&report).unwrap();
}

#[test]
fn it_does_not_start_on_a_lamp_that_is_not_as_expected() {
    let refused = |change: &dyn Fn(&mut Lamp)| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(2));
        change(&mut lamp);
        let news = script(&clock.time);
        let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
        assert!(
            matches!(report.stop, Stop::Refused { .. }),
            "{:?}",
            report.stop
        );
        assert!(!report.moved);
        assert!(lamp.goals.is_empty());
        let Stop::Refused { why } = report.stop else {
            unreachable!()
        };
        // Found hot or loaded, a joint is pushing at something: as V1, the
        // whole lamp is let go and nothing else is written. Any other
        // reason writes nothing.
        if why.contains("already") {
            assert!(let_go_of_the_whole_lamp(&lamp), "{:?}", lamp.registers);
            assert_eq!(lamp.registers.len(), 5);
        } else {
            assert!(lamp.nothing_was_written(), "{why}");
        }
        why
    };
    // The head pressed on the base, as it was found on 2026-10-12.
    assert!(refused(&|lamp| lamp.servos[PITCH].position = 2_095.0).contains("2095"));
    assert!(refused(&|lamp| lamp.servos[ROLL].torque = 0).contains("torque"));
    assert!(refused(&|lamp| lamp.servos[PITCH].status = 0x20).contains("0x20"));
    assert!(refused(&|lamp| lamp.servos[YAW].temperature = 70).contains("70"));
    // Over 100 on every first read is unreadable, not heat: refused with
    // nothing written and nothing let go.
    assert!(refused(&|lamp| lamp.servos[YAW].temperature = 150).contains("unreadable at start"));
    assert!(refused(&|lamp| lamp.servos[PITCH].load = 200).contains("200"));
    // A joint that does not answer, commanded or not.
    assert!(refused(&|lamp| lamp.servos[1].answers = false).contains("first read"));
}

#[test]
fn a_load_that_rises_stops_it_where_it_is_and_lets_the_whole_lamp_go() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    // Something in the way of the head's tilt, ten counts from home.
    lamp.wall = Some((ROLL, 2_054.0));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert_eq!(
        report.stop,
        Stop::Load {
            joint: 4,
            load: 300
        },
        "{report:?}"
    );
    // It stopped within a few steps of meeting it, not at the end of the tilt.
    assert!(
        clock.time.get() < Duration::from_millis(1_700),
        "{:?}",
        clock.time.get()
    );
    let held = report.held_at.unwrap();
    assert_eq!(held[1], 2_054);
    // The last thing written is the hold: every goal is where its joint is.
    assert_eq!(
        lamp.goals.last().unwrap(),
        &vec![(5, held[0]), (4, held[1]), (1, held[2])]
    );
    assert_eq!(lamp.servos[ROLL].goal, 2_054);
    assert!(
        lamp.servos[PITCH]
            .goal
            .abs_diff(lamp.servos[PITCH].position.round() as u16)
            <= 1
    );
    // Then, as V1, torque off on all five: a stuck joint left held with
    // torque on keeps pushing and heats (lamp-52e6, 2026-10-16).
    assert!(let_go_of_the_whole_lamp(&lamp), "{:?}", lamp.registers);
}

#[test]
fn a_joint_that_meets_resistance_stops_pushing_for_two_seconds_and_then_goes_on() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    // A fifth of a second into its tilt, a load like the one the head read on
    // lamp-4ace: 217 for a moment and nothing in its way.
    let ms = Duration::from_millis;
    lamp.strains = vec![(ms(1_200), ms(1_400), ROLL, 217)];
    let news = script(&clock.time);
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        None,
        news,
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!((report.eased, report.peak_load[1]), ([0, 1, 0], 217));
    // It gave way on the sixth step over the bound, where it was. A step is
    // watched before it is judged, so the next one is the first to show it.
    let strained = seen
        .iter()
        .position(|glimpse| glimpse.loads[1] == 217)
        .unwrap();
    let gave = seen
        .iter()
        .position(|glimpse| glimpse.eased[1] == 1)
        .unwrap();
    assert_eq!(gave, strained + 6);
    let held = seen[gave].goals[1];
    assert!(
        held.abs_diff(seen[gave - 1].positions[1]) <= 1,
        "{:?}",
        seen[gave - 1]
    );
    assert_ne!(held, HOME[1], "it had begun to tilt");
    // For two seconds its goal does not change, while the other joints go
    // on with what the lamp is doing.
    let waiting = &seen[gave..gave + usize::from(EASE_STEPS) - 1];
    assert!(waiting.iter().all(|glimpse| glimpse.goals[1] == held));
    assert!(waiting.iter().any(|glimpse| glimpse.goals[2] != HOME[2]));
    // Then it moves again, from where it stopped.
    let after = &seen[gave + usize::from(EASE_STEPS)..];
    assert!(
        after
            .iter()
            .any(|glimpse| glimpse.goals[1].abs_diff(held) >= 10)
    );
    assert!(
        after
            .windows(2)
            .all(|pair| pair[1].goals[1].abs_diff(pair[0].goals[1]) <= 2),
        "it jumped"
    );
}

#[test]
fn resistance_for_the_fourth_time_in_a_minute_ends_the_run() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(40));
    let ms = Duration::from_millis;
    lamp.strains = [1_200, 6_000, 12_000, 18_000]
        .map(|at| (ms(at), ms(at + 200), YAW, 260))
        .to_vec();
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert_eq!(
        report.stop,
        Stop::Load {
            joint: 1,
            load: 260
        },
        "{report:?}"
    );
    assert_eq!(report.eased, [0, 0, 3]);
    assert!(clock.time.get() < ms(18_200), "{:?}", clock.time.get());
    // Three in a minute and a fourth later are four separate moments.
    let (mut lamp, mut clock) = folded(Duration::from_secs(90));
    lamp.strains = [1_200, 6_000, 12_000, 75_000]
        .map(|at| (ms(at), ms(at + 200), YAW, 260))
        .to_vec();
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.eased, [0, 0, 4]);
}

#[test]
fn a_joint_still_loaded_after_it_stopped_pushing_ends_the_run() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    let ms = Duration::from_millis;
    // Half a second of it: longer than giving way explains.
    lamp.strains = vec![(ms(1_200), ms(1_700), ROLL, 240)];
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert_eq!(
        report.stop,
        Stop::Load {
            joint: 4,
            load: 240
        },
        "{report:?}"
    );
    assert_eq!(report.eased, [0, 1, 0]);
    // Twelve steps: six to give way and six more of the same.
    let ended = clock.time.get();
    assert!(
        (ms(1_200 + 11 * 20)..ms(1_200 + 13 * 20)).contains(&ended),
        "{ended:?}"
    );
}

#[test]
fn a_new_flag_a_moved_elbow_or_a_hot_joint_stops_it() {
    let ended = |index: usize, event: Event| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(8));
        lamp.event = Some((Duration::from_millis(1_500), index, event));
        // Heat comes on slowly: from two degrees under, not 18 in a step
        // (`judge_temperatures`).
        if let Event::Heat(to) = event {
            lamp.servos[index].temperature = to - 2;
        }
        let news = script(&clock.time);
        let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
        // At once, except for heat, which has to last a second to count.
        let by = if matches!(event, Event::Heat(_)) {
            2_600
        } else {
            1_600
        };
        assert!(clock.time.get() < Duration::from_millis(by));
        assert!(report.held_at.is_some());
        assert!(let_go_of_the_whole_lamp(&lamp), "{:?}", lamp.registers);
        report.stop
    };
    // The shoulder trips its overload flag, though nothing commands it.
    assert_eq!(
        ended(1, Event::Flag(0x20)),
        Stop::Status {
            joint: 2,
            found: 0,
            now: 0x20
        }
    );
    // The elbow's old flag was there at start; a different one is news.
    assert_eq!(
        ended(2, Event::Flag(0x24)),
        Stop::Status {
            joint: 3,
            found: 0x20,
            now: 0x24
        }
    );
    assert_eq!(
        ended(2, Event::Push(9.0)),
        Stop::Drift {
            joint: 3,
            found: 2_047,
            now: 2_056
        }
    );
    assert_eq!(
        ended(PITCH, Event::Heat(63)),
        Stop::Temperature {
            joint: 5,
            temperature: 63
        }
    );
}

#[test]
fn one_hot_reading_while_it_moves_is_not_heat() {
    // As on lamp-4ace: the yaw servo read 72 on one step of its first move.
    // 27 over the step before is a bad read, not the peak.
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.event = Some((Duration::from_millis(3_200), YAW, Event::HeatOnce(72)));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.peak_temperature[2], 45);
    assert_eq!(report.bad_temperatures, [1, 0, 0, 0, 0]);
}

#[test]
fn a_home_it_is_given_is_reached_slowly_from_where_it_is_found() {
    // Left 43 counts above its home by an earlier stop, and turned a little.
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.servos[PITCH].position = 1_973.0;
    lamp.servos[PITCH].goal = 1_973;
    lamp.servos[YAW].position = 2_033.0;
    lamp.servos[YAW].goal = 2_033;
    let home = [1_930, 2_044, 2_015];
    let news = script(&clock.time);
    let report = run(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(home),
        news,
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.home, Some(home));
    // Every write is a small step from the one before, starting where it was.
    let mut previous = [1_973, 2_044, 2_033];
    for write in &lamp.goals {
        for (index, joint) in COMMANDED.iter().enumerate() {
            let goal = i32::from(goal_of(write, *joint));
            assert!(
                (goal - previous[index]).abs() <= 2,
                "{joint} {previous:?} {write:?}"
            );
            previous[index] = goal;
        }
    }
    // The head never moved away from its home on the way in.
    assert!(
        lamp.goals
            .iter()
            .all(|write| goal_of(write, Joint::WristPitch) <= 1_973)
    );
    assert_eq!(
        [
            lamp.servos[PITCH].goal,
            lamp.servos[ROLL].goal,
            lamp.servos[YAW].goal
        ],
        home
    );

    // Too far from the home it is given: nothing is written.
    let (mut lamp, mut clock) = folded(Duration::from_secs(2));
    let news = script(&clock.time);
    let far = Some([1_900, 2_044, 2_015]);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, far, news, |_| {});
    assert!(
        matches!(&report.stop, Stop::Refused { why } if why.contains("100 counts")),
        "{:?}",
        report.stop
    );
    assert!(lamp.nothing_was_written());
}

#[test]
fn a_wider_reach_and_speed_can_be_chosen_only_inside_their_caps() {
    let tuning = Tuning {
        reach: [150, 15, 80, 200],
        speed: 200,
        servo_speed: 600,
        servo_acceleration: 100,
    };
    let limits = Limits::folded_with(tuning).unwrap();
    assert_eq!(
        (limits.pitch_up, limits.yaw, limits.speed),
        (150.0, 200.0, 200.0)
    );
    // What is not a reach or a speed is exactly as before.
    assert_eq!(limits.pitch_window, Limits::FOLDED.pitch_window);
    assert_eq!(limits.load, Limits::FOLDED.load);
    assert_eq!(limits.bystander_drift, Limits::FOLDED.bystander_drift);
    for refused in [
        Tuning {
            reach: [181, 15, 80, 200],
            ..tuning
        },
        Tuning {
            reach: [150, 26, 80, 200],
            ..tuning
        },
        Tuning {
            reach: [150, 15, 101, 200],
            ..tuning
        },
        Tuning {
            reach: [150, 15, 80, 251],
            ..tuning
        },
        Tuning {
            reach: [0, 15, 80, 200],
            ..tuning
        },
        Tuning {
            speed: 301,
            ..tuning
        },
        Tuning {
            servo_speed: 1_001,
            ..tuning
        },
        Tuning {
            servo_acceleration: 151,
            ..tuning
        },
    ] {
        assert!(Limits::folded_with(refused).is_err(), "{refused:?}");
    }

    // With it the head turns much farther, still in small steps and still
    // never past the pitch window: a glance when the answer comes soon,
    // and the whole look away only when the lamp goes on thinking.
    let turned = |thinks_until_ms: u64, lasts: u64| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(lasts));
        lamp.servos[PITCH].position = 1_950.0;
        lamp.servos[PITCH].goal = 1_950;
        for servo in [PITCH, ROLL, YAW] {
            lamp.servos[servo].speed = 600;
            lamp.servos[servo].acceleration = 100;
        }
        let time = clock.time.clone();
        let news = move || {
            let at = time.get().as_millis() as u64;
            Some(match at {
                0..1_000 => (Mood::Resting, 0),
                1_000..3_000 => (Mood::Listening, 120),
                at if at < thinks_until_ms => (Mood::Thinking, 0),
                at if at < thinks_until_ms + 2_500 => (Mood::Speaking, 30),
                _ => (Mood::Resting, 0),
            })
        };
        let report = run(&mut lamp, &mut clock, limits, None, news, |_| {});
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        lamp
    };
    // An answer after 1.2 s: the base has glanced aside, 36 counts and a
    // little more, and no farther.
    let lamp = turned(4_200, 9);
    let farthest = |lamp: &Lamp, joint: Joint, home: u16| {
        lamp.goals
            .iter()
            .map(|write| goal_of(write, joint).abs_diff(home))
            .max()
            .unwrap()
    };
    let glanced = farthest(&lamp, Joint::BaseYaw, 2_015);
    assert!((30..=50).contains(&glanced), "{glanced}");
    // Six seconds of thinking: the head all the way, the base still at its
    // glance (#1722, 20 Oct: a base turning on in a slow answer was heard
    // as someone speaking).
    let lamp = turned(9_000, 14);
    let farthest = |joint: Joint, home: u16| farthest(&lamp, joint, home);
    assert!(
        (30..=50).contains(&farthest(Joint::BaseYaw, 2_015)),
        "{}",
        farthest(Joint::BaseYaw, 2_015)
    );
    assert!(farthest(Joint::WristPitch, 1_950) >= 90);
    let mut previous = [1_950, 2_044, 2_015];
    for write in &lamp.goals {
        for (index, joint) in COMMANDED.iter().enumerate() {
            let goal = i32::from(goal_of(write, *joint));
            assert!(
                (goal - previous[index]).abs() <= 5,
                "{joint} {previous:?} {write:?}"
            );
            previous[index] = goal;
        }
        assert!(goal_of(write, Joint::WristPitch) <= 2_030);
    }
}

#[test]
fn every_step_can_be_watched_with_the_mood_it_acted_on() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    let news = script(&clock.time);
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        None,
        news,
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled);
    assert_eq!(seen.len() as u64, report.steps);
    let moods: Vec<Mood> = seen.iter().map(|glimpse| glimpse.mood).collect();
    for mood in [
        Mood::Resting,
        Mood::Listening,
        Mood::Thinking,
        Mood::Speaking,
    ] {
        assert!(moods.contains(&mood), "{mood:?}");
    }
    // While it looked away its goal and its position were both off home.
    let away = seen
        .iter()
        .find(|glimpse| glimpse.mood == Mood::Thinking && glimpse.goals[2] != 2_015);
    assert!(away.is_some());
    assert!(
        seen.iter()
            .any(|glimpse| glimpse.positions[2].abs_diff(2_015) >= 20)
    );
}

#[test]
fn at_rest_a_joint_that_stopped_short_is_no_longer_pushed() {
    // The yaw servo comes to rest eight counts from wherever it is sent.
    let (mut lamp, mut clock) = folded(Duration::from_secs(12));
    lamp.short = Some((YAW, 8.0));
    let news = script(&clock.time);
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        None,
        news,
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    // From two and a half seconds into the rest that follows speaking until
    // it is asked to stop, the yaw goal is the yaw position: nothing pushes.
    let rested: Vec<&Glimpse> = seen
        .iter()
        .filter(|glimpse| glimpse.step > 9_500 / 20 && glimpse.step < 11_900 / 20)
        .collect();
    assert!(!rested.is_empty());
    assert!(
        rested
            .iter()
            .all(|glimpse| glimpse.goals[2] == glimpse.positions[2])
    );
    // And it still turned away while it thought, a real move from there.
    assert!(seen.iter().any(|glimpse| glimpse.mood == Mood::Thinking
        && glimpse.goals[2].abs_diff(2_015) >= 25));
}

#[test]
fn a_joint_that_has_arrived_is_not_pushed_at_and_a_small_change_does_not_move_it() {
    // The roll servo stops five counts from wherever it is sent, as the
    // head's did on lamp-4ace from the goals of its nods.
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.short = Some((ROLL, 5.0));
    let news = script(&clock.time);
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        None,
        news,
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.eased, [0; 3]);
    let during = |from: u64, until: u64| -> Vec<&Glimpse> {
        seen.iter()
            .filter(|glimpse| (from / 20..until / 20).contains(&glimpse.step))
            .collect()
    };
    // Listening, once the head has tilted: every goal is where its joint
    // is, the one that stopped short included.
    let listening = during(2_600, 2_950);
    assert!(!listening.is_empty());
    for glimpse in &listening {
        assert_eq!(glimpse.goals, glimpse.positions, "{glimpse:?}");
    }
    assert!(listening[0].goals[1].abs_diff(HOME[1]) >= 15, "it did tilt");
    // Speaking: it came down from its thinking pose, and the nods on its
    // voice, a few counts each at this reach, ask nothing of it.
    let speaking = during(5_800, 6_950);
    assert!(
        speaking
            .iter()
            .all(|glimpse| glimpse.mood == Mood::Speaking)
    );
    assert!(
        speaking
            .iter()
            .all(|glimpse| glimpse.goals == speaking[0].goals),
        "a goal changed while it spoke"
    );
    assert!(speaking[0].goals[0] < HOME[0] && speaking[0].goals[0] > HOME[0] - 20);
    // A real move still happens: thinking turned it and lifted it.
    assert!(seen.iter().any(|glimpse| glimpse.mood == Mood::Thinking
        && glimpse.goals[2].abs_diff(HOME[2]) >= 25
        && glimpse.goals[0] + 25 <= HOME[0]));
}

#[test]
fn a_wire_that_goes_silent_stops_it_after_half_a_second() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.silent_from = Some(Duration::from_millis(1_500));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert!(matches!(report.stop, Stop::Bus { .. }), "{:?}", report.stop);
    assert_eq!(report.missed_reads, u64::from(WIRE_STEPS));
    // And one try at torque off on each of the five, not waited on.
    assert!(clock.time.get() < Duration::from_millis(1_500 + 20 * 27) + EXCHANGE * 5);
    // Nothing was asked of the joints once the wire had gone silent, but
    // the hold: no goal moved further from where the joints were.
    let last = lamp.goals.last().unwrap();
    for (id, goal) in last {
        let servo = lamp.servos[usize::from(*id) - 1];
        assert!(
            goal.abs_diff(servo.position.round() as u16) <= 8,
            "{last:?}"
        );
    }
}

#[test]
fn a_wire_that_fails_for_a_moment_is_ridden_out() {
    // Four steps of nothing, as after a stall of the whole machine, in the
    // middle of the look away.
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.silent_from = Some(Duration::from_millis(3_400));
    lamp.silent_until = Some(Duration::from_millis(3_480));
    let news = script(&clock.time);
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        None,
        news,
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!((3..=6).contains(&report.missed_reads), "{report:?}");
    // It went on with the look away afterwards and came home at the end.
    assert!(seen.iter().any(|glimpse| glimpse.mood == Mood::Thinking
        && glimpse.step > 3_500 / 20
        && glimpse.goals[2].abs_diff(HOME[2]) >= 25));
    assert_eq!(lamp.goals.last().unwrap()[0], (5, HOME[0]));
}

#[test]
fn a_mood_that_stops_arriving_is_rest() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(6));
    let time = clock.time.clone();
    // Listening for a second and a half, then the sender is gone.
    let news = move || (time.get() < Duration::from_millis(1_500)).then_some((Mood::Listening, 90));
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert_eq!(report.stop, Stop::Cancelled);
    let tilted = lamp
        .goals
        .iter()
        .any(|write| goal_of(write, Joint::WristRoll).abs_diff(2_044) >= 20);
    assert!(tilted);
    // Long before it was asked to stop, the goals were back at home.
    assert_eq!(lamp.servos[ROLL].goal, 2_044);
    assert!((lamp.servos[ROLL].position - 2_044.0).abs() <= 1.0);
    let last_change = lamp.goals.len();
    assert!(last_change < 260, "{last_change}");
}

#[test]
fn the_heads_pitch_never_passes_its_window_however_it_is_found() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    // Found near the top of where it may be found: little room to dip.
    lamp.servos[PITCH].position = 2_018.0;
    lamp.servos[PITCH].goal = 2_018;
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let highest = lamp
        .goals
        .iter()
        .map(|write| goal_of(write, Joint::WristPitch))
        .max()
        .unwrap();
    assert!(highest <= 2_030, "{highest}");
}

/// A lamp's own window holds the head inside it as lamp-4ace's does, and is
/// taken only inside the head pitch servo's own limits.
#[test]
fn a_lamp_s_own_pitch_window_holds_the_head_and_must_fit_its_servo() {
    let own = Limits {
        pitch_window: (1_670, 1_995),
        ..Limits::FOLDED
    };
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.servos[PITCH].position = 1_990.0;
    lamp.servos[PITCH].goal = 1_990;
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, own, None, news, |_| {});
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let pitches: Vec<u16> = lamp
        .goals
        .iter()
        .map(|write| goal_of(write, Joint::WristPitch))
        .collect();
    // Found five counts under the window's top: every goal stays inside it.
    assert!(
        pitches.iter().all(|pitch| (1_670..=1_995).contains(pitch)),
        "{pitches:?}"
    );
    // A servo that may not reach the low end refuses the run, writing nothing.
    let (mut lamp, mut clock) = folded(Duration::from_secs(2));
    lamp.servos[PITCH].range = (1_700, 3_000);
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, own, None, news, |_| {});
    match &report.stop {
        Stop::Refused { why } => assert!(why.contains("1700..=3000"), "{why}"),
        stop => panic!("{stop:?}"),
    }
    assert!(lamp.nothing_was_written());
}

/// Without a window of its own, a servo's limits are not even read: a lamp
/// whose servo says it can reach nothing runs exactly as before.
#[test]
fn without_a_window_of_its_own_the_lamp_runs_exactly_as_before() {
    let writes = |range: (u16, u16)| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(8));
        lamp.servos[PITCH].range = range;
        let news = script(&clock.time);
        let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        (lamp.goals, lamp.registers)
    };
    assert_eq!(writes((0, 0)), writes((0, 4_095)));
}

/// A pose to stand in for the simulated lamp, whose arm is found folded at
/// 2,050 and 2,047: the whole way in four seconds unless said otherwise.
fn standing(share: u8, awake: u16) -> Standing {
    Standing::new(
        [2_050, 2_047],
        [1_900, 2_044, 2_015, 2_650, 2_600],
        share,
        4,
        awake,
        [950, 950],
    )
    .unwrap()
}

/// Spoken to from one second to seven, then left alone.
fn spoken_to(time: &Rc<Cell<Duration>>) -> impl FnMut() -> Option<(Mood, u8)> + use<> {
    let time = time.clone();
    move || {
        let at = time.get().as_millis() as u64;
        Some(match at {
            1_000..3_000 => (Mood::Listening, 120),
            3_000..4_500 => (Mood::Thinking, 0),
            4_500..7_000 => (Mood::Speaking, 60),
            _ => (Mood::Resting, 0),
        })
    }
}

#[test]
fn the_two_writes_of_a_lamp_that_stands_are_exactly_these_bytes() {
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(
        hex(&all_goals_write(HOME, [2_048, 2_047])),
        "fffffe13832a0205d00704fc0701df0702000803ff0762"
    );
    assert_eq!(hex(&torque_write(Joint::ElbowPitch, 1)), "ffff0304032801cc");
}

/// The force limit: off unless chosen; written to the joints that carry
/// the lamp and no other, before anything moves; and read back.
#[test]
fn a_force_limit_is_written_to_the_joints_that_carry_the_lamp_and_read_back() {
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(
        hex(&force_write(Joint::BasePitch, 600)),
        "ffff0205033058026b"
    );
    let force_writes = |lamp: &Lamp| -> Vec<(u8, Vec<u8>)> {
        lamp.registers
            .iter()
            .filter(|(_, address, _)| *address == 48)
            .map(|(id, _, bytes)| (*id, bytes.clone()))
            .collect()
    };
    let capped = Limits {
        force: Some(600),
        ..Limits::FOLDED
    };

    // A lamp that stays folded: the base's turn alone. The shoulder and
    // the elbow are not this body's to write to unless it may stand.
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, capped, None, news, |_| {});
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!((report.force_writes, report.force), (1, Some(600)));
    assert_eq!(force_writes(&lamp), vec![(1, vec![88, 2])]);
    assert_eq!(
        lamp.servos.map(|servo| servo.force),
        [600, 1_000, 1_000, 1_000, 1_000]
    );
    // It was written before the first goal was.
    assert!(report.moved);

    // A lamp that may stand: the base's turn, the shoulder and the elbow,
    // and it stands up as before.
    let (mut lamp, mut clock) = folded(Duration::from_secs(22));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let news = spoken_to(&clock.time);
    let report = run_standing(
        &mut lamp,
        &mut clock,
        capped,
        Some(HOME),
        Some(standing(100, 5)),
        news,
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.stood && report.retreats.is_empty());
    assert_eq!((report.force_writes, report.force), (3, Some(600)));
    assert_eq!(
        force_writes(&lamp),
        vec![(1, vec![88, 2]), (2, vec![88, 2]), (3, vec![88, 2])]
    );
    assert_eq!(
        lamp.servos.map(|servo| servo.force),
        [600, 600, 600, 1_000, 1_000]
    );

    // With none chosen nothing is written to that register at all.
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert_eq!((report.force_writes, report.force), (0, None));
    assert!(force_writes(&lamp).is_empty());
}

/// A servo that does not keep the limit it was given is not moved: the
/// run ends before the first goal, and says what was read back.
#[test]
fn a_force_limit_that_is_not_kept_ends_the_run_before_anything_moves() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.servos[0].keeps_force = false;
    let news = script(&clock.time);
    let capped = Limits {
        force: Some(600),
        ..Limits::FOLDED
    };
    let report = run(&mut lamp, &mut clock, capped, None, news, |_| {});
    assert_eq!(
        report.stop,
        Stop::Bus {
            detail: "force limit read back as [1000]".into()
        },
        "{report:?}"
    );
    assert!(!report.moved && lamp.goals.is_empty());
    assert_eq!(report.force, None);
}

#[test]
fn a_standing_pose_is_taken_only_inside_its_caps() {
    let pose = [1_900, 2_044, 2_015, 2_650, 2_600];
    let new = |folded, pose, share, seconds, awake, load| {
        Standing::new(folded, pose, share, seconds, awake, load)
    };
    let taken = new([2_050, 2_047], pose, 30, 5, 90, [950, 950]).unwrap();
    assert_eq!((taken.share, taken.seconds), (0.3, 5.0));
    assert_eq!(taken.awake, Duration::from_secs(90));
    // Too far for a shoulder, off the encoder, no share, too quick, too
    // briefly awake, a load bound that is no bound.
    assert!(
        new(
            [2_050, 2_047],
            [1_900, 2_044, 2_015, 3_500, 2_600],
            30,
            5,
            90,
            [950, 950]
        )
        .is_err()
    );
    assert!(
        new(
            [2_050, 2_047],
            [4_096, 2_044, 2_015, 2_650, 2_600],
            30,
            5,
            90,
            [950, 950]
        )
        .is_err()
    );
    assert!(new([2_050, 2_047], pose, 0, 5, 90, [950, 950]).is_err());
    assert!(new([2_050, 2_047], pose, 101, 5, 90, [950, 950]).is_err());
    assert!(new([2_050, 2_047], pose, 30, 1, 90, [950, 950]).is_err());
    assert!(new([2_050, 2_047], pose, 30, 5, 4, [950, 950]).is_err());
    assert!(new([2_050, 2_047], pose, 30, 5, 90, [950, 1_001]).is_err());
    // The way up eases in and out, and its inverse finds the same place.
    assert_eq!((smooth(0.0), smooth(0.5), smooth(1.0)), (0.0, 0.5, 1.0));
    for share in [0.1f32, 0.37, 0.8] {
        assert!((unsmooth(smooth(share)) - share).abs() < 0.001);
    }
    assert_eq!(head_share(0.25), 0.0);
    assert_eq!(head_share(1.0), 1.0);
}

/// Held by `lamp-control`'s lease (#3504): a lamp spoken to stands, and
/// while held it stays up after its awake time, as with someone in front
/// of it; let go, it folds. A hold never stands a folded lamp up by itself.
#[test]
fn a_lamp_held_by_its_lease_stays_up_and_a_hold_alone_never_stands_it() {
    let run = |held_from: Duration, held_until: Duration, spoken: bool| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(30));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        let time = clock.time.clone();
        let heard = clock.time.clone();
        let stood_at = clock.time.clone();
        let mut stand = Vec::new();
        run_alive(
            &mut lamp,
            &mut clock,
            Limits::FOLDED,
            Some(HOME),
            Some(standing(100, 5)),
            None,
            move || {
                let at = heard.get();
                Some(
                    if spoken && (Duration::from_secs(1)..Duration::from_secs(3)).contains(&at) {
                        (Mood::Listening, 120)
                    } else {
                        (Mood::Resting, 0)
                    },
                )
            },
            move || Asked {
                held: (held_from..held_until).contains(&time.get()),
                ..Asked::default()
            },
            |glimpse| stand.push((stood_at.get(), glimpse.stand)),
        );
        stand
    };
    let at = |stand: &[(Duration, f32)], seconds: u64| {
        stand
            .iter()
            .find(|(at, _)| *at >= Duration::from_secs(seconds))
            .map(|(_, stand)| *stand)
            .unwrap()
    };
    // Spoken to and never held: down well before 20 s (3 s + 5 s awake +
    // 4 s down).
    let alone = run(Duration::ZERO, Duration::ZERO, true);
    assert_eq!(at(&alone, 20), 0.0);
    // Held from 2 s to 18 s: still up at 20 s (awake 5 s past the hold),
    // and down again by 29 s.
    let held = run(Duration::from_secs(2), Duration::from_secs(18), true);
    assert_eq!(at(&held, 20), 1.0, "a held lamp stays up");
    assert_eq!(at(&held, 29), 0.0, "let go, it folds");
    // Held all along and never spoken to: it never stands.
    let never = run(Duration::ZERO, Duration::from_secs(30), false);
    assert!(never.iter().all(|(_, stand)| *stand == 0.0));
}

#[test]
fn spoken_to_it_stands_up_and_left_alone_it_folds_down_again() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(22));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let news = spoken_to(&clock.time);
    let up = standing(100, 5);
    let mut highest = 0.0f32;
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(up),
        news,
        |glimpse| highest = highest.max(glimpse.stand),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.standing, Some(up));
    assert_eq!(report.standing_refused, None);
    assert!(report.stood && report.retreats.is_empty());
    assert_eq!(highest, 1.0);

    // The elbow's old flag: torque written once, to it alone, and it is
    // gone. Then the slow profile on all five.
    assert_eq!(report.torque_writes, 1);
    let torque: Vec<_> = lamp
        .registers
        .iter()
        .filter(|(_, address, _)| *address == 40)
        .collect();
    assert_eq!(torque, vec![&(3, 40, vec![1])]);
    assert_eq!(lamp.servos[2].status, 0);
    for id in [2u8, 3] {
        assert!(
            lamp.registers
                .iter()
                .any(|(to, address, _)| (*to, *address) == (id, 46))
        );
        assert!(
            lamp.registers
                .iter()
                .any(|(to, address, _)| (*to, *address) == (id, 41))
        );
    }

    // Every goal write names all five joints, the arm stays between folded
    // and stood up, reaches the pose, and no goal jumps.
    let mut previous = [2_050i32, 2_047];
    let mut top = [0u16; 2];
    for write in &lamp.goals {
        let ids: Vec<u8> = write.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, vec![5, 4, 1, 2, 3]);
        for (index, joint) in ARM.iter().enumerate() {
            let goal = goal_of(write, *joint);
            assert!(
                (up.folded[index]..=up.pose[3 + index]).contains(&goal),
                "{write:?}"
            );
            assert!(
                (i32::from(goal) - previous[index]).abs() <= 6,
                "{previous:?} {write:?}"
            );
            previous[index] = i32::from(goal);
            top[index] = top[index].max(goal);
        }
    }
    assert_eq!(top, [2_650, 2_600]);
    // The head went with it: its home rose to the standing pose's, and its
    // gestures stayed within their reach of that.
    let lowest = lamp
        .goals
        .iter()
        .map(|write| goal_of(write, Joint::WristPitch))
        .min()
        .unwrap();
    assert!((1_855..=1_900).contains(&lowest), "{lowest}");
    // And it is folded again as it was found, the head at its home.
    assert_eq!((lamp.servos[1].goal, lamp.servos[2].goal), (2_050, 2_047));
    assert!((lamp.servos[1].position - 2_050.0).abs() <= 2.0);
    assert!((lamp.servos[2].position - 2_047.0).abs() <= 2.0);
    assert_eq!(lamp.servos[PITCH].goal, 2_000);
    assert!((lamp.servos[PITCH].position - 2_000.0).abs() <= 2.0);
    serde_json::to_string(&report).unwrap();
}

#[test]
fn the_head_keeps_its_folded_pose_until_the_arm_has_lifted_it_clear() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(9));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    // A pose whose head pitch is toward the base: it must not go there early.
    let up = Standing::new(
        [2_050, 2_047],
        [2_100, 2_044, 2_015, 2_650, 2_600],
        100,
        4,
        60,
        [950, 950],
    )
    .unwrap();
    let news = spoken_to(&clock.time);
    let mut seen = Vec::new();
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(up),
        news,
        |glimpse| seen.push((glimpse.stand, glimpse.goals[0])),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let limits = Limits::FOLDED;
    for (stand, pitch) in &seen {
        if *stand <= 0.25 {
            // Beside the base: inside the folded reach and window.
            assert!(
                f32::from(*pitch) <= 2_000.0 + limits.pitch_down,
                "{stand} {pitch}"
            );
            assert!(*pitch <= limits.pitch_window.1);
        }
    }
    assert!(
        seen.iter()
            .any(|(stand, pitch)| *stand > 0.99 && *pitch > 2_060)
    );
    assert_eq!(lamp.servos[PITCH].goal, 2_000);
}

#[test]
fn an_arm_that_strains_goes_back_down_and_stays_down_for_a_while() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(12));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    // The shoulder reads over its bound for a second on the way up.
    lamp.strains = vec![(
        Duration::from_millis(2_500),
        Duration::from_millis(3_500),
        1,
        990,
    )];
    let time = clock.time.clone();
    let news = move || Some((Mood::Listening, 100));
    let mut after = Vec::new();
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 60)),
        news,
        |glimpse| {
            if time.get() > Duration::from_secs(9) {
                after.push(glimpse.stand);
            }
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.stood);
    assert_eq!(report.retreats.len(), 1, "{:?}", report.retreats);
    assert!(
        report.retreats[0].contains("base_pitch load 990"),
        "{:?}",
        report.retreats
    );
    assert_eq!(report.arm_peak_load[0], 990);
    // Still spoken to, and it stays down: it is resting.
    assert!(
        !after.is_empty() && after.iter().all(|stand| *stand == 0.0),
        "{after:?}"
    );
    assert_eq!((lamp.servos[1].goal, lamp.servos[2].goal), (2_050, 2_047));
}

/// lamp-4ace, 20 to 24 Oct: three strains of the elbow in half an hour sat
/// the lamp down for the rest of a five-day run, and it never stood again
/// until restarted. Three inside `Standing::RETREAT_WINDOW` rest it
/// `Standing::LONG_REST`, and then, still spoken to, it stands again.
#[test]
fn three_strains_rest_the_lamp_a_long_while_not_for_the_rest_of_the_run() {
    let long = Standing::LONG_REST;
    let ends = long + Duration::from_secs(300);
    let (mut lamp, mut clock) = folded(ends);
    lamp.arm_moves = true;
    lamp.overload_clears = 3;
    // The shoulder over its bound for a second on each of the first three
    // rises: at once, then after each minute's rest.
    lamp.strains = [2_500u64, 64_000, 126_000]
        .iter()
        .map(|&at| {
            (
                Duration::from_millis(at),
                Duration::from_millis(at + 1_000),
                1,
                990,
            )
        })
        .collect();
    let time = clock.time.clone();
    let news = move || Some((Mood::Listening, 100));
    let (mut resting, mut resting_s, mut after, mut after_s) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 60)),
        news,
        |glimpse| {
            let now = time.get();
            if now > Duration::from_secs(200) && now < long {
                resting.push(glimpse.stand);
                resting_s.push(glimpse.resting_s);
            }
            if now > ends - Duration::from_secs(60) {
                after.push(glimpse.stand);
                after_s.push(glimpse.resting_s);
            }
        },
    );
    assert_eq!(report.retreats.len(), 3, "{:?}", report.retreats);
    assert!(
        !resting.is_empty() && resting.iter().all(|stand| *stand == 0.0),
        "it rests after the third"
    );
    // Its health says so all through the rest, the seconds counting down.
    assert!(
        resting_s.iter().all(|left| *left > 0),
        "said while it rests"
    );
    assert!(resting_s.first() > resting_s.last(), "counting down");
    assert!(
        !after_s.is_empty() && after_s.iter().all(|left| *left == 0),
        "no longer said once it may rise: {after_s:?}"
    );
    assert!(
        after.iter().any(|stand| *stand > 0.99),
        "it stands again after the long rest: {:?}",
        after.last()
    );
}

/// #1518, the reviewer's P2: the fold that is itself the sit-down for a
/// strain, on the way up or while standing, says so, not "nothing retreated".
#[test]
fn the_fold_line_counts_a_sit_down_for_a_strain_on_the_way_up_or_standing() {
    for (from_ms, to_ms) in [(2_500, 3_500), (6_000, 7_000)] {
        let (mut lamp, mut clock) = folded(Duration::from_secs(12));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        lamp.strains = vec![(
            Duration::from_millis(from_ms),
            Duration::from_millis(to_ms),
            1,
            990,
        )];
        let news = move || Some((Mood::Listening, 100));
        let mut said = crate::bus_out::FoldSaid::default();
        let mut lines = Vec::new();
        let report = run_standing(
            &mut lamp,
            &mut clock,
            Limits::FOLDED,
            Some(HOME),
            Some(standing(100, 60)),
            news,
            |glimpse| lines.extend(said.step(glimpse)),
        );
        assert_eq!(report.retreats.len(), 1, "{:?}", report.retreats);
        assert_eq!(lines.len(), 1, "strain {from_ms} ms: {lines:?}");
        assert!(
            lines[0].ends_with("it sat down for a strain once since it was last folded"),
            "strain {from_ms} ms: {}",
            lines[0]
        );
    }
}

#[test]
fn an_arm_still_strained_two_seconds_on_ends_the_run_where_it_is() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(12));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    lamp.strains = vec![(
        Duration::from_millis(2_500),
        Duration::from_secs(60),
        2,
        990,
    )];
    let news = || Some((Mood::Listening, 100));
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 60)),
        news,
        |_| {},
    );
    assert_eq!(
        report.stop,
        Stop::Load {
            joint: 3,
            load: 990
        },
        "{report:?}"
    );
    assert_eq!(report.retreats.len(), 1);
    // Nothing is written after it: the arm holds where it had got to.
    let last = lamp.goals.last().unwrap();
    assert_eq!(goal_of(last, Joint::ElbowPitch), lamp.servos[2].goal);
}

#[test]
fn found_standing_it_is_taken_as_it_is_and_folds_down_when_asked_to_stop() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(1));
    lamp.arm_moves = true;
    for (index, at) in [(1usize, 2_650.0f32), (2, 2_600.0), (PITCH, 1_900.0)] {
        lamp.servos[index].position = at;
        lamp.servos[index].goal = at as u16;
        lamp.servos[index].status = 0;
    }
    let news = || Some((Mood::Resting, 0));
    let mut first = None;
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 60)),
        news,
        |glimpse| {
            first.get_or_insert(glimpse.stand);
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.torque_writes, 0);
    assert!(first.unwrap() > 0.98, "{first:?}");
    // Down again, by its own way, before it left.
    assert_eq!((lamp.servos[1].goal, lamp.servos[2].goal), (2_050, 2_047));
    assert_eq!(lamp.servos[PITCH].goal, 2_000);
    assert!((lamp.servos[1].position - 2_050.0).abs() <= 2.0);

    // Found somewhere that is not on the way, it runs as a folded lamp
    // would have to: it refuses, since its head is not at its home either.
    let (mut lamp, mut clock) = folded(Duration::from_secs(1));
    lamp.servos[1].position = 2_650.0;
    lamp.servos[1].goal = 2_650;
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 60)),
        || None,
        |_| {},
    );
    assert_eq!(report.standing, None);
    assert!(report.standing_refused.unwrap().contains("off the way"));
    assert!(lamp.goals.iter().all(|write| write.len() == 3));
}

/// `--let-go-at-stop` (the factory's, #1682; the first builder on #1722,
/// 19 Oct): a lamp found standing and asked to stop folds down first, as
/// always, and only then is let go, torque off on all five, so that it can
/// be read at rest. Without it the same stop keeps holding.
#[test]
fn asked_to_stop_standing_it_folds_first_and_only_then_lets_go_with_let_go_at_stop() {
    for let_go_at_stop in [true, false] {
        let (mut lamp, mut clock) = folded(Duration::from_secs(1));
        lamp.arm_moves = true;
        for (index, at) in [(1usize, 2_650.0f32), (2, 2_600.0), (PITCH, 1_900.0)] {
            lamp.servos[index].position = at;
            lamp.servos[index].goal = at as u16;
            lamp.servos[index].status = 0;
        }
        let report = run_standing(
            &mut lamp,
            &mut clock,
            Limits {
                let_go_at_stop,
                ..Limits::FOLDED
            },
            Some(HOME),
            Some(standing(100, 60)),
            || Some((Mood::Resting, 0)),
            |_| {},
        );
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        // Down first, by its own way, whichever.
        assert_eq!((lamp.servos[1].goal, lamp.servos[2].goal), (2_050, 2_047));
        assert!((lamp.servos[1].position - 2_050.0).abs() <= 2.0);
        assert_eq!(report.let_go_at_stop, let_go_at_stop);
        if let_go_at_stop {
            assert_eq!(report.torque_writes, 5, "{report:?}");
            assert!(lamp.servos.iter().all(|servo| servo.torque == 0));
        } else {
            assert_eq!(report.torque_writes, 0);
            assert!(lamp.servos.iter().all(|servo| servo.torque == 1));
        }
    }
}

/// The rule that keeps a standing arm from dropping (the reviewer's P2 on
/// #1776): a lamp that did not get down keeps holding even with the option.
/// Here the elbow stops 40 counts short of folded, which is no surprise (its
/// goal step is 50) but is not down either.
#[test]
fn asked_to_stop_with_its_elbow_short_of_folded_it_keeps_holding_with_let_go_at_stop() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(1));
    lamp.arm_moves = true;
    for (index, at) in [(1usize, 2_650.0f32), (2, 2_600.0), (PITCH, 1_900.0)] {
        lamp.servos[index].position = at;
        lamp.servos[index].goal = at as u16;
        lamp.servos[index].status = 0;
    }
    lamp.short = Some((2, 40.0));
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits {
            let_go_at_stop: true,
            ..Limits::FOLDED
        },
        Some(HOME),
        Some(standing(100, 60)),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    // Told to fold, it got 40 short of it, and is left resting there.
    assert!(lamp.servos[2].goal >= 2_080, "{}", lamp.servos[2].goal);
    assert!(
        lamp.servos[2].position >= 2_080.0,
        "{}",
        lamp.servos[2].position
    );
    assert!(!report.let_go_at_stop);
    assert_eq!(report.torque_writes, 0, "{report:?}");
    assert!(lamp.servos.iter().all(|servo| servo.torque == 1));
}

/// And a lamp folded at rest the whole run is let go at the stop with it,
/// and held without it.
#[test]
fn asked_to_stop_folded_at_rest_it_lets_go_only_with_let_go_at_stop() {
    let mut writes = Vec::new();
    for let_go_at_stop in [true, false] {
        let (mut lamp, mut clock) = folded(Duration::from_secs(1));
        let report = run_standing(
            &mut lamp,
            &mut clock,
            Limits {
                let_go_at_stop,
                ..Limits::FOLDED
            },
            Some(HOME),
            Some(standing(100, 60)),
            || Some((Mood::Resting, 0)),
            |_| {},
        );
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        assert!(!report.stood);
        assert_eq!(report.let_go_at_stop, let_go_at_stop);
        writes.push(report.torque_writes);
        assert_eq!(
            lamp.servos.iter().all(|servo| servo.torque == 0),
            let_go_at_stop
        );
    }
    // The elbow's old flag has its torque written at start either way; the
    // stop adds one torque off to each of the five.
    assert_eq!(writes[0], writes[1] + 5, "{writes:?}");
}

#[test]
fn a_flag_that_stays_or_a_missing_home_keeps_it_folded_and_it_runs_as_before() {
    // Torque on, then off and on: the flag is still there.
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.arm_moves = true;
    let news = script(&clock.time);
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 60)),
        news,
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.torque_writes, 3);
    assert_eq!(report.standing, None);
    assert!(
        report
            .standing_refused
            .unwrap()
            .contains("elbow_pitch keeps status 0x20")
    );
    assert!(!report.stood);
    assert!(lamp.goals.len() > 100);
    assert!(lamp.goals.iter().all(|write| write.len() == 3));
    assert_eq!((lamp.servos[1].goal, lamp.servos[2].goal), (2_050, 2_047));
    assert_eq!(lamp.servos[2].torque, 1);

    // One that clears only when torque goes off and on again does stand.
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.arm_moves = true;
    lamp.overload_clears = 2;
    let news = spoken_to(&clock.time);
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 60)),
        news,
        |_| {},
    );
    assert_eq!(report.torque_writes, 3);
    assert!(report.stood && report.standing.is_some(), "{report:?}");

    // Without a home there is no telling folded from stood up.
    let (mut lamp, mut clock) = folded(Duration::from_secs(2));
    let news = script(&clock.time);
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        None,
        Some(standing(100, 60)),
        news,
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled);
    assert_eq!(report.standing, None);
    assert_eq!(report.torque_writes, 0);
    assert!(report.standing_refused.unwrap().contains("needs a home"));
}

/// The eyes: a face at this bearing (counts of yaw and pitch from the home
/// direction), seen by a camera in the simulated head six times a second.
fn face_at(
    lamp_yaw_pitch: Rc<Cell<(f32, f32)>>,
    time: &Rc<Cell<Duration>>,
    bearing: (f32, f32),
) -> impl FnMut() -> Option<Seen> + use<> {
    let time = time.clone();
    let mut last = Duration::ZERO;
    move || {
        let now = time.get();
        if now.saturating_sub(last) < Duration::from_millis(150) {
            return None;
        }
        last = now;
        let (yaw, pitch) = lamp_yaw_pitch.get();
        // The picture is worth 342 counts across and 200 down, each half.
        let x = (bearing.0 - (yaw - f32::from(HOME[2]))) / 342.0;
        let y = (bearing.1 - (pitch - f32::from(HOME[0]))) / 200.0;
        Some(Seen {
            face: (x.abs() <= 1.0 && y.abs() <= 1.0).then_some(crate::gaze::Sight {
                x,
                y,
                size: 0.2,
                turned: 0.0,
            }),
        })
    }
}

#[test]
fn the_eyes_take_the_head_further_round_than_any_pose_does() {
    // A person standing 300 counts to one side, about 26 degrees: further
    // than this body turns for any pose (150), inside what its eyes may ask.
    let turned_for = |gaze_reach: u16| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(16));
        let looking = Looking::new([342, 200], [gaze_reach, 120]).unwrap();
        let limits = Limits::folded_with(Tuning {
            reach: [100, 15, 30, 150],
            speed: 120,
            servo_speed: 300,
            servo_acceleration: 60,
        })
        .unwrap();
        let seen_from = Rc::new(Cell::new((f32::from(HOME[2]), f32::from(HOME[0]))));
        let eyes = face_at(seen_from.clone(), &clock.time, (300.0, -60.0));
        let news = || Some((Mood::Listening, 100));
        let furthest = Cell::new(0.0f32);
        let report = run_looking(
            &mut lamp,
            &mut clock,
            limits,
            None,
            None,
            Some(looking),
            news,
            eyes,
            |glimpse| {
                seen_from.set((
                    f32::from(glimpse.positions[2]),
                    f32::from(glimpse.positions[0]),
                ));
                let yaw = f32::from(glimpse.positions[2]) - f32::from(HOME[2]);
                furthest.set(furthest.get().max(yaw));
            },
        );
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        furthest.get()
    };
    // With the eyes held to the poses' own reach it stops there, short.
    let short = turned_for(150);
    assert!((140.0..=152.0).contains(&short), "{short}");
    // Given more, it comes round to the person, and not past them.
    let round = turned_for(400);
    assert!((250.0..=330.0).contains(&round), "{round}");
}

#[test]
fn with_eyes_the_head_turns_to_the_face_while_it_listens_and_not_beyond_its_reach() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(14));
    let looking = Looking::new([342, 200], [250, 120]).unwrap();
    // A reach wide enough to turn to someone 120 counts to one side and 60
    // above: a third of the way to the edge of the picture.
    let limits = Limits::folded_with(Tuning {
        reach: [100, 15, 30, 150],
        speed: 120,
        servo_speed: 300,
        servo_acceleration: 60,
    })
    .unwrap();
    let seen_from = Rc::new(Cell::new((f32::from(HOME[2]), f32::from(HOME[0]))));
    let eyes = face_at(seen_from.clone(), &clock.time, (120.0, -60.0));
    let time = clock.time.clone();
    let news = move || {
        Some(if time.get() < Duration::from_secs(10) {
            (Mood::Listening, 100)
        } else {
            (Mood::Resting, 0)
        })
    };
    let mut looks = Vec::new();
    let while_listening = Cell::new((0.0f32, 0.0f32));
    let watched = clock.time.clone();
    let report = run_looking(
        &mut lamp,
        &mut clock,
        limits,
        None,
        None,
        Some(looking),
        news,
        eyes,
        |glimpse| {
            // The camera is where the head is.
            seen_from.set((
                f32::from(glimpse.positions[2]),
                f32::from(glimpse.positions[0]),
            ));
            if watched.get() < Duration::from_secs(9) {
                while_listening.set(seen_from.get());
            }
            looks.push(glimpse.look);
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    // It ended up looking toward the face, within what counts as near.
    // (The last looks are of it letting go, once it was asked to stop.)
    let turned = looks
        .iter()
        .copied()
        .max_by(|a, b| a[0].total_cmp(&b[0]))
        .unwrap();
    assert!((turned[0] - 120.0).abs() < 342.0 * 0.15, "{turned:?}");
    // Lifting to listen had already brought the face near the middle up
    // and down, so nothing was added there. Where the camera ended up, the
    // face is near the middle of its picture both ways.
    let (yaw, pitch) = while_listening.get();
    assert!(((120.0 - (yaw - 2_015.0)) / 342.0).abs() < 0.15, "{yaw}");
    assert!(
        ((-60.0 - (pitch - 2_000.0)) / 200.0).abs() < 0.20,
        "{pitch}"
    );
    // The head did go there, and every goal stayed inside the same reach
    // as every gesture.
    let furthest = lamp
        .goals
        .iter()
        .map(|write| goal_of(write, Joint::BaseYaw))
        .max()
        .unwrap();
    assert!(furthest >= 2_015 + 60, "{furthest}");
    for write in &lamp.goals {
        let yaw = f32::from(goal_of(write, Joint::BaseYaw));
        let pitch = f32::from(goal_of(write, Joint::WristPitch));
        assert!((yaw - 2_015.0).abs() <= limits.yaw, "{write:?}");
        assert!(pitch >= 2_000.0 - limits.pitch_up && pitch <= 2_000.0 + limits.pitch_down);
        assert_eq!(write.len(), 3);
    }
    // And it came home when asked to stop.
    assert_eq!(lamp.servos[YAW].goal, 2_015);
    assert_eq!(lamp.servos[PITCH].goal, 2_000);
}

/// The eyes may send the face the lamp answers (#1810), but a hand on the
/// base still wins: a base turning to that face meets the hand, is left
/// where it is for the whole ease, and is not turned on to the face. This
/// guards the body's existing rule, which #1810 rests on: the body can't
/// tell which face the eyes chose, so it is not a test of that choice.
#[test]
fn a_base_turning_to_a_face_that_meets_a_hand_is_left_where_it_is() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(12));
    let looking = Looking::new([342, 200], [400, 120]).unwrap();
    let limits = Limits::folded_with(Tuning {
        reach: [100, 15, 30, 150],
        speed: 120,
        servo_speed: 300,
        servo_acceleration: 60,
    })
    .unwrap();
    let seen_from = Rc::new(Cell::new((f32::from(HOME[2]), f32::from(HOME[0]))));
    // The one answered, 300 counts to one side, about 26 degrees.
    let eyes = face_at(seen_from.clone(), &clock.time, (300.0, 0.0));
    let (hand_from, hand_until) = (ms(3_000), ms(3_200));
    lamp.strains = vec![(hand_from, hand_until, YAW, 300)];
    let news = || Some((Mood::Speaking, 100));
    let watched = clock.time.clone();
    let mut seen = Vec::new();
    let report = run_looking(
        &mut lamp,
        &mut clock,
        limits,
        None,
        None,
        Some(looking),
        news,
        eyes,
        |glimpse| {
            seen_from.set((
                f32::from(glimpse.positions[2]),
                f32::from(glimpse.positions[0]),
            ));
            seen.push((watched.get(), *glimpse));
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    // It was turning to the face when the hand came.
    let before = seen.iter().rfind(|step| step.0 < hand_from).unwrap().1;
    assert!(before.look[0] > 0.0, "{before:?}");
    // It met the hand once, and was left where it was for the whole ease,
    // the face still to one side.
    assert_eq!(report.eased[2], 1, "{report:?}");
    let gave = seen.iter().position(|step| step.1.eased[2] == 1).unwrap();
    let held = seen[gave].1.goals[2];
    assert!(
        seen[gave..gave + usize::from(EASE_STEPS) - 1]
            .iter()
            .all(|step| step.1.goals[2] == held)
    );
    assert!(held < 2_015 + 300 - 30, "{held}");
    // And it goes on to the face once the ease is over.
    let after = seen[gave + usize::from(EASE_STEPS)..]
        .iter()
        .filter(|step| step.1.mood == Mood::Speaking)
        .map(|step| step.1.goals[2])
        .max()
        .unwrap();
    assert!(after > held, "{after} {held}");
}

#[test]
fn without_a_face_or_without_eyes_nothing_is_added_to_what_the_mood_wants() {
    let run_with = |looking: Option<Looking>| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(8));
        let news = script(&clock.time);
        let report = run_looking(
            &mut lamp,
            &mut clock,
            Limits::FOLDED,
            None,
            None,
            looking,
            news,
            || Some(Seen { face: None }),
            |_| {},
        );
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        lamp.goals
    };
    let plain = run_with(None);
    let eyed = run_with(Some(Looking::new([342, 200], [250, 120]).unwrap()));
    assert!(plain.len() > 100);
    assert_eq!(plain, eyed);
}

#[test]
fn an_elbow_pushing_under_an_old_overload_is_told_to_stop_and_then_carries_the_lamp() {
    // As lamp-4ace's was found: three counts short of its goal, which its
    // own friction keeps it from, flagged, and the flag goes only once it
    // is no longer pushing.
    let (mut lamp, mut clock) = folded(Duration::from_secs(22));
    lamp.arm_moves = true;
    lamp.overload_clears = 3;
    lamp.short = Some((2, 3.0));
    lamp.servos[2].position = 2_044.0;
    lamp.servos[2].goal = 2_047;
    let news = spoken_to(&clock.time);
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 5)),
        news,
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.standing_refused, None);
    // No torque was written: stopping the push was enough.
    assert_eq!(report.torque_writes, 0);
    assert!(lamp.registers.contains(&(3, 42, vec![0xfc, 0x07])));
    assert!(report.stood, "{report:?}");
    assert_eq!(lamp.servos[2].status, 0);
    // Down again, its arm is left resting where it is, not pushed at.
    let (shoulder, elbow) = (lamp.servos[1], lamp.servos[2]);
    assert!((f32::from(shoulder.goal) - shoulder.position).abs() < 1.0);
    assert!((f32::from(elbow.goal) - elbow.position).abs() < 1.0);
}

#[test]
fn stood_up_the_head_may_look_down_as_far_as_up_and_folded_it_may_not() {
    // A face well below the middle of the picture, for a lamp that listens.
    let below = || {
        Some(Seen {
            face: Some(crate::gaze::Sight {
                x: 0.0,
                y: 0.9,
                size: 0.2,
                turned: 0.0,
            }),
        })
    };
    let looking = Looking::new([342, 200], [250, 120]).unwrap();
    // The furthest down the head's pitch was asked to go, folded and at
    // its full height.
    let lowest = |stands: Option<Standing>| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(20));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        let (mut down, mut up) = (0u16, 0u16);
        let report = run_looking(
            &mut lamp,
            &mut clock,
            Limits::FOLDED,
            Some(HOME),
            stands,
            Some(looking),
            || Some((Mood::Listening, 100)),
            below,
            |glimpse| {
                if glimpse.stand <= 0.0 {
                    down = down.max(glimpse.goals[0]);
                } else if glimpse.stand >= 1.0 {
                    up = up.max(glimpse.goals[0]);
                }
            },
        );
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        (down, up)
    };
    // Folded, with its home at 2,000: fifteen counts under it and no more,
    // whatever is seen.
    assert_eq!(lowest(None), (2_015, 0));
    // Stood up, with its home at 1,900: down by its whole upward reach of 45.
    let (folded_low, stood_low) = lowest(Some(standing(100, 60)));
    assert!(folded_low <= 2_015, "{folded_low}");
    // (It comes to rest within a count of that bound.)
    assert!((1_944..=1_945).contains(&stood_low), "{stood_low}");
}

/// One step of a run as a gesture test watches it: when, the gesture being
/// made, the head's goals and the arm's.
type Step = (Duration, Option<&'static str>, [u16; 3], [u16; 2]);

/// A lamp that stands at once and is spoken to throughout, asked for one
/// gesture at a given moment.
fn gesture_run(
    clip: &'static Clip,
    ask_at: Duration,
    seconds: u64,
    prepare: impl FnOnce(&mut Lamp),
) -> (BodyReport, Lamp, Vec<Step>) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(seconds));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    prepare(&mut lamp);
    let limits = Limits::folded_with(Tuning {
        reach: [150, 15, 1, 250],
        speed: 120,
        servo_speed: 1_000,
        servo_acceleration: 60,
    })
    .unwrap();
    let time = clock.time.clone();
    let seen_at = clock.time.clone();
    let mut asked_once = false;
    let mut seen = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, 60)),
        None,
        || Some((Mood::Speaking, 60)),
        move || {
            let due = !asked_once && time.get() >= ask_at;
            asked_once |= due;
            Asked {
                toward: None,
                seen: None,
                gesture: due.then_some(clip),
                expression: None,
                called: false,
                still: false,
                paused: false,
                unplug: false,
                held_by_test: false,
                held: false,
                asleep: false,
                woken: false,
            }
        },
        |glimpse| {
            seen.push((
                seen_at.get(),
                glimpse.gesture,
                glimpse.goals,
                glimpse.arm_goals,
            ));
        },
    );
    (report, lamp, seen)
}

/// A lamp that stands at once and is in one mood throughout, asked by
/// Expression for `offsets` from `from` until `until`. Each step: when,
/// and the head's three goals.
fn expression_run(
    mood: Mood,
    offsets: [f32; 3],
    from: Duration,
    until: Duration,
    seconds: u64,
) -> (BodyReport, Vec<(Duration, [u16; 3])>) {
    expression_run_with(mood, offsets, from, until, seconds, false)
}

fn expression_run_with(
    mood: Mood,
    offsets: [f32; 3],
    from: Duration,
    until: Duration,
    seconds: u64,
    rest_glance: bool,
) -> (BodyReport, Vec<(Duration, [u16; 3])>) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(seconds));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let limits = Limits::folded_with(Tuning {
        reach: [150, 15, 1, 250],
        speed: 120,
        servo_speed: 1_000,
        servo_acceleration: 60,
    })
    .map(|limits| Limits {
        rest_glance,
        ..limits
    })
    .unwrap();
    let time = clock.time.clone();
    let seen_at = clock.time.clone();
    let mut seen = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, 60)),
        None,
        move || Some((mood, 60)),
        move || Asked {
            toward: None,
            seen: None,
            gesture: None,
            expression: (time.get() >= from && time.get() < until).then_some(offsets),
            called: false,
            still: false,
            paused: false,
            unplug: false,
            held_by_test: false,
            held: false,
            asleep: false,
            woken: false,
        },
        |glimpse| seen.push((seen_at.get(), glimpse.goals)),
    );
    (report, seen)
}

/// What Expression asks is added to the pose the lamp listens in: the
/// head dips for as long as it is asked and comes back to where it was,
/// at the speed everything else moves at, and nothing else turns.
#[test]
fn a_listening_lamp_dips_its_head_for_expression_and_comes_back() {
    let (from, until) = (Duration::from_secs(8), Duration::from_millis(8_600));
    // The run goes on for four seconds after what is looked at here: a
    // lamp that is told to stop folds down, and that is not the dip.
    let watched = Duration::from_secs(12);
    let (report, seen) = expression_run(Mood::Listening, [40.0, 0.0, 0.0], from, until, 16);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.expression_steps, 30, "0.6 s of 20 ms steps");
    assert!(report.retreats.is_empty(), "{:?}", report.retreats);
    let at = |when: Duration| seen.iter().rfind(|step| step.0 < when).unwrap().1;
    let before = at(from);
    // It listens with its head lifted, well clear of its home.
    assert!(before[0] + 40 < HOME[0], "{before:?} against {HOME:?}");
    let lowest = seen
        .iter()
        .filter(|step| step.0 >= from && step.0 < watched)
        .map(|step| step.1[0])
        .max()
        .unwrap();
    // A dip of 40 was asked for 0.6 s. The pose follows at 120 counts a
    // second with its own gentle start, so most of it is made.
    let dip = lowest - before[0];
    assert!((24..=41).contains(&dip), "the head dipped {dip} counts");
    // Back where it listened from, and at rest there.
    let after = at(Duration::from_millis(11_500));
    assert!(
        after[0].abs_diff(before[0]) <= 6,
        "{after:?} against {before:?}"
    );
    // Nothing else moved, and no goal leapt.
    for (index, name) in [(1, "roll"), (2, "yaw")] {
        let values: Vec<u16> = seen
            .iter()
            .filter(|step| step.0 >= from && step.0 < watched)
            .map(|step| step.1[index])
            .collect();
        let swing = values.iter().max().unwrap() - values.iter().min().unwrap();
        assert!(swing <= 1, "{name} moved {swing}");
    }
    for pair in seen
        .windows(2)
        .filter(|pair| pair[0].0 >= from && pair[1].0 < watched)
    {
        assert!(pair[1].1[0].abs_diff(pair[0].1[0]) <= 3, "{pair:?}");
    }
}

/// What is asked is cut to a few degrees and then to the joint's own
/// reach, a number that is not one is nothing, and a lamp at rest is not
/// stirred by it.
#[test]
fn expression_is_held_to_its_own_reach_and_the_joints_and_does_not_stir_a_resting_lamp() {
    let (from, until) = (Duration::from_secs(8), Duration::from_secs(10));
    let (report, seen) =
        expression_run(Mood::Listening, [500.0, f32::NAN, -500.0], from, until, 14);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let before = seen.iter().rfind(|step| step.0 < from).unwrap().1;
    let during: Vec<[u16; 3]> = seen
        .iter()
        .filter(|step| step.0 >= from && step.0 < until)
        .map(|step| step.1)
        .collect();
    let lowest = during.iter().map(|goals| goals[0]).max().unwrap();
    let leftmost = during.iter().map(|goals| goals[2]).min().unwrap();
    // Sixty counts of dip at the most, and never past what the head may
    // dip below its home; seventy of turn.
    assert!(lowest - before[0] <= 60, "dipped {}", lowest - before[0]);
    assert!(lowest <= HOME[0] + 15, "{lowest} against {HOME:?}");
    assert!(
        (60..=70).contains(&(before[2] - leftmost)),
        "turned {}",
        before[2] - leftmost
    );
    let roll: Vec<u16> = during.iter().map(|goals| goals[1]).collect();
    assert!(roll.iter().max().unwrap() - roll.iter().min().unwrap() <= 1);
    // At rest for eight seconds, and then asked: nothing is written.
    let (report, seen) = expression_run(Mood::Resting, [40.0, 0.0, 40.0], from, until, 14);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let before = seen.iter().rfind(|step| step.0 < from).unwrap().1;
    for step in seen
        .iter()
        .filter(|step| step.0 >= from && step.0 < Duration::from_secs(11))
    {
        assert_eq!(step.1, before, "at {:?}", step.0);
    }
}

/// A lamp that stands at once, listens, is in `mood` from six seconds to
/// twelve and listens again, asked by Expression for `offsets` from `from` until
/// `until`; `strains` as `Lamp::strains`. Each step: when, and the glimpse.
fn speaking_run(
    mood: Mood,
    offsets: [f32; 3],
    from: Duration,
    until: Duration,
    strains: Vec<(Duration, Duration, usize, u16)>,
) -> (BodyReport, Vec<(Duration, Glimpse)>) {
    speaking_run_with(mood, offsets, from, until, strains, |_, _| {})
}

/// The same, with the lamp and its limits changed first.
fn speaking_run_with(
    mood: Mood,
    offsets: [f32; 3],
    from: Duration,
    until: Duration,
    strains: Vec<(Duration, Duration, usize, u16)>,
    prepare: impl FnOnce(&mut Lamp, &mut Limits),
) -> (BodyReport, Vec<(Duration, Glimpse)>) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(18));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    lamp.strains = strains;
    let mut limits = Limits::folded_with(Tuning {
        reach: [150, 15, 1, 250],
        speed: 120,
        servo_speed: 1_000,
        servo_acceleration: 60,
    })
    .unwrap();
    prepare(&mut lamp, &mut limits);
    let time = clock.time.clone();
    let mood_at = clock.time.clone();
    let seen_at = clock.time.clone();
    let mut seen = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, 60)),
        None,
        move || {
            let at = mood_at.get().as_millis();
            Some(if (6_000..12_000).contains(&at) {
                (mood, 60)
            } else {
                (Mood::Listening, 60)
            })
        },
        move || Asked {
            toward: None,
            seen: None,
            gesture: None,
            expression: (time.get() >= from && time.get() < until).then_some(offsets),
            called: false,
            still: false,
            paused: false,
            unplug: false,
            held_by_test: false,
            held: false,
            asleep: false,
            woken: false,
        },
        |glimpse| seen.push((seen_at.get(), *glimpse)),
    );
    (report, seen)
}

/// Speaking, a standing lamp leans toward the person by `LEAN`, eased in
/// as the answer starts and out as it ends, no faster than the pose
/// follows; and a turn of the base Expression asks while it speaks (a sway
/// between sentences) is made, within 70 counts, and comes back.
#[test]
fn a_speaking_lamp_leans_in_and_back_and_its_base_sways_for_expression() {
    let ms = Duration::from_millis;
    let (report, seen) = speaking_run(
        Mood::Speaking,
        [0.0, 0.0, 52.0],
        ms(8_000),
        ms(9_500),
        vec![],
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.retreats.is_empty(), "{:?}", report.retreats);
    let at = |when: Duration| seen.iter().rfind(|step| step.0 < when).unwrap().1;
    let stood = at(ms(6_000));
    assert_eq!(stood.arm_goals, [2_650, 2_600], "{stood:?}");
    let leaning = at(ms(11_900));
    for (index, lean) in LEAN.iter().enumerate() {
        let by = f32::from(leaning.arm_goals[index]) - f32::from(stood.arm_goals[index]);
        assert!((by - lean).abs() <= 1.0, "{leaning:?}");
    }
    // Back as it stood once it listens again.
    assert_eq!(at(ms(15_900)).arm_goals, stood.arm_goals);
    // The base turned for the sway, by most of what was asked and never
    // more, and came back.
    let turned = seen
        .iter()
        .filter(|step| step.0 >= ms(8_000) && step.0 < ms(11_000))
        .map(|step| step.1.goals[2].abs_diff(stood.goals[2]))
        .max()
        .unwrap();
    assert!((35..=52).contains(&turned), "turned {turned}");
    assert!(at(ms(11_900)).goals[2].abs_diff(stood.goals[2]) <= 6);
    // Nothing leapt: no faster than 120 counts a second, 2.4 a step.
    for pair in seen
        .windows(2)
        .filter(|pair| pair[0].0 >= ms(6_000) && pair[1].0 < ms(16_000))
    {
        let (was, is) = (pair[0].1, pair[1].1);
        assert!(is.goals[2].abs_diff(was.goals[2]) <= 3, "{pair:?}");
        for index in 0..2 {
            assert!(
                is.arm_goals[index].abs_diff(was.arm_goals[index]) <= 3,
                "{pair:?}"
            );
        }
    }
}

/// A base that meets resistance while Expression asks it to sway is left
/// where it gave way: what Expression asks does not push it on.
#[test]
fn a_base_that_meets_a_hand_is_not_swayed_on_by_expression() {
    let ms = Duration::from_millis;
    let (report, seen) = speaking_run(
        Mood::Speaking,
        [0.0, 0.0, 52.0],
        ms(8_000),
        ms(10_500),
        vec![(ms(8_200), ms(8_400), YAW, 300)],
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.eased[2], 1, "{report:?}");
    let gave = seen.iter().position(|step| step.1.eased[2] == 1).unwrap();
    let held = seen[gave].1.goals[2];
    // Held for two seconds, through the rest of what was asked.
    assert!(
        seen[gave..gave + usize::from(EASE_STEPS) - 1]
            .iter()
            .all(|step| step.1.goals[2] == held)
    );
}

/// An elbow held by a hand as the lamp leans in to speak is held where the
/// hand has it, and the lean waits: it does not ease out by itself, which
/// would end the hold with the hand still there, and lean into the hand
/// again. Only once the answer is over does the arm go back to standing.
#[test]
fn an_arm_held_while_it_speaks_is_not_leaned_into_again() {
    let ms = Duration::from_millis;
    let (report, seen) = speaking_run_with(
        Mood::Speaking,
        [0.0; 3],
        ms(0),
        ms(0),
        vec![],
        |lamp, limits| {
            // From half a second into the answer, a hand keeps the elbow
            // where it stands, and pushes back as hard as the arm does.
            lamp.hand = Some((ms(6_500), ms(11_000), 2, 2_600.0));
            lamp.strains = vec![(ms(6_500), ms(11_000), 2, 400)];
            limits.give_way = Some(GiveWay::Return);
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.stalls[4], 1, "it stopped pushing once: {report:?}");
    let stalled = seen.iter().position(|step| step.1.stalls[4] == 1).unwrap();
    let held = seen[stalled].1.arm_goals;
    // Held there while the hand is on it: no lean, in or out.
    for step in seen[stalled..].iter().filter(|step| step.0 < ms(11_000)) {
        assert_eq!(step.1.arm_goals, held, "{step:?}");
    }
    // And standing as before once it listens again.
    let after = seen.iter().rfind(|step| step.0 < ms(15_900)).unwrap().1;
    assert_eq!(after.arm_goals, [2_650, 2_600], "{after:?}");
}

/// A lamp that stands at once and speaks in `answers` (milliseconds, from
/// and until), listening otherwise, until it is asked to stop at `stop`.
fn held_lean_run(
    stop: Duration,
    answers: &'static [(u128, u128)],
    prepare: impl FnOnce(&mut Lamp),
) -> (BodyReport, Vec<(Duration, Glimpse)>) {
    let (mut lamp, mut clock) = folded(stop);
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    prepare(&mut lamp);
    let mut limits = Limits::folded_with(Tuning {
        reach: [150, 15, 1, 250],
        speed: 120,
        servo_speed: 1_000,
        servo_acceleration: 60,
    })
    .unwrap();
    limits.give_way = Some(GiveWay::Return);
    let mood_at = clock.time.clone();
    let seen_at = clock.time.clone();
    let mut seen = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, 60)),
        None,
        move || {
            let at = mood_at.get().as_millis();
            Some(
                if answers
                    .iter()
                    .any(|(from, until)| (*from..*until).contains(&at))
                {
                    (Mood::Speaking, 60)
                } else {
                    (Mood::Listening, 60)
                },
            )
        },
        Asked::default,
        |glimpse| seen.push((seen_at.get(), *glimpse)),
    );
    (report, seen)
}

/// The most any arm goal moves in one step from `from` until `until`.
fn largest_arm_step_between(seen: &[(Duration, Glimpse)], from: Duration, until: Duration) -> u16 {
    seen.windows(2)
        .filter(|pair| pair[0].0 >= from && pair[1].0 < until)
        .flat_map(|pair| {
            (0..2).map(|index| pair[1].1.arm_goals[index].abs_diff(pair[0].1.arm_goals[index]))
        })
        .max()
        .unwrap_or(0)
}

/// A hand holds one arm joint (`servo` 1 the shoulder, 2 the elbow) at `at`
/// from half a second into the 6–12 s answer until `until`. After the
/// answer no arm goal moves more than the lean's own step, and while the
/// hand is there none goes past the held place toward the lean; once it has
/// gone the arm stands as before.
fn eased_out_from_a_hand(servo: usize, at: f32, until: Duration) {
    let ms = Duration::from_millis;
    let (report, seen) = held_lean_run(ms(18_000), &[(6_000, 12_000)], |lamp| {
        lamp.hand = Some((ms(6_500), until, servo, at));
        lamp.strains = vec![(ms(6_500), until, servo, 400)];
    });
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let arm = servo - 1;
    let stalled = seen
        .iter()
        .position(|step| step.1.stalls[3 + arm] >= 1)
        .unwrap();
    let held = seen[stalled].1.arm_goals[arm];
    // About 1.2 counts a step at `--speed` 120, rounded.
    let largest = largest_arm_step_between(&seen, ms(12_000), ms(18_000));
    assert!(largest <= 2, "moved {largest} in a step");
    // `LEAN` is down for both: past the held place is below it.
    for step in seen[stalled..].iter().filter(|step| step.0 < until) {
        assert!(step.1.arm_goals[arm] >= held, "{step:?} held {held}");
    }
    let last = seen.iter().rfind(|step| step.0 < ms(17_900)).unwrap().1;
    assert_eq!(last.arm_goals, [2_650, 2_600], "{last:?}");
}

#[test]
fn a_lean_eases_out_from_where_a_hand_held_it() {
    eased_out_from_a_hand(2, 2_600.0, Duration::from_millis(11_000));
}

#[test]
fn a_lean_eases_out_from_a_hand_still_there_after_the_answer() {
    eased_out_from_a_hand(2, 2_600.0, Duration::from_millis(15_000));
}

/// The reviewer's 16:28 probes: a hand that lets go of the elbow while the
/// lean eases out, held at three places in the lean's way.
#[test]
fn a_hand_letting_go_mid_ease_out_leaves_no_jump() {
    for at in [2_570.0, 2_580.0, 2_600.0] {
        eased_out_from_a_hand(2, at, Duration::from_millis(12_700));
    }
}

#[test]
fn a_held_shoulder_eases_out_as_an_elbow_does() {
    eased_out_from_a_hand(1, 2_650.0, Duration::from_millis(15_000));
    eased_out_from_a_hand(1, 2_650.0, Duration::from_millis(11_000));
}

/// Folding while a held lean eases out, asked to stop or retreating from
/// a hard push: the held joint folds with the rest of the arm, no step of
/// it larger than the same fold makes with no hand at all.
#[test]
fn a_held_lean_folds_with_the_arm_without_a_jump() {
    let ms = Duration::from_millis;
    let elbow = |until: Duration, push: bool| {
        move |lamp: &mut Lamp| {
            lamp.hand = Some((ms(6_500), until, 2, 2_600.0));
            lamp.strains = if push {
                vec![(ms(9_000), ms(9_300), 2, 1_000), (ms(6_500), until, 2, 400)]
            } else {
                vec![(ms(6_500), until, 2, 400)]
            };
        }
    };
    // Asked to stop at 11 s, the hand gone soon after or still there.
    let (_, bare) = held_lean_run(ms(11_000), &[(6_000, 12_000)], |_| {});
    let unheld = largest_arm_step_between(&bare, ms(10_000), Duration::MAX);
    for until in [ms(11_500), ms(15_000)] {
        let (report, seen) = held_lean_run(ms(11_000), &[(6_000, 12_000)], elbow(until, false));
        assert_eq!(report.stalls[4], 1, "{report:?}");
        let largest = largest_arm_step_between(&seen, ms(10_000), Duration::MAX);
        assert!(largest <= unheld, "{largest} against {unheld} with no hand");
    }
    // Pushed hard enough to retreat, the hand gone at 9.3 s.
    let (_, bare) = held_lean_run(ms(18_000), &[(6_000, 12_000)], |lamp| {
        lamp.strains = vec![(ms(9_000), ms(9_300), 2, 1_000)];
    });
    let unheld = largest_arm_step_between(&bare, ms(8_000), ms(18_000));
    let (report, seen) = held_lean_run(ms(18_000), &[(6_000, 12_000)], elbow(ms(9_300), true));
    assert_eq!(report.retreats.len(), 1, "{report:?}");
    // From the stall on: the stall itself only stops the goal where the elbow is.
    let stalled = seen.iter().find(|step| step.1.stalls[4] == 1).unwrap().0;
    let largest = largest_arm_step_between(&seen, stalled, ms(18_000));
    assert!(largest <= unheld, "{largest} against {unheld} with no hand");
}

/// A hold is not kept with no hand on the arm: held at or past standing,
/// or pressed down past the lean, and let go before the answer ends, the
/// arm stands as before after it and the next answer leans again.
#[test]
fn a_hand_gone_does_not_keep_the_next_answer_from_leaning() {
    let ms = Duration::from_millis;
    for at in [2_600.0, 2_603.0, 2_520.0] {
        let (report, seen) =
            held_lean_run(ms(25_000), &[(6_000, 12_000), (17_000, 23_000)], |lamp| {
                lamp.hand = Some((ms(6_500), ms(11_000), 2, at));
                lamp.strains = vec![(ms(6_500), ms(11_000), 2, 400)];
            });
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        let before = seen.iter().rfind(|step| step.0 < ms(17_000)).unwrap().1;
        assert_eq!(before.arm_goals, [2_650, 2_600], "{at}: {before:?}");
        let leant = seen.iter().rfind(|step| step.0 < ms(23_000)).unwrap().1;
        assert_eq!(leant.arm_goals, [2_605, 2_540], "{at}: {leant:?}");
        let largest = largest_arm_step_between(&seen, ms(11_000), ms(25_000));
        assert!(largest <= 2, "{at}: moved {largest} in a step");
    }
}

/// The reviewer's 15:49 probe: a hand pushes the standing elbow 30 counts
/// up while the lamp listens and lets go; the answer after it leans.
#[test]
fn a_hold_from_before_the_answer_does_not_keep_it_from_leaning() {
    let ms = Duration::from_millis;
    let (report, seen) = held_lean_run(ms(14_000), &[(8_000, 12_000)], |lamp| {
        lamp.hand = Some((ms(5_000), ms(7_000), 2, 2_630.0));
        lamp.strains = vec![(ms(5_000), ms(7_000), 2, 400)];
    });
    assert_eq!(report.stalls[4], 1, "{report:?}");
    let leant = seen.iter().rfind(|step| step.0 < ms(12_000)).unwrap().1;
    assert_eq!(leant.arm_goals, [2_605, 2_540], "{leant:?}");
    let largest = largest_arm_step_between(&seen, ms(7_000), ms(14_000));
    assert!(largest <= 2, "moved {largest} in a step");
}

/// The reviewer's 23:39 probe: the wire goes silent for four steps while
/// the lamp leans. The lean is kept as it stands, not dropped and taken
/// again: no arm goal written moves more than the lean's own step.
#[test]
fn a_wire_failing_mid_lean_keeps_the_lean_as_it_stands() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(ms(14_000));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    lamp.silent_from = Some(ms(10_000));
    lamp.silent_until = Some(ms(10_080));
    let mut limits = Limits::folded_with(Tuning {
        reach: [150, 15, 1, 250],
        speed: 120,
        servo_speed: 1_000,
        servo_acceleration: 60,
    })
    .unwrap();
    limits.give_way = Some(GiveWay::Return);
    let mood_at = clock.time.clone();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, 60)),
        None,
        move || {
            let at = mood_at.get().as_millis();
            Some(if (6_000..12_000).contains(&at) {
                (Mood::Speaking, 60)
            } else {
                (Mood::Listening, 60)
            })
        },
        Asked::default,
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.missed_reads >= 3, "{report:?}");
    // Every arm goal written from 9 s until the stop, joint by joint.
    for id in ARM.map(Joint::id) {
        let written: Vec<u16> = lamp
            .goals
            .iter()
            .zip(&lamp.goal_times)
            .filter(|(_, at)| (ms(9_000)..ms(14_000)).contains(*at))
            .flat_map(|(goals, _)| {
                goals
                    .iter()
                    .filter(|(of, _)| *of == id)
                    .map(|(_, goal)| *goal)
            })
            .collect();
        let largest = written
            .windows(2)
            .map(|pair| pair[1].abs_diff(pair[0]))
            .max()
            .unwrap_or(0);
        assert!(largest <= 2, "servo {id} moved {largest} in a write");
    }
}

/// The reviewer's 23:39 probe: asked to stop mid-answer with a hand still
/// pressing the elbow down past the lean. Leaving, no stall check runs, so
/// the hand's offset is not kept: the elbow folds to its folded pose.
#[test]
fn a_hand_on_the_elbow_does_not_take_the_fold_past_its_pose() {
    let ms = Duration::from_millis;
    let (report, seen) = held_lean_run(ms(11_000), &[(6_000, 12_000)], |lamp| {
        lamp.hand = Some((ms(6_500), ms(15_000), 2, 2_450.0));
        lamp.strains = vec![(ms(6_500), ms(15_000), 2, 400)];
    });
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let last = seen.last().unwrap().1;
    assert_eq!(last.arm_goals[1], 2_047, "{last:?}");
    assert!(
        seen.iter().all(|step| step.1.arm_goals[1] >= 2_047),
        "went past the fold"
    );
}

/// Reading, a standing lamp looks down at the desk as far as its head may
/// look down stood up, leans its arm forward, holds that, and comes back
/// once it is not reading; what Expression asks of the head's pitch does
/// not bob it meanwhile.
#[test]
fn a_reading_lamp_looks_down_at_the_desk_holds_it_and_comes_back() {
    let ms = Duration::from_millis;
    let (report, seen) = speaking_run(
        Mood::Reading,
        [-40.0, 0.0, 0.0],
        ms(9_000),
        ms(10_000),
        vec![],
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.retreats.is_empty(), "{:?}", report.retreats);
    let at = |when: Duration| seen.iter().rfind(|step| step.0 < when).unwrap().1;
    let listening = at(ms(6_000));
    // Stood up at 1,900, the head may look down 150 counts, its upward
    // reach: 2,050, reached (a joint that has arrived rests a count or
    // two short) and held to the end of reading.
    for when in [8_500, 9_500, 11_900] {
        let reading = at(ms(when));
        assert!(
            (2_048..=2_050).contains(&reading.goals[0]),
            "at {when}: {reading:?}"
        );
        for (index, lean) in LEAN.iter().enumerate() {
            let by = f32::from(reading.arm_goals[index]) - f32::from(listening.arm_goals[index]);
            assert!((by - lean).abs() <= 1.0, "at {when}: {reading:?}");
        }
    }
    let back = at(ms(15_900));
    assert!(back.goals[0].abs_diff(listening.goals[0]) <= 6, "{back:?}");
    assert_eq!(back.arm_goals, listening.arm_goals);
    // Never farther down than that, nor faster than the pose follows.
    assert!(seen.iter().all(|step| step.1.goals[0] <= 2_050));
    for pair in seen
        .windows(2)
        .filter(|pair| pair[0].0 >= ms(6_000) && pair[1].0 < ms(16_000))
    {
        let (was, is) = (pair[0].1, pair[1].1);
        assert!(is.goals[0].abs_diff(was.goals[0]) <= 3, "{pair:?}");
        for index in 0..2 {
            assert!(
                is.arm_goals[index].abs_diff(was.arm_goals[index]) <= 3,
                "{pair:?}"
            );
        }
    }
}

/// A lamp in `mood` from six seconds to twelve, listening before and after,
/// asked to turn `toward` a place across the picture all the while, with
/// `looking` as its gaze and the eyes seeing a `face` that far across, if
/// any, every step. Each step: when, and the glimpse.
fn toward_run(
    mood: Mood,
    looking: Option<Looking>,
    toward: Option<f32>,
    face: Option<f32>,
) -> (BodyReport, Vec<(Duration, Glimpse)>) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(18));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let limits = Limits::folded_with(Tuning {
        reach: [150, 15, 1, 250],
        speed: 120,
        servo_speed: 1_000,
        servo_acceleration: 60,
    })
    .unwrap();
    let mood_at = clock.time.clone();
    let seen_at = clock.time.clone();
    let mut seen = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, 60)),
        looking,
        move || {
            let at = mood_at.get().as_millis();
            Some(if (6_000..12_000).contains(&at) {
                (mood, 60)
            } else {
                (Mood::Listening, 60)
            })
        },
        move || Asked {
            toward,
            seen: face.map(|x| Seen {
                face: Some(crate::gaze::Sight {
                    x,
                    y: 0.0,
                    size: 0.2,
                    turned: 0.0,
                }),
            }),
            ..Asked::default()
        },
        |glimpse| seen.push((seen_at.get(), *glimpse)),
    );
    (report, seen)
}

/// #1679, "light my book": reading toward a place on the left of the
/// picture, the base turns as it would to a face there (70% of the
/// half-picture's 342 counts times a half: 120), at the pose's own speed,
/// holds it while reading and comes back after. Not without a gaze, which
/// alone knows how far a place in the picture is, nor in another mood; a
/// place past the gaze's reach is turned to as far as the reach.
#[test]
fn a_reading_lamp_turns_toward_the_place_it_is_asked_and_back() {
    let ms = Duration::from_millis;
    let looking = Looking::new([342, 200], [250, 120]).unwrap();
    let at = |seen: &[(Duration, Glimpse)], when: Duration| {
        seen.iter().rfind(|step| step.0 < when).unwrap().1
    };
    let (report, seen) = toward_run(Mood::Reading, Some(looking), Some(-0.5), None);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let (_, plain) = toward_run(Mood::Reading, Some(looking), None, None);
    for when in [9_000, 11_900] {
        let (turned, straight) = (at(&seen, ms(when)), at(&plain, ms(when)));
        let by = i32::from(turned.goals[2]) - i32::from(straight.goals[2]);
        assert!((118..=121).contains(&by.abs()), "at {when}: {by}");
        // The head still looks down at the desk as reading does.
        assert_eq!(turned.goals[0], straight.goals[0], "at {when}");
    }
    let back = at(&seen, ms(15_900));
    assert!(
        back.goals[2].abs_diff(at(&plain, ms(15_900)).goals[2]) <= 6,
        "{back:?}"
    );
    for pair in seen
        .windows(2)
        .filter(|pair| pair[0].0 >= ms(6_000) && pair[1].0 < ms(16_000))
    {
        assert!(
            pair[1].1.goals[2].abs_diff(pair[0].1.goals[2]) <= 3,
            "{pair:?}"
        );
    }
    // Without a gaze, or listening: as if nothing were asked.
    let (_, blind) = toward_run(Mood::Reading, None, Some(-0.5), None);
    let (_, blind_plain) = toward_run(Mood::Reading, None, None, None);
    assert_eq!(blind, blind_plain);
    let (_, listening) = toward_run(Mood::Listening, Some(looking), Some(-0.5), None);
    let (_, listening_plain) = toward_run(Mood::Listening, Some(looking), None, None);
    assert_eq!(listening, listening_plain);
    // Past the reach (1.0 would be 239 counts): as far as a reach of 100.
    let near = Looking::new([342, 200], [100, 120]).unwrap();
    let (_, far) = toward_run(Mood::Reading, Some(near), Some(1.0), None);
    let (_, far_plain) = toward_run(Mood::Reading, Some(near), None, None);
    let by =
        i32::from(at(&far, ms(11_900)).goals[2]) - i32::from(at(&far_plain, ms(11_900)).goals[2]);
    assert!((98..=101).contains(&by.abs()), "{by}");
}

/// #1738, "look for my keys": a search never looks toward a person. A face
/// seen on the left keeps a look to the left over the desk, as if none were
/// asked, and says which place it refused; a face on the right refuses
/// nothing to the left, and no face turns as before.
#[test]
fn a_search_does_not_turn_toward_the_side_a_face_is_seen_on() {
    let ms = Duration::from_millis;
    let looking = Looking::new([342, 200], [250, 120]).unwrap();
    let at = |seen: &[(Duration, Glimpse)], when: Duration| {
        seen.iter().rfind(|step| step.0 < when).unwrap().1
    };
    let run = |toward, face| toward_run(Mood::Reading, Some(looking), toward, face).1;
    let (left, left_plain) = (run(Some(-0.5), Some(-0.5)), run(None, Some(-0.5)));
    let (right, right_plain) = (run(Some(-0.5), Some(0.5)), run(None, Some(0.5)));
    let (none, none_plain) = (run(Some(-0.5), None), run(None, None));
    for when in [9_000, 11_900] {
        let by = |turned: &[(Duration, Glimpse)], plain: &[(Duration, Glimpse)]| {
            i32::from(at(turned, ms(when)).goals[2]) - i32::from(at(plain, ms(when)).goals[2])
        };
        assert_eq!(by(&left, &left_plain), 0, "at {when}");
        assert_eq!(at(&left, ms(when)).toward_refused, Some(-0.5), "at {when}");
        assert!(
            (118..=121).contains(&by(&right, &right_plain).abs()),
            "at {when}"
        );
        assert_eq!(at(&right, ms(when)).toward_refused, None, "at {when}");
        assert!(
            (118..=121).contains(&by(&none, &none_plain).abs()),
            "at {when}"
        );
        assert_eq!(at(&none, ms(when)).toward_refused, None, "at {when}");
    }
}

/// Without `rest_glance`, a lamp at rest writes exactly what it writes
/// when nothing is asked of it.
#[test]
fn without_the_switch_a_resting_lamp_writes_exactly_as_when_nothing_is_asked() {
    let (from, until) = (Duration::from_secs(8), Duration::from_millis(9_500));
    let (_, asked) = expression_run(Mood::Resting, [-46.0, 30.0, 40.0], from, until, 14);
    let (_, quiet) = expression_run(Mood::Resting, [0.0; 3], from, from, 14);
    assert_eq!(asked, quiet);
}

/// With `rest_glance`, a lamp at rest lifts its head for as long as it is
/// asked, within the smaller reach, turns no base, and rests again where it
/// was.
#[test]
fn a_resting_lamp_glances_with_the_switch_and_comes_back_to_rest() {
    let (from, until) = (Duration::from_secs(8), Duration::from_millis(9_500));
    let (report, seen) =
        expression_run_with(Mood::Resting, [-500.0, 500.0, 40.0], from, until, 14, true);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let before = seen.iter().rfind(|step| step.0 < from).unwrap().1;
    let during: Vec<[u16; 3]> = seen
        .iter()
        .filter(|step| step.0 >= from && step.0 < until)
        .map(|step| step.1)
        .collect();
    let highest = during.iter().map(|goals| goals[0]).min().unwrap();
    let lift = before[0] - highest;
    // Fifty counts at the most, its goal led forty ahead of that.
    assert!((80..=90).contains(&lift), "lifted {lift}");
    let tilt = during.iter().map(|goals| goals[1]).max().unwrap() - before[1];
    // The roll's own reach here is one count, and it is kept to it.
    assert!(tilt <= 1, "tilted {tilt}");
    assert!(during.iter().all(|goals| goals[2] == before[2]));
    let after = seen
        .iter()
        .rfind(|step| step.0 < Duration::from_millis(13_500))
        .unwrap()
        .1;
    for index in 0..3 {
        assert!(
            after[index].abs_diff(before[index]) <= 6,
            "{after:?} against {before:?}"
        );
    }
}

const GLANCE_FROM: Duration = Duration::from_secs(10);
const GLANCE_UNTIL: Duration = Duration::from_millis(11_000);

/// The glance of the rest-glance tests: from `GLANCE_FROM` to
/// `GLANCE_UNTIL` Expression dips the head of a lamp resting where the
/// factory's self-test finds it, and `set` gives the lamp and the limits
/// what the test needs. Each step's time, and the pitch's goal and position.
fn rest_glance(
    set: impl FnOnce(&mut Lamp, &mut Limits),
) -> (BodyReport, Vec<(Duration, u16, u16)>) {
    rest_glances(set, vec![(GLANCE_FROM, GLANCE_UNTIL, 40.0)])
}

/// The same with these glances: from, until and the pitch asked.
fn rest_glances(
    set: impl FnOnce(&mut Lamp, &mut Limits),
    glances: Vec<(Duration, Duration, f32)>,
) -> (BodyReport, Vec<(Duration, u16, u16)>) {
    rest_glances_watched(set, glances, |_, _| {})
}

/// The same fixture, with each full step available to reporting tests.
fn rest_glances_watched(
    set: impl FnOnce(&mut Lamp, &mut Limits),
    glances: Vec<(Duration, Duration, f32)>,
    mut watch: impl FnMut(Duration, &Glimpse),
) -> (BodyReport, Vec<(Duration, u16, u16)>) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(16));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    // Found standing, as the factory's self-test finds it, its head at its
    // goal there.
    for (servo, at) in lamp
        .servos
        .iter_mut()
        .zip([2_015, 2_650, 2_600, 2_044, 1_900])
    {
        servo.position = f32::from(at);
        servo.goal = at;
    }
    let mut limits = Limits::folded_with(Tuning {
        reach: [150, 15, 1, 250],
        speed: 120,
        servo_speed: 1_000,
        servo_acceleration: 60,
    })
    .map(|limits| Limits {
        rest_glance: true,
        ..limits
    })
    .unwrap();
    set(&mut lamp, &mut limits);
    let time = clock.time.clone();
    let seen_at = clock.time.clone();
    let mut seen = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, 60)),
        None,
        || Some((Mood::Resting, 0)),
        move || Asked {
            toward: None,
            seen: None,
            gesture: None,
            expression: glances
                .iter()
                .find(|(from, until, _)| (*from..*until).contains(&time.get()))
                .map(|(_, _, pitch)| [*pitch, 0.0, 0.0]),
            called: false,
            still: false,
            paused: false,
            unplug: false,
            held_by_test: false,
            held: false,
            asleep: false,
            woken: false,
        },
        |glimpse| {
            let at = seen_at.get();
            seen.push((at, glimpse.goals[0], glimpse.positions[0]));
            watch(at, glimpse);
        },
    );
    (report, seen)
}

/// A stationary hand can cross the pitch's contact floor while a glance
/// permits a higher motion load. Contact still pins it, and the hand must
/// be reported throughout that hold before sustained pressure makes it slump.
#[test]
fn a_pitch_contact_during_glance_cooling_is_said_as_a_hand_before_it_slumps() {
    let ms = Duration::from_millis;
    let pressed = GLANCE_UNTIL;
    let load = 675;
    let mut seen = Vec::new();
    let (report, _) = rest_glances_watched(
        |lamp, limits| {
            assert!(limits.load[0] < load);
            assert!(CONTACT_FLOOR[0] < load && load <= GESTURE_LOAD);
            limits.give_way = Some(GiveWay::Return);
            limits.limp = true;
            lamp.strains
                .push((pressed, pressed + ms(2_000), PITCH, load));
        },
        vec![(GLANCE_FROM, pressed + ms(3_000), 40.0)],
        |at, glimpse| seen.push((at, *glimpse)),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.expression_steps > 0, "{report:?}");
    assert_eq!(report.eased, [1, 0, 0], "{report:?}");
    assert_eq!(report.released_by_hand, [1, 0, 0], "{report:?}");
    assert_eq!(report.released.len(), 1, "{report:?}");
    assert_eq!(report.released[0].joint, Joint::WristPitch.name());
    assert_eq!(report.released[0].load, load);

    // Contact's three reads pin it before the ordinary load guard's six.
    let first = seen.iter().find(|(_, g)| g.eased[0] == 1).unwrap();
    assert!(
        first.0 >= pressed && first.0 < pressed + TICK * u32::from(Limits::FOLDED.load_steps),
        "first contact at {:?}, pressed at {pressed:?}",
        first.0
    );
    let window = pressed + ms(200)..pressed + ms(900);
    assert!(Duration::from_secs_f32(report.released[0].seconds) > window.end);
    let held: Vec<_> = seen.iter().filter(|(at, _)| window.contains(at)).collect();
    assert!(!held.is_empty(), "no steps before the slump");
    for (_, g) in held {
        assert!(!g.limp && g.released == 0 && g.eased == [1, 0, 0], "{g:?}");
        assert_eq!(g.loads[0], load, "{g:?}");
        assert!(g.positions[0].abs_diff(first.1.positions[0]) <= HAND_MOVED);
        assert!(g.goals[0].abs_diff(g.positions[0]) <= HAND_MOVED);
        assert!(g.hand, "the pinned contact was not reported: {g:?}");
    }
}

/// A head that stops short only when it lifts, as lamp-52e6's does (26
/// counts short in the factory's self-test on #2818): dipped by a glance at
/// rest, it is led back up to where it rested, and is back within the
/// self-test's 12 counts within its 3 s, not left resting 26 below.
#[test]
fn a_resting_head_dipped_by_a_glance_is_led_back_up_where_it_stops_short_lifting() {
    let (report, seen) = rest_glance(|lamp, _| {
        // Lifting, the head stops 26 short of its goal; it dips all the way.
        lamp.heavy = Some((PITCH, 0.0, 26.0, 25));
    });
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let at = |when: Duration| seen.iter().rfind(|step| step.0 < when).unwrap().2;
    let before = at(GLANCE_FROM);
    let lowest = seen
        .iter()
        .filter(|step| step.0 >= GLANCE_FROM && step.0 < GLANCE_UNTIL)
        .map(|step| step.2)
        .max()
        .unwrap();
    assert!(lowest >= before + 30, "dipped from {before} to {lowest}");
    let after = at(GLANCE_UNTIL + Duration::from_secs(3));
    assert!(
        after.abs_diff(before) <= 12,
        "back at {after}, rested at {before}"
    );
}

/// How far the goal is above where the head was read: led, it is more than
/// `ARRIVED_COUNTS`.
fn lead_of(step: &(Duration, u16, u16)) -> f32 {
    f32::from(step.2) - f32::from(step.1)
}

/// A head that arrives is no longer led: a free head back within
/// `ARRIVED_COUNTS` of its way is never led at all, and one led back up
/// from 26 short (above) has its goal back at its way once it is there.
#[test]
fn a_resting_head_back_within_its_way_after_a_glance_is_not_led() {
    let (report, seen) = rest_glance(|_, _| {});
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let most = seen.iter().map(lead_of).fold(f32::MIN, f32::max);
    assert!(most <= ARRIVED_COUNTS, "a free head led {most} counts");
    let (report, seen) = rest_glance(|lamp, _| lamp.heavy = Some((PITCH, 0.0, 26.0, 25)));
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(
        seen.iter()
            .any(|step| lead_of(step) > GLANCE_LIFT_LEAD / 2.0)
    );
    let back = GLANCE_UNTIL + Duration::from_secs(1);
    for step in seen.iter().filter(|step| step.0 >= back) {
        assert!(
            lead_of(step).abs() <= ARRIVED_COUNTS,
            "at {:?} the goal is {} and the head {}",
            step.0,
            step.1,
            step.2
        );
    }
}

/// A head held part way up by a hand as a glance lets it go back: a glance
/// is judged with a gesture's bounds, so neither the load nor the tracking
/// check ends a lead into the hand. It is led for no more steps than the
/// tracking check lets a head stay off its goal, and then pushes no
/// harder than its way. The hand reads 150, under the run's own bound.
#[test]
fn a_head_held_part_way_back_from_a_glance_is_not_led_into_the_hand() {
    let held = (GLANCE_UNTIL, GLANCE_UNTIL + Duration::from_secs(3));
    let mut tracking_steps = 0;
    let (report, seen) = rest_glance(|lamp, limits| {
        lamp.hand = Some((held.0, held.1, PITCH, 1_920.0));
        lamp.strains.push((held.0, held.1, PITCH, 150));
        tracking_steps = usize::from(limits.tracking_steps);
    });
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let led = seen
        .iter()
        .filter(|step| (held.0..held.1).contains(&step.0) && lead_of(step) > GLANCE_LIFT_LEAD)
        .count();
    assert!(
        led <= usize::from(GLANCE_LEAD_STILL_STEPS) + 1,
        "led {led} steps into a hand"
    );
}

/// A head that stops 26 short lifting and rises slowly, 2 counts a step,
/// keeps closing on its way, so it stays led until it arrives: on #2818
/// lamp-52e6's reached its way only 10 s later once its lead was cut after
/// a count of steps rather than when it stopped closing.
#[test]
fn a_head_rising_slowly_back_from_a_glance_stays_led_until_it_arrives() {
    let mut tracking_steps = 0;
    let (report, seen) = rest_glance(|lamp, limits| {
        lamp.heavy = Some((PITCH, 0.0, 26.0, 25));
        lamp.climb = Some((PITCH, 100.0));
        tracking_steps = usize::from(limits.tracking_steps);
    });
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    // Led for longer than the tracking check's steps, while it closes.
    let led = seen
        .iter()
        .filter(|step| step.0 >= GLANCE_UNTIL && lead_of(step) > GLANCE_LIFT_LEAD / 2.0)
        .count();
    assert!(led > tracking_steps + 1, "led {led} steps only");
    // And back within the self-test's 12 counts within its 3 s.
    let at = |when: Duration| seen.iter().rfind(|step| step.0 < when).unwrap().2;
    let before = at(GLANCE_FROM);
    let after = at(GLANCE_UNTIL + Duration::from_secs(3));
    assert!(
        after.abs_diff(before) <= 12,
        "back at {after}, rested at {before}"
    );
}

/// A head that creeps back up from 40 counts down, 2 counts a step for
/// 100 ms and then still for 400 ms, over and over, as lamp-52e6's did on
/// #2818: its pauses are not a hand, so it keeps its lead through them and
/// is back within the self-test's 12 counts within its 3 s.
#[test]
fn a_head_creeping_back_from_a_glance_with_pauses_keeps_its_lead() {
    let (report, seen) = rest_glance(|lamp, _| {
        lamp.heavy = Some((PITCH, 0.0, 26.0, 25));
        lamp.climb = Some((PITCH, 100.0));
        lamp.creep = Some((
            PITCH,
            Duration::from_millis(100),
            Duration::from_millis(400),
        ));
    });
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let at = |when: Duration| seen.iter().rfind(|step| step.0 < when).unwrap().2;
    let before = at(GLANCE_FROM);
    let lowest = seen
        .iter()
        .filter(|step| step.0 >= GLANCE_FROM && step.0 < GLANCE_UNTIL)
        .map(|step| step.2)
        .max()
        .unwrap();
    assert!(lowest >= before + 36, "dipped from {before} to {lowest}");
    let after = at(GLANCE_UNTIL + Duration::from_secs(3));
    assert!(
        after.abs_diff(before) <= 12,
        "back at {after}, rested at {before}"
    );
}

/// The self-test's moves come before a lamp that has just stood up has
/// rested a second, and each next one before it rests again: still at
/// rest, its head is led back up from the dip all the same, not left where
/// it stopped short. On lamp-52e6 it rested 17 below after the factory's
/// self-test, and no lead began (#2818).
#[test]
fn a_head_dipped_before_its_rest_has_lasted_is_led_back_up() {
    let ms = Duration::from_millis;
    let dip = (ms(1_800), ms(2_800));
    let (report, seen) = rest_glances(
        |lamp, _| lamp.heavy = Some((PITCH, 0.0, 26.0, 25)),
        vec![(ms(400), ms(1_200), -40.0), (dip.0, dip.1, 40.0)],
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let at = |when: Duration| seen.iter().rfind(|step| step.0 < when).unwrap().2;
    let before = at(dip.0);
    let lowest = seen
        .iter()
        .filter(|step| (dip.0..dip.1).contains(&step.0))
        .map(|step| step.2)
        .max()
        .unwrap();
    assert!(lowest >= before + 30, "dipped from {before} to {lowest}");
    let after = at(dip.1 + Duration::from_secs(3));
    assert!(
        after.abs_diff(before) <= 12,
        "back at {after}, rested at {before}"
    );
}

#[test]
fn a_standing_lamp_nods_and_is_left_exactly_where_it_was() {
    // Standing takes a little over four seconds; the nod is asked at eight.
    let (report, lamp, seen) = gesture_run(&crate::clips::NOD, Duration::from_secs(8), 16, |_| {});
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!((report.gestures, report.gestures_dropped), (1, 0));
    assert!(report.retreats.is_empty(), "{:?}", report.retreats);
    let during: Vec<_> = seen.iter().filter(|step| step.1 == Some("nod")).collect();
    let before = seen
        .iter()
        .rfind(|step| step.0 < Duration::from_secs(8))
        .unwrap();
    let after = seen
        .iter()
        .rfind(|step| step.0 < Duration::from_secs(13))
        .unwrap();
    // It lasted as long as the clip, moved the head's pitch and both joints
    // of the arm by about what the clip says, and nothing else.
    let lasted = during.last().unwrap().0 - during[0].0;
    assert!(
        (lasted.as_millis() as i64 - 3_068).abs() < 100,
        "{lasted:?}"
    );
    let swing = |pick: &dyn Fn(&Step) -> u16| {
        let values: Vec<u16> = during.iter().map(|step| pick(step)).collect();
        values.iter().max().unwrap() - values.iter().min().unwrap()
    };
    assert!(
        (100..=115).contains(&swing(&|step| step.2[0])),
        "{}",
        swing(&|step| step.2[0])
    );
    assert!(
        (135..=150).contains(&swing(&|step| step.3[0])),
        "{}",
        swing(&|step| step.3[0])
    );
    assert!(
        (200..=220).contains(&swing(&|step| step.3[1])),
        "{}",
        swing(&|step| step.3[1])
    );
    assert_eq!(swing(&|step| step.2[2]), 0);
    // No goal jumps: a clip is followed, not leapt to.
    for pair in during.windows(2) {
        assert!(pair[1].2[0].abs_diff(pair[0].2[0]) <= 24, "{pair:?}");
        assert!(pair[1].3[1].abs_diff(pair[0].3[1]) <= 21, "{pair:?}");
    }
    // And afterwards it stands as it stood.
    assert_eq!((before.2, before.3), (after.2, after.3));
    assert_eq!(after.1, None);
    assert_eq!((lamp.servos[1].goal, lamp.servos[2].goal), (2_050, 2_047));
}

/// V1's anti-tip rule (src/cog.rs): a gesture any frame of which would
/// take the lamp's centre of gravity more than 22 mm off its base's axis is
/// refused whole before it moves anything, and the run goes on.
/// A clip made here past the line, so the test does not hang on how far
/// the shipped clips reach: the shoulder and elbow swung 400 counts.
static PAST_THE_LINE: Clip = Clip {
    name: "past_the_line",
    step_ms: 50,
    frames: &[
        [0, 0, 0, 0, 0],
        [0, 0, 0, 200, -200],
        [0, 0, 0, 400, -400],
        [0, 0, 0, 200, -200],
        [0, 0, 0, 0, 0],
    ],
};

#[test]
fn a_gesture_past_v1s_tip_over_line_is_refused_whole_and_one_inside_it_plays() {
    let (report, lamp, seen) = gesture_run(&PAST_THE_LINE, Duration::from_secs(8), 12, |_| {});
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.tipping.refused, 1, "{:?}", report.tipping);
    let (name, _, mm) = report.tipping.last_refused.unwrap();
    assert_eq!(name, "past_the_line");
    assert!(mm > 22.0, "{mm}");
    assert!(seen.iter().all(|step| step.1.is_none()));
    // Nothing of it was played: the pose before is the pose after.
    let at = |when: u64| {
        let step = seen
            .iter()
            .rfind(|step| step.0 < Duration::from_secs(when))
            .unwrap();
        (step.2, step.3)
    };
    assert_eq!(at(8), at(12));
    assert_eq!((lamp.servos[1].goal, lamp.servos[2].goal), (2_050, 2_047));
    // One inside the line plays as before, and nothing is said of it.
    let (report, _, _) = gesture_run(&crate::clips::NOD, Duration::from_secs(8), 12, |_| {});
    assert_eq!(report.gestures, 1);
    assert_eq!(report.tipping, Tipping::default());
}

/// A skill's move, sent whole on the mood socket (`{"move":{...}}`),
/// becomes a gesture like the body's own and is played the same way: a
/// move with `nod`'s frames writes every goal `nod` writes, and one inside
/// the move rules but past V1's 22 mm (the shoulder and elbow swung their
/// whole 350 counts) is refused whole by the anti-tip rule, which the
/// journal says as `anti-tip: REFUSED gesture NAME`.
#[test]
fn a_skills_move_plays_as_a_kept_move_does_and_one_past_v1s_line_is_refused() {
    let mut sent = crate::moves::Sent::default();
    let mut send = |name: &str, step_ms: u16, frames: &[[i16; 5]]| {
        let datagram =
            serde_json::json!({ "move": { "name": name, "step_ms": step_ms, "frames": frames }})
                .to_string();
        sent.take(datagram.as_bytes()).unwrap().unwrap()
    };
    let nod = &crate::clips::NOD;
    let nod_again = send("nod-again", nod.step_ms, nod.frames);
    let (kept, _, kept_steps) = gesture_run(&crate::clips::NOD, Duration::from_secs(8), 12, |_| {});
    let (skills, _, skills_steps) = gesture_run(nod_again, Duration::from_secs(8), 12, |_| {});
    assert_eq!(skills.gestures, 1, "{skills:?}");
    assert_eq!(skills.tipping, Tipping::default());
    let goals = |steps: &[Step]| -> Vec<_> { steps.iter().map(|s| (s.0, s.2, s.3)).collect() };
    assert_eq!(goals(&skills_steps), goals(&kept_steps));
    assert_eq!(kept.gestures, 1);
    assert!(skills_steps.iter().any(|s| s.1 == Some("nod-again")));
    // Past the line: refused whole, named, and nothing of it played.
    let swing: Vec<[i16; 5]> = (0..=40i16)
        .map(|step| {
            let counts = 35 * step.min(10).min(40 - step);
            [0, 0, 0, counts, -counts]
        })
        .collect();
    let past = send("past-the-line", 50, &swing);
    let (report, lamp, seen) = gesture_run(past, Duration::from_secs(8), 12, |_| {});
    assert_eq!(report.tipping.refused, 1, "{:?}", report.tipping);
    let (name, _, mm) = report.tipping.last_refused.unwrap();
    assert_eq!(name, "past-the-line");
    assert!(mm > 22.0, "{mm}");
    assert!(seen.iter().all(|step| step.1.is_none()));
    assert_eq!((lamp.servos[1].goal, lamp.servos[2].goal), (2_050, 2_047));
}

/// V1's anti-tip rule: no goal leans the lamp back past 22 mm while it
/// rises, nor past where it is once it stands (16 mm, and it is past
/// that): a standing pose behind that line is stopped short of, and said.
#[test]
fn a_lamp_is_not_stood_up_farther_back_than_v1s_line() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(10));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let limits = Limits::folded_with(Tuning {
        reach: [150, 15, 1, 250],
        speed: 120,
        servo_speed: 1_000,
        servo_acceleration: 60,
    })
    .unwrap();
    // The shoulder 250 counts under folded: some 27 mm back.
    let pose = [1_900, 2_044, 2_015, 1_800, 2_047];
    let up = Standing::new([2_050, 2_047], pose, 100, 4, 60, [950, 950]).unwrap();
    let back = |head: [u16; 3], arm: [u16; 2]| {
        crate::cog::lean_back_mm([head[2], arm[0], arm[1], head[1], head[0]].map(f32::from))
    };
    assert!(back([1_900, 2_044, 2_015], [1_800, 2_047]) > 25.0);
    let mut steps = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(up),
        None,
        || Some((Mood::Speaking, 60)),
        || Asked {
            toward: None,
            seen: None,
            gesture: None,
            expression: None,
            called: false,
            still: false,
            paused: false,
            unplug: false,
            held_by_test: false,
            held: false,
            asleep: false,
            woken: false,
        },
        |glimpse| steps.push((glimpse.goals, glimpse.arm_goals, glimpse.stand)),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.stood);
    assert_eq!(report.tipping.cuts, 1, "{:?}", report.tipping);
    assert_eq!(report.tipping.ceiling_mm, 22.0);
    assert!(report.tipping.would_mm > 22.0);
    let most = steps
        .iter()
        .map(|(head, arm, _)| back(*head, *arm))
        .fold(0.0, f64::max);
    assert!(most <= 22.0 && most > 21.5, "{most}");
    // It stood as far as the line, not where it was told.
    let lowest = steps.iter().map(|(_, arm, _)| arm[0]).min().unwrap();
    assert!(lowest > 1_850 && lowest < 2_050, "{lowest}");
    // Held there as the head is: its way up goes no farther than the
    // arm, so nothing is left to jump the rest once the rule lets go.
    for (_, arm, stand) in &steps {
        let way = 2_050.0 + (1_800.0 - 2_050.0) * smooth(*stand);
        assert!((f32::from(arm[0]) - way).abs() <= 2.0, "{arm:?} at {stand}");
    }
    assert!(steps.last().unwrap().2 < 1.0);
}

#[test]
fn a_gesture_waits_for_a_lamp_that_is_rising_and_is_forgotten_by_one_that_is_not() {
    // Asked three seconds in, while it rises: it begins once it stands.
    let (report, _, seen) = gesture_run(&crate::clips::NOD, Duration::from_secs(3), 12, |_| {});
    assert_eq!(report.gestures, 1, "{report:?}");
    let began = seen.iter().find(|step| step.1.is_some()).unwrap().0;
    assert!(
        began > Duration::from_millis(4_400) && began < Duration::from_secs(6),
        "{began:?}"
    );
    // Asked at once, more than three seconds before it stands: it waits
    // for the rest of the rise too (the reviewer on #1080), and begins.
    let (report, _, seen) = gesture_run(&crate::clips::NOD, Duration::from_millis(100), 10, |_| {});
    assert_eq!(report.gestures, 1, "{report:?}");
    assert!(seen.iter().any(|step| step.1.is_some()));
    // A folded lamp that may not stand makes none either.
    let (mut lamp, mut clock) = folded(Duration::from_secs(4));
    let mut asked_once = false;
    let report = run_alive(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        None,
        None,
        None,
        || Some((Mood::Speaking, 60)),
        move || Asked {
            toward: None,
            seen: None,
            gesture: (!std::mem::replace(&mut asked_once, true)).then_some(&crate::clips::NOD),
            expression: None,
            called: false,
            still: false,
            paused: false,
            unplug: false,
            held_by_test: false,
            held: false,
            asleep: false,
            woken: false,
        },
        |_| {},
    );
    assert_eq!((report.stop, report.gestures), (Stop::Cancelled, 0));
    assert!(lamp.goals.iter().all(|write| write.len() == 3));
}

#[test]
fn a_joint_that_cannot_keep_up_ends_the_gesture_and_not_the_run() {
    // The head's pitch meets something a little above where it stands.
    let (report, lamp, seen) =
        gesture_run(&crate::clips::NOD, Duration::from_secs(8), 16, |lamp| {
            lamp.strains = vec![(
                Duration::from_millis(8_600),
                Duration::from_millis(9_200),
                PITCH,
                900,
            )];
        });
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!((report.gestures, report.gestures_dropped), (1, 1));
    // It let go early, went back to where it stood, and stayed standing.
    let during: Vec<_> = seen.iter().filter(|step| step.1 == Some("nod")).collect();
    let lasted = during.last().unwrap().0 - during[0].0;
    assert!(lasted < Duration::from_millis(1_200), "{lasted:?}");
    let before = seen
        .iter()
        .rfind(|step| step.0 < Duration::from_secs(8))
        .unwrap();
    let after = seen
        .iter()
        .rfind(|step| step.0 < Duration::from_secs(13))
        .unwrap();
    assert_eq!((before.2, before.3), (after.2, after.3));
    assert!(report.retreats.is_empty());
    assert_eq!(lamp.servos[2].status, 0);
}

#[test]
fn the_heads_pitch_is_never_asked_under_its_floor_whatever_adds_up() {
    // A stretch lifts the head 112 counts; with a floor just under where it
    // stands, the head stops there and the arm makes its move all the same.
    let (mut lamp, mut clock) = folded(Duration::from_secs(16));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let limits = Limits {
        pitch_floor: 1_880,
        ..Limits::folded_with(Tuning {
            reach: [150, 15, 1, 250],
            speed: 120,
            servo_speed: 1_000,
            servo_acceleration: 60,
        })
        .unwrap()
    };
    let time = clock.time.clone();
    let mut asked_once = false;
    let (mut lowest, mut highest_arm) = (u16::MAX, 0u16);
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, 60)),
        None,
        || Some((Mood::Thinking, 0)),
        move || {
            let due = !asked_once && time.get() >= Duration::from_secs(8);
            asked_once |= due;
            Asked {
                toward: None,
                seen: None,
                gesture: due.then_some(&crate::clips::STRETCHING),
                expression: None,
                called: false,
                still: false,
                paused: false,
                unplug: false,
                held_by_test: false,
                held: false,
                asleep: false,
                woken: false,
            }
        },
        |glimpse| {
            lowest = lowest.min(glimpse.goals[0]);
            highest_arm = highest_arm.max(glimpse.arm_goals[0]);
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!((report.gestures, report.gestures_dropped), (1, 0));
    assert_eq!(lowest, 1_880);
    assert!(highest_arm >= 2_650 + 290, "{highest_arm}");
}

#[test]
fn stood_up_the_head_tilts_to_listen_and_folded_it_does_not() {
    // A lamp whose head cannot tilt where it is folded: a roll reach of one
    // count, and more once it has stood up.
    let tilt = |stand_roll: f32| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(12));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        let limits = Limits {
            stand_roll,
            ..Limits::folded_with(Tuning {
                reach: [150, 15, 1, 250],
                speed: 120,
                servo_speed: 1_000,
                servo_acceleration: 60,
            })
            .unwrap()
        };
        let (mut folded_tilt, mut stood_tilt) = (0u16, 0u16);
        let report = run_standing(
            &mut lamp,
            &mut clock,
            limits,
            Some(HOME),
            Some(standing(100, 60)),
            || Some((Mood::Listening, 100)),
            |glimpse| {
                let tilt = glimpse.goals[1].abs_diff(HOME[1]);
                if glimpse.stand <= 0.25 {
                    folded_tilt = folded_tilt.max(tilt);
                } else if glimpse.stand >= 1.0 {
                    stood_tilt = stood_tilt.max(tilt);
                }
            },
        );
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        (folded_tilt, stood_tilt)
    };
    // Listening tilts 85% of the reach: of one count folded, and of 110
    // once it stands.
    let (folded_tilt, stood_tilt) = tilt(110.0);
    assert!(folded_tilt <= 1, "{folded_tilt}");
    assert!((90..=94).contains(&stood_tilt), "{stood_tilt}");
    // Without a standing reach it never tilts.
    assert!(tilt(0.0).1 <= 1);
}

/// A shift at rest seen in a run: when it began, when it ended, which.
type Shift = (Duration, Duration, &'static str);

/// Spoken to for two seconds, then left standing for `minutes`, in `mood`
/// after that, with a hand bumping the head's tilt every `bumps` seconds
/// if given (and giving way to it): the shifts it made.
fn shifts_at_rest(
    idle: bool,
    minutes: u64,
    mood: Mood,
    share: u8,
    bumps: Option<u64>,
    stream: Option<fn(Duration) -> Asked>,
) -> (BodyReport, Vec<Shift>) {
    let (report, shifts, _) = run_at_rest(idle, minutes, mood, share, bumps, stream);
    (report, shifts)
}

/// The same, and the head's pitch goal at every step.
#[allow(clippy::type_complexity)]
fn run_at_rest(
    idle: bool,
    minutes: u64,
    mood: Mood,
    share: u8,
    bumps: Option<u64>,
    stream: Option<fn(Duration) -> Asked>,
) -> (BodyReport, Vec<Shift>, Vec<(Duration, u16)>) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(minutes * 60));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    if let Some(every) = bumps {
        lamp.strains = (1..minutes * 60 / every)
            .map(|at| {
                let from = Duration::from_secs(at * every);
                (from, from + Duration::from_millis(200), ROLL, 217)
            })
            .collect();
    }
    let limits = Limits {
        give_way: bumps.map(|_| GiveWay::Return),
        rest_glance: stream.is_some(),
        ..Limits::folded_with(Tuning {
            reach: [150, 15, 1, 250],
            speed: 120,
            servo_speed: 1_000,
            servo_acceleration: 60,
        })
        .unwrap()
    };
    let time = clock.time.clone();
    let watched = clock.time.clone();
    let heard = clock.time.clone();
    let mut shifts: Vec<Shift> = Vec::new();
    let mut playing: Option<(Duration, &'static str)> = None;
    let mut pitch: Vec<(Duration, u16)> = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(share, 3_600).idling(idle).seeded(5)),
        None,
        move || {
            Some(if time.get() < Duration::from_secs(2) {
                (Mood::Listening, 100)
            } else {
                (mood, 0)
            })
        },
        move || stream.map_or_else(Asked::default, |stream| stream(heard.get())),
        |glimpse| {
            pitch.push((watched.get(), glimpse.goals[0]));
            let shift = glimpse
                .gesture
                .filter(|name| idle::MOVES.iter().any(|shift| shift.name == *name));
            match (playing, shift) {
                (None, Some(name)) => playing = Some((watched.get(), name)),
                (Some((from, name)), now) if now != Some(name) => {
                    shifts.push((from, watched.get(), name));
                    playing = now.map(|name| (watched.get(), name));
                }
                _ => {}
            }
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    (report, shifts, pitch)
}

/// Left standing at rest, it shifts now and then: never sooner than 40 s
/// after the last shift ended, nor later than three minutes, no two waits
/// alike, and by more than one move. The first comes at least 40 s after it
/// was spoken to.
#[test]
fn standing_with_nothing_to_do_it_shifts_rarely_and_irregularly() {
    let (report, shifts) = shifts_at_rest(true, 20, Mood::Resting, 100, None, None);
    assert_eq!(report.idles as usize, shifts.len());
    assert!(shifts.len() >= 4, "{shifts:?}");
    assert!(
        shifts[0].0 >= Duration::from_secs(2) + idle::GAP_LEAST,
        "{shifts:?}"
    );
    let mut waits: Vec<Duration> = shifts
        .windows(2)
        .map(|pair| pair[1].0 - pair[0].1)
        .collect();
    for wait in &waits {
        assert!(*wait >= idle::GAP_LEAST, "{shifts:?}");
        assert!(*wait <= idle::GAP_MOST + TICK, "{shifts:?}");
    }
    assert!(
        shifts.iter().any(|shift| shift.2 != shifts[0].2),
        "{shifts:?}"
    );
    waits.sort();
    waits.dedup();
    assert_eq!(waits.len(), shifts.len() - 1, "{shifts:?}");
    // Each move is played whole, as recorded.
    for (from, to, name) in &shifts {
        let clip = idle::MOVES
            .iter()
            .find(|shift| shift.name == *name)
            .unwrap();
        assert!((*to - *from).abs_diff(Duration::from_micros(clip.duration_us())) <= TICK * 2);
    }
    assert!(report.retreats.is_empty());
    // Not asked to, it stands still.
    let (report, shifts) = shifts_at_rest(false, 10, Mood::Resting, 100, None, None);
    assert_eq!((report.idles, shifts.len()), (0, 0));
}

/// Never while someone talks near it, never folded or half up, and never
/// while a hand is on it: each push on the head starts the wait again.
#[test]
fn it_does_not_shift_while_talked_to_folded_or_held() {
    for mood in [Mood::Listening, Mood::Speaking, Mood::Thinking] {
        let (report, _) = shifts_at_rest(true, 10, mood, 100, None, None);
        assert_eq!(report.idles, 0, "{mood:?}");
    }
    let (report, _) = shifts_at_rest(true, 10, Mood::Resting, 60, None, None);
    assert_eq!(report.idles, 0);
    // A bump every 30 s: always inside the shortest wait.
    let (report, _) = shifts_at_rest(true, 10, Mood::Resting, 100, Some(30), None);
    assert!(report.eased[1] >= 15, "{report:?}");
    assert_eq!(report.idles, 0, "{report:?}");
}

/// Expression's rest stream, renewed every step under `--rest-glance`
/// (zeros, or a small breath), is no reason not to shift: the shift plays
/// over it as with no stream. Stillness asked of it (a hold) and a pause
/// are: no shift at all.
#[test]
fn it_shifts_over_expressions_rest_stream_and_not_while_held_still_or_paused() {
    let (_, alone) = shifts_at_rest(true, 20, Mood::Resting, 100, None, None);
    let zeros: fn(Duration) -> Asked = |_| Asked {
        expression: Some([0.0; 3]),
        ..Asked::default()
    };
    let breath: fn(Duration) -> Asked = |at| Asked {
        expression: Some([(at.as_secs_f32() * 1.2).sin() * 4.0, 0.0, 0.0]),
        ..Asked::default()
    };
    for stream in [zeros, breath] {
        let (report, shifts) = shifts_at_rest(true, 20, Mood::Resting, 100, None, Some(stream));
        assert_eq!(report.idles as usize, shifts.len());
        assert!(shifts.len() >= 4, "{shifts:?}");
        let names = |all: &[Shift]| all.iter().map(|shift| shift.2).collect::<Vec<_>>();
        assert_eq!(names(&shifts), names(&alone));
    }
    let held: fn(Duration) -> Asked = |_| Asked {
        still: true,
        ..Asked::default()
    };
    let paused: fn(Duration) -> Asked = |_| Asked::PAUSED;
    for stream in [held, paused] {
        let (report, _) = shifts_at_rest(true, 10, Mood::Resting, 100, None, Some(stream));
        assert_eq!(report.idles, 0);
    }
}

/// One layer with Expression's: a shift starts and ends where Expression's
/// rest offsets hold the head, lifted 30 counts here, with no dip back to
/// plain rest before it or after it.
#[test]
fn a_shift_starts_and_ends_at_expressions_rest_offset() {
    let lifted: fn(Duration) -> Asked = |_| Asked {
        expression: Some([-30.0, 0.0, 0.0]),
        ..Asked::default()
    };
    let (_, shifts, pitch) = run_at_rest(true, 20, Mood::Resting, 100, None, Some(lifted));
    assert!(shifts.len() >= 4, "{shifts:?}");
    let goal_at = |at: Duration| pitch.iter().rfind(|step| step.0 <= at).unwrap().1;
    for (from, to, name) in &shifts {
        let before = goal_at(*from - Duration::from_secs(1));
        // Its first 0.2 s: the clip has barely begun, and the head stays
        // where the rest offset had it (it would sink 30 counts otherwise).
        for step in pitch
            .iter()
            .filter(|step| step.0 >= *from && step.0 < *from + Duration::from_millis(200))
        {
            assert!(
                step.1.abs_diff(before) <= 4,
                "{name} at {from:?}: {step:?} from {before}"
            );
        }
        // Over: back where it was, and not by way of plain rest.
        let after = goal_at(*to + Duration::from_secs(2));
        assert!(
            after.abs_diff(before) <= 2,
            "{name} at {to:?}: {after} from {before}"
        );
        let lowest_after = pitch
            .iter()
            .filter(|step| step.0 >= *to && step.0 < *to + Duration::from_secs(2))
            .map(|step| step.1.abs_diff(before))
            .max()
            .unwrap();
        assert!(lowest_after <= 6, "{name} at {to:?}: {lowest_after}");
    }
}

/// One quiet window: no shift starts within 30 s of a drift of
/// Expression's rest layer (a breath, and every 50 s the head drifts 20
/// counts up or back down over 3 s), and shifts still come between them.
#[test]
fn no_shift_starts_within_thirty_seconds_of_a_drift() {
    let drifting: fn(Duration) -> Asked = |at| {
        let seconds = at.as_secs_f32();
        let drift = |seconds: f32| {
            let into = (seconds % 50.0 / 3.0).min(1.0);
            let ease = (1.0 - (into * std::f32::consts::PI).cos()) / 2.0;
            let (from, to) = if ((seconds / 50.0) as u32).is_multiple_of(2) {
                (0.0, -20.0)
            } else {
                (-20.0, 0.0)
            };
            from + (to - from) * ease
        };
        let breath = (1.0 - (seconds / 5.2 * std::f32::consts::TAU).cos()) / 2.0 * -20.0;
        Asked {
            expression: Some([drift(seconds) + breath, 0.0, 0.0]),
            ..Asked::default()
        }
    };
    let (_, shifts) = shifts_at_rest(true, 20, Mood::Resting, 100, None, Some(drifting));
    assert!(shifts.len() >= 3, "{shifts:?}");
    let mut seen_early = 0;
    for (from, _, name) in &shifts {
        let since_drift = from.as_secs_f32() % 50.0;
        // A drift is seen by the average a couple of seconds into it: a
        // shift begun before then plays on (the two overlap as one move).
        assert!(
            !(3.0..30.0).contains(&since_drift),
            "{name} at {from:?}: {since_drift} s after a drift"
        );
        seen_early += usize::from(since_drift < 3.0);
    }
    assert!(seen_early * 3 < shifts.len(), "{shifts:?}");
    // The breath alone holds off nothing.
    let breathing: fn(Duration) -> Asked = |at| Asked {
        expression: Some([
            (1.0 - (at.as_secs_f32() / 5.2 * std::f32::consts::TAU).cos()) / 2.0 * -20.0,
            0.0,
            0.0,
        ]),
        ..Asked::default()
    };
    let (_, alone) = shifts_at_rest(true, 20, Mood::Resting, 100, None, None);
    let (_, breathed) = shifts_at_rest(true, 20, Mood::Resting, 100, None, Some(breathing));
    assert_eq!(breathed.len(), alone.len(), "{breathed:?} {alone:?}");
}

/// Spoken to in the middle of a shift, it lets the shift go at once.
#[test]
fn spoken_to_in_the_middle_of_a_shift_it_lets_it_go_at_once() {
    let (_, shifts) = shifts_at_rest(true, 5, Mood::Resting, 100, None, None);
    let (from, _, _) = shifts[0];
    let again = from + Duration::from_secs(1);
    let (mut lamp, mut clock) = folded(again + Duration::from_secs(10));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let time = clock.time.clone();
    let watched = clock.time.clone();
    let mut seen: Vec<(Duration, Option<&'static str>)> = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        Limits::folded_with(Tuning {
            reach: [150, 15, 1, 250],
            speed: 120,
            servo_speed: 1_000,
            servo_acceleration: 60,
        })
        .unwrap(),
        Some(HOME),
        Some(standing(100, 3_600).idling(true).seeded(5)),
        None,
        move || {
            let at = time.get();
            Some(if at < Duration::from_secs(2) || at >= again {
                (Mood::Listening, 100)
            } else {
                (Mood::Resting, 0)
            })
        },
        Asked::default,
        |glimpse| seen.push((watched.get(), glimpse.gesture)),
    );
    assert_eq!(report.idles, 1);
    let last = seen.iter().rfind(|step| step.1.is_some()).unwrap().0;
    assert!(last >= from && last < again + TICK * 5, "{last:?}");
}

#[test]
fn it_stands_up_for_somebody_in_front_of_it_and_stays_up_while_they_are_there() {
    // Nobody speaks. A face is in front of the lamp from two seconds to
    // twenty, seen six times a second, and then gone.
    let company = |from: u64, until: u64| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(34));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        let time = clock.time.clone();
        let watched = clock.time.clone();
        let mut last = Duration::ZERO;
        let mut stood: Vec<(Duration, f32)> = Vec::new();
        let report = run_alive(
            &mut lamp,
            &mut clock,
            Limits::FOLDED,
            Some(HOME),
            Some(standing(100, 5)),
            Some(Looking::new([342, 200], [250, 120]).unwrap()),
            || Some((Mood::Resting, 0)),
            move || {
                let now = time.get();
                if now.saturating_sub(last) < Duration::from_millis(150) {
                    return Asked::default();
                }
                last = now;
                let there = (from..until).contains(&now.as_secs());
                Asked {
                    toward: None,
                    seen: Some(Seen {
                        face: there.then_some(crate::gaze::Sight {
                            x: 0.0,
                            y: 0.0,
                            size: 0.2,
                            turned: 0.0,
                        }),
                    }),
                    gesture: None,
                    expression: None,
                    called: false,
                    still: false,
                    paused: false,
                    unplug: false,
                    held_by_test: false,
                    held: false,
                    asleep: false,
                    woken: false,
                }
            },
            |glimpse| stood.push((watched.get(), glimpse.stand)),
        );
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        stood
    };
    let stood = company(2, 20);
    let at = |seconds: u64| {
        stood
            .iter()
            .rfind(|step| step.0 <= Duration::from_secs(seconds))
            .unwrap()
            .1
    };
    // Folded until a second after the face appeared, standing by eight
    // seconds, still standing while it is there, and through the five
    // seconds it stays awake after; folded again after that.
    assert_eq!(at(2), 0.0);
    assert_eq!(at(9), 1.0);
    assert_eq!(at(19), 1.0);
    assert_eq!(at(24), 1.0);
    assert_eq!(at(33), 0.0);
    // A face for half a second, somebody walking past, wakes nothing.
    let passed = company(2, 2);
    assert!(passed.iter().all(|step| step.1 == 0.0));
}

/// Paused while somebody is still in front of it, a standing lamp sees
/// nobody from then on and folds once its awake time runs out.
#[test]
fn paused_with_somebody_still_in_front_of_it_a_standing_lamp_folds() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(34));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let time = clock.time.clone();
    let watched = clock.time.clone();
    let mut last = Duration::ZERO;
    let mut stood: Vec<(Duration, f32)> = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 5)),
        Some(Looking::new([342, 200], [250, 120]).unwrap()),
        || Some((Mood::Resting, 0)),
        move || {
            let now = time.get();
            if now.saturating_sub(last) < Duration::from_millis(150) {
                return Asked::default();
            }
            last = now;
            // A face from two seconds on, all the way; paused at twelve.
            if now >= Duration::from_secs(12) {
                return Asked::PAUSED;
            }
            Asked {
                toward: None,
                seen: Some(Seen {
                    face: (now >= Duration::from_secs(2)).then_some(crate::gaze::Sight {
                        x: 0.0,
                        y: 0.0,
                        size: 0.2,
                        turned: 0.0,
                    }),
                }),
                gesture: None,
                expression: None,
                called: false,
                still: false,
                paused: false,
                unplug: false,
                held_by_test: false,
                held: false,
                asleep: false,
                woken: false,
            }
        },
        |glimpse| stood.push((watched.get(), glimpse.stand)),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let at = |seconds: u64| {
        stood
            .iter()
            .rfind(|step| step.0 <= Duration::from_secs(seconds))
            .unwrap()
            .1
    };
    assert_eq!(at(11), 1.0);
    assert_eq!(at(33), 0.0);
}

/// Put to sleep by a hand on its head (#1362) with somebody in front of it
/// and a long awake time, a standing lamp folds at once, not when its awake
/// time runs out: the slow fold is the sleep a person sees. Asked to listen
/// once it is awake again, it stands back up: the lift.
#[test]
fn put_to_sleep_by_the_head_with_somebody_in_front_of_it_a_standing_lamp_folds_now() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(44));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let time = clock.time.clone();
    let moods = clock.time.clone();
    let watched = clock.time.clone();
    let mut last = Duration::ZERO;
    let mut stood: Vec<(Duration, f32)> = Vec::new();
    let asleep = Duration::from_secs(12)..Duration::from_secs(30);
    let sleeping = asleep.clone();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        // Awake for 90 s after the last mood or face, as the product.
        Some(standing(100, 90)),
        Some(Looking::new([342, 200], [250, 120]).unwrap()),
        // Listening once woken, as the voice is after the tap.
        move || {
            let now = moods.get();
            Some(
                if now >= sleeping.end && now < sleeping.end + Duration::from_secs(2) {
                    (Mood::Listening, 0)
                } else {
                    (Mood::Resting, 0)
                },
            )
        },
        move || {
            let now = time.get();
            if now.saturating_sub(last) < Duration::from_millis(150) {
                return Asked::default();
            }
            last = now;
            // A face from two seconds on, all the way; asleep from twelve
            // to thirty.
            if asleep.contains(&now) {
                return Asked::ASLEEP;
            }
            Asked {
                toward: None,
                seen: Some(Seen {
                    face: (now >= Duration::from_secs(2)).then_some(crate::gaze::Sight {
                        x: 0.0,
                        y: 0.0,
                        size: 0.2,
                        turned: 0.0,
                    }),
                }),
                gesture: None,
                expression: None,
                called: false,
                still: false,
                paused: false,
                asleep: false,
                woken: false,
                unplug: false,
                held_by_test: false,
                held: false,
            }
        },
        |glimpse| stood.push((watched.get(), glimpse.stand)),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let at = |seconds: u64| {
        stood
            .iter()
            .rfind(|step| step.0 <= Duration::from_secs(seconds))
            .unwrap()
            .1
    };
    assert_eq!(at(11), 1.0);
    // Folding from the hold on, and down within its four seconds and a
    // little settling, with the face still there.
    assert!(at(13) < 1.0, "{}", at(13));
    assert_eq!(at(20), 0.0);
    assert_eq!(at(29), 0.0);
    // Woken and spoken to: up again.
    assert_eq!(at(38), 1.0);
}

/// Told to take hold: what would refuse it for want of torque is given
/// torque, each joint's goal first set to where it is. Without the switch
/// such a lamp is refused as before (see the test of a lamp that is not as
/// expected) and nothing is written.
#[test]
fn a_joint_found_without_torque_is_given_it_goal_first_and_does_not_move() {
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(
        hex(&goal_write(Joint::WristPitch, 2_000)),
        "ffff0505032ad007f1"
    );
    assert_eq!(hex(&torque_write(Joint::WristPitch, 1)), "ffff0504032801ca");
    let holding = Limits {
        torque_on: true,
        ..Limits::FOLDED
    };
    let (mut lamp, mut clock) = folded(Duration::from_secs(1));
    lamp.may_take_hold = true;
    // As on lamp-52e6 after its old runtime stopped: torque off, and each
    // goal some way from where the joint is.
    for (index, stale) in [(PITCH, 2_025), (ROLL, 2_014), (YAW, 2_033)] {
        lamp.servos[index].torque = 0;
        lamp.servos[index].goal = stale;
    }
    let found = lamp.servos.map(|servo| servo.position);
    let report = run(
        &mut lamp,
        &mut clock,
        holding,
        None,
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.torque_writes, 3);
    assert_eq!(
        report.torque_given,
        vec!["wrist_pitch", "wrist_roll", "base_yaw"]
    );
    // Goal, then torque, for each in turn, before anything else.
    let goal = |value: u16| value.to_le_bytes().to_vec();
    assert_eq!(
        lamp.registers[..6],
        [
            (5, 42, goal(2_000)),
            (5, 40, vec![1]),
            (4, 42, goal(2_044)),
            (4, 40, vec![1]),
            (1, 42, goal(2_015)),
            (1, 40, vec![1]),
        ]
    );
    assert!(lamp.servos.iter().all(|servo| servo.torque == 1));
    assert_eq!(lamp.servos.map(|servo| servo.position), found);
}

/// A servo just given power that does not answer its first writes is
/// written again, a little later, and the lamp is taken hold of as if it
/// had answered at once; one that never answers still ends the run.
#[test]
fn a_joint_that_does_not_answer_its_first_writes_is_taken_hold_of_a_little_later() {
    let holding = Limits {
        torque_on: true,
        ..Limits::FOLDED
    };
    let ran = |deaf: u8| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(6));
        lamp.may_take_hold = true;
        lamp.servos[ROLL].torque = 0;
        lamp.servos[ROLL].goal = 2_074;
        lamp.deaf = Some((ROLL, deaf));
        let report = run(
            &mut lamp,
            &mut clock,
            holding,
            None,
            || Some((Mood::Resting, 0)),
            |_| {},
        );
        (report, lamp)
    };
    let (report, lamp) = ran(3);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.torque_given, vec!["wrist_roll"]);
    let goal = |value: u16| value.to_le_bytes().to_vec();
    assert_eq!(
        lamp.registers[..2],
        [(4, 42, goal(2_044)), (4, 40, vec![1])]
    );
    assert_eq!(lamp.servos[ROLL].torque, 1);
    let (report, _) = ran(u8::MAX);
    assert_eq!(
        report.stop,
        Stop::Bus {
            detail: "taking hold: wrist_roll did not acknowledge a write".into()
        },
        "{report:?}"
    );
}

/// A goal that was not taken shows as a joint that moves when its torque
/// comes on: the run ends there, naming it, with its goal where it went.
#[test]
fn a_joint_that_moves_when_its_torque_comes_on_ends_the_run() {
    let holding = Limits {
        torque_on: true,
        ..Limits::FOLDED
    };
    let (mut lamp, mut clock) = folded(Duration::from_secs(2));
    lamp.may_take_hold = true;
    lamp.servos[ROLL].torque = 0;
    lamp.servos[ROLL].goal = 2_074;
    lamp.drops_goal = Some(ROLL);
    let report = run(
        &mut lamp,
        &mut clock,
        holding,
        None,
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(
        report.stop,
        Stop::Refused {
            why: "wrist_roll moved 30 counts when its torque came on".into()
        }
    );
    assert!(!report.moved && lamp.goals.is_empty());
    // Goal, torque, and the goal that stops it pushing; nothing else.
    let goal = |value: u16| value.to_le_bytes().to_vec();
    assert_eq!(
        lamp.registers,
        [(4, 42, goal(2_044)), (4, 40, vec![1]), (4, 42, goal(2_074))]
    );
    assert_eq!(lamp.servos[ROLL].goal, 2_074);
}

/// The shoulder and the elbow are taken hold of only on a lamp that may
/// stand; on any other they are never written.
#[test]
fn the_arm_is_taken_hold_of_only_on_a_lamp_that_may_stand() {
    let holding = Limits {
        torque_on: true,
        ..Limits::FOLDED
    };
    let without_torque = || {
        let (mut lamp, clock) = folded(Duration::from_secs(1));
        lamp.may_take_hold = true;
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        lamp.servos[1].torque = 0;
        lamp.servos[1].goal = 2_070;
        lamp.servos[2].torque = 0;
        lamp.servos[2].goal = 2_022;
        (lamp, clock)
    };

    let (mut lamp, mut clock) = without_torque();
    let report = run_standing(
        &mut lamp,
        &mut clock,
        holding,
        Some(HOME),
        Some(standing(100, 5)),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.standing.is_some(), "{report:?}");
    assert_eq!(report.torque_given, vec!["base_pitch", "elbow_pitch"]);
    let goal = |value: u16| value.to_le_bytes().to_vec();
    assert_eq!(
        lamp.registers[..4],
        [
            (2, 42, goal(2_050)),
            (2, 40, vec![1]),
            (3, 42, goal(2_047)),
            (3, 40, vec![1]),
        ]
    );
    assert_eq!(
        (lamp.servos[1].position, lamp.servos[2].position),
        (2_050.0, 2_047.0)
    );

    let (mut lamp, mut clock) = without_torque();
    let report = run(
        &mut lamp,
        &mut clock,
        holding,
        None,
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.torque_writes, 0);
    assert!(lamp.registers.iter().all(|(id, _, _)| *id != 2 && *id != 3));
}

/// The folded limits with waking on.
fn waking() -> Limits {
    Limits {
        wake: true,
        ..Limits::FOLDED
    }
}

/// The goals a joint was written, every write in order.
fn goals_of(lamp: &Lamp, joint: Joint) -> Vec<u16> {
    lamp.goals
        .iter()
        .map(|write| goal_of(write, joint))
        .collect()
}

#[test]
fn a_head_found_pressed_on_the_base_rises_out_of_it_slowly_and_then_runs() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(12));
    lamp.servos[PITCH].position = 2_095.0;
    lamp.servos[PITCH].goal = 2_095;
    let mut first = None;
    let report = run(
        &mut lamp,
        &mut clock,
        waking(),
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |glimpse| {
            first.get_or_insert(glimpse.positions);
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let woke = report.woke.unwrap();
    assert_eq!(woke.joints, vec!["wrist_pitch"]);
    assert_eq!((woke.from, woke.to), (vec![2_095], vec![2_000]));
    // A stir of 24 counts, a second's pause and the rest of the 95, each
    // eased at 20 counts a second at most: a little over eight seconds.
    assert!((8.0..9.0).contains(&woke.seconds), "{}", woke.seconds);
    // It ran once it was up, inside its window.
    let first = first.unwrap();
    assert!((1_760..=2_020).contains(&first[0]), "{first:?}");
    // Never toward the base, and never more than a count a write.
    let pitch = goals_of(&lamp, Joint::WristPitch);
    assert_eq!(pitch[0], 2_094);
    let risen = pitch.iter().position(|goal| *goal == 2_000).unwrap();
    assert!(
        pitch[..=risen]
            .windows(2)
            .all(|two| two[1] <= two[0] && two[0] - two[1] <= 1)
    );
    assert!(
        pitch[risen..].iter().all(|goal| *goal <= 2_030),
        "{pitch:?}"
    );

    // Without waking, the same lamp is refused as it always was.
    let (mut lamp, mut clock) = folded(Duration::from_secs(2));
    lamp.servos[PITCH].position = 2_095.0;
    let report = run(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        || None,
        |_| {},
    );
    assert_eq!(
        report.stop,
        Stop::Refused {
            why: "head pitch 2095 is outside 1760..=2020".into()
        }
    );
    assert!(lamp.nothing_was_written() && report.woke.is_none());

    // Waking needs a home it was given: without one, where it is found is home.
    let (mut lamp, mut clock) = folded(Duration::from_secs(2));
    lamp.servos[PITCH].position = 2_095.0;
    let report = run(&mut lamp, &mut clock, waking(), None, || None, |_| {});
    assert!(matches!(report.stop, Stop::Refused { .. }), "{report:?}");
    assert!(lamp.nothing_was_written());
}

/// lamp-0c89 (#1682): rested at pitch 1755, under its window, held with
/// torque on, its arm where its person confirmed it folded (2057, 2056).
fn held_at_1755(arm: [u16; 2], folded_arm: Option<[u16; 2]>) -> (Lamp, BodyReport) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(2));
    lamp.servos[PITCH].position = 1_755.0;
    lamp.servos[PITCH].goal = 1_755;
    for (index, counts) in [1, 2].into_iter().zip(arm) {
        lamp.servos[index].position = f32::from(counts);
        lamp.servos[index].goal = counts;
    }
    let let_go = Limits {
        let_go_at_stop: true,
        let_go_folded_arm: folded_arm,
        ..Limits::FOLDED
    };
    let report = run(&mut lamp, &mut clock, let_go, None, || None, |_| {});
    (lamp, report)
}

/// #2396, a seated lamp (no --stand) resting on its stop at 1755 with the
/// measured --home 1760: woken (lamp-body's default), its head is brought
/// home and it runs; with --wake off its start refuses the live 1755.
#[test]
fn a_seated_lamp_at_1755_with_home_1760_wakes_home_and_without_waking_is_refused() {
    let at_1755 = |limits: Limits| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(20));
        lamp.servos[PITCH].position = 1_755.0;
        lamp.servos[PITCH].goal = 1_755;
        let home = [1_760, HOME[1], HOME[2]];
        run(&mut lamp, &mut clock, limits, Some(home), || None, |_| {})
    };
    let woken = at_1755(waking());
    assert_eq!(woken.stop, Stop::Cancelled, "{woken:?}");
    assert_eq!(woken.woke.unwrap().to[0], 1_760);
    assert_eq!(
        at_1755(Limits::FOLDED).stop,
        Stop::Refused {
            why: "head pitch 1755 is outside 1760..=2020".into()
        }
    );
}

/// A let-go run outside the window, its arm at the folded counts given,
/// is let go: torque off on all five, no goal, until asked to stop.
#[test]
fn a_let_go_run_outside_the_window_with_its_arm_folded_writes_torque_off_only() {
    let (lamp, report) = held_at_1755([2_057, 2_056], Some([2_057, 2_056]));
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.let_go_at_stop && report.let_go_outside_window.is_some());
    assert!(lamp.goals.is_empty() && report.goal_writes == 0);
    let off: Vec<_> = (1..=5).map(|id| (id, 40, vec![0])).collect();
    assert_eq!(lamp.registers, off);
    assert!(lamp.servos.iter().all(|servo| servo.torque == 0));
}

/// The same with its arm standing, or with no folded counts given, is
/// refused as before, nothing written: nothing standing falls.
#[test]
fn a_let_go_run_outside_the_window_standing_or_without_folded_counts_is_refused() {
    for (arm, folded_arm) in [
        ([2_657, 2_602], Some([2_057, 2_056])),
        ([2_057, 2_056], None),
    ] {
        let (lamp, report) = held_at_1755(arm, folded_arm);
        assert_eq!(
            report.stop,
            Stop::Refused {
                why: "head pitch 1755 is outside 1760..=2020".into()
            },
            "{arm:?} {folded_arm:?}"
        );
        assert!(lamp.nothing_was_written() && report.let_go_outside_window.is_none());
    }
}

/// Every other run outside the window is refused, nothing written: one
/// with a home, or with --torque on, though it lets go at the stop.
#[test]
fn a_run_that_is_not_only_a_let_go_outside_the_window_is_still_refused() {
    for (home, torque_on) in [(Some(HOME), false), (None, true)] {
        let (mut lamp, mut clock) = folded(Duration::from_secs(2));
        lamp.servos[PITCH].position = 1_755.0;
        lamp.servos[PITCH].goal = 1_755;
        let limits = Limits {
            let_go_at_stop: true,
            torque_on,
            ..Limits::FOLDED
        };
        let report = run(&mut lamp, &mut clock, limits, home, || None, |_| {});
        assert_eq!(
            report.stop,
            Stop::Refused {
                why: "head pitch 1755 is outside 1760..=2020".into()
            }
        );
        assert!(lamp.nothing_was_written() && report.let_go_outside_window.is_none());
    }
}

#[test]
fn a_base_turned_300_counts_comes_home_without_moving_away_or_faster_than_waking() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(28));
    lamp.servos[YAW].position = 2_315.0;
    lamp.servos[YAW].goal = 2_315;
    let report = run(
        &mut lamp,
        &mut clock,
        waking(),
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let woke = report.woke.unwrap();
    assert_eq!(woke.joints, vec!["base_yaw"]);
    // 1.8 s of stir, a second, then 276 counts eased at 20 a second at most.
    assert!((23.0..24.5).contains(&woke.seconds), "{}", woke.seconds);
    let yaw = goals_of(&lamp, Joint::BaseYaw);
    assert!(
        yaw.windows(2)
            .all(|two| two[1] <= two[0] && two[0] - two[1] <= 1)
    );
    assert!(yaw.iter().all(|goal| (2_015..2_315).contains(goal)));
    assert_eq!(lamp.servos[YAW].goal, 2_015);
    // The others were never asked to move.
    assert!(
        goals_of(&lamp, Joint::WristRoll)
            .iter()
            .all(|goal| *goal == 2_044)
    );

    // The way itself, step by step: never away from home and never over
    // the wake's speed.
    let (from, to) = ([2_000, 2_044, 2_315], [2_000, 2_044, 2_015]);
    let mut was = wake_pose(from, to, 0.0).0;
    assert_eq!(was, [2_000.0, 2_044.0, 2_315.0]);
    let mut elapsed = 0.0f32;
    loop {
        elapsed += TICK.as_secs_f32();
        let (now, over) = wake_pose(from, to, elapsed);
        let moved = was[2] - now[2];
        assert!((0.0..=WAKE_SPEED * TICK.as_secs_f32() + 0.001).contains(&moved));
        assert_eq!(&now[..2], &[2_000.0, 2_044.0]);
        was = now;
        if over {
            break;
        }
    }
    assert_eq!(was[2], 2_015.0);
}

#[test]
fn a_head_slumped_on_the_base_lifts_before_the_base_turns() {
    // As lamp-52e6 on 2026-10-16: the head on its own base and the base
    // turned, both to wake. The base must not drag the head across it.
    let (mut lamp, mut clock) = folded(Duration::from_secs(40));
    lamp.servos[PITCH].position = 2_095.0;
    lamp.servos[PITCH].goal = 2_095;
    lamp.servos[YAW].position = 2_115.0;
    lamp.servos[YAW].goal = 2_115;
    let report = run(
        &mut lamp,
        &mut clock,
        waking(),
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let woke = report.woke.unwrap();
    assert_eq!(woke.joints, vec!["wrist_pitch", "base_yaw"]);
    let (pitch, yaw) = (
        goals_of(&lamp, Joint::WristPitch),
        goals_of(&lamp, Joint::BaseYaw),
    );
    let lifted = pitch.iter().position(|goal| *goal == 2_000).unwrap();
    let turned = yaw.iter().position(|goal| *goal != 2_115).unwrap();
    assert!(
        lifted < turned,
        "pitch home at {lifted}, yaw moved at {turned}"
    );
    assert_eq!(lamp.servos[YAW].goal, 2_015);
}

#[test]
fn a_joint_beyond_what_waking_brings_back_is_refused() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(2));
    lamp.servos[YAW].position = 2_465.0;
    lamp.servos[YAW].goal = 2_465;
    let report = run(&mut lamp, &mut clock, waking(), Some(HOME), || None, |_| {});
    assert_eq!(
        report.stop,
        Stop::Refused {
            why: "base_yaw is 450 counts from its home, more than waking brings back (400)".into()
        }
    );
    assert!(lamp.nothing_was_written());
}

#[test]
fn an_arm_found_off_its_way_is_not_lowered_and_the_lamp_runs_folded() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(2));
    lamp.servos[1].position = 2_650.0;
    lamp.servos[1].goal = 2_650;
    let report = run_standing(
        &mut lamp,
        &mut clock,
        waking(),
        Some(HOME),
        Some(standing(100, 60)),
        || Some((Mood::Listening, 120)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.standing, None);
    assert!(
        report
            .standing_refused
            .unwrap()
            .ends_with("waking does not lower an arm found off its way")
    );
    // Not a goal to the arm, and nothing to wake.
    assert!(lamp.goals.iter().all(|write| write.len() == 3));
    assert!(report.woke.is_none());
}

#[test]
fn waking_on_a_lamp_found_as_expected_writes_exactly_what_it_did_without() {
    let writes = |limits: Limits| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(8));
        let news = script(&clock.time);
        let report = run(&mut lamp, &mut clock, limits, Some(HOME), news, |_| {});
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        assert!(report.woke.is_none());
        (lamp.goals, lamp.registers)
    };
    assert_eq!(writes(waking()), writes(Limits::FOLDED));
}

/// The folded limits with room to lift the head 100 counts, and a choice of
/// how it gives way.
fn giving_way(give_way: GiveWay) -> Limits {
    Limits {
        give_way: Some(give_way),
        ..Limits::folded_with(Tuning {
            reach: [100, 15, 30, 40],
            speed: 50,
            servo_speed: 150,
            servo_acceleration: 50,
        })
        .unwrap()
    }
}

/// Resting for six seconds, then listening.
fn rest_then_listen(time: &Rc<Cell<Duration>>) -> impl FnMut() -> Option<(Mood, u8)> + use<> {
    let time = time.clone();
    move || {
        Some(if time.get() < Duration::from_secs(6) {
            (Mood::Resting, 0)
        } else {
            (Mood::Listening, 100)
        })
    }
}

/// The time of a step, from its number.
fn clock_of(glimpse: &Glimpse) -> Duration {
    TICK * glimpse.step as u32
}

/// Told to stay, a bump that does not move the head, and a hand that moves
/// it for under a second, each give way and come back as without it.
#[test]
fn told_to_stay_a_short_push_still_gives_way_and_comes_back() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.strains = vec![(ms(1_200), ms(1_400), ROLL, 217)];
    let news = script(&clock.time);
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        Limits {
            give_way: Some(GiveWay::Stay),
            ..Limits::FOLDED
        },
        None,
        news,
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!((report.eased, report.kept_where_put), ([0, 1, 0], [0; 3]));
    let gave = seen.iter().position(|g| g.eased[1] == 1).unwrap();
    let held = seen[gave].goals[1];
    assert!(
        seen[gave + usize::from(EASE_STEPS)..]
            .iter()
            .any(|g| g.goals[1].abs_diff(held) >= 10),
        "it came back"
    );

    // A hand lifts the head 40 counts for half a second, while it listens.
    let (mut lamp, mut clock) = folded(Duration::from_secs(12));
    lamp.hand = Some((ms(7_200), ms(7_700), PITCH, 1_920.0));
    lamp.strains = vec![(ms(7_200), ms(7_700), PITCH, 300)];
    let news = rest_then_listen(&clock.time);
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        giving_way(GiveWay::Stay),
        None,
        news,
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!((report.eased, report.kept_where_put), ([1, 0, 0], [0; 3]));
    // Listening lifts it 40 counts; it is back near there once the hold is
    // over, and not where the hand had it, 40 counts further.
    let back = seen
        .iter()
        .rev()
        .find(|g| clock_of(g) < ms(11_900))
        .unwrap();
    assert!(back.goals[0].abs_diff(1_960) <= 12, "{back:?}");
    assert!(back.positions[0].abs_diff(1_960) <= 12, "{back:?}");
}

/// Told to stay, a hand that moves the head for a second and a half leaves
/// it there, and the next mood is laid from where it was put.
#[test]
fn told_to_stay_a_head_moved_by_hand_stays_and_the_next_mood_is_relative_to_it() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(10));
    // Lifted 30 counts, held there, let go.
    lamp.hand = Some((ms(1_500), ms(3_000), PITCH, 1_970.0));
    lamp.strains = vec![(ms(1_500), ms(3_000), PITCH, 300)];
    let news = rest_then_listen(&clock.time);
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        giving_way(GiveWay::Stay),
        None,
        news,
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.eased, [1, 0, 0]);
    assert_eq!(report.kept_where_put, [1, 0, 0]);
    // Let go, it stays where it was put while it rests.
    let resting: Vec<_> = seen
        .iter()
        .filter(|g| (ms(3_500)..ms(6_000)).contains(&clock_of(g)))
        .collect();
    assert!(
        resting.iter().all(|g| g.goals[0].abs_diff(1_970) <= 1),
        "{:?}",
        resting.iter().map(|g| g.goals[0]).collect::<Vec<_>>()
    );
    // Listening lifts it 40 counts from there, not from home, and on the way
    // no goal jumps.
    let listening = seen
        .iter()
        .rev()
        .find(|g| (ms(8_500)..ms(10_000)).contains(&clock_of(g)))
        .unwrap();
    assert!(listening.goals[0].abs_diff(1_930) <= 2, "{listening:?}");
    let after: Vec<_> = seen.iter().filter(|g| clock_of(g) > ms(3_500)).collect();
    assert!(
        after
            .windows(2)
            .all(|pair| pair[1].goals[0].abs_diff(pair[0].goals[0]) <= 2),
        "it jumped"
    );
    // Asked to stop, it goes home, not to where it was put.
    assert_eq!(lamp.servos[PITCH].goal, HOME[0]);
}

/// A light head: a hand that lifts it 40 counts reads only 120, under the
/// head's load bound of 240, as lamp-52e6's did. Told to stay, it stays
/// where it was put; told to come back, it comes back.
#[test]
fn a_light_head_lifted_by_hand_stays_when_told_to_and_comes_back_when_not() {
    let ms = Duration::from_millis;
    let ran = |give_way: GiveWay| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(8));
        // It reaches 1,920 in under half a second and is held there for
        // a second and a half.
        lamp.hand = Some((ms(1_500), ms(3_500), PITCH, 1_920.0));
        lamp.strains = vec![(ms(1_500), ms(3_500), PITCH, 120)];
        let news = || Some((Mood::Listening, 100));
        let mut seen = Vec::new();
        let report = run(
            &mut lamp,
            &mut clock,
            giving_way(give_way),
            None,
            news,
            |glimpse| seen.push(*glimpse),
        );
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        let after = *seen.iter().rev().find(|g| clock_of(g) < ms(7_900)).unwrap();
        (report, after)
    };
    let (report, after) = ran(GiveWay::Stay);
    assert_eq!(report.kept_where_put, [1, 0, 0], "{report:?}");
    assert!(after.goals[0].abs_diff(1_920) <= 6, "{after:?}");
    // Listening holds it at 1,960; the same hand, told to come back.
    let (report, after) = ran(GiveWay::Return);
    assert_eq!(report.kept_where_put, [0; 3], "{report:?}");
    assert!(after.goals[0].abs_diff(1_960) <= 6, "{after:?}");
}

/// lamp-52e6, 15 Oct, run 4: a hand lifts the head 60 counts; it gives way
/// as the push begins (that read 192 there), and the hand then holds it for
/// three seconds at a load of only 95, under every load bound, the head kept
/// 30 to 40 counts off the goal it is held at. Told to stay it stays
/// where it was put; told to come back it comes back; without the option
/// the same hand ends the run, as before.
#[test]
fn a_head_held_off_its_goal_by_a_hand_at_a_low_load_is_in_the_hand() {
    let ms = Duration::from_millis;
    let ran = |give_way: Option<GiveWay>| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(10));
        lamp.hand = Some((ms(1_500), ms(4_500), PITCH, 1_900.0));
        lamp.strains = vec![
            (ms(1_500), ms(2_100), PITCH, 150),
            (ms(2_100), ms(4_500), PITCH, 95),
        ];
        let news = || Some((Mood::Listening, 100));
        let mut seen = Vec::new();
        let limits = Limits {
            give_way,
            ..giving_way(GiveWay::Return)
        };
        let report = run(&mut lamp, &mut clock, limits, None, news, |glimpse| {
            seen.push(*glimpse)
        });
        let after = *seen.iter().rev().find(|g| clock_of(g) < ms(9_900)).unwrap();
        (report, after)
    };
    let (report, after) = ran(Some(GiveWay::Stay));
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.kept_where_put[0] >= 1, "{report:?}");
    assert!(after.goals[0].abs_diff(1_900) <= 8, "{after:?}");
    // Listening holds it at 1,960.
    let (report, after) = ran(Some(GiveWay::Return));
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.kept_where_put, [0; 3], "{report:?}");
    assert!(after.goals[0].abs_diff(1_960) <= 6, "{after:?}");
    let (report, _) = ran(None);
    assert!(
        matches!(report.stop, Stop::Tracking { joint: 5, .. }),
        "{report:?}"
    );
}

/// #3925: a joint turning toward a goal that moved faster than it turns is
/// following, whether the gap shrinks or, while the goal ramps away, grows;
/// held still (its reading jittering), or turned away by a hand, it is not.
/// Step by step, as the loop calls it: (goal, position, steps off track).
#[test]
fn a_joint_turning_toward_its_goal_is_following_not_pushed() {
    let walk = |steps: &[(u16, u16, u8)]| {
        let (mut recent, mut seen) = ([0; FOLLOW_STEPS], 0);
        steps
            .iter()
            .map(|&(goal, position, off)| note_turn(&mut recent, &mut seen, goal, position, off))
            .collect::<Vec<_>>()
    };
    // lamp-52e6, 27 Oct, 01:26:22: the base's goal jumped from 2,025 to
    // 2,061, the base closing on it.
    let closing = walk(&[
        (2_025, 2_025, 0),
        (2_061, 2_025, 1),
        (2_061, 2_028, 2),
        (2_061, 2_031, 3),
        (2_061, 2_034, 4),
        (2_061, 2_037, 5),
        (2_061, 2_040, 6),
        (2_061, 2_043, 7),
    ]);
    assert!(closing.iter().all(|f| *f), "{closing:?}");
    // lamp-52e6, 28 Oct (8001): a goal ramping away faster than the base
    // gathers speed; the gap grows, the base turns toward it all the same.
    let ramping = walk(&[
        (1_991, 1_991, 0),
        (1_955, 1_989, 1),
        (1_920, 1_986, 2),
        (1_885, 1_982, 3),
        (1_850, 1_976, 4),
        (1_815, 1_968, 5),
        (1_780, 1_958, 6),
        (1_745, 1_946, 7),
    ]);
    assert!(ramping.iter().all(|f| *f), "{ramping:?}");
    // Caught partway and held, its reading jittering two counts back and
    // forth (the Reviewer-lead's P2): not following once the window holds
    // only the hold.
    let mut held = vec![(2_215, 2_015, 0), (2_215, 2_040, 1), (2_215, 2_070, 2)];
    for step in 0..10u8 {
        held.push((2_215, 2_095 + 2 * u16::from(step % 2), 3 + step));
    }
    let held = walk(&held);
    assert!(!held[3 + FOLLOW_STEPS + 1], "{held:?}");
    assert!(!held[held.len() - 1], "{held:?}");
    // Turned away from its goal by a hand: not following.
    let away = walk(&[
        (2_015, 2_015, 0),
        (2_015, 2_030, 1),
        (2_015, 2_045, 2),
        (2_015, 2_060, 3),
        (2_015, 2_075, 4),
        (2_015, 2_090, 5),
        (2_015, 2_105, 6),
        (2_015, 2_120, 7),
    ]);
    assert!(away[2..].iter().all(|f| !*f), "{away:?}");
}

/// lamp-06c1, 23 Oct: a head pitch that reads 124 to 136 at its own rest,
/// over `STALL_LOAD`, with nothing on it and its position unmoving all
/// night. Given way to a short push, it is let go after its ease and is
/// asked its moods again, not held for good.
#[test]
fn a_head_resting_still_at_its_own_load_over_100_is_let_go_after_its_ease() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(12));
    lamp.strains = vec![
        (ms(1_500), ms(2_000), PITCH, 300),
        (ms(0), ms(12_000), PITCH, 130),
    ];
    let news = rest_then_listen(&clock.time);
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        giving_way(GiveWay::Return),
        None,
        news,
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.eased, [1, 0, 0], "{report:?}");
    // Its ease of two seconds from the push's end, and a second more.
    let late: Vec<_> = seen.iter().filter(|g| clock_of(g) >= ms(5_000)).collect();
    assert!(
        late.iter().all(|g| !g.hand),
        "still held at {:?}",
        late.iter().rev().find(|g| g.hand).map(|g| clock_of(g))
    );
    // Listening, from six seconds, lifts it 40 counts.
    let listening = seen
        .iter()
        .rev()
        .find(|g| clock_of(g) < ms(11_900))
        .unwrap();
    assert!(listening.goals[0].abs_diff(1_960) <= 2, "{listening:?}");
}

/// lamp-06c1, 22 Oct: the head pitch never moved (1,765 in every sample)
/// while its goal wandered away from it, and its load rose with the gap (96
/// to 112, once 192, all under its bound of 240). It gave way as a joint
/// held off its goal does, where its goal was, not where it stood. Still
/// where it stood, it is let go after its ease and asked its moods again.
#[test]
fn a_head_that_never_moves_while_its_goal_wanders_off_is_let_go_after_its_ease() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(12));
    // The servo stays at 1,995 whatever its goal; listening asks 1,960.
    lamp.hand = Some((ms(0), ms(12_000), PITCH, 1_995.0));
    lamp.strains = vec![
        (ms(2_000), ms(2_100), PITCH, 192),
        (ms(0), ms(12_000), PITCH, 130),
    ];
    let news = || Some((Mood::Listening, 100));
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        giving_way(GiveWay::Return),
        None,
        news,
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.eased[0] >= 1, "{report:?}");
    let first = seen.iter().find(|g| g.hand).map(clock_of).unwrap();
    // Let go within its ease and a second of giving way, and asked toward
    // listening's 1,960 again, not left at where it stands.
    let let_go = seen
        .iter()
        .filter(|g| clock_of(g) > first)
        .find(|g| !g.hand)
        .map(clock_of);
    assert!(
        let_go.is_some_and(|at| at <= first + ms(3_000)),
        "gave way at {first:?}, let go at {let_go:?}"
    );
    assert!(
        seen.iter()
            .filter(|g| clock_of(g) > let_go.unwrap())
            .any(|g| g.goals[0] + 15 < 1_995),
        "never asked its moods again"
    );
}

/// The same head, pressed past its bound of 240 for a tenth of a second
/// while it rests at 130: that brief press is no hand left on it, and the
/// distance from the goal it first gave at is not pressure. Still let go
/// within its ease (reviewer-two, #3308).
#[test]
fn a_head_that_never_moves_pressed_briefly_past_its_bound_is_let_go_after_its_ease() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(12));
    // The servo stays at 1,995 whatever its goal; listening asks 1,960.
    lamp.hand = Some((ms(0), ms(12_000), PITCH, 1_995.0));
    lamp.strains = vec![
        (ms(2_000), ms(2_100), PITCH, 192),
        (ms(2_300), ms(2_400), PITCH, 300),
        (ms(0), ms(12_000), PITCH, 130),
    ];
    let news = || Some((Mood::Listening, 100));
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        giving_way(GiveWay::Return),
        None,
        news,
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.eased[0] >= 1, "{report:?}");
    let first = seen.iter().find(|g| g.hand).map(clock_of).unwrap();
    // Let go within its ease and a second of giving way, and asked toward
    // listening's 1,960 again, not left at where it stands.
    let let_go = seen
        .iter()
        .filter(|g| clock_of(g) > first)
        .find(|g| !g.hand)
        .map(clock_of);
    assert!(
        let_go.is_some_and(|at| at <= first + ms(3_000)),
        "gave way at {first:?}, let go at {let_go:?}"
    );
    assert!(
        seen.iter()
            .filter(|g| clock_of(g) > let_go.unwrap())
            .any(|g| g.goals[0] + 15 < 1_995),
        "never asked its moods again"
    );
}

/// What keeps a joint held over its own resting load: a hand that moved it
/// 40 counts and holds it there at 120, or one pressing past the head's
/// bound of 240 without moving it. Either keeps it held for as long as it
/// stays.
#[test]
fn a_head_moved_and_held_or_pressed_past_its_bound_stays_held_while_the_hand_does() {
    let ms = Duration::from_millis;
    let ran = |moved: bool| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(12));
        if moved {
            lamp.hand = Some((ms(1_500), ms(10_000), PITCH, f32::from(HOME[0]) + 40.0));
            lamp.strains = vec![(ms(1_500), ms(10_000), PITCH, 120)];
        } else {
            lamp.strains = vec![(ms(1_500), ms(10_000), PITCH, 300)];
        }
        let news = || Some((Mood::Listening, 100));
        let mut seen = Vec::new();
        let report = run(
            &mut lamp,
            &mut clock,
            giving_way(GiveWay::Return),
            None,
            news,
            |glimpse| seen.push(*glimpse),
        );
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        assert!(report.eased[0] >= 1, "{report:?}");
        let let_go = seen
            .iter()
            .filter(|g| (ms(3_000)..ms(10_000)).contains(&clock_of(g)))
            .find(|g| !g.hand)
            .map(clock_of);
        assert_eq!(let_go, None, "moved {moved}");
    };
    ran(true);
    ran(false);
}

/// A hand that takes the head past its reach leaves it at the edge of it.
#[test]
fn told_to_stay_a_head_pushed_past_its_reach_is_kept_at_the_edge() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    // 150 counts up where the reach is 100.
    lamp.hand = Some((ms(1_500), ms(3_000), PITCH, 1_850.0));
    lamp.strains = vec![(ms(1_500), ms(3_000), PITCH, 300)];
    let news = rest_then_listen(&clock.time);
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        giving_way(GiveWay::Stay),
        None,
        news,
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.kept_where_put[0], 1, "{report:?}");
    assert!(
        lamp.goals
            .iter()
            .all(|write| goal_of(write, Joint::WristPitch) >= 1_900)
    );
    let rested = seen.iter().find(|g| clock_of(g) >= ms(5_500)).unwrap();
    assert_eq!(rested.goals[0], 1_900, "{rested:?}");
    assert!(rested.positions[0].abs_diff(1_900) <= 1, "{rested:?}");
}

/// Told to come back, a hand that tilts the head and holds it there for
/// three seconds, past where it first gave way, does not end the run: it
/// gives way again where the hand has it, and comes back once let go. As on
/// lamp-52e6 on 2026-10-15, where the run ended on tracking 45 counts on.
#[test]
fn told_to_return_a_head_tilted_and_held_by_hand_gives_way_again_and_comes_back() {
    let ms = Duration::from_millis;
    let ran = |give_way: Option<GiveWay>| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(10));
        lamp.hand = Some((ms(1_500), ms(4_500), ROLL, f32::from(HOME[1]) + 60.0));
        lamp.strains = vec![(ms(1_500), ms(4_500), ROLL, 240)];
        // Awake: a lamp at rest lies where it is whatever the option.
        let news = || Some((Mood::Listening, 100));
        let mut seen = Vec::new();
        let limits = Limits {
            give_way,
            ..giving_way(GiveWay::Return)
        };
        let report = run(&mut lamp, &mut clock, limits, Some(HOME), news, |glimpse| {
            seen.push(*glimpse)
        });
        (report, seen)
    };
    let (report, seen) = ran(Some(GiveWay::Return));
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.eased[1] >= 1, "{report:?}");
    assert_eq!(report.kept_where_put, [0; 3]);
    // Let go, it is back where it was before the hand, within what counts
    // as arrived (it gave way a few counts into the push), and not where
    // the hand had it, 35 counts on.
    let before = seen.iter().rev().find(|g| clock_of(g) < ms(1_500)).unwrap();
    let back = seen.iter().rev().find(|g| clock_of(g) < ms(9_900)).unwrap();
    assert!(back.goals[1].abs_diff(before.goals[1]) <= 6, "{back:?}");
    assert!(
        back.positions[1].abs_diff(before.positions[1]) <= 6,
        "{back:?}"
    );
    // Without the option it goes as before: the same hand ends the run.
    let (report, _) = ran(None);
    assert_ne!(report.stop, Stop::Cancelled, "{report:?}");
}

/// While a joint is held where a hand made it give way, each step says so
/// (`body/hand`); before the hand and once it is back, never.
#[test]
fn a_step_says_a_hand_while_a_joint_is_held_where_it_gave_way_and_never_without_one() {
    let ms = Duration::from_millis;
    let ran = |hand: bool| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(10));
        if hand {
            lamp.hand = Some((ms(1_500), ms(4_500), YAW, f32::from(HOME[2]) + 40.0));
            lamp.strains = vec![(ms(1_500), ms(4_500), YAW, 120)];
        }
        let news = || Some((Mood::Listening, 100));
        let mut seen = Vec::new();
        run(
            &mut lamp,
            &mut clock,
            giving_way(GiveWay::Return),
            Some(HOME),
            news,
            |glimpse| seen.push(*glimpse),
        );
        seen
    };
    let seen = ran(true);
    assert!(
        seen.iter()
            .filter(|g| clock_of(g) < ms(1_500))
            .all(|g| !g.hand)
    );
    assert!(
        seen.iter()
            .any(|g| g.hand && (ms(1_500)..ms(4_500)).contains(&clock_of(g)))
    );
    assert!(
        seen.iter()
            .filter(|g| clock_of(g) > ms(8_000))
            .all(|g| !g.hand)
    );
    assert!(ran(false).iter().all(|g| !g.hand));
}

/// lamp-06c1's head on 2026-10-23: resting, nobody at the lamp, it pushed
/// at its own rest for a moment (a load over its bound, without moving) and
/// gave way; held where it gave, it went on reading its own weight (124 to
/// 136, over the 100 a hand under its bound reads) and jittered a few counts,
/// and every step said a hand (`body/hand pushed`, 969 in 120 s). A joint
/// resting inside its band, whatever its rest and its resting load, never
/// says a hand. A real one, moving it or pressing past its bound, still does.
#[test]
fn a_resting_joint_jittering_inside_its_band_never_says_a_hand() {
    let ms = Duration::from_millis;
    // Pitch resting at `rest` counts from its default place, reading `load`
    // at rest; `push` is what else is done to it from 1 s to 3 s.
    let ran = |rest: f32, load: u16, push: Option<(f32, u16)>| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(12));
        let at = lamp.servos[PITCH].position + rest;
        lamp.servos[PITCH].position = at;
        lamp.servos[PITCH].goal = at as u16;
        lamp.strains = match push {
            // Its own push at its rest, 300 ms, then its weight.
            None => vec![
                (ms(0), ms(1_000), PITCH, load),
                (ms(1_000), ms(1_300), PITCH, 300),
                (ms(1_300), ms(12_000), PITCH, load),
            ],
            Some((_, pressed)) => vec![
                (ms(0), ms(1_000), PITCH, load),
                (ms(1_000), ms(3_000), PITCH, pressed),
                (ms(3_000), ms(12_000), PITCH, load),
            ],
        };
        if let Some((by, _)) = push
            && by != 0.0
        {
            lamp.hand = Some((ms(1_000), ms(3_000), PITCH, at + by));
        }
        // Read a few counts either side of where it rests, every 100 ms,
        // unless a hand moves it.
        let moved = push.is_some_and(|(moved, _)| moved != 0.0);
        lamp.misreads = (0..if moved { 0 } else { 120u32 })
            .map(|tick| {
                let by = [3, -3, 2, -1, 0, -2][tick as usize % 6];
                (ms(u64::from(tick) * 100), PITCH, (at as i32 + by) as u16, 1)
            })
            .collect();
        let mut seen = Vec::new();
        let report = run(
            &mut lamp,
            &mut clock,
            Limits {
                limp: true,
                ..giving_way(GiveWay::Return)
            },
            None,
            || Some((Mood::Resting, 0)),
            |glimpse| seen.push(*glimpse),
        );
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        (report, seen)
    };
    for rest in [0.0, 12.0, -14.0] {
        for load in [130, 68, 160] {
            let (report, seen) = ran(rest, load, None);
            // It did give way to its own push, and is held where it gave.
            assert!(report.eased[0] >= 1, "{rest} {load} {report:?}");
            let said: Vec<&Glimpse> = seen
                .iter()
                .filter(|g| clock_of(g) > ms(2_500) && g.hand)
                .collect();
            assert!(
                said.is_empty(),
                "rest {rest}, load {load}: {} steps said a hand, first {:?}",
                said.len(),
                said.first()
            );
        }
    }
    // A hand moving the head 40 counts, and one pressing it past its bound
    // without moving it, each still say a hand while they are on it.
    for push in [(-40.0, 130), (0.0, 300)] {
        let (_, seen) = ran(12.0, 130, Some(push));
        assert!(
            seen.iter()
                .any(|g| g.hand && (ms(1_000)..ms(3_000)).contains(&clock_of(g))),
            "{push:?}"
        );
    }
}

/// A hand that moves the head 40 counts and keeps it there, pressing
/// gently (130, under the head's bound), is a hand for as long as it stays:
/// re-held where it is each second, the head is still that far from where
/// it first gave, so every step of the hold says a hand, and none does once
/// the hand has gone and the hold has run out (held where it is, its goal
/// where it is, it reads next to nothing: lamp-06c1's pitch read 0 a count
/// off its goal).
#[test]
fn a_hand_holding_a_joint_off_where_it_gave_says_a_hand_until_it_lets_go() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(12));
    let at = lamp.servos[PITCH].position + 12.0;
    lamp.servos[PITCH].position = at;
    lamp.servos[PITCH].goal = at as u16;
    lamp.strains = vec![
        (ms(0), ms(1_000), PITCH, 130),
        (ms(1_000), ms(6_000), PITCH, 130),
    ];
    lamp.hand = Some((ms(1_000), ms(6_000), PITCH, at - 40.0));
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        Limits {
            limp: true,
            ..giving_way(GiveWay::Return)
        },
        None,
        || Some((Mood::Resting, 0)),
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.eased[0] >= 1, "{report:?}");
    let late: Vec<&Glimpse> = seen
        .iter()
        .filter(|g| (ms(2_500)..ms(6_000)).contains(&clock_of(g)))
        .collect();
    assert!(!late.is_empty());
    let dropped: Vec<Duration> = late
        .iter()
        .filter(|g| !g.hand)
        .map(|g| clock_of(g))
        .collect();
    assert!(
        dropped.is_empty(),
        "{} of {} steps in the hold said no hand, first at {:?}",
        dropped.len(),
        late.len(),
        dropped.first()
    );
    assert!(
        seen.iter()
            .filter(|g| clock_of(g) > ms(8_500))
            .all(|g| !g.hand),
        "a hand said after it let go"
    );
}

/// The release acceptance rule: once both load and displacement are gone,
/// the hand report clears within one control tick. This is a new reporting
/// requirement, not a claim about how long the earlier body kept a hand.
#[test]
fn stationary_pressure_clears_within_one_tick_after_load_and_displacement_clear() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(7));
    // Above pitch's ordinary bound, below its contact floor: no motion,
    // no physical hand displacement and no contact-floor slump. Releasing
    // the pressure restores the fixture's quiet load of 25 at the goal.
    lamp.strains = vec![(ms(1_500), ms(4_500), PITCH, 300)];
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        Limits {
            limp: true,
            ..giving_way(GiveWay::Return)
        },
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let pressed: Vec<_> = seen
        .iter()
        .filter(|g| (ms(2_500)..ms(4_400)).contains(&clock_of(g)))
        .collect();
    assert!(!pressed.is_empty());
    assert!(
        pressed
            .iter()
            .all(|g| { g.loads[0] == 300 && g.positions[0] == g.goals[0] && g.hand }),
        "stationary pressure was not reported: {pressed:?}"
    );
    let first_pressed = seen.iter().position(|g| g.loads[0] == 300).unwrap();
    let quiet_at = seen
        .iter()
        .enumerate()
        .skip(first_pressed + 1)
        .find(|(_, g)| g.loads[0] == 25 && g.positions[0] == g.goals[0])
        .map(|(index, _)| index)
        .expect("the pressure never cleared at the pinned goal");
    let quiet = &seen[quiet_at..];
    assert!(
        quiet.len() >= 2,
        "no observation after the first quiet read"
    );
    assert_eq!(quiet[1].step, quiet[0].step + 1);
    // `Glimpse` may precede this step's hand decision. Allow that first
    // quiet row, then require every subsequent row to report no hand.
    assert!(
        quiet[1..]
            .iter()
            .all(|g| { g.loads[0] == 25 && g.positions[0] == g.goals[0] && !g.hand }),
        "hand outlasted the first quiet read by more than one tick: {:?}",
        quiet.iter().take(3).collect::<Vec<_>>()
    );
}

/// The hysteresis acceptance rule: a push admitted above the ordinary
/// bound remains a hand above half that bound, even without displacement.
/// This specifies the new retention rule, not the earlier body's policy.
#[test]
fn stationary_pressure_remains_a_hand_when_it_eases_above_half_the_bound() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(10));
    // Pitch's bound is 240. First qualify at 300, then hold at 130: below
    // entry but above retention at 120. Its position never leaves its goal.
    lamp.strains = vec![
        (ms(1_500), ms(3_000), PITCH, 300),
        (ms(3_000), ms(7_000), PITCH, 130),
    ];
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        Limits {
            limp: true,
            ..giving_way(GiveWay::Return)
        },
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    for (from, until, load) in [(ms(2_000), ms(2_900), 300), (ms(5_000), ms(6_900), 130)] {
        let held: Vec<_> = seen
            .iter()
            .filter(|g| (from..until).contains(&clock_of(g)))
            .collect();
        assert!(!held.is_empty(), "no observations at load {load}");
        assert!(
            held.iter()
                .all(|g| g.loads[0] == load && g.positions[0] == g.goals[0] && g.hand),
            "stationary pressure at load {load} was not retained: {held:?}"
        );
    }
}

/// Once contact has pinned a joint, thirteen counts of displacement keep
/// its limp guard held even without load: above LIMP_LAG, below HAND_MOVED.
/// That continuing physical hold must not disappear from the hand report.
#[test]
fn a_pinned_pitch_held_thirteen_counts_away_still_says_a_hand_before_slump() {
    let ms = Duration::from_millis;
    let pressed = GLANCE_UNTIL;
    let moved = 13;
    let mut pinned_at = 0;
    let mut seen = Vec::new();
    let (report, _) = rest_glances_watched(
        |lamp, limits| {
            assert!(LIMP_LAG < moved && moved <= HAND_MOVED);
            limits.give_way = Some(GiveWay::Return);
            limits.limp = true;
            pinned_at = lamp.servos[PITCH].position.round() as u16;
            lamp.strains = vec![
                (pressed, pressed + ms(200), PITCH, 675),
                (pressed + ms(200), Duration::from_secs(16), PITCH, 0),
            ];
            // After contact stops it, a hand moves the real simulated
            // joint; this is not a changed goal or an injected misreading.
            lamp.hand = Some((
                pressed + ms(600),
                pressed + ms(2_500),
                PITCH,
                f32::from(pinned_at + moved),
            ));
        },
        vec![(GLANCE_FROM, pressed + ms(3_000), 0.0)],
        |at, glimpse| seen.push((at, *glimpse)),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.eased, [1, 0, 0], "{report:?}");
    assert_eq!(report.released_by_hand, [1, 0, 0], "{report:?}");
    assert_eq!(report.released.len(), 1, "{report:?}");
    assert_eq!(report.released[0].joint, Joint::WristPitch.name());
    assert_eq!(report.released[0].load, 0, "the slump must be from lag");

    let first = seen.iter().find(|(_, g)| g.eased[0] == 1).unwrap();
    assert!(first.0 >= pressed && first.0 < pressed + ms(100));
    assert_eq!(first.1.positions[0], pinned_at);
    // Establish that contact itself was reported before the later move.
    let contact: Vec<_> = seen
        .iter()
        .filter(|(at, _)| (pressed + ms(100)..pressed + ms(180)).contains(at))
        .collect();
    assert!(!contact.is_empty());
    assert!(contact.iter().all(|(_, g)| {
        g.hand && g.eased == [1, 0, 0] && g.loads[0] == 675 && g.positions[0] == pinned_at
    }));

    // The initial load evidence is more than a second old. The real joint
    // remains held thirteen counts from the pinned goal until the slump.
    let window = pressed + ms(1_300)..pressed + ms(1_600);
    assert!(Duration::from_secs_f32(report.released[0].seconds) > window.end);
    let held: Vec<_> = seen.iter().filter(|(at, _)| window.contains(at)).collect();
    assert!(!held.is_empty(), "no held steps before the slump");
    for (_, g) in held {
        assert!(!g.limp && g.released == 0 && g.eased == [1, 0, 0], "{g:?}");
        assert_eq!(g.goals[0], pinned_at, "{g:?}");
        assert_eq!(g.positions[0], pinned_at + moved, "{g:?}");
        assert_eq!(g.loads[0], 0, "{g:?}");
        assert!(g.hand, "the continuing limp hold disappeared: {g:?}");
    }
}

/// A gentle hand that stays on the base is still said after the joint has
/// repeatedly adopted its position. Reporting must not time out while
/// the same physical push still holds it there under its load bound.
#[test]
fn a_sustained_gentle_push_is_said_until_the_hand_lets_go() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(14));
    let held_at = HOME[2] + 40;
    lamp.hand = Some((ms(1_500), ms(9_500), YAW, f32::from(held_at)));
    lamp.strains = vec![(ms(1_500), ms(9_500), YAW, 120)];
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        giving_way(GiveWay::Return),
        Some(HOME),
        || Some((Mood::Listening, 100)),
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.eased[2] >= 1, "the push never gave way: {report:?}");
    // This window begins well after both the first give-way and the
    // subsequent one-second observations of the continuing hand.
    let held: Vec<_> = seen
        .iter()
        .filter(|g| (ms(5_000)..ms(9_400)).contains(&clock_of(g)))
        .collect();
    assert!(!held.is_empty());
    assert!(
        held.iter()
            .all(|g| g.positions[2] == held_at && g.loads[2] == 120),
        "the fixture did not maintain the physical push: {held:?}"
    );
    assert!(
        held.iter().all(|g| g.hand),
        "the continuing hand disappeared: {:?}",
        held.iter().find(|g| !g.hand)
    );
    let released: Vec<_> = seen
        .iter()
        .filter(|g| (ms(13_000)..ms(13_900)).contains(&clock_of(g)))
        .collect();
    assert!(!released.is_empty());
    assert!(released.iter().all(|g| !g.hand), "{released:?}");
}

/// Told to come back, a base turned by a hand gives way though it reads
/// under the load that ends a run (lamp-52e6's read 120 turned 38 counts),
/// and comes back once let go; without the option the run ends on
/// tracking, as before.
#[test]
fn told_to_return_a_base_turned_by_hand_under_its_load_gives_way_and_comes_back() {
    let ms = Duration::from_millis;
    let ran = |give_way: Option<GiveWay>| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(10));
        lamp.hand = Some((ms(1_500), ms(4_500), YAW, f32::from(HOME[2]) + 40.0));
        lamp.strains = vec![(ms(1_500), ms(4_500), YAW, 120)];
        // Awake: a lamp at rest lies where it is whatever the option.
        let news = || Some((Mood::Listening, 100));
        let mut seen = Vec::new();
        let limits = Limits {
            give_way,
            ..giving_way(GiveWay::Return)
        };
        let report = run(&mut lamp, &mut clock, limits, Some(HOME), news, |glimpse| {
            seen.push(*glimpse)
        });
        (report, seen)
    };
    let (report, seen) = ran(Some(GiveWay::Return));
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.eased[2] >= 1, "{report:?}");
    let before = seen.iter().rev().find(|g| clock_of(g) < ms(1_500)).unwrap();
    let back = seen.iter().rev().find(|g| clock_of(g) < ms(9_900)).unwrap();
    assert!(back.goals[2].abs_diff(before.goals[2]) <= 3, "{back:?}");
    assert!(
        back.positions[2].abs_diff(before.positions[2]) <= 6,
        "{back:?}"
    );
    let (report, _) = ran(None);
    assert!(
        matches!(report.stop, Stop::Tracking { joint: 1, .. }),
        "{report:?}"
    );
}

/// A head that pushes at a goal it does not reach, without moving, is told
/// to stop after a second and a half; without the option it goes on.
#[test]
fn a_joint_pushing_without_moving_is_told_to_stop_after_a_second_and_a_half() {
    let ran = |give_way: Option<GiveWay>| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(6));
        // It stops 27 counts short of any goal, pushing at 130: past where
        // a joint that has arrived is let rest, short of a tracking stop.
        lamp.short = Some((PITCH, 27.0));
        lamp.strains = vec![(Duration::ZERO, Duration::from_secs(60), PITCH, 130)];
        let news = || Some((Mood::Listening, 100));
        let mut seen = Vec::new();
        let limits = Limits {
            give_way,
            ..giving_way(GiveWay::Return)
        };
        let report = run(&mut lamp, &mut clock, limits, None, news, |glimpse| {
            seen.push(*glimpse)
        });
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        (report, seen)
    };
    let (report, seen) = ran(Some(GiveWay::Return));
    assert_eq!(report.stalls, [1, 0, 0, 0, 0]);
    // Pushing without moving from the step it came within six counts of
    // where it stops...
    let began = seen
        .iter()
        .position(|g| g.positions[0] <= 1_987 + STALL_SLACK)
        .unwrap();
    // ...to the step its goal became where it is.
    let stopped = seen
        .iter()
        .position(|g| g.goals[0] == g.positions[0] && g.stalls[0] == 1)
        .unwrap();
    let steps = stopped - began;
    assert!((75..=78).contains(&steps), "{steps}");
    // And it is not pushed at again while it listens.
    assert!(
        seen[stopped..]
            .iter()
            .filter(|g| clock_of(g) < Duration::from_secs(6))
            .all(|g| g.goals[0] == 1_987)
    );
    // Without the option it pushes for as long as it listens.
    let (report, seen) = ran(None);
    assert_eq!(report.stalls, [0; 5]);
    assert!(
        seen.iter()
            .filter(|g| (Duration::from_secs(3)..Duration::from_secs(6)).contains(&clock_of(g)))
            .all(|g| g.goals[0] == 1_960)
    );
}

/// A standing shoulder that rests short of its goal and is pushed well over
/// what it stands with is told to stop: its goal becomes where it is, and
/// torque is never written off.
#[test]
fn a_standing_shoulder_pushing_short_of_its_goal_is_told_to_stop_and_keeps_its_torque() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(12));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    // As lamp-52e6's did: 30 counts short of where it stands, at 150, its
    // own weight and not counted; then a hand pushes it at 400.
    lamp.short = Some((1, 30.0));
    lamp.strains = vec![
        (Duration::ZERO, Duration::from_secs(7), 1, 150),
        (Duration::from_secs(7), Duration::from_secs(60), 1, 400),
    ];
    let news = || Some((Mood::Listening, 100));
    let mut seen = Vec::new();
    let report = run_standing(
        &mut lamp,
        &mut clock,
        giving_way(GiveWay::Stay),
        Some(HOME),
        Some(standing(100, 60)),
        news,
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.stood && report.retreats.is_empty(), "{report:?}");
    assert_eq!(report.stalls, [0, 0, 0, 1, 0]);
    let held = seen.iter().find(|g| g.stalls[3] == 1).unwrap();
    assert_eq!(held.arm_goals[0], 2_620, "{held:?}");
    // Torque was written once, on, to clear the elbow's old flag; never off.
    assert!(
        lamp.registers
            .iter()
            .filter(|(_, address, _)| *address == 40)
            .all(|(_, _, data)| data == &vec![1]),
        "{:?}",
        lamp.registers
    );
    assert!(lamp.servos.iter().all(|servo| servo.torque == 1));
}

/// A shoulder already held 30 counts short at 400 when the lamp stands up
/// does not learn 400 as what it carries: its bound stays 100, and the same
/// push is told to stop.
#[test]
fn a_shoulder_already_pushed_when_the_lamp_stands_learns_no_load_and_still_stops() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(12));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    lamp.short = Some((1, 30.0));
    lamp.strains = vec![(Duration::ZERO, Duration::from_secs(60), 1, 400)];
    let news = || Some((Mood::Listening, 100));
    let report = run_standing(
        &mut lamp,
        &mut clock,
        giving_way(GiveWay::Stay),
        Some(HOME),
        Some(standing(100, 60)),
        news,
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.stood, "{report:?}");
    assert!(report.stalls[3] >= 1, "{report:?}");
}

/// A folded arm lifted by a hand and held is told to stop pushing, and is
/// held there only for a while: then it goes back down to where it rested,
/// so that the next start does not find it off its way.
#[test]
fn a_folded_elbow_lifted_and_held_by_a_hand_goes_back_down_once_let_go() {
    let ms = Duration::from_millis;
    let ran = |give_way: Option<GiveWay>| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(14));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        let rest = lamp.servos[2].position;
        // Lifted 200 counts, past what a standing arm may be off its goal,
        // held there for three seconds, let go.
        lamp.hand = Some((ms(3_000), ms(7_000), 2, rest + 200.0));
        lamp.strains = vec![(ms(3_000), ms(7_000), 2, 200)];
        let news = || Some((Mood::Resting, 0));
        let mut seen = Vec::new();
        let limits = Limits {
            give_way,
            ..giving_way(GiveWay::Return)
        };
        let report = run_standing(
            &mut lamp,
            &mut clock,
            limits,
            Some(HOME),
            Some(standing(100, 60)),
            news,
            |glimpse| seen.push(*glimpse),
        );
        (report, seen, rest)
    };
    let (report, seen, rest) = ran(Some(GiveWay::Return));
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(!report.stood, "{report:?}");
    assert!(report.stalls[4] >= 1, "{report:?}");
    let end = seen.last().unwrap();
    assert!(
        (f32::from(end.arm_goals[1]) - rest).abs() <= 2.0
            && (f32::from(end.arm_positions[1]) - rest).abs() <= 6.0,
        "{end:?}"
    );
    // Without the option, as before: no stall rule, and the elbow's goal
    // never leaves where it rested.
    let (report, seen, rest) = ran(None);
    assert_eq!(report.stalls, [0; 5], "{report:?}");
    assert!(
        seen.iter()
            .all(|g| (f32::from(g.arm_goals[1]) - rest).abs() <= 2.0),
        "{report:?}"
    );
}

/// `--give-way off --limp off` (#857) gives the body Limits::FOLDED's
/// give_way None and limp false; over a session with moods and a hand on the
/// head, those limits write exactly what the body wrote before give-way was on
/// by default, and with give-way on the same session does not.
#[test]
fn off_is_the_body_before_give_way_was_on_by_default() {
    let ms = Duration::from_millis;
    let session = |limits: Limits| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(10));
        lamp.hand = Some((ms(1_500), ms(3_000), ROLL, f32::from(HOME[1]) + 40.0));
        lamp.strains = vec![(ms(1_500), ms(3_000), ROLL, 240)];
        let news = script(&clock.time);
        let report = run(&mut lamp, &mut clock, limits, Some(HOME), news, |_| {});
        (report.stop, lamp.goals, lamp.registers)
    };
    let off = Limits {
        give_way: None,
        limp: false,
        ..Limits::FOLDED
    };
    let before = session(Limits::FOLDED);
    assert_eq!(session(off), before);
    let on = Limits {
        give_way: Some(GiveWay::Return),
        limp: true,
        ..Limits::FOLDED
    };
    assert_ne!(session(on), before, "the comparison can tell on from off");
}

/// lamp-52e6 after its power was cut and came back on 2026-10-14: every
/// servo with torque off and a goal of 0, the head drooped from 1,771 to
/// 2,034, the base and the roll moved. Its unit's options, with `--wake on`.
#[test]
fn a_head_that_drooped_in_a_power_cut_is_lifted_home_with_lamp_52e6s_options() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(40));
    lamp.may_take_hold = true;
    lamp.arm_moves = true;
    for (index, at) in [
        (YAW, 1_958.0),
        (1, 2_045.0),
        (2, 2_083.0),
        (ROLL, 2_100.0),
        (PITCH, 2_034.0),
    ] {
        let servo = &mut lamp.servos[index];
        (servo.position, servo.goal, servo.torque, servo.status) = (at, 0, 0, 0);
    }
    // Lifting from the droop it lags its goal by 37 counts at a load of
    // 280, and does not move for a goal less far ahead.
    lamp.heavy = Some((PITCH, 1_850.0, 37.0, 320));
    let limits = Limits {
        force: Some(600),
        torque_on: true,
        wake: true,
        ..Limits::folded_with(Tuning {
            reach: [150, 15, 1, 250],
            speed: 120,
            servo_speed: 1_000,
            servo_acceleration: 60,
        })
        .unwrap()
    };
    let up = Standing::new(
        [2_053, 2_069],
        [1_772, 2_035, 2_035, 2_657, 2_602],
        100,
        5,
        90,
        [560, 560],
    )
    .unwrap();
    let report = run_standing(
        &mut lamp,
        &mut clock,
        limits,
        Some([1_771, 2_035, 2_035]),
        Some(up),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.standing.is_some(), "{report:?}");
    let woke = report.woke.unwrap();
    // All three come home, the base after the head.
    assert_eq!(woke.joints, vec!["wrist_pitch", "wrist_roll", "base_yaw"]);
    assert_eq!(
        (woke.from, woke.to),
        (vec![2_034, 2_100, 1_958], vec![1_771, 2_035, 2_035])
    );
    // A stir that reaches 80 counts ahead, a pause, then the rest of 263;
    // then the base's 77, in its stir and a pause.
    assert!((26.0..30.0).contains(&woke.seconds), "{}", woke.seconds);
    // Lifting, it read 320 and lagged its goal by 37, and was let.
    assert!(report.peak_load[0] >= 320, "{:?}", report.peak_load);
    // Never toward the base.
    let pitch = goals_of(&lamp, Joint::WristPitch);
    let home = pitch.iter().position(|goal| *goal == 1_771).unwrap();
    assert!(
        pitch[..=home].windows(2).all(|two| two[1] <= two[0]),
        "{pitch:?}"
    );
    assert!(pitch.iter().all(|goal| *goal <= 2_034));
    assert!((lamp.servos[PITCH].position - 1_771.0).abs() <= 30.0);
}

#[test]
fn a_head_that_cannot_lift_ends_the_wake_held_where_it_is_and_says_how_far_it_got() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(40));
    lamp.servos[PITCH].position = 2_095.0;
    lamp.servos[PITCH].goal = 2_095;
    // It never moves for a goal less than 150 counts ahead: its goal gets
    // home, it stays still for half a second, and the run ends.
    lamp.heavy = Some((PITCH, 1_850.0, 150.0, 200));
    let report = run(
        &mut lamp,
        &mut clock,
        waking(),
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |_| panic!("a lamp that did not wake does not run"),
    );
    assert!(
        report.stop
            == Stop::Tracking {
                joint: 5,
                goal: 2_000,
                position: 2_095
            },
        "{:?}",
        report.stop
    );
    let woke = report.woke.unwrap();
    assert_eq!(woke.from, vec![2_095]);
    assert_eq!(woke.to, vec![2_000]);
    assert_eq!(report.held_at.map(|held| held[0]), Some(2_095));
    // Held, then let go of, as any surprise: a head pushing at a goal it
    // cannot reach heats.
    assert!(let_go_of_the_whole_lamp(&lamp), "{:?}", lamp.registers);
}

/// lamp-52e6 woken from a real droop on 2026-10-14: the head lifted and
/// stayed 32 counts short of home. It has woken there, and runs from there.
#[test]
fn a_head_that_settles_32_short_of_home_wakes_there_and_runs_from_where_it_is() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(50));
    lamp.may_take_hold = true;
    for (index, at) in [(YAW, 1_979.0), (ROLL, 2_101.0), (PITCH, 2_029.0)] {
        let servo = &mut lamp.servos[index];
        (servo.position, servo.goal, servo.torque) = (at, 0, 0);
    }
    lamp.servos[2].status = 0;
    // Lifting, it always stops 32 short of its goal.
    lamp.heavy = Some((PITCH, 0.0, 32.0, 232));
    let limits = Limits {
        torque_on: true,
        wake: true,
        ..Limits::folded_with(Tuning {
            reach: [150, 15, 1, 250],
            speed: 120,
            servo_speed: 1_000,
            servo_acceleration: 60,
        })
        .unwrap()
    };
    let report = run(
        &mut lamp,
        &mut clock,
        limits,
        Some([1_771, 2_035, 2_035]),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let woke = report.woke.unwrap();
    assert_eq!(woke.to, vec![1_771, 2_035, 2_035]);
    assert_eq!(woke.settled, vec![1_803, 2_035, 2_035]);
    // Its home is where it settled; nothing pushes at the gap.
    assert_eq!(report.home, Some([1_803, 2_035, 2_035]));
    let pitch = goals_of(&lamp, Joint::WristPitch);
    // Home stays its goal while the base wakes after it; then the run's.
    let home = pitch.iter().position(|goal| *goal == 1_771).unwrap();
    let settled = home
        + pitch[home..]
            .iter()
            .position(|goal| *goal != 1_771)
            .unwrap();
    assert_eq!(pitch[settled], 1_803);
    assert!(
        pitch[settled..].iter().all(|goal| *goal >= 1_803),
        "{pitch:?}"
    );
    assert_eq!(lamp.servos[PITCH].goal, 1_803);
    assert_eq!(lamp.servos[PITCH].position, 1_803.0);
}

/// The folded limits, letting go under overload.
fn releasing() -> Limits {
    Limits {
        overload_release: true,
        ..Limits::FOLDED
    }
}

/// A joint at 800 for a second and a half: torque off to all five, byte
/// for byte; nothing written for two minutes, whatever it is told; then
/// each joint held where it slumped, goal first; a head that drooped 250
/// counts lifted as a lamp wakes from a power cut, its goal leading, and
/// woken where it settles 32 short; the arm left where it slumped.
/// Overloaded again within ten minutes, it lets go and the run ends there.
#[test]
fn a_joint_at_800_for_a_second_and_a_half_lets_go_of_all_five_and_is_taken_up_again_slowly() {
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(hex(&torque_write(Joint::BaseYaw, 0)), "ffff0104032800cf");
    let secs = Duration::from_secs_f32;
    let (mut lamp, mut clock) = folded(Duration::from_secs(200));
    lamp.may_take_hold = true;
    // The elbow at 800 for 1.6 s, and again once the lamp is up again.
    lamp.strains = vec![
        (secs(1.0), secs(2.6), 2, 800),
        (secs(150.0), secs(151.6), 2, 800),
    ];
    // Let go, the head droops 250 counts and the elbow falls 40. This head,
    // as lamp-52e6's does, stops 32 counts short of a goal it lifts to.
    lamp.hand = Some((secs(5.0), secs(8.0), PITCH, 2_250.0));
    lamp.short = Some((PITCH, 32.0));
    lamp.event = Some((secs(9.0), 2, Event::Push(-40.0)));
    // Spoken to while it rests: nothing is done for it.
    let time = clock.time.clone();
    let news = move || {
        Some(if (secs(30.0)..secs(60.0)).contains(&time.get()) {
            (Mood::Listening, 120)
        } else {
            (Mood::Resting, 0)
        })
    };
    let mut limp = Vec::new();
    let report = run(&mut lamp, &mut clock, releasing(), Some(HOME), news, |g| {
        if g.limp {
            limp.push(*g);
        }
    });
    assert_eq!(
        report.stop,
        Stop::Overload {
            joint: 3,
            load: 800,
            detail: "let go a second time within ten minutes; torque left off".into()
        },
        "{report:?}"
    );
    assert_eq!(report.released.len(), 2);
    let first = &report.released[0];
    assert_eq!((first.joint, first.load), ("elbow_pitch", 800));
    // 75 steps of 20 ms after the load began at 1 s.
    assert!((2.45..2.6).contains(&first.seconds), "{}", first.seconds);
    let tripped = secs(first.seconds);

    // Torque off to the five, in servo order, before anything else.
    let off: Vec<(u8, u8, Vec<u8>)> = Joint::ALL.iter().map(|j| (j.id(), 40, vec![0])).collect();
    let at = lamp
        .registers
        .iter()
        .position(|(_, address, data)| *address == 40 && data == &vec![0])
        .unwrap();
    assert_eq!(lamp.registers[at..at + 5], off[..]);
    assert!(lamp.register_times[..at].iter().all(|t| *t < tripped));
    // Nothing written for two minutes: no goal, no register.
    let rest = tripped..tripped + OVERLOAD_REST;
    assert!(lamp.goal_times.iter().all(|t| !rest.contains(t)));
    assert!(
        lamp.register_times[at + 5..]
            .iter()
            .all(|t| *t >= tripped + OVERLOAD_REST)
    );
    // It watched every step of it, at rest whatever it was told.
    assert!(limp.len() >= 6_000, "{}", limp.len());
    assert!(
        limp.iter()
            .filter(|g| g.released == 1)
            .all(|g| g.mood == Mood::Resting && g.goals == HOME)
    );

    // Then each joint held where it slumped: its goal, then its torque.
    let goal = |value: u16| value.to_le_bytes().to_vec();
    assert_eq!(
        lamp.registers[at + 5..at + 15],
        [
            (5, 42, goal(2_250)),
            (5, 40, vec![1]),
            (4, 42, goal(2_044)),
            (4, 40, vec![1]),
            (1, 42, goal(2_015)),
            (1, 40, vec![1]),
            (2, 42, goal(2_050)),
            (2, 40, vec![1]),
            (3, 42, goal(2_007)),
            (3, 40, vec![1]),
        ]
    );
    // And the head lifted as a lamp wakes from a power cut: a count a
    // write at most, never away from home, the goal leading the head; it
    // settled 32 short, and runs from there, its home moved by as much.
    let woke = first.woke.as_ref().unwrap();
    assert_eq!(
        (&woke.joints, &woke.from, &woke.to, &woke.settled),
        (
            &vec!["wrist_pitch"],
            &vec![2_250],
            &vec![2_000],
            &vec![2_032]
        )
    );
    // A stir of 80 counts, a second's pause, the other 170, and settling.
    assert!((19.0..22.5).contains(&woke.seconds), "{}", woke.seconds);
    assert_eq!(report.home, Some([2_032, 2_044, 2_015]));
    let taken = lamp.register_times[at + 14];
    let pitch: Vec<u16> = lamp
        .goals
        .iter()
        .zip(&lamp.goal_times)
        .filter(|(_, t)| (taken..secs(150.0)).contains(*t))
        .map(|(write, _)| goal_of(write, Joint::WristPitch))
        .collect();
    assert_eq!(pitch[0], 2_249);
    let home = pitch.iter().position(|goal| *goal == 2_000).unwrap();
    assert!(
        pitch[..=home]
            .windows(2)
            .all(|two| two[1] <= two[0] && two[0] - two[1] <= 1),
        "{pitch:?}"
    );
    assert_eq!(pitch[home + 1], 2_032);
    // Nothing pushes at the gap: a count of the run's own easing at most.
    assert!(
        pitch[home + 1..].iter().all(|goal| *goal >= 2_031),
        "{pitch:?}"
    );
    assert_eq!(lamp.servos[PITCH].position, 2_032.0);
    // The arm of a lamp that does not stand stays where it slumped.
    assert!(lamp.goals.iter().all(|write| write.len() == 3));
    assert_eq!(lamp.servos[2].position, 2_007.0);

    // The second time: let go, and nothing after it.
    let second = secs(report.released[1].seconds);
    assert!((151.4..151.7).contains(&report.released[1].seconds));
    assert_eq!(lamp.registers[lamp.registers.len() - 5..], off[..]);
    assert!(lamp.goal_times.iter().all(|t| *t < second));
    assert!(lamp.servos.iter().all(|servo| servo.torque == 0));
    serde_json::to_string(&report).unwrap();
}

/// 800 for 1.4 s is not an overload, and with the option a run where none
/// comes writes exactly what it writes without it.
#[test]
fn a_load_of_800_for_less_than_a_second_and_a_half_changes_nothing() {
    let writes = |limits: Limits| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(8));
        lamp.strains = vec![(Duration::from_secs(1), Duration::from_millis(2_400), 2, 800)];
        let news = script(&clock.time);
        let report = run(&mut lamp, &mut clock, limits, None, news, |_| {});
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        assert!(report.released.is_empty());
        (lamp.goals, lamp.registers)
    };
    assert_eq!(writes(releasing()), writes(Limits::FOLDED));
}

/// An elbow held to a force limit of 600 trips at 480, 80% of it, where
/// 800 could never be read; without the limit 500 is not an overload.
/// Asked to stop while it rests, it ends with torque left off.
#[test]
fn a_force_limited_arm_lets_go_at_80_percent_of_its_limit() {
    let ran = |force: Option<u16>| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(5));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        lamp.may_take_hold = true;
        lamp.strains = vec![(Duration::from_secs(1), Duration::from_secs(3), 2, 500)];
        let limits = Limits {
            force,
            ..releasing()
        };
        let report = run_standing(
            &mut lamp,
            &mut clock,
            limits,
            Some(HOME),
            Some(standing(100, 5)),
            || Some((Mood::Resting, 0)),
            |_| {},
        );
        (report, lamp)
    };
    let (report, lamp) = ran(Some(600));
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.released.len(), 1);
    let released = &report.released[0];
    assert_eq!((released.joint, released.load), ("elbow_pitch", 500));
    assert!((2.45..2.6).contains(&released.seconds));
    assert!(lamp.servos.iter().all(|servo| servo.torque == 0));
    assert!(
        lamp.goal_times
            .iter()
            .all(|t| *t < Duration::from_secs_f32(released.seconds))
    );

    let (report, lamp) = ran(None);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.released.is_empty());
    assert!(lamp.servos.iter().all(|servo| servo.torque == 1));
}

/// lamp-52e6 unplugged while it stood, on 2026-10-14: the shoulder stayed
/// up at 2,633 on its gears and the elbow fell folded, to 2,074; every
/// servo without torque and with a goal of 0; the head drooped.
#[test]
fn a_shoulder_left_up_by_a_power_cut_is_lowered_first_and_the_lamp_may_then_stand() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(70));
    lamp.may_take_hold = true;
    lamp.arm_moves = true;
    for (index, at) in [
        (YAW, 1_979.0),
        (1, 2_633.0),
        (2, 2_074.0),
        (ROLL, 2_101.0),
        (PITCH, 2_029.0),
    ] {
        let servo = &mut lamp.servos[index];
        (servo.position, servo.goal, servo.torque, servo.status) = (at, 0, 0, 0);
    }
    lamp.heavy = Some((PITCH, 1_850.0, 37.0, 232));
    let limits = Limits {
        force: Some(600),
        torque_on: true,
        wake: true,
        ..Limits::folded_with(Tuning {
            reach: [150, 15, 1, 250],
            speed: 120,
            servo_speed: 1_000,
            servo_acceleration: 60,
        })
        .unwrap()
    };
    let up = Standing::new(
        [2_053, 2_069],
        [1_772, 2_035, 2_035, 2_657, 2_602],
        100,
        5,
        90,
        [560, 560],
    )
    .unwrap();
    // Spoken to once it is awake.
    let time = clock.time.clone();
    let news = move || {
        Some(if time.get() > Duration::from_secs(50) {
            (Mood::Listening, 120)
        } else {
            (Mood::Resting, 0)
        })
    };
    let report = run_standing(
        &mut lamp,
        &mut clock,
        limits,
        Some([1_771, 2_035, 2_035]),
        Some(up),
        news,
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.standing_refused, None, "{report:?}");
    let lowered = report.lowered.clone().unwrap();
    assert_eq!((lowered.from, lowered.to), (vec![2_633], vec![2_053]));
    // 580 counts eased at 40 a second at most: a little under 22 s.
    assert!(
        (21.5..23.0).contains(&lowered.seconds),
        "{}",
        lowered.seconds
    );
    assert!(report.woke.is_some());
    assert!(report.stood, "{report:?}");
    // On the way down the shoulder only ever went down, under a count a
    // write, and the elbow was never asked to move.
    let shoulder = goals_of(&lamp, Joint::BasePitch);
    let down = shoulder.iter().position(|goal| *goal == 2_053).unwrap();
    assert!(
        shoulder[..=down]
            .windows(2)
            .all(|two| two[1] <= two[0] && two[0] - two[1] <= 1),
        "{shoulder:?}"
    );
    assert!(
        goals_of(&lamp, Joint::ElbowPitch)[..=down]
            .iter()
            .all(|goal| *goal == 2_074)
    );

    // Without waking it is refused standing, as before.
    let (mut lamp, mut clock) = folded(Duration::from_secs(2));
    lamp.servos[1].position = 2_633.0;
    lamp.servos[1].goal = 2_633;
    lamp.servos[2].status = 0;
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 60)),
        || None,
        |_| {},
    );
    assert!(report.lowered.is_none());
    assert!(report.standing_refused.unwrap().contains("off the way"));
}

/// A standing lamp let go under overload (`by_hand` false) or by a hand
/// on its base, as on lamp-52e6 on 2026-10-16. With `elbow_falls` the
/// elbow falls folded in the rest while the shoulder stays up on its
/// gears. `shoulder_strain`: the shoulder reads 700 for that while, as a
/// hand stopping it. Spoken to from 190 s to 200 s. Each step watched, with
/// when.
fn let_go_standing(
    by_hand: bool,
    elbow_falls: bool,
    shoulder_strain: Option<(f32, f32)>,
) -> (Lamp, BodyReport, Vec<(Duration, Glimpse)>) {
    let_go_standing_turned(
        by_hand,
        elbow_falls,
        shoulder_strain,
        (3.0, 2_065.0),
        |_| {},
    )
}

/// As `let_go_standing`, the hand turning the base until `hand.0` s,
/// toward `hand.1`, and `more` done to the lamp last.
fn let_go_standing_turned(
    by_hand: bool,
    elbow_falls: bool,
    shoulder_strain: Option<(f32, f32)>,
    hand: (f32, f32),
    more: fn(&mut Lamp),
) -> (Lamp, BodyReport, Vec<(Duration, Glimpse)>) {
    let secs = Duration::from_secs_f32;
    let (mut lamp, mut clock) = folded(secs(215.0));
    lamp.may_take_hold = true;
    lamp.arm_moves = true;
    // Found standing.
    for (index, at) in [
        (YAW, 2_035.0),
        (1, 2_657.0),
        (2, 2_602.0),
        (ROLL, 2_035.0),
        (PITCH, 1_772.0),
    ] {
        let servo = &mut lamp.servos[index];
        (servo.position, servo.goal, servo.status) = (at, at as u16, 0);
    }
    // The elbow over the cut-off (80% of 600) for 1.6 s, or a hand on the
    // base a second after it stopped.
    lamp.strains = if by_hand {
        lamp.hand = Some((secs(1.5), secs(hand.0), YAW, hand.1));
        vec![(secs(1.5), secs(3.0), YAW, 600)]
    } else {
        vec![(secs(1.0), secs(2.6), 2, 500)]
    };
    if let Some((from, until)) = shoulder_strain {
        lamp.strains.push((secs(from), secs(until), 1, 700));
    }
    if elbow_falls {
        lamp.event = Some((secs(4.0), 2, Event::Fall(-520.0)));
    }
    more(&mut lamp);
    let limits = Limits {
        force: Some(600),
        overload_release: !by_hand,
        give_way: by_hand.then_some(GiveWay::Return),
        limp: by_hand,
        ..Limits::folded_with(Tuning {
            reach: [150, 15, 1, 250],
            speed: 120,
            servo_speed: 1_000,
            servo_acceleration: 60,
        })
        .unwrap()
    };
    let up = Standing::new(
        [2_053, 2_069],
        [1_772, 2_035, 2_035, 2_657, 2_602],
        100,
        5,
        90,
        [560, 560],
    )
    .unwrap();
    // Spoken to also while the shoulder is strained: a mood that wants the
    // lamp up while a hand stops it.
    let strained = shoulder_strain.map_or(Duration::ZERO..Duration::ZERO, |(from, until)| {
        secs(from)..secs(until)
    });
    let time = clock.time.clone();
    let news = move || {
        let now = time.get();
        Some(
            if (secs(190.0)..secs(200.0)).contains(&now) || strained.contains(&now) {
                (Mood::Speaking, 60)
            } else {
                (Mood::Resting, 0)
            },
        )
    };
    let mut seen: Vec<(Duration, Glimpse)> = Vec::new();
    let time = clock.time.clone();
    let report = run_standing(
        &mut lamp,
        &mut clock,
        limits,
        Some([1_771, 2_035, 2_035]),
        Some(up),
        news,
        |g| seen.push((time.get(), *g)),
    );
    (lamp, report, seen)
}

/// Spoken to, it stands by the ordinary way up: fully stood, the arm at
/// its pose or within a speaking lean of it (up to 60 counts, #1010), and
/// the arm where it is asked to be.
fn stood_when_spoken_to(seen: &[(Duration, Glimpse)]) {
    let (_, after) = seen
        .iter()
        .rfind(|(t, _)| *t < Duration::from_secs(200))
        .unwrap();
    assert!(after.stands && after.stand == 1.0, "{after:?}");
    for (index, pose) in [2_657u16, 2_602].into_iter().enumerate() {
        assert!(after.arm_goals[index].abs_diff(pose) <= 60, "{after:?}");
        assert!(
            after.arm_positions[index].abs_diff(after.arm_goals[index]) <= 2,
            "{after:?}"
        );
    }
}

/// The shoulder's and the elbow's goals, in every write from when it was
/// first taken up again (each joint's goal written as where it lay) until
/// it is spoken to, never more than `most` counts from the last.
fn arm_never_jumps(lamp: &Lamp, report: &BodyReport, most: u16) {
    let taken = Duration::from_secs_f32(report.released[0].seconds) + Duration::from_secs(1);
    for joint in ARM {
        let goals: Vec<u16> = lamp
            .goals
            .iter()
            .zip(&lamp.goal_times)
            .filter(|(_, t)| (taken..Duration::from_secs(190)).contains(*t))
            .map(|(write, _)| goal_of(write, joint))
            .collect();
        assert!(
            goals.windows(2).all(|two| two[0].abs_diff(two[1]) <= most),
            "{joint}: {:?}",
            goals.windows(2).find(|two| two[0].abs_diff(two[1]) > most)
        );
    }
}

/// As lamp-52e6 on 2026-10-16: standing, let go, and the elbow fell folded
/// while the shoulder stayed up on its gears. Taken up again, the shoulder
/// is lowered as after a power cut, every step of it watched (systemd's
/// `WatchdogSec=5` hears from the body each step, not 22 s later), the
/// lamp counts as folded (it said `stand 1.00` and never stood again), the
/// head is brought to its folded home, and the next time it is spoken to
/// it stands by the ordinary way up. Under overload only: a hand never
/// lets go of the arm (#3702).
#[test]
fn let_go_standing_with_the_elbow_fallen_it_is_folded_and_stands_again_when_spoken_to() {
    let secs = Duration::from_secs_f32;
    {
        let by_hand = false;
        let (lamp, report, seen) = let_go_standing(by_hand, true, None);
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        assert_eq!(report.released.len(), 1, "{report:?}");
        assert_eq!(report.released_by_hand, [0, 0, u16::from(by_hand)]);
        assert_eq!(report.standing_refused, None, "{report:?}");
        let lowered = report.lowered.clone().unwrap();
        assert_eq!((lowered.from, lowered.to), (vec![2_657], vec![2_053]));
        assert!(lowered.seconds > 20.0, "{}", lowered.seconds);
        // Never a second without a step watched, the lowering included.
        let gap = seen
            .windows(2)
            .map(|two| two[1].0 - two[0].0)
            .max()
            .unwrap();
        assert!(gap <= secs(1.0), "{gap:?}");
        // Let go, and until spoken to again, it never says it stands.
        let quiet = |(t, g): &&(Duration, Glimpse)| g.released > 0 && *t < secs(190.0);
        assert!(
            seen.iter().filter(quiet).all(|(_, g)| g.stand < 0.05),
            "{:?}",
            seen.iter().filter(quiet).find(|(_, g)| g.stand >= 0.05)
        );
        // Folded, the head is at its folded home before it is spoken to.
        let (_, before) = seen.iter().rfind(|(t, _)| *t < secs(190.0)).unwrap();
        assert!(before.stands && before.stand == 0.0, "{before:?}");
        assert_eq!(before.goals, [1_771, 2_035, 2_035]);
        assert!(before.arm_positions[0].abs_diff(2_053) <= 80, "{before:?}");
        arm_never_jumps(&lamp, &report, 2);
        stood_when_spoken_to(&seen);
    }
}

/// One reading off, under `READ_JUMP`, of the held base, the head's pitch or
/// the elbow while the shoulder comes down after an overload (at 127 s, the
/// lowering under way from about 123 s): not a hand. Nothing
/// happens: no second let-go, the run goes on, and no hold asks for it.
#[test]
fn one_reading_off_while_the_shoulder_comes_down_is_not_a_hand() {
    fn base(lamp: &mut Lamp) {
        lamp.misreads = vec![(Duration::from_secs(127), YAW, 2_135, 1)];
    }
    fn pitch(lamp: &mut Lamp) {
        lamp.misreads = vec![(Duration::from_secs(127), PITCH, 1_872, 1)];
    }
    fn elbow(lamp: &mut Lamp) {
        lamp.misreads = vec![(Duration::from_secs(127), 2, 2_200, 1)];
    }
    for misread in [base as fn(&mut Lamp), pitch, elbow] {
        let (lamp, report, seen) =
            let_go_standing_turned(false, true, None, (3.0, 2_065.0), misread);
        assert!(lamp.misreads.iter().all(|misread| misread.3 == 0));
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        assert_eq!(report.released.len(), 1, "{report:?}");
        assert_eq!(report.lowered.clone().unwrap().to, vec![2_053]);
        arm_never_jumps(&lamp, &report, 2);
        stood_when_spoken_to(&seen);
    }
}

/// The held joints' loads while the shoulder comes down are in the run's
/// peaks (the reviewer on #1115): a press of 200 on the head's pitch from
/// 125 s to 133 s, after an overload, under its bound of 240 so nothing lets go, while only the
/// lowering reads it (it reads about 25 otherwise), shows as its
/// `peak_load`.
#[test]
fn a_press_on_a_held_joint_while_the_shoulder_comes_down_is_in_the_peaks() {
    let (_, report, _) = let_go_standing_turned(false, true, None, (3.0, 2_065.0), |lamp| {
        let secs = Duration::from_secs_f32;
        lamp.strains.push((secs(125.0), secs(133.0), PITCH, 200));
    });
    assert_eq!(report.released.len(), 1, "{report:?}");
    assert_eq!(report.peak_load[0], 200, "{:?}", report.peak_load);
}

/// The reviewer's 16:10 probe (#1115): a press on the base stops the
/// lowering after an overload (a load of 400 from 125 s), which ends the
/// run, while the elbow or the head's pitch reads off (2,200 or 1,872, five
/// reads from 125.06 s). The hold writes
/// where the joint was found, never the misread: no goal written asks for
/// it. Without `now[index].position = was.position` it writes it once.
#[test]
fn a_press_that_stops_the_lowering_while_a_joint_reads_off_never_holds_the_misread() {
    fn elbow(lamp: &mut Lamp) {
        let secs = Duration::from_secs_f32;
        lamp.strains.push((secs(125.0), secs(133.0), YAW, 400));
        lamp.misreads = vec![(secs(125.06), 2, 2_200, 5)];
    }
    fn pitch(lamp: &mut Lamp) {
        let secs = Duration::from_secs_f32;
        lamp.strains.push((secs(125.0), secs(133.0), YAW, 400));
        lamp.misreads = vec![(secs(125.06), PITCH, 1_872, 5)];
    }
    for (misread, joint, reading) in [
        (elbow as fn(&mut Lamp), Joint::ElbowPitch, 2_200),
        (pitch, Joint::WristPitch, 1_872),
    ] {
        let (lamp, report, _) = let_go_standing_turned(false, true, None, (3.0, 2_065.0), misread);
        assert!(report.held_at.is_some(), "{joint}: {report:?}");
        let asked = lamp
            .goals
            .iter()
            .filter(|write| {
                write
                    .iter()
                    .any(|(id, goal)| *id == joint.id() && *goal == reading)
            })
            .count();
        assert_eq!(asked, 0, "{joint}: a goal of {reading} was written");
    }
}

/// The same under overload: a lowering that fails holds the joints where
/// they are, ends the run, and lets go of the whole lamp, as at start.
#[test]
fn a_lowering_that_fails_after_an_overload_ends_the_run_and_lets_the_whole_lamp_go() {
    let (lamp, report, _) = let_go_standing(false, true, Some((126.0, 128.0)));
    assert!(
        matches!(
            report.stop,
            Stop::Load {
                joint: 2,
                load: 700
            }
        ),
        "{report:?}"
    );
    assert_eq!(report.released.len(), 1, "{report:?}");
    assert!(report.held_at.is_some(), "{report:?}");
    assert!(let_go_of_the_whole_lamp(&lamp));
    arm_never_jumps(&lamp, &report, 2);
}

/// Let go standing, its elbow did not fall: nothing is lowered, the lamp
/// counts as standing as far as the arm is, and spoken to it stands from
/// there.
#[test]
fn let_go_standing_with_the_arm_still_up_nothing_is_lowered_and_it_stands_from_there() {
    let (lamp, report, seen) = let_go_standing(true, false, None);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.released.len(), 1, "{report:?}");
    assert!(report.lowered.is_none(), "{report:?}");
    assert_eq!(report.standing_refused, None, "{report:?}");
    let taken = seen
        .iter()
        .filter(|(t, g)| g.released > 0 && !g.limp && *t < Duration::from_secs(190));
    assert!(taken.clone().count() > 0);
    assert!(taken.into_iter().all(|(_, g)| g.stand > 0.9), "{report:?}");
    arm_never_jumps(&lamp, &report, 2);
    stood_when_spoken_to(&seen);
}

/// #3702: a standing lamp under `--limp on` that a hand trips lets go of
/// the head and the base only. Torque off is written to head pitch, head
/// roll and base yaw, never to the shoulder or the elbow, and the lamp
/// stays standing: no step while it is let go or after says it folded.
#[test]
fn a_hand_lets_go_of_the_head_and_base_only_and_a_standing_lamp_stays_up() {
    let (lamp, report, seen) = let_go_standing(true, false, None);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.released_by_hand, [0, 0, 1]);
    let off: Vec<u8> = lamp
        .registers
        .iter()
        .filter(|(_, address, data)| *address == 40 && data == &vec![0])
        .map(|(id, _, _)| *id)
        .collect();
    assert_eq!(off, COMMANDED.map(|joint| joint.id()), "{off:?}");
    assert!(
        lamp.registers
            .iter()
            .all(|(id, address, _)| *address != 40 || ARM.iter().all(|j| j.id() != *id)),
        "an arm joint was written torque"
    );
    assert!(report.standing.is_some(), "{report:?}");
    let tripped = Duration::from_secs_f32(report.released[0].seconds);
    let after: Vec<&Glimpse> = seen
        .iter()
        .filter(|(t, _)| *t >= tripped && *t < Duration::from_secs(190))
        .map(|(_, g)| g)
        .collect();
    assert!(after.iter().any(|g| g.limp));
    assert!(after.iter().all(|g| g.stand > 0.9), "{report:?}");
}

/// #3702 review: during a hand's rest the arm is still held, and keeps the
/// overload rule. A person pressing the limp head down into a standing lamp
/// strains the shoulder (700, over the 480 cut-off) from 3.5 s to 7.5 s,
/// inside the hand's rest (from about 2.5 s): it trips as an overload, all
/// five are let go, and the arm's peak load is in the report.
#[test]
fn a_strained_arm_during_a_hands_rest_lets_go_of_all_five_as_an_overload() {
    let (lamp, report, _) = let_go_standing(true, false, Some((3.5, 7.5)));
    assert_eq!(report.released.len(), 2, "{report:?}");
    assert_eq!(
        report.released[1].joint,
        Joint::BasePitch.name(),
        "{report:?}"
    );
    assert!(report.released[1].load >= 480, "{report:?}");
    assert!(report.released[1].seconds < 7.5, "{report:?}");
    let off: Vec<u8> = lamp
        .registers
        .iter()
        .filter(|(_, address, data)| *address == 40 && data == &vec![0])
        .map(|(id, _, _)| *id)
        .collect();
    for joint in Joint::ALL {
        assert!(off.contains(&joint.id()), "{joint}: {off:?}");
    }
    assert!(report.arm_peak_load[0] >= 700, "{report:?}");
}

/// A shoulder left up by a power cut that does not come down when it is
/// lowered: the run ends there, held where it is, and the whole lamp is
/// let go, as any surprise.
#[test]
fn a_shoulder_that_will_not_come_down_ends_the_run_and_lets_the_whole_lamp_go() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(70));
    lamp.may_take_hold = true;
    lamp.arm_moves = false;
    for (index, at) in [
        (YAW, 1_979.0),
        (1, 2_633.0),
        (2, 2_074.0),
        (ROLL, 2_101.0),
        (PITCH, 2_029.0),
    ] {
        let servo = &mut lamp.servos[index];
        (servo.position, servo.goal, servo.torque, servo.status) = (at, 0, 0, 0);
    }
    let limits = Limits {
        torque_on: true,
        wake: true,
        ..Limits::folded_with(Tuning {
            reach: [150, 15, 1, 250],
            speed: 120,
            servo_speed: 1_000,
            servo_acceleration: 60,
        })
        .unwrap()
    };
    let up = Standing::new(
        [2_053, 2_069],
        [1_772, 2_035, 2_035, 2_657, 2_602],
        100,
        5,
        90,
        [560, 560],
    )
    .unwrap();
    let report = run_standing(
        &mut lamp,
        &mut clock,
        limits,
        Some([1_771, 2_035, 2_035]),
        Some(up),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert!(
        matches!(report.stop, Stop::Tracking { joint: 2, .. }),
        "{:?}",
        report.stop
    );
    assert!(report.held_at.is_some(), "{report:?}");
    assert!(let_go_of_the_whole_lamp(&lamp), "{:?}", lamp.registers);
}

/// A head held 35 counts short of its goal by its own weight, at rest:
/// with `--wake on` it rests where it is; without, the run ends as before.
#[test]
fn with_waking_a_head_held_35_short_at_rest_rests_where_it_is() {
    let held = |limits: Limits| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(8));
        lamp.servos[PITCH].position = 2_035.0;
        lamp.servos[PITCH].goal = 2_035;
        lamp.servos[PITCH].load = 120;
        lamp.short = Some((PITCH, 35.0));
        let report = run(
            &mut lamp,
            &mut clock,
            limits,
            Some(HOME),
            || Some((Mood::Resting, 0)),
            |_| {},
        );
        (report, lamp.servos[PITCH].position)
    };
    let (report, _) = held(Limits {
        pitch_home: (1_760, 2_040),
        pitch_window: (1_740, 2_040),
        ..Limits::FOLDED
    });
    assert!(
        matches!(report.stop, Stop::Tracking { joint: 5, .. }),
        "{:?}",
        report.stop
    );
    let (report, at) = held(Limits {
        pitch_home: (1_760, 2_040),
        pitch_window: (1_740, 2_040),
        ..waking()
    });
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.rested_short[0] >= 1, "{report:?}");
    assert!(report.woke.is_none());
    assert_eq!(at, 2_035.0);
}

/// lamp-4ace woken toward a base home 286 counts round on 2026-10-14: the
/// base stuck 34 behind its slow goal. Led and given room, it comes round.
#[test]
fn a_base_that_sticks_34_behind_a_slow_goal_is_led_round_and_settles() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(40));
    lamp.servos[YAW].position = 2_029.0;
    lamp.servos[YAW].goal = 2_029;
    lamp.short = Some((YAW, 34.0));
    let home = [2_000, 2_044, 2_315];
    let report = run(
        &mut lamp,
        &mut clock,
        waking(),
        Some(home),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let woke = report.woke.unwrap();
    assert_eq!((woke.from, woke.to), (vec![2_029], vec![2_315]));
    assert_eq!(woke.settled, vec![2_281]);
    // It runs from where it settled, and nothing pushes at the gap.
    assert_eq!(report.home, Some([2_000, 2_044, 2_281]));
    assert_eq!(lamp.servos[YAW].position, 2_281.0);
    // Its goal never went back on the way round.
    let yaw = goals_of(&lamp, Joint::BaseYaw);
    let round = yaw.iter().position(|goal| *goal == 2_315).unwrap();
    assert!(yaw[..=round].windows(2).all(|two| two[1] >= two[0]));
}

/// lamp-4ace woken the other way: as the base turned, the shoulder swayed
/// 7 counts. That is a sway, not a move; 20 is a move.
#[test]
fn an_arm_that_sways_7_counts_as_the_base_wakes_is_let_and_one_that_moves_20_is_not() {
    let swayed = |by: f32| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(40));
        lamp.servos[YAW].position = 2_029.0;
        lamp.servos[YAW].goal = 2_029;
        lamp.event = Some((Duration::from_secs(10), 1, Event::Push(by)));
        run(
            &mut lamp,
            &mut clock,
            waking(),
            Some([2_000, 2_044, 1_729]),
            || Some((Mood::Resting, 0)),
            |_| {},
        )
    };
    let report = swayed(7.0);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.woke.unwrap().settled, vec![1_729]);
    assert_eq!(
        swayed(20.0).stop,
        Stop::Drift {
            joint: 2,
            found: 2_050,
            now: 2_070
        }
    );
}

/// When, the gesture it makes, how far up it is and its base's goal.
type CalledStep = (Duration, Option<&'static str>, f32, u16);

/// A folded lamp at rest, called by its name at `calls` (seconds), with a
/// face in view from `face_from` on if given, in `mood` throughout; once
/// stood, shifting now and then at rest if `idle`: then it stays up and
/// runs until 20 s past the last call, as shifts are a minute or more apart.
fn called_run(
    look_at_name: bool,
    idle: bool,
    mood: Mood,
    calls: &'static [u64],
    face_from: Option<u64>,
) -> (BodyReport, Lamp, Vec<CalledStep>) {
    let (run_for, awake) = match calls.last() {
        Some(last) if idle => (20 + last, 3_600),
        _ => (20, 30),
    };
    let (mut lamp, mut clock) = folded(Duration::from_secs(run_for));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let limits = Limits {
        look_at_name,
        ..Limits::folded_with(Tuning {
            reach: [150, 15, 1, 250],
            speed: 120,
            servo_speed: 1_000,
            servo_acceleration: 60,
        })
        .unwrap()
    };
    let time = clock.time.clone();
    let watched = clock.time.clone();
    let mut answered = 0;
    let mut last = Duration::ZERO;
    let mut seen = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, awake).idling(idle)),
        Some(Looking::new([342, 200], [250, 120]).unwrap()),
        move || Some((mood, 0)),
        move || {
            let now = time.get();
            let called = calls
                .get(answered)
                .is_some_and(|at| now >= Duration::from_secs(*at));
            answered += usize::from(called);
            // The eyes look six times a second.
            let look = now.saturating_sub(last) >= Duration::from_millis(150);
            if look {
                last = now;
            }
            Asked {
                toward: None,
                seen: look.then(|| Seen {
                    face: face_from
                        .is_some_and(|from| now >= Duration::from_secs(from))
                        .then_some(crate::gaze::Sight {
                            x: 0.0,
                            y: 0.0,
                            size: 0.2,
                            turned: 0.0,
                        }),
                }),
                gesture: None,
                expression: None,
                called,
                still: false,
                paused: false,
                unplug: false,
                held_by_test: false,
                held: false,
                asleep: false,
                woken: false,
            }
        },
        |glimpse| {
            seen.push((
                watched.get(),
                glimpse.gesture,
                glimpse.stand,
                glimpse.goals[2],
            ))
        },
    );
    (report, lamp, seen)
}

/// Without the switch a call changes nothing: the lamp writes exactly
/// what one that was never called writes.
#[test]
fn without_the_switch_a_called_lamp_writes_exactly_as_one_never_called() {
    let (called, called_lamp, _) = called_run(false, false, Mood::Resting, &[1, 12], None);
    let (quiet, quiet_lamp, _) = called_run(false, false, Mood::Resting, &[], None);
    assert_eq!(called.calls, 0);
    assert_eq!(called_lamp.goals, quiet_lamp.goals);
    assert_eq!(called_lamp.registers, quiet_lamp.registers);
    assert_eq!(called.stop, quiet.stop);
}

/// Called with nobody in view: it rises at once, glances a little to one
/// side and the other once it stands, comes back to where it looked, and
/// a second call within ten seconds is let go.
#[test]
fn called_with_nobody_in_view_it_rises_and_glances_once() {
    let (report, _, seen) = called_run(true, false, Mood::Resting, &[1, 6], None);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!((report.calls, report.gestures), (1, 1), "{report:?}");
    let rising = seen.iter().find(|step| step.2 > 0.0).unwrap().0;
    assert!(rising < Duration::from_millis(1_100), "{rising:?}");
    let glance: Vec<_> = seen
        .iter()
        .filter(|step| step.1 == Some("called"))
        .collect();
    let (first, last) = (glance[0].0, glance[glance.len() - 1].0);
    assert!(first >= Duration::from_secs(5) && last - first <= Duration::from_millis(1_100));
    let yaw: Vec<i32> = glance
        .iter()
        .map(|step| i32::from(step.3) - 2_015)
        .collect();
    let (left, right) = (*yaw.iter().min().unwrap(), *yaw.iter().max().unwrap());
    assert!(
        (-60..=-30).contains(&left) && (30..=60).contains(&right),
        "{yaw:?}"
    );
    // Back where it looked from by the end of the run.
    assert_eq!(seen.last().unwrap().3, 2_015);
}

/// Called with somebody in view: no glance; the gaze has them. Called
/// while it speaks: nothing.
#[test]
fn called_with_a_face_in_view_it_does_not_glance_and_speaking_it_takes_no_call() {
    let (report, _, seen) = called_run(true, false, Mood::Resting, &[1], Some(0));
    assert_eq!((report.calls, report.gestures), (1, 0), "{report:?}");
    assert!(seen.iter().all(|step| step.1.is_none()));
    let (report, _, _) = called_run(true, false, Mood::Speaking, &[1], None);
    assert_eq!(report.calls, 0);
}

/// A lamp that stands and shifts now and then at rest, called again in the
/// middle of a shift: the shift gives way to the glance.
#[test]
fn a_call_is_glanced_for_over_a_shift_at_rest() {
    // When its first shift begins, called once (and again far later).
    let (_, _, seen) = called_run(true, true, Mood::Resting, &[1, 400], None);
    let shift = seen
        .iter()
        .find(|step| step.1.is_some_and(|name| name != "called"))
        .unwrap()
        .0
        .as_secs()
        + 1;
    let calls: &'static [u64] = Box::leak(vec![1, shift].into_boxed_slice());
    let (report, _, seen) = called_run(true, true, Mood::Resting, calls, None);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.idles >= 1, "{report:?}");
    assert_eq!((report.calls, report.gestures), (2, 2), "{report:?}");
    let second = seen
        .iter()
        .find(|step| step.0 >= Duration::from_secs(shift) && step.1 == Some("called"))
        .unwrap()
        .0;
    assert!(second < Duration::from_secs(shift) + Duration::from_millis(300));
}

/// As V1: a base held by a hand a second after it stopped hands over to
/// the overload cut-off, of the head and the base only (#3702): torque off
/// to those three, byte for byte, never to the shoulder or the elbow;
/// nothing written for `HAND_REST` (seconds, not V1's two minutes, which
/// are for overload); then each of the three held where it lies, goal first. Pushed again once it is up, it slumps again and the run goes on:
/// a hand never ends it. Without the option no torque is ever written and
/// the same hand ends the run as before.
#[test]
fn a_base_still_pushed_a_second_after_it_stopped_makes_the_lamp_slump_as_v1() {
    let secs = Duration::from_secs_f32;
    let ran = |give_way: Option<GiveWay>, limp: bool, cancel: f32, again: f32| {
        let (mut lamp, mut clock) = folded(secs(cancel));
        lamp.may_take_hold = true;
        // A hand turns the base 30 counts and holds it there until 3 s;
        // pushed again, without moving it, once it is up.
        lamp.hand = Some((secs(1.5), secs(3.0), YAW, 2_045.0));
        // Over V1's contact floor for the base (550): only a joint stopped
        // there may make the lamp slump.
        lamp.strains = vec![
            (secs(1.5), secs(3.0), YAW, 600),
            (secs(again), secs(again + 2.0), YAW, 600),
        ];
        let limits = Limits {
            give_way,
            limp,
            ..giving_way(GiveWay::Return)
        };
        let report = run(
            &mut lamp,
            &mut clock,
            limits,
            None,
            || Some((Mood::Resting, 0)),
            |_| {},
        );
        (lamp, report)
    };

    let (lamp, report) = ran(Some(GiveWay::Return), true, 40.0, 20.0);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.released.len(), 2, "{:?}", report.released);
    assert_eq!(report.released[0].joint, "base_yaw");
    assert_eq!(report.released_by_hand, [0, 0, 2]);
    // The first: a second after it stopped (about 1.62 s), torque off to
    // the head and the base, nothing written for `HAND_REST`, then each
    // taken hold of where it lies, its goal first. The arm is never
    // written torque, off or on.
    let tripped = secs(report.released[0].seconds);
    assert!((secs(2.5)..secs(2.8)).contains(&tripped), "{tripped:?}");
    let off: Vec<(u8, u8, Vec<u8>)> = COMMANDED.iter().map(|j| (j.id(), 40, vec![0])).collect();
    let at = lamp
        .registers
        .iter()
        .position(|(_, address, data)| *address == 40 && data == &vec![0])
        .unwrap();
    assert_eq!(lamp.registers[at..at + 3], off[..]);
    assert!(
        lamp.registers
            .iter()
            .all(|(id, address, _)| *address != 40 || ARM.iter().all(|j| j.id() != *id))
    );
    let rest = tripped..tripped + HAND_REST;
    assert!(lamp.goal_times.iter().all(|t| !rest.contains(t)));
    // And no longer: taken hold of within a second of it.
    assert!(
        lamp.register_times[at + 3..]
            .iter()
            .any(|t| *t < tripped + HAND_REST + secs(1.0)),
        "{:?}",
        &lamp.register_times[at + 3..]
    );
    assert!(
        lamp.register_times[at + 3..]
            .iter()
            .all(|t| !rest.contains(t))
    );
    assert_eq!(
        lamp.registers[at + 7],
        (1, 42, 2_045u16.to_le_bytes().to_vec())
    );
    assert_eq!(lamp.registers[at + 8], (1, 40, vec![1]));
    // The second, from the push once it was up.
    assert!(report.released[1].seconds > 20.0, "{:?}", report.released);

    // Pushed while it is brought up again: it lets go again at once.
    let (_, report) = ran(Some(GiveWay::Return), true, 40.0, 8.0);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.released.len(), 2, "{:?}", report.released);
    assert!(report.released[1].seconds < 11.0, "{:?}", report.released);

    // Without `--limp`, `--give-way return` as it was: the same hands are
    // given way to, the run goes on, never a torque write, and every goal
    // written is the one `--give-way return` alone writes.
    let (with_limp_off, report) = ran(Some(GiveWay::Return), false, 40.0, 20.0);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.released.is_empty(), "{:?}", report.released);
    assert!(
        with_limp_off
            .registers
            .iter()
            .all(|(_, address, _)| *address != 40)
    );
    let (mut alone, mut clock) = folded(secs(40.0));
    alone.may_take_hold = true;
    alone.hand = Some((secs(1.5), secs(3.0), YAW, 2_045.0));
    alone.strains = vec![
        (secs(1.5), secs(3.0), YAW, 600),
        (secs(20.0), secs(22.0), YAW, 600),
    ];
    let baseline = run(
        &mut alone,
        &mut clock,
        giving_way(GiveWay::Return),
        None,
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(baseline.stop, report.stop);
    assert_eq!(alone.goals, with_limp_off.goals);
    assert_eq!(alone.registers, with_limp_off.registers);

    // Without either option the same hand ends the run, as before.
    let (lamp, report) = ran(None, false, 8.0, 20.0);
    assert!(
        matches!(report.stop, Stop::Load { joint: 1, .. }),
        "{report:?}"
    );
    // Torque is written only as the run ends: off, on all five.
    assert!(let_go_of_the_whole_lamp(&lamp), "{:?}", lamp.registers);
    assert_eq!(
        lamp.registers
            .iter()
            .filter(|(_, address, _)| *address == 40)
            .count(),
        5
    );
}

/// A slump from a hand is not an overload: the first true overload after
/// it rests two minutes and is taken up again, rather than being counted
/// as the second within ten minutes, which would end the run with torque
/// left off.
#[test]
fn a_hand_slump_then_an_overload_rests_and_comes_back_instead_of_ending_the_run() {
    let secs = Duration::from_secs_f32;
    let (mut lamp, mut clock) = folded(secs(400.0));
    lamp.may_take_hold = true;
    lamp.hand = Some((secs(1.5), secs(3.0), YAW, 2_045.0));
    lamp.strains = vec![
        (secs(1.5), secs(3.0), YAW, 600),
        // Then the shoulder, which no hand rule watches, at 900 for 3 s.
        (secs(150.0), secs(153.0), 1, 900),
    ];
    let limits = Limits {
        limp: true,
        overload_release: true,
        ..giving_way(GiveWay::Return)
    };
    let report = run(
        &mut lamp,
        &mut clock,
        limits,
        None,
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.released.len(), 2, "{:?}", report.released);
    assert_eq!(report.released_by_hand, [0, 0, 1]);
    assert_eq!(report.released[1].joint, "base_pitch");
    // Taken hold of again two minutes after the overload: torque on again.
    let overload = secs(report.released[1].seconds);
    assert!(
        lamp.register_times
            .iter()
            .zip(&lamp.registers)
            .any(|(t, (_, address, data))| *t > overload + OVERLOAD_REST
                && *address == 40
                && data == &vec![1]),
        "{:?}",
        report.released
    );
}

/// As lamp-52e6 on 2026-10-15: after a slump the head lies low and lifts
/// itself home heavily (37 counts short at a load of 144). That is not a
/// hand: until the lamp has settled only V1's contact floor tells a hand,
/// so it does not slump again with nobody touching it.
#[test]
fn a_head_lifting_itself_after_a_slump_does_not_make_the_lamp_slump_again() {
    let secs = Duration::from_secs_f32;
    let (mut lamp, mut clock) = folded(secs(50.0));
    lamp.may_take_hold = true;
    lamp.hand = Some((secs(1.5), secs(3.0), YAW, 2_045.0));
    lamp.strains = vec![(secs(1.5), secs(3.0), YAW, 600)];
    // Slumped, the head sinks 60 counts.
    lamp.event = Some((secs(2.9), PITCH, Event::Push(60.0)));
    lamp.heavy = Some((PITCH, 1_850.0, 37.0, 144));
    let time = clock.time.clone();
    let news = move || {
        Some(if time.get() < secs(15.0) {
            (Mood::Resting, 0)
        } else {
            (Mood::Listening, 100)
        })
    };
    let limits = Limits {
        limp: true,
        ..giving_way(GiveWay::Return)
    };
    let report = run(&mut lamp, &mut clock, limits, None, news, |_| {});
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.released.len(), 1, "{:?}", report.released);
}

/// After a slump a hand may leave the base far off. With `--limp on` the
/// next start takes hold of it where it lies, as V1 did, and runs from
/// there; without, it is refused as before.
#[test]
fn a_base_found_far_from_home_is_taken_hold_of_where_it_lies_only_with_limp() {
    let ran = |limp: bool| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(3));
        let far = f32::from(HOME[2]) + 545.0;
        (lamp.servos[YAW].position, lamp.servos[YAW].goal) = (far, far as u16);
        let limits = Limits {
            give_way: Some(GiveWay::Return),
            limp,
            ..waking()
        };
        run(
            &mut lamp,
            &mut clock,
            limits,
            Some(HOME),
            || Some((Mood::Resting, 0)),
            |_| {},
        )
    };
    let report = ran(true);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.home.unwrap()[2], HOME[2] + 545);
    let report = ran(false);
    assert!(
        matches!(&report.stop, Stop::Refused { why } if why.contains("more than waking brings back")),
        "{report:?}"
    );
}

/// A push under a second gives way and comes back with its torque on;
/// V1's contact floor stops a joint after three steps, before its own
/// bound's six; the shoulder and the elbow are never let go by a hand.
#[test]
fn a_short_push_a_contact_floor_and_the_arm_under_a_hand() {
    let ms = Duration::from_millis;
    for mode in [GiveWay::Stay, GiveWay::Return] {
        let (mut lamp, mut clock) = folded(Duration::from_secs(6));
        lamp.may_take_hold = true;
        lamp.hand = Some((ms(1_500), ms(2_200), YAW, 2_045.0));
        lamp.strains = vec![(ms(1_500), ms(2_200), YAW, 300)];
        let report = run(
            &mut lamp,
            &mut clock,
            Limits {
                limp: true,
                ..giving_way(mode)
            },
            None,
            || Some((Mood::Resting, 0)),
            |_| {},
        );
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        assert_eq!((report.eased, report.released_by_hand), ([0, 0, 1], [0; 3]));
        assert!(lamp.registers.iter().all(|(_, address, _)| *address != 40));
        assert!((lamp.servos[YAW].position.round() as u16).abs_diff(HOME[2]) <= 2);
    }

    // 700 on the head's pitch for 80 ms: over V1's 650 for three steps, so
    // it gives way; its own bound (240) would want six.
    let contact = |give_way: Option<GiveWay>| {
        let (mut lamp, mut clock) = folded(Duration::from_secs(4));
        lamp.strains = vec![(ms(1_500), ms(1_580), PITCH, 700)];
        let limits = Limits {
            give_way,
            limp: give_way.is_some(),
            ..giving_way(GiveWay::Return)
        };
        let report = run(
            &mut lamp,
            &mut clock,
            limits,
            None,
            || Some((Mood::Resting, 0)),
            |_| {},
        );
        report.eased
    };
    assert_eq!(contact(Some(GiveWay::Return)), [1, 0, 0]);
    assert_eq!(contact(None), [0; 3]);

    // A hand on the elbow at 900 for two seconds: never let go.
    let (mut lamp, mut clock) = folded(Duration::from_secs(5));
    lamp.may_take_hold = true;
    lamp.hand = Some((ms(1_000), ms(3_000), 2, 2_050.0));
    lamp.strains = vec![(ms(1_000), ms(3_000), 2, 900)];
    let report = run(
        &mut lamp,
        &mut clock,
        Limits {
            limp: true,
            ..giving_way(GiveWay::Stay)
        },
        None,
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.released_by_hand, [0; 3]);
    assert!(lamp.registers.iter().all(|(_, address, _)| *address != 40));
}

/// A standing shoulder resting short of its goal under its own weight stops
/// pushing once and is counted once; each later goal that moves a little and
/// stops it at the same place again is held again (it still stops pushing,
/// which an overload rule relies on) but not counted. A hand that takes it
/// elsewhere and stops it there is counted, and so is a stall at the old
/// place after it has moved freely away for a while.
#[test]
fn a_shoulder_resting_short_at_the_same_place_is_counted_once() {
    let rest = Sample {
        position: 2_629,
        load: 124,
        temperature: 40,
        status: 0,
    };
    // (holds, counted) over one stall's worth of steps.
    let stop = |stall: &mut Stall, goal: u16, is: Sample| {
        let mut held = (0, 0);
        for _ in 0..=STALL_STEPS {
            if stall.step(goal, is, goal) {
                held.0 += 1;
                held.1 += usize::from(!stall.again);
            }
        }
        held
    };
    let mut stall = Stall::default();
    assert_eq!(stop(&mut stall, 2_659, rest), (1, 1));
    // A scene moves the goal 25 counts: let go, the same rest again: held,
    // so it stops pushing, but not counted.
    assert!(stall.holding(2_684).is_none());
    assert_eq!(stop(&mut stall, 2_684, rest), (1, 0));
    // It moves freely to 2,750 for 100 steps, unloaded, then stalls at the
    // old 2,629 again at a load of 500: a new stall, counted.
    assert!(stall.holding(2_800).is_none());
    let free = Sample {
        position: 2_750,
        load: 20,
        ..rest
    };
    for _ in 0..100 {
        assert!(!stall.step(2_750, free, 2_750));
    }
    let pushed = Sample { load: 500, ..rest };
    assert_eq!(stop(&mut stall, 2_659, pushed), (1, 1));
    // A hand lifts it 70 counts and holds it there: counted.
    assert!(stall.holding(2_720).is_none());
    let lifted = Sample {
        position: 2_699,
        ..rest
    };
    assert_eq!(stop(&mut stall, 2_740, lifted), (1, 1));
}

#[test]
fn a_standing_arm_carrying_its_own_weight_counts_no_stall_and_a_push_well_over_it_counts_one() {
    // lamp-4ace's shoulder at rest: 30 counts short of its goal at load 112.
    let mut stall = Stall {
        carries: Some(112),
        ..Stall::default()
    };
    let standing = Sample {
        position: 2_000,
        load: 112,
        temperature: 40,
        status: 0,
    };
    // An hour of steps, the goal moved a little now and then as a scene does.
    for step in 0..180_000u32 {
        let goal = 2_030 + (step / 3_000 % 5) as u16 * 10;
        assert!(!stall.step(goal, standing, goal), "step {step}");
    }
    // A hand holding it at a load well over what it stands with.
    let pushed = Sample {
        load: 400,
        ..standing
    };
    let counted = (0..u32::from(STALL_STEPS))
        .filter(|_| stall.step(2_030, pushed, 2_030) && !stall.again)
        .count();
    assert_eq!(counted, 1);
    // The head keeps 100: the same push of 112 counts there.
    let mut head = Stall::default();
    let counted = (0..u32::from(STALL_STEPS))
        .filter(|_| head.step(2_030, standing, 2_030) && !head.again)
        .count();
    assert_eq!(counted, 1);
}

/// What a run with a gesture asked from outside said of it, and each step:
/// when, how far up it stood, the gesture being made.
type Fates = (
    BodyReport,
    Vec<(&'static str, &'static str)>,
    Vec<(Duration, f32, Option<&'static str>)>,
);

/// A folded lamp that may stand, asked by `ask` at each step, in the mood
/// `news` says. `prepare` sets up the servos.
fn skill_run(
    seconds: u64,
    up: Standing,
    limits: Limits,
    prepare: impl FnOnce(&mut Lamp),
    news: impl FnMut() -> Option<(Mood, u8)>,
    mut ask: impl FnMut(Duration) -> Asked,
) -> Fates {
    let (mut lamp, mut clock) = folded(Duration::from_secs(seconds));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    prepare(&mut lamp);
    let time = clock.time.clone();
    let seen_at = clock.time.clone();
    let mut fates = Vec::new();
    let mut steps = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(up),
        None,
        news,
        move || ask(time.get()),
        |glimpse| {
            fates.extend(glimpse.gesture_fates);
            steps.push((seen_at.get(), glimpse.stand, glimpse.gesture));
        },
    );
    (report, fates, steps)
}

/// A gesture asked once, at `at`.
fn once(at: Duration, paused: bool) -> impl FnMut(Duration) -> Asked {
    let mut asked = false;
    move |now| {
        let due = !asked && now >= at;
        asked |= due;
        Asked {
            gesture: due.then_some(&crate::clips::NOD),
            ..if paused {
                Asked::PAUSED
            } else {
                Asked::default()
            }
        }
    }
}

fn gesture_limits() -> Limits {
    Limits::folded_with(Tuning {
        reach: [150, 15, 1, 250],
        speed: 120,
        servo_speed: 1_000,
        servo_acceleration: 60,
    })
    .unwrap()
}

/// A skill's gesture asked of a folded lamp at rest (lamp-4ace, 15:16 on
/// 2026-10-16: `stretching` asked at `stand 0.00`, and nothing): it stands
/// up for it as for any mood but rest, then plays it, and says so once.
#[test]
fn a_gesture_asked_of_a_folded_lamp_stands_it_up_and_then_plays() {
    let (report, fates, steps) = skill_run(
        14,
        standing(100, 5),
        gesture_limits(),
        |_| {},
        || Some((Mood::Resting, 0)),
        once(Duration::from_secs(1), false),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.stood);
    assert_eq!(report.gestures, 1, "{report:?}");
    assert_eq!(fates, [("nod", "taken: it was folded and stood up for it")]);
    // Folded when asked; it played only once it stood at its full height.
    let asked = steps
        .iter()
        .rfind(|s| s.0 < Duration::from_secs(1))
        .unwrap();
    assert_eq!(asked.1, 0.0);
    let began = steps.iter().find(|s| s.2.is_some()).unwrap();
    assert_eq!(began.1, 1.0);
    assert!(began.0 > Duration::from_secs(4), "{:?}", began.0);
    assert!(steps.iter().filter(|s| s.1 < 1.0).all(|s| s.2.is_none()));
}

/// Paused (`lamp/pause`), the same gesture is dropped and said, and the
/// lamp stays folded.
#[test]
fn a_paused_lamp_drops_a_gesture_and_says_so() {
    let (report, fates, steps) = skill_run(
        8,
        standing(100, 5),
        gesture_limits(),
        |_| {},
        || Some((Mood::Resting, 0)),
        once(Duration::from_secs(1), true),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(!report.stood);
    assert_eq!(report.gestures, 0);
    assert_eq!(fates, [("nod", "dropped: paused")]);
    assert!(steps.iter().all(|s| s.1 == 0.0 && s.2.is_none()));
}

/// A lamp a hand put to sleep (#1362) is not stood up for a gesture: it is
/// dropped and said as asleep.
#[test]
fn a_lamp_put_to_sleep_drops_a_gesture_and_says_so() {
    let mut asked = false;
    let (report, fates, steps) = skill_run(
        8,
        standing(100, 5),
        gesture_limits(),
        |_| {},
        || Some((Mood::Resting, 0)),
        move |now| {
            let due = !asked && now >= Duration::from_secs(1);
            asked |= due;
            Asked {
                gesture: due.then_some(&crate::clips::NOD),
                ..Asked::ASLEEP
            }
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(!report.stood);
    assert_eq!(report.gestures, 0);
    assert_eq!(fates, [("nod", "dropped: asleep")]);
    assert!(steps.iter().all(|s| s.1 == 0.0 && s.2.is_none()));
}

/// Nor does a standing lamp a hand put to sleep nod as it folds: a gesture
/// asked then is dropped as it is asked, never begun.
#[test]
fn a_standing_lamp_put_to_sleep_drops_a_gesture_as_it_folds() {
    let mut steps_heard = 0u32;
    let mut asked = false;
    let (report, fates, steps) = skill_run(
        14,
        standing(100, 90),
        gesture_limits(),
        |_| {},
        // Spoken to for six seconds: it stands.
        move || {
            steps_heard += 1;
            Some(if TICK * steps_heard < Duration::from_secs(6) {
                (Mood::Listening, 0)
            } else {
                (Mood::Resting, 0)
            })
        },
        // Asleep from eight seconds; a nod asked half a second later.
        move |now| {
            if now < Duration::from_secs(8) {
                return Asked::default();
            }
            let due = !asked && now >= Duration::from_millis(8_500);
            asked |= due;
            Asked {
                gesture: due.then_some(&crate::clips::NOD),
                ..Asked::ASLEEP
            }
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.stood);
    assert_eq!(report.gestures, 0);
    assert_eq!(fates, [("nod", "dropped: asleep")]);
    assert!(steps.iter().all(|s| s.2.is_none()));
    let at = |seconds: u64| {
        steps
            .iter()
            .rfind(|step| step.0 <= Duration::from_secs(seconds))
            .unwrap()
            .1
    };
    assert_eq!(at(7), 1.0);
    assert_eq!(at(13), 0.0);
}

/// Woken by a tap (#1362), a lamp a hand put to sleep stands up at once,
/// the lift, though the voice has said no mood yet.
#[test]
fn woken_after_a_hands_sleep_it_stands_at_once() {
    let mut steps_heard = 0u32;
    let mut woken = false;
    let (report, _, steps) = skill_run(
        24,
        standing(100, 90),
        gesture_limits(),
        |_| {},
        // Spoken to for six seconds, and never again.
        move || {
            steps_heard += 1;
            Some(if TICK * steps_heard < Duration::from_secs(6) {
                (Mood::Listening, 0)
            } else {
                (Mood::Resting, 0)
            })
        },
        // Asleep from eight seconds to fourteen, then woken.
        move |now| {
            if (Duration::from_secs(8)..Duration::from_secs(14)).contains(&now) {
                return Asked::ASLEEP;
            }
            let wakes = !woken && now >= Duration::from_secs(14);
            woken |= wakes;
            Asked {
                woken: wakes,
                ..Asked::default()
            }
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let at = |at: Duration| steps.iter().rfind(|step| step.0 <= at).unwrap().1;
    let secs = Duration::from_secs;
    assert_eq!(at(secs(7)), 1.0);
    assert_eq!(at(secs(13)), 0.0);
    // Rising within a fifth of a second of the wake, and up by six.
    assert!(at(secs(14) + Duration::from_millis(200)) > 0.0);
    assert_eq!(at(secs(20)), 1.0);
    assert_eq!(at(secs(23)), 1.0);
}

/// A gesture a folded lamp was rising for when a hand put it to sleep is
/// dropped then, and the lamp goes back down.
#[test]
fn a_gesture_waiting_when_the_lamp_is_put_to_sleep_is_dropped() {
    let mut asked = false;
    let (report, fates, steps) = skill_run(
        12,
        standing(100, 5),
        gesture_limits(),
        |_| {},
        || Some((Mood::Resting, 0)),
        move |now| {
            if now >= Duration::from_millis(1_500) {
                return Asked::ASLEEP;
            }
            let due = !asked && now >= Duration::from_secs(1);
            asked |= due;
            Asked {
                gesture: due.then_some(&crate::clips::NOD),
                ..Asked::default()
            }
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.gestures, 0);
    assert_eq!(fates, [("nod", "dropped: asleep")]);
    assert!(steps.iter().all(|s| s.2.is_none()));
    assert_eq!(steps.last().unwrap().1, 0.0);
}

/// Held by a hand where it gave way, a folded lamp that may stand is asked
/// nothing new: it is not stood up, and the gesture is dropped and said.
#[test]
fn a_lamp_held_by_a_hand_drops_a_gesture_and_says_so() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.hand = Some((ms(1_500), ms(4_500), YAW, f32::from(HOME[2]) + 40.0));
    lamp.strains = vec![(ms(1_500), ms(4_500), YAW, 120)];
    let held = Rc::new(Cell::new(false));
    let told = held.clone();
    let mut asked = false;
    let mut fates = Vec::new();
    let mut stood = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        giving_way(GiveWay::Return),
        Some(HOME),
        Some(standing(100, 5)),
        None,
        || Some((Mood::Resting, 0)),
        move || {
            let due = !asked && held.get();
            asked |= due;
            Asked {
                gesture: due.then_some(&crate::clips::NOD),
                ..Asked::default()
            }
        },
        |glimpse| {
            told.set(glimpse.hand);
            fates.extend(glimpse.gesture_fates);
            stood.push(glimpse.stand);
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.gestures, 0);
    assert_eq!(fates, [("nod", "dropped: a hand holds it")]);
    assert!(!report.stood);
    assert!(stood.iter().all(|stand| *stand == 0.0));
}

/// A lamp that let go under overload is not stood up by a gesture: while
/// it lies limp, and once taken up again where it slumped, each is dropped
/// and said.
#[test]
fn a_slumped_lamp_is_not_stood_up_by_a_gesture() {
    let secs = Duration::from_secs;
    let mut asked = [false; 2];
    let (report, fates, steps) = skill_run(
        135,
        standing(100, 5),
        Limits {
            force: Some(600),
            ..releasing()
        },
        |lamp| {
            lamp.may_take_hold = true;
            lamp.strains = vec![(secs(1), secs(3), 2, 500)];
        },
        || Some((Mood::Resting, 0)),
        move |now| {
            let at = [secs(10), secs(130)];
            let due = (0..2).find(|i| !asked[*i] && now >= at[*i]);
            if let Some(i) = due {
                asked[i] = true;
            }
            Asked {
                gesture: due.map(|_| &crate::clips::NOD),
                ..Asked::default()
            }
        },
    );
    assert_eq!(report.released.len(), 1, "{report:?}");
    assert_eq!(report.gestures, 0);
    assert_eq!(
        fates,
        [
            ("nod", "dropped: it has let go and lies limp"),
            ("nod", "dropped: it let go and lies where it slumped"),
        ]
    );
    assert!(steps.iter().all(|s| s.1 == 0.0 && s.2.is_none()));
}

/// A gesture asked just before the lamp would fold keeps it up for the
/// whole of it (fifth's review of #1080): spoken to until five seconds,
/// awake five more, a nod asked at 9.5 s plays to its end, and then the
/// lamp folds.
#[test]
fn a_gesture_asked_as_the_lamp_would_fold_keeps_it_up_until_it_ends() {
    let (report, fates, steps) = skill_run(
        24,
        standing(100, 5),
        gesture_limits(),
        |_| {},
        {
            let mut step = 0u32;
            move || {
                step += 1;
                Some(if step * 20 < 5_000 {
                    (Mood::Listening, 100)
                } else {
                    (Mood::Resting, 0)
                })
            }
        },
        once(Duration::from_millis(9_500), false),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(fates, [("nod", "taken: it plays")]);
    let during: Vec<_> = steps.iter().filter(|s| s.2 == Some("nod")).collect();
    let lasted = during.last().unwrap().0 - during[0].0;
    let clip = Duration::from_micros(crate::clips::NOD.duration_us());
    assert!(lasted + Duration::from_millis(100) >= clip, "{lasted:?}");
    assert!(during.iter().all(|s| s.1 == 1.0));
    // And it folds afterwards.
    assert_eq!(steps.last().unwrap().1, 0.0);
}

/// Paused after a gesture was taken, before it began (the reviewer's probe
/// on #1080): the gesture is dropped and said, the window it opened is
/// given back, and the lamp folds instead of rising and playing it.
#[test]
fn a_pause_after_a_gesture_was_taken_drops_it_and_the_lamp_folds() {
    let mut asked = false;
    let (report, fates, steps) = skill_run(
        16,
        standing(100, 5),
        gesture_limits(),
        |_| {},
        || Some((Mood::Resting, 0)),
        move |now| {
            if now >= Duration::from_millis(1_500) {
                return Asked::PAUSED;
            }
            let due = !asked && now >= Duration::from_secs(1);
            asked |= due;
            Asked {
                gesture: due.then_some(&crate::clips::NOD),
                ..Asked::default()
            }
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.gestures, 0, "{report:?}");
    assert_eq!(fates, [("nod", "dropped: paused")]);
    assert!(steps.iter().all(|s| s.2.is_none()));
    // It began to rise for half a second, and is folded again well before
    // the five seconds' window would have run out.
    assert!(
        steps
            .iter()
            .filter(|s| s.0 >= Duration::from_secs(4))
            .all(|s| s.1 == 0.0)
    );
}

/// Two gestures in a row while the lamp rises for the first (the
/// reviewer's probe 1 on #1080): the second waits for the rise to end, and
/// plays.
#[test]
fn a_second_gesture_asked_while_the_lamp_rises_for_the_first_plays() {
    let mut asked = [false; 2];
    let (report, fates, steps) = skill_run(
        14,
        standing(100, 5),
        gesture_limits(),
        |_| {},
        || Some((Mood::Resting, 0)),
        move |now| {
            let at = [Duration::from_secs(1), Duration::from_millis(1_500)];
            let due = (0..2).find(|i| !asked[*i] && now >= at[*i]);
            if let Some(i) = due {
                asked[i] = true;
            }
            Asked {
                gesture: due.map(|i| [&crate::clips::NOD, &crate::clips::SHY][i]),
                ..Asked::default()
            }
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.gestures, 1, "{report:?}");
    assert_eq!(
        fates,
        [
            ("nod", "dropped: another was asked after it"),
            ("shy", "taken: it was folded and stood up for it"),
        ]
    );
    assert!(steps.iter().any(|s| s.2 == Some("shy")));
}

/// A gesture asked just after a spoken turn begins, while the lamp rises
/// for the mood (the reviewer's probe 2 on #1080): it waits for the rise
/// to end, and plays.
#[test]
fn a_gesture_asked_while_the_lamp_rises_for_a_turn_plays() {
    let (report, fates, steps) = skill_run(
        14,
        standing(100, 5),
        gesture_limits(),
        |_| {},
        {
            let mut step = 0u32;
            move || {
                step += 1;
                Some(if step * 20 >= 1_000 {
                    (Mood::Listening, 100)
                } else {
                    (Mood::Resting, 0)
                })
            }
        },
        once(Duration::from_millis(1_500), false),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.gestures, 1, "{report:?}");
    assert_eq!(fates, [("nod", "taken: it plays")]);
    assert!(steps.iter().any(|s| s.2 == Some("nod")));
}

/// A lamp that never stands at its full height (`--stand` at half) drops
/// a gesture it woke for once it has waited, and says it went stale.
#[test]
fn a_gesture_the_lamp_never_stands_up_for_goes_stale_and_is_said() {
    let (report, fates, steps) = skill_run(
        14,
        standing(50, 5),
        gesture_limits(),
        |_| {},
        || Some((Mood::Resting, 0)),
        once(Duration::from_secs(1), false),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.stood);
    assert_eq!(report.gestures, 0, "{report:?}");
    assert_eq!(
        fates,
        [(
            "nod",
            "dropped: stale, it was not standing 3 s after it could have been"
        )]
    );
    assert!(steps.iter().all(|s| s.2.is_none()));
}

/// Why a face is not looked at, each step, for a body with eyes: folded,
/// neither listening nor speaking, or paused; nothing while it would be.
#[test]
fn a_body_with_eyes_says_why_it_does_not_look() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(16));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let time = clock.time.clone();
    let watched = clock.time.clone();
    let mut said: Vec<(Duration, Option<&'static str>)> = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 5)),
        Some(Looking::new([342, 200], [250, 120]).unwrap()),
        spoken_to(&time),
        {
            let time = time.clone();
            move || {
                if time.get() >= Duration::from_secs(14) {
                    Asked::PAUSED
                } else {
                    Asked {
                        seen: Some(Seen { face: None }),
                        ..Asked::default()
                    }
                }
            }
        },
        |glimpse| said.push((watched.get(), glimpse.looks_dropped)),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let at = |millis: u64| {
        said.iter()
            .rfind(|step| step.0 <= Duration::from_millis(millis))
            .unwrap()
            .1
    };
    assert_eq!(at(800), Some("folded or folding"));
    assert_eq!(at(2_000), None);
    assert_eq!(at(3_500), Some("neither listening nor speaking"));
    assert_eq!(at(5_000), None);
    assert_eq!(at(9_000), Some("neither listening nor speaking"));
    assert_eq!(at(15_000), Some("paused"));
}

/// Paused once the gesture it stood up for plays (the reviewer's 21:50 on
/// #1080): the gesture plays to its end, and the lamp then folds instead of
/// staying up, paused, for the rest of a 90 s awake window.
#[test]
fn a_pause_while_the_gesture_it_stood_up_for_plays_lets_it_end_and_folds() {
    let pause_at = Duration::from_secs(6);
    let mut asked = false;
    let (report, fates, steps) = skill_run(
        30,
        standing(100, 90),
        gesture_limits(),
        |_| {},
        || Some((Mood::Resting, 0)),
        move |now| {
            if now >= pause_at {
                return Asked::PAUSED;
            }
            let due = !asked && now >= Duration::from_secs(1);
            asked |= due;
            Asked {
                gesture: due.then_some(&crate::clips::NOD),
                ..Asked::default()
            }
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.gestures, 1, "{report:?}");
    assert_eq!(fates, [("nod", "taken: it was folded and stood up for it")]);
    let nod: Vec<_> = steps.iter().filter(|s| s.2 == Some("nod")).collect();
    assert!(nod[0].0 < pause_at, "{:?}", nod[0].0);
    let lasted = nod.last().unwrap().0 - nod[0].0;
    let clip = Duration::from_micros(crate::clips::NOD.duration_us());
    assert!(lasted + Duration::from_millis(100) >= clip, "{lasted:?}");
    assert!(nod.iter().all(|s| s.1 == 1.0));
    assert!(
        steps
            .iter()
            .filter(|s| s.0 >= Duration::from_secs(20))
            .all(|s| s.1 == 0.0)
    );
}

/// The body's own moves are never journalled as a gesture taken or dropped.
#[test]
fn shifts_at_rest_and_the_call_are_not_from_outside() {
    assert!(idle::MOVES.iter().all(|clip| !from_outside(clip)));
    assert!(!from_outside(&clips::CALLED));
    assert!(from_outside(&clips::NOD));
}

/// A second gesture taken after the first has ended (fifth's P3 on #1080):
/// a pause while it plays still gives back the window from before both, so
/// the lamp folds once it ends instead of staying up for the nod's window.
#[test]
fn a_pause_during_a_second_gesture_after_the_first_ended_still_folds() {
    let mut asked = (false, false);
    let (report, fates, steps) = skill_run(
        30,
        standing(100, 90),
        gesture_limits(),
        |_| {},
        || Some((Mood::Resting, 0)),
        move |now| {
            if now >= Duration::from_secs(14) {
                return Asked::PAUSED;
            }
            let nod = !asked.0 && now >= Duration::from_secs(1);
            asked.0 |= nod;
            let shy = !asked.1 && now >= Duration::from_secs(11);
            asked.1 |= shy;
            Asked {
                gesture: if nod {
                    Some(&crate::clips::NOD)
                } else {
                    shy.then_some(&crate::clips::SHY)
                },
                ..Asked::default()
            }
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.gestures, 2, "{report:?}");
    assert_eq!(
        fates,
        [
            ("nod", "taken: it was folded and stood up for it"),
            ("shy", "taken: it plays"),
        ]
    );
    let shy: Vec<_> = steps.iter().filter(|s| s.2 == Some("shy")).collect();
    assert!(
        shy.iter().any(|s| s.0 >= Duration::from_secs(14)),
        "{shy:?}"
    );
    assert!(
        steps
            .iter()
            .filter(|s| s.0 >= Duration::from_secs(22))
            .all(|s| s.1 == 0.0)
    );
}

/// lamp-4ace's standing pose and folded arm (`deploy/lamp-4ace/lamp-body.service`).
fn lamp_4ace_standing() -> Standing {
    Standing::new(
        [2_048, 2_047],
        [1_772, 2_043, 2_015, 2_657, 2_602],
        100,
        6,
        90,
        [950, 950],
    )
    .unwrap()
}

/// lamp-4ace, 2026-10-17 01:58: the elbow's goal register held 37,639 at
/// start. The body took it as the arm's goal, the anti-tip cut clamped it
/// to 4,095 and wrote it, and the elbow was driven past the seam. A
/// register that is not a count near where the joint is found is not
/// taken: the goal is where the joint is.
#[test]
fn a_goal_register_far_from_the_arm_is_not_taken_as_its_goal() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(1));
    lamp.arm_moves = true;
    // Its elbow without the old flag, and its goal register far off.
    lamp.servos[2].status = 0;
    lamp.servos[2].goal = 4_200;
    // The head a little under home, so that it steps up to it.
    lamp.servos[PITCH].position = 1_980.0;
    lamp.servos[PITCH].goal = 1_980;
    let report = run_alive(
        &mut lamp,
        &mut clock,
        gesture_limits(),
        Some(HOME),
        Some(lamp_4ace_standing()),
        None,
        || Some((Mood::Resting, 0)),
        Asked::default,
        |_| {},
    );
    let elbow: Vec<u16> = lamp
        .goals
        .iter()
        .flatten()
        .filter(|(id, _)| *id == Joint::ElbowPitch.id())
        .map(|(_, goal)| *goal)
        .collect();
    // Its first goal is where it is, not the register.
    assert_eq!(elbow.first(), Some(&2_047), "{report:?}");
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(
        elbow.iter().all(|goal| (1_627..=3_022).contains(goal)),
        "{elbow:?}"
    );
}

/// The guard where every arm goal goes to the wire: a goal outside its
/// joint's window, or more than a step from the last, is refused before it
/// is written, and says which joint, which goal and which window.
#[test]
fn an_arm_goal_outside_its_window_never_reaches_the_wire() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(1));
    lamp.servos[2].status = 0;
    {
        let mut bus = Bus::new(&mut lamp, &mut clock, Some(lamp_4ace_standing()));
        bus.samples().unwrap();
        // Where it is, and a step from there: written.
        bus.send_goals(HOME, Some([2_050, 2_047])).unwrap();
        bus.send_goals(HOME, Some([2_050, 2_090])).unwrap();
        // The seam, as lamp-4ace's elbow was sent: refused.
        assert!(bus.send_goals(HOME, Some([2_050, 4_095])).is_err());
        assert_eq!(
            bus.refused.take(),
            Some(Stop::Window {
                joint: 3,
                goal: 4_095,
                window: (1_627, 3_022),
                from: 2_090,
            })
        );
        // Inside the window, but a jump of 310 counts in one step, 353 from
        // where the elbow is: refused.
        assert!(bus.send_goals(HOME, Some([2_050, 2_400])).is_err());
        assert!(matches!(
            bus.refused.take(),
            Some(Stop::Window {
                joint: 3,
                goal: 2_400,
                ..
            })
        ));
    }
    let elbow: Vec<u16> = lamp
        .goals
        .iter()
        .flatten()
        .filter(|(id, _)| *id == Joint::ElbowPitch.id())
        .map(|(_, goal)| *goal)
        .collect();
    assert_eq!(elbow, [2_047, 2_090]);
}

/// lamp-4ace at 01:58:42 on 2026-10-17 (#705, the 02:05 P1): folded, at
/// rest, its joints found at [2028, 2054, 2049, 2047, 1948], started as its
/// unit starts it (`--stand 1772,2043,2015,2657,2602 --folded-arm
/// 2048,2047 --stand-seconds 6 --awake-seconds 90 --idle on --wake on
/// --rest-glance on --look-at-name on --home 1950,2043,2015`), with the
/// elbow's goal register (42) reading 37639 (0x9307, as its journal
/// showed) while the elbow sits at 2049. The body takes that register as the arm's last goal; folded, it is
/// carried as the arm's goal; V1's anti-tip rule sees the lamp 47 mm back
/// and cuts the head's first step home, and the cut's rounding clamps the
/// elbow to 4095 and writes it: the lamp's own report, `cuts 1 ... 47.1 mm`,
/// `stop: tracking, joint 3, goal 4095`. No goal outside a joint's own
/// window (0..=4095, and here the way between folded and stood) may be
/// written, whatever a register reads.
#[test]
fn an_elbow_goal_register_past_its_range_is_never_written_as_a_goal() {
    for register in [37_639u16, 4_096] {
        let (mut lamp, mut clock) = folded(Duration::from_secs(10));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        for (servo, at) in lamp
            .servos
            .iter_mut()
            .zip([2_028.0, 2_054.0, 2_049.0, 2_047.0, 1_948.0])
        {
            servo.position = at;
            servo.goal = at as u16;
            servo.status = 0;
        }
        lamp.servos[2].goal = register;
        let limits = Limits {
            wake: true,
            rest_glance: true,
            look_at_name: true,
            ..Limits::folded_with(Tuning {
                reach: [150, 15, 1, 250],
                speed: 120,
                servo_speed: 1_000,
                servo_acceleration: 60,
            })
            .unwrap()
        };
        let up = Standing::new(
            [2_048, 2_047],
            [1_772, 2_043, 2_015, 2_657, 2_602],
            100,
            6,
            90,
            [950, 950],
        )
        .unwrap()
        .idling(true);
        let report = run_alive(
            &mut lamp,
            &mut clock,
            limits,
            Some([1_950, 2_043, 2_015]),
            Some(up),
            None,
            || Some((Mood::Resting, 0)),
            Asked::default,
            |_| {},
        );
        let elbow: Vec<u16> = lamp
            .goals
            .iter()
            .flatten()
            .filter(|(id, _)| *id == 3)
            .map(|(_, goal)| *goal)
            .collect();
        // Its first goal is where it is, not the register.
        assert_eq!(elbow.first(), Some(&2_049), "register {register}");
        assert!(
            elbow.iter().all(|goal| (2_040..=2_610).contains(goal)),
            "register {register}: elbow asked {elbow:?}, {:?}, {:?}",
            report.stop,
            report.tipping
        );
        assert_eq!(report.stop, Stop::Cancelled, "register {register}");
    }
}

/// lamp-4ace standing and folded as the seventh's test above has it, its
/// goal registers where it is, with `misreads` of its positions: the run
/// until it is asked to stop at ten seconds, and every elbow goal written.
fn misread_4ace(misreads: Vec<(Duration, usize, u16, u8)>) -> (BodyReport, Vec<u16>, Lamp) {
    misread_4ace_with(misreads, |_| {})
}

/// The same, with the lamp first changed by `prepare`.
fn misread_4ace_with(
    misreads: Vec<(Duration, usize, u16, u8)>,
    prepare: impl FnOnce(&mut Lamp),
) -> (BodyReport, Vec<u16>, Lamp) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(10));
    prepare(&mut lamp);
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    for (servo, at) in lamp
        .servos
        .iter_mut()
        .zip([2_028.0, 2_054.0, 2_049.0, 2_047.0, 1_948.0])
    {
        servo.position = at;
        servo.goal = at as u16;
        servo.status = 0;
    }
    lamp.misreads = misreads;
    let limits = Limits {
        wake: true,
        rest_glance: true,
        look_at_name: true,
        ..Limits::folded_with(Tuning {
            reach: [150, 15, 1, 250],
            speed: 120,
            servo_speed: 1_000,
            servo_acceleration: 60,
        })
        .unwrap()
    };
    let up = Standing::new(
        [2_048, 2_047],
        [1_772, 2_043, 2_015, 2_657, 2_602],
        100,
        6,
        90,
        [950, 950],
    )
    .unwrap()
    .idling(true);
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some([1_950, 2_043, 2_015]),
        Some(up),
        None,
        || Some((Mood::Resting, 0)),
        Asked::default,
        |_| {},
    );
    let elbow = lamp
        .goals
        .iter()
        .flatten()
        .filter(|(id, _)| *id == 3)
        .map(|(_, goal)| *goal)
        .collect();
    (report, elbow, lamp)
}

/// lamp-4ace, 01:58:39: the body ended on `Cancelled`, and its exit sent
/// the arm's last positions back as goals. One elbow reading of 37639 went
/// out as the elbow's goal. A reading outside 0..=4095 is not where the
/// joint is: it is counted, never written, and the run ends as asked.
#[test]
fn an_elbow_read_past_its_range_just_before_cancelled_is_never_written_as_a_goal() {
    let (report, _, lamp) = misread_4ace(Vec::new());
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let last = *lamp.read_times.last().unwrap();
    let (report, elbow, lamp) = misread_4ace(vec![(last, 2, 37_639, 1)]);
    assert!(lamp.misreads.iter().all(|misread| misread.3 == 0));
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.bad_reads, [0, 0, 1, 0, 0]);
    assert_eq!(report.last.unwrap()[2].position, 2_049);
    assert!(
        elbow.iter().all(|goal| (2_040..=2_610).contains(goal)),
        "elbow asked {elbow:?}"
    );
}

/// One bad read, out of range or too far from the step before, is counted
/// and skipped: the run goes on as if the joint read where it was.
#[test]
fn one_bad_read_of_a_joint_is_counted_and_skipped() {
    for reading in [37_639, 3_100] {
        let (report, elbow, _) = misread_4ace(vec![(Duration::from_secs(4), 2, reading, 1)]);
        assert_eq!(report.stop, Stop::Cancelled, "{reading}: {report:?}");
        assert_eq!(report.bad_reads, [0, 0, 1, 0, 0], "{reading}");
        assert!(
            elbow.iter().all(|goal| (2_040..=2_610).contains(goal)),
            "{reading}: elbow asked {elbow:?}"
        );
    }
}

/// Three bad reads of one joint in a row are a servo or a wire that cannot
/// be trusted: the body stops, naming the joint, and writes none of them.
#[test]
fn three_bad_reads_of_a_joint_in_a_row_stop_the_body_naming_it() {
    let (report, elbow, lamp) = misread_4ace(vec![(Duration::from_secs(4), 2, 37_639, 3)]);
    // It stops on the third, not after more steps tried as a failed read.
    let reads = lamp
        .read_times
        .iter()
        .filter(|at| **at >= Duration::from_secs(4));
    assert_eq!(reads.count(), 3, "{report:?}");
    assert!(
        matches!(
            report.stop,
            Stop::BadRead {
                joint: 3,
                position: 37_639,
                from: 2_040..=2_610,
            }
        ),
        "{report:?}"
    );
    assert_eq!(report.bad_reads, [0, 0, 3, 0, 0]);
    assert!(
        elbow.iter().all(|goal| (2_040..=2_610).contains(goal)),
        "elbow asked {elbow:?}"
    );
}

/// lamp-52e6, 2026-10-17: every run that stood ended on
/// `arm_peak_temperature [150,150]` while its logged steps read 43 to 54,
/// and single steps between them read 60 to 150 in replies whose positions
/// and loads were good. One reading of 150 is a bad temperature: counted,
/// never a peak, and the run goes on.
#[test]
fn a_shoulder_reading_150_degrees_once_is_counted_and_never_a_peak() {
    let (report, _, _) = misread_4ace_with(Vec::new(), |lamp| {
        lamp.event = Some((Duration::from_secs(4), 1, Event::HeatOnce(150)));
    });
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.arm_peak_temperature, [45, 45]);
    assert_eq!(report.bad_temperatures, [0, 1, 0, 0, 0]);
    assert_eq!(report.bad_reads, [0; 5]);
}

/// Bad temperatures in a row, fewer than `temperature_steps`, are not heat
/// and do not stop the run: a shoulder reading 150 for 0.2 s is counted on
/// each read, never the peak, and the run ends as asked.
#[test]
fn bad_temperatures_in_a_row_do_not_stop_the_run() {
    let (report, _, lamp) = misread_4ace_with(Vec::new(), |lamp| {
        lamp.heats = vec![
            (Duration::from_secs(4), 1, 150),
            (Duration::from_millis(4_200), 1, 45),
        ];
    });
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let reads = lamp
        .read_times
        .iter()
        .filter(|at| (Duration::from_secs(4)..Duration::from_millis(4_200)).contains(*at))
        .count();
    assert!(reads >= 3, "{reads}");
    assert_eq!(usize::from(report.bad_temperatures[1]), reads);
    assert_eq!(report.arm_peak_temperature, [45, 45]);
}

/// Folded, the head pitch at `start`, with `heats` of it: how the run
/// ended and what it counted.
fn heated_pitch(start: u8, glitch: Option<u8>, heats: Vec<(Duration, usize, u8)>) -> BodyReport {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.servos[PITCH].temperature = start;
    lamp.event = glitch.map(|to| (Duration::from_millis(1_500), PITCH, Event::HeatOnce(to)));
    lamp.heats = heats;
    let news = script(&clock.time);
    run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {})
}

/// Real heat still stops it: a head that warms from 58 to 63 a degree
/// each 0.1 s, a rise `judge_temperatures` lets in, ends the run after `temperature_steps`
/// hot steps.
#[test]
fn a_joint_that_really_warms_still_stops_after_its_hot_steps() {
    let ramp = (59..=63)
        .zip(0..)
        .map(|(to, n)| (Duration::from_millis(1_500 + 100 * n), PITCH, to))
        .collect();
    let report = heated_pitch(58, None, ramp);
    let hot = Stop::Temperature {
        joint: 5,
        temperature: 63,
    };
    assert_eq!(report.stop, hot, "{report:?}");
    assert_eq!(report.bad_temperatures, [0; 5]);
}

/// A low first read does not hide real heat: the servo really at 70, only
/// its first read 40. 70 stays, so after `temperature_steps` reads it is
/// the good reading, and heat stops the run as on lamp-voice.
#[test]
fn a_joint_really_at_70_after_a_first_read_of_40_still_stops_on_heat() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.servos[PITCH].temperature = 70;
    lamp.event = Some((Duration::ZERO, PITCH, Event::HeatOnce(40)));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    let hot = Stop::Temperature {
        joint: 5,
        temperature: 70,
    };
    assert_eq!(report.stop, hot, "{report:?}");
}

/// A jump that stays is not dropped for ever: 45, then 70 from 1.5 s, and
/// a steady 150, each end on heat as on lamp-voice.
#[test]
fn a_jump_that_stays_becomes_the_reading_and_stops_on_heat() {
    for to in [70, 150] {
        let report = heated_pitch(45, None, vec![(Duration::from_millis(1_500), PITCH, to)]);
        let hot = Stop::Temperature {
            joint: 5,
            temperature: to,
        };
        assert_eq!(report.stop, hot, "{to}: {report:?}");
        assert_eq!(report.temperature_settles, vec![(5, 45, to)], "{to}");
        assert_eq!(
            report.bad_temperatures[4],
            // It settles on the read half of `temperature_steps` in.
            u16::from(Limits::FOLDED.temperature_steps / 2) - 1,
            "{to}"
        );
    }
}

/// One glitch within the old 15 is not let in to walk the reading: 47, one
/// read of 32, then 48. The run judges 48, not 32.
#[test]
fn a_glitch_of_32_between_47_and_48_is_dropped() {
    let report = heated_pitch(
        47,
        Some(32),
        vec![(Duration::from_millis(1_600), PITCH, 48)],
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.bad_temperatures, [0, 0, 0, 0, 1]);
    assert_eq!(report.peak_temperature[0], 48);
}

/// The seventh's case (a): the servo at 60, one read of 63 let in, and its
/// register a degree down to 59 from the next read. The 59s are replaced
/// by 63 until they settle; a replaced read holds the heat streak, so the
/// one 63 never becomes 50 hot steps and nothing is let go.
#[test]
fn a_small_glitch_near_the_limit_then_a_flicker_down_is_not_heat() {
    let report = heated_pitch(
        60,
        Some(63),
        vec![(Duration::from_millis(1_510), PITCH, 59)],
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.bad_temperatures[4] > 0, "{report:?}");
}

/// The seventh's 5 degrees a second row: the first read 5, the servo
/// rising from 45 at once. The rise settles a read at a time, so it is
/// judged on it within a second and stops on heat near 62, not at 120.
#[test]
fn a_fast_rise_after_a_low_first_read_still_stops_near_the_limit() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.servos[PITCH].temperature = 45;
    lamp.event = Some((Duration::ZERO, PITCH, Event::HeatOnce(5)));
    lamp.heats = (0..100)
        .map(|n| (Duration::from_millis(200 * (n + 1)), PITCH, 46 + n as u8))
        .collect();
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert!(
        matches!(
            report.stop,
            Stop::Temperature {
                joint: 5,
                temperature: 62..=70
            }
        ),
        "{report:?}"
    );
}

/// Folded until `until`, the head pitch at `start`, warmed by `heats`, its
/// temperature reading `readings` read by read from `from`.
fn scripted_pitch(
    start: u8,
    heats: Vec<(Duration, usize, u8)>,
    from: Duration,
    readings: Vec<Option<u8>>,
    until: Duration,
) -> (BodyReport, Lamp) {
    let (mut lamp, mut clock) = folded(until);
    lamp.servos[PITCH].temperature = start;
    lamp.heats = heats;
    lamp.flicker = Some((from, PITCH, readings, 0));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    (report, lamp)
}

/// 150 and 5 by turns, `reads` reads.
fn by_turns(reads: usize) -> Vec<Option<u8>> {
    (0..reads)
        .map(|n| Some(if n % 2 == 0 { 150 } else { 5 }))
        .collect()
}

/// The seventh's 10:12 case: the last good 40, then the joint really
/// warming at 5 degrees a second behind 60 reads of 150 and 5 by turns,
/// then a 150 on one read in 27 (lamp-52e6's elbow rate). Its last good
/// one is cool, so no heat stop could see it: it ends as unreadable on the
/// 50th bad read in a row, within a second of the warming starting.
#[test]
fn a_joint_warming_behind_bad_reads_stops_as_unreadable_within_a_second() {
    let at = Duration::from_millis(1_500);
    let heats = (0..40)
        .map(|n| (at + Duration::from_millis(200 * n), PITCH, 41 + n as u8))
        .collect();
    let mut readings = by_turns(60);
    readings.extend((0..2_000).map(|n| (n % 27 == 0).then_some(150)));
    let (report, lamp) = scripted_pitch(40, heats, at, readings, Duration::from_secs(40));
    assert!(
        matches!(
            report.stop,
            Stop::BadTemperature {
                joint: 5,
                from: 40..=43,
                ..
            }
        ),
        "{report:?}"
    );
    let reads = lamp.read_times.iter().filter(|t| **t >= at).count();
    assert!(
        reads <= usize::from(Limits::FOLDED.temperature_steps) + 1,
        "{reads}"
    );
}

/// A joint already hot when its reads stop settling ends the same way.
#[test]
fn a_hot_joint_whose_reads_never_settle_stops_as_unreadable() {
    let ramp = (61..=70)
        .zip(0..)
        .map(|(to, n)| (Duration::from_millis(1_500 + 40 * n), PITCH, to))
        .collect();
    let at = Duration::from_millis(1_900);
    let (report, _) = scripted_pitch(60, ramp, at, by_turns(2_000), Duration::from_secs(8));
    assert!(
        matches!(report.stop, Stop::BadTemperature { joint: 5, .. }),
        "{report:?}"
    );
    assert_eq!(
        report.bad_temperatures[4],
        u16::from(Limits::FOLDED.temperature_steps)
    );
}

/// Glitches between good reads never end the run, however many: a cool
/// head with a 150 on one read in 27 runs its full 60 s. The stop counts
/// bad reads in a row only.
#[test]
fn a_cool_joint_glitching_one_read_in_27_runs_on_for_a_minute() {
    let readings = (0..3_100).map(|n| (n % 27 == 0).then_some(150)).collect();
    let (report, _) = scripted_pitch(
        45,
        Vec::new(),
        Duration::from_millis(1_500),
        readings,
        Duration::from_secs(60),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.bad_temperatures[4] > 100, "{report:?}");
    assert_eq!(report.peak_temperature[0], 45);
}

/// At lamp-52e6's measured rate, one read in 27 at random, a cool head runs
/// ten minutes and nothing stops it.
#[test]
fn a_cool_joint_glitching_at_random_one_read_in_27_runs_ten_minutes() {
    let mut seed: u32 = 0x5eed_1e57;
    let readings = (0..30_500)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8).is_multiple_of(27).then_some(150)
        })
        .collect();
    let (report, _) = scripted_pitch(
        45,
        Vec::new(),
        Duration::from_millis(1_500),
        readings,
        Duration::from_secs(600),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.bad_temperatures[4] > 900, "{report:?}");
    assert_eq!(report.peak_temperature[0], 45);
}

/// Garbage with one good read in 50 is still garbage: a head warmed to 70,
/// then 49 reads of 150 and 5 by turns and one true, again and again. A
/// good read takes one off the bad reads in a row instead of starting them
/// again, so it ends as unreadable within seconds (44.7 s before).
#[test]
fn garbage_with_one_good_read_in_50_still_stops_within_seconds() {
    let ramp = (61..=70)
        .zip(0..)
        .map(|(to, n)| (Duration::from_millis(1_500 + 40 * n), PITCH, to))
        .collect();
    let at = Duration::from_millis(1_900);
    let readings = (0..2_000)
        .map(|n| (n % 50 != 49).then_some(if n % 2 == 0 { 150 } else { 5 }))
        .collect();
    let (report, lamp) = scripted_pitch(60, ramp, at, readings, Duration::from_secs(8));
    assert!(
        matches!(report.stop, Stop::BadTemperature { joint: 5, .. }),
        "{report:?}"
    );
    let reads = lamp.read_times.iter().filter(|t| **t >= at).count();
    assert!(reads <= 150, "{reads}");
}

/// Every arm goal written, shoulder and elbow, in order.
fn arm_goal_writes(lamp: &Lamp) -> Vec<[u16; 2]> {
    lamp.goals
        .iter()
        .filter(|write| write.iter().any(|(id, _)| *id == 2))
        .map(|write| {
            [
                goal_of(write, Joint::BasePitch),
                goal_of(write, Joint::ElbowPitch),
            ]
        })
        .collect()
}

/// The arm folded down by steps of at most `ARM_GOAL_STEP` to the folded
/// pose (2,050 and 2,047), and then the whole lamp let go.
fn assert_folded_then_let_go(lamp: &Lamp) {
    let arm = arm_goal_writes(lamp);
    assert!(
        arm.windows(2).all(
            |pair| (0..2).all(|index| pair[0][index].abs_diff(pair[1][index]) <= ARM_GOAL_STEP)
        ),
        "{arm:?}"
    );
    assert_eq!(arm.last(), Some(&[2_050, 2_047]), "{arm:?}");
    assert!(
        (lamp.servos[1].position - 2_050.0).abs() <= 3.0,
        "{}",
        lamp.servos[1].position
    );
    assert!(let_go_of_the_whole_lamp(lamp), "{:?}", lamp.registers);
}

/// The reviewer's 11:50 and 12:15 cases on #1281: standing, an elbow whose
/// temperature reads 150 and 5 by turns from 8 s. Let go up, the lamp would
/// fall; held up, it may be heating with nothing to end it. It folds down
/// at its own pace, its arm goals a step at a time, is let go once folded,
/// and ends on `BadTemperature`. Started again 15 s later (the unit's
/// restart) with the elbow still unreadable, no arm is left up with torque
/// on.
#[test]
fn a_standing_lamp_whose_elbow_temperature_is_unreadable_folds_then_lets_go() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(20));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    lamp.flicker = Some((Duration::from_secs(8), 2, by_turns(5_000), 0));
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 60)),
        || Some((Mood::Listening, 100)),
        |_| {},
    );
    assert!(report.stood, "{report:?}");
    assert!(
        matches!(report.stop, Stop::BadTemperature { joint: 3, .. }),
        "{report:?}"
    );
    let up = arm_goal_writes(&lamp);
    assert!(up.iter().any(|arm| arm[0] > 2_600), "{up:?}");
    assert_folded_then_let_go(&lamp);

    let again = lamp.time.get() + Duration::from_secs(15);
    lamp.time.set(again);
    let mut clock = Clock {
        time: lamp.time.clone(),
        cancel_at: again + Duration::from_secs(20),
        inhibit_at: None,
    };
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 60)),
        || Some((Mood::Listening, 100)),
        |_| {},
    );
    assert_ne!(report.stop, Stop::Cancelled, "{report:?}");
    for index in [1, 2] {
        let servo = lamp.servos[index];
        assert!(
            servo.torque == 0 || servo.position < 2_100.0,
            "{index}: {report:?}"
        );
    }
}

/// The seventh's 12:52 P3 on #1281: another surprise while a standing lamp
/// folds down let go of it mid-air. Stood up and asked to stop at 12 s, the
/// head turning hot, or unreadable, as it folds: it keeps folding, a step
/// at a time, and is let go folded, ending on that surprise.
#[test]
fn a_surprise_while_a_standing_lamp_folds_keeps_it_folding_then_lets_go() {
    for hot in [true, false] {
        let (mut lamp, mut clock) = folded(Duration::from_secs(12));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        let from = Duration::from_millis(12_300);
        if hot {
            lamp.heats = vec![(from, PITCH, 70)];
        } else {
            lamp.flicker = Some((from, PITCH, by_turns(5_000), 0));
        }
        let report = run_standing(
            &mut lamp,
            &mut clock,
            Limits::FOLDED,
            Some(HOME),
            Some(standing(100, 60)),
            || Some((Mood::Listening, 100)),
            |_| {},
        );
        assert!(report.stood, "{report:?}");
        if hot {
            assert!(
                matches!(report.stop, Stop::Temperature { joint: 5, .. }),
                "{report:?}"
            );
        } else {
            assert!(
                matches!(report.stop, Stop::BadTemperature { joint: 5, .. }),
                "{report:?}"
            );
        }
        assert_folded_then_let_go(&lamp);
    }
}
/// Folded, the same ends at once, let go: it is down already.
#[test]
fn a_folded_lamp_whose_elbow_temperature_is_unreadable_lets_go() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(10));
    lamp.flicker = Some((Duration::from_millis(1_500), 2, by_turns(2_000), 0));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert!(
        matches!(report.stop, Stop::BadTemperature { joint: 3, .. }),
        "{report:?}"
    );
    assert!(let_go_of_the_whole_lamp(&lamp), "{:?}", lamp.registers);
}

/// Found standing with an elbow too warm, or reading over 100 on every
/// read: it is not let go up nor left up held. It folds down first and is
/// let go folded, on `Temperature`.
#[test]
fn a_lamp_found_standing_with_a_warm_or_unreadable_elbow_folds_then_lets_go() {
    for (reads, temperature) in [(vec![], 70), (vec![Some(150); 6], 45)] {
        let (mut lamp, mut clock) = folded(Duration::from_secs(20));
        lamp.arm_moves = true;
        for (index, at) in [(1usize, 2_650.0f32), (2, 2_600.0), (PITCH, 1_900.0)] {
            lamp.servos[index].position = at;
            lamp.servos[index].goal = at as u16;
            lamp.servos[index].status = 0;
        }
        lamp.servos[2].temperature = temperature;
        lamp.flicker = Some((Duration::ZERO, 2, reads, 0));
        let report = run_standing(
            &mut lamp,
            &mut clock,
            Limits::FOLDED,
            Some(HOME),
            Some(standing(100, 60)),
            || Some((Mood::Resting, 0)),
            |_| {},
        );
        assert!(
            matches!(report.stop, Stop::Temperature { joint: 3, .. }),
            "{temperature}: {report:?}"
        );
        assert_folded_then_let_go(&lamp);
    }
}

/// A low first read, then the true value rising 5 degrees a second with a
/// 150 every 20 reads: each glitch is skipped by the settle, not a reason to
/// start it again, so the true value settles and heat stops it near the
/// limit (it ended as unreadable on a cool lamp before).
#[test]
fn a_true_value_settles_through_glitches_after_a_low_first_read() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.servos[PITCH].temperature = 45;
    lamp.event = Some((Duration::ZERO, PITCH, Event::HeatOnce(5)));
    lamp.heats = (0..100)
        .map(|n| (Duration::from_millis(200 * (n + 1)), PITCH, 46 + n as u8))
        .collect();
    let readings = (0..400).map(|n| (n % 20 == 19).then_some(150)).collect();
    lamp.flicker = Some((Duration::from_millis(20), PITCH, readings, 0));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert!(
        matches!(
            report.stop,
            Stop::Temperature {
                joint: 5,
                temperature: 62..=72
            }
        ),
        "{report:?}"
    );
    assert_eq!(report.temperature_settles.first().map(|s| s.0), Some(5));
}

/// One glitch on the first read is read again before it is acted on: a
/// first read of 150 and then 45 twice, agreeing, starts as usual, nothing
/// let go.
#[test]
fn a_hot_first_read_that_does_not_repeat_starts_as_usual() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(4));
    lamp.event = Some((Duration::ZERO, PITCH, Event::HeatOnce(150)));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.hot_rereads, vec![[45; 5]; 2]);
    assert!(report.moved, "{report:?}");
}

/// Heat that repeats on the read again is acted on as before: refused, and
/// the whole lamp let go.
#[test]
fn a_hot_first_read_that_repeats_is_refused_as_before() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(4));
    lamp.servos[PITCH].temperature = 70;
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert!(
        matches!(&report.stop, Stop::Refused { why } if why.contains("70 already")),
        "{report:?}"
    );
    assert_eq!(report.hot_rereads, vec![[45, 45, 45, 45, 70]]);
    assert!(let_go_of_the_whole_lamp(&lamp), "{:?}", lamp.registers);
}

/// A real heat found on the first read is not let in by one low glitch on
/// a read again: the head pitch truly at 70 reads 70, 5, 70, 70. Only the
/// last two agree, hot, so it is refused with nothing moved.
#[test]
fn a_hot_first_read_with_one_low_glitch_read_again_is_still_refused() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(4));
    lamp.servos[PITCH].temperature = 70;
    lamp.flicker = Some((Duration::ZERO, PITCH, vec![Some(70), Some(5)], 0));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert!(
        matches!(&report.stop, Stop::Refused { why } if why.contains("70 already")),
        "{report:?}"
    );
    assert_eq!(report.hot_rereads.len(), 3, "{report:?}");
    assert_eq!(report.goal_writes, 0, "{report:?}");
    assert!(!report.moved, "{report:?}");
}

/// A read again that fails after a read found the joint hot is acted on
/// with the hottest read: refused and let go, not left held on a hot joint.
#[test]
fn a_failed_read_again_after_a_hot_first_read_is_refused_and_let_go() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(4));
    lamp.servos[PITCH].temperature = 70;
    lamp.silent_from = Some(Duration::from_micros(100));
    lamp.silent_until = Some(Duration::from_micros(400));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert!(
        matches!(&report.stop, Stop::Refused { why } if why.contains("70 already")),
        "{report:?}"
    );
    assert!(report.hot_rereads.is_empty(), "{report:?}");
    assert!(let_go_of_the_whole_lamp(&lamp), "{:?}", lamp.registers);
}

/// A lamp found standing, its head pitch's first reads `reads` and then
/// what it is (45): run until asked to stop at 1 s.
fn found_standing_with_first_reads(reads: Vec<Option<u8>>) -> (BodyReport, Lamp) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(1));
    lamp.arm_moves = true;
    for (index, at) in [(1usize, 2_650.0f32), (2, 2_600.0), (PITCH, 1_900.0)] {
        lamp.servos[index].position = at;
        lamp.servos[index].goal = at as u16;
        lamp.servos[index].status = 0;
    }
    lamp.flicker = Some((Duration::ZERO, PITCH, reads, 0));
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 60)),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    (report, lamp)
}

/// No torque off was written to the shoulder or the elbow.
fn arm_never_let_go(lamp: &Lamp) -> bool {
    !lamp
        .registers
        .iter()
        .any(|(id, address, data)| (*id == 2 || *id == 3) && *address == 40 && data == &[0])
}

/// Two glitches of 150 in a row on the first reads are not heat: a read
/// over 100 never agrees hot. 150, 150, then 45 and 45 starts as usual,
/// and a lamp found standing is not let go.
#[test]
fn two_first_reads_of_150_then_45_start_as_usual_standing() {
    let (report, lamp) = found_standing_with_first_reads(vec![Some(150), Some(150)]);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.hot_rereads.len(), 3, "{report:?}");
    assert!(arm_never_let_go(&lamp), "{:?}", lamp.registers);
    assert_eq!((lamp.servos[1].torque, lamp.servos[2].torque), (1, 1));
}

/// Goal writes that name servo `id` after `after`.
fn goals_to_after(lamp: &Lamp, id: u8, after: Duration) -> usize {
    lamp.goals
        .iter()
        .zip(&lamp.goal_times)
        .filter(|(write, at)| **at > after && write.iter().any(|(to, _)| *to == id))
        .count()
}

/// The reviewer's 13:14 P2 on #1296: a status flag is the servo's own
/// protection, and a goal turns a servo that cut itself out back on. Stood
/// up and asked to stop at 12 s, a joint raising 0x20 at 12.5 s is written
/// no goal after that. The elbow, without which the arm cannot fold, lets
/// go at once; the head's pitch is left as it is while the arm folds.
#[test]
fn a_flag_while_a_standing_lamp_folds_writes_that_joint_nothing_more() {
    for (index, id) in [(2usize, 3u8), (PITCH, 5)] {
        let (mut lamp, mut clock) = folded(Duration::from_secs(12));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        let at = Duration::from_millis(12_500);
        lamp.event = Some((at, index, Event::Flag(0x20)));
        let report = run_standing(
            &mut lamp,
            &mut clock,
            Limits::FOLDED,
            Some(HOME),
            Some(standing(100, 60)),
            || Some((Mood::Listening, 100)),
            |_| {},
        );
        assert!(report.stood, "{report:?}");
        assert!(
            matches!(report.stop, Stop::Status { joint, now: 0x20, .. } if joint == id),
            "{report:?}"
        );
        assert_eq!(
            goals_to_after(&lamp, id, at + Duration::from_millis(40)),
            0,
            "{id}"
        );
        if id == 5 {
            assert_folded_then_let_go(&lamp);
        } else {
            assert!(lamp.servos[1].position > 2_100.0, "{report:?}");
            assert!(let_go_of_the_whole_lamp(&lamp), "{:?}", lamp.registers);
        }
    }
}

/// The reviewer's 13:14 P3 on #1296: found standing, a head joint with
/// torque off and over 100 on every read may be the servo's own heat
/// cut-out. It is not taken hold of nor written a goal: the arm folds
/// without it, then all five are let go. The 13:31 P3: the same with
/// `--torque on`, whose start takes hold of a joint found without torque.
#[test]
fn a_head_found_without_torque_and_unreadable_is_left_while_the_arm_folds() {
    for torque_on in [false, true] {
        let (mut lamp, mut clock) = folded(Duration::from_secs(20));
        lamp.arm_moves = true;
        for (index, at) in [(1usize, 2_650.0f32), (2, 2_600.0), (PITCH, 1_900.0)] {
            lamp.servos[index].position = at;
            lamp.servos[index].goal = at as u16;
            lamp.servos[index].status = 0;
        }
        lamp.servos[PITCH].torque = 0;
        // A take-hold would be let through, as on lamp-52e6.
        lamp.may_take_hold = torque_on;
        lamp.flicker = Some((Duration::ZERO, PITCH, vec![Some(150); 100_000], 0));
        let report = run_standing(
            &mut lamp,
            &mut clock,
            Limits {
                torque_on,
                ..Limits::FOLDED
            },
            Some(HOME),
            Some(standing(100, 60)),
            || Some((Mood::Resting, 0)),
            |_| {},
        );
        assert!(
            matches!(&report.stop, Stop::Refused { why }
                if why == "temperature unreadable at start: wrist_pitch reads 150"),
            "{report:?}"
        );
        assert!(report.torque_given.is_empty(), "{report:?}");
        assert!(
            !lamp
                .registers
                .iter()
                .any(|(id, address, data)| *id == 5 && *address == 40 && data.first() == Some(&1)),
            "{:?}",
            lamp.registers
        );
        assert_eq!(goals_to_after(&lamp, 5, Duration::ZERO), 0);
        assert!(
            !lamp
                .registers
                .iter()
                .any(|(id, address, _)| *id == 5 && *address != 40),
            "{:?}",
            lamp.registers
        );
        assert_folded_then_let_go(&lamp);
    }
}

/// The reviewer's 12:36 P3 on #1281: a lamp found standing whose start is
/// refused with nothing written (head pitch 150 on every read, or its torque
/// off) was left up, held, by every restart. It folds down first, a step at
/// a time, then is let go and the run ends on the refusal. Started again
/// 15 s later, it is refused folded and nothing is left powered.
#[test]
fn a_lamp_found_standing_whose_start_is_refused_folds_then_lets_go() {
    let cases: [(Option<u8>, u8, &str); 2] = [
        (
            Some(150),
            1,
            "temperature unreadable at start: wrist_pitch reads 150",
        ),
        (None, 0, "wrist_pitch torque is not on"),
    ];
    for (read, torque, why_wanted) in cases {
        let (mut lamp, mut clock) = folded(Duration::from_secs(20));
        lamp.arm_moves = true;
        for (index, at) in [(1usize, 2_650.0f32), (2, 2_600.0), (PITCH, 1_900.0)] {
            lamp.servos[index].position = at;
            lamp.servos[index].goal = at as u16;
            lamp.servos[index].status = 0;
        }
        lamp.servos[PITCH].torque = torque;
        lamp.may_take_hold = torque == 0;
        lamp.flicker = Some((Duration::ZERO, PITCH, vec![read; 100_000], 0));
        let run = |lamp: &mut Lamp, clock: &mut Clock| {
            run_standing(
                lamp,
                clock,
                Limits::FOLDED,
                Some(HOME),
                Some(standing(100, 60)),
                || Some((Mood::Resting, 0)),
                |_| {},
            )
        };
        let report = run(&mut lamp, &mut clock);
        assert!(
            matches!(&report.stop, Stop::Refused { why } if why == why_wanted),
            "{report:?}"
        );
        assert_folded_then_let_go(&lamp);

        let again = lamp.time.get() + Duration::from_secs(15);
        lamp.time.set(again);
        let mut clock = Clock {
            time: lamp.time.clone(),
            cancel_at: again + Duration::from_secs(20),
            inhibit_at: None,
        };
        let report = run(&mut lamp, &mut clock);
        assert!(matches!(report.stop, Stop::Refused { .. }), "{report:?}");
        assert!(lamp.servos.iter().all(|servo| servo.torque == 0));
    }
}

/// A first read at the limit with a cool read agreeing after it is not
/// heat: both agreeing reads must be hot. 62, then 61, starts as usual
/// (70 and 70: `a_hot_first_read_that_repeats_is_refused_as_before`).
#[test]
fn a_first_read_at_the_limit_then_one_below_starts_as_usual() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(4));
    lamp.servos[PITCH].temperature = 61;
    lamp.flicker = Some((Duration::ZERO, PITCH, vec![Some(62)], 0));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.hot_rereads, vec![[45, 45, 45, 45, 61]]);
    assert!(report.moved, "{report:?}");
}

/// A folded lamp's arm is not judged on heat at the start, so a warm
/// shoulder is not read again: folded, the shoulder at 65 starts as usual.
#[test]
fn a_folded_lamps_warm_shoulder_is_not_read_again() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(4));
    lamp.servos[1].temperature = 65;
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, Limits::FOLDED, None, news, |_| {});
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.hot_rereads.is_empty(), "{report:?}");
    assert!(report.moved, "{report:?}");
}

/// A replaced read holds the heat streak, neither adding to it nor
/// resetting it: 30 hot reads at a true 62, one 65 let in, then 24 reads at
/// a true 61 replaced by 65 until they settle. Counted as hot, the streak
/// would reach 55 and stop on heat.
fn hot_streak_then_held_reads() -> Vec<Option<u8>> {
    let mut readings = vec![Some(62); 30];
    readings.push(Some(65));
    readings.extend([Some(61); 24]);
    readings
}

/// The hold on the head: the streak of 30 then the 24 held reads never
/// reach 50 hot steps.
#[test]
fn held_reads_after_a_hot_streak_do_not_add_to_the_head_heat_streak() {
    let (report, _) = scripted_pitch(
        61,
        Vec::new(),
        Duration::from_millis(1_500),
        hot_streak_then_held_reads(),
        Duration::from_secs(4),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.temperature_settles, vec![(5, 65, 61)], "{report:?}");
}

/// The hold on a standing arm: the same reads on the shoulder of a lamp
/// that has stood up never make it sit down for heat.
#[test]
fn held_reads_after_a_hot_streak_do_not_add_to_the_arm_heat_streak() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(10));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    lamp.servos[1].temperature = 61;
    lamp.flicker = Some((
        Duration::from_millis(7_500),
        1,
        hot_streak_then_held_reads(),
        0,
    ));
    let news = spoken_to(&clock.time);
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 5)),
        news,
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.stood && report.retreats.is_empty(), "{report:?}");
    assert_eq!(report.temperature_settles, vec![(2, 65, 61)], "{report:?}");
}

/// lamp-4ace's shoulder on 2026-10-17, which ended at 15:25:38 on
/// `BadTemperature` after 618,587 steps though every read was under 62: its
/// register read about 55 and 59 to 61 by turns. Here, a minute at a time,
/// 60 and 61 with a 55 one read in 66 (its 9,354 bad of 618,587), then 55
/// with a 59 to 61 one read in 8, which ended it.
fn flapping_4ace_shoulder(reads: usize, low: u8, high: u8) -> Vec<Option<u8>> {
    (0..reads)
        .map(|n| {
            let minute = (n / 3_000) % 2 == 0;
            Some(match (minute, n % 66 == 65, n % 8 == 7) {
                (true, true, _) => low,
                (true, false, _) => high - u8::from(n % 2 == 0),
                (false, _, true) => high - (n % 3) as u8,
                (false, _, false) => low,
            })
        })
        .collect()
}

/// That shoulder, standing for 20 minutes (awake the whole run, so an arm
/// heat retreat would show): never ended as unreadable, and
/// judged on its higher values, 61.
#[test]
fn a_shoulder_flapping_between_55_and_61_runs_20_minutes_judged_at_61() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(1_210));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    lamp.servos[1].temperature = 60;
    lamp.flicker = Some((
        Duration::from_millis(7_500),
        1,
        flapping_4ace_shoulder(62_000, 55, 61),
        0,
    ));
    let news = spoken_to(&clock.time);
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 1_300)),
        news,
        |_| {},
    );
    assert_eq!(
        report.stop,
        Stop::Cancelled,
        "{:?} {:?}",
        report.stop,
        report.bad_temperatures
    );
    assert!(
        report.stood && report.retreats.is_empty(),
        "{:?}",
        report.retreats
    );
    assert!(
        report.arm_peak_temperature[0] >= 61,
        "{:?}",
        report.arm_peak_temperature
    );
    assert!(
        report.temperature_flaps[1] > 0
            && report.temperature_flaps[1] <= report.bad_temperatures[1],
        "{:?}",
        report.temperature_flaps
    );
    assert!(
        report.bad_temperatures[1] > 1_000,
        "{:?}",
        report.bad_temperatures
    );
}

/// The same flap two degrees over the limit, 60 and 64 by turns, is heat:
/// judged on 64, it stops on `Temperature`, not as unreadable.
#[test]
fn a_joint_flapping_between_60_and_64_stops_on_heat() {
    let readings = (0..3_000)
        .map(|n| Some(if n % 2 == 0 { 64 } else { 60 }))
        .collect();
    let (report, _) = scripted_pitch(
        60,
        Vec::new(),
        Duration::from_millis(1_500),
        readings,
        Duration::from_secs(60),
    );
    assert!(
        matches!(
            report.stop,
            Stop::Temperature {
                joint: 5,
                temperature: 62..
            }
        ),
        "{report:?}"
    );
    // The exit report as the body prints it reads back through
    // `lamp-body exit-report`: a renamed field fails here.
    let line = format!(
        "lamp-body[1]: {{\"port\":{{}},\"body\":{}}}",
        serde_json::to_string(&report).unwrap()
    );
    let Some(crate::exit_report::Found::Read(summary)) = crate::exit_report::summary(&line) else {
        panic!("no readable exit report in {line}");
    };
    assert!(
        summary.contains("\nstop: wrist_pitch (joint 5): too hot at "),
        "{summary}"
    );
    // Every field is read by its name: one renamed would say "not in this
    // report" here.
    assert!(!summary.contains("not in this report"), "{summary}");
    assert!(
        summary.contains("\ntemperature_flaps: base_yaw "),
        "{summary}"
    );
}

/// eighth's probe on #1347: a cool head at 56 whose register reads 57 to
/// 66 on one read in 27 at random (lamp-52e6's rate) runs ten minutes. Lone
/// glitches inside the flap window hold; none adds up to heat.
#[test]
fn a_cool_joint_glitching_within_the_flap_window_runs_ten_minutes() {
    let mut seed: u32 = 0x5eed_1e57;
    let readings = (0..30_500)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            Some(if (seed >> 8).is_multiple_of(27) {
                57 + ((seed >> 20) % 10) as u8
            } else {
                56
            })
        })
        .collect();
    let (report, _) = scripted_pitch(
        56,
        Vec::new(),
        Duration::from_millis(1_500),
        readings,
        Duration::from_secs(600),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
}

/// A sensor whose reads are all 4 to 10 from a stale good one is still
/// unreadable: last good 60, then 64 and 70 by turns (a joint really at
/// 70), ends on `BadTemperature` within about a second, as on lamp-voice;
/// a flap holds only next to its good value. So does the 60/64 flap after
/// a last good 55, which never reads 55 again.
#[test]
fn reads_within_the_flap_window_of_a_stale_good_one_still_stop() {
    for (good, low, high) in [(60, 64, 70), (55, 60, 64)] {
        let at = Duration::from_millis(1_500);
        let readings = (0..3_000)
            .map(|n| Some(if n % 2 == 0 { low } else { high }))
            .collect();
        let (report, lamp) =
            scripted_pitch(good, Vec::new(), at, readings, Duration::from_secs(60));
        assert!(
            matches!(
                report.stop,
                Stop::BadTemperature { joint: 5, .. } | Stop::Temperature { joint: 5, .. }
            ),
            "{good}: {report:?}"
        );
        if good == 60 {
            assert!(
                matches!(report.stop, Stop::BadTemperature { from: 60, .. }),
                "{report:?}"
            );
        }
        let reads = lamp.read_times.iter().filter(|t| **t >= at).count();
        assert!(reads <= 75, "{good}: {reads}");
    }
}

/// The reviewer's noise probes on #1347: reads 4 to 10 from a good 60 that
/// never repeat within 2 are bad reads, though the 60 comes back. A joint
/// really near 70 read as 64 to 70 at random with its last good 60 again
/// one read in 8, or as 64 and 70 by turns with 60 once in 3, 6, 8, 11 or
/// 12, ends on `BadTemperature` within about 3 s, as on lamp-voice.
#[test]
fn lone_flaps_around_a_good_value_that_returns_still_stop() {
    let mut seed: u32 = 0x5eed_1e57;
    let random = (0..3_000)
        .map(|n| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            Some(if n % 8 == 7 {
                60
            } else {
                64 + ((seed >> 20) % 7) as u8
            })
        })
        .collect::<Vec<_>>();
    let mut cases = vec![(8, random)];
    for every in [3, 6, 8, 11, 12] {
        let turns = (0..3_000)
            .map(|n| {
                Some(if n % every == every - 1 {
                    60
                } else if n % 2 == 0 {
                    64
                } else {
                    70
                })
            })
            .collect();
        cases.push((every, turns));
    }
    for (every, readings) in cases {
        let at = Duration::from_millis(1_500);
        let (report, lamp) = scripted_pitch(60, Vec::new(), at, readings, Duration::from_secs(60));
        assert!(
            matches!(report.stop, Stop::BadTemperature { joint: 5, .. }),
            "{every}: {report:?}"
        );
        let last = lamp.read_times.last().copied().unwrap_or_default();
        assert!(last <= at + Duration::from_secs(3), "{every}: {last:?}");
    }
}

/// Taking hold of an arm joint writes its goal by itself, and that write is
/// checked as every other arm goal is: an elbow found without torque
/// outside its window is not given that as its goal.
#[test]
fn an_arm_joint_taken_hold_of_outside_its_window_is_not_given_that_goal() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(1));
    lamp.may_take_hold = true;
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    lamp.servos[2].torque = 0;
    lamp.servos[2].position = 3_900.0;
    lamp.servos[2].goal = 3_900;
    let report = run_standing(
        &mut lamp,
        &mut clock,
        Limits {
            torque_on: true,
            ..Limits::FOLDED
        },
        Some(HOME),
        Some(standing(100, 5)),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert!(
        lamp.registers
            .iter()
            .all(|(id, address, _)| (*id, *address) != (3, 42)),
        "{:?}",
        lamp.registers
    );
    assert!(
        matches!(
            report.stop,
            Stop::Window {
                joint: 3,
                goal: 3_900,
                ..
            }
        ),
        "{report:?}"
    );
}

/// An arm goal moves at most `ARM_GOAL_STEP` a step from the last one
/// written, however near the arm reads: 240 counts from a goal and a
/// reading both at 2047 is refused, and a hold may only lie between the
/// last goal and where the arm steadily reads.
#[test]
fn an_arm_goal_is_never_more_than_a_step_from_the_last_unless_it_holds_where_the_arm_is() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(1));
    lamp.servos[2].status = 0;
    {
        let mut bus = Bus::new(&mut lamp, &mut clock, Some(lamp_4ace_standing()));
        bus.samples().unwrap();
        bus.send_goals(HOME, Some([2_050, 2_047])).unwrap();
        assert!(bus.send_goals(HOME, Some([2_050, 2_287])).is_err());
        assert!(matches!(
            bus.refused.take(),
            Some(Stop::Window {
                joint: 3,
                goal: 2_287,
                from: 2_047,
                ..
            })
        ));
    }
    let elbow: Vec<u16> = lamp
        .goals
        .iter()
        .flatten()
        .filter(|(id, _)| *id == Joint::ElbowPitch.id())
        .map(|(_, goal)| *goal)
        .collect();
    assert_eq!(elbow, [2_047]);
}

/// The seventh's probe on #1192: lamp-4ace folded at rest, the elbow truly
/// at 2049, reads 900 counts off from the last step before it is asked to
/// stop on. That reading is never written back as the elbow's goal at the
/// exit, however long it stays.
#[test]
fn an_elbow_read_far_off_in_the_last_step_is_not_written_back_at_the_exit() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(10));
    lamp.overload_clears = 1;
    for (servo, at) in lamp
        .servos
        .iter_mut()
        .zip([2_028.0, 2_054.0, 2_049.0, 2_047.0, 1_948.0])
    {
        servo.position = at;
        servo.goal = at as u16;
        servo.status = 0;
    }
    lamp.event = Some((Duration::from_millis(9_980), 2, Event::Push(900.0)));
    let limits = Limits {
        wake: true,
        rest_glance: true,
        look_at_name: true,
        ..gesture_limits()
    };
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some([1_950, 2_043, 2_015]),
        Some(lamp_4ace_standing().idling(true)),
        None,
        || Some((Mood::Resting, 0)),
        Asked::default,
        |_| {},
    );
    let elbow: Vec<u16> = lamp
        .goals
        .iter()
        .flatten()
        .filter(|(id, _)| *id == 3)
        .map(|(_, goal)| *goal)
        .collect();
    assert!(
        elbow.iter().all(|goal| goal.abs_diff(2_049) <= 50),
        "{elbow:?} {:?}",
        report.stop
    );
    // It stays there, so three reads in a row are bad: the body stops on it.
    assert!(
        matches!(
            report.stop,
            Stop::BadRead {
                joint: 3,
                position: 2_949,
                from: 2_049,
            }
        ),
        "{report:?}"
    );
}

/// Head and base goals are checked at the wire too: one past 4095, in the
/// sync write or by itself, is never written.
#[test]
fn a_head_goal_past_the_servos_range_never_reaches_the_wire() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(1));
    lamp.may_take_hold = true;
    {
        let mut bus = Bus::new(&mut lamp, &mut clock, None);
        bus.samples().unwrap();
        bus.send_goals(HOME, None).unwrap();
        assert!(bus.send_goals([2_000, 2_044, 37_639], None).is_err());
        assert!(matches!(
            bus.refused.take(),
            Some(Stop::Window {
                joint: 1,
                goal: 37_639,
                window: (0, 4_095),
                from: 2_015,
            })
        ));
        assert!(bus.write_goal(Joint::WristRoll, 4_096, false).is_err());
        assert!(matches!(
            bus.refused.take(),
            Some(Stop::Window { joint: 4, .. })
        ));
    }
    assert_eq!(lamp.goals, [vec![(5, 2_000), (4, 2_044), (1, 2_015)]]);
    assert!(lamp.registers.is_empty(), "{:?}", lamp.registers);
}

/// Part 4 of #1192: the hold at the end of a run that ended on a surprise
/// writes the arm where it was last read, not the goal last written to it,
/// which may be the one that ended the run. The elbow pushed 200 counts off
/// its goal ends the run on `Tracking`, and is held where it was pushed.
#[test]
fn the_hold_after_a_surprise_writes_the_arm_where_it_was_last_read() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(10));
    lamp.overload_clears = 1;
    for (servo, at) in lamp
        .servos
        .iter_mut()
        .zip([2_028.0, 2_054.0, 2_049.0, 2_047.0, 1_948.0])
    {
        servo.position = at;
        servo.goal = at as u16;
        servo.status = 0;
    }
    lamp.event = Some((Duration::from_millis(4_000), 2, Event::Push(200.0)));
    let limits = Limits {
        wake: true,
        rest_glance: true,
        look_at_name: true,
        ..gesture_limits()
    };
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some([1_950, 2_043, 2_015]),
        Some(lamp_4ace_standing().idling(true)),
        None,
        || Some((Mood::Resting, 0)),
        Asked::default,
        |_| {},
    );
    assert_eq!(
        report.stop,
        Stop::Tracking {
            joint: 3,
            goal: 2_049,
            position: 2_249,
        },
        "{report:?}"
    );
    assert!(report.held_at.is_some(), "{report:?}");
    let last = lamp.goals.last().unwrap();
    assert!(last.contains(&(3, 2_249)), "{last:?}");
}

/// The seventh's 03:33 probe on #1192: the elbow truly at 2049 reads 2289
/// or 2349 (under `READ_JUMP`, steady) from the last steps before it is
/// asked to stop. The exit, the one write that keeps torque on, never
/// writes it: the run ends on `Window` and lets go.
#[test]
fn a_steady_wrong_elbow_reading_before_cancelled_is_not_written_at_the_exit() {
    for (from, reading) in [(9_900, 2_289), (9_960, 2_289), (9_900, 2_349)] {
        let (report, elbow, _) =
            misread_4ace(vec![(Duration::from_millis(from), 2, reading, u8::MAX)]);
        assert!(
            elbow.iter().all(|goal| goal.abs_diff(2_049) <= 50),
            "{from} {reading}: {elbow:?} {:?}",
            report.stop
        );
        assert!(
            matches!(
                report.stop,
                Stop::Window {
                    joint: 3,
                    goal,
                    from: 2_049,
                    ..
                } if goal == reading
            ),
            "{from} {reading}: {report:?}"
        );
    }
}

/// A hold where the arm is asks only for a reading the arm has held for a
/// step: the elbow reads 200 counts off in the very step its servo flags a
/// fault. The hold does not write that reading, and the run ends on the
/// fault, not on the refused hold.
#[test]
fn a_hold_at_a_reading_that_just_jumped_is_not_written_and_the_fault_stands() {
    let at = Duration::from_secs(4);
    let (mut lamp, mut clock) = folded(Duration::from_secs(10));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    for (servo, position) in lamp
        .servos
        .iter_mut()
        .zip([2_028.0, 2_054.0, 2_049.0, 2_047.0, 1_948.0])
    {
        servo.position = position;
        servo.goal = position as u16;
        servo.status = 0;
    }
    lamp.misreads = vec![(at, 2, 2_249, u8::MAX)];
    lamp.event = Some((at, 2, Event::Flag(0x20)));
    let limits = Limits {
        wake: true,
        rest_glance: true,
        look_at_name: true,
        ..gesture_limits()
    };
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some([1_950, 2_043, 2_015]),
        Some(lamp_4ace_standing().idling(true)),
        None,
        || Some((Mood::Resting, 0)),
        Asked::default,
        |_| {},
    );
    let elbow: Vec<u16> = lamp
        .goals
        .iter()
        .flatten()
        .filter(|(id, _)| *id == 3)
        .map(|(_, goal)| *goal)
        .collect();
    assert!(
        elbow.iter().all(|goal| goal.abs_diff(2_049) <= 50),
        "{elbow:?} {:?}",
        report.stop
    );
    assert!(
        matches!(report.stop, Stop::Status { joint: 3, .. }),
        "{report:?}"
    );
}

/// Three bad reads in a row end the run on the third, whatever it is doing:
/// the reads made from when they begin.
fn reads_from(lamp: &Lamp, from: Duration) -> usize {
    lamp.read_times.iter().filter(|at| **at >= from).count()
}

/// While the head wakes out of the base, three bad reads of the elbow stop
/// the body on the third.
#[test]
fn three_bad_reads_while_waking_stop_the_body_on_the_third() {
    let from = Duration::from_secs(2);
    let (mut lamp, mut clock) = folded(Duration::from_secs(12));
    lamp.servos[PITCH].position = 2_095.0;
    lamp.servos[PITCH].goal = 2_095;
    lamp.misreads = vec![(from, 2, 37_639, 3)];
    let report = run(
        &mut lamp,
        &mut clock,
        waking(),
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert!(
        matches!(report.stop, Stop::BadRead { joint: 3, .. }),
        "{report:?}"
    );
    assert_eq!(reads_from(&lamp, from), 3, "{report:?}");
}

/// While the lamp lies let go under overload, three bad reads of the base
/// stop the body on the third.
#[test]
fn three_bad_reads_while_let_go_stop_the_body_on_the_third() {
    let from = Duration::from_secs(20);
    let (mut lamp, mut clock) = folded(Duration::from_secs(60));
    lamp.may_take_hold = true;
    lamp.strains = vec![(Duration::from_secs(1), Duration::from_secs_f32(2.6), 2, 800)];
    lamp.misreads = vec![(from, YAW, 37_639, 3)];
    let report = run(
        &mut lamp,
        &mut clock,
        releasing(),
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert!(
        matches!(report.stop, Stop::BadRead { joint: 1, .. }),
        "{report:?}"
    );
    assert_eq!(reads_from(&lamp, from), 3, "{report:?}");
}

/// While a shoulder left up is lowered, three bad reads of the base stop
/// the body on the third.
#[test]
fn three_bad_reads_while_lowering_the_shoulder_stop_the_body_on_the_third() {
    let from = Duration::from_secs(10);
    let (mut lamp, mut clock) = folded(Duration::from_secs(70));
    lamp.may_take_hold = true;
    lamp.arm_moves = true;
    for (index, at) in [
        (YAW, 1_979.0),
        (1, 2_633.0),
        (2, 2_074.0),
        (ROLL, 2_101.0),
        (PITCH, 2_029.0),
    ] {
        let servo = &mut lamp.servos[index];
        (servo.position, servo.goal, servo.torque, servo.status) = (at, 0, 0, 0);
    }
    lamp.misreads = vec![(from, YAW, 37_639, 3)];
    lamp.heavy = Some((PITCH, 1_850.0, 37.0, 232));
    let limits = Limits {
        force: Some(600),
        torque_on: true,
        wake: true,
        ..Limits::folded_with(Tuning {
            reach: [150, 15, 1, 250],
            speed: 120,
            servo_speed: 1_000,
            servo_acceleration: 60,
        })
        .unwrap()
    };
    let up = Standing::new(
        [2_053, 2_069],
        [1_772, 2_035, 2_035, 2_657, 2_602],
        100,
        5,
        90,
        [560, 560],
    )
    .unwrap();
    // Spoken to once it is awake.
    let time = clock.time.clone();
    let news = move || {
        Some(if time.get() > Duration::from_secs(50) {
            (Mood::Listening, 120)
        } else {
            (Mood::Resting, 0)
        })
    };
    let report = run_standing(
        &mut lamp,
        &mut clock,
        limits,
        Some([1_771, 2_035, 2_035]),
        Some(up),
        news,
        |_| {},
    );
    assert!(
        matches!(report.stop, Stop::BadRead { joint: 1, .. }),
        "{report:?}"
    );
    assert!(report.lowered.is_some(), "{report:?}");
    assert_eq!(reads_from(&lamp, from), 3, "{report:?}");
}

/// While a shoulder left up is lowered, its servo flags a fault in the
/// step it reads 200 counts off. The hold where it is asks only for a
/// reading the arm has held for a step, so it does not write that one, and
/// the run ends on the fault.
#[test]
fn a_fault_while_lowering_the_shoulder_stands_whatever_its_hold() {
    let at = Duration::from_secs(10);
    let (mut lamp, mut clock) = folded(Duration::from_secs(70));
    lamp.may_take_hold = true;
    lamp.arm_moves = true;
    for (index, at) in [
        (YAW, 1_979.0),
        (1, 2_633.0),
        (2, 2_074.0),
        (ROLL, 2_101.0),
        (PITCH, 2_029.0),
    ] {
        let servo = &mut lamp.servos[index];
        (servo.position, servo.goal, servo.torque, servo.status) = (at, 0, 0, 0);
    }
    lamp.misreads = vec![(at, 1, 2_200, u8::MAX)];
    lamp.event = Some((at, 1, Event::Flag(0x20)));
    lamp.heavy = Some((PITCH, 1_850.0, 37.0, 232));
    let limits = Limits {
        force: Some(600),
        torque_on: true,
        wake: true,
        ..Limits::folded_with(Tuning {
            reach: [150, 15, 1, 250],
            speed: 120,
            servo_speed: 1_000,
            servo_acceleration: 60,
        })
        .unwrap()
    };
    let up = Standing::new(
        [2_053, 2_069],
        [1_772, 2_035, 2_035, 2_657, 2_602],
        100,
        5,
        90,
        [560, 560],
    )
    .unwrap();
    // Spoken to once it is awake.
    let time = clock.time.clone();
    let news = move || {
        Some(if time.get() > Duration::from_secs(50) {
            (Mood::Listening, 120)
        } else {
            (Mood::Resting, 0)
        })
    };
    let report = run_standing(
        &mut lamp,
        &mut clock,
        limits,
        Some([1_771, 2_035, 2_035]),
        Some(up),
        news,
        |_| {},
    );
    let shoulder = goals_of(&lamp, Joint::BasePitch);
    assert!(!shoulder.contains(&2_200), "{shoulder:?}");
    assert!(
        matches!(report.stop, Stop::Status { joint: 2, .. }),
        "{report:?}"
    );
}

/// The seventh's 04:03 probe on #1192: the elbow, truly at 2049 with its
/// goal register there, reads 2117 from the very first read. Its first goal
/// is that reading; the next step asks it back to the register, 68 counts
/// at once, and is refused. That ends the run at that step on `Window`,
/// not tried again as a failed write.
#[test]
fn an_arm_goal_refused_in_a_step_ends_the_run_at_that_step() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(10));
    lamp.overload_clears = 1;
    for (servo, at) in lamp
        .servos
        .iter_mut()
        .zip([2_028.0, 2_054.0, 2_049.0, 2_047.0, 1_948.0])
    {
        servo.position = at;
        servo.goal = at as u16;
        servo.status = 0;
    }
    // Wrong for the whole run: more reads than one entry gives.
    lamp.misreads = vec![(Duration::ZERO, 2, 2_117, u8::MAX); 4];
    let limits = Limits {
        wake: true,
        rest_glance: true,
        look_at_name: true,
        ..gesture_limits()
    };
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some([1_950, 2_043, 2_015]),
        Some(lamp_4ace_standing().idling(true)),
        None,
        || Some((Mood::Resting, 0)),
        Asked::default,
        |_| {},
    );
    let elbow: Vec<u16> = lamp
        .goals
        .iter()
        .flatten()
        .filter(|(id, _)| *id == 3)
        .map(|(_, goal)| *goal)
        .collect();
    assert_eq!(elbow, [2_117], "{report:?}");
    assert_eq!(
        report.stop,
        Stop::Window {
            joint: 3,
            goal: 2_049,
            window: (1_627, 3_022),
            from: 2_117,
        },
        "{report:?}"
    );
    assert_eq!(report.missed_reads, 0, "{report:?}");
}

/// The reviewer's 04:14 probe on #1192: lamp-4ace talked to from 1 to 3 s,
/// awake 5 s, so it stands, folds and comes to rest. The elbow, truly at
/// 2047, reads 2189 from somewhere between 13 and 15.5 s on. Coming to
/// rest takes the arm's resting goal from where it reads only within a
/// step of the goal it had: that reading is never written.
#[test]
fn a_steady_wrong_elbow_reading_as_the_arm_comes_to_rest_is_not_its_resting_goal() {
    for start in [13_000, 14_000, 15_000, 15_500] {
        let start = Duration::from_millis(start);
        let (mut lamp, mut clock) = folded(Duration::from_secs(20));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        for (servo, at) in lamp
            .servos
            .iter_mut()
            .zip([2_028.0, 2_054.0, 2_047.0, 2_047.0, 1_948.0])
        {
            servo.position = at;
            servo.goal = at as u16;
            servo.status = 0;
        }
        lamp.misreads = vec![(start, 2, 2_189, u8::MAX); 4];
        let up = Standing::new(
            [2_048, 2_047],
            [1_772, 2_043, 2_015, 2_657, 2_602],
            100,
            6,
            5,
            [950, 950],
        )
        .unwrap();
        let time = clock.time.clone();
        let news = move || {
            Some(
                if (Duration::from_secs(1)..Duration::from_secs(3)).contains(&time.get()) {
                    (Mood::Listening, 100)
                } else {
                    (Mood::Resting, 0)
                },
            )
        };
        let report = run_alive(
            &mut lamp,
            &mut clock,
            gesture_limits(),
            Some([1_950, 2_043, 2_015]),
            Some(up),
            None,
            news,
            Asked::default,
            |_| {},
        );
        assert!(report.stood, "{report:?}");
        let late: Vec<u16> = lamp
            .goals
            .iter()
            .zip(&lamp.goal_times)
            .filter(|(_, at)| **at >= start)
            .flat_map(|(write, _)| write.iter())
            .filter(|(id, _)| *id == 3)
            .map(|(_, goal)| *goal)
            .collect();
        // Still folding at the start or already down: from then on the
        // elbow is only ever asked down, to its rest at 2047.
        assert!(
            late.windows(2).all(|pair| pair[1] <= pair[0])
                && late.last().is_none_or(|goal| goal.abs_diff(2_047) <= 50),
            "{start:?}: {late:?} {:?}",
            report.stop
        );
    }
}

/// An arm pose whose window (420 either side) would reach past 0 or 4095
/// is refused, so no window is ever cut at the seam.
#[test]
fn an_arm_pose_whose_window_would_reach_the_seam_is_refused() {
    let pose = |folded: [u16; 2], shoulder: u16, elbow: u16| {
        Standing::new(
            folded,
            [1_772, 2_043, 2_015, shoulder, elbow],
            100,
            6,
            90,
            [950, 950],
        )
    };
    assert!(pose([2_048, 2_047], 2_657, 2_602).is_ok());
    assert!(pose([2_048, 3_400], 2_657, 3_675).is_ok());
    assert!(pose([2_048, 3_400], 2_657, 3_676).is_err());
    assert!(pose([420, 2_047], 600, 2_602).is_ok());
    assert!(pose([419, 2_047], 600, 2_602).is_err());
}

/// One lamp's standing flags: `--stand`, `--folded-arm`, `--home` and
/// `--arm-load`.
struct Flags {
    stand: [u16; 5],
    folded: [u16; 2],
    home: [u16; 3],
    load: [u16; 2],
}

const LAMP_4ACE: Flags = Flags {
    stand: [1_772, 2_043, 2_015, 2_657, 2_602],
    folded: [2_048, 2_047],
    home: [1_950, 2_043, 2_015],
    load: [950, 950],
};

const LAMP_52E6: Flags = Flags {
    stand: [1_772, 2_035, 2_035, 2_657, 2_602],
    folded: [2_053, 2_069],
    home: [1_771, 2_035, 2_035],
    load: [560, 560],
};

/// A lamp with these flags, found folded, run for `seconds` as its unit
/// runs it (give way, limp, wake, rest glance, idle), awake `awake` s after
/// it is talked to from 1 to 3 s and in each of `talks`, `prepare`d, asked
/// to nod at each of `nods`. The run's end and every arm goal write,
/// shoulder and elbow.
fn standing_session(
    flags: &Flags,
    seconds: u64,
    awake: u16,
    talks: &[(Duration, Duration)],
    nods: &[Duration],
    prepare: impl FnOnce(&mut Lamp),
) -> (BodyReport, Vec<[u16; 2]>) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(seconds));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let found = [
        flags.home[2],
        flags.folded[0],
        flags.folded[1],
        flags.home[1],
        flags.home[0],
    ];
    for (servo, at) in lamp.servos.iter_mut().zip(found) {
        servo.position = f32::from(at);
        servo.goal = at;
        servo.status = 0;
    }
    prepare(&mut lamp);
    let up = Standing::new(flags.folded, flags.stand, 100, 6, awake, flags.load)
        .unwrap()
        .idling(true);
    let limits = Limits {
        give_way: Some(GiveWay::Return),
        limp: true,
        wake: true,
        rest_glance: true,
        look_at_name: true,
        ..gesture_limits()
    };
    let time = clock.time.clone();
    let mut talks = talks.to_vec();
    talks.push((Duration::from_secs(1), Duration::from_secs(3)));
    let news = move || {
        let now = time.get();
        Some(
            if talks
                .iter()
                .any(|(from, until)| (*from..*until).contains(&now))
            {
                (Mood::Listening, 100)
            } else {
                (Mood::Resting, 0)
            },
        )
    };
    let asked_at = clock.time.clone();
    let mut due: Vec<Duration> = nods.to_vec();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some(flags.home),
        Some(up),
        None,
        news,
        move || {
            let now = asked_at.get();
            let nod = due.first().is_some_and(|at| now >= *at);
            if nod {
                due.remove(0);
            }
            Asked {
                gesture: nod.then_some(&crate::clips::NOD),
                ..Asked::default()
            }
        },
        |_| {},
    );
    let arm = lamp
        .goals
        .iter()
        .filter_map(|write| {
            let of = |id: u8| {
                write
                    .iter()
                    .find(|(at, _)| *at == id)
                    .map(|(_, goal)| *goal)
            };
            Some([of(2)?, of(3)?])
        })
        .collect();
    (report, arm)
}

/// The largest move of an arm goal from one write to the next.
fn largest_arm_step(arm: &[[u16; 2]]) -> u16 {
    arm.windows(2)
        .flat_map(|pair| (0..2).map(move |index| pair[0][index].abs_diff(pair[1][index])))
        .max()
        .unwrap_or(0)
}

/// lamp-52e6 on 2026-10-17 at 04:26 (#1192): stood up, its shoulder short
/// of its pose at 2609 and told to stop pushing there, then asked to nod.
/// The nod's first goal was 2683, 74 counts from the hold in one step, and
/// the guard let the standing lamp go (`window`, joint 2). With each lamp's
/// flags: stand, a nod while held short, rest, idle shifts (awake five
/// minutes, so that some come), fold. No arm goal moves more than
/// `ARM_GOAL_STEP` a write, and the run ends as asked.
#[test]
fn a_standing_session_never_moves_an_arm_goal_more_than_a_step_a_write() {
    for (name, flags) in [("lamp-4ace", &LAMP_4ACE), ("lamp-52e6", &LAMP_52E6)] {
        let nod = [Duration::from_secs(8)];
        let (report, arm) = standing_session(flags, 400, 300, &[], &nod, |lamp| {
            lamp.wall = Some((1, 2_609.0));
        });
        assert!(report.stalls[3] >= 1, "{name}: {report:?}");
        assert!(report.idles >= 1, "{name}: {report:?}");
        assert!(report.gestures >= 1, "{name}: {report:?}");
        assert_eq!(report.stop, Stop::Cancelled, "{name}: {report:?}");
        assert!(
            largest_arm_step(&arm) <= ARM_GOAL_STEP,
            "{name}: {}",
            largest_arm_step(&arm)
        );
    }
}

/// A nod asked right after a hand lets go of the folded elbow it lifted
/// 150 counts and held: the arm is still on its way back down, and the lamp
/// stands up for the nod from there. With each lamp's flags (awake 5 s, so
/// it is folded at rest by then), no arm goal moves more than a step a
/// write except the hold where the hand put it, and no `Window` stop.
#[test]
fn a_nod_right_after_a_hand_lets_go_of_the_arm_does_not_stop_the_lamp() {
    for (name, flags) in [("lamp-4ace", &LAMP_4ACE), ("lamp-52e6", &LAMP_52E6)] {
        let ms = Duration::from_millis;
        let nod = [ms(25_200)];
        let (report, _) = standing_session(flags, 45, 5, &[], &nod, |lamp| {
            let rest = f32::from(flags.folded[1]);
            lamp.hand = Some((ms(20_000), ms(24_000), 2, rest + 150.0));
            lamp.strains = vec![(ms(20_000), ms(24_000), 2, 200)];
        });
        assert!(report.stalls[4] >= 1, "{name}: {report:?}");
        assert!(report.gestures >= 1, "{name}: {report:?}");
        assert_eq!(report.stop, Stop::Cancelled, "{name}: {report:?}");
    }
}

/// A give-way hold on the arm let go by the rise: the folded elbow pushed
/// 80 counts down by a hand and held there, then the lamp is talked to and
/// rises, which lets the hold go 100 counts from where the rise wants the
/// elbow. With each lamp's flags it eases there, and no `Window` stop.
#[test]
fn a_hold_let_go_by_the_rise_eases_and_does_not_stop_the_lamp() {
    for (name, flags) in [("lamp-4ace", &LAMP_4ACE), ("lamp-52e6", &LAMP_52E6)] {
        let ms = Duration::from_millis;
        let talk = [(ms(23_500), ms(26_000))];
        let (report, _) = standing_session(flags, 45, 5, &talk, &[], |lamp| {
            let rest = f32::from(flags.folded[1]);
            lamp.hand = Some((ms(20_000), ms(24_000), 2, rest - 80.0));
            lamp.strains = vec![(ms(20_000), ms(24_000), 2, 200)];
        });
        assert!(report.stalls[4] >= 1, "{name}: {report:?}");
        assert!(
            !matches!(report.stop, Stop::Window { .. }),
            "{name}: {report:?}"
        );
    }
}

/// The same let go by the fold: the standing elbow pushed 80 counts up by
/// a hand and held there, then the lamp's awake time (20 s) runs out and it
/// folds.
#[test]
fn a_hold_let_go_by_the_fold_eases_and_does_not_stop_the_lamp() {
    for (name, flags) in [("lamp-4ace", &LAMP_4ACE), ("lamp-52e6", &LAMP_52E6)] {
        let ms = Duration::from_millis;
        let (report, _) = standing_session(flags, 45, 20, &[], &[], |lamp| {
            let pose = f32::from(flags.stand[4]);
            lamp.hand = Some((ms(12_000), ms(20_000), 2, pose + 80.0));
            lamp.strains = vec![(ms(12_000), ms(20_000), 2, 450)];
        });
        assert!(report.stalls[4] >= 1, "{name}: {report:?}");
        assert!(
            !matches!(report.stop, Stop::Window { .. }),
            "{name}: {report:?}"
        );
    }
}

/// The reviewer's 04:39 probe on #1192: the folded elbow truly rests 60
/// counts short of its fold (on a mat, a stiff joint) and pushes there. As
/// it comes to rest it is left where it lies, at 2107, not walked back to
/// 2047 to push again, and the run ends as asked rather than on `Window` at
/// the exit; a misread with nothing pushing keeps its goal (the test above).
#[test]
fn an_arm_resting_short_of_its_fold_and_pushing_is_left_where_it_lies() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(30));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    for (servo, at) in lamp
        .servos
        .iter_mut()
        .zip([2_028.0, 2_054.0, 2_047.0, 2_047.0, 1_948.0])
    {
        servo.position = at;
        servo.goal = at as u16;
        servo.status = 0;
    }
    lamp.short = Some((2, 60.0));
    lamp.strains = vec![(
        Duration::from_millis(14_700),
        Duration::from_secs(20),
        2,
        160,
    )];
    let up = Standing::new(
        [2_048, 2_047],
        [1_772, 2_043, 2_015, 2_657, 2_602],
        100,
        6,
        5,
        [950, 950],
    )
    .unwrap();
    let time = clock.time.clone();
    let news = move || {
        Some(
            if (Duration::from_secs(1)..Duration::from_secs(3)).contains(&time.get()) {
                (Mood::Listening, 100)
            } else {
                (Mood::Resting, 0)
            },
        )
    };
    let limits = Limits {
        give_way: Some(GiveWay::Return),
        ..gesture_limits()
    };
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some([1_950, 2_043, 2_015]),
        Some(up),
        None,
        news,
        Asked::default,
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let elbow: Vec<u16> = lamp
        .goals
        .iter()
        .flatten()
        .filter(|(id, _)| *id == 3)
        .map(|(_, goal)| *goal)
        .collect();
    assert_eq!(elbow.last(), Some(&2_107), "{elbow:?}");
}

/// The reviewer's 04:53 probe on #1192: an elbow goal outside its window
/// (4095 on lamp-4ace) is not eased toward, 50 counts a step, to the edge:
/// it is left for the guard, which refuses it at once, nothing written.
#[test]
fn an_arm_goal_outside_its_window_is_not_eased_toward() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(1));
    lamp.servos[2].status = 0;
    {
        let mut bus = Bus::new(&mut lamp, &mut clock, Some(lamp_4ace_standing()));
        bus.samples().unwrap();
        bus.send_goals(HOME, Some([2_050, 2_047])).unwrap();
        let goal = bus.eased(Joint::ElbowPitch, 2_047, 4_095, false);
        assert_eq!(goal, 4_095);
        assert!(bus.send_goals(HOME, Some([2_050, goal])).is_err());
        assert_eq!(
            bus.refused.take(),
            Some(Stop::Window {
                joint: 3,
                goal: 4_095,
                window: (1_627, 3_022),
                from: 2_047,
            })
        );
        // Inside the window it still eases.
        assert_eq!(bus.eased(Joint::ElbowPitch, 2_047, 2_400, false), 2_097);
    }
    let elbow: Vec<u16> = lamp
        .goals
        .iter()
        .flatten()
        .filter(|(id, _)| *id == 3)
        .map(|(_, goal)| *goal)
        .collect();
    assert_eq!(elbow, [2_047]);
}

/// The seventh's 05:12 probe on #1192: the same misread of 2189 while the
/// elbow also reads a load of 160. It reached that reading by a jump, not
/// by coming down, so it is still not its resting goal.
#[test]
fn a_steady_wrong_elbow_reading_under_load_as_the_arm_comes_to_rest_is_not_its_resting_goal() {
    for start in [13_000, 14_000, 15_000, 15_500] {
        let start = Duration::from_millis(start);
        let (mut lamp, mut clock) = folded(Duration::from_secs(20));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        for (servo, at) in lamp
            .servos
            .iter_mut()
            .zip([2_028.0, 2_054.0, 2_047.0, 2_047.0, 1_948.0])
        {
            servo.position = at;
            servo.goal = at as u16;
            servo.status = 0;
        }
        lamp.misreads = vec![(start, 2, 2_189, u8::MAX); 4];
        lamp.strains = vec![(start, Duration::from_secs(20), 2, 160)];
        let up = Standing::new(
            [2_048, 2_047],
            [1_772, 2_043, 2_015, 2_657, 2_602],
            100,
            6,
            5,
            [950, 950],
        )
        .unwrap();
        let time = clock.time.clone();
        let news = move || {
            Some(
                if (Duration::from_secs(1)..Duration::from_secs(3)).contains(&time.get()) {
                    (Mood::Listening, 100)
                } else {
                    (Mood::Resting, 0)
                },
            )
        };
        let report = run_alive(
            &mut lamp,
            &mut clock,
            gesture_limits(),
            Some([1_950, 2_043, 2_015]),
            Some(up),
            None,
            news,
            Asked::default,
            |_| {},
        );
        assert!(report.stood, "{report:?}");
        let late: Vec<u16> = lamp
            .goals
            .iter()
            .zip(&lamp.goal_times)
            .filter(|(_, at)| **at >= start)
            .flat_map(|(write, _)| write.iter())
            .filter(|(id, _)| *id == 3)
            .map(|(_, goal)| *goal)
            .collect();
        // Still folding at the start or already down: from then on the
        // elbow is only ever asked down, to its rest at 2047.
        assert!(
            late.windows(2).all(|pair| pair[1] <= pair[0])
                && late.last().is_none_or(|goal| goal.abs_diff(2_047) <= 50),
            "{start:?}: {late:?} {:?}",
            report.stop
        );
    }
}

/// The reviewer's 05:32 probe on #1192: lamp-4ace folded at rest, its
/// elbow asked 2900 at once, inside its window but 851 from its goal. It is
/// not walked there 50 a step: it is refused at once, nothing written.
#[test]
fn an_arm_goal_farther_than_a_hold_release_is_refused_not_eased_toward() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(1));
    lamp.servos[2].status = 0;
    lamp.servos[2].position = 2_049.0;
    {
        let mut bus = Bus::new(&mut lamp, &mut clock, Some(lamp_4ace_standing()));
        bus.samples().unwrap();
        bus.send_goals(HOME, Some([2_050, 2_049])).unwrap();
        let goal = bus.eased(Joint::ElbowPitch, 2_049, 2_900, false);
        assert_eq!(goal, 2_900);
        assert!(bus.send_goals(HOME, Some([2_050, goal])).is_err());
        assert_eq!(
            bus.refused.take(),
            Some(Stop::Window {
                joint: 3,
                goal: 2_900,
                window: (1_627, 3_022),
                from: 2_049,
            })
        );
        // A hold let go, 318 off at most in the runs measured, still eases.
        assert_eq!(bus.eased(Joint::ElbowPitch, 2_049, 2_367, false), 2_099);
    }
    let elbow: Vec<u16> = lamp
        .goals
        .iter()
        .flatten()
        .filter(|(id, _)| *id == 3)
        .map(|(_, goal)| *goal)
        .collect();
    assert_eq!(elbow, [2_049]);
}

/// The reviewer's 05:51 probe on #1192: a folded elbow at rest asked 2449,
/// 400 from its goal and inside both its window and `ARM_EASE_REACH`. A
/// resting arm has no hold to let go, so nothing eases it there: the guard
/// refuses it at once, nothing written.
#[test]
fn a_resting_arm_goal_more_than_a_step_away_is_refused_not_eased_toward() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(1));
    lamp.servos[2].status = 0;
    lamp.servos[2].position = 2_049.0;
    {
        let mut bus = Bus::new(&mut lamp, &mut clock, Some(lamp_4ace_standing()));
        bus.samples().unwrap();
        bus.send_goals(HOME, Some([2_050, 2_049])).unwrap();
        assert_eq!(bus.eased(Joint::ElbowPitch, 2_049, 2_449, false), 2_099);
        let goal = bus.eased(Joint::ElbowPitch, 2_049, 2_449, true);
        assert_eq!(goal, 2_449);
        assert!(bus.send_goals(HOME, Some([2_050, goal])).is_err());
        assert!(matches!(
            bus.refused.take(),
            Some(Stop::Window {
                joint: 3,
                goal: 2_449,
                from: 2_049,
                ..
            })
        ));
    }
    let elbow: Vec<u16> = lamp
        .goals
        .iter()
        .flatten()
        .filter(|(id, _)| *id == 3)
        .map(|(_, goal)| *goal)
        .collect();
    assert_eq!(elbow, [2_049]);
}

/// Under `Limits::arbitration` (#940) or not, a lamp that stands at once
/// and is spoken to throughout, asked for each gesture at its moment and
/// priority. What was said of each gesture, and each step: when, the
/// gesture being made, the head's goals and the arm's.
fn arbitrated_run(
    arbitration: bool,
    asks: Vec<(Duration, &'static Clip, Priority)>,
    seconds: u64,
) -> (BodyReport, Vec<(&'static str, &'static str)>, Vec<Step>) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(seconds));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let limits = Limits {
        arbitration,
        ..gesture_limits()
    };
    let time = clock.time.clone();
    let seen_at = clock.time.clone();
    let mut asks = VecDeque::from(asks);
    let (mut fates, mut seen) = (Vec::new(), Vec::new());
    let report = run_arbitrated(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, 60)),
        None,
        || Some((Mood::Speaking, 60)),
        move || match asks.front() {
            Some((at, clip, by)) if time.get() >= *at => {
                let asked = Asked {
                    gesture: Some(*clip),
                    ..Asked::default()
                };
                let by = *by;
                asks.pop_front();
                (asked, by)
            }
            _ => (Asked::default(), Priority::Conversation),
        },
        |glimpse| {
            fates.extend(glimpse.gesture_fates);
            seen.push((
                seen_at.get(),
                glimpse.gesture,
                glimpse.goals,
                glimpse.arm_goals,
            ));
        },
    );
    (report, fates, seen)
}

const PRIORITIES: [Priority; 5] = [
    Priority::Idle,
    Priority::Skill,
    Priority::Conversation,
    Priority::Safety,
    Priority::Person,
];

/// Every pair: a nod asked while a stretch plays ends it when asked
/// higher and plays, and is dropped, naming who holds the body, when
/// asked as high or lower. Without the switch, whoever asks, it is
/// dropped as before.
#[test]
fn a_gesture_asked_higher_ends_the_one_playing_and_one_asked_no_higher_is_dropped() {
    let secs = Duration::from_secs;
    for holder in PRIORITIES {
        for asker in PRIORITIES {
            let asks = vec![
                (secs(8), &crate::clips::STRETCHING, holder),
                (secs(10), &crate::clips::NOD, asker),
            ];
            let (report, fates, _) = arbitrated_run(true, asks, 12);
            assert_eq!(report.stop, Stop::Cancelled, "{holder:?} {asker:?}");
            if asker > holder {
                assert_eq!(
                    fates,
                    [
                        ("stretching", "taken: it plays"),
                        ("stretching", crate::moves::ended(asker)),
                        ("nod", "taken: it plays"),
                    ],
                    "{holder:?} {asker:?}"
                );
                assert_eq!(report.gestures, 2);
            } else {
                assert_eq!(
                    fates,
                    [
                        ("stretching", "taken: it plays"),
                        ("nod", crate::moves::busy(holder))
                    ],
                    "{holder:?} {asker:?}"
                );
                assert_eq!(report.gestures, 1);
            }
        }
    }
    let asks = vec![
        (secs(8), &crate::clips::STRETCHING, Priority::Skill),
        (secs(10), &crate::clips::NOD, Priority::Person),
    ];
    let (_, fates, _) = arbitrated_run(false, asks, 12);
    assert_eq!(
        fates,
        [
            ("stretching", "taken: it plays"),
            ("nod", "dropped: another gesture plays"),
        ]
    );
}

/// Under arbitration, a gesture asked of a lamp a hand put to sleep (#1362)
/// is dropped as asleep however high it is asked: it does not end the one
/// already playing and play on a sleeping lamp.
#[test]
fn asleep_a_gesture_asked_higher_does_not_end_the_one_playing() {
    let secs = Duration::from_secs;
    let (mut lamp, mut clock) = folded(secs(14));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let limits = Limits {
        arbitration: true,
        ..gesture_limits()
    };
    let time = clock.time.clone();
    let moods = clock.time.clone();
    let mut asks = VecDeque::from([
        (secs(8), &crate::clips::STRETCHING, Priority::Skill),
        (
            Duration::from_millis(9_500),
            &crate::clips::NOD,
            Priority::Person,
        ),
    ]);
    let mut fates = Vec::new();
    let report = run_arbitrated(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, 60)),
        None,
        // Speaking until the hand puts it to sleep at nine seconds.
        move || {
            Some(if moods.get() < secs(9) {
                (Mood::Speaking, 60)
            } else {
                (Mood::Resting, 0)
            })
        },
        move || {
            let now = time.get();
            let base = if now >= secs(9) {
                Asked::ASLEEP
            } else {
                Asked::default()
            };
            match asks.front() {
                Some((at, clip, by)) if now >= *at => {
                    let by = *by;
                    let asked = Asked {
                        gesture: Some(*clip),
                        ..base
                    };
                    asks.pop_front();
                    (asked, by)
                }
                _ => (base, Priority::Conversation),
            }
        },
        |glimpse| fates.extend(glimpse.gesture_fates),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(
        fates,
        [
            ("stretching", "taken: it plays"),
            ("nod", "dropped: asleep"),
        ]
    );
    assert_eq!(report.gestures, 1);
}

/// An ask at a time, of a clip, at a priority, naming its asker or nobody.
type NamedAsk = (Duration, &'static Clip, Priority, Option<&'static str>);
/// A fate with whom it is told of.
type ToldFate = ((&'static str, &'static str), Told);

/// [`arbitrated_run`]'s run with arbitration, each ask naming its asker or
/// nobody, through [`run_told`]: each fate with whom it is told of, and
/// each step's gesture and goals.
fn told_run(asks: Vec<NamedAsk>, seconds: u64) -> (Vec<ToldFate>, Vec<Step>) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(seconds));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let limits = Limits {
        arbitration: true,
        ..gesture_limits()
    };
    let time = clock.time.clone();
    let seen_at = clock.time.clone();
    let mut asks = VecDeque::from(asks);
    let (mut fates, mut seen) = (Vec::new(), Vec::new());
    run_told(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, 60)),
        None,
        || Some((Mood::Speaking, 60)),
        move || match asks.front() {
            Some((at, clip, by, source)) if time.get() >= *at => {
                let asked = Asked {
                    gesture: Some(*clip),
                    ..Asked::default()
                };
                let told = (asked, *by, source.and_then(Source::new));
                asks.pop_front();
                told
            }
            _ => (Asked::default(), Priority::Conversation, None),
        },
        |glimpse| {
            fates.extend(glimpse.gesture_fates.iter_told());
            seen.push((
                seen_at.get(),
                glimpse.gesture,
                glimpse.goals,
                glimpse.arm_goals,
            ));
        },
    );
    (fates, seen)
}

/// #940, the fifth's conditions: a gesture dropped or ended says whose it
/// was and who holds the body, as their datagrams named them; a holder
/// that named nobody is not named. Who wins, and every goal written, is
/// exactly as without names.
#[test]
fn a_gesture_dropped_or_ended_is_told_whose_it_was_and_who_holds_the_body() {
    let (secs, ms) = (Duration::from_secs, Duration::from_millis);
    let named = |text| Source::new(text);
    let taken = |name| ((name, "taken: it plays"), Told::default());
    let stretch = &crate::clips::STRETCHING;
    let nod = &crate::clips::NOD;
    let cases: [(Vec<NamedAsk>, _); 3] = [
        // Two skills: the second is dropped, and told which holds the body.
        (
            vec![
                (
                    secs(8),
                    stretch,
                    Priority::Skill,
                    Some("skill:stretch-break"),
                ),
                (secs(10), nod, Priority::Skill, Some("skill:dance")),
            ],
            vec![
                taken("stretching"),
                (
                    ("nod", "dropped: busy, skill"),
                    Told {
                        source: named("skill:dance"),
                        by: named("skill:stretch-break"),
                    },
                ),
            ],
        ),
        // The conversation ends a skill's move: the skill is told by whom.
        (
            vec![
                (
                    secs(8),
                    stretch,
                    Priority::Skill,
                    Some("skill:stretch-break"),
                ),
                (ms(11_200), nod, Priority::Conversation, Some("voice")),
            ],
            vec![
                taken("stretching"),
                (
                    ("stretching", "ended: by conversation"),
                    Told {
                        source: named("skill:stretch-break"),
                        by: named("voice"),
                    },
                ),
                taken("nod"),
            ],
        ),
        // A gesture of the conversation's that named nobody holds it.
        (
            vec![
                (ms(500), nod, Priority::Conversation, None),
                (ms(1_500), stretch, Priority::Skill, Some("skill:dance")),
            ],
            vec![
                (
                    ("stretching", "dropped: busy, conversation"),
                    Told {
                        source: named("skill:dance"),
                        by: None,
                    },
                ),
                taken("nod"),
            ],
        ),
    ];
    for (asks, told) in cases {
        let unnamed = asks
            .iter()
            .map(|(at, clip, by, _)| (*at, *clip, *by))
            .collect();
        let (_, fates, moved) = arbitrated_run(true, unnamed, 16);
        let (got, seen) = told_run(asks, 16);
        assert_eq!(got, told);
        assert_eq!(
            got.iter().map(|(fate, _)| *fate).collect::<Vec<_>>(),
            fates,
            "the same fates as without names"
        );
        // And the same motion, step by step: the names only report (the
        // fifth's first condition, the reviewer on #1473).
        assert_eq!(seen, moved, "the same gestures and goals as without names");
    }
}

/// A skill's move asked while a conversation's gesture waits for the lamp
/// to stand does not take its place; one asked higher would.
#[test]
fn a_skill_asked_while_a_conversation_gesture_waits_is_dropped() {
    let ms = Duration::from_millis;
    let asks = vec![
        (ms(500), &crate::clips::NOD, Priority::Conversation),
        (ms(1_500), &crate::clips::STRETCHING, Priority::Skill),
    ];
    let (report, fates, _) = arbitrated_run(true, asks.clone(), 10);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(
        fates,
        [
            ("stretching", "dropped: busy, conversation"),
            ("nod", "taken: it plays"),
        ]
    );
    let (_, fates, _) = arbitrated_run(false, asks, 10);
    assert_eq!(
        fates,
        [
            ("nod", "dropped: another was asked after it"),
            ("stretching", "taken: it plays"),
        ]
    );
}

/// A skill's stretch ended by the conversation's nod at its widest eases
/// out at the gesture's own step, under the arm's 50 counts a step and
/// with nothing refused at the wire, and the nod begins once it has.
#[test]
fn a_skill_move_ended_for_a_conversation_gesture_eases_out_and_then_it_plays() {
    let secs = Duration::from_secs;
    let asks = vec![
        (secs(8), &crate::clips::STRETCHING, Priority::Skill),
        (
            Duration::from_millis(11_200),
            &crate::clips::NOD,
            Priority::Conversation,
        ),
    ];
    let (report, fates, seen) = arbitrated_run(true, asks, 16);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.gestures, 2);
    assert!(
        fates.contains(&("stretching", "ended: by conversation")),
        "{fates:?}"
    );
    let ended = seen
        .iter()
        .position(|step| step.0 >= Duration::from_millis(11_200))
        .unwrap();
    let nod = seen.iter().position(|step| step.1 == Some("nod")).unwrap();
    // The stretch had the arm far out: easing it back took a while, with
    // no gesture made meanwhile.
    assert!(nod - ended >= 5, "{ended} {nod}");
    assert!(seen[ended..nod].iter().all(|step| step.1.is_none()));
    for pair in seen[ended - 1..].windows(2) {
        let (before, after) = (&pair[0], &pair[1]);
        for joint in 0..3 {
            assert!(after.2[joint].abs_diff(before.2[joint]) <= 50, "{pair:?}");
        }
        for joint in 0..2 {
            assert!(after.3[joint].abs_diff(before.3[joint]) <= 50, "{pair:?}");
        }
    }
    // A real ease, not a snap back: from the step before it ended until the
    // nod begins, each joint's change a step differs from the one before by
    // a few counts at most, and the last step into nothing is a small one.
    // The arm, the head's roll and the base's turn carry nothing else; the
    // head's pitch also nods with the voice.
    let moves = |pick: &dyn Fn(&Step) -> f32| -> Vec<f32> {
        seen[ended - 2..nod]
            .windows(2)
            .map(|pair| pick(&pair[1]) - pick(&pair[0]))
            .collect()
    };
    for (joint, change) in [
        moves(&|step| f32::from(step.3[0])),
        moves(&|step| f32::from(step.3[1])),
        moves(&|step| f32::from(step.2[0])),
    ]
    .into_iter()
    .enumerate()
    {
        for pair in change.windows(2) {
            assert!((pair[1] - pair[0]).abs() <= 4.0, "{joint}: {change:?}");
        }
        assert!(change.last().unwrap().abs() <= 4.0, "{joint}: {change:?}");
    }
}

/// A speaking lamp leaned in when a skill's stretch begins: the lean
/// eases out under the stretch, the stretch ended by the conversation's
/// nod eases back on top of it, and the lean comes back after the nod.
/// Every step of the arm stays within the 50 counts the wire takes, so
/// the run ends as asked and not on the wire.
#[test]
fn a_speaking_lean_and_an_arbitration_ease_out_never_step_past_the_arm() {
    let ms = Duration::from_millis;
    let asks = vec![
        (ms(8_000), &crate::clips::STRETCHING, Priority::Skill),
        (ms(11_200), &crate::clips::NOD, Priority::Conversation),
    ];
    let (report, fates, seen) = arbitrated_run(true, asks, 18);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(
        fates.contains(&("stretching", "ended: by conversation")),
        "{fates:?}"
    );
    let at = |when: Duration| seen.iter().rfind(|step| step.0 < when).unwrap().3;
    let stood = at(ms(5_000));
    for (when, share) in [(ms(7_900), 1.0), (ms(11_900), 0.0), (ms(16_900), 1.0)] {
        for (index, lean) in LEAN.iter().enumerate() {
            let by = f32::from(at(when)[index]) - f32::from(stood[index]);
            assert!((by - lean * share).abs() <= 2.0, "{when:?} {:?}", at(when));
        }
    }
    for pair in seen.windows(2) {
        for joint in 0..2 {
            assert!(
                pair[1].3[joint].abs_diff(pair[0].3[joint]) <= 50,
                "{pair:?}"
            );
        }
    }
}

/// A lamp at rest asked for one gesture at 1 s, at this priority, and
/// spoken to (`listening`) from `spoken_at` on, if ever: what was said
/// of the gesture, and each step.
fn spoken_to_run(
    arbitration: bool,
    clip: &'static Clip,
    by: Priority,
    spoken_at: Option<Duration>,
) -> (BodyReport, Vec<(&'static str, &'static str)>, Vec<Step>) {
    let moods: Vec<(Duration, Mood)> = spoken_at
        .map(|at| (at, Mood::Listening))
        .into_iter()
        .collect();
    moods_run(arbitration, false, clip, by, moods)
}

/// The same under `LAMP_GESTURE_PAUSE=on` (#2602).
fn paused_run(
    arbitration: bool,
    clip: &'static Clip,
    by: Priority,
    spoken_at: Option<Duration>,
) -> (BodyReport, Vec<(&'static str, &'static str)>, Vec<Step>) {
    let moods: Vec<(Duration, Mood)> = spoken_at
        .map(|at| (at, Mood::Listening))
        .into_iter()
        .collect();
    moods_run(arbitration, true, clip, by, moods)
}

/// The same, the voice's mood each from its time on (resting before the
/// first), with the pause on or off: `listening` then `thinking` is a line
/// the lamp answers, `listening` then `resting` one it left alone (#2602).
fn moods_run(
    arbitration: bool,
    pause: bool,
    clip: &'static Clip,
    by: Priority,
    moods: Vec<(Duration, Mood)>,
) -> (BodyReport, Vec<(&'static str, &'static str)>, Vec<Step>) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(16));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let limits = Limits {
        arbitration,
        gesture_pause: pause,
        ..gesture_limits()
    };
    let (time, mood_time, seen_at) = (clock.time.clone(), clock.time.clone(), clock.time.clone());
    let mut asked = false;
    let (mut fates, mut seen) = (Vec::new(), Vec::new());
    let report = run_arbitrated(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, 60)),
        None,
        move || {
            let now = mood_time.get();
            match moods.iter().rev().find(|(at, _)| now >= *at) {
                Some((_, Mood::Resting)) | None => Some((Mood::Resting, 0)),
                Some((_, mood)) => Some((*mood, 60)),
            }
        },
        move || {
            let due = !asked && time.get() >= Duration::from_secs(1);
            asked |= due;
            let asked = Asked {
                gesture: due.then_some(clip),
                ..Asked::default()
            };
            (asked, by)
        },
        |glimpse| {
            fates.extend(glimpse.gesture_fates);
            seen.push((
                seen_at.get(),
                glimpse.gesture,
                glimpse.goals,
                glimpse.arm_goals,
            ));
        },
    );
    (report, fates, seen)
}

/// The joys' stretch (lamp-4ace, 21 October): a skill's `stretching`
/// played on for 3 s after the person spoke. An unprompted gesture, a
/// skill's or idle life's, ends the moment a conversation begins, with
/// the switch on or off, and eases out as one ended for a higher ask
/// does; the conversation's own gesture plays on.
#[test]
fn a_skill_or_idle_gesture_playing_ends_when_a_conversation_begins_and_eases_out() {
    let spoken = Duration::from_secs(8);
    for arbitration in [true, false] {
        for by in [Priority::Skill, Priority::Idle] {
            let stretch = &crate::clips::STRETCHING;
            let (report, fates, seen) = spoken_to_run(arbitration, stretch, by, Some(spoken));
            assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
            assert_eq!(
                fates,
                [
                    ("stretching", "taken: it was folded and stood up for it"),
                    ("stretching", "ended: by conversation"),
                ],
                "{arbitration} {by:?}"
            );
            // It played until the word and not a step after.
            let ended = seen.iter().position(|step| step.0 >= spoken).unwrap();
            assert_eq!(seen[ended - 1].1, Some("stretching"));
            assert!(seen[ended..].iter().all(|step| step.1.is_none()));
            // Eased out, not snapped back: the arm's change a step grows and
            // shrinks by a few counts and never jumps.
            for joint in 0..2 {
                let change: Vec<f32> = seen[ended - 2..ended + 20]
                    .windows(2)
                    .map(|pair| f32::from(pair[1].3[joint]) - f32::from(pair[0].3[joint]))
                    .collect();
                for pair in change.windows(2) {
                    assert!((pair[1] - pair[0]).abs() <= 4.0, "{joint}: {change:?}");
                }
            }
            // Never spoken to, it plays out.
            let (_, fates, _) = spoken_to_run(arbitration, stretch, by, None);
            assert_eq!(
                fates,
                [("stretching", "taken: it was folded and stood up for it")],
                "{arbitration} {by:?}"
            );
        }
        // The conversation's own nod, a person's or safety's, plays on.
        for by in [Priority::Conversation, Priority::Safety, Priority::Person] {
            let stretch = &crate::clips::STRETCHING;
            let (_, fates, seen) = spoken_to_run(arbitration, stretch, by, Some(spoken));
            assert_eq!(
                fates,
                [("stretching", "taken: it was folded and stood up for it")],
                "{arbitration} {by:?}"
            );
            assert!(seen.iter().any(|step| step.0 > spoken && step.1.is_some()));
        }
    }
}

/// No wait between the word and the ease: the step that reads `listening`
/// is the stretch's first eased one, so the lamp stops within a motor step
/// of the mood (the owner's 300 ms from "Lamp," to the stretch giving way).
#[test]
fn a_skill_gesture_eases_out_on_the_very_step_the_listening_mood_is_read() {
    let spoken = Duration::from_secs(8);
    let stretch = &crate::clips::STRETCHING;
    let (_, _, played) = spoken_to_run(false, stretch, Priority::Skill, None);
    let (_, _, seen) = spoken_to_run(false, stretch, Priority::Skill, Some(spoken));
    let read = seen.iter().position(|step| step.0 >= spoken).unwrap();
    assert_eq!(seen[..read], played[..read], "the same until the word");
    assert_eq!((seen[read - 1].1, seen[read].1), (Some("stretching"), None));
    // The arm, which only the stretch moves, already leaves the stretch's
    // path on that step.
    assert_ne!(seen[read].3, played[read].3);
}

/// Under `LAMP_GESTURE_PAUSE=on` (#2602), the same stretch pauses as the
/// lamp listens and ends the moment it answers (`thinking`), with the
/// arbitration switch on or off, eased out as above; the conversation's
/// own gesture plays on.
#[test]
fn a_paused_skill_or_idle_gesture_ends_when_the_lamp_answers_and_eases_out() {
    let (heard, answers) = (Duration::from_secs(8), Duration::from_millis(9_200));
    let spoken = answers;
    for arbitration in [true, false] {
        for by in [Priority::Skill, Priority::Idle] {
            let stretch = &crate::clips::STRETCHING;
            let moods = vec![(heard, Mood::Listening), (answers, Mood::Thinking)];
            let (report, fates, seen) = moods_run(arbitration, true, stretch, by, moods);
            assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
            assert_eq!(
                fates,
                [
                    ("stretching", "taken: it was folded and stood up for it"),
                    ("stretching", "ended: by conversation"),
                ],
                "{arbitration} {by:?}"
            );
            // It held, paused, until the answer, and not a step after.
            let ended = seen.iter().position(|step| step.0 >= spoken).unwrap();
            assert_eq!(seen[ended - 1].1, Some("stretching"));
            assert!(seen[ended..].iter().all(|step| step.1.is_none()));
            // Eased out, not snapped back: the arm's change a step grows and
            // shrinks by a few counts and never jumps.
            for joint in 0..2 {
                let change: Vec<f32> = seen[ended - 2..ended + 20]
                    .windows(2)
                    .map(|pair| f32::from(pair[1].3[joint]) - f32::from(pair[0].3[joint]))
                    .collect();
                for pair in change.windows(2) {
                    assert!((pair[1] - pair[0]).abs() <= 4.0, "{joint}: {change:?}");
                }
            }
            // Never spoken to, it plays out.
            let (_, fates, _) = paused_run(arbitration, stretch, by, None);
            assert_eq!(
                fates,
                [("stretching", "taken: it was folded and stood up for it")],
                "{arbitration} {by:?}"
            );
        }
        // The conversation's own nod, a person's or safety's, plays on.
        for by in [Priority::Conversation, Priority::Safety, Priority::Person] {
            let stretch = &crate::clips::STRETCHING;
            let (_, fates, seen) = paused_run(arbitration, stretch, by, Some(spoken));
            assert_eq!(
                fates,
                [("stretching", "taken: it was folded and stood up for it")],
                "{arbitration} {by:?}"
            );
            assert!(seen.iter().any(|step| step.0 > spoken && step.1.is_some()));
        }
    }
}

/// No wait between the word and the pause: the step that reads `listening`
/// is the stretch's first eased one, so the lamp stops within a motor step
/// of the mood (the owner's 300 ms from "Lamp," to the stretch giving way),
/// as before the pause (#2602). It is still the stretch's, paused.
#[test]
fn a_skill_gesture_pauses_on_the_very_step_the_listening_mood_is_read() {
    let spoken = Duration::from_secs(8);
    let stretch = &crate::clips::STRETCHING;
    let (_, _, played) = paused_run(false, stretch, Priority::Skill, None);
    let (_, _, seen) = paused_run(false, stretch, Priority::Skill, Some(spoken));
    let read = seen.iter().position(|step| step.0 >= spoken).unwrap();
    assert_eq!(seen[..read], played[..read], "the same until the word");
    assert_eq!(
        (seen[read - 1].1, seen[read].1),
        (Some("stretching"), Some("stretching"))
    );
    // The arm, which only the stretch moves, already leaves the stretch's
    // path on that step.
    assert_ne!(seen[read].3, played[read].3);
}

/// #2602: a line the lamp leaves alone (music, a cough, not said to it)
/// pauses the stretch, and it goes on from where it was once the voice is
/// resting again: never ended, played out later by the pause. While
/// paused the arm comes to a hold and stays there, never snapping.
#[test]
fn a_skill_gesture_paused_for_a_line_left_alone_goes_on_where_it_was() {
    let ms = Duration::from_millis;
    let stretch = &crate::clips::STRETCHING;
    for arbitration in [true, false] {
        let (_, played_fates, played) = paused_run(arbitration, stretch, Priority::Skill, None);
        let moods = vec![(ms(8_000), Mood::Listening), (ms(9_500), Mood::Resting)];
        let (report, fates, seen) = moods_run(arbitration, true, stretch, Priority::Skill, moods);
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        assert_eq!(fates, played_fates, "{arbitration}: not ended");
        // It played out the hold later than unpaused, less the third of the
        // ease the hold carried it on (`EASE_OUT` / 3), give or take a
        // step: it goes on from where it was held.
        let over = |steps: &[Step]| {
            steps
                .iter()
                .rposition(|step| step.1 == Some("stretching"))
                .map(|at| steps[at].0)
                .unwrap()
        };
        let late = over(&seen).saturating_sub(over(&played));
        assert!(
            late >= ms(1_167) && late <= ms(1_367),
            "{arbitration}: {late:?}"
        );
        // Held: from 0.7 s into the pause to its end the arm is still.
        let held: Vec<_> = seen
            .iter()
            .filter(|step| step.0 >= ms(8_750) && step.0 < ms(9_500))
            .map(|step| step.3)
            .collect();
        assert!(
            held.windows(2).all(|pair| pair[0] == pair[1]),
            "{arbitration}: {held:?}"
        );
        // Into the hold and out of it again, the arm's change a step never
        // jumps (the hold begins at 8 s and goes on at 9.5 s).
        let at = seen.iter().position(|step| step.0 >= ms(8_000)).unwrap();
        let on = seen.iter().position(|step| step.0 >= ms(9_500)).unwrap();
        for joint in 0..2 {
            // Going on, it never swings back: the arm moves on the way it
            // was moving as it paused, not back toward where it paused.
            let way = f32::from(seen[at].3[joint]) - f32::from(seen[at - 1].3[joint]);
            for pair in seen[on..on + 18].windows(2) {
                let step = f32::from(pair[1].3[joint]) - f32::from(pair[0].3[joint]);
                assert!(
                    step * way >= 0.0,
                    "{arbitration} {joint}: back {step} against {way}"
                );
            }
            let change: Vec<f32> = seen[at - 2..on + 60]
                .windows(2)
                .map(|pair| f32::from(pair[1].3[joint]) - f32::from(pair[0].3[joint]))
                .collect();
            for pair in change.windows(2) {
                assert!(
                    (pair[1] - pair[0]).abs() <= 4.0,
                    "{arbitration} {joint}: {change:?}"
                );
            }
        }
    }
}

/// #2602: listening that goes on 3 s or more ends the paused stretch, eased
/// out, so it never picks a move back up long after the moment.
#[test]
fn a_skill_gesture_paused_three_seconds_or_more_ends() {
    let ms = Duration::from_millis;
    let stretch = &crate::clips::STRETCHING;
    let (_, fates, seen) = paused_run(false, stretch, Priority::Skill, Some(ms(8_000)));
    assert_eq!(
        fates,
        [
            ("stretching", "taken: it was folded and stood up for it"),
            ("stretching", "ended: by conversation"),
        ]
    );
    let ended = seen
        .iter()
        .position(|step| step.1.is_none() && step.0 > ms(8_000))
        .unwrap();
    assert!(
        seen[ended].0 >= ms(11_000) && seen[ended].0 < ms(11_100),
        "{:?}",
        seen[ended].0
    );
    // Back to resting after that, nothing resumes.
    let moods = vec![(ms(8_000), Mood::Listening), (ms(11_500), Mood::Resting)];
    let (_, fates, seen) = moods_run(false, true, stretch, Priority::Skill, moods);
    assert_eq!(
        fates.last(),
        Some(&("stretching", "ended: by conversation"))
    );
    assert!(
        seen.iter()
            .all(|step| step.0 < ms(11_100) || step.1.is_none())
    );
}

/// A conversation's nod playing when the person speaks is not ended.
#[test]
fn a_conversation_nod_playing_when_a_conversation_begins_plays_on() {
    let nod = &crate::clips::NOD;
    let (_, played, _) = spoken_to_run(true, nod, Priority::Conversation, None);
    let (_, fates, seen) = spoken_to_run(
        true,
        nod,
        Priority::Conversation,
        Some(Duration::from_millis(5_500)),
    );
    assert_eq!(fates, played);
    assert!(!fates.iter().any(|(_, why)| why.starts_with("ended")));
    assert!(
        seen.iter()
            .any(|step| step.0 > Duration::from_millis(5_500) && step.1 == Some("nod"))
    );
}

/// A skill's stretch playing when a hand comes on the head ends, named for
/// the hand, with the switch on or off; one of the conversation's plays on.
#[test]
fn a_skill_gesture_playing_ends_when_a_hand_comes_on_the_lamp() {
    let ms = Duration::from_millis;
    let cases = [true, false].into_iter().flat_map(|arbitration| {
        [(Priority::Skill, true), (Priority::Conversation, false)]
            .map(|(by, ended)| (arbitration, by, ended))
    });
    for (arbitration, by, ended) in cases {
        let (mut lamp, mut clock) = folded(Duration::from_secs(14));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        // A hand takes the head a second into the stretch and holds it.
        lamp.hand = Some((ms(6_000), ms(10_000), PITCH, 1_950.0));
        lamp.strains = vec![(ms(6_000), ms(10_000), PITCH, 400)];
        let time = clock.time.clone();
        let mut asked = false;
        let mut fates = Vec::new();
        let report = run_arbitrated(
            &mut lamp,
            &mut clock,
            Limits {
                give_way: Some(GiveWay::Return),
                arbitration,
                ..gesture_limits()
            },
            Some(HOME),
            Some(standing(100, 60)),
            None,
            || Some((Mood::Resting, 0)),
            move || {
                let due = !asked && time.get() >= ms(1_000);
                asked |= due;
                let asked = Asked {
                    gesture: due.then_some(&crate::clips::STRETCHING),
                    ..Asked::default()
                };
                (asked, by)
            },
            |glimpse| fates.extend(glimpse.gesture_fates),
        );
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        assert_eq!(report.gestures, 1, "{fates:?}");
        assert_eq!(
            fates.contains(&("stretching", crate::moves::ENDED_BY_HAND)),
            ended,
            "{arbitration} {by:?} {fates:?}"
        );
    }
}

/// A skill's stretch still waiting for the lamp to stand when a hand comes
/// on the head is dropped for the hand, with the switch on or off.
#[test]
fn a_skill_gesture_still_waiting_is_dropped_when_a_hand_comes_on_the_lamp() {
    let ms = Duration::from_millis;
    for arbitration in [true, false] {
        let (mut lamp, mut clock) = folded(Duration::from_secs(14));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        // Asked of the folded lamp at a second, the stretch waits the 4 s
        // rise; a hand takes the head halfway up and holds it.
        lamp.hand = Some((ms(3_000), ms(10_000), PITCH, 1_950.0));
        lamp.strains = vec![(ms(3_000), ms(10_000), PITCH, 400)];
        let time = clock.time.clone();
        let mut asked = false;
        let mut fates = Vec::new();
        let report = run_arbitrated(
            &mut lamp,
            &mut clock,
            Limits {
                give_way: Some(GiveWay::Return),
                arbitration,
                ..gesture_limits()
            },
            Some(HOME),
            Some(standing(100, 60)),
            None,
            || Some((Mood::Resting, 0)),
            move || {
                let due = !asked && time.get() >= ms(1_000);
                asked |= due;
                let asked = Asked {
                    gesture: due.then_some(&crate::clips::STRETCHING),
                    ..Asked::default()
                };
                (asked, Priority::Skill)
            },
            |glimpse| fates.extend(glimpse.gesture_fates),
        );
        let dropped = if arbitration {
            "dropped: held by a hand"
        } else {
            "dropped: a hand holds it"
        };
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        // It never begins, even once the hand has gone.
        assert_eq!(report.gestures, 0, "{arbitration} {fates:?}");
        assert_eq!(fates, [("stretching", dropped)], "{arbitration}");
    }
}

/// One step of a run under the test input's hand (`--hold-from`): whether
/// it held the head, the gesture, the three goals and the arm's.
type HeldStep = (bool, Option<&'static str>, [u16; 3], [u16; 2], bool);

/// A folded lamp asked a skill's stretch at a second; the test input
/// (`Asked::held_by_test`) holds the head over `held`, no hand on any servo.
fn held_from_test_input_run(
    held: Option<(Duration, Duration)>,
    ask_at: Duration,
) -> (
    BodyReport,
    Vec<(&'static str, &'static str)>,
    Vec<HeldStep>,
    Lamp,
) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(14));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let time = clock.time.clone();
    let holding = Rc::new(Cell::new(false));
    let told = holding.clone();
    let mut asked = false;
    let (mut fates, mut seen) = (Vec::new(), Vec::new());
    let report = run_arbitrated(
        &mut lamp,
        &mut clock,
        Limits {
            give_way: Some(GiveWay::Return),
            arbitration: true,
            ..gesture_limits()
        },
        Some(HOME),
        Some(standing(100, 60)),
        None,
        || Some((Mood::Resting, 0)),
        move || {
            let now = time.get();
            let due = !asked && now >= ask_at;
            asked |= due;
            let held = held.is_some_and(|(from, until)| now >= from && now < until);
            holding.set(held);
            let asked = Asked {
                gesture: due.then_some(&crate::clips::STRETCHING),
                held_by_test: held,
                held: false,
                ..Asked::default()
            };
            (asked, Priority::Skill)
        },
        |glimpse| {
            fates.extend(glimpse.gesture_fates);
            seen.push((
                told.get(),
                glimpse.gesture,
                glimpse.goals,
                glimpse.arm_goals,
                glimpse.hand,
            ));
        },
    );
    (report, fates, seen, lamp)
}

/// `--hold-from` (#2904): each step the test input holds says a hand
/// (`body/hand`), as a push does, so a search a push ends can be run from
/// it; before it and once it lets go, never.
#[test]
fn a_hand_from_the_test_input_is_said_as_a_push() {
    let ms = Duration::from_millis;
    let (_, _, seen, _) = held_from_test_input_run(Some((ms(6_000), ms(10_000))), ms(1_000));
    assert!(seen.iter().any(|step| step.0));
    assert!(seen.iter().all(|step| step.4 == step.0), "{seen:?}");
    let (_, _, free, _) = held_from_test_input_run(None, ms(1_000));
    assert!(free.iter().all(|step| !step.4));
}

/// `--hold-from` (#2668): the test input's hand coming mid-stretch ends it
/// as a hand on the head does, `ended: by a hand`, eased out, the head's
/// pitch kept where it was and moved on from there once it lets go, and no
/// torque written or servo given way; without the input the stretch plays
/// out the same as ever.
#[test]
fn a_skill_gesture_playing_ends_by_a_hand_from_the_test_input_eased_out() {
    let ms = Duration::from_millis;
    let (free, free_fates, played, free_lamp) = held_from_test_input_run(None, ms(1_000));
    assert_eq!(
        free_fates,
        [("stretching", "taken: it was folded and stood up for it")]
    );
    let (report, fates, seen, lamp) =
        held_from_test_input_run(Some((ms(6_000), ms(10_000))), ms(1_000));
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(
        fates,
        [
            ("stretching", "taken: it was folded and stood up for it"),
            ("stretching", crate::moves::ENDED_BY_HAND),
        ]
    );
    let first = seen.iter().position(|step| step.0).unwrap();
    let last = seen.iter().rposition(|step| step.0).unwrap();
    // The same as without it until the hand, and played until then.
    assert_eq!(seen[..first], played[..first]);
    assert_eq!(seen[first - 1].1, Some("stretching"));
    assert!(seen[first..].iter().all(|step| step.1.is_none()));
    // The head's pitch held where it was, all the hold.
    let pitch = seen[first - 1].2[0];
    assert!(seen[first..=last].iter().all(|step| step.2[0] == pitch));
    // Eased out, not snapped back: the arm's change a step grows and
    // shrinks by a few counts; the head moves on from where it was held.
    for joint in 0..2 {
        let change: Vec<f32> = seen[first - 2..first + 20]
            .windows(2)
            .map(|pair| f32::from(pair[1].3[joint]) - f32::from(pair[0].3[joint]))
            .collect();
        for pair in change.windows(2) {
            assert!((pair[1] - pair[0]).abs() <= 4.0, "{joint}: {change:?}");
        }
    }
    for pair in seen[last - 1..last + 20].windows(2) {
        assert!(pair[1].2[0].abs_diff(pair[0].2[0]) <= 4, "{seen:?}");
    }
    // No servo gave way and no torque was written for it.
    assert_eq!(report.released_by_hand, [0; 3]);
    assert_eq!(report.torque_writes, free.torque_writes);
    let torque = |lamp: &Lamp| {
        lamp.registers
            .iter()
            .filter(|(_, address, _)| *address == 40)
            .count()
    };
    assert_eq!(torque(&lamp), torque(&free_lamp));
    assert!(lamp.servos.iter().all(|servo| servo.torque == 1));
}

/// The test input's hand on the head while the stretch still waits for the
/// lamp to stand drops it as held, and it never begins.
#[test]
fn a_skill_gesture_still_waiting_is_dropped_by_a_hand_from_the_test_input() {
    let ms = Duration::from_millis;
    let (report, fates, _, _) = held_from_test_input_run(Some((ms(3_000), ms(10_000))), ms(1_000));
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.gestures, 0, "{fates:?}");
    assert_eq!(fates, [("stretching", "dropped: held by a hand")]);
}

/// A skill's move asked while a hand holds the lamp is dropped, naming
/// the hand.
#[test]
fn a_skill_move_asked_while_a_hand_holds_the_lamp_is_dropped_by_the_hand() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.hand = Some((ms(1_500), ms(4_500), YAW, f32::from(HOME[2]) + 40.0));
    lamp.strains = vec![(ms(1_500), ms(4_500), YAW, 120)];
    let held = Rc::new(Cell::new(false));
    let told = held.clone();
    let mut asked = false;
    let mut fates = Vec::new();
    let report = run_arbitrated(
        &mut lamp,
        &mut clock,
        Limits {
            arbitration: true,
            ..giving_way(GiveWay::Return)
        },
        Some(HOME),
        Some(standing(100, 5)),
        None,
        || Some((Mood::Resting, 0)),
        move || {
            let due = !asked && held.get();
            asked |= due;
            let asked = Asked {
                gesture: due.then_some(&crate::clips::STRETCHING),
                ..Asked::default()
            };
            (asked, Priority::Skill)
        },
        |glimpse| {
            told.set(glimpse.hand);
            fates.extend(glimpse.gesture_fates);
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.gestures, 0);
    assert_eq!(fates, [("stretching", "dropped: held by a hand")]);
}

/// A slump in the middle of an ease back (`Limits::arbitration`) ends it
/// with everything else: taken up two minutes later, the lamp rises with
/// no gesture's offsets left in its goals (reviewer's 09:46 on #1239: the
/// head jumped 24 and 22 counts, the shoulder 24, 21 and 20).
#[test]
fn a_slump_during_an_ease_back_leaves_no_offset_after_the_take_up() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(136));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    lamp.may_take_hold = true;
    // The elbow at 820 from 8.7 s: it lets go 1.5 s on, inside the ease.
    lamp.strains = vec![(ms(8_700), ms(10_400), 2, 820)];
    let limits = Limits {
        arbitration: true,
        overload_release: true,
        ..gesture_limits()
    };
    let time = clock.time.clone();
    let seen_at = clock.time.clone();
    let mut asks = VecDeque::from([
        (ms(8_000), Priority::Skill),
        (ms(9_950), Priority::Conversation),
    ]);
    let (mut fates, mut seen, mut limp) = (Vec::new(), Vec::<Step>::new(), Vec::new());
    let report = run_arbitrated(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, 60)),
        None,
        || Some((Mood::Speaking, 60)),
        move || match asks.front() {
            Some((at, by)) if time.get() >= *at => {
                let by = *by;
                asks.pop_front();
                let asked = Asked {
                    gesture: Some(&crate::clips::NOD),
                    ..Asked::default()
                };
                (asked, by)
            }
            _ => (Asked::default(), Priority::Conversation),
        },
        |glimpse| {
            fates.extend(glimpse.gesture_fates);
            limp.push(glimpse.limp);
            seen.push((
                seen_at.get(),
                glimpse.gesture,
                glimpse.goals,
                glimpse.arm_goals,
            ));
        },
    );
    assert_eq!(report.released.len(), 1, "{report:?}");
    assert!(
        fates.contains(&("nod", "ended: by conversation")),
        "{fates:?}"
    );
    // It let go inside the 0.7 s ease, and was taken up two minutes on.
    let let_go = limp.iter().position(|limp| *limp).unwrap();
    let taken_up = let_go + limp[let_go..].iter().position(|limp| !*limp).unwrap();
    assert!(
        (ms(9_950)..ms(10_650)).contains(&seen[let_go].0),
        "{:?}",
        seen[let_go].0
    );
    let after: Vec<_> = seen[taken_up..].iter().collect();
    assert!(after.len() > 100);
    let mut most = [0u16; 5];
    for pair in after.windows(2) {
        let (before, now) = (pair[0], pair[1]);
        let changes = (0..3)
            .map(|joint| now.2[joint].abs_diff(before.2[joint]))
            .chain((0..2).map(|joint| now.3[joint].abs_diff(before.3[joint])));
        for (most, change) in most.iter_mut().zip(changes) {
            *most = (*most).max(change);
        }
    }
    // The head and the shoulder rise smoothly; the elbow takes one step of
    // some 30 counts as it is taken hold of, as it does with no gesture
    // before the slump at all.
    assert!(most[..4].iter().all(|most| *most <= 12), "{most:?}");
}

/// One step of a reading session: when, the gesture playing, the head's
/// goals and the arm's.
type ReadingStep = (Duration, Option<&'static str>, [u16; 3], [u16; 2]);

/// A lamp with these flags, found folded, talked to from 1 to 3 s and
/// in `mood` from then on (a skill saying it again and again), as its unit
/// runs it, asked `clip` at `ask_at`. The run's report, each step, and
/// every arm goal write, shoulder and elbow.
fn reading_session(
    flags: &Flags,
    seconds: u64,
    mood: Mood,
    clip: &'static Clip,
    ask_at: Duration,
) -> (BodyReport, Vec<ReadingStep>, Vec<[u16; 2]>) {
    let (mut lamp, mut clock) = folded(Duration::from_secs(seconds));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let found = [
        flags.home[2],
        flags.folded[0],
        flags.folded[1],
        flags.home[1],
        flags.home[0],
    ];
    for (servo, at) in lamp.servos.iter_mut().zip(found) {
        servo.position = f32::from(at);
        servo.goal = at;
        servo.status = 0;
    }
    let up = Standing::new(flags.folded, flags.stand, 100, 6, 60, flags.load).unwrap();
    let limits = Limits {
        give_way: Some(GiveWay::Return),
        limp: true,
        wake: true,
        ..gesture_limits()
    };
    let time = clock.time.clone();
    let news = move || {
        Some(if time.get() < Duration::from_secs(3) {
            (Mood::Listening, 100)
        } else {
            (mood, 100)
        })
    };
    let asked_at = clock.time.clone();
    let seen_at = clock.time.clone();
    let mut asked = false;
    let mut seen = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        limits,
        Some(flags.home),
        Some(up),
        None,
        news,
        move || {
            let due = !asked && asked_at.get() >= ask_at;
            asked |= due;
            Asked {
                gesture: due.then_some(clip),
                ..Asked::default()
            }
        },
        |glimpse| {
            seen.push((
                seen_at.get(),
                glimpse.gesture,
                glimpse.goals,
                glimpse.arm_goals,
            ))
        },
    );
    let arm = lamp
        .goals
        .iter()
        .filter_map(|write| {
            let of = |id: u8| {
                write
                    .iter()
                    .find(|(at, _)| *at == id)
                    .map(|(_, goal)| *goal)
            };
            Some([of(2)?, of(3)?])
        })
        .collect();
    (report, seen, arm)
}

/// What every reading session with a gesture must keep, on each lamp's
/// flags: it plays, the run ends as asked (no `Window` stop), no arm goal
/// moves more than `ARM_GOAL_STEP` a write, the anti-tip rule neither
/// refused nor cut anything, and no pose written is past its 22 mm.
fn assert_reading_gesture_safe(
    name: &str,
    report: &BodyReport,
    steps: &[ReadingStep],
    arm: &[[u16; 2]],
) {
    assert_eq!(report.gestures, 1, "{name}: {report:?}");
    assert_eq!(report.stop, Stop::Cancelled, "{name}: {report:?}");
    assert!(
        largest_arm_step(arm) <= ARM_GOAL_STEP,
        "{name}: {}",
        largest_arm_step(arm)
    );
    assert_eq!(
        (report.tipping.refused, report.tipping.cuts),
        (0, 0),
        "{name}: {:?}",
        report.tipping
    );
    for (at, _, head, arm) in steps {
        let mm = cog::off_axis_mm([head[2], arm[0], arm[1], head[1], head[0]].map(f32::from));
        assert!(mm <= cog::CLIP_MM, "{name}: {mm} mm at {at:?}");
    }
}

/// #1260 review, 09:19: reading, `acknowledge` (a skill's "right" at the
/// page) is laid over the lean and leaves it where it was. The arm never
/// rises above the reading lean, its dip is the gesture's on top of the
/// lean, and once it is over the arm is at the lean on every step, with no
/// easing out and back in. With each lamp's flags.
#[test]
fn a_reading_lamp_keeps_its_lean_through_a_gesture() {
    let ms = Duration::from_millis;
    for (name, flags) in [("lamp-4ace", &LAMP_4ACE), ("lamp-52e6", &LAMP_52E6)] {
        let (report, steps, arm) = reading_session(
            flags,
            20,
            Mood::Reading,
            &crate::clips::ACKNOWLEDGE,
            ms(12_000),
        );
        assert_reading_gesture_safe(name, &report, &steps, &arm);
        let lean: [u16; 2] = std::array::from_fn(|index| {
            (f32::from(flags.stand[3 + index]) + LEAN[index]).round() as u16
        });
        let at = |when: Duration| steps.iter().rfind(|step| step.0 < when).unwrap();
        assert_eq!(at(ms(11_900)).3, lean, "{name}");
        let after: Vec<_> = steps
            .iter()
            .filter(|step| (ms(12_000)..ms(19_900)).contains(&step.0))
            .collect();
        for index in 0..2 {
            let top = after.iter().map(|step| step.3[index]).max().unwrap();
            assert!(top <= lean[index] + 3, "{name}: {top} over {lean:?}");
        }
        // The elbow dips the gesture's 325 counts from the lean, not from
        // the pose without it.
        let low = after.iter().map(|step| step.3[1]).min().unwrap();
        assert!(low <= lean[1] - 300, "{name}: {low}");
        let over = after.iter().rfind(|step| step.1.is_some()).unwrap().0;
        for step in after.iter().filter(|step| step.0 > over + ms(400)) {
            for index in 0..2 {
                assert!(
                    step.3[index].abs_diff(lean[index]) <= 3,
                    "{name}: {step:?} against {lean:?}"
                );
            }
        }
    }
}

/// #1265 review, 10:13: speaking is unchanged. The same `acknowledge`
/// while it speaks lets the lean go: once the gesture is over the elbow is
/// back at its stand pose (the lean's 60 counts above where reading keeps
/// it) before it leans in again. With each lamp's flags.
#[test]
fn a_speaking_lamp_still_lets_its_lean_go_for_a_gesture() {
    let ms = Duration::from_millis;
    for (name, flags) in [("lamp-4ace", &LAMP_4ACE), ("lamp-52e6", &LAMP_52E6)] {
        let (report, steps, arm) = reading_session(
            flags,
            20,
            Mood::Speaking,
            &crate::clips::ACKNOWLEDGE,
            ms(12_000),
        );
        assert_reading_gesture_safe(name, &report, &steps, &arm);
        let over = steps.iter().rfind(|step| step.1.is_some()).unwrap().0;
        let top = steps
            .iter()
            .filter(|step| (over..over + ms(500)).contains(&step.0))
            .map(|step| step.3[1])
            .max()
            .unwrap();
        assert!(
            top.abs_diff(flags.stand[4]) <= 3,
            "{name}: elbow at {top} after the gesture, stand pose {}",
            flags.stand[4]
        );
    }
}

/// Asked while the reading lean is still easing in, the gesture is laid
/// over the lean as it was then, which is held while it plays: the pose
/// the anti-tip rule checked it over is the pose it plays over, and it
/// ends with the arm where it began. The lean goes on easing in after.
#[test]
fn a_gesture_asked_as_the_reading_lean_eases_in_holds_it_where_it_was() {
    let ms = Duration::from_millis;
    for (name, flags) in [("lamp-4ace", &LAMP_4ACE), ("lamp-52e6", &LAMP_52E6)] {
        let (report, steps, arm) = reading_session(
            flags,
            16,
            Mood::Reading,
            &crate::clips::ACKNOWLEDGE,
            ms(7_300),
        );
        assert_reading_gesture_safe(name, &report, &steps, &arm);
        let first = steps.iter().position(|step| step.1.is_some()).unwrap();
        let began = steps[first - 1].3;
        let last = steps.iter().rposition(|step| step.1.is_some()).unwrap();
        // The gesture's last offsets are a few counts, eased off in a step.
        let ended = steps[last + 1].3;
        for index in 0..2 {
            assert!(
                ended[index].abs_diff(began[index]) <= 3,
                "{name}: began {began:?}, ended {ended:?}"
            );
        }
        let lean = (f32::from(flags.stand[4]) + LEAN[1]).round() as u16;
        assert!(
            began[1] > lean + 10,
            "{name}: {began:?} was not still easing in"
        );
        let before_end = steps.iter().rfind(|step| step.0 < ms(15_900)).unwrap();
        assert_eq!(before_end.3[1], lean, "{name}");
    }
}

/// #1260 review, 09:19, its second point: reading holds the head's pitch
/// at its reach, and a gesture's pitch is laid over that after the reach is
/// applied, kept only from the floor and the seam. So the whole of it is
/// made: `curious` looks up and then down past the reading pitch by what it
/// has of each, and `acknowledge` (whose pitch only lifts) lifts its 28.
#[test]
fn a_reading_lamp_makes_the_whole_of_a_gestures_pitch() {
    let ms = Duration::from_millis;
    for (name, flags) in [("lamp-4ace", &LAMP_4ACE), ("lamp-52e6", &LAMP_52E6)] {
        for (clip, up, down) in [
            (&crate::clips::CURIOUS, 50, 180),
            (&crate::clips::ACKNOWLEDGE, 24, 0),
        ] {
            let (report, steps, arm) = reading_session(flags, 20, Mood::Reading, clip, ms(12_000));
            assert_reading_gesture_safe(name, &report, &steps, &arm);
            let reading = steps.iter().rfind(|step| step.0 < ms(12_000)).unwrap().2[0];
            let during = steps.iter().filter(|step| step.1.is_some());
            let (low, high) = during.fold((u16::MAX, 0), |(low, high), step| {
                (low.min(step.2[0]), high.max(step.2[0]))
            });
            assert!(
                reading - low >= up,
                "{name} {}: {low} from {reading}",
                clip.name
            );
            assert!(
                high - reading >= down,
                "{name} {}: {high} from {reading}",
                clip.name
            );
        }
    }
}

/// What `body/gesture` says of a run of [`skill_run`]'s kind: each
/// gesture's name and what became of it, step by step, through the same
/// [`crate::bus_out::GestureSaid`] that lamp-body's journal reads.
fn gestures_said(
    seconds: u64,
    mut ask: impl FnMut(Duration) -> Asked,
) -> Vec<(&'static str, lamp_msgs::GestureState)> {
    let (mut lamp, mut clock) = folded(Duration::from_secs(seconds));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let time = clock.time.clone();
    let mut said = crate::bus_out::GestureSaid::default();
    let mut gestures = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        gesture_limits(),
        Some(HOME),
        Some(standing(100, 5)),
        None,
        || Some((Mood::Resting, 0)),
        move || ask(time.get()),
        |glimpse| {
            gestures.extend(
                said.step(glimpse)
                    .into_iter()
                    .map(|(name, state, _)| (name, state)),
            );
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    gestures
}

/// A nod asked of a folded lamp: `body/gesture` says it began once the
/// lamp stood up, then that it was over, in that order, and nothing else.
#[test]
fn a_nod_says_began_and_then_over_on_body_gesture() {
    use lamp_msgs::GestureState::{Began, Over};
    let said = gestures_said(14, once(Duration::from_secs(1), false));
    assert_eq!(said, [("nod", Began), ("nod", Over)]);
}

/// Paused while it rises for a nod: the nod is dropped, and `body/gesture`
/// says so; it never began, so no `over` follows.
#[test]
fn a_pause_during_the_rise_says_dropped_on_body_gesture() {
    use lamp_msgs::GestureState::Dropped;
    let mut asked = false;
    let said = gestures_said(16, move |now| {
        if now >= Duration::from_millis(1_500) {
            return Asked::PAUSED;
        }
        let due = !asked && now >= Duration::from_secs(1);
        asked |= due;
        Asked {
            gesture: due.then_some(&crate::clips::NOD),
            ..Asked::default()
        }
    });
    assert_eq!(said, [("nod", Dropped)]);
}

/// A gesture past V1's tip-over line, asked from outside, is refused whole
/// before it moves anything: `body/gesture` says it `dropped` (Fifth on
/// #1230), and nothing began.
#[test]
fn a_gesture_the_anti_tip_rule_refuses_says_dropped_on_body_gesture() {
    use lamp_msgs::GestureState::Dropped;
    let mut asked = false;
    let said = gestures_said(10, move |now| {
        let due = !asked && now >= Duration::from_secs(1);
        asked |= due;
        Asked {
            gesture: due.then_some(&PAST_THE_LINE),
            ..Asked::default()
        }
    });
    assert_eq!(said, [("past_the_line", Dropped)]);
}

/// A nod waiting for the lamp to stand, and a shy asked in the step it
/// stands, which begins then: the nod's `dropped` and the shy's `taken`
/// are both kept, in that one step's glimpse (Fifth and the reviewer on
/// #1230).
#[test]
fn two_fates_in_one_step_are_both_kept() {
    // When the nod alone begins: the step the lamp stands.
    let (_, _, steps) = skill_run(
        14,
        standing(100, 5),
        gesture_limits(),
        |_| {},
        || Some((Mood::Resting, 0)),
        once(Duration::from_secs(1), false),
    );
    let stands = steps.iter().find(|s| s.2 == Some("nod")).unwrap().0;
    let mut both = false;
    for back_ms in [0u64, 20, 40, 60] {
        let at = stands - Duration::from_millis(back_ms);
        let (mut lamp, mut clock) = folded(Duration::from_secs(14));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        let time = clock.time.clone();
        let mut asked = [false; 2];
        let mut in_one = Vec::new();
        run_alive(
            &mut lamp,
            &mut clock,
            gesture_limits(),
            Some(HOME),
            Some(standing(100, 5)),
            None,
            || Some((Mood::Resting, 0)),
            move || {
                let now = time.get();
                let when = [Duration::from_secs(1), at];
                let due = (0..2).find(|i| !asked[*i] && now >= when[*i]);
                if let Some(i) = due {
                    asked[i] = true;
                }
                Asked {
                    gesture: due.map(|i| [&crate::clips::NOD, &crate::clips::SHY][i]),
                    ..Asked::default()
                }
            },
            |glimpse| {
                let fates: Vec<_> = glimpse.gesture_fates.iter().collect();
                if fates.len() > 1 {
                    in_one.push(fates);
                }
            },
        );
        both |= in_one.iter().any(|fates| {
            fates
                == &[
                    ("nod", "dropped: another was asked after it"),
                    ("shy", "taken: it was folded and stood up for it"),
                ]
        });
    }
    assert!(
        both,
        "no step's glimpse had the nod dropped and the shy taken"
    );
}

/// [`arbitrated_run`]'s lamp under `Limits::arbitration`, with what
/// `body/gesture` says of each gesture through [`crate::bus_out::GestureSaid`].
fn arbitrated_said(
    asks: Vec<(Duration, &'static Clip, Priority)>,
    seconds: u64,
) -> Vec<(&'static str, lamp_msgs::GestureState)> {
    let (mut lamp, mut clock) = folded(Duration::from_secs(seconds));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let limits = Limits {
        arbitration: true,
        ..gesture_limits()
    };
    let time = clock.time.clone();
    let mut asks = VecDeque::from(asks);
    let mut said = crate::bus_out::GestureSaid::default();
    let mut gestures = Vec::new();
    let report = run_arbitrated(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        Some(standing(100, 60)),
        None,
        || Some((Mood::Speaking, 60)),
        move || match asks.front() {
            Some((at, clip, by)) if time.get() >= *at => {
                let asked = Asked {
                    gesture: Some(*clip),
                    ..Asked::default()
                };
                let by = *by;
                asks.pop_front();
                (asked, by)
            }
            _ => (Asked::default(), Priority::Conversation),
        },
        |glimpse| {
            gestures.extend(
                said.step(glimpse)
                    .into_iter()
                    .map(|(name, state, _)| (name, state)),
            );
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    gestures
}

/// A skill's stretch ended for the conversation's nod (#1239, `ended: by
/// conversation`) did not play out: `body/gesture` says it `dropped`, with
/// no `over`, and the nod began after it (the fifth on #1230).
#[test]
fn a_gesture_ended_for_a_higher_ask_says_dropped_not_over() {
    use lamp_msgs::GestureState::{Began, Dropped, Over};
    let said = arbitrated_said(
        vec![
            (
                Duration::from_secs(8),
                &crate::clips::STRETCHING,
                Priority::Skill,
            ),
            (
                Duration::from_millis(11_200),
                &crate::clips::NOD,
                Priority::Conversation,
            ),
        ],
        16,
    );
    let stretching: Vec<_> = said
        .iter()
        .filter(|(name, _)| *name == "stretching")
        .collect();
    assert_eq!(
        stretching,
        [&("stretching", Began), &("stretching", Dropped)],
        "{said:?}"
    );
    let nod = said
        .iter()
        .position(|news| *news == ("nod", Began))
        .unwrap();
    let dropped = said
        .iter()
        .position(|news| *news == ("stretching", Dropped))
        .unwrap();
    assert!(dropped < nod, "{said:?}");
    assert!(!said.contains(&("stretching", Over)), "{said:?}");
}

/// A skill's stretch asked while the conversation's nod plays (#1239,
/// `dropped: busy, conversation`): `body/gesture` says it `dropped`.
#[test]
fn an_ask_dropped_as_busy_says_dropped() {
    use lamp_msgs::GestureState::Dropped;
    let ms = Duration::from_millis;
    let said = arbitrated_said(
        vec![
            (ms(500), &crate::clips::NOD, Priority::Conversation),
            (ms(1_500), &crate::clips::STRETCHING, Priority::Skill),
        ],
        10,
    );
    let stretching: Vec<_> = said
        .iter()
        .filter(|(name, _)| *name == "stretching")
        .collect();
    assert_eq!(stretching, [&("stretching", Dropped)], "{said:?}");
}

/// A skill's stretch asked while a hand holds the lamp (#1239, `dropped:
/// held by a hand`): `body/gesture` says it `dropped`.
#[test]
fn an_ask_dropped_as_held_by_a_hand_says_dropped() {
    use lamp_msgs::GestureState::Dropped;
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.hand = Some((ms(1_500), ms(4_500), YAW, f32::from(HOME[2]) + 40.0));
    lamp.strains = vec![(ms(1_500), ms(4_500), YAW, 120)];
    let held = Rc::new(Cell::new(false));
    let told = held.clone();
    let mut asked = false;
    let mut said = crate::bus_out::GestureSaid::default();
    let mut gestures = Vec::new();
    let report = run_arbitrated(
        &mut lamp,
        &mut clock,
        Limits {
            arbitration: true,
            ..giving_way(GiveWay::Return)
        },
        Some(HOME),
        Some(standing(100, 5)),
        None,
        || Some((Mood::Resting, 0)),
        move || {
            let due = !asked && held.get();
            asked |= due;
            let asked = Asked {
                gesture: due.then_some(&crate::clips::STRETCHING),
                ..Asked::default()
            };
            (asked, Priority::Skill)
        },
        |glimpse| {
            told.set(glimpse.hand);
            gestures.extend(said.step(glimpse));
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(
        gestures,
        [("stretching", Dropped, Some("dropped: held by a hand"))]
    );
}

/// A hand that turns the base and keeps holding it, gently (lamp-52e6 on
/// 2026-10-19 read 172 to 196 on the base, under its bound of 220 and its
/// contact floor), is given way to once. It is not fought: the base is not
/// sent back home while the hand is still on it.
#[test]
fn a_base_held_gently_by_a_hand_is_not_sent_back_into_it() {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(12));
    lamp.hand = Some((ms(1_500), ms(9_500), YAW, f32::from(HOME[2]) + 40.0));
    // Noisy: a hand holding still reads under the stall bound now and then
    // (a servo whose goal is where it is pushes at nothing).
    lamp.strains = (0..40u64)
        .flat_map(|k| {
            let at = 1_500 + k * 200;
            [
                (ms(at), ms(at + 160), YAW, 180),
                (ms(at + 160), ms(at + 200), YAW, 60),
            ]
        })
        .collect();
    let limits = Limits {
        limp: true,
        ..giving_way(GiveWay::Return)
    };
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        limits,
        Some(HOME),
        || Some((Mood::Listening, 100)),
        |glimpse| seen.push(*glimpse),
    );
    // Each step on which, the hand still there, the base's goal moved back
    // toward home: the lamp pushing into the hand.
    let pushed_back = seen
        .windows(2)
        .filter(|pair| (ms(2_500)..ms(9_500)).contains(&clock_of(&pair[1])))
        .filter(|pair| pair[1].goals[2] < pair[0].goals[2])
        .count();
    eprintln!(
        "eased {:?} released_by_hand {:?} pushed_back_steps {pushed_back}",
        report.eased, report.released_by_hand
    );
    assert_eq!(pushed_back, 0, "{report:?}");
}

/// An unplug request cancels the 90-second awake window and, while it is in
/// force (the adapter's latch, read every step), stays authoritative over
/// later speaking, directed reading holds, name, face, expression and skill
/// input.
#[test]
fn unplug_folds_now_and_latches_over_later_inputs() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(25));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    lamp.misreads = vec![(Duration::from_secs(21), Joint::BasePitch.index(), 4_000, 1)];
    let time = clock.time.clone();
    let watched = clock.time.clone();
    let mood_at = clock.time.clone();
    let mut frames = Vec::new();
    let report = run_alive(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 90)),
        Some(Looking::new([727, 553], [480, 120]).unwrap()),
        move || {
            Some((
                if mood_at.get() >= Duration::from_secs(16) {
                    Mood::Reading
                } else {
                    Mood::Speaking
                },
                80,
            ))
        },
        move || {
            let unplug = time.get() >= Duration::from_secs(8);
            Asked {
                unplug,
                called: true,
                toward: Some(-0.5),
                expression: Some([20.0, 0.0, 0.0]),
                gesture: (time.get() > Duration::from_secs(16)).then_some(&crate::clips::NOD),
                ..Asked::default()
            }
        },
        |glimpse| frames.push((watched.get(), *glimpse)),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(
        frames
            .iter()
            .any(|(at, frame)| *at < Duration::from_secs(8) && frame.stand > 0.9)
    );
    let later: Vec<_> = frames
        .iter()
        .filter(|(at, _)| *at >= Duration::from_secs(20) && *at < Duration::from_secs(25))
        .collect();
    assert!(!later.is_empty());
    assert!(later.iter().all(|(_, frame)| frame.stand == 0.0
        && frame.gesture.is_none()
        && frame.mood == Mood::Resting));
    let rejected = frames
        .windows(2)
        .find(|pair| pair[1].1.bad_reads != pair[0].1.bad_reads)
        .expect("injected read must be rejected");
    assert!(
        !rejected[1].1.unplug_safe,
        "substituting an old good position cannot prove physical readiness"
    );
    assert!(
        later.iter().any(|(_, frame)| frame.unplug_safe),
        "no fresh physically folded evidence: {:?}",
        later.last()
    );
}

/// Commanded stand=0 cannot attest that a blocked physical arm is folded.
#[test]
fn unplug_never_attests_an_arm_stuck_away_from_its_fold() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(20));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    lamp.servos[Joint::BasePitch.index()].position += 40.0;
    lamp.servos[Joint::BasePitch.index()].goal += 40;
    lamp.short = Some((Joint::BasePitch.index(), 40.0));
    let mut safe = false;
    let _ = run_alive(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 90)),
        None,
        || Some((Mood::Resting, 0)),
        || Asked {
            unplug: true,
            held_by_test: false,
            held: false,
            ..Asked::default()
        },
        |glimpse| safe |= glimpse.unplug_safe,
    );
    assert!(!safe);
}

/// lamp-52e6's elbow rests 13 counts past its folded goal every time it
/// folds; the fold the body itself reports is a fold it can attest.
#[test]
fn unplug_attests_a_fold_whose_elbow_rests_a_few_counts_off() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(20));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let elbow = Joint::ElbowPitch.index();
    lamp.servos[elbow].position += 13.0;
    lamp.short = Some((elbow, 13.0));
    let mut safe = false;
    let _ = run_alive(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 90)),
        None,
        || Some((Mood::Resting, 0)),
        || Asked {
            unplug: true,
            held_by_test: false,
            held: false,
            ..Asked::default()
        },
        |glimpse| safe |= glimpse.unplug_safe,
    );
    assert!(safe);
}

/// lamp-52e6 on 2026-10-20: asked to unplug with its base turned 60-odd
/// counts toward the person, it folded and rested there, so its proof never
/// came. The fold for unplugging brings the head and base to the configured
/// home, from wherever they were turned (here by Expression), and attests.
#[test]
fn unplug_brings_a_base_looking_aside_home_before_it_attests() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(30));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let time = lamp.time.clone();
    let asked_at = Duration::from_secs(8);
    let mut safe = false;
    let _ = run_alive(
        &mut lamp,
        &mut clock,
        waking(),
        Some(HOME),
        Some(standing(100, 90)),
        None,
        || Some((Mood::Resting, 0)),
        || {
            if time.get() < asked_at {
                Asked {
                    expression: Some([0.0, 0.0, 70.0]),
                    ..Asked::default()
                }
            } else {
                Asked {
                    unplug: true,
                    held_by_test: false,
                    held: false,
                    ..Asked::default()
                }
            }
        },
        |glimpse| safe |= glimpse.unplug_safe,
    );
    let aside = lamp
        .goals
        .iter()
        .flatten()
        .filter(|(id, _)| *id == 1)
        .map(|(_, goal)| goal.abs_diff(HOME[2]))
        .max()
        .unwrap_or(0);
    assert!(
        aside > 24,
        "fixture must turn the base aside first ({aside})"
    );
    assert!(safe, "base at {}", lamp.servos[YAW].position);
    assert!((lamp.servos[YAW].position - f32::from(HOME[2])).abs() <= 24.0);
}

/// lamp-52e6 on 2026-10-20 (#2069 try 1): folded and Ready, a second voice
/// turn came while the readiness line played (listening, thinking,
/// speaking), and Ready was lost for good. Nothing a turn sends (moods, a
/// face to look at, Expression's offsets, a gesture) moves a latched lamp,
/// and reads missed meanwhile are a gap, never a step judged unsafe.
#[test]
fn unplug_ready_stays_ready_through_a_second_turn() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(30));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let time = lamp.time.clone();
    let turn = Duration::from_secs(15);
    // The serial line busy for a moment as the turn begins: reads missed.
    lamp.silent_from = Some(turn + Duration::from_millis(500));
    lamp.silent_until = Some(turn + Duration::from_millis(560));
    let mood_at = |at: Duration| match at.saturating_sub(turn).as_secs() {
        _ if at < turn => Mood::Resting,
        0..=1 => Mood::Listening,
        2..=3 => Mood::Thinking,
        _ => Mood::Speaking,
    };
    let looking = Looking::new([342, 200], [300, 120]).unwrap();
    let (news_time, asked_time) = (time.clone(), time.clone());
    let (mut ready_at, mut lost_at) = (None, None);
    let report = run_alive(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 90)),
        Some(looking),
        move || Some((mood_at(news_time.get()), 100)),
        move || {
            let late = asked_time.get() >= turn;
            Asked {
                unplug: true,
                held_by_test: false,
                held: false,
                seen: late.then_some(Seen {
                    face: Some(crate::gaze::Sight {
                        x: 0.8,
                        y: -0.3,
                        size: 0.2,
                        turned: 0.0,
                    }),
                }),
                expression: late.then_some([40.0, 15.0, 60.0]),
                gesture: late.then_some(&clips::DIP),
                called: late,
                ..Asked::default()
            }
        },
        |glimpse| {
            let now = time.get();
            if glimpse.unplug_safe {
                ready_at.get_or_insert(now);
            } else if ready_at.is_some() && glimpse.unplug_unsafe.is_some() {
                lost_at.get_or_insert((now, glimpse.unplug_unsafe));
            }
        },
    );
    assert!(report.missed_reads > 0, "fixture must miss reads");
    let ready_at = ready_at.expect("never Ready");
    assert!(ready_at < turn, "Ready only at {ready_at:?}");
    assert_eq!(lost_at, None, "Ready lost");
}

/// lamp-52e6 on 2026-10-20 09:46 (#2069 retry): Ready, then while the
/// readiness line played, "physical readiness was lost (a read was bad or a
/// goal unsent)". A bad read is a gap in the evidence: never a step judged
/// unsafe, and the next good read is folded evidence again.
#[test]
fn unplug_ready_survives_a_bad_read() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(25));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    let time = lamp.time.clone();
    let at = Duration::from_secs(15);
    lamp.misreads = vec![(at, Joint::ElbowPitch.index(), 4_500, 1)];
    let (mut ready_at, mut unsafe_after, mut gaps, mut last_safe) = (None, None, 0, None);
    let report = run_alive(
        &mut lamp,
        &mut clock,
        Limits::FOLDED,
        Some(HOME),
        Some(standing(100, 90)),
        None,
        || Some((Mood::Resting, 0)),
        || Asked {
            unplug: true,
            held_by_test: false,
            held: false,
            ..Asked::default()
        },
        |glimpse| {
            let now = time.get();
            if glimpse.unplug_safe {
                ready_at.get_or_insert(now);
                last_safe = Some(now);
            } else if ready_at.is_some() {
                if glimpse.unplug_unsafe.is_some() {
                    unsafe_after.get_or_insert((now, glimpse.unplug_unsafe));
                }
                gaps += u32::from(glimpse.unplug_gap);
            }
        },
    );
    assert!(
        report.bad_reads.iter().any(|bad| *bad > 0),
        "fixture must read badly"
    );
    assert!(ready_at.expect("never Ready") < at);
    assert_eq!(unsafe_after, None, "a bad read judged unsafe");
    assert!(gaps > 0, "the bad read is a gap");
    assert!(last_safe.expect("never safe") > at + Duration::from_secs(1));
}

#[test]
fn unplug_during_startup_wake_stops_automatic_motion() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(30));
    let at = Duration::from_millis(500);
    clock.inhibit_at = Some(at);
    lamp.servos[YAW].position = 2_315.0;
    lamp.servos[YAW].goal = 2_315;
    let report = run(
        &mut lamp,
        &mut clock,
        waking(),
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(report.woke.as_ref().unwrap().seconds < 0.6);
    assert!(clock.time.get() < at + Duration::from_millis(100));
    assert!(
        lamp.servos[YAW].goal > 2_300,
        "the wake must not finish after unplug"
    );
}

#[test]
fn unplug_during_take_hold_retry_never_turns_torque_back_on() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(10));
    clock.inhibit_at = Some(Duration::from_millis(10));
    lamp.may_take_hold = true;
    lamp.servos[ROLL].torque = 0;
    lamp.servos[ROLL].goal = 2_074;
    lamp.deaf = Some((ROLL, 3));
    let report = run(
        &mut lamp,
        &mut clock,
        Limits {
            torque_on: true,
            ..Limits::FOLDED
        },
        None,
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(lamp.servos[ROLL].torque, 0);
    assert!(
        !lamp
            .registers
            .iter()
            .any(|(_, address, bytes)| *address == 40 && bytes == &[1])
    );
}

#[test]
fn unplug_during_hand_recovery_wake_stops_before_it_finishes() {
    let secs = Duration::from_secs_f32;
    let (mut lamp, mut clock) = folded(secs(25.0));
    let inhibit = secs(8.2);
    clock.inhibit_at = Some(inhibit);
    lamp.may_take_hold = true;
    lamp.hand = Some((secs(1.5), secs(3.0), YAW, 2_045.0));
    lamp.strains = vec![(secs(1.5), secs(3.0), YAW, 600)];
    let report = run(
        &mut lamp,
        &mut clock,
        Limits {
            limp: true,
            ..giving_way(GiveWay::Return)
        },
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.released.len(), 1, "{report:?}");
    assert!(
        report.released[0].woke.is_some(),
        "inhibit must land during recovery wake"
    );
    assert!(
        clock.time.get() < inhibit + Duration::from_millis(100),
        "{:?}",
        clock.time.get()
    );
    assert!(
        lamp.servos[YAW].goal > HOME[2] + 10,
        "the wake must not finish"
    );
}

#[test]
fn unplug_never_attests_a_shifted_runtime_home_as_the_configured_fold() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(10));
    lamp.arm_moves = true;
    lamp.overload_clears = 1;
    lamp.servos[YAW].position = 2_600.0;
    lamp.servos[YAW].goal = 2_600;
    let mut safe = false;
    let report = run_alive(
        &mut lamp,
        &mut clock,
        Limits {
            limp: true,
            ..waking()
        },
        Some(HOME),
        Some(standing(100, 90)),
        None,
        || Some((Mood::Resting, 0)),
        || Asked {
            unplug: true,
            held_by_test: false,
            held: false,
            ..Asked::default()
        },
        |glimpse| safe |= glimpse.unplug_safe,
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_ne!(
        report.home.unwrap()[2],
        HOME[2],
        "fixture must shift runtime home"
    );
    assert!(
        (lamp.servos[YAW].position - f32::from(HOME[2])).abs() <= 24.0,
        "the fold brings the base to the configured home, not the shifted one: {}",
        lamp.servos[YAW].position
    );
    assert!(
        !safe,
        "unplug target must remain the explicit calibrated home"
    );
}

/// A matching "never mind" clears the adapter's latch: the folded lamp stands
/// again through the ordinary rise, as when spoken to; a keeper pause wins.
#[test]
fn unplug_released_stands_through_the_ordinary_rise_unless_paused() {
    for paused in [false, true] {
        let (mut lamp, mut clock) = folded(Duration::from_secs(40));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        let time = clock.time.clone();
        let mood_at = clock.time.clone();
        let watched = clock.time.clone();
        let mut frames = Vec::new();
        let report = run_alive(
            &mut lamp,
            &mut clock,
            Limits::FOLDED,
            Some(HOME),
            Some(standing(100, 90)),
            None,
            // Spoken to only before the request: standing again afterwards
            // is the release's own wake window, not a mood's.
            move || {
                Some(if mood_at.get() < Duration::from_secs(8) {
                    (Mood::Speaking, 80)
                } else {
                    (Mood::Resting, 0)
                })
            },
            move || {
                let at = time.get();
                let latched = at >= Duration::from_secs(8) && at < Duration::from_secs(16);
                Asked {
                    unplug: latched,
                    held_by_test: false,
                    held: false,
                    paused: paused && at >= Duration::from_secs(16),
                    ..Asked::default()
                }
            },
            |glimpse| frames.push((watched.get(), *glimpse)),
        );
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        let between = |from: u64, to: u64| {
            frames
                .iter()
                .filter(move |(at, _)| {
                    *at >= Duration::from_secs(from) && *at < Duration::from_secs(to)
                })
                .map(|(_, frame)| frame.stand)
        };
        assert!(between(0, 8).any(|stand| stand > 0.9), "stood before");
        assert!(
            between(14, 16).all(|stand| stand == 0.0),
            "folded while latched"
        );
        if paused {
            assert!(
                between(16, 40).all(|stand| stand == 0.0),
                "a pause keeps it down"
            );
        } else {
            assert!(between(16, 40).any(|stand| stand > 0.9), "stood again");
        }
    }
}

/// `LAMP_GESTURE_PAUSE` (#2602) is on for the exact value `on` only.
#[test]
fn the_gesture_pause_is_on_for_on_only() {
    assert!(super::gesture_pause_from(Some("on")));
    for value in [
        None,
        Some(""),
        Some("On"),
        Some("1"),
        Some("off"),
        Some("on "),
    ] {
        assert!(!super::gesture_pause_from(value), "{value:?}");
    }
}

/// #2602: waking and `refused_let_go`, as the product's body runs.
fn let_go_when_refused() -> Limits {
    Limits {
        refused_let_go: true,
        ..waking()
    }
}

/// A folded lamp whose head pitch a knock left at 2,500, 500 counts from
/// home, more than waking brings back (as lamp-e82d's was), and a hand
/// that moves it to `to` from `from` to `until` at 100 counts a second.
fn knocked(cancel_at: Duration, hand: Option<(Duration, Duration, f32)>) -> (Lamp, Clock) {
    let (mut lamp, clock) = folded(cancel_at);
    lamp.may_take_hold = true;
    lamp.servos[PITCH].position = 2_500.0;
    lamp.servos[PITCH].goal = 2_500;
    lamp.hand = hand.map(|(from, until, to)| (from, until, PITCH, to));
    (lamp, clock)
}

/// Refused for a pose a hand can set straight, the lamp lets go of the
/// three it commands (torque off, nothing else written, no goal), says
/// what it asks on every step it is let go, and stays so until asked to
/// stop while the pose stays out of range.
#[test]
fn refused_for_its_pose_it_lets_go_says_so_and_stays_let_go_while_out_of_range() {
    let (mut lamp, mut clock) = knocked(Duration::from_secs(5), None);
    let mut said = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        let_go_when_refused(),
        Some(HOME),
        || None,
        |glimpse| said.push(glimpse.waiting),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.refused_let_go, Some(SET_STRAIGHT[0]));
    assert!(lamp.goals.is_empty(), "{:?}", lamp.goals);
    let off: Vec<_> = COMMANDED
        .iter()
        .map(|joint| (joint.id(), 40, vec![0]))
        .collect();
    assert_eq!(lamp.registers, off);
    for joint in COMMANDED {
        assert_eq!(lamp.servos[usize::from(joint.id()) - 1].torque, 0);
    }
    // Read ten times a second for the five seconds, each said.
    assert!((45..=51).contains(&said.len()), "{}", said.len());
    assert!(said.iter().all(|say| *say == Some(SET_STRAIGHT[0])));

    // Off, the same lamp is refused as before, nothing written.
    let (mut lamp, mut clock) = knocked(Duration::from_secs(5), None);
    let report = run(&mut lamp, &mut clock, waking(), Some(HOME), || None, |_| {});
    assert_eq!(
        report.stop,
        Stop::Refused {
            why: "wrist_pitch is 500 counts from its home, more than waking brings back (400)"
                .into()
        }
    );
    assert!(lamp.nothing_was_written() && report.refused_let_go.is_none());
}

/// #2938: a head found tipped up out of range, against its weight, is not
/// let go, or it would drop onto the base or a hand: its pitch keeps its
/// torque, the roll and the base are let go, and it says it holds the head.
#[test]
fn refused_with_its_head_tipped_up_it_keeps_holding_the_head() {
    let (mut lamp, mut clock) = knocked(Duration::from_secs(2), None);
    lamp.servos[PITCH].position = 1_500.0;
    lamp.servos[PITCH].goal = 1_500;
    let mut said = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        let_go_when_refused(),
        Some(HOME),
        || None,
        |glimpse| said.push(glimpse.waiting),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.refused_let_go, Some(PROPPED));
    assert_eq!(lamp.servos[PITCH].torque, 1);
    let pitch = Joint::WristPitch.id();
    assert!(
        !lamp.registers.iter().any(|(id, _, _)| *id == pitch),
        "{:?}",
        lamp.registers
    );
    for joint in [Joint::WristRoll, Joint::BaseYaw] {
        assert_eq!(lamp.servos[usize::from(joint.id()) - 1].torque, 0);
    }
    assert!(!said.is_empty() && said.iter().all(|say| *say == Some(PROPPED)));
}

/// #2940, El's lamp-06c1 case (22 Oct 18:48–19:08): woken with the shoulder
/// left up, it was lowered but stopped short (settled 2,140 for 2,053, past
/// `Standing::FOUND_SLACK`), a `Stop::Tracking` before any pose was judged.
/// Each start exited and systemd gave up after three, so a hand folding the
/// arm started nothing. With `--refused-let-go` it lets go of all five, asks
/// for its arm to be folded, and starts once a hand has folded it.
#[test]
fn a_shoulder_that_stops_short_while_lowered_is_let_go_and_waited_for() {
    let start = |cancel_at: Duration, hand: Option<(Duration, Duration, usize, f32)>| {
        let (mut lamp, mut clock) = folded(cancel_at);
        lamp.may_take_hold = true;
        lamp.arm_moves = true;
        for (index, at) in [
            (YAW, 1_979.0),
            (1, 2_633.0),
            (2, 2_074.0),
            (ROLL, 2_101.0),
            (PITCH, 1_771.0),
        ] {
            let servo = &mut lamp.servos[index];
            (servo.position, servo.goal, servo.torque, servo.status) = (at, 0, 0, 0);
        }
        // The shoulder stops 87 counts short of any goal, so lowered to
        // 2,053 it settles at 2,140, as 06c1's did.
        lamp.short = Some((1, 87.0));
        lamp.hand = hand;
        let limits = Limits {
            force: Some(600),
            torque_on: true,
            wake: true,
            refused_let_go: true,
            ..Limits::folded_with(Tuning {
                reach: [150, 15, 1, 250],
                speed: 120,
                servo_speed: 1_000,
                servo_acceleration: 60,
            })
            .unwrap()
        };
        let up = Standing::new(
            [2_053, 2_069],
            [1_772, 2_035, 2_035, 2_657, 2_602],
            100,
            5,
            90,
            [560, 560],
        )
        .unwrap();
        let mut said = Vec::new();
        let report = run_standing(
            &mut lamp,
            &mut clock,
            limits,
            Some([1_771, 2_035, 2_035]),
            Some(up),
            || None,
            |glimpse: &Glimpse| said.push(glimpse.waiting),
        );
        (report, lamp, said)
    };
    // Nobody folds it: the arm let go, the head still held, asking, until
    // asked to stop.
    let (report, lamp, said) = start(Duration::from_secs(60), None);
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.refused_let_go, Some(ARM_FOLD), "{report:?}");
    for joint in ARM {
        assert_eq!(
            lamp.servos[usize::from(joint.id()) - 1].torque,
            0,
            "{joint:?}"
        );
    }
    for joint in COMMANDED {
        assert_eq!(
            lamp.servos[usize::from(joint.id()) - 1].torque,
            1,
            "{joint:?}"
        );
    }
    assert!(said.contains(&Some(ARM_FOLD)));
    // A hand folds it at 40 s: the lamp takes hold again and starts.
    let (report, lamp, said) = start(
        Duration::from_secs(70),
        Some((Duration::from_secs(40), Duration::from_secs(42), 1, 2_053.0)),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.refused_let_go, Some(ARM_FOLD), "{report:?}");
    assert!(said.contains(&Some(ARM_FOLD)));
    assert!(
        said.last().is_some_and(Option::is_none),
        "started again: {:?}",
        said.last()
    );
    assert!(lamp.servos.iter().all(|servo| servo.torque == 1));
}

/// The Reviewer-lead's P2 on #3168: a start after a wait for a hand has
/// sent READY=1, so systemd's 5 s watchdog is armed. A shoulder lowered then
/// is watched each step, never 22 s unwatched (lamp-52e6's `Watchdog
/// timeout`). Here `run_on` starts as after a wait, the shoulder left up.
#[test]
fn a_shoulder_lowered_in_a_start_after_a_wait_is_watched_each_step() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(40));
    lamp.may_take_hold = true;
    lamp.arm_moves = true;
    for (index, at) in [
        (YAW, 1_979.0),
        (1, 2_633.0),
        (2, 2_074.0),
        (ROLL, 2_035.0),
        (PITCH, 1_771.0),
    ] {
        let servo = &mut lamp.servos[index];
        (servo.position, servo.goal, servo.torque, servo.status) = (at, 0, 0, 0);
    }
    let limits = Limits {
        force: Some(600),
        torque_on: true,
        wake: true,
        refused_let_go: true,
        ..Limits::folded_with(Tuning {
            reach: [150, 15, 1, 250],
            speed: 120,
            servo_speed: 1_000,
            servo_acceleration: 60,
        })
        .unwrap()
    };
    let up = Standing::new(
        [2_053, 2_069],
        [1_772, 2_035, 2_035, 2_657, 2_602],
        100,
        5,
        90,
        [560, 560],
    )
    .unwrap();
    let time = clock.time.clone();
    let mut watched = Vec::new();
    let report = {
        let mut bus = Bus::new(&mut lamp, &mut clock, Some(up));
        run_on(
            &mut bus,
            limits,
            Some([1_771, 2_035, 2_035]),
            Some(up),
            None,
            || None,
            || (Asked::default(), Priority::Conversation, None),
            |_: &Glimpse| watched.push(time.get()),
            10,
        )
    };
    let lowered = report.lowered.clone().unwrap();
    assert_eq!(lowered.from, vec![2_633], "{report:?}");
    assert!(lowered.seconds > 20.0, "{}", lowered.seconds);
    // Watched from the start, with no gap near the 5 s watchdog.
    assert!(
        watched
            .first()
            .is_some_and(|at| *at < Duration::from_secs(1)),
        "{watched:?}"
    );
    let (at, widest) = watched
        .windows(2)
        .map(|two| (two[0], two[1] - two[0]))
        .max_by_key(|(_, gap)| *gap)
        .unwrap();
    assert!(widest < Duration::from_secs(1), "{widest:?} from {at:?}");
}

/// El's P2 on #2940: a head tipped up keeps its pitch held while the rest
/// waits, so that pitch is judged by the run's own bounds. Pressed over its
/// load bound, or at its temperature bound, the wait ends on that stop as
/// the run's check would, never holding on unchecked.
#[test]
fn a_head_held_while_it_waits_is_judged_by_the_runs_load_and_heat() {
    let limits = let_go_when_refused();
    let pressed = |lamp: &mut Lamp| {
        lamp.strains = vec![(
            Duration::from_secs(3),
            Duration::from_secs(5),
            PITCH,
            limits.load[0] + 60,
        )];
    };
    let heated = |lamp: &mut Lamp| {
        lamp.heats = vec![(Duration::from_secs(3), PITCH, limits.temperature + 1)];
    };
    for (what, set, expect) in [
        ("load", &pressed as &dyn Fn(&mut Lamp), "load"),
        ("heat", &heated, "temperature"),
    ] {
        let (mut lamp, mut clock) = knocked(Duration::from_secs(60), None);
        lamp.servos[PITCH].position = 1_500.0;
        lamp.servos[PITCH].goal = 1_500;
        set(&mut lamp);
        let report = run(&mut lamp, &mut clock, limits, Some(HOME), || None, |_| {});
        let pitch = Joint::WristPitch.id();
        match (&report.stop, expect) {
            (Stop::Load { joint, .. }, "load")
            | (Stop::Temperature { joint, .. }, "temperature") => {
                assert_eq!(*joint, pitch, "{what}");
            }
            (stop, _) => panic!("{what}: {stop:?}"),
        }
        assert_eq!(report.refused_let_go, Some(PROPPED), "{what}");
    }
}

/// #2938: nobody setting it straight, after `WAIT_ASKING` it stops asking,
/// stays let go and reads every `WAIT_READ_LATE` instead of `WAIT_READ`.
#[test]
fn nobody_setting_it_straight_it_stops_asking_and_reads_less() {
    let (mut lamp, mut clock) = knocked(WAIT_ASKING + Duration::from_secs(60), None);
    let mut said = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        let_go_when_refused(),
        Some(HOME),
        || None,
        |glimpse| said.push(glimpse.waiting),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let asking = said
        .iter()
        .filter(|say| **say == Some(SET_STRAIGHT[0]))
        .count();
    let stopped = said
        .iter()
        .filter(|say| **say == Some(STOPPED_ASKING))
        .count();
    // Ten reads a second for ten minutes, then two a second for one.
    assert!((5_900..=6_001).contains(&asking), "{asking}");
    assert!((110..=121).contains(&stopped), "{stopped}");
    assert_eq!(asking + stopped, said.len());
    for joint in COMMANDED {
        assert_eq!(lamp.servos[usize::from(joint.id()) - 1].torque, 0);
    }
}

/// Once a hand has set the head in range and it has stayed there a second,
/// the lamp takes hold where it is and starts as it would have: the wake
/// brings it home slowly and it runs.
#[test]
fn after_a_wait_its_run_is_watched_without_a_gap() {
    // The Reviewer-lead's P2 on #3168: the run that starts after a wait
    // counts its schedule from the steps already taken, so its first step
    // is due a tick later, never the wait's steps later.
    let hand = (Duration::from_secs(1), Duration::from_secs(6), 2_150.0);
    let (mut lamp, mut clock) = knocked(Duration::from_secs(40), Some(hand));
    let time = clock.time.clone();
    let mut watched = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        let_go_when_refused(),
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |_| watched.push(time.get()),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let (at, widest) = watched
        .windows(2)
        .map(|two| (two[0], two[1] - two[0]))
        .max_by_key(|(_, gap)| *gap)
        .unwrap();
    assert!(widest < Duration::from_secs(1), "{widest:?} from {at:?}");
}

#[test]
fn set_back_in_range_it_takes_hold_where_it_is_and_starts_by_itself() {
    let hand = (Duration::from_secs(1), Duration::from_secs(6), 2_150.0);
    let (mut lamp, mut clock) = knocked(Duration::from_secs(30), Some(hand));
    let mut waited = false;
    let mut running = false;
    let report = run(
        &mut lamp,
        &mut clock,
        let_go_when_refused(),
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |glimpse| {
            if glimpse.waiting.is_some() {
                assert!(!running, "let go again after it started");
                waited = true;
            } else if waited {
                running = true;
            }
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(running && waited);
    assert_eq!(report.refused_let_go, Some(SET_STRAIGHT[0]));
    // Taken hold of where the hand left it, then woken home.
    let woke = report.woke.as_ref().unwrap();
    assert_eq!(woke.joints, vec!["wrist_pitch"]);
    assert_eq!(woke.to[0], 2_000);
    assert!((2_140..=2_160).contains(&woke.from[0]), "{woke:?}");
    assert!((lamp.servos[PITCH].position - 2_000.0).abs() < 15.0);
    for joint in COMMANDED {
        assert_eq!(lamp.servos[usize::from(joint.id()) - 1].torque, 1);
    }
    // No goal beyond where the hand left it, and none faster than waking.
    let pitch = goals_of(&lamp, Joint::WristPitch);
    assert!(pitch.iter().all(|goal| *goal <= 2_160), "{pitch:?}");
}

/// A lamp found standing is never let go for its pose: it would fall.
#[test]
fn found_standing_out_of_range_it_is_refused_as_before_and_not_let_go() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(3));
    lamp.may_take_hold = true;
    lamp.arm_moves = true;
    for (index, at) in [(1usize, 2_650.0f32), (2, 2_600.0), (PITCH, 2_500.0)] {
        lamp.servos[index].position = at;
        lamp.servos[index].goal = at as u16;
        lamp.servos[index].status = 0;
    }
    let report = run_standing(
        &mut lamp,
        &mut clock,
        let_go_when_refused(),
        Some(HOME),
        Some(standing(100, 60)),
        || None,
        |_| {},
    );
    assert!(matches!(report.stop, Stop::Refused { .. }), "{report:?}");
    assert!(report.refused_let_go.is_none());
    assert!(
        !lamp
            .registers
            .iter()
            .any(|(_, address, data)| *address == 40 && data == &[0]),
        "{:?}",
        lamp.registers
    );
}

/// The ranges waited for are the start's own: in range by them, the start
/// does not refuse for the pose; one count out, it does.
#[test]
fn the_ranges_waited_for_are_the_starts_own_bounds() {
    let home = [2_000, 2_044, 2_015];
    let at = home.map(f32::from);
    // Not waking, beside the base: the head inside its start window and
    // each joint within the start reach of home.
    let limits = Limits::FOLDED;
    let wait = pose_wait(
        &limits,
        home,
        at,
        [1_900, 2_044, 2_015],
        (true, false),
        false,
    )
    .unwrap();
    assert_eq!(
        wait.ranges,
        [(1_940, 2_020), (1_984, 2_104), (1_955, 2_075)]
    );
    // Found under its window, the head is tipped up: it keeps its torque.
    assert_eq!(wait.joints, vec![Joint::WristRoll, Joint::BaseYaw]);
    assert_eq!(wait.say, PROPPED);
    let drooped = pose_wait(
        &limits,
        home,
        at,
        [2_030, 2_044, 2_015],
        (true, false),
        false,
    )
    .unwrap();
    assert_eq!(drooped.joints, COMMANDED.to_vec());
    assert!(
        pose_wait(
            &limits,
            home,
            at,
            [2_020, 2_104, 1_955],
            (true, false),
            false
        )
        .is_none()
    );
    // Waking: within WAKE_REACH, the window aside; the base said first
    // when it alone is out, and all five let go for a lamp that stands.
    let limits = waking();
    let wait = pose_wait(&limits, home, at, [2_000, 2_044, 2_416], (true, true), true).unwrap();
    assert_eq!(wait.ranges[2], (1_615, 2_415));
    assert_eq!(wait.say, SET_STRAIGHT[2]);
    assert_eq!(wait.joints, Joint::ALL.to_vec());
    // A home given outside the window: no hand helps, so it is not waited for.
    let outside = [1_700, 2_044, 2_015];
    assert!(
        pose_wait(
            &limits,
            outside,
            at,
            [1_000, 2_044, 2_015],
            (true, true),
            false
        )
        .is_none()
    );
}

/// lamp-52e6's own stored limits, read without moving (body.md's 52e6
/// table), by `Joint::index`: yaw, shoulder, elbow, roll, pitch.
const LAMP_52E6_RANGES: [(u16, u16); 5] = [
    (809, 3_239),
    (2_047, 3_799),
    (0, 4_095),
    (1_065, 2_413),
    (1_451, 3_067),
];

fn own() -> Limits {
    Limits {
        own_ranges: true,
        ..Limits::FOLDED
    }
}

fn ranged(ranges: [(u16, u16); 5], cancel_at: Duration) -> (Lamp, Clock) {
    let (mut lamp, clock) = folded(cancel_at);
    for (servo, range) in lamp.servos.iter_mut().zip(ranges) {
        servo.range = range;
    }
    (lamp, clock)
}

/// #3558: off, the servos' own limits are not read, so a lamp whose servos
/// store anything at all runs exactly as before.
#[test]
fn without_own_ranges_the_servos_limits_change_nothing() {
    let writes = |ranges: [(u16, u16); 5]| {
        let (mut lamp, mut clock) = ranged(ranges, Duration::from_secs(8));
        let news = script(&clock.time);
        let report = run(
            &mut lamp,
            &mut clock,
            Limits::FOLDED,
            Some(HOME),
            news,
            |_| {},
        );
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        assert_eq!(report.own_ranges, None);
        (lamp.goals, lamp.registers)
    };
    assert_eq!(writes([(2_000, 2_001); 5]), writes([(0, 4_095); 5]));
}

/// #3558: on, all five are read at start and a lamp whose poses fit them,
/// as lamp-52e6's do, runs as it would without them.
#[test]
fn with_own_ranges_all_five_are_read_and_a_lamp_inside_them_runs_as_before() {
    let writes = |limits: Limits| {
        let (mut lamp, mut clock) = ranged(LAMP_52E6_RANGES, Duration::from_secs(8));
        let news = script(&clock.time);
        let report = run(&mut lamp, &mut clock, limits, Some(HOME), news, |_| {});
        assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
        (report.own_ranges, lamp.goals)
    };
    let (read, goals) = writes(own());
    assert_eq!(read, Some(LAMP_52E6_RANGES));
    assert_eq!(goals, writes(Limits::FOLDED).1);
}

/// #3504: with a window of its own and own ranges, the head pitch servo's
/// limits are read once at start, not once for each.
#[test]
fn with_own_ranges_and_a_pitch_window_the_pitch_limits_are_read_once() {
    let limits = Limits {
        pitch_window: (1_740, 2_040),
        ..own()
    };
    let (mut lamp, mut clock) = ranged(LAMP_52E6_RANGES, Duration::from_secs(2));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, limits, Some(HOME), news, |_| {});
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.own_ranges, Some(LAMP_52E6_RANGES));
    let pitch = Joint::WristPitch.id();
    let reads = lamp.limit_reads.iter().filter(|id| **id == pitch).count();
    assert_eq!(reads, 1, "{:?}", lamp.limit_reads);
}

/// #3504: a stored limit past the encoder, or a range upside down, is a
/// failed read: the lamp does not start, writing nothing, and says which.
#[test]
fn with_own_ranges_a_stored_range_past_the_encoder_or_upside_down_is_refused() {
    for (joint, range) in [
        (Joint::BaseYaw, (809, 5_000)),
        (Joint::WristPitch, (3_000, 2_000)),
    ] {
        let mut ranges = LAMP_52E6_RANGES;
        ranges[joint.index()] = range;
        let (mut lamp, mut clock) = ranged(ranges, Duration::from_secs(2));
        let news = script(&clock.time);
        let report = run(&mut lamp, &mut clock, own(), Some(HOME), news, |_| {});
        match &report.stop {
            Stop::Refused { why } => {
                assert!(why.starts_with("first read: "), "{why}");
                assert!(why.contains(joint.name()), "{why}");
            }
            stop => panic!("{stop:?}"),
        }
        assert_eq!(report.own_ranges, None);
        assert!(lamp.nothing_was_written());
    }
}

/// #3558: a home outside a joint's own limits (less the margin) does not
/// start, writing nothing, and says which.
#[test]
fn with_own_ranges_a_home_outside_them_is_refused() {
    let mut ranges = LAMP_52E6_RANGES;
    // Home's yaw, 2015, is 10 inside this low end: inside the servo, not
    // inside its margin.
    ranges[Joint::BaseYaw.index()] = (2_005, 3_239);
    let (mut lamp, mut clock) = ranged(ranges, Duration::from_secs(2));
    let news = script(&clock.time);
    let report = run(&mut lamp, &mut clock, own(), Some(HOME), news, |_| {});
    match &report.stop {
        Stop::Refused { why } => assert!(why.contains("home base_yaw 2015"), "{why}"),
        stop => panic!("{stop:?}"),
    }
    assert!(lamp.nothing_was_written());
}

/// #3558: a head or base goal outside its joint's own limits (less the
/// margin) ends the run on `Stop::Window`, as an arm goal does.
#[test]
fn with_own_ranges_a_head_goal_outside_them_ends_the_run_on_the_window() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    let mut bus = Bus::new(&mut lamp, &mut clock, None);
    // Without the bound, any goal in 0..=4095 is the guard's to take.
    for goal in [0, 828, 3_220, 4_095] {
        assert_eq!(bus.goal_refused(Joint::BaseYaw, goal), None, "{goal}");
    }
    bus.windows = LAMP_52E6_RANGES.map(own_window);
    let (low, high) = (829, 3_219);
    for goal in [low, 2_015, high] {
        assert_eq!(bus.goal_refused(Joint::BaseYaw, goal), None, "{goal}");
    }
    for goal in [low - 1, high + 1] {
        assert_eq!(
            bus.goal_refused(Joint::BaseYaw, goal),
            Some(Stop::Window {
                joint: Joint::BaseYaw.id(),
                goal,
                window: (low, high),
                from: goal,
            })
        );
    }
    // lamp-52e6's elbow stores the full turn: its goals keep its arm window.
    assert_eq!(bus.window(Joint::ElbowPitch), (0, 4_095));
}

/// #3558: a hold where a head or base joint already is (take hold, its
/// write-back, stop pushing) is checked against the servo's own range, not
/// the inset window: lamp-52e6's roll resting at 2410, inside its stored
/// max 2413 but past max - 20, takes hold, while a motion goal there is
/// still refused.
#[test]
fn with_own_ranges_a_joint_resting_near_its_stop_still_takes_hold() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    let mut bus = Bus::new(&mut lamp, &mut clock, None);
    bus.windows = LAMP_52E6_RANGES.map(own_window);
    bus.ranges = LAMP_52E6_RANGES;
    assert_eq!(bus.hold_refused(Joint::WristRoll, 2_410), None);
    assert!(matches!(
        bus.goal_refused(Joint::WristRoll, 2_410),
        Some(Stop::Window {
            goal: 2_410,
            window: (1_085, 2_393),
            ..
        })
    ));
    // Past the servo's own stop, a hold is refused too.
    assert!(matches!(
        bus.hold_refused(Joint::WristRoll, 2_414),
        Some(Stop::Window {
            goal: 2_414,
            window: (1_065, 2_413),
            ..
        })
    ));
}

/// #3558: with the bound on, a lamp whose roll rests at 2410 with torque
/// off is taken hold of where it is at start, as without the bound.
#[test]
fn with_own_ranges_a_roll_resting_near_its_stop_is_taken_hold_of_at_start() {
    let holding = Limits {
        torque_on: true,
        ..own()
    };
    let (mut lamp, mut clock) = ranged(LAMP_52E6_RANGES, Duration::from_secs(1));
    lamp.may_take_hold = true;
    lamp.servos[ROLL].torque = 0;
    lamp.servos[ROLL].position = 2_410.0;
    let report = run(
        &mut lamp,
        &mut clock,
        holding,
        None,
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.torque_given, vec!["wrist_roll"]);
    assert_eq!(lamp.servos[ROLL].position, 2_410.0);
}

/// #3558: the head and base held where they are in one write (`send_holds`)
/// are checked against their own ranges, as a hold of one joint is; the same
/// goals sent as motion (`send_goals`) are still refused.
#[test]
fn with_own_ranges_a_hold_of_the_head_near_its_stop_is_written_and_motion_is_not() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    let mut bus = Bus::new(&mut lamp, &mut clock, None);
    bus.windows = LAMP_52E6_RANGES.map(own_window);
    bus.ranges = LAMP_52E6_RANGES;
    let hold = [2_000, 2_410, 2_015];
    assert!(bus.send_goals(hold, None).is_err());
    assert!(matches!(
        bus.refused.take(),
        Some(Stop::Window { goal: 2_410, .. })
    ));
    assert_eq!(bus.send_holds(hold, None), Ok(()));
    assert_eq!(bus.goals[Joint::WristRoll.index()], Some(2_410));
    assert_eq!(bus.refused, None);
}

/// A roll whose own stored max is 2064, home's 2044 plus the margin, kept
/// at 2060 by a hand while its goal stays home: past the inset window,
/// inside the servo, and within `REST_SLACK` of its goal.
fn roll_near_its_stop(cancel_at: Duration) -> (Lamp, Clock) {
    let mut ranges = LAMP_52E6_RANGES;
    ranges[Joint::WristRoll.index()] = (1_065, 2_064);
    let (mut lamp, clock) = ranged(ranges, cancel_at);
    lamp.servos[ROLL].position = 2_044.0;
    lamp.servos[ROLL].goal = 2_044;
    lamp.hand = Some((Duration::ZERO, cancel_at, ROLL, 2_060.0));
    (lamp, clock)
}

/// #3558: a joint that has arrived and rests near its goal is given where
/// it is, so nothing pushes; a roll resting inside its own range but past
/// the inset window is rested there, not refused, and the eased way home
/// at the end brings it back inside (#3504).
#[test]
fn with_own_ranges_a_joint_arrived_near_a_stop_rests_there() {
    let (mut lamp, mut clock) = roll_near_its_stop(Duration::from_secs(4));
    let report = run(
        &mut lamp,
        &mut clock,
        own(),
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert_eq!(
        lamp.goals
            .first()
            .map(|write| write.contains(&(Joint::WristRoll.id(), 2_060))),
        Some(true),
        "{:?}",
        lamp.goals
    );
    // It rested until the end, not stopped at once.
    assert!(lamp.goal_times[0] < Duration::from_secs(1));
    assert!(clock.time.get() >= Duration::from_secs(4));
    // At the end the eased way home walks it back in from where it rested
    // (#3504): each step nearer the window, none refused, the last inside.
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    let rolls: Vec<u16> = lamp
        .goals
        .iter()
        .filter_map(|write| {
            write
                .iter()
                .find(|(id, _)| *id == Joint::WristRoll.id())
                .map(|(_, goal)| *goal)
        })
        .skip_while(|goal| *goal != 2_060)
        .collect();
    assert!(rolls.windows(2).all(|pair| pair[1] <= pair[0]), "{rolls:?}");
    assert_eq!(rolls.last(), Some(&2_044), "{rolls:?}");
}

/// #3504: a head or base joint lying past its inset window may be sent back
/// toward it, from where it lies to the window and inside its own stored
/// range; never farther out, past the stored range, or out the other side.
#[test]
fn with_own_ranges_a_goal_back_toward_the_window_is_taken() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    let mut bus = Bus::new(&mut lamp, &mut clock, None);
    let mut ranges = LAMP_52E6_RANGES;
    ranges[Joint::WristRoll.index()] = (1_065, 2_064);
    bus.windows = ranges.map(own_window);
    bus.ranges = ranges;
    let roll = Joint::WristRoll;
    bus.good[roll.index()] = Some(2_060);
    for goal in [2_060, 2_059, 2_050, 2_044, 1_085] {
        assert_eq!(bus.goal_refused(roll, goal), None, "{goal}");
    }
    for goal in [2_061, 2_065, 1_084] {
        assert!(
            matches!(
                bus.goal_refused(roll, goal),
                Some(Stop::Window {
                    window: (1_085, 2_044),
                    ..
                })
            ),
            "{goal}"
        );
    }
    // Read past its stored stop, it is still never sent past that stop.
    bus.good[roll.index()] = Some(2_070);
    assert_eq!(bus.goal_refused(roll, 2_064), None);
    assert!(bus.goal_refused(roll, 2_065).is_some());
    // The base on its low side: yaw stores 809..=3239, window 829..=3219.
    let yaw = Joint::BaseYaw;
    bus.good[yaw.index()] = Some(812);
    for goal in [812, 813, 820, 829] {
        assert_eq!(bus.goal_refused(yaw, goal), None, "{goal}");
    }
    for goal in [811, 808, 3_220] {
        assert!(bus.goal_refused(yaw, goal).is_some(), "{goal}");
    }
    // With no read, the goal last written is where it lies.
    bus.good[yaw.index()] = None;
    bus.goals[yaw.index()] = Some(815);
    assert_eq!(bus.goal_refused(yaw, 815), None);
    assert!(bus.goal_refused(yaw, 814).is_some());
}

/// #3558: a run that ends stops pushing where each joint is; a roll there,
/// inside its own range but past the inset window, is held there and the
/// run ends on what ended it, not on `Stop::Window`.
#[test]
fn with_own_ranges_a_stop_pushing_hold_near_a_stop_is_written() {
    let (mut lamp, mut clock) = roll_near_its_stop(Duration::from_secs(8));
    lamp.event = Some((Duration::from_millis(500), 1, Event::Flag(0x20)));
    let report = run(
        &mut lamp,
        &mut clock,
        own(),
        Some(HOME),
        || Some((Mood::Resting, 0)),
        |_| {},
    );
    assert!(matches!(report.stop, Stop::Status { .. }), "{report:?}");
    assert_eq!(report.held_at.map(|held| held[1]), Some(2_060));
}

/// #3558: the folded arm, the stand and the pitch window are each checked
/// against their joints' own limits; a joint storing 0..=4095 (lamp-52e6's
/// elbow) is never checked, and its goals keep 0..=4095.
#[test]
fn own_ranges_check_every_pose_and_leave_a_full_turn_alone() {
    assert_eq!(own_window((0, 4_095)), (0, 4_095));
    assert_eq!(own_window((809, 3_239)), (829, 3_219));
    let up = standing(1, 10);
    let window = Limits::FOLDED.pitch_window;
    assert_eq!(
        outside_own_ranges(&LAMP_52E6_RANGES, Some(HOME), window, Some(up)),
        None
    );
    // A fold rests on its stop, which may lie past the stored limit: up to
    // `Standing::FOUND_SLACK` (80) under it is taken, as lamp-52e6's factory
    // fold at 2038 under its 2047 (#3504); one count more is refused.
    let mut ranges = LAMP_52E6_RANGES;
    ranges[Joint::BasePitch.index()] = (2_130, 3_799);
    assert_eq!(
        outside_own_ranges(&ranges, Some(HOME), window, Some(up)),
        None
    );
    ranges[Joint::BasePitch.index()] = (2_131, 3_799);
    let why = outside_own_ranges(&ranges, Some(HOME), window, Some(up)).unwrap();
    assert!(why.starts_with("folded arm base_pitch 2050"), "{why}");
    let factory = Standing {
        folded: [2_038, 2_101],
        ..up
    };
    assert_eq!(
        outside_own_ranges(&LAMP_52E6_RANGES, Some(HOME), window, Some(factory)),
        None
    );
    let mut ranges = LAMP_52E6_RANGES;
    ranges[Joint::WristRoll.index()] = (1_065, 2_050);
    let why = outside_own_ranges(&ranges, None, window, Some(up)).unwrap();
    assert!(why.starts_with("stand wrist_roll 2044"), "{why}");
    let mut ranges = LAMP_52E6_RANGES;
    ranges[Joint::WristPitch.index()] = (1_451, 2_040);
    let why = outside_own_ranges(&ranges, None, window, None).unwrap();
    assert!(why.starts_with("pitch window wrist_pitch 2030"), "{why}");
    // Any count at all of a joint storing the full turn.
    let full = [(0, 4_095); 5];
    let far = Standing {
        folded: [0, 4_095],
        ..up
    };
    assert_eq!(
        outside_own_ranges(&full, Some([0, 4_095, 0]), (0, 4_095), Some(far)),
        None
    );
}

/// A hand that turns the base 33 counts at a time, quickly, `turns` times:
/// each turn presses past the base's bound until it gives way where the hand
/// has it, and the hand then holds it there at 130, under the bound, over a
/// rest of 100. A turn every 2.6 s, each after the last hold has eased.
fn turned_in_steps(turns: u16, limp: bool) -> (BodyReport, Vec<Glimpse>) {
    let ms = Duration::from_millis;
    let (mut lamp, mut clock) = folded(Duration::from_secs(12));
    let home = f32::from(HOME[2]);
    lamp.servos[YAW].load = 100;
    lamp.hands = (1..=turns)
        .map(|k| {
            let from = ms(1_000 + 2_600 * u64::from(k - 1));
            (from, from + ms(2_600), YAW, home - 33.0 * f32::from(k))
        })
        .collect();
    let gone = ms(1_000 + 2_600 * u64::from(turns));
    lamp.strains = lamp
        .hands
        .iter()
        .map(|(from, _, _, _)| (*from, *from + ms(120), YAW, 300))
        .chain([(ms(1_000), gone, YAW, 130)])
        .collect();
    let mut seen = Vec::new();
    let report = run(
        &mut lamp,
        &mut clock,
        Limits {
            limp,
            ..giving_way(GiveWay::Return)
        },
        Some(HOME),
        || Some((Mood::Listening, 100)),
        |glimpse| seen.push(*glimpse),
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert_eq!(report.eased[2], turns, "{report:?}");
    (report, seen)
}

/// lamp-52e6, 27 Oct, onboarding's "giving way": a hand turned the base
/// about 30 counts at a time, each turn a stop-pushing of its own, at loads
/// of 104 to 160 under the base's bound of 220, and `body/hand` never said
/// a push. Moved past `HAND_MOVED` from where it first gave, over two holds
/// of the same push, it is a hand, and released once the hand has gone.
#[test]
fn a_base_turned_by_hand_in_small_steps_under_its_bound_says_a_hand() {
    let ms = Duration::from_millis;
    for limp in [false, true] {
        let (report, seen) = turned_in_steps(2, limp);
        assert!(
            seen.iter()
                .any(|g| g.hand && (ms(3_600)..ms(6_200)).contains(&clock_of(g))),
            "limp {limp}: never a hand in the second hold; {report:?}"
        );
        assert!(
            seen.iter()
                .filter(|g| clock_of(g) > ms(10_000))
                .all(|g| !g.hand),
            "limp {limp}: still a hand after it let go"
        );
    }
}

/// One such turn, held still where it gave and pressed under its bound, is
/// not a hand: it never moved `HAND_MOVED` from where it gave.
#[test]
fn a_base_given_way_and_held_still_under_its_bound_is_not_a_hand() {
    for limp in [false, true] {
        let (report, seen) = turned_in_steps(1, limp);
        assert!(seen.iter().all(|g| !g.hand), "limp {limp}: {report:?}");
    }
}

/// The let-go before the power goes (#3697): a lamp found down, as the
/// stop left it, has torque off written to all five and reads each back.
#[test]
fn a_down_lamp_is_let_go_and_each_joint_reads_back_let_go() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(1));
    let out = let_go_stopped(&mut lamp, &mut clock, HOME, Some([2_050, 2_047]));
    assert!(out.down && out.let_go, "{out:?}");
    assert_eq!(out.error, None);
    assert!(let_go_of_the_whole_lamp(&lamp));
    let names: Vec<_> = out.joints.iter().map(|joint| joint.joint).collect();
    assert_eq!(names, Joint::ALL.map(Joint::name));
    assert!(out.joints.iter().all(|joint| joint.torque == Some(0)));
    assert_eq!(out.joints[PITCH].position, 2_000);
}

/// A lamp found up, its arm off its folded counts or its head off home,
/// keeps its torque: let go, it would fall. Nothing is written.
#[test]
fn a_lamp_found_up_keeps_its_torque() {
    for (servo, at) in [(1usize, 2_650.0f32), (PITCH, 1_900.0)] {
        let (mut lamp, mut clock) = folded(Duration::from_secs(1));
        lamp.servos[servo].position = at;
        let out = let_go_stopped(&mut lamp, &mut clock, HOME, Some([2_050, 2_047]));
        assert!(!out.down && !out.let_go, "{out:?}");
        assert!(lamp.registers.is_empty());
        assert!(out.joints.iter().all(|joint| joint.torque == Some(1)));
    }
}

/// lamp-52e6, 28 Oct, 09:39: a base following a goal that moves faster
/// than it turns stayed more than 30 off it for over 10 steps at a load of
/// 120. Following, it is not a hand (#3925), and not a stuck servo either:
/// it does not end the run on tracking, which lets go and dropped the
/// standing lamp with nobody near. Here the base lifts toward fewer counts
/// no faster than 60 counts a second, after a face 300 counts round.
#[test]
fn a_base_following_a_goal_that_outruns_it_does_not_end_on_tracking() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.climb = Some((YAW, 60.0));
    lamp.strains = vec![(Duration::ZERO, Duration::from_secs(8), YAW, 120)];
    let looking = Looking::new([342, 200], [400, 120]).unwrap();
    let limits = Limits {
        give_way: Some(GiveWay::Return),
        ..Limits::folded_with(Tuning {
            reach: [100, 15, 30, 150],
            speed: 120,
            servo_speed: 300,
            servo_acceleration: 60,
        })
        .unwrap()
    };
    let tracking = usize::from(limits.tracking_steps);
    let seen_from = Rc::new(Cell::new((f32::from(HOME[2]), f32::from(HOME[0]))));
    let eyes = face_at(seen_from.clone(), &clock.time, (-300.0, 0.0));
    let (mut off, mut longest) = (0usize, 0usize);
    let report = run_looking(
        &mut lamp,
        &mut clock,
        limits,
        None,
        None,
        Some(looking),
        || Some((Mood::Listening, 100)),
        eyes,
        |glimpse| {
            seen_from.set((
                f32::from(glimpse.positions[2]),
                f32::from(glimpse.positions[0]),
            ));
            off = if glimpse.goals[2].abs_diff(glimpse.positions[2]) > 30 {
                off + 1
            } else {
                0
            };
            longest = longest.max(off);
        },
    );
    assert_eq!(report.stop, Stop::Cancelled, "{report:?}");
    assert!(longest > 2 * tracking, "off its goal {longest} steps only");
    assert_eq!(report.eased, [0; 3], "{report:?}");
}

/// A base stuck 100 counts short of any goal, not turning toward it, at a
/// load under `STALL_LOAD` (so not a hand): it still ends on tracking.
#[test]
fn a_base_stuck_short_of_its_goal_still_ends_on_tracking() {
    let (mut lamp, mut clock) = folded(Duration::from_secs(8));
    lamp.short = Some((YAW, 100.0));
    lamp.strains = vec![(Duration::ZERO, Duration::from_secs(8), YAW, 90)];
    let looking = Looking::new([342, 200], [400, 120]).unwrap();
    let limits = Limits {
        give_way: Some(GiveWay::Return),
        ..Limits::folded_with(Tuning {
            reach: [100, 15, 30, 150],
            speed: 120,
            servo_speed: 300,
            servo_acceleration: 60,
        })
        .unwrap()
    };
    let seen_from = Rc::new(Cell::new((f32::from(HOME[2]), f32::from(HOME[0]))));
    let eyes = face_at(seen_from.clone(), &clock.time, (300.0, 0.0));
    let report = run_looking(
        &mut lamp,
        &mut clock,
        limits,
        None,
        None,
        Some(looking),
        || Some((Mood::Listening, 100)),
        eyes,
        |glimpse| {
            seen_from.set((
                f32::from(glimpse.positions[2]),
                f32::from(glimpse.positions[0]),
            ));
        },
    );
    assert!(
        matches!(report.stop, Stop::Tracking { joint: 1, .. }),
        "{report:?}"
    );
}

/// A gentle hand turning the head of a standing lamp, under the load that
/// eases it, is followed by the stall rule alone: said as a hand all the
/// same, and only after it began (lamp-29e7 on 8080: the roll put 34 counts
/// over a minute at 100 to 144, its bound 150, and no hand was ever said,
/// so onboarding's giving_way failed with the base held). A joint stuck
/// short of its goal at the same load, never carried, is not a hand, nor is
/// a lamp nobody touches.
#[test]
fn a_head_joint_given_way_under_a_gentle_hand_is_said_as_a_hand() {
    let s = Duration::from_secs;
    let ran = |mood: Mood, prepare: &dyn Fn(&mut Lamp)| {
        let (mut lamp, mut clock) = folded(s(14));
        lamp.arm_moves = true;
        lamp.overload_clears = 1;
        prepare(&mut lamp);
        let news = || Some((mood, 100));
        let time = clock.time.clone();
        let mut said = Vec::new();
        let report = run_standing(
            &mut lamp,
            &mut clock,
            giving_way(GiveWay::Return),
            Some(HOME),
            Some(standing(100, 60)),
            news,
            |glimpse| said.push((time.get(), glimpse.hand)),
        );
        let hand: Vec<Duration> = said
            .iter()
            .filter(|(_, hand)| *hand)
            .map(|(at, _)| *at)
            .collect();
        (report, hand)
    };
    // From 7 s a hand turns the roll 18 counts from where it stands
    // (under the 30 that would ease it), at 120, and holds it there.
    // Resting, as 29e7 was in the try: no listening tilt on the roll.
    let (report, hand) = ran(Mood::Resting, &|lamp| {
        lamp.hand = Some((s(7), s(60), ROLL, 2_044.0 - 18.0));
        lamp.strains = vec![(s(7), s(60), ROLL, 120)];
    });
    assert_eq!(report.eased, [0; 3], "under its bound: {report:?}");
    assert!(report.stalls[1] >= 1, "{report:?}");
    assert!(!hand.is_empty(), "never said a hand: {report:?}");
    assert!(hand.iter().all(|at| *at >= s(7)), "{hand:?}");
    // A pitch stuck 30 counts short of every goal at 120 as the lamp
    // listens and looks, never moved by anything.
    let (report, hand) = ran(Mood::Listening, &|lamp| {
        lamp.short = Some((PITCH, 30.0));
        lamp.strains = vec![(Duration::ZERO, s(60), PITCH, 120)];
    });
    assert!(report.stalls[0] >= 1, "{report:?}");
    assert!(hand.is_empty(), "{hand:?}");
    let (report, hand) = ran(Mood::Listening, &|_| {});
    assert_eq!(report.stalls, [0; 5], "{report:?}");
    assert!(hand.is_empty(), "{hand:?}");
}
