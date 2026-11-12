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
/// chose; absent from a voice that does not say it. `hears_name`
/// (migrate-P2, `LAMP_SLEEP_HEARS_NAME`), only while true: asleep from a
/// hand's hold and still listening for its name alone, so the app can say
/// "asleep, wakes to its name", never "muted".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Mic {
    pub muted: bool,
    pub hw_switch_muted: Option<bool>,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted_by: Option<MutedBy>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hears_name: bool,
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
    /// `signed_out` while the lamp's login is no longer accepted
    /// (`LAMP_SIGNED_OUT`, "Signed out" in `docs/cloud.md`). None otherwise.
    fn account(&mut self) -> Option<&'static str> {
        None
    }
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
    /// Whether the skills' lines on `messages/out` go up (`crate::unasked`):
    /// with texts taken, or with `LAMP_SKILL_MESSAGES=on` alone (#1685),
    /// which takes no text and carries nothing else. Off unless a lamp says
    /// so.
    fn skill_lines(&self) -> bool {
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
    /// while [`Lamp::skill_lines`]: those heard by `now_ms` are taken off the bus, and
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

    /// lamp-tasks' app socket, for signing the lamp's Harness in from the
    /// owner's app (`LAMP_TASKS_SIGN_IN_APP=on`, #5198,
    /// `crate::harness_sign_in`).
    fn harness_sign_in(&self) -> Option<crate::harness_sign_in::Place> {
        None
    }

    /// When the setup walk handed out its done line, in Unix ms, from the
    /// voice's `onboarding/pending` heartbeat (#5812): none while unknown.
    fn onboarding(&self) -> Option<u64> {
        None
    }

    /// The file the app's `onboarding.done` acked is kept in
    /// (`crate::onboarding_done`): there only with
    /// `LAMP_APP_ONBOARDING_DONE=on` and a state folder.
    fn onboarding_done(&self) -> Option<std::path::PathBuf> {
        None
    }

    /// The setup walk's id and its steps ended, from the voice's
    /// `onboarding/pending` heartbeat (#5979): none without an id.
    fn onboarding_steps(&self) -> Option<(u64, Vec<lamp_msgs::OnboardingStepEnded>)> {
        None
    }

    /// The app's open onboarding page (#6076, `crate::onboarding_page`).
    fn onboarding_page(&mut self, data: &Value) -> Result<Value, String> {
        crate::onboarding_page::PAGES
            .lock()
            .map_err(|_| "the page cannot be kept".to_owned())?
            .open(data, crate::onboarding_page::Seen::default(), |_| None)
    }

    /// The open page's line due now handed to the voice, and the reply's
    /// `data` when the page is done with it (#6076, #6353).
    fn onboarding_page_done(&mut self) -> Option<Value> {
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

    /// lamp-voice's app-make socket, for a skill written in the owner's
    /// app in their own words (`LAMP_SKILL_WRITTEN=on` with the skills
    /// page, `crate::make_skill::write`).
    fn write_skill(&self) -> Option<std::path::PathBuf> {
        None
    }

    /// The Skills Store's owner upload and this lamp's key, for a made
    /// skill sent to the Store (`LAMP_SKILLS_SHARE=on` with the skills page,
    /// fourteen-P13, `crate::share`).
    fn share(&self) -> Option<crate::share::Store> {
        None
    }

    /// A skill added from a file in the owner's app
    /// (`LAMP_SKILL_UPLOAD=on` with the skills page, `crate::upload`).
    fn upload(&self) -> Option<std::sync::Arc<crate::upload::Uploads>> {
        None
    }

    /// The lamp's timers and reminders (`LAMP_APP_SCHEDULE=on`, #2195,
    /// `crate::schedule`): there only with the switch.
    fn schedule(&self) -> Option<&lamp_actions::schedule::Adapter> {
        None
    }

    /// The owner's face expressions and room sounds yeses from the app
    /// (`LAMP_APP_CONSENT=on`, #5500, `crate::consent`): there only with the
    /// switch.
    fn consent(&self) -> Option<crate::consent::Place> {
        None
    }

    /// The person's own services linked from the app (`LAMP_SERVICES=on`,
    /// Queue #4710, `crate::services`): there only with the switch.
    fn services(&self) -> Option<std::sync::Arc<crate::services::Services>> {
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
    /// With `retry` set to a refused build's sequence (#6266): lamp-update
    /// drops that refusal before the round.
    fn check_update(
        &mut self,
        _now: std::time::Instant,
        _retry: Option<u64>,
    ) -> Result<(), String> {
        Err(not_here("update.check"))
    }

    /// Whether the app may see and install the lamp's agents
    /// (`LAMP_APP_ENGINES=on` with `LAMP_APP_UPDATE=on`, #6240,
    /// `crate::update`).
    fn engines_page(&self) -> bool {
        false
    }

    /// Asks `lamp-update serve` to install `engine` at its next round
    /// (`engines.install`, #6240, `crate::update::Checks::ask_install`), at
    /// `now`: at most once a minute, and only when a running lamp-update
    /// reads it.
    fn install_engine(&mut self, _engine: &str, _now: std::time::Instant) -> Result<(), String> {
        Err(not_here("engines.install"))
    }

    /// lamp-tasks' app socket, where the lamp's default agent is chosen
    /// and read (`engines.default`, #6492, `crate::engine_default`).
    fn tasks_app_socket(&self) -> std::path::PathBuf {
        crate::harness_sign_in::SOCKET.into()
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

    /// lamp-control's admin socket and this lamp's device id, for the
    /// developer door from the owner's app (`--control-admin-socket`,
    /// #5299, `crate::control`).
    fn control(&self) -> Option<crate::control::Door> {
        None
    }
}

/// Whether a kind is answered on `lamp` now: its switch, read without
/// asking any of the lamp's parts.
type Switch = fn(&dyn Lamp) -> bool;

const ALWAYS: Switch = |_| true;

/// Every kind the app may ask that this lamp can answer, with its switch,
/// in the order `system.info`'s `can` lists them (#2864).
pub const KINDS: [(&str, Switch); 69] = [
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
    // The owner's face expressions and room sounds yeses (#5500), as the
    // reports' choice: both kinds with the switch.
    (crate::consent::KINDS[0], CONSENT),
    (crate::consent::KINDS[1], CONSENT),
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
    (crate::settings::KINDS[4], SETTINGS),
    (crate::settings::KINDS[5], SETTINGS),
    (crate::settings::PLACE_KINDS[0], PLACE),
    (crate::settings::PLACE_KINDS[1], PLACE),
    (crate::settings::VOICE_KINDS[0], VOICE),
    (crate::settings::VOICE_KINDS[1], VOICE),
    (crate::settings::VOICE_GET_KIND, VOICE),
    ("faces.add_photo", |lamp| lamp.photos().is_some()),
    ("face.list", |lamp| lamp.faces().is_some()),
    ("face.remove", |lamp| lamp.faces().is_some()),
    ("schedule.list", |lamp| lamp.schedule().is_some()),
    ("schedule.cancel", |lamp| lamp.schedule().is_some()),
    ("guard.on", |lamp| lamp.guard().is_some()),
    ("guard.off", |lamp| lamp.guard().is_some()),
    ("skills.make", MAKE_SKILL),
    ("skills.made", MADE_SKILL),
    ("skills.write", WRITE_SKILL),
    (crate::share::KINDS[0], SHARE),
    (crate::share::KINDS[1], SHARE),
    (crate::share::KINDS[2], SHARE),
    (crate::upload::KIND, |lamp| lamp.upload().is_some()),
    ("update.status", |lamp| lamp.update_page()),
    ("update.check", |lamp| lamp.update_page()),
    // The lamp's agents (#6240): a missing one installed from the app.
    ("engines.install", |lamp| lamp.engines_page()),
    // The lamp's default agent, chosen in the app (#6492).
    (crate::engine_default::KIND, |lamp| lamp.engines_page()),
    ("system.reboot", |lamp| lamp.restarts()),
    ("system.shutdown", |lamp| lamp.shuts_down()),
    // The person's own services (Queue #4710): `services` says
    // `connector.set.<code>` and `connector.remove.<code>` are taken, and
    // both rows wait for lamp-services' admin socket as well.
    (crate::services::CAN, SERVICES),
    (crate::services::LIST_KIND, SERVICES),
    // Harness signed in from the app (#5198): the code shown there. The
    // feature's word first, as `services`: an app shows the screen only
    // on a lamp that names it.
    (crate::harness_sign_in::CAN, HARNESS_SIGN_IN),
    (crate::harness_sign_in::KINDS[0], HARNESS_SIGN_IN),
    (crate::harness_sign_in::KINDS[1], HARNESS_SIGN_IN),
    (crate::harness_sign_in::KINDS[2], HARNESS_SIGN_IN),
    // The developer door (#5299): named only while lamp-control's admin
    // socket is there, so the app never offers a door the lamp can't open.
    (crate::control::KINDS[0], CONTROL),
    (crate::control::KINDS[1], CONTROL),
    (crate::control::KINDS[2], CONTROL),
    (crate::control::KINDS[3], CONTROL),
    (crate::control::KINDS[4], CONTROL),
    (crate::control::KINDS[5], CONTROL),
];

const CONSENT: Switch = |lamp| lamp.consent().is_some();
const HARNESS_SIGN_IN: Switch = |lamp| lamp.harness_sign_in().is_some();

const WIFI: Switch = |lamp| lamp.changes().is_some_and(|c| c.wifi_on());
/// The switch, and a computer to make the skill on (fourteen-P15).
const MAKE_SKILL: Switch = |lamp| lamp.make_skill().is_some() && lamp.make_skill_ready();
/// A skill written in the app: its switch alone, since no computer and no
/// model writes it.
const WRITE_SKILL: Switch = |lamp| lamp.write_skill().is_some();
/// What became of the app's ask, made or written.
const MADE_SKILL: Switch = |lamp| MAKE_SKILL(lamp) || WRITE_SKILL(lamp);
/// The skills page and the Store's upload (fourteen-P13).
const SHARE: Switch = |lamp| lamp.skills().is_some() && lamp.share().is_some();
const KEYS: Switch = |lamp| lamp.changes().is_some_and(|c| c.keys_on());
const SETTINGS: Switch = |lamp| lamp.changes().is_some();
const PLACE: Switch = |lamp| lamp.changes().is_some_and(|c| c.place_on());
const VOICE: Switch = |lamp| lamp.changes().is_some_and(|c| c.voice_on());
/// The switch, and lamp-services there to ask.
const SERVICES: Switch = |lamp| lamp.services().is_some_and(|s| s.here());
/// The socket given, and lamp-control there to ask.
const CONTROL: Switch = |lamp| lamp.control().is_some_and(|door| door.here());

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
            // `developer` (App Only's C): only while lamp-control's admin socket is
            // there (developer access on); whether a camera, microphone or speaker
            // stream is usable, so the app's grants screen can disable that tick.
            if let Some(door) = lamp.control()
                && let Some(developer) = crate::control::developer(&door)
            {
                info["developer"] = developer;
            }
            // `reset` (#2937): a lamp that can't tell says none, so the
            // app reads it as before, the 5 s hold.
            if let Some(reset) = lamp.reset() {
                info["reset"] = json!(reset);
            }
            // `account` (`LAMP_SIGNED_OUT`): only while signed out, so a
            // lamp signed in says what it always said.
            if let Some(account) = lamp.account() {
                info["account"] = json!(account);
            }
            // `onboarding_done_at` (#5812): the walk's end in Unix ms, only
            // when known, so an app that missed `onboarding.done` reads it.
            if let Some(at) = lamp.onboarding() {
                info["onboarding_done_at"] = json!(at);
            }
            // `onboarding_done_supported` (#5812, Mobile's ask): this lamp
            // will send `onboarding.done`, so the app may wait for it. Only
            // with the switch on (and a state folder); none otherwise.
            if lamp.onboarding_done().is_some() {
                info["onboarding_done_supported"] = json!(true);
                // And `onboarding.step` as each walk step ends (#5979),
                // behind the same switch.
                info["onboarding_step_supported"] = json!(true);
                // And `onboarding.page`, its page done pushed (#6076).
                info["onboarding_page_supported"] = json!(true);
                // Which page contract (#6366): 2 is per-tap Add and `done`.
                info["onboarding_page_version"] = json!(crate::onboarding_page::VERSION);
            }
            some(info)
        }
        "system.version" => some(lamp.versions()),
        // The app's open onboarding page (#6076), with `onboarding.done`'s
        // switch.
        crate::onboarding_page::KIND if lamp.onboarding_done().is_some() => {
            lamp.onboarding_page(data).map(Some)
        }
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
        "lamp.health" => lamp.health().map(|mut health| {
            if let Some(account) = lamp.account() {
                health["account"] = json!(account);
            }
            Some(health)
        }),
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
        // The owner's two yeses from the app (#5500): read where they are
        // kept, and set through root's helper, which answers at once; the
        // reply is what the files say after it. Room sounds only on a lamp
        // with no slide, where lamp-ears reads it.
        kind if crate::consent::KINDS.contains(&kind) && on(lamp, kind) => {
            let place = lamp.consent().ok_or_else(|| not_here(kind))?;
            if kind == crate::consent::KINDS[0] {
                some(crate::consent::get(&place))
            } else {
                crate::consent::set(&place, data).map(Some)
            }
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
        // The owner's chosen voice and character words (#5394), with
        // LAMP_CHOSEN_VOICE=on too: checked here, then to root's helper as
        // the names go, which checks them again and keeps them.
        kind if crate::settings::VOICE_KINDS.contains(&kind) && on(lamp, kind) => {
            let changes = lamp.changes().ok_or_else(|| not_here(kind))?;
            let line = crate::settings::voice_line(kind, data)?;
            changes.start(kind, line);
            Ok(None)
        }
        // What the lamp keeps now (#5394's follow-up), read where the
        // helper writes it, as the language is: nothing is asked of it.
        crate::settings::VOICE_GET_KIND if on(lamp, kind) => {
            let file = std::path::Path::new(lamp_msgs::chosen_voice::VOICE_FILE);
            let dir = file.parent().ok_or_else(|| not_here(kind))?;
            Ok(Some(crate::settings::chosen_now(dir)))
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
            crate::faces::remove(&eyes, data, lamp.schedule()).map(Some)
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
        "skills.made" if lamp.make_skill().is_some() || lamp.write_skill().is_some() => {
            let socket = lamp
                .make_skill()
                .or_else(|| lamp.write_skill())
                .ok_or_else(|| not_here(kind))?;
            let told = crate::make_skill::made(&socket, data)?;
            // A made skill that needs a connector not linked: told to the
            // app once a make (`LAMP_SKILL_LINK_APP`).
            if lamp.skills().is_some_and(|places| places.link.is_some())
                && let Some(told) = &told
            {
                crate::skills::note_made(told);
            }
            Ok(told)
        }
        // A skill written in the app (`LAMP_SKILL_WRITTEN=on`): its text
        // goes to lamp-voice as written, kept there with no model; only a
        // spoken yes at the lamp puts it on.
        "skills.write" if on(lamp, kind) => {
            let socket = lamp.write_skill().ok_or_else(|| not_here(kind))?;
            crate::make_skill::write(&socket, data)
        }
        // A made skill sent to the Skills Store (fourteen-P13): packed here,
        // only a skill made on this lamp and on, and uploaded with the
        // lamp's key; a reviewer reads it before anyone can add it.
        "skills.share" if on(lamp, kind) => {
            let (places, store) = lamp
                .skills()
                .zip(lamp.share())
                .ok_or_else(|| not_here(kind))?;
            crate::share::share(&store, &places, data, &std::env::temp_dir()).map(Some)
        }
        "skills.shared" if on(lamp, kind) => {
            let store = lamp.share().ok_or_else(|| not_here(kind))?;
            crate::share::shared(&store).map(Some)
        }
        "skills.unshare" if on(lamp, kind) => {
            let store = lamp.share().ok_or_else(|| not_here(kind))?;
            crate::share::unshare(&store, data).map(Some)
        }
        // A skill added from a file in the owner's app (`LAMP_SKILL_UPLOAD`),
        // `skills.upload.<upload_id>`: no file inline. Taken `starting`; the
        // file fetched from the backend's message, checked and handed to
        // lamp-voice on a thread of its own; its result follows once a
        // spoken yes put it on, or it was refused or left off.
        kind if crate::upload::id_of(kind).is_some() && on(lamp, crate::upload::KIND) => {
            let uploads = lamp.upload().ok_or_else(|| not_here(kind))?;
            uploads.start(kind)?;
            Ok(None)
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
        "update.status" if on(lamp, kind) => {
            let mut status = crate::update::status(lamp.update_status(), lamp.engines_page())?;
            // The default agent beside the agents (#6492), when lamp-tasks
            // says one; never a failure of the status.
            if let Some(status) = status.as_mut() {
                crate::engine_default::add_to(status, &lamp.tasks_app_socket());
            }
            Ok(status)
        }
        // The app's Check for update (#4153): `serve`'s next round now, not
        // after its wait. Refused while a round runs and within a minute of
        // the last; the round keeps its own lock and quiet moment.
        "update.check" if on(lamp, kind) => {
            let status = crate::update::status(lamp.update_status(), lamp.engines_page())?
                .unwrap_or_default();
            crate::update::idle(&status)?;
            // The owner's retry of a refused build (#6266): its sequence, a whole
            // number, which lamp-update drops the refusal of before the round.
            let retry = match data.get("retry") {
                None | Some(Value::Null) => None,
                Some(value) => Some(
                    value
                        .as_u64()
                        .ok_or("retry must be the refused build's sequence, a whole number")?,
                ),
            };
            lamp.check_update(std::time::Instant::now(), retry)?;
            some(crate::update::started(status))
        }
        // The app's install of a missing agent (#6240): only one lamp-update
        // says is missing or failed, at most once a minute; the install
        // waits for lamp-update's next round and keeps its own rules.
        "engines.install" if on(lamp, kind) => {
            let engine = data["engine"]
                .as_str()
                .ok_or("engine must be the agent's name")?;
            let status = crate::update::status(lamp.update_status(), true)?.unwrap_or_default();
            crate::update::installable(&status, engine)?;
            lamp.install_engine(engine, std::time::Instant::now())?;
            some(crate::update::install_started())
        }
        // The lamp's default agent chosen in the app (#6492): the name
        // checked here, the choice lamp-tasks' own.
        crate::engine_default::KIND if on(lamp, kind) => {
            crate::engine_default::choose(&lamp.tasks_app_socket(), data).map(Some)
        }
        // A restart from the app (#2193): asked of setup's change helper,
        // never done here; it answers first and refuses under an update.
        "system.reboot" if on(lamp, kind) => lamp.restart().map(Some),
        // The lamp turned off from the app (#3697): asked of the same helper,
        // which answers first and runs `lamp-setup shutdown`; never here.
        "system.shutdown" if on(lamp, kind) => lamp.shut_down().map(Some),
        // A service linked or unlinked from the app (Queue #4710): taken on
        // the switch alone, answered `starting` now and its result later
        // (`crate::services`); a set's connector is fetched from the backend,
        // never carried here.
        kind if crate::services::connector_kind(kind).is_some() && lamp.services().is_some() => {
            let services = lamp.services().ok_or_else(|| not_here(kind))?;
            services.start(kind)?;
            Ok(None)
        }
        crate::services::LIST_KIND if lamp.services().is_some() => {
            let services = lamp.services().ok_or_else(|| not_here(kind))?;
            services.list().map(Some)
        }
        // Harness's sign-in from the app (#5198): asked of lamp-tasks' app
        // route, which decides who may answer; its view comes back at once.
        kind if crate::harness_sign_in::KINDS.contains(&kind) && on(lamp, kind) => {
            let place = lamp.harness_sign_in().ok_or_else(|| not_here(kind))?;
            crate::harness_sign_in::ask(&place, kind, data)
        }
        // The developer door from the owner's app (#5299): taken with the
        // socket given, so an app that asks while lamp-control is off is told
        // so; a code goes back only sealed to the app's own key.
        kind if crate::control::KINDS.contains(&kind) && lamp.control().is_some() => {
            let door = lamp.control().ok_or_else(|| not_here(kind))?;
            crate::control::answer(&door, kind, data)
        }
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
        /// Skills' lines carried with texts not taken (`LAMP_SKILL_MESSAGES`).
        pub skill_lines: bool,
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
        pub harness_sign_in: Option<crate::harness_sign_in::Place>,
        /// lamp-tasks' app socket for the default agent (#6492); none is a
        /// path where nothing listens.
        pub tasks_app: Option<std::path::PathBuf>,
        pub make_skill: Option<std::path::PathBuf>,
        pub make_skill_ready: bool,
        pub write_skill: Option<std::path::PathBuf>,
        pub share: Option<crate::share::Store>,
        pub upload: Option<std::sync::Arc<crate::upload::Uploads>>,
        pub update_page: bool,
        /// The bus lamp-update's `update/status` is read from (#2194) and
        /// `update.check` asks on, and the checks asked (#4153).
        pub update_bus: Option<lamp_bus::Bus>,
        pub update_checks: crate::update::Checks,
        pub restart: bool,
        pub shutdown: bool,
        /// What `account` says (`LAMP_SIGNED_OUT`).
        pub account: Option<&'static str>,
        /// What `recovery` says (#3020).
        pub recovery: bool,
        /// What `reset` says (#2937).
        pub reset: Option<&'static str>,
        /// The person's own services (Queue #4710), with their switch.
        pub services: Option<std::sync::Arc<crate::services::Services>>,
        /// lamp-control's admin socket (#5299).
        pub control: Option<crate::control::Door>,
        /// The owner's two yeses from the app (#5500), with their switch.
        pub consent: Option<crate::consent::Place>,
        /// What `onboarding` says, and the switch's file (#5812).
        pub onboarding: Option<u64>,
        pub onboarding_done: Option<std::path::PathBuf>,
        /// What `onboarding_steps` says (#5979).
        pub onboarding_steps: Option<(u64, Vec<lamp_msgs::OnboardingStepEnded>)>,
    }

    impl Lamp for Fake {
        fn account(&mut self) -> Option<&'static str> {
            self.account
        }
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
                hears_name: false,
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
        fn skill_lines(&self) -> bool {
            self.texting || self.skill_lines
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
        fn harness_sign_in(&self) -> Option<crate::harness_sign_in::Place> {
            self.harness_sign_in.clone()
        }
        fn onboarding(&self) -> Option<u64> {
            self.onboarding
        }
        fn onboarding_done(&self) -> Option<std::path::PathBuf> {
            self.onboarding_done.clone()
        }
        fn onboarding_steps(&self) -> Option<(u64, Vec<lamp_msgs::OnboardingStepEnded>)> {
            self.onboarding_steps.clone()
        }
        fn make_skill(&self) -> Option<std::path::PathBuf> {
            self.make_skill.clone()
        }
        fn make_skill_ready(&self) -> bool {
            self.make_skill_ready
        }
        fn write_skill(&self) -> Option<std::path::PathBuf> {
            self.write_skill.clone()
        }
        fn share(&self) -> Option<crate::share::Store> {
            self.share.clone()
        }
        fn upload(&self) -> Option<std::sync::Arc<crate::upload::Uploads>> {
            self.upload.clone()
        }
        fn update_page(&self) -> bool {
            self.update_page
        }
        fn services(&self) -> Option<std::sync::Arc<crate::services::Services>> {
            self.services.clone()
        }
        fn control(&self) -> Option<crate::control::Door> {
            self.control.clone()
        }
        fn consent(&self) -> Option<crate::consent::Place> {
            self.consent.clone()
        }
        fn update_status(&self) -> Option<lamp_bus::Message> {
            self.update_bus
                .as_ref()?
                .latest(lamp_msgs::UpdateStatus::TOPIC)
                .ok()
                .flatten()
        }
        fn check_update(
            &mut self,
            now: std::time::Instant,
            retry: Option<u64>,
        ) -> Result<(), String> {
            self.update_checks.ask(self.update_bus.as_ref(), now, retry)
        }
        fn engines_page(&self) -> bool {
            self.update_page && self.update_checks.engines()
        }
        fn install_engine(&mut self, engine: &str, now: std::time::Instant) -> Result<(), String> {
            self.update_checks
                .ask_install(self.update_bus.as_ref(), engine, now)
        }
        fn tasks_app_socket(&self) -> std::path::PathBuf {
            self.tasks_app
                .clone()
                .unwrap_or_else(|| "/nowhere/app.sock".into())
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

    /// A stand-in for root's change helper on `socket`: each line's
    /// `consent` made in `folder` as `lamp-setup change` makes it, and the
    /// lines kept; `{"refused": ..}` for anything else.
    fn consent_helper(
        socket: &std::path::Path,
        folder: &std::path::Path,
    ) -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
        use std::io::{BufRead, Write};
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let (kept, folder) = (lines.clone(), folder.to_path_buf());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut line = String::new();
                let _ = std::io::BufReader::new(&stream).read_line(&mut line);
                kept.lock().unwrap().push(line.trim().to_owned());
                let asked: Value = serde_json::from_str(&line).unwrap_or_default();
                let reply = match asked["consent"].as_object().and_then(|c| c.iter().next()) {
                    Some((key, yes)) => {
                        let file = lamp_msgs::consent::file_in(&folder, key).unwrap();
                        if yes.as_bool() == Some(true) {
                            std::fs::write(&file, lamp_msgs::consent::bytes(key, 9, "app"))
                                .unwrap();
                        } else {
                            let _ = std::fs::remove_file(&file);
                        }
                        json!({"done": {key.as_str(): yes}})
                    }
                    None => json!({"refused": "the request names no change this lamp makes"}),
                };
                let _ = writeln!(stream, "{reply}");
            }
        });
        lines
    }

    /// #5500: the owner's two yeses, with `LAMP_APP_CONSENT=on` only.
    /// Off: neither kind in `can`, each "not on this lamp". On a lamp with a
    /// slide: face expressions alone, read from its file and set through
    /// the helper, the reply what the files then say; room sounds refused
    /// and never sent. With no slide, room sounds too. Anything but one
    /// key this lamp has as true or false is refused before the helper.
    #[test]
    fn the_owners_yeses_are_read_and_set_through_the_helper_with_their_switch() {
        let folder = std::path::PathBuf::from(format!("/tmp/lcc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let socket = folder.join("change.sock");
        let lines = consent_helper(&socket, &folder);
        let kinds = ["consent.get", "consent.set"];

        let mut off = lamp();
        let can = can(&off);
        assert!(kinds.iter().all(|kind| !can.contains(kind)));
        for kind in kinds {
            assert_eq!(
                answer(&mut off, kind, &json!({"face_expressions": true})),
                Err(format!("not on this lamp: {kind}"))
            );
        }

        let mut slide = Fake {
            consent: Some(crate::consent::place_in(&folder, &socket, false)),
            ..lamp()
        };
        let can = super::can(&slide);
        assert!(kinds.iter().all(|kind| can.contains(kind)));
        let get = |lamp: &mut Fake| answer(lamp, "consent.get", &json!({})).unwrap().unwrap();
        assert_eq!(get(&mut slide), json!({"face_expressions": false}));
        let set = |lamp: &mut Fake, data: Value| answer(lamp, "consent.set", &data);
        assert_eq!(
            set(&mut slide, json!({"face_expressions": true})),
            Ok(Some(json!({"face_expressions": true})))
        );
        assert_eq!(get(&mut slide), json!({"face_expressions": true}));
        assert!(
            set(&mut slide, json!({"room_sounds": true}))
                .unwrap_err()
                .contains("privacy slide")
        );
        assert!(!folder.join("room-sounds.json").exists());
        for data in [
            json!({}),
            json!({"face_expressions": "yes"}),
            json!({"face_expressions": true, "room_sounds": true}),
            json!({"camera": false}),
        ] {
            assert!(set(&mut slide, data.clone()).is_err(), "{data}");
        }
        assert_eq!(
            set(&mut slide, json!({"face_expressions": false})),
            Ok(Some(json!({"face_expressions": false})))
        );
        assert!(!folder.join("face-expressions.json").exists());

        let mut no_slide = Fake {
            consent: Some(crate::consent::place_in(&folder, &socket, true)),
            ..lamp()
        };
        assert_eq!(
            get(&mut no_slide),
            json!({"face_expressions": false, "room_sounds": false})
        );
        assert_eq!(
            set(&mut no_slide, json!({"room_sounds": true})),
            Ok(Some(
                json!({"face_expressions": false, "room_sounds": true})
            ))
        );
        // Only what passed the checks reached the helper.
        assert_eq!(
            *lines.lock().unwrap(),
            [
                r#"{"consent":{"face_expressions":true}}"#,
                r#"{"consent":{"face_expressions":false}}"#,
                r#"{"consent":{"room_sounds":true}}"#,
            ]
        );
        // The helper not there: said, nothing claimed.
        let mut gone = Fake {
            consent: Some(crate::consent::place_in(
                &folder,
                &folder.join("none.sock"),
                true,
            )),
            ..lamp()
        };
        assert_eq!(
            set(&mut gone, json!({"room_sounds": false})),
            Err("the lamp's settings are not running".to_owned())
        );
        let _ = std::fs::remove_dir_all(&folder);
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
        // `account` only while signed out; a lamp signed in says what it did.
        assert!(info.get("account").is_none(), "{info}");
        // `developer` only where lamp-control's admin socket is: none here.
        assert!(info.get("developer").is_none(), "{info}");
        let mut out = Fake {
            account: Some("signed_out"),
            ..Fake::default()
        };
        let told = answer(&mut out, "system.info", &Value::Null)
            .unwrap()
            .unwrap();
        assert_eq!(told["account"], "signed_out");
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
        // #5812: `onboarding_done_at` only once the walk's end is known.
        assert!(info.get("onboarding_done_at").is_none(), "{info}");
        let mut done = Fake {
            onboarding: Some(1_762_675_200_000),
            ..Fake::default()
        };
        let told = answer(&mut done, "system.info", &Value::Null)
            .unwrap()
            .unwrap();
        assert_eq!(told["onboarding_done_at"], 1_762_675_200_000u64);
        // Mobile's ask: `onboarding_done_supported` only with the switch on.
        assert!(info.get("onboarding_done_supported").is_none(), "{info}");
        assert!(told.get("onboarding_done_supported").is_none(), "{told}");
        let mut will = Fake {
            onboarding_done: Some("/nonexistent/onboarding-done".into()),
            ..Fake::default()
        };
        let told = answer(&mut will, "system.info", &Value::Null)
            .unwrap()
            .unwrap();
        assert_eq!(told["onboarding_done_supported"], true);
        // #5979: `onboarding_step_supported` beside it, with the same switch.
        assert_eq!(told["onboarding_step_supported"], true);
        assert!(info.get("onboarding_step_supported").is_none(), "{info}");
        // #6076: `onboarding_page_supported` beside them, with the same
        // switch, and `onboarding.page` answered only then.
        assert_eq!(told["onboarding_page_supported"], true);
        assert!(info.get("onboarding_page_supported").is_none(), "{info}");
        // #6366: the page contract's version beside it, same switch.
        assert_eq!(told["onboarding_page_version"], 2);
        assert!(info.get("onboarding_page_version").is_none(), "{info}");
        assert!(answer(&mut will, "onboarding.page", &json!({"page": "create"})).is_ok());
        assert!(
            answer(
                &mut Fake::default(),
                "onboarding.page",
                &json!({"page": "create"})
            )
            .is_err()
        );
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
            "control.set",
            "control.set_agent",
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
            // Without LAMP_SKILL_WRITTEN=on.
            "skills.write",
            // Without LAMP_TASKS_SIGN_IN_APP=on (#5198).
            "harness.sign_in.start",
            "harness.sign_in.state",
            "harness.sign_in.answer",
        ] {
            assert_eq!(
                answer(&mut lamp, kind, &Value::Null),
                Err(format!("not on this lamp: {kind}"))
            );
        }
        assert!(answer(&mut lamp, "", &Value::Null).is_err());
    }

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

    /// #1685: with only skills' lines carried (`LAMP_SKILL_MESSAGES=on`,
    /// `LAMP_MESSAGES` unset), a text from the app is refused as with both
    /// off, and nothing is put on `chat/in`.
    #[test]
    fn with_only_skills_lines_carried_a_text_is_refused_as_with_both_off() {
        for (texting, skill_lines) in [(false, true), (false, false)] {
            let mut lamp = Fake {
                texting,
                skill_lines,
                ..lamp()
            };
            assert!(!lamp.texting());
            assert_eq!(
                answer(&mut lamp, "chat.send", &json!({"message": "hi"})),
                Err(not_here("chat.send")),
                "skill lines {skill_lines}"
            );
            assert!(lamp.texts.is_empty(), "skill lines {skill_lines}");
        }
    }

    /// #2864: `system.info`'s `can` holds each switch's kinds only while it
    /// is on, as [`answer`] takes them; the always-answered kinds are in it
    /// with every switch off.
    #[test]
    fn system_info_says_each_switchs_kinds_only_while_it_is_on() {
        let switches: Vec<Switch> = vec![
            ("texting", |f| f.texting = true, &["chat.send"]),
            // Skills' lines alone (#1685) open no kind: `chat.send` stays
            // refused.
            ("skill lines", |f| f.skill_lines = true, &[]),
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
                    "control.set",
                    "control.set_agent",
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
                    "control.set",
                    "control.set_agent",
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
                    "control.set",
                    "control.set_agent",
                    "owner.place",
                    "owner.place.clear",
                ],
            ),
            (
                "chosen voice",
                |f| {
                    f.changes =
                        changes_of(false, false).map(|changes| changes.taking_the_voice(true))
                },
                &[
                    "timezone.set",
                    "person.name.set",
                    "device.rename",
                    "language.set",
                    "control.set",
                    "control.set_agent",
                    "voice.set",
                    "character.set",
                    "voice.get",
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
                // A made skill sent to the Store (fourteen-P13): its three
                // kinds only with the page and the Store's upload.
                "skills share",
                |f| {
                    f.skills = Some(places_of(false));
                    f.share = Some(crate::share::Store {
                        base: "https://api.test".into(),
                        key: "k".into(),
                    });
                },
                &[
                    "skills.list",
                    "skills.remove",
                    "skills.off",
                    "skills.on",
                    "skills.camera",
                    "skills.share",
                    "skills.shared",
                    "skills.unshare",
                ],
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
                // A skill added from a file (`LAMP_SKILL_UPLOAD`).
                "skill upload",
                |f| {
                    f.upload = Some(std::sync::Arc::new(crate::upload::Uploads::new(
                        "https://api.test",
                        "k",
                        "d",
                        std::path::Path::new("/nowhere/skills"),
                        std::path::Path::new("/nowhere/pushed"),
                        std::path::Path::new("/nowhere/key.pub"),
                        std::path::Path::new("/nowhere/make.sock"),
                    )))
                },
                &["skills.upload"],
            ),
            (
                // A skill written in the app: its switch alone, no
                // computer needed; `skills.made` tells what became of it.
                "write skill",
                |f| f.write_skill = Some(std::path::PathBuf::from("/nowhere/make.sock")),
                &["skills.made", "skills.write"],
            ),
            (
                "update page",
                |f| f.update_page = true,
                &["update.status", "update.check"],
            ),
            (
                // The lamp's agents (#6240): with the update page's switch.
                "engines",
                |f| {
                    f.update_page = true;
                    f.update_checks = crate::update::Checks::with_engines(true);
                },
                &[
                    "update.status",
                    "update.check",
                    "engines.install",
                    "engines.default",
                ],
            ),
            (
                "harness sign-in",
                |f| {
                    f.harness_sign_in = Some(crate::harness_sign_in::tests::place(
                        std::path::Path::new("/nowhere"),
                        Default::default(),
                    ))
                },
                &[
                    "harness.sign_in",
                    "harness.sign_in.start",
                    "harness.sign_in.state",
                    "harness.sign_in.answer",
                ],
            ),
            ("restart", |f| f.restart = true, &["system.reboot"]),
            ("shutdown", |f| f.shutdown = true, &["system.shutdown"]),
            (
                // lamp-services' socket there: a folder that is.
                "services",
                |f| {
                    f.services = Some(crate::services::tests::services(
                        std::env::temp_dir(),
                        Err("never fetched".into()),
                    ))
                },
                &["services", "services.list"],
            ),
            (
                // lamp-control's admin socket there: a path that is.
                "developer door",
                |f| {
                    f.control = Some(crate::control::Door {
                        socket: std::env::temp_dir(),
                        device_id: "d-42".into(),
                    })
                },
                &crate::control::KINDS,
            ),
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
                harness: None,
                round: None,
                refused: None,
                engines: None,
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

    /// The owner's retry of a refused build (#6266) is `update.check`'s `retry`:
    /// a whole number, else refused and nothing is sent.
    #[test]
    fn a_retry_is_a_whole_number_or_refused() {
        let root = tempfile::tempdir().unwrap();
        let bus = lamp_bus::Bus::new(root.path()).unwrap();
        let mut lamp = Fake {
            update_bus: Some(lamp_bus::Bus::new(root.path()).unwrap()),
            update_page: true,
            ..lamp()
        };
        let mut update = bus
            .subscribe_typed(
                lamp_msgs::UpdateCheck::TOPIC,
                "update",
                lamp_msgs::UpdateCheck::TYPE,
            )
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(250));
        for bad in [json!("12226"), json!(1.5), json!(-3), json!(true)] {
            assert_eq!(
                answer(&mut lamp, "update.check", &json!({"retry": bad})),
                Err("retry must be the refused build's sequence, a whole number".to_owned()),
                "{bad}"
            );
        }
        assert!(
            update
                .recv(std::time::Duration::from_millis(100))
                .unwrap()
                .is_none()
        );
        assert!(
            answer(&mut lamp, "update.check", &json!({"retry": 12226})).is_ok(),
            "a whole number is taken"
        );
        let heard = update
            .recv(std::time::Duration::from_secs(1))
            .unwrap()
            .expect("the retry is on the bus");
        assert_eq!(
            heard.parse::<lamp_msgs::UpdateCheck>().unwrap(),
            lamp_msgs::UpdateCheck { retry: Some(12226) }
        );
    }

    /// #6240: `engines.install` is answered only with both switches, for
    /// an agent lamp-update says is missing or failed; refused otherwise,
    /// within a minute of the last, and when no lamp-update reads it.
    #[test]
    fn the_app_installs_a_missing_agent_only_when_lamp_update_says_it_is_missing() {
        let root = tempfile::tempdir().unwrap();
        let bus = lamp_bus::Bus::new(root.path()).unwrap();
        let mut lamp = Fake {
            update_bus: Some(lamp_bus::Bus::new(root.path()).unwrap()),
            update_page: true,
            ..lamp()
        };
        let hermes = json!({"engine": "hermes"});
        assert_eq!(
            answer(&mut lamp, "engines.install", &hermes),
            Err(not_here("engines.install"))
        );
        lamp.update_checks = crate::update::Checks::with_engines(true);
        // lamp-update has said nothing: no such agent.
        assert_eq!(
            answer(&mut lamp, "engines.install", &hermes),
            Err(crate::update::NO_ENGINE.to_owned())
        );
        let mut says = bus
            .publisher_declared(
                lamp_msgs::UpdateStatus::TOPIC,
                &lamp_msgs::UpdateStatus::declaration(),
                "update",
            )
            .unwrap();
        let with = |state: &str| lamp_msgs::UpdateStatus {
            running: None,
            now: "idle".into(),
            last: None,
            harness: None,
            round: None,
            refused: None,
            engines: Some(std::collections::BTreeMap::from([(
                "hermes".to_owned(),
                lamp_msgs::EngineInstall {
                    state: state.into(),
                    version: None,
                    reason: (state == "failed").then(|| "no_space".into()),
                    at_unix_ms: Some(3),
                },
            )])),
        };
        for state in ["off", "waiting", "installing", "frozen"] {
            says.publish_json(0, &with(state)).unwrap();
            assert_eq!(
                answer(&mut lamp, "engines.install", &hermes),
                Err(crate::update::NOT_MISSING.to_owned()),
                "{state}"
            );
        }
        says.publish_json(0, &with("not_installed")).unwrap();
        let status = answer(&mut lamp, "update.status", &json!({}))
            .unwrap()
            .unwrap();
        assert_eq!(status["engines"]["hermes"]["state"], "not_installed");
        assert_eq!(
            answer(&mut lamp, "engines.install", &json!({"engine": "openclaw"})),
            Err(crate::update::NO_ENGINE.to_owned())
        );
        assert!(answer(&mut lamp, "engines.install", &json!({})).is_err());
        // No lamp-update reads asks: said, and not counted.
        assert_eq!(
            answer(&mut lamp, "engines.install", &hermes),
            Err(crate::update::NOT_LISTENING.to_owned())
        );
        let mut update = bus
            .subscribe_typed(
                lamp_msgs::EngineInstallAsk::TOPIC,
                "update",
                lamp_msgs::EngineInstallAsk::TYPE,
            )
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(250));
        assert_eq!(
            answer(&mut lamp, "engines.install", &hermes),
            Ok(Some(json!({"install": "started"})))
        );
        let heard = update
            .recv(std::time::Duration::from_secs(1))
            .unwrap()
            .expect("one ask on the bus");
        assert_eq!(
            heard.parse::<lamp_msgs::EngineInstallAsk>().unwrap().engine,
            "hermes"
        );
        says.publish_json(0, &with("failed")).unwrap();
        assert_eq!(
            answer(&mut lamp, "engines.install", &hermes),
            Err(crate::update::TOO_SOON.to_owned())
        );
        // The update page's switch off: neither is here.
        lamp.update_page = false;
        assert_eq!(
            answer(&mut lamp, "engines.install", &hermes),
            Err(not_here("engines.install"))
        );
    }

    /// #6492: `engines.default` is answered only with both switches; the
    /// engine goes to lamp-tasks and its answer comes back; an empty one
    /// is refused before lamp-tasks is asked; lamp-tasks' refusal passes
    /// through; `update.status` carries the default lamp-tasks says, and
    /// leaves it out when lamp-tasks is not reached.
    #[test]
    fn the_app_chooses_the_default_agent_through_lamp_tasks() {
        let dir = crate::harness_sign_in::tests::folder("ead");
        let asked = crate::harness_sign_in::tests::tasks(
            &dir,
            vec![
                r#"{"ok":true,"default":"openclaw","chosen_in_app":true}"#,
                r#"{"ok":false,"error":"engine_not_installed"}"#,
                r#"{"ok":true,"default":"openclaw","chosen_in_app":true}"#,
            ],
        );
        let root = tempfile::tempdir().unwrap();
        let bus = lamp_bus::Bus::new(root.path()).unwrap();
        let mut lamp = Fake {
            update_bus: Some(lamp_bus::Bus::new(root.path()).unwrap()),
            update_page: true,
            tasks_app: Some(dir.join("app.sock")),
            ..lamp()
        };
        let openclaw = json!({"engine": "openclaw"});
        // The agents' switch off: not here, nothing asked.
        assert_eq!(
            answer(&mut lamp, "engines.default", &openclaw),
            Err(not_here("engines.default"))
        );
        lamp.update_checks = crate::update::Checks::with_engines(true);
        assert_eq!(
            answer(&mut lamp, "engines.default", &openclaw),
            Ok(Some(json!({"default": "openclaw", "chosen_in_app": true})))
        );
        assert_eq!(
            serde_json::from_str::<Value>(&asked.recv().unwrap()).unwrap(),
            json!({"op": "app_engine_default", "engine": "openclaw"})
        );
        assert_eq!(
            answer(&mut lamp, "engines.default", &json!({"engine": ""})),
            Err(crate::engine_default::ENGINE_UNKNOWN.to_owned())
        );
        assert_eq!(
            answer(&mut lamp, "engines.default", &json!({"engine": "hermes"})),
            Err(crate::engine_default::ENGINE_NOT_INSTALLED.to_owned())
        );
        assert_eq!(
            serde_json::from_str::<Value>(&asked.recv().unwrap()).unwrap(),
            json!({"op": "app_engine_default", "engine": "hermes"}),
            "the empty name was never forwarded"
        );
        // update.status: the default beside the agents.
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
                now: "idle".into(),
                last: None,
                harness: None,
                round: None,
                refused: None,
                engines: Some(std::collections::BTreeMap::from([(
                    "hermes".to_owned(),
                    lamp_msgs::EngineInstall {
                        state: "installed".into(),
                        version: Some("1.0.0".into()),
                        reason: None,
                        at_unix_ms: Some(3),
                    },
                )])),
            },
        )
        .unwrap();
        let status = answer(&mut lamp, "update.status", &json!({}))
            .unwrap()
            .unwrap();
        assert_eq!(status["engines"]["default"], "openclaw", "{status}");
        assert_eq!(status["engines"]["default_chosen_in_app"], true);
        assert_eq!(status["engines"]["hermes"]["state"], "installed");
        assert_eq!(
            serde_json::from_str::<Value>(&asked.recv().unwrap()).unwrap(),
            json!({"op": "app_engine_default_state"})
        );
        // lamp-tasks not reached: the status still answers, no default.
        lamp.tasks_app = Some(dir.join("none.sock"));
        let status = answer(&mut lamp, "update.status", &json!({}))
            .unwrap()
            .unwrap();
        assert!(status["engines"].get("default").is_none(), "{status}");
        assert_eq!(status["engines"]["hermes"]["state"], "installed");
        // The update page's switch off: not here.
        lamp.update_page = false;
        assert_eq!(
            answer(&mut lamp, "engines.default", &openclaw),
            Err(not_here("engines.default"))
        );
        let _ = std::fs::remove_dir_all(&dir);
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
            link: None,
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
