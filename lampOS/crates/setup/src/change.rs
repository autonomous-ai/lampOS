//! A name or the time zone changed after setup, without a reset (#1959,
//! `docs/setup.md`, "Changed after setup").
//!
//! `lamp-cloud` runs as `lampos` with no write access, so it asks root's
//! helper, `lamp-setup change`, one line each way on a socket systemd holds
//! (`lamp-setup-change.socket`). The helper checks every request itself,
//! whoever connects, and writes with setup's own writers ([`crate::serve::Files`]),
//! so a change leaves the files as a setup with the new value would.
//!
//! What reads each value, and how it follows:
//! - the person's name (`/etc/lampOS/person/name`): the voice reads it again
//!   at each hello and each new session, so nothing restarts;
//! - the lamp's name in the app (`/etc/lampOS/setup/display-name`):
//!   `lamp-cloud` reads it at each answer. It never changes what the lamp
//!   answers to: the wake word stays "Lamp";
//! - the zone (`/etc/localtime`, `/etc/timezone`, `/etc/lampOS/zone.env`):
//!   the voice, memory, Expression and skills follow `/etc/localtime` at each
//!   reading. `lamp-schedule` takes `LAMP_ZONE` from `zone.env` at start, so
//!   it is restarted, once the lamp is quiet ([`wait_for_quiet`]). Reminders
//!   already set keep the zone they were set in;
//! - the owner's approximate place (`/etc/lampOS/person/place`, beside the
//!   name, #4333, `docs/owner-place.md`): rounded here again to about a
//!   kilometre ([`lamp_msgs::owner_place::fit`]) and kept root's, 0600; the
//!   voice reads it again for each question that needs a place, so nothing
//!   restarts and a clear holds from the next question. `lamp-cloud` sends
//!   it only with `LAMP_OWNER_PLACE=on`;
//! - the owner's language (`/etc/lampOS/setup/language`, #4473): one code,
//!   root's, 0644, as setup writes it. The voice reads it at start, so once
//!   it changed the voice is restarted when it rests between turns (5 s, at
//!   most [`REST_MOST`]), as the app's restart waits ([`when_rested`]).
use crate::serve::{Config, Files, Save, write_private};
use lamp_update::quiet::{Heard, QUIET, Quiet};
use serde_json::{Value, json};
use std::{fs, io, path::Path, time::Duration};

/// The switch on `lamp-cloud`: `on` takes the kinds below from the app.
/// Anything else, and they are refused as kinds this lamp lacks.
pub const ENVIRONMENT: &str = "LAMP_SETTINGS_CHANGE";

/// The longest name kept, in characters: setup's and the voice's own bound.
pub const NAME_MOST: usize = 64;
/// The longest request line read, in bytes.
pub const LINE_MOST: usize = 4_096;
/// How long a zone change waits for the lamp to be quiet before it leaves
/// `lamp-schedule` on the old zone until its next start.
pub const QUIET_MOST: Duration = Duration::from_secs(30 * 60);
/// The unit restarted after a zone change.
pub const SCHEDULE_UNIT: &str = "lamp-schedule.service";

/// One change, checked.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// The person's name; `None` removes it.
    PersonName(Option<String>),
    /// The lamp's name in the app.
    DisplayName(String),
    /// An IANA zone the lamp's zone data and the parts' both have.
    Zone(String),
    /// The owner's place, rounded; `None` clears it.
    OwnerPlace(Option<lamp_msgs::owner_place::Place>),
    /// The owner's language: one of [`crate::serve::LANGUAGES`].
    Language(&'static str),
}

/// A control character, a line or paragraph separator, or a mark that turns
/// the text's direction: none belongs in a name (the voice's own rule,
/// `crates/voice/src/person.rs`).
fn breaks_or_turns(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{2028}' | '\u{2029}' | '\u{200e}' | '\u{200f}')
        || ('\u{202a}'..='\u{202e}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
}

/// A name trimmed, or why it can't be kept. Never echoes it.
pub fn fit_name(name: &str) -> Result<String, &'static str> {
    let name = name.trim();
    if name.chars().count() > NAME_MOST || name.chars().any(breaks_or_turns) {
        return Err("name must be at most 64 characters on one line");
    }
    Ok(name.to_owned())
}

/// The one line `lamp-cloud` sends, checked against `config`: exactly
/// `{"person_name": s}`, `{"display_name": s}`, `{"timezone": s}`,
/// `{"language": s}`, or `{"owner_place": {"name", "lat", "lon", "set_at"}}`
/// (`null` clears it).
pub fn parse(line: &str, config: &Config) -> Result<Change, String> {
    if line.len() > LINE_MOST {
        return Err("the request is too long".into());
    }
    let value: Value = serde_json::from_str(line).map_err(|_| "the request is not JSON")?;
    let object = value.as_object().ok_or("the request is not an object")?;
    if object.len() != 1 {
        return Err("the request holds one change".into());
    }
    let (key, given) = object.iter().next().ok_or("the request holds one change")?;
    let text = given.as_str();
    match key.as_str() {
        "person_name" => {
            let name = fit_name(text.ok_or("name must be text")?)?;
            Ok(Change::PersonName((!name.is_empty()).then_some(name)))
        }
        "display_name" => {
            let name = fit_name(text.ok_or("name must be text")?)?;
            if name.is_empty() {
                return Err("name must be 1 to 64 characters".into());
            }
            Ok(Change::DisplayName(name))
        }
        "timezone" => {
            let zone = text.ok_or("timezone must be text")?.trim();
            if !crate::serve::zone_known(zone, &config.zoneinfo)
                || lamp_local_time::LocalTime::zone(zone).is_err()
            {
                return Err(
                    "timezone must be a zone the lamp knows, such as Asia/Ho_Chi_Minh".into(),
                );
            }
            Ok(Change::Zone(zone.to_owned()))
        }
        "language" => crate::serve::language(text.ok_or(crate::serve::LANGUAGE_REFUSED)?)
            .map(Change::Language)
            .ok_or_else(|| crate::serve::LANGUAGE_REFUSED.into()),
        // Rounded again here, whatever lamp-cloud sent: root's copy of the
        // rule is the one that writes.
        "owner_place" if given.is_null() => Ok(Change::OwnerPlace(None)),
        "owner_place" => Ok(Change::OwnerPlace(Some(lamp_msgs::owner_place::fit(
            given,
        )?))),
        _ => Err("the request names no change this lamp makes".into()),
    }
}

/// What a change wrote, for the reply and for what follows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    /// The reply's `data`.
    pub data: Value,
    /// `lamp-schedule` must take the new zone: `zone.env` changed.
    pub restart_schedule: bool,
    /// The voice must take the new language: the language file changed.
    pub restart_voice: bool,
    /// The journal's line: what changed, never a name.
    pub said: String,
}

/// Remove `path`, durably; one that is not there is removed already.
fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

/// Make `change` with setup's writers on the files `config` names.
pub fn apply(change: &Change, config: &Config) -> io::Result<Done> {
    let mut files = Files(config);
    match change {
        Change::PersonName(Some(name)) => {
            files.name(name)?;
            Ok(Done {
                data: json!({"name": name}),
                restart_schedule: false,
                restart_voice: false,
                said: format!(
                    "the person's name was changed ({} characters)",
                    name.chars().count()
                ),
            })
        }
        Change::PersonName(None) => {
            remove(&config.name)?;
            Ok(Done {
                data: json!({"name": ""}),
                restart_schedule: false,
                restart_voice: false,
                said: "the person's name was removed".into(),
            })
        }
        Change::DisplayName(name) => {
            files.display_name(name)?;
            Ok(Done {
                data: json!({"name": name}),
                restart_schedule: false,
                restart_voice: false,
                said: format!(
                    "the lamp's name in the app was changed ({} characters)",
                    name.chars().count()
                ),
            })
        }
        Change::Zone(zone) => {
            let before = fs::read(&config.zone_env).ok();
            files.timezone(zone)?;
            let after = fs::read(&config.zone_env).ok();
            Ok(Done {
                data: json!({"timezone": zone}),
                restart_schedule: before != after,
                restart_voice: false,
                // A zone is no one's name: said, as setup's own line says it.
                said: format!("the time zone is now {zone}"),
            })
        }
        // The journal says only that it changed: never the name or where.
        Change::OwnerPlace(Some(place)) => {
            write_private(
                &lamp_msgs::owner_place::beside_name(&config.name),
                lamp_msgs::owner_place::text(place).as_bytes(),
            )?;
            Ok(Done {
                data: json!(place),
                restart_schedule: false,
                restart_voice: false,
                said: "the owner's place was set".into(),
            })
        }
        Change::Language(code) => {
            let before = fs::read(&config.language).ok();
            files.language(code)?;
            let after = fs::read(&config.language).ok();
            Ok(Done {
                data: json!({"language": code}),
                restart_schedule: false,
                restart_voice: before != after,
                said: format!("the language is now {code}"),
            })
        }
        Change::OwnerPlace(None) => {
            remove(&lamp_msgs::owner_place::beside_name(&config.name))?;
            Ok(Done {
                data: json!({"cleared": true}),
                restart_schedule: false,
                restart_voice: false,
                said: "the owner's place was cleared".into(),
            })
        }
    }
}

/// The refusal of every request without `--enable`.
pub const OFF: &str = "changes after setup are off";

/// The helper's reply line to one request: `{"done": data}` or
/// `{"refused": why}`. A refusal never holds what was asked.
pub fn answer(line: &str, config: &Config, enabled: bool) -> (Value, Option<Done>) {
    if !enabled {
        return (json!({ "refused": OFF }), None);
    }
    let change = match parse(line.trim_end_matches(['\n', '\r']), config) {
        Ok(change) => change,
        Err(why) => return (json!({"refused": why}), None),
    };
    match apply(&change, config) {
        Ok(done) => (json!({"done": done.data}), Some(done)),
        Err(error) => (
            json!({"refused": format!("the change could not be saved: {}", error.kind())}),
            None,
        ),
    }
}

/// [`answer`], with lamp-update's lock in `records` for a new language
/// (#4473, Fourth's P2 on #4485): the voice restarts after it, and a round
/// puts its build back when a unit it watches restarts within its watch.
/// So the lock is taken before anything is written; held by an update,
/// the change is refused with the restart's words and nothing is written.
/// Otherwise it is given back with the reply, to hold through the voice's
/// restart, or let go at once when there is none (the same language).
pub fn answer_holding(
    line: &str,
    config: &Config,
    enabled: bool,
    records: &lamp_update::records::Records,
) -> (Value, Option<Done>, Option<lamp_update::records::Lock>) {
    let language = enabled
        && matches!(
            parse(line.trim_end_matches(['\n', '\r']), config),
            Ok(Change::Language(_))
        );
    if !language {
        let (reply, done) = answer(line, config, enabled);
        return (reply, done, None);
    }
    let lock = match lamp_keeper::restart::update_lock(records) {
        Ok(lock) => lock,
        Err(why) => return (json!({ "refused": why }), None, None),
    };
    let (reply, done) = answer(line, config, enabled);
    let held = done
        .as_ref()
        .filter(|done| done.restart_voice)
        .map(|_| lock);
    (reply, done, held)
}

/// The app's restart and shutdown (`system.reboot`, `system.shutdown`;
/// #2193, #3697): `lamp_keeper::restart`'s requests, answers and update
/// lock, the keeper's own.
pub use lamp_keeper::restart::Power;

/// How long a restart from the app waits for the voice to rest between
/// turns before it restarts all the same: the keeper's own bound.
pub const REST_MOST: Duration = QUIET;

/// The reply to a restart or shutdown that `line` asks, read against
/// lamp-update's lock in `records`, and the lock to hold while acting when
/// it is to act; `None` when `line` asks neither. Off (no `--enable`),
/// refused as every change is.
pub fn power(
    line: &str,
    enabled: bool,
    records: &lamp_update::records::Records,
) -> Option<(Power, Value, Option<lamp_update::records::Lock>)> {
    let power = Power::of(line)?;
    if !enabled {
        return Some((power, json!({ "refused": OFF }), None));
    }
    let lock = lamp_keeper::restart::update_lock(records);
    let (reply, go) = power.reply(lock.as_ref().map(|_| ()).map_err(Clone::clone));
    Some((power, reply, lock.ok().filter(|_| go)))
}

/// A restart from the app, once answered: wait for the voice to rest
/// between turns (5 s resting, nobody heard, nothing said), at most
/// [`REST_MOST`], then `restart`, all the same if it never rested, as the
/// keeper's restart does. What happened, for the journal.
pub fn restart_when_rested(
    ears: &mut impl Ears,
    restart: impl FnOnce() -> Result<(), String>,
) -> String {
    when_rested(ears, "lampos.target", restart)
}

/// [`restart_when_rested`] for any `unit`, named in what it says: the
/// voice after a new language too (#4473).
pub fn when_rested(
    ears: &mut impl Ears,
    unit: &str,
    restart: impl FnOnce() -> Result<(), String>,
) -> String {
    let waited = match wait_for_rest(ears, lamp_update::quiet::BETWEEN_TURNS, REST_MOST) {
        Ok(()) => "the voice rested".to_owned(),
        Err(why) => format!(
            "the voice did not rest within {} s ({why})",
            REST_MOST.as_secs()
        ),
    };
    match restart() {
        Ok(()) => format!("{waited}; {unit} restarted"),
        Err(why) => format!("{waited}; {unit} could not be restarted: {why}"),
    }
}

/// What the voice says on the bus, for [`wait_for_quiet`]. Injected for tests.
pub trait Ears {
    /// What the voice said in the next `wait`, which this takes.
    fn listen(&mut self, wait: Duration) -> Vec<Heard>;
    /// Monotonic milliseconds.
    fn now_ms(&mut self) -> u64;
}

/// Wait until the lamp is quiet as an update judges it before it restarts
/// the voice (`lamp_update::quiet`): the voice resting, nobody heard and
/// nothing said for 45 s. At most `most`; why not, when it never was.
pub fn wait_for_quiet(ears: &mut impl Ears, most: Duration) -> Result<(), String> {
    wait_for_rest(ears, QUIET, most)
}

/// How long [`quiet_and_unlocked`] waits before it looks for lamp-update's
/// lock again while an update holds it.
pub const UPDATE_LOOK_EVERY: Duration = Duration::from_secs(10);

/// [`wait_for_quiet`], and then lamp-update's lock, for the caller to hold
/// through a restart of its own: a round restarts and watches the lamp's
/// units, and puts its build back when one it restarted changes process
/// within its watch (`updates.md` step 9, `MainPID`), so a zone's
/// `lamp-schedule` restart or a voice restart for new keys must never fall
/// in it. While an update holds the lock, the quiet is waited for again
/// [`UPDATE_LOOK_EVERY`] later. At most `most` in all; why not.
pub fn quiet_and_unlocked(
    ears: &mut impl Ears,
    records: &lamp_update::records::Records,
    most: Duration,
) -> Result<lamp_update::records::Lock, String> {
    let started = ears.now_ms();
    let mut installing = false;
    loop {
        let spent = Duration::from_millis(ears.now_ms().saturating_sub(started));
        if let Err(why) = wait_for_quiet(ears, most.saturating_sub(spent)) {
            return Err(if installing {
                format!("an update was installing, and then {why}")
            } else {
                why
            });
        }
        match records.lock() {
            Ok(lock) => return Ok(lock),
            Err(why) if why.kind() == io::ErrorKind::WouldBlock => installing = true,
            Err(why) => return Err(format!("the update's state could not be checked: {why}")),
        }
        if ears.now_ms().saturating_sub(started) >= most.as_millis() as u64 {
            return Err("an update was installing all along".into());
        }
        ears.listen(UPDATE_LOOK_EVERY);
    }
}

/// Wait until the voice has rested, nobody heard and nothing said, for
/// `long`. At most `most`; why not, when it never did.
pub fn wait_for_rest(ears: &mut impl Ears, long: Duration, most: Duration) -> Result<(), String> {
    let started = ears.now_ms();
    let mut quiet = Quiet::new(started);
    loop {
        let now = ears.now_ms();
        if quiet.for_at_least(long, now).is_ok() {
            return Ok(());
        }
        if now.saturating_sub(started) >= most.as_millis() as u64 {
            return Err(quiet
                .for_at_least(long, now)
                .err()
                .unwrap_or_else(|| "never quiet".into()));
        }
        for heard in ears.listen(Duration::from_secs(1)) {
            let at = ears.now_ms();
            quiet.hear(heard, at);
        }
    }
}

/// The bus name the helper subscribes under. The bus refuses a `-`
/// (`[a-z][a-z0-9_]{0,31}`), and refused, quiet can never be judged.
pub const SUBSCRIBER: &str = "setup_change";

/// The real bus: `voice/state`, `voice/heard` and `voice/said`.
pub struct BusEars {
    state: Option<lamp_bus::Subscriber>,
    speech: Vec<lamp_bus::Subscriber>,
    started: std::time::Instant,
}

impl BusEars {
    /// Subscribes under [`SUBSCRIBER`]; a topic that can't be subscribed to
    /// is said and left out, and quiet then cannot be judged.
    pub fn new(bus: &lamp_bus::Bus, say: &mut dyn FnMut(&str)) -> Self {
        use lamp_msgs::{Speech, VoiceState};
        let mut subscribe = |topic: &str, type_name: &str| match bus
            .subscribe_typed(topic, SUBSCRIBER, type_name)
        {
            Ok(subscriber) => Some(subscriber),
            Err(error) => {
                say(&format!("{topic} cannot be subscribed to: {error}"));
                None
            }
        };
        let state = subscribe(VoiceState::TOPIC, VoiceState::TYPE);
        let speech = Speech::TOPICS
            .iter()
            .filter_map(|topic| subscribe(topic, Speech::TYPE))
            .collect();
        Self {
            state,
            speech,
            started: std::time::Instant::now(),
        }
    }
}

impl Ears for BusEars {
    fn listen(&mut self, wait: Duration) -> Vec<Heard> {
        let until = std::time::Instant::now() + wait;
        let mut heard = Vec::new();
        loop {
            let left = until.saturating_duration_since(std::time::Instant::now());
            match &mut self.state {
                Some(state) => {
                    if let Ok(Some(message)) = state.recv(left.min(Duration::from_millis(250))) {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |since| since.as_millis() as u64);
                        // The kept value of a voice that stopped says nothing of now.
                        if now.saturating_sub(message.header.at_unix_ms)
                            <= lamp_update::quiet::STATE_LASTS.as_millis() as u64
                            && let Ok(said) = message.parse::<lamp_msgs::VoiceState>()
                        {
                            heard.push(Heard::State(said.state));
                        }
                    }
                }
                None => std::thread::sleep(left.min(Duration::from_millis(250))),
            }
            for speech in &mut self.speech {
                while let Ok(Some(_)) = speech.recv(Duration::ZERO) {
                    heard.push(Heard::Speech);
                }
            }
            if std::time::Instant::now() >= until {
                return heard;
            }
        }
    }

    fn now_ms(&mut self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}
