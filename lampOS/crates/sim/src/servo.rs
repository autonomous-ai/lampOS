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
}

impl Servo {
    /// A servo found at `position` counts, free to turn between `min` and `max`.
    pub fn new(id: u8, position: u16, min: u16, max: u16) -> Self {
        let mut servo = Servo {
            id,
            regs: [0; 256],
            position: f64::from(position),
            last: f64::from(position),
        };
        servo.put(reg::MODEL, &MODEL_NUMBER.to_le_bytes());
        servo.put(reg::LIMITS, &min.to_le_bytes());
        servo.put(reg::LIMITS + 2, &max.to_le_bytes());
        servo.put(reg::GOAL, &position.to_le_bytes());
        servo.put(reg::POSITION, &position.to_le_bytes());
        servo.regs[reg::VOLTAGE] = 120;
        servo.regs[reg::TEMPERATURE] = 32;
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

    /// `dt` seconds pass: with torque on it turns toward its goal at its speed.
    fn step(&mut self, dt: f64) {
        let before = self.position;
        if self.torque() && dt > 0.0 {
            let (min, max) = (
                f64::from(self.word(reg::LIMITS)),
                f64::from(self.word(reg::LIMITS + 2)),
            );
            let goal = f64::from(self.goal()).clamp(min.min(max), max.max(min));
            let reach = self.speed() * dt;
            let gap = goal - self.position;
            self.position += gap.clamp(-reach, reach);
        }
        let velocity = if dt > 0.0 {
            (self.position - before) / dt
        } else {
            0.0
        };
        let moving = velocity.abs() >= 1.0;
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
        // A held servo shows a little load, a moving one more (0.1% units).
        let load: u16 = if moving {
            220
        } else if self.torque() {
            40
        } else {
            0
        };
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
}
