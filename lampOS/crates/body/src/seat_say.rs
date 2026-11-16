//! What a lamp the strain guard seated says, once (#7281, `docs/body.md`,
//! "The strain guard"): as it is seated, one fixed line through the voice's
//! say service (`/run/lampos-say/say.sock`, `"exact": true`, so the voice
//! says it in the owner's language from `language::FIXED_LINES`). The line
//! is true: [`RESTS`] only when it will stand again by itself
//! (`Glimpse::seat_rests`), [`WAITS`] when it waits for a touch. Never in
//! the lamp's quiet hours, where the seated pose says it; the say service
//! itself refuses it while privacy-muted or paused. Nothing more is said
//! until it stands.

/// Said as a hot seat begins that will end by itself once cool.
pub const RESTS: &str = "I'm a bit warm, so I'll rest a few minutes.";
/// Said as a seat begins that waits for a person.
pub const WAITS: &str = "I need a rest. Hold my head a second when you'd like me up.";
/// The voice's say service, as the first stand asks it (`first_stand`).
pub const SOCKET: &str = "/run/lampos-say/say.sock";
const SKILL: &str = "body_seat";
const PRIORITY: u8 = 5;
const EXPIRES_MS: u64 = 120_000;

/// Whether the lamp was seated on the last glimpse.
#[derive(Debug, Default)]
pub struct SeatSay {
    was: bool,
}

impl SeatSay {
    /// One glimpse: `seated`, whether its seat `rests` (ends by itself), and
    /// whether it is the lamp's quiet hours. The line to say, only on the
    /// glimpse it is seated, and not in quiet hours.
    pub fn step(&mut self, seated: bool, rests: bool, quiet: bool) -> Option<&'static str> {
        let began = seated && !self.was;
        self.was = seated;
        let _ = (began, quiet, rests);
        None // STUB-7281
    }
}

/// The say ask for `line`, one JSON line.
fn ask_line(line: &str) -> String {
    serde_json::json!({"skill": SKILL, "text": line, "exact": true,
        "priority": PRIORITY, "expires_in_ms": EXPIRES_MS})
    .to_string()
}

/// Asks the say service at `socket` for `line` on a thread of its own, so
/// the body's step never waits on it, and writes its outcome to the
/// journal.
#[cfg(unix)]
pub fn ask(socket: &std::path::Path, line: &'static str) {
    use std::io::{BufRead, BufReader, Write};
    let socket = socket.to_owned();
    std::thread::spawn(move || {
        let said = (|| {
            let mut stream = std::os::unix::net::UnixStream::connect(&socket)?;
            stream.write_all(format!("{}\n", ask_line(line)).as_bytes())?;
            for answer in BufReader::new(stream).lines() {
                let value: serde_json::Value =
                    serde_json::from_str(&answer?).unwrap_or(serde_json::Value::Null);
                if let Some(outcome) = value["outcome"].as_str() {
                    return Ok(match value["why"].as_str() {
                        Some(why) => format!("{outcome} ({why})"),
                        None => outcome.to_owned(),
                    });
                }
            }
            Ok::<_, std::io::Error>("no outcome".to_owned())
        })();
        match said {
            Ok(outcome) => eprintln!("lamp-body: seated, said \"{line}\": {outcome}"),
            Err(error) => eprintln!("lamp-body: seated, \"{line}\" not said: {error}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #7281: the line is said once, as the lamp is seated, and not again
    /// while it stays seated; the next seat says it again, the line for that
    /// seat; quiet hours say nothing.
    #[test]
    fn the_say_ask_is_written_once_a_seat() {
        let mut say = SeatSay::default();
        let lines: Vec<_> = [
            (false, false),
            (true, true),
            (true, true),
            (true, true),
            (false, false),
        ]
        .into_iter()
        .filter_map(|(seated, rests)| say.step(seated, rests, false))
        .collect();
        assert_eq!(lines, [RESTS]);
        assert_eq!(say.step(true, false, false), Some(WAITS));
        assert_eq!(say.step(true, false, false), None);
        say.step(false, false, false);
        assert_eq!(say.step(true, true, true), None);
        assert_eq!(say.step(true, true, false), None);
    }

    #[test]
    fn the_ask_is_exact_words() {
        let value: serde_json::Value = serde_json::from_str(&ask_line(WAITS)).unwrap();
        assert_eq!(value["text"], WAITS);
        assert_eq!(value["exact"], true);
        assert_eq!(value["skill"], SKILL);
    }

    /// The ask reaches the socket as one line, and the body does not wait.
    #[cfg(unix)]
    #[test]
    fn the_ask_is_written_to_the_say_socket() {
        use std::io::{BufRead, BufReader, Write};
        let dir = std::env::temp_dir().join(format!("seat-say-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("say.sock");
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        ask(&path, RESTS);
        let (mut stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line.trim_end(), ask_line(RESTS));
        stream.write_all(b"{\"outcome\":\"spoken\"}\n").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
