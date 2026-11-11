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
//!   most [`REST_MOST`]), as the app's restart waits ([`when_rested`]);
//! - the owner's developer setting (`control.set`, `docs/control.md`):
//!   lamp-control's drop-in ([`CONTROL_DROPIN`], `LAMP_CONTROL=on`). On:
//!   written and answered, then, once no update is at work (no quiet: its
//!   restart makes no sound), `systemctl daemon-reload` and a restart of
//!   [`CONTROL_UNIT`] ([`restart_control_when_unlocked`]). Off: the door is
//!   closed before the answer ([`close_before_answer`]);
//! - the owner's chosen voice and character words (`voice` and `character`
//!   beside the language, #5394, `docs/chosen-voice.md`): one line each,
//!   root's, 0644, checked by [`lamp_msgs::chosen_voice`]'s rules; an empty
//!   one removes the file. The voice reads them at start, so it restarts as
//!   for a new language. The reset's plan wipes both. `lamp-cloud` sends
//!   them only with `LAMP_CHOSEN_VOICE=on`;
//! - the owner's two yeses from the app (`consent`, #5500,
//!   `docs/onboarding.md`): face expressions for lamp-eyes and room sounds
//!   for lamp-ears, each a file in [`Config::consent`] as the voice's
//!   onboarding keeps it, with `"by":"app"` ([`lamp_msgs::consent`]),
//!   `root:lampos` 0640 (the folder's group; 0600 with none). Off removes
//!   it. lamp-eyes reads its yes every 2 s, so nothing restarts; lamp-ears
//!   reads room sounds' only at start, so a new yes there restarts it once
//!   no update is at work ([`restart_ears_when_unlocked`]); an off is
//!   followed within 2 s as it is. `lamp-cloud` sends them only with
//!   `LAMP_APP_CONSENT=on`, room sounds only on a lamp with no slide. The
//!   reset's plan, a forget of everything and a redo of setup remove both,
//!   whoever gave them.
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
/// The unit restarted after a new room sounds yes from the app.
pub const EARS_UNIT: &str = "lamp-ears.service";
/// The unit restarted after the owner's developer setting changed.
pub const CONTROL_UNIT: &str = "lamp-control.service";
/// lamp-control's drop-in while developer access is on.
pub const CONTROL_DROPIN: &str = "/etc/systemd/system/lamp-control.service.d/developer.conf";
/// What the drop-in holds, exactly.
pub const CONTROL_DROPIN_TEXT: &str = "[Service]\nEnvironment=LAMP_CONTROL=on\n";

/// lamp-control's second drop-in, while the owner's local agent switch is on:
/// the one user the lamp's coding agents run as, and the group and folder its
/// socket needs. Its text is [`lamp_msgs::local_agent::DROPIN_TEXT`], fixed:
/// no request names a user.
pub const AGENT_DROPIN: &str = "/etc/systemd/system/lamp-control.service.d/agent.conf";

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
    /// The owner's developer setting: lamp-control on or off.
    Control(bool),
    /// The owner's switch for the local agent (`docs/control.md`, "The local
    /// agent"): lamp-control's socket for the one fixed user, on or off.
    LocalAgent(bool),
    /// The owner's chosen voice's name; `None` goes back to the default.
    Voice(Option<String>),
    /// The owner's words for the character; `None` clears them.
    Character(Option<String>),
    /// One of the owner's yeses from the app (a key of
    /// [`lamp_msgs::consent::FILES`]): kept, or removed.
    Consent(&'static str, bool),
}

/// Where the chosen voice and the character words are kept: beside the
/// language, in the folder updates keep and the reset's plan names.
pub fn beside_language(config: &Config, name: &str) -> std::path::PathBuf {
    config.language.with_file_name(name)
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
/// `{"language": s}`, `{"voice": s}`, `{"character": s}` (`""` clears
/// either), or `{"owner_place": {"name", "lat", "lon", "set_at"}}`
/// (`null` clears it), or `{"control": bool}`, or `{"local_agent": bool}`, or `{"consent": {KEY: bool}}`
/// with exactly one key of [`lamp_msgs::consent::FILES`].
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
        // Checked again here, whatever lamp-cloud sent (#5394).
        "voice" => lamp_msgs::chosen_voice::fit_voice(text.ok_or("voice must be text")?)
            .map(Change::Voice)
            .map_err(Into::into),
        "character" => {
            lamp_msgs::chosen_voice::fit_character(text.ok_or("character must be text")?)
                .map(Change::Character)
                .map_err(Into::into)
        }
        // Rounded again here, whatever lamp-cloud sent: root's copy of the
        // rule is the one that writes.
        "owner_place" if given.is_null() => Ok(Change::OwnerPlace(None)),
        "owner_place" => Ok(Change::OwnerPlace(Some(lamp_msgs::owner_place::fit(
            given,
        )?))),
        "control" => Ok(Change::Control(
            given.as_bool().ok_or("control must be true or false")?,
        )),
        // The agent's socket is lamp-control's, which runs only while
        // developer access is on: turning the agent on with it off is
        // refused in words, and nothing is written.
        "local_agent" => {
            let on = given.as_bool().ok_or("local_agent must be true or false")?;
            if on && fs::symlink_metadata(&config.control_dropin).is_err() {
                return Err("turn developer access on first: the local agent's socket is the developer door's".into());
            }
            Ok(Change::LocalAgent(on))
        }
        // Checked again here, whatever lamp-cloud sent (#5500).
        "consent" => {
            let why = "consent holds one of face_expressions or room_sounds, true or false";
            let asked = given
                .as_object()
                .filter(|asked| asked.len() == 1)
                .ok_or(why)?;
            let (key, yes) = asked.iter().next().ok_or(why)?;
            let key = lamp_msgs::consent::FILES
                .iter()
                .map(|(known, _)| *known)
                .find(|known| known == key)
                .ok_or(why)?;
            Ok(Change::Consent(key, yes.as_bool().ok_or(why)?))
        }
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
    /// lamp-control must follow its drop-in: it was written or removed.
    pub restart_control: bool,
    /// lamp-ears must take a new room sounds yes, read only at its start.
    pub restart_ears: bool,
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
                restart_control: false,
                restart_ears: false,
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
                restart_control: false,
                restart_ears: false,
                said: "the person's name was removed".into(),
            })
        }
        Change::DisplayName(name) => {
            files.display_name(name)?;
            Ok(Done {
                data: json!({"name": name}),
                restart_schedule: false,
                restart_voice: false,
                restart_control: false,
                restart_ears: false,
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
                restart_control: false,
                restart_ears: false,
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
                restart_control: false,
                restart_ears: false,
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
                restart_control: false,
                restart_ears: false,
                said: format!("the language is now {code}"),
            })
        }
        // Written only when it is not there as it should be, so a repeat
        // restarts nothing.
        Change::Control(on) => {
            let path = &config.control_dropin;
            let changed = if *on {
                let there = fs::read(path).ok();
                if there.as_deref() != Some(CONTROL_DROPIN_TEXT.as_bytes()) {
                    files.control_dropin(CONTROL_DROPIN_TEXT.as_bytes())?;
                    true
                } else {
                    false
                }
            } else if fs::symlink_metadata(path).is_ok() {
                remove(path)?;
                true
            } else {
                false
            };
            Ok(Done {
                data: json!({"on": on}),
                restart_schedule: false,
                restart_voice: false,
                restart_control: changed,
                restart_ears: false,
                said: format!(
                    "developer access is {}{}",
                    if *on { "on" } else { "off" },
                    if changed { "" } else { " already" }
                ),
            })
        }
        // Written only when it is not there as it should be, so a repeat
        // restarts nothing.
        Change::LocalAgent(on) => {
            let path = &config.agent_dropin;
            let text = lamp_msgs::local_agent::DROPIN_TEXT.as_bytes();
            let changed = if *on {
                let there = fs::read(path).ok();
                if there.as_deref() != Some(text) {
                    files.agent_dropin(text)?;
                    true
                } else {
                    false
                }
            } else if fs::symlink_metadata(path).is_ok() {
                remove(path)?;
                true
            } else {
                false
            };
            Ok(Done {
                data: json!({"on": on}),
                restart_schedule: false,
                restart_voice: false,
                restart_control: changed,
                restart_ears: false,
                said: format!(
                    "the local agent is {}{}",
                    if *on { "on" } else { "off" },
                    if changed { "" } else { " already" }
                ),
            })
        }
        Change::Voice(voice) => {
            let path = beside_language(config, "voice");
            let changed = keep_line(&path, voice.as_deref())?;
            Ok(Done {
                data: json!({"provider": "elevenlabs", "voice": voice.as_deref().unwrap_or(""), "speed": 1.0}),
                restart_schedule: false,
                restart_voice: changed,
                restart_control: false,
                restart_ears: false,
                // A voice's name is no one's: said, as the language is.
                said: format!(
                    "the chosen voice is now {}",
                    voice.as_deref().unwrap_or("the default")
                ),
            })
        }
        // The journal says only how long the words are, never them.
        Change::Character(text) => {
            let path = beside_language(config, "character");
            let changed = keep_line(&path, text.as_deref())?;
            Ok(Done {
                data: json!({"text": text.as_deref().unwrap_or("")}),
                restart_schedule: false,
                restart_voice: changed,
                restart_control: false,
                restart_ears: false,
                said: match text {
                    Some(text) => format!(
                        "the character's words were set ({} characters)",
                        text.chars().count()
                    ),
                    None => "the character's words were cleared".into(),
                },
            })
        }
        Change::Consent(key, yes) => {
            let path = lamp_msgs::consent::file_in(&config.consent, key)
                .ok_or_else(|| io::Error::other("no such yes"))?;
            let before = lamp_msgs::consent::read(&path, key).yes;
            if *yes && !before {
                keep_yes(&path, key)?;
            } else if !*yes {
                remove(&path)?;
            }
            let words = if *key == lamp_msgs::consent::ROOM_SOUNDS {
                "room sounds"
            } else {
                "face expressions"
            };
            Ok(Done {
                data: json!({ key.to_string(): *yes }),
                restart_schedule: false,
                restart_voice: false,
                restart_control: false,
                restart_ears: *yes && !before && *key == lamp_msgs::consent::ROOM_SOUNDS,
                said: format!(
                    "the {words} yes is {} from the app{}",
                    if *yes { "on" } else { "off" },
                    if *yes == before { " already" } else { "" }
                ),
            })
        }
        Change::OwnerPlace(None) => {
            remove(&lamp_msgs::owner_place::beside_name(&config.name))?;
            Ok(Done {
                data: json!({"cleared": true}),
                restart_schedule: false,
                restart_voice: false,
                restart_control: false,
                restart_ears: false,
                said: "the owner's place was cleared".into(),
            })
        }
    }
}

/// `line` kept at `path` as one line, root's, 0644 (the voice reads it as
/// `lampos`), or the file removed for `None`; whether its bytes changed.
fn keep_line(path: &Path, line: Option<&str>) -> io::Result<bool> {
    let before = fs::read(path).ok();
    match line {
        Some(line) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let temporary = crate::serve::stage(path, format!("{line}\n").as_bytes(), 0o644)?;
            crate::serve::put_in_place(&temporary, path)?;
        }
        None => remove(path)?,
    }
    Ok(before != fs::read(path).ok())
}

/// A yes from the app kept at `path`, whole and in place at once:
/// `{"version":1,KEY:"yes","at_unix_s":N,"by":"app"}`, 0640 and the
/// folder's group (`lampos` on a lamp, which the eyes and the ears are in),
/// or 0600 where the folder's group is root's, as the voice keeps its own.
fn keep_yes(path: &Path, key: &str) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let folder = path
        .parent()
        .ok_or_else(|| io::Error::other("a yes has no folder"))?;
    let group = Some(fs::metadata(folder)?.gid()).filter(|group| *group != 0);
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let bytes = lamp_msgs::consent::bytes(key, at, lamp_msgs::consent::BY_APP);
    let next = path.with_extension("change-next");
    let _ = fs::remove_file(&next);
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&next)?;
        let mode = if group.is_some() { 0o640 } else { 0o600 };
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        if group.is_some() {
            std::os::unix::fs::fchown(&file, None, group)?;
        }
        io::Write::write_all(&mut file, &bytes)?;
        file.sync_all()?;
        fs::rename(&next, path)?;
        fs::File::open(folder)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&next);
    }
    result
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
/// So the lock is taken before anything is written, waited for as turning
/// developer access off waits for it ([`CLOSE_LOCK_MOST`], under
/// lamp-cloud's wait for the reply): the change before it holds it until
/// the voice has restarted (#5394), so one made meanwhile waits and then
/// applies. Still held then, the change is refused with [`STILL_APPLYING`]
/// and nothing is written. Otherwise the lock is given back with the
/// reply, to hold through the voice's restart, or let go at once when
/// there is none (the same language).
pub fn answer_holding(
    line: &str,
    config: &Config,
    enabled: bool,
    records: &lamp_update::records::Records,
    ears: &mut impl Ears,
) -> (Value, Option<Done>, Option<lamp_update::records::Lock>) {
    // The chosen voice and character restart the voice too (#5394).
    let language = enabled
        && matches!(
            parse(line.trim_end_matches(['\n', '\r']), config),
            Ok(Change::Language(_) | Change::Voice(_) | Change::Character(_))
        );
    if !language {
        let (reply, done) = answer(line, config, enabled);
        return (reply, done, None);
    }
    let lock = match unlocked(ears, records, CLOSE_LOCK_EVERY, CLOSE_LOCK_MOST) {
        Ok(lock) => lock,
        Err(why) if why == STILL_HELD => {
            return (json!({ "refused": STILL_APPLYING }), None, None);
        }
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

/// lamp-update's lock alone, with no quiet: for lamp-control, whose restart
/// makes no sound but must still never fall in a round's watch. Looked for
/// every `every`, at most `most`; why not.
pub fn unlocked(
    ears: &mut impl Ears,
    records: &lamp_update::records::Records,
    every: Duration,
    most: Duration,
) -> Result<lamp_update::records::Lock, String> {
    let started = ears.now_ms();
    loop {
        match records.lock() {
            Ok(lock) => return Ok(lock),
            Err(why) if why.kind() == io::ErrorKind::WouldBlock => {}
            Err(why) => return Err(format!("the update's state could not be checked: {why}")),
        }
        if ears.now_ms().saturating_sub(started) >= most.as_millis() as u64 {
            return Err(STILL_HELD.into());
        }
        ears.listen(every);
    }
}

/// Why [`unlocked`] gave up: the lock was held all along.
pub const STILL_HELD: &str = "an update was installing all along";

/// How long turning developer access off, and a change that restarts the
/// voice, wait for lamp-update's lock before they refuse: under
/// lamp-cloud's 10 s wait for this helper (`lamp_cloud::settings::WAIT`),
/// so the app hears the result, never a timeout.
pub const CLOSE_LOCK_MOST: Duration = Duration::from_secs(8);
/// How often it looks for the lock meanwhile.
pub const CLOSE_LOCK_EVERY: Duration = Duration::from_secs(1);
/// The refusal of a change that restarts the voice while the lock is held
/// past [`CLOSE_LOCK_MOST`]: by the change before it, still waiting for the
/// voice to rest (at most [`REST_MOST`]), or by an update; which one is not
/// told, so the words fit both.
pub const STILL_APPLYING: &str = "the lamp is still applying a change; try again in a moment";
/// The refusal while an update holds the lock past [`CLOSE_LOCK_MOST`].
pub const CLOSE_INSTALLING: &str = "an update is installing: try again in a few minutes";
/// Said when the drop-in is gone but lamp-control did not stop (or, for the
/// local agent, did not restart).
pub const CLOSE_MAY_STAY_OPEN: &str = "the door may stay open until the lamp next restarts";

/// systemd, as the change helper drives it for lamp-control. Injected for tests.
pub trait Systemd {
    /// `systemctl` with `args`; why not, when it failed.
    fn systemctl(&mut self, args: &[&str]) -> Result<(), String>;
}

/// The real one: `/usr/bin/systemctl`.
pub struct Systemctl;

impl Systemd for Systemctl {
    fn systemctl(&mut self, args: &[&str]) -> Result<(), String> {
        match std::process::Command::new("/usr/bin/systemctl")
            .args(args)
            .status()
        {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(status.to_string()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// A clock that only waits, for [`unlocked`] where no voice is listened to.
pub struct Clock(std::time::Instant);

impl Default for Clock {
    fn default() -> Self {
        Self(std::time::Instant::now())
    }
}

impl Ears for Clock {
    fn listen(&mut self, wait: Duration) -> Vec<Heard> {
        std::thread::sleep(wait);
        Vec::new()
    }
    fn now_ms(&mut self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }
}

/// Developer access turned off while lamp-control's drop-in is there, or the
/// local agent turned off while its drop-in is there: the door is closed
/// before the app is told. lamp-update's lock first, at most
/// [`CLOSE_LOCK_MOST`], else refused with nothing changed; then, holding it,
/// the drop-in removed (developer off also the agent's), `daemon-reload` and
/// `stop` (a `restart` for the agent off beside developer access still on, so
/// the agent's socket goes and the developer door stays), and only then the reply,
/// which this returns for the caller to write. A stop that failed is said,
/// never claimed closed. `None` when `line` is not such a request: it is
/// answered by [`answer`] as any other.
pub fn close_before_answer(
    line: &str,
    config: &Config,
    enabled: bool,
    records: &lamp_update::records::Records,
    ears: &mut impl Ears,
    systemd: &mut impl Systemd,
) -> Option<(Value, Option<Done>)> {
    if !enabled {
        return None;
    }
    let there = |path: &Path| fs::symlink_metadata(path).is_ok();
    // Developer access off removes the agent's drop-in too: its socket is
    // the developer door's, and a later developer yes must not reopen the
    // agent without the owner's own yes. The agent off alone removes only its
    // drop-in and keeps the developer door, so the unit is restarted then
    // (a stop only when developer access is off as well).
    let (files, what, restart): (Vec<&Path>, &str, bool) =
        match parse(line.trim_end_matches(['\n', '\r']), config) {
            // The agent's drop-in first, the developer's last: a removal that
            // fails then leaves the retry on this path. An agent drop-in left
            // with no developer one beside it is cleared too.
            Ok(Change::Control(false))
                if there(&config.control_dropin) || there(&config.agent_dropin) =>
            {
                (
                    vec![
                        config.agent_dropin.as_path(),
                        config.control_dropin.as_path(),
                    ],
                    "developer access",
                    false,
                )
            }
            Ok(Change::LocalAgent(false)) if there(&config.agent_dropin) => (
                vec![config.agent_dropin.as_path()],
                "the local agent",
                there(&config.control_dropin),
            ),
            _ => return None,
        };
    let _held = match unlocked(ears, records, CLOSE_LOCK_EVERY, CLOSE_LOCK_MOST) {
        Ok(lock) => lock,
        Err(_) => return Some((json!({ "refused": CLOSE_INSTALLING }), None)),
    };
    // A removal that fails does not stop the rest: the unit is still reloaded
    // and stopped (or restarted), so the door is not left open with its drop-in
    // half gone, and the failure is then answered in words.
    let mut failed = None;
    for file in files {
        if there(file)
            && let Err(error) = remove(file)
        {
            failed.get_or_insert(error.kind());
        }
    }
    let (verb, did) = if restart {
        ("restart", "restarted")
    } else {
        ("stop", "stopped")
    };
    let stopped = systemd
        .systemctl(&["daemon-reload"])
        .and_then(|()| systemd.systemctl(&[verb, CONTROL_UNIT]));
    if let Some(kind) = failed {
        return Some((
            json!({"refused": format!("the change could not be saved: {kind}")}),
            None,
        ));
    }
    let (data, said) = match stopped {
        Ok(()) => (
            json!({"on": false, "closed": true}),
            format!("{what} is off; lamp-control {did}"),
        ),
        Err(why) => (
            json!({"on": false, "closed": false, "note": CLOSE_MAY_STAY_OPEN}),
            format!("{what} is off; lamp-control did not {verb} ({why})"),
        ),
    };
    let done = Done {
        data: data.clone(),
        restart_schedule: false,
        restart_voice: false,
        restart_control: false,
        restart_ears: false,
        said,
    };
    Some((json!({ "done": data }), Some(done)))
}

/// After developer access was turned on and answered: lamp-update's lock
/// alone ([`unlocked`], every [`UPDATE_LOOK_EVERY`], at most [`QUIET_MOST`]),
/// held through `daemon-reload` and a `restart` (never try-restart, so an
/// enabled unit that exited off starts). What happened, for the journal.
pub fn restart_control_when_unlocked(
    ears: &mut impl Ears,
    records: &lamp_update::records::Records,
    systemd: &mut impl Systemd,
) -> String {
    let _held = match unlocked(ears, records, UPDATE_LOOK_EVERY, QUIET_MOST) {
        Ok(lock) => lock,
        Err(why) => {
            return format!(
                "no lock within {} min ({why}); lamp-control follows the developer setting when it next starts",
                QUIET_MOST.as_secs() / 60
            );
        }
    };
    match systemd
        .systemctl(&["daemon-reload"])
        .and_then(|()| systemd.systemctl(&["restart", CONTROL_UNIT]))
    {
        Ok(()) => "lamp-control restarted on the developer setting".into(),
        Err(why) => format!("lamp-control's restart failed: {why}"),
    }
}

/// After a new room sounds yes from the app was kept and answered: lamp-ears
/// starts its classifier only at its start, so it is restarted, holding
/// lamp-update's lock alone ([`unlocked`], every [`UPDATE_LOOK_EVERY`], at
/// most [`QUIET_MOST`]), as lamp-control is: its restart makes no sound, but
/// a round's watch must never see it. `try-restart`: an ears not running
/// reads the yes when it next starts. What happened, for the journal.
pub fn restart_ears_when_unlocked(
    ears: &mut impl Ears,
    records: &lamp_update::records::Records,
    systemd: &mut impl Systemd,
) -> String {
    let _held = match unlocked(ears, records, UPDATE_LOOK_EVERY, QUIET_MOST) {
        Ok(lock) => lock,
        Err(why) => {
            return format!(
                "no lock within {} min ({why}); lamp-ears takes the room sounds yes when it next starts",
                QUIET_MOST.as_secs() / 60
            );
        }
    };
    match systemd.systemctl(&["try-restart", EARS_UNIT]) {
        Ok(()) => "lamp-ears restarted on the room sounds yes".into(),
        Err(why) => format!("lamp-ears' restart failed: {why}"),
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
