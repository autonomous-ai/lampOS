//! What the part that plans what the lamp shows asks of the body
//! (`docs/expression.md`): a little to add to the pose the head is in, as
//! requests on `expression/body` that each run out a fifth of a second
//! after they were made. A planner that stops, stalls or is switched off
//! leaves the body exactly as it is without it.
//!
//! The body's loop reads and writes the servos every 20 ms and never waits
//! for anything else (`crate::bus_out`). So a thread of this module's own
//! takes the requests from the bus and hands them over, and the loop looks
//! at what has come without waiting. Which request counts is decided in
//! [`Requests`], which has no bus and takes its clock from the caller.
//!
//! What is taken from a request: the head's pitch, the head's roll and the
//! base's turn. The shoulder and the elbow are the arm that carries the
//! lamp and are not moved for expression; a gaze is not taken, because the
//! body already looks at the face its own eyes report. Off unless the
//! body's environment says `on`.

use lamp_bus::Bus;
use lamp_msgs::{EXPRESSION_LEASE_US, ExpressionBody};
use std::{f64::consts::TAU, io, sync::mpsc, thread, time::Duration};

/// `on` for the body to take what Expression asks. Anything else: never.
pub const ENVIRONMENT: &str = "LAMP_EXPRESSION_BODY";
/// A servo's turn is 4,096 counts.
pub const COUNTS_A_RADIAN: f64 = 4096.0 / TAU;
/// A request made this far ahead of the body's own clock is not taken: the
/// two read one clock, and a moment's difference is the reading of it.
const AHEAD_US: u64 = 50_000;

/// Which request counts now.
#[derive(Debug)]
pub struct Requests {
    clock_id: String,
    /// The planner's own session, and the newest request taken from it.
    planner: Option<(String, u64)>,
    newest: Option<ExpressionBody>,
    pub taken: u64,
    pub refused: u64,
}

impl Requests {
    /// `clock_id` names this start of the machine, as the planner's does.
    pub fn new(clock_id: impl Into<String>) -> Self {
        Self {
            clock_id: clock_id.into(),
            planner: None,
            newest: None,
            taken: 0,
            refused: 0,
        }
    }

    /// A request came, at `now_us` by the machine's own clock. It is kept
    /// if it is sound, of this start of the machine, has not run out, asks
    /// for no longer than a request may last, and is newer than the one in
    /// hand. Why not, otherwise.
    pub fn take(&mut self, request: ExpressionBody, now_us: u64) -> Result<(), &'static str> {
        let refused = self.judge(&request, now_us);
        match refused {
            Ok(()) => {
                self.planner = Some((request.lease.session_id.clone(), request.lease.sequence));
                self.newest = Some(request);
                self.taken += 1;
            }
            Err(_) => self.refused += 1,
        }
        refused
    }

    fn judge(&self, request: &ExpressionBody, now_us: u64) -> Result<(), &'static str> {
        let lease = &request.lease;
        if request.check().is_err() {
            return Err("it fails its own check");
        }
        if lease.clock_id != self.clock_id {
            return Err("it is of another start of the machine");
        }
        if lease.valid_until_us <= now_us {
            return Err("it has run out");
        }
        if lease.issued_us > now_us.saturating_add(AHEAD_US)
            || lease.valid_until_us.saturating_sub(lease.issued_us) > EXPRESSION_LEASE_US
        {
            return Err("it is from ahead of this clock, or lasts longer than a request may");
        }
        if let Some((session, sequence)) = &self.planner
            && *session == lease.session_id
            && lease.sequence <= *sequence
        {
            return Err("it is no newer than the one in hand");
        }
        Ok(())
    }

    /// Whether the planner asks for stillness now, with a live lease.
    pub fn holding(&self, now_us: u64) -> bool {
        self.newest
            .as_ref()
            .is_some_and(|request| request.hold && request.lease.valid_at(&self.clock_id, now_us))
    }

    /// What to add to the pose at `now_us`: counts for the head's pitch,
    /// its roll and the base's turn. A positive pitch dips the head, as in
    /// the body's own poses. Nothing when nothing is asked, when what was
    /// asked has run out, or while the planner asks for stillness.
    pub fn offsets(&mut self, now_us: u64) -> Option<[f32; 3]> {
        let request = self.newest.as_ref()?;
        if now_us >= request.lease.valid_until_us {
            self.newest = None;
            return None;
        }
        if !request.lease.valid_at(&self.clock_id, now_us) || request.hold {
            return None;
        }
        let counts = |radians: f64| (radians * COUNTS_A_RADIAN) as f32;
        Some([
            counts(request.offsets.head_pitch),
            counts(request.offsets.head_roll),
            counts(request.offsets.base_yaw),
        ])
    }
}

/// This machine's boot identity: the Linux boot id, or on macOS the boot
/// session uuid (`kern.bootsessionuuid`), which changes at every boot too.
fn boot_identity() -> io::Result<String> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
    }
    #[cfg(not(target_os = "linux"))]
    {
        let out = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "kern.bootsessionuuid"])
            .output()?;
        let text = String::from_utf8(out.stdout)
            .map_err(|_| io::Error::other("the boot session id is not text"))?;
        if !out.status.success() || text.trim().is_empty() || text.len() > 128 {
            return Err(io::Error::other("the boot session id is unavailable"));
        }
        Ok(text)
    }
}

/// The body's end of `expression/body`.
pub struct ExpressionIn {
    requests: Requests,
    from_bus: mpsc::Receiver<ExpressionBody>,
    said_refused: u64,
}

impl ExpressionIn {
    /// Listening, if the environment says `on` and the bus and this
    /// machine's boot identity can be had. Otherwise `say` is told why,
    /// once, and the body goes on without.
    pub fn from_environment(say: impl Fn(&str)) -> Option<Self> {
        if std::env::var(ENVIRONMENT).ok().as_deref() != Some("on") {
            return None;
        }
        let made = Bus::from_environment().and_then(|bus| {
            let clock_id = boot_identity()?;
            Self::on(&bus, clock_id.trim())
        });
        match made {
            Ok(listening) => {
                say("it takes what expression asks of the head, from expression/body");
                Some(listening)
            }
            Err(error) => {
                say(&format!("nothing is taken from expression: {error}"));
                None
            }
        }
    }

    pub fn on(bus: &Bus, clock_id: &str) -> io::Result<Self> {
        let mut heard = bus.subscribe_typed(
            ExpressionBody::TOPIC,
            crate::bus_out::SOURCE,
            ExpressionBody::TYPE,
        )?;
        // Room for a second of requests: the loop takes them every 20 ms.
        let (to_loop, from_bus) = mpsc::sync_channel(64);
        thread::Builder::new()
            .name("expression-in".into())
            .spawn(move || {
                loop {
                    match heard.recv(Duration::from_millis(500)) {
                        Ok(Some(message)) => {
                            if let Ok(request) = message.parse::<ExpressionBody>() {
                                match to_loop.try_send(request) {
                                    // The loop has gone: so does this.
                                    Err(mpsc::TrySendError::Disconnected(_)) => return,
                                    // No room: the newest is what matters,
                                    // and the next one is 20 ms away.
                                    Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
                                }
                            }
                        }
                        Ok(None) => {}
                        Err(_) => thread::sleep(Duration::from_millis(200)),
                    }
                }
            })?;
        Ok(Self {
            requests: Requests::new(clock_id),
            from_bus,
            said_refused: 0,
        })
    }

    /// What to add to the pose at `now_us`, by the machine's own clock
    /// (`lamp_ipc::monotonic_us`). Never waits.
    pub fn offsets(&mut self, now_us: u64, say: impl Fn(&str)) -> Option<[f32; 3]> {
        while let Ok(request) = self.from_bus.try_recv() {
            if let Err(why) = self.requests.take(request, now_us)
                && self.requests.refused > self.said_refused
                && (self.requests.refused <= 3 || self.requests.refused.is_power_of_two())
            {
                self.said_refused = self.requests.refused;
                say(&format!(
                    "a request from expression was not taken ({} so far): {why}",
                    self.requests.refused
                ));
            }
        }
        self.requests.offsets(now_us)
    }

    /// Whether stillness is asked now.
    pub fn holding(&self, now_us: u64) -> bool {
        self.requests.holding(now_us)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{BodyOffsets, ExpressionLease};

    const MS: u64 = 1_000;

    fn request(session: &str, sequence: u64, issued_us: u64, pitch: f64) -> ExpressionBody {
        ExpressionBody {
            lease: ExpressionLease {
                clock_id: "boot".into(),
                session_id: session.into(),
                voice_session_id: "voice".into(),
                sequence,
                turn: 1,
                request: 1,
                issued_us,
                apply_us: issued_us,
                valid_until_us: issued_us + EXPRESSION_LEASE_US,
            },
            offsets: BodyOffsets {
                head_pitch: pitch,
                base_yaw: -pitch / 2.0,
                ..BodyOffsets::ZERO
            },
            hold: false,
            gaze: None,
        }
    }

    /// A request is added to the pose for as long as it lasts, in counts,
    /// and is gone a fifth of a second after it was made unless another
    /// has come.
    #[test]
    fn what_is_asked_counts_until_it_runs_out_and_is_given_in_counts() {
        let mut requests = Requests::new("boot");
        assert_eq!(requests.offsets(0), None);
        requests
            .take(request("planner", 1, 1_000 * MS, 0.05), 1_000 * MS)
            .unwrap();
        let asked = requests.offsets(1_010 * MS).unwrap();
        // 0.05 rad is 32.6 counts; the turn was asked for half as much the
        // other way; no tilt was asked.
        assert!((asked[0] - 32.6).abs() < 0.05, "{asked:?}");
        assert!((asked[2] + 16.3).abs() < 0.05, "{asked:?}");
        assert_eq!(asked[1], 0.0);
        assert!(requests.offsets(1_199 * MS).is_some());
        assert_eq!(requests.offsets(1_200 * MS), None);
        // Renewed every 20 ms it goes on, each new one in the last one's place.
        let mut requests = Requests::new("boot");
        for step in 0..50u64 {
            let now = 2_000 * MS + step * 20 * MS;
            requests
                .take(
                    request("planner", step + 1, now, 0.01 * step as f64 / 50.0),
                    now,
                )
                .unwrap();
            assert!(requests.offsets(now + 5 * MS).is_some());
        }
        assert_eq!((requests.taken, requests.refused), (50, 0));
        assert_eq!(requests.offsets(2_000 * MS + 49 * 20 * MS + 200 * MS), None);
    }

    #[test]
    fn a_request_that_is_stale_out_of_order_or_not_of_this_machine_is_not_taken() {
        let mut requests = Requests::new("boot");
        let now = 5_000 * MS;
        requests
            .take(request("planner", 10, now, 0.02), now)
            .unwrap();
        // Run out already, when it comes.
        assert!(
            requests
                .take(request("planner", 11, now - 300 * MS, 0.05), now)
                .is_err()
        );
        // The same again, and an older one.
        assert!(
            requests
                .take(request("planner", 10, now, 0.05), now)
                .is_err()
        );
        assert!(
            requests
                .take(request("planner", 9, now, 0.05), now)
                .is_err()
        );
        // Another start of the machine.
        let mut other = request("planner", 12, now, 0.05);
        other.lease.clock_id = "another-boot".into();
        assert!(requests.take(other, now).is_err());
        // Dated from ahead of this clock, and one that would last a second.
        assert!(
            requests
                .take(request("planner", 13, now + 80 * MS, 0.05), now)
                .is_err()
        );
        let mut long = request("planner", 14, now, 0.05);
        long.lease.valid_until_us = now + 1_000 * MS;
        assert!(requests.take(long, now).is_err());
        // More than the message itself allows.
        assert!(
            requests
                .take(request("planner", 15, now, 0.5), now)
                .is_err()
        );
        assert_eq!((requests.taken, requests.refused), (1, 7));
        // None of them took the place of the one in hand.
        let asked = requests.offsets(now + MS).unwrap();
        assert!((asked[0] - 0.02 * COUNTS_A_RADIAN as f32).abs() < 0.05);
        // A planner that has started again counts from one, and is taken.
        requests
            .take(
                request("planner-again", 1, now + 10 * MS, 0.03),
                now + 10 * MS,
            )
            .unwrap();
        let asked = requests.offsets(now + 11 * MS).unwrap();
        assert!((asked[0] - 0.03 * COUNTS_A_RADIAN as f32).abs() < 0.05);
    }

    /// A request for stillness asks nothing of the pose, and one dated to
    /// apply a moment from now is not applied before then.
    #[test]
    fn stillness_adds_nothing_and_a_request_is_not_applied_before_its_time() {
        let mut requests = Requests::new("boot");
        let now = 9_000 * MS;
        let mut still = request("planner", 1, now, 0.05);
        still.hold = true;
        assert!(!requests.holding(now));
        requests.take(still, now).unwrap();
        assert_eq!(requests.offsets(now + MS), None);
        // Stillness is asked while its lease lasts, and not after.
        assert!(requests.holding(now + MS));
        assert!(!requests.holding(now + EXPRESSION_LEASE_US));
        let mut later = request("planner", 2, now, 0.05);
        later.lease.apply_us = now + 40 * MS;
        requests.take(later, now).unwrap();
        assert_eq!(requests.offsets(now + 20 * MS), None);
        assert!(requests.offsets(now + 40 * MS).is_some());
        // A plain request after it ends the stillness.
        assert!(!requests.holding(now + 40 * MS));
    }
}
