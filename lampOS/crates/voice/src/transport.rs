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
    /// An in-order message from the peer's boot is taken however late
    /// ([`Self::taking_late`]); the run of late ones taken, and the oldest.
    take_late: bool,
    /// Until a message is taken, the run of stale ones is dropped and said
    /// ([`Self::dropping_stale_first`]): from the first one's stamp, the
    /// count, the oldest and the bound.
    drop_stale_first: bool,
    dropped_from_us: Option<u64>,
    dropped_run: u64,
    dropped_oldest_us: u64,
    dropped_bound_us: u64,
    late_run: u64,
    late_oldest_us: u64,
    /// Stale renewals passed over in the run now going on
    /// ([`Self::receive_renewed`]), and the oldest.
    passed_run: u64,
    passed_oldest_us: u64,
    #[cfg(test)]
    probe: probe::Slot,
}

/// A worker's first control message is sent as soon as its peer is there and
/// waits until the receiver first reads, which at start may be after it has
/// started other workers. On lamp-4ace on 2026-10-15 a plain
/// `systemctl restart lamp-voice` ended twice on such a message, older than
/// the usual bound, and the lamp was deaf for 23 s. Later control messages,
/// and every data message (sound must not arrive late), keep the bound; the
/// microphone's stale frames at the start are dropped instead, while the
/// first of them is at most this old ([`Channel::dropping_stale_first`]).
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
            take_late: false,
            drop_stale_first: false,
            dropped_from_us: None,
            dropped_run: 0,
            dropped_oldest_us: 0,
            dropped_bound_us: 0,
            late_run: 0,
            late_oldest_us: 0,
            passed_run: 0,
            passed_oldest_us: 0,
            #[cfg(test)]
            probe: probe::Slot::new(sender, peer),
        })
    }

    /// The socket, so a test can see which processes hold it.
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn socket(&self) -> std::os::fd::BorrowedFd<'_> {
        std::os::fd::AsFd::as_fd(&self.endpoint)
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

    /// For a channel whose messages carry no authority of their own: each is
    /// checked against fresh authority when it is used. The provider's input
    /// (`provider.d.w`): every Start, Audio, End and the rest is submitted
    /// only under the newest snapshot from its control channel, with its
    /// privacy generation and its turn's owner. A message there that comes
    /// late, in order and from the peer's boot, is taken, and said: the
    /// worker reads it one at a time, and none while a command waits for
    /// room in Gemini's queue; each look was under the bound, and the backlog
    /// behind grew 0.2 ms past it and ended the voice (lamp-4ace, 19 Oct
    /// 05:08). A message from another
    /// boot, or out of order, is still refused.
    fn taking_late(mut self) -> Self {
        self.take_late = true;
        self
    }

    /// For the microphone's frames to the voice (`capture.d.p`): the run of
    /// frames at the start, in order from the peer's boot and stale by their
    /// age alone, is dropped up to the first fresh one, and said once.
    /// Capture sends a frame every 10 ms from when it is asked to, and they
    /// wait until the voice first reads; on lamp-0c89 at build 8545 the
    /// first waited 154 ms and ended the voice at boot (#4332). A frame is
    /// sound now or not at all, so they are dropped, never taken late. The
    /// run is bounded: once its first frame is over
    /// [`FIRST_CONTROL_MAX_AGE_US`] old, a stale one ends the voice. One from
    /// another boot, out of order or stamped after now ends it anywhere in
    /// the run, and once a frame is taken, later ones keep the bound.
    fn dropping_stale_first(mut self) -> Self {
        self.drop_stale_first = true;
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
        now: impl FnMut() -> u64,
    ) -> io::Result<Option<T>> {
        self.receive_renewed_with(now, |_: &T| false)
    }

    /// [`Self::receive`] for a worker's control channel, where `renews` says
    /// which messages are authority renewals (#1569 row 42, the first
    /// builder on #1722, 20 Oct 17:46). A renewal that is stale, but only by
    /// its age, with the next renewal in order queued right behind it, is
    /// passed over: that one carries the authority now, and at most it costs
    /// the gap a lapse costs. A stale renewal alone or followed by any other
    /// message, a stale message of any other kind, and reordered and
    /// wrong-boot ones still end the worker, since those carry state a skip
    /// would lose. A worker that starts slowly (the speaker opening its
    /// device before its first look) otherwise finds its third renewal
    /// stale, after the first message's grace and one late message, and
    /// ends.
    pub fn receive_renewed<T: DeserializeOwned>(
        &mut self,
        renews: impl Fn(&T) -> bool,
    ) -> io::Result<Option<T>> {
        self.receive_renewed_with(monotonic_us, renews)
    }

    /// [`Self::receive_renewed`] on the clock `now` reads, as
    /// [`Self::receive_with`].
    pub fn receive_renewed_with<T: DeserializeOwned>(
        &mut self,
        now: impl FnMut() -> u64,
        renews: impl Fn(&T) -> bool,
    ) -> io::Result<Option<T>> {
        self.receive_matching(now, renews, |_: &T| false)
    }

    fn receive_matching<T: DeserializeOwned>(
        &mut self,
        now: impl FnMut() -> u64,
        renews: impl Fn(&T) -> bool,
        initial: impl Fn(&T) -> bool,
    ) -> io::Result<Option<T>> {
        let received = self.take_next(now, &renews, &initial);
        if received.is_err() {
            // The runs worth reading end so (the reviewer on #1660).
            self.say_run("ended by an error");
            self.say_dropped("then ended by an error");
        }
        received
    }

    /// The message right behind `stale` in the socket, if it is the next
    /// renewal in order from the same boot ([`Self::receive_renewed`]). What
    /// is read and is not one is dropped: `stale` then ends the worker.
    fn renewal_behind<T: DeserializeOwned>(
        &mut self,
        stale: &Envelope<T>,
        renews: &impl Fn(&T) -> bool,
    ) -> io::Result<Option<Envelope<T>>> {
        let mut data = [0; MAX_DATAGRAM_BYTES];
        let count = match self.endpoint.try_recv(&mut data) {
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
            Err(error) => return Err(error),
        };
        let behind: Envelope<T> = decode(&data[..count])?;
        Ok((behind.boot == stale.boot
            && stale.sequence.checked_add(1) == Some(behind.sequence)
            && behind.sent_at_us >= stale.sent_at_us
            && renews(&behind.payload))
        .then_some(behind))
    }

    fn take_next<T: DeserializeOwned>(
        &mut self,
        mut now: impl FnMut() -> u64,
        renews: &impl Fn(&T) -> bool,
        initial: &impl Fn(&T) -> bool,
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
        let mut packet: Envelope<T> = decode(&data[..count])?;
        // Read the clock after the receive: a message sent since `looked`
        // would otherwise look sent in the future, and be refused.
        let mut at_us = now();
        loop {
            let first_grace = self.first_grace
                || (initial(&packet.payload) && self.incoming.is_in_order(&packet));
            let max_age_us = if first_grace && !self.received {
                self.max_age_us.max(FIRST_CONTROL_MAX_AGE_US)
            } else {
                self.max_age_us
            };
            // The run of stale frames at the start ([`Self::dropping_stale_first`]):
            // in order, same boot, none stamped after now, and only while the
            // run's first frame is at most 1 s old; beyond that, fatal.
            let bound_us = max_age_us + self.held_up_us;
            if self.drop_stale_first
                && self.incoming.is_in_order(&packet)
                && packet.sent_at_us <= at_us
                && at_us - packet.sent_at_us >= bound_us
            {
                let run_from_us = self.dropped_from_us.unwrap_or(packet.sent_at_us);
                let limit_us = if at_us - run_from_us <= FIRST_CONTROL_MAX_AGE_US {
                    u64::MAX
                } else {
                    bound_us
                };
                self.incoming
                    .accept(&packet, at_us, limit_us)
                    .map_err(|error| {
                        io::Error::new(error.kind(), format!("{}: {error}", self.name))
                    })?;
                self.dropped_from_us = Some(run_from_us);
                self.dropped_run += 1;
                self.dropped_oldest_us = self.dropped_oldest_us.max(at_us - packet.sent_at_us);
                self.dropped_bound_us = bound_us;
                let mut data = [0; MAX_DATAGRAM_BYTES];
                let count = match self.endpoint.try_recv(&mut data) {
                    Ok(count) => count,
                    // Nothing fresh yet: the next receive goes on with the run.
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(None);
                    }
                    Err(error) => return Err(error),
                };
                packet = decode(&data[..count])?;
                // Again after this receive, as above.
                at_us = now();
                continue;
            }
            match self.judge(&packet, at_us, max_age_us) {
                Ok(()) => break,
                // Stale by its age alone, a renewal, the next renewal behind.
                // One stamped after now is not stale: it is refused as ever,
                // its channel named (the lead's reviewer on #2293).
                Err(error)
                    if renews(&packet.payload)
                        && self.incoming.is_in_order(&packet)
                        && packet.sent_at_us <= at_us =>
                {
                    let Some(behind) = self.renewal_behind(&packet, renews)? else {
                        return Err(error);
                    };
                    self.incoming.accept(&packet, at_us, u64::MAX)?;
                    self.received = true;
                    self.passed_over(at_us.saturating_sub(packet.sent_at_us), max_age_us);
                    packet = behind;
                    // Again after this receive, for the same reason.
                    at_us = now();
                }
                Err(error) => return Err(error),
            }
        }
        self.received = true;
        if self.drop_stale_first {
            // Taken: the run is over, and later frames keep the bound.
            self.drop_stale_first = false;
            self.say_dropped("the next taken");
        }
        #[cfg(test)]
        self.probe
            .record(probe::Direction::Receive, packet.boot, packet.sequence);
        Ok(Some(packet.payload))
    }

    /// Takes `packet` in order if it is on time, or late as this channel
    /// allows; otherwise the error that ends the worker, its order unmoved.
    fn judge<T>(&mut self, packet: &Envelope<T>, now: u64, max_age_us: u64) -> io::Result<()> {
        let next = self.incoming.is_next(packet);
        let on_time = self
            .incoming
            .accept(packet, now, max_age_us + self.held_up_us);
        // A sender held up between stamping a message and sending it (#784,
        // 52e6 on 2026-10-15: one frame 423 ms old after a 400 ms stall)
        // delivers that one message late, in order, and fresh ones after
        // it. One such message is taken; a second late one in a row is a
        // backlog, refused as before.
        let late_but_next =
            || next && !self.sender_held_up && now.saturating_sub(packet.sent_at_us) < HELD_UP_US;
        match on_time {
            Ok(()) => {
                self.sender_held_up = false;
                self.say_run("on time again");
            }
            Err(_) if self.take_late && self.incoming.is_in_order(packet) => {
                self.incoming
                    .accept(packet, now, u64::MAX)
                    .map_err(|error| {
                        io::Error::new(error.kind(), format!("{}: {error}", self.name))
                    })?;
                self.taken_late(
                    now.saturating_sub(packet.sent_at_us),
                    max_age_us + self.held_up_us,
                );
            }
            Err(_) if late_but_next() => {
                self.incoming
                    .accept(packet, now, HELD_UP_US)
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
        Ok(())
    }

    /// A late message taken ([`Self::taking_late`]), against the bound it
    /// failed, the receiver's held-up time included: the first of a run is
    /// said, and the run when it ends ([`Self::say_run`]).
    fn taken_late(&mut self, age_us: u64, bound_us: u64) {
        self.late_run += 1;
        self.late_oldest_us = self.late_oldest_us.max(age_us);
        if self.late_run == 1 {
            say(format!(
                "lamp-voice: {}: a message {} us old (bound {bound_us} us) taken late, in order: the receiver was behind",
                self.name, age_us
            ));
        }
    }

    /// A stale renewal passed over ([`Self::receive_renewed`]): the first of
    /// a run is said, and the run when it ends ([`Self::say_run`]), so a long
    /// hold-up's backlog of renewals is two lines, not one each (the
    /// reviewer on #2293).
    fn passed_over(&mut self, age_us: u64, bound_us: u64) {
        self.passed_run += 1;
        self.passed_oldest_us = self.passed_oldest_us.max(age_us);
        if self.passed_run == 1 {
            say(format!(
                "lamp-voice: {}: a renewal {age_us} us old (bound {bound_us} us) passed over: the next renewal is behind it",
                self.name
            ));
        }
    }

    /// A run of late messages, said as it ends: `on time again`, `ended by
    /// an error` or `closed` with the channel, so a run that ends the
    /// worker or goes quiet still says its count and its oldest (the
    /// reviewer on #1660). A run of renewals passed over is said so too.
    fn say_run(&mut self, how: &str) {
        if self.passed_run > 0 {
            say(format!(
                "lamp-voice: {}: {how} after {} renewals passed over, the oldest {} us",
                self.name, self.passed_run, self.passed_oldest_us
            ));
            self.passed_run = 0;
            self.passed_oldest_us = 0;
        }
        if self.late_run > 0 {
            say(format!(
                "lamp-voice: {}: {how} after {} late messages, the oldest {} us",
                self.name, self.late_run, self.late_oldest_us
            ));
            self.late_run = 0;
            self.late_oldest_us = 0;
        }
    }

    /// The run of stale first frames dropped, said once as it ends
    /// ([`Self::dropping_stale_first`]): its count, the oldest and the bound.
    fn say_dropped(&mut self, how: &str) {
        if self.dropped_run > 0 {
            say(format!(
                "lamp-voice: {}: {} stale first messages dropped, the oldest {} us (bound {} us), {how}: they waited for the receiver's first look",
                self.name, self.dropped_run, self.dropped_oldest_us, self.dropped_bound_us
            ));
            self.dropped_run = 0;
            self.dropped_oldest_us = 0;
        }
    }

    /// Late messages taken in the run now going on.
    #[cfg(test)]
    pub(crate) fn late_run(&self) -> u64 {
        self.late_run
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        self.say_run("closed");
    }
}

/// A line for the journal; a test reads what its own thread said.
fn say(line: String) {
    #[cfg(test)]
    SAID.with(|said| said.borrow_mut().push(line.clone()));
    eprintln!("{line}");
}

#[cfg(test)]
thread_local! {
    static SAID: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The channels' lines this test thread said so far, taken.
#[cfg(test)]
pub(crate) fn said() -> Vec<String> {
    SAID.with(|said| std::mem::take(&mut *said.borrow_mut()))
}

/// The channel's socket, for a loop that waits on several with `poll`
/// instead of asking each in turn (`LAMP_CAPTURE_WAIT`, #705). Nothing is
/// read ahead of [`Channel::receive`], so a readable socket is all there is.
impl std::os::fd::AsFd for Channel {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        std::os::fd::AsFd::as_fd(&self.endpoint)
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
        let data = Channel::bind(&dir.join(format!("{role}.d.{side}")), sender, peer, 100_000)?;
        Ok(Self {
            control: Channel::bind(&dir.join(format!("{role}.c.{side}")), sender, peer, 100_000)?
                .with_first_grace(),
            // The provider's input is checked against fresh authority when it
            // is used, so a late one is taken (lamp-4ace, 19 Oct 05:08).
            data: if role == "provider" && worker_side {
                data.taking_late()
            } else if role == "capture" && !worker_side {
                // The microphone's first frame may wait for the voice's first
                // look; stale, it is dropped (lamp-0c89, #4332).
                data.dropping_stale_first()
            } else {
                data
            },
        })
    }
    /// This worker's input takes a message late, in order and from the
    /// parent's boot, as the Live provider's does (40de19327): for the second
    /// route when it is the lamp's only voice (`--only-voice`), whose input is
    /// checked against fresh authority when it is used, as the provider's is,
    /// and whose end would end the voice.
    pub fn taking_late_input(mut self) -> Self {
        self.data = self.data.taking_late();
        self
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
    initial_capture_prepared: bool,
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
            initial_capture_prepared: role == "speaker",
        })
    }

    /// The capture side sends its initial readiness once. It may queue while
    /// the speaker starts, like the worker's initial control message. Only
    /// that first, in-order CapturePrepared gets the startup allowance; prime
    /// acknowledgements, later controls and reference data keep their bounds.
    /// Readiness grants no playback authority and does not satisfy a prime.
    pub fn receive_control(&mut self) -> io::Result<Option<ReferenceControl>> {
        self.receive_control_with(monotonic_us)
    }

    /// [`Self::receive_control`] with an explicit clock, as [`Channel::receive_with`].
    pub fn receive_control_with(
        &mut self,
        now: impl FnMut() -> u64,
    ) -> io::Result<Option<ReferenceControl>> {
        let initial_capture_prepared = self.initial_capture_prepared;
        self.control.receive_matching(
            now,
            |_| false,
            |control| {
                initial_capture_prepared
                    && matches!(control, ReferenceControl::CapturePrepared { .. })
            },
        )
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

    fn pair(dir: &Path) -> (Channel, Channel) {
        let (a, b) = (new_boot().unwrap(), new_boot().unwrap());
        let mut sender = Channel::bind(&dir.join("s"), a, b, 100_000).unwrap();
        let mut receiver = Channel::bind(&dir.join("r"), b, a, 100_000).unwrap();
        sender.connect(&dir.join("r")).unwrap();
        receiver.connect(&dir.join("s")).unwrap();
        (sender, receiver)
    }

    fn reference_pair(dir: &Path) -> (ReferenceChannels, ReferenceChannels) {
        let (capture_boot, speaker_boot) = (new_boot().unwrap(), new_boot().unwrap());
        let mut capture =
            ReferenceChannels::bind(dir, "capture", capture_boot, speaker_boot).unwrap();
        let mut speaker =
            ReferenceChannels::bind(dir, "speaker", speaker_boot, capture_boot).unwrap();
        assert!(capture.connect_step().unwrap());
        assert!(speaker.connect_step().unwrap());
        (capture, speaker)
    }

    fn capture_pair(dir: &Path) -> (WorkerChannels, WorkerChannels) {
        let (parent, worker) = (new_boot().unwrap(), new_boot().unwrap());
        let mut voice = WorkerChannels::bind(dir, "capture", parent, worker, false).unwrap();
        let mut capture = WorkerChannels::bind(dir, "capture", parent, worker, true).unwrap();
        voice.connect(dir, "capture", false).unwrap();
        capture.connect(dir, "capture", true).unwrap();
        (voice, capture)
    }

    /// Frames every 10 ms from 10 us, as capture sends them, the first `n`.
    fn frames(capture: &mut WorkerChannels, n: u32) {
        for k in 1..=n {
            let stamp = 10 + u64::from(k - 1) * 10_000;
            capture.data.send_with(k, || stamp).unwrap();
        }
    }

    #[test]
    fn the_microphones_stale_frames_at_the_start_are_dropped_said_once_and_the_first_fresh_taken() {
        let dir = SessionDirectory::create().unwrap();
        let (mut voice, mut capture) = capture_pair(&dir.path);
        said();
        // lamp-0c89's first look, 154 ms after the first frame (#4332), at
        // capture's own pace: frames 1 to 6 are 100 ms old or more.
        frames(&mut capture, 16);
        assert_eq!(voice.data.receive_with::<u32>(|| 154_182).unwrap(), Some(7));
        let lines = said();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].starts_with(
                "lamp-voice: capture.d.p: 6 stale first messages dropped, the oldest 154172 us (bound 100000 us), the next taken"
            ),
            "{lines:?}"
        );
        for k in 8..=16 {
            assert_eq!(voice.data.receive_with::<u32>(|| 154_182).unwrap(), Some(k));
        }
        assert!(said().is_empty());
        // Once one is taken, a stale frame later ends the voice as before.
        capture.data.send_with(17u32, || 154_200).unwrap();
        let refused = voice.data.receive_with::<u32>(|| 1_854_200).unwrap_err();
        assert!(
            refused.to_string().starts_with(
                "capture.d.p: stale, reordered or wrong-boot runtime message (sequence 17 after 16"
            ),
            "{refused}"
        );
        // Alone in the socket, the first is dropped and nothing is taken; the
        // next receive goes on with the run, said once when a frame is taken.
        let dir = SessionDirectory::create().unwrap();
        let (mut voice, mut capture) = capture_pair(&dir.path);
        frames(&mut capture, 1);
        assert_eq!(voice.data.receive_with::<u32>(|| 154_182).unwrap(), None);
        frames_from(&mut capture, 2, 20_010);
        assert_eq!(voice.data.receive_with::<u32>(|| 154_182).unwrap(), None);
        assert!(said().is_empty());
        frames_from(&mut capture, 3, 154_100);
        assert_eq!(voice.data.receive_with::<u32>(|| 154_182).unwrap(), Some(3));
        let lines = said();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].starts_with("lamp-voice: capture.d.p: 2 stale first messages dropped"),
            "{lines:?}"
        );
    }

    /// Frame `k`, stamped `stamp`.
    fn frames_from(capture: &mut WorkerChannels, k: u32, stamp: u64) {
        capture.data.send_with(k, || stamp).unwrap();
    }

    #[test]
    fn a_run_of_stale_frames_beyond_its_bound_ends_the_voice() {
        // The first frame over 1 s old.
        let dir = SessionDirectory::create().unwrap();
        let (mut voice, mut capture) = capture_pair(&dir.path);
        frames(&mut capture, 3);
        let refused = voice.data.receive_with::<u32>(|| 1_000_011).unwrap_err();
        assert!(
            refused.to_string().starts_with(
                "capture.d.p: stale, reordered or wrong-boot runtime message (sequence 1 after 0"
            ),
            "{refused}"
        );
        // A run that goes on past 1 s from its first frame, across receives.
        let dir = SessionDirectory::create().unwrap();
        let (mut voice, mut capture) = capture_pair(&dir.path);
        said();
        frames(&mut capture, 1);
        assert_eq!(voice.data.receive_with::<u32>(|| 154_182).unwrap(), None);
        frames_from(&mut capture, 2, 20);
        let refused = voice.data.receive_with::<u32>(|| 1_100_000).unwrap_err();
        assert!(
            refused.to_string().starts_with(
                "capture.d.p: stale, reordered or wrong-boot runtime message (sequence 2 after 1"
            ),
            "{refused}"
        );
        // The run is still said, once.
        let lines = said();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].contains("1 stale first messages dropped")
                && lines[0].contains("then ended by an error"),
            "{lines:?}"
        );
    }

    #[test]
    fn a_stale_first_frame_from_another_boot_out_of_order_or_after_now_still_ends_the_voice() {
        for case in [
            "wrong-boot",
            "not-first-sequence",
            "future",
            "wrong-boot-in-run",
            "out-of-order-in-run",
            "future-in-run",
        ] {
            let dir = SessionDirectory::create().unwrap();
            let (mut voice, mut capture) = capture_pair(&dir.path);
            if case == "wrong-boot" {
                capture.data.sender = new_boot().unwrap();
            }
            if case == "not-first-sequence" {
                capture.data.send_sequence = 1;
            }
            let sent = if case == "future" { 200_000 } else { 10 };
            capture.data.send_with(1u32, || sent).unwrap();
            if case == "wrong-boot-in-run" {
                capture.data.sender = new_boot().unwrap();
            }
            if case == "out-of-order-in-run" {
                capture.data.send_sequence += 1;
            }
            // Fresh, so only the order guard refuses the not-first case; in
            // the run, stale but for the case's own fault.
            let behind = match case {
                "future" | "future-in-run" => 200_000,
                "wrong-boot-in-run" | "out-of-order-in-run" => 20,
                _ => 154_150,
            };
            capture.data.send_with(2u32, || behind).unwrap();
            let refused = voice.data.receive_with::<u32>(|| 154_182).unwrap_err();
            assert_eq!(refused.kind(), io::ErrorKind::InvalidData, "{case}");
            assert!(
                refused.to_string().starts_with("capture.d.p: stale"),
                "{case}: {refused}"
            );
        }
    }

    #[test]
    fn other_data_channels_keep_a_stale_first_message_fatal() {
        let dir = SessionDirectory::create().unwrap();
        let (mut voice, mut capture) = capture_pair(&dir.path);
        voice.data.send_with(1u32, || 10).unwrap();
        assert!(capture.data.receive_with::<u32>(|| 154_182).is_err());
        let dir = SessionDirectory::create().unwrap();
        let (mut sender, mut receiver) = pair(&dir.path);
        sender.send_with(1u32, || 10).unwrap();
        assert!(receiver.receive_with::<u32>(|| 154_182).is_err());
    }

    #[test]
    fn initial_capture_prepared_survives_bounded_speaker_startup() {
        for age in [143_402, FIRST_CONTROL_MAX_AGE_US - 1] {
            let dir = SessionDirectory::create().unwrap();
            let (mut capture, mut speaker) = reference_pair(&dir.path);
            capture
                .control
                .send_with(
                    ReferenceControl::CapturePrepared {
                        privacy_generation: 7,
                    },
                    || 10,
                )
                .unwrap();
            assert!(matches!(
                speaker.receive_control_with(|| 10 + age).unwrap(),
                Some(ReferenceControl::CapturePrepared {
                    privacy_generation: 7
                })
            ));
        }
    }

    #[test]
    fn initial_capture_prepared_still_requires_age_boot_order_and_nonfuture_time() {
        for case in ["expired", "future", "wrong-boot", "not-first-sequence"] {
            let dir = SessionDirectory::create().unwrap();
            let (mut capture, mut speaker) = reference_pair(&dir.path);
            if case == "wrong-boot" {
                capture.control.sender = new_boot().unwrap();
            }
            if case == "not-first-sequence" {
                capture.control.send_sequence = 1;
            }
            capture
                .control
                .send_with(
                    ReferenceControl::CapturePrepared {
                        privacy_generation: 7,
                    },
                    || 10,
                )
                .unwrap();
            let at = match case {
                "expired" => 10 + FIRST_CONTROL_MAX_AGE_US,
                "future" => 9,
                _ => 143_412,
            };
            assert_eq!(
                speaker.receive_control_with(|| at).unwrap_err().kind(),
                io::ErrorKind::InvalidData,
                "{case}"
            );
        }
    }

    #[test]
    fn reference_startup_allowance_does_not_cover_primes_data_or_capture_input() {
        use crate::wire::{PreparedPrime, PrimeDisposition};
        let dir = SessionDirectory::create().unwrap();
        let (mut capture, mut speaker) = reference_pair(&dir.path);
        capture
            .control
            .send_with(
                ReferenceControl::ReferencePrimed {
                    prepared: PreparedPrime {
                        playback_epoch: 1,
                        privacy_generation: 7,
                        prepared_through: 480,
                        analysed_through: 480,
                        disposition: PrimeDisposition::Analysed,
                    },
                },
                || 10,
            )
            .unwrap();
        assert!(speaker.receive_control_with(|| 143_412).is_err());
        capture.data.send_with(1u32, || 10).unwrap();
        assert!(speaker.data.receive_with::<u32>(|| 143_412).is_err());
        speaker
            .control
            .send_with(
                ReferenceControl::CapturePrepared {
                    privacy_generation: 7,
                },
                || 10,
            )
            .unwrap();
        assert!(capture.receive_control_with(|| 143_412).is_err());
    }

    #[test]
    fn later_capture_prepared_keeps_the_existing_single_sender_stall_allowance() {
        let dir = SessionDirectory::create().unwrap();
        let (mut capture, mut speaker) = reference_pair(&dir.path);
        for _ in 0..3 {
            capture
                .control
                .send_with(
                    ReferenceControl::CapturePrepared {
                        privacy_generation: 7,
                    },
                    || 10,
                )
                .unwrap();
        }
        assert!(speaker.receive_control_with(|| 143_412).unwrap().is_some());
        // The pre-existing one-message sender-stall allowance still applies
        // after startup. A run of late messages must not gain first grace.
        assert!(speaker.receive_control_with(|| 143_412).unwrap().is_some());
        let error = speaker.receive_control_with(|| 143_412).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("bound 100000 us"), "{error}");
    }

    /// A receiver held up for 400 ms takes the backlog that queued meanwhile;
    /// held up past `HELD_UP_US`, it is refused as before. Both sides read
    /// one clock the test moves on by what it once slept (#1128).
    #[test]
    fn a_receiver_held_up_takes_what_queued_meanwhile_within_a_bound() {
        let dir = SessionDirectory::create().unwrap();
        let (mut sender, mut receiver) = pair(&dir.path);
        let mut t = monotonic_us();
        assert_eq!(receiver.receive_with::<u32>(|| t).unwrap(), None);
        sender.send_with(1u32, || t).unwrap();
        sender.send_with(2u32, || t).unwrap();
        t += 400_000;
        assert_eq!(receiver.receive_with::<u32>(|| t).unwrap(), Some(1));
        assert_eq!(receiver.receive_with::<u32>(|| t).unwrap(), Some(2));
        assert_eq!(receiver.receive_with::<u32>(|| t).unwrap(), None);

        sender.send_with(3u32, || t).unwrap();
        t += 1_700_000;
        assert!(receiver.receive_with::<u32>(|| t).is_err());
    }

    /// #1569 row 42 (the first builder on #1722, 20 Oct 17:46): a renewal
    /// stale by its age alone, with the next renewal queued behind it, is
    /// passed over, and the next judged in turn by the channel's own rules
    /// (one late message in order is taken, as from a held-up sender). Here
    /// renewals are numbers under 100. Each case is a receiver's first look,
    /// after the messages queued, as when a worker starts slowly, so no time
    /// of its own being held up excuses them.
    #[test]
    fn a_stale_renewal_with_the_next_renewal_behind_it_is_passed_over() {
        // Messages sent `apart_us` apart, read `read_us` after the first:
        // what is taken until the queue is empty, and the error that ends it.
        let first_look = |messages: &[u32], apart_us: u64, read_us: u64, renewing: bool| {
            let dir = SessionDirectory::create().unwrap();
            let (mut sender, mut receiver) = pair(&dir.path);
            let t = monotonic_us();
            for (i, message) in messages.iter().enumerate() {
                sender
                    .send_with(*message, || t + i as u64 * apart_us)
                    .unwrap();
            }
            let read = t + read_us;
            let mut taken = Vec::new();
            loop {
                let received = if renewing {
                    receiver.receive_renewed_with(|| read, |value: &u32| *value < 100)
                } else {
                    receiver.receive_with::<u32>(|| read)
                };
                match received {
                    Ok(Some(value)) => taken.push(value),
                    Ok(None) => return (taken, None),
                    Err(error) => return (taken, Some(error.to_string())),
                }
            }
        };
        let stale = |error: Option<String>| error.is_some_and(|error| error.contains("stale"));
        // 150 and 90 ms old: the first passed over, the second on time.
        assert_eq!(first_look(&[1, 2], 60_000, 150_000, true), (vec![2], None));
        // Without the rule the first ends it, as before.
        let (taken, error) = first_look(&[1, 2], 60_000, 150_000, false);
        assert!(taken.is_empty() && stale(error));
        // 210, 170, 130 and 90 ms old: 1 passed over, 2 taken late in order,
        // 3 passed over, 4 on time. The run is said in two lines, its first
        // and its end, not one a renewal.
        let _ = said();
        assert_eq!(
            first_look(&[1, 2, 3, 4], 40_000, 210_000, true),
            (vec![2, 4], None)
        );
        let lines = said();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[0].contains("a renewal 210000 us old (bound 100000 us) passed over"),
            "{lines:?}"
        );
        assert!(
            lines[1].ends_with("on time again after 2 renewals passed over, the oldest 210000 us"),
            "{lines:?}"
        );
        // A stale renewal alone still ends it.
        let (taken, error) = first_look(&[1], 0, 150_000, true);
        assert!(taken.is_empty() && stale(error));
        // So does the last of a run of stale renewals, with none fresh.
        let (taken, error) = first_look(&[1, 2, 3], 10_000, 300_000, true);
        assert!(taken == [2] && stale(error));
        // A stale renewal followed by another kind of message ends it.
        let (taken, error) = first_look(&[1, 100], 60_000, 150_000, true);
        assert!(taken.is_empty() && stale(error));
        // A stale message of another kind ends it, a renewal behind or not.
        let (taken, error) = first_look(&[100, 2], 60_000, 150_000, true);
        assert!(taken.is_empty() && stale(error));
        // A renewal stamped after the reading is not passed over, a renewal
        // behind or not: it is refused as before, its channel named.
        let dir = SessionDirectory::create().unwrap();
        let (mut sender, mut receiver) = pair(&dir.path);
        let t = monotonic_us();
        sender.send_with(1u32, || t + 50_000).unwrap();
        sender.send_with(2u32, || t + 60_000).unwrap();
        let error = receiver
            .receive_renewed_with(|| t, |value: &u32| *value < 100)
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with(&format!("{}: ", receiver.name)),
            "{error}"
        );
        assert!(error.contains("stale"), "{error}");
    }

    /// Past its first message, a receiver that reads its renewals one at a
    /// time and on time never needs the pass: each is taken as it comes,
    /// and `receive_renewed` gives what `receive` gives.
    #[test]
    fn renewals_read_on_time_are_taken_one_by_one() {
        let dir = SessionDirectory::create().unwrap();
        let (mut sender, mut receiver) = pair(&dir.path);
        let mut t = monotonic_us();
        for value in 1..=5u32 {
            sender.send_with(value, || t).unwrap();
            t += 20_000;
            assert_eq!(
                receiver
                    .receive_renewed_with(|| t, |value: &u32| *value < 100)
                    .unwrap(),
                Some(value)
            );
        }
        assert_eq!(
            receiver
                .receive_renewed_with(|| t, |value: &u32| *value < 100)
                .unwrap(),
            None
        );
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

    /// The provider's input and its output, bound as the worker and its
    /// parent bind them, connected.
    fn provider(dir: &Path) -> (WorkerChannels, WorkerChannels) {
        let (parent, worker) = (new_boot().unwrap(), new_boot().unwrap());
        let mut parent_side = WorkerChannels::bind(dir, "provider", parent, worker, false).unwrap();
        let mut worker_side = WorkerChannels::bind(dir, "provider", parent, worker, true).unwrap();
        parent_side.connect(dir, "provider", false).unwrap();
        worker_side.connect(dir, "provider", true).unwrap();
        (parent_side, worker_side)
    }

    /// Lamp-4ace, 19 Oct 05:08: the provider read its input one message at
    /// a time, each look under the 100 ms bound, and the second message in a
    /// row to come late, 0.2 ms past it, in order and from the parent's
    /// boot, ended the voice. On the provider's input such a message is
    /// taken, two in a row too, and the run ends with the next on time. Its
    /// output to the parent keeps the bound: the second late one there is
    /// refused as before.
    #[test]
    fn the_providers_input_takes_a_message_late_in_order_and_its_output_does_not() {
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut worker) = provider(&dir.path);
        let start = monotonic_us();
        let at = |ms: u64| start + ms * 1_000;
        assert_eq!(worker.data.receive_with::<u32>(|| at(0)).unwrap(), None);
        parent.data.send_with(1u32, || at(0)).unwrap();
        assert_eq!(worker.data.receive_with::<u32>(|| at(1)).unwrap(), Some(1));
        for (message, ms) in [(2u32, 1), (3, 2), (4, 3)] {
            parent.data.send_with(message, || at(ms)).unwrap();
        }
        // Each look under the bound since the last, the queue never empty.
        assert_eq!(worker.data.receive_with::<u32>(|| at(60)).unwrap(), Some(2));
        assert_eq!(
            worker.data.receive_with::<u32>(|| at(103)).unwrap(),
            Some(3)
        );
        assert_eq!(
            worker.data.receive_with::<u32>(|| at(104)).unwrap(),
            Some(4)
        );
        assert_eq!(worker.data.late_run(), 2);
        parent.data.send_with(5u32, || at(105)).unwrap();
        assert_eq!(
            worker.data.receive_with::<u32>(|| at(106)).unwrap(),
            Some(5)
        );
        assert_eq!(worker.data.late_run(), 0, "on time again");
        // The output, the same way round: the second late one is refused.
        assert_eq!(parent.data.receive_with::<u32>(|| at(0)).unwrap(), None);
        worker.data.send_with(1u32, || at(0)).unwrap();
        assert_eq!(parent.data.receive_with::<u32>(|| at(1)).unwrap(), Some(1));
        for (message, ms) in [(2u32, 1), (3, 2), (4, 3)] {
            worker.data.send_with(message, || at(ms)).unwrap();
        }
        assert_eq!(parent.data.receive_with::<u32>(|| at(60)).unwrap(), Some(2));
        assert_eq!(
            parent.data.receive_with::<u32>(|| at(103)).unwrap(),
            Some(3)
        );
        let refused = parent.data.receive_with::<u32>(|| at(104)).unwrap_err();
        assert!(
            refused.to_string().starts_with("provider.d.p: "),
            "{refused}"
        );
    }

    /// Two messages in a row late through `sender` to `receiver`, each look
    /// under the bound: the second's answer.
    fn second_late(sender: &mut Channel, receiver: &mut Channel) -> io::Result<Option<u32>> {
        let start = monotonic_us();
        let at = |ms: u64| start + ms * 1_000;
        assert_eq!(receiver.receive_with::<u32>(|| at(0)).unwrap(), None);
        sender.send_with(1u32, || at(0)).unwrap();
        assert_eq!(receiver.receive_with::<u32>(|| at(1)).unwrap(), Some(1));
        for (message, ms) in [(2u32, 1), (3, 2), (4, 3)] {
            sender.send_with(message, || at(ms)).unwrap();
        }
        assert_eq!(receiver.receive_with::<u32>(|| at(60)).unwrap(), Some(2));
        assert_eq!(receiver.receive_with::<u32>(|| at(103)).unwrap(), Some(3));
        receiver.receive_with::<u32>(|| at(104))
    }

    /// The reviewer on #1660: taking late is the provider's input alone.
    /// Every other channel refuses the second late message in a row: the
    /// provider's control both ways and its output, and every other
    /// worker's data and control, both ways.
    #[test]
    fn only_the_providers_input_takes_late_and_every_other_channel_keeps_its_bound() {
        let roles = [
            "provider", "capture", "speaker", "privacy", "second", "ring", "probe", "camera",
        ];
        for role in roles {
            let dir = SessionDirectory::create().unwrap();
            let (parent, worker) = (new_boot().unwrap(), new_boot().unwrap());
            let mut parent_side =
                WorkerChannels::bind(&dir.path, role, parent, worker, false).unwrap();
            let mut worker_side =
                WorkerChannels::bind(&dir.path, role, parent, worker, true).unwrap();
            parent_side.connect(&dir.path, role, false).unwrap();
            worker_side.connect(&dir.path, role, true).unwrap();
            let into_worker = second_late(&mut parent_side.data, &mut worker_side.data);
            if role == "provider" {
                assert_eq!(into_worker.unwrap(), Some(4), "{role}.d.w");
            } else {
                let refused = into_worker.unwrap_err().to_string();
                assert!(refused.starts_with(&format!("{role}.d.w: ")), "{refused}");
            }
            for (name, refused) in [
                (
                    "d.p",
                    second_late(&mut worker_side.data, &mut parent_side.data),
                ),
                (
                    "c.w",
                    second_late(&mut parent_side.control, &mut worker_side.control),
                ),
                (
                    "c.p",
                    second_late(&mut worker_side.control, &mut parent_side.control),
                ),
            ] {
                let refused = refused.unwrap_err().to_string();
                assert!(
                    refused.starts_with(&format!("{role}.{name}: ")),
                    "{refused}"
                );
            }
        }
    }

    /// The reviewer on #1660: a run of late messages is said when it ends
    /// in an error and when the channel closes, not only when the next is
    /// on time; and the first line names the bound used, the receiver's
    /// held-up time included.
    #[test]
    fn a_run_of_late_messages_is_said_however_it_ends() {
        let start = monotonic_us();
        let at = |ms: u64| start + ms * 1_000;
        let late_run = |parent: &mut WorkerChannels, worker: &mut WorkerChannels| {
            assert_eq!(worker.data.receive_with::<u32>(|| at(0)).unwrap(), None);
            parent.data.send_with(1u32, || at(0)).unwrap();
            assert_eq!(worker.data.receive_with::<u32>(|| at(1)).unwrap(), Some(1));
            for (message, ms) in [(2u32, 1), (3, 2), (4, 3)] {
                parent.data.send_with(message, || at(ms)).unwrap();
            }
            assert_eq!(worker.data.receive_with::<u32>(|| at(60)).unwrap(), Some(2));
            assert_eq!(
                worker.data.receive_with::<u32>(|| at(103)).unwrap(),
                Some(3)
            );
            assert_eq!(
                worker.data.receive_with::<u32>(|| at(104)).unwrap(),
                Some(4)
            );
        };
        let _ = said();
        // Ended by an error: a message from another boot.
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut worker) = provider(&dir.path);
        late_run(&mut parent, &mut worker);
        let lines = said();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].contains("101000 us old (bound 100000 us) taken late"),
            "{lines:?}"
        );
        let data = encode(&Envelope {
            boot: new_boot().unwrap(),
            sequence: 5,
            sent_at_us: at(104),
            payload: 5u32,
        })
        .unwrap();
        parent.data.endpoint.try_send(&data).unwrap();
        assert!(worker.data.receive_with::<u32>(|| at(105)).is_err());
        assert_eq!(
            said(),
            [
                "lamp-voice: provider.d.w: ended by an error after 2 late messages, the oldest 101000 us"
            ]
        );
        // Closed with the channel, the worker gone quiet.
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut worker) = provider(&dir.path);
        late_run(&mut parent, &mut worker);
        let _ = said();
        drop(worker);
        assert_eq!(
            said(),
            ["lamp-voice: provider.d.w: closed after 2 late messages, the oldest 101000 us"]
        );
        drop(parent);
        // The bound said is the one used: the receiver held up 150 ms
        // between its looks, so 250 ms.
        let dir = SessionDirectory::create().unwrap();
        let (mut parent, mut worker) = provider(&dir.path);
        assert_eq!(worker.data.receive_with::<u32>(|| at(1)).unwrap(), None);
        parent.data.send_with(1u32, || at(1)).unwrap();
        parent.data.send_with(2u32, || at(2)).unwrap();
        assert_eq!(
            worker.data.receive_with::<u32>(|| at(150)).unwrap(),
            Some(1)
        );
        assert_eq!(
            worker.data.receive_with::<u32>(|| at(300)).unwrap(),
            Some(2)
        );
        let lines = said();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("298000 us old (bound 250000 us) taken late")),
            "{lines:?}"
        );
    }

    /// The second route alone (`--only-voice`) takes its input late as the
    /// provider does; standing by, it keeps the bound, as before.
    #[test]
    fn the_second_routes_input_takes_a_message_late_only_as_the_only_voice() {
        let late = |only_voice: bool| {
            let dir = SessionDirectory::create().unwrap();
            let (parent_boot, worker_boot) = (new_boot().unwrap(), new_boot().unwrap());
            let mut parent =
                WorkerChannels::bind(&dir.path, "second", parent_boot, worker_boot, false).unwrap();
            let mut worker =
                WorkerChannels::bind(&dir.path, "second", parent_boot, worker_boot, true).unwrap();
            if only_voice {
                worker = worker.taking_late_input();
            }
            parent.connect(&dir.path, "second", false).unwrap();
            worker.connect(&dir.path, "second", true).unwrap();
            second_late(&mut parent.data, &mut worker.data)
        };
        assert_eq!(late(true).unwrap(), Some(4));
        assert!(late(false).is_err(), "standing by: refused, as before");
    }

    /// On the provider's input, a late message from another boot, out of
    /// order, or after one missing, is still refused: only lateness is
    /// taken. Each look is under the bound, so none is a held-up receiver's.
    #[test]
    fn the_providers_input_still_refuses_another_boot_or_another_order() {
        let late = |boot: Option<BootId>, sequence: u64| {
            let dir = SessionDirectory::create().unwrap();
            let (mut parent, mut worker) = provider(&dir.path);
            let start = monotonic_us();
            let at = |ms: u64| start + ms * 1_000;
            assert_eq!(worker.data.receive_with::<u32>(|| at(0)).unwrap(), None);
            parent.data.send_with(1u32, || at(0)).unwrap();
            assert_eq!(worker.data.receive_with::<u32>(|| at(1)).unwrap(), Some(1));
            parent.data.send_with(2u32, || at(2)).unwrap();
            let data = encode(&Envelope {
                boot: boot.unwrap_or(parent.data.sender),
                sequence,
                sent_at_us: at(3),
                payload: 3u32,
            })
            .unwrap();
            parent.data.endpoint.try_send(&data).unwrap();
            assert_eq!(worker.data.receive_with::<u32>(|| at(60)).unwrap(), Some(2));
            worker.data.receive_with::<u32>(|| at(104))
        };
        assert_eq!(late(None, 3).unwrap(), Some(3), "in order: taken");
        assert!(late(Some(new_boot().unwrap()), 3).is_err(), "another boot");
        assert!(late(None, 2).is_err(), "the same again");
        assert!(late(None, 4).is_err(), "one missing before it");
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
