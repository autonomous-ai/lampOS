//! The real voice's speaker on the bus, for the local route: lamp-voice with
//! `--speaker-device bus:speaker1` publishes what it plays on
//! [`TOPIC`] (`speaker1/play`: `lamp_msgs::Audio` header and `json+pcm`, 24 kHz mono S16LE,
//! 10 to 100 ms a block, at the voice's own real-time pace), the mirror of
//! `bus:mic1`, and lamp-sim plays it on the host through the stand-in's
//! player (`paplay`, `aplay`, else `ffplay`). The bus has no back-pressure:
//! a block the player's queue has no room for is counted, never waited on.
//! `LAMP_SIM_SPEAKER=silent` counts and plays nothing. Each spoken run ends
//! with one line: its blocks, milliseconds, dropped blocks and gaps.
use crate::voice::{PLAYER_QUEUED, end_player, speaker_player};
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
/// How long a receive waits before the run's quiet is looked at.
const WAIT: Duration = Duration::from_millis(50);
/// No block for this long ends a run (and says its line).
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

impl Counter {
    /// The samples of `payload` when it is a block of the speaker's sound
    /// (24 kHz, one channel, S16LE, whole samples); anything else is passed over.
    fn take<'a>(&mut self, payload: &'a [u8]) -> Option<&'a [u8]> {
        let (header, bytes) = Audio::split(payload).ok()?;
        if header.sample_rate != Audio::SPEAKER_SAMPLE_RATE
            || header.channels != 1
            || header.sample_format != Audio::S16LE
            || bytes.len() % 2 != 0
        {
            return None;
        }
        if let Some(next) = self.next
            && header.sequence > next
        {
            self.played.gaps += header.sequence - next;
        }
        self.next = Some(header.sequence + 1);
        self.played.blocks += 1;
        self.played.samples += (bytes.len() / 2) as u64;
        Some(bytes)
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
}

impl Player {
    fn start() -> Self {
        let mut child = speaker_player();
        let (feed, writer) = match child.as_mut().and_then(|child| child.stdin.take()) {
            Some(mut stdin) => {
                let (feed, queued) = mpsc::sync_channel::<Vec<u8>>(PLAYER_QUEUED);
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
        }
    }

    /// Hands `bytes` to the player: false when its queue is full.
    fn feed(&mut self, bytes: &[u8]) -> bool {
        match &self.feed {
            Some(queue) => match queue.try_send(bytes.to_vec()) {
                Ok(()) => true,
                Err(mpsc::TrySendError::Full(_)) => false,
                // The player ended: counted from now on, and started again after the run.
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    self.feed = None;
                    true
                }
            },
            None => true,
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

/// Plays the voice's speaker from `bus`, forever. It subscribes before the
/// voice starts (the bus takes a topic nobody has declared yet, and the
/// voice's publisher finds the subscription when it does), and subscribes
/// again if the bus fails.
pub fn run(bus: &Bus) {
    let mut player = Player::start();
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
        let mut counter = Counter::default();
        let mut last: Option<Instant> = None;
        loop {
            match blocks.recv(WAIT) {
                Ok(Some(message)) => {
                    if let Some(bytes) = counter.take(&message.payload) {
                        if !player.feed(bytes) {
                            counter.played.dropped += 1;
                        }
                        last = Some(Instant::now());
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!(
                        "lamp-sim: the voice's speaker lost the bus ({error}); looking again"
                    );
                    break;
                }
            }
            if last.is_some_and(|at| at.elapsed() >= QUIET) {
                last = None;
                let run = counter.end_run();
                eprintln!("lamp-sim: {}", run_line(&run));
                if player.gone() {
                    player = Player::start();
                }
            }
        }
    }
}

/// The one line a run ends with.
fn run_line(run: &Played) -> String {
    format!(
        "the voice's speaker played {} ms in {} blocks{}{}",
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
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(rate: u32, channels: u8, sequence: u64, bytes: usize) -> Vec<u8> {
        Audio {
            sample_rate: rate,
            channels,
            sample_format: Audio::S16LE.to_owned(),
            sequence,
        }
        .block(&vec![0u8; bytes])
        .unwrap()
    }

    #[test]
    fn only_the_speakers_sound_counts_and_a_sequence_skip_is_a_gap() {
        let mut counter = Counter::default();
        // 100 ms and 20 ms of 24 kHz mono S16LE.
        assert_eq!(
            counter.take(&block(24_000, 1, 0, 4_800)).unwrap().len(),
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
                gaps: 2
            }
        );
        assert_eq!(run.ms(), 220);
        // A new run starts afresh: its first block is no gap, whatever its sequence.
        assert!(counter.take(&block(24_000, 1, 40, 480)).is_some());
        assert_eq!(counter.end_run().gaps, 0);
    }

    #[test]
    fn the_runs_line_says_what_was_played_and_only_the_trouble_there_was() {
        let clean = Played {
            blocks: 15,
            samples: 36_000,
            dropped: 0,
            gaps: 0,
        };
        assert_eq!(
            run_line(&clean),
            "the voice's speaker played 1500 ms in 15 blocks"
        );
        let rough = Played {
            dropped: 2,
            gaps: 1,
            ..clean
        };
        assert_eq!(
            run_line(&rough),
            "the voice's speaker played 1500 ms in 15 blocks, 2 dropped (the player was behind), 1 missing on the bus"
        );
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
