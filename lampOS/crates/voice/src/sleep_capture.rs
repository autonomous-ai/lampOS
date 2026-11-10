//! A hand's sleep that still hears its name (migrate-P2,
//! `LAMP_SLEEP_HEARS_NAME`; the Queue comment
//! <https://github.com/autonomous-ai/lampOS/issues/3997#issuecomment-6526292114>).
//!
//! A lamp put to sleep by a hand held three seconds on its head tears its
//! whole audio rig down and waits in the privacy wait, as any mute does. With
//! this switch `on` (or `shadow`) and the physical slide observed open, that
//! wait also runs the sleep capture ([`crate::sleep_worker`]): one microphone,
//! read under its own name-only grant, published for the name listener alone
//! on `mic2/sleep_audio`, never on `mic2/audio`. The wait reads `name/heard`,
//! and when the wake rule ([`judge`]) holds, it closes the capture and its
//! grant and asks the same wake a tap asks: the persisted clear, the "hm?",
//! the tap's window. With `shadow`, everything runs the same but no wake is
//! asked: each hearing is journaled with the rule's verdict, so the rule can
//! be set from a lamp's numbers.
//!
//! What stays as it was, by the reviewers' conditions:
//! - The wait's controller keeps microphone and camera `Denied`, so
//!   `voice/privacy` says not permitted, Ears stays closed, the red line on
//!   the ring is the slide's alone, and "Denied" still means the main
//!   capture is silent. The grant is separate ([`lamp_turn_owner::NameGrant`]).
//! - Only a sleep by choice ([`crate::hand_controls::HandControls::asleep_by_choice`])
//!   listens: never a person's mute, the restart guard's, an untrusted record
//!   or onboarding (#3747). A closed or unreadable slide revokes the grant at
//!   once, at the look that reads it, and the lease (as long as the main
//!   capture's authority) ends it without a renewal.
//! - The topic's readers are checked before each publish: every listening
//!   subscriber must be the name listener (`lamp-name`, bus name `name`).
//!   Any other stops the capture, says so once, and leaves the full sleep of
//!   today until the lamp wakes ([`Readers`]).
//! - Nothing of it reaches a conversation: the blocks go to the bus and
//!   nowhere else in the voice, and the woken cycle opens its own rig, whose
//!   activity and pre-roll start empty.
use crate::{
    process::{SessionDirectory, Worker, new_boot},
    wire::{Control, SleepBlock, WorkerEvent},
};
use lamp_bus::{Bus, Listener, Publisher, Subscriber};
use lamp_ipc::monotonic_us;
use lamp_msgs::{Audio, NameHeard};
use lamp_turn_owner::{BootId, MonoTime, NameGrantIssuer};
use std::{
    collections::VecDeque,
    io,
    path::Path,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// The switch: `on` wakes on the name, `shadow` listens and only journals
/// what the rule would do; anything else, or unset, is today's full sleep.
pub const SWITCH: &str = "LAMP_SLEEP_HEARS_NAME";

/// What [`SWITCH`] says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    /// Listens for the name while asleep and wakes on it.
    On,
    /// Listens as `On` does, journals each hearing with the rule's verdict,
    /// and never wakes.
    Shadow,
}

impl Mode {
    /// `on` and `shadow` exactly; anything else is off.
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("on") => Self::On,
            Some("shadow") => Self::Shadow,
            _ => Self::Off,
        }
    }

    pub fn from_environment() -> Self {
        Self::parse(std::env::var(SWITCH).ok().as_deref())
    }

    /// Whether the sleep capture runs at all.
    pub fn listens(self) -> bool {
        self != Self::Off
    }
}

/// The name listener's own bar (`lamp-name --least`, the product's 0.9): a
/// hearing under it is no hit, whatever bar the listener runs at.
pub const NAME_LEAST: f32 = 0.9;
/// One hearing this sure is a hit alone, for [`WakeRule::OneStrong`].
pub const STRONG_LEAST: f32 = 0.97;
/// Two hits this close together, for [`WakeRule::TwoHits`].
pub const PAIR_WITHIN_US: u64 = 3_000_000;

/// Which hearings wake a sleeping lamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeRule {
    /// Two hearings at [`NAME_LEAST`] or over within [`PAIR_WITHIN_US`].
    TwoHits,
    /// One hearing at [`STRONG_LEAST`] or over.
    OneStrong,
    /// Either of the two.
    Either,
}

/// The rule in force, "one at 0.97 or two at 0.9 within 3 s": the turns
/// reviewer's prior, and the design owner's answer that one call is enough.
pub const WAKE_RULE: WakeRule = WakeRule::Either;

/// One hearing of the name on the sleep source: when it began, on the
/// voice's monotonic clock, and how sure the listener was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub began_us: u64,
    pub score: f32,
}

/// What the rule's two halves say of the newest hearing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Judged {
    /// It and an earlier one, both at [`NAME_LEAST`] or over, within
    /// [`PAIR_WITHIN_US`].
    pub two: bool,
    /// It alone, at [`STRONG_LEAST`] or over.
    pub strong: bool,
}

impl Judged {
    /// Whether `rule` wakes on it.
    pub fn wakes(self, rule: WakeRule) -> bool {
        match rule {
            WakeRule::TwoHits => self.two,
            WakeRule::OneStrong => self.strong,
            WakeRule::Either => self.two || self.strong,
        }
    }

    /// The verdict as the journal says it.
    pub fn said(self) -> &'static str {
        match (self.two, self.strong) {
            (true, true) => "would wake: two >=0.9 in 3 s, and one >=0.97",
            (true, false) => "would wake: two >=0.9 in 3 s",
            (false, true) => "would wake: one >=0.97",
            (false, false) => "no",
        }
    }
}

/// The rule's halves for the newest of `hits` (in the order heard), a pure
/// function of the hearings: the newest alone at [`STRONG_LEAST`], or the
/// newest and any earlier one at [`NAME_LEAST`] within [`PAIR_WITHIN_US`].
pub fn judge(hits: &[Hit]) -> Judged {
    let Some((newest, earlier)) = hits.split_last() else {
        return Judged::default();
    };
    let hit = |hit: &Hit| hit.score >= NAME_LEAST;
    Judged {
        strong: newest.score >= STRONG_LEAST,
        two: hit(newest)
            && earlier.iter().any(|before| {
                hit(before)
                    && newest.began_us >= before.began_us
                    && newest.began_us - before.began_us <= PAIR_WITHIN_US
            }),
    }
}

/// How far before a hearing its level looks: `ago_ms` runs from when the
/// score peaked, near the name's end, so the name's own sound is before it.
/// On lamp-e82d a window from the peak alone read the silence after the
/// word (-74 dBFS beside -24 dBFS for the same voice).
const WORDS_BEFORE_US: u64 = 1_000_000;
/// How long a block's level is kept: past the longest a hearing's words run.
const LEVELS_KEPT_US: u64 = 3_000_000;
/// What a block of no sound reads, and the least [`block_dbfs`] says.
const SILENCE_DBFS: f32 = -120.0;

/// A block's level: its root mean square against full scale. For the
/// shadow's line alone, to learn whether a call from another room reads
/// quieter than one to this lamp: no rule reads it yet.
pub fn block_dbfs(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return SILENCE_DBFS;
    }
    let sum: f64 = samples.iter().map(|&s| f64::from(s).powi(2)).sum();
    let rms = (sum / samples.len() as f64).sqrt() / f64::from(i16::MAX);
    if rms <= 0.0 {
        return SILENCE_DBFS;
    }
    ((20.0 * rms.log10()) as f32).max(SILENCE_DBFS)
}

/// The loudest block from `began_us` on, or [`SILENCE_DBFS`] with none.
pub fn loudest_since(levels: &VecDeque<(u64, f32)>, began_us: u64) -> f32 {
    levels
        .iter()
        .filter(|(at, _)| *at >= began_us)
        .map(|(_, level)| *level)
        .fold(SILENCE_DBFS, f32::max)
}

/// The name listener's bus name (`lamp-name`'s `NAME`): a subscriber's
/// socket is `<name>.<process>.<count>.sock`.
pub const NAME_LISTENER: &str = "name";

/// Who may hear the sleep's sound: the name listener, and nobody else.
pub struct Readers;

impl Readers {
    /// The first listening subscriber that is not the name listener, if
    /// any. A socket with no process behind it hears nothing (the topic's
    /// next publish removes it) and is passed over.
    pub fn other(listeners: &[Listener]) -> Option<String> {
        listeners
            .iter()
            .filter(|listener| listener.listening)
            .find(|listener| listener.socket.split('.').next() != Some(NAME_LISTENER))
            .map(|listener| listener.socket.clone())
    }
}

/// How often the grant is renewed while the slide stays open: well inside
/// its 250 ms lease, as the wait's other authority is.
const RENEW_EVERY_US: u64 = 25_000;
/// How often the readers are looked at while no block waits to go.
const READERS_EVERY: Duration = Duration::from_millis(100);
/// At most this many blocks, controls and hearings are read in one look.
const MOST_A_LOOK: usize = 32;
/// How long a stopped worker has to end before it is forced, as any worker.
const STOP_GRACE: Duration = Duration::from_millis(300);

/// What one look of a running sleep capture found.
#[derive(Debug, PartialEq)]
pub enum Found {
    /// Nothing that changes it.
    Nothing,
    /// The rule woke it at this time (only with [`Mode::On`]).
    Woke(u64),
    /// The worker ended (its grant revoked or lapsed): close it; a later
    /// look may start another.
    Ended(String),
    /// Something it must not go on with: close it and keep today's full
    /// sleep until the lamp wakes.
    Refused(String),
}

/// The sleep capture as the wait runs it: its worker, its grant, the
/// topic's publisher and the name's hearings.
pub struct SleepCapture {
    // Dropped before its directory, whose endpoints it removes.
    worker: Worker,
    _directory: Option<SessionDirectory>,
    issuer: NameGrantIssuer,
    granted: bool,
    last_grant_us: Option<u64>,
    bus: Bus,
    audio: Publisher,
    heard: Subscriber,
    hits: Vec<Hit>,
    /// The recent blocks' levels, for what a hearing's own sound measured.
    levels: VecDeque<(u64, f32)>,
    started_us: u64,
    sequence: u64,
    readers_at: Option<Instant>,
    mode: Mode,
}

impl SleepCapture {
    /// Start the worker on `device` and grant it at once: the slide was
    /// just read open. `keep` renews the privacy worker's lease while the
    /// new worker starts.
    pub fn start(
        executable: &Path,
        device: &str,
        bus: Bus,
        mode: Mode,
        keep: &mut dyn FnMut() -> io::Result<()>,
    ) -> Result<Self> {
        let directory = SessionDirectory::create()?;
        let boot = new_boot()?;
        let worker =
            Worker::spawn_with_tick(executable, &directory.path, "sleep", boot, &[device], keep)?;
        Self::with_worker(worker, Some(directory), boot, bus, mode)
    }

    /// Around a worker already started, its grants from `controller`.
    pub fn with_worker(
        worker: Worker,
        directory: Option<SessionDirectory>,
        controller: BootId,
        bus: Bus,
        mode: Mode,
    ) -> Result<Self> {
        let audio = bus.publisher_declared(
            Audio::SLEEP_TOPIC,
            &Audio::declaration(),
            crate::bus_out::SOURCE,
        )?;
        let heard = bus.subscribe_typed(NameHeard::TOPIC, "voice_sleep", NameHeard::TYPE)?;
        let mut capture = Self {
            worker,
            _directory: directory,
            issuer: NameGrantIssuer::new(controller),
            granted: false,
            last_grant_us: None,
            bus,
            audio,
            heard,
            hits: Vec::new(),
            levels: VecDeque::new(),
            started_us: monotonic_us(),
            sequence: 0,
            readers_at: None,
            mode,
        };
        capture.renew(true)?;
        Ok(capture)
    }

    /// The slide as the wait read it: anything but open and fresh revokes
    /// the grant at once.
    pub fn slide(&mut self, open: bool) {
        if !open && self.granted {
            let _ = self.renew(false);
        }
    }

    /// Whether its grant stands: what `hears_name` says.
    pub fn granted(&self) -> bool {
        self.granted
    }

    fn renew(&mut self, open: bool) -> Result<()> {
        let now = monotonic_us();
        if open
            && self.granted
            && self
                .last_grant_us
                .is_some_and(|last| now.saturating_sub(last) < RENEW_EVERY_US)
        {
            return Ok(());
        }
        let at = MonoTime::from_micros(now);
        let grant = if open {
            self.issuer.grant(at)?
        } else {
            self.issuer.revoke(at)?
        };
        // Taken as revoked before it is sent: a block read after this is
        // never published, even if the send fails.
        self.granted = open;
        self.last_grant_us = Some(now);
        self.worker
            .channels
            .control
            .send(Control::NameGrant { grant })?;
        Ok(())
    }

    /// One look: renew or revoke the grant by the slide, read what the
    /// worker sent, publish its blocks to the name listener alone, and read
    /// the name's hearings.
    pub fn look(&mut self, slide_open: bool) -> Result<Found> {
        if let Err(error) = self.renew(slide_open) {
            return Ok(Found::Ended(format!(
                "its grant could not be sent: {error}"
            )));
        }
        if let Some(status) = self.worker.try_exit()? {
            return Ok(Found::Ended(format!("its worker ended: {status}")));
        }
        // Every event of this worker ends it: it says only that its grant
        // ended, or a fault.
        match self.worker.channels.control.receive::<WorkerEvent>() {
            Ok(Some(WorkerEvent::CaptureStopped)) => {
                return Ok(Found::Ended("its grant ended".into()));
            }
            Ok(Some(WorkerEvent::Fault { code })) => return Ok(Found::Refused(code)),
            Ok(Some(event)) => {
                return Ok(Found::Refused(format!("unexpected event {event:?}")));
            }
            Ok(None) => {}
            Err(error) => return Ok(Found::Ended(error.to_string())),
        }
        let mut blocks = Vec::new();
        for _ in 0..MOST_A_LOOK {
            match self.worker.channels.data.receive::<SleepBlock>() {
                Ok(Some(block)) => blocks.push(block),
                Ok(None) => break,
                // A block held past its bound (the wait held up): the sound
                // is lost and the capture starts again later.
                Err(error) => return Ok(Found::Ended(error.to_string())),
            }
        }
        let due = self
            .readers_at
            .is_none_or(|at| at.elapsed() >= READERS_EVERY);
        if (!blocks.is_empty() || due)
            && let Some(refused) = self.readers_checked(!blocks.is_empty())?
        {
            return Ok(Found::Refused(refused));
        }
        for block in blocks {
            self.note_level(&block);
            if !self.granted {
                break;
            }
            self.publish(&block)?;
        }
        Ok(self.hearings())
    }

    /// The topic's readers, after the publisher has looked for them: any
    /// one that is not the name listener refuses. Done before a publish, so
    /// the guarded publish (which never looks again) reaches only these.
    fn readers_checked(&mut self, publishing: bool) -> Result<Option<String>> {
        if publishing {
            self.audio.prepare_guarded_event()?;
        }
        self.readers_at = Some(Instant::now());
        let listeners = self.bus.subscribers(Audio::SLEEP_TOPIC)?;
        Ok(Readers::other(&listeners)
            .map(|other| format!("{other} subscribes to {}", Audio::SLEEP_TOPIC)))
    }

    /// Keeps this block's level for [`LEVELS_KEPT_US`], read off the sound
    /// the listener hears, for the shadow's line.
    fn note_level(&mut self, block: &SleepBlock) {
        let at = block.read_completed_at_us;
        self.levels.push_back((at, block_dbfs(&block.samples)));
        while self
            .levels
            .front()
            .is_some_and(|(then, _)| at.saturating_sub(*then) > LEVELS_KEPT_US)
        {
            self.levels.pop_front();
        }
    }

    fn publish(&mut self, block: &SleepBlock) -> Result<()> {
        let header = Audio {
            sample_rate: 16_000,
            channels: 1,
            sample_format: Audio::S16LE.into(),
            sequence: self.sequence,
        };
        let samples: Vec<u8> = block
            .samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        let payload = header.block(&samples)?;
        self.sequence += 1;
        let granted = self.granted;
        match self.audio.publish_bytes_guarded(
            block.read_completed_at_us,
            Audio::ENCODING,
            &payload,
            || granted,
        ) {
            Ok(_) => Ok(()),
            // A listener gone or slow loses the block, and nothing else.
            Err(error) if error.kind() != io::ErrorKind::PermissionDenied => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// The name's hearings since the last look: each begun after the
    /// capture started is a hit, judged with those of the last
    /// [`PAIR_WITHIN_US`]; each is journaled in shadow.
    fn hearings(&mut self) -> Found {
        for _ in 0..MOST_A_LOOK {
            let message = match self.heard.recv(Duration::ZERO) {
                Ok(Some(message)) => message,
                _ => break,
            };
            let Ok(name) = message.parse::<NameHeard>() else {
                continue;
            };
            if name.check().is_err() {
                continue;
            }
            let now = monotonic_us();
            let began_us = now.saturating_sub(u64::from(name.ago_ms) * 1_000);
            // A hearing begun before the first block was sent is no sleep's.
            if began_us < self.started_us {
                continue;
            }
            self.hits
                .retain(|hit| began_us.saturating_sub(hit.began_us) <= PAIR_WITHIN_US);
            self.hits.push(Hit {
                began_us,
                score: name.score,
            });
            let judged = judge(&self.hits);
            if self.mode == Mode::Shadow {
                eprintln!(
                    "lamp-voice: asleep, its name heard at {:.3}, {} ms ago, loudest {:.1} dBFS (shadow): {}",
                    name.score,
                    name.ago_ms,
                    loudest_since(&self.levels, began_us.saturating_sub(WORDS_BEFORE_US)),
                    judged.said()
                );
            }
            if self.mode == Mode::On && judged.wakes(WAKE_RULE) {
                eprintln!(
                    "lamp-voice: asleep, its name heard at {:.3}: wakes ({})",
                    name.score,
                    judged.said()
                );
                return Found::Woke(now);
            }
        }
        Found::Nothing
    }

    /// Revoke, stop and verify the worker's end, renewing the privacy
    /// worker's lease with `keep` while it is waited for. A worker that must
    /// be forced is killed and reaped, and said as a failure.
    pub fn close(mut self, keep: &mut dyn FnMut() -> io::Result<()>) -> io::Result<()> {
        if self.granted {
            let _ = self.renew(false);
        }
        let stop = self.worker.channels.control.send(Control::Stop);
        #[cfg(test)]
        self.worker.run_on_stop();
        let deadline = Instant::now() + STOP_GRACE;
        loop {
            if let Some(status) = self.worker.try_exit()? {
                return self.worker.exit_result(status);
            }
            if Instant::now() >= deadline {
                let mut why = "did not stop within 300 ms".to_owned();
                if let Err(error) = stop {
                    why.push_str(&format!("; its stop was refused: {error}"));
                }
                return Err(self.worker.terminate_and_reap(io::ErrorKind::TimedOut, why));
            }
            keep()?;
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// How the sleep capture is started, given the wait's `keep`.
pub type Start<'a> =
    Box<dyn FnMut(Mode, &mut dyn FnMut() -> io::Result<()>) -> Result<SleepCapture> + 'a>;

/// The muted wait's sleep capture across its looks: when it runs, why it
/// may not, and what `voice/mute` says of it.
pub struct SleepName<'a> {
    mode: Mode,
    start: Start<'a>,
    capture: Option<SleepCapture>,
    /// Why it stays closed until this sleep is over: said once.
    refused: Option<String>,
    /// After a wake by name, it does not start again while that wake is
    /// saved and the wait opens; a wake the controls refused lets it listen
    /// again after this.
    woke_until: Option<Instant>,
}

/// How long after a wake by name the capture stays closed while the wake is
/// saved: the wait opens well within it.
const AFTER_WAKE: Duration = Duration::from_secs(1);

impl<'a> SleepName<'a> {
    pub fn new(mode: Mode, start: Start<'a>) -> Self {
        Self {
            mode,
            start,
            capture: None,
            refused: None,
            woke_until: None,
        }
    }

    /// Whether it listens for the name now: what `voice/mute` says as
    /// `hears_name`.
    pub fn hearing(&self) -> bool {
        self.capture.as_ref().is_some_and(SleepCapture::granted)
    }

    /// The slide as the wait just read it: revoked at once unless open.
    pub fn slide(&mut self, open: bool) {
        if let Some(capture) = self.capture.as_mut() {
            capture.slide(open);
        }
    }

    /// One look of a running capture. A wake closes it and its grant before
    /// it is returned, so nothing of it is open when the woken cycle starts.
    pub fn look(
        &mut self,
        slide_open: bool,
        keep: &mut dyn FnMut() -> io::Result<()>,
    ) -> Result<Option<u64>> {
        let Some(capture) = self.capture.as_mut() else {
            return Ok(None);
        };
        let found = match capture.look(slide_open) {
            Ok(found) => found,
            Err(error) => Found::Refused(error.to_string()),
        };
        match found {
            Found::Nothing => Ok(None),
            Found::Woke(at) => {
                self.close(keep)?;
                self.woke_until = Some(Instant::now() + AFTER_WAKE);
                Ok(Some(at))
            }
            Found::Ended(why) => {
                self.close(keep)?;
                if slide_open {
                    eprintln!("lamp-voice: the sleep capture ended ({why}); it starts again");
                }
                Ok(None)
            }
            Found::Refused(why) => {
                self.close(keep)?;
                self.refuse(format!(
                    "the sleep capture stopped: {why}; asleep as before until woken"
                ));
                Ok(None)
            }
        }
    }

    /// Start or stop it as this look wants: `wanted` while a sleep by
    /// choice waits, `slide_open` while the slide reads open and fresh.
    pub fn want(
        &mut self,
        wanted: bool,
        slide_open: bool,
        keep: &mut dyn FnMut() -> io::Result<()>,
    ) -> Result<()> {
        if !wanted {
            // A new sleep may try again.
            self.refused = None;
        }
        if self.woke_until.is_some_and(|until| Instant::now() >= until) {
            self.woke_until = None;
        }
        let run = self.mode.listens()
            && wanted
            && slide_open
            && self.refused.is_none()
            && self.woke_until.is_none();
        match (run, self.capture.is_some()) {
            (true, false) => match (self.start)(self.mode, keep) {
                Ok(capture) => {
                    eprintln!(
                        "lamp-voice: asleep, and listening for its name alone{}",
                        if self.mode == Mode::Shadow {
                            " (shadow: it does not wake on it)"
                        } else {
                            ""
                        }
                    );
                    self.capture = Some(capture);
                }
                Err(error) => self.refuse(format!(
                    "the sleep capture could not start: {error}; asleep as before until woken"
                )),
            },
            (false, true) => self.close(keep)?,
            _ => {}
        }
        Ok(())
    }

    fn refuse(&mut self, why: String) {
        eprintln!("lamp-voice: {why}");
        self.refused = Some(why);
    }

    /// Closed, its end verified. A worker that had to be forced is killed
    /// and reaped, said, and this sleep stays as before; only a failed
    /// reap is returned.
    pub fn close(&mut self, keep: &mut dyn FnMut() -> io::Result<()>) -> Result<()> {
        let Some(capture) = self.capture.take() else {
            return Ok(());
        };
        if let Err(error) = capture.close(keep) {
            self.refuse(format!(
                "the sleep capture's end was not clean: {error}; asleep as before until woken"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::activity::{Activity, ObservedAudio, TurnDetector};
    use lamp_audio::{
        CAPTURE_SAMPLES,
        device::DeviceState,
        testing::{CaptureControl, ScriptedCapture},
    };
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
        thread,
    };

    #[test]
    fn a_hearings_level_is_its_own_loudest_block() {
        assert_eq!(block_dbfs(&[]), SILENCE_DBFS);
        assert_eq!(block_dbfs(&[0; 320]), SILENCE_DBFS);
        assert!(block_dbfs(&[i16::MAX; 320]).abs() < 0.01);
        assert!((block_dbfs(&[i16::MAX / 10; 320]) + 20.0).abs() < 0.01);
        let levels = VecDeque::from([(100, -10.0), (200, -40.0), (300, -30.0)]);
        assert_eq!(loudest_since(&levels, 150), -30.0);
        assert_eq!(loudest_since(&levels, 400), SILENCE_DBFS);
    }

    // Process ownership: this test process writes and reads this fixture state; not IPC.
    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn hit(began_s: f64, score: f32) -> Hit {
        Hit {
            began_us: (began_s * 1_000_000.0) as u64,
            score,
        }
    }

    /// The rule as a pure function: one hearing at the name's bar does not
    /// wake; two within 3 s do; two 4 s apart don't; one at 0.97 does, by
    /// the strong half alone; and the constant picks the rule.
    #[test]
    fn two_hearings_within_three_seconds_wake_and_one_or_two_far_apart_do_not() {
        assert_eq!(judge(&[]), Judged::default());
        let one = judge(&[hit(1.0, 0.92)]);
        assert!(!one.wakes(WakeRule::TwoHits) && !one.wakes(WakeRule::Either));
        let two = judge(&[hit(1.0, 0.92), hit(3.5, 0.91)]);
        assert!(two.two && !two.strong && two.wakes(WakeRule::TwoHits));
        assert!(!two.wakes(WakeRule::OneStrong) && two.wakes(WakeRule::Either));
        let apart = judge(&[hit(1.0, 0.95), hit(5.0, 0.95)]);
        assert!(!apart.two && !apart.wakes(WakeRule::Either));
        // A hearing under the name's own bar is no hit.
        assert!(!judge(&[hit(1.0, 0.6), hit(1.5, 0.92)]).two);
        let strong = judge(&[hit(1.0, 0.98)]);
        assert!(strong.strong && strong.wakes(WakeRule::OneStrong));
        assert!(!strong.wakes(WakeRule::TwoHits) && strong.wakes(WakeRule::Either));
        assert_eq!(WAKE_RULE, WakeRule::Either);
        assert_eq!(one.said(), "no");
        assert_eq!(two.said(), "would wake: two >=0.9 in 3 s");
        assert_eq!(strong.said(), "would wake: one >=0.97");
    }

    #[test]
    fn the_switch_is_on_or_shadow_exactly_and_off_otherwise() {
        assert_eq!(Mode::parse(Some("on")), Mode::On);
        assert_eq!(Mode::parse(Some("shadow")), Mode::Shadow);
        for off in [None, Some("off"), Some("On"), Some("1"), Some("")] {
            assert_eq!(Mode::parse(off), Mode::Off, "{off:?}");
        }
        assert!(!Mode::Off.listens() && Mode::On.listens() && Mode::Shadow.listens());
    }

    #[test]
    fn only_the_name_listener_may_read_the_sleeps_sound() {
        let listener = |socket: &str, listening| Listener {
            socket: socket.into(),
            listening,
        };
        assert_eq!(Readers::other(&[]), None);
        assert_eq!(Readers::other(&[listener("name.12.0", true)]), None);
        // A socket left by one that died hears nothing.
        assert_eq!(
            Readers::other(&[listener("name.12.0", true), listener("ears.9.1", false)]),
            None
        );
        assert_eq!(
            Readers::other(&[listener("name.12.0", true), listener("ears.9.1", true)]),
            Some("ears.9.1".into())
        );
        assert_eq!(
            Readers::other(&[listener("named.1.0", true)]),
            Some("named.1.0".into())
        );
    }

    /// A sleep capture over a scripted microphone, its worker's real loop
    /// on a thread, publishing on a bus of its own; and a feeder that says
    /// `value` into the microphone every 20 ms while `feeding`.
    pub(crate) struct Rig {
        pub(crate) capture: Option<SleepCapture>,
        microphone: CaptureControl,
        pub(crate) bus: Bus,
        root: std::path::PathBuf,
        feeding: Arc<AtomicBool>,
        value: Arc<std::sync::atomic::AtomicI32>,
        worker: Option<thread::JoinHandle<()>>,
        feeder: Option<thread::JoinHandle<()>>,
        _directory: SessionDirectory,
    }

    impl Rig {
        pub(crate) fn new(mode: Mode, readers: &[&str]) -> (Self, Vec<Subscriber>) {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let root = std::path::PathBuf::from(format!("/tmp/sc-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let bus = Bus::new(&root).unwrap();
            // The readers subscribe first, as lamp-name does at its start.
            let subscribers = readers
                .iter()
                .map(|name| {
                    bus.subscribe_typed(Audio::SLEEP_TOPIC, name, Audio::TYPE)
                        .unwrap()
                })
                .collect();
            let device = format!("sleep-capture-{}-{n}", std::process::id());
            let microphone = CaptureControl::register(&device);
            let directory = SessionDirectory::create().unwrap();
            let (mut worker, peer, input) = Worker::cooperative_fixture(&directory.path, "sleep");
            worker.end_on_stop(input);
            let controller = new_boot().unwrap();
            let thread = thread::spawn(move || {
                let _ = crate::sleep_worker::run::<ScriptedCapture>(peer, controller, &device);
            });
            // Its Ready, as a spawn reads it before handing the worker over.
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                match worker.channels.control.receive::<WorkerEvent>().unwrap() {
                    Some(WorkerEvent::Ready) => break,
                    Some(other) => panic!("before Ready: {other:?}"),
                    None => assert!(Instant::now() < until, "no Ready"),
                }
                thread::sleep(Duration::from_millis(1));
            }
            let capture =
                SleepCapture::with_worker(worker, None, controller, bus.clone(), mode).unwrap();
            let feeding = Arc::new(AtomicBool::new(true));
            let value = Arc::new(std::sync::atomic::AtomicI32::new(1111));
            let feeder = {
                let (feeding, value, microphone) =
                    (feeding.clone(), value.clone(), microphone.clone());
                thread::spawn(move || {
                    while feeding.load(Ordering::SeqCst) {
                        let sample = value.load(Ordering::SeqCst) as i16;
                        microphone.push(&[sample; crate::sleep_worker::BLOCK_SAMPLES]);
                        thread::sleep(Duration::from_millis(20));
                    }
                })
            };
            (
                Self {
                    capture: Some(capture),
                    microphone,
                    bus,
                    root,
                    feeding,
                    value,
                    worker: Some(thread),
                    feeder: Some(feeder),
                    _directory: directory,
                },
                subscribers,
            )
        }

        /// Whether its worker's loop has ended: the capture is closed. The
        /// fixture's exit is said on its Stop, a moment before its thread
        /// returns (a busy ARM64 runner saw the gap), so its end is waited
        /// for. Nothing else sends a Stop while the wait runs, so a loop
        /// that ends here was stopped by the look before this one.
        pub(crate) fn worker_ended(&self) -> bool {
            let until = Instant::now() + Duration::from_secs(2);
            loop {
                let ended = self
                    .worker
                    .as_ref()
                    .is_none_or(thread::JoinHandle::is_finished);
                if ended || Instant::now() >= until {
                    return ended;
                }
                thread::sleep(Duration::from_millis(1));
            }
        }

        fn capture(&mut self) -> &mut SleepCapture {
            self.capture.as_mut().unwrap()
        }

        /// Looks for `long`, the slide as `open` says, until one finds
        /// something.
        fn looks(&mut self, open: bool, long: Duration) -> Found {
            let until = Instant::now() + long;
            while Instant::now() < until {
                let found = self.capture().look(open).unwrap();
                if found != Found::Nothing {
                    return found;
                }
                thread::sleep(Duration::from_millis(3));
            }
            Found::Nothing
        }

        fn close(&mut self) {
            if let Some(capture) = self.capture.take() {
                capture.close(&mut || Ok(())).unwrap();
            }
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            self.close();
            self.feeding.store(false, Ordering::SeqCst);
            if let Some(feeder) = self.feeder.take() {
                let _ = feeder.join();
            }
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// Each block heard by `reader` now, by its first sample.
    fn heard(reader: &mut Subscriber) -> Vec<i16> {
        let mut values = Vec::new();
        while let Ok(Some(message)) = reader.recv(Duration::ZERO) {
            let (header, samples) = Audio::split(&message.payload).unwrap();
            assert_eq!((header.sample_rate, header.channels), (16_000, 1));
            assert_eq!(samples.len(), crate::sleep_worker::BLOCK_SAMPLES * 2);
            values.push(i16::from_le_bytes([samples[0], samples[1]]));
        }
        values
    }

    /// Reviewer-three's first condition: a slide that closes while asleep
    /// and hearing its name revokes the grant at the look that reads it;
    /// the worker closes the microphone and ends well within the main
    /// capture's authority lease (250 ms), and no block is published on
    /// `mic2/sleep_audio` after the revocation.
    #[test]
    fn a_slide_closing_stops_the_sleep_capture_and_no_block_is_published_after_it() {
        let (mut rig, mut readers) = Rig::new(Mode::On, &["name"]);
        assert_eq!(rig.looks(true, Duration::from_millis(300)), Found::Nothing);
        assert!(rig.capture().granted());
        assert!(
            !heard(&mut readers[0]).is_empty(),
            "no block reached the name listener"
        );
        let closed = Instant::now();
        rig.capture().slide(false);
        assert!(!rig.capture().granted());
        // Whatever was published before the revocation is taken away now.
        let _ = heard(&mut readers[0]);
        let found = rig.looks(false, Duration::from_millis(500));
        assert!(matches!(found, Found::Ended(_)), "{found:?}");
        assert!(
            closed.elapsed() < Duration::from_micros(lamp_turn_owner::MAX_AUTHORITY_LEASE_US),
            "ended {} ms after the slide closed",
            closed.elapsed().as_millis()
        );
        let (_, _, discards, _) = rig.microphone.counts();
        assert!(discards >= 1, "the microphone was not closed");
        rig.close();
        thread::sleep(Duration::from_millis(60));
        assert_eq!(heard(&mut readers[0]), Vec::<i16>::new());
    }

    /// The second: a reader that is not the name listener stops the
    /// capture, said once, and the sleep stays the full sleep of today
    /// (not started again) until the lamp wakes.
    #[test]
    fn another_reader_stops_the_sleep_capture_and_the_sleep_stays_full() {
        let (mut rig, _readers) = Rig::new(Mode::On, &["name"]);
        assert_eq!(rig.looks(true, Duration::from_millis(100)), Found::Nothing);
        let _other = rig
            .bus
            .subscribe_typed(Audio::SLEEP_TOPIC, "ears", Audio::TYPE)
            .unwrap();
        // Through the wait's own keeper: closed at the look that finds the
        // other reader, and not started again while the same sleep lasts.
        let starts = Arc::new(AtomicU64::new(0));
        let counted = starts.clone();
        let mut name = SleepName::new(
            Mode::On,
            Box::new(move |_, _| {
                counted.fetch_add(1, Ordering::SeqCst);
                Err(io::Error::other("not started in this test").into())
            }),
        );
        name.capture = rig.capture.take();
        let until = Instant::now() + Duration::from_millis(500);
        while name.capture.is_some() && Instant::now() < until {
            name.look(true, &mut || Ok(())).unwrap();
            thread::sleep(Duration::from_millis(3));
        }
        assert!(!name.hearing() && name.capture.is_none());
        assert!(
            name.refused
                .as_deref()
                .is_some_and(|why| why.contains("ears") && why.contains(Audio::SLEEP_TOPIC)),
            "{:?}",
            name.refused
        );
        name.want(true, true, &mut || Ok(())).unwrap();
        assert_eq!(
            starts.load(Ordering::SeqCst),
            0,
            "started again while refused"
        );
        // A new sleep may try again.
        name.want(false, true, &mut || Ok(())).unwrap();
        name.want(true, true, &mut || Ok(())).unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    /// The third: while asleep and listening, of the sound topics only
    /// `mic2/sleep_audio` is published, and nothing is on `mic2/audio`
    /// (`voice/mute`, `voice/privacy` and the wait's health are the wait's
    /// own, unchanged by this).
    #[test]
    fn while_asleep_it_publishes_only_its_sleep_audio_and_nothing_on_mic2_audio() {
        let (mut rig, _readers) = Rig::new(Mode::On, &["name"]);
        let mut main = rig
            .bus
            .subscribe_typed("mic2/audio", "name", Audio::TYPE)
            .unwrap();
        assert_eq!(rig.looks(true, Duration::from_millis(300)), Found::Nothing);
        let published: Vec<String> = rig
            .bus
            .topics()
            .unwrap()
            .into_iter()
            .filter(|topic| !rig.bus.publishers(&topic.name).unwrap().is_empty())
            .map(|topic| topic.name)
            .collect();
        assert_eq!(published, vec![Audio::SLEEP_TOPIC.to_owned()]);
        assert!(main.recv(Duration::ZERO).unwrap().is_none());
    }

    pub(crate) fn say_name(bus: &Bus, publisher: &mut Option<Publisher>, score: f32) {
        let publisher = publisher.get_or_insert_with(|| {
            bus.publisher_declared(NameHeard::TOPIC, &NameHeard::declaration(), "name")
                .unwrap()
        });
        publisher
            .publish_json(1, &NameHeard { score, ago_ms: 0 })
            .unwrap();
    }

    /// The third condition's wake: one hearing does not wake, two do, and
    /// the capture and its grant are closed before the wake is returned.
    /// In shadow nothing wakes.
    #[test]
    fn two_hearings_wake_it_closed_first_and_shadow_never_wakes() {
        for mode in [Mode::On, Mode::Shadow] {
            let (mut rig, _readers) = Rig::new(mode, &["name"]);
            let mut name_out = None;
            assert_eq!(rig.looks(true, Duration::from_millis(60)), Found::Nothing);
            let mut name = SleepName::new(mode, Box::new(|_, _| unreachable!()));
            name.capture = rig.capture.take();
            say_name(&rig.bus, &mut name_out, 0.92);
            thread::sleep(Duration::from_millis(20));
            assert_eq!(name.look(true, &mut || Ok(())).unwrap(), None);
            assert!(name.hearing());
            say_name(&rig.bus, &mut name_out, 0.93);
            thread::sleep(Duration::from_millis(20));
            let woke = name.look(true, &mut || Ok(())).unwrap();
            match mode {
                Mode::On => {
                    assert!(woke.is_some());
                    assert!(
                        !name.hearing() && name.capture.is_none(),
                        "closed after the wake"
                    );
                }
                _ => {
                    assert_eq!(woke, None);
                    assert!(name.hearing());
                    rig.capture = name.capture.take();
                }
            }
        }
    }

    /// The fourth: nothing heard while asleep reaches the woken turn. The
    /// room's sound and the name go to the bus only; the woken cycle's
    /// detector, as a new rig makes it, starts empty, and the first line's
    /// request carries only what was heard after the wake.
    #[test]
    fn nothing_heard_asleep_reaches_the_first_turn_after_the_wake() {
        let (mut rig, mut readers) = Rig::new(Mode::On, &["name"]);
        let mut name_out = None;
        // The room, asleep, then the name, until the listener has heard both.
        let mut asleep = Vec::new();
        let until = Instant::now() + Duration::from_secs(3);
        while !asleep.contains(&1111) && Instant::now() < until {
            assert_eq!(rig.looks(true, Duration::from_millis(20)), Found::Nothing);
            asleep.extend(heard(&mut readers[0]));
        }
        rig.value.store(2222, Ordering::SeqCst);
        while !asleep.contains(&2222) && Instant::now() < until {
            assert_eq!(rig.looks(true, Duration::from_millis(20)), Found::Nothing);
            asleep.extend(heard(&mut readers[0]));
        }
        say_name(&rig.bus, &mut name_out, 0.95);
        say_name(&rig.bus, &mut name_out, 0.95);
        let woke = rig.looks(true, Duration::from_millis(300));
        assert!(matches!(woke, Found::Woke(_)), "{woke:?}");
        rig.close();
        asleep.extend(heard(&mut readers[0]));
        assert!(
            asleep.contains(&1111) && asleep.contains(&2222),
            "{asleep:?}"
        );
        // The woken cycle: a new detector, fed only its own capture.
        let mut detector = TurnDetector::default();
        let mut started = None;
        for sequence in 1..=40u64 {
            if let Activity::Start(prefix) = detector.push(
                ObservedAudio {
                    sequence,
                    captured_at_us: sequence * 10_000,
                    samples: [3333; CAPTURE_SAMPLES],
                },
                1.0,
            ) {
                started = Some(prefix);
                break;
            }
        }
        let prefix = started.expect("the line began");
        assert!(
            prefix
                .iter()
                .all(|block| block.samples.iter().all(|sample| *sample == 3333))
        );
        let _ = DeviceState::Running;
    }
}
