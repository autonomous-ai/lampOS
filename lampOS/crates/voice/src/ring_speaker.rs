//! The speaker muted or at 0, on the ring. The product owner, 27 Nov 2026,
//! after a test lamp's speaker sat at 0% for hours and the lamp heard and
//! answered eight calls nobody heard, with nothing on the lamp to show it:
//! "when lamp is on mute, we should have a different color code or
//! something on the lamp so people know."
//!
//! With `LAMP_RING_SPEAKER_MUTED=on`, which the voice hands its ring worker
//! alone, the ring reads what lamp-actions, the speaker's only writer, says
//! of it, and while the speaker is muted or at 0 the face shows it
//! ([`crate::ring_face::Muted`]): the speaking face in the muted colour
//! ([`crate::ring_face::MUTED_TINT`]), one swell of it when lamp-name hears
//! the lamp's name (`name/heard`), so a caller sees it heard them though its
//! voice is off, and the resting face too while the person's light is off or
//! at level 1 (above it, the light they set wins). Heard again, the plain
//! faces are back with the next frame.
//!
//! What counts, the first that holds ([`speaker`]):
//! - a fresh `speaker/state` (`lamp_msgs::SpeakerState`: lamp-actions'
//!   read of the mixer about once a second, good for 3 s, on this boot's
//!   clock): muted when its level is 0, or when it says muted and knows no
//!   level; heard at a level over 0, even under a mute asked for, since that
//!   is what a person hears; nothing known when it knows no level. A level
//!   set to 0 outside lamp-actions, by `amixer`, is seen here, at its next
//!   read of the mixer;
//! - else `speaker/volume` (`lamp_msgs::SpeakerVolume`, kept by the bus,
//!   said at lamp-actions' start and after each change it makes), if it was
//!   said after the newest `speaker/state` or there is none: muted at 0. One
//!   said before is older than that state's word, which has lapsed;
//! - else nothing is known, and nothing is shown.
//!
//! A hearing counts while it is fresh, said under half a second before it is
//! read ([`hearing`]), by the clock its header calls for (`docs/bus.md`): a
//! header's `at_us` is its publisher's own unless it says this boot's clock,
//! and lamp-name's says none (its `at_us` counts from its own start), so its
//! wall stamp decides. The swell is dated by when it was read.
//!
//! Only the face changes: whether a frame may be written at all is the
//! ring's guard's, as for every face, so with the privacy switch closed the
//! ring writes none of it and the privacy worker's breath is the one shown;
//! that worker never reads the speaker. As with the ring's other senses
//! ([`crate::ring_head`], [`crate::ring_night`]), the bus is read by a
//! thread of its own, never by the ring's loop.
use crate::ring_face::Muted;
use lamp_bus::{Bus, Message, Now, StampClock, Subscriber};
use lamp_msgs::{NameHeard, SpeakerState, SpeakerVolume};
use std::{
    io,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// The switch: the speaker's mute shows on the ring only with `on`.
pub const ENVIRONMENT: &str = "LAMP_RING_SPEAKER_MUTED";

/// Whether the switch is on in this process's environment.
pub fn enabled() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// The bus's name for this reader of `name/heard`.
const SUBSCRIBER: &str = "ring_speaker";
/// lamp-name's name on the bus: a hearing from anyone else is passed over.
const NAME_SOURCE: &str = "name";
/// The thread waits this long for a hearing before it reads the speaker
/// again: a change of the speaker is seen within it, a hearing at once.
const LOOK_EVERY: Duration = Duration::from_millis(50);
/// A hearing older than this when read is passed over, so no swell comes
/// for a call long gone, or for the last one kept from before this ring.
const HEARING_LASTS_US: u64 = 500_000;
/// At most this many hearings are read in one look.
const MOST_HEARD: usize = 8;
/// What the thread found holds this long: a stalled thread's last word is
/// no word, and the face goes back to plain.
const FOUND_LASTS_US: u64 = 1_000_000;

/// What the ring knows of the speaker, and which word said it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speaker {
    /// Muted or at 0. `level` is what the word said, if any.
    Muted {
        said: &'static str,
        level: Option<u8>,
    },
    /// At a level over 0.
    Heard { said: &'static str, level: u8 },
    /// Nothing fresh, or newer, to go by: nothing is shown.
    Unknown,
}

impl Speaker {
    pub fn muted(self) -> bool {
        matches!(self, Self::Muted { .. })
    }

    /// The journal's words for it. The two topics' numbers are not one
    /// scale: `speaker/state`'s is the mixer's raw value, `speaker/volume`'s
    /// a percent of what the lamp allows, so each is said as what it is.
    fn told(self) -> String {
        let amount = |said: &str, level: u8| {
            if said == SpeakerVolume::TOPIC {
                format!("{level}%")
            } else {
                format!("raw {level}")
            }
        };
        match self {
            Self::Muted {
                said,
                level: Some(level),
            } => format!("muted ({said}, {})", amount(said, level)),
            Self::Muted { said, level: None } => format!("muted ({said}, no level read)"),
            Self::Heard { said, level } => format!("heard ({said}, {})", amount(said, level)),
            Self::Unknown => "not known".to_owned(),
        }
    }
}

/// The speaker as `speaker/state` and `speaker/volume`, the latest each
/// that the bus keeps, say it at `now_us` (the host's monotonic clock) and
/// `now_unix_ms`, on the host boot `clock_id`.
pub fn speaker(
    state: Option<&Message>,
    volume: Option<&Message>,
    clock_id: &str,
    now_us: u64,
    now_unix_ms: u64,
) -> Speaker {
    if let Some(fresh) = state
        .and_then(|message| SpeakerState::from_message(message, clock_id, now_us, now_unix_ms).ok())
    {
        let said = SpeakerState::TOPIC;
        return match fresh.volume_raw {
            Some(0) => Speaker::Muted {
                said,
                level: Some(0),
            },
            Some(level) => Speaker::Heard { said, level },
            None if fresh.muted => Speaker::Muted { said, level: None },
            None => Speaker::Unknown,
        };
    }
    // No fresh state: the volume counts only as the newest word. Both stamps
    // are lamp-actions' own, on this host's CLOCK_MONOTONIC (`speaker/state`
    // by its contract, whose stamp is its header's; `speaker/volume` as
    // `volume_out.rs` stamps it), so they order; no other part's are compared.
    let newest_state_us = state
        .filter(|message| message.header.source == SpeakerState::SOURCE)
        .map(|message| message.header.at_us);
    let Some(message) = volume.filter(|message| {
        message.header.source == SpeakerVolume::SOURCE
            && newest_state_us.is_none_or(|state_us| message.header.at_us > state_us)
    }) else {
        return Speaker::Unknown;
    };
    let said = SpeakerVolume::TOPIC;
    match message.parse::<SpeakerVolume>() {
        Ok(volume) if volume.check().is_err() => Speaker::Unknown,
        Ok(volume) if volume.muted || volume.volume == 0 => Speaker::Muted {
            said,
            level: Some(volume.volume),
        },
        Ok(volume) => Speaker::Heard {
            said,
            level: volume.volume,
        },
        Err(_) => Speaker::Unknown,
    }
}

/// How the face shows `speaker`: over the person's own light (`own_light`)
/// the resting face too; under a light someone asked for, the turns only.
pub fn shown(speaker: Speaker, own_light: bool) -> Muted {
    match (speaker.muted(), own_light) {
        (false, _) => Muted::No,
        (true, true) => Muted::AtRest,
        (true, false) => Muted::InTurns,
    }
}

/// Whether `message`, read when the reader's clocks say `now`, is a hearing
/// of the lamp's name to swell for: lamp-name's own, at least as sure as a
/// call ([`crate::called::LEAST_SCORE`]), and said within
/// [`HEARING_LASTS_US`]. How long ago it was said is judged by the clock its
/// header calls for (`docs/bus.md`, `Header::stamp_clock`): `at_us` only
/// when the header says this boot's CLOCK_MONOTONIC, since without a `clock`
/// it is its publisher's own (lamp-name's counts from its own start); else
/// its wall stamp, `at_unix_ms`. One from another boot, or ahead of now by
/// more than the bus's grace, is none. Its score, if so.
pub fn hearing(message: &Message, now: &Now<'_>) -> Option<f32> {
    let header = &message.header;
    if message.expired || header.source != NAME_SOURCE || header.ahead_of(now) {
        return None;
    }
    let said_us_ago = match header.stamp_clock(now) {
        StampClock::Monotonic => now.monotonic_us.saturating_sub(header.at_us),
        StampClock::Wall => now
            .unix_ms
            .saturating_sub(header.at_unix_ms)
            .saturating_mul(1_000),
        StampClock::OtherBoot => return None,
    };
    if said_us_ago > HEARING_LASTS_US {
        return None;
    }
    let heard = message.parse::<NameHeard>().ok()?;
    (heard.check().is_ok() && heard.score >= crate::called::LEAST_SCORE).then_some(heard.score)
}

/// What came of a frame the ring tried while a hearing's swell was due.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tried {
    /// Written, with the swell in it.
    Drawn,
    /// No swell shown, and why: the privacy switch, a computer's own pixels
    /// at rest, a warning drawn in its place, the night's asleep face.
    Held(&'static str),
}

/// What the journal is told of a hearing's swell (#8934's review): one line
/// once the first frame with it is written, or, when its second ends or the
/// ring stops with none written, that it was not drawn and why. Never
/// "one teal swell" for a swell nobody saw.
#[derive(Debug, Default)]
pub struct SwellSaid {
    due: Option<Due>,
}

#[derive(Debug)]
struct Due {
    score: f32,
    until_us: u64,
    /// Why the last frame tried had no swell drawn, if one was tried.
    held: Option<&'static str>,
}

impl SwellSaid {
    /// The face took a hearing of `score` at `at_us`: its swell is due for
    /// [`crate::ring_face::HEARD_US`]. One still due before it is said not
    /// drawn first.
    pub fn taken(&mut self, score: f32, at_us: u64) -> Option<String> {
        let before = self.stop();
        self.due = Some(Due {
            score,
            until_us: at_us.saturating_add(crate::ring_face::HEARD_US),
            held: None,
        });
        before
    }

    /// A frame was tried, as `tried` says: the line for a swell drawn.
    pub fn tried(&mut self, tried: Tried) -> Option<String> {
        let due = self.due.as_mut()?;
        match tried {
            Tried::Drawn => {
                let score = due.score;
                self.due = None;
                Some(format!(
                    "lamp-voice ring: speaker muted: the name heard (score {score:.2}), one teal swell"
                ))
            }
            Tried::Held(why) => {
                due.held = Some(why);
                None
            }
        }
    }

    /// Time passes: a swell whose second has ended with none drawn is said
    /// not drawn.
    pub fn tick(&mut self, now_us: u64) -> Option<String> {
        if self.due.as_ref().is_some_and(|due| now_us >= due.until_us) {
            return self.stop();
        }
        None
    }

    /// The ring stops: a swell still due is said not drawn, and why.
    pub fn stop(&mut self) -> Option<String> {
        let due = self.due.take()?;
        Some(format!(
            "lamp-voice ring: speaker muted: the name heard (score {:.2}), no swell (not drawn: {})",
            due.score,
            due.held.unwrap_or("no frame")
        ))
    }
}

/// What the thread last found, on the ring's monotonic clock.
#[derive(Clone, Copy, Debug)]
struct Read {
    speaker: Speaker,
    at_us: u64,
    /// A hearing not yet taken by the ring's loop: when it was read, and
    /// its score.
    heard: Option<(u64, f32)>,
}

struct Found {
    read: Mutex<Read>,
    stop: AtomicBool,
}

pub struct RingSpeaker {
    found: Arc<Found>,
    /// Whether the journal was last told the speaker muted.
    said: Option<bool>,
    /// A hearing's swell, until the journal is told of it.
    swell: SwellSaid,
    /// Every line this told the journal, for tests of the worker's loop.
    #[cfg(test)]
    lines: Arc<Mutex<Vec<String>>>,
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            since.as_millis().min(u128::from(u64::MAX)) as u64
        })
}

/// One look at the two topics on `bus`. A bus that cannot be read is no
/// word.
fn look(bus: &Bus, clock_id: &str) -> Speaker {
    let state = bus.latest(SpeakerState::TOPIC).ok().flatten();
    let volume = bus.latest(SpeakerVolume::TOPIC).ok().flatten();
    speaker(
        state.as_ref(),
        volume.as_ref(),
        clock_id,
        lamp_ipc::monotonic_us(),
        unix_ms(),
    )
}

/// Wait up to [`LOOK_EVERY`] for a hearing, and take any others waiting:
/// the newest to swell for, when it was read, and its score.
fn hearings(names: &mut Subscriber) -> Option<(u64, f32)> {
    let mut heard = None;
    let mut wait = LOOK_EVERY;
    for _ in 0..MOST_HEARD {
        match names.recv(wait) {
            Ok(Some(message)) => {
                // Judged by the clock its header calls for, and dated for the
                // swell by when this reader took it, as the voice's own name
                // reader dates a hearing (`bus_out::NameIn`).
                if let Some(score) = hearing(&message, &names.now()) {
                    heard = Some((lamp_ipc::monotonic_us(), score));
                }
                wait = Duration::ZERO;
            }
            Ok(None) => break,
            Err(_) => {
                // Not a tight loop on a bus gone wrong.
                if wait > Duration::ZERO {
                    thread::sleep(LOOK_EVERY);
                }
                break;
            }
        }
    }
    heard
}

impl RingSpeaker {
    /// The speaker and the lamp's name on the bus under `root`, on the host
    /// boot `clock_id`: looked at once now, so a ring that starts muted shows
    /// it from its first frame, then by a thread of its own.
    pub fn on(root: &Path, clock_id: &str) -> io::Result<Self> {
        let bus = Bus::new(root)?;
        let mut names = bus.subscribe_typed(NameHeard::TOPIC, SUBSCRIBER, NameHeard::TYPE)?;
        let clock_id = clock_id.to_owned();
        let found = Arc::new(Found {
            read: Mutex::new(Read {
                speaker: look(&bus, &clock_id),
                at_us: lamp_ipc::monotonic_us(),
                heard: None,
            }),
            stop: AtomicBool::new(false),
        });
        let reading = Arc::clone(&found);
        thread::Builder::new()
            .name("voice-ring-speaker".into())
            .spawn(move || {
                while !reading.stop.load(Ordering::Acquire) {
                    let heard = hearings(&mut names);
                    let speaker = look(&bus, &clock_id);
                    let Ok(mut read) = reading.read.lock() else {
                        break;
                    };
                    read.speaker = speaker;
                    read.at_us = lamp_ipc::monotonic_us();
                    if heard.is_some() {
                        read.heard = heard;
                    }
                }
            })?;
        Ok(Self {
            found,
            said: None,
            swell: SwellSaid::default(),
            #[cfg(test)]
            lines: Arc::default(),
        })
    }

    /// Call from the ring's loop with its clock: the speaker as last found,
    /// and a hearing not taken yet (when it was read, and its score), taken
    /// once. It reads no bus and never waits. Each change between muted and
    /// not is told in the journal, once, as is a hearing's swell whose second
    /// ended with none drawn.
    pub fn tick(&mut self, now_us: u64) -> (Speaker, Option<(u64, f32)>) {
        let Ok(mut read) = self.found.read.lock() else {
            return (Speaker::Unknown, None);
        };
        let speaker = if now_us.saturating_sub(read.at_us) < FOUND_LASTS_US {
            read.speaker
        } else {
            Speaker::Unknown
        };
        let heard = read.heard.take();
        drop(read);
        if let Some(line) = self.said(speaker) {
            self.journal(format!("lamp-voice ring: speaker {line}"));
        }
        if let Some(line) = self.swell.tick(now_us) {
            self.journal(line);
        }
        (speaker, heard)
    }

    /// The face took a hearing of `score` at `at_us` for its swell
    /// ([`crate::ring_face::Face::heard`]). Nothing is said until a frame
    /// with it is written ([`RingSpeaker::swell_tried`]).
    pub fn swell_taken(&mut self, score: f32, at_us: u64) {
        if let Some(line) = self.swell.taken(score, at_us) {
            self.journal(line);
        }
    }

    /// A frame was tried while a swell may be due, as `tried` says.
    pub fn swell_tried(&mut self, tried: Tried) {
        if let Some(line) = self.swell.tried(tried) {
            self.journal(line);
        }
    }

    /// Tell the journal `line`.
    fn journal(&self, line: String) {
        eprintln!("{line}");
        #[cfg(test)]
        if let Ok(mut lines) = self.lines.lock() {
            lines.push(line);
        }
    }

    /// The journal's line for `speaker`, when it changes between muted and
    /// not, with the word that changed it. A ring that starts heard says
    /// nothing.
    fn said(&mut self, speaker: Speaker) -> Option<String> {
        let muted = speaker.muted();
        let before = self.said.replace(muted);
        match (before, muted) {
            (Some(true), true) | (Some(false) | None, false) => None,
            (_, true) => Some(format!("{}: its faces in teal", speaker.told())),
            (Some(true), false) => Some(format!("{}: its faces as ever", speaker.told())),
        }
    }
}

impl Drop for RingSpeaker {
    /// The thread ends at its next look; the ring's end never waits on it. A
    /// swell still due as the ring stops is said not drawn.
    fn drop(&mut self) {
        self.found.stop.store(true, Ordering::Release);
        if let Some(line) = self.swell.stop() {
            self.journal(line);
        }
    }
}

#[cfg(test)]
impl RingSpeaker {
    /// The ring's loop as if the thread had found `speaker` now.
    pub(crate) fn found(speaker: Speaker) -> Self {
        Self::found_heard(speaker, None).0
    }

    /// The same with a hearing of `score` read now, if any, and every line
    /// it will tell the journal.
    pub(crate) fn found_heard(
        speaker: Speaker,
        score: Option<f32>,
    ) -> (Self, Arc<Mutex<Vec<String>>>) {
        let now = lamp_ipc::monotonic_us();
        let ring = Self {
            found: Arc::new(Found {
                read: Mutex::new(Read {
                    speaker,
                    at_us: now,
                    heard: score.map(|score| (now, score)),
                }),
                stop: AtomicBool::new(true),
            }),
            said: None,
            swell: SwellSaid::default(),
            lines: Arc::default(),
        };
        let lines = Arc::clone(&ring.lines);
        (ring, lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_bus::Kind;
    use lamp_msgs::ClockStamp;
    use std::sync::atomic::AtomicU64;

    const CLOCK: &str = "host-boot";
    /// A wall clock the test's messages were sent at.
    const UNIX_MS: u64 = 1_800_000_000_000;

    /// A root of its own, short enough for a socket's address.
    fn root(label: &str) -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::path::PathBuf::from(format!(
            "/tmp/lrs-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    /// A `speaker/state` message as lamp-actions sends it, at `at_us`.
    fn state_at(at_us: u64, volume_raw: Option<u8>, muted: bool) -> Message {
        let session = "0123456789abcdef";
        let state = SpeakerState {
            stamp: ClockStamp {
                clock_id: CLOCK.into(),
                at_us,
                sequence: 1,
            },
            session_id: session.into(),
            mute_generation: 0,
            volume_raw,
            muted,
        };
        Message {
            header: lamp_bus::Header {
                topic: SpeakerState::TOPIC.into(),
                kind: Kind::Latest,
                enc: lamp_bus::JSON.into(),
                type_name: Some(SpeakerState::TYPE.into()),
                source: SpeakerState::SOURCE.into(),
                boot: session.into(),
                seq: 1,
                at_us,
                at_unix_ms: UNIX_MS,
                expires_ms: Some(lamp_msgs::SPEAKER_STATE_FRESH_US / 1_000),
                clock: None,
            },
            payload: serde_json::to_vec(&state).unwrap(),
            gap: 0,
            publisher_restarted: false,
            expired: false,
        }
    }

    /// A `speaker/volume` message from `source` at `at_us`.
    fn volume_from(source: &str, at_us: u64, volume: u8, muted: bool) -> Message {
        Message {
            header: lamp_bus::Header {
                topic: SpeakerVolume::TOPIC.into(),
                kind: Kind::Latest,
                enc: lamp_bus::JSON.into(),
                type_name: Some(SpeakerVolume::TYPE.into()),
                source: source.into(),
                boot: "fedcba9876543210".into(),
                seq: 1,
                at_us,
                at_unix_ms: UNIX_MS,
                expires_ms: None,
                clock: None,
            },
            payload: serde_json::to_vec(&SpeakerVolume { volume, muted }).unwrap(),
            gap: 0,
            publisher_restarted: false,
            expired: false,
        }
    }

    fn volume_at(at_us: u64, volume: u8) -> Message {
        volume_from(SpeakerVolume::SOURCE, at_us, volume, volume == 0)
    }

    /// The speaker at `now_us` from what the bus keeps.
    fn read(state: Option<&Message>, volume: Option<&Message>, now_us: u64) -> Speaker {
        speaker(state, volume, CLOCK, now_us, UNIX_MS)
    }

    /// The task's cases: a fresh state decides, a level of 0 is muted
    /// whatever its mute says, an unknown level shows nothing, and a stale
    /// state leaves only a volume said after it.
    #[test]
    fn a_fresh_state_decides_and_a_level_of_zero_is_muted() {
        let at = 10_000_000;
        let state = |raw, muted| state_at(at, raw, muted);
        // Muted, as lamp-actions mutes: the level read 0.
        assert!(read(Some(&state(Some(0), true)), None, at + 1).muted());
        // A mute asked for whose level could not be read: muted.
        assert_eq!(
            read(Some(&state(None, true)), None, at + 1),
            Speaker::Muted {
                said: SpeakerState::TOPIC,
                level: None
            }
        );
        // Heard at any level over 0, even under a mute asked for: a level
        // raised again outside lamp-actions is what a person hears.
        assert_eq!(
            read(Some(&state(Some(35), false)), None, at + 1),
            Speaker::Heard {
                said: SpeakerState::TOPIC,
                level: 35
            }
        );
        assert!(!read(Some(&state(Some(35), true)), None, at + 1).muted());
        // No level known and no mute: nothing is known, and an older volume
        // of 0 does not stand in for it.
        assert_eq!(
            read(
                Some(&state(None, false)),
                Some(&volume_at(at - 1, 0)),
                at + 1
            ),
            Speaker::Unknown
        );
        // A fresh state decides over any volume, newer or older.
        assert!(
            !read(
                Some(&state(Some(40), false)),
                Some(&volume_at(at + 5, 0)),
                at + 10
            )
            .muted()
        );
        assert!(
            read(
                Some(&state(Some(0), true)),
                Some(&volume_at(at + 5, 40)),
                at + 10
            )
            .muted()
        );
    }

    /// Without a fresh state the kept volume counts only as the newest
    /// word; with neither, nothing is known.
    #[test]
    fn without_a_fresh_state_only_a_newer_volume_counts() {
        let at = 10_000_000;
        let stale = at + lamp_msgs::SPEAKER_STATE_FRESH_US;
        let state = state_at(at, Some(40), false);
        // Fresh up to its 3 s, not at them.
        assert!(!read(Some(&state), None, stale - 1).muted());
        assert_eq!(read(Some(&state), None, stale), Speaker::Unknown);
        // Lapsed: a volume said before it is older than its word.
        assert_eq!(
            read(Some(&state), Some(&volume_at(at - 1, 0)), stale),
            Speaker::Unknown
        );
        // Said after it: the newest word on the speaker.
        assert_eq!(
            read(Some(&state), Some(&volume_at(at + 1, 0)), stale),
            Speaker::Muted {
                said: SpeakerVolume::TOPIC,
                level: Some(0)
            }
        );
        assert_eq!(
            read(Some(&state), Some(&volume_at(at + 1, 20)), stale),
            Speaker::Heard {
                said: SpeakerVolume::TOPIC,
                level: 20
            }
        );
        // No state at all (requested audio off): the volume alone.
        assert!(read(None, Some(&volume_at(at, 0)), stale).muted());
        assert!(!read(None, Some(&volume_at(at, 1)), stale).muted());
        // Neither: nothing.
        assert_eq!(read(None, None, stale), Speaker::Unknown);
    }

    /// Only lamp-actions is the speaker's word: a state on another boot's
    /// clock or a volume from another part is none, and a volume that
    /// breaks its own rule (muted at 40) is none either.
    #[test]
    fn only_lamp_actions_own_valid_words_count() {
        let at = 10_000_000;
        let state = state_at(at, Some(0), true);
        assert_eq!(
            speaker(Some(&state), None, "another-boot", at + 1, UNIX_MS),
            Speaker::Unknown
        );
        let mut foreign = state_at(at, Some(0), true);
        foreign.header.source = "replay".into();
        assert_eq!(read(Some(&foreign), None, at + 1), Speaker::Unknown);
        assert_eq!(
            read(None, Some(&volume_from("control", at, 0, true)), at + 1),
            Speaker::Unknown
        );
        assert_eq!(
            read(None, Some(&volume_from("actions", at, 40, true)), at + 1),
            Speaker::Unknown
        );
        assert_eq!(
            read(None, Some(&volume_from("actions", at, 0, false)), at + 1),
            Speaker::Unknown
        );
    }

    /// Over the person's own light the rest shows it too; under a light
    /// someone asked for, only the turns; heard or unknown, nothing.
    #[test]
    fn the_rest_shows_it_only_over_the_persons_own_light() {
        let muted = Speaker::Muted {
            said: SpeakerState::TOPIC,
            level: Some(0),
        };
        assert_eq!(shown(muted, true), Muted::AtRest);
        assert_eq!(shown(muted, false), Muted::InTurns);
        let heard = Speaker::Heard {
            said: SpeakerState::TOPIC,
            level: 30,
        };
        for own in [true, false] {
            assert_eq!(shown(heard, own), Muted::No);
            assert_eq!(shown(Speaker::Unknown, own), Muted::No);
        }
    }

    /// A hearing to swell for is lamp-name's own, sure, fresh and whole, and
    /// how fresh is judged by the clock its header calls for: with no
    /// `clock`, as lamp-name says it, by its wall stamp whatever its `at_us`
    /// (lamp-dbda's were 17,513 s behind the ring's, #8934's row); with this
    /// boot's clock, by `at_us`; from another boot, never.
    #[test]
    fn a_hearing_is_lamp_names_own_sure_and_fresh_by_the_clock_its_header_calls_for() {
        let now = Now {
            unix_ms: UNIX_MS,
            monotonic_us: 44_062_000_000,
            boot_id: Some("this-boot"),
        };
        let said = |source: &str, clock: Option<&str>, at_us: u64, at_unix_ms: u64, score| {
            let mut message = volume_from(source, at_us, 0, true);
            message.header.topic = NameHeard::TOPIC.into();
            message.header.kind = Kind::Event;
            message.header.type_name = Some(NameHeard::TYPE.into());
            message.header.at_unix_ms = at_unix_ms;
            message.header.clock = clock.map(str::to_owned);
            message.payload = serde_json::to_vec(&NameHeard { score, ago_ms: 560 }).unwrap();
            message
        };
        let lasts_ms = HEARING_LASTS_US / 1_000;
        // As lamp-name says it: no clock, `at_us` on its own clock from its
        // start, far behind this one; its wall stamp decides.
        let own = |at_unix_ms| said("name", None, 26_597_801_385, at_unix_ms, 0.999_999_5);
        assert_eq!(hearing(&own(UNIX_MS - 80), &now), Some(0.999_999_5));
        assert!(hearing(&own(UNIX_MS - lasts_ms), &now).is_some());
        assert!(hearing(&own(UNIX_MS - lasts_ms - 1), &now).is_none());
        assert!(
            hearing(&own(UNIX_MS + 40), &now).is_some(),
            "within the grace"
        );
        assert!(hearing(&own(UNIX_MS + 51), &now).is_none(), "ahead");
        // This boot's CLOCK_MONOTONIC said in `clock`: `at_us` decides, and
        // the wall stamp is not read.
        let mono = |at_us| said("name", Some("this-boot"), at_us, 0, 0.97);
        assert!(hearing(&mono(now.monotonic_us - 80_000), &now).is_some());
        assert!(hearing(&mono(now.monotonic_us - HEARING_LASTS_US), &now).is_some());
        assert!(hearing(&mono(now.monotonic_us - HEARING_LASTS_US - 1), &now).is_none());
        assert!(
            hearing(&mono(now.monotonic_us + 51_000), &now).is_none(),
            "ahead"
        );
        // Another boot's clock: a hearing from before the lamp started again.
        let other = said(
            "name",
            Some("another-boot"),
            now.monotonic_us,
            UNIX_MS,
            0.97,
        );
        assert!(hearing(&other, &now).is_none());
        // Not lamp-name's, not sure enough, not whole, or expired: none.
        assert!(hearing(&said("replay", None, 1, UNIX_MS, 0.97), &now).is_none());
        assert!(hearing(&said("name", None, 1, UNIX_MS, 0.5), &now).is_none());
        let mut broken = own(UNIX_MS);
        broken.payload = b"{}".to_vec();
        assert!(hearing(&broken, &now).is_none());
        let mut expired = own(UNIX_MS);
        expired.expired = true;
        assert!(hearing(&expired, &now).is_none());
    }

    /// The journal hears each change once: shown when it turns muted, gone
    /// with the word that ended it; a ring that starts heard says nothing.
    #[test]
    fn the_journal_hears_each_change_once() {
        let mut ring = RingSpeaker::found(Speaker::Unknown);
        let muted = Speaker::Muted {
            said: SpeakerState::TOPIC,
            level: Some(0),
        };
        let heard = Speaker::Heard {
            said: SpeakerVolume::TOPIC,
            level: 35,
        };
        let lines: Vec<Option<String>> = [heard, muted, muted, heard, Speaker::Unknown]
            .into_iter()
            .chain([muted, Speaker::Unknown])
            .map(|speaker| ring.said(speaker))
            .collect();
        assert_eq!(
            lines,
            [
                None,
                Some("muted (speaker/state, raw 0): its faces in teal".into()),
                None,
                Some("heard (speaker/volume, 35%): its faces as ever".into()),
                None,
                Some("muted (speaker/state, raw 0): its faces in teal".into()),
                Some("not known: its faces as ever".into()),
            ]
        );
        // The two topics' numbers are not one scale (#8934's review): the
        // mixer's raw value from `speaker/state`, a percent of what the lamp
        // allows from `speaker/volume`, each said as what it is.
        for (speaker, told) in [
            (
                Speaker::Muted {
                    said: SpeakerState::TOPIC,
                    level: None,
                },
                "muted (speaker/state, no level read)",
            ),
            (
                Speaker::Heard {
                    said: SpeakerState::TOPIC,
                    level: 35,
                },
                "heard (speaker/state, raw 35)",
            ),
            (
                Speaker::Muted {
                    said: SpeakerVolume::TOPIC,
                    level: Some(0),
                },
                "muted (speaker/volume, 0%)",
            ),
        ] {
            assert_eq!(speaker.told(), told);
        }
    }

    /// A swell's journal line in itself: drawn, once; not drawn, with the
    /// last reason, when its second ends or the ring stops; nothing for a
    /// frame tried with none due; a new hearing says the last one first.
    #[test]
    fn a_swells_line_says_drawn_once_or_not_drawn_and_why() {
        use crate::ring_face::HEARD_US;
        let line = |score: &str, end: &str| {
            format!("lamp-voice ring: speaker muted: the name heard (score {score}), {end}")
        };
        let mut said = SwellSaid::default();
        assert_eq!(said.tried(Tried::Drawn), None, "nothing due");
        assert_eq!(said.taken(0.97, 1_000), None);
        assert_eq!(said.tried(Tried::Held("privacy")), None);
        assert_eq!(
            said.tried(Tried::Drawn),
            Some(line("0.97", "one teal swell"))
        );
        assert_eq!(said.tried(Tried::Drawn), None, "once");
        assert_eq!(said.stop(), None);
        // Never drawn: said at the end of its second, with the last reason.
        assert_eq!(said.taken(0.96, 2_000_000), None);
        assert_eq!(said.tried(Tried::Held("privacy")), None);
        assert_eq!(said.tried(Tried::Held("a computer's pixels")), None);
        assert_eq!(said.tick(2_000_000 + HEARD_US - 1), None);
        assert_eq!(
            said.tick(2_000_000 + HEARD_US),
            Some(line("0.96", "no swell (not drawn: a computer's pixels)"))
        );
        assert_eq!(said.tick(2_000_000 + HEARD_US + 1), None);
        // Nothing tried before the ring stops.
        assert_eq!(said.taken(0.95, 5_000_000), None);
        assert_eq!(
            said.stop(),
            Some(line("0.95", "no swell (not drawn: no frame)"))
        );
        // A new hearing while one is still due says the last one first.
        assert_eq!(said.taken(0.94, 7_000_000), None);
        assert_eq!(said.tried(Tried::Held("privacy")), None);
        assert_eq!(
            said.taken(0.93, 7_500_000),
            Some(line("0.94", "no swell (not drawn: privacy)"))
        );
    }

    /// What `read` gives once the speaker's thread has found it, within a
    /// second.
    fn soon<T: PartialEq + std::fmt::Debug>(wanted: T, mut read: impl FnMut() -> T) {
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            let found = read();
            if found == wanted {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{found:?}, not {wanted:?}"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// On a real bus: a ring that starts with the speaker at 0 is muted at
    /// once, from its first look; raised, it is heard within the thread's
    /// look; a hearing of the name is handed over once; and a thread that
    /// has found nothing for a second is no word.
    #[test]
    fn the_thread_reads_the_speaker_and_hands_each_hearing_over_once() {
        let root = root("thread");
        let bus = Bus::new(&root).unwrap();
        let mut volume = bus
            .publisher_declared(
                SpeakerVolume::TOPIC,
                &SpeakerVolume::declaration(),
                SpeakerVolume::SOURCE,
            )
            .unwrap();
        volume
            .publish_json(
                lamp_ipc::monotonic_us(),
                &SpeakerVolume {
                    volume: 0,
                    muted: true,
                },
            )
            .unwrap();
        let mut ring = RingSpeaker::on(&root, CLOCK).unwrap();
        // From the look before the first frame, without the thread.
        assert!(ring.tick(lamp_ipc::monotonic_us()).0.muted());
        volume
            .publish_json(
                lamp_ipc::monotonic_us(),
                &SpeakerVolume {
                    volume: 30,
                    muted: false,
                },
            )
            .unwrap();
        soon(false, || ring.tick(lamp_ipc::monotonic_us()).0.muted());
        // The name, heard: handed over once, at about when it was read.
        let mut names = bus
            .publisher_declared(NameHeard::TOPIC, &NameHeard::declaration(), NAME_SOURCE)
            .unwrap();
        let sent = lamp_ipc::monotonic_us();
        names
            .publish_json(
                sent,
                &NameHeard {
                    score: 0.98,
                    ago_ms: 300,
                },
            )
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let (taken, score) = loop {
            if let (_, Some(heard)) = ring.tick(lamp_ipc::monotonic_us()) {
                break heard;
            }
            assert!(std::time::Instant::now() < deadline, "never handed over");
            thread::sleep(Duration::from_millis(2));
        };
        assert!(taken >= sent);
        assert_eq!(score, 0.98);
        thread::sleep(LOOK_EVERY * 3);
        assert_eq!(ring.tick(lamp_ipc::monotonic_us()).1, None, "taken once");
        // Past what the thread last found by a second: no word.
        assert_eq!(
            ring.tick(
                lamp_ipc::monotonic_us() + FOUND_LASTS_US + LOOK_EVERY.as_micros() as u64 * 4
            )
            .0,
            Speaker::Unknown
        );
        drop(ring);
        let _ = std::fs::remove_dir_all(root);
    }

    /// #8934's lamp row (lamp-dbda, 28 Nov 03:12): lamp-name stamps `at_us`
    /// on its own clock, counted from its own start, and says no `clock`, so
    /// there it was 17,513 s behind the ring's; every real hearing read as
    /// 4.9 h old and no swell came. A header's `at_us` is its publisher's own
    /// unless it says this boot's clock (docs/bus.md), so such a hearing is
    /// judged by its wall stamp: just said, it is handed over whatever its
    /// `at_us`; said 2 s before it is read, it is not.
    #[test]
    fn a_hearing_on_lamp_names_own_clock_is_taken_fresh_and_never_old() {
        let root = root("ownclock");
        let mut ring = RingSpeaker::on(&root, CLOCK).unwrap();
        let wall_ms = || {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64
        };
        // As lamp-name says it: a few seconds into its own clock, far behind
        // this host's monotonic one, with no `clock` in the header.
        let say = |bus: Bus| {
            let mut names = bus
                .publisher_declared(NameHeard::TOPIC, &NameHeard::declaration(), NAME_SOURCE)
                .unwrap();
            names
                .publish_json(
                    5_000_000,
                    &NameHeard {
                        score: 0.999_999_5,
                        ago_ms: 560,
                    },
                )
                .unwrap();
            names
        };
        let fresh = say(Bus::new(&root).unwrap());
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let score = loop {
            if let (_, Some((_, score))) = ring.tick(lamp_ipc::monotonic_us()) {
                break score;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "a fresh hearing on lamp-name's own clock was never handed over"
            );
            thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(score, 0.999_999_5);
        // The same said by a wall clock 2 s behind: old, never handed over.
        let late = Bus::new(&root)
            .unwrap()
            .clocks(move || wall_ms() - 2_000, lamp_bus::monotonic_us);
        let old = say(late);
        thread::sleep(LOOK_EVERY * 4);
        assert_eq!(ring.tick(lamp_ipc::monotonic_us()).1, None, "an old one");
        drop((ring, fresh, old));
        let _ = std::fs::remove_dir_all(root);
    }

    /// The worker's control link: one authority, then nothing until `until`,
    /// then Stop.
    struct Link {
        authority: Option<crate::wire::Control>,
        until: std::time::Instant,
    }
    impl crate::ring_worker::RingLink for Link {
        fn receive_control(&mut self) -> io::Result<Option<crate::wire::Control>> {
            Ok(self.authority.take().or_else(|| {
                (std::time::Instant::now() >= self.until).then_some(crate::wire::Control::Stop)
            }))
        }
        fn receive_frame(&mut self) -> io::Result<Option<crate::ring_wire::RingFrame>> {
            Ok(None)
        }
        fn publish(&mut self, _: crate::wire::WorkerEvent) -> io::Result<()> {
            Ok(())
        }
    }
    struct Sink(Arc<Mutex<Vec<lamp_ring::EncodedFrame>>>);
    impl lamp_ring::FrameSink for Sink {
        fn write_frame(&mut self, frame: &lamp_ring::EncodedFrame) -> io::Result<()> {
            self.0.lock().unwrap().push(frame.clone());
            Ok(())
        }
    }

    /// The pixels an encoded frame carries: green, red and blue, a byte
    /// each, one wire byte per bit.
    fn decoded(frame: &lamp_ring::EncodedFrame) -> [lamp_ring::Rgb; lamp_ring::PIXEL_COUNT] {
        let byte = |bits: &[u8]| {
            bits.iter()
                .fold(0u8, |value, bit| (value << 1) | u8::from(*bit == 0xFC))
        };
        std::array::from_fn(|pixel| {
            let at = lamp_ring::PRIMER_BYTES + pixel * 24;
            let bits = &frame.bytes()[at..at + 24];
            lamp_ring::Rgb::new(byte(&bits[8..16]), byte(&bits[..8]), byte(&bits[16..]))
        })
    }

    /// The frames the actual ring worker writes for 200 ms of rest, its
    /// speaker as `speaker` says and the microphone `permission`, with
    /// `light` its own and the light it begins in (a request held on it), or
    /// none.
    fn resting_frames(
        speaker: Speaker,
        permission: lamp_turn_owner::Permission,
        light: Option<(crate::ring_light::RingLight, crate::ring_face::Lit)>,
    ) -> Vec<[lamp_ring::Rgb; lamp_ring::PIXEL_COUNT]> {
        resting_frames_with(RingSpeaker::found(speaker), permission, light)
    }

    /// The same with a speaker sense of the test's own making.
    fn resting_frames_with(
        speaker: RingSpeaker,
        permission: lamp_turn_owner::Permission,
        light: Option<(crate::ring_light::RingLight, crate::ring_face::Lit)>,
    ) -> Vec<[lamp_ring::Rgb; lamp_ring::PIXEL_COUNT]> {
        use lamp_turn_owner::{AdmissionState, CaptureState, Controller, MonoTime};
        let now = || MonoTime::from_micros(lamp_ipc::monotonic_us());
        let boot = crate::process::new_boot().unwrap();
        let mut controller = Controller::new(boot, now());
        controller
            .set_microphone_permission(now(), lamp_turn_owner::Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(now().as_micros() + 500_000);
        controller
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        controller
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
        // Closed after the input was ready, as the switch closes it.
        controller
            .set_microphone_permission(now(), permission)
            .unwrap();
        let link = Link {
            authority: Some(crate::wire::Control::Authority {
                snapshot: controller.snapshot(now()).unwrap(),
            }),
            until: std::time::Instant::now() + Duration::from_millis(200),
        };
        let frames = Arc::new(Mutex::new(Vec::new()));
        crate::ring_worker::run_with_sink_senses(
            link,
            boot,
            Sink(frames.clone()),
            Some(60),
            crate::ring_worker::Senses {
                speaker: Some(speaker),
                light,
                ..Default::default()
            },
        )
        .unwrap();
        frames.lock().unwrap().iter().map(decoded).collect()
    }

    /// The lines a hearing's swell gave the journal, through the actual
    /// worker loop and its guarded sink for 200 ms of rest: the speaker
    /// muted, a hearing scored `score` read as it starts, the microphone
    /// `permission`, and `light` its own.
    fn swell_lines(
        score: f32,
        permission: lamp_turn_owner::Permission,
        light: Option<(crate::ring_light::RingLight, crate::ring_face::Lit)>,
    ) -> Vec<String> {
        let muted = Speaker::Muted {
            said: SpeakerState::TOPIC,
            level: Some(0),
        };
        let (speaker, lines) = RingSpeaker::found_heard(muted, Some(score));
        resting_frames_with(speaker, permission, light);
        let lines = lines.lock().unwrap().clone();
        lines
            .into_iter()
            .filter(|line| line.contains("the name heard"))
            .collect()
    }

    /// #8934's review: a swell is journalled only as drawn. Drawn, one line,
    /// once; refused at the privacy switch, never "one teal swell" but that
    /// it was not drawn, and why.
    #[test]
    fn a_swell_is_journalled_once_drawn_and_never_over_privacy() {
        use lamp_turn_owner::Permission;
        assert_eq!(
            swell_lines(0.97, Permission::Allowed, None),
            ["lamp-voice ring: speaker muted: the name heard (score 0.97), one teal swell"]
        );
        assert_eq!(
            swell_lines(0.96, Permission::Denied, None),
            [
                "lamp-voice ring: speaker muted: the name heard (score 0.96), no swell (not drawn: privacy)"
            ]
        );
    }

    /// The same at rest under a paired computer's own pixels, which the face
    /// draws as given, with no swell in them.
    #[test]
    fn a_swell_under_a_computers_pixels_is_journalled_not_drawn() {
        use lamp_msgs::{Claim, Light, LightRequest, Priority};
        use lamp_turn_owner::Permission;
        let root = root("pixels");
        let (mut light, _) = crate::ring_light::RingLight::on(&root).unwrap();
        light.take_requests(&root, "boot-test").unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut asks = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "control")
            .unwrap();
        let named = Light::new(true, "red", 3).unwrap();
        asks.publish_json(
            1,
            &LightRequest {
                claim: Claim {
                    source: "control:c_1".into(),
                    priority: Priority::Skill,
                    clock_id: "boot-test".into(),
                    until_us: lamp_ipc::monotonic_us() + 20_000_000,
                },
                light: named.clone(),
                pattern: None,
                fade_ms: None,
                pixels: Some(vec![[200, 0, 200]; LightRequest::PIXELS]),
            },
        )
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while light.pixels().is_none() {
            assert!(std::time::Instant::now() < deadline, "never taken");
            light.tick(lamp_ipc::monotonic_us());
            thread::sleep(Duration::from_millis(2));
        }
        let begins = crate::ring_light::lit(&named).unwrap();
        assert_eq!(
            swell_lines(0.95, Permission::Allowed, Some((light, begins))),
            [
                "lamp-voice ring: speaker muted: the name heard (score 0.95), no swell (not drawn: a computer's pixels)"
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// The whole ring's red, green and blue in the last lit frame, if any.
    fn last_lit(frames: &[[lamp_ring::Rgb; lamp_ring::PIXEL_COUNT]]) -> Option<[u32; 3]> {
        frames
            .iter()
            .rev()
            .map(|pixels| {
                pixels.iter().fold([0u32; 3], |[r, g, b], pixel| {
                    [
                        r + u32::from(pixel.red),
                        g + u32::from(pixel.green),
                        b + u32::from(pixel.blue),
                    ]
                })
            })
            .find(|sums| sums.iter().any(|sum| *sum > 0))
    }

    /// Through the actual worker loop and its guarded sink: the speaker
    /// muted, the resting face is teal; heard, or not known, it is the warm
    /// first light; with the privacy switch closed nothing but black is
    /// written, muted or not; under a light a skill holds, the rest is the
    /// skill's light; and the person's own light asked for at level 4 stays
    /// theirs while muted.
    #[test]
    fn the_worker_rests_in_teal_only_while_the_speaker_is_muted_and_never_over_privacy() {
        use lamp_turn_owner::Permission;
        let muted = Speaker::Muted {
            said: SpeakerState::TOPIC,
            level: Some(0),
        };
        let heard = Speaker::Heard {
            said: SpeakerState::TOPIC,
            level: 40,
        };
        let [red, green, blue] = last_lit(&resting_frames(muted, Permission::Allowed, None))
            .expect("the muted face is drawn");
        assert!(red == 0 && green > blue && blue > 0, "{red} {green} {blue}");
        for plain in [heard, Speaker::Unknown] {
            let [red, green, blue] =
                last_lit(&resting_frames(plain, Permission::Allowed, None)).expect("drawn");
            assert!(red > green && blue == 0, "{plain:?}: {red} {green} {blue}");
        }
        for speaker in [muted, heard] {
            assert_eq!(
                last_lit(&resting_frames(speaker, Permission::Denied, None)),
                None,
                "{speaker:?}"
            );
        }
        // A skill's light held over the person's: white at level 5, kept.
        use lamp_msgs::{Claim, Light, LightRequest, Priority};
        let root = root("held");
        let (mut light, _) = crate::ring_light::RingLight::on(&root).unwrap();
        light.take_requests(&root, "boot-test").unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut asks = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
            .unwrap();
        let white = Light::new(true, "white", 5).unwrap();
        asks.publish_json(
            1,
            &LightRequest {
                claim: Claim {
                    source: "skill:welcome-back".into(),
                    priority: Priority::Skill,
                    clock_id: "boot-test".into(),
                    until_us: lamp_ipc::monotonic_us() + 20_000_000,
                },
                light: white.clone(),
                pattern: None,
                fade_ms: None,
                pixels: None,
            },
        )
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while light.face() != &white {
            assert!(std::time::Instant::now() < deadline, "never taken");
            light.tick(lamp_ipc::monotonic_us());
            thread::sleep(Duration::from_millis(2));
        }
        assert!(!light.persons_own());
        let begins = crate::ring_light::lit(&white).unwrap();
        let [red, green, blue] = last_lit(&resting_frames(
            muted,
            Permission::Allowed,
            Some((light, begins)),
        ))
        .expect("the skill's light is drawn");
        assert!(
            red > green && green > blue && blue > 0,
            "white, not teal: {red} {green} {blue}"
        );
        let _ = std::fs::remove_dir_all(root);
        // The person's own light, asked for on the bus at level 4: theirs,
        // warm, while muted; at level 1, teal.
        for (level, teal) in [(4, false), (1, true)] {
            let root = self::root("level");
            let bus = Bus::new(&root).unwrap();
            let mut asked = bus
                .publisher(Light::TOPICS[0], lamp_bus::Kind::Latest, "actions")
                .unwrap();
            asked
                .publish_json(
                    lamp_ipc::monotonic_us(),
                    &Light::new(true, "warm", level).unwrap(),
                )
                .unwrap();
            let (light, begins) = crate::ring_light::RingLight::on(&root).unwrap();
            assert_eq!(begins.level(), level);
            assert!(light.persons_own());
            let [red, green, blue] = last_lit(&resting_frames(
                muted,
                Permission::Allowed,
                Some((light, begins)),
            ))
            .expect("the person's light is drawn");
            if teal {
                assert!(
                    red == 0 && green > blue,
                    "level {level}: {red} {green} {blue}"
                );
            } else {
                assert!(
                    red > green && blue == 0,
                    "level {level}, warm: {red} {green} {blue}"
                );
            }
            let _ = std::fs::remove_dir_all(root);
        }
    }
}
