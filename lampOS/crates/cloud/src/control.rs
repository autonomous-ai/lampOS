//! The developer door from the owner's app (#5299, docs/cloud.md, "The
//! developer door"): `control.code`, `control.clients` and `control.revoke`,
//! each one line asked of lamp-control's admin socket (docs/control.md,
//! "Running the door"), as `lamp-control code`, `clients` and `revoke` ask
//! it at the lamp. Only with `--control-admin-socket`, and named in `can`
//! only while that socket is there, so an app never shows a door the lamp
//! can't open.
//!
//! `control.code`'s code and pin pair a computer, so they never travel in
//! the clear: the app sends a one-time X25519 public key (`seal_to`) and the
//! answer is sealed to it ([`crate::seal`], HPKE), bound to this lamp's
//! device id and the app's `request_id`. An app that sends no key is refused
//! before any code is made, so an older app never voids the owner's code
//! out, and never sees one in the clear. Neither the code, the pin, the
//! sealed message nor the app's key is in a journal line or an error.
//!
//! The owner's alone, as every kind here is: only the lamp's own account
//! channel commands it (`online::commanded`).
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The kinds, in `can`'s order.
pub const KINDS: [&str; 6] = [
    "control.code",
    "control.clients",
    "control.revoke",
    "control.code_cancel",
    "control.take_back",
    "control.set_grants",
];

/// lamp-control's admin socket on a lamp (`lamp-control serve
/// --admin-socket`'s default), for the unit's `--control-admin-socket`.
pub const ADMIN_SOCKET: &str = "/run/lampos-control/admin.sock";

/// The HPKE `info` every code is sealed under.
pub const INFO: &[u8] = b"lampos/v1/control.code";

/// The sealed answer's format, so a later one can be told apart.
pub const SEALED_V: u64 = 1;

/// lamp-control answers its admin socket at once; the broker's loop waits
/// on it, so it is bounded short.
pub const WAIT: Duration = Duration::from_secs(3);

/// The most bytes read of lamp-control's answer: 16 computers' rows at most.
const ANSWER_MOST: u64 = 64 * 1024;

/// A code lasts this long on lamp-control (`pairing::CODE_FOR_US`), for an
/// older lamp-control that doesn't say `until_unix_ms`.
const CODE_FOR_MS: u64 = 5 * 60 * 1_000;

/// The app's refusals, shown as they are.
pub const MUST_SEAL: &str = "this app must seal the code: update the app";
pub const NO_REQUEST_ID: &str = "this app must send a request_id with control.code";
pub const BAD_GRANTS: &str = "grants must be a list of names";
pub const UNKNOWN_GRANT: &str = "a grant this lamp doesn't know";
pub const BAD_CLIENT: &str = "client must be a computer's id";
/// The computer `control.set_grants` names is not paired.
pub const NO_SUCH_COMPUTER: &str = "no such computer";
/// lamp-control could not keep the new grants: the computer keeps the ones it had.
pub const GRANTS_NOT_SAVED: &str = "not saved, so the computer keeps the grants it had";
/// lamp-control's socket isn't there or refused the connection: its switch
/// is off, or it isn't running.
pub const OFF: &str = "developer access is off on this lamp";
/// Connected, but no answer that can be read in time.
pub const UNREACHED: &str = "developer access did not answer on the lamp now";
/// The code couldn't be sealed (the system's random source or the cipher).
pub const NOT_SEALED: &str = "the code could not be sealed on the lamp now";

/// lamp-control's admin socket, and the device id each sealed code is bound
/// to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Door {
    pub socket: PathBuf,
    /// The backend's device id, as every reply's `id` says it.
    pub device_id: String,
}

impl Door {
    /// Whether lamp-control's socket is there: `can` names the kinds only
    /// then. lamp-control's runtime folder goes with it when it stops.
    pub fn here(&self) -> bool {
        self.socket.exists()
    }
}

/// `kind`, one of [`KINDS`], with the app's `data`: its reply's `data`, or
/// the refusal the app shows. What it did goes to the journal.
pub fn answer(door: &Door, kind: &str, data: &Value) -> Result<Option<Value>, String> {
    answer_saying(door, kind, data, &mut |line| {
        eprintln!("lamp-cloud: {line}")
    })
}

/// [`answer`], its journal lines handed to `say`.
pub fn answer_saying(
    door: &Door,
    kind: &str,
    data: &Value,
    say: &mut dyn FnMut(&str),
) -> Result<Option<Value>, String> {
    match kind {
        "control.code" => code(door, data, say),
        "control.clients" => clients(door, say),
        "control.revoke" => revoke(door, data, say),
        "control.code_cancel" => code_cancel(door, say),
        "control.take_back" => take_back(door, data, say),
        "control.set_grants" => set_grants(door, data, say),
        other => Err(crate::wire::not_here(other)),
    }
}

/// `{"grants": [...], "seal_to", "request_id"}` → `{"sealed": {"v", "enc",
/// "ct"}, "until_unix_ms", "shown"}`, `ct` opening to `{"code", "pin",
/// "until_unix_ms"}`. Everything the app sent is checked before lamp-control
/// is asked, since a new code voids the one out.
fn code(door: &Door, data: &Value, say: &mut dyn FnMut(&str)) -> Result<Option<Value>, String> {
    let seal_to = data
        .get("seal_to")
        .and_then(Value::as_str)
        .and_then(key_of)
        .ok_or(MUST_SEAL)?;
    let request_id = data
        .get("request_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_graphic()))
        .ok_or(NO_REQUEST_ID)?;
    let grants = match data.get("grants") {
        Some(Value::Array(grants)) if grants.iter().all(Value::is_string) => grants.clone(),
        None => Vec::new(),
        Some(_) => return Err(BAD_GRANTS.to_owned()),
    };
    // The lamp says the start of its pin aloud for the owner to match
    // (the spoken pin check, #5299).
    let asked = json!({"code": {"grants": grants, "say_pin": true}});
    let issued = ask(&door.socket, &asked, "control.code", say)?;
    if let Some(error) = issued.get("error").and_then(Value::as_str) {
        // lamp-control's own words name the grant; the app is told plainly.
        say(&format!(
            "control.code from the app refused: {}",
            short(error)
        ));
        return Err(if error.starts_with("grants") {
            UNKNOWN_GRANT.to_owned()
        } else {
            UNREACHED.to_owned()
        });
    }
    let (Some(code), Some(pin)) = (
        issued.get("code").and_then(Value::as_str),
        issued.get("pin").and_then(Value::as_str),
    ) else {
        say("control.code from the app: lamp-control's answer had no code");
        return Err(UNREACHED.to_owned());
    };
    let until_unix_ms = issued
        .get("until_unix_ms")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| crate::chat::now_ms().saturating_add(CODE_FOR_MS));
    let opened = json!({"code": code, "pin": pin, "until_unix_ms": until_unix_ms});
    let aad = format!("{}\n{request_id}", door.device_id);
    let sealed = crate::seal::seal(
        &seal_to,
        INFO,
        aad.as_bytes(),
        opened.to_string().as_bytes(),
    )
    .map_err(|refused| match refused {
        crate::seal::Refused::BadKey => {
            say("control.code from the app: its key is no X25519 key, nothing sealed");
            MUST_SEAL.to_owned()
        }
        crate::seal::Refused::Failed => {
            say("control.code from the app: the seal failed");
            NOT_SEALED.to_owned()
        }
    })?;
    say(&format!(
        "control.code from the app: a code for {} grants, sealed",
        grants.len()
    ));
    Ok(Some(json!({
        "sealed": {
            "v": SEALED_V,
            "enc": URL_SAFE_NO_PAD.encode(sealed.enc),
            "ct": URL_SAFE_NO_PAD.encode(&sealed.ct),
        },
        "until_unix_ms": until_unix_ms,
        "shown": issued.get("shown").cloned().unwrap_or_else(|| json!([])),
        // Whether the lamp's voice took the spoken pin check; an older
        // lamp-control says nothing, and the app has no check to match.
        "pin_said": issued.get("pin_said").and_then(Value::as_bool).unwrap_or(false),
    })))
}

/// `{}` → `{"clients": [{"id", "name", "grants", "paired_at_unix_ms"}]}`.
fn clients(door: &Door, say: &mut dyn FnMut(&str)) -> Result<Option<Value>, String> {
    let listed = ask(
        &door.socket,
        &json!({"clients": true}),
        "control.clients",
        say,
    )?;
    let Some(rows) = listed.get("clients").and_then(Value::as_array) else {
        say("control.clients from the app: lamp-control's answer had no list");
        return Err(UNREACHED.to_owned());
    };
    let clients: Vec<Value> = rows
        .iter()
        .map(|row| {
            // An older lamp-control says only `paired_at`, in seconds.
            let paired_at_unix_ms = row
                .get("paired_at_unix_ms")
                .and_then(Value::as_u64)
                .or_else(|| {
                    row.get("paired_at")
                        .and_then(Value::as_u64)
                        .map(|seconds| seconds.saturating_mul(1_000))
                });
            json!({
                "id": row.get("id").cloned().unwrap_or(Value::Null),
                "name": row.get("name").cloned().unwrap_or(Value::Null),
                "grants": row.get("grants").cloned().unwrap_or_else(|| json!([])),
                "paired_at_unix_ms": paired_at_unix_ms,
            })
        })
        .collect();
    say(&format!(
        "control.clients from the app: {} computers",
        clients.len()
    ));
    // The words for every grant the lamp knows, once, for a grant the app has no
    // label of its own for; an older lamp-control says none.
    let mut answer = json!({"clients": clients});
    if let Some(shown) = listed.get("shown").filter(|shown| shown.is_object()) {
        answer["shown"] = shown.clone();
    }
    Ok(Some(answer))
}

/// `{"client": "c_…"}` → `{"revoked", "saved"}`, with lamp-control's words
/// as `why` when `saved` is false: refused at once either way, but a restart
/// before a save succeeds would bring the computer back.
fn revoke(door: &Door, data: &Value, say: &mut dyn FnMut(&str)) -> Result<Option<Value>, String> {
    let client = data
        .get("client")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_graphic()))
        .ok_or(BAD_CLIENT)?;
    let revoked = ask(
        &door.socket,
        &json!({"revoke": client}),
        "control.revoke",
        say,
    )?;
    let (Some(was), Some(saved)) = (
        revoked.get("revoked").and_then(Value::as_bool),
        revoked.get("saved").and_then(Value::as_bool),
    ) else {
        say("control.revoke from the app: lamp-control's answer can't be read");
        return Err(UNREACHED.to_owned());
    };
    let mut answer = json!({"revoked": was, "saved": saved});
    if !saved {
        answer["why"] = json!(
            revoked
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("not saved yet, so a restart now would bring it back")
        );
    }
    say(&format!(
        "control.revoke from the app: {client} revoked {was}, saved {saved}"
    ));
    Ok(Some(answer))
}

/// `{}` → `{"dropped": true|false}`: the code out is dropped, so nobody can
/// pair with it. The app sends it when the lamp's spoken pin check doesn't
/// match its own, or when its screen closes before the code is used.
fn code_cancel(door: &Door, say: &mut dyn FnMut(&str)) -> Result<Option<Value>, String> {
    let answered = ask(
        &door.socket,
        &json!({"cancel_code": true}),
        "control.code_cancel",
        say,
    )?;
    let Some(dropped) = answered.get("dropped").and_then(Value::as_bool) else {
        say("control.code_cancel from the app: lamp-control's answer can't be read");
        return Err(UNREACHED.to_owned());
    };
    say(&format!(
        "control.code_cancel from the app: the pairing code dropped ({dropped})"
    ));
    Ok(Some(json!({"dropped": dropped})))
}

/// `{}` or `{"client": "c_1"}` → `{"taken": N}`: the owner takes the lamp
/// back from one computer, or from every computer, which keep their
/// pairing. Their leases end as `taken_by_owner_app`.
fn take_back(
    door: &Door,
    data: &Value,
    say: &mut dyn FnMut(&str),
) -> Result<Option<Value>, String> {
    let asked = match data.get("client") {
        None => json!({"take_back": true}),
        Some(client) => {
            let client = client
                .as_str()
                .filter(|id| {
                    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_graphic())
                })
                .ok_or(BAD_CLIENT)?;
            json!({"take_back": true, "client": client})
        }
    };
    let answered = ask(&door.socket, &asked, "control.take_back", say)?;
    let Some(taken) = answered.get("taken").and_then(Value::as_u64) else {
        say("control.take_back from the app: lamp-control's answer can't be read");
        return Err(UNREACHED.to_owned());
    };
    say(&format!(
        "control.take_back from the app: {} lease(s) taken back from {}",
        taken,
        asked
            .get("client")
            .and_then(Value::as_str)
            .unwrap_or("every computer")
    ));
    Ok(Some(json!({"taken": taken})))
}

/// `{"client": "c_1", "grants": ["body", "camera"]}` → `{"client", "grants",
/// "added", "removed", "saved": true}`: the computer's grants become exactly
/// that list, with no new code. The owner's tap in the app is the consent
/// (the same channel and guard as a code); lamp-control saves it, ends a
/// stream whose grant goes, and closes the computer's connections so it reads
/// its new grants at its next `hello`.
fn set_grants(
    door: &Door,
    data: &Value,
    say: &mut dyn FnMut(&str),
) -> Result<Option<Value>, String> {
    let client = data
        .get("client")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_graphic()))
        .ok_or(BAD_CLIENT)?;
    let grants = match data.get("grants") {
        Some(Value::Array(grants)) if grants.iter().all(Value::is_string) => grants.clone(),
        _ => return Err(BAD_GRANTS.to_owned()),
    };
    let asked = json!({"set_grants": {"client": client, "grants": grants}});
    let answered = ask(&door.socket, &asked, "control.set_grants", say)?;
    if let Some(error) = answered.get("error").and_then(Value::as_str) {
        say(&format!(
            "control.set_grants from the app refused: {}",
            short(error)
        ));
        return Err(if error == "no such computer" {
            NO_SUCH_COMPUTER.to_owned()
        } else if error.starts_with("set_grants: grants") {
            // lamp-control wants the whole list (a missing `grants` is refused): the app sends it, so this is a slip.
            BAD_GRANTS.to_owned()
        } else if error.starts_with("grants") {
            UNKNOWN_GRANT.to_owned()
        } else if error.starts_with("not saved") {
            GRANTS_NOT_SAVED.to_owned()
        } else {
            UNREACHED.to_owned()
        });
    }
    let names = |key: &str| answered.get(key).filter(|v| v.is_array()).cloned();
    let (Some(now), Some(added), Some(removed), Some(true)) = (
        names("grants"),
        names("added"),
        names("removed"),
        answered.get("saved").and_then(Value::as_bool),
    ) else {
        say("control.set_grants from the app: lamp-control's answer can't be read");
        return Err(UNREACHED.to_owned());
    };
    say(&format!(
        "control.set_grants from the app: {client} now has {} grant(s), added {added}, removed {removed}",
        now.as_array().map_or(0, Vec::len)
    ));
    let mut answer = json!({
        "client": client,
        "grants": now,
        "added": added,
        "removed": removed,
        "saved": true,
    });
    if let Some(shown) = answered.get("shown").filter(|shown| shown.is_object()) {
        answer["shown"] = shown.clone();
    }
    Ok(Some(answer))
}

/// The app's one-time X25519 public key: 32 bytes of base64url, padded or
/// not.
fn key_of(text: &str) -> Option<[u8; crate::seal::KEY_BYTES]> {
    URL_SAFE_NO_PAD
        .decode(text.trim_end_matches('='))
        .ok()?
        .try_into()
        .ok()
}

/// One line asked of lamp-control's admin socket, and its one line back.
fn ask(
    socket: &Path,
    asked: &Value,
    kind: &str,
    say: &mut dyn FnMut(&str),
) -> Result<Value, String> {
    let mut stream = UnixStream::connect(socket).map_err(|error| {
        say(&format!(
            "{kind} from the app: lamp-control not reached: {error}"
        ));
        OFF.to_owned()
    })?;
    let mut exchanged = || -> std::io::Result<String> {
        stream.set_read_timeout(Some(WAIT))?;
        stream.set_write_timeout(Some(WAIT))?;
        stream.write_all(format!("{asked}\n").as_bytes())?;
        let mut line = String::new();
        BufReader::new((&stream).take(ANSWER_MOST)).read_line(&mut line)?;
        Ok(line)
    };
    let line = exchanged().map_err(|error| {
        say(&format!(
            "{kind} from the app: lamp-control did not answer: {error}"
        ));
        UNREACHED.to_owned()
    })?;
    serde_json::from_str(line.trim()).map_err(|_| {
        say(&format!(
            "{kind} from the app: lamp-control's answer can't be read"
        ));
        UNREACHED.to_owned()
    })
}

/// lamp-control's own words, bounded for a journal line.
fn short(text: &str) -> String {
    text.chars().take(120).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// A short folder of its own: a socket's whole path fits in 103 bytes.
    fn folder(tag: &str) -> PathBuf {
        let dir = Path::new("/tmp").join(format!("cc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A stand-in for lamp-control's admin socket: each line asked, and the
    /// answer `answer` gives it.
    fn admin(dir: &Path, answer: fn(&Value) -> Value) -> (Door, std::sync::mpsc::Receiver<Value>) {
        let socket = dir.join("admin.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (sent, asked) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let line: Value = serde_json::from_str(&line).unwrap();
                let reply = answer(&line);
                let _ = sent.send(line);
                let _ = (&stream).write_all(format!("{reply}\n").as_bytes());
            }
        });
        let door = Door {
            socket,
            device_id: "d-42".into(),
        };
        (door, asked)
    }

    const CODE: &str = "40719283";
    const PIN: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    fn issues(asked: &Value) -> Value {
        if asked["code"]["grants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|grant| grant == "air")
        {
            return json!({"error": "grants: unknown variant `air`"});
        }
        json!({
            "code": CODE,
            "pin": PIN,
            "shown": ["can move the lamp's body", "can change the light"],
            "until_unix_ms": 1_800_000_300_000u64,
            "pin_said": asked["code"]["say_pin"] == true,
        })
    }

    const APP_KEY: [u8; 32] = [9; 32];

    fn asked_with(seal_to: &str, request_id: &str) -> Value {
        json!({"grants": ["body", "light"], "seal_to": seal_to, "request_id": request_id})
    }

    /// The app's key, sealed to: the code and pin open only with it, under
    /// this lamp's id and the app's request id; nothing the lamp said or
    /// wrote holds either.
    #[test]
    fn a_code_goes_sealed_to_the_apps_key_and_nowhere_in_the_clear() {
        let dir = folder("code");
        let (door, asked) = admin(&dir, issues);
        let seal_to = URL_SAFE_NO_PAD.encode(crate::seal::public_of(&APP_KEY));
        let mut lines = Vec::new();
        let answered = answer_saying(
            &door,
            "control.code",
            &asked_with(&seal_to, "r-7"),
            &mut |line| lines.push(line.to_owned()),
        )
        .unwrap()
        .unwrap();
        // The lamp is asked to say its pin check aloud, and the app is told
        // it did.
        assert_eq!(
            asked.recv().unwrap(),
            json!({"code": {"grants": ["body", "light"], "say_pin": true}})
        );
        assert_eq!(answered["pin_said"], true);
        assert_eq!(answered["sealed"]["v"], 1);
        assert_eq!(answered["until_unix_ms"], 1_800_000_300_000u64);
        assert_eq!(
            answered["shown"],
            json!(["can move the lamp's body", "can change the light"])
        );
        let sealed = crate::seal::Sealed {
            enc: URL_SAFE_NO_PAD
                .decode(answered["sealed"]["enc"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap(),
            ct: URL_SAFE_NO_PAD
                .decode(answered["sealed"]["ct"].as_str().unwrap())
                .unwrap(),
        };
        let opened = crate::seal::open(&APP_KEY, &sealed, INFO, b"d-42\nr-7").unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&opened).unwrap(),
            json!({"code": CODE, "pin": PIN, "until_unix_ms": 1_800_000_300_000u64})
        );
        // Bound to this lamp and this request.
        assert!(crate::seal::open(&APP_KEY, &sealed, INFO, b"d-42\nr-8").is_err());
        assert!(crate::seal::open(&APP_KEY, &sealed, INFO, b"d-43\nr-7").is_err());
        assert!(crate::seal::open(&[1; 32], &sealed, INFO, b"d-42\nr-7").is_err());

        assert_eq!(
            lines,
            ["control.code from the app: a code for 2 grants, sealed"]
        );
        let reply = answered.to_string();
        let ct = answered["sealed"]["ct"].as_str().unwrap();
        for said in lines.iter().chain([&reply]) {
            for secret in [CODE, PIN, &PIN[..16]] {
                assert!(!said.contains(secret), "{said}");
            }
        }
        for line in &lines {
            assert!(!line.contains(ct) && !line.contains(&seal_to), "{line}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// No key, or one that isn't 32 bytes of base64url: refused with the
    /// app's words, and lamp-control never asked, so the owner's code out
    /// stays good. No request id or bad grants likewise.
    #[test]
    fn an_app_that_cant_seal_is_refused_before_any_code_is_made() {
        let dir = folder("refuse");
        let (door, asked) = admin(&dir, issues);
        let good = URL_SAFE_NO_PAD.encode(crate::seal::public_of(&APP_KEY));
        let short_key = URL_SAFE_NO_PAD.encode([1u8; 31]);
        let refusals = [
            (json!({"grants": ["body"], "request_id": "r-1"}), MUST_SEAL),
            (asked_with("", "r-1"), MUST_SEAL),
            (asked_with("not base64!", "r-1"), MUST_SEAL),
            (asked_with(&short_key, "r-1"), MUST_SEAL),
            (
                json!({"grants": ["body"], "seal_to": 7, "request_id": "r-1"}),
                MUST_SEAL,
            ),
            (json!({"grants": ["body"], "seal_to": good}), NO_REQUEST_ID),
            (asked_with(&good, ""), NO_REQUEST_ID),
            (asked_with(&good, "a b"), NO_REQUEST_ID),
            (
                json!({"grants": "body", "seal_to": good, "request_id": "r-1"}),
                BAD_GRANTS,
            ),
            (
                json!({"grants": [1], "seal_to": good, "request_id": "r-1"}),
                BAD_GRANTS,
            ),
        ];
        for (data, why) in refusals {
            assert_eq!(
                answer_saying(&door, "control.code", &data, &mut |_| {}),
                Err(why.to_owned()),
                "{data}"
            );
        }
        assert!(asked.try_recv().is_err(), "lamp-control was asked");
        // A small-order key passes as base64 but is refused at the seal.
        let zero = URL_SAFE_NO_PAD.encode([0u8; 32]);
        let mut lines = Vec::new();
        assert_eq!(
            answer_saying(
                &door,
                "control.code",
                &asked_with(&zero, "r-1"),
                &mut |line| { lines.push(line.to_owned()) }
            ),
            Err(MUST_SEAL.to_owned())
        );
        assert!(
            lines
                .iter()
                .all(|line| !line.contains(CODE) && !line.contains(PIN))
        );
        // A grant lamp-control doesn't know: its words in the journal only.
        let mut lines = Vec::new();
        let unknown = json!({"grants": ["air"], "seal_to": good, "request_id": "r-1"});
        assert_eq!(
            answer_saying(&door, "control.code", &unknown, &mut |line| {
                lines.push(line.to_owned())
            }),
            Err(UNKNOWN_GRANT.to_owned())
        );
        assert_eq!(
            lines,
            ["control.code from the app refused: grants: unknown variant `air`"]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Each row in the app's shape; an older lamp-control's seconds made
    /// milliseconds.
    #[test]
    fn the_computers_are_listed_with_milliseconds() {
        let dir = folder("clients");
        let (door, asked) = admin(&dir, |_| {
            json!({"clients": [
                {"id": "c_1", "name": "laptop", "grants": ["body"], "paired_at": 1_800_000_000u64, "paired_at_unix_ms": 1_800_000_000_000u64},
                {"id": "c_2", "name": "older", "grants": [], "paired_at": 1_700_000_000u64},
            ]})
        });
        assert_eq!(
            answer(&door, "control.clients", &json!({})),
            Ok(Some(json!({"clients": [
                {"id": "c_1", "name": "laptop", "grants": ["body"], "paired_at_unix_ms": 1_800_000_000_000u64},
                {"id": "c_2", "name": "older", "grants": [], "paired_at_unix_ms": 1_700_000_000_000u64},
            ]})))
        );
        assert_eq!(asked.recv().unwrap(), json!({"clients": true}));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The words for every grant, once per answer, when lamp-control says them
    /// (an older one says none, and the answer has no `shown`).
    #[test]
    fn the_words_for_every_grant_come_once_with_the_list() {
        let dir = folder("clients-shown");
        let (door, _asked) = admin(&dir, |_| {
            json!({"clients": [{"id": "c_1", "name": "laptop", "grants": ["body"], "paired_at_unix_ms": 1u64}],
                   "shown": {"body": "can move the lamp's body", "microphone": "can hear what the lamp's microphone hears"}})
        });
        let answered = answer(&door, "control.clients", &json!({}))
            .unwrap()
            .unwrap();
        assert_eq!(
            answered["shown"]["microphone"],
            "can hear what the lamp's microphone hears"
        );
        assert_eq!(answered["clients"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A revoke passes lamp-control's answer on, its words as `why` when
    /// not saved; a bad id is refused without asking.
    #[test]
    fn a_revoke_says_whether_it_was_saved() {
        let dir = folder("revoke");
        let (door, asked) = admin(&dir, |asked| {
            if asked["revoke"] == "c_1" {
                json!({"revoked": true, "saved": true})
            } else {
                json!({"revoked": true, "saved": false, "error": "not saved yet, so a restart now would bring it back: disk full"})
            }
        });
        assert_eq!(
            answer(&door, "control.revoke", &json!({"client": "c_1"})),
            Ok(Some(json!({"revoked": true, "saved": true})))
        );
        assert_eq!(asked.recv().unwrap(), json!({"revoke": "c_1"}));
        assert_eq!(
            answer(&door, "control.revoke", &json!({"client": "c_2"})),
            Ok(Some(json!({
                "revoked": true,
                "saved": false,
                "why": "not saved yet, so a restart now would bring it back: disk full",
            })))
        );
        assert_eq!(asked.recv().unwrap(), json!({"revoke": "c_2"}));
        for bad in [json!({}), json!({"client": ""}), json!({"client": 3})] {
            assert_eq!(
                answer(&door, "control.revoke", &bad),
                Err(BAD_CLIENT.to_owned())
            );
        }
        assert!(asked.try_recv().is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `control.set_grants` passes lamp-control's answer on, and says its
    /// refusals plainly; a bad id or a grants list that isn't names is
    /// refused without asking.
    #[test]
    fn set_grants_passes_the_answer_and_says_the_refusals() {
        let dir = folder("set-grants");
        let (door, asked) = admin(&dir, |asked| match asked["set_grants"]["client"].as_str() {
            Some("c_1") => json!({
                "client": "c_1", "grants": ["camera", "light"],
                "added": ["camera"], "removed": [], "saved": true,
                "shown": {"camera": "can see what the lamp's camera sees"}
            }),
            Some("c_2") => json!({"error": "no such computer"}),
            Some("c_3") => json!({"error": "grants: unknown variant `teleport`"}),
            Some("c_5") => json!({"error": "set_grants: grants: the whole list, [] for none"}),
            _ => json!({"error": "not saved, so the computer keeps the grants it had: disk full"}),
        });
        let ask = |client: &str| {
            answer(
                &door,
                "control.set_grants",
                &json!({"client": client, "grants": ["light"]}),
            )
        };
        assert_eq!(
            ask("c_1"),
            Ok(Some(json!({
                "client": "c_1", "grants": ["camera", "light"],
                "added": ["camera"], "removed": [], "saved": true,
                "shown": {"camera": "can see what the lamp's camera sees"}
            })))
        );
        assert_eq!(
            asked.recv().unwrap(),
            json!({"set_grants": {"client": "c_1", "grants": ["light"]}})
        );
        assert_eq!(ask("c_2"), Err(NO_SUCH_COMPUTER.to_owned()));
        assert_eq!(ask("c_3"), Err(UNKNOWN_GRANT.to_owned()));
        assert_eq!(ask("c_4"), Err(GRANTS_NOT_SAVED.to_owned()));
        assert_eq!(ask("c_5"), Err(BAD_GRANTS.to_owned()));
        for _ in 0..4 {
            asked.recv().unwrap();
        }
        for bad in [
            json!({"grants": ["light"]}),
            json!({"client": "", "grants": []}),
            json!({"client": "c_1"}),
            json!({"client": "c_1", "grants": [3]}),
        ] {
            assert!(answer(&door, "control.set_grants", &bad).is_err(), "{bad}");
        }
        assert!(asked.try_recv().is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// No socket: the door is off, said plainly, and not `here`.
    #[test]
    fn a_door_that_is_off_is_said_so() {
        let door = Door {
            socket: PathBuf::from("/nowhere/admin.sock"),
            device_id: "d-42".into(),
        };
        assert!(!door.here());
        assert_eq!(
            answer(&door, "control.clients", &json!({})),
            Err(OFF.to_owned())
        );
        assert_eq!(
            answer(&door, "control.revoke", &json!({"client": "c_1"})),
            Err(OFF.to_owned())
        );
    }

    /// The app drops the code out on a spoken check that doesn't match:
    /// lamp-control is asked to, and says whether there was one.
    #[test]
    fn a_code_cancel_drops_the_code_out() {
        let dir = folder("cancel");
        let (door, asked) = admin(&dir, |asked| {
            assert_eq!(asked, &json!({"cancel_code": true}));
            json!({"dropped": true})
        });
        let mut lines = Vec::new();
        let answered = answer_saying(&door, "control.code_cancel", &json!({}), &mut |line| {
            lines.push(line.to_owned())
        })
        .unwrap()
        .unwrap();
        assert_eq!(asked.recv().unwrap(), json!({"cancel_code": true}));
        assert_eq!(answered, json!({"dropped": true}));
        assert_eq!(
            lines,
            ["control.code_cancel from the app: the pairing code dropped (true)"]
        );
        // With lamp-control off, the app is told so.
        let off = Door {
            socket: dir.join("gone.sock"),
            device_id: "d-42".into(),
        };
        assert_eq!(
            answer_saying(&off, "control.code_cancel", &json!({}), &mut |_| {}),
            Err(OFF.to_owned())
        );
    }

    /// The owner takes the lamp back: from every computer with `{}`, from
    /// one with `{"client"}`; lamp-control says how many leases ended.
    #[test]
    fn a_take_back_ends_the_computers_leases() {
        let dir = folder("takeback");
        let (door, asked) = admin(
            &dir,
            |asked| json!({"taken": if asked.get("client").is_some() { 1 } else { 2 }}),
        );
        let mut lines = Vec::new();
        let all = answer_saying(&door, "control.take_back", &json!({}), &mut |line| {
            lines.push(line.to_owned())
        })
        .unwrap()
        .unwrap();
        assert_eq!(asked.recv().unwrap(), json!({"take_back": true}));
        assert_eq!(all, json!({"taken": 2}));
        let one = answer_saying(
            &door,
            "control.take_back",
            &json!({"client": "c_1"}),
            &mut |line| lines.push(line.to_owned()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            asked.recv().unwrap(),
            json!({"take_back": true, "client": "c_1"})
        );
        assert_eq!(one, json!({"taken": 1}));
        assert_eq!(
            lines,
            [
                "control.take_back from the app: 2 lease(s) taken back from every computer",
                "control.take_back from the app: 1 lease(s) taken back from c_1",
            ]
        );
        // A client that isn't an id is refused before lamp-control is asked.
        assert_eq!(
            answer_saying(
                &door,
                "control.take_back",
                &json!({"client": 7}),
                &mut |_| {}
            ),
            Err(BAD_CLIENT.to_owned())
        );
    }
}
