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
/// The longest the server waits on its socket for a caller before it looks
/// at the stop flag, its health report and what it can do. A caller is taken
/// as soon as it connects, not at the end of this wait (#1806).
pub const IDLE: Duration = Duration::from_millis(50);

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
    mut caps: Option<&mut crate::caps_service::Service>,
    mut speaker: Option<&mut crate::speaker_state::Service>,
    now: &dyn Fn() -> Instant,
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
        let intent = call.and_then(|(name, args)| crate::speaker_state::intent(name, args));
        let observed_before = executive
            .sound
            .as_ref()
            .map(|volume| volume.observation().sequence);
        if let (Some(speaker), Some(intent)) = (speaker.as_mut(), intent) {
            speaker.before_call(intent);
        }
        let cap_op = request
            .as_ref()
            .and_then(|request| request.get("op"))
            .and_then(Value::as_str)
            .filter(|op| crate::caps_service::is_operation(op));
        let (name, response, outcome) = if let Some(op) = cap_op {
            let response = match caps.as_mut() {
                Some(caps) => caps.answer_guarded(&line, &mut executive.sound, &mut || {
                    if let Some(speaker) = speaker.as_mut() {
                        speaker.before_restriction();
                    }
                }),
                None => crate::caps_service::unavailable(),
            };
            (op.to_owned(), response, None)
        } else {
            match (call, schedule) {
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
                    let line = outcome.line_for(name, executive.short);
                    (name.to_owned(), line, Some(outcome))
                }
                (None, _) => {
                    let outcome = Outcome::Failed("I can't do that.".into());
                    ("(not a call)".to_owned(), outcome.line(), Some(outcome))
                }
            }
        };
        let observation = executive
            .sound
            .as_ref()
            .map(crate::sound::Volume::observation);
        // Rejected arguments do not touch the mixer and cannot invalidate a
        // confirmed restriction. Every actual read/write outcome advances the
        // retained sequence; an exhausted identity must still fail quiet.
        let observed = observation.map(|observation| observation.sequence) != observed_before
            || observation.is_some_and(|observation| observation.sequence.is_none());
        if observed && let Some(caps) = caps.as_mut() {
            caps.observe_volume_result(&name, &response);
        }
        if let (Some(speaker), Some(intent)) = (speaker.as_mut(), intent) {
            if let Some(caps) = caps.as_mut() {
                caps.observe_volume(observation);
            }
            speaker.after_call(
                intent,
                response["ok"] == true,
                observation,
                caps.as_ref().is_none_or(|caps| caps.volume_permitted()),
                now(),
            );
        }
        // Private cap operations may have read or changed the same mixer.
        // Their accepted:true receipt is durable acceptance, not audibility.
        // Optional speaker publication can fail without stopping ordinary
        // commands or the caps reconciler's observation of their outcomes.
        refresh_speaker(
            executive,
            caps.as_deref_mut(),
            speaker.as_deref_mut(),
            now,
            false,
        );
        let mut reply = response.to_string();
        reply.push('\n');
        stream.write_all(reply.as_bytes())?;
        Ok((name, response, outcome))
    })();
    let took = started.elapsed().as_millis();
    match served {
        Ok((name, _, Some(Outcome::Done(_) | Outcome::Already(_)))) => {
            eprintln!("lamp-actions: {name}: done in {took} ms")
        }
        Ok((name, _, Some(Outcome::Failed(said)))) => {
            // Its own fixed sentences, never a caller's words.
            eprintln!("lamp-actions: {name}: not done in {took} ms: {said}");
        }
        // The cap reconciler reads once a second. Successful observation
        // is not an action or a useful journal transition.
        Ok((name, reply, None)) if name == "caps_get" && reply["ok"] == true => {}
        Ok((name, reply, None)) => {
            // Neither reminder text nor arbitrary service messages are logged.
            let status = if reply["ok"] == true {
                if matches!(name.as_str(), "caps_put" | "caps_remove") {
                    // Persistence acceptance is distinct from mixer readback
                    // and ring publication, which the reply reports separately.
                    "accepted"
                } else {
                    "done"
                }
            } else {
                reply["code"].as_str().unwrap_or("unavailable")
            };
            eprintln!("lamp-actions: {name}: {status} in {took} ms");
        }
        Err(error) => eprintln!("lamp-actions: a caller was dropped after {took} ms: {error}"),
    }
}

fn refresh_speaker<S: Mixer, L: Light>(
    executive: &mut Executive<S, L>,
    mut caps: Option<&mut crate::caps_service::Service>,
    speaker: Option<&mut crate::speaker_state::Service>,
    now: &dyn Fn() -> Instant,
    poll: bool,
) {
    if let Some(speaker) = speaker {
        let _ = speaker.tick(executive.sound.as_mut(), now, poll, |observation, fresh| {
            if fresh && let Some(caps) = caps.as_mut() {
                caps.observe_volume(observation);
            }
            caps.as_ref().is_none_or(|caps| caps.volume_permitted())
        });
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

/// Output caps are independent of Schedule and never enter the model catalog.
/// Open their state only after owning the socket and clearing stale readiness.
pub fn serve_reporting_with_caps<S: Mixer, L: Light>(
    directory: &Path,
    executive: &mut Executive<S, L>,
    stop: &AtomicBool,
    health: Option<lamp_bus::Publisher>,
    schedule: Option<&crate::schedule::Adapter>,
    config: Option<&crate::caps_service::Config>,
) -> io::Result<()> {
    serve_reporting_with_outputs(directory, executive, stop, health, schedule, config, None)
}

/// Both optional outputs share the existing socket loop and mixer owner.
pub fn serve_reporting_with_outputs<S: Mixer, L: Light>(
    directory: &Path,
    executive: &mut Executive<S, L>,
    stop: &AtomicBool,
    health: Option<lamp_bus::Publisher>,
    schedule: Option<&crate::schedule::Adapter>,
    config: Option<&crate::caps_service::Config>,
    speaker: Option<crate::speaker_state::Service>,
) -> io::Result<()> {
    serve_reporting_with_volume(
        directory, executive, stop, health, schedule, config, speaker, None,
    )
}

/// [`serve_reporting_with_outputs`] that also says the speaker's volume on
/// the bus after each change (`speaker/volume`, [`crate::volume_out`]).
#[allow(clippy::too_many_arguments)]
pub fn serve_reporting_with_volume<S: Mixer, L: Light>(
    directory: &Path,
    executive: &mut Executive<S, L>,
    stop: &AtomicBool,
    health: Option<lamp_bus::Publisher>,
    schedule: Option<&crate::schedule::Adapter>,
    config: Option<&crate::caps_service::Config>,
    speaker: Option<crate::speaker_state::Service>,
    volume_out: Option<crate::volume_out::VolumeOut>,
) -> io::Result<()> {
    serve_configured(
        directory,
        executive,
        stop,
        health,
        schedule,
        &Instant::now,
        || config.map(crate::caps_service::Service::open).transpose(),
        speaker,
        volume_out,
    )
}

/// Requested audio alone preserves the baseline cap and storage behavior.
pub fn serve_reporting_with_speaker<S: Mixer, L: Light>(
    directory: &Path,
    executive: &mut Executive<S, L>,
    stop: &AtomicBool,
    health: Option<lamp_bus::Publisher>,
    schedule: Option<&crate::schedule::Adapter>,
    speaker: Option<crate::speaker_state::Service>,
) -> io::Result<()> {
    serve_reporting_with_outputs(directory, executive, stop, health, schedule, None, speaker)
}

/// [`serve_reporting_with_schedule`] with the clock its two periods are
/// read on ([`LOOK_AGAIN`] and [`REPORT_EVERY`]): for a test that moves
/// that clock on instead of waiting out the lamp's seconds. A caller's
/// waits, the idle pause and the light's freshness stay on the host's.
pub fn serve_with_clock<S: Mixer, L: Light>(
    directory: &Path,
    executive: &mut Executive<S, L>,
    stop: &AtomicBool,
    health: Option<lamp_bus::Publisher>,
    schedule: Option<&crate::schedule::Adapter>,
    now: &dyn Fn() -> Instant,
) -> io::Result<()> {
    serve_with_clock_and_caps(directory, executive, stop, health, schedule, now, None)
}

/// The existing fixture clock with an optional cap service using its own
/// injected host/wall clock. Product startup uses `serve_reporting_with_caps`.
pub fn serve_with_clock_and_caps<S: Mixer, L: Light>(
    directory: &Path,
    executive: &mut Executive<S, L>,
    stop: &AtomicBool,
    health: Option<lamp_bus::Publisher>,
    schedule: Option<&crate::schedule::Adapter>,
    now: &dyn Fn() -> Instant,
    caps: Option<crate::caps_service::Service>,
) -> io::Result<()> {
    serve_with_clock_and_outputs(
        directory, executive, stop, health, schedule, now, caps, None,
    )
}

/// The requested-audio fixture wrapper retains its original argument order.
pub fn serve_with_clock_and_speaker<S: Mixer, L: Light>(
    directory: &Path,
    executive: &mut Executive<S, L>,
    stop: &AtomicBool,
    health: Option<lamp_bus::Publisher>,
    schedule: Option<&crate::schedule::Adapter>,
    speaker: Option<crate::speaker_state::Service>,
    now: &dyn Fn() -> Instant,
) -> io::Result<()> {
    serve_with_clock_and_outputs(
        directory, executive, stop, health, schedule, now, None, speaker,
    )
}

/// Both existing injectable clocks and optional services, on one owner loop.
#[allow(clippy::too_many_arguments)]
pub fn serve_with_clock_and_outputs<S: Mixer, L: Light>(
    directory: &Path,
    executive: &mut Executive<S, L>,
    stop: &AtomicBool,
    health: Option<lamp_bus::Publisher>,
    schedule: Option<&crate::schedule::Adapter>,
    now: &dyn Fn() -> Instant,
    caps: Option<crate::caps_service::Service>,
    speaker: Option<crate::speaker_state::Service>,
) -> io::Result<()> {
    serve_configured(
        directory,
        executive,
        stop,
        health,
        schedule,
        now,
        || Ok(caps),
        speaker,
        None,
    )
}

/// What the part's health warns of about its caps: held back for this run,
/// or begun empty beside a later build's store kept aside.
fn caps_warning(caps: &crate::caps_service::Service) -> Option<&str> {
    caps.held_back().or(caps.kept_aside())
}

struct ServingFiles<'a>(&'a Path);
impl Drop for ServingFiles<'_> {
    fn drop(&mut self) {
        for name in [READY_FILE, CATALOG_FILE, CALL_SOCKET] {
            let _ = std::fs::remove_file(self.0.join(name));
        }
    }
}

fn set_ready(directory: &Path, ready: bool, previous: &mut Option<bool>) -> io::Result<()> {
    if *previous == Some(ready) {
        return Ok(());
    }
    if ready {
        put(directory, READY_FILE, "ready\n")?;
    } else {
        match std::fs::remove_file(directory.join(READY_FILE)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    *previous = Some(ready);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn serve_configured<S: Mixer, L: Light>(
    directory: &Path,
    executive: &mut Executive<S, L>,
    stop: &AtomicBool,
    mut health: Option<lamp_bus::Publisher>,
    schedule: Option<&crate::schedule::Adapter>,
    now: &dyn Fn() -> Instant,
    start_caps: impl FnOnce() -> io::Result<Option<crate::caps_service::Service>>,
    mut speaker: Option<crate::speaker_state::Service>,
    mut volume_out: Option<crate::volume_out::VolumeOut>,
) -> io::Result<()> {
    std::fs::create_dir_all(directory)?;
    let listener = listen(directory)?;
    let _files = ServingFiles(directory);
    let mut ready = None;
    set_ready(directory, false, &mut ready)?;
    let mut caps = None;
    let result = (|| -> io::Result<()> {
        if let Some(speaker) = speaker.as_mut() {
            // Attempt retirement before storage recovery or mixer I/O. An
            // optional publication failure must not prevent ordinary serving.
            let _ = speaker.start();
        }
        caps = start_caps()?;
        if let Some(why) = caps.as_ref().and_then(caps_warning) {
            eprintln!("lamp-actions: {why}");
        }
        if let Some(caps) = caps.as_mut() {
            caps.tick_guarded(&mut executive.sound, &mut || {
                if let Some(speaker) = speaker.as_mut() {
                    speaker.before_restriction();
                }
            })?;
        }
        refresh_speaker(executive, caps.as_mut(), speaker.as_mut(), now, false);
        // The volume is said once at the start, from one read.
        if let (Some(out), Some(sound)) = (volume_out.as_mut(), executive.sound.as_mut()) {
            let _ = sound.level();
            out.after(sound.heard());
        }
        let offered_now = |executive: &mut Executive<S, L>| {
            let mut offered = executive.offered();
            if schedule.is_some() {
                offered.extend(crate::schedule::catalog());
            }
            offered
        };
        let mut offered: Vec<Function> = offered_now(executive);
        put(directory, CATALOG_FILE, &catalog_file(&offered))?;
        set_ready(
            directory,
            caps.as_ref().is_none_or(|caps| caps.ready()),
            &mut ready,
        )?;
        let names = |offered: &[Function]| -> String {
            offered.iter().map(|f| f.name).collect::<Vec<_>>().join(" ")
        };
        eprintln!("lamp-actions: serving; can do now: {}", names(&offered));
        let mut looked = now();
        let mut reported: Option<Instant> = None;
        let mut answered = 0u64;
        while !stop.load(Ordering::Acquire) {
            if let Some(caps) = caps.as_mut() {
                caps.tick_guarded(&mut executive.sound, &mut || {
                    if let Some(speaker) = speaker.as_mut() {
                        speaker.before_restriction();
                    }
                })?;
            }
            refresh_speaker(executive, caps.as_mut(), speaker.as_mut(), now, true);
            // Whatever read or changed the mixer last (a call, the caps'
            // reconciler, the speaker's poll), said if it is news: no read.
            if let Some(out) = volume_out.as_mut() {
                out.after(
                    executive
                        .sound
                        .as_ref()
                        .and_then(crate::sound::Volume::heard),
                );
            }
            set_ready(
                directory,
                caps.as_ref().is_none_or(|caps| caps.ready()),
                &mut ready,
            )?;
            if let Some(health) = health.as_mut()
                && reported.is_none_or(|at| now().saturating_duration_since(at) >= REPORT_EVERY)
            {
                reported = Some(now());
                let mut diagnostic = report(&offered, answered);
                if let Some(caps) = caps.as_ref() {
                    diagnostic = diagnostic.with("output_caps_ready", caps.ready());
                    if let Some(why) = caps_warning(caps) {
                        diagnostic.level = lamp_msgs::Level::Warn;
                        diagnostic.message = why.to_owned();
                    }
                }
                if let Some(speaker) = speaker.as_ref() {
                    let available = speaker.check().is_ok();
                    diagnostic = diagnostic.with("speaker_state_available", available);
                    if !available {
                        if diagnostic.level == lamp_msgs::Level::Ok {
                            diagnostic.message = "requested audio unavailable until restart".into();
                        }
                        diagnostic.level = lamp_msgs::Level::Warn;
                    }
                }
                let _ = health.publish_json(lamp_ipc::monotonic_us(), &diagnostic);
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    answered += 1;
                    answer(
                        stream,
                        executive,
                        schedule,
                        caps.as_mut(),
                        speaker.as_mut(),
                        now,
                    );
                    if let Some(out) = volume_out.as_mut() {
                        out.after(
                            executive
                                .sound
                                .as_ref()
                                .and_then(crate::sound::Volume::heard),
                        );
                    }
                    set_ready(
                        directory,
                        caps.as_ref().is_none_or(|caps| caps.ready()),
                        &mut ready,
                    )?;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    // Nobody yet: wait for a caller, at most IDLE. A sleep here
                    // kept each caller waiting for the rest of it, up to 50 ms.
                    if lamp_ipc::readable(&listener, IDLE).is_err() {
                        std::thread::sleep(IDLE);
                    }
                }
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
        Ok(())
    })();
    // Attempt speaker inhibition even when recovery, a cap tick or shutdown
    // observes a fatal error. Neither final check reads or writes the mixer.
    // In particular a concurrent stop must preserve a prior storage failure.
    let cap_result = caps
        .as_ref()
        .map_or(Ok(()), crate::caps_service::Service::check);
    if let Some(speaker) = speaker.as_mut() {
        let _ = speaker.finish();
    }
    result.and(cap_result)
}
