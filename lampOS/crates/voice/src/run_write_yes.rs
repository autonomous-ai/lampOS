//! A skill's run's held write, read back and sent on the person's spoken
//! yes (#6432, `LAMP_SKILL_TASKS`, off unless `on`).
//!
//! A skill run on the lamp's Hermes prepares a write through lamp-control
//! and `lamp-services` (`service.write`): read back, held two minutes, and
//! no `send_id` for the run. The voice is told it this way:
//!
//! - **Asked, not pushed.** The coordinator asks `lamp-services` for the
//!   run's held write (`run_write_waiting`, the voice's own call on its
//!   tools socket, never offered to its model,
//!   [`crate::services::run_write_waiting`]) at most every [`ASK_EVERY`],
//!   off its loop, while it could speak ([`Waits::due`]).
//! - **Said as the lamp's own turn**, as task news is: someone at the lamp,
//!   the first pause after any turn (a turn under way, the person's or the
//!   lamp's, is waited out), nothing owed, the room not busy, never in the
//!   lamp's quiet hours ([`quiet_hours_now`]), and with at least
//!   [`LEAST_LEFT`] of its two minutes left. Once per write. Its note
//!   ([`RunWrite::note`]) has the model say `lamp-services`' own read-back
//!   word for word, after the skill's name: "Your <skill> skill wants to
//!   send: …. Shall I send it?" A read-back too long for one note
//!   (`lamp_gemini::wire::MAX_NOTE_BYTES`) is never cut: the lamp says only
//!   that the skill wanted to send something too long to read out, and
//!   nothing is sent ([`too_long`]).
//! - **Waits for the yes as any write** the voice prepared
//!   ([`crate::services::wait_for_run_yes`]): the worker reads the note it
//!   is given back ([`parse`], [`written`]) and keeps the write waiting
//!   with that request as its read-back's, so the gate the voice's own
//!   sends pass holds it (`services::gate`): heard live, a later request,
//!   within the two minutes, the read-back not cut, the echo tail's voice
//!   check, the second route only. No identity check: with
//!   `LAMP_PRIVATE_ROOM` whoever speaks is the owner. On top, its send
//!   needs a plain yes (`services::plain_yes`: "yes", "send it", "ừ, gửi
//!   đi"…), so nothing the run wrote can steer the model into a send, and
//!   names the SHA-256 of the read-back the lamp said, which
//!   `lamp-services` holds to the write it keeps (`serve::RUN_NOT_SAID`).
//! - **Sent by `lamp-services`**, counted against the skills' bounds (3 a
//!   day a skill, 10 across them); the model is told "sent" only from its
//!   answer (`services::answered`). A no, a wait, another request answered
//!   or the two minutes over: nothing is sent.
use serde_json::Value;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// The switch, as `lamp-services` and lamp-control read it.
pub const ENVIRONMENT: &str = lamp_services::SKILL_TASKS_ENVIRONMENT;

/// Whether `value` turns it on: `on` only.
pub fn on_for(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Whether [`ENVIRONMENT`] is on in this process.
pub fn on() -> bool {
    on_for(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// How often `lamp-services` is asked, at most.
pub const ASK_EVERY: Duration = Duration::from_secs(3);
/// A write with less of its hold left is not read back: too little time
/// to say it and hear the yes.
pub const LEAST_LEFT: Duration = Duration::from_secs(30);
/// The admission basis of the turn that reads it back.
pub const BASIS: &str = "run_write_read_back";
/// How many writes told are remembered, so each is told once.
const TOLD_KEPT: usize = 8;

/// How a read-back note opens, then the header's four fields.
const OPENS: &str = "(Nobody has spoken. [run-write ";
/// After the header: what the model does, then the line, quoted.
const SAY: &str = "] A skill of the person's wants to send something. Nothing is sent, and you call nothing now. Say only this, word for word as written, leaving nothing out, every address and every quoted word, then stop (if the conversation is in another language, say it in that language too, after it): \"";
/// How the line ends: its question.
const ASKS: &str = " Shall I send it?";
/// How the note ends.
const CLOSES: &str = "\")";

/// A skill run's write held by `lamp-services`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunWrite {
    pub send_id: String,
    /// The catalog code it was prepared for.
    pub service: String,
    pub skill: String,
    /// `lamp-services`' own words for it.
    pub read_back: String,
    /// How much of its hold was left when told.
    pub left: Duration,
}

fn id_ok(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_hexdigit())
}

fn code_ok(code: &str) -> bool {
    lamp_services::catalog::service(code).is_some_and(|service| service.code == code)
}

fn skill_ok(skill: &str) -> bool {
    lamp_services::skill_writes::skill_name_ok(skill)
}

impl RunWrite {
    /// From `run_write_waiting`'s `waiting`: none when it is `null` or any
    /// field is not as `lamp-services` gives it.
    pub fn from_waiting(waiting: &Value) -> Option<Self> {
        let text = |key: &str| waiting.get(key).and_then(Value::as_str);
        let run = Self {
            send_id: text("send_id").filter(|id| id_ok(id))?.to_owned(),
            service: text("service").filter(|code| code_ok(code))?.to_owned(),
            skill: text("skill").filter(|skill| skill_ok(skill))?.to_owned(),
            read_back: text("read_back")
                .filter(|said| said.chars().any(char::is_alphanumeric))?
                .to_owned(),
            left: Duration::from_millis(waiting.get("left_ms")?.as_u64()?),
        };
        Some(run)
    }

    /// The line the lamp says: the skill's name, then the read-back word
    /// for word, then its question.
    pub fn line(&self) -> String {
        format!(
            "Your {} skill wants to send: {}{ASKS}",
            crate::made_skill::spoken_name(&self.skill),
            self.read_back
        )
    }

    /// The written turn that reads it back, with the header the worker
    /// reads it by ([`parse`]); [`too_long`] when it would not fit in one
    /// note.
    pub fn note(&self) -> String {
        let note = format!(
            "{OPENS}{} {} {} {}{SAY}{}{CLOSES}",
            self.send_id,
            self.service,
            self.skill,
            self.left.as_secs(),
            self.line()
        );
        if note.len() <= lamp_gemini::wire::MAX_NOTE_BYTES && !note.contains('\0') {
            note
        } else {
            too_long(&self.skill)
        }
    }
}

/// What the lamp says of a write too long to read back in one note:
/// nothing is sent, and nothing waits for a yes.
pub fn too_long(skill: &str) -> String {
    format!(
        "(Nobody has spoken. Say only, in a few words: your {} skill wanted to send something too long for me to read out, so nothing was sent.)",
        crate::made_skill::spoken_name(skill)
    )
}

/// The write a read-back note ([`RunWrite::note`]) is for, read back
/// exactly: its header's fields, and the read-back between the line's fixed
/// words. Any other text is none.
pub fn parse(text: &str) -> Option<RunWrite> {
    let (header, rest) = text.strip_prefix(OPENS)?.split_once(SAY)?;
    let fields: Vec<&str> = header.split(' ').collect();
    let [send_id, service, skill, left] = fields.as_slice() else {
        return None;
    };
    if !id_ok(send_id) || !code_ok(service) || !skill_ok(skill) {
        return None;
    }
    let left = Duration::from_secs(left.parse().ok()?);
    let opens = format!(
        "Your {} skill wants to send: ",
        crate::made_skill::spoken_name(skill)
    );
    let read_back = rest
        .strip_suffix(CLOSES)?
        .strip_prefix(opens.as_str())?
        .strip_suffix(ASKS)?;
    read_back
        .chars()
        .any(char::is_alphanumeric)
        .then(|| RunWrite {
            send_id: (*send_id).to_owned(),
            service: (*service).to_owned(),
            skill: (*skill).to_owned(),
            read_back: read_back.to_owned(),
            left,
        })
}

/// A written request this worker was given (`request`, `text`): a run's
/// read-back note keeps its write waiting for the yes, with the switch on.
pub fn written(request: u64, text: &str) {
    written_with(on(), request, text, Instant::now());
}

/// [`written`] with the switch as `on`; whether a write now waits.
pub fn written_with(on: bool, request: u64, text: &str, now: Instant) -> bool {
    if !on {
        return false;
    }
    let Some(run) = parse(text) else {
        return false;
    };
    crate::services::wait_for_run_yes(request, &run, now);
    true
}

/// Whether the lamp's quiet hours hold now (`lamp_local_time::quiet_hours`,
/// the one definition skills keep): a local time not known is quiet, as a
/// skill's is.
pub fn quiet_hours_now() -> bool {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    crate::local_time::system()
        .is_none_or(|zone| lamp_local_time::quiet_hours(zone.minute_of_day(now_ms)))
}

/// The run's write as last asked, and when.
#[derive(Default)]
struct Asked {
    got: Option<(RunWrite, Instant)>,
    at: Option<Instant>,
    asking: bool,
}

/// The coordinator's side: what `lamp-services` holds, asked off the loop,
/// and the writes already told.
pub struct Waits {
    folder: PathBuf,
    asked: Arc<Mutex<Asked>>,
    told: VecDeque<String>,
}

impl Waits {
    /// With the switch on, asking `lamp-services`' tools folder.
    pub fn from_environment() -> Option<Self> {
        on().then(|| Self::new(PathBuf::from(crate::services::FOLDER)))
    }

    pub fn new(folder: PathBuf) -> Self {
        eprintln!(
            "lamp-voice: a skill run's held write is read back for a spoken yes (LAMP_SKILL_TASKS)"
        );
        Self {
            folder,
            asked: Arc::new(Mutex::new(Asked::default())),
            told: VecDeque::new(),
        }
    }

    /// Ask on a thread of its own, when not asked within [`ASK_EVERY`] and
    /// not asking now. Never waits.
    fn ask(&self, now: Instant) {
        {
            let mut asked = self.asked.lock().unwrap_or_else(|p| p.into_inner());
            if asked.asking
                || asked
                    .at
                    .is_some_and(|at| now.saturating_duration_since(at) < ASK_EVERY)
            {
                return;
            }
            asked.asking = true;
            asked.at = Some(now);
        }
        let asked = Arc::clone(&self.asked);
        let folder = self.folder.clone();
        let spawned = std::thread::Builder::new()
            .name("lamp-voice-run-write".into())
            .spawn(move || {
                let got = crate::services::run_write_waiting(&folder);
                let mut asked = asked.lock().unwrap_or_else(|p| p.into_inner());
                asked.asking = false;
                asked.got = got.map(|run| (run, Instant::now()));
            });
        if spawned.is_err() {
            self.asked.lock().unwrap_or_else(|p| p.into_inner()).asking = false;
        }
    }

    /// The run's write to read back now, if one waits untold with at least
    /// [`LEAST_LEFT`] of its hold left; asks again when due.
    pub fn due(&mut self, now: Instant) -> Option<RunWrite> {
        self.ask(now);
        let got = self
            .asked
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .got
            .clone();
        due_of(got.as_ref(), &self.told, now)
    }

    /// `send_id`'s write was read back: never again.
    pub fn told(&mut self, send_id: &str) {
        if self.told.len() == TOLD_KEPT {
            self.told.pop_front();
        }
        self.told.push_back(send_id.to_owned());
    }
}

/// [`Waits::due`] of what was last asked (`got`, with when), the writes
/// already `told`, at `now`: its hold's time left then.
pub fn due_of(
    got: Option<&(RunWrite, Instant)>,
    told: &VecDeque<String>,
    now: Instant,
) -> Option<RunWrite> {
    let (run, at) = got?;
    let left = run.left.checked_sub(now.saturating_duration_since(*at))?;
    (left >= LEAST_LEFT && !told.contains(&run.send_id)).then(|| RunWrite {
        left,
        ..run.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services;
    use lamp_gemini::ToolCall;
    use serde_json::json;

    const READ_BACK: &str =
        "Send a mail from your Gmail to mom@home.test, subject \"Dinner\", saying: \"Home by 7\".";

    fn run() -> RunWrite {
        RunWrite::from_waiting(&json!({
            "send_id": "ab12cd34ab12cd34ab12cd34ab12cd34",
            "service": "gmail",
            "skill": "dinner-note",
            "read_back": READ_BACK,
            "left_ms": 110_000,
        }))
        .unwrap()
    }

    fn send(service: &str, send_id: &str) -> ToolCall {
        ToolCall {
            id: "1".into(),
            name: services::WRITE.into(),
            args: json!({"service": service, "send_id": send_id}),
        }
    }

    fn turn(plain_yes: bool) -> services::Turn<'static> {
        services::Turn {
            heard: true,
            owner: Some("p-owner"),
            sends: true,
            seen_owner: true,
            plain_yes,
            ..services::Turn::default()
        }
    }

    /// The read-back is said with the skill's name, word for word, and the
    /// worker reads the very write back from the note; any other note,
    /// or a field not as lamp-services gives it, is none.
    #[test]
    fn a_held_run_write_is_said_with_the_skills_name() {
        let run = run();
        let note = run.note();
        assert!(
            note.contains(&format!(
                "\"Your dinner note skill wants to send: {READ_BACK} Shall I send it?\")"
            )),
            "{note}"
        );
        assert!(note.len() <= lamp_gemini::wire::MAX_NOTE_BYTES);
        let back = parse(&note).unwrap();
        assert_eq!(back.read_back, READ_BACK);
        assert_eq!(back.left, Duration::from_secs(110));
        assert_eq!(
            RunWrite {
                left: run.left,
                ..back
            },
            run
        );
        for other in [
            "(Nobody has spoken. Claude Code just finished.)",
            &note.replace("gmail", "Gmail"),
            &note.replace("dinner-note", "Dinner Note"),
            &note.replace(" Shall I send it?", ""),
        ] {
            assert!(parse(other).is_none(), "{other}");
        }
        for (key, bad) in [
            ("send_id", json!("not hex!")),
            ("service", json!("Gmail")),
            ("skill", json!("../x")),
            ("read_back", json!("  ")),
        ] {
            let mut waiting = json!({"send_id": "ab12", "service": "gmail",
                "skill": "dinner-note", "read_back": READ_BACK, "left_ms": 1});
            waiting[key] = bad;
            assert!(RunWrite::from_waiting(&waiting).is_none(), "{key}");
        }
        assert!(RunWrite::from_waiting(&Value::Null).is_none());
        // Too long for one note: never cut, only said not sent.
        let long = RunWrite {
            read_back: "word ".repeat(300),
            ..run
        };
        assert_eq!(long.note(), too_long("dinner-note"));
        assert!(parse(&long.note()).is_none());
    }

    /// Told once, with enough of its hold left; asked again later, a new
    /// write is told.
    #[test]
    fn a_held_write_is_told_once_with_time_left() {
        let now = Instant::now();
        let mut told = VecDeque::new();
        let got = (run(), now);
        let due = due_of(Some(&got), &told, now + Duration::from_secs(10)).unwrap();
        assert_eq!(due.left, Duration::from_secs(100));
        assert!(due_of(Some(&got), &told, now + Duration::from_secs(81)).is_none());
        told.push_back(got.0.send_id.clone());
        assert!(due_of(Some(&got), &told, now).is_none());
        assert!(due_of(None, &VecDeque::new(), now).is_none());
    }

    /// The switch off: a run's read-back note keeps nothing waiting, and a
    /// send finds nothing.
    #[test]
    fn the_switch_off_ignores_it() {
        let now = Instant::now();
        assert!(!on_for(None) && !on_for(Some("1")) && on_for(Some("on")));
        assert!(!written_with(false, 4, &run().note(), now));
        let mut pending = None;
        assert!(services::waiting_note(5, Some("p-owner"), now).is_none());
        let told = services::gate(
            &mut pending,
            &send("gmail", &run().send_id),
            5,
            &turn(true),
            now,
        );
        assert_eq!(told.unwrap_err()["error"], services::NOTHING_WAITING);
    }

    /// Read back in request 4, a plain yes in request 5 sends exactly the
    /// write held, with the SHA-256 of the words read back; the model is
    /// told of it in that turn.
    #[test]
    fn a_plain_yes_sends_it() {
        let now = Instant::now();
        let run = run();
        assert!(written_with(true, 4, &run.note(), now));
        services::lamp_said(4, &run.line(), now);
        let note = services::waiting_note(5, Some("p-owner"), now);
        assert!(note.is_none() || note.as_deref().is_some_and(|n| n.contains(&run.send_id)));
        let sent = services::with_pending(|pending| {
            services::gate(
                pending,
                &send("gmail", &run.send_id),
                5,
                &turn(true),
                now + Duration::from_secs(10),
            )
        })
        .unwrap();
        assert_eq!(
            sent.args,
            json!({"service": "gmail", "send_id": run.send_id, "said": "send",
                "sha256": lamp_services::serve::read_back_sha256(READ_BACK)})
        );
        // Taken: a second send finds nothing.
        assert!(services::with_pending(|pending| pending.is_none()));
    }

    /// Anything but a plain yes, a no answered, or its hold over: nothing
    /// is sent.
    #[test]
    fn a_no_drops_it() {
        let now = Instant::now();
        let run = run();
        // The model sends on words that are no plain yes: refused, dropped.
        written_with(true, 4, &run.note(), now);
        services::lamp_said(4, &run.line(), now);
        let refused = services::with_pending(|pending| {
            services::gate(pending, &send("gmail", &run.send_id), 5, &turn(false), now)
        });
        assert_eq!(refused.unwrap_err()["error"], services::RUN_NOT_PLAIN_YES);
        assert!(services::with_pending(|pending| pending.is_none()));
        // "No": the lamp answers with no send, and the next line's send
        // finds nothing.
        written_with(true, 4, &run.note(), now);
        services::lamp_said(4, &run.line(), now);
        services::lamp_said(5, "Okay, I won't send it.", now);
        assert!(services::waiting_note(6, Some("p-owner"), now).is_none());
        assert!(services::with_pending(|pending| pending.is_none()));
        // Past its hold, as lamp-services counted it: cancelled.
        let late = RunWrite {
            left: Duration::from_secs(5),
            ..run.clone()
        };
        written_with(true, 4, &late.note(), now);
        let refused = services::with_pending(|pending| {
            services::gate(
                pending,
                &send("gmail", &run.send_id),
                5,
                &turn(true),
                now + Duration::from_secs(6),
            )
        });
        assert_eq!(refused.unwrap_err()["error"], services::CANCELLED);
    }

    /// The model is told "sent" only when lamp-services answered sent: a
    /// refusal (over the skills' bounds, the read-back not the one held)
    /// or no answer is told as not sent.
    #[test]
    fn no_sent_is_said_without_done() {
        let now = Instant::now();
        let run = run();
        let call = ToolCall {
            id: "1".into(),
            name: services::WRITE.into(),
            args: json!({"service": "gmail", "send_id": run.send_id, "said": "send",
                "sha256": "00"}),
        };
        for (told, next) in [
            (
                json!({"error": "not sent: mail-count may send at most 3 times a day"}),
                services::NOT_SENT_WHY_NEXT,
            ),
            (
                json!({"error": lamp_services::serve::RUN_NOT_SAID}),
                services::NOT_SENT_WHY_NEXT,
            ),
            (json!({"sent": false}), services::NOT_SENT_NEXT),
            (
                json!({"sent": true, "service": "Gmail"}),
                services::SENT_NEXT,
            ),
        ] {
            let mut told = told;
            services::with_pending(|pending| {
                services::answered(pending, &call, 5, &mut told, now, "p-owner");
            });
            assert_eq!(told["next"], next, "{told}");
        }
    }
}
