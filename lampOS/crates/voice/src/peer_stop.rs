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
//!   a clean end only when an end is already queued for this worker: the
//!   coordinator's Stop, a denied authority (the slide closed, so the
//!   speaker ended on its own copy, #7224), or the speaker's own `Stopped`
//!   on the reference link. A refused link with none of those queued (the
//!   speaker crashed, or was killed) is still the fault it was.
use crate::{
    transport::{Channel, WorkerChannels},
    wire::{Control, ReferenceControl},
};
use lamp_turn_owner::Permission;
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

/// Whether `kind` is what the OS says of a socket whose peer has closed:
/// ECONNREFUSED on Linux (only that, byte for byte as before), and
/// ECONNRESET as well off Linux, where macOS answers a send to a closed peer
/// so (`os error 54`, seen in the sim-macos job on #7137).
fn closed_peer_kind(kind: io::ErrorKind) -> bool {
    #[cfg(target_os = "linux")]
    {
        kind == io::ErrorKind::ConnectionRefused
    }
    #[cfg(not(target_os = "linux"))]
    {
        matches!(
            kind,
            io::ErrorKind::ConnectionRefused | io::ErrorKind::ConnectionReset
        )
    }
}

/// Whether a receive on a worker's control socket that failed with `kind`, in the coordinator's
/// cleanup, is only the closed socket of a worker that has ended: off Linux, where macOS answers
/// a receive on a datagram socket whose peer has gone with ECONNRESET (`os error 54`, run 4 of the
/// Mac start, sim-macos on #7407). The worker's exit, read after the drain, is still the check.
/// On Linux this is never true: its cleanup is as it was.
pub(crate) fn control_receive_of_ended_worker(kind: io::ErrorKind) -> bool {
    #[cfg(target_os = "linux")]
    {
        let _ = kind;
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        closed_peer_kind(kind)
    }
}

/// Whether `error` is the reference link between capture and speaker
/// refused by the other side (ECONNREFUSED on Linux, ECONNRESET on macOS, on
/// a send or a receive): its peer has closed its socket. The coordinator's
/// own channel refused is not this.
pub(crate) fn refused_by_reference_peer(error: &(dyn std::error::Error + 'static)) -> bool {
    error.downcast_ref::<io::Error>().is_some_and(|error| {
        let said = error.to_string();
        closed_peer_kind(error.kind()) && (said.starts_with("ref.c.") || said.starts_with("ref.d."))
    })
}

/// Whether what is held ends this worker as the other one ended: a Stop, or
/// an authority that denies the microphone (the slide closed, #7224). An
/// authority whose permission is only unknown is not one.
fn end_held(held: &VecDeque<Control>) -> bool {
    held.iter().any(|control| match control {
        Control::Stop => true,
        Control::Authority { snapshot } => snapshot.microphone_permission() == Permission::Denied,
        _ => false,
    })
}

/// Whether the peer said it stopped before it closed the link: a `Stopped`
/// among at most [`STOP_LOOK_BOUND`] controls queued on `reference`. What is
/// read is dropped: the worker is ending. A read that fails finds nothing.
fn peer_said_stopped(reference: &mut Channel) -> bool {
    for _ in 0..STOP_LOOK_BOUND {
        match reference.receive::<ReferenceControl>() {
            Ok(Some(ReferenceControl::Stopped { .. })) => return true,
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => return false,
        }
    }
    false
}

/// Whether a worker that ended on `error` ended as its peer did: the
/// reference link was refused because the other worker ended first, and
/// what ended it is queued for this one too: the coordinator's Stop or a
/// denied authority, in `held` (what the worker had read ahead) or on its
/// socket, or the peer's own `Stopped` on `reference`, this worker's side of
/// the reference link's control (#7224). Then the end is clean. Any other
/// error, or a refused link with none of those queued, is not.
pub(crate) fn stopped_with_peer(
    channels: &mut WorkerChannels,
    held: &mut VecDeque<Control>,
    reference: Option<&mut Channel>,
    error: &(dyn std::error::Error + 'static),
) -> bool {
    refused_by_reference_peer(error)
        && match stop_queued(channels, held, STOP_LOOK_BOUND) {
            Ok(true) => true,
            Ok(false) => end_held(held) || reference.is_some_and(peer_said_stopped),
            Err(_) => false,
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cleanup's excuse for a control receive from an ended worker: only the closed-peer kind
    /// the OS gives, and none at all on Linux, so Linux's cleanup is as it was.
    #[test]
    fn a_control_receive_from_an_ended_worker_is_excused_off_linux_only() {
        let reset = io::ErrorKind::ConnectionReset;
        assert_eq!(
            control_receive_of_ended_worker(reset),
            !cfg!(target_os = "linux")
        );
        assert_eq!(
            control_receive_of_ended_worker(io::ErrorKind::ConnectionRefused),
            !cfg!(target_os = "linux")
        );
        for other in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::InvalidData,
            io::ErrorKind::TimedOut,
            io::ErrorKind::Other,
        ] {
            assert!(!control_receive_of_ended_worker(other), "{other:?}");
        }
    }

    use crate::process::{SessionDirectory, new_boot};
    use crate::transport::ReferenceChannels;
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
            None,
            &refused("ref.c.capture")
        ));
    }

    /// lamp-c0a4, 16 Nov 14:48 (#6672 leg 2): the mute's denied authority
    /// reached both workers, the speaker ended on its own copy
    /// (PrivacyClosed) and closed the link, and capture's reply to it was
    /// refused. The denied authority queued is the same clean end as a Stop.
    #[test]
    fn a_refused_reference_link_with_a_denied_authority_queued_is_a_clean_end() {
        let directory = SessionDirectory::create().unwrap();
        let (mut coordinator, mut capture, mut controller) = pair(&directory);
        controller
            .set_microphone_permission(now(), lamp_turn_owner::Permission::Denied)
            .unwrap();
        let snapshot = controller.snapshot(now()).unwrap();
        coordinator
            .control
            .send(Control::Authority { snapshot })
            .unwrap();
        assert!(stopped_with_peer(
            &mut capture,
            &mut VecDeque::new(),
            None,
            &refused("ref.c.capture")
        ));
    }

    /// Capture's side and the speaker's side of the reference link,
    /// connected both ways.
    fn reference_pair(directory: &SessionDirectory) -> (ReferenceChannels, ReferenceChannels) {
        let (capture_boot, speaker_boot) = (new_boot().unwrap(), new_boot().unwrap());
        let mut capture =
            ReferenceChannels::bind(&directory.path, "capture", capture_boot, speaker_boot)
                .unwrap();
        let mut speaker =
            ReferenceChannels::bind(&directory.path, "speaker", speaker_boot, capture_boot)
                .unwrap();
        assert!(capture.connect_step().unwrap());
        assert!(speaker.connect_step().unwrap());
        (capture, speaker)
    }

    /// #7224, the speaker's side: it ended on its own copy of the closing
    /// authority, said Stopped on the reference link and closed it, before
    /// that authority reached capture. Its Stopped queued is a clean end.
    #[test]
    fn a_refused_reference_link_with_the_speakers_stopped_queued_is_a_clean_end() {
        let directory = SessionDirectory::create().unwrap();
        let (mut coordinator, mut capture, mut controller) = pair(&directory);
        let snapshot = controller.snapshot(now()).unwrap();
        coordinator
            .control
            .send(Control::Authority { snapshot })
            .unwrap();
        let (mut link, mut speaker) = reference_pair(&directory);
        speaker
            .control
            .send(ReferenceControl::Stopped {
                playback_epoch: 1,
                observed_at_us: lamp_ipc::monotonic_us(),
            })
            .unwrap();
        drop(speaker);
        assert!(stopped_with_peer(
            &mut capture,
            &mut VecDeque::new(),
            Some(&mut link.control),
            &refused("ref.c.capture")
        ));
    }

    /// The speaker gone with nothing said on the link and only an authority
    /// that grants or has not yet read the slide queued: still a fault, with
    /// the link looked at as without it.
    #[test]
    fn a_refused_reference_link_with_nothing_queued_still_fails() {
        let directory = SessionDirectory::create().unwrap();
        let (mut coordinator, mut capture, mut controller) = pair(&directory);
        let unknown = controller.snapshot(now()).unwrap();
        controller
            .set_microphone_permission(now(), lamp_turn_owner::Permission::Allowed)
            .unwrap();
        let allowed = controller.snapshot(now()).unwrap();
        for snapshot in [unknown, allowed] {
            coordinator
                .control
                .send(Control::Authority { snapshot })
                .unwrap();
        }
        let (mut link, speaker) = reference_pair(&directory);
        drop(speaker);
        assert!(!stopped_with_peer(
            &mut capture,
            &mut VecDeque::new(),
            Some(&mut link.control),
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
            None,
            &refused("ref.c.capture")
        ));
        assert!(!stopped_with_peer(
            &mut capture,
            &mut VecDeque::new(),
            None,
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
            None,
            &refused("ref.d.capture")
        ));
        let mut held = VecDeque::from([Control::StartCapture]);
        assert!(!stopped_with_peer(
            &mut capture,
            &mut held,
            None,
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
            None,
            &refused("capture.c.w")
        ));
        let other = io::Error::other("ref.c.capture: Connection refused");
        assert!(!stopped_with_peer(
            &mut capture,
            &mut VecDeque::new(),
            None,
            &other
        ));
        // The Stop is still there to be read: none of those looked.
        assert!(stopped_with_peer(
            &mut capture,
            &mut VecDeque::new(),
            None,
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
    /// A reset on the reference link is a closed peer only where the OS says
    /// so: macOS answers a send to a closed peer with ECONNRESET, so there it
    /// is a clean end at a queued Stop as a refusal is on Linux; on Linux it
    /// stays a fault. A reset on any other channel is a fault on both.
    #[test]
    fn a_reset_reference_link_is_a_closed_peer_only_off_linux() {
        let directory = SessionDirectory::create().unwrap();
        let (mut coordinator, mut capture, _) = pair(&directory);
        coordinator.control.send(Control::Stop).unwrap();
        let other_channel = io::Error::new(io::ErrorKind::ConnectionReset, "capture.c.w: reset");
        assert!(!stopped_with_peer(
            &mut capture,
            &mut VecDeque::new(),
            None,
            &other_channel
        ));
        let reset = io::Error::new(io::ErrorKind::ConnectionReset, "ref.c.capture: reset");
        assert_eq!(
            stopped_with_peer(&mut capture, &mut VecDeque::new(), None, &reset),
            cfg!(not(target_os = "linux")),
        );
    }
}
