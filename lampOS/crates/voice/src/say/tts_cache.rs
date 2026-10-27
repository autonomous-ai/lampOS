//! The say service's speech cache (#3697): V1's `_TTS_CACHE_DIR`
//! (`hal/drivers/voice/tts/service.py`) for the lamp's own system lines. A
//! `text` asked with `"cache": true` by root's helpers is rendered once
//! through the speech service the voice already speaks with (the second
//! route's, ElevenLabs through the relay, its voice and model), kept as a
//! WAV named by the SHA-256 of `voice|model|language|text`, and played from
//! there on every later ask, with no network. [`WARM`] is rendered at start,
//! so the first shutdown is said from the cache even offline.
use super::SOUND_RATE;
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
/// but not stored. Only root's cache-flagged lines come here, a handful.
pub const MOST_FILES: usize = 16;
/// The longest line kept: 20 s at the speaker's rate.
pub const MOST_FRAMES: u32 = SOUND_RATE * 20;
/// The longest one rendering may take.
pub const RENDER_WITHIN: Duration = Duration::from_secs(5);
/// Rendered at start, after the speech service is configured, until each is
/// kept: the safe shutdown's line, one source with the shutdown.
pub const WARM: &[&str] = &[lamp_msgs::GOING_TO_SLEEP];
/// How often an unrendered [`WARM`] line is tried again (offline at start).
const WARM_EVERY: Duration = Duration::from_secs(60);

/// Renders a line to 24 kHz mono samples, or says why not.
pub type Render = Box<dyn Fn(&str) -> Result<Vec<i16>, String> + Send + Sync>;

pub struct TtsCache {
    dir: PathBuf,
    /// `voice|model|language`, the key's prefix.
    identity: String,
    render: Render,
    /// One render at a time, so a line asked while it warms is rendered once.
    rendering: Mutex<()>,
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
            rendering: Mutex::new(()),
        }
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
        let _one = self
            .rendering
            .lock()
            .unwrap_or_else(|held| held.into_inner());
        if let Some(samples) = read(&path) {
            return Ok(samples);
        }
        let samples = (self.render)(text)
            .map_err(|why| format!("not in the speech cache, and not rendered: {why}"))?;
        if samples.is_empty() || samples.len() > MOST_FRAMES as usize {
            return Err("not in the speech cache, and rendered empty or too long".into());
        }
        if let Err(why) = self.store(&path, &samples) {
            eprintln!("lamp-voice: a line rendered for the speech cache was not kept: {why}");
        }
        Ok(samples)
    }

    /// Each of `lines` rendered and kept, unless it is already.
    pub fn warm(&self, lines: &[&str]) -> Result<(), String> {
        lines
            .iter()
            .try_for_each(|line| self.samples(line).map(drop))
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
/// is refused and its asker says the words another way.
pub fn from_environment() -> Option<Arc<TtsCache>> {
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
    let cache = Arc::new(TtsCache::new(DIR, &voice, &model, &language, render));
    let warming = Arc::clone(&cache);
    let _ = std::thread::Builder::new()
        .name("say-tts-warm".into())
        .spawn(move || {
            let mut told = false;
            while let Err(why) = warming.warm(WARM) {
                if !told {
                    eprintln!("lamp-voice: the speech cache's lines are not rendered yet, tried each minute: {why}");
                    told = true;
                }
                std::thread::sleep(WARM_EVERY);
            }
            eprintln!("lamp-voice: the speech cache holds its {} line(s)", WARM.len());
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
    fn warm_renders_the_shutdown_line_once() {
        let dir = scratch("warm");
        let (cache, calls) = counted(&dir, "Rachel");
        cache.warm(WARM).unwrap();
        cache.warm(WARM).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(cache.path(lamp_msgs::GOING_TO_SLEEP).is_file());
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
