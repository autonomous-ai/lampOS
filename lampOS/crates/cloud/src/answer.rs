//! The device API: the `data` kinds this lamp answers, each carried out by
//! the part of the lamp that already owns it (#1396). Everything else is
//! refused plainly, in V1's shape, so the app shows it.
use crate::light;
use crate::wire::not_here;
use lamp_msgs::{Light, MutedBy};
use serde::Serialize;
use serde_json::{Value, json};

/// The speaker's volume as V1's app shows it (`docs/mqtt.md`, `volume.*`):
/// `volume` is 0-100 of the allowed range, `raw` the mixer's percent and
/// `max_volume` the ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Volume {
    pub volume: u8,
    pub raw: u8,
    pub max_volume: u8,
}

/// The microphones as V1's app shows them (`mic.get`): `muted` is the
/// lamp's own mute, `hw_switch_muted` the privacy switch (`None` where
/// there is none), `available` whether the voice is running. `muted_by`
/// (#4332), only while muted, says who or what muted it, so a setup screen
/// can tell the restart guard's mute after a fault from one the person
/// chose; absent from a voice that does not say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Mic {
    pub muted: bool,
    pub hw_switch_muted: Option<bool>,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted_by: Option<MutedBy>,
}

/// What the answers read from and ask of the lamp.
pub trait Lamp {
    /// Each part's installed version, by name.
    fn versions(&mut self) -> Value;
    /// The default route's interface: `private_ip`, `interface`, `mac`,
    /// `ssid`, `gateway`, as V1's `system.network`.
    fn network(&mut self) -> Value;
    /// `hostname`, `device_id`, `device_name`, `uptime_seconds`, `timezone`.
    fn host(&mut self) -> Value;
    fn volume(&mut self) -> Result<Volume, String>;
    /// Sets the volume to `volume` of 100 of the allowed range.
    fn set_volume(&mut self, volume: u8) -> Result<Volume, String>;
    fn mic(&mut self) -> Result<Mic, String>;
    /// `lamp/health`'s level and message.
    fn health(&mut self) -> Result<Value, String>;
    /// The person's own light as the ring shows it (`light/state`).
    fn light(&mut self) -> Result<Light, String>;
    /// Asks for the person's light (`light/command`), after giving back a
    /// preview still shown, and reads back what the ring shows.
    fn set_light(&mut self, light: &Light) -> Result<Light, String>;
    /// Shows `light` over the person's for [`PREVIEW_HOLDS`] unless asked
    /// again, without keeping it (`light/request`), until the person asks
    /// for a light at the lamp.
    fn preview_light(&mut self, light: &Light) -> Result<(), String>;
    /// Whether setup's hotspot opens by itself when the saved network is
    /// gone (#3020, `LAMP_WIFI_RECOVERY=on`), so the app may name it. False:
    /// no field, read as before.
    fn recovery(&self) -> bool {
        false
    }
    /// Whether the reset button's 5 s hold takes this lamp back into setup
    /// (#2937, for the app's Re-set up): `hold` when the controls' pins name
    /// a reset, `none` when they don't (a lamp moved from V1, which never
    /// has one). None, said by no field: not known, read as before.
    fn reset(&self) -> Option<&'static str> {
        None
    }
    /// Whether texts from the app are taken (`LAMP_MESSAGES=on`,
    /// `crate::chat`). Off unless a lamp says so.
    fn texting(&self) -> bool {
        false
    }
    /// A new run's name, at `now_ms`.
    fn new_run(&mut self, now_ms: u64) -> String {
        format!("run-{now_ms}")
    }
    /// Puts `text` on the bus for the voice to answer (`chat/in`), and waits
    /// for its answer from `now_ms`.
    fn send_text(&mut self, _text: &lamp_msgs::ChatIn, _now_ms: u64) -> Result<(), String> {
        Err(not_here("chat.send"))
    }
    /// The `chat.event`s' `data` due at `now_ms`: pieces of answers from
    /// `chat/out`, and runs that waited too long.
    fn chat_events(&mut self, _now_ms: u64) -> Vec<Value> {
        Vec::new()
    }
    /// The skills' lines for the phone (`messages/out`, `crate::unasked`),
    /// with texts taken: those heard by `now_ms` are taken off the bus, and
    /// the pieces waiting are handed back to send when `give`, as the
    /// broker is there. With the journal's lines about those not sent.
    fn unasked(&mut self, _now_ms: u64, _give: bool) -> crate::unasked::Heard {
        crate::unasked::Heard::default()
    }
    /// Pieces not handed to the client, or lost before the broker's ack, to
    /// go again first.
    fn owe_unasked(&mut self, _pieces: Vec<crate::unasked::Piece>) {}
    /// A piece the broker acknowledged: how many have been since the start.
    fn unasked_sent(&mut self) -> u64 {
        0
    }
    /// Health and crash reports (#1683, `crate::reports`): there only with
    /// `LAMP_CLOUD=on` and `LAMP_REPORTS=on`.
    fn reports(&self) -> Option<std::sync::Arc<crate::reports::Shared>> {
        None
    }
    /// Where the skills are read for the app's skills page (#1722,
    /// `crate::skills`): there only with `LAMP_CLOUD=on` and
    /// `LAMP_SKILLS_PAGE=on`.
    fn skills(&self) -> Option<crate::skills::Places> {
        None
    }
    /// Names and the time zone changed after setup (#1959,
    /// `crate::settings`): there only with `LAMP_CLOUD=on` and
    /// `LAMP_SETTINGS_CHANGE=on`.
    fn changes(&self) -> Option<std::sync::Arc<crate::settings::Changes>> {
        None
    }
    /// Whether the broker is left a will that says the lamp is offline
    /// (`LAMP_CLOUD_OFFLINE_WILL=on`, `crate::online::options_with_will`).
    /// Off unless a lamp says so.
    fn offline_will(&self) -> bool {
        false
    }
    /// Where a face from the app's photo goes, and the eyes to ask (#1968,
    /// `crate::photos`): there only with `LAMP_PHOTO_FACES=on`.
    fn photos(&self) -> Option<crate::photos::Place> {
        None
    }
    /// The eyes asked for the faces they know and to forget one (#2192,
    /// `crate::faces`): there only with `LAMP_FACES_PAGE=on`.
    fn faces(&self) -> Option<crate::faces::Eyes> {
        None
    }

    /// lamp-skills' call socket, for guard mode from the owner's app
    /// (`LAMP_GUARD_FROM_APP=on`, #2562, `crate::guard`).
    fn guard(&self) -> Option<std::path::PathBuf> {
        None
    }

    /// lamp-voice's app-make socket, for a skill typed in the owner's app
    /// (`LAMP_APP_MAKE_SKILL=on` with the skills page, #2813,
    /// `crate::make_skill`).
    fn make_skill(&self) -> Option<std::path::PathBuf> {
        None
    }

    /// lamp-voice's maker has a computer to hand the work to
    /// (`crate::make_skill::is_ready`, fourteen-P15): `can` names
    /// `skills.make` only then.
    fn make_skill_ready(&self) -> bool {
        false
    }

    /// The lamp's timers and reminders (`LAMP_APP_SCHEDULE=on`, #2195,
    /// `crate::schedule`): there only with the switch.
    fn schedule(&self) -> Option<&lamp_actions::schedule::Adapter> {
        None
    }

    /// Whether the app may read the lamp's software update
    /// (`LAMP_APP_UPDATE=on`, #2194, `crate::update`).
    fn update_page(&self) -> bool {
        false
    }

    /// lamp-update's kept `update/status`, if it has said one.
    fn update_status(&self) -> Option<lamp_bus::Message> {
        None
    }

    /// Asks `lamp-update serve` for its next round now (`update.check`,
    /// #4153, `crate::update::Checks`), at `now`: at most once a minute, and
    /// only when a running lamp-update reads it. Only with
    /// `LAMP_APP_UPDATE=on`.
    fn check_update(&mut self, _now: std::time::Instant) -> Result<(), String> {
        Err(not_here("update.check"))
    }

    /// Asks setup's change helper to restart the lamp (`system.reboot`, #2193): its
    /// answer, given before the lamp goes down, or why not. Only with
    /// `LAMP_APP_RESTART=on`.
    fn restart(&mut self) -> Result<Value, String> {
        Err(not_here("system.reboot"))
    }

    /// Whether `system.reboot` is answered (`LAMP_APP_RESTART=on`), asked
    /// of nobody.
    fn restarts(&self) -> bool {
        false
    }

    /// Asks setup's change helper to turn the lamp off (`system.shutdown`, #3697): its
    /// answer, given before the lamp goes down, or why not. Only with
    /// `LAMP_SHUTDOWN=on`.
    fn shut_down(&mut self) -> Result<Value, String> {
        Err(not_here("system.shutdown"))
    }

    /// Whether `system.shutdown` is answered (`LAMP_SHUTDOWN=on`), asked of
    /// nobody.
    fn shuts_down(&self) -> bool {
        false
    }
}

/// Whether a kind is answered on `lamp` now: its switch, read without
/// asking any of the lamp's parts.
type Switch = fn(&dyn Lamp) -> bool;

const ALWAYS: Switch = |_| true;

/// Every kind the app may ask that this lamp can answer, with its switch,
/// in the order `system.info`'s `can` lists them (#2864).
pub const KINDS: [(&str, Switch); 43] = [
    ("system.info", ALWAYS),
    ("system.version", ALWAYS),
    ("system.network", ALWAYS),
    ("volume.get", ALWAYS),
    ("volume.set", ALWAYS),
    ("mic.get", ALWAYS),
    ("lamp.health", ALWAYS),
    ("led.resting.get", ALWAYS),
    ("led.resting.set", ALWAYS),
    ("led.resting.preview", ALWAYS),
    ("chat.send", |lamp| lamp.texting()),
    ("reports.get", |lamp| lamp.reports().is_some()),
    ("reports.set", |lamp| lamp.reports().is_some()),
    ("skills.list", |lamp| lamp.skills().is_some()),
    ("skills.remove", |lamp| lamp.skills().is_some()),
    // Off, on and the camera's yes with the page (#4099), so `can` tells an
    // app the lamp that takes them from an older one.
    ("skills.off", |lamp| lamp.skills().is_some()),
    ("skills.on", |lamp| lamp.skills().is_some()),
    ("skills.camera", |lamp| lamp.skills().is_some()),
    ("skills.install", |lamp| {
        lamp.skills().is_some_and(|places| places.store)
    }),
    (crate::settings::WIFI_KIND, WIFI),
    (crate::settings::WIFI_CONFIRM_KIND, WIFI),
    (crate::settings::KEYS_KINDS[0], KEYS),
    (crate::settings::KEYS_KINDS[1], KEYS),
    (crate::settings::KEYS_KINDS[2], KEYS),
    (crate::settings::KINDS[0], SETTINGS),
    (crate::settings::KINDS[1], SETTINGS),
    (crate::settings::KINDS[2], SETTINGS),
    (crate::settings::KINDS[3], SETTINGS),
    (crate::settings::PLACE_KINDS[0], PLACE),
    (crate::settings::PLACE_KINDS[1], PLACE),
    ("faces.add_photo", |lamp| lamp.photos().is_some()),
    ("face.list", |lamp| lamp.faces().is_some()),
    ("face.remove", |lamp| lamp.faces().is_some()),
    ("schedule.list", |lamp| lamp.schedule().is_some()),
    ("schedule.cancel", |lamp| lamp.schedule().is_some()),
    ("guard.on", |lamp| lamp.guard().is_some()),
    ("guard.off", |lamp| lamp.guard().is_some()),
    ("skills.make", MAKE_SKILL),
    ("skills.made", MAKE_SKILL),
    ("update.status", |lamp| lamp.update_page()),
    ("update.check", |lamp| lamp.update_page()),
    ("system.reboot", |lamp| lamp.restarts()),
    ("system.shutdown", |lamp| lamp.shuts_down()),
];

const WIFI: Switch = |lamp| lamp.changes().is_some_and(|c| c.wifi_on());
/// The switch, and a computer to make the skill on (fourteen-P15).
const MAKE_SKILL: Switch = |lamp| lamp.make_skill().is_some() && lamp.make_skill_ready();
const KEYS: Switch = |lamp| lamp.changes().is_some_and(|c| c.keys_on());
const SETTINGS: Switch = |lamp| lamp.changes().is_some();
const PLACE: Switch = |lamp| lamp.changes().is_some_and(|c| c.place_on());

/// Whether this lamp answers `kind` now, by its row in [`KINDS`]: the one
/// rule [`answer`] and `system.info`'s `can` both read (#2864), so the app
/// is never shown a row the lamp refuses.
pub fn on(lamp: &impl Lamp, kind: &str) -> bool {
    KINDS
        .iter()
        .any(|(name, switch)| *name == kind && switch(lamp))
}

/// `system.info`'s `can`: the kinds [`on`] now, read only.
pub fn can(lamp: &impl Lamp) -> Vec<&'static str> {
    KINDS
        .iter()
        .filter(|(_, switch)| switch(lamp))
        .map(|(name, _)| *name)
        .collect()
}

/// How long a preview is shown after the last one, as V1's: an abandoned
/// screen never leaves a colour behind.
pub const PREVIEW_HOLDS: std::time::Duration = std::time::Duration::from_secs(10);

/// The kinds answered only when they fail, as V1's: a drag sends a preview
/// five times a second, and a reply to each would flood `fd_channel`.
pub const QUIET_WHEN_DONE: [&str; 1] = ["led.resting.preview"];

/// Why `mic.set` is refused: only the person at the lamp may unmute it.
pub const MIC_SET_REFUSED: &str =
    "on this lamp the microphones are muted and unmuted with the button or the switch";

/// The answer to `kind` with `data`: its reply's `data`, or why not.
pub fn answer(lamp: &mut impl Lamp, kind: &str, data: &Value) -> Result<Option<Value>, String> {
    let some = |value: Value| Ok(Some(value));
    match kind {
        // `can`: the kinds the app may ask now (#2864); a lamp without it
        // is read as before.
        "system.info" => {
            let can = can(lamp);
            let mut info = json!({
                "versions": lamp.versions(),
                "network": lamp.network(),
                "host": lamp.host(),
                "can": can,
            });
            // `recovery` (#3020): only when it's on, so the app names the
            // hotspot only to a lamp that will open one.
            if lamp.recovery() {
                info["recovery"] = json!(true);
            }
            // `reset` (#2937): a lamp that can't tell says none, so the
            // app reads it as before, the 5 s hold.
            if let Some(reset) = lamp.reset() {
                info["reset"] = json!(reset);
            }
            some(info)
        }
        "system.version" => some(lamp.versions()),
        "system.network" => some(lamp.network()),
        "volume.get" => lamp.volume().map(|volume| Some(json!(volume))),
        "volume.set" => {
            let volume = data
                .get("volume")
                .and_then(Value::as_u64)
                .filter(|volume| *volume <= 100)
                .ok_or("volume must be a whole number from 0 to 100")?;
            lamp.set_volume(volume as u8)
                .map(|volume| Some(json!(volume)))
        }
        "mic.get" => lamp.mic().map(|mic| Some(json!(mic))),
        "mic.set" => Err(MIC_SET_REFUSED.to_owned()),
        "lamp.health" => lamp.health().map(Some),
        // The app's Light row (Fourth on #1396, the first builder's 10:2x on
        // #705): the person's own light, in V1's shape (`crate::light`).
        "led.resting.get" => lamp.light().map(|shown| Some(light::snapshot(&shown))),
        "led.resting.set" => {
            let now = lamp.light().ok();
            let wanted = light::chosen(data, now.as_ref())?;
            lamp.set_light(&wanted)
                .map(|shown| Some(light::snapshot(&shown)))
        }
        "led.resting.preview" => {
            let now = lamp.light().ok();
            let wanted = light::previewed(data, now.as_ref())?;
            lamp.preview_light(&wanted).map(|()| None)
        }
        // A text from the app (#1673): acked with its run, and put on the
        // bus for the voice; the answer goes back as `chat.event`.
        "chat.send" if on(lamp, kind) => {
            let now_ms = crate::chat::now_ms();
            let run = lamp.new_run(now_ms);
            let text = crate::chat::text_in(data, run)?;
            lamp.send_text(&text, now_ms)?;
            some(json!({"run_id": text.run_id, "session_id": text.session_id}))
        }
        // Health and crash reports (#1683): whether the person's yes stands,
        // since when and how many wait; and the person's own choice, from
        // their account, the only way to turn them on after setup.
        "reports.get" | "reports.set" if on(lamp, kind) => {
            let shared = lamp.reports().ok_or_else(|| not_here(kind))?;
            if kind == "reports.set" {
                let share = data
                    .get("share")
                    .and_then(Value::as_bool)
                    .ok_or("share must be true or false")?;
                let now = crate::reports::now_s();
                shared
                    .with(|reports| reports.set(share, now))
                    .map_err(|_| "the choice could not be kept".to_owned())?;
                shared.wake.notify_one();
            }
            some(shared.with(|reports| reports.answer()))
        }
        // The app's skills page (#1722): every skill on the lamp, its group,
        // whether it can be removed or turned off and what it may use. Read
        // only.
        "skills.list" if on(lamp, kind) => {
            let places = lamp.skills().ok_or_else(|| not_here(kind))?;
            crate::skills::list(&places).map(Some)
        }
        // Names and the time zone after setup (#1959): handed to root's
        // helper, answered `starting` now and its result later.
        // The Wi-Fi (#1959's second piece), with its own switch too.
        crate::settings::WIFI_KIND if on(lamp, kind) => {
            let changes = lamp.changes().ok_or_else(|| not_here(kind))?;
            changes.start_wifi(data)?;
            Ok(None)
        }
        // The app reached the lamp over its new network (#1959's P1): kept
        // only with this, within the helper's window, which starts when the
        // broker acks `wifi.joined` (#2617's P2-1 and P2-2).
        crate::settings::WIFI_CONFIRM_KIND if on(lamp, kind) => {
            let changes = lamp.changes().ok_or_else(|| not_here(kind))?;
            changes.confirm_wifi(data)?;
            Ok(None)
        }
        // A person's own keys (#2094), with LAMP_OWN_KEYS=on too: to root's
        // keys helper, which checks each once before it keeps it.
        kind if crate::settings::KEYS_KINDS.contains(&kind) && on(lamp, kind) => {
            let changes = lamp.changes().ok_or_else(|| not_here(kind))?;
            changes.start_keys(kind, data)?;
            Ok(None)
        }
        kind if crate::settings::KINDS.contains(&kind) && on(lamp, kind) => {
            let changes = lamp.changes().ok_or_else(|| not_here(kind))?;
            let line = crate::settings::line(kind, data)?;
            changes.start(kind, line);
            Ok(None)
        }
        // The owner's approximate place (#4333), with LAMP_OWNER_PLACE=on
        // too: rounded here, then to root's helper as the names go, which
        // rounds it again and keeps it beside the person's name. Its clear
        // is taken whenever the helper is here, the switch on or not, so a
        // place kept while it was on never outlives it (#4356); `can` lists
        // it only with the switch on.
        kind if crate::settings::PLACE_KINDS.contains(&kind)
            && (on(lamp, kind)
                || (kind == crate::settings::PLACE_CLEAR_KIND && lamp.changes().is_some())) =>
        {
            let changes = lamp.changes().ok_or_else(|| not_here(kind))?;
            let line = crate::settings::place_line(kind, data)?;
            changes.start(kind, line);
            Ok(None)
        }
        // A face from a photo the app sends (#1968): handed to the eyes,
        // which keep it unconfirmed until its person says yes at the lamp.
        // It comes over the lamp's own account channel only.
        "faces.add_photo" if on(lamp, kind) => {
            let place = lamp.photos().ok_or_else(|| not_here(kind))?;
            let (name, photo) = crate::photos::asked(data)?;
            crate::photos::add(&place, &name, &photo, crate::chat::now_ms()).map(Some)
        }
        // The faces the lamp knows, and removing one (#2192): names only,
        // and the eyes' own forget. Over the lamp's own account channel only.
        "face.list" if on(lamp, kind) => {
            let eyes = lamp.faces().ok_or_else(|| not_here(kind))?;
            crate::faces::list(&eyes).map(Some)
        }
        "face.remove" if on(lamp, kind) => {
            let eyes = lamp.faces().ok_or_else(|| not_here(kind))?;
            crate::faces::remove(&eyes, data).map(Some)
        }
        // The lamp's timers and reminders, for the app's Lamp 2 page
        // (#2195): only with `LAMP_APP_SCHEDULE=on`.
        "schedule.list" | "schedule.cancel" if on(lamp, kind) => {
            let adapter = lamp.schedule().ok_or_else(|| not_here(kind))?;
            if kind == "schedule.list" {
                crate::schedule::list(adapter, data)
            } else {
                crate::schedule::cancel(adapter, data)
            }
        }
        // An installed or made-here skill taken off on the person's tap
        // (#1722), never a core one: the page as it now is.
        "skills.remove" if on(lamp, kind) => {
            let places = lamp.skills().ok_or_else(|| not_here(kind))?;
            crate::skills::remove(&places, data).map(Some)
        }
        // A skill turned off or on on the person's tap (#2815), kept as
        // their own choice: the page as it now is.
        "skills.off" | "skills.on" if on(lamp, kind) => {
            let places = lamp.skills().ok_or_else(|| not_here(kind))?;
            crate::skills::turn(&places, data, kind == "skills.off").map(Some)
        }
        // The owner's yes to a skill's camera, given or taken back
        // (#3504's piece C), kept as off and on are: the page as it now is.
        "skills.camera" if on(lamp, kind) => {
            let places = lamp.skills().ok_or_else(|| not_here(kind))?;
            crate::skills::camera(&places, data).map(Some)
        }
        // Guard mode on or off from the owner's app (#2562), as
        // `skills.remove` goes: handed to lamp-skills' call socket, which
        // holds it to the watch's own rules, its mark among them.
        "guard.on" | "guard.off" if on(lamp, kind) => {
            let socket = lamp.guard().ok_or_else(|| not_here(kind))?;
            crate::guard::turn(&socket, kind)
        }
        // A skill typed in the owner's app (#2813): handed to lamp-voice,
        // which makes it as the voice's own; only a spoken yes at the lamp
        // puts it on, never anything from the app.
        // Taken on the switch alone: `can` waits for a computer as well
        // (fourteen-P15), but an app that asks anyway is told the voice's
        // own reason, as before.
        "skills.make" if lamp.make_skill().is_some() => {
            let socket = lamp.make_skill().ok_or_else(|| not_here(kind))?;
            crate::make_skill::make(&socket, data)
        }
        "skills.made" if lamp.make_skill().is_some() => {
            let socket = lamp.make_skill().ok_or_else(|| not_here(kind))?;
            crate::make_skill::made(&socket, data)
        }
        // A package from the Skills Store, from a URL (#1722), with
        // `LAMP_SKILLS_STORE=on` too: root's manage helper fetches it and
        // installs it only signed with the Store's key.
        "skills.install" if on(lamp, kind) => {
            let places = lamp.skills().ok_or_else(|| not_here(kind))?;
            crate::skills::install(&places, data).map(Some)
        }
        // The lamp's software update, for the app's Lamp 2 page (#2194):
        // only with `LAMP_APP_UPDATE=on`.
        "update.status" if on(lamp, kind) => crate::update::status(lamp.update_status()),
        // The app's Check for update (#4153): `serve`'s next round now, not
        // after its wait. Refused while a round runs and within a minute of
        // the last; the round keeps its own lock and quiet moment.
        "update.check" if on(lamp, kind) => {
            let status = crate::update::status(lamp.update_status())?.unwrap_or_default();
            crate::update::idle(&status)?;
            lamp.check_update(std::time::Instant::now())?;
            some(crate::update::started(status))
        }
        // A restart from the app (#2193): asked of setup's change helper,
        // never done here; it answers first and refuses under an update.
        "system.reboot" if on(lamp, kind) => lamp.restart().map(Some),
        // The lamp turned off from the app (#3697): asked of the same helper,
        // which answers first and runs `lamp-setup shutdown`; never here.
        "system.shutdown" if on(lamp, kind) => lamp.shut_down().map(Some),
        "" => Err("no kind".to_owned()),
        // `device.soft_reset` too: a destructive act from anywhere is the
        // owner's call (the first builder on #1396, 21:52).
        other => Err(not_here(other)),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A lamp in memory: what each answer reads, and the volumes asked.
    #[derive(Default)]
    pub struct Fake {
        pub raw: u8,
        pub ceiling: u8,
        pub set: Vec<u8>,
        /// The person's light the ring shows; none while it is not running.
        pub shown: Option<Light>,
        pub lights: Vec<Light>,
        pub previews: Vec<Light>,
        /// Texts (#1673): taken when `texting`; those sent; and the
        /// `chat.event`s' `data` it gives once a text has been sent.
        pub texting: bool,
        pub texts: Vec<lamp_msgs::ChatIn>,
        pub to_give: Vec<Value>,
        /// The skills' lines for the phone (`crate::unasked`): those to
        /// give, and those owed back.
        pub unasked: crate::unasked::Outbox,
        /// No line is given before this, if set: so a test's lines go after
        /// the connection's first publishes.
        pub unasked_from: Option<std::time::Instant>,
        /// Reports (#1683), with their switch.
        pub reports: Option<std::sync::Arc<crate::reports::Shared>>,
        /// The skills page (#1722), with its switch.
        pub skills: Option<crate::skills::Places>,
        /// Changes after setup (#1959), with their switch.
        pub changes: Option<std::sync::Arc<crate::settings::Changes>>,
        /// A will left with the broker (`LAMP_CLOUD_OFFLINE_WILL`).
        pub will: bool,
        /// The other switches `can` reads (#2864).
        pub photos: Option<crate::photos::Place>,
        pub faces: Option<crate::faces::Eyes>,
        pub schedule: Option<lamp_actions::schedule::Adapter>,
        pub guard: Option<std::path::PathBuf>,
        pub make_skill: Option<std::path::PathBuf>,
        pub make_skill_ready: bool,
        pub update_page: bool,
        /// The bus lamp-update's `update/status` is read from (#2194) and
        /// `update.check` asks on, and the checks asked (#4153).
        pub update_bus: Option<lamp_bus::Bus>,
        pub update_checks: crate::update::Checks,
        pub restart: bool,
        pub shutdown: bool,
        /// What `recovery` says (#3020).
        pub recovery: bool,
        /// What `reset` says (#2937).
        pub reset: Option<&'static str>,
    }

    impl Lamp for Fake {
        fn recovery(&self) -> bool {
            self.recovery
        }
        fn reset(&self) -> Option<&'static str> {
            self.reset
        }
        fn versions(&mut self) -> Value {
            json!({"lamp-voice": "0.1.0+abc"})
        }
        fn network(&mut self) -> Value {
            json!({"private_ip": "192.168.1.42", "interface": "wlan0"})
        }
        fn host(&mut self) -> Value {
            json!({"hostname": "lamp-7f72", "device_id": "d-42"})
        }
        fn volume(&mut self) -> Result<Volume, String> {
            Ok(Volume {
                volume: (u16::from(self.raw) * 100 / u16::from(self.ceiling.max(1))) as u8,
                raw: self.raw,
                max_volume: self.ceiling,
            })
        }
        fn set_volume(&mut self, volume: u8) -> Result<Volume, String> {
            self.set.push(volume);
            self.raw = (u16::from(volume) * u16::from(self.ceiling) / 100) as u8;
            self.volume()
        }
        fn mic(&mut self) -> Result<Mic, String> {
            Ok(Mic {
                muted: true,
                hw_switch_muted: Some(true),
                available: true,
                muted_by: Some(MutedBy::Button),
            })
        }
        fn health(&mut self) -> Result<Value, String> {
            Ok(json!({"level": "ok", "message": ""}))
        }
        fn light(&mut self) -> Result<Light, String> {
            self.shown
                .clone()
                .ok_or("the ring is not running".to_owned())
        }
        fn set_light(&mut self, light: &Light) -> Result<Light, String> {
            self.lights.push(light.clone());
            self.shown = Some(light.clone());
            self.light()
        }
        fn preview_light(&mut self, light: &Light) -> Result<(), String> {
            self.previews.push(light.clone());
            Ok(())
        }
        fn texting(&self) -> bool {
            self.texting
        }
        fn reports(&self) -> Option<std::sync::Arc<crate::reports::Shared>> {
            self.reports.clone()
        }
        fn skills(&self) -> Option<crate::skills::Places> {
            self.skills.clone()
        }
        fn changes(&self) -> Option<std::sync::Arc<crate::settings::Changes>> {
            self.changes.clone()
        }
        fn offline_will(&self) -> bool {
            self.will
        }
        fn photos(&self) -> Option<crate::photos::Place> {
            self.photos.clone()
        }
        fn faces(&self) -> Option<crate::faces::Eyes> {
            self.faces.clone()
        }
        fn schedule(&self) -> Option<&lamp_actions::schedule::Adapter> {
            self.schedule.as_ref()
        }
        fn guard(&self) -> Option<std::path::PathBuf> {
            self.guard.clone()
        }
        fn make_skill(&self) -> Option<std::path::PathBuf> {
            self.make_skill.clone()
        }
        fn make_skill_ready(&self) -> bool {
            self.make_skill_ready
        }
        fn update_page(&self) -> bool {
            self.update_page
        }
        fn update_status(&self) -> Option<lamp_bus::Message> {
            self.update_bus
                .as_ref()?
                .latest(lamp_msgs::UpdateStatus::TOPIC)
                .ok()
                .flatten()
        }
        fn check_update(&mut self, now: std::time::Instant) -> Result<(), String> {
            self.update_checks.ask(self.update_bus.as_ref(), now)
        }
        fn restarts(&self) -> bool {
            self.restart
        }
        fn shuts_down(&self) -> bool {
            self.shutdown
        }
        fn send_text(&mut self, text: &lamp_msgs::ChatIn, _now_ms: u64) -> Result<(), String> {
            self.texts.push(text.clone());
            Ok(())
        }
        fn unasked(&mut self, _now_ms: u64, give: bool) -> crate::unasked::Heard {
            crate::unasked::Heard {
                // The eyes' word is Real's to check: here each yes stands.
                due: if give
                    && self
                        .unasked_from
                        .is_none_or(|from| std::time::Instant::now() >= from)
                {
                    self.unasked.due_checked(|_| true)
                } else {
                    Vec::new()
                },
                lines: Vec::new(),
            }
        }
        fn owe_unasked(&mut self, pieces: Vec<crate::unasked::Piece>) {
            self.unasked.owe(pieces);
        }
        fn unasked_sent(&mut self) -> u64 {
            self.unasked.sent()
        }
        fn chat_events(&mut self, _now_ms: u64) -> Vec<Value> {
            if self.texts.is_empty() {
                Vec::new()
            } else {
                std::mem::take(&mut self.to_give)
            }
        }
    }

    fn lamp() -> Fake {
        Fake {
            raw: 60,
            ceiling: 80,
            shown: Some(crate::light::first()),
            ..Fake::default()
        }
    }

    #[test]
    fn the_kinds_this_lamp_has_are_answered_by_its_parts() {
        let mut lamp = lamp();
        let info = answer(&mut lamp, "system.info", &Value::Null)
            .unwrap()
            .unwrap();
        assert_eq!(info["host"]["hostname"], "lamp-7f72");
        assert_eq!(info["versions"]["lamp-voice"], "0.1.0+abc");
        assert_eq!(info["network"]["interface"], "wlan0");
        // #3020: `recovery` only while it's on.
        assert!(info.get("recovery").is_none(), "{info}");
        let mut recovering = Fake {
            recovery: true,
            ..Fake::default()
        };
        let told = answer(&mut recovering, "system.info", &Value::Null)
            .unwrap()
            .unwrap();
        assert_eq!(told["recovery"], true);
        // #2937: a lamp that can't tell its reset says nothing of it; one
        // that can says hold or none.
        assert!(info.get("reset").is_none(), "{info}");
        for said in ["hold", "none"] {
            let mut told = Fake {
                reset: Some(said),
                ..Fake::default()
            };
            let info = answer(&mut told, "system.info", &Value::Null)
                .unwrap()
                .unwrap();
            assert_eq!(info["reset"], said);
        }
        assert_eq!(
            answer(&mut lamp, "system.network", &Value::Null).unwrap(),
            Some(json!({"private_ip": "192.168.1.42", "interface": "wlan0"}))
        );
        assert_eq!(
            answer(&mut lamp, "volume.get", &Value::Null).unwrap(),
            Some(json!({"volume": 75, "raw": 60, "max_volume": 80}))
        );
        assert_eq!(
            answer(&mut lamp, "volume.set", &json!({"volume": 50})).unwrap(),
            Some(json!({"volume": 50, "raw": 40, "max_volume": 80}))
        );
        assert_eq!(lamp.set, [50]);
        assert_eq!(
            answer(&mut lamp, "mic.get", &Value::Null).unwrap(),
            Some(
                json!({"muted": true, "hw_switch_muted": true, "available": true, "muted_by": "button"})
            )
        );
        assert_eq!(
            answer(&mut lamp, "lamp.health", &Value::Null).unwrap(),
            Some(json!({"level": "ok", "message": ""}))
        );
    }

    /// The app's Light row (the first builder's 10:2x on #705): V1's
    /// envelopes, `docs/mqtt.md`'s own examples, on the person's light.
    #[test]
    fn the_light_row_reads_sets_and_previews_the_persons_light() {
        let mut lamp = lamp();
        let got = answer(&mut lamp, "led.resting.get", &Value::Null)
            .unwrap()
            .unwrap();
        assert_eq!(got["mode"], "default");
        assert_eq!(got["effective"]["color"], json!([51, 28, 0]));
        // A drag: each preview is shown, none kept, and none answered.
        assert_eq!(
            answer(
                &mut lamp,
                "led.resting.preview",
                &json!({"color": [20, 0, 10]})
            ),
            Ok(None)
        );
        assert_eq!(lamp.previews, [Light::new(true, "pink", 1).unwrap()]);
        assert!(lamp.lights.is_empty());
        // Save: kept, and the light read back in V1's snapshot.
        let saved = answer(
            &mut lamp,
            "led.resting.set",
            &json!({"mode": "custom", "color": [8, 4, 1]}),
        )
        .unwrap()
        .unwrap();
        assert_eq!(lamp.lights, [Light::new(true, "warm", 1).unwrap()]);
        assert_eq!(saved["mode"], "default");
        answer(
            &mut lamp,
            "led.resting.set",
            &json!({"mode": "custom", "color": [0, 0, 200]}),
        )
        .unwrap();
        let off = answer(&mut lamp, "led.resting.set", &json!({"mode": "off"}))
            .unwrap()
            .unwrap();
        assert_eq!(off["mode"], "off");
        assert_eq!(
            off["light"],
            json!({"on": false, "colour": "blue", "level": 4})
        );
        // Bad input is refused before anything is asked; a ring that is not
        // running is said.
        let asked = lamp.lights.len();
        for (kind, data) in [
            (
                "led.resting.set",
                json!({"mode": "custom", "color": [1, 2]}),
            ),
            ("led.resting.set", json!({"mode": "bright"})),
            ("led.resting.preview", json!({"color": [1, 2, 300]})),
        ] {
            assert!(answer(&mut lamp, kind, &data).is_err(), "{kind} {data}");
        }
        assert_eq!((lamp.lights.len(), lamp.previews.len()), (asked, 1));
        lamp.shown = None;
        assert_eq!(
            answer(&mut lamp, "led.resting.get", &Value::Null),
            Err("the ring is not running".to_owned())
        );
    }

    /// #1683: the app reads and sets the person's choice, only with the
    /// switch; a no deletes what waits.
    #[test]
    fn reports_are_read_and_set_from_the_app_only_with_their_switch() {
        let mut lamp = lamp();
        for kind in ["reports.get", "reports.set"] {
            assert_eq!(
                answer(&mut lamp, kind, &json!({"share": true})),
                Err(not_here(kind))
            );
        }
        let places = crate::reports::tests::places("answer");
        lamp.reports = Some(crate::reports::Shared::new(crate::reports::Reports::open(
            places.clone(),
        )));
        let got = answer(&mut lamp, "reports.get", &Value::Null)
            .unwrap()
            .unwrap();
        assert_eq!(
            got,
            json!({"share": false, "since": null, "queued": 0, "by": null, "held": false, "at": null})
        );
        let on = answer(&mut lamp, "reports.set", &json!({"share": true}))
            .unwrap()
            .unwrap();
        assert_eq!(on["share"], true);
        assert!(on["since"].is_string());
        for data in [json!({"share": "true"}), json!({"share": 1}), json!({})] {
            assert_eq!(
                answer(&mut lamp, "reports.set", &data),
                Err("share must be true or false".to_owned()),
                "{data}"
            );
        }
        let off = answer(&mut lamp, "reports.set", &json!({"share": false}))
            .unwrap()
            .unwrap();
        assert_eq!((&off["share"], &off["queued"]), (&json!(false), &json!(1)));
        // How and when, for the app's row: the app's own no, never held.
        assert_eq!((&off["by"], &off["held"]), (&json!("app"), &json!(false)));
        assert_eq!(off["at"], off["since"]);
        assert!(off["at"].is_string());
    }

    /// #1722: the skills page is answered only with its switch, and
    /// without it refused as every kind this lamp lacks.
    #[test]
    fn the_skills_are_listed_for_the_app_only_with_their_switch() {
        let mut lamp = lamp();
        assert_eq!(
            answer(&mut lamp, "skills.list", &Value::Null),
            Err("not on this lamp: skills.list".to_owned())
        );
        let places = crate::skills::tests::places("answer");
        crate::skills::tests::helper(&places);
        crate::skills::tests::skill(
            &places,
            "dance",
            "Dance",
            "",
            "kind = \"core\"\n[permissions]\nbody = true",
        );
        lamp.skills = Some(places);
        let listed = answer(&mut lamp, "skills.list", &Value::Null)
            .unwrap()
            .unwrap();
        assert_eq!(listed["version"], 1);
        assert_eq!(listed["skills"][0]["name"], "dance");
        assert_eq!(listed["skills"][0]["can_remove"], false);
        // A core skill is not removed, and an installed one is.
        assert_eq!(
            answer(&mut lamp, "skills.remove", &json!({"name": "dance"})),
            Err("dance is a core skill: it comes with the lamp, so it can't be removed".to_owned())
        );
        crate::skills::tests::skill(
            lamp.skills.as_ref().unwrap(),
            "tutor",
            "Desk tutor",
            "",
            "[permissions]\nbody = true",
        );
        let after = answer(&mut lamp, "skills.remove", &json!({"name": "tutor"}))
            .unwrap()
            .unwrap();
        // The Store's switch off, with the page's on: an install is a kind
        // this lamp lacks, and nothing is asked of the helper.
        lamp.skills.as_mut().unwrap().store = false;
        assert_eq!(
            answer(
                &mut lamp,
                "skills.install",
                &json!({"url": "https://store.example/x.tar"})
            ),
            Err("not on this lamp: skills.install".to_owned())
        );
        lamp.skills.as_mut().unwrap().store = true;
        assert_eq!(
            answer(
                &mut lamp,
                "skills.install",
                &json!({"url": "http://x/y.tar"})
            ),
            Err(lamp_skills::package::NOT_A_STORE_URL.to_owned())
        );
        assert_eq!(after["skills"].as_array().unwrap().len(), 1);
        assert_eq!(after["skills"][0]["name"], "dance");
        // Off, removing is refused as any kind this lamp lacks.
        lamp.skills = None;
        assert_eq!(
            answer(&mut lamp, "skills.remove", &json!({"name": "dance"})),
            Err("not on this lamp: skills.remove".to_owned())
        );
        // The other skills kinds are not built: refused as before.
        for kind in ["skills.catalog", "skills.install"] {
            assert_eq!(
                answer(&mut lamp, kind, &json!({"name": "dance"})),
                Err(not_here(kind))
            );
        }
    }

    #[test]
    fn a_volume_out_of_range_is_refused_before_anything_is_asked() {
        let mut lamp = lamp();
        for data in [
            json!({"volume": 101}),
            json!({"volume": -1}),
            json!({"volume": "90"}),
            json!({}),
            Value::Null,
        ] {
            assert!(answer(&mut lamp, "volume.set", &data).is_err(), "{data}");
        }
        assert!(lamp.set.is_empty());
    }

    #[test]
    fn unmuting_from_the_app_and_what_this_lamp_lacks_are_refused_plainly() {
        let mut lamp = lamp();
        assert_eq!(
            answer(&mut lamp, "mic.set", &json!({"muted": false})),
            Err(MIC_SET_REFUSED.to_owned())
        );
        for kind in [
            "device.soft_reset",
            "device.rename",
            "add_channel",
            "skills.install_store",
            "face.enroll",
            "led.state.set",
            "system.reboot",
            "system.shutdown",
            // Without LAMP_GUARD_FROM_APP=on (#2562).
            "guard.on",
            "guard.off",
            // Without LAMP_APP_MAKE_SKILL=on (#2813).
            "skills.make",
            "skills.made",
        ] {
            assert_eq!(
                answer(&mut lamp, kind, &Value::Null),
                Err(format!("not on this lamp: {kind}"))
            );
        }
        assert!(answer(&mut lamp, "", &Value::Null).is_err());
    }

    /// #2864: `system.info`'s `can` holds each switch's kinds only while it
    /// Making a skill (fourteen-P15): with the switch on and no computer
    /// yet, `can` names neither kind, but a make asked anyway still reaches
    /// lamp-voice for its own answer; with a computer, both are named.
    #[test]
    fn make_skill_is_named_only_while_a_computer_is_ready() {
        let named = |lamp: &mut Fake| -> Vec<String> {
            let info = answer(lamp, "system.info", &Value::Null).unwrap().unwrap();
            serde_json::from_value(info["can"].clone()).unwrap()
        };
        let mut lamp = lamp();
        lamp.make_skill = Some(std::path::PathBuf::from("/nowhere/make.sock"));
        let can = named(&mut lamp);
        assert!(
            !can.iter().any(|kind| kind.starts_with("skills.ma")),
            "{can:?}"
        );
        assert_eq!(
            answer(&mut lamp, "skills.made", &json!({"request_id": "r1"})),
            Err(crate::make_skill::UNREACHED.to_owned()),
            "asked anyway: lamp-voice is asked, here unreached"
        );
        lamp.make_skill_ready = true;
        let can = named(&mut lamp);
        assert!(can.contains(&"skills.make".to_owned()) && can.contains(&"skills.made".to_owned()));
    }

    /// is on, as [`answer`] takes them; the always-answered kinds are in it
    /// with every switch off.
    #[test]
    fn system_info_says_each_switchs_kinds_only_while_it_is_on() {
        let switches: Vec<Switch> = vec![
            ("texting", |f| f.texting = true, &["chat.send"]),
            (
                "reports",
                |f| {
                    let places = crate::reports::tests::places("can");
                    f.reports = Some(crate::reports::Shared::new(crate::reports::Reports::open(
                        places,
                    )));
                },
                &["reports.get", "reports.set"],
            ),
            (
                "skills page",
                |f| f.skills = Some(places_of(false)),
                &[
                    "skills.list",
                    "skills.remove",
                    "skills.off",
                    "skills.on",
                    "skills.camera",
                ],
            ),
            (
                "skills store",
                |f| f.skills = Some(places_of(true)),
                &[
                    "skills.list",
                    "skills.remove",
                    "skills.off",
                    "skills.on",
                    "skills.camera",
                    "skills.install",
                ],
            ),
            (
                "settings",
                |f| f.changes = changes_of(false, false),
                &crate::settings::KINDS,
            ),
            (
                "wifi",
                |f| f.changes = changes_of(true, false),
                &[
                    "timezone.set",
                    "person.name.set",
                    "device.rename",
                    "language.set",
                    "wifi.set",
                    "wifi.confirm",
                ],
            ),
            (
                "keys",
                |f| f.changes = changes_of(false, true),
                &[
                    "timezone.set",
                    "person.name.set",
                    "device.rename",
                    "language.set",
                    "keys.set",
                    "keys.remove",
                    "keys.get",
                ],
            ),
            (
                "owner place",
                |f| {
                    f.changes =
                        changes_of(false, false).map(|changes| changes.taking_the_place(true))
                },
                &[
                    "timezone.set",
                    "person.name.set",
                    "device.rename",
                    "language.set",
                    "owner.place",
                    "owner.place.clear",
                ],
            ),
            (
                "photos",
                |f| {
                    f.photos = Some(crate::photos::Place {
                        dir: std::path::PathBuf::from("/nowhere/photos"),
                        socket: std::path::PathBuf::from("/nowhere/eyes.sock"),
                    })
                },
                &["faces.add_photo"],
            ),
            (
                "faces page",
                |f| {
                    f.faces = Some(crate::faces::Eyes {
                        socket: std::path::PathBuf::from("/nowhere/eyes.sock"),
                        memory: None,
                    })
                },
                &["face.list", "face.remove"],
            ),
            (
                "schedule",
                |f| {
                    f.schedule = Some(
                        lamp_actions::schedule::Adapter::new(std::path::PathBuf::from(
                            "/run/lampos-schedule/call.sock",
                        ))
                        .unwrap(),
                    )
                },
                &["schedule.list", "schedule.cancel"],
            ),
            (
                "guard",
                |f| f.guard = Some(std::path::PathBuf::from("/nowhere/call.sock")),
                &["guard.on", "guard.off"],
            ),
            (
                "make skill",
                |f| {
                    f.make_skill = Some(std::path::PathBuf::from("/nowhere/make.sock"));
                    f.make_skill_ready = true;
                },
                &["skills.make", "skills.made"],
            ),
            (
                "update page",
                |f| f.update_page = true,
                &["update.status", "update.check"],
            ),
            ("restart", |f| f.restart = true, &["system.reboot"]),
            ("shutdown", |f| f.shutdown = true, &["system.shutdown"]),
        ];
        let always = [
            "system.info",
            "system.version",
            "system.network",
            "volume.get",
            "volume.set",
            "mic.get",
            "lamp.health",
            "led.resting.get",
            "led.resting.set",
            "led.resting.preview",
        ];
        let said = |lamp: &mut Fake| -> Vec<String> {
            let info = answer(lamp, "system.info", &Value::Null).unwrap().unwrap();
            serde_json::from_value(info["can"].clone()).unwrap()
        };
        let mut off = lamp();
        assert_eq!(said(&mut off), always, "every switch off");
        for (name, turn_on, kinds) in switches {
            let mut lamp = lamp();
            for kind in kinds {
                assert!(!on(&lamp, kind), "{name} off: {kind}");
                assert_eq!(
                    answer(&mut lamp, kind, &Value::Null),
                    Err(not_here(kind)),
                    "{name} off"
                );
            }
            turn_on(&mut lamp);
            let mut wanted: Vec<&str> = always.to_vec();
            wanted.extend(
                KINDS
                    .iter()
                    .map(|(kind, _)| *kind)
                    .filter(|kind| kinds.contains(kind)),
            );
            assert_eq!(said(&mut lamp), wanted, "{name} on");
            // Read only: asking again says the same.
            assert_eq!(said(&mut lamp), wanted, "{name} read again");
        }
    }

    /// #4153: `update.check` is answered only with `LAMP_APP_UPDATE=on`,
    /// with `update.status`'s object and `"check": "started"` once a
    /// running lamp-update has the check. Refused, with what the app shows,
    /// while a round runs, within a minute of the last, and when no
    /// lamp-update reads checks; nothing is sent then.
    #[test]
    fn the_app_asks_for_a_round_now_between_rounds_and_once_a_minute() {
        let root = tempfile::tempdir().unwrap();
        let bus = lamp_bus::Bus::new(root.path()).unwrap();
        let mut lamp = Fake {
            update_bus: Some(lamp_bus::Bus::new(root.path()).unwrap()),
            ..lamp()
        };
        assert_eq!(
            answer(&mut lamp, "update.check", &json!({})),
            Err(not_here("update.check"))
        );
        lamp.update_page = true;
        // No lamp-update reads checks (an older one, or none running): said,
        // and not counted as asked.
        assert_eq!(
            answer(&mut lamp, "update.check", &json!({})),
            Err(crate::update::NOT_LISTENING.to_owned())
        );
        let mut update = bus
            .subscribe_typed(
                lamp_msgs::UpdateCheck::TOPIC,
                "update",
                lamp_msgs::UpdateCheck::TYPE,
            )
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(250));
        let started = answer(&mut lamp, "update.check", &json!({}))
            .unwrap()
            .unwrap();
        assert_eq!(
            started,
            json!({"running": null, "now": "unknown", "last": null, "stale": false, "check": "started"})
        );
        let heard = update
            .recv(std::time::Duration::from_secs(1))
            .unwrap()
            .expect("one check on the bus");
        assert_eq!(
            heard.header.type_name.as_deref(),
            Some(lamp_msgs::UpdateCheck::TYPE)
        );
        assert_eq!(
            answer(&mut lamp, "update.check", &json!({})),
            Err(crate::update::TOO_SOON.to_owned())
        );
        // A round under way is said first, whatever the minute.
        let mut says = bus
            .publisher_declared(
                lamp_msgs::UpdateStatus::TOPIC,
                &lamp_msgs::UpdateStatus::declaration(),
                "update",
            )
            .unwrap();
        says.publish_json(
            0,
            &lamp_msgs::UpdateStatus {
                running: None,
                now: "switching".into(),
                last: None,
            },
        )
        .unwrap();
        assert_eq!(
            answer(&mut lamp, "update.check", &json!({})),
            Err(crate::update::RUNNING.to_owned())
        );
        assert!(
            update
                .recv(std::time::Duration::from_millis(100))
                .unwrap()
                .is_none(),
            "a refused ask sends nothing"
        );
        // Off again: the kind is not here, as before.
        lamp.update_page = false;
        assert_eq!(
            answer(&mut lamp, "update.check", &json!({})),
            Err(not_here("update.check"))
        );
    }

    /// A switch's name, how it is turned on, and the kinds it brings.
    type Switch = (&'static str, fn(&mut Fake), &'static [&'static str]);

    fn places_of(store: bool) -> crate::skills::Places {
        crate::skills::Places {
            folder: std::path::PathBuf::from("/nowhere/skills"),
            off: std::path::PathBuf::from("/nowhere/off"),
            manage: std::path::PathBuf::from("/nowhere/manage.sock"),
            store,
            addons: false,
        }
    }

    fn changes_of(wifi: bool, keys: bool) -> Option<std::sync::Arc<crate::settings::Changes>> {
        let wait = std::time::Duration::from_secs(1);
        Some(crate::settings::Changes::with_helpers(
            "/nowhere/settings.sock",
            wait,
            wifi.then(|| (std::path::PathBuf::from("/nowhere/wifi.sock"), wait)),
            keys.then(|| (std::path::PathBuf::from("/nowhere/keys.sock"), wait)),
        ))
    }
}
