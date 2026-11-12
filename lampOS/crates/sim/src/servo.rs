//! Five simulated STS3215 servos on one half-duplex bus (docs/hardware-contract.md,
//! "Five servos, one bus owner"): the packets the body speaks, a register file
//! for each servo, and a position that follows its goal at the servo's own
//! speed. Pure: bytes in, bytes out, and time given by the caller.

/// Where the five servos are now, in counts, for whoever looks (the camera).
pub type Pose = std::sync::Arc<[std::sync::atomic::AtomicU16; 5]>;

/// The servos on the bus, ids 1 to 5: base yaw, shoulder, elbow, head roll,
/// head pitch (the order of `lamp-body`'s servo ids).
pub const IDS: [u8; 5] = [1, 2, 3, 4, 5];

const BROADCAST: u8 = 0xfe;
const PING: u8 = 0x01;
const READ: u8 = 0x02;
const WRITE: u8 = 0x03;
const SYNC_READ: u8 = 0x82;
const SYNC_WRITE: u8 = 0x83;

/// Register addresses the body uses.
pub mod reg {
    pub const MODEL: usize = 3;
    pub const LIMITS: usize = 9;
    pub const HOMING_OFFSET: usize = 31;
    pub const TORQUE: usize = 40;
    pub const ACCELERATION: usize = 41;
    pub const GOAL: usize = 42;
    pub const GOAL_VELOCITY: usize = 46;
    pub const POSITION: usize = 56;
    pub const VELOCITY: usize = 58;
    pub const LOAD: usize = 60;
    pub const VOLTAGE: usize = 62;
    pub const TEMPERATURE: usize = 63;
    pub const STATUS: usize = 65;
    pub const MOVING: usize = 66;
}

/// The STS3215's model number.
const MODEL_NUMBER: u16 = 777;
/// Gravity load the arm holds when its link is level, in 0.1% of the drive, for the
/// shoulder and the elbow, and the load a joint shows for every 1,000 counts a
/// second squared it speeds up at and for every 1,000 counts a second it moves at.
/// Gravity is calibrated on lamp-75c9 (stable 11756, 11 Nov, 180 s at rest in its
/// standing pose, `body.state`, about 12 a second): the shoulder at 47.8 deg showed
/// 0.09 to 0.10 and the elbow at 43 deg 0.16, the base, roll and pitch 0; against the
/// model's turn from the folded pose there (51.9 and 54.8 deg, sine 0.79 and 0.82)
/// that is 120 and 196. Speed and acceleration are still guesses (no moves read).
const GRAVITY: [f64; 5] = [0.0, 120.0, 196.0, 0.0, 0.0];
const LOAD_PER_ACCEL: f64 = 5.0;
const LOAD_PER_SPEED: f64 = 30.0;
/// The temperature a servo rests at in the lamp's base, degrees (the unloaded base,
/// roll and pitch read 51, 43 and 42 at lamp-75c9 on 11 Nov), how fast it heats with
/// its load (degrees a second at a full load: the shoulder at 0.095 read 48 to 49 and
/// the elbow at 0.16 read 52 to 55) and how fast it cools (a fraction a second: not
/// measured, a guess).
const AMBIENT: f64 = 43.0;
const HEAT: f64 = 0.5;
const COOL: f64 = 1.0 / 120.0;
/// The longest a single step moves and warms the servos, in seconds.
const MOST_STEP: f64 = 0.25;
/// The top acceleration when the register says none, counts a second squared.
const TOP_ACCEL: f64 = 20_000.0;

/// Counts a second when the goal velocity register says none (the servo's own
/// top speed, about 0.7 turn a second).
const TOP_SPEED: f64 = 3_000.0;

/// One servo: its registers and where it is.
#[derive(Clone, Debug)]
pub struct Servo {
    pub id: u8,
    regs: [u8; 256],
    position: f64,
    last: f64,
    /// Counts a second now, signed.
    velocity: f64,
    /// Degrees Celsius, a float behind the temperature register.
    heat: f64,
    /// Where it is at rest folded, the zero of the arm's gravity angle.
    folded: f64,
}

impl Servo {
    /// A servo found at `position` counts, free to turn between `min` and `max`.
    pub fn new(id: u8, position: u16, min: u16, max: u16) -> Self {
        let mut servo = Servo {
            id,
            regs: [0; 256],
            position: f64::from(position),
            last: f64::from(position),
            velocity: 0.0,
            heat: AMBIENT,
            folded: f64::from(position),
        };
        servo.put(reg::MODEL, &MODEL_NUMBER.to_le_bytes());
        servo.put(reg::LIMITS, &min.to_le_bytes());
        servo.put(reg::LIMITS + 2, &max.to_le_bytes());
        servo.put(reg::GOAL, &position.to_le_bytes());
        servo.put(reg::POSITION, &position.to_le_bytes());
        servo.regs[reg::VOLTAGE] = 120;
        servo.regs[reg::TEMPERATURE] = AMBIENT as u8;
        servo
    }

    fn put(&mut self, at: usize, data: &[u8]) {
        for (k, byte) in data.iter().enumerate() {
            if let Some(slot) = self.regs.get_mut(at + k) {
                *slot = *byte;
            }
        }
    }

    fn word(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.regs[at], self.regs[at + 1]])
    }

    /// Where it is, in counts.
    pub fn position(&self) -> u16 {
        self.position.round().clamp(0.0, 4095.0) as u16
    }

    /// Where it was told to go, in counts.
    pub fn goal(&self) -> u16 {
        self.word(reg::GOAL)
    }

    fn torque(&self) -> bool {
        self.regs[reg::TORQUE] == 1
    }

    /// The speed cap, counts a second: the goal velocity register, none meaning the top.
    fn speed(&self) -> f64 {
        match self.word(reg::GOAL_VELOCITY) & 0x7fff {
            0 => TOP_SPEED,
            v => f64::from(v).min(TOP_SPEED),
        }
    }

    /// The acceleration it may use, counts a second squared: the register's
    /// 100 counts a second squared a step, none meaning the top.
    fn accel(&self) -> f64 {
        match self.regs[reg::ACCELERATION] {
            0 => TOP_ACCEL,
            a => f64::from(a) * 100.0,
        }
    }

    /// The gravity load the joint holds at `position`, in 0.1% of the drive:
    /// the arm holds most when its link is level, nothing when it hangs down.
    fn gravity(&self, index: usize) -> f64 {
        let turned = (self.position - self.folded) * std::f64::consts::TAU / 4096.0;
        GRAVITY[index] * turned.sin().abs().max(0.15)
    }

    /// `dt` seconds pass: with torque on it speeds up toward its goal at its
    /// acceleration and speed cap, and slows to arrive at it, and its load and
    /// heat follow what it did.
    fn step(&mut self, dt: f64) {
        let before = self.position;
        let before_velocity = self.velocity;
        if self.torque() && dt > 0.0 {
            let (min, max) = (
                f64::from(self.word(reg::LIMITS)),
                f64::from(self.word(reg::LIMITS + 2)),
            );
            let goal = f64::from(self.goal()).clamp(min.min(max), max.max(min));
            let gap = goal - self.position;
            let accel = self.accel();
            // The speed it may have and still stop at the goal.
            let wanted = gap.signum() * self.speed().min((2.0 * accel * gap.abs()).sqrt());
            self.velocity += (wanted - self.velocity).clamp(-accel * dt, accel * dt);
            let reach = self.velocity * dt;
            // Never past the goal.
            self.position += if reach.abs() >= gap.abs() && reach.signum() == gap.signum() {
                self.velocity = 0.0;
                gap
            } else {
                reach
            };
        } else {
            self.velocity = 0.0;
        }
        let velocity = if dt > 0.0 {
            (self.position - before) / dt
        } else {
            0.0
        };
        let moving = velocity.abs() >= 1.0;
        let accelerating = if dt > 0.0 {
            (self.velocity - before_velocity) / dt
        } else {
            0.0
        };
        let position = self.position();
        self.put(reg::POSITION, &position.to_le_bytes());
        let magnitude = velocity.abs().round().min(32_767.0) as u16;
        let signed = if velocity < 0.0 {
            magnitude | 0x8000
        } else {
            magnitude
        };
        self.put(reg::VELOCITY, &signed.to_le_bytes());
        self.regs[reg::MOVING] = u8::from(moving);
        // What a servo shows: gravity on the arm, the push of speeding up and a
        // little for moving (0.1% units); a joint held at rest with no arm to hold
        // shows none (the base, roll and pitch of lamp-75c9 read 0).
        let index = usize::from(self.id.saturating_sub(1)).min(4);
        let load: u16 = if self.torque() {
            (self.gravity(index)
                + accelerating.abs() / 1_000.0 * LOAD_PER_ACCEL
                + velocity.abs() / 1_000.0 * LOAD_PER_SPEED)
                .min(1_000.0) as u16
        } else {
            0
        };
        // It warms with its load and cools toward the room.
        // Toward where this load settles, exactly (not a step of `COOL * dt`, which
        // overshoots for a `dt` of 120 s or more).
        let settles = AMBIENT + f64::from(load) / 1_000.0 * HEAT * 120.0;
        self.heat += (settles - self.heat) * (1.0 - (-COOL * dt).exp());
        self.regs[reg::TEMPERATURE] = self.heat.round().clamp(0.0, 255.0) as u8;
        self.put(reg::LOAD, &load.to_le_bytes());
        self.last = before;
    }
}

/// The bus: bytes the body wrote go in with [`Bus::feed`], the replies come out.
#[derive(Clone, Debug)]
pub struct Bus {
    servos: Vec<Servo>,
    buffer: Vec<u8>,
    pose: Pose,
}

/// What every servo stores unless told otherwise: the full 0..=4095, which `lamp-body` takes
/// as "no range stored" and keeps its own guard for.
pub const FULL: (u16, u16) = (0, 4095);

/// lamp-75c9's stored angle limits, read by the lamp's own `lamp-body` (a unit test comment at
/// #6184's head, `a_lamps_reach_is_90_percent_of_its_hardware_from_the_pose_on_each_side`: "75c9's
/// own ranges, read on 10 Nov: yaw 820-3268 from 2012, roll 897-3153 from 2059, pitch 1457-3029
/// from 1791"). Only those three are known: the shoulder and the elbow stay at [`FULL`] (their
/// reach comes from the arm's window, not stored ranges).
pub const RANGES_75C9: [(u16, u16); 5] = [(820, 3268), FULL, FULL, (897, 3153), (1457, 3029)];

/// The stored ranges asked of lamp-sim (`LAMP_SIM_SERVO_RANGES`): none or `full`, every servo at
/// [`FULL`]; `75c9`, [`RANGES_75C9`]; or a list `joint=LOW-HIGH,...` (joints `yaw`, `shoulder`,
/// `elbow`, `roll`, `pitch`), the rest at [`FULL`].
pub fn ranges_from(spec: Option<&str>) -> Result<[(u16, u16); 5], String> {
    let spec = spec.map(str::trim).unwrap_or("");
    match spec {
        "" | "full" => return Ok([FULL; 5]),
        "75c9" => return Ok(RANGES_75C9),
        _ => {}
    }
    let mut ranges = [FULL; 5];
    for part in spec.split(',') {
        let bad = || format!("LAMP_SIM_SERVO_RANGES: {part:?} is not joint=LOW-HIGH");
        let (joint, range) = part.trim().split_once('=').ok_or_else(bad)?;
        let k = ["yaw", "shoulder", "elbow", "roll", "pitch"]
            .iter()
            .position(|name| *name == joint.trim())
            .ok_or_else(|| format!("LAMP_SIM_SERVO_RANGES: no joint {joint:?}"))?;
        let (low, high) = range.split_once('-').ok_or_else(bad)?;
        let (low, high) = (
            low.trim().parse::<u16>().map_err(|_| bad())?,
            high.trim().parse::<u16>().map_err(|_| bad())?,
        );
        if low >= high || high > 4095 {
            return Err(format!(
                "LAMP_SIM_SERVO_RANGES: {part:?}: LOW below HIGH, HIGH at most 4095"
            ));
        }
        ranges[k] = (low, high);
    }
    Ok(ranges)
}

/// The counts of one lamp's body: where its servos are found folded, and what `lamp-body` is
/// told of its home, folded arm and stand (`--home`, `--folded-arm`, `--stand`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts {
    /// Found folded, in the servos' order: base yaw, shoulder, elbow, head roll, head pitch.
    pub found: [u16; 5],
    /// The head's home: pitch, roll, yaw.
    pub home: [u16; 3],
    /// The folded arm: shoulder, elbow.
    pub folded_arm: [u16; 2],
    /// The stand: pitch, roll, yaw, shoulder, elbow.
    pub stand: [u16; 5],
}

/// lamp-fc4b's, the default (its measured home).
pub const FC4B: Counts = Counts {
    found: [2028, 2048, 1946, 2085, 1817],
    home: [1817, 2085, 2028],
    folded_arm: [2048, 1946],
    stand: [1772, 2085, 2028, 2621, 2558],
};

/// lamp-75c9's. Sources: the arm (shoulder 2076, elbow 2045 folded; 2639 and 2570 at the stand) and the head's folded pitch
/// 1791 are `get body` read on 10 Nov (the same numbers are in `docs/lamp.model.json`, "measured"). The head's roll, yaw and
/// stand pose are the real lamp-body's own journal reads of lamp-75c9 on 11 Nov, as [pitch, roll, yaw]: standing at 17:35
/// [1780, 2057, 2026]; folded at 17:42 [1756-1760, 2057, 2000] (pitch read mid-move, not used); folded at 18:05
/// [1785, 2057, 1999]. So roll is 2057 and yaw 2000 folded (1999-2000 in both folded reads), the stand's head pose is the 17:35
/// read, and the folded pitch stays the 10 Nov `get body` 1791 (the 18:05 journal read is 1785, 6 counts or half a degree under it).
pub const L75C9: Counts = Counts {
    found: [2000, 2076, 2045, 2057, 1791],
    home: [1791, 2057, 2000],
    folded_arm: [2076, 2045],
    stand: [1780, 2057, 2026, 2639, 2570],
};

/// The body counts asked of lamp-sim (`LAMP_SIM_ARM`): none or `fc4b`, [`FC4B`]; `75c9`, [`L75C9`].
pub fn counts_from(spec: Option<&str>) -> Result<Counts, String> {
    match spec.map(str::trim).unwrap_or("") {
        "" | "fc4b" => Ok(FC4B),
        "75c9" => Ok(L75C9),
        other => Err(format!("LAMP_SIM_ARM: {other:?} is fc4b or 75c9")),
    }
}

impl Bus {
    /// A bus of servos 1 to 5, found at `found` counts, free inside `limits`.
    pub fn new(found: [u16; 5], limits: [(u16, u16); 5]) -> Self {
        Bus {
            servos: IDS
                .iter()
                .enumerate()
                .map(|(k, id)| Servo::new(*id, found[k], limits[k].0, limits[k].1))
                .collect(),
            buffer: Vec::new(),
            pose: Pose::default(),
        }
    }

    /// Where the servos are, kept up to date by [`Bus::step`].
    pub fn pose(&self) -> Pose {
        std::sync::Arc::clone(&self.pose)
    }

    pub fn servos(&self) -> &[Servo] {
        &self.servos
    }

    /// `dt` seconds pass.
    pub fn step(&mut self, dt: f64) {
        // A process that was stopped (a laptop asleep) hands the whole gap over:
        // the servos move and warm for at most this long, as one tick of a lamp would.
        let dt = dt.min(MOST_STEP);
        for (servo, slot) in self.servos.iter_mut().zip(self.pose.iter()) {
            servo.step(dt);
            slot.store(servo.position(), std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn servo(&mut self, id: u8) -> Option<&mut Servo> {
        self.servos.iter_mut().find(|servo| servo.id == id)
    }

    /// Bytes from the body; what the servos answer, in order. A packet cut
    /// short waits for the rest; bytes that start no packet are dropped.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.buffer.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            let Some(start) = self.buffer.windows(2).position(|w| w == [0xff, 0xff]) else {
                // Keep a lone 0xff, which may be a packet's first byte.
                let keep = usize::from(self.buffer.last() == Some(&0xff));
                let cut = self.buffer.len() - keep;
                self.buffer.drain(..cut);
                return out;
            };
            self.buffer.drain(..start);
            if self.buffer.len() < 4 {
                return out;
            }
            let length = usize::from(self.buffer[3]);
            let total = length + 4;
            if length < 2 || total > 250 {
                self.buffer.drain(..2);
                continue;
            }
            if self.buffer.len() < total {
                return out;
            }
            let packet: Vec<u8> = self.buffer.drain(..total).collect();
            let sum = packet[2..total - 1]
                .iter()
                .fold(0u8, |s, b| s.wrapping_add(*b));
            if !sum != packet[total - 1] {
                continue;
            }
            self.packet(packet[2], packet[4], &packet[5..total - 1], &mut out);
        }
    }

    fn packet(&mut self, id: u8, instruction: u8, data: &[u8], out: &mut Vec<u8>) {
        match instruction {
            PING => {
                if let Some(servo) = self.servo(id) {
                    status(servo.id, &[], out);
                }
            }
            READ if data.len() == 2 => {
                if let Some(servo) = self.servo(id) {
                    let (at, count) = (usize::from(data[0]), usize::from(data[1]));
                    let end = (at + count).min(256);
                    let bytes = servo.regs[at.min(256)..end].to_vec();
                    status(servo.id, &bytes, out);
                }
            }
            WRITE if !data.is_empty() => {
                let at = usize::from(data[0]);
                let targets: Vec<u8> = if id == BROADCAST {
                    IDS.to_vec()
                } else {
                    vec![id]
                };
                for target in targets {
                    if let Some(servo) = self.servo(target) {
                        servo.put(at, &data[1..]);
                        if id != BROADCAST {
                            status(servo.id, &[], out);
                        }
                    }
                }
            }
            SYNC_READ if data.len() >= 2 => {
                let (at, count) = (usize::from(data[0]), usize::from(data[1]));
                for wanted in &data[2..] {
                    if let Some(servo) = self.servo(*wanted) {
                        let end = (at + count).min(256);
                        let bytes = servo.regs[at.min(256)..end].to_vec();
                        status(servo.id, &bytes, out);
                    }
                }
            }
            SYNC_WRITE if data.len() >= 2 => {
                let (at, count) = (usize::from(data[0]), usize::from(data[1]));
                for entry in data[2..].chunks_exact(count + 1) {
                    if let Some(servo) = self.servo(entry[0]) {
                        servo.put(at, &entry[1..]);
                    }
                }
            }
            _ => {}
        }
    }
}

/// A status packet from servo `id`, no error, with `parameters`.
fn status(id: u8, parameters: &[u8], out: &mut Vec<u8>) {
    let mut packet = vec![0xff, 0xff, id, parameters.len() as u8 + 2, 0];
    packet.extend_from_slice(parameters);
    let sum = packet[2..].iter().fold(0u8, |s, b| s.wrapping_add(*b));
    packet.push(!sum);
    out.extend(packet);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: u8, instruction: u8, parameters: &[u8]) -> Vec<u8> {
        let mut packet = vec![0xff, 0xff, id, parameters.len() as u8 + 2, instruction];
        packet.extend_from_slice(parameters);
        let sum = packet[2..].iter().fold(0u8, |s, b| s.wrapping_add(*b));
        packet.push(!sum);
        packet
    }

    fn bus() -> Bus {
        Bus::new([2000, 2100, 2200, 2048, 2048], [(0, 4095); 5])
    }

    /// A bus whose servos were found at `found` (lamp-sim's own `found_bus`).
    fn bus_from(found: [u16; 5]) -> Bus {
        Bus::new(found, [(0, 4095); 5])
    }

    #[test]
    fn a_sync_read_answers_one_status_packet_for_each_servo() {
        let mut bus = bus();
        let out = bus.feed(&request(
            BROADCAST,
            SYNC_READ,
            &[reg::POSITION as u8, 2, 1, 2, 3, 4, 5],
        ));
        assert_eq!(out.len(), 5 * 8);
        assert_eq!(
            &out[..8],
            &[0xff, 0xff, 1, 4, 0, 0xd0, 0x07, !(1u8 + 4 + 0xd0 + 7)]
        );
    }

    #[test]
    fn a_sync_write_sets_goals_and_the_position_follows_at_the_speed_cap() {
        let mut bus = bus();
        bus.feed(&request(1, WRITE, &[reg::TORQUE as u8, 1]));
        bus.feed(&request(1, WRITE, &[reg::GOAL_VELOCITY as u8, 100, 0]));
        let mut goals = vec![reg::GOAL as u8, 2];
        goals.extend([1, 0xe8, 0x07]);
        bus.feed(&request(BROADCAST, SYNC_WRITE, &goals));
        assert_eq!(bus.servos()[0].goal(), 2024);
        bus.step(0.1);
        assert_eq!(bus.servos()[0].position(), 2010);
        bus.step(10.0);
        assert_eq!(bus.servos()[0].position(), 2024);
    }

    #[test]
    fn without_torque_it_stays_where_it_is() {
        let mut bus = bus();
        bus.feed(&request(2, WRITE, &[reg::GOAL as u8, 0x00, 0x08]));
        bus.step(5.0);
        assert_eq!(bus.servos()[1].position(), 2100);
    }

    #[test]
    fn a_packet_in_two_pieces_is_answered_once_and_noise_is_skipped() {
        let mut bus = bus();
        let packet = request(3, PING, &[]);
        assert!(bus.feed(&[0x00, 0x12]).is_empty());
        assert!(bus.feed(&packet[..3]).is_empty());
        let out = bus.feed(&packet[3..]);
        assert_eq!(out, vec![0xff, 0xff, 3, 2, 0, !(3u8 + 2)]);
    }

    #[test]
    fn a_bad_checksum_gets_no_answer() {
        let mut bus = bus();
        let mut packet = request(3, PING, &[]);
        *packet.last_mut().unwrap() ^= 1;
        assert!(bus.feed(&packet).is_empty());
    }

    #[test]
    fn it_speeds_up_by_its_acceleration_and_stops_at_the_goal_without_overshooting() {
        let mut bus = bus();
        bus.feed(&request(1, WRITE, &[reg::TORQUE as u8, 1]));
        bus.feed(&request(1, WRITE, &[reg::ACCELERATION as u8, 10]));
        bus.feed(&request(1, WRITE, &[reg::GOAL_VELOCITY as u8, 0xe8, 0x03]));
        bus.feed(&request(1, WRITE, &[reg::GOAL as u8, 0xd0, 0x08]));
        // 1,000 counts a second squared: after 0.1 s it has moved 5 counts, not 100.
        bus.step(0.1);
        let early = i32::from(bus.servos()[0].position()) - 2000;
        assert!((1..=10).contains(&early), "{early}");
        let mut last = bus.servos()[0].position();
        for _ in 0..400 {
            bus.step(0.05);
            let now = bus.servos()[0].position();
            assert!(now >= last && now <= 2256, "{now}");
            last = now;
        }
        assert_eq!(bus.servos()[0].position(), 2256);
    }

    #[test]
    fn the_arm_shows_a_gravity_load_the_base_does_not_and_it_warms_under_it() {
        let mut bus = bus();
        for id in [1u8, 2] {
            bus.feed(&request(id, WRITE, &[reg::TORQUE as u8, 1]));
        }
        // The shoulder raised a quarter turn from where it was found: level link.
        bus.feed(&request(2, WRITE, &[reg::GOAL as u8, 0x34, 0x0c]));
        for _ in 0..2_000 {
            bus.step(0.05);
        }
        let (yaw, shoulder) = (&bus.servos()[0], &bus.servos()[1]);
        let load = |s: &Servo| u16::from_le_bytes([s.regs[reg::LOAD], s.regs[reg::LOAD + 1]]);
        assert!(
            load(shoulder) > load(yaw) + 100,
            "{} {}",
            load(shoulder),
            load(yaw)
        );
        assert!(shoulder.regs[reg::TEMPERATURE] > yaw.regs[reg::TEMPERATURE]);
    }

    #[test]
    fn a_long_gap_neither_overshoots_the_heat_nor_throws_the_servo() {
        // One step of an hour (the exact update) settles at the load's own
        // temperature and never passes it, however long the gap.
        let mut servo = Servo::new(1, 2000, 0, 4095);
        servo.heat = 80.0;
        servo.step(3_600.0);
        assert!(
            servo.heat < AMBIENT + 1.0 && servo.heat >= AMBIENT,
            "{}",
            servo.heat
        );
    }

    #[test]
    fn a_long_gap_moves_a_servo_for_one_tick_not_the_whole_gap() {
        // Torque on, a far goal, a speed cap of 1,000 counts a second: an hour's gap
        // is handed over in steps no longer than MOST_STEP, so the servo moves at
        // most speed * MOST_STEP (+ a count), not the 2,000 counts to its goal.
        let mut bus = bus();
        bus.feed(&request(1, WRITE, &[reg::TORQUE as u8, 1]));
        bus.feed(&request(1, WRITE, &[reg::GOAL_VELOCITY as u8, 0xe8, 0x03]));
        bus.feed(&request(1, WRITE, &[reg::GOAL as u8, 0xa0, 0x0f]));
        assert_eq!(bus.servos()[0].goal(), 4000);
        let before = f64::from(bus.servos()[0].position());
        bus.step(3_600.0);
        let moved = f64::from(bus.servos()[0].position()) - before;
        assert!(moved > 0.0 && moved <= 1_000.0 * MOST_STEP + 1.0, "{moved}");
    }

    #[test]
    fn at_lamp_75c9s_standing_pose_the_arm_reads_what_the_lamp_read() {
        // Stand 2639 / 2570 counts for the shoulder and elbow, torque on, still:
        // lamp-75c9 (11756) read 0.09 to 0.10 and 0.16, the others 0.
        let mut bus = bus_from([2028, 2048, 1946, 2085, 1817]);
        for id in 1u8..=5 {
            bus.feed(&request(id, WRITE, &[reg::TORQUE as u8, 1]));
        }
        bus.feed(&request(2, WRITE, &[reg::GOAL as u8, 0x4f, 0x0a]));
        bus.feed(&request(3, WRITE, &[reg::GOAL as u8, 0x0a, 0x0a]));
        for _ in 0..400 {
            bus.step(0.05);
        }
        let load = |at: usize| {
            let servo = &bus.servos()[at];
            f64::from(u16::from_le_bytes([
                servo.regs[reg::LOAD],
                servo.regs[reg::LOAD + 1],
            ])) / 1_000.0
        };
        assert!((0.09..=0.10).contains(&load(1)), "shoulder {}", load(1));
        assert!((0.15..=0.17).contains(&load(2)), "elbow {}", load(2));
        assert_eq!((load(0), load(3), load(4)), (0.0, 0.0, 0.0));
        // And it has warmed to where the lamp read (shoulder 48 to 49, elbow 52 to 55).
        for _ in 0..4_000 {
            bus.step(0.25);
        }
        let heat = |at: usize| f64::from(bus.servos()[at].regs[reg::TEMPERATURE]);
        assert!((47.0..=50.0).contains(&heat(1)), "shoulder {}", heat(1));
        assert!((51.0..=55.0).contains(&heat(2)), "elbow {}", heat(2));
        assert_eq!(heat(0), AMBIENT);
    }

    #[test]
    fn stored_ranges_default_to_full_and_take_75c9s_or_a_list() {
        assert_eq!(ranges_from(None).unwrap(), [FULL; 5]);
        assert_eq!(ranges_from(Some("full")).unwrap(), [FULL; 5]);
        assert_eq!(ranges_from(Some("75c9")).unwrap(), RANGES_75C9);
        assert_eq!(
            ranges_from(Some("yaw=820-3268, pitch=1457-3029")).unwrap(),
            [(820, 3268), FULL, FULL, FULL, (1457, 3029)]
        );
        for bad in [
            "yaw",
            "yaw=5",
            "nose=1-2",
            "yaw=9-3",
            "yaw=1-5000",
            "yaw=a-b",
        ] {
            assert!(ranges_from(Some(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_body_counts_default_to_fc4b_and_75c9_is_a_preset() {
        assert_eq!(counts_from(None).unwrap(), FC4B);
        assert_eq!(counts_from(Some("fc4b")).unwrap(), FC4B);
        assert_eq!(counts_from(Some("75c9")).unwrap(), L75C9);
        assert!(counts_from(Some("fc4c")).is_err());
        // Found folded at the folded arm and the head's home, in the servos' order.
        for lamp in [FC4B, L75C9] {
            assert_eq!(lamp.found[1..3], lamp.folded_arm);
            assert_eq!([lamp.found[4], lamp.found[3], lamp.found[0]], lamp.home);
        }
        assert_eq!(
            L75C9.stand,
            [1780, 2057, 2026, 2639, 2570],
            "the journal's standing read and get body's arm"
        );
        assert_eq!(L75C9.home, [1791, 2057, 2000]);
    }
}
