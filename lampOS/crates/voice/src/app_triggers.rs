//! An installed app's spoken triggers, on the voice's side (#7415 piece 6,
//! `LAMP_APP_TRIGGERS`, off). The apps running now list their manifests'
//! triggers on `apps/triggers` (lamp-control); a final line that says one is
//! told to lamp-control on `apps/trigger`, and if the app takes it
//! (`apps/took`) within [`HOLD`] the voice ends that request in code and
//! says nothing: the app speaks for itself. A line that says no trigger, or
//! one nothing takes, goes to the model exactly as before, so a lamp with
//! the switch off, or no app running, is unchanged.
//!
//! A trigger is matched on the whole line: its own words, with a polite word
//! before ("please", "lamp", "can you") or after ("please"), and nothing
//! else. "Don't start my english lesson" is not "start my english lesson".
use lamp_msgs::{AppTook, AppTrigger, AppTriggers, AppsTriggers};
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

/// `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_APP_TRIGGERS";
/// How long a line that says a trigger waits for the app to take it before
/// it goes to the model as any other line (a matched line only).
pub const HOLD: Duration = Duration::from_secs(3);

/// Whether [`ENVIRONMENT`] is `on`.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// Words before a trigger that only ask politely.
const BEFORE: [&str; 11] = [
    "please", "lamp", "hey", "ok", "okay", "can", "could", "would", "you", "will", "now",
];
/// Words after it that only ask politely.
const AFTER: [&str; 3] = ["please", "now", "thanks"];

/// The words the lamp itself reads as an answer or a command: a trigger made
/// only of these ("yes go ahead", "send it now", "forget everything") is no
/// trigger an app may hold, whatever its manifest says, or it would take the
/// person's reply to the lamp (#7736).
const LAMPS_OWN: &[&str] = &[
    "yes",
    "yeah",
    "yep",
    "no",
    "nope",
    "ok",
    "okay",
    "sure",
    "stop",
    "cancel",
    "forget",
    "send",
    "go",
    "ahead",
    "it",
    "now",
    "everything",
    "that",
    "this",
    "never",
    "mind",
    "please",
    "do",
    "the",
    "a",
    "confirm",
    "delete",
    "remove",
    "rename",
    "change",
    "keep",
    "not",
    "all",
    "thanks",
    "thank",
    "you",
    "start",
    "again",
    "continue",
    "proceed",
];

/// A line's words, lower case, without apostrophes ("it's" and "its" are one).
fn words(line: &str) -> Vec<String> {
    line.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|word| word.replace('\'', "").to_lowercase())
        .filter(|word| !word.is_empty())
        .collect()
}

/// The app and trigger a whole line says, if one does: the first app, then
/// the first trigger, in the order `apps/triggers` lists them. The trigger's
/// words may be heard run together or apart ("goodnight", "good night"),
/// as the door's `phrases_in` takes them, with only polite words around.
pub fn matched(apps: &[AppsTriggers], line: &str) -> Option<(String, String)> {
    let said = words(line);
    for app in apps {
        for trigger in &app.triggers {
            let own = words(trigger);
            let wanted: String = own.concat();
            // One of the lamp's own words, or only those, is no trigger.
            if wanted.is_empty() || own.iter().all(|word| LAMPS_OWN.contains(&word.as_str())) {
                continue;
            }
            // A run of the heard words that spells the trigger.
            for start in 0..said.len() {
                let mut spelled = String::new();
                for end in start..said.len() {
                    spelled.push_str(&said[end]);
                    if spelled.len() > wanted.len() || !wanted.starts_with(&spelled) {
                        break;
                    }
                    if spelled == wanted {
                        let (before, after) = (&said[..start], &said[end + 1..]);
                        if before.iter().all(|word| BEFORE.contains(&word.as_str()))
                            && after.iter().all(|word| AFTER.contains(&word.as_str()))
                        {
                            return Some((app.app.clone(), trigger.clone()));
                        }
                        break;
                    }
                }
            }
        }
    }
    None
}

/// Tell lamp-control `line` says `app`'s `trigger` and wait up to `hold` for
/// that app to take that request: whether it did. The app is told the
/// manifest's own trigger words and never the line around them (apps.md:
/// "its phrases, never the words around them"). `took` is this line's own
/// answers; one for another request or app is not an answer.
pub fn told_and_taken(
    publish: impl FnOnce(AppTrigger),
    took: &Receiver<AppTook>,
    request: u64,
    (app, trigger): (String, String),
    at_ms: u64,
    hold: Duration,
) -> bool {
    // A took left from before this line is not its answer.
    while took.try_recv().is_ok() {}
    publish(AppTrigger {
        turn: request,
        app: app.clone(),
        phrase: trigger.clone(),
        trigger,
        at_ms,
    });
    let end = std::time::Instant::now() + hold;
    loop {
        let left = end.saturating_duration_since(std::time::Instant::now());
        match took.recv_timeout(left) {
            Ok(answer) if answer.turn == request && answer.app == app => return true,
            Ok(_) => continue,
            Err(_) => return false,
        }
    }
}

/// What the second route is given: a line's request and words, and whether
/// an app took it.
pub type LineTaker = Arc<dyn Fn(u64, &str) -> bool + Send + Sync>;

/// The lines waiting for an app to take them, by request: the `app-took`
/// thread hands each took to its own line, so no lock is held across a wait
/// and one line's wait never delays, or drops the reply of, another's.
type Waiting = Arc<Mutex<HashMap<u64, mpsc::Sender<AppTook>>>>;

/// The line-taker the second route is given with the switch on and a bus:
/// reads the running apps' triggers (`apps/triggers`, the newest kept and
/// not expired) as each line comes, and for a line that says one tells
/// lamp-control on `apps/trigger` and waits for `apps/took` (see
/// [`told_and_taken`]). A bus that cannot be reached gives none: every line
/// goes to the model as ever.
pub fn line_taker(root: &Path) -> Option<LineTaker> {
    let bus = lamp_bus::Bus::new(root).ok()?;
    let publisher = bus
        .publisher_declared(AppTrigger::TOPIC, &AppTrigger::declaration(), "voice")
        .ok()?;
    let mut tooks = bus.subscribe(AppTook::TOPIC, "voice").ok()?;
    let waiting: Waiting = Arc::default();
    let hand_on = waiting.clone();
    // The bus's subscriber waits on this thread; each line's wait is a channel.
    std::thread::Builder::new()
        .name("app-took".into())
        .spawn(move || {
            while let Ok(message) = tooks.recv(Duration::from_secs(1)) {
                let Some(message) = message else { continue };
                if let Ok(took) = message.parse::<AppTook>()
                    && took.check().is_ok()
                    && let Ok(waiting) = hand_on.lock()
                    && let Some(line) = waiting.get(&took.turn)
                {
                    let _ = line.send(took);
                }
            }
        })
        .ok()?;
    let publisher = Mutex::new(publisher);
    Some(Arc::new(move |request, line| {
        let apps = bus
            .latest(AppTriggers::TOPIC)
            .ok()
            .flatten()
            .filter(|message| !message.expired)
            .and_then(|message| message.parse::<AppTriggers>().ok())
            .filter(|triggers| triggers.check().is_ok())
            .map(|triggers| triggers.apps)
            .unwrap_or_default();
        let Some(found) = matched(&apps, line) else {
            return false;
        };
        let (send, took) = mpsc::channel::<AppTook>();
        match waiting.lock() {
            Ok(mut waiting) => waiting.insert(request, send),
            Err(_) => return false,
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        let taken = told_and_taken(
            |trigger| {
                // Held for the one publish, never across the wait.
                if let Ok(mut publisher) = publisher.lock() {
                    let _ = publisher.publish_json(lamp_ipc::monotonic_us(), &trigger);
                }
            },
            &took,
            request,
            found,
            now_ms,
            HOLD,
        );
        if let Ok(mut waiting) = waiting.lock() {
            waiting.remove(&request);
        }
        taken
    }))
}

#[cfg(test)]
mod tests;
