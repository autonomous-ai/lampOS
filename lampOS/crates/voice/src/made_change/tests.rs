//! A spoken change to a skill the person made (#7975, #8008), each
//! written first and seen failing with the change kept as a new make
//! (`made_skill::ChangeAs` not set): a second skill, `evening-dim-2` or
//! the model's own name, beside the first.
use super::*;
use crate::make_skill_local::tests::{chat_at, good, service, service_sent};
use crate::make_skill_local::{make_with, set_owner_here, take_said_line};
use std::io::{BufRead, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const DIM: &str = "make me a skill that dims the light at 10 pm";
const LATER: &str = "change evening dim to half past ten";
const HALF_PAST: &str = "make me a skill that dims the light at half past ten";
const NOW: u64 = 1_800_000_000_000;

/// The changed skill as the making model writes it: the same trigger at
/// 22:30, and a name of its own, which a change never keeps.
fn later() -> String {
    good()
        .replace(
            "during = { from = \"22:00\", to = \"22:30\" }",
            "during = { from = \"22:30\", to = \"23:00\" }",
        )
        .replace("name: evening-dim", "name: late-evening-dim")
        .replace("at 10 pm", "at 10:30 pm")
}

/// A folder of the test's own, with a stand-in for `lamp-skills pack`
/// whose package holds the `skill.toml` it packs, so a changed skill is
/// another package.
fn keeping(name: &str) -> (PathBuf, Keeping) {
    let root = std::env::temp_dir().join(format!("made-change-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let program = root.join("pack");
    fs::write(
        &program,
        "#!/bin/sh\nname=$(basename \"$4\")\n{ printf 'tar of %s\\n' \"$name\"; cat \"$4/skill.toml\"; } > \"$3/$name-1.0.0.tar\"\n",
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let keeping = Keeping {
        directory: root.join("made"),
        program,
        manage: root.join("manage.sock"),
        installed: root.join("skills"),
    };
    (root, keeping)
}

/// A manage helper at `socket` that answers done to each of `lines` asks:
/// the lines it was asked, in order.
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

/// The quoted line a `next` tells the model to say.
fn quoted(next: &str) -> &str {
    next.rsplit_once(": \"")
        .map(|(_, line)| line.trim_end_matches('"'))
        .unwrap_or_default()
}

/// evening-dim made and on, as the person's own: its package's SHA-256.
fn made_first(keeping: &Keeping) -> String {
    let (url, served) = service(vec![(200, good())]);
    let asked = helper(&keeping.manage, 1);
    let told = make_with(3, Some(DIM), DIM, keeping, chat_at(url), NOW, || None);
    assert_eq!(served.join().unwrap(), 1);
    assert_eq!(told["on"], true, "{told}");
    asked.join().unwrap();
    let _ = take_said_line();
    let sha256 = made_skill::entries(&keeping.directory)[0]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    went_on_as(keeping, "evening-dim", &sha256);
    sha256
}

/// The change `LATER` of evening-dim, with `off` the skills turned off:
/// what the model is told, what the helper was asked, and the chat's body.
fn change_later(keeping: &Keeping, off: &BTreeSet<String>) -> (Value, Vec<Value>, String) {
    let (url, sent) = service_sent(vec![(200, later())]);
    let asked = helper(&keeping.manage, 1);
    let told = changed(
        keeping,
        "evening dim",
        "at half past ten",
        || true,
        Some(off),
        |words| {
            make_with(
                4,
                Some(LATER),
                words,
                keeping,
                chat_at(url),
                NOW + 1,
                || None,
            )
        },
        crate::app_make::now_unix_ms(),
    );
    let sent = sent.join().unwrap();
    assert_eq!(sent.len(), 1, "{told}");
    (told, asked.join().unwrap(), sent[0].clone())
}

fn switches(on: bool) {
    let on = on.then_some(true);
    crate::make_no_asks::set_here(on);
    set_owner_here(on);
}

/// #8008's row on lamp-9adf, 20 Nov 01:30: "change rain check to seven
/// thirty" made a second skill. A change naming the person's made skill
/// re-makes that one, under its own name, in place: one skill, the new
/// content, its old version kept under `old`; never `-2`, never the
/// model's own name, never a remove.
#[test]
fn a_change_remakes_the_named_skill_in_place_never_a_second() {
    switches(true);
    let (root, keeping) = keeping("in-place");
    let first = made_first(&keeping);
    let (told, asked, sent) = change_later(&keeping, &BTreeSet::new());
    assert_eq!(told["on"], true, "{told}");
    assert_eq!(told["changed"], "evening-dim", "{told}");
    let line = "Changed evening dim. Say 'put it back' to undo.";
    assert_eq!(quoted(told["next"].as_str().unwrap()), line, "{told}");
    assert!(
        told["next"]
            .as_str()
            .unwrap()
            .starts_with(made_skill::ALREADY_DONE)
    );
    assert_eq!(take_said_line().as_deref(), Some(line));
    // The make was asked for the old skill with the change, the change
    // winning.
    assert!(sent.contains("it dims the light a step"), "{sent}");
    assert!(
        sent.contains("Change to make to it now: at half past ten."),
        "{sent}"
    );
    assert!(sent.contains("the change wins"), "{sent}");
    // One skill, the same name, the new content.
    let entries = made_skill::entries(&keeping.directory);
    assert_eq!(entries.len(), 1, "{entries:?}");
    let entry = &entries[0];
    assert_eq!(entry["name"], "evening-dim");
    assert_eq!(entry["on"], true);
    assert_ne!(entry["sha256"], first.as_str());
    assert_eq!(entry["changed_from"], first.as_str());
    assert_eq!(entry["old"]["sha256"], first.as_str());
    assert!(entry.get("changes").is_none(), "{entry}");
    let package =
        fs::read_to_string(keeping.directory.join(entry["package"].as_str().unwrap())).unwrap();
    assert!(package.contains("from = \"22:30\""), "{package}");
    // Put on by its SHA-256 in the old one's place: nothing removed.
    assert_eq!(asked, vec![json!({ "install_made": entry["sha256"] })]);
    // Its make's own "remove it" is not its undo.
    assert!(
        crate::make_no_asks::latest_at(&keeping.directory, crate::app_make::now_unix_ms())
            .is_none_or(|at| at <= entry["old"]["replaced_at_ms"].as_u64().unwrap())
    );
    switches(false);
    let _ = fs::remove_dir_all(&root);
}

/// The first builder's ruling (01:38): the on/off state is kept, and "put
/// it back" restores the old version, in place, for a week.
#[test]
fn a_change_keeps_off_and_put_it_back_restores_the_old_version() {
    switches(true);
    let (root, keeping) = keeping("put-back");
    let first = made_first(&keeping);
    let off = BTreeSet::from(["evening-dim".to_owned()]);
    let (told, asked, _) = change_later(&keeping, &off);
    assert_eq!(
        quoted(told["next"].as_str().unwrap()),
        "Changed evening dim. It stays off. Say 'put it back' to undo.",
        "{told}"
    );
    // Neither turned on nor off: the install keeps lamp-skills' `off`.
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(asked[0]["install_made"].is_string(), "{asked:?}");
    let entry = made_skill::entries(&keeping.directory)[0].clone();
    assert_eq!(entry["old"]["was_off"], true, "{entry}");
    let new_package = keeping.directory.join(entry["package"].as_str().unwrap());
    let old_package = keeping
        .directory
        .join(entry["old"]["package"].as_str().unwrap());
    assert!(new_package.is_file() && old_package.is_file());
    let _ = take_said_line();
    // "Put it back": the change is the latest thing done.
    let now = crate::app_make::now_unix_ms();
    assert_eq!(
        put_back_for_undo(&keeping.directory, "last", now).as_deref(),
        Some("evening-dim")
    );
    assert_eq!(
        put_back_for_undo(&keeping.directory, "evening dim", now).as_deref(),
        Some("evening-dim")
    );
    let putting = helper(&keeping.manage, 1);
    let back = put_back(&keeping, "evening-dim", || true, now);
    assert_eq!(back["on"], true, "{back}");
    assert_eq!(
        quoted(back["next"].as_str().unwrap()),
        "Your old evening dim is back."
    );
    assert_eq!(
        putting.join().unwrap(),
        vec![json!({ "install_made": first })]
    );
    let entries = made_skill::entries(&keeping.directory);
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["sha256"], first.as_str());
    assert_eq!(entries[0]["on"], true);
    assert!(entries[0].get("old").is_none(), "{entries:?}");
    assert!(old_package.is_file() && !new_package.is_file());
    // Nothing more to put back.
    assert!(put_back_for_undo(&keeping.directory, "last", now).is_none());
    switches(false);
    let _ = fs::remove_dir_all(&root);
}

/// The old version is kept a week, then deleted, and put back no more.
#[test]
fn a_changed_skills_old_version_goes_after_a_week() {
    switches(true);
    let (root, keeping) = keeping("week");
    made_first(&keeping);
    change_later(&keeping, &BTreeSet::new());
    let _ = take_said_line();
    let entry = made_skill::entries(&keeping.directory)[0].clone();
    let old_package = keeping
        .directory
        .join(entry["old"]["package"].as_str().unwrap());
    let week = entry["old"]["replaced_at_ms"].as_u64().unwrap() + UNDO_FOR.as_millis() as u64;
    assert!(put_back_for_undo(&keeping.directory, "last", week - 1).is_some());
    assert!(put_back_for_undo(&keeping.directory, "last", week).is_none());
    drop_old(&keeping.directory, week - 1);
    assert!(old_package.is_file());
    drop_old(&keeping.directory, week);
    assert!(!old_package.is_file());
    assert!(
        made_skill::entries(&keeping.directory)[0]
            .get("old")
            .is_none()
    );
    switches(false);
    let _ = fs::remove_dir_all(&root);
}

/// A change never removes, and never makes when it can't be told which
/// skill: a name none of theirs has (the row's "rain check", kept as
/// another name) asks which, naming theirs; anyone else's ask, or one
/// they didn't make, changes nothing. Nothing is made in any.
#[test]
fn a_change_never_removes_and_asks_which_when_unsure() {
    switches(true);
    let (root, keeping) = keeping("unsure");
    let first = made_first(&keeping);
    let never = |_: &str| -> Value { panic!("made") };
    let off = BTreeSet::new();
    let told = changed(
        &keeping,
        "rain check",
        "at seven thirty",
        || true,
        Some(&off),
        never,
        NOW,
    );
    let next = told["next"].as_str().unwrap();
    assert!(
        next.contains("which skill they mean: evening dim"),
        "{next}"
    );
    assert!(next.contains("one short question"), "{next}");
    let told = changed(
        &keeping,
        "evening dim",
        "at 10:30",
        || false,
        Some(&off),
        never,
        NOW,
    );
    assert_eq!(told["error"], NOT_OWNER, "{told}");
    fs::create_dir_all(keeping.installed.join("guard-mode")).unwrap();
    let told = changed(
        &keeping,
        "guard mode",
        "at 10:30",
        || true,
        Some(&off),
        never,
        NOW,
    );
    assert!(
        told["next"]
            .as_str()
            .unwrap()
            .contains("isn't a skill they made"),
        "{told}"
    );
    // evening-dim as it was, never removed.
    let entries = made_skill::entries(&keeping.directory);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["sha256"], first.as_str());
    assert!(keeping.installed.join("evening-dim").is_dir());
    switches(false);
    let _ = fs::remove_dir_all(&root);
}

/// A new make with no change is as before: kept beside the first under
/// its own name, the first left on as it was.
#[test]
fn a_new_make_with_no_change_is_unchanged() {
    switches(true);
    let (root, keeping) = keeping("no-change");
    let first = made_first(&keeping);
    let (url, served) = service(vec![(200, later())]);
    let asked = helper(&keeping.manage, 1);
    let told = make_with(
        5,
        Some(HALF_PAST),
        HALF_PAST,
        &keeping,
        chat_at(url),
        NOW + 2,
        || None,
    );
    assert_eq!(served.join().unwrap(), 1);
    asked.join().unwrap();
    let _ = take_said_line();
    assert_eq!(told["made"], "late-evening-dim", "{told}");
    assert!(told.get("changed").is_none(), "{told}");
    let entries = made_skill::entries(&keeping.directory);
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert!(entries.iter().all(|entry| entry.get("old").is_none()));
    assert!(
        entries
            .iter()
            .any(|entry| entry["sha256"] == first.as_str() && entry["on"] == true)
    );
    switches(false);
    let _ = fs::remove_dir_all(&root);
}

/// The tool: `change` names the skill, with its request, never a new
/// skill or a remove, and the model asks which when unsure; the remove is
/// never for a change. Within the link's bounds with every switch on.
#[test]
fn the_tool_takes_a_change_within_the_bound() {
    crate::made_skill::set_off_said_here(Some(true));
    crate::make_no_asks::set_here(Some(true));
    crate::made_skill::set_writes_here(Some(true));
    crate::made_skill::set_scripts_here(Some(true));
    crate::skill_task_yes::set_on_here(Some(true));
    let tool = crate::make_skill_local::tool();
    let parameters = tool.parameters.unwrap();
    let change = parameters["properties"][CHANGE]["description"]
        .as_str()
        .unwrap();
    assert!(
        change.starts_with("To change a skill they made"),
        "{change}"
    );
    assert!(change.contains("with request as the change"), "{change}");
    assert!(change.contains("Never a new one or a remove."), "{change}");
    assert!(change.contains("Unsure which: ask."), "{change}");
    let remove = parameters["properties"][crate::made_remove::REMOVE]["description"]
        .as_str()
        .unwrap();
    assert!(remove.contains("stop or change it"), "{remove}");
    assert!(!remove.contains("no change yet"), "{remove}");
    let bytes = parameters.to_string().len();
    eprintln!(
        "make_skill: parameters {bytes} bytes, description {} bytes",
        tool.description.len()
    );
    assert!(bytes <= 4_096, "{bytes}");
    assert!(
        tool.description.len() <= 1_024,
        "{}",
        tool.description.len()
    );
    crate::skill_task_yes::set_on_here(None);
    crate::made_skill::set_scripts_here(None);
    crate::made_skill::set_writes_here(None);
    crate::make_no_asks::set_here(None);
    crate::made_skill::set_off_said_here(None);
}
