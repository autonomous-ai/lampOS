//! The executive's place on the lamp: a directory with the list of what it
//! can do now (`tools.json`), a socket to ask it on (`call.sock`) and a
//! file that says it is serving (`ready`). The interface is the voice's
//! (docs/tools.md): one line in, one line out, on a connection of its own.

use crate::{Executive, Function, Outcome, catalog_file, light::Light, sound::Mixer};
use serde_json::Value;
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub const CATALOG_FILE: &str = "tools.json";
pub const CALL_SOCKET: &str = "call.sock";
pub const READY_FILE: &str = "ready";

/// How long a caller has to send its line, and to take its answer.
const CALLER_WAIT: Duration = Duration::from_secs(1);
/// The most a request line may be.
const MAX_REQUEST_BYTES: u64 = 8_192;
/// How often what can be done is looked at again: a ring owner that starts
/// or stops changes the list.
pub const LOOK_AGAIN: Duration = Duration::from_secs(2);
const IDLE: Duration = Duration::from_millis(50);

/// Write a file whole: beside its place, then moved into it.
fn put(directory: &Path, name: &str, text: &str) -> io::Result<()> {
    let beside = directory.join(format!(".{name}.new"));
    std::fs::write(&beside, text)?;
    std::fs::set_permissions(&beside, std::fs::Permissions::from_mode(0o644))?;
    std::fs::rename(beside, directory.join(name))
}

/// Bind the socket. One that is answered belongs to another executive and
/// is left alone; one that nobody answers is a dead process's and is
/// replaced.
fn listen(directory: &Path) -> io::Result<UnixListener> {
    let path = directory.join(CALL_SOCKET);
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "another executive answers on this socket",
            ));
        }
        std::fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// One caller: its line, the call, the answer. Names and outcomes are
/// logged; nothing else of what was asked.
fn answer<S: Mixer, L: Light>(
    mut stream: UnixStream,
    executive: &mut Executive<S, L>,
    schedule: Option<&crate::schedule::Adapter>,
) {
    let started = Instant::now();
    let served = (|| -> io::Result<(String, Value, Option<Outcome>)> {
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(CALLER_WAIT))?;
        stream.set_write_timeout(Some(CALLER_WAIT))?;
        let mut line = Vec::new();
        BufReader::new(stream.try_clone()?.take(MAX_REQUEST_BYTES)).read_until(b'\n', &mut line)?;
        let request: Option<Value> = line
            .ends_with(b"\n")
            .then(|| serde_json::from_slice(&line).ok())
            .flatten();
        let call = request.as_ref().and_then(|request| {
            (request.get("op")?.as_str()? == "call").then_some(())?;
            Some((
                request.get("name")?.as_str()?,
                request.get("args").unwrap_or(&Value::Null),
            ))
        });
        let (name, response, outcome) = match (call, schedule) {
            (Some((name, args)), Some(adapter)) if crate::schedule::is_tool(name) => {
                let exact = request
                    .as_ref()
                    .and_then(Value::as_object)
                    .is_some_and(|r| {
                        r.len() == 3
                            && r.keys()
                                .all(|k| matches!(k.as_str(), "op" | "name" | "args"))
                    });
                let response = if exact {
                    adapter.call(name, args)
                } else {
                    serde_json::json!({"ok":false,"code":"bad_arguments","error":"I need the exact reminder arguments before I can do that."})
                };
                (name.to_owned(), response, None)
            }
            (Some((name, args)), _) if crate::is_app_call(name) => {
                let response = executive
                    .app_call(name, args)
                    .expect("an app call is answered");
                (name.to_owned(), response, None)
            }
            (Some((name, args)), _) => {
                let outcome = executive.call(name, args);
                (name.to_owned(), outcome.line(), Some(outcome))
            }
            (None, _) => {
                let outcome = Outcome::Failed("I can't do that.".into());
                ("(not a call)".to_owned(), outcome.line(), Some(outcome))
            }
        };
        let mut reply = response.to_string();
        reply.push('\n');
        stream.write_all(reply.as_bytes())?;
        Ok((name, response, outcome))
    })();
    let took = started.elapsed().as_millis();
    match served {
        Ok((name, _, Some(Outcome::Done(_)))) => {
            eprintln!("lamp-actions: {name}: done in {took} ms")
        }
        Ok((name, _, Some(Outcome::Failed(said)))) => {
            // Its own fixed sentences, never a caller's words.
            eprintln!("lamp-actions: {name}: not done in {took} ms: {said}");
        }
        Ok((name, reply, None)) => {
            // Neither reminder text nor arbitrary service messages are logged.
            let status = if reply["ok"] == true {
                "done"
            } else {
                reply["code"].as_str().unwrap_or("unavailable")
            };
            eprintln!("lamp-actions: {name}: {status} in {took} ms");
        }
        Err(error) => eprintln!("lamp-actions: a caller was dropped after {took} ms: {error}"),
    }
}

/// Serve until `stop` is set. Calls are taken one at a time: each is a few
/// tens of milliseconds, and two changes to one control must not cross.
pub fn serve<S: Mixer, L: Light>(
    directory: &Path,
    executive: &mut Executive<S, L>,
    stop: &AtomicBool,
) -> io::Result<()> {
    serve_reporting(directory, executive, stop, None)
}

/// How often the part says it is well, for the lamp's health
/// (`diagnostics`, docs/health.md).
pub const REPORT_EVERY: Duration = Duration::from_secs(1);

/// What the part says of itself: well, with what it can do now and how
/// many calls it has answered.
pub fn report(offered: &[Function], answered: u64) -> lamp_msgs::Diagnostic {
    let names = offered.iter().map(|f| f.name).collect::<Vec<_>>().join(" ");
    lamp_msgs::Diagnostic::ok("actions")
        .with(
            "can_do",
            if names.is_empty() {
                "nothing now".to_owned()
            } else {
                names
            },
        )
        .with("answered", answered)
}

/// The same, saying on the bus once a second that it is well, when given
/// where to say it.
pub fn serve_reporting<S: Mixer, L: Light>(
    directory: &Path,
    executive: &mut Executive<S, L>,
    stop: &AtomicBool,
    health: Option<lamp_bus::Publisher>,
) -> io::Result<()> {
    serve_reporting_with_schedule(directory, executive, stop, health, None)
}

/// Opt-in Schedule entry point. Existing wrappers pass no adapter and keep
/// their original catalog and dispatch. Schedule owns persistence; this server
/// never acknowledges reminders, opens an output device or retries a mutation.
pub fn serve_reporting_with_schedule<S: Mixer, L: Light>(
    directory: &Path,
    executive: &mut Executive<S, L>,
    stop: &AtomicBool,
    health: Option<lamp_bus::Publisher>,
    schedule: Option<&crate::schedule::Adapter>,
) -> io::Result<()> {
    serve_with_clock(directory, executive, stop, health, schedule, &Instant::now)
}

/// [`serve_reporting_with_schedule`] with the clock its two periods are
/// read on ([`LOOK_AGAIN`] and [`REPORT_EVERY`]): for a test that moves
/// that clock on instead of waiting out the lamp's seconds. A caller's
/// waits, the idle pause and the light's freshness stay on the host's.
pub fn serve_with_clock<S: Mixer, L: Light>(
    directory: &Path,
    executive: &mut Executive<S, L>,
    stop: &AtomicBool,
    mut health: Option<lamp_bus::Publisher>,
    schedule: Option<&crate::schedule::Adapter>,
    now: &dyn Fn() -> Instant,
) -> io::Result<()> {
    std::fs::create_dir_all(directory)?;
    let listener = listen(directory)?;
    let offered_now = |executive: &mut Executive<S, L>| {
        let mut offered = executive.offered();
        if schedule.is_some() {
            offered.extend(crate::schedule::catalog());
        }
        offered
    };
    let mut offered: Vec<Function> = offered_now(executive);
    put(directory, CATALOG_FILE, &catalog_file(&offered))?;
    put(directory, READY_FILE, "ready\n")?;
    let names = |offered: &[Function]| -> String {
        offered.iter().map(|f| f.name).collect::<Vec<_>>().join(" ")
    };
    eprintln!("lamp-actions: serving; can do now: {}", names(&offered));
    let mut looked = now();
    let mut reported: Option<Instant> = None;
    let mut answered = 0u64;
    while !stop.load(Ordering::Acquire) {
        if let Some(health) = health.as_mut()
            && reported.is_none_or(|at| now().saturating_duration_since(at) >= REPORT_EVERY)
        {
            reported = Some(now());
            let _ = health.publish_json(lamp_ipc::monotonic_us(), &report(&offered, answered));
        }
        match listener.accept() {
            Ok((stream, _)) => {
                answered += 1;
                answer(stream, executive, schedule)
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(IDLE),
            Err(error) => {
                eprintln!("lamp-actions: accept failed: {error}");
                std::thread::sleep(IDLE);
            }
        }
        if now().saturating_duration_since(looked) >= LOOK_AGAIN {
            looked = now();
            let current = offered_now(executive);
            if current != offered {
                put(directory, CATALOG_FILE, &catalog_file(&current))?;
                eprintln!("lamp-actions: can do now: {}", names(&current));
                offered = current;
            }
        }
    }
    let _ = std::fs::remove_file(directory.join(READY_FILE));
    let _ = std::fs::remove_file(directory.join(CATALOG_FILE));
    let _ = std::fs::remove_file(directory.join(CALL_SOCKET));
    Ok(())
}
