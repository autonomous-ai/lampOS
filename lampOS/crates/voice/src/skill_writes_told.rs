//! "I sent your 'leaving mail' mail to mai at 18:02." (#6066, the first
//! builder's conditions of 21:44: a made skill's write is told to the
//! person the next time they are there). With `LAMP_SKILL_WRITES=on` and
//! the services' folder, the second route asks `lamp-services` for its
//! sent-log (`skill_writes_sent`, [`crate::services::skill_writes_sent`])
//! off the line's path, at most every [`ASK_EVERY`], as a heard line begins;
//! the next heard line is told each send since the last one told, once,
//! in one short note. Nobody's identity is checked (the owner, 10 Nov):
//! whoever speaks at the lamp is told.
//!
//! What was told is marked in the voice's own folder ([`MARK_FILE`] in
//! `made_skill::DIRECTORY`): the newest send's time, and the sends of that
//! second told, so a send is told once across restarts. With no mark yet,
//! only the last day's sends are told. The sent-log holds a skill's name,
//! a service, a time and where it went (a recipient's local part, or a
//! place), never a text; the journal says counts only.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

/// How often the sent-log is asked, at most.
pub const ASK_EVERY: Duration = Duration::from_secs(5);
/// What is told is marked in this file of the made skills' folder.
pub const MARK_FILE: &str = "writes-told.json";
/// With no mark, sends older than this are not told.
pub const FIRST_TOLD_WITHIN_S: i64 = 86_400;

/// The told-up-to mark: the newest send told, and the sends of that very
/// second told (two in one second are each told once).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub at: i64,
    pub keys: Vec<String>,
}

/// A send's key in the mark.
fn key(sent: &Value) -> String {
    format!(
        "{}|{}|{}|{}",
        sent["skill"].as_str().unwrap_or_default(),
        sent["service"].as_str().unwrap_or_default(),
        sent["at"].as_i64().unwrap_or_default(),
        sent["to"].as_str().unwrap_or_default()
    )
}

/// The sends of `sent` not told by `mark` (none: only those after
/// `now_s` less [`FIRST_TOLD_WITHIN_S`]), oldest first, and the mark once
/// they are told.
pub fn untold(sent: &[Value], mark: Option<&Mark>, now_s: i64) -> (Vec<Value>, Option<Mark>) {
    let fresh: Vec<Value> = sent
        .iter()
        .filter(|one| {
            let at = one["at"].as_i64().unwrap_or_default();
            match mark {
                Some(mark) => at > mark.at || (at == mark.at && !mark.keys.contains(&key(one))),
                None => at > now_s - FIRST_TOLD_WITHIN_S,
            }
        })
        .cloned()
        .collect();
    let Some(newest) = fresh.iter().filter_map(|one| one["at"].as_i64()).max() else {
        return (fresh, None);
    };
    let mut keys: Vec<String> = match mark {
        Some(mark) if mark.at == newest => mark.keys.clone(),
        _ => Vec::new(),
    };
    keys.extend(
        fresh
            .iter()
            .filter(|one| one["at"].as_i64() == Some(newest))
            .map(key),
    );
    (fresh, Some(Mark { at: newest, keys }))
}

/// One send in words: "I sent your 'leaving mail' mail to mai at 18:02."
/// `hh_mm` gives the local time of a send's Unix seconds.
pub fn sentence(sent: &Value, hh_mm: impl Fn(i64) -> String) -> String {
    let skill = crate::made_skill::spoken_name(sent["skill"].as_str().unwrap_or_default());
    let (verb, what) = match sent["service"].as_str().unwrap_or_default() {
        "google_calendar" => ("added", "event"),
        "github" => ("posted", "GitHub post"),
        "linear" => ("posted", "Linear issue"),
        "asana" => ("added", "Asana task"),
        "figma" | "figma-api" => ("posted", "Figma comment"),
        _ => ("sent", "mail"),
    };
    let to = sent["to"].as_str().unwrap_or_default();
    let at = hh_mm(sent["at"].as_i64().unwrap_or_default());
    match to {
        "" => format!("I {verb} your '{skill}' {what} at {at}."),
        to => format!("I {verb} your '{skill}' {what} to {to} at {at}."),
    }
}

/// The note the model is told: each send's sentence, said once, first.
pub fn note(sentences: &[String]) -> Option<String> {
    (!sentences.is_empty()).then(|| {
        format!(
            "(For you to tell the person first, in the conversation's language, before \
             anything else, once and as it is: {} Then answer them.)",
            sentences.join(" ")
        )
    })
}

/// A send's time where the lamp stands, "18:02": the lamp's zone, else UTC.
pub fn local_hh_mm(at_s: i64) -> String {
    let ms = u64::try_from(at_s.max(0)).unwrap_or_default() * 1_000;
    let minute = crate::local_time::system()
        .map(|zone| u32::from(zone.minute_of_day(ms)))
        .unwrap_or(((at_s.rem_euclid(86_400)) / 60) as u32);
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

/// The sent-log as last asked, and when it was asked.
#[derive(Default)]
struct Asked {
    sent: Option<Vec<Value>>,
    at: Option<Instant>,
    asking: bool,
}

/// The teller: the services' folder, the mark's file, and what was asked.
pub struct Teller {
    folder: PathBuf,
    mark: PathBuf,
    asked: Mutex<Asked>,
}

static TELLER: OnceLock<Arc<Teller>> = OnceLock::new();

/// Start telling, once a process: with a made skill's writes on and the
/// services' `folder`; the mark in `made` (`made_skill::DIRECTORY`).
pub fn start(folder: PathBuf, made: &Path) {
    if !crate::made_skill::writes_on() {
        return;
    }
    let _ = TELLER.set(Arc::new(Teller {
        folder,
        mark: made.join(MARK_FILE),
        asked: Mutex::new(Asked::default()),
    }));
    eprintln!(
        "lamp-voice second: a made skill's sends are told at the next heard line (LAMP_SKILL_WRITES)"
    );
}

impl Teller {
    /// Ask `lamp-services` for the sent-log on a thread of its own, when
    /// not asked within [`ASK_EVERY`] and not asking now. Never waits.
    fn ask(self: &Arc<Self>) {
        {
            let mut asked = self.asked.lock().unwrap_or_else(|p| p.into_inner());
            if asked.asking || asked.at.is_some_and(|at| at.elapsed() < ASK_EVERY) {
                return;
            }
            asked.asking = true;
            asked.at = Some(Instant::now());
        }
        let teller = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("lamp-voice-sent-told".into())
            .spawn(move || {
                let sent = crate::services::skill_writes_sent(&teller.folder);
                let mut asked = teller.asked.lock().unwrap_or_else(|p| p.into_inner());
                asked.asking = false;
                if sent.is_some() {
                    asked.sent = sent;
                }
            });
        if spawned.is_err() {
            self.asked.lock().unwrap_or_else(|p| p.into_inner()).asking = false;
        }
    }

    /// The note for a heard line: the sends since the mark, from what was
    /// last asked, marked told.
    fn note(self: &Arc<Self>, request: u64) -> Option<String> {
        self.ask();
        let sent = self
            .asked
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .sent
            .clone()?;
        let mark = std::fs::read(&self.mark)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Mark>(&bytes).ok());
        let now_s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs() as i64);
        let (fresh, marked) = untold(&sent, mark.as_ref(), now_s);
        let marked = marked?;
        // Marked before it is told: told at most once, never twice.
        let kept = serde_json::to_vec(&marked)
            .ok()
            .is_some_and(|bytes| std::fs::write(&self.mark, bytes).is_ok());
        if !kept {
            eprintln!(
                "lamp-voice second: request {request}: a made skill's sends not told: the mark can't be kept"
            );
            return None;
        }
        eprintln!(
            "lamp-voice second: request {request}: the person is told {} send(s) of a made skill",
            fresh.len()
        );
        let sentences: Vec<String> = fresh.iter().map(|one| sentence(one, local_hh_mm)).collect();
        note(&sentences)
    }
}

/// The note for the heard line of `request`: the made skills' sends not
/// told yet, once; none when not started or nothing new.
pub fn note_for(request: u64) -> Option<String> {
    TELLER.get()?.note(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn send(skill: &str, at: i64, to: &str) -> Value {
        json!({"skill": skill, "service": "gmail", "at": at, "to": to})
    }

    /// Each send is told once: those after the mark, then none; two in one
    /// second each once; with no mark only the last day's.
    #[test]
    fn each_send_is_told_once_after_the_mark() {
        let now = 1_000_000;
        let log = vec![
            send("old-one", now - 90_000, "boss"),
            send("leaving-mail", now - 60, "mai"),
        ];
        let (fresh, mark) = untold(&log, None, now);
        assert_eq!(fresh, vec![log[1].clone()]);
        let mark = mark.unwrap();
        assert_eq!(mark.at, now - 60);
        let (fresh, again) = untold(&log, Some(&mark), now);
        assert!(fresh.is_empty() && again.is_none());
        // Another in the same second, then a later one.
        let mut log = log;
        log.push(send("arrive-mail", now - 60, "boss"));
        let (fresh, mark2) = untold(&log, Some(&mark), now);
        assert_eq!(fresh, vec![log[2].clone()]);
        let mark2 = mark2.unwrap();
        assert_eq!(mark2.keys.len(), 2);
        log.push(send("leaving-mail", now, "mai"));
        let (fresh, _) = untold(&log, Some(&mark2), now);
        assert_eq!(fresh, vec![log[3].clone()]);
    }

    /// Said as the person hears it, with the local time, and the note
    /// asks for it first, once.
    #[test]
    fn a_send_is_said_with_its_time() {
        let said = sentence(&send("leaving-mail", 0, "mai"), |_| "18:02".into());
        assert_eq!(said, "I sent your 'leaving mail' mail to mai at 18:02.");
        let event = json!({"skill": "standup", "service": "google_calendar", "at": 0, "to": "your calendar"});
        assert_eq!(
            sentence(&event, |_| "09:00".into()),
            "I added your 'standup' event to your calendar at 09:00."
        );
        assert!(note(&[]).is_none());
        assert!(
            note(&[said])
                .unwrap()
                .contains("once and as it is: I sent your")
        );
        assert_eq!(local_hh_mm(-1).len(), 5);
    }
}
