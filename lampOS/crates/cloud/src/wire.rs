//! V1's messages on the company broker, as the app and the backend read
//! them (`docs/mqtt.md`): commands on `fa_channel`, `{"cmd": …}`, and the
//! lamp's replies on `fd_channel`, each with the same identity fields.
use serde_json::{Map, Value, json};

/// A command from the backend or the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// `{"cmd":"info"}`: who this lamp is.
    Info,
    /// `{"cmd":"data","kind":K,"data":{…}}`.
    Data { kind: String, data: Value },
    /// Any other `cmd` V1 has (`add_channel`, `whatsapp_pair`, …): refused
    /// plainly on this lamp.
    Other(String),
}

/// The command in a message, or why there is none.
pub fn parse(payload: &[u8]) -> Result<Command, String> {
    let value: Value =
        serde_json::from_slice(payload).map_err(|_| "not a JSON object".to_owned())?;
    let cmd = value
        .get("cmd")
        .and_then(Value::as_str)
        .ok_or("no cmd")?
        .trim();
    Ok(match cmd {
        "info" => Command::Info,
        "data" => Command::Data {
            kind: value
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_owned(),
            data: value.get("data").cloned().unwrap_or(Value::Null),
        },
        other => Command::Other(other.to_owned()),
    })
}

/// The fields every reply carries (V1's `MQTTInfoResponse`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    /// V1 says `lamp`; whether V2 says `lamp-v2` is the backend's call.
    pub device: String,
    /// The build `lamp-update` installed.
    pub version: String,
    /// The backend's device id.
    pub id: String,
    /// The lamp's hardware name, `lamp-XXXX`, as setup's status says it.
    pub mac: String,
    pub timezone: String,
    pub local_ip: String,
}

impl Identity {
    fn base(&self, kind_of_reply: &str, now: &str) -> Map<String, Value> {
        let mut reply = Map::new();
        reply.insert("device".into(), json!(self.device));
        reply.insert("type".into(), json!(kind_of_reply));
        reply.insert("version".into(), json!(self.version));
        reply.insert("id".into(), json!(self.id));
        reply.insert("mac".into(), json!(self.mac));
        reply.insert("time".into(), json!(now));
        // V1 always says these two, so an older reader never misses them.
        reply.insert("wakeword_enabled".into(), json!(false));
        reply.insert("voice_input_mode".into(), json!("automatic"));
        if !self.timezone.is_empty() {
            reply.insert("timezone".into(), json!(self.timezone));
        }
        if !self.local_ip.is_empty() {
            reply.insert("local_ip".into(), json!(self.local_ip));
        }
        reply
    }

    /// The reply to `info`, sent unasked on every connection too.
    pub fn info(&self, now: &str) -> Value {
        Value::Object(self.base("info", now))
    }

    /// The reply to a `data` kind: its result, or why it failed.
    pub fn data(&self, now: &str, kind: &str, result: &Result<Option<Value>, String>) -> Value {
        let mut reply = self.base("data", now);
        reply.insert("kind".into(), json!(kind));
        match result {
            Ok(data) => {
                reply.insert("status".into(), json!("success"));
                if let Some(data) = data {
                    reply.insert("data".into(), data.clone());
                }
            }
            Err(why) => {
                reply.insert("status".into(), json!("failure"));
                reply.insert("error".into(), json!(why));
            }
        }
        Value::Object(reply)
    }

    /// The reply to a `cmd` this lamp does not have: V1's shape for it,
    /// failed, saying so.
    pub fn refused(&self, now: &str, cmd: &str) -> Value {
        let mut reply = self.base(cmd, now);
        reply.insert("status".into(), json!("failure"));
        reply.insert("error".into(), json!(not_here(cmd)));
        Value::Object(reply)
    }
}

/// What a lamp without it says of a command or a kind.
pub fn not_here(what: &str) -> String {
    format!("not on this lamp: {what}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> Identity {
        Identity {
            device: "lamp".into(),
            version: "0.1.0+abc".into(),
            id: "d-42".into(),
            mac: "lamp-7f72".into(),
            timezone: "Asia/Ho_Chi_Minh".into(),
            local_ip: String::new(),
        }
    }

    #[test]
    fn commands_are_read_as_v1_sends_them() {
        assert_eq!(parse(br#"{"cmd":"info"}"#), Ok(Command::Info));
        assert_eq!(
            parse(br#"{"cmd":"data","kind":"volume.set","data":{"volume":90}}"#),
            Ok(Command::Data {
                kind: "volume.set".into(),
                data: json!({"volume": 90})
            })
        );
        assert_eq!(
            parse(br#"{"cmd":"data","kind":"system.info"}"#),
            Ok(Command::Data {
                kind: "system.info".into(),
                data: Value::Null
            })
        );
        assert_eq!(
            parse(br#"{"cmd":"add_channel","channel":"telegram"}"#),
            Ok(Command::Other("add_channel".into()))
        );
        assert!(parse(b"nonsense").is_err());
        assert!(parse(br#"{"kind":"x"}"#).is_err());
    }

    #[test]
    fn replies_carry_v1s_identity_fields() {
        let info = me().info("2026-10-17T15:00:00Z");
        assert_eq!(
            info,
            json!({"device":"lamp","type":"info","version":"0.1.0+abc","id":"d-42",
                "mac":"lamp-7f72","time":"2026-10-17T15:00:00Z","wakeword_enabled":false,
                "voice_input_mode":"automatic","timezone":"Asia/Ho_Chi_Minh"})
        );
        let done = me().data("t", "volume.get", &Ok(Some(json!({"volume": 60}))));
        assert_eq!(
            (&done["type"], &done["kind"], &done["status"], &done["data"]),
            (
                &json!("data"),
                &json!("volume.get"),
                &json!("success"),
                &json!({"volume": 60})
            )
        );
        assert!(done.get("error").is_none());
        let failed = me().data("t", "face.enroll", &Err(not_here("face.enroll")));
        assert_eq!(failed["status"], "failure");
        assert_eq!(failed["error"], "not on this lamp: face.enroll");
        assert!(failed.get("data").is_none());
        let refused = me().refused("t", "add_channel");
        assert_eq!(
            (&refused["type"], &refused["status"], &refused["error"]),
            (
                &json!("add_channel"),
                &json!("failure"),
                &json!("not on this lamp: add_channel")
            )
        );
    }
}
