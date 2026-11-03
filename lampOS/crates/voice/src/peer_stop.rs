//! A Stop that finds the other audio worker already gone (#4332).
//!
//! The coordinator sends Stop to every worker at once. On 06c1's cold boot
//! the capture worker was still in `prepare()`, opening the microphone, when
//! the speaker took its Stop and closed its end of the reference link.
//! Capture then told the speaker it was prepared, the send was refused
//! (`ref.c.capture: Connection refused`), capture ended with a fault, and
//! that failed cleanup turned a privacy freshness loss, which the voice
//! waits out, into an exit with 1.
//!
//! Two things keep that end clean, and only that one:
//! - [`stop_queued`]: after `prepare()` capture reads what the coordinator
//!   queued meanwhile, keeps it in order for its next tick, and sends
//!   nothing to the speaker when a Stop is among it.
//! - [`stopped_with_peer`]: a reference link refused by its peer counts as
//!   a clean end only when the coordinator's Stop is already queued for this
//!   worker. A refused link with no Stop queued is still the fault it was.
use crate::{transport::WorkerChannels, wire::Control};
use std::{collections::VecDeque, io};

/// The most a look for a queued Stop reads: snapshots come no faster than
/// one a capture block, so a worker held up for 1.5 s finds at most 150.
pub(crate) const STOP_LOOK_BOUND: usize = 256;

/// Reads what the coordinator has queued for this worker, at most up to
/// `bound` held, onto the back of `held` in the order it came, and says
/// whether a Stop is among what is held. The worker's next tick takes
/// `held` before its socket, so nothing is lost or reordered. An error
/// reading is returned as the tick's own drain would have returned it.
pub(crate) fn stop_queued(
    channels: &mut WorkerChannels,
    held: &mut VecDeque<Control>,
    bound: usize,
) -> io::Result<bool> {
    while held.len() < bound {
        let Some(control) = channels.control.receive_renewed(Control::is_renewal)? else {
            break;
        };
        held.push_back(control);
    }
    Ok(held.iter().any(|control| matches!(control, Control::Stop)))
}

/// Whether `error` is the reference link between capture and speaker
/// refused by the other side (ECONNREFUSED on Linux, on a send or a
/// receive): its peer has closed its socket. The coordinator's own channel
/// refused is not this.
pub(crate) fn refused_by_reference_peer(error: &(dyn std::error::Error + 'static)) -> bool {
    error.downcast_ref::<io::Error>().is_some_and(|error| {
        let said = error.to_string();
        error.kind() == io::ErrorKind::ConnectionRefused
            && (said.starts_with("ref.c.") || said.starts_with("ref.d."))
    })
}

/// Whether a worker that ended on `error` ended at the coordinator's Stop:
/// the reference link was refused because the other worker took the same
/// Stop first, and that Stop is queued for this one too, in `held` (what
/// the worker had read ahead) or on its socket. Then the end is clean. Any
/// other error, or a refused link with no Stop queued, is not.
pub(crate) fn stopped_with_peer(
    channels: &mut WorkerChannels,
    held: &mut VecDeque<Control>,
    error: &(dyn std::error::Error + 'static),
) -> bool {
    refused_by_reference_peer(error)
        && stop_queued(channels, held, STOP_LOOK_BOUND).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{SessionDirectory, new_boot};
    use lamp_turn_owner::{Controller, MonoTime};

    fn now() -> MonoTime {
        MonoTime::from_micros(lamp_ipc::monotonic_us())
    }

    /// The coordinator's side and the capture worker's side of the worker
    /// channels, connected.
    fn pair(directory: &SessionDirectory) -> (WorkerChannels, WorkerChannels, Controller) {
        let (parent, worker) = (new_boot().unwrap(), new_boot().unwrap());
        let mut coordinator =
            WorkerChannels::bind(&directory.path, "capture", parent, worker, false).unwrap();
        let mut capture =
            WorkerChannels::bind(&directory.path, "capture", parent, worker, true).unwrap();
        coordinator
            .connect(&directory.path, "capture", false)
            .unwrap();
        capture.connect(&directory.path, "capture", true).unwrap();
        (coordinator, capture, Controller::new(parent, now()))
    }

    fn refused(name: &str) -> io::Error {
        io::Error::new(
            io::ErrorKind::ConnectionRefused,
            format!("{name}: Connection refused (os error 111)"),
        )
    }

    /// 06c1's boot 1: the speaker had gone at the coordinator's Stop, which
    /// waits queued for capture. A clean end.
    #[test]
    fn a_refused_reference_link_with_the_stop_queued_is_a_clean_end() {
        let directory = SessionDirectory::create().unwrap();
        let (mut coordinator, mut capture, mut controller) = pair(&directory);
        let snapshot = controller.snapshot(now()).unwrap();
        coordinator
            .control
            .send(Control::Authority { snapshot })
            .unwrap();
        coordinator.control.send(Control::Stop).unwrap();
        assert!(stopped_with_peer(
            &mut capture,
            &mut VecDeque::new(),
            &refused("ref.c.capture")
        ));
    }

    /// The speaker gone with no Stop sent (it crashed, or was killed): the
    /// refused link is the fault it always was.
    #[test]
    fn a_refused_reference_link_without_a_stop_still_fails() {
        let directory = SessionDirectory::create().unwrap();
        let (mut coordinator, mut capture, mut controller) = pair(&directory);
        let snapshot = controller.snapshot(now()).unwrap();
        coordinator
            .control
            .send(Control::Authority { snapshot })
            .unwrap();
        assert!(!stopped_with_peer(
            &mut capture,
            &mut VecDeque::new(),
            &refused("ref.c.capture")
        ));
        assert!(!stopped_with_peer(
            &mut capture,
            &mut VecDeque::new(),
            &refused("ref.d.speaker")
        ));
    }

    /// A Stop capture already read ahead after `prepare()`, not yet taken,
    /// is the coordinator's Stop as much as one still on the socket.
    #[test]
    fn a_stop_already_read_ahead_counts() {
        let directory = SessionDirectory::create().unwrap();
        let (_coordinator, mut capture, _) = pair(&directory);
        let mut held = VecDeque::from([Control::StartCapture, Control::Stop]);
        assert!(stopped_with_peer(
            &mut capture,
            &mut held,
            &refused("ref.d.capture")
        ));
        let mut held = VecDeque::from([Control::StartCapture]);
        assert!(!stopped_with_peer(
            &mut capture,
            &mut held,
            &refused("ref.d.capture")
        ));
    }

    /// Only the reference link refused is the peer gone at a Stop: the
    /// coordinator's own channel refused, another error on the link, or an
    /// error that isn't I/O, still fail with the Stop queued.
    #[test]
    fn only_the_reference_link_refused_counts_at_a_stop() {
        let directory = SessionDirectory::create().unwrap();
        let (mut coordinator, mut capture, _) = pair(&directory);
        coordinator.control.send(Control::Stop).unwrap();
        assert!(!stopped_with_peer(
            &mut capture,
            &mut VecDeque::new(),
            &refused("capture.c.w")
        ));
        let reset = io::Error::new(io::ErrorKind::ConnectionReset, "ref.c.capture: reset");
        assert!(!stopped_with_peer(
            &mut capture,
            &mut VecDeque::new(),
            &reset
        ));
        let other = io::Error::other("ref.c.capture: Connection refused");
        assert!(!stopped_with_peer(
            &mut capture,
            &mut VecDeque::new(),
            &other
        ));
        // The Stop is still there to be read: none of those looked.
        assert!(stopped_with_peer(
            &mut capture,
            &mut VecDeque::new(),
            &refused("ref.d.capture")
        ));
        for name in ["ref.c.capture", "ref.d.capture", "ref.c.speaker"] {
            assert!(refused_by_reference_peer(&refused(name)), "{name}");
        }
    }

    /// What `prepare()` let queue is read in the order it came and kept for
    /// the next tick, Stop or not: a renewal before a Stop is still taken
    /// first, and without a Stop nothing is dropped.
    #[test]
    fn what_is_read_for_a_stop_is_held_in_order() {
        let directory = SessionDirectory::create().unwrap();
        let (mut coordinator, mut capture, mut controller) = pair(&directory);
        let snapshot = controller.snapshot(now()).unwrap();
        coordinator
            .control
            .send(Control::Authority { snapshot })
            .unwrap();
        coordinator.control.send(Control::StartCapture).unwrap();
        let mut held = VecDeque::new();
        assert!(!stop_queued(&mut capture, &mut held, STOP_LOOK_BOUND).unwrap());
        assert!(matches!(
            held.iter().collect::<Vec<_>>()[..],
            [Control::Authority { .. }, Control::StartCapture]
        ));
        coordinator.control.send(Control::Stop).unwrap();
        assert!(stop_queued(&mut capture, &mut held, STOP_LOOK_BOUND).unwrap());
        assert!(matches!(
            held.iter().collect::<Vec<_>>()[..],
            [
                Control::Authority { .. },
                Control::StartCapture,
                Control::Stop
            ]
        ));
        // Bounded: a full hold reads no further, and the rest waits queued.
        coordinator.control.send(Control::Stop).unwrap();
        let mut full = VecDeque::from([Control::StartCapture]);
        assert!(!stop_queued(&mut capture, &mut full, 1).unwrap());
        assert_eq!(full.len(), 1);
        assert!(stop_queued(&mut capture, &mut VecDeque::new(), STOP_LOOK_BOUND).unwrap());
    }
}
