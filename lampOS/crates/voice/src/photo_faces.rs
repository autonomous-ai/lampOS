//! A face added from a photo, asked about once at the lamp
//! (`LAMP_PHOTO_FACES=on`, the first builder on #1968, 22:12): the eyes keep
//! it unconfirmed (`lamp-eyes --photo-dir`, `docs/eyes.md`, "A face from a
//! photo") until its person says yes there.
//!
//! When the eyes say an unconfirmed face is at the lamp
//! (`{"photo_here":true}`), the voice tells them it is asking
//! (`{"photo_asked":NAME}`) and asks once, as a written turn ([`note`]):
//! "Someone added your face from a photo, as NAME. Shall I remember you?"
//! Their spoken answer calls [`YES`] or [`NO`], which tell the eyes
//! (`photo_yes`, `photo_no`). No answer in that meeting is the eyes' to let
//! go. Off unless set, and only with the eyes' request socket.
use crate::known_faces::DEADLINE;
use lamp_gemini::{Tool, ToolCall};
use serde_json::{Value, json};
use std::{path::Path, sync::Mutex, time::Duration};

pub const ENVIRONMENT: &str = "LAMP_PHOTO_FACES";
/// The switch as the voice's workers are given it, after `--who-socket`.
pub const FLAG: &str = "--photo-faces";
pub const YES: &str = "photo_face_yes";
pub const NO: &str = "photo_face_no";
/// The eyes are asked whether a face from a photo is here at most this
/// often, while the lamp is quiet.
pub const LOOK_EVERY: Duration = Duration::from_secs(2);
/// A quick question: the eyes answer at once or not at all.
const HERE_DEADLINE: Duration = Duration::from_millis(300);

/// The name asked about, in the coordinator's process: so it is asked once.
/// The answer runs in the provider's worker and takes its face from the
/// eyes.
static ASKED: Mutex<Option<String>> = Mutex::new(None);

pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

pub fn is_tool(name: &str) -> bool {
    matches!(name, YES | NO)
}

/// The two functions for the person's answer.
pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: YES.into(),
            description: "Call this when the person you just asked whether to remember their face, \
                          added from a photo, says yes. Then say in a few words that you'll know them now."
                .into(),
            parameters: None,
            silent: false,
        },
        Tool {
            name: NO.into(),
            description: "Call this when the person you just asked whether to remember their face, \
                          added from a photo, says no or doesn't want it. Then say in a few words: okay, \
                          you've let it go."
                .into(),
            parameters: None,
            silent: false,
        },
    ]
}

/// What the model is told to say, once: the question in its own words,
/// never the name used as theirs until they agree.
pub fn note(name: &str) -> String {
    format!(
        "Someone added the face of the person at the lamp from a photo, under the name {name}. \
         Ask them once, gently, in your own words: \"Someone added your face from a photo, as {name}. \
         Shall I remember you?\" If they say yes, call {YES}. If they say no, call {NO}. \
         Don't call them {name} unless they say yes, and don't ask again."
    )
}

/// The unconfirmed face at the lamp now, if the eyes say one is.
pub fn here(socket: &Path) -> Option<String> {
    let answer = crate::tools::exchange(socket, &json!({"photo_here": true}), HERE_DEADLINE)?;
    answer["photo_here"].as_str().map(str::to_owned)
}

/// Tell the eyes the voice is asking about `name`; `true` if they took it,
/// and then a yes or a no is for that name.
pub fn asking(socket: &Path, name: &str) -> bool {
    let taken = crate::tools::exchange(socket, &json!({"photo_asked": name}), HERE_DEADLINE)
        .is_some_and(|answer| answer["asked"] == true);
    if taken && let Ok(mut asked) = ASKED.lock() {
        *asked = Some(name.to_owned());
    }
    taken
}

/// Whether `name` was the one asked already in this run: never asked twice.
pub fn was_asked(name: &str) -> bool {
    ASKED
        .lock()
        .map(|asked| asked.as_deref() == Some(name))
        .unwrap_or(false)
}

/// The eyes asked off the voice's loop, which never waits on them: a
/// thread asks whether a face from a photo is here every [`LOOK_EVERY`],
/// and tells the eyes the voice is asking when the loop wants to.
pub struct Watch {
    state: std::sync::Arc<Mutex<WatchState>>,
}

#[derive(Default)]
struct WatchState {
    here: Option<String>,
    want: Option<String>,
    asked: Option<String>,
    stop: bool,
}

impl Watch {
    pub fn start(socket: std::path::PathBuf) -> Self {
        let state = std::sync::Arc::new(Mutex::new(WatchState::default()));
        let shared = state.clone();
        std::thread::spawn(move || {
            let mut looked: Option<std::time::Instant> = None;
            loop {
                let want = match shared.lock() {
                    Ok(mut state) if !state.stop => state.want.take(),
                    _ => return,
                };
                if let Some(name) = want
                    && asking(&socket, &name)
                    && let Ok(mut state) = shared.lock()
                {
                    state.asked = Some(name);
                }
                if looked.is_none_or(|at| at.elapsed() >= LOOK_EVERY) {
                    looked = Some(std::time::Instant::now());
                    let found = here(&socket);
                    if let Ok(mut state) = shared.lock() {
                        state.here = found;
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        Self { state }
    }

    /// The face from a photo here, not yet asked about.
    pub fn to_ask(&self) -> Option<String> {
        let here = self.state.lock().ok()?.here.clone()?;
        (!was_asked(&here)).then_some(here)
    }

    /// Ask about `name`: the thread tells the eyes, then [`Self::asked`].
    pub fn ask(&self, name: String) {
        if let Ok(mut state) = self.state.lock() {
            state.want = Some(name);
        }
    }

    /// The name the eyes took as asked, once: the question is to be said.
    pub fn asked(&self) -> Option<String> {
        self.state.lock().ok()?.asked.take()
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.stop = true;
        }
    }
}

/// The person's answer, told to the eyes, and what to tell the model; the
/// second value says whether the eyes answered. It runs in the provider's
/// worker, a process apart from the coordinator that asked, so the face it
/// answers for is the eyes' own word: the unconfirmed face here now. The
/// eyes take a yes only for that face, and only once it was asked
/// (`not_here`, `not_asked`; the reviewer on #1977, 22:50).
pub fn run(socket: &Path, call: &ToolCall) -> (Value, bool) {
    let Some(name) = here(socket) else {
        return (
            json!({"error": "Nobody added from a photo is at the lamp now, so there is nothing to answer."}),
            true,
        );
    };
    let line = match call.name.as_str() {
        YES => json!({"photo_yes": name}),
        NO => json!({"photo_no": name}),
        _ => return (crate::tools::unavailable(), false),
    };
    let Some(answer) = crate::tools::exchange(socket, &line, DEADLINE) else {
        return (
            json!({"error": "My eye is not answering right now, so I could not do that."}),
            false,
        );
    };
    if let Ok(mut asked) = ASKED.lock() {
        *asked = None;
    }
    let told = match (call.name.as_str(), answer["error"].as_str()) {
        (YES, None) => {
            json!({"ok": true, "result": "Remembered: you'll know them now. Tell them so in a few words."})
        }
        (NO, None) => {
            json!({"ok": true, "result": "Let go. Tell them in a few words: okay, you've let it go."})
        }
        (_, Some("not_here")) => {
            json!({"error": "They're not at the lamp now, so nothing was changed."})
        }
        (_, Some("not_asked")) => {
            json!({"error": "They weren't asked yet, so nothing was changed."})
        }
        (_, Some(_)) => json!({"error": "That could not be done just now."}),
        _ => json!({"error": "That could not be done just now."}),
    };
    (told, true)
}

#[cfg(test)]
pub(crate) fn forget_asked() {
    if let Ok(mut asked) = ASKED.lock() {
        *asked = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The asked name is one for the process: its tests take turns.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    use std::io::{BufRead, BufReader, Write};

    /// A stand-in for the eyes' socket: each line asked is kept, each
    /// answered from `answer`.
    fn eyes(
        name: &str,
        answer: fn(&Value) -> Value,
    ) -> (std::path::PathBuf, std::sync::Arc<Mutex<Vec<Value>>>) {
        let path =
            std::env::temp_dir().join(format!("photo-faces-{name}-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let asked = std::sync::Arc::new(Mutex::new(Vec::new()));
        let kept = asked.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let value: Value = serde_json::from_str(line.trim()).unwrap();
                let reply = answer(&value);
                kept.lock().unwrap().push(value);
                let _ = writeln!(&stream, "{reply}");
            }
        });
        (path, asked)
    }

    fn call(name: &str) -> ToolCall {
        ToolCall {
            id: "c".into(),
            name: name.into(),
            args: json!({}),
        }
    }

    #[test]
    fn the_note_asks_once_and_never_uses_the_name_as_theirs() {
        let note = note("Lan");
        assert!(
            note.contains("Someone added your face from a photo, as Lan. Shall I remember you?")
        );
        assert!(note.contains(YES) && note.contains(NO));
        assert!(note.contains("Don't call them Lan unless they say yes"));
        let names: Vec<String> = tools().into_iter().map(|tool| tool.name).collect();
        assert_eq!(names, [YES, NO]);
        assert!(is_tool(YES) && is_tool(NO) && !is_tool("remember_my_face"));
    }

    /// The face here is asked about, and the person's yes or no goes to the
    /// eyes for that name; with nobody asked there is nothing to answer.
    #[test]
    fn a_yes_or_a_no_is_told_to_the_eyes_for_the_face_asked_about() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let (socket, asked) = eyes("answers", |line| {
            if line.get("photo_here").is_some() {
                json!({"photo_here": "Lan"})
            } else if line.get("photo_asked").is_some() {
                json!({"asked": true})
            } else if line.get("photo_yes").is_some() {
                json!({"remembered": "Lan"})
            } else {
                json!({"let_go": true})
            }
        });
        forget_asked();
        assert_eq!(here(&socket).as_deref(), Some("Lan"));
        assert!(!was_asked("Lan"));
        assert!(asking(&socket, "Lan"));
        assert!(was_asked("Lan"));
        // The answer runs in the provider's worker, where nothing was
        // asked: the face is the eyes' word, the one here now.
        forget_asked();
        let (said, answered) = run(&socket, &call(YES));
        assert!(answered && said["ok"] == true, "{said}");
        let (said, _) = run(&socket, &call(NO));
        assert!(said["result"].as_str().unwrap().contains("let it go"));
        let lines = asked.lock().unwrap().clone();
        assert_eq!(
            lines,
            [
                json!({"photo_here": true}),
                json!({"photo_asked": "Lan"}),
                json!({"photo_here": true}),
                json!({"photo_yes": "Lan"}),
                json!({"photo_here": true}),
                json!({"photo_no": "Lan"}),
            ]
        );
        let _ = std::fs::remove_file(socket);
    }

    /// The watch asks the eyes off the loop: the face here comes to it,
    /// asking is told to the eyes, and then the question is said once.
    #[test]
    fn the_watch_asks_the_eyes_off_the_loop_and_once() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let (socket, asked) = eyes("watch", |line| {
            if line.get("photo_here").is_some() {
                json!({"photo_here": "Hoa"})
            } else {
                json!({"asked": true})
            }
        });
        forget_asked();
        let watch = Watch::start(socket.clone());
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut found = None;
        while found.is_none() && std::time::Instant::now() < deadline {
            found = watch.to_ask();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(found.as_deref(), Some("Hoa"));
        watch.ask("Hoa".into());
        let mut taken = None;
        while taken.is_none() && std::time::Instant::now() < deadline {
            taken = watch.asked();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(taken.as_deref(), Some("Hoa"));
        // Asked once: no longer to ask, and said once.
        assert!(watch.to_ask().is_none());
        assert!(watch.asked().is_none());
        assert!(
            asked
                .lock()
                .unwrap()
                .contains(&json!({"photo_asked": "Hoa"}))
        );
        drop(watch);
        forget_asked();
        let _ = std::fs::remove_file(socket);
    }

    /// The switch reaches a worker as `--photo-faces` after the eyes'
    /// socket, declares the two functions, and routes them to the eyes;
    /// without it they are not declared.
    #[test]
    fn the_switch_reaches_the_worker_and_routes_to_the_eyes() {
        let flags: Vec<String> = vec![
            crate::known_faces::FLAG.into(),
            "/run/lampos-eyes/who.sock".into(),
            FLAG.into(),
        ];
        let (settings, rest) = crate::tools::take(&flags).unwrap();
        assert!(rest.is_empty() && settings.photo_faces);
        let names: Vec<String> = crate::tools::declared(&settings)
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert!(names.iter().any(|n| n == YES) && names.iter().any(|n| n == NO));
        assert!(matches!(
            crate::tools::route(&settings, YES),
            crate::tools::Route::Faces(..)
        ));
        let (plain, _) = crate::tools::take(&flags[..2]).unwrap();
        assert!(!plain.photo_faces);
        assert!(
            crate::tools::declared(&plain)
                .iter()
                .all(|tool| !is_tool(&tool.name))
        );
    }

    /// The eyes refuse a face that has gone: the model is told nothing
    /// changed.
    #[test]
    fn a_face_gone_from_the_lamp_changes_nothing() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let (socket, _) = eyes("gone", |line| {
            if line.get("photo_here").is_some() {
                json!({"photo_here": "Minh"})
            } else {
                json!({"error": "not_asked"})
            }
        });
        let (said, _) = run(&socket, &call(YES));
        assert!(said["error"].as_str().unwrap().contains("weren't asked"));
        // Nobody from a photo here: nothing is sent.
        let (empty, lines) = eyes("empty", |_| json!({"photo_here": null}));
        let (said, _) = run(&empty, &call(YES));
        assert!(
            said["error"]
                .as_str()
                .unwrap()
                .contains("Nobody added from a photo")
        );
        assert_eq!(*lines.lock().unwrap(), [json!({"photo_here": true})]);
        let _ = std::fs::remove_file(&empty);
        let _ = std::fs::remove_file(socket);
    }
}
