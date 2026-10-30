//! The say service's speech cache (#3697): V1's `_TTS_CACHE_DIR`
//! (`hal/drivers/voice/tts/service.py`) for the lamp's own system lines. A
//! `text` asked with `"cache": true` by root's helpers is rendered once
//! through the speech service the voice already speaks with (the second
//! route's, ElevenLabs through the relay, its voice and model), kept as a
//! WAV named by the SHA-256 of `voice|model|language|text`, and played from
//! there on every later ask, with no network. [`WARM`] is rendered at start,
//! so the first shutdown is said from the cache even offline, and so is a
//! hand's hold's sleep line (#3748, `LAMP_SLEEP_LINE`).
//!
//! For an owner's language other than English (#4473 (c),
//! `crate::language`), the lamp's fixed lines in that language are warmed
//! first ([`TtsCache::warm_lines`]) and played from here by their English
//! words ([`TtsCache::owners_kept`]); a line not kept yet is said in
//! English, as before. English warms and plays exactly [`WARM`].
use super::SOUND_RATE;
use crate::language::Language;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

/// Where the rendered lines are kept: the voice's own state (root's, 0700).
pub const DIR: &str = "/var/lib/lampOS/voice/tts-cache";
/// At most this many lines are kept; past it a line is rendered and played
/// but not stored. Only the lamp's own fixed lines come here: [`WARM`] and
/// seven lines in each of four other languages (#4473 (c)), with room for a
/// change of voice.
pub const MOST_FILES: usize = 48;
/// The longest line kept: 20 s at the speaker's rate.
pub const MOST_FRAMES: u32 = SOUND_RATE * 20;
/// The longest one rendering may take.
pub const RENDER_WITHIN: Duration = Duration::from_secs(5);
/// Rendered at start, after the speech service is configured, until each is
/// kept: the safe shutdown's line, one source with the shutdown, then the
/// hold's sleep line (#3748), read by the coordinator with [`TtsCache::kept`].
pub const WARM: &[&str] = &[lamp_msgs::GOING_TO_SLEEP, lamp_msgs::SLEEP_LINE];
/// How soon an unrendered warm line is first tried again (offline at
/// start); each later try waits twice as long, up to [`WARM_EVERY_MOST`].
const WARM_EVERY: Duration = Duration::from_secs(60);
/// The longest wait between tries: a lamp offline for days asks the speech
/// service at most once an hour.
const WARM_EVERY_MOST: Duration = Duration::from_secs(3_600);

/// Why a line that rendered empty or too long is not played.
const REFUSED: &str = "not in the speech cache, and rendered empty or too long";

/// Renders a line to 24 kHz mono samples, or says why not.
pub type Render = Box<dyn Fn(&str) -> Result<Vec<i16>, String> + Send + Sync>;

pub struct TtsCache {
    dir: PathBuf,
    /// `voice|model|language`, the key's prefix.
    identity: String,
    render: Render,
    /// The owner's language, when not English (#4473 (c)).
    owners: Option<Language>,
    /// One render at a time, so a line asked while it warms is rendered once.
    /// Held while rendering, so a line asked while it warms is rendered
    /// once. It holds the lines that rendered empty or past [`MOST_FRAMES`]:
    /// a render is the same each time, so they are never sent again in this
    /// process (Thirteen's P2 on #4523).
    rendering: Mutex<std::collections::HashSet<String>>,
}

impl std::fmt::Debug for TtsCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TtsCache").field("dir", &self.dir).finish()
    }
}

impl TtsCache {
    pub fn new(
        dir: impl Into<PathBuf>,
        voice: &str,
        model: &str,
        language: &str,
        render: Render,
    ) -> Self {
        Self {
            dir: dir.into(),
            identity: format!("{voice}|{model}|{language}"),
            render,
            owners: None,
            rendering: Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// The owner's language, `None` for English: the fixed lines it warms
    /// and plays ([`Self::warm_lines`], [`Self::owners_kept`]).
    pub fn speaking(mut self, owners: Option<Language>) -> Self {
        self.owners = owners;
        self
    }

    /// The lines warmed at start: the owner's fixed lines first, when the
    /// owner's language is not English, then [`WARM`], the English fallback.
    pub fn warm_lines(&self) -> Vec<&'static str> {
        self.owners
            .into_iter()
            .flat_map(Language::fixed_lines)
            .chain(WARM.iter().copied())
            .collect()
    }

    /// The fixed line `english` in the owner's language, if it is kept:
    /// never rendered here. None for English, for a line with no
    /// translation, and for one not kept yet: the caller says the English.
    pub fn owners_kept(&self, english: &str) -> Option<Vec<i16>> {
        self.kept(self.owners_words(english)?.1)
    }

    /// The owner's language and its words for the fixed line `english`;
    /// none for English or a line with no translation.
    pub fn owners_words(&self, english: &str) -> Option<(Language, &'static str)> {
        let owners = self.owners?;
        Some((owners, owners.fixed(english)?))
    }

    /// The file name's stem: hex SHA-256 of `voice|model|language|text`.
    pub fn key(&self, text: &str) -> String {
        Sha256::digest(format!("{}|{text}", self.identity))
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn path(&self, text: &str) -> PathBuf {
        self.dir.join(format!("{}.wav", self.key(text)))
    }

    /// `text`'s samples: from the cache, or rendered once and kept.
    pub fn samples(&self, text: &str) -> Result<Vec<i16>, String> {
        let path = self.path(text);
        if let Some(samples) = read(&path) {
            return Ok(samples);
        }
        let mut refused = self
            .rendering
            .lock()
            .unwrap_or_else(|held| held.into_inner());
        if let Some(samples) = read(&path) {
            return Ok(samples);
        }
        if refused.contains(text) {
            return Err(REFUSED.into());
        }
        let samples = (self.render)(text)
            .map_err(|why| format!("not in the speech cache, and not rendered: {why}"))?;
        if samples.is_empty() || samples.len() > MOST_FRAMES as usize {
            eprintln!(
                "lamp-voice: a line for the speech cache rendered empty or past {} s: not asked again",
                MOST_FRAMES / SOUND_RATE
            );
            refused.insert(text.to_owned());
            return Err(REFUSED.into());
        }
        if let Err(why) = self.store(&path, &samples) {
            eprintln!("lamp-voice: a line rendered for the speech cache was not kept: {why}");
        }
        Ok(samples)
    }

    /// `text`'s samples if kept already, never rendered here: for a caller
    /// that must not wait on the speech service (#3748).
    pub fn kept(&self, text: &str) -> Option<Vec<i16>> {
        read(&self.path(text))
    }

    /// The sleep line (`lamp_msgs::SLEEP_LINE`, #3748) if kept: the owner's
    /// words when kept, else the English. Never rendered here. The one
    /// lookup for a hand's hold and a sleep asked aloud alike.
    pub fn sleep_line(&self) -> Option<Vec<i16>> {
        self.owners_kept(lamp_msgs::SLEEP_LINE)
            .or_else(|| self.kept(lamp_msgs::SLEEP_LINE))
    }

    /// Each of `lines` rendered and kept, unless it is already: every one
    /// tried, so one that fails (too long) never keeps the rest from being
    /// kept; the first failure is returned.
    ///
    /// A line that rendered empty or too long is done with: it is not
    /// kept, and not tried again, so it never holds the warming open.
    pub fn warm(&self, lines: &[&str]) -> Result<(), String> {
        lines
            .iter()
            .map(|line| match self.samples(line) {
                Err(why) if why == REFUSED => Ok(()),
                other => other.map(drop),
            })
            .fold(Ok(()), Result::and)
    }

    /// Written whole or not at all: the lamp is unplugged after a shutdown.
    fn store(&self, path: &Path, samples: &[i16]) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let kept = fs::read_dir(&self.dir)?
            .filter(|entry| {
                entry
                    .as_ref()
                    .is_ok_and(|entry| entry.path().extension().is_some_and(|e| e == "wav"))
            })
            .count();
        if kept >= MOST_FILES {
            return Err(std::io::Error::other(format!(
                "{MOST_FILES} lines kept already"
            )));
        }
        let mut bytes = Cursor::new(Vec::new());
        let mut writer =
            hound::WavWriter::new(&mut bytes, spec()).map_err(std::io::Error::other)?;
        for sample in samples {
            writer
                .write_sample(*sample)
                .map_err(std::io::Error::other)?;
        }
        writer.finalize().map_err(std::io::Error::other)?;
        let tmp = path.with_extension("tmp");
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes.get_ref())?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        fs::File::open(&self.dir)?.sync_all()
    }
}

fn spec() -> hound::WavSpec {
    hound::WavSpec {
        channels: 1,
        sample_rate: SOUND_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    }
}

/// A kept line, or none: missing or not one of ours is a miss.
fn read(path: &Path) -> Option<Vec<i16>> {
    let reader = hound::WavReader::open(path).ok()?;
    if reader.spec() != spec() || !(1..=MOST_FRAMES).contains(&reader.duration()) {
        return None;
    }
    reader.into_samples::<i16>().collect::<Result<_, _>>().ok()
}

/// The cache over the second route's speech service, when it is configured
/// (`LAMP_VOICE_SECOND_CONFIG`), with [`WARM`] rendered on a thread of its
/// own, tried again every minute until kept. Without it, none: a cached ask
/// is refused and its asker says the words another way. Made once per
/// process: the say service and the hold's sleep line (#3748) share it.
pub fn from_environment() -> Option<Arc<TtsCache>> {
    static SHARED: std::sync::OnceLock<Option<Arc<TtsCache>>> = std::sync::OnceLock::new();
    SHARED.get_or_init(made_from_environment).clone()
}

fn made_from_environment() -> Option<Arc<TtsCache>> {
    use crate::second_provider::net::{Tts, default_model, voice_name};
    let config = match crate::second_config::load_from_env().and_then(|config| {
        config
            .map(|config| config.with_environment(|name| std::env::var_os(name)))
            .transpose()
    }) {
        Ok(Some(config)) => config,
        Ok(None) => return None,
        Err(why) => {
            eprintln!("lamp-voice: no speech cache: {why}");
            return None;
        }
    };
    let voice = voice_name(config.tts_provider, config.tts_voice.as_deref()).to_owned();
    let model = config
        .tts_model
        .clone()
        .unwrap_or_else(|| default_model(config.tts_provider).to_owned());
    let language = config.stt_language.clone().unwrap_or_default();
    let render: Render = Box::new(move |text: &str| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|why| why.to_string())?;
        runtime.block_on(async {
            let whole = async {
                let tts = Tts::new(&config)?;
                let mut speech = tts.speak(text).await?;
                let mut samples = Vec::new();
                while let Some(packet) = speech.next().await? {
                    samples.extend_from_slice(&packet);
                }
                Ok::<_, crate::second_provider::net::Error>(samples)
            };
            match tokio::time::timeout(RENDER_WITHIN, whole).await {
                Ok(samples) => samples.map_err(|why| why.to_string()),
                Err(_) => Err(format!("not within {} s", RENDER_WITHIN.as_secs())),
            }
        })
    });
    let cache = Arc::new(
        TtsCache::new(DIR, &voice, &model, &language, render).speaking(crate::language::at_start()),
    );
    let warming = Arc::clone(&cache);
    let _ = std::thread::Builder::new()
        .name("say-tts-warm".into())
        .spawn(move || {
            let lines = warming.warm_lines();
            let mut told = false;
            let mut every = WARM_EVERY;
            while let Err(why) = warming.warm(&lines) {
                if !told {
                    eprintln!("lamp-voice: the speech cache's lines are not rendered yet, tried again after a minute, then less often, at most hourly: {why}");
                    told = true;
                }
                std::thread::sleep(every);
                every = (every * 2).min(WARM_EVERY_MOST);
            }
            eprintln!("lamp-voice: the speech cache holds its {} line(s)", lines.len());
        });
    Some(cache)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh folder of this test's own.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lamp-tts-cache-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn counted(dir: &Path, voice: &str) -> (TtsCache, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let render: Render = Box::new(move |text: &str| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(vec![text.len() as i16; 2_400])
        });
        (
            TtsCache::new(dir, voice, "eleven_v4_turbo", "en", render),
            calls,
        )
    }

    #[test]
    fn the_key_is_stable_for_one_line_and_voice_and_differs_otherwise() {
        let dir = scratch("the_key");
        let (one, _) = counted(&dir, "Rachel");
        let (other, _) = counted(&dir, "Sarah");
        let key = one.key(lamp_msgs::GOING_TO_SLEEP);
        assert_eq!(key.len(), 64);
        assert_eq!(key, one.key(lamp_msgs::GOING_TO_SLEEP));
        assert_ne!(key, one.key("Hello."));
        assert_ne!(key, other.key(lamp_msgs::GOING_TO_SLEEP));
        let render: Render = Box::new(|_: &str| Ok(vec![]));
        let model = TtsCache::new(&dir, "Rachel", "eleven_flash_v2_5", "en", render);
        assert_ne!(key, model.key(lamp_msgs::GOING_TO_SLEEP));
    }

    #[test]
    fn a_miss_renders_once_and_keeps_it_whole_and_a_hit_never_renders() {
        let dir = scratch("a_miss");
        let (cache, calls) = counted(&dir, "Rachel");
        let first = cache.samples("Hello.").unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let kept: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            kept,
            [std::ffi::OsString::from(format!(
                "{}.wav",
                cache.key("Hello.")
            ))]
        );
        // A fresh cache over the same folder: the voice started again, offline.
        let (again, calls) = counted(&dir, "Rachel");
        assert_eq!(again.samples("Hello.").unwrap(), first);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_broken_file_is_a_miss_and_a_failed_render_is_said() {
        let dir = scratch("a_broken");
        let render: Render = Box::new(|_: &str| Err("offline".into()));
        let cache = TtsCache::new(&dir, "Rachel", "m", "en", render);
        fs::write(cache.path("Hi."), b"RIFF cut short").unwrap();
        assert_eq!(
            cache.samples("Hi.").unwrap_err(),
            "not in the speech cache, and not rendered: offline"
        );
    }

    #[test]
    fn warm_renders_the_shutdown_line_and_the_sleep_line_once() {
        assert_eq!(WARM, [lamp_msgs::GOING_TO_SLEEP, lamp_msgs::SLEEP_LINE]);
        let dir = scratch("warm");
        let (cache, calls) = counted(&dir, "Rachel");
        assert_eq!(cache.kept(lamp_msgs::SLEEP_LINE), None);
        cache.warm(WARM).unwrap();
        cache.warm(WARM).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(cache.path(lamp_msgs::GOING_TO_SLEEP).is_file());
        assert_eq!(
            cache.kept(lamp_msgs::SLEEP_LINE),
            Some(vec![lamp_msgs::SLEEP_LINE.len() as i16; 2_400])
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2, "kept never renders");
    }

    /// #4473 (c): English warms exactly [`WARM`] and has no line of the
    /// owner's; another language warms its seven fixed lines first, then
    /// [`WARM`] for the fallback, each under a key of its own words.
    #[test]
    fn the_owners_language_warms_its_own_lines_first_and_english_is_as_today() {
        let dir = scratch("owners");
        let (english, _) = counted(&dir, "Rachel");
        assert_eq!(english.warm_lines(), WARM);
        english.warm(&english.warm_lines()).unwrap();
        assert_eq!(english.owners_kept(lamp_msgs::SLEEP_LINE), None);
        assert_eq!(english.owners_words(lamp_msgs::SLEEP_LINE), None);
        for language in [
            Language::Vietnamese,
            Language::Japanese,
            Language::Chinese,
            Language::Korean,
        ] {
            let dir = scratch(&format!("owners-{}", language.code()));
            let (cache, calls) = counted(&dir, "Rachel");
            let cache = cache.speaking(Some(language));
            let lines = cache.warm_lines();
            assert_eq!(lines.len(), 8 + WARM.len(), "{language:?}");
            assert_eq!(&lines[8..], WARM);
            let words = language.fixed(lamp_msgs::SLEEP_LINE).unwrap();
            assert_eq!(lines[1], words);
            assert_ne!(cache.key(words), cache.key(lamp_msgs::SLEEP_LINE));
            // Not kept yet: none, never rendered here; the caller says English.
            assert_eq!(cache.owners_kept(lamp_msgs::SLEEP_LINE), None);
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            cache.warm(&lines).unwrap();
            assert_eq!(
                cache.owners_kept(lamp_msgs::SLEEP_LINE),
                Some(vec![words.len() as i16; 2_400])
            );
            assert_eq!(cache.owners_kept("Hello."), None, "not a fixed line");
        }
    }

    /// One line that cannot be kept never keeps the rest from being kept.
    #[test]
    fn warm_tries_every_line_and_returns_the_first_failure() {
        let dir = scratch("warm_every");
        let render: Render = Box::new(|text: &str| {
            if text == "too long" {
                Err("too long".into())
            } else {
                Ok(vec![1; 2_400])
            }
        });
        let cache = TtsCache::new(&dir, "Rachel", "m", "en", render);
        assert!(cache.warm(&["too long", "Hi."]).is_err());
        assert!(cache.kept("Hi.").is_some());
    }

    /// Thirteen's P2 on #4523: a line that renders past the cap is sent to
    /// the speech service once, not on every round, and the warming ends;
    /// a failed render (offline) is tried again.
    #[test]
    fn a_line_rendered_too_long_is_asked_for_once() {
        let dir = scratch("too_long");
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let offline = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let down = Arc::clone(&offline);
        let render: Render = Box::new(move |text: &str| {
            seen.fetch_add(1, Ordering::SeqCst);
            if text == "Hi." && down.load(Ordering::SeqCst) {
                Err("offline".into())
            } else if text == "long" {
                Ok(vec![1; MOST_FRAMES as usize + 1])
            } else {
                Ok(vec![1; 2_400])
            }
        });
        let cache = TtsCache::new(&dir, "Rachel", "m", "en", render);
        assert!(cache.warm(&["long", "Hi."]).is_err(), "Hi. is offline");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        offline.store(false, Ordering::SeqCst);
        cache.warm(&["long", "Hi."]).unwrap();
        cache.warm(&["long", "Hi."]).unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "long asked once, Hi. twice"
        );
        assert_eq!(cache.samples("long").unwrap_err(), REFUSED);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!(cache.kept("long").is_none());
    }

    #[test]
    fn past_the_most_files_a_line_is_played_but_not_kept() {
        let dir = scratch("past");
        let (cache, calls) = counted(&dir, "Rachel");
        for n in 0..MOST_FILES {
            cache.samples(&format!("line {n}")).unwrap();
        }
        assert!(cache.samples("one more").is_ok());
        assert!(!cache.path("one more").exists());
        assert_eq!(calls.load(Ordering::SeqCst), MOST_FILES + 1);
    }
}
