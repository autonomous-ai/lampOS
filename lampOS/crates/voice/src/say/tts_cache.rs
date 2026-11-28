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
/// owner's current lines (#4883, [`TtsCache::make_room`]). Only the lamp's
/// own fixed lines come here: [`WARM`] and the fixed lines in the owner's
/// language (#4473 (c), 45 since #5999's skill lines), with room for a change
/// of voice or language and the lines asked since.
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
/// How long the person's own Gemini voice gives way to the lamp's own
/// after it fails two lines in a row (#8762's review, [`Strikes`]): the
/// lines in that time never wait on Google. Past it, theirs is asked again,
/// so a fixed line the warming said in the lamp's voice is warmed in theirs
/// on a later try ([`TtsCache::warm`]), never left the lamp's until the
/// voice restarts. The reply path gives way until the voice restarts
/// (`own-keys.md`); a say line is rarer and kept, so it tries again.
pub(crate) const GIVE_WAY_FOR: Duration = Duration::from_secs(600);
pub(crate) const WARM_EVERY_MOST: Duration = Duration::from_secs(3_600);

/// Why a line that rendered empty or too long is not played.
const REFUSED: &str = "not in the speech cache, and rendered empty or too long";

/// Why a warm line is not done yet: said in the lamp's own voice in place
/// of the person's Gemini voice, which is asked again on a later try.
const FOR_NOW: &str =
    "said in the lamp's own voice for now, the person's Gemini voice asked again later";

/// The voice a line was said in ([`TtsCache::said_in`], #8762's review).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SaidIn {
    /// This cache's own voice: the person's, or the lamp's when no
    /// person's voice is in front of it.
    Own,
    /// The lamp's own voice for the rest of this run: the person's was
    /// refused (#8367), or rendered this line empty or too long, which is
    /// never asked of it again.
    Lamps,
    /// The lamp's own voice for now, for the person's Gemini voice only
    /// (#8762): theirs failed this line, or is giving way ([`Strikes`]);
    /// theirs is asked again later.
    LampsForNow,
}

/// The person's own Gemini voice's failures in a row (#8762's review): two
/// in a row give way to the lamp's own voice for [`GIVE_WAY_FOR`], as the
/// reply path gives way to it after a piece fails twice, so a slow or
/// failing Google costs at most two lines their whole bound.
struct Strikes {
    give_way_for: Duration,
    /// Failures since the last line theirs said, and until when theirs
    /// gives way.
    state: Mutex<(u8, Option<std::time::Instant>)>,
}

impl Strikes {
    fn new(give_way_for: Duration) -> Self {
        Self {
            give_way_for,
            state: Mutex::new((0, None)),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, (u8, Option<std::time::Instant>)> {
        self.state.lock().unwrap_or_else(|held| held.into_inner())
    }

    /// Theirs gives way now: not asked.
    fn giving_way(&self) -> bool {
        self.state()
            .1
            .is_some_and(|until| std::time::Instant::now() < until)
    }

    /// Theirs said a line: the count starts over.
    fn said(&self) {
        *self.state() = (0, None);
    }

    /// Theirs failed a line (`why`): the second in a row gives way.
    fn failed(&self, why: &str) {
        let mut state = self.state();
        state.0 = state.0.saturating_add(1);
        if state.0 >= 2 {
            *state = (0, Some(std::time::Instant::now() + self.give_way_for));
            eprintln!(
                "lamp-voice: the speech cache: the person's own Gemini voice failed two lines in a row ({why}); the lamp's own voice for the next {} s, then theirs again (LAMP_OWN_KEYS)",
                self.give_way_for.as_secs()
            );
        }
    }
}

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
    /// Every line the person's voice fails is said in the lamp's, not only
    /// once it is refused, and two in a row give way for a while
    /// ([`TtsCache::giving_way_to`], #8762). None for the person's
    /// ElevenLabs voice ([`TtsCache::falling_back_to`]).
    strikes: Option<Strikes>,
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

/// The voice part of the cache's key for the person's own Gemini voice
/// (#8754, #8762): its name with the address it is asked at, so their
/// lines are never the lamp's, an ElevenLabs voice's ([`own_voice`]) or
/// another name's.
pub fn own_gemini_voice(voice: &str, at: &str) -> String {
    format!("own-gemini:{voice}@{at}")
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
        self.lamps = Some(Box::new(Lamps {
            cache: lamps,
            lost,
            strikes: None,
        }));
        self
    }

    /// [`Self::falling_back_to`] for the person's own Gemini voice (#8762),
    /// as the reply path gives way (#8754): a line theirs fails for any
    /// reason is said in the lamp's own voice, never left unsaid, and kept,
    /// if at all, under the lamp's key; the next line asks theirs again,
    /// until `lost` says it was refused. Two lines in a row theirs fails
    /// give way to the lamp's for [`GIVE_WAY_FOR`] ([`Strikes`], #8762's
    /// review), so a slow Google never makes every line wait its bound.
    pub fn giving_way_to(mut self, lamps: TtsCache, lost: Lost) -> Self {
        self.lamps = Some(Box::new(Lamps {
            cache: lamps,
            lost,
            strikes: Some(Strikes::new(GIVE_WAY_FOR)),
        }));
        self
    }

    /// [`Self::giving_way_to`]'s while as `give_way_for`, for a test.
    #[cfg(test)]
    pub(crate) fn giving_way_for(mut self, give_way_for: Duration) -> Self {
        if let Some(strikes) = self.lamps.as_mut().and_then(|lamps| lamps.strikes.as_mut()) {
            strikes.give_way_for = give_way_for;
        }
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
        Sha256::digest(format!("{}|{text}", self.identity))
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn path(&self, text: &str) -> PathBuf {
        self.dir.join(format!("{}.wav", self.key(text)))
    }

    /// `text`'s samples: from the cache, or rendered once and kept, in the
    /// voice the lamp speaks in now ([`Self::in_voice`]).
    pub fn samples(&self, text: &str) -> Result<Vec<i16>, String> {
        self.samples_said(text).map(|(samples, _)| samples)
    }

    /// [`Self::samples`] and the voice that said them: a line kept in the
    /// person's voice plays theirs even while it gives way.
    fn samples_said(&self, text: &str) -> Result<(Vec<i16>, SaidIn), String> {
        self.said_in(|cache| cache.samples_here(text), || read(&self.path(text)))
    }

    /// `say` in the voice the lamp speaks in now, as [`Self::said_in`].
    fn in_voice<T>(&self, say: impl Fn(&TtsCache) -> Result<T, String>) -> Result<T, String> {
        self.said_in(say, || None).map(|(said, _)| said)
    }

    /// `say` in the voice the lamp speaks in now (#8367), and which voice
    /// that was: the person's own until it is refused, then the lamp's
    /// own. For their Gemini voice (#8762), the lamp's for any line theirs
    /// fails too, and for every line while theirs gives way after two
    /// failures in a row ([`Strikes`]), apart from a line `theirs_kept`
    /// finds kept in their voice already. A line the person's voice was
    /// refused for is said again in the lamp's, and kept, if at all, under
    /// the lamp's key only: the person's render has no fallback of its
    /// own, so it never hands back the lamp's voice to keep under theirs.
    fn said_in<T>(
        &self,
        say: impl Fn(&TtsCache) -> Result<T, String>,
        theirs_kept: impl FnOnce() -> Option<T>,
    ) -> Result<(T, SaidIn), String> {
        let Some(lamps) = &self.lamps else {
            return say(self).map(|said| (said, SaidIn::Own));
        };
        let lamps_say = |said_in| say(&lamps.cache).map(|said| (said, said_in));
        if (lamps.lost)() {
            return lamps_say(SaidIn::Lamps);
        }
        let Some(strikes) = &lamps.strikes else {
            // The person's ElevenLabs voice (#8367): theirs until refused.
            return match say(self) {
                Err(_) if (lamps.lost)() => lamps_say(SaidIn::Lamps),
                said => said.map(|said| (said, SaidIn::Own)),
            };
        };
        if strikes.giving_way() {
            if let Some(said) = theirs_kept() {
                return Ok((said, SaidIn::Own));
            }
            return lamps_say(SaidIn::LampsForNow);
        }
        match say(self) {
            Ok(said) => {
                strikes.said();
                Ok((said, SaidIn::Own))
            }
            Err(_) if (lamps.lost)() => lamps_say(SaidIn::Lamps),
            Err(why) if why == REFUSED => lamps_say(SaidIn::Lamps),
            Err(why) => {
                strikes.failed(&why);
                lamps_say(SaidIn::LampsForNow)
            }
        }
    }

    /// [`Self::samples`] in this cache's own voice only.
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

    /// [`Self::render_once`], and whether the line may be kept under
    /// [`Self::identity`] (an offline ring's clip, `crate::ring_clips`;
    /// #8762's review): not when the lamp's own voice said it in place of
    /// the person's Gemini voice, so a clip under their key is always
    /// theirs. The person's ElevenLabs voice keeps every line as before
    /// (#8367).
    pub fn render_once_to_keep(&self, text: &str) -> Result<(Vec<i16>, bool), String> {
        let (samples, said_in) = self.said_in(
            |cache| in_pieces(text, |piece| (cache.render)(piece)),
            || None,
        )?;
        let gemini = self
            .lamps
            .as_ref()
            .is_some_and(|lamps| lamps.strikes.is_some());
        Ok((samples, said_in == SaidIn::Own || !gemini))
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
    ///
    /// A line said in the lamp's own voice in place of the person's Gemini
    /// voice (#8762's review) is kept under the lamp's key only, and is not
    /// done: the warming tries it in theirs again later, so a fixed line is
    /// never left in the lamp's voice for the run by one failure.
    pub fn warm(&self, lines: &[&str]) -> Result<(), String> {
        lines
            .iter()
            .map(|line| match self.samples_said(line) {
                Err(why) if why == REFUSED => Ok(()),
                Ok((_, SaidIn::LampsForNow)) => Err(FOR_NOW.to_owned()),
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

/// `text` rendered whole in the person's own Gemini voice `own` (#8762),
/// at `base_url` in [`crate::second_config::GEMINI_SPEECH_MODEL`], within
/// [`render_within`], on a runtime of its own; `refused` is told the
/// status Google refused their key with (401, 403, or 400 for a 400 its
/// body says is the key).
fn render_gemini(
    base_url: &str,
    own: &crate::second_config::OwnGeminiVoice,
    text: &str,
    refused: &dyn Fn(u16),
) -> Result<Vec<i16>, String> {
    use crate::second_provider::net::{Error, GeminiTts};
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|why| why.to_string())?;
    runtime.block_on(async {
        let whole = async {
            let tts = GeminiTts::persons(
                base_url,
                crate::second_config::GEMINI_SPEECH_MODEL,
                own.key.expose().trim(),
                &own.voice,
            )?;
            let mut speech = tts.speak(text).await.inspect_err(|why| match why {
                Error::Status(status @ (401 | 403)) => refused(*status),
                Error::KeyInvalid => refused(400),
                _ => {}
            })?;
            let mut samples = Vec::new();
            while let Some(packet) = speech.next().await? {
                samples.extend_from_slice(&packet);
            }
            Ok::<_, Error>(samples)
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

/// Called with the status a speech service refused the person's own key
/// or voice with.
pub(crate) type Refused = Box<dyn Fn(u16) + Send + Sync>;

/// How [`voiced`] renders its lines: through the speech services
/// ([`Renderers::services`]), or stand-ins in tests.
pub(crate) struct Renderers {
    /// The configured voice of `config` (`tts_*`): the lamp's own, or the
    /// person's own ElevenLabs voice put in its place (#8367).
    pub configured: Box<dyn Fn(crate::second_config::SecondConfig, Refused) -> Render>,
    /// The person's own Gemini voice (#8754, #8762), at Google's own API
    /// in the speech model the keys helper checked.
    pub gemini: Box<dyn Fn(crate::second_config::OwnGeminiVoice, Refused) -> Render>,
}

impl Renderers {
    /// The speech services themselves, each line on a runtime of its own.
    fn services() -> Self {
        Self {
            configured: Box::new(|config, refused| {
                Box::new(move |text: &str| render_with(&config, text, &*refused))
            }),
            gemini: Box::new(|own, refused| {
                Box::new(move |text: &str| {
                    render_gemini(crate::second_config::GEMINI_BASE_URL, &own, text, &*refused)
                })
            }),
        }
    }
}

/// The speech cache over `dir` in the voice `own` (the route's
/// configuration, `with_own_keys` read) speaks in, `settled` as the
/// environment has it: the person's own voice over the lamp's when one is
/// kept (#8367), else the lamp's own.
pub(crate) fn voiced(
    own: crate::second_config::SecondConfig,
    dir: &Path,
    renderers: Renderers,
    settled: &dyn Fn(TtsCache) -> TtsCache,
) -> TtsCache {
    use crate::second_provider::net::{default_model, voice_name};
    let lamps = lamps_config(&own);
    let model = lamps
        .tts_model
        .clone()
        .unwrap_or_else(|| default_model(lamps.tts_provider).to_owned());
    let language = lamps.stt_language.clone().unwrap_or_default();
    let lamps_voice = voice_name(lamps.tts_provider, lamps.tts_voice.as_deref()).to_owned();
    let configured = &renderers.configured;
    let lamps_cache = |lamps: crate::second_config::SecondConfig| {
        let render = configured(lamps, Box::new(|_| {}));
        settled(TtsCache::new(dir, &lamps_voice, &model, &language, render))
    };
    // The person's own Gemini voice (#8762): every line the cache says, an
    // exact one through the say service or a fixed one, in their voice, as
    // the route's answers are (#8754); before, all of them were the lamp's.
    // `with_own_keys` keeps it alone, never beside their ElevenLabs voice.
    if let Some(gemini) = own.hosted.own_gemini_voice.clone() {
        let voice = own_gemini_voice(&gemini.voice, crate::second_config::GEMINI_BASE_URL);
        let lost = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let refused = Arc::clone(&lost);
        let render = (renderers.gemini)(
            gemini,
            Box::new(move |status| {
                if !refused.swap(true, std::sync::atomic::Ordering::AcqRel) {
                    eprintln!(
                        "lamp-voice: the speech cache: Gemini refused the person's own key or voice (HTTP {status}); its lines are the lamp's own voice, kept as the lamp's, until the voice starts again (LAMP_OWN_KEYS)"
                    );
                }
            }),
        );
        eprintln!(
            "lamp-voice: the speech cache keeps its lines in the person's own Gemini voice, apart from the lamp's (LAMP_OWN_KEYS)"
        );
        return settled(TtsCache::new(
            dir,
            &voice,
            crate::second_config::GEMINI_SPEECH_MODEL,
            &language,
            render,
        ))
        .giving_way_to(
            lamps_cache(lamps),
            Box::new(move || lost.load(std::sync::atomic::Ordering::Acquire)),
        );
    }
    if own.hosted.lamps_voice.is_some() {
        let mut own = own;
        own.hosted.lamps_voice = None;
        let voice = own_voice(
            voice_name(own.tts_provider, own.tts_voice.as_deref()),
            Some(&own.tts_base_url),
        );
        let lost = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let refused = Arc::clone(&lost);
        let render = configured(
            own,
            Box::new(move |status| {
                if !refused.swap(true, std::sync::atomic::Ordering::AcqRel) {
                    eprintln!(
                        "lamp-voice: the speech cache: ElevenLabs refused the person's own key or voice (HTTP {status}); its lines are the lamp's own voice, kept as the lamp's, until the voice starts again (LAMP_OWN_KEYS)"
                    );
                }
            }),
        );
        eprintln!(
            "lamp-voice: the speech cache keeps its lines in the person's own voice, apart from the lamp's (LAMP_OWN_KEYS)"
        );
        settled(TtsCache::new(dir, &voice, &model, &language, render)).falling_back_to(
            lamps_cache(lamps),
            Box::new(move || lost.load(std::sync::atomic::Ordering::Acquire)),
        )
    } else {
        lamps_cache(lamps)
    }
}

/// The cache over the second route's speech service, when it is
/// configured, as [`from_environment`] makes it but on its own and with
/// nothing warmed: for the Schedule consumer, which renders the offline
/// rings' clips through it (`crate::ring_clips`, #8708) and keeps no line
/// of the speech cache.
pub fn built_from_environment() -> Option<TtsCache> {
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
    let cache = voiced(own, Path::new(DIR), Renderers::services(), &settled);
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

    /// What the stand-in voices of [`stand_ins`] were asked, in order:
    /// `lamp:<voice>`, `persons:<voice>` (the person's ElevenLabs voice) or
    /// `gemini:<voice>` (the person's Gemini voice, #8754).
    type Asked = Arc<std::sync::Mutex<Vec<String>>>;

    /// How the person's Gemini stand-in answers.
    #[derive(Clone, Copy)]
    enum Google {
        Speaks,
        Refuses,
        Fails,
    }

    /// The lamp's own voice says `[1; 2_400]`, the person's ElevenLabs
    /// voice `[3; 2_400]`, their Gemini voice `[2; 2_400]`, as `google`
    /// says; each asking is kept in the returned list.
    fn stand_ins(google: Google) -> (Renderers, Asked) {
        stand_ins_as(Arc::new(Mutex::new(google)))
    }

    /// [`stand_ins`] with the person's Gemini stand-in answering as
    /// `google` says at each asking, so a test can change it between lines.
    fn stand_ins_as(google: Arc<Mutex<Google>>) -> (Renderers, Asked) {
        let asked: Asked = Arc::default();
        let seen = Arc::clone(&asked);
        let configured = Box::new(
            move |config: crate::second_config::SecondConfig, _: Refused| -> Render {
                let seen = Arc::clone(&seen);
                Box::new(move |_: &str| {
                    let voice = config.tts_voice.clone().unwrap_or_default();
                    let persons = config.tts_base_url == ELEVENLABS;
                    let who = if persons { "persons" } else { "lamp" };
                    seen.lock().unwrap().push(format!("{who}:{voice}"));
                    Ok(vec![if persons { 3 } else { 1 }; 2_400])
                })
            },
        );
        let seen = Arc::clone(&asked);
        let gemini = Box::new(
            move |own: crate::second_config::OwnGeminiVoice, refused: Refused| -> Render {
                let seen = Arc::clone(&seen);
                let google = Arc::clone(&google);
                Box::new(move |_: &str| {
                    seen.lock().unwrap().push(format!("gemini:{}", own.voice));
                    let answer = *google.lock().unwrap();
                    match answer {
                        Google::Speaks => Ok(vec![2; 2_400]),
                        Google::Refuses => {
                            refused(401);
                            Err("HTTP 401".into())
                        }
                        Google::Fails => Err("HTTP 503".into()),
                    }
                })
            },
        );
        (Renderers { configured, gemini }, asked)
    }

    /// A route speaking with the lamp's own ElevenLabs voice ("Rachel")
    /// through the relay, as `second.json` has it.
    fn route(name: &str) -> crate::second_config::SecondConfig {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch(&format!("route-{name}"));
        let private = |path: &Path, text: &str| {
            fs::write(path, text).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        };
        let credential = dir.join("llm.key");
        private(&credential, "k-relay");
        let file = dir.join("second.json");
        private(
            &file,
            &format!(
                r#"{{"llm_base_url":"https://relay.example.invalid/api/v1/ai","llm_model":"m","llm_credential_file":"{}","tts_provider":"elevenlabs","tts_voice":"Rachel","stt_language":"en"}}"#,
                credential.display()
            ),
        );
        crate::second_config::load_from_path(&file).unwrap()
    }

    /// [`route`] with the person's own Gemini voice kept ("Kore"), as
    /// `with_own_keys` reads it (#8754).
    fn with_gemini(name: &str) -> crate::second_config::SecondConfig {
        let mut route = route(name);
        route.hosted.own_gemini_voice = Some(crate::second_config::OwnGeminiVoice {
            key: crate::second_config::Key::for_test("AIza-planted-8754"),
            voice: "Kore".into(),
        });
        route
    }

    /// The lamp's own key for `text`, as its cache keys it.
    fn lamps_key(dir: &Path, text: &str) -> String {
        use crate::second_config::TtsProvider::ElevenLabs;
        let voice = crate::second_provider::net::voice_name(ElevenLabs, Some("Rachel"));
        let model = crate::second_provider::net::default_model(ElevenLabs);
        let render: Render = Box::new(|_: &str| Ok(vec![]));
        TtsCache::new(dir, voice, model, "en", render).key(text)
    }

    /// The test lamp's row on #8762: with the person's Gemini voice kept,
    /// a Store app's exact line through the say service
    /// (`ok, to the voice, by say.sock`) and the lamp's cached fixed lines
    /// were rendered by the lamp's own ElevenLabs voice. Both are theirs,
    /// in Gemini, the fixed line kept under a key of its own
    /// (`own-gemini:<voice>@<address>`), never the lamp's or an ElevenLabs
    /// voice's; the lamp's voice is never asked.
    #[test]
    fn a_say_line_and_a_fixed_line_are_said_in_the_persons_gemini_voice_while_one_is_kept() {
        let dir = scratch("own_gemini_said");
        let (renderers, asked) = stand_ins(Google::Speaks);
        let cache = Arc::new(voiced(with_gemini("said"), &dir, renderers, &|cache| cache));
        let exact = "ok, to the voice, by say.sock";
        match cache.render_played(exact, None).unwrap() {
            Played::Whole(samples) => assert_eq!(samples, vec![2; 2_400], "their Gemini voice"),
            Played::Pieces(_) => panic!("a short line is rendered whole"),
        }
        assert_eq!(cache.render_once(exact).unwrap(), vec![2; 2_400]);
        let fixed = cache.samples(lamp_msgs::GOING_TO_SLEEP).unwrap();
        assert_eq!(fixed, vec![2; 2_400]);
        assert_eq!(*asked.lock().unwrap(), ["gemini:Kore"; 3]);
        assert!(
            cache.identity().starts_with(&format!(
                "own-gemini:Kore@{}|{}|",
                crate::second_config::GEMINI_BASE_URL,
                crate::second_config::GEMINI_SPEECH_MODEL
            )),
            "{}",
            cache.identity()
        );
        assert_eq!(
            kept_names(&dir),
            [format!("{}.wav", cache.key(lamp_msgs::GOING_TO_SLEEP))]
        );
        assert_ne!(
            cache.key(lamp_msgs::GOING_TO_SLEEP),
            lamps_key(&dir, lamp_msgs::GOING_TO_SLEEP)
        );
        assert_eq!(cache.kept(lamp_msgs::GOING_TO_SLEEP), Some(fixed));
    }

    /// As the reply path does (#8754): Google refusing the person's key
    /// says the line in the lamp's own voice, kept under the lamp's key
    /// only, and every line after is the lamp's, theirs never asked again
    /// in this run; the refusal is the process's one.
    #[test]
    fn a_refused_gemini_voice_says_its_line_in_the_lamps_own_and_keeps_it_as_the_lamps() {
        let dir = scratch("own_gemini_refused");
        let (renderers, asked) = stand_ins(Google::Refuses);
        let cache = voiced(with_gemini("refused"), &dir, renderers, &|cache| cache);
        let said = cache.samples(lamp_msgs::GOING_TO_SLEEP).unwrap();
        assert_eq!(said, vec![1; 2_400], "the lamp's own voice");
        assert_eq!(
            kept_names(&dir),
            [format!(
                "{}.wav",
                lamps_key(&dir, lamp_msgs::GOING_TO_SLEEP)
            )]
        );
        assert_eq!(cache.render_once("Build passed.").unwrap(), vec![1; 2_400]);
        assert_eq!(cache.kept(lamp_msgs::GOING_TO_SLEEP), Some(said));
        assert_eq!(
            *asked.lock().unwrap(),
            ["gemini:Kore", "lamp:Rachel", "lamp:Rachel"]
        );
    }

    /// A line the person's Gemini voice fails for another reason (a 429, a
    /// 5xx, no sound in time) is said in the lamp's own voice, never left
    /// unsaid, and kept, if a fixed line, under the lamp's key; the next
    /// line asks theirs again.
    #[test]
    fn a_gemini_voice_failing_a_line_says_that_line_in_the_lamps_own_and_asks_theirs_again() {
        let dir = scratch("own_gemini_failed");
        let (renderers, asked) = stand_ins(Google::Fails);
        let cache = voiced(with_gemini("failed"), &dir, renderers, &|cache| cache);
        assert_eq!(cache.render_once("Build passed.").unwrap(), vec![1; 2_400]);
        assert_eq!(
            cache.samples(lamp_msgs::GOING_TO_SLEEP).unwrap(),
            vec![1; 2_400]
        );
        assert_eq!(
            kept_names(&dir),
            [format!(
                "{}.wav",
                lamps_key(&dir, lamp_msgs::GOING_TO_SLEEP)
            )]
        );
        assert_eq!(
            *asked.lock().unwrap(),
            ["gemini:Kore", "lamp:Rachel", "gemini:Kore", "lamp:Rachel"]
        );
    }

    /// #8762's review, P3 2: a slow or failing Google made every say line
    /// wait its whole bound before the lamp's own voice said it. As the
    /// reply path gives way after two failures, two lines in a row that
    /// the person's Gemini voice fails give way to the lamp's own for
    /// [`GIVE_WAY_FOR`]: the lines in that time never ask Google, and a
    /// line already kept in their voice still plays theirs. Past it, theirs
    /// is asked again; a line it says starts the count over.
    #[test]
    fn two_gemini_failures_in_a_row_give_say_lines_to_the_lamps_own_for_a_while() {
        let dir = scratch("own_gemini_two_strikes");
        let google = Arc::new(Mutex::new(Google::Speaks));
        let (renderers, asked) = stand_ins_as(Arc::clone(&google));
        let cache = voiced(with_gemini("strikes"), &dir, renderers, &|cache| cache)
            .giving_way_for(Duration::from_millis(300));
        let fixed = cache.samples(lamp_msgs::GOING_TO_SLEEP).unwrap();
        assert_eq!(fixed, vec![2; 2_400], "kept in their voice");
        *google.lock().unwrap() = Google::Fails;
        assert_eq!(cache.render_once("One.").unwrap(), vec![1; 2_400]);
        assert_eq!(cache.render_once("Two.").unwrap(), vec![1; 2_400]);
        asked.lock().unwrap().clear();
        assert_eq!(cache.render_once("Three.").unwrap(), vec![1; 2_400]);
        assert_eq!(cache.samples(lamp_msgs::GOING_TO_SLEEP).unwrap(), fixed);
        assert_eq!(
            *asked.lock().unwrap(),
            ["lamp:Rachel"],
            "Google is not asked while theirs gives way"
        );
        std::thread::sleep(Duration::from_millis(350));
        *google.lock().unwrap() = Google::Speaks;
        assert_eq!(cache.render_once("Four.").unwrap(), vec![2; 2_400]);
        *google.lock().unwrap() = Google::Fails;
        assert_eq!(cache.render_once("Five.").unwrap(), vec![1; 2_400]);
        *google.lock().unwrap() = Google::Speaks;
        assert_eq!(cache.render_once("Six.").unwrap(), vec![2; 2_400]);
        *google.lock().unwrap() = Google::Fails;
        assert_eq!(cache.render_once("Seven.").unwrap(), vec![1; 2_400]);
        assert_eq!(
            asked.lock().unwrap()[1..],
            [
                "gemini:Kore",
                "gemini:Kore",
                "lamp:Rachel",
                "gemini:Kore",
                "gemini:Kore",
                "lamp:Rachel"
            ],
            "past the while theirs is asked again, and a line said starts the count over"
        );
    }

    /// #8762's review, P3 3: one Gemini failure while warming said that
    /// fixed line in the lamp's own voice and counted it warmed, so it was
    /// never asked in theirs again until the voice restarted. The lamp's
    /// render is kept under the lamp's key only, the warming says the line
    /// is not done, and its next try asks the person's voice again.
    #[test]
    fn a_warm_line_gemini_failed_is_kept_as_the_lamps_and_warmed_in_theirs_again() {
        let dir = scratch("own_gemini_warm_again");
        let google = Arc::new(Mutex::new(Google::Fails));
        let (renderers, asked) = stand_ins_as(Arc::clone(&google));
        let cache = voiced(with_gemini("warm_again"), &dir, renderers, &|cache| cache);
        let line = [lamp_msgs::GOING_TO_SLEEP];
        assert!(
            cache.warm(&line).is_err(),
            "not warmed in the person's voice"
        );
        assert_eq!(cache.kept(lamp_msgs::GOING_TO_SLEEP), Some(vec![1; 2_400]));
        assert_eq!(
            kept_names(&dir),
            [format!(
                "{}.wav",
                lamps_key(&dir, lamp_msgs::GOING_TO_SLEEP)
            )],
            "kept under the lamp's key only"
        );
        *google.lock().unwrap() = Google::Speaks;
        assert_eq!(cache.warm(&line), Ok(()));
        assert_eq!(cache.kept(lamp_msgs::GOING_TO_SLEEP), Some(vec![2; 2_400]));
        assert_eq!(
            *asked.lock().unwrap(),
            ["gemini:Kore", "lamp:Rachel", "gemini:Kore"]
        );
    }

    /// Pinned: with no own voice the lamp's says every line under its key
    /// as before; with the person's ElevenLabs voice (#8367) theirs says
    /// it, keyed `own:<voice_id>@<address>`.
    #[test]
    fn the_lamps_voice_and_the_persons_elevenlabs_voice_render_as_before() {
        let dir = scratch("own_none_or_elevenlabs");
        let (renderers, asked) = stand_ins(Google::Speaks);
        let lamps = voiced(route("none"), &dir, renderers, &|cache| cache);
        assert_eq!(lamps.samples("Hello.").unwrap(), vec![1; 2_400]);
        assert_eq!(lamps.key("Hello."), lamps_key(&dir, "Hello."));

        let mut own = route("elevenlabs");
        own.hosted.lamps_voice = Some(crate::second_config::LampsVoice {
            base_url: std::mem::replace(&mut own.tts_base_url, ELEVENLABS.to_owned()),
            key: std::mem::replace(
                &mut own.tts_key,
                crate::second_config::Key::for_test("sk-planted"),
            ),
            voice: own.tts_voice.replace("abc123".into()),
        });
        let (renderers, persons) = stand_ins(Google::Speaks);
        let cache = voiced(own, &dir, renderers, &|cache| cache);
        assert_eq!(cache.samples("Hello.").unwrap(), vec![3; 2_400]);
        assert!(
            cache
                .identity()
                .starts_with(&format!("own:abc123@{ELEVENLABS}|"))
        );
        assert_eq!(*asked.lock().unwrap(), ["lamp:Rachel"]);
        assert_eq!(*persons.lock().unwrap(), ["persons:abc123"]);
    }

    /// A loopback stand-in for Google that answers one request with
    /// `answer`: its address, and the request it read (head and body).
    fn google_once(answer: Vec<u8>) -> (String, std::thread::JoinHandle<String>) {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let served = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            let mut buf = [0; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                seen.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&seen).into_owned();
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let length = text[..head_end]
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())?
                        })
                        .unwrap_or(0);
                    if seen.len() >= head_end + 4 + length || n == 0 {
                        break;
                    }
                } else if n == 0 {
                    break;
                }
            }
            stream.write_all(&answer).unwrap();
            String::from_utf8_lossy(&seen).into_owned()
        });
        (url, served)
    }

    /// The render behind the person's Gemini lines (#8762) asks Google's
    /// speech model with their key in its header and their voice's name,
    /// and hands back its 24 kHz samples whole; a refusal of their key is
    /// told with its status, so the cache gives way to the lamp's voice.
    #[test]
    fn the_persons_gemini_render_asks_google_with_their_key_and_voice_and_reads_a_refusal() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let own = crate::second_config::OwnGeminiVoice {
            key: crate::second_config::Key::for_test("AIza-planted-8762\n"),
            voice: "Kore".into(),
        };
        let pcm: Vec<u8> = [5i16, 6, 7].iter().flat_map(|s| s.to_le_bytes()).collect();
        let event = serde_json::json!({"candidates": [{"content": {"parts": [
            {"inlineData": {"mimeType": "audio/l16; rate=24000; channels=1",
                "data": STANDARD.encode(pcm)}}
        ]}}]});
        let mut answer =
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n"
                .to_vec();
        answer.extend_from_slice(format!("data: {event}\n\n").as_bytes());
        let (url, served) = google_once(answer);
        let statuses = std::sync::Mutex::new(Vec::new());
        let said = render_gemini(&url, &own, "ok, to the voice, by say.sock", &|status| {
            statuses.lock().unwrap().push(status)
        })
        .unwrap();
        assert_eq!(said, [5, 6, 7]);
        let asked = served.join().unwrap();
        assert!(
            asked.starts_with(&format!(
                "POST /models/{}:streamGenerateContent?alt=sse ",
                crate::second_config::GEMINI_SPEECH_MODEL
            )),
            "{asked}"
        );
        assert!(
            asked
                .to_ascii_lowercase()
                .contains("x-goog-api-key: aiza-planted-8762\r\n")
        );
        assert!(asked.contains(r#""voiceName":"Kore""#), "{asked}");
        assert!(asked.contains("ok, to the voice, by say.sock"));

        let (url, served) = google_once(
            b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_vec(),
        );
        assert!(
            render_gemini(&url, &own, "Hello.", &|status| {
                statuses.lock().unwrap().push(status)
            })
            .is_err()
        );
        served.join().unwrap();
        assert_eq!(*statuses.lock().unwrap(), [401]);
    }
}
