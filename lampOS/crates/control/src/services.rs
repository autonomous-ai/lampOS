//! A skill run's linked services (docs/control.md, "A skill run's linked
//! services", #6432): `service.read` and `service.write` on the run socket,
//! for the services the run's token names (`<code>-read`, `<code>-write`),
//! sent on to lamp-services' tools socket, whose own gate answers them. No
//! service token ever reaches lamp-control or the run: lamp-services keeps
//! them in root's store, and a write is only prepared and held there for the
//! person's spoken yes. Off unless `LAMP_SKILL_TASKS=on`, here and in
//! lamp-services, which takes lamp-control's cgroup for these two calls only.
use crate::protocol::{Error, ErrorCode};
use serde_json::{Map, Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

/// The switch, read by lamp-control and lamp-services (and lamp-tasks for
/// the run itself): off unless `on`.
pub const ENVIRONMENT: &str = "LAMP_SKILL_TASKS";
/// lamp-services' tools socket (`lamp_services::tools::DIR`, `call.sock`).
pub const SOCKET: &str = "/run/lampos-services/tools/call.sock";
/// How long a call may take: a mail box or a web service takes seconds, as
/// the voice's own wait for the same socket (`lamp_voice::services::DEADLINE`).
pub const WAIT: Duration = Duration::from_secs(20);
/// The longest answer read: lamp-services answers a run in one line under
/// 7 KiB (`lamp_services::serve::RUN_ANSWER_MOST`), a longer read cut to
/// fit and marked `cut`, so 1 KiB of this is spare.
pub const ANSWER_MOST: u64 = 8 * 1024 + 1;

/// What a `service.read` or `service.write` is told with the switch off.
pub const OFF: &str = "a skill's linked services are off on this lamp (LAMP_SKILL_TASKS)";
/// What one from anyone but a skill's run is told.
pub const RUNS_ONLY: &str = "a linked service is a skill run's alone";

/// Whether the switch is on, as `var` gives it.
pub fn on(var: Option<&str>) -> bool {
    var == Some("on")
}

/// Where a run's service calls go.
#[derive(Clone, Debug)]
pub struct Services {
    socket: PathBuf,
}

impl Services {
    pub fn new(socket: PathBuf) -> Self {
        Self { socket }
    }

    /// The one line lamp-services' tools socket takes for skill `skill`'s
    /// run: its `service_read` or `service_write`, `service` among the args.
    pub fn line(skill: &str, write: bool, service: &str, args: &Map<String, Value>) -> String {
        let mut args = args.clone();
        args.insert("service".to_owned(), json!(service));
        json!({
            "op": "call",
            "name": if write { "service_write" } else { "service_read" },
            "args": args,
            "skill": skill,
        })
        .to_string()
    }

    /// Sends `line` and waits for its answer, off the door's lock: the
    /// reply's `result`, or lamp-services' refusal in its own words.
    pub fn call(&self, line: &str) -> Result<Value, Error> {
        let refused = |why: &str| Error::new(ErrorCode::Refused, why);
        let gone = |_| refused("lamp-services is not reachable");
        let mut stream = UnixStream::connect(&self.socket).map_err(gone)?;
        stream.set_read_timeout(Some(WAIT)).map_err(gone)?;
        stream.set_write_timeout(Some(WAIT)).map_err(gone)?;
        writeln!(stream, "{line}").map_err(gone)?;
        let mut answer = String::new();
        BufReader::new(stream.take(ANSWER_MOST))
            .read_line(&mut answer)
            .map_err(|_| refused("lamp-services did not answer in time"))?;
        let answer: Value = serde_json::from_str(&answer)
            .map_err(|_| refused("lamp-services gave an answer it can't read"))?;
        if answer["ok"] == true {
            Ok(json!({"result": answer["result"]}))
        } else {
            Err(refused(
                answer["error"]
                    .as_str()
                    .unwrap_or("lamp-services refused it"),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn the_line_names_the_function_the_service_and_the_skill() {
        let args: Map<String, Value> =
            serde_json::from_str(r#"{"what":"search","query":"is:unread"}"#).unwrap();
        let read: Value =
            serde_json::from_str(&Services::line("mail-count", false, "gmail", &args)).unwrap();
        assert_eq!(
            read,
            json!({"op":"call","name":"service_read","skill":"mail-count",
                "args":{"what":"search","query":"is:unread","service":"gmail"}})
        );
        let write: Value =
            serde_json::from_str(&Services::line("s", true, "github", &Map::new())).unwrap();
        assert_eq!(write["name"], "service_write");
    }

    #[test]
    fn an_answer_is_its_result_and_a_refusal_its_words() {
        let dir =
            std::env::temp_dir().join(format!("lamp-control-services-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let socket = dir.join("call.sock");
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        let served = std::thread::spawn(move || {
            for answer in [
                r#"{"ok":true,"result":{"items":[]}}"#,
                r#"{"ok":false,"error":"Gmail isn't linked to this lamp yet."}"#,
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                writeln!(stream, "{answer}").unwrap();
            }
        });
        let services = Services::new(socket.clone());
        assert_eq!(
            services.call("{}").unwrap(),
            json!({"result": {"items": []}})
        );
        let refused = services.call("{}").unwrap_err();
        assert_eq!(refused.code, ErrorCode::Refused);
        assert_eq!(refused.why, "Gmail isn't linked to this lamp yet.");
        served.join().unwrap();
        // Nobody there: refused in words.
        let _ = std::fs::remove_file(&socket);
        assert_eq!(
            services.call("{}").unwrap_err().why,
            "lamp-services is not reachable"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
