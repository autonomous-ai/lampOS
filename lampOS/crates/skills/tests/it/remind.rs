//! A skill's reminder (#6066, item 2): `remind = { at | in, words }` sets one
//! timer or reminder through the actions part's own `set_reminder` or
//! `set_timer`, held to `[permissions] actions`, at most
//! `remind::MOST_A_DAY` a day, never stacked on the same words, and its
//! words fixed plain text checked when it loads. The actions part is stood
//! in for by a socket that keeps a list as the schedule does.
use lamp_skills::Permissions;
use lamp_skills::remind::{self, MOST_A_DAY, Remind, RemindFile, Tally};
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The stand-in actions part: every call it took, in order, and what it
/// keeps set (`id`, `text`, `clock`).
#[derive(Default)]
struct Schedule {
    calls: Vec<(String, Value)>,
    set: Vec<(u64, String, String)>,
}

/// The actions part on `dir/actions.sock`, answering as the real one's
/// schedule adapter does: `list_reminders` one page of what is set, and a
/// set `committed` with the local `clock` it is due.
fn actions(dir: &Path) -> (PathBuf, Arc<Mutex<Schedule>>) {
    let socket = dir.join("actions.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let schedule = Arc::new(Mutex::new(Schedule::default()));
    let kept = Arc::clone(&schedule);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let asked: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(asked["op"], "call");
            let name = asked["name"].as_str().unwrap().to_owned();
            let args = asked["args"].clone();
            // A skill's call is the room's: it names no caller.
            assert!(asked.get("caller").is_none());
            let mut schedule = kept.lock().unwrap();
            schedule.calls.push((name.clone(), args.clone()));
            let answer = match name.as_str() {
                "list_reminders" => {
                    let items: Vec<Value> = schedule
                        .set
                        .iter()
                        .map(|(id, text, clock)| {
                            json!({"id": id, "kind": "reminder", "text": text,
                                "state": "scheduled", "clock": clock})
                        })
                        .collect();
                    json!({"ok": true, "result": {"items": items, "next_after_id": null,
                        "pending_only": false, "complete": true}})
                }
                "set_reminder" | "set_timer" => {
                    let id = schedule.set.len() as u64 + 1;
                    let clock = args["time"].as_str().unwrap_or("10:20").to_owned();
                    let text = args["text"].as_str().unwrap().to_owned();
                    schedule.set.push((id, text, clock.clone()));
                    json!({"ok": true, "result": {"status": "committed", "id": id,
                        "delivery": 1, "clock": clock}})
                }
                _ => json!({"ok": false, "error": "I can't do that."}),
            };
            (&stream)
                .write_all(format!("{answer}\n").as_bytes())
                .unwrap();
        }
    });
    (socket, schedule)
}

fn step(toml: &str) -> Result<Remind, String> {
    toml::from_str::<RemindFile>(toml)
        .map_err(|error| error.to_string())?
        .step()
}

fn may(actions: &[&str]) -> Permissions {
    Permissions {
        actions: actions.iter().map(|name| (*name).to_owned()).collect(),
        ..Permissions::default()
    }
}

fn socket_dir() -> tempfile::TempDir {
    // Short: a socket's whole path has to fit in 103 bytes.
    tempfile::Builder::new()
        .prefix("rm")
        .tempdir_in("/tmp")
        .unwrap()
}

/// A skill folder `name` with this `skill.toml`, loaded.
fn load(dir: &Path, name: &str, toml: &str) -> Result<lamp_skills::Skill, String> {
    let folder = dir.join(name);
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: A test.\n---\n"),
    )
    .unwrap();
    fs::write(folder.join("skill.toml"), toml).unwrap();
    lamp_skills::load(&folder)
}

/// "At 10 pm, remind me to drink water", as a skill writes it: set when
/// someone is at the desk in the evening, once a day.
const WATER: &str = r#"
[trigger]
topic = "room/presence"
field = "present"
equals = true
[ask]
policy = "reminder"
[[do]]
remind = { at = "22:00", words = "Time to drink some water." }
[permissions]
topics = ["room/presence"]
actions = ["set_reminder"]
"#;

#[test]
fn a_reminder_is_set_once_through_the_actions_part() {
    let dir = socket_dir();
    let (socket, schedule) = actions(dir.path());
    let remind = step(
        r#"at = "22:00"
words = "Time to drink some water.""#,
    )
    .unwrap();
    assert_eq!(remind.action(), "set_reminder");
    let mut tally = Tally::default();
    let said = remind::set(
        &socket,
        "water",
        &may(&["set_reminder"]),
        &remind,
        &mut tally,
        1_000_000,
    );
    assert_eq!(said, "remind at 22:00: set, due 22:00");
    let schedule = schedule.lock().unwrap();
    let names: Vec<&str> = schedule
        .calls
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(names, ["list_reminders", "set_reminder"]);
    assert_eq!(
        schedule.calls[1].1,
        json!({"text": "Time to drink some water.", "time": "22:00"})
    );
    // Its words never reach the journal's line.
    assert!(!said.contains("water"));
    assert_eq!(tally.count("water", 1_000_000), 1);
}

#[test]
fn a_timer_from_now_is_set_with_set_timer() {
    let dir = socket_dir();
    let (socket, schedule) = actions(dir.path());
    let remind = step(
        r#"in = "1h"
words = "Time to stand up and stretch.""#,
    )
    .unwrap();
    assert_eq!(remind.action(), "set_timer");
    let said = remind::set(
        &socket,
        "stretch",
        &may(&["set_timer"]),
        &remind,
        &mut Tally::default(),
        0,
    );
    assert!(said.starts_with("remind in 3600 s: set, due "), "{said}");
    assert_eq!(
        schedule.lock().unwrap().calls[1],
        (
            "set_timer".into(),
            json!({"text": "Time to stand up and stretch.", "seconds": 3600})
        )
    );
}

#[test]
fn the_same_words_already_set_are_not_stacked() {
    let dir = socket_dir();
    let (socket, schedule) = actions(dir.path());
    let remind = step(
        r#"at = "22:00"
words = "Time to drink some water.""#,
    )
    .unwrap();
    // Already set, by the person or an earlier run, spaced and cased otherwise.
    schedule
        .lock()
        .unwrap()
        .set
        .push((7, "time to  drink some WATER.".into(), "22:00".into()));
    let mut tally = Tally::default();
    for at in [0, 3_600_000_000] {
        let said = remind::set(
            &socket,
            "water",
            &may(&["set_reminder"]),
            &remind,
            &mut tally,
            at,
        );
        assert_eq!(
            said,
            "remind at 22:00: not set, the same words are set already, due 22:00"
        );
    }
    let schedule = schedule.lock().unwrap();
    assert!(
        schedule
            .calls
            .iter()
            .all(|(name, _)| name == "list_reminders")
    );
    assert_eq!(schedule.set.len(), 1);
    // Not counted against its day: nothing was asked.
    assert_eq!(tally.count("water", 3_600_000_000), 0);
}

#[test]
fn at_most_a_few_a_day_are_set() {
    let dir = socket_dir();
    let (socket, schedule) = actions(dir.path());
    let permissions = may(&["set_timer"]);
    let mut tally = Tally::default();
    let hour = 3_600_000_000;
    for n in 0..MOST_A_DAY as u64 {
        let remind = step(&format!("in = \"20m\"\nwords = \"Stretch number {n}.\"")).unwrap();
        let said = remind::set(
            &socket,
            "stretch",
            &permissions,
            &remind,
            &mut tally,
            n * hour,
        );
        assert!(said.contains(": set, due "), "{said}");
    }
    let more = step("in = \"20m\"\nwords = \"One stretch too many.\"").unwrap();
    let said = remind::set(
        &socket,
        "stretch",
        &permissions,
        &more,
        &mut tally,
        MOST_A_DAY as u64 * hour,
    );
    assert_eq!(
        said,
        format!("remind in 1200 s: not set, {MOST_A_DAY} set within a day already")
    );
    let calls = schedule.lock().unwrap().calls.len();
    // The bound is the skill's own: another skill still sets one.
    let other = remind::set(
        &socket,
        "water",
        &permissions,
        &more,
        &mut tally,
        MOST_A_DAY as u64 * hour,
    );
    assert!(other.contains(": set, due "), "{other}");
    // A day after the first, one more is allowed.
    let later = step("in = \"20m\"\nwords = \"Stretch again.\"").unwrap();
    let said = remind::set(
        &socket,
        "stretch",
        &permissions,
        &later,
        &mut tally,
        24 * hour,
    );
    assert!(said.contains(": set, due "), "{said}");
    let schedule = schedule.lock().unwrap();
    // The refused one asked nothing of the actions part.
    assert_eq!(schedule.calls.len(), calls + 4);
    assert_eq!(schedule.set.len(), MOST_A_DAY + 2);
}

#[test]
fn without_the_permission_it_is_refused() {
    let dir = socket_dir();
    let (socket, schedule) = actions(dir.path());
    let remind = step(
        r#"at = "22:00"
words = "Time to drink some water.""#,
    )
    .unwrap();
    // Neither nothing, nor the other function, nor the light gives it.
    for permissions in [
        may(&[]),
        may(&["set_timer"]),
        Permissions {
            light: true,
            ..Permissions::default()
        },
    ] {
        let said = remind::set(
            &socket,
            "water",
            &permissions,
            &remind,
            &mut Tally::default(),
            0,
        );
        assert!(
            said.contains(
                "refused, the actions part's set_reminder is not in [permissions] actions"
            ),
            "{said}"
        );
    }
    assert!(schedule.lock().unwrap().calls.is_empty());
    // And it does not load.
    let skills = tempfile::tempdir().unwrap();
    let why = load(
        skills.path(),
        "water",
        &WATER.replace("actions = [\"set_reminder\"]", "actions = [\"set_timer\"]"),
    )
    .err()
    .unwrap();
    assert!(
        why.contains(
            "not permitted: the actions part's set_reminder is not in [permissions] actions"
        ),
        "{why}"
    );
    let loaded = load(skills.path(), "water-ok", WATER).unwrap();
    assert_eq!(loaded.permissions.actions, ["set_reminder"]);
}

#[test]
fn bad_words_or_times_are_refused_at_load() {
    let skills = tempfile::tempdir().unwrap();
    let good = "remind = { at = \"22:00\", words = \"Time to drink some water.\" }";
    let long = format!(
        "remind = {{ at = \"22:00\", words = \"{}.\" }}",
        "a".repeat(120)
    );
    for (n, (bad, why)) in [
        (
            r#"remind = { at = "22:00", words = "Read https://example.com now." }"#,
            "remind.words",
        ),
        (
            r#"remind = { at = "22:00", words = "Say 'hi' {now}." }"#,
            "remind.words",
        ),
        (
            "remind = { at = \"22:00\", words = \"One. Two. Three.\" }",
            "remind.words",
        ),
        (
            "remind = { at = \"22:00\", words = \"Zero\u{200B}width.\" }",
            "remind.words",
        ),
        (
            "remind = { at = \"22:00\", words = \"Line\\nbreak.\" }",
            "remind.words",
        ),
        (
            "remind = { at = \"22:00\", words = \"   \" }",
            "remind.words",
        ),
        (long.as_str(), "remind.words"),
        (
            r#"remind = { at = "10pm", words = "Drink water." }"#,
            "is not a time of day",
        ),
        (
            r#"remind = { in = "30s", words = "Drink water." }"#,
            "from 1m to 24h",
        ),
        (
            r#"remind = { in = "25h", words = "Drink water." }"#,
            "from 1m to 24h",
        ),
        (
            r#"remind = { at = "22:00", in = "1h", words = "Drink water." }"#,
            "one of at or in",
        ),
        (r#"remind = { words = "Drink water." }"#, "one of at or in"),
        (
            r#"remind = { at = "22:00", note = "Drink water." }"#,
            "unknown field",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let toml = WATER.replace(good, bad);
        assert_ne!(toml, WATER);
        let refused = load(skills.path(), &format!("bad-{n}"), &toml).err();
        assert!(
            refused.as_deref().is_some_and(|said| said.contains(why)),
            "{bad}: {refused:?}"
        );
    }
}

#[test]
fn a_tool_skill_has_no_remind() {
    let skills = tempfile::tempdir().unwrap();
    let toml = r#"
tool = "water_reminder"
[[do]]
remind = { at = "22:00", words = "Time to drink some water." }
[permissions]
actions = ["set_reminder"]
"#;
    let why = load(skills.path(), "asked", toml).err().unwrap();
    assert!(why.contains("remind, in a skill with a tool"), "{why}");
}

#[test]
fn a_list_that_cannot_be_read_sets_nothing() {
    let dir = socket_dir();
    // Nothing listens: the actions part is not reached, so nothing is set
    // and nothing counted.
    let socket = dir.path().join("actions.sock");
    let remind = step(
        r#"at = "22:00"
words = "Time to drink some water.""#,
    )
    .unwrap();
    let mut tally = Tally::default();
    let said = remind::set(
        &socket,
        "water",
        &may(&["set_reminder"]),
        &remind,
        &mut tally,
        0,
    );
    assert!(
        said.starts_with("remind at 22:00: not set, what is set could not be read"),
        "{said}"
    );
    assert_eq!(tally.count("water", 0), 0);
}
