//! `pose` (docs/control.md, "Poses"; docs/npu.md): one JPEG of the
//! client's own, sent on to the lamp's NPU service (`lamp-npu`) and its
//! answer given back. The grant (`camera`), the local-only rule and the
//! lamp's privacy are the door's, checked before anything is sent; the
//! service's own rules (its rate, the board's heat, its model) answer in
//! its words. Off unless `LAMP_NPU=on` in lamp-control's environment.
use crate::protocol::{Error, ErrorCode};
use serde_json::{Map, Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

/// The switch, read by lamp-control and lamp-npu: off unless `on`.
pub const ENVIRONMENT: &str = "LAMP_NPU";
/// lamp-npu's socket (`lamp_npu::SOCKET`).
pub const SOCKET: &str = "/run/lampos-npu/npu.sock";
/// How long one pose may take: a run is tens of ms, a first load under a
/// second.
pub const WAIT: Duration = Duration::from_secs(5);
/// The longest answer read: 17 keypoints and the times, under 2 KiB.
pub const ANSWER_MOST: u64 = 8 * 1024;
/// What a `pose` is told with the switch off.
pub const OFF: &str = "the lamp's NPU is off on this lamp (LAMP_NPU)";

/// Whether the switch is on, as `var` gives it.
pub fn on(var: Option<&str>) -> bool {
    var == Some("on")
}

/// Where poses are asked.
#[derive(Clone, Debug)]
pub struct Npu {
    socket: PathBuf,
}

impl Npu {
    pub fn new(socket: PathBuf) -> Self {
        Self { socket }
    }

    /// `client`'s pose of `jpeg` (base64, checked by the op): what the
    /// reply adds, or the service's refusal with its code.
    pub fn pose(&self, client: &str, jpeg: &str) -> Result<Value, Error> {
        let line = json!({"op": "pose", "client": client, "jpeg": jpeg}).to_string();
        let gone = |_| {
            Error::new(
                ErrorCode::Refused,
                "the lamp's NPU service is not reachable",
            )
        };
        let mut stream = UnixStream::connect(&self.socket).map_err(gone)?;
        stream.set_read_timeout(Some(WAIT)).map_err(gone)?;
        stream.set_write_timeout(Some(WAIT)).map_err(gone)?;
        writeln!(stream, "{line}").map_err(gone)?;
        let mut answer = String::new();
        BufReader::new(stream.take(ANSWER_MOST))
            .read_line(&mut answer)
            .map_err(|_| Error::new(ErrorCode::Refused, "the lamp's NPU did not answer in time"))?;
        reply(&answer)
    }
}

/// lamp-npu's answer line as the door's reply: its fields but `ok`, or its
/// refusal. A code this build does not know is `refused`, in its words.
pub fn reply(answer: &str) -> Result<Value, Error> {
    let answer: Value = serde_json::from_str(answer).map_err(|_| {
        Error::new(
            ErrorCode::Refused,
            "the lamp's NPU gave an answer it can't read",
        )
    })?;
    if answer["ok"] == true {
        let fields: Map<String, Value> = answer
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(key, _)| matches!(key.as_str(), "pose" | "width" | "height" | "model" | "ms"))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        return Ok(Value::Object(fields));
    }
    let code = match answer["code"].as_str() {
        Some("private") => ErrorCode::Private,
        Some("rate") => ErrorCode::Rate,
        Some("busy") => ErrorCode::Busy,
        Some("preparing") => ErrorCode::Preparing,
        Some("unavailable") => ErrorCode::Unavailable,
        Some("bad_request") => ErrorCode::BadRequest,
        _ => ErrorCode::Refused,
    };
    let mut error = Error::new(
        code,
        answer["why"]
            .as_str()
            .unwrap_or("the lamp's NPU refused it")
            .chars()
            .take(200)
            .collect::<String>(),
    );
    error.retry_ms = answer["retry_ms"].as_u64();
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn a_pose_is_its_fields_and_a_refusal_its_code() {
        let ok = reply(
            r#"{"ok":true,"pose":null,"width":640,"height":480,"model":"m","ms":{"total":1.0},"extra":1}"#,
        )
        .unwrap();
        assert_eq!(
            ok,
            json!({"pose": null, "width": 640, "height": 480, "model": "m", "ms": {"total": 1.0}})
        );
        let preparing =
            reply(r#"{"ok":false,"code":"preparing","why":"fetching","retry_ms":2000}"#)
                .unwrap_err();
        assert_eq!(
            (preparing.code, preparing.retry_ms),
            (ErrorCode::Preparing, Some(2000))
        );
        for (code, expected) in [
            ("unavailable", ErrorCode::Unavailable),
            ("rate", ErrorCode::Rate),
            ("busy", ErrorCode::Busy),
            ("private", ErrorCode::Private),
            ("bad_request", ErrorCode::BadRequest),
            ("something_new", ErrorCode::Refused),
        ] {
            let line = json!({"ok": false, "code": code, "why": "w"}).to_string();
            assert_eq!(reply(&line).unwrap_err().code, expected, "{code}");
        }
        assert_eq!(reply("not json").unwrap_err().code, ErrorCode::Refused);
    }

    #[test]
    fn the_line_names_the_client_and_nobody_there_is_refused() {
        let dir = std::env::temp_dir().join(format!("lamp-control-npu-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let socket = dir.join("npu.sock");
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        let served = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            writeln!(
                stream,
                r#"{{"ok":true,"pose":null,"width":1,"height":1,"model":"m","ms":{{}}}}"#
            )
            .unwrap();
            line
        });
        let npu = Npu::new(socket.clone());
        assert_eq!(npu.pose("c_1", "AAAA").unwrap()["model"], "m");
        let sent: Value = serde_json::from_str(&served.join().unwrap()).unwrap();
        assert_eq!(sent, json!({"op": "pose", "client": "c_1", "jpeg": "AAAA"}));
        let _ = std::fs::remove_file(&socket);
        assert_eq!(
            npu.pose("c_1", "AAAA").unwrap_err().code,
            ErrorCode::Refused
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
