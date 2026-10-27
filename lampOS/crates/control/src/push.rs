//! Skills pushed from a computer (docs/control.md, "Skills pushed from a
//! computer"): a package's parts gathered in order, checked against its
//! size and SHA-256, and kept for the manage helper with `push.json`
//! beside it, then the helper asked to install, list or remove.
//!
//! The door holds the grant, the parts, the sizes and the rates; the
//! helper (root's, in lamp-skills) holds the rest: it reads the package's
//! bytes once and hashes them again, loads it with every check a load
//! makes, holds it to the grants in `push.json`, keeps the list and the
//! cap of 16, and refuses another computer's name.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::protocol::{Error, ErrorCode};

/// The longest one push may take from its first part to its last, in µs.
pub const PUSH_WITHIN_US: u64 = 60_000_000;
/// The longest the helper may take to install a package: its load and
/// checks, then the install.
pub const INSTALL_WAIT: Duration = Duration::from_secs(60);
/// The longest the helper may take for a list, a remove or a turn off.
pub const ASK_WAIT: Duration = Duration::from_secs(10);
/// The folder in the state folder the package and `push.json` are kept in.
pub const PUSH_FOLDER: &str = "push";

/// What a part did.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Taken: this many parts are in.
    Got(u32),
    /// The last part: the package is whole, checked and kept, and the
    /// helper may be asked to install it.
    Whole { sha256: String },
}

/// A push being gathered.
struct Flight {
    name: String,
    sha256: String,
    size: usize,
    parts: u32,
    next: u32,
    bytes: Vec<u8>,
    started_us: u64,
}

/// One part's fields, as `skill.push` sends them.
pub struct Part<'a> {
    pub name: &'a str,
    pub sha256: &'a str,
    pub size: usize,
    pub part: u32,
    pub parts: u32,
    pub data: &'a str,
}

/// The one push in flight on the lamp, and whose, then the one install
/// the helper is doing: one at a time lamp-wide, as `push.json` is one
/// file (Eighth on #3504), so a package's bytes are held once at most.
#[derive(Default)]
pub struct Pushes {
    flight: Option<(String, Flight)>,
    installing: Option<String>,
}

impl Pushes {
    /// Whether `client` has a push in flight (part 0 taken, not finished).
    pub fn in_flight(&self, client: &str) -> bool {
        self.flight
            .as_ref()
            .is_some_and(|(whose, _)| whose == client)
    }

    /// One part from `client`. Part 0 starts a push, dropping one it had in
    /// flight (a broken push is sent again from 0, never resumed), and is
    /// `busy` while another computer's push is in flight (until it is over
    /// [`PUSH_WITHIN_US`] old) or any push is being installed. Any other
    /// part must be the next of this computer's push in flight, the same
    /// package, within [`PUSH_WITHIN_US`] of its first part, or the push is
    /// dropped. The last part checks the size and the SHA-256 and keeps the
    /// package with `push.json` in `folder` (none: not kept, for tests).
    pub fn part(
        &mut self,
        client: &str,
        part: &Part<'_>,
        now_us: u64,
        keep: Option<(&Path, &Value)>,
    ) -> Result<Step, Error> {
        let bad = |why: &str| Error::new(ErrorCode::BadRequest, why);
        if part.part == 0 {
            if let Some(installing) = &self.installing {
                let why = if installing == client {
                    "your last push is still being installed"
                } else {
                    "another computer's push is being installed"
                };
                return Err(Error::new(ErrorCode::Busy, why));
            }
            if let Some((whose, flight)) = &self.flight
                && whose != client
                && now_us.saturating_sub(flight.started_us) <= PUSH_WITHIN_US
            {
                return Err(Error::new(
                    ErrorCode::Busy,
                    "another computer is pushing a skill",
                ));
            }
            self.flight = Some((
                client.to_owned(),
                Flight {
                    name: part.name.to_owned(),
                    sha256: part.sha256.to_ascii_lowercase(),
                    size: part.size,
                    parts: part.parts,
                    next: 0,
                    bytes: Vec::with_capacity(part.size),
                    started_us: now_us,
                },
            ));
        }
        let Some(flight) = self
            .flight
            .as_mut()
            .filter(|(whose, _)| whose == client)
            .map(|(_, flight)| flight)
        else {
            return Err(bad("part: a push starts at part 0"));
        };
        let same = flight.name == part.name
            && flight.sha256 == part.sha256.to_ascii_lowercase()
            && flight.size == part.size
            && flight.parts == part.parts;
        let failed = if !same {
            Some(
                "part: not the push in flight (its name, sha256, size and parts are kept from part 0); send it again from 0",
            )
        } else if part.part != flight.next {
            Some("part: out of order; send the push again from 0")
        } else if now_us.saturating_sub(flight.started_us) > PUSH_WITHIN_US {
            Some("part: the push took over 60 s; send it again from 0")
        } else {
            None
        };
        if let Some(why) = failed {
            self.flight = None;
            return Err(bad(why));
        }
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(part.data) else {
            self.flight = None;
            return Err(bad("data: not base64; send the push again from 0"));
        };
        if flight.bytes.len() + bytes.len() > flight.size {
            self.flight = None;
            return Err(bad("data: more bytes than size"));
        }
        flight.bytes.extend_from_slice(&bytes);
        flight.next += 1;
        if flight.next < flight.parts {
            return Ok(Step::Got(flight.next));
        }
        let (_, flight) = self.flight.take().expect("in flight");
        if flight.bytes.len() != flight.size {
            return Err(bad("data: fewer bytes than size"));
        }
        let sha256 = hex(&Sha256::digest(&flight.bytes));
        if sha256 != flight.sha256 {
            return Err(bad("sha256: not the package's"));
        }
        if let Some((folder, about)) = keep {
            keep_package(folder, &flight.bytes, &sha256, &flight.name, about).map_err(|error| {
                eprintln!("lamp-control: skill.push: keeping the package: {error}");
                Error::new(ErrorCode::Refused, "the lamp could not keep the package")
            })?;
        }
        self.installing = Some(client.to_owned());
        Ok(Step::Whole { sha256 })
    }

    /// The helper has answered the install (or failed to): another may
    /// begin, and the package and `push.json` in `folder` are removed, as
    /// the helper has read them (Fourteen's P3 on #3910).
    pub fn installed(&mut self, folder: Option<&Path>) {
        self.installing = None;
        if let Some(folder) = folder {
            let push = folder.join(PUSH_FOLDER);
            for kept in ["push.json", "package.tar"] {
                // Gone already is fine; anything else is said (harness-
                // reviewer's P3 on #3910): at most 1 MiB, 0600, and the next
                // push writes over it.
                match std::fs::remove_file(push.join(kept)) {
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                        eprintln!("lamp-control: skill.push: removing {kept}: {error}");
                    }
                    _ => {}
                }
            }
        }
    }

    /// `client` revoked or gone for good: its push in flight is dropped.
    pub fn forget(&mut self, client: &str) {
        if self.in_flight(client) {
            self.flight = None;
        }
    }
}

/// The package and then `push.json` in `folder`/push, each written aside
/// and renamed into place, 0600, so the helper never reads half of one:
/// `push.json` holds the SHA-256, the name, and `about` (the computer, its
/// name, its grants now, and when).
fn keep_package(
    folder: &Path,
    bytes: &[u8],
    sha256: &str,
    name: &str,
    about: &Value,
) -> std::io::Result<()> {
    let push = folder.join(PUSH_FOLDER);
    std::fs::create_dir_all(&push)?;
    let mut said = about.clone();
    said["sha256"] = json!(sha256);
    said["name"] = json!(name);
    write_aside(&push.join("package.tar"), bytes)?;
    write_aside(&push.join("push.json"), said.to_string().as_bytes())
}

fn write_aside(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let aside = path.with_extension("new");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&aside)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&aside, path)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What the door asks of the manage helper for a client's request, and how
/// its answer is told.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    /// The request's id, which the answer carries.
    pub re: u64,
    /// The computer that asked.
    pub client: String,
    pub kind: AskKind,
    /// The line the helper is sent.
    pub request: Value,
    /// The computer's pairing generation when it asked (`Door`'s): an
    /// install answered under another one tells the helper again what the
    /// computer may have now.
    pub rev: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AskKind {
    Install,
    List,
    Remove,
}

impl Ask {
    pub fn install(re: u64, client: &str, sha256: &str) -> Self {
        Self {
            re,
            client: client.to_owned(),
            kind: AskKind::Install,
            request: json!({ "install_pushed": sha256 }),
            rev: 0,
        }
    }

    pub fn list(re: u64, client: &str) -> Self {
        Self {
            re,
            client: client.to_owned(),
            kind: AskKind::List,
            request: json!({ "list_pushed_by": client }),
            rev: 0,
        }
    }

    pub fn remove(re: u64, client: &str, name: &str) -> Self {
        Self {
            re,
            client: client.to_owned(),
            kind: AskKind::Remove,
            request: json!({ "remove_pushed": { "client": client, "name": name } }),
            rev: 0,
        }
    }

    /// How long the helper may take.
    pub fn wait(&self) -> Duration {
        match self.kind {
            AskKind::Install => INSTALL_WAIT,
            AskKind::List | AskKind::Remove => ASK_WAIT,
        }
    }

    /// The client's answer from the helper's (`{"done": …}` or
    /// `{"refused": WHY}`, with `"code": "no_grant"` when the skill asks
    /// for more than the pairing grants), or from why it could not be
    /// asked.
    pub fn answer(&self, helper: Result<Value, String>) -> Value {
        let code = match helper.as_ref().ok().and_then(|answer| answer.get("code")) {
            Some(Value::String(code)) if code == "no_grant" => ErrorCode::NoGrant,
            _ => ErrorCode::Refused,
        };
        let refused = |why: String| crate::protocol::refused(Some(self.re), &Error::new(code, why));
        match helper {
            Ok(answer) => match (answer.get("done"), answer.get("refused")) {
                (Some(done), None) => match self.kind {
                    AskKind::Install => json!({"re": self.re, "ok": true, "skill": done}),
                    AskKind::List => json!({"re": self.re, "ok": true, "skills": done}),
                    AskKind::Remove => json!({"re": self.re, "ok": true}),
                },
                (None, Some(Value::String(why))) => refused(why.clone()),
                _ => refused("the lamp's skill helper answered something else".to_owned()),
            },
            Err(why) => {
                eprintln!("lamp-control: the manage helper: {why}");
                refused("the lamp's skill helper did not answer".to_owned())
            }
        }
    }
}

/// `request` asked of the manage helper at `socket`, one line each way,
/// waiting at most `within` for the answer.
pub fn ask_helper(socket: &Path, request: &Value, within: Duration) -> Result<Value, String> {
    let mut stream = UnixStream::connect(socket).map_err(|error| format!("connect: {error}"))?;
    stream
        .set_write_timeout(Some(ASK_WAIT))
        .and_then(|()| stream.set_read_timeout(Some(within)))
        .map_err(|error| format!("timeouts: {error}"))?;
    writeln!(stream, "{request}").map_err(|error| format!("write: {error}"))?;
    let mut line = String::new();
    BufReader::new(std::io::Read::take(stream, ANSWER_MOST))
        .read_line(&mut line)
        .map_err(|error| format!("read: {error}"))?;
    serde_json::from_str(line.trim_end()).map_err(|error| format!("answer: {error}"))
}

/// The most an answer may hold: a list of 16 skills, well within.
const ANSWER_MOST: u64 = 64 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(bytes: &[u8]) -> String {
        hex(&Sha256::digest(bytes))
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn parts_in_order_make_a_checked_package_kept_with_push_json() {
        let folder = std::env::temp_dir().join(format!("lamp-control-push-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("folder");
        let package = b"0123456789abcdef".repeat(10);
        let sha256 = sha(&package);
        let about =
            json!({"client": "c_1", "client_name": "desk", "grants": ["body"], "at_unix_ms": 5});
        let mut pushes = Pushes::default();
        let chunks: Vec<&[u8]> = package.chunks(64).collect();
        for (n, chunk) in chunks.iter().enumerate() {
            let data = b64(chunk);
            let part = Part {
                name: "desk-wave",
                sha256: &sha256,
                size: package.len(),
                part: n as u32,
                parts: chunks.len() as u32,
                data: &data,
            };
            let step = pushes
                .part(
                    "c_1",
                    &part,
                    1_000 + n as u64,
                    Some((folder.as_path(), &about)),
                )
                .expect("taken");
            if n + 1 < chunks.len() {
                assert_eq!(step, Step::Got(n as u32 + 1));
            } else {
                assert_eq!(
                    step,
                    Step::Whole {
                        sha256: sha256.clone()
                    }
                );
            }
        }
        let kept = folder.join(PUSH_FOLDER);
        assert_eq!(
            std::fs::read(kept.join("package.tar")).expect("package"),
            package
        );
        let said: Value =
            serde_json::from_slice(&std::fs::read(kept.join("push.json")).expect("push.json"))
                .expect("json");
        assert_eq!(said["sha256"], json!(sha256));
        assert_eq!(said["name"], json!("desk-wave"));
        assert_eq!(said["client"], json!("c_1"));
        assert_eq!(said["grants"], json!(["body"]));
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(kept.join("package.tar"))
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_part_out_of_order_late_or_of_another_package_drops_the_push() {
        let package = vec![7u8; 100];
        let sha256 = sha(&package);
        let half = b64(&package[..50]);
        let part = |n: u32, sha256: &str| Part {
            name: "desk-wave",
            sha256: Box::leak(sha256.to_owned().into_boxed_str()),
            size: 100,
            part: n,
            parts: 2,
            data: Box::leak(half.clone().into_boxed_str()),
        };
        let code = |result: Result<Step, Error>| result.expect_err("refused").code;
        let mut pushes = Pushes::default();
        // Not started at 0.
        assert_eq!(
            code(pushes.part("c", &part(1, &sha256), 0, None)),
            ErrorCode::BadRequest
        );
        // Out of order: part 0 twice is a fresh start, part 1 after it fine,
        // but a part 1 again is past the end of nothing.
        pushes.part("c", &part(0, &sha256), 0, None).expect("first");
        let other = "f".repeat(64);
        assert_eq!(
            code(pushes.part("c", &part(1, &other), 1, None)),
            ErrorCode::BadRequest
        );
        assert!(
            !pushes.in_flight("c"),
            "another package's part drops the push"
        );
        // Late.
        pushes.part("c", &part(0, &sha256), 0, None).expect("first");
        assert_eq!(
            code(pushes.part("c", &part(1, &sha256), PUSH_WITHIN_US + 1, None)),
            ErrorCode::BadRequest
        );
        assert!(!pushes.in_flight("c"));
    }

    #[test]
    fn a_wrong_sha_or_size_is_refused_and_one_install_at_a_time() {
        let package = vec![7u8; 100];
        let data = b64(&package);
        let whole = |sha256: &str, size: usize| Part {
            name: "desk-wave",
            sha256: Box::leak(sha256.to_owned().into_boxed_str()),
            size,
            part: 0,
            parts: 1,
            data: Box::leak(data.clone().into_boxed_str()),
        };
        let mut pushes = Pushes::default();
        let wrong = "0".repeat(64);
        let refused = pushes
            .part("c", &whole(&wrong, 100), 0, None)
            .expect_err("sha");
        assert!(refused.why.contains("sha256"), "{refused:?}");
        let refused = pushes
            .part("c", &whole(&sha(&package), 99), 0, None)
            .expect_err("size");
        assert!(refused.why.contains("more bytes"), "{refused:?}");
        let refused = pushes
            .part("c", &whole(&sha(&package), 101), 0, None)
            .expect_err("size");
        assert!(refused.why.contains("fewer bytes"), "{refused:?}");
        // One install at a time on the lamp, whoever asks.
        pushes
            .part("c", &whole(&sha(&package), 100), 0, None)
            .expect("whole");
        let busy = pushes
            .part("d", &whole(&sha(&package), 100), 0, None)
            .expect_err("busy");
        assert_eq!(busy.code, ErrorCode::Busy);
        pushes.installed(None);
        pushes
            .part("d", &whole(&sha(&package), 100), 0, None)
            .expect("after");
    }

    #[test]
    fn the_helpers_answers_become_the_clients() {
        let install = Ask::install(9, "c_1", &"a".repeat(64));
        assert_eq!(install.request, json!({"install_pushed": "a".repeat(64)}));
        let skill = json!({"name": "desk-wave", "on": true});
        assert_eq!(
            install.answer(Ok(json!({"done": skill}))),
            json!({"re": 9, "ok": true, "skill": skill})
        );
        assert_eq!(
            install.answer(Ok(json!({"refused": "run.sh: a skill has no scripts"}))),
            json!({"re": 9, "ok": false, "error": {"code": "refused", "why": "run.sh: a skill has no scripts"}})
        );
        assert_eq!(
            install.answer(Ok(
                json!({"refused": "sound: waits for format 2", "code": "no_grant"})
            )),
            json!({"re": 9, "ok": false, "error": {"code": "no_grant", "why": "sound: waits for format 2"}})
        );
        assert_eq!(
            install.answer(Err("connect: no such file".to_owned()))["error"]["why"],
            json!("the lamp's skill helper did not answer")
        );
        let list = Ask::list(10, "c_1");
        assert_eq!(list.request, json!({"list_pushed_by": "c_1"}));
        assert_eq!(
            list.answer(Ok(json!({"done": []}))),
            json!({"re": 10, "ok": true, "skills": []})
        );
        let remove = Ask::remove(11, "c_1", "desk-wave");
        assert_eq!(
            remove.request,
            json!({"remove_pushed": {"client": "c_1", "name": "desk-wave"}})
        );
        assert_eq!(
            remove.answer(Ok(json!({"done": "removed"}))),
            json!({"re": 11, "ok": true})
        );
    }
}
