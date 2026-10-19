//! The device API: the `data` kinds this lamp answers, each carried out by
//! the part of the lamp that already owns it (#1396). Everything else is
//! refused plainly, in V1's shape, so the app shows it.
use crate::light;
use crate::wire::not_here;
use lamp_msgs::Light;
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
/// there is none), `available` whether the voice is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Mic {
    pub muted: bool,
    pub hw_switch_muted: Option<bool>,
    pub available: bool,
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
    /// Where a face from the app's photo goes, and the eyes to ask (#1968,
    /// `crate::photos`): there only with `LAMP_PHOTO_FACES=on`.
    fn photos(&self) -> Option<crate::photos::Place> {
        None
    }
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
        "system.info" => some(json!({
            "versions": lamp.versions(),
            "network": lamp.network(),
            "host": lamp.host(),
        })),
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
        "chat.send" if lamp.texting() => {
            let now_ms = crate::chat::now_ms();
            let run = lamp.new_run(now_ms);
            let text = crate::chat::text_in(data, run)?;
            lamp.send_text(&text, now_ms)?;
            some(json!({"run_id": text.run_id, "session_id": text.session_id}))
        }
        // Health and crash reports (#1683): whether the person's yes stands,
        // since when and how many wait; and the person's own choice, from
        // their account, the only way to turn them on after setup.
        "reports.get" | "reports.set" if lamp.reports().is_some() => {
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
        "skills.list" if lamp.skills().is_some() => {
            let places = lamp.skills().ok_or_else(|| not_here(kind))?;
            crate::skills::list(&places).map(Some)
        }
        // Names and the time zone after setup (#1959): handed to root's
        // helper, answered `starting` now and its result later.
        kind if crate::settings::KINDS.contains(&kind) && lamp.changes().is_some() => {
            let changes = lamp.changes().ok_or_else(|| not_here(kind))?;
            let line = crate::settings::line(kind, data)?;
            changes.start(kind, line);
            Ok(None)
        }
        // A face from a photo the app sends (#1968): handed to the eyes,
        // which keep it unconfirmed until its person says yes at the lamp.
        // It comes over the lamp's own account channel only.
        "faces.add_photo" if lamp.photos().is_some() => {
            let place = lamp.photos().ok_or_else(|| not_here(kind))?;
            let (name, photo) = crate::photos::asked(data)?;
            crate::photos::add(&place, &name, &photo, crate::chat::now_ms()).map(Some)
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
        pub texts: Vec<lamp_msgs::ChatIn>,
        pub to_give: Vec<Value>,
        /// Reports (#1683), with their switch.
        pub reports: Option<std::sync::Arc<crate::reports::Shared>>,
        /// The skills page (#1722), with its switch.
        pub skills: Option<crate::skills::Places>,
        /// Changes after setup (#1959), with their switch.
        pub changes: Option<std::sync::Arc<crate::settings::Changes>>,
    }

    impl Lamp for Fake {
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
        fn send_text(&mut self, text: &lamp_msgs::ChatIn, _now_ms: u64) -> Result<(), String> {
            self.texts.push(text.clone());
            Ok(())
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
            Some(json!({"muted": true, "hw_switch_muted": true, "available": true}))
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
        // The other skills kinds are not built: refused as before.
        for kind in ["skills.remove", "skills.catalog", "skills.install"] {
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
        ] {
            assert_eq!(
                answer(&mut lamp, kind, &Value::Null),
                Err(format!("not on this lamp: {kind}"))
            );
        }
        assert!(answer(&mut lamp, "", &Value::Null).is_err());
    }
}
