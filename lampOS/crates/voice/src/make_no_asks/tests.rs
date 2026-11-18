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
    assert_eq!(undone["removed"], "evening-dim", "{undone}");
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
    set_owner_here(Some(true));
    let (root, keeping) = keeping("no-asks-clash");
    set_state_here(Some(root.join("state")));
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
        "Your old rest glow is back on, and I removed evening dim. Say 'put it back' to undo.",
        "{undone}"
    );
    // The fields `make_skill`'s other answers have (#7730's status).
    assert_eq!(
        (&undone["made"], &undone["on"], &undone["removed"]),
        (&json!("rest-glow"), &json!(true), &json!("evening-dim")),
        "{undone}"
    );
    let undoing = undoing.join().unwrap();
    assert_eq!(undoing[0], json!({ "on": "rest-glow" }));
    assert_eq!(undoing[1], json!({ "remove": "evening-dim" }));
    assert!(names(&keeping).is_empty());
    // The new one kept aside, as a make's undo keeps it: "put it back".
    assert!(
        keeping
            .directory
            .join(KEPT_FOLDER)
            .join(format!("{sha256}.tar"))
            .is_file()
    );
    set_state_here(None);
    set_owner_here(None);
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
    assert_eq!(told["removed"], "moon-fact", "{told}");
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
    assert_eq!(
        (&back["made"], &back["on"]),
        (&json!("moon-fact"), &json!(true)),
        "{back}"
    );
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

/// A records file in `keeping`'s folder holding `records`.
fn records_are(keeping: &Keeping, records: &[Value]) {
    fs::create_dir_all(&keeping.directory).unwrap();
    fs::write(
        keeping.directory.join(RECORDS),
        serde_json::to_vec(records).unwrap(),
    )
    .unwrap();
}

/// A removed skill `name` kept aside at `at`, its package in the keep.
fn removed_at(keeping: &Keeping, name: &str, sha: char, at: u64) -> Value {
    let sha256 = sha.to_string().repeat(64);
    let kept = keeping.directory.join(KEPT_FOLDER);
    fs::create_dir_all(&kept).unwrap();
    fs::write(kept.join(format!("{sha256}.tar")), "tar").unwrap();
    json!({
        "kind": "removed",
        "name": name,
        "at_unix_ms": at,
        "entry": {
            "name": name,
            "sha256": sha256,
            "package": format!("{name}-1.0.0.tar"),
            "version": "1.0.0",
            "description": "",
            "uses": {},
            "on": true,
        },
    })
}

const MINUTE: u64 = 60_000;
const DAY: u64 = 86_400_000;

/// Twelve's P1 on #7783: a guest's make that clashes goes on beside the
/// owner's skill, which stays on, and nothing is asked; a clash with a
/// skill already off turns nothing off and restores nothing later.
#[test]
fn a_guests_clash_and_a_clash_with_an_off_skill_turn_nothing_off() {
    set_here(Some(true));
    let (root, keeping) = keeping("no-asks-guest");
    set_state_here(Some(root.join("state")));
    installed(&keeping, "rest-glow");
    set_owner_here(Some(false));
    let (url, served) = service(vec![(200, good())]);
    let asked = helper(&keeping.manage, 1);
    let told = make_with(3, Some(DIM), DIM, &keeping, chat_at(url), NOW, || None);
    assert_eq!(served.join().unwrap(), 1);
    assert_eq!(told["on"], true, "{told}");
    assert!(told.get("replaced").is_none(), "{told}");
    assert_eq!(
        quoted(told["next"].as_str().unwrap()),
        "Made evening dim. Your other rest glow is still on. Say 'remove it' to undo."
    );
    assert!(!told.to_string().contains("keep the old one"), "{told}");
    // Only the put-on was asked of the helper: rest glow never off.
    assert_eq!(asked.join().unwrap().len(), 1);
    let _ = take_said_line();
    let _ = fs::remove_dir_all(&root);

    // The owner's make, the other skill off already: nothing turned off,
    // and nothing to turn back on.
    let (root, keeping) = crate::make_skill_local::tests::keeping("no-asks-off-clash");
    set_state_here(Some(root.join("state")));
    fs::create_dir_all(root.join("state")).unwrap();
    fs::write(root.join("state").join("off"), "rest-glow\n").unwrap();
    installed(&keeping, "rest-glow");
    set_owner_here(Some(true));
    let (url, served) = service(vec![(200, good())]);
    let asked = helper(&keeping.manage, 1);
    let told = make_with(3, Some(DIM), DIM, &keeping, chat_at(url), NOW, || None);
    assert_eq!(served.join().unwrap(), 1);
    assert_eq!(
        quoted(told["next"].as_str().unwrap()),
        "Made evening dim. Say 'remove it' to undo."
    );
    assert_eq!(asked.join().unwrap().len(), 1);
    let records = records(&keeping.directory, now_unix_ms());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["kind"], "made", "{records:?}");
    let _ = take_said_line();
    set_state_here(None);
    set_owner_here(None);
    set_here(None);
    let _ = fs::remove_dir_all(&root);
}

/// Twelve's P2 on #7783: "last" is the latest action, undone only within
/// its own window. A remove on Monday, a make 15 minutes ago: "remove it"
/// is past the make's 10 minutes and never puts Monday's remove back; the
/// remove alone, latest, still is put back within its 7 days.
#[test]
fn an_undo_is_bound_to_the_latest_action_and_its_own_window() {
    let (root, keeping) = keeping("no-asks-last");
    let now = NOW;
    let monday = removed_at(&keeping, "moon-fact", 'a', now - 3 * DAY);
    records_are(
        &keeping,
        &[
            monday.clone(),
            json!({ "kind": "made", "name": "joke-time", "sha256": "b".repeat(64), "at_unix_ms": now - 15 * MINUTE }),
        ],
    );
    assert_eq!(undo(&keeping, LAST, now)["error"], NOTHING_TO_UNDO);
    assert_eq!(undo(&keeping, "joke time", now)["error"], NOTHING_TO_UNDO);
    assert!(
        made_skill::entries(&keeping.directory).is_empty(),
        "nothing put back"
    );
    // Named, the remove is its own latest action: put back.
    let asked = helper(&keeping.manage, 1);
    let back = undo(&keeping, "moon fact", now);
    assert_eq!(
        (&back["made"], &back["on"]),
        (&json!("moon-fact"), &json!(true)),
        "{back}"
    );
    asked.join().unwrap();
    // A replace's record outlives the remove of its new skill.
    let (root2, keeping2) = crate::make_skill_local::tests::keeping("no-asks-last-replaced");
    records_are(
        &keeping2,
        &[
            json!({ "kind": "replaced", "name": "evening-dim", "sha256": "c".repeat(64), "others": ["rest-glow"], "at_unix_ms": now_unix_ms() - MINUTE }),
        ],
    );
    noted(
        &keeping2.directory,
        removed_at(&keeping2, "evening-dim", 'c', 0),
    );
    let kinds: Vec<Value> = records(&keeping2.directory, now_unix_ms())
        .iter()
        .map(|record| record["kind"].clone())
        .collect();
    assert_eq!(kinds, [json!("replaced"), json!("removed")]);
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&root2);
}

/// The 7-day keep: past it, a record and its package go at the purge (at
/// the voice's start and hourly), and a package no record names; at most
/// `MOST_KEPT` removes are kept, the oldest out first; the owner's
/// "forget everything" clears it all.
#[test]
fn the_keep_is_purged_bounded_and_forgotten_with_everything() {
    let (root, keeping) = keeping("no-asks-purge");
    let now = NOW;
    let old = removed_at(&keeping, "old-one", 'a', now - 8 * DAY);
    let fresh = removed_at(&keeping, "fresh-one", 'b', now - DAY);
    records_are(&keeping, &[old, fresh]);
    let stray = keeping.directory.join(KEPT_FOLDER).join("stray.tar");
    fs::write(&stray, "tar").unwrap();
    purge(&keeping.directory, now);
    let kept = records(&keeping.directory, now);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0]["name"], "fresh-one");
    let folder = keeping.directory.join(KEPT_FOLDER);
    assert!(!folder.join(format!("{}.tar", "a".repeat(64))).exists());
    assert!(folder.join(format!("{}.tar", "b".repeat(64))).exists());
    assert!(!stray.exists());
    // Bounded: the oldest removes out first, their packages with them.
    let many: Vec<Value> = (0..MOST_KEPT + 3)
        .map(|at| {
            let mut record =
                removed_at(&keeping, &format!("skill-{at}"), 'c', now - DAY + at as u64);
            record["entry"]["sha256"] = json!(format!("{at:064x}"));
            fs::write(folder.join(format!("{at:064x}.tar")), "tar").unwrap();
            record
        })
        .collect();
    records_are(&keeping, &many);
    purge(&keeping.directory, now);
    let kept = records(&keeping.directory, now);
    assert_eq!(kept.len(), MOST_KEPT);
    assert_eq!(kept[0]["name"], "skill-3");
    assert!(!folder.join(format!("{:064x}.tar", 0)).exists());
    assert!(folder.join(format!("{:064x}.tar", 3)).exists());
    // Another person's forget, or one not emptied, leaves it.
    let owners = ["owner".to_owned()];
    let told = (json!({ "forgotten": true }), true);
    let (left, _) = forget_if(&keeping.directory, told.clone(), Some("guest"), &owners);
    assert!(left.get("removed_skills_kept").is_none());
    assert!(keeping.directory.join(RECORDS).exists());
    let (left, _) = forget_if(
        &keeping.directory,
        (json!({}), true),
        Some("owner"),
        &owners,
    );
    assert!(left.get("removed_skills_kept").is_none());
    // The owner's "forget everything": the records and the packages go.
    let (cleared, _) = forget_if(&keeping.directory, told, Some("owner"), &owners);
    assert_eq!(cleared["removed_skills_kept"], "forgotten too");
    assert!(!keeping.directory.join(RECORDS).exists());
    assert!(!folder.exists());
    let _ = fs::remove_dir_all(&root);
}

/// A skill that sends keeps its read-back and yes: nothing here puts it on.
#[test]
fn a_sending_skill_keeps_its_yes() {
    set_here(Some(true));
    let (root, keeping) = keeping("no-asks-sends");
    let mut made = Made {
        name: "leaving-mail".to_owned(),
        version: "1.0.0".to_owned(),
        sha256: "d".repeat(64),
        description: String::new(),
        uses: Default::default(),
        package: root.join("none"),
    };
    made.uses.writes = vec!["gmail".to_owned()];
    assert!(!keeps_no_yes(&made));
    assert!(put_on(&keeping, &made, || true).is_none());
    made.uses.writes.clear();
    made.uses.code = true;
    made.uses.messages = true;
    assert!(!keeps_no_yes(&made));
    assert!(put_on(&keeping, &made, || true).is_none());
    assert!(!keeping.directory.join(RECORDS).exists());
    set_here(None);
    let _ = fs::remove_dir_all(&root);
}

/// A put-back of a skill with code of its own, `LAMP_MADE_SCRIPTS` off:
/// back in the index, kept off, and why said; never on.
#[test]
fn a_code_skill_put_back_with_scripts_off_stays_off() {
    made_skill::set_scripts_here(Some(false));
    let (root, keeping) = keeping("no-asks-put-back-code");
    let mut record = removed_at(&keeping, "photo-booth", 'e', NOW);
    record["entry"]["uses"] = json!({ "code": true });
    records_are(&keeping, &[record]);
    let back = undo(&keeping, LAST, NOW);
    assert_eq!(
        (&back["made"], &back["on"]),
        (&json!("photo-booth"), &json!(false)),
        "{back}"
    );
    assert_eq!(back["not_on"], CODE_OFF);
    assert_eq!(
        quoted(back["next"].as_str().unwrap()),
        format!("I put photo booth back, but it stays off: {CODE_OFF}.")
    );
    let entries = made_skill::entries(&keeping.directory);
    assert_eq!(entries.len(), 1);
    assert_ne!(entries[0]["on"], true);
    made_skill::set_scripts_here(None);
    let _ = fs::remove_dir_all(&root);
}

/// Fix 1 of #7828's DEFAULT-ON (lamp-9adf, 19 Nov, on the second route:
/// "Do you want me to make it again?", "Did you mean the flip-a-coin
/// skill? Say the word and I'll remove it.", "Which one…?" to "remove
/// it"): with the switch on, `make_skill` tells the model to make and to
/// remove at once with the undo in its line, never asking first, and
/// asking which only when two skills they made fit alike, never of "it";
/// within the link's bounds with every switch the lamp's unit sets on.
/// Off, the description and the remove are as before.
#[test]
fn make_and_remove_are_told_at_once_and_never_asked_first() {
    made_skill::set_scripts_here(Some(true));
    made_skill::set_writes_here(Some(true));
    made_skill::set_off_said_here(Some(true));
    crate::skill_task_yes::set_on_here(Some(true));
    let remove_of = |tool: &lamp_gemini::Tool| {
        tool.parameters.as_ref().unwrap()["properties"][crate::made_remove::REMOVE]["description"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    set_here(Some(false));
    let off = crate::make_skill_local::tool();
    assert!(
        !off.description.contains(AT_ONCE_TOLD),
        "{}",
        off.description
    );
    assert!(!remove_of(&off).contains(REMOVE_AT_ONCE_TOLD));
    set_here(Some(true));
    let on = crate::make_skill_local::tool();
    assert!(on.description.ends_with(AT_ONCE_TOLD), "{}", on.description);
    for part in ["at once", "never ask", "undo"] {
        assert!(on.description.contains(part), "{part}: {}", on.description);
    }
    let remove = remove_of(&on);
    assert!(remove.ends_with(REMOVE_AT_ONCE_TOLD), "{remove}");
    for part in [
        "at once",
        "never asking first",
        "two skills they made",
        "\"it\"",
    ] {
        assert!(remove.contains(part), "{part}: {remove}");
    }
    // "Put it back" by a removed skill's name is the undo, never a make.
    let undo = on.parameters.as_ref().unwrap()["properties"][UNDO]["description"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(undo.contains("never a new make"), "{undo}");
    assert!(on.description.len() <= 1_024, "{}", on.description.len());
    let parameters = on.parameters.as_ref().unwrap().to_string().len();
    assert!(
        parameters <= lamp_gemini::MAX_TOOL_JSON_BYTES,
        "{parameters}"
    );
    assert!(on.validate().is_ok());
    crate::skill_task_yes::set_on_here(None);
    made_skill::set_off_said_here(None);
    made_skill::set_writes_here(None);
    made_skill::set_scripts_here(None);
    set_here(None);
}

/// Fix 2 of #7828's DEFAULT-ON (lamp-9adf, 19 Nov: "Put the flip a coin
/// skill back", just removed, was called as a make and made a new one): a
/// make whose words name a skill they removed, kept aside, puts that one
/// back and on, "flip a coin is back on.", and the making model is never
/// asked. Matched by spelling as a remove's name is, so "moon fact" is
/// moon fact, never moon beside it. Anyone else's line, or one naming no
/// skill kept aside, is a make as before; off, nothing is put back.
#[test]
fn a_make_naming_a_skill_kept_aside_puts_it_back() {
    set_here(Some(true));
    set_owner_here(Some(true));
    crate::skill_write_yes::clear_for_test();
    let (root, keeping) = keeping("no-asks-named-back");
    crate::made_remove::tests::made_here(
        &keeping,
        &[("flip-a-coin", true), ("moon", true), ("moon-fact", true)],
    );
    for name in ["flip a coin", "moon", "moon fact"] {
        let told = removing(name, &keeping, || true, |_| Ok(()));
        assert!(told["removed"].is_string(), "{name}: {told}");
    }
    for name in ["flip-a-coin", "moon", "moon-fact"] {
        let _ = fs::remove_dir_all(keeping.installed.join(name));
    }
    assert!(names(&keeping).is_empty());
    const BACK: &str = "put the flip a coin skill back";
    // Off: a make as before, which the making model is asked for.
    set_here(Some(false));
    let (url, served) = service(Vec::new());
    let told = make_with(8, Some(BACK), BACK, &keeping, chat_at(url), NOW, || None);
    served.join().unwrap();
    assert!(told.get("put_back").is_none(), "{told}");
    assert!(names(&keeping).is_empty());
    let _ = take_said_line();
    // Anyone else's line: nothing put back.
    set_here(Some(true));
    set_owner_here(Some(false));
    let (url, served) = service(Vec::new());
    let told = make_with(9, Some(BACK), BACK, &keeping, chat_at(url), NOW, || None);
    served.join().unwrap();
    assert!(told.get("put_back").is_none(), "{told}");
    assert!(names(&keeping).is_empty());
    let _ = take_said_line();
    // The owner's: put back and on, the making model never asked.
    set_owner_here(Some(true));
    for (at, (words, name, said)) in [
        (BACK, "flip-a-coin", "flip a coin is back on."),
        (
            "bring back my moon fact skill",
            "moon-fact",
            "moon fact is back on.",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let (url, served) = service(Vec::new());
        let asked = helper(&keeping.manage, 1);
        let told = make_with(
            10 + at as u64,
            Some(words),
            words,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 0, "{words}: {told}");
        assert_eq!(
            (&told["made"], &told["on"], &told["put_back"]),
            (&json!(name), &json!(true), &json!(true)),
            "{words}: {told}"
        );
        assert_eq!(quoted(told["next"].as_str().unwrap()), said);
        assert!(asked.join().unwrap()[0]["install_made"].is_string());
    }
    assert_eq!(names(&keeping), ["flip-a-coin", "moon-fact"]);
    // A make naming no skill kept aside is a make.
    let (url, served) = service(vec![(200, good())]);
    let asked = helper(&keeping.manage, 1);
    let told = make_with(12, Some(DIM), DIM, &keeping, chat_at(url), NOW, || None);
    assert_eq!(served.join().unwrap(), 1);
    assert_eq!(told["made"], "evening-dim", "{told}");
    asked.join().unwrap();
    let _ = take_said_line();
    set_owner_here(None);
    set_here(None);
    let _ = fs::remove_dir_all(&root);
}
