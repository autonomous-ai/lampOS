//! The real voice's speaker on the bus: lamp-voice with
//! `--speaker-device bus:speaker1` (lamp-sim passes it when
//! `LAMP_SIM_VOICE_SPEAKER=bus:speaker1`, Linux only) publishes what it plays on
//! [`TOPIC`] (`speaker1/play`: `lamp_msgs::Audio` header and `json+pcm`, 24 kHz mono S16LE,
//! up to 10 ms a block, one per write the voice's queue takes, at the voice's own real-time pace), the mirror of
//! `bus:mic1`, and lamp-sim plays it on the host through the stand-in's
//! player (`paplay`, `aplay`, else `ffplay`). The bus has no back-pressure:
//! a block the player's queue has no room for is counted, never waited on.
//! What is handed to the player when the voice stops (a cut-in, a privacy
//! stop) is still played, so lamp-sim keeps at most [`AHEAD`] of sound handed
//! out ahead of the clock (the voice's own queue is 40 ms): a block past that
//! is dropped and counted. The queue in front of the player is bounded too
//! ([`QUEUED`] blocks), but the pipe to the player holds far more (64 KiB, about
//! 0.7 s), so the clock is what keeps a stop on time. The trade-off: after a
//! stall (a held-up receive catching up), the blocks past the cap are dropped,
//! so that moment is heard as a hole, never as late sound. A block longer than
//! [`AHEAD`] would never fit while anything is queued; the voice's are 10 ms.
//! The player is started once, when lamp-sim starts, and kept for the lamp's
//! life (an idle stream on the host's sound server), so a run's first block is
//! not held up by a player's start.
//! Between answers the voice's clock keeps running and it writes a zero block
//! every 10 ms tick, so the topic never goes quiet: an all-zero block is fed to
//! the player (it keeps the stream's timing) but is not the run's sound, and a
//! run ends after [`QUIET`] with only zero blocks (or none).
//! `LAMP_SIM_SPEAKER=silent` counts and plays nothing. Each spoken run ends
//! with one line: its blocks, milliseconds, dropped blocks and gaps.
//! The code is sim-3's (the local route's speaker), taken alone.
use crate::voice::{end_player, speaker_player};
use lamp_bus::Bus;
use lamp_msgs::Audio;
use std::{
    io::Write,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// The voice's played sound: `lamp_msgs`' own name for it, which lamp-voice
/// publishes with `--speaker-device bus:speaker1` (#7020).
pub const TOPIC: &str = Audio::PLAY_TOPIC;
/// The `--speaker-device` that puts the voice's sound on the bus, and the
/// `LAMP_SIM_VOICE_SPEAKER` value that asks lamp-sim to play it.
pub const DEVICE: &str = "bus:speaker1";
/// Blocks queued for the player at most: about 80 ms of the voice's 10 ms
/// blocks. A bigger queue plays a stop late by what it holds.
pub const QUEUED: usize = 8;
/// Sound handed to the player ahead of the clock at most: the voice's 40 ms
/// lead and room for a late receive. A block past it is dropped, so a stop is
/// never played later than this.
pub const AHEAD: Duration = Duration::from_millis(100);
/// How long a receive waits before the run's quiet is looked at.
const WAIT: Duration = Duration::from_millis(50);
/// No block of sound (only zero blocks, or none) for this long ends a run (and says its line).
const QUIET: Duration = Duration::from_secs(1);
/// How long to wait before subscribing again after the bus refused or failed.
const RETRY_WAIT: Duration = Duration::from_millis(500);

/// What one run of the voice's sound was.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Played {
    pub blocks: u64,
    pub samples: u64,
    /// Blocks the player's queue had no room for.
    pub dropped: u64,
    /// Sequence skips: blocks the bus never gave.
    pub gaps: u64,
    /// Blocks taken after the player had ended: counted, never played.
    pub unplayed: u64,
}

/// Whether `speaker` (`LAMP_SIM_VOICE_SPEAKER`) asks for the voice's sound on the bus.
pub fn wanted(speaker: Option<&str>) -> bool {
    speaker == Some(DEVICE)
}

impl Played {
    pub fn ms(&self) -> u64 {
        self.samples * 1000 / u64::from(Audio::SPEAKER_SAMPLE_RATE)
    }
}

/// Counts the blocks of a run and says which are the speaker's sound.
#[derive(Default)]
struct Counter {
    played: Played,
    next: Option<u64>,
}

/// One block of the speaker's stream.
struct Block<'a> {
    bytes: &'a [u8],
    /// Not all zeros: the run's sound. The voice's clock writes zero blocks between answers.
    sound: bool,
}

impl Counter {
    /// The block in `payload` when it is the speaker's stream (24 kHz, one channel, S16LE,
    /// whole samples); anything else is passed over. Only a block of sound counts as the run's;
    /// a sequence skip counts as a gap during a run or next to sound, never in the silence between.
    fn take<'a>(&mut self, payload: &'a [u8]) -> Option<Block<'a>> {
        let (header, bytes) = Audio::split(payload).ok()?;
        if header.sample_rate != Audio::SPEAKER_SAMPLE_RATE
            || header.channels != 1
            || header.sample_format != Audio::S16LE
            || bytes.len() % 2 != 0
        {
            return None;
        }
        let sound = bytes.iter().any(|byte| *byte != 0);
        if (sound || self.played.blocks > 0)
            && let Some(next) = self.next
            && header.sequence > next
        {
            self.played.gaps += header.sequence - next;
        }
        self.next = Some(header.sequence + 1);
        if sound {
            self.played.blocks += 1;
            self.played.samples += (bytes.len() / 2) as u64;
        }
        Some(Block { bytes, sound })
    }

    /// The run's counts, and a fresh start for the next.
    fn end_run(&mut self) -> Played {
        self.next = None;
        std::mem::take(&mut self.played)
    }
}

/// The host's player, fed by a thread of its own through a bounded queue:
/// a player that stops reading holds that thread, never the bus.
struct Player {
    child: Option<std::process::Child>,
    feed: Option<mpsc::SyncSender<Vec<u8>>>,
    writer: Option<thread::JoinHandle<()>>,
    /// When what was handed out ends if played at the clock's pace.
    plays_until: Option<Instant>,
    /// How far ahead of the clock it may end.
    ahead: Duration,
}

/// What became of a block handed to the player.
#[derive(Debug, PartialEq, Eq)]
enum Fed {
    /// Queued for the player.
    Queued,
    /// The queue was full, or the block would end more than [`AHEAD`] past the clock: dropped.
    Full,
    /// No player to play it (silent, or none found): counted only.
    Counted,
    /// The player had ended: not played.
    Ended,
}

impl Player {
    fn start(queued: usize, ahead: Duration) -> Self {
        let mut child = speaker_player();
        let (feed, writer) = match child.as_mut().and_then(|child| child.stdin.take()) {
            Some(mut stdin) => {
                let (feed, queued) = mpsc::sync_channel::<Vec<u8>>(queued);
                let writer = thread::spawn(move || {
                    for bytes in queued {
                        if stdin.write_all(&bytes).is_err() {
                            break;
                        }
                    }
                });
                (Some(feed), Some(writer))
            }
            None => (None, None),
        };
        Self {
            child,
            feed,
            writer,
            plays_until: None,
            ahead,
        }
    }

    /// Hands `bytes` to the player, never waiting.
    fn feed(&mut self, bytes: &[u8]) -> Fed {
        let now = Instant::now();
        let from = self.plays_until.filter(|at| *at > now).unwrap_or(now);
        let ends = from + sound_length(bytes);
        match &self.feed {
            Some(_) if ends > now + self.ahead => Fed::Full,
            Some(queue) => match queue.try_send(bytes.to_vec()) {
                Ok(()) => {
                    self.plays_until = Some(ends);
                    Fed::Queued
                }
                Err(mpsc::TrySendError::Full(_)) => Fed::Full,
                // The player ended: not played from now on, and started again after the run.
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    self.feed = None;
                    Fed::Ended
                }
            },
            None if self.gone() => Fed::Ended,
            None => Fed::Counted,
        }
    }

    /// Whether a player was found but has since ended.
    fn gone(&self) -> bool {
        self.child.is_some() && self.feed.is_none()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.feed = None;
        if let Some(child) = self.child.take() {
            end_player(child, self.writer.take());
        }
    }
}

/// The speaker's stream cut into runs: a run begins with a block of sound and ends after
/// [`QUIET`] with no sound (only zero blocks, or nothing at all).
#[derive(Default)]
struct Runs {
    counter: Counter,
    /// When the run's last block of sound came; none between runs.
    last: Option<Instant>,
}

impl Runs {
    /// One message of the stream at `now`: played (silence too, it keeps the stream's timing),
    /// and counted when it is sound.
    fn block(&mut self, payload: &[u8], player: &mut Player, now: Instant) {
        let Some(block) = self.counter.take(payload) else {
            return;
        };
        let fed = player.feed(block.bytes);
        if block.sound {
            match fed {
                Fed::Full => self.counter.played.dropped += 1,
                Fed::Ended => self.counter.played.unplayed += 1,
                Fed::Queued | Fed::Counted => {}
            }
            self.last = Some(now);
        }
    }

    /// The run that ended by `now`, once.
    fn ended(&mut self, now: Instant) -> Option<Played> {
        let last = self.last?;
        if now.duration_since(last) < QUIET {
            return None;
        }
        self.last = None;
        Some(self.counter.end_run())
    }
}

/// How long `bytes` of 24 kHz mono S16LE play.
fn sound_length(bytes: &[u8]) -> Duration {
    Duration::from_micros(
        bytes.len() as u64 / 2 * 1_000_000 / u64::from(Audio::SPEAKER_SAMPLE_RATE),
    )
}

/// Plays the voice's speaker from `bus`, forever. It subscribes before the
/// voice starts (the bus takes a topic nobody has declared yet, and the
/// voice's publisher finds the subscription when it does), and subscribes
/// again if the bus fails.
pub fn run(bus: &Bus) {
    run_with(bus, QUEUED, AHEAD);
}

/// [`run`] with a player queue of `queued` blocks and `ahead` of sound ahead of the clock at most.
pub fn run_with(bus: &Bus, queued: usize, ahead: Duration) {
    let mut player = Player::start(queued, ahead);
    if player.child.is_none() {
        eprintln!(
            "lamp-sim: the voice's speaker is counted, not played (LAMP_SIM_SPEAKER=silent, or no paplay, aplay or ffplay)"
        );
    }
    loop {
        let mut blocks = match bus.subscribe_typed(TOPIC, "sim_speaker1", Audio::TYPE) {
            Ok(blocks) => blocks,
            Err(error) => {
                eprintln!("lamp-sim: the voice's speaker cannot subscribe ({error}); trying again");
                thread::sleep(RETRY_WAIT);
                continue;
            }
        };
        let mut runs = Runs::default();
        loop {
            match blocks.recv(WAIT) {
                Ok(Some(message)) => runs.block(&message.payload, &mut player, Instant::now()),
                Ok(None) => {}
                Err(error) => {
                    eprintln!(
                        "lamp-sim: the voice's speaker lost the bus ({error}); looking again"
                    );
                    break;
                }
            }
            if let Some(run) = runs.ended(Instant::now()) {
                eprintln!("lamp-sim: {}", run_line(&run));
                if player.gone() {
                    player = Player::start(queued, ahead);
                }
            }
        }
    }
}

/// The one line a run ends with.
fn run_line(run: &Played) -> String {
    format!(
        "the voice's speaker played {} ms in {} blocks{}{}{}",
        run.ms(),
        run.blocks,
        if run.dropped > 0 {
            format!(", {} dropped (the player was behind)", run.dropped)
        } else {
            String::new()
        },
        if run.gaps > 0 {
            format!(", {} missing on the bus", run.gaps)
        } else {
            String::new()
        },
        if run.unplayed > 0 {
            format!(", {} not played (the player ended)", run.unplayed)
        } else {
            String::new()
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A block of `bytes` of sound (not silence).
    fn block(rate: u32, channels: u8, sequence: u64, bytes: usize) -> Vec<u8> {
        filled(rate, channels, sequence, bytes, 1)
    }

    /// A block of 24 kHz mono silence, as the voice's clock writes between answers.
    fn silence(sequence: u64) -> Vec<u8> {
        filled(24_000, 1, sequence, 480, 0)
    }

    fn filled(rate: u32, channels: u8, sequence: u64, bytes: usize, byte: u8) -> Vec<u8> {
        Audio {
            sample_rate: rate,
            channels,
            sample_format: Audio::S16LE.to_owned(),
            sequence,
        }
        .block(&vec![byte; bytes])
        .unwrap()
    }

    #[test]
    fn only_the_speakers_sound_counts_and_a_sequence_skip_is_a_gap() {
        let mut counter = Counter::default();
        // 100 ms and 20 ms of 24 kHz mono S16LE.
        assert_eq!(
            counter
                .take(&block(24_000, 1, 0, 4_800))
                .unwrap()
                .bytes
                .len(),
            4_800
        );
        assert!(counter.take(&block(24_000, 1, 1, 960)).is_some());
        // Not the speaker's: another rate, two channels, half a sample, no header.
        assert!(counter.take(&block(16_000, 1, 2, 640)).is_none());
        assert!(counter.take(&block(24_000, 2, 2, 640)).is_none());
        assert!(counter.take(&block(24_000, 1, 2, 641)).is_none());
        assert!(counter.take(b"no header").is_none());
        // Blocks 2 and 3 never came.
        assert!(counter.take(&block(24_000, 1, 4, 4_800)).is_some());
        let run = counter.end_run();
        assert_eq!(
            run,
            Played {
                blocks: 3,
                samples: 5_280,
                dropped: 0,
                gaps: 2,
                unplayed: 0,
            }
        );
        assert_eq!(run.ms(), 220);
        // A new run starts afresh: its first block is no gap, whatever its sequence.
        assert!(counter.take(&block(24_000, 1, 40, 480)).is_some());
        assert_eq!(counter.end_run().gaps, 0);
    }

    /// The voice's clock writes a zero block every tick between answers: they are taken (to be
    /// played) but are not the run's sound, a skip among them is no gap, and a restarted voice
    /// (its sequence back at 0) is no gap either.
    #[test]
    fn zero_blocks_are_silence_not_the_runs_sound() {
        let mut counter = Counter::default();
        for sequence in 0..50 {
            let payload = silence(sequence);
            let taken = counter.take(&payload).unwrap();
            assert!(!taken.sound);
            assert_eq!(taken.bytes.len(), 480);
        }
        // A block lost in the silence is not the run's trouble.
        assert!(!counter.take(&silence(52)).unwrap().sound);
        assert_eq!(counter.played, Played::default());
        // An answer: 3 blocks of sound, a pause of silence inside it, 1 more block of sound.
        for sequence in 53..56 {
            assert!(
                counter
                    .take(&block(24_000, 1, sequence, 480))
                    .unwrap()
                    .sound
            );
        }
        assert!(!counter.take(&silence(56)).unwrap().sound);
        assert!(counter.take(&block(24_000, 1, 57, 480)).unwrap().sound);
        // A skip inside the run is a gap.
        assert!(counter.take(&silence(59)).is_some());
        let run = counter.end_run();
        assert_eq!((run.blocks, run.ms(), run.gaps), (4, 40, 1));
        // The voice restarted: its sequence begins at 0 again; no gap.
        assert!(counter.take(&silence(0)).is_some());
        assert!(counter.take(&block(24_000, 1, 1, 480)).is_some());
        assert_eq!(counter.end_run().gaps, 0);
    }

    #[test]
    fn the_runs_line_says_what_was_played_and_only_the_trouble_there_was() {
        let clean = Played {
            blocks: 15,
            samples: 36_000,
            dropped: 0,
            gaps: 0,
            unplayed: 0,
        };
        assert_eq!(
            run_line(&clean),
            "the voice's speaker played 1500 ms in 15 blocks"
        );
        let rough = Played {
            dropped: 2,
            gaps: 1,
            unplayed: 3,
            ..clean
        };
        assert_eq!(
            run_line(&rough),
            "the voice's speaker played 1500 ms in 15 blocks, 2 dropped (the player was behind), 1 missing on the bus, 3 not played (the player ended)"
        );
    }

    /// A player that stops reading fills its queue: the next block is dropped at once, never waited on.
    /// A player that ended is said as not played; no player at all (silent) is only counted.
    #[test]
    fn a_full_queue_drops_at_once_and_an_ended_player_is_not_played() {
        let (feed, held) = mpsc::sync_channel::<Vec<u8>>(QUEUED);
        // An hour ahead: only the queue's bound counts here.
        let mut stuck = Player {
            child: None,
            feed: Some(feed),
            writer: None,
            plays_until: None,
            ahead: Duration::from_secs(3600),
        };
        for _ in 0..QUEUED {
            assert_eq!(stuck.feed(&[0; 480]), Fed::Queued);
        }
        let at = Instant::now();
        assert_eq!(stuck.feed(&[0; 480]), Fed::Full);
        assert!(
            at.elapsed() < Duration::from_millis(50),
            "{:?}",
            at.elapsed()
        );
        drop(held);
        assert_eq!(stuck.feed(&[0; 480]), Fed::Ended);
        let mut silent = Player {
            child: None,
            feed: None,
            writer: None,
            plays_until: None,
            ahead: AHEAD,
        };
        assert_eq!(silent.feed(&[0; 480]), Fed::Counted);
    }

    /// With the voice's clock running, zero blocks keep coming between answers: the run still
    /// ends QUIET after its last sound, says only the sound, and silence alone starts no run.
    #[test]
    fn a_run_ends_after_quiet_of_only_zero_blocks() {
        let mut player = Player {
            child: None,
            feed: None,
            writer: None,
            plays_until: None,
            ahead: AHEAD,
        };
        let mut runs = Runs::default();
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        // Silence only, for 2 s: no run.
        for tick in 0..200 {
            runs.block(&silence(tick), &mut player, at(tick * 10));
            assert!(runs.ended(at(tick * 10)).is_none());
        }
        // 0.5 s of sound, then the clock's zero blocks for 2 s.
        for tick in 200..250 {
            runs.block(&block(24_000, 1, tick, 480), &mut player, at(tick * 10));
        }
        let mut ended = None;
        for tick in 250..450 {
            runs.block(&silence(tick), &mut player, at(tick * 10));
            if let Some(run) = runs.ended(at(tick * 10)) {
                assert!(ended.is_none(), "one line a run");
                ended = Some((tick, run));
            }
        }
        let (tick, run) = ended.expect("the run ended though zero blocks kept coming");
        // The last sound was at 2490 ms: the run ends at 3490 ms, QUIET later.
        assert_eq!(tick * 10, 2490 + QUIET.as_millis() as u64);
        assert_eq!((run.blocks, run.ms(), run.gaps), (50, 500, 0));
    }

    /// A burst (a held-up receive catching up) is played only up to AHEAD past the clock; the rest is dropped,
    /// so a stop right after it is not played late. Sound at the clock's pace is all taken.
    #[test]
    fn sound_more_than_ahead_of_the_clock_is_dropped() {
        let (feed, _held) = mpsc::sync_channel::<Vec<u8>>(1000);
        let mut player = Player {
            child: None,
            feed: Some(feed),
            writer: None,
            plays_until: None,
            ahead: AHEAD,
        };
        // 300 ms at once, in 10 ms blocks: the first 100 ms are taken, the rest dropped.
        let fed: Vec<Fed> = (0..30).map(|_| player.feed(&[0; 480])).collect();
        let taken = fed.iter().filter(|f| **f == Fed::Queued).count();
        assert_eq!(taken, 10, "{fed:?}");
        assert!(fed[taken..].iter().all(|f| *f == Fed::Full));
        // Once the clock has caught up, the next block is taken again.
        thread::sleep(Duration::from_millis(110));
        assert_eq!(player.feed(&[0; 480]), Fed::Queued);
    }

    #[test]
    fn only_bus_speaker1_asks_for_the_voice_on_the_bus() {
        assert!(wanted(Some("bus:speaker1")));
        for other in [
            None,
            Some("default"),
            Some("bus:speaker2"),
            Some("bus:speaker1 "),
        ] {
            assert!(!wanted(other), "{other:?}");
        }
    }

    #[test]
    fn the_voices_blocks_are_taken_from_a_subscription_made_before_the_voice_declared_the_topic() {
        let dir = crate::test_root().join(format!("lsm-bus-speaker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bus = Bus::new(dir.join("bus")).unwrap();
        // lamp-sim subscribes first, as run() does before the voice starts; then the voice declares and publishes.
        let mut blocks = bus
            .subscribe_typed(TOPIC, "sim_speaker1", Audio::TYPE)
            .unwrap();
        let mut voice = bus
            .publisher_declared(TOPIC, &Audio::speaker_declaration(), "voice")
            .unwrap();
        for sequence in 0..3 {
            voice
                .publish_bytes(
                    lamp_ipc::monotonic_us(),
                    Audio::ENCODING,
                    &block(24_000, 1, sequence, 2_400),
                )
                .unwrap();
        }
        let mut counter = Counter::default();
        let until = Instant::now() + Duration::from_secs(2);
        while counter.played.blocks < 3 && Instant::now() < until {
            if let Ok(Some(message)) = blocks.recv(WAIT) {
                counter.take(&message.payload);
            }
        }
        assert_eq!(counter.end_run().ms(), 150);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
