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
//! English, as before. English warms and plays exactly [`WARM`]. A full
//! cache makes room by its oldest files, never the owner's current lines
//! (#4883, [`MOST_FILES`]).
use super::SOUND_RATE;
use crate::language::{Language, WOKEN_BY_NAME_LINE};
use crate::speaker_stream::Feed;
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
/// At most this many lines are kept; past it the oldest go first, never the
/// owner's current lines (#4883, [`TtsCache::make_room`]): [`WARM`] and the
/// fixed lines in the owner's language (#4473 (c), 45 since #5999's skill
/// lines), with room for a change of voice or language and the lines asked
/// since. An installed item's first-run line adds two files of the Store's
/// text per install (`crate::install_told::arrived`, #7814), kept after it
/// is said; they are never among the protected lines, so they go first.
pub const MOST_FILES: usize = 64;
/// The longest line kept: 20 s at the speaker's rate.
pub const MOST_FRAMES: u32 = SOUND_RATE * 20;
/// The longest one rendering of a short line may take: a line of up to
/// [`RENDER_SHORT_CHARS`] characters, every cached line ([`WARM`], the fixed
/// lines) and most exact ones.
pub const RENDER_WITHIN: Duration = Duration::from_secs(5);
/// A line up to this many characters (about 8 s of speech at ~15 a second)
/// is given [`RENDER_WITHIN`], as before (#7742).
pub const RENDER_SHORT_CHARS: usize = 120;
/// Past [`RENDER_SHORT_CHARS`], each started this many characters (about
/// 2 s of speech) adds 1 s: a render at twice the speed it plays, the pace
/// the first 120 characters' 5 s already asks, connection included. A
/// skill's agent's two-sentence answer (363 bytes on lamp-c0a4, #7742) was
/// refused whole at 5 s and nothing was said.
pub const RENDER_MORE_CHARS: usize = 30;
/// The longest any render may take: as long as the longest line kept or
/// played ([`MOST_FRAMES`], 20 s) plays, so a render slower than the line
/// itself is still refused. Reached at 570 characters, past what fits in
/// [`MOST_FRAMES`] at the voice's pace.
pub const RENDER_MOST: Duration = Duration::from_secs((MOST_FRAMES / SOUND_RATE) as u64);
/// The most characters of an exact line rendered as one piece (#7892):
/// half of what plays in [`MOST_FRAMES`] at the voice's pace
/// ([`RENDER_MORE_CHARS`] in about 2 s), so a slower voice's piece still
/// renders within it. A longer line is rendered in pieces
/// ([`TtsCache::render_once`]); the cap stays [`MOST_FRAMES`] for every
/// render.
pub const MOST_PIECE_CHARS: usize = (MOST_FRAMES / SOUND_RATE) as usize * RENDER_MORE_CHARS / 2 / 2;
/// Rendered at start, after the speech service is configured, until each is
/// kept: the safe shutdown's line, one source with the shutdown, then the
/// hold's sleep line (#3748), read by the coordinator with [`TtsCache::kept`],
/// then the factory reset's line (#5929), said as the reset button is let go.
pub const WARM: &[&str] = &[
    lamp_msgs::GOING_TO_SLEEP,
    lamp_msgs::SLEEP_LINE,
    lamp_msgs::RESET_STARTING,
];
/// How soon an unrendered warm line is first tried again (offline at
/// start); each later try waits twice as long, up to [`WARM_EVERY_MOST`].
pub(crate) const WARM_EVERY: Duration = Duration::from_secs(60);
/// The longest wait between tries: a lamp offline for days asks the speech
/// service at most once an hour.
pub(crate) const WARM_EVERY_MOST: Duration = Duration::from_secs(3_600);

/// Why a line that rendered empty or too long is not played.
const REFUSED: &str = "not in the speech cache, and rendered empty or too long";

/// Renders a line to 24 kHz mono samples, or says why not.
pub type Render = Box<dyn Fn(&str) -> Result<Vec<i16>, String> + Send + Sync>;

/// Renders a line in another of the lamp's voices, by its name
/// (`lamp_msgs::chosen_voice::ELEVENLABS_VOICES`), or says why not: the
/// app's voice preview (#7485), one line, the voice in use left as it is.
pub type RenderIn = Box<dyn Fn(&str, &str) -> Result<Vec<i16>, String> + Send + Sync>;

/// Whether the person's own voice was refused in this run (#8367).
pub type Lost = Box<dyn Fn() -> bool + Send + Sync>;

/// The lamp's own voice behind the person's (#8367, `LAMP_OWN_KEYS`,
/// [`TtsCache::falling_back_to`]): its own lines under its own key.
struct Lamps {
    cache: TtsCache,
    lost: Lost,
}

/// The voice part of the cache's key: `voice` as it is, or, for the
/// person's own voice (#8367), with the address it is asked at (`own_at`),
/// so their lines are never the lamp's, another voice ID's or another
/// address's.
pub fn own_voice(voice: &str, own_at: Option<&str>) -> String {
    match own_at {
        Some(at) => format!("own:{voice}@{at}"),
        None => voice.to_owned(),
    }
}

pub struct TtsCache {
    dir: PathBuf,
    /// `voice|model|language`, the key's prefix.
    identity: String,
    render: Render,
    /// [`RenderIn`], when the speech service can speak in another voice:
    /// none, a line in another voice is refused.
    render_in: Option<RenderIn>,
    /// The owner's language, when not English (#4473 (c)).
    owners: Option<Language>,
    /// `LAMP_SLEEP_HEARS_NAME=on` (migrate-P2): the line a wake by name says
    /// is warmed ([`Self::wake_line`]). Off, it is neither warmed nor kept
    /// from going.
    hears_name: bool,
    /// [`Self::keeping_outage_lines`].
    outage_lines: bool,
    /// [`Self::keeping_offline_rings`].
    offline_rings: bool,
    /// One render at a time, so a line asked while it warms is rendered once.
    /// Held while rendering, so a line asked while it warms is rendered
    /// once. It holds the lines that rendered empty or past [`MOST_FRAMES`]:
    /// a render is the same each time, so they are never sent again in this
    /// process (Thirteen's P2 on #4523).
    rendering: Mutex<std::collections::HashSet<String>>,
    /// The lamp's own voice, when this one is the person's (#8367).
    lamps: Option<Box<Lamps>>,
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
            render_in: None,
            owners: None,
            hears_name: false,
            outage_lines: false,
            offline_rings: false,
            rendering: Mutex::new(std::collections::HashSet::new()),
            lamps: None,
        }
    }

    /// `voice|model|language`, what a line is rendered in: an offline
    /// ring's clip is kept with it (`crate::ring_clips`, #8708).
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// The owner's language, `None` for English ([`Self::speaking`]).
    pub fn owners(&self) -> Option<Language> {
        self.owners
    }

    /// This cache speaks in the person's own voice (#8367), with `lamps`,
    /// the lamp's own, over the same folder: once `lost` says the person's
    /// was refused, every line is the lamp's, kept under the lamp's key,
    /// never theirs.
    pub fn falling_back_to(mut self, lamps: TtsCache, lost: Lost) -> Self {
        self.lamps = Some(Box::new(Lamps { cache: lamps, lost }));
        self
    }

    /// The owner's language, `None` for English: the fixed lines it warms
    /// and plays ([`Self::warm_lines`], [`Self::owners_kept`]).
    pub fn speaking(mut self, owners: Option<Language>) -> Self {
        self.owners = owners;
        self
    }

    /// Lines in another of the lamp's voices are rendered by `render_in`
    /// ([`Self::render_once_in`], #7485).
    pub fn speaking_in(mut self, render_in: RenderIn) -> Self {
        self.render_in = Some(render_in);
        self
    }

    /// The lamp hears its name while it sleeps (migrate-P2): the line that
    /// wake says is warmed too.
    pub fn hearing_its_name(mut self, on: bool) -> Self {
        self.hears_name = on;
        self
    }

    /// `LAMP_OUTAGE_LINE_AGAIN=on` (#8235): an English lamp warms the
    /// failed turn's line (`language::TURN_LOST`), which its release has no
    /// file for, and the second route's down line, said by a turn of the
    /// service's outage; another language warms both as fixed lines.
    pub fn keeping_outage_lines(mut self, on: bool) -> Self {
        self.outage_lines = on;
        self
    }

    /// `LAMP_OFFLINE_RINGS=on` (#8632): the offline ring's two lines,
    /// "Your timer is done." and "You have a reminder.", are warmed in the
    /// owner's words ([`crate::language::ring_words`]), so a timer or
    /// reminder due with no link rings from here ([`Self::ring_clip`]).
    pub fn keeping_offline_rings(mut self, on: bool) -> Self {
        self.offline_rings = on;
        self
    }

    /// The offline ring's line for a timer (`timer`) or a reminder, if it
    /// is kept: the owner's words, else the English; never rendered here,
    /// so a ring never waits on the network (#8632). None without
    /// [`Self::keeping_offline_rings`].
    pub fn ring_clip(&self, timer: bool) -> Option<Vec<i16>> {
        if !self.offline_rings {
            return None;
        }
        let words = crate::language::ring_words(self.owners, timer);
        self.kept(words)
            .or_else(|| self.kept(crate::language::ring_words(None, timer)))
    }

    /// The lines warmed at start: the owner's fixed lines first, when the
    /// owner's language is not English, then [`WARM`], the English fallback,
    /// and with [`Self::hearing_its_name`] the wake's English on an English
    /// lamp. Without it the wake's line is no line of this cache's.
    pub fn warm_lines(&self) -> Vec<&'static str> {
        self.owners
            .into_iter()
            .flat_map(Language::fixed_lines)
            .chain(WARM.iter().copied())
            .chain(self.hears_name.then(|| self.wake_words()))
            .chain(
                (self.outage_lines && self.owners.is_none())
                    .then_some([
                        crate::language::TURN_LOST,
                        crate::second_provider::DOWN_LINE,
                    ])
                    .into_iter()
                    .flatten(),
            )
            .chain(
                self.offline_rings
                    .then(|| {
                        [true, false].map(|timer| crate::language::ring_words(self.owners, timer))
                    })
                    .into_iter()
                    .flatten(),
            )
            .collect()
    }

    /// What a wake by name says, if it is kept (migrate-P2): one file read,
    /// never rendered, so a wake never waits on the network. In the owner's
    /// words only: a line not kept yet is the "hm?", not English to someone
    /// who speaks another language.
    pub fn wake_line(&self) -> Option<Vec<i16>> {
        self.kept(self.wake_words())
    }

    fn wake_words(&self) -> &'static str {
        self.owners
            .map_or(WOKEN_BY_NAME_LINE, Language::woken_by_name)
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
        key_of(&self.identity, text)
    }

    fn path(&self, text: &str) -> PathBuf {
        path_in(&self.dir, &self.identity, text)
    }

    /// `text`'s samples: from the cache, or rendered once and kept, in the
    /// voice the lamp speaks in now ([`Self::in_voice`]).
    pub fn samples(&self, text: &str) -> Result<Vec<i16>, String> {
        self.in_voice(|cache| cache.samples_here(text))
    }

    /// `say` in the voice the lamp speaks in now (#8367): the person's own
    /// until it is refused, then the lamp's own. A line the person's voice
    /// was refused for is said again in the lamp's, and kept, if at all,
    /// under the lamp's key only: the person's render has no fallback of its
    /// own, so it never hands back the lamp's voice to keep under theirs.
    fn in_voice<T>(&self, say: impl Fn(&TtsCache) -> Result<T, String>) -> Result<T, String> {
        let Some(lamps) = &self.lamps else {
            return say(self);
        };
        if !(lamps.lost)() {
            match say(self) {
                Err(_) if (lamps.lost)() => {}
                said => return said,
            }
        }
        say(&lamps.cache)
    }

    /// [`Self::samples`] in this cache's own voice only.
    ///
    /// `rendering` is held across the render, network included (up to
    /// [`render_within`]), so one line is rendered once: another uncached
    /// caller waits behind it, e.g. a cached `say` behind an install's
    /// first-run render on its listener thread (`crate::install_told`).
    /// Never the coordinator's loop, which only reads kept lines
    /// ([`Self::kept`]).
    fn samples_here(&self, text: &str) -> Result<Vec<i16>, String> {
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

    /// `text` rendered now through the speech service, never kept: a paired
    /// computer's exact words (`exact`, lamp-control's `say`), which are
    /// not the lamp's own fixed lines and must not fill [`MOST_FILES`]. Or
    /// why not: the service's refusal, or a piece empty or past
    /// [`MOST_FRAMES`]. A line past [`MOST_PIECE_CHARS`] is rendered in
    /// pieces ([`pieces`], #7892) and played as one line, every piece in
    /// order.
    pub fn render_once(&self, text: &str) -> Result<Vec<i16>, String> {
        self.in_voice(|cache| in_pieces(text, |piece| (cache.render)(piece)))
    }

    /// `text` rendered now in the lamp's voice named `voice`, never kept:
    /// the app's voice preview (#7485), which must not fill [`MOST_FILES`]
    /// with lines in voices the lamp does not speak in. Or why not, as
    /// [`Self::render_once`] says it.
    pub fn render_once_in(&self, text: &str, voice: &str) -> Result<Vec<i16>, String> {
        let render_in = self
            .render_in
            .as_ref()
            .ok_or_else(|| "this lamp's speech service speaks in its own voice only".to_owned())?;
        in_pieces(text, |piece| render_in(piece, voice))
    }

    /// `text` rendered now, never kept, to be played as it comes (#7892,
    /// V2's 12:35 on #7897), in `voice` when one is given (the app's voice
    /// preview, as [`Self::render_once_in`]). A line within
    /// [`MOST_PIECE_CHARS`] is rendered whole, as before. A longer one's
    /// first piece is rendered here, so its failure refuses the line as
    /// before, and played at once from a feed of its own; the pieces after
    /// it are rendered on a thread of their own ([`Self::render_rest`]),
    /// each while the one before it plays.
    pub fn render_played(
        self: &Arc<Self>,
        text: &str,
        voice: Option<&str>,
    ) -> Result<Played, String> {
        let pieces = pieces(text);
        if pieces.len() == 1 {
            return match voice {
                Some(voice) => self.render_once_in(text, voice),
                None => self.render_once(text),
            }
            .map(Played::Whole);
        }
        let first = self.render_piece(&pieces[0], voice)?;
        // Room for the piece playing and the one rendered after it, each
        // within the cap.
        let feed = Feed::for_line(2 * MOST_FRAMES as usize);
        feed.push(&first);
        let cache = Arc::clone(self);
        let rest = pieces[1..].to_vec();
        let voice = voice.map(str::to_owned);
        let fed = Arc::clone(&feed);
        let spawned = std::thread::Builder::new()
            .name("say-pieces".into())
            .spawn(move || cache.render_rest(&rest, voice.as_deref(), first.len(), &fed));
        if let Err(why) = spawned {
            eprintln!("lamp-voice: a long exact line's later pieces are not rendered: {why}");
            feed.close_short();
        }
        Ok(Played::Pieces(Arc::new(Pieces { feed })))
    }

    /// One piece rendered, in `voice` when given, or why not, as
    /// [`Self::render_once`] says it.
    fn render_piece(&self, piece: &str, voice: Option<&str>) -> Result<Vec<i16>, String> {
        match voice {
            None => self.in_voice(|cache| in_pieces(piece, |piece| (cache.render)(piece))),
            Some(voice) => {
                let render_in = self.render_in.as_ref().ok_or_else(|| {
                    "this lamp's speech service speaks in its own voice only".to_owned()
                })?;
                in_pieces(piece, |piece| render_in(piece, voice))
            }
        }
    }

    /// The pieces after the first, rendered in turn into `feed`, in order.
    /// Each is rendered once all before the last one put in was taken to
    /// play (`last` its samples): while that one plays, so at most one
    /// piece waits rendered and the next is ready before it ends. Stopped,
    /// and nothing more rendered, once nobody plays the line
    /// ([`Feed::abandoned`]); a piece not rendered ends the line there,
    /// not said whole ([`Feed::close_short`]).
    fn render_rest(&self, rest: &[String], voice: Option<&str>, mut last: usize, feed: &Feed) {
        for piece in rest {
            while feed.held_samples() > last && !feed.abandoned() {
                std::thread::sleep(PIECE_LOOK);
            }
            if feed.abandoned() {
                return;
            }
            match self.render_piece(piece, voice) {
                Ok(_) if feed.abandoned() => return,
                Ok(samples) => {
                    last = samples.len();
                    feed.push(&samples);
                }
                Err(why) => {
                    eprintln!(
                        "lamp-voice: a long exact line's next piece was {why}: the line ends there"
                    );
                    feed.close_short();
                    return;
                }
            }
        }
        feed.close();
    }

    /// `text`'s samples if kept already, never rendered here: for a caller
    /// that must not wait on the speech service (#3748). In the person's own
    /// voice (#8367), the lamp's line while theirs is not kept yet, and only
    /// the lamp's once theirs was refused.
    pub fn kept(&self, text: &str) -> Option<Vec<i16>> {
        match &self.lamps {
            Some(lamps) if (lamps.lost)() => lamps.cache.kept(text),
            Some(lamps) => read(&self.path(text)).or_else(|| lamps.cache.kept(text)),
            None => read(&self.path(text)),
        }
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
        self.make_room()?;
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

impl TtsCache {
    /// Below [`MOST_FILES`] before a line is written (#4883): past it, the
    /// oldest files go first, never one of the owner's current lines
    /// ([`Self::warm_lines`] under this voice, model and language).
    ///
    /// A file's voice, model and language cannot be read from its hashed
    /// name, and a cache left full by an older voice has no index (lamp-52e6,
    /// 2 Nov), so no index is kept: the protected set is computed from the
    /// lines this cache warms, and every other file may go. A switch of
    /// voice or language writes the new lines after the old ones, so oldest
    /// first drops the other voices' and languages' lines before a line
    /// asked since. The owner's lines are at most 22, so room is always
    /// found.
    fn make_room(&self) -> std::io::Result<()> {
        let owners: std::collections::HashSet<_> = self
            .warm_lines()
            .iter()
            .map(|line| format!("{}.wav", self.key(line)))
            .collect();
        let mut kept = 0;
        let mut others = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let name = entry.file_name();
            if !name.to_str().is_some_and(|name| name.ends_with(".wav")) {
                continue;
            }
            kept += 1;
            if !owners.contains(name.to_str().unwrap_or_default()) {
                let at = entry.metadata()?.modified()?;
                others.push((at, entry.path()));
            }
        }
        if kept < MOST_FILES {
            return Ok(());
        }
        others.sort();
        let over = kept + 1 - MOST_FILES;
        if others.len() < over {
            return Err(std::io::Error::other(format!(
                "{MOST_FILES} lines kept already, all the owner's"
            )));
        }
        for (_, path) in &others[..over] {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

/// A kept clip's format: 24 kHz mono 16-bit, as the speech cache keeps its
/// lines (an offline ring's clip too, `crate::ring_clips`).
pub(crate) fn clip_spec() -> hound::WavSpec {
    spec()
}

/// A kept clip at `path`, or none: missing, not one of ours, or past
/// [`MOST_FRAMES`] (`crate::ring_clips`, #8708).
pub(crate) fn read_clip(path: &Path) -> Option<Vec<i16>> {
    read(path)
}

fn spec() -> hound::WavSpec {
    hound::WavSpec {
        channels: 1,
        sample_rate: SOUND_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    }
}

/// The file name's stem of `text` under `identity` ([`TtsCache::key`]).
fn key_of(identity: &str, text: &str) -> String {
    Sha256::digest(format!("{identity}|{text}"))
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn path_in(dir: &Path, identity: &str, text: &str) -> PathBuf {
    dir.join(format!("{}.wav", key_of(identity, text)))
}

/// `text`'s samples if a cache in `dir` keeps them under `identity`
/// (`voice|model|language`, [`TtsCache::identity`]), never rendered: for the
/// second route's worker, which keeps no cache of its own and says an
/// installed item's first-run line from here (`crate::install_told`).
pub fn kept_under(dir: &Path, identity: &str, text: &str) -> Option<Vec<i16>> {
    read(&path_in(dir, identity, text))
}

/// The identity `config` speaks as (`voice|model|language`), as the cache
/// [`built_from_environment`] is keyed: the person's own voice
/// ([`own_voice`], #8367) when the lamp's is kept apart from it, else the
/// lamp's. The second route's worker reads a line kept by the voice with it
/// ([`kept_under`]).
pub fn identity_of(config: &crate::second_config::SecondConfig) -> String {
    use crate::second_provider::net::{default_model, voice_name};
    let lamps = lamps_config(config);
    let voice = if config.hosted.lamps_voice.is_some() {
        own_voice(
            voice_name(config.tts_provider, config.tts_voice.as_deref()),
            Some(&config.tts_base_url),
        )
    } else {
        voice_name(lamps.tts_provider, lamps.tts_voice.as_deref()).to_owned()
    };
    let model = lamps
        .tts_model
        .clone()
        .unwrap_or_else(|| default_model(lamps.tts_provider).to_owned());
    let language = lamps.stt_language.clone().unwrap_or_default();
    format!("{voice}|{model}|{language}")
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

/// `text` rendered by `render` piece by piece ([`pieces`]), the samples
/// one after another, or why not: the service's refusal, or a piece empty
/// or past [`MOST_FRAMES`], the cap of every render.
fn in_pieces(
    text: &str,
    render: impl Fn(&str) -> Result<Vec<i16>, String>,
) -> Result<Vec<i16>, String> {
    let mut samples = Vec::new();
    for piece in pieces(text) {
        let rendered = render(&piece).map_err(|why| format!("not rendered: {why}"))?;
        if rendered.is_empty() || rendered.len() > MOST_FRAMES as usize {
            return Err(format!(
                "rendered empty or past {} s",
                MOST_FRAMES / SOUND_RATE
            ));
        }
        samples.extend_from_slice(&rendered);
    }
    Ok(samples)
}

/// A long exact line's sound (#7892): played from its feed as its pieces
/// come ([`TtsCache::render_played`]). Let go, by the last say or turn
/// that holds it (the line cut, refused, expired or ended), its renderer
/// stops and no later piece is rendered or played.
#[derive(Debug)]
pub struct Pieces {
    feed: Arc<Feed>,
}

impl Pieces {
    /// The feed its pieces come through, for the speaker.
    pub fn feed(&self) -> Arc<Feed> {
        Arc::clone(&self.feed)
    }
}

impl Drop for Pieces {
    fn drop(&mut self) {
        self.feed.abandon();
    }
}

/// A line's pieces are their own: equal only to themselves.
impl PartialEq for Pieces {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for Pieces {}

/// An exact line rendered to play ([`TtsCache::render_played`]): whole, or
/// in pieces that come as they are rendered.
#[derive(Debug)]
pub enum Played {
    Whole(Vec<i16>),
    Pieces(Arc<Pieces>),
}

/// How often a long line's renderer looks whether its last piece began.
const PIECE_LOOK: Duration = Duration::from_millis(10);

/// Where a long line is cut, coarsest first: sentence ends, then clause
/// ends; past both, between words.
const MARKS: [&[char]; 2] = [&['.', '!', '?'], &[',', ';', ':']];

/// An exact `line` as it is rendered (#7892): whole within
/// [`MOST_PIECE_CHARS`], else in pieces within it, cut at sentence ends,
/// then clause ends, then between words, every word kept in its order.
pub fn pieces(line: &str) -> Vec<String> {
    split(line, &MARKS)
}

/// `text` within [`MOST_PIECE_CHARS`]: whole when it fits, else its units
/// (its words up to each one ending in one of `marks`' first; each word,
/// with no marks left) packed in order, a unit past it split again by the
/// finer marks. A word alone past it is cut between its characters.
fn split(text: &str, marks: &[&[char]]) -> Vec<String> {
    let count = |text: &str| text.chars().count();
    if count(text) <= MOST_PIECE_CHARS {
        return vec![text.to_owned()];
    }
    let units: Vec<String> = match marks.first() {
        Some(ends) => {
            let mut units = Vec::new();
            let mut unit = String::new();
            for word in text.split_whitespace() {
                if !unit.is_empty() {
                    unit.push(' ');
                }
                unit.push_str(word);
                if word.ends_with(*ends) {
                    units.push(std::mem::take(&mut unit));
                }
            }
            if !unit.is_empty() {
                units.push(unit);
            }
            units
        }
        None if !text.contains(char::is_whitespace) => {
            let chars: Vec<char> = text.chars().collect();
            return chars
                .chunks(MOST_PIECE_CHARS)
                .map(|cut| cut.iter().collect())
                .collect();
        }
        None => text.split_whitespace().map(str::to_owned).collect(),
    };
    let finer = marks.get(1..).unwrap_or_default();
    let mut said = Vec::new();
    let mut piece = String::new();
    for part in units.iter().flat_map(|unit| split(unit, finer)) {
        if !piece.is_empty() && count(&piece) + 1 + count(&part) > MOST_PIECE_CHARS {
            said.push(std::mem::take(&mut piece));
        }
        if !piece.is_empty() {
            piece.push(' ');
        }
        piece.push_str(&part);
    }
    if !piece.is_empty() {
        said.push(piece);
    }
    said
}

/// How long rendering `text` whole may take: [`RENDER_WITHIN`] for a short
/// line, 1 s more for each started [`RENDER_MORE_CHARS`] past
/// [`RENDER_SHORT_CHARS`], at most [`RENDER_MOST`]. Counted in characters,
/// not bytes, so a line in Vietnamese is given the time its English would.
pub fn render_within(text: &str) -> Duration {
    let past = text.chars().count().saturating_sub(RENDER_SHORT_CHARS);
    let more = Duration::from_secs(past.div_ceil(RENDER_MORE_CHARS) as u64);
    (RENDER_WITHIN + more).min(RENDER_MOST)
}

/// `render`, `text`'s whole rendering, awaited at most
/// [`render_within`]`(text)`, or why not.
async fn whole_within<E: std::fmt::Display>(
    text: &str,
    render: impl std::future::Future<Output = Result<Vec<i16>, E>>,
) -> Result<Vec<i16>, String> {
    let within = render_within(text);
    match tokio::time::timeout(within, render).await {
        Ok(samples) => samples.map_err(|why| why.to_string()),
        Err(_) => Err(format!("not within {} s", within.as_secs())),
    }
}

/// `text` rendered through the speech service `config` names, whole, within
/// [`render_within`], on a runtime of its own; `refused` is told the status
/// the service refused the key or voice with (401 to 404).
fn render_with(
    config: &crate::second_config::SecondConfig,
    text: &str,
    refused: &dyn Fn(u16),
) -> Result<Vec<i16>, String> {
    use crate::second_provider::net::{Error, Tts};
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|why| why.to_string())?;
    runtime.block_on(async {
        let whole = async {
            let tts = Tts::new(config)?;
            let mut speech = tts.speak(text).await.inspect_err(|why| {
                if let Error::Status(status @ 401..=404) = why {
                    refused(*status);
                }
            })?;
            let mut samples = Vec::new();
            while let Some(packet) = speech.next().await? {
                samples.extend_from_slice(&packet);
            }
            Ok::<_, crate::second_provider::net::Error>(samples)
        };
        whole_within(text, whole).await
    })
}

/// `config` with the lamp's own speech (#8367): as the route had it before
/// the person's own voice, when that is set; else as it is.
fn lamps_config(config: &crate::second_config::SecondConfig) -> crate::second_config::SecondConfig {
    let mut lamps = config.clone();
    if let Some(voice) = lamps.hosted.lamps_voice.take() {
        lamps.tts_base_url = voice.base_url;
        lamps.tts_key = voice.key;
        lamps.tts_voice = voice.voice;
    }
    lamps
}

fn made_from_environment() -> Option<Arc<TtsCache>> {
    let cache = Arc::new(built_from_environment()?);
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

/// The cache over the second route's speech service, when it is
/// configured, as [`from_environment`] makes it but on its own and with
/// nothing warmed: for the Schedule consumer, which renders the offline
/// rings' clips through it (`crate::ring_clips`, #8708) and keeps no line
/// of the speech cache.
pub fn built_from_environment() -> Option<TtsCache> {
    use crate::second_provider::net::{default_model, voice_name};
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
    // The person's own voice (#8367), read as the second route's worker
    // reads it (`LAMP_OWN_KEYS`, the unit's credentials); its lines are
    // journalled by the worker.
    let (own, _) = config.with_own_keys(
        std::env::var_os(crate::second_config::OWN_KEYS_ENV).is_some_and(|value| value == "on"),
        std::env::var_os(crate::second_config::CREDENTIALS_ENV).as_deref(),
    );
    let lamps = lamps_config(&own);
    let model = lamps
        .tts_model
        .clone()
        .unwrap_or_else(|| default_model(lamps.tts_provider).to_owned());
    let language = lamps.stt_language.clone().unwrap_or_default();
    let settled = |cache: TtsCache| {
        cache
            .speaking(crate::language::at_start())
            .hearing_its_name(
                crate::sleep_capture::Mode::from_environment() == crate::sleep_capture::Mode::On,
            )
            .keeping_outage_lines(
                std::env::var(crate::coordinator::OUTAGE_LINE_AGAIN_ENVIRONMENT)
                    .ok()
                    .as_deref()
                    == Some("on"),
            )
            .keeping_offline_rings(crate::offline_rings::on())
    };
    // Another of the lamp's voices (#7485): the same service and model,
    // only its voice changed, for ElevenLabs, whose voices the lamp names;
    // always with the lamp's own key, never the person's (#8367).
    let render_in: Option<RenderIn> =
        (lamps.tts_provider == crate::second_config::TtsProvider::ElevenLabs).then(|| {
            let config = lamps.clone();
            Box::new(move |text: &str, voice: &str| {
                let mut config = config.clone();
                config.tts_voice = Some(voice.to_owned());
                render_with(&config, text, &|_| {})
            }) as RenderIn
        });
    let lamps_voice = voice_name(lamps.tts_provider, lamps.tts_voice.as_deref()).to_owned();
    let lamps_cache = |lamps: crate::second_config::SecondConfig| {
        let render: Render = Box::new(move |text: &str| render_with(&lamps, text, &|_| {}));
        settled(TtsCache::new(DIR, &lamps_voice, &model, &language, render))
    };
    let cache = if own.hosted.lamps_voice.is_some() {
        let mut own = own;
        own.hosted.lamps_voice = None;
        let voice = own_voice(
            voice_name(own.tts_provider, own.tts_voice.as_deref()),
            Some(&own.tts_base_url),
        );
        let lost = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let refused = Arc::clone(&lost);
        let render: Render = Box::new(move |text: &str| {
            render_with(&own, text, &|status| {
                if !refused.swap(true, std::sync::atomic::Ordering::AcqRel) {
                    eprintln!(
                        "lamp-voice: the speech cache: ElevenLabs refused the person's own key or voice (HTTP {status}); its lines are the lamp's own voice, kept as the lamp's, until the voice starts again (LAMP_OWN_KEYS)"
                    );
                }
            })
        });
        eprintln!(
            "lamp-voice: the speech cache keeps its lines in the person's own voice, apart from the lamp's (LAMP_OWN_KEYS)"
        );
        settled(TtsCache::new(DIR, &voice, &model, &language, render)).falling_back_to(
            lamps_cache(lamps),
            Box::new(move || lost.load(std::sync::atomic::Ordering::Acquire)),
        )
    } else {
        lamps_cache(lamps)
    };
    let cache = match render_in {
        Some(render_in) => cache.speaking_in(render_in),
        None => cache,
    };
    Some(cache)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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
        assert_eq!(
            WARM,
            [
                lamp_msgs::GOING_TO_SLEEP,
                lamp_msgs::SLEEP_LINE,
                lamp_msgs::RESET_STARTING
            ]
        );
        let dir = scratch("warm");
        let (cache, calls) = counted(&dir, "Rachel");
        assert_eq!(cache.kept(lamp_msgs::SLEEP_LINE), None);
        cache.warm(WARM).unwrap();
        cache.warm(WARM).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!(cache.path(lamp_msgs::GOING_TO_SLEEP).is_file());
        assert_eq!(
            cache.kept(lamp_msgs::SLEEP_LINE),
            Some(vec![lamp_msgs::SLEEP_LINE.len() as i16; 2_400])
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3, "kept never renders");
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
            Language::French,
        ] {
            let dir = scratch(&format!("owners-{}", language.code()));
            let (cache, calls) = counted(&dir, "Rachel");
            let cache = cache.speaking(Some(language));
            let lines = cache.warm_lines();
            let fixed = language.fixed_lines().count();
            assert_eq!(lines.len(), fixed + WARM.len(), "{language:?}");
            assert_eq!(&lines[fixed..], WARM);
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

    /// #8632: the offline ring's lines are warmed only with the switch, in
    /// the owner's words; off, the lines warmed are exactly as before. A
    /// ring reads its clip, never renders it: the owner's words, else the
    /// English, else none.
    #[test]
    fn the_offline_rings_lines_are_warmed_only_with_the_switch_and_read_not_rendered() {
        use crate::language::{RING_REMINDER_DUE, RING_TIMER_DONE, ring_words};
        let dir = scratch("rings-en");
        let (english, calls) = counted(&dir, "Rachel");
        assert_eq!(english.warm_lines(), WARM, "off: as before");
        english.warm(&[RING_TIMER_DONE]).unwrap();
        assert_eq!(english.ring_clip(true), None, "off: no ring clip");
        let (english, calls_on) = counted(&dir, "Rachel");
        let english = english.keeping_offline_rings(true);
        assert_eq!(
            english.warm_lines()[WARM.len()..],
            [RING_TIMER_DONE, RING_REMINDER_DUE]
        );
        assert_eq!(
            english.ring_clip(true),
            Some(vec![RING_TIMER_DONE.len() as i16; 2_400])
        );
        assert_eq!(english.ring_clip(false), None, "not kept: none");
        assert_eq!(calls_on.load(Ordering::SeqCst), 0, "a ring never renders");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let dir = scratch("rings-vi");
        let (off, _) = counted(&dir, "Rachel");
        let off = off.speaking(Some(Language::Vietnamese));
        let before = off.warm_lines();
        assert_eq!(
            before.len(),
            Language::Vietnamese.fixed_lines().count() + WARM.len()
        );
        let (vietnamese, _) = counted(&dir, "Rachel");
        let vietnamese = vietnamese
            .speaking(Some(Language::Vietnamese))
            .keeping_offline_rings(true);
        let words = ring_words(Some(Language::Vietnamese), false);
        assert_eq!(vietnamese.warm_lines()[..before.len()], *before);
        assert_eq!(
            vietnamese.warm_lines()[before.len()..],
            [ring_words(Some(Language::Vietnamese), true), words]
        );
        // The English only, kept by an English voice before: said, rather
        // than a timer silent.
        vietnamese.warm(&[RING_REMINDER_DUE]).unwrap();
        assert_eq!(
            vietnamese.ring_clip(false),
            Some(vec![RING_REMINDER_DUE.len() as i16; 2_400])
        );
        vietnamese.warm(&[words]).unwrap();
        assert_eq!(
            vietnamese.ring_clip(false),
            Some(vec![words.len() as i16; 2_400])
        );
    }

    /// migrate-P2: the wake's line is warmed only for a lamp that hears its
    /// name, in the owner's words or the English on an English lamp. Off, the
    /// lines warmed are as before. Never rendered to be said, and a line only
    /// in English on another language's lamp is not said.
    #[test]
    fn the_wake_by_names_line_is_warmed_only_with_the_switch_and_read_not_rendered() {
        let dir = scratch("wake-en");
        let (english, calls) = counted(&dir, "Rachel");
        assert_eq!(english.warm_lines(), WARM);
        let english = english.hearing_its_name(true);
        assert_eq!(english.wake_line(), None);
        assert_eq!(calls.load(Ordering::SeqCst), 0, "not kept: nothing asked");
        assert_eq!(english.warm_lines()[..WARM.len()], *WARM);
        assert_eq!(english.warm_lines()[WARM.len()], "Yes?");
        english.warm(&["Yes?"]).unwrap();
        assert_eq!(english.wake_line(), Some(vec![4; 2_400]));
        let dir = scratch("wake-vi");
        let (off, _) = counted(&dir, "Rachel");
        let off = off.speaking(Some(Language::Vietnamese));
        assert!(!off.warm_lines().contains(&"Dạ?"));
        assert_eq!(
            off.warm_lines().len(),
            Language::Vietnamese.fixed_lines().count() + WARM.len()
        );
        let (vietnamese, calls) = counted(&dir, "Rachel");
        let vietnamese = vietnamese
            .speaking(Some(Language::Vietnamese))
            .hearing_its_name(true);
        assert_eq!(vietnamese.warm_lines().last(), Some(&"Dạ?"));
        assert!(!vietnamese.warm_lines().contains(&"Yes?"));
        vietnamese.warm(&["Yes?"]).unwrap();
        assert_eq!(vietnamese.wake_line(), None, "the English isn't said");
        calls.store(0, Ordering::SeqCst);
        assert_eq!(vietnamese.wake_line(), None);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        vietnamese.warm(&["Dạ?"]).unwrap();
        assert_eq!(
            vietnamese.wake_line(),
            Some(vec!["Dạ?".len() as i16; 2_400])
        );
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

    /// #4883: on lamp-52e6 a cache left full by earlier languages (48
    /// files, no index) never kept Korean's fixed lines, so a Korean lamp
    /// said the English notices. Each switch here (en, ko, ja, ko, one
    /// voice, as on that lamp) keeps every one of the owner's lines and the
    /// cache stays within its cap.
    #[test]
    fn a_language_switch_makes_room_for_the_owners_lines_in_a_full_cache() {
        let dir = scratch("switch");
        let (old, _) = counted(&dir, "an older voice");
        for n in 0..MOST_FILES {
            old.samples(&format!("an old line {n}")).unwrap();
        }
        for owners in [
            None,
            Some(Language::Korean),
            Some(Language::Japanese),
            Some(Language::Korean),
        ] {
            let (cache, _) = counted(&dir, "Rachel");
            let cache = cache.speaking(owners);
            let lines = cache.warm_lines();
            cache.warm(&lines).unwrap();
            let files = fs::read_dir(&dir).unwrap().count();
            assert!(files <= MOST_FILES, "{owners:?}: {files} files");
            for line in &lines {
                assert!(cache.kept(line).is_some(), "{owners:?}: {line:?} not kept");
            }
        }
    }

    /// The owner's warmed lines are never dropped for another line: when
    /// only they and lines asked since are left, the oldest asked goes.
    #[test]
    fn the_owners_lines_are_never_dropped_to_make_room() {
        let dir = scratch("never_owners");
        let (cache, _) = counted(&dir, "Rachel");
        let cache = cache.speaking(Some(Language::Korean));
        let lines = cache.warm_lines();
        cache.warm(&lines).unwrap();
        for n in 0..MOST_FILES * 2 {
            cache.samples(&format!("asked {n}")).unwrap();
        }
        assert_eq!(fs::read_dir(&dir).unwrap().count(), MOST_FILES);
        assert!(lines.iter().all(|line| cache.kept(line).is_some()));
        assert!(
            cache
                .kept(&format!("asked {}", MOST_FILES * 2 - 1))
                .is_some()
        );
        assert!(cache.kept("asked 0").is_none());
    }

    /// Past the most files the oldest line goes and the new one is kept.
    #[test]
    fn past_the_most_files_the_oldest_line_makes_room() {
        let dir = scratch("past");
        let (cache, calls) = counted(&dir, "Rachel");
        for n in 0..MOST_FILES {
            cache.samples(&format!("line {n}")).unwrap();
        }
        assert!(cache.samples("one more").is_ok());
        assert!(cache.path("one more").exists());
        assert!(!cache.path("line 0").exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), MOST_FILES);
        assert_eq!(calls.load(Ordering::SeqCst), MOST_FILES + 1);
    }

    /// A renderer taking `secs` (paused time) to give `text`'s samples.
    async fn slow(secs: u64) -> Result<Vec<i16>, String> {
        tokio::time::sleep(Duration::from_secs(secs)).await;
        Ok(vec![1; 2_400])
    }

    /// A skill's agent's two-sentence answer (#7742, 363 bytes on c0a4) is
    /// rendered whole in 7 s and said; a short line keeps the 5 s.
    #[tokio::test(start_paused = true)]
    async fn a_long_exact_line_is_given_time_by_its_length_and_a_short_one_keeps_5_s() {
        let long = "Bitcoin is trading at about sixty-one thousand two hundred dollars \
            right now, up one point eight percent over the last twenty-four hours on the \
            major exchanges I checked. It has stayed between sixty thousand and sixty-two \
            thousand dollars all week, so nothing unusual yet, and I will check again in \
            three minutes and tell you if it moves by much at all today.";
        assert_eq!(long.chars().count(), 359);
        assert_eq!(whole_within(long, slow(7)).await.unwrap().len(), 2_400);
        assert_eq!(render_within(long), Duration::from_secs(13));

        let short = "Build passed.";
        assert_eq!(render_within(short), RENDER_WITHIN);
        assert_eq!(
            render_within(&"a".repeat(RENDER_SHORT_CHARS)),
            RENDER_WITHIN
        );
        assert_eq!(
            render_within(&"a".repeat(RENDER_SHORT_CHARS + 1)),
            RENDER_WITHIN + Duration::from_secs(1)
        );
        assert_eq!(
            whole_within(short, slow(6)).await.unwrap_err(),
            "not within 5 s"
        );
        assert!(whole_within(short, slow(4)).await.is_ok());
        assert_eq!(render_within(&"a".repeat(10_000)), RENDER_MOST);
    }

    /// A cache whose speech service speaks at the voice's pace,
    /// [`RENDER_MORE_CHARS`] characters in 2 s, and the lines it was asked
    /// to render, in turn.
    fn paced(dir: &Path) -> (TtsCache, Arc<Mutex<Vec<String>>>) {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&asked);
        let render: Render = Box::new(move |text: &str| {
            seen.lock().unwrap().push(text.to_owned());
            let frames =
                text.chars().count() * 2 * crate::say::SOUND_RATE as usize / RENDER_MORE_CHARS;
            Ok(vec![1; frames])
        });
        (TtsCache::new(dir, "Rachel", "m", "en", render), asked)
    }

    /// How long `text` plays at the paced voice's pace, in frames.
    fn paced_frames(text: &str) -> usize {
        text.chars().count() * 2 * crate::say::SOUND_RATE as usize / RENDER_MORE_CHARS
    }

    /// The words of `pieces`, in order.
    fn words_of(pieces: &[String]) -> Vec<&str> {
        pieces
            .iter()
            .flat_map(|piece| piece.split_whitespace())
            .collect()
    }

    /// #7892: an exact line about a minute long (a task's told answer,
    /// OpenClaw's, on lamp-52e6) rendered past 20 s and was refused whole,
    /// so nothing was said. It is rendered whole, in pieces each within
    /// [`MOST_FRAMES`], and played as one line: every piece, in order.
    #[test]
    fn a_minute_long_exact_line_is_rendered_whole_in_pieces() {
        let dir = scratch("minute");
        let (cache, asked) = paced(&dir);
        let sentence = "The lamp kept the room warm at night, dimmed itself slowly \
            while you read, and asked before it turned the speaker up past half. ";
        let line = sentence.repeat(7);
        let line = line.trim_end();
        assert!(line.len() <= crate::say::MOST_WORDS_BYTES);
        let seconds = paced_frames(line) / crate::say::SOUND_RATE as usize;
        assert!((55..=60).contains(&seconds), "{seconds} s");

        let samples = cache.render_once(line).unwrap();
        let asked = asked.lock().unwrap().clone();
        assert!(asked.len() > 1, "rendered in pieces: {asked:?}");
        for piece in &asked {
            assert!(paced_frames(piece) <= MOST_FRAMES as usize, "{piece:?}");
        }
        assert_eq!(
            samples.len(),
            asked.iter().map(|piece| paced_frames(piece)).sum::<usize>(),
            "every piece played, none dropped"
        );
        assert!(samples.len() > MOST_FRAMES as usize);
        assert_eq!(
            words_of(&asked),
            line.split_whitespace().collect::<Vec<_>>()
        );
        assert!(cache.kept(line).is_none(), "never kept");
    }

    /// A sentence alone past 20 s is cut where its clauses end; one with no
    /// clause mark, between its words. No word is cut, lost, added or moved.
    #[test]
    fn an_exact_sentence_past_20_s_is_split_at_a_clause_or_a_word() {
        let dir = scratch("sentence");
        let (cache, asked) = paced(&dir);
        let clauses = (1..=8)
            .map(|at| format!("the {at} small lamp on the shelf by the window glows softly"))
            .collect::<Vec<_>>()
            .join(", ")
            + ".";
        assert!(paced_frames(&clauses) > MOST_FRAMES as usize);
        cache.render_once(&clauses).unwrap();
        let pieces = std::mem::take(&mut *asked.lock().unwrap());
        assert!(pieces.len() > 1, "{pieces:?}");
        for piece in &pieces {
            assert!(paced_frames(piece) <= MOST_FRAMES as usize, "{piece:?}");
            assert!(
                piece.ends_with([',', '.']),
                "cut inside a clause: {piece:?}"
            );
        }
        assert_eq!(
            words_of(&pieces),
            clauses.split_whitespace().collect::<Vec<_>>()
        );

        let plain = (1..=70)
            .map(|at| format!("word{at}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(paced_frames(&plain) > MOST_FRAMES as usize);
        cache.render_once(&plain).unwrap();
        let pieces = std::mem::take(&mut *asked.lock().unwrap());
        assert!(pieces.len() > 1, "{pieces:?}");
        let original: Vec<&str> = plain.split_whitespace().collect();
        for piece in &pieces {
            assert!(paced_frames(piece) <= MOST_FRAMES as usize, "{piece:?}");
            for word in piece.split_whitespace() {
                assert!(original.contains(&word), "a word cut: {word:?}");
            }
        }
        assert_eq!(words_of(&pieces), original);
    }

    /// No word is lost, added or moved, whatever marks a long line has;
    /// a short line is rendered once as it is; and a piece that still
    /// renders past [`MOST_FRAMES`] refuses the line, the cap unchanged.
    #[test]
    fn an_exact_lines_pieces_keep_its_words_in_order_and_the_cap_stays() {
        let dir = scratch("words");
        let (cache, asked) = paced(&dir);
        let line = "First: the kettle is on! Is the door shut? Yes; it is, and \
            the window too. "
            .repeat(6)
            + "Then (at nine) the lamp dims - slowly - to a glow; that's all.";
        cache.render_once(&line).unwrap();
        let pieces = std::mem::take(&mut *asked.lock().unwrap());
        assert!(pieces.len() > 1, "{pieces:?}");
        assert_eq!(
            words_of(&pieces),
            line.split_whitespace().collect::<Vec<_>>()
        );

        cache.render_once("Build passed.").unwrap();
        assert_eq!(*asked.lock().unwrap(), ["Build passed."]);

        let render: Render = Box::new(|_: &str| Ok(vec![1; MOST_FRAMES as usize + 1]));
        let slow = TtsCache::new(&dir, "Rachel", "m", "en", render);
        assert!(slow.render_once("Hi.").is_err());
        assert!(slow.render_once(&line).is_err());
    }

    /// A cache in the person's own voice (#8367) over `dir`, behind the
    /// lamp's ("Rachel"); `refuses`, the person's voice is refused (a 401).
    /// Its renders and the lamp's are counted.
    fn own_over_lamps(dir: &Path, refuses: bool) -> (TtsCache, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let lost = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let (seen, refused) = (Arc::clone(&calls), Arc::clone(&lost));
        let render: Render = Box::new(move |_: &str| {
            seen.fetch_add(1, Ordering::SeqCst);
            if refuses {
                refused.store(true, Ordering::SeqCst);
                return Err("HTTP 401".into());
            }
            Ok(vec![7; 2_400])
        });
        let (lamps, lamps_calls) = counted(dir, "Rachel");
        let voice = own_voice("abc123", Some(ELEVENLABS));
        let own = TtsCache::new(dir, &voice, "eleven_v4_turbo", "en", render)
            .falling_back_to(lamps, Box::new(move || lost.load(Ordering::SeqCst)));
        (own, calls, lamps_calls)
    }

    const ELEVENLABS: &str = "https://api.elevenlabs.io/v1";

    fn kept_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    /// The Reviewer-lead's P2 on #8394: a line the person's voice is
    /// refused for is said in the lamp's own voice and kept under the
    /// lamp's key only, never theirs; the person's is not asked again in
    /// this run, for kept lines or exact ones.
    #[test]
    fn a_refused_own_voice_is_never_kept_under_it_and_the_lamps_line_is_kept_as_the_lamps() {
        let dir = scratch("own_refused");
        let (own, calls, lamps_calls) = own_over_lamps(&dir, true);
        let said = own.samples("Hello.").unwrap();
        assert_eq!(said, vec!["Hello.".len() as i16; 2_400], "the lamp's voice");
        let (lamps, _) = counted(&dir, "Rachel");
        assert_eq!(kept_names(&dir), [format!("{}.wav", lamps.key("Hello."))]);
        assert_eq!(own.kept("Hello."), Some(said));
        own.samples("Goodbye.").unwrap();
        own.render_once("Build passed.").unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(lamps_calls.load(Ordering::SeqCst), 3);
        assert!(!kept_names(&dir).contains(&format!("{}.wav", own.key("Hello."))));
    }

    /// The person's lines are keyed by the voice that rendered them: apart
    /// from the lamp's, by voice ID and by the address asked; the lamp's
    /// key is as before. A new voice ID misses the old one's lines.
    #[test]
    fn the_persons_lines_are_keyed_by_their_voice_id_and_address_apart_from_the_lamps() {
        assert_eq!(own_voice("Rachel", None), "Rachel");
        let own = own_voice("abc123", Some(ELEVENLABS));
        assert_ne!(own, "abc123");
        assert_ne!(own, own_voice("def456", Some(ELEVENLABS)));
        assert_ne!(
            own,
            own_voice("abc123", Some("https://relay.example.invalid/api/v1/ai"))
        );
        let dir = scratch("own_keyed");
        let (first, calls, _) = own_over_lamps(&dir, false);
        first.samples("Hello.").unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(kept_names(&dir), [format!("{}.wav", first.key("Hello."))]);
        let (changed, changed_calls) = counted(&dir, &own_voice("def456", Some(ELEVENLABS)));
        assert_eq!(changed.kept("Hello."), None);
        changed.samples("Hello.").unwrap();
        assert_eq!(changed_calls.load(Ordering::SeqCst), 1);
    }

    /// The person's key removed (the voice starts again in the lamp's own):
    /// the lines kept in the lamp's voice while theirs was refused play,
    /// none rendered again, and none of the person's is ever played.
    #[test]
    fn with_the_persons_key_removed_the_lamps_kept_lines_play() {
        let dir = scratch("own_removed");
        let (own, _, _) = own_over_lamps(&dir, true);
        let said = own.samples("Hello.").unwrap();
        let (lamps, calls) = counted(&dir, "Rachel");
        assert_eq!(lamps.samples("Hello.").unwrap(), said);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
