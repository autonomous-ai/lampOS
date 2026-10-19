//! Whether the camera may be open: `{"camera":true}` on the permit socket
//! at least every [`STALE`], and `{"camera":false}` closes it at once.
//! Anything else on the socket (a full look's ask, `--full-look`; a close
//! receipt's ask, [`receipt_asked`]) is kept for the eyes' loop to read
//! ([`Permit::heard`]).

use std::os::unix::net::UnixDatagram;
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
}

/// Datagrams other than a permission kept for the loop at most: an older
/// one is dropped when another comes.
pub const HEARD: usize = 8;

impl Permit {
    /// Hears the socket from now on. If the socket fails, nothing renews the
    /// permission and the camera closes once it has run out.
    pub fn listen(socket: UnixDatagram) -> std::io::Result<Self> {
        socket.set_nonblocking(false)?;
        let permit = Permit::default();
        let at = Arc::clone(&permit.at);
        let others = Arc::clone(&permit.others);
        std::thread::Builder::new()
            .name("permit".into())
            .spawn(move || {
                let mut datagram = [0u8; 64];
                while let Ok(length) = socket.recv(&mut datagram) {
                    let heard = match of(&datagram[..length]) {
                        Some(true) => Some(Instant::now()),
                        Some(false) => None,
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
                    *at.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = heard;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// How long the camera takes to open in these tests, unless said.
    const OPENING: Duration = Duration::from_millis(100);

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
