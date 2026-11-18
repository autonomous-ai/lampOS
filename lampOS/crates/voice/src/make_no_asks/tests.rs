//! `LAMP_MAKE_NO_ASKS` (the first builder, 18:41 and 18:45): each written
//! first and seen failing with the switch-on branch disabled.
use super::*;
use crate::make_skill_local::tests::{
    CODE_REQUEST, CODE_TOML, FOURTEEN, FOURTEEN_ASK, MOON_TIME, chat_at, code_answer_of, good,
    keeping, service, service_sent, task_asks,
};
use crate::make_skill_local::{make_with, set_owner_here, set_tasks_here, take_said_line};
use std::io::{BufRead, Write};
use std::path::Path;

const DIM: &str = "make me a skill that dims the light at 10 pm";
const NOW: u64 = 1_800_000_000_000;

/// The made test skill's trigger (`good()`'s): the voice resting.
const RESTING: &str = "topic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n";

/// A manage helper at `socket` that answers done to each of `lines` asks,
/// one a connection: the lines it was asked, in order.
fn helper(socket: &Path, lines: usize) -> std::thread::JoinHandle<Vec<Value>> {
    let _ = fs::remove_file(socket);
    let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
    std::thread::spawn(move || {
        (0..lines)
            .map(|_| {
                let (stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                std::io::BufReader::new(&stream)
                    .read_line(&mut line)
                    .unwrap();
                writeln!(&stream, r#"{{"done": "ok"}}"#).unwrap();
                serde_json::from_str(&line).unwrap()
            })
            .collect()
    })
}

/// A skill on the lamp named `name` on the resting trigger, lighting.
fn installed(keeping: &Keeping, name: &str) {
    let folder = keeping.installed.join(name);
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("skill.toml"),
        format!(
            "kind = \"installable\"\nversion = \"1.0.0\"\n\n[trigger]\n{RESTING}cooldown = \
             \"1h\"\n\n[[do]]\nlight = {{ colour = \"blue\" }}\n"
        ),
    )
    .unwrap();
}

/// The folder `name` on the lamp went on as `sha256`, as the manage
/// helper's install writes it.
fn went_on_as(keeping: &Keeping, name: &str, sha256: &str) {
    let folder = keeping.installed.join(name);
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join(".package.json"),
        json!({ "sha256": sha256 }).to_string(),
    )
    .unwrap();
}

fn names(keeping: &Keeping) -> Vec<String> {
    made_skill::entries(&keeping.directory)
        .iter()
        .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
        .collect()
}

/// A remove asked by voice of `name`, as `make_skill`'s `remove` asks
/// it: the owner's by `owners`, taken off by `remove`.
fn removing(
    name: &str,
    keeping: &Keeping,
    owners: impl FnOnce() -> bool,
    remove: impl FnOnce(&str) -> Result<(), String>,
) -> Value {
    crate::made_remove::asked_or_removed(
        crate::made_remove::Ask {
            name,
            request: 5,
            heard: true,
            sends_here: true,
        },
        keeping,
        || true,
        owners,
        remove,
    )
}

/// The quoted line a `next` tells the model to say.
fn quoted(next: &str) -> &str {
    next.rsplit_once(": \"")
        .map(|(_, line)| line.trim_end_matches('"'))
        .unwrap_or_default()
}

/// A plain make goes on at once and is said in one line with its undo,
/// through the model, in the conversation's language, with no question.
#[test]
fn a_plain_make_is_on_at_once_with_the_one_line() {
    set_here(Some(true));
    let (root, keeping) = keeping("no-asks-plain");
    let (url, served) = service(vec![(200, good())]);
    let asked = helper(&keeping.manage, 1);
    let told = make_with(7, Some(DIM), DIM, &keeping, chat_at(url), NOW, || None);
    assert_eq!(served.join().unwrap(), 1);
    assert_eq!(told["on"], true, "{told}");
    let next = told["next"].as_str().unwrap();
    assert!(next.starts_with(made_skill::ALREADY_DONE), "{next}");
    assert!(
        next.contains("in the language of the conversation"),
        "{next}"
    );
    assert!(next.contains("Ask nothing and add no question"), "{next}");
    assert!(
        next.contains("one short clause of what it will do"),
        "{next}"
    );
    assert_eq!(quoted(next), "Made evening dim. Say 'remove it' to undo.");
    assert!(!quoted(next).contains('?'), "{next}");
    assert!(told.get(crate::skill_write_yes::APPROVE_ID).is_none());
    assert_eq!(
        take_said_line().as_deref(),
        Some("Made evening dim. Say 'remove it' to undo.")
    );
    assert_eq!(
        asked.join().unwrap()[0]["install_made"],
        made_skill::entries(&keeping.directory)[0]["sha256"]
    );
    // Its undo, within ten minutes: "remove it" removes it.
    let remover = helper(&keeping.manage, 1);
    let sha256 = made_skill::entries(&keeping.directory)[0]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    went_on_as(&keeping, "evening-dim", &sha256);
    let undone = undo(&keeping, LAST, now_unix_ms());
    assert_eq!(
        quoted(undone["next"].as_str().unwrap()),
        "Removed evening dim. Say 'put it back' to undo.",
        "{undone}"
    );
    assert_eq!(remover.join().unwrap()[0]["remove"], "evening-dim");
    assert!(names(&keeping).is_empty());
    set_here(None);
    let _ = fs::remove_dir_all(&root);
}

/// A code make goes on at once with no yes, even one granted the camera,
/// only on the owner's ask; with `LAMP_MADE_SCRIPTS` off it never goes on,
/// and neither does it for anyone but the owner: each said plainly.
#[test]
fn a_code_make_is_on_at_once_with_no_yes_and_scripts_on_still_holds() {
    set_here(Some(true));
    made_skill::set_scripts_here(Some(true));
    crate::skill_task_yes::set_on_here(Some(true));
    set_owner_here(Some(true));
    let folder = "cd".repeat(32);
    set_tasks_here(Some(task_asks("photo-booth", &folder, "", &["camera"])));
    let (root, keeping) = keeping("no-asks-code");
    let (url, served) = service(vec![(200, code_answer_of(CODE_TOML))]);
    let asked = helper(&keeping.manage, 1);
    let told = make_with(
        7,
        Some(CODE_REQUEST),
        CODE_REQUEST,
        &keeping,
        chat_at(url),
        NOW,
        || None,
    );
    assert_eq!(served.join().unwrap(), 1);
    assert_eq!(told["on"], true, "{told}");
    assert_eq!(told["task"], true, "{told}");
    for none in [
        crate::skill_write_yes::APPROVE_ID,
        "code_waits_for_yes",
        "not_on",
    ] {
        assert!(told.get(none).is_none(), "{none}: {told}");
    }
    assert!(!crate::skill_write_yes::awaited(std::time::Instant::now()));
    assert_eq!(
        quoted(told["next"].as_str().unwrap()),
        "Made photo booth. Say 'remove it' to undo."
    );
    assert!(asked.join().unwrap()[0]["install_made"].is_string());
    assert_eq!(made_skill::entries(&keeping.directory)[0]["on"], true);
    let _ = take_said_line();
    set_tasks_here(None);

    // The same skill, kept and not on: never on with scripts off, nor on
    // anyone's ask but the owner's.
    let mut made = made_skill::made_of(
        &keeping.directory,
        &made_skill::entries(&keeping.directory)[0],
    )
    .unwrap();
    made.uses.code = true;
    made_skill::set_scripts_here(Some(false));
    let (on, line) = put_on(&keeping, &made, || true).unwrap();
    assert_eq!(on, Err(CODE_OFF.to_owned()));
    assert_eq!(line, made_skill::not_on_line(CODE_OFF));
    made_skill::set_scripts_here(Some(true));
    let (on, line) = put_on(&keeping, &made, || false).unwrap();
    assert_eq!(on, Err(NOT_OWNERS_CODE.to_owned()));
    assert!(
        line.starts_with("I made it but couldn't put it on: "),
        "{line}"
    );
    let told = made_told(&made, &on, &line, None);
    assert_eq!(told["on"], false);
    assert!(
        told["next"]
            .as_str()
            .unwrap()
            .contains("Never say it is on")
    );
    set_owner_here(None);
    crate::skill_task_yes::set_on_here(None);
    made_skill::set_scripts_here(None);
    set_here(None);
    let _ = fs::remove_dir_all(&root);
}

/// A clash replaces: the new one on, the old one turned off (never
/// deleted), one line with its undo; "keep the old one" puts the old one
/// back on and removes the new one.
#[test]
fn a_clash_replaces_and_its_undo_restores() {
    set_here(Some(true));
    let (root, keeping) = keeping("no-asks-clash");
    installed(&keeping, "rest-glow");
    let (url, served) = service(vec![(200, good())]);
    let asked = helper(&keeping.manage, 2);
    let told = make_with(3, Some(DIM), DIM, &keeping, chat_at(url), NOW, || None);
    assert_eq!(served.join().unwrap(), 1);
    assert_eq!(told["on"], true, "{told}");
    assert!(told.get(crate::made_clash::CLASH_ID).is_none(), "{told}");
    assert_eq!(told["replaced"], json!(["rest-glow"]), "{told}");
    let line = "Made evening dim and replaced your old rest glow. Say 'keep the old one' to undo.";
    assert_eq!(quoted(told["next"].as_str().unwrap()), line);
    assert_eq!(take_said_line().as_deref(), Some(line));
    let asked = asked.join().unwrap();
    assert!(asked[0]["install_made"].is_string(), "{asked:?}");
    assert_eq!(asked[1], json!({ "off": "rest-glow" }));
    assert!(
        keeping.installed.join("rest-glow").is_dir(),
        "never deleted"
    );
    assert!(!crate::made_clash::awaited(std::time::Instant::now()));
    // "Keep the old one": the model's undo.
    let sha256 = made_skill::entries(&keeping.directory)[0]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    went_on_as(&keeping, "evening-dim", &sha256);
    let undoing = helper(&keeping.manage, 2);
    let undone = undo(&keeping, "rest glow", now_unix_ms());
    assert_eq!(
        quoted(undone["next"].as_str().unwrap()),
        "Your old rest glow is back on, and I removed evening dim.",
        "{undone}"
    );
    let undoing = undoing.join().unwrap();
    assert_eq!(undoing[0], json!({ "on": "rest-glow" }));
    assert_eq!(undoing[1], json!({ "remove": "evening-dim" }));
    assert!(names(&keeping).is_empty());
    // Nothing more to undo.
    assert_eq!(
        undo(&keeping, LAST, now_unix_ms())["error"],
        NOTHING_TO_UNDO
    );
    set_here(None);
    let _ = fs::remove_dir_all(&root);
}

/// An unsure line makes nothing and asks nothing: no "could you say it
/// again?", nothing kept, no line left for a cut turn.
#[test]
fn an_unsure_line_makes_nothing_and_asks_nothing() {
    set_here(Some(true));
    made_skill::set_unrelated_here(Some(true));
    made_skill::set_english_here(Some(true));
    made_skill::set_asks_here(Some(true));
    let (root, keeping) = keeping("no-asks-unsure");
    let (url, served) = service_sent(vec![(200, MOON_TIME.to_owned()), (200, "No.".to_owned())]);
    let told = make_with(
        7,
        Some(FOURTEEN),
        FOURTEEN_ASK,
        &keeping,
        chat_at(url),
        NOW,
        || None,
    );
    assert_eq!(served.join().unwrap().len(), 2);
    assert_eq!(told["error"], NOT_ACTED, "{told}");
    assert_eq!(told["next"], NOT_ACTED_NEXT);
    assert!(!told.to_string().contains('?'), "{told}");
    assert!(!told.to_string().contains(made_skill::UNRELATED), "{told}");
    assert_eq!(take_said_line(), None);
    assert!(made_skill::entries(&keeping.directory).is_empty());
    made_skill::set_asks_here(None);
    made_skill::set_english_here(None);
    made_skill::set_unrelated_here(None);
    set_here(None);
    let _ = fs::remove_dir_all(&root);
}

/// A remove happens at once on the owner's ask, with no yes, said in one
/// line; "put it back" restores it from its keep and puts it on. Anyone
/// else's ask removes nothing, and several alike ask nothing.
#[test]
fn a_remove_happens_with_no_yes_and_put_it_back_restores() {
    set_here(Some(true));
    crate::skill_write_yes::clear_for_test();
    let (root, keeping) = keeping("no-asks-remove");
    crate::made_remove::tests::made_here(&keeping, &[("moon-fact", true), ("joke-time", true)]);
    // Not the owner's: nothing removed, no skill named.
    let told = removing("moon fact", &keeping, || false, |_| panic!("never removed"));
    assert_eq!(told["error"], NOT_OWNER, "{told}");
    assert_eq!(names(&keeping).len(), 2);
    // The owner's: removed at once, kept aside.
    let mut removed = Vec::new();
    let told = removing(
        "moon fact",
        &keeping,
        || true,
        |name| {
            removed.push(name.to_owned());
            Ok(())
        },
    );
    assert_eq!(removed, ["moon-fact"]);
    assert_eq!(told["removed"], true, "{told}");
    assert!(
        told.get(crate::skill_write_yes::APPROVE_ID).is_none(),
        "{told}"
    );
    assert!(!crate::skill_write_yes::awaited(std::time::Instant::now()));
    assert_eq!(
        quoted(told["next"].as_str().unwrap()),
        "Removed moon fact. Say 'put it back' to undo."
    );
    assert_eq!(names(&keeping), ["joke-time"]);
    assert!(!keeping.directory.join("moon-fact-1.0.0.tar").exists());
    // "Put it back", by its name: back in the index and on.
    let _ = fs::remove_dir_all(keeping.installed.join("moon-fact"));
    let asked = helper(&keeping.manage, 1);
    let back = undo(&keeping, "moon fact", now_unix_ms());
    assert_eq!(back["on"], true, "{back}");
    assert_eq!(
        quoted(back["next"].as_str().unwrap()),
        "moon fact is back on."
    );
    assert!(asked.join().unwrap()[0]["install_made"].is_string());
    assert!(keeping.directory.join("moon-fact-1.0.0.tar").is_file());
    let entry = made_skill::entries(&keeping.directory)
        .into_iter()
        .find(|entry| entry["name"] == "moon-fact")
        .unwrap();
    assert_eq!(entry["on"], true, "{entry}");
    // Two alike: nothing removed, nothing asked.
    crate::made_remove::tests::made_here(&keeping, &[("moon", true), ("moon-fact", true)]);
    let told = removing("moon fax", &keeping, || true, |_| panic!("never removed"));
    let next = told["next"].as_str().unwrap();
    assert!(next.contains("nothing was removed"), "{next}");
    assert!(next.contains("Ask nothing"), "{next}");
    set_here(None);
    let _ = fs::remove_dir_all(&root);
}

/// A failed put-on is told in one plain line with why, never as on, and
/// leaves nothing to undo.
#[test]
fn a_failed_put_on_is_told_plainly() {
    set_here(Some(true));
    let (root, keeping) = keeping("no-asks-failed");
    let (url, served) = service(vec![(200, good())]);
    // No manage helper listening.
    let told = make_with(7, Some(DIM), DIM, &keeping, chat_at(url), NOW, || None);
    assert_eq!(served.join().unwrap(), 1);
    assert_eq!(told["on"], false, "{told}");
    let next = told["next"].as_str().unwrap();
    assert!(next.contains("Never say it is on"), "{next}");
    let line = quoted(next);
    assert!(
        line.starts_with("I made it but couldn't put it on: "),
        "{line}"
    );
    assert!(!line.contains("remove it"), "{line}");
    assert!(told["not_on"].is_string(), "{told}");
    assert_eq!(take_said_line().as_deref(), Some(line));
    assert_eq!(
        undo(&keeping, LAST, now_unix_ms())["error"],
        NOTHING_TO_UNDO
    );
    set_here(None);
    let _ = fs::remove_dir_all(&root);
}

/// The switch off: a clash still asks, a remove still asks its one
/// question and waits for the yes, and `make_skill` has no undo.
#[test]
fn with_the_switch_off_nothing_changes() {
    set_here(Some(false));
    crate::skill_write_yes::clear_for_test();
    let parameters = crate::make_skill_local::tool().parameters.unwrap();
    assert!(parameters["properties"].get(UNDO).is_none());
    let (root, keeping) = keeping("no-asks-off");
    installed(&keeping, "rest-glow");
    let (url, served) = service(vec![(200, good())]);
    let told = make_with(3, Some(DIM), DIM, &keeping, chat_at(url), NOW, || None);
    assert_eq!(served.join().unwrap(), 1);
    assert_eq!(told["on"], false, "{told}");
    assert!(told[crate::made_clash::CLASH_ID].is_string(), "{told}");
    let _ = take_said_line();
    crate::made_remove::tests::made_here(&keeping, &[("moon-fact", true)]);
    let told = removing("moon fact", &keeping, || true, |_| panic!("never removed"));
    assert!(
        told[crate::skill_write_yes::APPROVE_ID].is_string(),
        "{told}"
    );
    assert!(told["next"].as_str().unwrap().contains("Remove moon fact?"));
    assert_eq!(names(&keeping), ["moon-fact"]);
    crate::skill_write_yes::clear_for_test();
    set_here(None);
    let _ = fs::remove_dir_all(&root);
}

/// The switch on: `make_skill` takes the undo, and its description keeps
/// within the link's 1,024 bytes with every switch on.
#[test]
fn with_the_switch_on_the_tool_takes_the_undo() {
    set_here(Some(true));
    made_skill::set_scripts_here(Some(true));
    let tool = crate::make_skill_local::tool();
    assert!(tool.parameters.unwrap()["properties"][UNDO].is_object());
    assert!(!tool.description.contains("once the person agrees"));
    assert!(
        tool.description.len() <= 1_024,
        "{}",
        tool.description.len()
    );
    made_skill::set_scripts_here(None);
    set_here(None);
}
