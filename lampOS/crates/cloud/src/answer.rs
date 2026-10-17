//! The device API: the `data` kinds this lamp answers, each carried out by
//! the part of the lamp that already owns it (#1396). Everything else is
//! refused plainly, in V1's shape, so the app shows it.
use crate::wire::not_here;
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
}

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
    }

    fn lamp() -> Fake {
        Fake {
            raw: 60,
            ceiling: 80,
            set: Vec::new(),
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
            "led.resting.set",
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
