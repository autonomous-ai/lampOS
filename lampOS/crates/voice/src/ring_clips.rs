//! An offline reminder rings in its own words (`LAMP_OFFLINE_RINGS`,
//! #8708).
//!
//! With #8632 a reminder due with no link rang the same "You have a
//! reminder." for every reminder: its own words need the model or the
//! speech service, and neither answers offline. Here, while the link is up,
//! each reminder (never a timer) is rendered ahead into a clip of its own,
//! [`words`]: the owner's "You have a reminder." and its text, through the
//! second route's speech service ([`TtsCache::render_once`]). Offline, its
//! clip is what rings.
//!
//! The clips are rendered by the Schedule consumer, the one process that
//! already reads each reminder's text, on a thread of its own ([`spawn`]),
//! never in its 200 ms tick. Its packets to the voice keep carrying ids and
//! times only, never words (#8687's test). The voice reads a clip by its
//! item's id and checks it against the offer's text, which it is given
//! when the item is due ([`Clips::own`]).
//!
//! Each clip is kept at [`DIR`] (0700) as `<store_id>-<id>.wav`, with a
//! sidecar `<store_id>-<id>.json` holding the SHA-256 of its words and the
//! `voice|model|language` it was rendered in: a change in either renders
//! it again. It is keyed by the item's id, so a repeat or a snooze keeps
//! it. At each walk's end ([`Renderer::pass`]) a clip whose id is no longer
//! in Schedule goes (said, cancelled, an edit's old id, the person
//! forgotten), and every clip of another `store_id`.
//!
//! A render that fails (the link down) is tried again after
//! [`crate::say::tts_cache::WARM_EVERY`], then twice as long each time, up
//! to [`crate::say::tts_cache::WARM_EVERY_MOST`]. The folder never holds
//! more than [`MOST_BYTES`]: past it nothing more is rendered, and the
//! journal says so once.
use crate::language::{Language, ring_words};
use crate::say::{
    SOUND_RATE,
    tts_cache::{MOST_FRAMES, TtsCache, WARM_EVERY, WARM_EVERY_MOST},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Cursor, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

/// Where the clips are kept: the voice's own state (root's, 0700). A
/// factory reset wipes it (`crates/setup/fixtures/reset-plan-product.json`).
pub const DIR: &str = "/var/lib/lampOS/voice/ring-clips";

/// A WAV file's header, as `hound` writes a mono 16-bit one.
pub const WAV_HEADER_BYTES: u64 = 44;

/// The largest clip kept: [`MOST_FRAMES`] (20 s) at the speech service's
/// rate, [`SOUND_RATE`] (24 kHz), 16-bit mono, and its header: 960,044
/// bytes. A render longer than that is not kept, and the reminder rings
/// with the generic line.
pub const CLIP_MOST_BYTES: u64 = WAV_HEADER_BYTES + MOST_FRAMES as u64 * 2;

/// The largest sidecar kept beside a clip: its words' hash and the voice
/// it was rendered in, well under this.
pub const SIDECAR_MOST_BYTES: u64 = 256;

/// The most the folder holds (the first builder's bound on #8708): a clip
/// and its sidecar for each of Schedule's
/// [`lamp_schedule::engine::MAX_ITEMS`] (128) items at most, each clip at
/// its largest. A reminder's text is at most 300 bytes
/// ([`lamp_schedule::engine::MAX_TEXT_BYTES`]), and at the speech's pace
/// (about 15 characters a second, [`crate::say::tts_cache::RENDER_MORE_CHARS`]
/// in 2 s) 300 characters and the prefix reach the 20 s cap, so the cap is
/// the real worst case: 128 × (960,044 + 256) = 122,918,400 bytes, about
/// 123 MB. At 16 kHz it would be about 82 MB; the speech service renders at
/// 24 kHz. Past it nothing more is rendered, and the journal says so once.
pub const MOST_BYTES: u64 =
    lamp_schedule::engine::MAX_ITEMS as u64 * (CLIP_MOST_BYTES + SIDECAR_MOST_BYTES);

/// What a reminder rings with offline, in the owner's language: the
/// offline ring's reminder line ([`ring_words`], "You have a reminder.")
/// and its text as the model wrote it.
pub fn words(owners: Option<Language>, text: &str) -> String {
    format!("{} {}", ring_words(owners, false), text.trim())
}

/// A clip's sidecar: the SHA-256 of its words, and the
/// `voice|model|language` it was rendered in.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Sidecar {
    words_sha256: String,
    voice: String,
}

impl Sidecar {
    fn of(words: &str, voice: &str) -> Self {
        Self {
            words_sha256: Sha256::digest(words.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            voice: voice.to_owned(),
        }
    }
}

/// The folder of clips ([`DIR`]).
#[derive(Clone, Debug)]
pub struct Clips {
    dir: PathBuf,
}

impl Clips {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The clips at [`DIR`].
    pub fn at_dir() -> Self {
        Self::new(DIR)
    }

    fn stem(store_id: &str, id: u64) -> String {
        format!("{store_id}-{id}")
    }

    fn wav(&self, store_id: &str, id: u64) -> PathBuf {
        self.dir.join(format!("{}.wav", Self::stem(store_id, id)))
    }

    fn sidecar(&self, store_id: &str, id: u64) -> PathBuf {
        self.dir.join(format!("{}.json", Self::stem(store_id, id)))
    }

    /// Whether the clip kept for `store_id`'s item `id` is `words` in
    /// `voice`: its sidecar says so, and the clip is there.
    pub fn current(&self, store_id: &str, id: u64, words: &str, voice: &str) -> bool {
        self.read_sidecar(store_id, id)
            .is_some_and(|kept| kept == Sidecar::of(words, voice))
            && self.wav(store_id, id).is_file()
    }

    fn read_sidecar(&self, store_id: &str, id: u64) -> Option<Sidecar> {
        let path = self.sidecar(store_id, id);
        let meta = fs::metadata(&path).ok()?;
        if meta.len() > SIDECAR_MOST_BYTES {
            return None;
        }
        serde_json::from_slice(&fs::read(path).ok()?).ok()
    }

    /// The clip of `store_id`'s item `id`, if it is `words` in `voice`:
    /// one sidecar read and one clip read, never rendered here, so a ring
    /// never waits on the network. None for a clip missing, stale (other
    /// words, another voice) or not one of ours.
    pub fn own(&self, store_id: &str, id: u64, words: &str, voice: &str) -> Option<Vec<i16>> {
        if self.read_sidecar(store_id, id)? != Sidecar::of(words, voice) {
            return None;
        }
        crate::say::tts_cache::read_clip(&self.wav(store_id, id))
    }

    /// Every clip and sidecar not of `store_id` or whose id is not in
    /// `live` removed, and any half-written file: how many files went.
    pub fn sweep(&self, store_id: &str, live: &BTreeSet<u64>) -> usize {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return 0;
        };
        let mut gone = 0;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let kept = name
                .strip_suffix(".wav")
                .or_else(|| name.strip_suffix(".json"))
                .and_then(|stem| stem.rsplit_once('-'))
                .is_some_and(|(store, id)| {
                    store == store_id && id.parse::<u64>().is_ok_and(|id| live.contains(&id))
                });
            if !kept && fs::remove_file(entry.path()).is_ok() {
                gone += 1;
            }
        }
        gone
    }

    /// Every file in the folder removed, whatever its name (#8894): with
    /// the switch off nothing renders or sweeps them, so a start clears
    /// them all. One directory read; nothing rendered, nothing sent. How
    /// many files went; none, with no error, for a folder that is absent.
    /// The caller says it in the journal, with its own name.
    pub fn clear(&self) -> usize {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return 0;
        };
        entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter(|entry| fs::remove_file(entry.path()).is_ok())
            .count()
    }

    /// The bytes the folder holds now.
    pub fn used_bytes(&self) -> u64 {
        fs::read_dir(&self.dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|entry| entry.metadata().ok())
                    .filter(|meta| meta.is_file())
                    .map(|meta| meta.len())
                    .sum()
            })
            .unwrap_or(0)
    }

    /// The ids of the clips kept for `store_id`, by their files.
    pub fn kept(&self, store_id: &str) -> BTreeSet<u64> {
        fs::read_dir(&self.dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|entry| {
                        let name = entry.file_name().into_string().ok()?;
                        let (store, id) = name.strip_suffix(".wav")?.rsplit_once('-')?;
                        (store == store_id).then(|| id.parse().ok()).flatten()
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `samples` kept as `store_id`'s item `id`, `words` in `voice`: the old
    /// sidecar removed first, then the clip and its sidecar each written
    /// whole and renamed into place, so a power cut leaves either no clip
    /// or one its sidecar matches.
    fn keep(
        &self,
        store_id: &str,
        id: u64,
        words: &str,
        voice: &str,
        samples: &[i16],
    ) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))?;
        let sidecar = self.sidecar(store_id, id);
        match fs::remove_file(&sidecar) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        let mut bytes = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut bytes, crate::say::tts_cache::clip_spec())
            .map_err(io::Error::other)?;
        for sample in samples {
            writer.write_sample(*sample).map_err(io::Error::other)?;
        }
        writer.finalize().map_err(io::Error::other)?;
        write_whole(&self.wav(store_id, id), bytes.get_ref())?;
        let json = serde_json::to_vec(&Sidecar::of(words, voice))?;
        write_whole(&sidecar, &json)?;
        fs::File::open(&self.dir)?.sync_all()
    }
}

/// `bytes` written to a temporary file beside `path`, synced and renamed.
fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&tmp, path)
}

/// What a walk of Schedule found, for the clips: the store, every id in it
/// (timers too), and each reminder's id and text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wanted {
    pub store_id: String,
    pub live: BTreeSet<u64>,
    pub reminders: Vec<(u64, String)>,
}

/// What one [`Renderer::pass`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pass {
    /// Files swept.
    pub swept: usize,
    /// Clips rendered and kept.
    pub rendered: usize,
    /// A render failed: tried again after the backoff.
    pub failed: bool,
    /// Rendering stopped at the folder's bound ([`MOST_BYTES`]).
    pub full: bool,
    /// The journal said the bound was reached, once for the run of passes
    /// that reach it.
    pub full_told: bool,
}

/// Renders the reminders' clips, on the consumer's thread of its own.
pub struct Renderer {
    clips: Clips,
    cache: TtsCache,
    most_bytes: u64,
    retry_at: Option<Instant>,
    every: Duration,
    full_told: bool,
}

impl Renderer {
    /// Clips rendered through `cache`'s speech service, in its voice and
    /// the owner's language, kept in `clips`.
    pub fn new(clips: Clips, cache: TtsCache) -> Self {
        Self {
            clips,
            cache,
            most_bytes: MOST_BYTES,
            retry_at: None,
            every: WARM_EVERY,
            full_told: false,
        }
    }

    /// The folder's bound as `most_bytes`, for a test.
    #[cfg(test)]
    pub fn bounded(mut self, most_bytes: u64) -> Self {
        self.most_bytes = most_bytes;
        self
    }

    /// One walk's end: clips no longer wanted swept, then each reminder
    /// without a current clip rendered and kept, unless a failed render's
    /// backoff still runs or the folder would pass its bound.
    pub fn pass(&mut self, wanted: &Wanted, now: Instant) -> Pass {
        let mut pass = Pass {
            swept: self.clips.sweep(&wanted.store_id, &wanted.live),
            ..Pass::default()
        };
        if self.retry_at.is_some_and(|at| now < at) {
            return pass;
        }
        let voice = self.cache.identity().to_owned();
        let owners = self.cache.owners();
        let mut used = None;
        for (id, text) in &wanted.reminders {
            let words = words(owners, text);
            if self.clips.current(&wanted.store_id, *id, &words, &voice) {
                continue;
            }
            let used_now = *used.get_or_insert_with(|| self.clips.used_bytes());
            if used_now.saturating_add(CLIP_MOST_BYTES + SIDECAR_MOST_BYTES) > self.most_bytes {
                pass.full = true;
                break;
            }
            match self.cache.render_once_to_keep(&words) {
                Ok((_, false)) => {
                    // Said in the lamp's own voice in place of the person's
                    // Gemini voice (#8762's review): never kept under
                    // theirs, so it would ring offline in the wrong voice
                    // for its whole life; theirs is asked again after the
                    // backoff.
                    pass.failed = true;
                    eprintln!(
                        "lamp-voice schedule-consumer: reminder clips not rendered in the person's own voice, not kept: tried again in {} s (LAMP_OFFLINE_RINGS)",
                        self.every.as_secs()
                    );
                    self.retry_at = Some(now + self.every);
                    self.every = (self.every * 2).min(WARM_EVERY_MOST);
                    return pass;
                }
                Ok((samples, true))
                    if !samples.is_empty() && samples.len() <= MOST_FRAMES as usize =>
                {
                    match self
                        .clips
                        .keep(&wanted.store_id, *id, &words, &voice, &samples)
                    {
                        Ok(()) => {
                            pass.rendered += 1;
                            used = None;
                        }
                        Err(why) => {
                            eprintln!(
                                "lamp-voice schedule-consumer: reminder id {id}'s offline clip was not kept: {why}"
                            );
                        }
                    }
                }
                Ok((_, true)) => {
                    // Too long for a clip: it rings with the generic line.
                    eprintln!(
                        "lamp-voice schedule-consumer: reminder id {id}'s offline clip rendered past {} s: not kept, it rings with the generic line",
                        MOST_FRAMES / SOUND_RATE
                    );
                }
                Err(why) => {
                    pass.failed = true;
                    eprintln!(
                        "lamp-voice schedule-consumer: reminder clips not rendered ({why}): tried again in {} s (LAMP_OFFLINE_RINGS)",
                        self.every.as_secs()
                    );
                    self.retry_at = Some(now + self.every);
                    self.every = (self.every * 2).min(WARM_EVERY_MOST);
                    return pass;
                }
            }
        }
        if pass.rendered > 0 {
            self.every = WARM_EVERY;
            self.retry_at = None;
        }
        if pass.full && !self.full_told {
            eprintln!(
                "lamp-voice schedule-consumer: the offline ring clips reached {} bytes: no more are rendered until some go (LAMP_OFFLINE_RINGS)",
                self.most_bytes
            );
            pass.full_told = true;
        }
        self.full_told = pass.full;
        pass
    }
}

/// The renderer on a thread of its own: each walk's [`Wanted`] is sent
/// here, the newest taken when several wait, and a pass made for it. A
/// send when one is already waiting is dropped (`try_send`), so the
/// consumer's tick never waits on it.
pub fn spawn(mut renderer: Renderer) -> io::Result<mpsc::SyncSender<Wanted>> {
    let (sender, receiver) = mpsc::sync_channel::<Wanted>(1);
    std::thread::Builder::new()
        .name("ring-clips".into())
        .spawn(move || {
            while let Ok(mut wanted) = receiver.recv() {
                while let Ok(newer) = receiver.try_recv() {
                    wanted = newer;
                }
                renderer.pass(&wanted, Instant::now());
            }
        })?;
    Ok(sender)
}

/// The renderer over the second route's speech service, when it is
/// configured, with the switch on; none otherwise.
pub fn renderer_from_environment() -> Option<Renderer> {
    if !crate::offline_rings::on() {
        return None;
    }
    let cache = crate::say::tts_cache::built_from_environment()?;
    Some(Renderer::new(Clips::at_dir(), cache))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::say::tts_cache::Render;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    const STORE: &str = "0123456789abcdef0123456789abcdef";
    const OTHER_STORE: &str = "11111111111111111111111111111111";

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lamp-ring-clips-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// A speech service that renders each line as `frames` samples of its
    /// length, counted, failing while `down` is set.
    fn service(
        voice: &str,
        frames: usize,
    ) -> (
        TtsCache,
        Arc<AtomicUsize>,
        Arc<std::sync::atomic::AtomicBool>,
    ) {
        let calls = Arc::new(AtomicUsize::new(0));
        let down = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (seen, off) = (Arc::clone(&calls), Arc::clone(&down));
        let render: Render = Box::new(move |text: &str| {
            seen.fetch_add(1, Ordering::SeqCst);
            if off.load(Ordering::SeqCst) {
                return Err("no host answers".into());
            }
            Ok(vec![text.len() as i16; frames])
        });
        let cache = TtsCache::new(
            std::env::temp_dir().join("lamp-ring-clips-unused-cache"),
            voice,
            "eleven_v4_turbo",
            "en",
            render,
        );
        (cache, calls, down)
    }

    fn wanted(store: &str, reminders: &[(u64, &str)], timers: &[u64]) -> Wanted {
        Wanted {
            store_id: store.into(),
            live: reminders
                .iter()
                .map(|(id, _)| *id)
                .chain(timers.iter().copied())
                .collect(),
            reminders: reminders
                .iter()
                .map(|(id, text)| (*id, (*text).to_owned()))
                .collect(),
        }
    }

    /// #8708: a reminder's clip is rendered once while online, then read,
    /// never rendered again; rendered again when its words or the voice
    /// change; a failed render waits for the backoff.
    #[test]
    fn a_clip_is_rendered_once_while_online_and_again_when_text_or_voice_changes() {
        let dir = scratch("rendered-once");
        let clips = Clips::new(&dir);
        let (cache, calls, down) = service("Rachel", 2_400);
        let mut renderer = Renderer::new(clips.clone(), cache);
        let start = Instant::now();
        let walk = wanted(STORE, &[(4, "Take your pills.")], &[5]);
        let pass = renderer.pass(&walk, start);
        assert_eq!(pass.rendered, 1, "{pass:?}");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let said = words(None, "Take your pills.");
        assert_eq!(said, "You have a reminder. Take your pills.");
        assert_eq!(
            clips.own(STORE, 4, &said, "Rachel|eleven_v4_turbo|en"),
            Some(vec![said.len() as i16; 2_400])
        );
        assert_eq!(clips.kept(STORE), BTreeSet::from([4]), "never a timer's");
        // The next walks: kept, not rendered again.
        for _ in 0..3 {
            assert_eq!(renderer.pass(&walk, start).rendered, 0);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // Its words changed under the same id: rendered again, the old clip
        // no longer its own.
        let changed = wanted(STORE, &[(4, "Take your vitamins.")], &[5]);
        assert_eq!(renderer.pass(&changed, start).rendered, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            clips.own(STORE, 4, &said, "Rachel|eleven_v4_turbo|en"),
            None
        );
        // Another voice: rendered again.
        let (sarah, sarah_calls, _) = service("Sarah", 2_400);
        let mut renderer = Renderer::new(clips.clone(), sarah);
        assert_eq!(renderer.pass(&changed, start).rendered, 1);
        assert_eq!(sarah_calls.load(Ordering::SeqCst), 1);
        assert!(clips.current(
            STORE,
            4,
            &words(None, "Take your vitamins."),
            "Sarah|eleven_v4_turbo|en"
        ));
        // Offline: the render fails, and is not tried again before the
        // backoff's first minute.
        down.store(true, Ordering::SeqCst);
        let (cache, calls, down) = service("Rachel", 2_400);
        down.store(true, Ordering::SeqCst);
        let mut renderer = Renderer::new(clips.clone(), cache);
        let new = wanted(STORE, &[(4, "Take your vitamins."), (6, "Call mum.")], &[]);
        assert!(renderer.pass(&new, start).failed);
        let tries = calls.load(Ordering::SeqCst);
        assert!(!renderer.pass(&new, start + WARM_EVERY / 2).failed);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            tries,
            "not before the backoff"
        );
        down.store(false, Ordering::SeqCst);
        assert_eq!(renderer.pass(&new, start + WARM_EVERY).rendered, 2);
        let _ = fs::remove_dir_all(&dir);
    }

    /// #8762's review, P3 1: a reminder's clip the person's Gemini voice
    /// failed (a 503, no sound in time) was rendered in the lamp's own
    /// voice and kept under theirs, so it rang offline in the lamp's voice
    /// for its whole life. A clip the lamp's voice said in theirs' place is
    /// not kept: the pass waits its backoff, and the next one asks theirs
    /// again and keeps that.
    #[test]
    fn a_clip_the_persons_gemini_voice_failed_is_not_kept_as_theirs() {
        let dir = scratch("gemini-failed");
        let clips = Clips::new(&dir);
        let down = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let off = Arc::clone(&down);
        let theirs: Render = Box::new(move |text: &str| {
            if off.load(Ordering::SeqCst) {
                return Err("HTTP 503".into());
            }
            Ok(vec![text.len() as i16; 2_400])
        });
        let (lamps, lamps_calls, _) = service("Rachel", 1_200);
        let cache = TtsCache::new(
            std::env::temp_dir().join("lamp-ring-clips-unused-cache"),
            "own-gemini:Kore@https://google.example.invalid",
            "gemini-tts",
            "en",
            theirs,
        )
        .giving_way_to(lamps, Box::new(|| false), "the person's own Gemini voice");
        let voice = cache.identity().to_owned();
        let mut renderer = Renderer::new(clips.clone(), cache);
        let start = Instant::now();
        let walk = wanted(STORE, &[(4, "Take your pills.")], &[]);
        let said = words(None, "Take your pills.");
        let pass = renderer.pass(&walk, start);
        assert_eq!(lamps_calls.load(Ordering::SeqCst), 1, "said in the lamp's");
        assert!(pass.failed && pass.rendered == 0, "{pass:?}");
        assert_eq!(clips.own(STORE, 4, &said, &voice), None);
        assert!(!clips.current(STORE, 4, &said, &voice));
        assert_eq!(clips.kept(STORE), BTreeSet::new());
        down.store(false, Ordering::SeqCst);
        assert_eq!(renderer.pass(&walk, start + WARM_EVERY).rendered, 1);
        assert_eq!(
            clips.own(STORE, 4, &said, &voice),
            Some(vec![said.len() as i16; 2_400]),
            "theirs, once Google answers"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The first builder's bound on #8708: the folder stops growing at its
    /// bound, one journal line for the run that reaches it, and the bound is
    /// the real worst case at the speech service's 24 kHz.
    #[test]
    fn the_clips_stop_rendering_at_their_bound_with_one_journal_line() {
        assert_eq!(SOUND_RATE, 24_000);
        assert_eq!(CLIP_MOST_BYTES, 960_044);
        assert_eq!(MOST_BYTES, 122_918_400);
        let dir = scratch("bound");
        let clips = Clips::new(&dir);
        // Each clip the largest kept, 20 s.
        let (cache, calls, _) = service("Rachel", MOST_FRAMES as usize);
        // Room for two of them.
        let mut renderer =
            Renderer::new(clips.clone(), cache).bounded(2 * (CLIP_MOST_BYTES + SIDECAR_MOST_BYTES));
        let start = Instant::now();
        let walk = wanted(
            STORE,
            &[(1, "One."), (2, "Two."), (3, "Three."), (4, "Four.")],
            &[],
        );
        let first = renderer.pass(&walk, start);
        assert!(first.full && first.full_told, "{first:?}");
        let kept = first.rendered;
        assert_eq!(kept, 2, "{first:?}");
        assert!(clips.used_bytes() <= 2 * (CLIP_MOST_BYTES + SIDECAR_MOST_BYTES));
        // Still full: not told again, nothing more rendered.
        let again = renderer.pass(&walk, start);
        assert!(
            again.full && !again.full_told && again.rendered == 0,
            "{again:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), kept);
        // Some go: rendered again, and a later fill is told again.
        let fewer = wanted(STORE, &[(3, "Three."), (4, "Four.")], &[]);
        let after = renderer.pass(&fewer, start);
        assert!(after.swept > 0 && after.rendered > 0, "{after:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Clips whose id left Schedule go at a walk's end, and every clip of a
    /// store that is no longer the lamp's.
    #[test]
    fn a_sweep_keeps_only_the_live_ids_of_the_store() {
        let dir = scratch("sweep");
        let clips = Clips::new(&dir);
        let (cache, _, _) = service("Rachel", 2_400);
        let mut renderer = Renderer::new(clips.clone(), cache);
        let start = Instant::now();
        renderer.pass(&wanted(STORE, &[(1, "One."), (2, "Two.")], &[]), start);
        assert_eq!(clips.kept(STORE), BTreeSet::from([1, 2]));
        renderer.pass(&wanted(STORE, &[(2, "Two.")], &[]), start);
        assert_eq!(clips.kept(STORE), BTreeSet::from([2]));
        renderer.pass(&wanted(OTHER_STORE, &[], &[]), start);
        assert!(clips.kept(STORE).is_empty());
        assert_eq!(clips.used_bytes(), 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
