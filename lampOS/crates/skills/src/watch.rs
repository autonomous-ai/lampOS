//! A watch: a skill that is on only while a person has asked for it, and
//! tells them by text who came to the desk while they were out. Guard mode
//! is the first (#1685, `docs/guard-mode.md`); "was anyone here while I was
//! out?" can count the same visits.
//!
//! - **On only when asked, by voice.** Its tool's `[choice]` turns it `on`
//!   or `off`. It is turned on only when the lamp heard someone ask for it,
//!   in words: the newest thing heard, within `[watch] asked_within` of the
//!   call, has one of `[watch] if_heard`'s entries among its whole words (a
//!   request typed in the app is not heard, and a call after talk of
//!   something else is refused), and only when its mark can be shown. Any
//!   voice at the lamp can ask: the texts go to the lamp's person, and the
//!   mark shows. It never starts itself: a watch has no trigger, and a load
//!   or a restart leaves it off.
//! - **A sign while on, or no counting.** Its faint `mark` is asked of the
//!   ring while it is on. It counts no visit and texts nothing until the
//!   ring says the mark is shown (`light/reached`), from the start and
//!   again whenever the ring says it is not (`light/outranked`): nobody is
//!   watched without a sign, also when the ring never answers (the peer
//!   read on #1807, reviewer-two on #1937). The ring says the mark shows
//!   again after each renewal, about every 10 s; with no such word for
//!   [`CONFIRMED_WITHIN_US`] (the ring worker stopped, or it forgot the
//!   claim), the mark is taken as hidden until the ring next says it shows
//!   (Third's privacy read on #1963). Outranked by another
//!   program's light, the program asks for the mark again with a new claim.
//!   Outranked by the person's own light ("turn the light off" on the way
//!   out), it never does: a person is never overridden. The watch pauses,
//!   says so once by text, and goes on only when they ask for it again in
//!   words, which is their yes to the mark over their light (the first
//!   builder's ruling on #1937).
//! - **A time limit.** It ends by itself `[watch] most` after it was asked,
//!   with one text that it has stopped.
//! - **Visits.** It counts a visit only once the desk has been empty since
//!   it was asked (`room/presence`, nobody): the person who asked has left.
//!   A visit is someone at the lamp, from when `room/presence` says so (or
//!   a face is seen near) until it says nobody again.
//! - **One text a while.** At a visit's end it sends one line, "Someone was
//!   at your desk at 14:31, for about 2 minutes.", at most once each
//!   `[watch] every`. Who it was is said only as "someone you've introduced
//!   me to" or "someone I don't know", never a name, unless names are on.
//! - **Names** (#2562 item 1, with [`NAMES_ENVIRONMENT`] on and
//!   `known_faces = true`). A known face whose person said yes to being
//!   named, the eyes' `tellable` that Who was here reads
//!   (`crate::known::tellable_in`), is named: "Minh and someone the lamp
//!   knows came in at 18:10". The file is read as the text goes, never as
//!   the face is seen, so a yes taken back before then names nobody. Only
//!   the eyes' known file gives a name, so a voice alone never does.
//! - **Arrivals by sound** (#2562, with [`SOUNDS_ENVIRONMENT`] on and
//!   [`SOUNDS`] in the skill's permissions). A door or footsteps on the
//!   ears' room sound classes is someone come in only when a face or speech
//!   follows within [`FOLLOWED_WITHIN_US`]: the visit then begins at the
//!   sound, and its text says they came in. A sound alone texts nothing.
//! - **The head turns once** to an arrival (#2562), as a creature would:
//!   one gaze to the largest face in view as a visit's first face is seen,
//!   then rest. No patrol, and none again within [`TURN_EVERY_US`]. The
//!   sound has no direction, so an arrival with no face turns nothing.
//! - **Ends.** On its tool's `off`; when the eyes know the face of the
//!   person who asked again, after they had left, with "I'm off guard" said
//!   aloud (its words' `off_guard`); or at its `most`, with that texted.
//! - **From the owner's app** (#2562, [`Watches::from_app`]): turned on and
//!   off by `guard.on` and `guard.off`, which only the owner's app sends,
//!   through lamp-cloud to the program's call socket. No voice is needed,
//!   but the mark is: it counts only while the ring confirms it.
//! - **The privacy switch ends it** (#2562). Closed, the watch is off at
//!   once, its mark given back, with one text (its words' `blind`); and it is never
//!   turned on while the switch is closed.
//!
//! No picture or sound is read or kept: only `room/presence` (counts and
//! times), the faces' sizes and places on `humans/faces/tracked`, the ears'
//! scores for a door, footsteps and speech, and whether the face at the
//! lamp is known, from the eyes' known file, which the program reads. A
//! name read there is kept in the program only to tell the person who asked
//! from anyone else, and, with names on, for the visit's text: it is never
//! in an event or the journal, and in a text only on the eyes' yes.
use crate::{Call, LightFile, Skill, Words as SayWords, duration_us, light_call, words};
use lamp_msgs::{Faces, OwnVoiceEvidence, SoundClasses, Voice, VoiceState};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The topics a watch listens to: who is at the lamp, the faces in view,
/// and what was heard (a call is turned on only just after someone spoke).
pub const LISTENS: [&str; 3] = ["room/presence", "humans/faces/tracked", "voice/heard"];
/// The privacy switch, read as the watch goes: `physical_muted` is the
/// switch closed. Said a hundred times a second, so looked at, never
/// listened to.
pub const PRIVACY: &str = "voice/privacy";
/// A face at least this share of the picture's height is someone at the
/// desk: the eyes recognise from 0.10 (`lamp-eyes`, `NEAR_ENOUGH`).
pub const NEAR: f32 = 0.10;
/// The tool's two values.
pub const ON: &str = "on";
pub const OFF: &str = "off";

/// The ears' room sound classes (#2562): a door, footsteps and speech, read
/// by a watch whose `[permissions]` topics name it, and only with
/// [`SOUNDS_ENVIRONMENT`] on.
pub const SOUNDS: &str = "ears/classes";
/// The voice's state, read by a watch whose `[permissions]` topics name it
/// beside [`SOUNDS`] (#2562): the ears score a window the lamp didn't speak
/// into with its own voice unknown, never clear (their speaking flag maps a
/// stale state to false), so the watch's own reading says it was quiet.
pub const VOICE: &str = "voice/state";
/// A reading of the voice lasts this long: it says its state once a second
/// and on each change, as a routine's quiet gate reads it.
pub const VOICE_FRESH_US: u64 = 3_000_000;
/// `on` takes arrivals by sound; off unless set (#2562).
pub const SOUNDS_ENVIRONMENT: &str = "LAMP_GUARD_SOUNDS";
/// `on` takes `guard.on` and `guard.off` from the owner's app on the call
/// socket; off unless set (#2562). lamp-cloud checks the same switch in its
/// own unit, so this is not the only gate.
pub const FROM_APP_ENVIRONMENT: &str = "LAMP_GUARD_FROM_APP";
/// `on` names, in a visit's text, a known face whose person said yes to
/// being named: the eyes' `tellable`, as Who was here reads it
/// (`crate::known::tellable_in`), read again as the text is made; off
/// unless set (#2562 item 1, the first builder's ruling of 22 Oct).
pub const NAMES_ENVIRONMENT: &str = "LAMP_GUARD_NAMES";
/// The most names a visit keeps, and so the most a text may say: a text is
/// one short line (`lamp_msgs::MESSAGE_TEXT_BYTES`).
pub const MOST_NAMES: usize = 3;
/// How long lamp-cloud waits for lamp-skills' answer to the app's
/// `guard.on` or `guard.off` (#2562, the reviewer's P2 on #2688): it puts
/// the moment that wait ends on the op, as [`DEADLINE`], so an op that
/// waited behind another call past it is let go here, never turned while
/// the app is told nothing was.
pub const FROM_APP_WAIT: std::time::Duration = std::time::Duration::from_secs(2);
/// The op's field: when lamp-cloud stops waiting, in µs on the host's
/// shared monotonic clock (`lamp_ipc::monotonic_us`, `CLOCK_MONOTONIC`, the
/// same in both programs on the one host): the moment it sent the op and
/// [`FROM_APP_WAIT`].
pub const DEADLINE: &str = "deadline_us";
/// Why the app's `name` (`guard.on` or `guard.off`), whose op said
/// `deadline_us`, is let go at `now_us`, before anything is turned: its
/// deadline reached, or none on it. An op with none comes from a lamp-cloud
/// older than this one, which did not say how long it waits; it is let go
/// too, which costs nothing, since taking the app's ops at all is behind
/// [`FROM_APP_ENVIRONMENT`], off by default.
pub fn let_go(name: &str, deadline_us: Option<u64>, now_us: u64) -> Option<String> {
    let wait_ms = FROM_APP_WAIT.as_millis();
    let Some(deadline_us) = deadline_us else {
        return Some(format!(
            "{name} let go: it says no {DEADLINE}, so it can't be told from one the app gave up on"
        ));
    };
    (now_us >= deadline_us).then(|| {
        let sent_us = deadline_us
            .saturating_sub(u64::try_from(FROM_APP_WAIT.as_micros()).unwrap_or(u64::MAX));
        let waited_ms = now_us.saturating_sub(sent_us) / 1_000;
        format!(
            "{name} let go: it waited {waited_ms} ms behind another call, past the app's {} s",
            wait_ms / 1_000
        )
    })
}
/// Whether a switch's value is `on`, exactly.
pub fn switched_on(value: Option<&str>) -> bool {
    value == Some("on")
}
/// Whether `/proc/<pid>/cgroup`'s text puts a peer of the call socket in
/// lamp-cloud's unit, the owner's app's link (#2562): the only one whose
/// `guard.on` and `guard.off` are taken, as the manage helper tells its
/// askers.
pub fn from_the_app(cgroup: &str) -> bool {
    crate::package::from_one_of(cgroup, &[crate::package::APP_LINK])
}
/// The skill the app's `guard.on` and `guard.off` turn.
pub const GUARD: &str = "guard-mode";
/// The app's two kinds, as the call socket's `guard` op names them to the
/// main loop: never a tool's name, which has no `.`.
pub const APP_ON: &str = "guard.on";
pub const APP_OFF: &str = "guard.off";
/// `on` gives guard mode's tool [`WORDS_V2_DESCRIPTION`] and
/// [`WORDS_V2_TURN`] in `tools.json`, in place of its `SKILL.md`
/// description and its `[choice]` `about`; off unless set (#2827). On
/// lamp-06c1 (22 Oct 09:14) "I'm back. You can stop keeping an eye on
/// things." was called `{"turn":"on"}`: the old words put on first and
/// name "keep an eye" for it, so a return that says those words read as on.
pub const WORDS_ENVIRONMENT: &str = "LAMP_GUARD_WORDS_V2";
/// Whether [`WORDS_ENVIRONMENT`]'s value turns the new words on: `on` only.
pub fn words_v2(value: Option<&str>) -> bool {
    value == Some("on")
}
/// `keep_watch`'s description with [`WORDS_ENVIRONMENT`] on: off first,
/// with the very lines that were read as on, and "even when their words
/// name keeping an eye or watching"; on only for a request to start. The
/// rest is `SKILL.md`'s.
pub const WORDS_V2_DESCRIPTION: &str = "Keeps an eye on the desk while the person is out and texts them once if someone comes to it. Call keep_watch with turn off when they tell you they're back or here now, thank you for watching, or say you can stop or there's no need, even when their words name keeping an eye or watching: \"I'm back, you can stop keeping an eye on things\", \"thanks, I'm here now\" and \"no need to keep watch\" are all off. Call it with turn on only when the person at the lamp asks you out loud to start keeping an eye on things while they go out (\"keep an eye on things while I'm out\", \"watch my desk\"), never on your own and never for a typed message. While it's on, the ring shows a faint blue mark. It says who came only as someone they've introduced you to or someone you don't know, never a name, and it never records, takes or sends a picture or a sound.";
/// The `turn` choice's description with [`WORDS_ENVIRONMENT`] on, off
/// first; at most 256 bytes, as a `[choice]` `about` is.
pub const WORDS_V2_TURN: &str = "off: they're back or here now, thanked you for watching, or said stop or no need, even with \"keep an eye\" or \"watch\" in it; on: they asked you out loud to start keeping an eye on things while they're out";
/// A door or footsteps scored at least this is a sound someone may have
/// come in by, and speech at least this, with the lamp's own voice clear,
/// is someone speaking. Not yet measured on a lamp.
pub const SOUND_AT_LEAST: f32 = 0.5;
/// A door or footsteps is someone come in only when a face or speech
/// follows within this.
pub const FOLLOWED_WITHIN_US: u64 = 30_000_000;
/// The head turns to an arrival at most once in this long (rule 8).
pub const TURN_EVERY_US: u64 = 120_000_000;
/// A shown mark is said reached again after each renewal, every 10 s
/// (`glow::GLOW_EVERY`): with no such word this long, about three
/// renewals, it is taken as hidden, and nothing is counted until the next.
pub const CONFIRMED_WITHIN_US: u64 = 30_000_000;
/// What the journal says when the ring has gone that long without a word.
pub const UNCONFIRMED: &str = "its mark has not been confirmed for 30 s: no visit counted";

/// The app's refusals that are not the skill's to say (#2562): guard mode
/// not there to say them, and a peer that is not the owner's app.
pub const NOT_INSTALLED: &str = "guard mode isn't installed on this lamp";
pub const NOT_THE_APP: &str = "guard mode is turned on and off from afar only by the owner's app";

/// `[watch]` as `skill.toml` says it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WatchFile {
    /// At most one text in this long.
    every: String,
    /// Turned on only when someone was heard at the lamp this lately...
    asked_within: String,
    /// ...asking for it: one of these, each one or more whole words.
    if_heard: Vec<String>,
    /// It ends by itself this long after it was asked.
    most: String,
    /// The faint light shown while it is on.
    mark: LightFile,
    /// What it says, in the skill's own words ([`Words`]); a file from
    /// before them says the built-in ones, for one release.
    words: Option<Words>,
}

/// What a watch says, in its skill's own words (`[watch.words]`, #940): its
/// answers to the model, its refusals, its texts to the person's phone, and
/// a visit's text, put together from `visit`'s `{who}`, `{at}` and `{long}`.
/// Each is words, as a say's are, and only the slots named here.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Words {
    /// The model's answer, turned on by someone whose face it knows...
    pub on: String,
    /// ...or by someone whose face it doesn't.
    pub on_unknown: String,
    /// Asked again while it's on.
    pub already_on: String,
    /// On again after the person's own light paused it.
    pub on_again: String,
    /// Turned off.
    pub off: String,
    /// Turned off while it wasn't on.
    pub was_off: String,
    /// Refused: not asked out loud at the lamp.
    pub not_asked: String,
    /// Refused: its mark can't be shown.
    pub no_mark: String,
    /// Refused: the privacy switch is closed (#2562).
    pub closed: String,
    /// The owner's app refused: the person's own light paused it (#2562).
    pub paused_from_app: String,
    /// Texted when the privacy switch closes, which ends it (#2562).
    pub blind: String,
    /// Texted when the person's own light pauses it.
    pub paused: String,
    /// Said aloud when the face of the person who asked ends it (#2562).
    pub off_guard: String,
    /// Texted when it ends by itself, with `{hours}` once (#2562)...
    pub stopped: String,
    /// ...`an_hour` for its `most` of one...
    pub an_hour: String,
    /// ...or `{hours}`, once.
    pub hours: String,
    /// A visit's text: `{who}`, `{at}` and `{long}`, each once.
    pub visit: String,
    /// A visit that came in by a door or footsteps (#2562): the same slots.
    pub came_in: String,
    /// `visit` when `{who}` is more than one person, by name and beside
    /// them (#2562): "Minh and Lan were at your desk…". A file from before
    /// it says the built-in.
    #[serde(default = "built_in_visit_many")]
    pub visit_many: String,
    /// `came_in` for more than one: "…came in at 18:10 and were here…".
    #[serde(default = "built_in_came_in_many")]
    pub came_in_many: String,
    /// `{who}`: a face the eyes know...
    pub known: String,
    /// ...a face they don't...
    pub unknown: String,
    /// ...or no face seen.
    pub someone: String,
    /// With [`NAMES_ENVIRONMENT`] on (#2562): `{who}` when known faces that
    /// may be named came with one that may not, `{names}` once: "Minh and
    /// someone the lamp knows". A file from before it says the built-in.
    #[serde(default = "built_in_named_and_known")]
    pub named_and_known: String,
    /// Two or more names, the last `{name}` after the rest as `{names}`,
    /// each once: "Minh and Lan".
    #[serde(default = "built_in_and")]
    pub and: String,
    /// `{at}`, with `{time}` once (`14:31`); nothing when the time isn't
    /// known.
    pub at: String,
    /// `{long}`: under a minute...
    pub under_a_minute: String,
    /// ...about one...
    pub a_minute: String,
    /// ...or about `{minutes}`, once.
    pub minutes: String,
}

impl Words {
    /// The words lamp-skills said before they were the skill's, said for
    /// one release by a watch whose file has no `[watch.words]`: guard mode
    /// is installable, so a lamp keeps the copy it has past a newer
    /// lamp-skills (as the first builder's P2 on #2632 found for
    /// prepare-unplug). The next release removes them, and such a file is
    /// then refused.
    pub fn built_in() -> Self {
        Self {
            on: "Say in one short line that you'll keep an eye on things while they're out and text them if someone comes to the desk.".into(),
            on_unknown: "Say in one short line that you'll keep an eye on things while they're out and text them if someone comes to the desk. You don't know their face, so ask them to tell you to stop when they're back.".into(),
            already_on: "You're already keeping an eye on things. Say so in one short line.".into(),
            on_again: "Say in one short line that you're keeping an eye on things again.".into(),
            off: "Say in one short line that you've stopped keeping an eye on things.".into(),
            was_off: "You weren't keeping an eye on things. Say so in one short line.".into(),
            not_asked: "I keep an eye on things only when someone asks me out loud, at the lamp.".into(),
            no_mark: "I can't show that I'm keeping an eye on things just now, so I won't do it.".into(),
            closed: "The privacy switch is closed, so I can't see: I won't keep an eye on things.".into(),
            paused_from_app: "Guard is paused while your light is off: ask at the lamp for it to go on.".into(),
            blind: "Guard is off: the privacy switch is closed.".into(),
            paused: "I've paused watching while your light is off.".into(),
            off_guard: "I'm off guard.".into(),
            stopped: "I'm off guard: it's been {hours}.".into(),
            an_hour: "an hour".into(),
            hours: "{hours} hours".into(),
            visit: "{who} was at your desk{at}, {long}.".into(),
            came_in: "{who} came in{at} and was here {long}.".into(),
            visit_many: built_in_visit_many(),
            came_in_many: built_in_came_in_many(),
            known: "Someone you've introduced me to".into(),
            unknown: "Someone I don't know".into(),
            someone: "Someone".into(),
            named_and_known: built_in_named_and_known(),
            and: built_in_and(),
            at: " at {time}".into(),
            under_a_minute: "for under a minute".into(),
            a_minute: "for about a minute".into(),
            minutes: "for about {minutes} minutes".into(),
        }
    }

    /// The text when it ends by itself at its `most` (#2562): "I'm off
    /// guard: it's been 12 hours."
    pub fn ended(&self, most_us: u64) -> String {
        let hours = match most_us / 3_600_000_000 {
            1 => self.an_hour.clone(),
            hours => self.hours.replace("{hours}", &hours.to_string()),
        };
        self.stopped.replace("{hours}", &hours)
    }

    /// Each is words of 1 to [`crate::MOST_WORDS_BYTES`] bytes with no
    /// control characters, with exactly the slots it may have, each once.
    fn checked(self) -> Result<Self, String> {
        checked_words(
            "[watch.words]",
            &[
                ("on", &self.on, &[][..]),
                ("on_unknown", &self.on_unknown, &[]),
                ("already_on", &self.already_on, &[]),
                ("on_again", &self.on_again, &[]),
                ("off", &self.off, &[]),
                ("was_off", &self.was_off, &[]),
                ("not_asked", &self.not_asked, &[]),
                ("no_mark", &self.no_mark, &[]),
                ("closed", &self.closed, &[]),
                ("paused_from_app", &self.paused_from_app, &[]),
                ("blind", &self.blind, &[]),
                ("paused", &self.paused, &[]),
                ("off_guard", &self.off_guard, &[]),
                ("stopped", &self.stopped, &["hours"]),
                ("an_hour", &self.an_hour, &[]),
                ("hours", &self.hours, &["hours"]),
                ("visit", &self.visit, &["at", "long", "who"]),
                ("came_in", &self.came_in, &["at", "long", "who"]),
                ("visit_many", &self.visit_many, &["at", "long", "who"]),
                ("came_in_many", &self.came_in_many, &["at", "long", "who"]),
                ("known", &self.known, &[]),
                ("unknown", &self.unknown, &[]),
                ("someone", &self.someone, &[]),
                ("named_and_known", &self.named_and_known, &["names"]),
                ("and", &self.and, &["name", "names"]),
                ("at", &self.at, &["time"]),
                ("under_a_minute", &self.under_a_minute, &[]),
                ("a_minute", &self.a_minute, &[]),
                ("minutes", &self.minutes, &["minutes"]),
            ],
        )?;
        Ok(self)
    }
}

/// `visit_many` for a `[watch.words]` from before it (#2562).
fn built_in_visit_many() -> String {
    "{who} were at your desk{at}, {long}.".into()
}

/// `came_in_many` for a `[watch.words]` from before it (#2562).
fn built_in_came_in_many() -> String {
    "{who} came in{at} and were here {long}.".into()
}

/// `named_and_known` for a `[watch.words]` from before it (#2562).
fn built_in_named_and_known() -> String {
    "{names} and someone the lamp knows".into()
}

/// `and` for a `[watch.words]` from before it (#2562).
fn built_in_and() -> String {
    "{names} and {name}".into()
}

/// Each line of a skill's own words is checked as the voice checks its
/// own skills' words (#940): one checker, so a line lamp-skills loads is
/// one the voice reads.
pub(crate) use lamp_voice::skill_words::checked_words;

/// A watch as loaded.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub every_us: u64,
    pub asked_within_us: u64,
    /// Each entry's whole words, lowercase.
    pub if_heard: Vec<Vec<String>>,
    pub most_us: u64,
    /// A light, at level 1 or 2, steady.
    pub mark: Call,
    /// What it says ([`Words`]).
    pub words: Words,
    /// Whether its file has no `[watch.words]`, so [`Words::built_in`] is
    /// said, for this release only: the journal says so at each load.
    pub built_in_words: bool,
}

/// `[watch]` checked: a text at most every 1 minute to a day, asked within 5
/// seconds to 2 minutes in words from 1 to 16 entries, at most 1 to 24
/// hours on, and a faint, steady mark.
pub(crate) fn spec(file: WatchFile) -> Result<Spec, String> {
    let every_us = duration_us(&file.every)?;
    if !(60_000_000..=86_400_000_000).contains(&every_us) {
        return Err("[watch] every is 1m to 24h".into());
    }
    let asked_within_us = duration_us(&file.asked_within)?;
    if !(5_000_000..=120_000_000).contains(&asked_within_us) {
        return Err("[watch] asked_within is 5s to 2m".into());
    }
    let if_heard: Vec<Vec<String>> = file.if_heard.iter().map(|entry| words(entry)).collect();
    if !(1..=16).contains(&if_heard.len())
        || if_heard.iter().any(Vec::is_empty)
        || file.if_heard.iter().any(|entry| entry.len() > 64)
    {
        return Err(
            "[watch] if_heard is 1 to 16 entries, each one or more words in at most 64 bytes"
                .into(),
        );
    }
    let most_us = duration_us(&file.most)?;
    if !(3_600_000_000..=86_400_000_000).contains(&most_us) {
        return Err("[watch] most is 1h to 24h".into());
    }
    let mark = light_call(file.mark)?;
    match &mark {
        Call::Light {
            level: Some(1 | 2),
            lasting: None,
            pattern: None,
            keep: false,
            until_ends: false,
            ..
        } => {}
        _ => {
            return Err(
                "[watch] mark is a faint light: a colour at level 1 or 2, steady, with no for and not kept"
                    .into(),
            );
        }
    }
    let built_in_words = file.words.is_none();
    let words = file
        .words
        .map_or_else(|| Ok(Words::built_in()), Words::checked)?;
    Ok(Spec {
        every_us,
        asked_within_us,
        if_heard,
        most_us,
        mark,
        words,
        built_in_words,
    })
}

/// What a watch does.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// Show the faint mark, this light, until [`Out::Unmark`].
    Mark(Call),
    /// Give the mark back.
    Unmark,
    /// One short line for the person's phone, on `messages/out`.
    Text(String),
    /// One look, this gaze, to an arrival (#2562): then the body rests.
    Gaze(Call),
    /// Words said aloud as written, "I'm off guard" alone (#2562).
    Say(SayWords),
    /// A [`Out::Text`] that names someone who said yes to being named
    /// ([`NAMES_ENVIRONMENT`], #2562): sent the same, with `unnamed`, the
    /// line as it reads with names off, and `named`, who it names as their
    /// `lamp_msgs::known::person_id`s, beside it on `messages/out`
    /// ([`text_message`]), so `lamp-cloud` sends it named only while each
    /// yes still stands. The journal and `skills/events` never say its
    /// words.
    Named {
        text: String,
        unnamed: String,
        named: Vec<String>,
    },
    /// What happened, for `skills/events` and the journal: never a name.
    Said(String),
}

/// Someone at the desk while the watch is on.
#[derive(Debug)]
struct Visit {
    began_us: u64,
    /// The local time it began, in minutes of the day, if known.
    began_minute: Option<u16>,
    /// The last moment someone was there.
    seen_us: u64,
    /// A face was seen near.
    face: bool,
    /// A face the eyes know, not the person who asked, was seen.
    known: bool,
    /// With [`NAMES_ENVIRONMENT`] on: the known faces' names, in the order
    /// first seen, at most [`MOST_NAMES`], kept in the program only. Whether
    /// each may be said is asked of the eyes' `tellable` as the text is
    /// made, never as the face is seen.
    names: Vec<String>,
    /// It began with a door or footsteps, a face or speech after (#2562).
    came_in: bool,
    /// The head turned to it, or was let be: once a visit.
    turned: bool,
}

impl Visit {
    /// "Someone was at your desk at 14:31, for about 2 minutes.": who only
    /// as known or not, and by name only for the names in `tellable`, the
    /// eyes' word now on who said yes to being named (#2562).
    fn text(&self, words: &Words, tellable: &[String]) -> String {
        let named: Vec<&str> = self
            .names
            .iter()
            .filter(|name| tellable.contains(name))
            .map(String::as_str)
            .collect();
        // Two or more named, or one named beside someone known: more than
        // one person, so the line says "were".
        let many = !named.is_empty() && self.names.len() > 1;
        let who = match named.split_last() {
            Some((last, rest)) => {
                let names = if rest.is_empty() {
                    (*last).to_owned()
                } else {
                    words
                        .and
                        .replace("{names}", &rest.join(", "))
                        .replace("{name}", last)
                };
                // Anyone else known, not to be named, is said as known.
                if self.names.len() > named.len() {
                    words.named_and_known.replace("{names}", &names)
                } else {
                    names
                }
            }
            None if self.known => words.known.clone(),
            None if self.face => words.unknown.clone(),
            None => words.someone.clone(),
        };
        let at = self.began_minute.map_or(String::new(), |minute| {
            words
                .at
                .replace("{time}", &format!("{}:{:02}", minute / 60, minute % 60))
        });
        let seconds = self.seen_us.saturating_sub(self.began_us) / 1_000_000;
        let long = match (seconds + 30) / 60 {
            _ if seconds < 60 => words.under_a_minute.clone(),
            1 => words.a_minute.clone(),
            minutes => words.minutes.replace("{minutes}", &minutes.to_string()),
        };
        // The parts have no slots of their own, so none is filled twice.
        let text = match (self.came_in, many) {
            (true, false) => &words.came_in,
            (true, true) => &words.came_in_many,
            (false, false) => &words.visit,
            (false, true) => &words.visit_many,
        };
        text.replace("{who}", &who)
            .replace("{at}", &at)
            .replace("{long}", &long)
    }
}

/// A watch that is on.
#[derive(Debug)]
struct On {
    /// When it was asked: it ends by itself `most` after.
    since_us: u64,
    /// The ring has not said its mark is shown (`light/reached`), or has
    /// said since that it is not: nothing about the desk is taken until it
    /// says it is. So from the start, until the ring's first word. And once
    /// the ring has said nothing of it for [`CONFIRMED_WITHIN_US`].
    hidden: bool,
    /// When the ring last said its mark is shown.
    reached_us: Option<u64>,
    /// The person's own light is over its mark: paused until they ask for
    /// it again in words, never asked for over them.
    paused: bool,
    /// The known person who asked, if the eyes knew them: their face back
    /// ends the watch. Kept in the program only.
    asker: Option<String>,
    /// The desk has been empty since it was asked: visits count from then.
    away: bool,
    visit: Option<Visit>,
    /// When the last text about a visit went.
    texted_us: Option<u64>,
    /// A door or footsteps heard with nobody at the desk, and the local
    /// minute: someone came in if a face or speech follows (#2562).
    sound: Option<(u64, Option<u16>)>,
    /// When the head last turned to an arrival.
    turned_us: Option<u64>,
}

impl On {
    fn new(since_us: u64, asker: Option<&str>) -> Self {
        Self {
            since_us,
            hidden: true,
            reached_us: None,
            paused: false,
            asker: asker.map(str::to_owned),
            away: false,
            visit: None,
            texted_us: None,
            sound: None,
            turned_us: None,
        }
    }

    /// Whether what it is told about the desk is taken: the desk empty
    /// since it was asked, and its mark shown.
    fn counting(&self) -> bool {
        self.away && !self.hidden
    }

    fn visit(&mut self, now_us: u64, minute: Option<u16>) -> &mut Visit {
        let visit = self.visit.get_or_insert(Visit {
            began_us: now_us,
            began_minute: minute,
            seen_us: now_us,
            face: false,
            known: false,
            names: Vec::new(),
            came_in: false,
            turned: false,
        });
        visit.seen_us = now_us;
        visit
    }

    /// A face or speech now: a door or footsteps within
    /// [`FOLLOWED_WITHIN_US`] before was someone coming in, and the visit
    /// begins at the sound (#2562).
    fn followed(&mut self, now_us: u64, minute: Option<u16>, by: &str) -> Vec<Out> {
        let Some((at_us, at_minute)) = self.sound.take() else {
            return Vec::new();
        };
        if now_us.saturating_sub(at_us) > FOLLOWED_WITHIN_US {
            return Vec::new();
        }
        let visit = self.visit(now_us, minute);
        if !visit.came_in {
            visit.came_in = true;
            visit.began_us = visit.began_us.min(at_us);
            visit.began_minute = at_minute.or(visit.began_minute);
        }
        vec![Out::Said(format!(
            "someone came in: a door or footsteps, then {by} {} s after",
            now_us.saturating_sub(at_us) / 1_000_000
        ))]
    }

    /// The visit's first face, at `face`: the head turns to it once, unless
    /// it turned to an arrival within [`TURN_EVERY_US`] (#2562).
    fn turn_to(&mut self, face: (f32, f32, f32), now_us: u64) -> Vec<Out> {
        let last = self.turned_us;
        let Some(visit) = self.visit.as_mut().filter(|visit| !visit.turned) else {
            return Vec::new();
        };
        visit.turned = true;
        if last.is_some_and(|at| now_us.saturating_sub(at) < TURN_EVERY_US) {
            return vec![Out::Said(
                "an arrival: the head turned to one under 2 min ago, so not again".into(),
            )];
        }
        self.turned_us = Some(now_us);
        vec![
            Out::Gaze(Call::Gaze { face: Some(face) }),
            Out::Said("an arrival: the head turns to it once".into()),
        ]
    }

    /// A visit over: its text, unless one went within `every`. With names
    /// on, `tellable` is the eyes' file, read now, as the text goes: a yes
    /// taken back since the face was seen names nobody (#2562).
    fn ended(
        &mut self,
        visit: Visit,
        now_us: u64,
        spec: &Spec,
        tellable: Option<&std::path::Path>,
    ) -> Vec<Out> {
        if let Some(texted_us) = self.texted_us
            && now_us.saturating_sub(texted_us) < spec.every_us
        {
            return vec![Out::Said(format!(
                "a visit not texted: one text each {} s, and the last went {} s ago",
                spec.every_us / 1_000_000,
                now_us.saturating_sub(texted_us) / 1_000_000
            ))];
        }
        self.texted_us = Some(now_us);
        let tellable = match tellable {
            Some(path) if !visit.names.is_empty() => crate::known::tellable_in(path),
            _ => Vec::new(),
        };
        let named: Vec<String> = visit
            .names
            .iter()
            .filter(|name| tellable.contains(name))
            .map(|name| lamp_msgs::known::person_id(name))
            .collect();
        let text = visit.text(&spec.words, &tellable);
        vec![if named.is_empty() {
            Out::Text(text)
        } else {
            Out::Named {
                text,
                unnamed: visit.text(&spec.words, &[]),
                named,
            }
        }]
    }
}

/// The `messages/out` message for a watch's text ([`Out::Text`] or
/// [`Out::Named`]), from `skill`, checked as it is published; none for any
/// other `out`. A named text carries its unnamed words and who it names
/// (#2562). One whose named words would not pass `MessageOut::check`, such
/// as long names past one line, goes as its unnamed words alone: the visit's
/// text is never lost for a name.
pub fn text_message(
    skill: &str,
    out: Out,
) -> Option<Result<lamp_msgs::MessageOut, lamp_msgs::Invalid>> {
    let from: String = format!("skill:{skill}").chars().take(64).collect();
    let message = match out {
        Out::Text(text) => lamp_msgs::MessageOut {
            from,
            text,
            unnamed: None,
            named: Vec::new(),
        },
        Out::Named {
            text,
            unnamed,
            named,
        } => {
            let message = lamp_msgs::MessageOut {
                from: from.clone(),
                text,
                unnamed: Some(unnamed.clone()),
                named,
            };
            if message.check().is_ok() {
                return Some(Ok(message));
            }
            lamp_msgs::MessageOut {
                from,
                text: unnamed,
                unnamed: None,
                named: Vec::new(),
            }
        }
        _ => return None,
    };
    Some(message.check().map(|()| message))
}

/// One watch: off, or on with what it has seen.
#[derive(Debug, Default)]
pub struct Watch {
    on: Option<On>,
}

impl Watch {
    pub fn is_on(&self) -> bool {
        self.on.is_some()
    }

    /// Its tool's call: `on` only when someone was heard at the lamp within
    /// `asked_within` (`heard`: when, and what), asking for it in words
    /// (`if_heard`), and the mark can be shown; `off` always. `asker` is the
    /// known person at the lamp as it is asked. The model's answer and what
    /// to do, or why not, in words the lamp can say.
    pub fn turn(
        &mut self,
        on: bool,
        spec: &Spec,
        now_us: u64,
        heard: Option<(u64, &str)>,
        asker: Option<&str>,
        can_mark: bool,
    ) -> Result<(Value, Vec<Out>), String> {
        if !on {
            return Ok(match self.on.take() {
                Some(_) => (
                    json!({ "next": spec.words.off }),
                    vec![Out::Unmark, Out::Said("off, as asked".into())],
                ),
                None => (json!({ "next": spec.words.was_off }), Vec::new()),
            });
        }
        let asked = heard.is_some_and(|(heard_us, text)| {
            let said = words(text);
            heard_us <= now_us
                && now_us - heard_us <= spec.asked_within_us
                && spec
                    .if_heard
                    .iter()
                    .any(|entry| said.windows(entry.len()).any(|run| run == entry.as_slice()))
        });
        if let Some(on) = &mut self.on {
            if !on.paused {
                return Ok((json!({ "next": spec.words.already_on }), Vec::new()));
            }
            // Paused by their own light: asked again in words, it goes on,
            // its mark asked for anew; it counts once the ring shows it.
            if !asked {
                return Err(spec.words.not_asked.clone());
            }
            on.paused = false;
            return Ok((
                json!({ "next": spec.words.on_again }),
                vec![
                    Out::Mark(spec.mark.clone()),
                    Out::Said("on again, asked at the lamp: its mark asked for anew".into()),
                ],
            ));
        }
        if !asked {
            return Err(spec.words.not_asked.clone());
        }
        if !can_mark {
            return Err(spec.words.no_mark.clone());
        }
        self.on = Some(On::new(now_us, asker));
        let (note, by) = match asker {
            Some(_) => (&spec.words.on, ", by someone whose face it knows"),
            None => (
                &spec.words.on_unknown,
                ", by someone whose face it does not know",
            ),
        };
        Ok((
            json!({ "next": note }),
            vec![
                Out::Mark(spec.mark.clone()),
                Out::Said(format!("on, asked at the lamp{by}")),
            ],
        ))
    }

    /// `guard.on` or `guard.off` from the owner's app (#2562): no voice is
    /// asked for, since the owner's account sent it; the mark is, and it
    /// counts only once the ring confirms it. Its asker is nobody's face, so
    /// no face ends it. A watch paused by the person's own light goes on
    /// only when asked at the lamp. The app's answer, or why not.
    pub fn from_app(
        &mut self,
        on: bool,
        spec: &Spec,
        now_us: u64,
        can_mark: bool,
    ) -> Result<(Value, Vec<Out>), String> {
        if !on {
            return Ok(match self.on.take() {
                Some(_) => (
                    json!({ "guard": OFF }),
                    vec![Out::Unmark, Out::Said("off, from the owner's app".into())],
                ),
                None => (json!({ "guard": OFF }), Vec::new()),
            });
        }
        match &self.on {
            Some(on) if on.paused => return Err(spec.words.paused_from_app.clone()),
            Some(_) => return Ok((json!({ "guard": ON }), Vec::new())),
            None => {}
        }
        if !can_mark {
            return Err(spec.words.no_mark.clone());
        }
        self.on = Some(On::new(now_us, None));
        Ok((
            json!({ "guard": ON }),
            vec![
                Out::Mark(spec.mark.clone()),
                Out::Said("on, from the owner's app".into()),
            ],
        ))
    }

    /// `room/presence`: someone at the lamp or not. A visit's text names
    /// only those in the eyes' `tellable` file, read as it goes, with names
    /// on ([`NAMES_ENVIRONMENT`]).
    pub fn presence(
        &mut self,
        someone: bool,
        now_us: u64,
        minute: Option<u16>,
        spec: &Spec,
        tellable: Option<&std::path::Path>,
    ) -> Vec<Out> {
        let Some(on) = &mut self.on else {
            return Vec::new();
        };
        if on.hidden {
            return Vec::new();
        }
        if !on.away {
            if someone {
                return Vec::new();
            }
            on.away = true;
            return vec![Out::Said("the desk is empty: keeping watch".into())];
        }
        if someone {
            on.visit(now_us, minute);
            return Vec::new();
        }
        match on.visit.take() {
            Some(visit) => on.ended(visit, now_us, spec, tellable),
            None => Vec::new(),
        }
    }

    /// The largest face in view, if any: its centre across and down from
    /// the middle of the picture (-1 to 1) and its height as a share of it,
    /// as the body's gaze takes it. Near the lamp, it is someone at the
    /// desk; anywhere in view, it follows a door or footsteps (#2562). A
    /// visit's first face turns the head to it, once.
    pub fn face(
        &mut self,
        seen: Option<(f32, f32, f32)>,
        now_us: u64,
        minute: Option<u16>,
    ) -> Vec<Out> {
        let (Some(on), Some(face)) = (&mut self.on, seen) else {
            return Vec::new();
        };
        if !on.counting() {
            return Vec::new();
        }
        let mut outs = on.followed(now_us, minute, "a face");
        if face.2 >= NEAR {
            on.visit(now_us, minute).face = true;
        }
        outs.extend(on.turn_to(face, now_us));
        outs
    }

    /// A door or footsteps (`door`), or speech (`speech`), on the ears'
    /// room sound classes (#2562). A door or footsteps with nobody at the
    /// desk waits [`FOLLOWED_WITHIN_US`] for a face or speech; alone, it is
    /// nothing. Speech follows one.
    pub fn sound(
        &mut self,
        door: bool,
        speech: bool,
        now_us: u64,
        minute: Option<u16>,
    ) -> Vec<Out> {
        let Some(on) = &mut self.on else {
            return Vec::new();
        };
        if !on.counting() {
            return Vec::new();
        }
        if speech {
            return on.followed(now_us, minute, "speech");
        }
        if !door || on.visit.is_some() || on.sound.is_some() {
            return Vec::new();
        }
        on.sound = Some((now_us, minute));
        vec![Out::Said(
            "a door or footsteps: someone came in only if a face or speech follows within 30 s"
                .into(),
        )]
    }

    /// The eyes know the face at the lamp as `name`, or no face there is
    /// known. The person who asked, back after they left, ends the watch.
    /// With `naming`, the name is kept with the visit, to be said only if
    /// the eyes say, as the text goes, that its person said yes.
    pub fn known(
        &mut self,
        name: Option<&str>,
        now_us: u64,
        minute: Option<u16>,
        spec: &Spec,
        naming: bool,
    ) -> Vec<Out> {
        let (Some(on), Some(name)) = (&mut self.on, name) else {
            return Vec::new();
        };
        if !on.counting() {
            return Vec::new();
        }
        if on.asker.as_deref() == Some(name) {
            self.on = None;
            return vec![
                Out::Unmark,
                Out::Say(SayWords {
                    words: spec.words.off_guard.clone(),
                    exact: true,
                    priority: crate::SAY_PRIORITY,
                    expires_ms: crate::SAY_EXPIRES_MS,
                }),
                Out::Said("off: the person who asked is back".into()),
            ];
        }
        let outs = on.followed(now_us, minute, "a known face");
        let visit = on.visit(now_us, minute);
        visit.face = true;
        visit.known = true;
        if naming && visit.names.len() < MOST_NAMES && !visit.names.iter().any(|kept| kept == name)
        {
            visit.names.push(name.to_owned());
        }
        outs
    }

    /// The privacy switch, closed or not: closed, the watch is off at once,
    /// its mark given back, with one text, its words' `blind` (#2562). It stays off when the
    /// switch opens again, until it is asked for again.
    pub fn privacy(&mut self, closed: bool, spec: &Spec) -> Vec<Out> {
        if !closed || self.on.take().is_none() {
            return Vec::new();
        }
        vec![
            Out::Unmark,
            Out::Text(spec.words.blind.clone()),
            Out::Said("off: the privacy switch closed".into()),
        ]
    }

    /// The ring says at `now_us` its mark is shown (`light/reached`) or not
    /// (`light/outranked`, `by` what): while it is not, nothing about the
    /// desk is taken, so nobody is watched without a sign.
    pub fn mark_shown(&mut self, shown: bool, by: Option<&str>, now_us: u64) -> Vec<Out> {
        let Some(on) = &mut self.on else {
            return Vec::new();
        };
        // Paused by the person's light, only their asking again ends it.
        if on.paused {
            return Vec::new();
        }
        if shown {
            on.reached_us = Some(now_us);
        }
        if on.hidden != shown {
            return Vec::new();
        }
        on.hidden = !shown;
        vec![Out::Said(if shown {
            "its mark shows: counting visits".into()
        } else {
            format!(
                "its mark is not shown (outranked by {}): no visit counted until it is",
                by.unwrap_or("something")
            )
        })]
    }

    /// The person's own light is over its mark (`light/outranked` by the
    /// person): paused, once, its line; nothing about the desk is taken, and
    /// its mark is not asked for over them.
    pub fn paused_by_person(&mut self, spec: &Spec) -> Vec<Out> {
        let Some(on) = &mut self.on else {
            return Vec::new();
        };
        if on.paused {
            return Vec::new();
        }
        on.paused = true;
        on.hidden = true;
        vec![
            Out::Text(spec.words.paused.clone()),
            Out::Said(
                "paused: the person's own light is over its mark; on again only when they ask"
                    .into(),
            ),
        ]
    }

    /// Time passes: at its `most` after it was asked, it ends, with one
    /// text that it is off guard. A shown mark the ring has not said shows
    /// for [`CONFIRMED_WITHIN_US`] is taken as hidden, said once (Third's
    /// privacy read on #1963): the ring's next word that it shows counts
    /// again. A door or footsteps that nothing followed within
    /// [`FOLLOWED_WITHIN_US`] is let go: nobody came in (#2562).
    pub fn tick(&mut self, now_us: u64, spec: &Spec) -> Vec<Out> {
        let Some(on) = &mut self.on else {
            return Vec::new();
        };
        if now_us.saturating_sub(on.since_us) < spec.most_us {
            let mut outs = Vec::new();
            if on
                .sound
                .is_some_and(|(at, _)| now_us.saturating_sub(at) > FOLLOWED_WITHIN_US)
            {
                on.sound = None;
                outs.push(Out::Said(
                    "a door or footsteps with no face or speech within 30 s: nobody came in".into(),
                ));
            }
            let unconfirmed = !on.hidden
                && on
                    .reached_us
                    .is_none_or(|at| now_us.saturating_sub(at) > CONFIRMED_WITHIN_US);
            if unconfirmed {
                on.hidden = true;
                outs.push(Out::Said(UNCONFIRMED.into()));
            }
            return outs;
        }
        self.on = None;
        vec![
            Out::Unmark,
            Out::Text(spec.words.ended(spec.most_us)),
            Out::Said(format!(
                "off: {} h since it was asked, its most",
                spec.most_us / 3_600_000_000
            )),
        ]
    }
}

/// One loaded watch skill: its watch as loaded, its tool and choice, and
/// whether it is on.
#[derive(Debug)]
struct Entry {
    spec: Spec,
    tool: String,
    choice: String,
    watch: Watch,
    /// Its `[permissions]` topics name [`SOUNDS`]: it may hear a door,
    /// footsteps and speech (#2562).
    sounds: bool,
    /// Its `[permissions]` topics name [`VOICE`] too: it may hear a window
    /// the ears scored with the lamp's own voice unknown, when the voice's
    /// own state says the lamp wasn't speaking over it (#2562).
    voice: bool,
    /// Its `[permissions]` have `known_faces = true`: with names on, it may
    /// name someone who said yes (#2562).
    known_faces: bool,
}

/// Every watch skill's watch, by skill, when the lamp last heard someone
/// speak, and whether the privacy switch is closed.
#[derive(Debug, Default)]
pub struct Watches {
    each: BTreeMap<String, Entry>,
    /// When the newest thing was heard at the lamp, and what.
    heard: Option<(u64, String)>,
    /// Arrivals by sound are taken ([`SOUNDS_ENVIRONMENT`], #2562): off
    /// unless the program says so.
    pub sounds: bool,
    /// The privacy switch was last seen closed: no watch goes on.
    closed: bool,
    /// Since when every reading of [`VOICE`], each within
    /// [`VOICE_FRESH_US`] of the one before, said the lamp wasn't speaking,
    /// and the newest's time, on the monotonic clock in µs, as the ears'
    /// windows are. None: speaking, or nothing fresh read (#2562).
    quiet: Option<(u64, u64)>,
    /// With [`NAMES_ENVIRONMENT`] on: the eyes' `tellable` file beside
    /// their known file (`crate::known::tellable_of`), for a watch whose
    /// skill has `known_faces = true` (#2562). None: nobody is named.
    pub tellable: Option<std::path::PathBuf>,
}

/// What a watch skill does, by skill.
pub type Outs = Vec<(String, Out)>;
/// A watch's tool turned: its skill, the model's answer, and what to do.
pub type Turned = (String, Value, Vec<Out>);

impl Watches {
    /// No watch yet, taking arrivals by sound if `sounds`.
    pub fn hearing(sounds: bool) -> Self {
        Self {
            sounds,
            ..Self::default()
        }
    }

    /// Naming, in a visit's text, those the eyes' `tellable` file at
    /// `tellable` says said yes, read as each text goes
    /// ([`NAMES_ENVIRONMENT`], #2562); none names nobody.
    pub fn naming(mut self, tellable: Option<std::path::PathBuf>) -> Self {
        self.tellable = tellable;
        self
    }

    /// The skills loaded again. A watch kept as it was stays as it was; one
    /// whose skill is gone, or whose `[watch]` changed, is off, its mark
    /// given back. A watch is never turned on here.
    pub fn replace(&mut self, skills: &[Skill]) -> Outs {
        let mut made = Vec::new();
        let mut each = BTreeMap::new();
        for skill in skills {
            let (Some(spec), Some(tool), Some(choice)) = (&skill.watch, &skill.tool, &skill.choice)
            else {
                continue;
            };
            let watch = match self.each.remove(&skill.name) {
                Some(old) if old.spec == *spec => old.watch,
                Some(old) => {
                    if old.watch.is_on() {
                        made.push((skill.name.clone(), Out::Unmark));
                        made.push((
                            skill.name.clone(),
                            Out::Said("its [watch] changed: off until asked again".into()),
                        ));
                    }
                    Watch::default()
                }
                None => Watch::default(),
            };
            each.insert(
                skill.name.clone(),
                Entry {
                    spec: spec.clone(),
                    tool: tool.clone(),
                    choice: choice.name.clone(),
                    watch,
                    sounds: skill.permissions.refuses_reading(SOUNDS).is_none(),
                    voice: skill.permissions.refuses_reading(VOICE).is_none(),
                    known_faces: skill.permissions.known_faces,
                },
            );
        }
        for (name, old) in std::mem::take(&mut self.each) {
            if old.watch.is_on() {
                made.push((name.clone(), Out::Unmark));
                made.push((name, Out::Said("its skill is gone: off".into())));
            }
        }
        self.each = each;
        made
    }

    /// The skill whose watch `tool` turns, if any.
    pub fn owner(&self, tool: &str) -> Option<&str> {
        self.each
            .iter()
            .find(|(_, entry)| entry.tool == tool)
            .map(|(name, _)| name.as_str())
    }

    /// Whether any watch is on.
    pub fn any_on(&self) -> bool {
        self.each.values().any(|entry| entry.watch.is_on())
    }

    /// Whether the skill's watch is on.
    pub fn is_on(&self, skill: &str) -> bool {
        self.each
            .get(skill)
            .is_some_and(|entry| entry.watch.is_on())
    }

    /// A call of a watch's `tool` with `args`: none if no watch has it;
    /// else the skill, the model's answer and what to do, or why not.
    pub fn tool(
        &mut self,
        tool: &str,
        args: &Value,
        now_us: u64,
        asker: Option<&str>,
        can_mark: bool,
    ) -> Option<Result<Turned, String>> {
        let heard = self.heard.clone();
        let (name, entry) = self.each.iter_mut().find(|(_, entry)| entry.tool == tool)?;
        let turn = match args {
            Value::Object(given) if given.len() == 1 => {
                given.get(&entry.choice).and_then(Value::as_str)
            }
            _ => None,
        };
        let Some(on) = turn.and_then(|turn| match turn {
            ON => Some(true),
            OFF => Some(false),
            _ => None,
        }) else {
            return Some(Err(format!("{} is \"on\" or \"off\"", entry.choice)));
        };
        if on && self.closed && !entry.watch.is_on() {
            return Some(Err(entry.spec.words.closed.clone()));
        }
        Some(
            entry
                .watch
                .turn(
                    on,
                    &entry.spec,
                    now_us,
                    heard.as_ref().map(|(at, text)| (*at, text.as_str())),
                    asker,
                    can_mark,
                )
                .map(|(answer, outs)| (name.clone(), answer, outs)),
        )
    }

    /// `guard.on` (`on`) or `guard.off` from the owner's app, for guard
    /// mode's watch ([`GUARD`], #2562): its answer and what to do, or why
    /// not. Never on while the privacy switch is closed.
    pub fn from_app(
        &mut self,
        on: bool,
        now_us: u64,
        can_mark: bool,
    ) -> Result<(Value, Vec<Out>), String> {
        let entry = self.each.get_mut(GUARD).ok_or(NOT_INSTALLED)?;
        if on && self.closed && !entry.watch.is_on() {
            return Err(entry.spec.words.closed.clone());
        }
        entry.watch.from_app(on, &entry.spec, now_us, can_mark)
    }

    /// A message on a topic a watch reads, at `now_us`, at local `minute`.
    /// `voice/privacy` is taken too, as a replay has it. The ears' room
    /// sound classes only with [`Self::sounds`], by a watch that may hear
    /// them.
    pub fn observe(
        &mut self,
        topic: &str,
        payload: &[u8],
        now_us: u64,
        minute: Option<u16>,
    ) -> Outs {
        let Ok(value) = serde_json::from_slice::<Value>(payload) else {
            return Vec::new();
        };
        let mut made = Vec::new();
        match topic {
            "voice/heard" => {
                if let Some(text) = value["text"]
                    .as_str()
                    .filter(|text| !text.trim().is_empty())
                {
                    self.heard = Some((now_us, text.to_owned()));
                    // Speech after a door or footsteps (#2562).
                    made = self.sounded(false, true, false, now_us, minute);
                }
            }
            SOUNDS => {
                // Only a window the lamp didn't speak into: its own words
                // are no one coming in. Clear as the ears score it, or
                // unknown there and quiet by the voice's own state over it.
                if let Ok(SoundClasses::Scored {
                    window,
                    scores,
                    own_voice,
                    ..
                }) = serde_json::from_value::<SoundClasses>(value)
                {
                    let by_voice = match own_voice {
                        OwnVoiceEvidence::Clear => Some(false),
                        OwnVoiceEvidence::Unknown => self
                            .quiet
                            .is_some_and(|(since, newest)| {
                                since <= window.start_us
                                    && newest <= now_us
                                    && now_us - newest < VOICE_FRESH_US
                            })
                            .then_some(true),
                        OwnVoiceEvidence::Present => None,
                    };
                    if let Some(by_voice) = by_voice {
                        let door = scores.door.max(scores.footsteps) >= SOUND_AT_LEAST;
                        let speech = scores.speech >= SOUND_AT_LEAST;
                        made = self.sounded(door, speech, by_voice, now_us, minute);
                    }
                }
            }
            VOICE => {
                // A gap longer than a reading lasts may have hidden speech:
                // quiet again only from this reading.
                let quiet = serde_json::from_value::<VoiceState>(value)
                    .is_ok_and(|voice| voice.state != Voice::Speaking);
                self.quiet = match self.quiet {
                    _ if !quiet => None,
                    Some((since, newest))
                        if newest <= now_us && now_us - newest < VOICE_FRESH_US =>
                    {
                        Some((since, now_us))
                    }
                    _ => Some((now_us, now_us)),
                };
            }
            "room/presence" => {
                if let Some(someone) = value["someone"].as_bool() {
                    for (name, entry) in &mut self.each {
                        let tellable = self.tellable.as_deref().filter(|_| entry.known_faces);
                        for out in
                            entry
                                .watch
                                .presence(someone, now_us, minute, &entry.spec, tellable)
                        {
                            made.push((name.clone(), out));
                        }
                    }
                }
            }
            "humans/faces/tracked" => {
                if let Ok(faces) = serde_json::from_value::<Faces>(value) {
                    // The largest, as a skill's look at a face takes it.
                    let largest = faces
                        .faces
                        .iter()
                        .max_by(|a, b| (a.ymax - a.ymin).total_cmp(&(b.ymax - b.ymin)))
                        .map(|face| {
                            (
                                face.xmin + face.xmax - 1.0,
                                face.ymin + face.ymax - 1.0,
                                face.ymax - face.ymin,
                            )
                        });
                    for (name, entry) in &mut self.each {
                        for out in entry.watch.face(largest, now_us, minute) {
                            made.push((name.clone(), out));
                        }
                    }
                }
            }
            PRIVACY => made = self.privacy(value["physical_muted"] == Value::Bool(true)),
            _ => {}
        }
        made
    }

    /// A door or footsteps, or speech, for each watch that may hear them,
    /// with [`Self::sounds`] on (#2562). Taken `by_voice`, the lamp quiet
    /// only by the voice's state, only by a watch that may read [`VOICE`].
    fn sounded(
        &mut self,
        door: bool,
        speech: bool,
        by_voice: bool,
        now_us: u64,
        minute: Option<u16>,
    ) -> Outs {
        let mut made = Vec::new();
        if !self.sounds {
            return made;
        }
        for (skill, entry) in self
            .each
            .iter_mut()
            .filter(|(_, entry)| entry.sounds && (entry.voice || !by_voice))
        {
            for out in entry.watch.sound(door, speech, now_us, minute) {
                made.push((skill.clone(), out));
            }
        }
        made
    }

    /// The eyes know the face at the lamp as `name`, or none there is known.
    pub fn known(&mut self, name: Option<&str>, now_us: u64, minute: Option<u16>) -> Outs {
        let mut made = Vec::new();
        for (skill, entry) in &mut self.each {
            let naming = self.tellable.is_some() && entry.known_faces;
            for out in entry.watch.known(name, now_us, minute, &entry.spec, naming) {
                made.push((skill.clone(), out));
            }
        }
        made
    }

    /// The privacy switch, closed or not: closed, every watch is off.
    pub fn privacy(&mut self, closed: bool) -> Outs {
        self.closed = closed;
        let mut made = Vec::new();
        for (skill, entry) in &mut self.each {
            for out in entry.watch.privacy(closed, &entry.spec) {
                made.push((skill.clone(), out));
            }
        }
        made
    }

    /// The ring says at `now_us` that `skill`'s mark is shown or not, `by`
    /// what.
    pub fn mark_shown(&mut self, skill: &str, shown: bool, by: Option<&str>, now_us: u64) -> Outs {
        self.each.get_mut(skill).map_or_else(Vec::new, |entry| {
            entry
                .watch
                .mark_shown(shown, by, now_us)
                .into_iter()
                .map(|out| (skill.to_owned(), out))
                .collect()
        })
    }

    /// The ring says `skill`'s mark is shown (`light/reached`) or not
    /// (`light/outranked`, `by` what): the person's own light over it
    /// pauses its watch ([`Self::paused_by_person`]); anything else hides
    /// it or shows it ([`Self::mark_shown`]). As the program takes the
    /// ring's word at `now_us`, and a case's recording.
    pub fn ring_said(&mut self, skill: &str, shown: bool, by: Option<&str>, now_us: u64) -> Outs {
        if !shown && by == Some(lamp_msgs::LightOutranked::PERSON) {
            return self.paused_by_person(skill);
        }
        self.mark_shown(skill, shown, by, now_us)
    }

    /// The person's own light is over `skill`'s mark: its watch pauses.
    pub fn paused_by_person(&mut self, skill: &str) -> Outs {
        self.each.get_mut(skill).map_or_else(Vec::new, |entry| {
            entry
                .watch
                .paused_by_person(&entry.spec)
                .into_iter()
                .map(|out| (skill.to_owned(), out))
                .collect()
        })
    }

    /// `skill`'s mark, while its watch is on: what the program asks for
    /// again with a new claim when the ring says it is not shown.
    pub fn mark_of(&self, skill: &str) -> Option<Call> {
        self.each
            .get(skill)
            .filter(|entry| entry.watch.is_on())
            .map(|entry| entry.spec.mark.clone())
    }

    /// Time passes for every watch: one at its `most` ends.
    pub fn tick(&mut self, now_us: u64) -> Outs {
        let mut made = Vec::new();
        for (skill, entry) in &mut self.each {
            for out in entry.watch.tick(now_us, &entry.spec) {
                made.push((skill.clone(), out));
            }
        }
        made
    }
}

/// The local minute of the day at `unix_ms`, `offset_minutes` east of UTC.
pub fn minute_of(unix_ms: u64, offset_minutes: i32) -> u16 {
    let minutes = i128::from(unix_ms).div_euclid(60_000) + i128::from(offset_minutes);
    minutes.rem_euclid(1_440) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: u64 = 1_000_000;
    const MINUTE: u64 = 60 * SECOND;

    /// What the person said as they asked.
    const ASKED: &str = "Lamp, keep an eye on things while I'm out";
    /// The largest face in view, near the desk and far off, as the body's
    /// gaze takes it.
    const NEAR_FACE: Option<(f32, f32, f32)> = Some((0.1, -0.2, 0.2));
    const FAR_FACE: Option<(f32, f32, f32)> = Some((-0.5, 0.0, 0.05));

    /// The reviewer's P2 on #2688: the app's op is let go once its
    /// deadline is reached, saying how long it waited, and when it says no
    /// deadline; inside it, it is taken.
    #[test]
    fn the_apps_op_past_its_deadline_or_with_none_is_let_go() {
        let sent = 100 * SECOND;
        let deadline = sent + 2 * SECOND;
        assert_eq!(let_go(APP_ON, Some(deadline), sent + SECOND), None);
        assert_eq!(let_go(APP_ON, Some(deadline), deadline - 1), None);
        assert_eq!(
            let_go(APP_ON, Some(deadline), sent + 2_400_000).as_deref(),
            Some("guard.on let go: it waited 2400 ms behind another call, past the app's 2 s")
        );
        assert!(let_go(APP_OFF, Some(deadline), deadline).is_some());
        assert!(
            let_go(APP_ON, None, sent)
                .unwrap()
                .contains("says no deadline_us")
        );
    }

    /// Heard at `at`, asking for it.
    fn heard(at: u64) -> Option<(u64, &'static str)> {
        Some((at, ASKED))
    }

    fn spec() -> Spec {
        Spec {
            every_us: 10 * MINUTE,
            asked_within_us: 30 * SECOND,
            if_heard: vec![
                vec!["keep".into(), "an".into(), "eye".into()],
                vec!["guard".into()],
            ],
            most_us: 12 * 60 * MINUTE,
            mark: Call::Light {
                colour: "blue".into(),
                level: Some(1),
                lasting: None,
                pattern: None,
                keep: false,
                until_ends: false,
                no_brighter: false,
            },
            words: guard_words(),
            built_in_words: false,
        }
    }

    /// [`spec`], under a name no test's own `spec` hides.
    fn the_spec() -> Spec {
        spec()
    }

    /// Guard mode's own words, as a lamp loads them.
    fn guard_words() -> Words {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/guard-mode");
        let watch = crate::load(&dir)
            .expect("guard mode loads")
            .watch
            .expect("it has a watch");
        assert!(!watch.built_in_words, "it says its own words");
        watch.words
    }

    /// The second move of "Everything is a skill" (#940) is a move, not a
    /// change: guard mode's words are the ones lamp-skills said before,
    /// byte for byte, and the tests below read its visits as before.
    #[test]
    fn the_watch_words_are_guard_modes_and_what_the_lamp_said_before() {
        assert_eq!(guard_words(), Words::built_in());
        assert_eq!(
            Words::built_in().visit,
            "{who} was at your desk{at}, {long}."
        );
    }

    /// Each word is words, with only its own slots, each once.
    #[test]
    fn a_watchs_words_have_only_their_own_slots() {
        assert!(Words::built_in().checked().is_ok());
        let broken = |change: fn(&mut Words)| {
            let mut words = Words::built_in();
            change(&mut words);
            words.checked().unwrap_err()
        };
        assert!(broken(|w| w.visit = "{who} was at your desk.".into()).contains("visit"));
        assert!(broken(|w| w.visit = "{who}{who} {at} {long}".into()).contains("visit"));
        assert!(broken(|w| w.known = "Someone {who}".into()).contains("known"));
        assert!(broken(|w| w.at = " at {time".into()).contains("brace"));
        assert!(broken(|w| w.minutes = "for {Minutes}".into()).contains("brace"));
        assert!(broken(|w| w.blind = "a\u{7}b".into()).contains("control"));
        assert!(broken(|w| w.stopped = " ".into()).contains("stopped"));
    }

    /// On, asked at 0 by `asker`, heard a second before.
    /// A watch on, its mark shown by the ring.
    fn on(asker: Option<&str>) -> Watch {
        let mut watch = Watch::default();
        watch
            .turn(true, &spec(), SECOND, heard(0), asker, true)
            .unwrap();
        watch.mark_shown(true, None, SECOND);
        watch
    }

    /// Reviewer-two on #1937: from `on` until the ring first says its mark
    /// is shown, nothing about the desk is taken, so a ring that never
    /// answers never has anyone watched without a sign.
    #[test]
    fn nothing_is_taken_until_the_ring_first_shows_its_mark() {
        let spec = spec();
        let mut watch = Watch::default();
        watch
            .turn(true, &spec, SECOND, heard(0), Some("known-a"), true)
            .unwrap();
        assert!(watch.presence(false, MINUTE, None, &spec, None).is_empty());
        assert!(
            watch
                .presence(true, 2 * MINUTE, Some(600), &spec, None)
                .is_empty()
        );
        watch.face(NEAR_FACE, 2 * MINUTE, None);
        assert!(
            watch
                .known(Some("known-a"), 3 * MINUTE, None, &the_spec(), false)
                .is_empty()
        );
        assert!(
            watch
                .presence(false, 4 * MINUTE, None, &spec, None)
                .is_empty()
        );
        assert!(watch.is_on(), "the asker's face ends nothing unseen");
        let shown = watch.mark_shown(true, None, 4 * MINUTE);
        assert!(matches!(&shown[..], [Out::Said(why)] if why.contains("shows")));
        assert!(
            watch.mark_shown(true, None, 4 * MINUTE).is_empty(),
            "said once"
        );
        watch.presence(false, 5 * MINUTE, None, &spec, None);
        watch.presence(true, 10 * MINUTE, Some(14 * 60 + 31), &spec, None);
        watch.presence(true, 12 * MINUTE, None, &spec, None);
        assert_eq!(
            texts(&watch.presence(false, 12 * MINUTE + SECOND, None, &spec, None)),
            ["Someone was at your desk at 14:31, for about 2 minutes."]
        );
    }

    fn texts(outs: &[Out]) -> Vec<&str> {
        outs.iter()
            .filter_map(|out| match out {
                Out::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The person leaves, someone comes at `at` (14:31 local) and stays
    /// `stays`: what the watch does when they go.
    fn visit(watch: &mut Watch, at: u64, stays: u64) -> Vec<Out> {
        let spec = spec();
        let mut outs = watch.presence(false, at - MINUTE, None, &spec, None);
        outs.extend(watch.presence(true, at, Some(14 * 60 + 31), &spec, None));
        // `room/presence` once a second while they are there.
        let mut now = at;
        while now < at + stays {
            now += SECOND;
            outs.extend(watch.presence(true, now, Some(14 * 60 + 31), &spec, None));
        }
        outs.extend(watch.presence(false, now + 20 * SECOND, None, &spec, None));
        outs
    }

    #[test]
    fn it_is_never_on_by_itself() {
        let spec = spec();
        let mut watch = Watch::default();
        assert!(!watch.is_on());
        // Nothing it sees turns it on.
        assert!(watch.presence(false, MINUTE, None, &spec, None).is_empty());
        assert!(
            watch
                .presence(true, 2 * MINUTE, None, &spec, None)
                .is_empty()
        );
        watch.face(NEAR_FACE, 2 * MINUTE, None);
        assert!(
            watch
                .known(Some("known-a"), 2 * MINUTE, None, &the_spec(), false)
                .is_empty()
        );
        assert!(watch.privacy(true, &the_spec()).is_empty());
        assert!(!watch.is_on());
        // Asked with nothing heard at the lamp (a text), or heard too long
        // before, or after talk of something else: refused.
        let not_asked = watch.turn(true, &spec, MINUTE, None, None, true);
        assert_eq!(not_asked.unwrap_err(), guard_words().not_asked);
        let long_ago = watch.turn(true, &spec, MINUTE, heard(MINUTE - 31 * SECOND), None, true);
        assert_eq!(long_ago.unwrap_err(), guard_words().not_asked);
        for other in ["what's the weather", "keep the light on", "an eye test"] {
            let elsewhere = watch.turn(
                true,
                &spec,
                MINUTE,
                Some((MINUTE - SECOND, other)),
                None,
                true,
            );
            assert_eq!(elsewhere.unwrap_err(), guard_words().not_asked, "{other}");
        }
        // Whole words, any case: "Guard the desk" asks; "guardian" does not.
        let guardian = watch.turn(
            true,
            &spec,
            MINUTE,
            Some((MINUTE - SECOND, "my guardian")),
            None,
            true,
        );
        assert_eq!(guardian.unwrap_err(), guard_words().not_asked);
        assert!(!watch.is_on());
        // No mark to show: refused.
        let unmarked = watch.turn(true, &spec, MINUTE, heard(MINUTE - SECOND), None, false);
        assert_eq!(unmarked.unwrap_err(), guard_words().no_mark);
        assert!(!watch.is_on());
        // Asked out loud, the mark shown.
        let (answer, outs) = watch
            .turn(
                true,
                &spec,
                MINUTE,
                Some((MINUTE - 30 * SECOND, "Guard the desk")),
                None,
                true,
            )
            .unwrap();
        assert!(watch.is_on());
        assert_eq!(outs[0], Out::Mark(spec.mark.clone()));
        assert!(
            answer["next"]
                .as_str()
                .unwrap()
                .contains("tell you to stop")
        );
    }

    /// #1685's test: someone arriving and leaving gives one text with the
    /// right time and length.
    #[test]
    fn a_visit_gives_one_text_with_its_time_and_length() {
        let mut watch = on(None);
        let outs = visit(&mut watch, 10 * MINUTE, 2 * MINUTE);
        assert_eq!(
            texts(&outs),
            ["Someone was at your desk at 14:31, for about 2 minutes."]
        );
        let short = visit(&mut on(None), 10 * MINUTE, 10 * SECOND);
        assert_eq!(
            texts(&short),
            ["Someone was at your desk at 14:31, for under a minute."]
        );
        let minute = visit(&mut on(None), 10 * MINUTE, 80 * SECOND);
        assert_eq!(
            texts(&minute),
            ["Someone was at your desk at 14:31, for about a minute."]
        );
        // No local time: no time said.
        let mut watch = on(None);
        let spec = spec();
        watch.presence(false, MINUTE, None, &spec, None);
        watch.presence(true, 2 * MINUTE, None, &spec, None);
        watch.presence(true, 5 * MINUTE, None, &spec, None);
        assert_eq!(
            texts(&watch.presence(false, 5 * MINUTE + SECOND, None, &spec, None)),
            ["Someone was at your desk, for about 3 minutes."]
        );
    }

    /// #1685's test: a second arrival within 10 minutes of a text gives
    /// none; one after them, one.
    #[test]
    fn a_second_visit_within_ten_minutes_is_not_texted() {
        let mut watch = on(None);
        assert_eq!(texts(&visit(&mut watch, 10 * MINUTE, MINUTE)).len(), 1);
        let again = visit(&mut watch, 16 * MINUTE, MINUTE);
        assert!(texts(&again).is_empty(), "{again:?}");
        assert!(matches!(&again[..], [Out::Said(why)] if why.contains("one text each 600 s")));
        let later = visit(&mut watch, 23 * MINUTE, MINUTE);
        assert_eq!(texts(&later).len(), 1, "{later:?}");
    }

    /// #1685's test: the face of the person who asked, back after they left,
    /// ends it, with no text; while they are still at the desk it does not.
    #[test]
    fn the_face_of_the_person_who_asked_ends_it() {
        let spec = spec();
        let mut watch = on(Some("known-a"));
        // Still at the desk, being recognised as they leave.
        assert!(
            watch
                .known(Some("known-a"), 2 * SECOND, None, &the_spec(), false)
                .is_empty()
        );
        assert!(watch.is_on());
        watch.presence(false, MINUTE, None, &spec, None);
        watch.presence(true, 30 * MINUTE, Some(15 * 60), &spec, None);
        let back = watch.known(
            Some("known-a"),
            30 * MINUTE + 2 * SECOND,
            None,
            &the_spec(),
            false,
        );
        assert_eq!(back[0], Out::Unmark);
        assert!(!watch.is_on());
        assert!(texts(&back).is_empty());
        // #2562: "I'm off guard", said aloud as written.
        assert!(
            matches!(&back[1], Out::Say(words) if words.words == guard_words().off_guard && words.exact)
        );
        // Nothing after: they are back.
        assert!(
            watch
                .presence(false, 32 * MINUTE, None, &spec, None)
                .is_empty()
        );
        // Someone else known does not end it.
        let mut watch = on(Some("known-a"));
        watch.presence(false, MINUTE, None, &spec, None);
        watch.presence(true, 30 * MINUTE, Some(15 * 60), &spec, None);
        assert!(
            watch
                .known(Some("known-b"), 30 * MINUTE, None, &the_spec(), false)
                .is_empty()
        );
        assert!(watch.is_on());
        // An asker the eyes did not know is never ended by a face.
        let mut watch = on(None);
        watch.presence(false, MINUTE, None, &spec, None);
        assert!(
            watch
                .known(Some("known-a"), 2 * MINUTE, None, &the_spec(), false)
                .is_empty()
        );
        assert!(watch.is_on());
    }

    /// Known or not, never whose: the text says only which.
    #[test]
    fn a_text_says_known_or_not_and_never_a_name() {
        let spec = spec();
        let mut watch = on(Some("known-a"));
        watch.presence(false, MINUTE, None, &spec, None);
        watch.presence(true, 10 * MINUTE, Some(14 * 60 + 31), &spec, None);
        watch.known(
            Some("known-b"),
            10 * MINUTE + SECOND,
            None,
            &the_spec(),
            false,
        );
        watch.presence(true, 12 * MINUTE, None, &spec, None);
        let outs = watch.presence(false, 12 * MINUTE + SECOND, None, &spec, None);
        assert_eq!(
            texts(&outs),
            ["Someone you've introduced me to was at your desk at 14:31, for about 2 minutes."]
        );
        assert!(!format!("{outs:?}").contains("known-b"));
        let mut watch = on(None);
        watch.presence(false, MINUTE, None, &spec, None);
        watch.presence(true, 10 * MINUTE, Some(9 * 60 + 5), &spec, None);
        watch.face(NEAR_FACE, 10 * MINUTE, None);
        watch.presence(true, 13 * MINUTE, None, &spec, None);
        assert_eq!(
            texts(&watch.presence(false, 13 * MINUTE + SECOND, None, &spec, None)),
            ["Someone I don't know was at your desk at 9:05, for about 3 minutes."]
        );
        // A face far off is not someone at the desk's.
        let mut watch = on(None);
        watch.presence(false, MINUTE, None, &spec, None);
        watch.presence(true, 10 * MINUTE, Some(9 * 60 + 5), &spec, None);
        watch.face(FAR_FACE, 10 * MINUTE, None);
        assert!(
            texts(&watch.presence(false, 13 * MINUTE, None, &spec, None))[0]
                .starts_with("Someone was")
        );
    }

    /// #2562: the privacy switch closed ends it at once, its mark given
    /// back, with one text; the switch staying closed, or opening and
    /// closing again, texts nothing more: it is off until asked again.
    #[test]
    fn the_privacy_switch_closed_ends_it_with_one_text() {
        let mut watch = on(None);
        assert!(watch.privacy(false, &the_spec()).is_empty());
        let muted = watch.privacy(true, &the_spec());
        assert_eq!(muted[0], Out::Unmark);
        assert_eq!(texts(&muted), [guard_words().blind.as_str()]);
        assert!(!watch.is_on());
        assert!(watch.privacy(true, &the_spec()).is_empty());
        assert!(watch.privacy(false, &the_spec()).is_empty());
        assert!(watch.privacy(true, &the_spec()).is_empty());
        // Off, nothing.
        let mut off = Watch::default();
        assert!(off.privacy(true, &the_spec()).is_empty());
    }

    /// The peer read's P2 on #1807: while the ring says its mark is not
    /// shown, nothing about the desk is taken: no visit begins, ends or is
    /// texted, and the asker's face ends nothing; shown again, it counts.
    #[test]
    fn while_its_mark_is_not_shown_nothing_about_the_desk_is_taken() {
        let spec = spec();
        let mut watch = on(Some("known-a"));
        watch.presence(false, MINUTE, None, &spec, None);
        let hidden = watch.mark_shown(false, Some("person"), MINUTE);
        assert!(matches!(&hidden[..], [Out::Said(why)] if why.contains("outranked by person")));
        assert!(watch.mark_shown(false, Some("person"), MINUTE).is_empty());
        assert!(
            watch
                .presence(true, 2 * MINUTE, Some(600), &spec, None)
                .is_empty()
        );
        watch.face(NEAR_FACE, 2 * MINUTE, None);
        assert!(
            watch
                .known(Some("known-a"), 3 * MINUTE, None, &the_spec(), false)
                .is_empty()
        );
        assert!(
            watch
                .presence(false, 4 * MINUTE, None, &spec, None)
                .is_empty()
        );
        assert!(watch.is_on());
        let shown = watch.mark_shown(true, None, 5 * MINUTE);
        assert!(matches!(&shown[..], [Out::Said(why)] if why.contains("shows")));
        watch.presence(true, 10 * MINUTE, Some(14 * 60 + 31), &spec, None);
        watch.presence(true, 12 * MINUTE, None, &spec, None);
        assert_eq!(
            texts(&watch.presence(false, 12 * MINUTE + SECOND, None, &spec, None)),
            ["Someone was at your desk at 14:31, for about 2 minutes."]
        );
        // Off, nothing.
        assert!(Watch::default().mark_shown(false, None, 0).is_empty());
    }

    /// Third's privacy read on #1963: the ring says the mark shows again
    /// after each renewal. With no such word for 30 s (the ring worker
    /// stopped, or it forgot the claim, and no `light/outranked` came),
    /// the mark is taken as hidden, said once: no visit begins, ends or is
    /// texted. The ring's next word counts again.
    #[test]
    fn a_mark_the_ring_has_not_confirmed_for_thirty_seconds_counts_nothing() {
        let spec = spec();
        let mut watch = on(None);
        watch.presence(false, 2 * SECOND, None, &spec, None);
        // Confirmed at 10 s: still counting 30 s after.
        watch.mark_shown(true, None, 10 * SECOND);
        assert!(watch.tick(40 * SECOND, &spec).is_empty());
        // 31 s without a word: hidden, said once.
        assert_eq!(
            watch.tick(41 * SECOND, &spec),
            [Out::Said(UNCONFIRMED.into())]
        );
        assert!(watch.tick(50 * SECOND, &spec).is_empty());
        // A visit now: nothing taken, nothing texted.
        assert!(
            watch
                .presence(true, MINUTE, Some(14 * 60 + 21), &spec, None)
                .is_empty()
        );
        watch.face(NEAR_FACE, MINUTE, None);
        assert!(
            watch
                .presence(false, 3 * MINUTE, None, &spec, None)
                .is_empty()
        );
        assert!(watch.is_on());
        // The ring's word again: the next visit counts.
        let shown = watch.mark_shown(true, None, 4 * MINUTE);
        assert!(matches!(&shown[..], [Out::Said(why)] if why.contains("shows")));
        assert!(watch.tick(4 * MINUTE + SECOND, &spec).is_empty());
        watch.presence(
            true,
            4 * MINUTE + 5 * SECOND,
            Some(14 * 60 + 31),
            &spec,
            None,
        );
        assert_eq!(
            texts(&watch.presence(false, 4 * MINUTE + 20 * SECOND, None, &spec, None)),
            ["Someone was at your desk at 14:31, for under a minute."]
        );
    }

    /// The first builder's ruling on #1937: the person's own light over its
    /// mark pauses it, with one text, and nothing is taken; it is never
    /// asked for over them, and goes on only when they ask again in words.
    #[test]
    fn the_persons_light_pauses_it_until_they_ask_again() {
        let spec = spec();
        let mut watch = on(None);
        watch.presence(false, MINUTE, None, &spec, None);
        let paused = watch.paused_by_person(&the_spec());
        assert_eq!(texts(&paused), [guard_words().paused.as_str()]);
        assert!(watch.paused_by_person(&the_spec()).is_empty());
        // The ring saying the old mark shows does not end a pause.
        assert!(watch.mark_shown(true, None, MINUTE).is_empty());
        assert!(
            watch
                .presence(true, 2 * MINUTE, Some(600), &spec, None)
                .is_empty()
        );
        assert!(
            watch
                .presence(false, 4 * MINUTE, None, &spec, None)
                .is_empty()
        );
        // Asked with no words for it: still paused.
        let quiet = watch.turn(
            true,
            &spec,
            5 * MINUTE,
            Some((5 * MINUTE - SECOND, "hello")),
            None,
            true,
        );
        assert_eq!(quiet.unwrap_err(), guard_words().not_asked);
        // Asked again in words: the mark asked for anew, counted once shown.
        let (answer, outs) = watch
            .turn(
                true,
                &spec,
                6 * MINUTE,
                heard(6 * MINUTE - SECOND),
                None,
                true,
            )
            .unwrap();
        assert_eq!(answer["next"], guard_words().on_again);
        assert_eq!(outs[0], Out::Mark(spec.mark.clone()));
        assert!(
            watch
                .presence(true, 7 * MINUTE, None, &spec, None)
                .is_empty()
        );
        watch.mark_shown(true, None, 7 * MINUTE);
        watch.presence(true, 10 * MINUTE, Some(14 * 60 + 31), &spec, None);
        watch.presence(true, 12 * MINUTE, None, &spec, None);
        assert_eq!(
            texts(&watch.presence(false, 12 * MINUTE + SECOND, None, &spec, None)),
            ["Someone was at your desk at 14:31, for about 2 minutes."]
        );
        // Not paused, asked again: already on.
        let (answer, outs) = watch
            .turn(
                true,
                &spec,
                13 * MINUTE,
                heard(13 * MINUTE - SECOND),
                None,
                true,
            )
            .unwrap();
        assert_eq!(answer["next"], guard_words().already_on);
        assert!(outs.is_empty());
    }

    /// The first builder's ruling on #1937: paused, it stays paused until
    /// asked again or its `most`, which ends it as it ends any watch.
    #[test]
    fn a_pause_lasts_until_asked_or_its_most() {
        let spec = spec();
        let mut watch = on(None);
        watch.paused_by_person(&the_spec());
        assert!(watch.tick(11 * 60 * MINUTE, &spec).is_empty());
        assert!(watch.is_on());
        let ended = watch.tick(12 * 60 * MINUTE + SECOND, &spec);
        assert_eq!(
            texts(&ended),
            [guard_words().ended(12 * 60 * MINUTE).as_str()]
        );
        assert!(!watch.is_on());
    }

    /// The peer read's P3 on #1807: it ends by itself at its `most` after it
    /// was asked, its mark given back, with one text that it has stopped.
    #[test]
    fn it_ends_by_itself_at_its_most() {
        let spec = spec();
        let mut watch = on(None);
        // The ring's word after the mark's last renewal before it.
        watch.mark_shown(true, None, 12 * 60 * MINUTE - SECOND);
        assert!(watch.tick(12 * 60 * MINUTE, &spec).is_empty());
        let ended = watch.tick(12 * 60 * MINUTE + SECOND, &spec);
        assert_eq!(ended[0], Out::Unmark);
        assert_eq!(
            texts(&ended),
            [guard_words().ended(12 * 60 * MINUTE).as_str()]
        );
        assert!(!watch.is_on());
        assert!(watch.tick(13 * 60 * MINUTE, &spec).is_empty());
    }

    /// reviewer-two's P3 on #1807, and #2562: once the switch has ended
    /// it, whoever says someone is at the desk, no visit begins, none ends
    /// and none is texted; and while the switch is closed it is not turned
    /// on, by voice or from the app.
    #[test]
    fn once_the_switch_has_ended_it_nothing_about_the_desk_is_taken() {
        let spec = spec();
        let mut watch = on(Some("known-a"));
        watch.presence(false, MINUTE, None, &spec, None);
        assert_eq!(
            texts(&watch.privacy(true, &the_spec())),
            [guard_words().blind.as_str()]
        );
        assert!(
            watch
                .presence(true, 2 * MINUTE, Some(600), &spec, None)
                .is_empty()
        );
        assert!(watch.face(NEAR_FACE, 2 * MINUTE, None).is_empty());
        assert!(
            watch
                .known(Some("known-a"), 3 * MINUTE, None, &the_spec(), false)
                .is_empty()
        );
        assert!(
            watch
                .presence(false, 4 * MINUTE, None, &spec, None)
                .is_empty()
        );
        assert!(!watch.is_on());
        // Closed, it is not turned on.
        let mut watches = guard();
        watches.privacy(true);
        watches.observe("voice/heard", HEARD, SECOND, None);
        let refused = watches.tool("keep_watch", &json!({"turn": "on"}), 2 * SECOND, None, true);
        assert_eq!(refused.unwrap().unwrap_err(), guard_words().closed);
        assert_eq!(
            watches.from_app(true, 2 * SECOND, true).unwrap_err(),
            guard_words().closed
        );
        assert!(!watches.any_on());
        // Open, asked again: on.
        watches.privacy(false);
        assert!(
            watches
                .tool("keep_watch", &json!({"turn": "on"}), 3 * SECOND, None, true)
                .unwrap()
                .is_ok()
        );
    }

    /// Off when asked, from anywhere: no voice needed to stop it.
    #[test]
    fn off_when_asked_gives_the_mark_back() {
        let spec = spec();
        let mut watch = on(None);
        let (answer, outs) = watch
            .turn(false, &spec, 5 * MINUTE, None, None, false)
            .unwrap();
        assert_eq!(outs[0], Out::Unmark);
        assert_eq!(answer["next"], guard_words().off);
        assert!(!watch.is_on());
        let (answer, outs) = watch
            .turn(false, &spec, 6 * MINUTE, None, None, false)
            .unwrap();
        assert!(outs.is_empty());
        assert_eq!(answer["next"], guard_words().was_off);
        // On twice is on once.
        let mut watch = on(None);
        let (answer, outs) = watch
            .turn(true, &spec, 2 * SECOND, heard(SECOND), None, true)
            .unwrap();
        assert!(outs.is_empty());
        assert_eq!(answer["next"], guard_words().already_on);
    }

    /// The repository's guard mode, as the program holds it: a reload
    /// never turns a watch on; the same skill again leaves it as it was; a
    /// changed `[watch]` or a removed skill turns it off and gives its mark
    /// back; and its tool takes only on or off.
    #[test]
    fn a_reload_never_turns_a_watch_on_and_a_changed_or_removed_one_is_off() {
        let skill = crate::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/guard-mode"),
        )
        .unwrap();
        let heard = br#"{"text":"keep an eye on things while I'm out","finished":true}"#;
        let on = json!({"turn": "on"});
        let mut watches = Watches::default();
        assert!(watches.replace(std::slice::from_ref(&skill)).is_empty());
        assert!(!watches.any_on());
        assert_eq!(watches.owner("keep_watch"), Some("guard-mode"));
        assert!(watches.tool("dance", &on, SECOND, None, true).is_none());
        for wrong in [
            json!({"turn": "maybe"}),
            json!({}),
            Value::Null,
            json!({"on": true}),
        ] {
            let refused = watches.tool("keep_watch", &wrong, SECOND, None, true);
            assert!(matches!(refused, Some(Err(_))), "{wrong}");
        }
        watches.observe("voice/heard", heard, SECOND, None);
        let (name, _, outs) = watches
            .tool("keep_watch", &on, 2 * SECOND, None, true)
            .unwrap()
            .unwrap();
        assert_eq!(name, "guard-mode");
        assert!(matches!(outs[0], Out::Mark(_)));
        assert!(watches.is_on("guard-mode"));
        // Loaded again as it was: still on, nothing done.
        assert!(watches.replace(std::slice::from_ref(&skill)).is_empty());
        assert!(watches.is_on("guard-mode"));
        // Its [watch] changed: off, the mark given back.
        let mut changed = skill.clone();
        changed.watch.as_mut().unwrap().every_us = 20 * MINUTE;
        let outs = watches.replace(std::slice::from_ref(&changed));
        assert_eq!(outs[0], ("guard-mode".to_owned(), Out::Unmark));
        assert!(!watches.is_on("guard-mode"));
        // On again, then the skill removed: off, and its tool no one's.
        watches.observe("voice/heard", heard, 3 * SECOND, None);
        watches
            .tool("keep_watch", &on, 4 * SECOND, None, true)
            .unwrap()
            .unwrap();
        let outs = watches.replace(&[]);
        assert_eq!(outs[0], ("guard-mode".to_owned(), Out::Unmark));
        assert!(!watches.any_on());
        assert!(watches.owner("keep_watch").is_none());
    }

    /// What was heard as the person asked.
    const HEARD: &[u8] = br#"{"text":"keep an eye on things while I'm out","finished":true}"#;

    /// The repository's guard mode, loaded, taking arrivals by sound.
    fn guard() -> Watches {
        let skill = crate::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/guard-mode"),
        )
        .unwrap();
        let mut watches = Watches::hearing(true);
        watches.replace(std::slice::from_ref(&skill));
        watches
    }

    /// The ears' room sound classes completed at `at`, for the 0.96 s
    /// window before it: a door scored `door`, speech `speech`, the lamp's
    /// own voice `own`. On a lamp it is `present` or `unknown`, never
    /// `clear` (lamp-ears since #2714).
    fn classes(door: f32, speech: f32, own: &str, at: u64) -> Vec<u8> {
        json!({
            "status": "scored",
            "model_id": "yamnet-qai-v0.59.0",
            "window": {"start_us": at - 1_000_000, "end_us": at - 40_000, "completed_us": at, "capture_epoch": 1},
            "scores": {"speech": speech, "music": 0.0, "laughter": 0.0, "door": door, "footsteps": 0.0},
            "own_voice": own,
        })
        .to_string()
        .into_bytes()
    }

    /// One face near, as the eyes track it.
    const FACE: &[u8] =
        br#"{"faces":[{"id":"f1","xmin":0.4,"ymin":0.3,"xmax":0.6,"ymax":0.6,"confidence":0.9}]}"#;

    /// The voice's state `state` once a second from `from` to `to`.
    fn voice(watches: &mut Watches, state: &str, from: u64, to: u64) {
        let state = json!({ "state": state }).to_string();
        for at in (from..=to).step_by(SECOND as usize) {
            watches.observe(VOICE, state.as_bytes(), at, None);
        }
    }

    /// The watch on, the desk empty and its mark confirmed at `now`.
    fn away(watch: &mut Watch, now: u64) {
        watch.presence(false, now, None, &spec(), None);
        watch.mark_shown(true, None, now);
    }

    /// #2562: a door or footsteps with no face or speech within 30 s is
    /// nobody coming in: nothing is texted, and a face after that is no
    /// arrival of the sound's.
    #[test]
    fn a_door_with_no_face_within_thirty_seconds_texts_nothing() {
        let spec = spec();
        let mut watch = on(None);
        away(&mut watch, 10 * MINUTE);
        let heard = watch.sound(true, false, 10 * MINUTE, Some(14 * 60 + 31));
        assert!(matches!(&heard[..], [Out::Said(why)] if why.contains("only if a face or speech")));
        // A second door while one waits says nothing new.
        assert!(
            watch
                .sound(true, false, 10 * MINUTE + SECOND, None)
                .is_empty()
        );
        watch.mark_shown(true, None, 10 * MINUTE + 20 * SECOND);
        let gone = watch.tick(10 * MINUTE + 31 * SECOND, &spec);
        assert!(matches!(&gone[..], [Out::Said(why)] if why.contains("nobody came in")));
        assert!(texts(&gone).is_empty());
        // A face far off after: no visit, no arrival, no turn.
        let late = watch.face(FAR_FACE, 10 * MINUTE + 40 * SECOND, None);
        assert!(late.is_empty(), "{late:?}");
        assert!(
            watch
                .presence(false, 11 * MINUTE, None, &spec, None)
                .is_empty()
        );
        assert!(watch.is_on());
    }

    /// #2562: a door, then a face within 30 s, is one arrival: the visit
    /// begins at the door, the head turns to the face once, and one text
    /// says they came in, when they go.
    #[test]
    fn a_door_then_a_face_gives_one_arrival() {
        let spec = spec();
        let mut watch = on(None);
        away(&mut watch, 10 * MINUTE);
        watch.sound(true, false, 10 * MINUTE, Some(14 * 60 + 31));
        let seen = watch.face(NEAR_FACE, 10 * MINUTE + 12 * SECOND, Some(14 * 60 + 31));
        assert!(matches!(&seen[0], Out::Said(why) if why.contains("someone came in")));
        assert_eq!(
            seen[1],
            Out::Gaze(Call::Gaze {
                face: Some((0.1, -0.2, 0.2))
            })
        );
        let mut outs = Vec::new();
        let mut now = 10 * MINUTE + 12 * SECOND;
        while now < 12 * MINUTE {
            now += SECOND;
            outs.extend(watch.presence(true, now, None, &spec, None));
            outs.extend(watch.face(NEAR_FACE, now, None));
        }
        outs.extend(watch.presence(false, now + 20 * SECOND, None, &spec, None));
        assert!(
            !outs.iter().any(|out| matches!(out, Out::Gaze(_))),
            "{outs:?}"
        );
        assert_eq!(
            texts(&outs),
            ["Someone I don't know came in at 14:31 and was here for about 2 minutes."]
        );
        // Speech after footsteps is one too, with no face to turn to.
        let mut watch = on(None);
        away(&mut watch, 10 * MINUTE);
        watch.sound(true, false, 10 * MINUTE, Some(9 * 60 + 5));
        let spoke = watch.sound(false, true, 10 * MINUTE + 5 * SECOND, None);
        assert!(matches!(&spoke[..], [Out::Said(why)] if why.contains("then speech")));
        watch.presence(true, 10 * MINUTE + 6 * SECOND, None, &spec, None);
        assert_eq!(
            texts(&watch.presence(false, 10 * MINUTE + 50 * SECOND, None, &spec, None)),
            ["Someone came in at 9:05 and was here for under a minute."]
        );
    }

    /// #2562 and rule 8: one turn of the head a visit, and none again
    /// within 2 min of the last, whoever comes; after, one again.
    #[test]
    fn a_second_arrival_within_two_minutes_does_not_turn_the_head() {
        let spec = spec();
        let mut watch = on(None);
        away(&mut watch, MINUTE);
        let turns = |outs: &[Out]| {
            outs.iter()
                .filter(|out| matches!(out, Out::Gaze(_)))
                .count()
        };
        let arrive = |watch: &mut Watch, at: u64| {
            let mut outs = watch.presence(true, at, None, &spec, None);
            outs.extend(watch.face(NEAR_FACE, at, None));
            outs.extend(watch.face(NEAR_FACE, at + SECOND, None));
            outs.extend(watch.presence(false, at + 30 * SECOND, None, &spec, None));
            outs
        };
        assert_eq!(turns(&arrive(&mut watch, 10 * MINUTE)), 1);
        let again = arrive(&mut watch, 11 * MINUTE);
        assert_eq!(turns(&again), 0);
        assert!(
            again
                .iter()
                .any(|out| matches!(out, Out::Said(why) if why.contains("under 2 min")))
        );
        assert_eq!(turns(&arrive(&mut watch, 13 * MINUTE)), 1);
        // The asker still at the desk turns nothing: it is not counting yet.
        let mut watch = on(None);
        assert!(watch.face(NEAR_FACE, 2 * SECOND, None).is_empty());
    }

    /// #2562: the owner's app turns it on with no voice, and off; its mark
    /// is asked for and nothing is counted until the ring confirms it. A
    /// watch paused by the person's light, no mark to show, or no guard
    /// mode installed: refused.
    #[test]
    fn guard_from_the_app_is_on_without_a_voice_and_counts_only_once_its_mark_shows() {
        let mut watches = guard();
        let (answer, outs) = watches.from_app(true, SECOND, true).unwrap();
        assert_eq!(answer, json!({"guard": "on"}));
        assert!(matches!(outs[0], Out::Mark(_)));
        assert!(watches.is_on(GUARD));
        // Before the ring's word: nothing taken.
        let empty = br#"{"someone":false}"#;
        let someone = br#"{"someone":true}"#;
        watches.observe("room/presence", empty, 2 * SECOND, None);
        watches.observe("room/presence", someone, 3 * SECOND, None);
        assert!(
            watches
                .observe("room/presence", empty, 4 * SECOND, None)
                .is_empty()
        );
        watches.ring_said(GUARD, true, None, 5 * SECOND);
        watches.observe("room/presence", empty, 6 * SECOND, None);
        watches.observe("room/presence", someone, 7 * SECOND, Some(600));
        let visit = watches.observe("room/presence", empty, 20 * SECOND, None);
        assert!(visit.iter().any(|(_, out)| matches!(out, Out::Text(_))));
        // On again: on. Off: its mark given back.
        assert!(
            watches
                .from_app(true, 21 * SECOND, true)
                .unwrap()
                .1
                .is_empty()
        );
        let (answer, outs) = watches.from_app(false, 22 * SECOND, false).unwrap();
        assert_eq!(answer, json!({"guard": "off"}));
        assert_eq!(outs[0], Out::Unmark);
        assert!(!watches.any_on());
        // No mark to show: refused.
        assert_eq!(
            watches.from_app(true, MINUTE, false).unwrap_err(),
            guard_words().no_mark
        );
        // Paused by the person's own light: only asked at the lamp.
        watches.from_app(true, MINUTE, true).unwrap();
        watches.paused_by_person(GUARD);
        assert_eq!(
            watches.from_app(true, 2 * MINUTE, true).unwrap_err(),
            guard_words().paused_from_app
        );
        // Asked from the app, no face is the asker's: a known face ends
        // nothing.
        let mut watch = Watch::default();
        watch.from_app(true, &spec(), SECOND, true).unwrap();
        away(&mut watch, MINUTE);
        assert!(
            watch
                .known(Some("known-a"), 2 * MINUTE, None, &the_spec(), false)
                .is_empty()
        );
        assert!(watch.is_on());
        // No guard mode: refused.
        assert_eq!(
            Watches::default().from_app(true, SECOND, true).unwrap_err(),
            NOT_INSTALLED
        );
    }

    /// #2562: only lamp-cloud's unit is the owner's app's link: not the
    /// voice, another part, or a unit whose name only begins the same.
    #[test]
    fn only_lamp_clouds_unit_is_the_app() {
        assert!(from_the_app(
            "0::/lampos.slice/lampos-parts.slice/lamp-cloud.service\n"
        ));
        for other in [
            "0::/lampos.slice/lampos-voice.slice/lamp-voice.service\n",
            "0::/lampos.slice/lampos-parts.slice/lamp-skills.service\n",
            "0::/lampos.slice/lampos-parts.slice/lamp-cloud.service-x\n",
            "",
        ] {
            assert!(!from_the_app(other), "{other}");
        }
        assert!(switched_on(Some("on")));
        for off in [None, Some("1"), Some("ON"), Some("")] {
            assert!(!switched_on(off));
        }
    }

    /// #2562: a door on the ears' classes is taken only with the switch on
    /// and with the lamp's own voice clear, or unknown with the voice's own
    /// state saying it wasn't speaking over the window; speech on
    /// `voice/heard` or the classes follows it.
    #[test]
    fn sounds_are_taken_only_with_the_switch_and_the_lamps_voice_clear() {
        let empty = br#"{"someone":false}"#;
        // The voice's state from 5 s to 10 s, a door scored at 10 s.
        for (sounds, own, state, came) in [
            // What lamp-ears sends: unknown, the voice resting over it.
            (true, "unknown", Some("resting"), true),
            (false, "unknown", Some("resting"), false),
            // Unknown while the voice was speaking, or with no state read.
            (true, "unknown", Some("speaking"), false),
            (true, "unknown", None, false),
            // The lamp spoke into it: never, whatever the state says.
            (true, "present", Some("resting"), false),
            // Clear, from an older or a later producer: as it was.
            (true, "clear", None, true),
            (false, "clear", None, false),
        ] {
            let mut watches = guard();
            watches.sounds = sounds;
            watches.observe("voice/heard", HEARD, SECOND, None);
            watches.tool("keep_watch", &json!({"turn": "on"}), 2 * SECOND, None, true);
            watches.ring_said(GUARD, true, None, 3 * SECOND);
            watches.observe("room/presence", empty, 4 * SECOND, None);
            if let Some(state) = state {
                voice(&mut watches, state, 5 * SECOND, 10 * SECOND);
            }
            let door = watches.observe(
                SOUNDS,
                &classes(0.9, 0.0, own, 10 * SECOND),
                10 * SECOND,
                None,
            );
            let seen = watches.observe(Faces::TOPIC, FACE, 15 * SECOND, None);
            let said = format!("{door:?}{seen:?}");
            assert_eq!(
                said.contains("someone came in"),
                came,
                "{sounds} {own} {state:?}: {said}"
            );
        }
        // Speaking within the window, resting before and after it: nothing.
        let mut watches = guard();
        watches.observe("voice/heard", HEARD, SECOND, None);
        watches.tool("keep_watch", &json!({"turn": "on"}), 2 * SECOND, None, true);
        watches.ring_said(GUARD, true, None, 3 * SECOND);
        watches.observe("room/presence", empty, 4 * SECOND, None);
        voice(&mut watches, "resting", 5 * SECOND, 9 * SECOND);
        voice(
            &mut watches,
            "speaking",
            9 * SECOND + 500_000,
            9 * SECOND + 500_000,
        );
        voice(
            &mut watches,
            "resting",
            9 * SECOND + 700_000,
            9 * SECOND + 700_000,
        );
        let door = watches.observe(
            SOUNDS,
            &classes(0.9, 0.0, "unknown", 10 * SECOND),
            10 * SECOND,
            None,
        );
        let seen = watches.observe(Faces::TOPIC, FACE, 15 * SECOND, None);
        assert!(!format!("{door:?}{seen:?}").contains("someone came in"));
        // A state gone stale before the window says nothing of it.
        let mut watches = guard();
        watches.observe("voice/heard", HEARD, SECOND, None);
        watches.tool("keep_watch", &json!({"turn": "on"}), 2 * SECOND, None, true);
        watches.ring_said(GUARD, true, None, 3 * SECOND);
        watches.observe("room/presence", empty, 4 * SECOND, None);
        voice(&mut watches, "resting", 4 * SECOND, 5 * SECOND);
        let door = watches.observe(
            SOUNDS,
            &classes(0.9, 0.0, "unknown", 10 * SECOND),
            10 * SECOND,
            None,
        );
        let seen = watches.observe(Faces::TOPIC, FACE, 15 * SECOND, None);
        assert!(!format!("{door:?}{seen:?}").contains("someone came in"));
        // Speech on the classes follows a door too.
        let mut watches = guard();
        watches.observe("voice/heard", HEARD, SECOND, None);
        watches.tool("keep_watch", &json!({"turn": "on"}), 2 * SECOND, None, true);
        watches.ring_said(GUARD, true, None, 3 * SECOND);
        watches.observe("room/presence", empty, 4 * SECOND, None);
        voice(&mut watches, "resting", 5 * SECOND, 10 * SECOND);
        watches.observe(
            SOUNDS,
            &classes(0.9, 0.0, "unknown", 10 * SECOND),
            10 * SECOND,
            None,
        );
        voice(&mut watches, "resting", 11 * SECOND, 12 * SECOND);
        let spoke = watches.observe(
            SOUNDS,
            &classes(0.0, 0.8, "unknown", 12 * SECOND),
            12 * SECOND,
            None,
        );
        assert!(format!("{spoke:?}").contains("then speech"));
    }

    #[test]
    fn the_local_minute_is_the_wall_clock_east_of_utc() {
        // 2026-10-19 07:31 UTC is 14:31 in Ho Chi Minh City (+07:00).
        let unix_ms = 1_792_395_060_000;
        assert_eq!(minute_of(unix_ms, 420), 14 * 60 + 31);
        assert_eq!(minute_of(unix_ms, 0), 7 * 60 + 31);
        assert_eq!(minute_of(unix_ms, -480), 23 * 60 + 31);
    }

    #[test]
    fn the_topics_are_the_messages_own() {
        assert_eq!(LISTENS[0], lamp_msgs::RoomPresence::TOPIC);
        assert_eq!(LISTENS[1], Faces::TOPIC);
        assert_eq!(LISTENS[2], lamp_msgs::Speech::TOPICS[0]);
        assert_eq!(PRIVACY, lamp_msgs::MicrophonePrivacy::TOPIC);
        assert_eq!(SOUNDS, SoundClasses::TOPIC);
        assert_eq!(VOICE, VoiceState::TOPIC);
    }

    /// What went to the phone: each text, and whether it names someone
    /// (#2562).
    fn sent(outs: &[Out]) -> Vec<(&str, bool)> {
        outs.iter()
            .filter_map(|out| match out {
                Out::Text(text) => Some((text.as_str(), false)),
                Out::Named { text, .. } => Some((text.as_str(), true)),
                _ => None,
            })
            .collect()
    }

    /// `room/presence`'s payload.
    fn presence_of(someone: bool) -> Vec<u8> {
        json!({ "someone": someone }).to_string().into_bytes()
    }

    /// Guard mode on from the app, its mark shown, the desk empty at 1 min;
    /// a door at 18:10 (10 min), then the eyes' known names `seen` in turn,
    /// a second apart, at the desk until 12 min; `before_text` just before
    /// they go: everything it did.
    fn came_in(
        watches: &mut Watches,
        seen: &[Option<&str>],
        before_text: impl FnOnce(&mut Watches) -> Outs,
    ) -> Vec<Out> {
        let mut outs: Outs = Vec::new();
        outs.extend(
            watches
                .from_app(true, SECOND, true)
                .unwrap()
                .1
                .into_iter()
                .map(|out| (GUARD.to_owned(), out)),
        );
        outs.extend(watches.ring_said(GUARD, true, None, SECOND));
        outs.extend(watches.observe("room/presence", &presence_of(false), MINUTE, None));
        voice(watches, "resting", 10 * MINUTE - 5 * SECOND, 10 * MINUTE);
        outs.extend(watches.observe(
            SOUNDS,
            &classes(0.9, 0.0, "unknown", 10 * MINUTE),
            10 * MINUTE,
            Some(18 * 60 + 10),
        ));
        let mut now = 10 * MINUTE;
        for name in seen {
            now += SECOND;
            outs.extend(watches.known(*name, now, None));
        }
        outs.extend(watches.observe("room/presence", &presence_of(true), 12 * MINUTE, None));
        outs.extend(before_text(watches));
        outs.extend(watches.observe(
            "room/presence",
            &presence_of(false),
            12 * MINUTE + SECOND,
            None,
        ));
        outs.into_iter().map(|(_, out)| out).collect()
    }

    /// Guard mode with `LAMP_GUARD_NAMES=on`: the eyes' `tellable` beside
    /// their known file in `dir`, holding `yes`.
    fn naming(dir: &std::path::Path, yes: &str) -> Watches {
        let tellable = crate::known::tellable_of(&dir.join("known"));
        std::fs::write(&tellable, yes).unwrap();
        let mut watches = guard();
        watches.tellable = Some(tellable);
        watches
    }

    /// #2562 item 1, the first builder's test: two known people arriving
    /// together, one who said yes to being named and one who didn't. The
    /// text names the one and says "someone the lamp knows" for the other;
    /// both who said yes are both named.
    #[test]
    fn two_known_one_tellable_names_one_and_says_someone_the_lamp_knows_for_the_other() {
        let dir = tempfile::tempdir().unwrap();
        let mut watches = naming(dir.path(), "Minh\n");
        let outs = came_in(&mut watches, &[Some("Minh"), Some("Lan")], |_| Vec::new());
        assert_eq!(
            sent(&outs),
            [(
                "Minh and someone the lamp knows came in at 18:10 and were here for about 2 minutes.",
                true
            )]
        );
        // Beside it, for lamp-cloud's check as it sends: the line with
        // names off, and who it names, by id, Minh alone.
        let message = outs
            .iter()
            .find(|out| matches!(out, Out::Named { .. }))
            .and_then(|out| text_message(GUARD, out.clone()))
            .unwrap()
            .unwrap();
        assert_eq!(
            message.unnamed.as_deref(),
            Some(
                "Someone you've introduced me to came in at 18:10 and was here for about 2 minutes."
            )
        );
        assert_eq!(message.named, [lamp_msgs::known::person_id("Minh")]);
        assert_eq!(message.from, "skill:guard-mode");
        // The name is in the text alone, never in what the journal says.
        assert!(outs.iter().all(
            |out| !matches!(out, Out::Said(why) if why.contains("Minh") || why.contains("Lan"))
        ));
        // Lan first, or Minh alone: the same, or just the name.
        let mut watches = naming(dir.path(), "Minh\n");
        let outs = came_in(
            &mut watches,
            &[Some("Lan"), Some("Minh"), Some("Lan")],
            |_| Vec::new(),
        );
        assert_eq!(
            sent(&outs)[0].0,
            "Minh and someone the lamp knows came in at 18:10 and were here for about 2 minutes."
        );
        let mut watches = naming(dir.path(), "Minh\n");
        let outs = came_in(&mut watches, &[Some("Minh")], |_| Vec::new());
        assert_eq!(
            sent(&outs)[0].0,
            "Minh came in at 18:10 and was here for about 2 minutes."
        );
        // Both said yes.
        let mut watches = naming(dir.path(), "Lan\nMinh\n");
        let outs = came_in(&mut watches, &[Some("Minh"), Some("Lan")], |_| Vec::new());
        assert_eq!(
            sent(&outs)[0].0,
            "Minh and Lan came in at 18:10 and were here for about 2 minutes."
        );
        // Neither did: as without names.
        let mut watches = naming(dir.path(), "Thu\n");
        let outs = came_in(&mut watches, &[Some("Minh"), Some("Lan")], |_| Vec::new());
        assert_eq!(
            sent(&outs),
            [(
                "Someone you've introduced me to came in at 18:10 and was here for about 2 minutes.",
                false
            )]
        );
    }

    /// #2562, seen on lamp-06c1 on 22 Oct with names on: "Bramble and
    /// someone the lamp knows was at your desk at 19:34". More than one
    /// person in `{who}` says "were"; one, or no name, says "was" as before.
    #[test]
    fn more_than_one_person_at_the_desk_were_one_was() {
        let words = Words::built_in();
        let visit = |names: &[&str]| Visit {
            began_us: 0,
            began_minute: Some(19 * 60 + 34),
            seen_us: 2 * MINUTE,
            face: true,
            known: true,
            names: names.iter().map(|name| (*name).to_owned()).collect(),
            came_in: false,
            turned: false,
        };
        let yes = |names: &[&str]| {
            names
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            visit(&["Bramble", "Lan"]).text(&words, &yes(&["Bramble"])),
            "Bramble and someone the lamp knows were at your desk at 19:34, for about 2 minutes."
        );
        assert_eq!(
            visit(&["Bramble"]).text(&words, &yes(&["Bramble"])),
            "Bramble was at your desk at 19:34, for about 2 minutes."
        );
        assert_eq!(
            visit(&["Bramble", "Lan"]).text(&words, &yes(&["Bramble", "Lan"])),
            "Bramble and Lan were at your desk at 19:34, for about 2 minutes."
        );
        // Nobody named: the words with names off, two known or one.
        for names in [&["Bramble", "Lan"][..], &["Bramble"], &[]] {
            assert_eq!(
                visit(names).text(&words, &[]),
                "Someone you've introduced me to was at your desk at 19:34, for about 2 minutes."
            );
        }
        let mut stranger = visit(&[]);
        stranger.known = false;
        assert_eq!(
            stranger.text(&words, &[]),
            "Someone I don't know was at your desk at 19:34, for about 2 minutes."
        );
        stranger.face = false;
        stranger.came_in = true;
        assert_eq!(
            stranger.text(&words, &[]),
            "Someone came in at 19:34 and was here for about 2 minutes."
        );
        // The door's line for two: "were" there too.
        let mut two = visit(&["Bramble", "Lan"]);
        two.came_in = true;
        assert_eq!(
            two.text(&words, &yes(&["Bramble", "Lan"])),
            "Bramble and Lan came in at 19:34 and were here for about 2 minutes."
        );
    }

    /// #2562: with `LAMP_GUARD_NAMES` off (no `tellable` given), nobody is
    /// named, whoever said yes: today's words, and no name kept or said.
    #[test]
    fn with_names_off_nobody_is_ever_named() {
        let dir = tempfile::tempdir().unwrap();
        let mut watches = naming(dir.path(), "Minh\nLan\n");
        watches.tellable = None;
        let outs = came_in(&mut watches, &[Some("Minh"), Some("Lan")], |_| Vec::new());
        assert_eq!(
            sent(&outs),
            [(
                "Someone you've introduced me to came in at 18:10 and was here for about 2 minutes.",
                false
            )]
        );
        assert!(!format!("{outs:?}").contains("Minh"), "{outs:?}");
        assert!(!format!("{watches:?}").contains("Minh"), "no name kept");
        assert_eq!(NAMES_ENVIRONMENT, "LAMP_GUARD_NAMES");
    }

    /// #2562: a skill without `known_faces = true` is never told a name,
    /// with names on.
    #[test]
    fn a_watch_without_known_faces_names_nobody() {
        let dir = tempfile::tempdir().unwrap();
        let mut watches = naming(dir.path(), "Minh\n");
        for entry in watches.each.values_mut() {
            entry.known_faces = false;
        }
        let outs = came_in(&mut watches, &[Some("Minh")], |_| Vec::new());
        assert!(
            sent(&outs)
                .iter()
                .all(|(text, named)| !named && !text.contains("Minh")),
            "{outs:?}"
        );
    }

    /// #2562: a yes taken back while the text still waits (the visit not
    /// over) names nobody: the eyes' `tellable` is read as the text goes,
    /// never as the face was seen. A forget, which removes the file, is the
    /// same.
    #[test]
    fn a_yes_taken_back_before_the_text_goes_names_nobody() {
        let dir = tempfile::tempdir().unwrap();
        let tellable = crate::known::tellable_of(&dir.path().join("known"));
        for took_back in ["Lan\n", ""] {
            let mut watches = naming(dir.path(), "Minh\nLan\n");
            let outs = came_in(&mut watches, &[Some("Minh")], |_| {
                if took_back.is_empty() {
                    std::fs::remove_file(&tellable).unwrap();
                } else {
                    std::fs::write(&tellable, took_back).unwrap();
                }
                Vec::new()
            });
            assert_eq!(
                sent(&outs),
                [(
                    "Someone you've introduced me to came in at 18:10 and was here for about 2 minutes.",
                    false
                )],
                "{took_back:?}"
            );
        }
    }

    /// #2562: never a name from a voice. Speech after the door, with the
    /// person saying who they are, and no face the eyes know: "someone".
    #[test]
    fn a_voice_alone_never_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut watches = naming(dir.path(), "Minh\n");
        let mut outs: Outs = Vec::new();
        watches.from_app(true, SECOND, true).unwrap();
        outs.extend(watches.ring_said(GUARD, true, None, SECOND));
        outs.extend(watches.observe("room/presence", &presence_of(false), MINUTE, None));
        voice(
            &mut watches,
            "resting",
            10 * MINUTE - 5 * SECOND,
            10 * MINUTE,
        );
        outs.extend(watches.observe(
            SOUNDS,
            &classes(0.9, 0.0, "unknown", 10 * MINUTE),
            10 * MINUTE,
            Some(18 * 60 + 10),
        ));
        let heard = json!({ "text": "Hi, it's Minh, I'm back", "finished": true }).to_string();
        outs.extend(watches.observe(
            "voice/heard",
            heard.as_bytes(),
            10 * MINUTE + 5 * SECOND,
            None,
        ));
        let mut now = 10 * MINUTE + 5 * SECOND;
        while now < 12 * MINUTE {
            outs.extend(watches.observe("room/presence", &presence_of(true), now, None));
            // The eyes know no face at the desk.
            outs.extend(watches.known(None, now, None));
            now += SECOND;
        }
        outs.extend(watches.observe("room/presence", &presence_of(false), now, None));
        let outs: Vec<Out> = outs.into_iter().map(|(_, out)| out).collect();
        assert_eq!(
            sent(&outs),
            [(
                "Someone came in at 18:10 and was here for about 2 minutes.",
                false
            )]
        );
        assert!(!format!("{outs:?}").contains("Minh"), "{outs:?}");
    }
    /// #2562 (reviewer-two's P2 on #2769): three names the eyes allow, 40
    /// letters of two bytes each, are past one line named; the visit's
    /// text is not lost: the message published is its unnamed words alone,
    /// and passes `MessageOut::check`.
    #[test]
    fn names_too_long_for_one_line_send_the_unnamed_text_not_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let names: Vec<String> = ["é", "ñ", "ü"]
            .iter()
            .map(|letter| letter.repeat(40))
            .collect();
        assert!(
            names
                .iter()
                .all(|name| name.chars().count() == 40 && name.len() == 80)
        );
        let mut watches = naming(dir.path(), &names.join("\n"));
        let seen: Vec<Option<&str>> = names.iter().map(|name| Some(name.as_str())).collect();
        let outs = came_in(&mut watches, &seen, |_| Vec::new());
        let named = outs
            .iter()
            .find(|out| matches!(out, Out::Named { .. }))
            .cloned()
            .expect("named before the bound");
        let Out::Named { text, .. } = &named else {
            unreachable!()
        };
        assert!(text.len() > lamp_msgs::MESSAGE_TEXT_BYTES, "{}", text.len());
        let message = text_message(GUARD, named)
            .unwrap()
            .expect("published, not dropped");
        assert_eq!(message.check(), Ok(()));
        assert_eq!(
            message.text,
            "Someone you've introduced me to came in at 18:10 and was here for about 2 minutes."
        );
        assert_eq!((message.unnamed, message.named.len()), (None, 0));
        // A plain text is its words; anything else is no message.
        let plain = text_message(GUARD, Out::Text("Someone came in.".into()))
            .unwrap()
            .unwrap();
        assert_eq!(
            (plain.text.as_str(), plain.unnamed),
            ("Someone came in.", None)
        );
        assert!(text_message(GUARD, Out::Unmark).is_none());
    }

    /// #2562: the named lines' slots are checked as the others are.
    #[test]
    fn the_named_lines_have_only_their_own_slots() {
        let broken = |change: fn(&mut Words)| {
            let mut words = guard_words();
            change(&mut words);
            words.checked().unwrap_err()
        };
        assert!(
            broken(|w| w.named_and_known = "someone the lamp knows".into())
                .contains("named_and_known")
        );
        assert!(broken(|w| w.and = "{names} and {who}".into()).contains("and"));
    }
}
