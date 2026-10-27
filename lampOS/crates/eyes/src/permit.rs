//! Whether the camera may be open: `{"camera":true}` on the permit socket
//! at least every [`STALE`], and `{"camera":false}` closes it at once.
//! Anything else on the socket (a full look's ask, `--full-look`; a close
//! receipt's ask, [`receipt_asked`]) is kept for the eyes' loop to read
//! ([`Permit::heard`]).

use std::os::unix::net::UnixDatagram;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A permission older than this has run out.
pub const STALE: Duration = Duration::from_millis(1_500);

/// What the voice service says about the camera: `Some(true)` it may be
/// open, `Some(false)` it must close now. Anything else says nothing.
pub fn of(datagram: &[u8]) -> Option<bool> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Message {
        camera: bool,
    }
    serde_json::from_slice::<Message>(datagram)
        .ok()
        .map(|message| message.camera)
}

/// The close the voice asks a receipt for (`{"close_receipt":N}`, #1802),
/// sent after its `{"camera":false}`. Anything else says nothing.
pub fn receipt_asked(datagram: &[u8]) -> Option<u64> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Ask {
        close_receipt: u64,
    }
    serde_json::from_slice::<Ask>(datagram)
        .ok()
        .map(|ask| ask.close_receipt)
}

/// The receipt asked for, said only once the camera is shut (#1802).
#[derive(Debug)]
pub struct CloseReceipt {
    asked: Option<u64>,
    /// Whether the last stop of the device returned without a fault. True
    /// before any open: nothing is held.
    released: bool,
}

impl Default for CloseReceipt {
    fn default() -> Self {
        Self {
            asked: None,
            released: true,
        }
    }
}

impl CloseReceipt {
    /// A receipt asked for; a newer ask replaces one not yet said.
    pub fn asked(&mut self, close: u64) {
        self.asked = Some(close);
    }

    /// Every stop of the device, with whether it returned without a fault.
    pub fn stopped<E>(&mut self, result: Result<(), E>) {
        self.released = result.is_ok();
    }

    /// The receipt to say now, once, if one is asked and the camera is
    /// shut. With the camera open nothing is said and the ask waits.
    pub fn due(&mut self, shut: bool) -> Option<lamp_msgs::CameraClosed> {
        if !shut {
            return None;
        }
        self.asked.take().map(|close| lamp_msgs::CameraClosed {
            close,
            released: self.released,
        })
    }
}

/// The newest permission heard, if it has not been taken back.
///
/// A thread of its own waits on the socket and stamps each permission when it
/// arrives. A permission is as old as when it came, not as when the eyes got
/// round to it: one that queued up while the camera was blocked opening has
/// aged all that time and cannot renew a permission that has run out.
#[derive(Debug, Default)]
pub struct Permit {
    at: Arc<Mutex<Option<Instant>>>,
    /// Datagrams that are not a permission, newest last, at most [`HEARD`].
    others: Arc<Mutex<Vec<Vec<u8>>>>,
    /// Which hold this is ([`Permit::held`]): one more at each close and at
    /// each permission after one ran out, changed only under `at`'s lock.
    hold: Arc<AtomicU64>,
    /// First permission of this hold, in the camera frame's monotonic domain.
    /// Renewals do not move it; queued frames from an older hold cannot pass.
    began_us: Arc<AtomicU64>,
}

/// Datagrams other than a permission kept for the loop at most: an older
/// one is dropped when another comes.
pub const HEARD: usize = 8;

impl Permit {
    /// Hears the socket from now on. If the socket fails, nothing renews the
    /// permission and the camera closes once it has run out.
    pub fn listen(socket: UnixDatagram) -> std::io::Result<Self> {
        Self::listen_with_authority(socket, None)
    }

    /// Optional private-disclosure owner. Ordinary camera permission is
    /// unchanged; explicit close revokes its kernel witness before `at`.
    pub fn listen_with_authority(
        socket: UnixDatagram,
        authority: Option<Arc<crate::camera_authority::Authority>>,
    ) -> std::io::Result<Self> {
        socket.set_nonblocking(false)?;
        let permit = Permit::default();
        let at = Arc::clone(&permit.at);
        let others = Arc::clone(&permit.others);
        let hold = Arc::clone(&permit.hold);
        let began_us = Arc::clone(&permit.began_us);
        std::thread::Builder::new()
            .name("permit".into())
            .spawn(move || {
                let mut datagram = [0u8; 64];
                while let Ok(length) = socket.recv(&mut datagram) {
                    let allowed = match of(&datagram[..length]) {
                        Some(allowed) => allowed,
                        None => {
                            let mut others = others
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            if others.len() >= HEARD {
                                others.remove(0);
                            }
                            others.push(datagram[..length].to_vec());
                            continue;
                        }
                    };
                    // Receipt time, not eventual lock acquisition, owns the
                    // lease age if a local publication briefly holds the lock.
                    let heard_at = Instant::now();
                    let heard_us = lamp_ipc::monotonic_us();
                    if !allowed && let Some(authority) = &authority {
                        // One shared-endpoint shutdown, before a blocked main
                        // publication can hold us at the timestamp mutex.
                        let _ = authority.close();
                    }
                    let mut received_at =
                        at.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    received_permission(
                        &mut received_at,
                        &hold,
                        &began_us,
                        allowed,
                        heard_at,
                        heard_us,
                    );
                    let current = hold.load(Ordering::Relaxed);
                    let began = began_us.load(Ordering::Relaxed);
                    drop(received_at);
                    if allowed && let Some(authority) = &authority {
                        update_disclosure_permission(
                            authority,
                            &at,
                            &hold,
                            &began_us,
                            current,
                            began,
                            Receipt {
                                at: heard_at,
                                us: heard_us,
                            },
                        );
                    }
                }
                if let Some(authority) = &authority {
                    let _ = authority.close();
                }
            })?;
        Ok(permit)
    }

    /// The datagrams heard since the last call that were not a permission,
    /// oldest first.
    pub fn heard(&self) -> Vec<Vec<u8>> {
        std::mem::take(
            &mut *self
                .others
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }

    /// Whether the camera may be open now.
    pub fn holds(&self) -> bool {
        self.at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some_and(|at| at.elapsed() < STALE)
    }

    /// The hold the camera may be open under now, or `None`: another once
    /// it was taken back or ran out, though it is given again. A photo yes
    /// waits only within one (`photo::PendingYes`, #2856).
    pub fn held(&self) -> Option<u64> {
        let at = self
            .at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        at.is_some_and(|at| at.elapsed() < STALE)
            .then(|| self.hold.load(Ordering::Relaxed))
    }

    /// Bind the bytes just read to their actual permission hold. V4L2's
    /// monotonic capture time must be after this hold began, not just the time
    /// the buffered frame was taken out of the driver.
    pub fn frame_hold(&self, frame_us: u64) -> Option<u64> {
        let at = self
            .at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (at.is_some_and(|at| at.elapsed() < STALE)
            && frame_us > 0
            && frame_us >= self.began_us.load(Ordering::Relaxed))
        .then(|| self.hold.load(Ordering::Relaxed))
    }

    /// Check and publish under the same permission lock used by the listener.
    /// The callback may only make bounded local publication/state changes;
    /// never run inference, network I/O or camera operations inside it.
    pub fn with_frame_hold<T>(
        &self,
        captured: Option<u64>,
        frame_us: u64,
        publish: impl FnOnce(Option<u64>, bool) -> T,
    ) -> T {
        self.with_frame_authority(captured, frame_us, |current, valid, _| {
            publish(current, valid)
        })
    }

    /// A conservative cross-process monotonic deadline from the original
    /// receipt. Sample monotonic time before elapsed age, never after it.
    /// No listener I/O and no renewal of receipt age while waiting for a lock.
    pub fn with_frame_authority<T>(
        &self,
        captured: Option<u64>,
        frame_us: u64,
        publish: impl FnOnce(Option<u64>, bool, Option<u64>) -> T,
    ) -> T {
        let at = self
            .at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let current = at
            .is_some_and(|at| at.elapsed() < STALE)
            .then(|| self.hold.load(Ordering::Relaxed));
        let valid = current.is_some()
            && frame_us > 0
            && captured == current
            && frame_us >= self.began_us.load(Ordering::Relaxed);
        let now_us = lamp_ipc::monotonic_us();
        let until = at.and_then(|at| permission_deadline(now_us, at.elapsed()));
        publish(current, valid && until.is_some(), until)
    }

    /// Publish one completed observation only while its captured frame is
    /// still authorized. Observe hold changes first, so a renewed hold cannot
    /// pair fresh bus evidence with identity left over from its predecessor.
    /// Both callbacks have the same bounded-local-work restriction as
    /// `with_frame_hold`; detector and mesh work must finish before this call.
    pub fn publish_observation<T>(
        &self,
        captured: Option<u64>,
        frame_us: u64,
        observe_hold: impl FnOnce(Option<u64>, bool) -> bool,
        publish: impl FnOnce() -> T,
    ) -> Option<T> {
        self.with_frame_hold(captured, frame_us, |current, valid| {
            let observed = observe_hold(current, valid);
            (valid && observed).then(publish)
        })
    }

    /// A camera that has just opened, kept only if the permission still
    /// holds: an open can take longer than a permission lasts, and one taken
    /// back meanwhile closes the camera before any frame is read.
    pub fn opened<P>(&self, port: P, close: impl FnOnce(P)) -> Option<P> {
        if self.holds() {
            Some(port)
        } else {
            close(port);
            None
        }
    }
}

#[derive(Clone, Copy)]
struct Receipt {
    at: Instant,
    us: u64,
}

/// Complete the private handoff after the timestamp update, with no `at`
/// guard held during shutdown/pair ownership work. One logical rollover only.
fn update_disclosure_permission(
    authority: &crate::camera_authority::Authority,
    at: &Mutex<Option<Instant>>,
    hold: &AtomicU64,
    began_us: &AtomicU64,
    current: u64,
    began: u64,
    receipt: Receipt,
) {
    let Receipt {
        at: heard_at,
        us: heard_us,
    } = receipt;
    let Some(until) = original_permission_deadline(heard_at, heard_us) else {
        let _ = authority.close();
        return;
    };
    if authority.permit(current, began, until) != crate::camera_authority::PermitUpdate::NeedNewHold
    {
        return;
    }
    // A worker expiry may have retired private authority after `at` was
    // renewed. Do not poison it or mint a second pair for the same old token.
    let mut at = at.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if *at != Some(heard_at)
        || hold.load(Ordering::Relaxed) != current
        || original_permission_deadline(heard_at, heard_us).is_none()
        || current.checked_add(1).is_none()
    {
        return;
    }
    // Lease age stays on ORIGINAL receipt. Capture floor is independently
    // AFTER completed old shutdown/activation: a queued pre-roll V4L2 frame
    // must not pass simply because it was dequeued under the new hold.
    let capture_floor = lamp_ipc::monotonic_us();
    *at = None;
    received_permission(&mut at, hold, began_us, true, heard_at, capture_floor);
    let next = hold.load(Ordering::Relaxed);
    let until = original_permission_deadline(heard_at, heard_us);
    drop(at);
    if let Some(until) = until {
        authority.permit(next, capture_floor, until);
    } else {
        let _ = authority.close();
    }
}
fn original_permission_deadline(heard_at: Instant, heard_us: u64) -> Option<u64> {
    let now = lamp_ipc::monotonic_us();
    if heard_us == 0 || heard_us > now {
        return None;
    }
    let receipt_limit = heard_us.checked_add(u64::try_from(STALE.as_micros()).ok()?)?;
    Some(permission_deadline(now, heard_at.elapsed())?.min(receipt_limit))
}

/// Flooring remaining microseconds makes the external deadline conservative.
fn permission_deadline(now_us: u64, age: Duration) -> Option<u64> {
    let remaining = STALE.checked_sub(age)?;
    let remaining = u64::try_from(remaining.as_micros()).ok()?;
    (now_us > 0 && remaining > 0).then_some(())?;
    now_us.checked_add(remaining)
}

/// The listener's state update, also used by deterministic boundary tests.
/// All callers hold the permission timestamp lock while changing this state.
fn received_permission(
    at: &mut Option<Instant>,
    hold: &AtomicU64,
    began_us: &AtomicU64,
    allowed: bool,
    now: Instant,
    now_us: u64,
) {
    if !allowed || at.is_none_or(|at| at.elapsed() >= STALE) {
        hold.fetch_add(1, Ordering::Relaxed);
        began_us.store(now_us, Ordering::Relaxed);
    }
    *at = allowed.then_some(now);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How long the camera takes to open in these tests, unless said.
    const OPENING: Duration = Duration::from_millis(100);

    #[test]
    fn permission_deadline_keeps_receipt_age_and_refuses_equality_and_overflow() {
        assert_eq!(
            permission_deadline(2_000_000, Duration::from_millis(1400)),
            Some(2_100_000)
        );
        assert_eq!(permission_deadline(2_000_000, STALE), None);
        assert_eq!(
            permission_deadline(2_000_000, STALE + Duration::from_nanos(1)),
            None
        );
        assert_eq!(permission_deadline(u64::MAX, Duration::ZERO), None);
        assert_eq!(permission_deadline(0, Duration::ZERO), None);
    }

    fn permitted() -> (Permit, UnixDatagram) {
        let (voice, eyes) = UnixDatagram::pair().unwrap();
        let permit = Permit::listen(eyes).unwrap();
        voice.send(br#"{"camera":true}"#).unwrap();
        let waited = Instant::now();
        while !permit.holds() {
            assert!(waited.elapsed() < Duration::from_secs(1), "never heard");
            std::thread::sleep(Duration::from_millis(1));
        }
        (permit, voice)
    }

    /// A photo yes waits within one hold (#2856): a renewal keeps it; a
    /// close, though the permission comes again, or one run out, is another.
    #[test]
    fn each_close_and_each_permission_after_one_ran_out_is_another_hold() {
        let (permit, voice) = permitted();
        let first = permit.held().unwrap();
        let heard = |permit: &Permit, wanted: bool| {
            let waited = Instant::now();
            while permit.held().is_some() != wanted {
                assert!(waited.elapsed() < Duration::from_secs(1), "never heard");
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        voice.send(br#"{"camera":true}"#).unwrap();
        std::thread::sleep(OPENING);
        assert_eq!(permit.held(), Some(first));
        voice.send(br#"{"camera":false}"#).unwrap();
        heard(&permit, false);
        voice.send(br#"{"camera":true}"#).unwrap();
        heard(&permit, true);
        let second = permit.held().unwrap();
        assert_ne!(second, first);
        *permit.at.lock().unwrap() = Instant::now().checked_sub(Duration::from_millis(1_600));
        assert_eq!(permit.held(), None);
        voice.send(br#"{"camera":true}"#).unwrap();
        heard(&permit, true);
        assert_ne!(permit.held(), Some(second));
    }

    #[test]
    fn expired_and_regranted_hold_discards_delayed_identity_until_a_fresh_frame() {
        use crate::who::{DisclosureHold, Held, KnownSpeakerPublisher};
        const BOOT: &str = "0123456789abcdef";
        for explicit_close in [false, true] {
            let root = std::env::temp_dir().join(format!(
                "eyes-permit-disclosure-{}-{explicit_close}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir(&root).unwrap();
            let known = root.join("known");
            let source = KnownSpeakerPublisher::new(known.clone());
            let permit = Permit::default();
            let receive = |allowed, now_us| {
                let mut at = permit.at.lock().unwrap();
                received_permission(
                    &mut at,
                    &permit.hold,
                    &permit.began_us,
                    allowed,
                    Instant::now(),
                    now_us,
                );
            };
            receive(true, 1_000_000);
            let captured = permit.frame_hold(1_100_000);
            assert!(captured.is_some());
            let mut boundary = DisclosureHold::default();
            let mut visual = None;
            let mut held = Held::new(true);
            assert!(
                permit.with_frame_hold(captured, 1_100_000, |current, valid| {
                    assert!(
                        permit.at.try_lock().is_err(),
                        "the listener cannot change the hold during publication"
                    );
                    assert!(boundary.frame(
                        valid,
                        current,
                        Some(&source),
                        Some(&known),
                        &mut visual,
                        &mut held
                    ));
                    source.seen("Alpha", None, BOOT, "f1");
                    crate::who::keep_known_legacy(&known, Some("Alpha"), None, Some("f1"));
                    true
                })
            );
            let read = || -> serde_json::Value {
                serde_json::from_slice(
                    &std::fs::read(crate::who::known_speaker_of(&known)).unwrap(),
                )
                .unwrap()
            };
            let initial = read();
            visual = Some(("Alpha".to_owned(), 1.0, Instant::now()));
            held.matched(Instant::now(), Some("f1".into()));

            // Recognition's result is ready only after a controlled permission
            // transition. No wall-clock sleep or camera/model timing is assumed.
            if explicit_close {
                receive(false, 2_600_000);
            } else {
                *permit.at.lock().unwrap() =
                    Instant::now().checked_sub(STALE + Duration::from_millis(1));
            }
            assert!(!permit.holds());
            receive(true, 2_700_000);
            assert!(permit.holds(), "both publication-time booleans can be true");
            assert_ne!(permit.held(), captured);
            let mut published_old_result = false;
            permit.with_frame_hold(captured, 1_100_000, |current, valid| {
                if boundary.frame(
                    valid,
                    current,
                    Some(&source),
                    Some(&known),
                    &mut visual,
                    &mut held,
                ) {
                    published_old_result = true;
                    source.seen("Alpha", None, BOOT, "f1");
                }
            });
            assert!(
                !published_old_result,
                "pre-loss inference must not publish after regrant"
            );
            let revoked = read();
            assert_eq!(revoked["state"], "revoked");
            assert!(
                revoked["generation"].as_u64().unwrap() > initial["generation"].as_u64().unwrap()
            );
            assert!(revoked.get("name").is_none());
            assert!(visual.is_none() && !held.keeps(Instant::now()));
            assert!(!known.exists() && !crate::who::known_face_of(&known).exists());
            // Reading old queued bytes after the regrant must not stamp them
            // with its new token merely because the current boolean is true.
            assert_eq!(permit.frame_hold(1_100_000), None);
            let fresh = permit.frame_hold(2_800_000);
            assert_eq!(fresh, permit.held());
            let mut published_fresh = false;
            permit.with_frame_hold(fresh, 2_800_000, |current, valid| {
                assert!(boundary.frame(
                    valid,
                    current,
                    Some(&source),
                    Some(&known),
                    &mut visual,
                    &mut held
                ));
                assert!(
                    source.candidate(true, Some(BOOT), Some("f1")),
                    "same track must be recognized in the new hold"
                );
                source.seen("Beta", None, BOOT, "f1");
                crate::who::keep_known_legacy(&known, Some("Beta"), None, Some("f1"));
                published_fresh = true;
            });
            assert!(published_fresh);
            let fresh_seen = read();
            assert_eq!(fresh_seen["state"], "seen");
            assert_eq!(fresh_seen["name"], "Beta");
            assert_eq!(fresh_seen["generation"], revoked["generation"]);
            drop(source);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn mesh_finished_after_permission_loss_cannot_publish_a_second_observation() {
        use crate::who::{DisclosureHold, Held, KnownSpeakerPublisher};
        const BOOT: &str = "0123456789abcdef";
        for regranted_before_completion in [false, true] {
            let root = std::env::temp_dir().join(format!(
                "eyes-mesh-permit-{}-{regranted_before_completion}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir(&root).unwrap();
            let bus = lamp_bus::Bus::new(root.join("bus")).unwrap();
            let mut out = crate::bus_out::BusOut::on(&bus).unwrap();
            let known = root.join("known");
            let source = KnownSpeakerPublisher::new(known.clone());
            let read_source = || -> serde_json::Value {
                serde_json::from_slice(
                    &std::fs::read(crate::who::known_speaker_of(&known)).unwrap(),
                )
                .unwrap()
            };
            let permit = Permit::default();
            let receive = |now_us| {
                let mut at = permit.at.lock().unwrap();
                received_permission(
                    &mut at,
                    &permit.hold,
                    &permit.began_us,
                    true,
                    Instant::now(),
                    now_us,
                );
            };
            receive(1_000_000);
            let captured = permit.frame_hold(1_100_000);
            let mut boundary = DisclosureHold::default();
            let mut visual = None;
            let mut held = Held::new(true);
            let face = crate::faces::Face {
                score: 0.93,
                x: 240.0,
                y: 120.0,
                width: 160.0,
                height: 200.0,
                marks: [
                    (288.0, 200.0),
                    (352.0, 200.0),
                    (320.0, 240.0),
                    (288.0, 280.0),
                    (352.0, 280.0),
                ],
            };
            let mut publish = |captured, frame_us, now_us| {
                permit.publish_observation(
                    captured,
                    frame_us,
                    |current, valid| {
                        boundary.frame(
                            valid,
                            current,
                            Some(&source),
                            Some(&known),
                            &mut visual,
                            &mut held,
                        )
                    },
                    || {
                        assert!(permit.at.try_lock().is_err());
                        // The actual producer callback must see old identity
                        // withdrawn before it can publish a new observation.
                        assert!(source.revoked());
                        assert!(!known.exists() && !crate::who::known_face_of(&known).exists());
                        out.saw(
                            std::slice::from_ref(&face),
                            &[Some(0.08)],
                            (640, 480),
                            now_us,
                        );
                    },
                )
            };
            assert!(publish(captured, 1_100_000, 1_200_000).is_some());
            source.seen("Alpha", None, BOOT, "f1");
            crate::who::keep_known_legacy(&known, Some("Alpha"), None, Some("f1"));
            let initial = read_source();
            let prior = bus.latest(lamp_msgs::Faces::TOPIC).unwrap().unwrap();
            assert_eq!(prior.parse::<lamp_msgs::Faces>().unwrap().faces.len(), 1);

            // The detector check succeeded, but mesh completion is delayed
            // across expiry, optionally followed by a regrant. This controls
            // completion order, not a camera or model's measured run time.
            *permit.at.lock().unwrap() =
                Instant::now().checked_sub(STALE + Duration::from_millis(1));
            if regranted_before_completion {
                receive(2_700_000);
            }
            assert!(publish(captured, 1_100_000, 2_750_000).is_none());
            let refused = bus.latest(lamp_msgs::Faces::TOPIC).unwrap().unwrap();
            assert_eq!(refused.header.boot, prior.header.boot);
            assert_eq!(
                refused.header.seq, prior.header.seq,
                "no post-loss observation"
            );
            let revoked = read_source();
            assert_eq!(revoked["state"], "revoked");
            assert!(
                revoked["generation"].as_u64().unwrap() > initial["generation"].as_u64().unwrap()
            );
            assert!(revoked.get("name").is_none());
            assert!(!known.exists() && !crate::who::known_face_of(&known).exists());

            if !regranted_before_completion {
                receive(2_700_000);
            }
            let fresh = permit.frame_hold(2_800_000);
            assert!(fresh.is_some() && fresh != captured);
            assert!(publish(fresh, 2_800_000, 2_900_000).is_some());
            let renewed = bus.latest(lamp_msgs::Faces::TOPIC).unwrap().unwrap();
            assert_eq!(renewed.header.boot, prior.header.boot);
            assert_eq!(renewed.header.seq, prior.header.seq + 1);
            assert_eq!(renewed.parse::<lamp_msgs::Faces>().unwrap().faces.len(), 1);
            assert!(source.revoked());
            drop(out);
            drop(source);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn renewal_keeps_the_capture_hold_and_queued_old_frames_never_gain_a_new_one() {
        let permit = Permit::default();
        let receive = |now_us| {
            let mut at = permit.at.lock().unwrap();
            received_permission(
                &mut at,
                &permit.hold,
                &permit.began_us,
                true,
                Instant::now(),
                now_us,
            );
        };
        receive(1_000_000);
        let first = permit.frame_hold(1_100_000);
        assert!(first.is_some());
        receive(1_500_000);
        assert_eq!(
            permit.frame_hold(1_100_000),
            first,
            "renewal preserves capture authority and hold start"
        );
        assert!(permit.with_frame_hold(first, 1_100_000, |_, valid| valid));
        assert_eq!(
            permit.frame_hold(0),
            None,
            "no timestamp is not a fresh frame"
        );
        assert_eq!(permit.frame_hold(999_999), None);
        *permit.at.lock().unwrap() = Instant::now().checked_sub(STALE + Duration::from_millis(1));
        receive(2_700_000);
        let newer = permit.held();
        assert_ne!(newer, first);
        assert_eq!(permit.frame_hold(1_100_000), None);
        assert!(
            !permit.with_frame_hold(newer, 1_100_000, |_, valid| valid),
            "even a newly sampled token cannot bless old bytes"
        );
        assert!(permit.with_frame_hold(newer, 2_800_000, |_, valid| valid));
    }

    #[test]
    fn an_aged_received_permission_is_not_restamped_when_the_lock_becomes_free() {
        let permit = Permit::default();
        let arrived = Instant::now()
            .checked_sub(STALE + Duration::from_millis(1))
            .unwrap();
        {
            let mut at = permit.at.lock().unwrap();
            // Apply the real listener update with a receipt that already aged
            // past its lease while publication held the mutex. No sleep or
            // alternate production update path substitutes for arrival age.
            received_permission(
                &mut at,
                &permit.hold,
                &permit.began_us,
                true,
                arrived,
                1_000_000,
            );
        }
        assert!(!permit.holds());
        assert_eq!(permit.held(), None);
        assert_eq!(permit.frame_hold(1_100_000), None);
        let expired = permit.hold.load(Ordering::Relaxed);
        assert!(!permit.with_frame_hold(Some(expired), 1_100_000, |_, valid| valid));
        {
            let mut at = permit.at.lock().unwrap();
            received_permission(
                &mut at,
                &permit.hold,
                &permit.began_us,
                true,
                Instant::now(),
                3_000_000,
            );
        }
        assert!(permit.holds());
        assert_ne!(permit.held(), Some(expired));
        assert_eq!(permit.frame_hold(1_100_000), None);
        assert!(permit.frame_hold(3_100_000).is_some());
    }

    #[test]
    fn a_permission_taken_back_during_the_open_closes_the_camera() {
        let (permit, voice) = permitted();
        // The camera is opening; meanwhile the voice service says no.
        voice.send(br#"{"camera":false}"#).unwrap();
        std::thread::sleep(OPENING);
        let mut closed = false;
        assert_eq!(permit.opened("port", |_| closed = true), None);
        assert!(closed);
    }

    #[test]
    fn a_permission_run_out_during_the_open_closes_the_camera() {
        let (permit, _voice) = permitted();
        // An open of 1.6 s outlives a permission of 1.5 s.
        *permit.at.lock().unwrap() = Instant::now().checked_sub(Duration::from_millis(1_600));
        let mut closed = false;
        assert_eq!(permit.opened("port", |_| closed = true), None);
        assert!(closed);
    }

    #[test]
    fn a_renewal_sent_early_in_a_long_open_has_run_out_when_it_returns() {
        let (permit, voice) = permitted();
        // About 40 ms into an open of 1.6 s comes the last renewal: the voice
        // service then stops. When the open returns, that renewal is over
        // 1.5 s old, however late the eyes read it.
        std::thread::sleep(Duration::from_millis(40));
        voice.send(br#"{"camera":true}"#).unwrap();
        std::thread::sleep(Duration::from_millis(1_600));
        let mut closed = false;
        assert_eq!(permit.opened("port", |_| closed = true), None);
        assert!(closed);
    }

    #[test]
    fn a_permission_that_still_holds_keeps_the_camera_open() {
        let (permit, voice) = permitted();
        voice.send(br#"{"camera":true}"#).unwrap();
        std::thread::sleep(OPENING);
        let mut closed = false;
        assert_eq!(permit.opened("port", |_| closed = true), Some("port"));
        assert!(!closed);
    }

    #[test]
    fn what_is_not_a_permission_is_kept_for_the_loop_and_changes_nothing() {
        let (permit, voice) = permitted();
        voice.send(br#"{"look_full":7}"#).unwrap();
        voice.send(br#"{"camera":true}"#).unwrap();
        let waited = Instant::now();
        let mut heard = Vec::new();
        while heard.is_empty() {
            assert!(waited.elapsed() < Duration::from_secs(1), "never heard");
            std::thread::sleep(Duration::from_millis(1));
            heard = permit.heard();
        }
        assert_eq!(heard, [br#"{"look_full":7}"#.to_vec()]);
        assert!(permit.holds());
        assert!(permit.heard().is_empty());
        // At most HEARD are kept, the newest.
        for id in 0..HEARD + 3 {
            voice
                .send(format!(r#"{{"look_full":{id}}}"#).as_bytes())
                .unwrap();
        }
        voice.send(br#"{"camera":false}"#).unwrap();
        while permit.holds() {
            assert!(waited.elapsed() < Duration::from_secs(2), "never closed");
            std::thread::sleep(Duration::from_millis(1));
        }
        let heard = permit.heard();
        assert_eq!(heard.len(), HEARD);
        assert_eq!(
            heard[HEARD - 1],
            format!(r#"{{"look_full":{}}}"#, HEARD + 2).into_bytes()
        );
    }

    #[test]
    fn a_receipt_is_said_once_only_after_the_camera_is_shut_and_says_a_failed_stop() {
        let mut receipt = CloseReceipt::default();
        assert_eq!(receipt.due(true), None, "nothing asked");
        receipt.asked(7);
        // Asked while the camera is still open: nothing yet, the ask waits.
        assert_eq!(receipt.due(false), None);
        receipt.stopped::<()>(Ok(()));
        assert_eq!(
            receipt.due(true),
            Some(lamp_msgs::CameraClosed {
                close: 7,
                released: true
            })
        );
        assert_eq!(receipt.due(true), None, "said once");
        // A stop that faulted may leave the device held: said so.
        receipt.stopped(Err("STREAMOFF"));
        receipt.asked(8);
        receipt.asked(9);
        assert_eq!(
            receipt.due(true),
            Some(lamp_msgs::CameraClosed {
                close: 9,
                released: false
            })
        );
    }

    #[test]
    fn a_receipt_is_asked_in_one_exact_object_and_is_not_a_permission() {
        assert_eq!(receipt_asked(br#"{"close_receipt":7}"#), Some(7));
        assert_eq!(of(br#"{"close_receipt":7}"#), None);
        let largest = format!(r#"{{"close_receipt":{}}}"#, u64::MAX);
        assert!(largest.len() < 64, "fits the permit thread's read");
        assert_eq!(receipt_asked(largest.as_bytes()), Some(u64::MAX));
        for ignored in [
            &br#"{"close_receipt":-1}"#[..],
            br#"{"close_receipt":7,"camera":false}"#,
            br#"{"camera":false}"#,
            b"",
        ] {
            assert_eq!(receipt_asked(ignored), None);
        }
    }

    #[test]
    fn a_permission_is_one_exact_object_and_anything_else_says_nothing() {
        assert_eq!(of(br#"{"camera":true}"#), Some(true));
        assert_eq!(of(br#"{"camera":false}"#), Some(false));
        for ignored in [
            &br#"{"camera":1}"#[..],
            br#"{"camera":true,"save":true}"#,
            br#"{"mood":"listening","level":3}"#,
            b"true",
            b"",
        ] {
            assert_eq!(of(ignored), None);
        }
    }
}

#[cfg(test)]
mod camera_authority_tests;
