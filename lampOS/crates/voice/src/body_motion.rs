//! The lamp's own motors, heard (#1712's B, the first builder at 06:44): a
//! joint that moves makes a sound the microphone takes for a voice. On
//! lamp-4ace at 04:34 the thinking pose's elbow sank 2.5 s into a slow first
//! answer, a line with no words began with it, and it took the turn from
//! the request in flight. So a line that begins while the body moves a
//! joint is taken as the lamp's own sound, as its playout is
//! (`LAMP_MOTION_KEEPS_ANSWER=on`).
//!
//! The body says where its joints are (`body/joint_states`, about 12.5 times
//! a second). Read here without waiting, at most every 20 ms, as the lamp's
//! name is (`bus_out::NameIn`).
use std::{collections::VecDeque, path::Path, time::Duration};

/// The switch: `on` exactly. Off, nothing is read and nothing changes.
pub const ENVIRONMENT: &str = "LAMP_MOTION_KEEPS_ANSWER";

/// A joint that moves, or is sent, 4 servo counts or more (4096 a turn)
/// between two of the body's messages is moving: 50 counts a second at the
/// body's cadence. The thinking pose's head moves at 50 a second and its
/// base at 100 (Fourth on #1722, 05:21); a held servo's jitter is a count or
/// two, under the body's own 6 for "arrived". Positions are whole counts, so
/// the bar sits between 3 and 4.
pub const MOVED_RADIANS: f64 = 3.5 * std::f64::consts::TAU / 4096.0;

/// Two of the body's messages further apart than this are not compared:
/// one was lost, and what moved between them is not known.
const COMPARED_WITHIN_US: u64 = 250_000;

/// A line that begins within this of a move is the move's: the motors'
/// sound and the detector's own lag, the playout's tail
/// (`turn_taking::TAIL_US`).
pub const MARGIN_US: u64 = crate::turn_taking::TAIL_US;

/// How long a line's start waits for the body's word on its moment: about
/// one of its messages and the bus. Under the admission's 200 ms to decide.
pub const WAITS_US: u64 = 120_000;

/// With no message this long, the body is taken as silent (not running, or
/// no arm), and a line is heard as before.
const SILENT_US: u64 = 300_000;

/// Moves are kept this long.
const KEPT_US: u64 = 3_000_000;

/// The bus is looked at this often.
const LOOK_EVERY_US: u64 = 20_000;

/// Whether the switch is on.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// What the body's motion says of a line that began between two moments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Judged {
    /// A joint moved with it: the lamp's own sound, moving from `began` to
    /// `ended`.
    Moved { began: u64, ended: u64 },
    /// The body has not yet said where it was then.
    Waits,
    /// Still then, or the body silent, or waited long enough: as before.
    Still,
}

/// The body's recent moves, each from the message before the one that
/// showed it to that one.
#[derive(Default)]
pub struct Motion {
    last: Option<(u64, Vec<f64>, Vec<f64>)>,
    moves: VecDeque<(u64, u64)>,
}

impl Motion {
    /// The body's joints at `at_us`: where they are and where they are sent.
    pub fn observe(&mut self, at_us: u64, position: &[f64], commanded: &[f64]) {
        if let Some((before, was, was_sent)) = &self.last {
            if at_us <= *before {
                return;
            }
            let moved = |now: &[f64], was: &[f64]| {
                now.len() == was.len()
                    && now
                        .iter()
                        .zip(was)
                        .any(|(now, was)| (now - was).abs() >= MOVED_RADIANS)
            };
            if at_us - before <= COMPARED_WITHIN_US
                && (moved(position, was) || moved(commanded, was_sent))
            {
                match self.moves.back_mut() {
                    Some((_, ended)) if *ended >= *before => *ended = at_us,
                    _ => self.moves.push_back((*before, at_us)),
                }
            }
        }
        self.last = Some((at_us, position.to_vec(), commanded.to_vec()));
        while self
            .moves
            .front()
            .is_some_and(|(_, ended)| at_us.saturating_sub(*ended) > KEPT_US)
        {
            self.moves.pop_front();
        }
    }

    /// A line whose start was heard from `from_us` to `to_us`, judged at
    /// `now_us`, waiting for the body no later than `waits_until_us`. A move
    /// that began after the line's start is not its: the lamp turning to a
    /// voice is the voice's.
    pub fn judge(&self, from_us: u64, to_us: u64, now_us: u64, waits_until_us: u64) -> Judged {
        if let Some(&(began, ended)) = self
            .moves
            .iter()
            .find(|(began, ended)| *began <= to_us && ended.saturating_add(MARGIN_US) >= from_us)
        {
            return Judged::Moved { began, ended };
        }
        match &self.last {
            Some((at, ..))
                if *at < to_us
                    && now_us.saturating_sub(*at) < SILENT_US
                    && now_us < waits_until_us =>
            {
                Judged::Waits
            }
            _ => Judged::Still,
        }
    }
}

/// The body's joints from the bus, for [`Motion`].
pub struct MotionIn {
    joints: lamp_bus::Subscriber,
    looked_at_us: Option<u64>,
    motion: Motion,
}

impl MotionIn {
    pub fn on(root: &Path) -> std::io::Result<Self> {
        Ok(Self {
            joints: lamp_bus::Bus::new(root)?.subscribe_typed(
                lamp_msgs::JointState::TOPIC,
                crate::bus_out::SOURCE,
                lamp_msgs::JointState::TYPE,
            )?,
            looked_at_us: None,
            motion: Motion::default(),
        })
    }

    /// The reader, with the switch and the bus both on (`LAMP_BUS=on`). A
    /// bus that cannot be used is said once and the voice goes on without.
    pub fn from_environment() -> Option<Self> {
        if !on(std::env::var(ENVIRONMENT).ok().as_deref()) {
            return None;
        }
        if std::env::var(crate::bus_out::ENVIRONMENT).ok().as_deref() != Some("on") {
            eprintln!(
                "lamp-voice: {ENVIRONMENT}=on needs {}=on to read the body; a line is heard as before",
                crate::bus_out::ENVIRONMENT
            );
            return None;
        }
        let root = std::env::var_os(lamp_bus::ROOT_VARIABLE)
            .filter(|root| !root.is_empty())
            .unwrap_or_else(|| lamp_bus::DEFAULT_ROOT.into());
        match Self::on(Path::new(&root)) {
            Ok(motion) => Some(motion),
            Err(error) => {
                eprintln!("lamp-voice: the body's motion is not taken from the bus: {error}");
                None
            }
        }
    }

    /// Takes what the body said since the last look, at most every 20 ms,
    /// never waiting. Each message is dated by its wall-clock time against
    /// `now_unix_ms`, on the clock `now_us` is of: the body's own `at_us`
    /// counts from its own start.
    pub fn look(&mut self, now_us: u64, now_unix_ms: u64) {
        if self
            .looked_at_us
            .is_some_and(|at| now_us.saturating_sub(at) < LOOK_EVERY_US)
        {
            return;
        }
        self.looked_at_us = Some(now_us);
        for _ in 0..16 {
            let Ok(Some(message)) = self.joints.recv(Duration::ZERO) else {
                break;
            };
            let Ok(joints) = message.parse::<lamp_msgs::JointState>() else {
                continue;
            };
            if joints.check().is_err() {
                continue;
            }
            let ago_us = now_unix_ms
                .saturating_sub(message.header.at_unix_ms)
                .saturating_mul(1_000);
            self.motion.observe(
                now_us.saturating_sub(ago_us),
                &joints.position,
                &joints.commanded,
            );
        }
    }

    /// [`Motion::judge`], after a [`Self::look`].
    pub fn judge(
        &mut self,
        from_us: u64,
        to_us: u64,
        now_us: u64,
        now_unix_ms: u64,
        waits_until_us: u64,
    ) -> Judged {
        self.look(now_us, now_unix_ms);
        self.motion.judge(from_us, to_us, now_us, waits_until_us)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COUNT: f64 = std::f64::consts::TAU / 4096.0;
    const EVERY: u64 = 80_000;

    /// One joint's positions in counts, one message every 80 ms from 0,
    /// sent where it is.
    fn fed(counts: &[f64]) -> Motion {
        let mut motion = Motion::default();
        for (i, count) in counts.iter().enumerate() {
            let at = i as u64 * EVERY;
            motion.observe(at, &[count * COUNT], &[count * COUNT]);
        }
        motion
    }

    #[test]
    fn only_exact_on_turns_it_on() {
        assert!(on(Some("on")));
        for value in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert!(!on(value), "{value:?}");
        }
    }

    /// A held servo's jitter of a count or two is no move; the thinking
    /// head's 50 counts a second (4 a message) is, from the message before
    /// the one that showed it.
    #[test]
    fn a_move_of_four_counts_a_message_is_a_move_and_jitter_is_not() {
        let still = fed(&[1995.0, 1996.0, 1994.0, 1995.0, 1997.0, 1995.0]);
        assert!(still.moves.is_empty());
        let moving = fed(&[1995.0, 1995.0, 1991.0, 1987.0, 1983.0, 1983.0, 1983.0]);
        assert_eq!(moving.moves, [(EVERY, 4 * EVERY)]);
    }

    /// A joint sent somewhere is moving from that message, before it has
    /// gone far: the goal leads the position.
    #[test]
    fn a_joint_sent_far_is_moving_at_once() {
        let mut motion = Motion::default();
        motion.observe(0, &[1995.0 * COUNT], &[1995.0 * COUNT]);
        motion.observe(EVERY, &[1994.0 * COUNT], &[1949.0 * COUNT]);
        assert_eq!(motion.moves, [(0, EVERY)]);
    }

    /// Messages a lost one apart are not compared, and old moves go.
    #[test]
    fn messages_far_apart_are_not_compared_and_old_moves_go() {
        let mut motion = Motion::default();
        motion.observe(0, &[0.0], &[0.0]);
        motion.observe(COMPARED_WITHIN_US + 1, &[50.0 * COUNT], &[50.0 * COUNT]);
        assert!(motion.moves.is_empty());
        let mut motion = fed(&[0.0, 10.0]);
        assert_eq!(motion.moves.len(), 1);
        motion.observe(EVERY + KEPT_US + 1, &[10.0 * COUNT], &[10.0 * COUNT]);
        assert!(motion.moves.is_empty());
    }

    /// A line that begins within a move, or 100 ms after one, is the
    /// move's; one that begins before a move began is not (the lamp turning
    /// to a voice); before the body has said where it was then, the line
    /// waits, no longer than its time, and a silent body never holds it.
    #[test]
    fn a_line_is_judged_by_the_moves_about_its_start() {
        // Moving from 80 to 320 ms.
        let motion = fed(&[0.0, 0.0, 5.0, 10.0, 15.0, 15.0, 15.0]);
        let moved = Judged::Moved {
            began: EVERY,
            ended: 4 * EVERY,
        };
        let now = 6 * EVERY + 10_000;
        assert_eq!(motion.judge(200_000, 260_000, now, now + 1), moved);
        assert_eq!(
            motion.judge(
                4 * EVERY + MARGIN_US,
                4 * EVERY + MARGIN_US + 60_000,
                now,
                now + 1
            ),
            moved,
            "begun in the move's tail"
        );
        assert_eq!(
            motion.judge(
                4 * EVERY + MARGIN_US + 1,
                4 * EVERY + MARGIN_US + 50_000,
                now,
                now + 1
            ),
            Judged::Still
        );
        assert_eq!(
            motion.judge(0, EVERY - 1, now, now + 1),
            Judged::Still,
            "begun before the move"
        );
        // Said up to 480 ms; a line to 500 ms waits, until its time.
        let last = 6 * EVERY;
        assert_eq!(
            motion.judge(440_000, 500_000, last + 30_000, last + 100_000),
            Judged::Waits
        );
        assert_eq!(
            motion.judge(440_000, 500_000, last + 100_000, last + 100_000),
            Judged::Still
        );
        // A body silent 300 ms holds nothing.
        assert_eq!(
            motion.judge(440_000, 500_000, last + SILENT_US, u64::MAX),
            Judged::Still
        );
        // No body at all.
        assert_eq!(
            Motion::default().judge(0, 60_000, 70_000, u64::MAX),
            Judged::Still
        );
    }

    /// Through a real bus, as the body says it: each message dated by its
    /// wall-clock time on the voice's clock (the body's own `at_us` counts
    /// from its start), so a move the body said 80 ms apart is found.
    #[test]
    fn the_body_said_on_a_real_bus_is_dated_and_found() {
        struct Root(std::path::PathBuf);
        impl Drop for Root {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        // A move is kept only between messages at most `COMPARED_WITHIN_US`
        // apart by their bus stamps, so a host that stretched the 80 ms
        // before the last message past that leaves nothing to find
        // (Reviewer-three's loaded whole run, #1569): that try is the host's,
        // said and run again. The line's window is set from the last
        // message's own publish, not from a later `now`.
        for attempt in 1..=TRIES {
            let root = Root(std::path::PathBuf::from(format!(
                "/tmp/vbm-{}-{}-{attempt}",
                std::process::id(),
                crate::privacy_bus::wall_ms()
            )));
            std::fs::create_dir(&root.0).unwrap();
            let mut joints = MotionIn::on(&root.0).unwrap();
            let mut body = lamp_bus::Bus::new(&root.0)
                .unwrap()
                .publisher_declared(
                    lamp_msgs::JointState::TOPIC,
                    &lamp_msgs::JointState::declaration(),
                    "body",
                )
                .unwrap();
            // Each message's publish, by the voice's clock and the wall's,
            // read just before the bus stamps it.
            let mut sent = Vec::new();
            for (i, count) in [1995.0, 1995.0, 1985.0].into_iter().enumerate() {
                if i > 0 {
                    std::thread::sleep(Duration::from_millis(80));
                }
                let state = lamp_msgs::JointState {
                    name: vec!["elbow".into()],
                    position: vec![count * COUNT],
                    velocity: Vec::new(),
                    effort: Vec::new(),
                    commanded: vec![count * COUNT],
                };
                sent.push((lamp_ipc::monotonic_us(), crate::privacy_bus::wall_ms()));
                // The body's own clock, from its start: not the voice's.
                body.publish_json(i as u64 * EVERY, &state).unwrap();
            }
            let apart_ms = sent[2].1.saturating_sub(sent[1].1);
            if apart_ms + 2 >= COMPARED_WITHIN_US / 1_000 && attempt < TRIES {
                eprintln!(
                    "body motion, try {attempt}: the host put {apart_ms} ms between the last two messages; run again"
                );
                continue;
            }
            let last = sent[2].0;
            let now = lamp_ipc::monotonic_us();
            let judged = joints.judge(
                last - 70_000,
                last - 20_000,
                now,
                crate::privacy_bus::wall_ms(),
                now,
            );
            assert!(
                matches!(judged, Judged::Moved { .. }),
                "{judged:?}; the last two messages {apart_ms} ms apart"
            );
            assert_eq!(joints.motion.moves.len(), 1);
            return;
        }
    }

    /// How often the dated-and-found test runs again when the host stretched
    /// its messages past `COMPARED_WITHIN_US`.
    const TRIES: u32 = 5;
}
