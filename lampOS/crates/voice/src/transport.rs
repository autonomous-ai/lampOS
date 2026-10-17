//! Two independent process channels. Control can progress while PCM is full.
#[cfg(test)]
use crate::scenario_clock::monotonic_us;
use crate::wire::{Envelope, ReceiveOrder, ReferenceControl, decode, encode};
#[cfg(not(test))]
use lamp_ipc::monotonic_us;
use lamp_ipc::{Endpoint, MAX_DATAGRAM_BYTES};
use lamp_turn_owner::BootId;
use serde::{Serialize, de::DeserializeOwned};
use std::{io, path::Path};

#[cfg(test)]
#[path = "transport_probe.rs"]
pub(crate) mod probe;

/// The longest a receiver may itself have been held up and still take what
/// queued for it meanwhile. On lamp-4ace the kernel re-tuning the SD/MMC
/// hosts held the voice's processes for about 0.3 s some eleven times a
/// day, and every worker then found its waiting messages older than the
/// channel's 100 ms and ended the whole voice (#784). Messages late through
/// the receiver's own stall are not stale: they come in order from the same
/// boot, and each still carries its own deadlines.
const HELD_UP_US: u64 = 1_500_000;

pub struct Channel {
    /// The socket's own name, said with a refused message.
    name: String,
    endpoint: Endpoint,
    sender: BootId,
    send_sequence: u64,
    incoming: ReceiveOrder,
    max_age_us: u64,
    /// Set for a worker's control channel: its first message may have waited
    /// up to [`FIRST_CONTROL_MAX_AGE_US`] for the receiver to start reading.
    first_grace: bool,
    received: bool,
    /// When `receive` last looked, and how long it had not looked before a
    /// backlog it is still draining.
    last_look_us: Option<u64>,
    held_up_us: u64,
    /// The last message was taken late because its sender was held up.
    sender_held_up: bool,
    #[cfg(test)]
    probe: probe::Slot,
}

/// A worker's first control message is sent as soon as its peer is there and
/// waits until the receiver first reads, which at start may be after it has
/// started other workers. On lamp-4ace on 2026-10-15 a plain
/// `systemctl restart lamp-voice` ended twice on such a message, older than
/// the usual bound, and the lamp was deaf for 23 s. Later control messages,
/// and every data message (sound must not arrive late), keep the bound.
pub const FIRST_CONTROL_MAX_AGE_US: u64 = 1_000_000;

impl Channel {
    pub fn bind(path: &Path, sender: BootId, peer: BootId, max_age_us: u64) -> io::Result<Self> {
        if max_age_us == 0 || max_age_us > 1_000_000 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid channel freshness bound",
            ));
        }
        Ok(Self {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            endpoint: Endpoint::bind(path)?,
            sender,
            send_sequence: 0,
            incoming: ReceiveOrder::new(peer),
            max_age_us,
            first_grace: false,
            received: false,
            last_look_us: None,
            held_up_us: 0,
            sender_held_up: false,
            #[cfg(test)]
            probe: probe::Slot::new(sender, peer),
        })
    }

    /// Attach fixture observations before any send or receive attempt.
    #[cfg(test)]
    pub(crate) fn install_probe(&mut self) -> io::Result<probe::Probe> {
        self.probe.install(&self.name)
    }

    fn with_first_grace(mut self) -> Self {
        self.first_grace = true;
        self
    }
    pub fn connect(&mut self, path: &Path) -> io::Result<()> {
        self.endpoint.connect(path)
    }

    pub fn send<T: Serialize>(&mut self, payload: T) -> io::Result<()> {
        self.send_with(payload, monotonic_us)
    }

    /// [`Self::send`], stamping the message with `now` instead of the host's
    /// clock, read at the same place: for a test that steps time with
    /// [`Self::receive_with`] instead of sleeping.
    pub fn send_with<T: Serialize>(
        &mut self,
        payload: T,
        now: impl FnOnce() -> u64,
    ) -> io::Result<()> {
        #[cfg(test)]
        self.probe.attempt();
        let sequence = self
            .send_sequence
            .checked_add(1)
            .ok_or_else(|| io::Error::other("channel sequence exhausted"))?;
        let data = encode(&Envelope {
            boot: self.sender,
            sequence,
            sent_at_us: now(),
            payload,
        })?;
        self.endpoint.try_send(&data).map_err(|error| {
            if error.kind() == io::ErrorKind::WouldBlock {
                return error;
            }
            // Said with the socket's name, as a refused receive is. On
            // lamp-cb94 a bare "Connection refused (os error 111)" ended the
            // voice and could not say whose peer had gone (#1178): a send
            // to a peer that has closed its socket is refused.
            io::Error::new(error.kind(), format!("{}: {error}", self.name))
        })?;
        self.send_sequence = sequence;
        #[cfg(test)]
        self.probe
            .record(probe::Direction::Send, self.sender, sequence);
        Ok(())
    }

    pub fn receive<T: DeserializeOwned>(&mut self) -> io::Result<Option<T>> {
        self.receive_with(monotonic_us)
    }

    /// [`Self::receive`], reading `now` instead of the host's clock at both
    /// of its places (before the receive, for how long since the last look,
    /// and after it, for the message's age): for a test that steps time with
    /// [`Self::send_with`] instead of sleeping.
    pub fn receive_with<T: DeserializeOwned>(
        &mut self,
        mut now: impl FnMut() -> u64,
    ) -> io::Result<Option<T>> {
        #[cfg(test)]
        self.probe.attempt();
        let looked = now();
        if let Some(last) = self.last_look_us {
            let gap = looked.saturating_sub(last);
            if gap > self.max_age_us {
                self.held_up_us = self.held_up_us.max(gap.min(HELD_UP_US));
            }
        }
        self.last_look_us = Some(looked);
        let mut data = [0; MAX_DATAGRAM_BYTES];
        let count = match self.endpoint.try_recv(&mut data) {
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                // Drained: what comes from now on is fresh again.
                self.held_up_us = 0;
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let packet: Envelope<T> = decode(&data[..count])?;
        let max_age_us = if self.first_grace && !self.received {
            self.max_age_us.max(FIRST_CONTROL_MAX_AGE_US)
        } else {
            self.max_age_us
        };
        // Read the clock after the receive: a message sent since `looked`
        // would otherwise look sent in the future, and be refused.
        let now = now();
        let next = self.incoming.is_next(&packet);
        let on_time = self
            .incoming
            .accept(&packet, now, max_age_us + self.held_up_us);
        // A sender held up between stamping a message and sending it (#784,
        // 52e6 on 2026-10-15: one frame 423 ms old after a 400 ms stall)
        // delivers that one message late, in order, and fresh ones after
        // it. One such message is taken; a second late one in a row is a
        // backlog, refused as before.
        let late_but_next =
            || next && !self.sender_held_up && now.saturating_sub(packet.sent_at_us) < HELD_UP_US;
        match on_time {
            Ok(()) => self.sender_held_up = false,
            Err(_) if late_but_next() => {
                self.incoming
                    .accept(&packet, now, HELD_UP_US)
                    .map_err(|error| {
                        io::Error::new(error.kind(), format!("{}: {error}", self.name))
                    })?;
                self.sender_held_up = true;
            }
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("{}: {error}", self.name),
                ));
            }
        }
        self.received = true;
        #[cfg(test)]
        self.probe
            .record(probe::Direction::Receive, packet.boot, packet.sequence);
        Ok(Some(packet.payload))
    }
}

pub struct WorkerChannels {
    pub control: Channel,
    pub data: Channel,
}

impl WorkerChannels {
    pub fn bind(
        dir: &Path,
        role: &str,
        parent: BootId,
        worker: BootId,
        worker_side: bool,
    ) -> io::Result<Self> {
        if !matches!(
            role,
            "capture" | "speaker" | "privacy" | "provider" | "second" | "ring" | "probe" | "camera"
        ) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid worker role",
            ));
        }
        let side = if worker_side { "w" } else { "p" };
        let (sender, peer) = if worker_side {
            (worker, parent)
        } else {
            (parent, worker)
        };
        Ok(Self {
            control: Channel::bind(&dir.join(format!("{role}.c.{side}")), sender, peer, 100_000)?
                .with_first_grace(),
            data: Channel::bind(&dir.join(format!("{role}.d.{side}")), sender, peer, 100_000)?,
        })
    }
    /// The peer must already have bound both sockets; startup retries are bounded
    /// by the supervisor and must not attempt to reconnect an established side.
    pub fn connect(&mut self, dir: &Path, role: &str, worker_side: bool) -> io::Result<()> {
        let side = if worker_side { "p" } else { "w" };
        self.control
            .connect(&dir.join(format!("{role}.c.{side}")))?;
        self.data.connect(&dir.join(format!("{role}.d.{side}")))
    }
}

/// Direct private reference channels. The peer boot is supplied only over the
/// trusted controller connection; it is never learned from a render packet.
/// Connecting is a bounded startup state, never a blocking audio-loop wait.
pub struct ReferenceChannels {
    pub control: Channel,
    pub data: Channel,
    peer_control: std::path::PathBuf,
    peer_data: std::path::PathBuf,
    control_connected: bool,
    connected: bool,
    deadline_us: u64,
}

impl ReferenceChannels {
    pub fn bind(dir: &Path, role: &str, own: BootId, peer: BootId) -> io::Result<Self> {
        let other = match role {
            "speaker" => "capture",
            "capture" => "speaker",
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid reference role",
                ));
            }
        };
        Ok(Self {
            control: Channel::bind(&dir.join(format!("ref.c.{role}")), own, peer, 100_000)?,
            data: Channel::bind(&dir.join(format!("ref.d.{role}")), own, peer, 100_000)?,
            peer_control: dir.join(format!("ref.c.{other}")),
            peer_data: dir.join(format!("ref.d.{other}")),
            control_connected: false,
            connected: false,
            deadline_us: monotonic_us()
                .checked_add(3_000_000)
                .ok_or_else(|| io::Error::other("reference deadline overflow"))?,
        })
    }

    /// Optional terminal notice, only after successful local PCM stop and reset.
    /// The independently stopped capture peer may already have closed its socket.
    /// Make at most one nonblocking send on an established control connection;
    /// never connect, retry, wait, or infer a peer acknowledgement here. Delivery
    /// failure cannot turn the completed local shutdown into an audio fault.
    /// Running reference/control/data traffic must still use the strict channels.
    pub fn notify_stopped_after_local_close(&mut self, playback_epoch: u64) {
        if self.control_connected {
            let _ = self.control.send(ReferenceControl::Stopped {
                playback_epoch,
                observed_at_us: monotonic_us(),
            });
        }
    }

    pub fn connect_step(&mut self) -> io::Result<bool> {
        if self.connected {
            return Ok(true);
        }
        if monotonic_us() >= self.deadline_us {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "direct reference connection timed out",
            ));
        }
        if !self.control_connected {
            match self.control.connect(&self.peer_control) {
                Ok(()) => self.control_connected = true,
                // Not there yet, or there and not yet private: try again.
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::WouldBlock
                    ) =>
                {
                    return Ok(false);
                }
                Err(error) => return Err(error),
            }
        }
        match self.data.connect(&self.peer_data) {
            Ok(()) => {
                self.connected = true;
                Ok(true)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::WouldBlock
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{SessionDirectory, new_boot};
    use std::{thread, time::Duration};

    fn pair(dir: &Path) -> (Channel, Channel) {
        let (a, b) = (new_boot().unwrap(), new_boot().unwrap());
        let mut sender = Channel::bind(&dir.join("s"), a, b, 100_000).unwrap();
        let mut receiver = Channel::bind(&dir.join("r"), b, a, 100_000).unwrap();
        sender.connect(&dir.join("r")).unwrap();
        receiver.connect(&dir.join("s")).unwrap();
        (sender, receiver)
    }

    /// A receiver held up for 400 ms takes the backlog that queued meanwhile;
    /// held up past `HELD_UP_US`, it is refused as before.
    #[test]
    fn a_receiver_held_up_takes_what_queued_meanwhile_within_a_bound() {
        let dir = SessionDirectory::create().unwrap();
        let (mut sender, mut receiver) = pair(&dir.path);
        assert_eq!(receiver.receive::<u32>().unwrap(), None);
        sender.send(1u32).unwrap();
        sender.send(2u32).unwrap();
        thread::sleep(Duration::from_millis(400));
        assert_eq!(receiver.receive::<u32>().unwrap(), Some(1));
        assert_eq!(receiver.receive::<u32>().unwrap(), Some(2));
        assert_eq!(receiver.receive::<u32>().unwrap(), None);

        sender.send(3u32).unwrap();
        thread::sleep(Duration::from_millis(1_700));
        assert!(receiver.receive::<u32>().is_err());
    }

    /// Only the next message in order may come late from a sender held up
    /// between stamping and sending it: not one after a gap in the order, nor
    /// one from another boot, nor the first.
    #[test]
    fn only_the_next_message_in_order_may_come_late_from_a_held_up_sender() {
        let boot = new_boot().unwrap();
        let at = monotonic_us();
        let message = |sequence| Envelope {
            boot,
            sequence,
            sent_at_us: at,
            payload: (),
        };
        let mut order = ReceiveOrder::new(boot);
        assert!(!order.is_next(&message(1)), "never the first");
        order.accept(&message(1), at, 100_000).unwrap();
        assert!(order.is_next(&message(2)));
        assert!(!order.is_next(&message(3)));
        let other = Envelope {
            boot: new_boot().unwrap(),
            ..message(2)
        };
        assert!(!order.is_next(&other));
    }

    /// #1178, lamp-cb94: a send to a peer that has closed its socket (a
    /// worker that ended) is refused, and the error says which socket, so
    /// the journal says whose peer had gone. A full queue is still a plain
    /// `WouldBlock`, for the callers that wait on it.
    #[test]
    fn a_send_to_a_peer_that_has_gone_is_refused_with_the_sockets_name() {
        let dir = SessionDirectory::create().unwrap();
        let (mut sender, receiver) = pair(&dir.path);
        sender.send(1u32).unwrap();
        drop(receiver);
        let error = sender.send(2u32).unwrap_err();
        assert_ne!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(error.to_string().starts_with("s: "), "{error}");
        #[cfg(target_os = "linux")]
        assert_eq!(error.kind(), io::ErrorKind::ConnectionRefused, "{error}");
        let dir = SessionDirectory::create().unwrap();
        let (mut sender, _receiver) = pair(&dir.path);
        let full = (0..100_000)
            .find_map(|n: u32| sender.send(n).err())
            .expect("the queue fills");
        assert_eq!(full.kind(), io::ErrorKind::WouldBlock);
        assert!(!full.to_string().starts_with("s: "), "{full}");
    }

    /// #1178: on Linux a send refused because its peer has gone also drops
    /// what that peer had sent and was not yet read. So a worker's last word,
    /// its fault, can be lost to the controller's next send, and the voice
    /// then ends on the refusal and not on the fault, as lamp-cb94's did.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_refused_send_drops_what_the_gone_peer_had_sent() {
        let dir = SessionDirectory::create().unwrap();
        let (mut sender, mut receiver) = pair(&dir.path);
        receiver.send(7u32).unwrap();
        drop(receiver);
        let refused = sender.send(1u32).unwrap_err();
        assert_eq!(
            refused.kind(),
            io::ErrorKind::ConnectionRefused,
            "{refused}"
        );
        assert_eq!(sender.receive::<u32>().unwrap(), None);
    }
}
