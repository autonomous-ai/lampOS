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
    change_to(keeping, later(), 1, off, (LATER, "at half past ten"))
}

/// [`change_later`] with the making model's `answer`, made `at` ms after
/// the first make, from the line `heard` asking the change `words`.
fn change_to(
    keeping: &Keeping,
    answer: String,
    at: u64,
    off: &BTreeSet<String>,
    (heard, words): (&str, &str),
) -> (Value, Vec<Value>, String) {
    let (url, sent) = service_sent(vec![(200, answer)]);
    let asked = helper(&keeping.manage, 1);
    let told = changed(
        keeping,
        "evening dim",
        words,
        || true,
        Some(off),
        |words| {
            make_with(
                4,
                Some(heard),
                words,
                keeping,
                chat_at(url),
                NOW + at,
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
    let back = put_back(&keeping, "evening-dim", || true, Some(&off), now);
    // Twelve's P3 on #8008: off, it is told off.
    assert_eq!(back["on"], false, "{back}");
    assert_eq!(
        quoted(back["next"].as_str().unwrap()),
        "Your old evening dim is back. It stays off."
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

/// The tool: `change_skill` names the skill, with its request, never a
/// new skill or a remove, and the model asks which when unsure: its own
/// function, within the link's bounds by itself, so `make_skill`'s
/// parameters gain nothing (#8002's `PRODUCT_MOST_PARAMETERS_BYTES`).
/// Run as `make_skill`'s call with `change`, and given back as made.
#[test]
fn the_tool_takes_a_change_within_the_bound() {
    crate::made_skill::set_off_said_here(Some(true));
    crate::make_no_asks::set_here(Some(true));
    crate::made_skill::set_writes_here(Some(true));
    crate::made_skill::set_scripts_here(Some(true));
    crate::skill_task_yes::set_on_here(Some(true));
    let tool = tool();
    assert_eq!(tool.name, FUNCTION);
    for part in [
        "Change a skill the person made",
        "changed in place, never made as a new skill and never removed",
        "say exactly the line you are told",
    ] {
        assert!(
            tool.description.contains(part),
            "{part}: {}",
            tool.description
        );
    }
    let parameters = tool.parameters.clone().unwrap();
    let name = parameters["properties"][NAME]["description"]
        .as_str()
        .unwrap();
    assert!(name.contains("Unsure which: ask."), "{name}");
    assert_eq!(parameters["required"], json!([NAME, REQUEST]));
    let bytes = parameters.to_string().len();
    eprintln!(
        "change_skill: parameters {bytes} bytes, description {} bytes",
        tool.description.len()
    );
    assert!(bytes <= 4_096, "{bytes}");
    assert!(tool.description.len() <= 1_024);
    assert!(tool.validate().is_ok());
    let make = crate::make_skill_local::tool().parameters.unwrap();
    assert!(make["properties"][CHANGE].is_null(), "{make}");
    let remove = make["properties"][crate::made_remove::REMOVE]["description"]
        .as_str()
        .unwrap();
    assert!(!remove.contains("no change yet"), "{remove}");
    let asked = json!({NAME: "rain check", REQUEST: "at seven thirty"});
    let made = as_make(FUNCTION, &asked).unwrap();
    assert_eq!(
        made,
        json!({CHANGE: "rain check", "request": "at seven thirty"})
    );
    assert_eq!(
        as_asked(crate::made_skill::MAKE, &made),
        Some((FUNCTION.to_owned(), asked))
    );
    assert_eq!(as_make(crate::made_skill::MAKE, &made), None);
    assert_eq!(
        as_asked(crate::made_skill::MAKE, &json!({"request": "a new one"})),
        None
    );
    assert_eq!(as_make_argument(NAME), CHANGE);
    assert_eq!(as_make_argument("request"), "request");
    crate::skill_task_yes::set_on_here(None);
    crate::made_skill::set_scripts_here(None);
    crate::made_skill::set_writes_here(None);
    crate::make_no_asks::set_here(None);
    crate::made_skill::set_off_said_here(None);
}

/// Twelve's P2 1 on #8008: a guest's change hears none of the owner's
/// skills named, whether two are as near or none has the name: the
/// owner's ask is read first.
#[test]
fn a_guests_change_names_no_skill() {
    crate::made_skill::set_off_said_here(Some(true));
    let (root, keeping) = keeping("guest");
    crate::made_remove::tests::made_here(
        &keeping,
        &[("good-morning-1", true), ("good-morning-2", true)],
    );
    let never = |_: &str| -> Value { panic!("made") };
    let off = BTreeSet::new();
    for asked in ["good morning", "rain check"] {
        let told = changed(&keeping, asked, "at 7:30", || false, Some(&off), never, NOW);
        assert_eq!(told["error"], NOT_OWNER, "{asked}: {told}");
        let next = told["next"].as_str().unwrap();
        assert!(!next.contains("good morning"), "{asked}: {next}");
    }
    // The owner is asked which, by name.
    let told = changed(
        &keeping,
        "good morning",
        "at 7:30",
        || true,
        Some(&off),
        never,
        NOW,
    );
    let next = told["next"].as_str().unwrap();
    assert!(next.contains("good morning 1 or good morning 2"), "{next}");
    crate::made_skill::set_off_said_here(None);
    let _ = fs::remove_dir_all(&root);
}

/// Twelve's P2 2 on #8008: with `LAMP_MAKE_NO_ASKS` on, a change never
/// turns off another skill, even a near twin doing the same thing (the
/// row's `-2` beside the one changed): only the changed one is put on.
#[test]
fn a_change_never_turns_off_a_twin() {
    switches(true);
    let (root, keeping) = keeping("twin");
    crate::make_no_asks::set_state_here(Some(root.join("state")));
    made_first(&keeping);
    // evening-dim-2 on the lamp, doing what the change will do.
    let toml = later();
    let toml = toml
        .split_once("```skill.toml\n")
        .and_then(|(_, rest)| rest.split_once("```"))
        .unwrap()
        .0
        .to_owned();
    let twin = keeping.installed.join("evening-dim-2");
    fs::create_dir_all(&twin).unwrap();
    fs::write(twin.join("skill.toml"), toml).unwrap();
    let (told, asked, _) = change_later(&keeping, &BTreeSet::new());
    let _ = take_said_line();
    assert_eq!(told["changed"], "evening-dim", "{told}");
    assert!(told.get("replaced").is_none(), "{told}");
    assert!(told.get("not_off").is_none(), "{told}");
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(asked[0]["install_made"].is_string(), "{asked:?}");
    assert!(twin.is_dir());
    crate::make_no_asks::set_state_here(None);
    switches(false);
    let _ = fs::remove_dir_all(&root);
}

/// Twelve's P2 3 on #8008: a second change within the week keeps only the
/// latest old version; the first one's package goes, so "forget
/// everything" leaves no package but the skill's own.
#[test]
fn two_changes_then_forget_everything_leave_no_old_package() {
    switches(true);
    let (root, keeping) = keeping("two-changes");
    made_first(&keeping);
    change_later(&keeping, &BTreeSet::new());
    let latest = later().replace(
        "during = { from = \"22:30\", to = \"23:00\" }",
        "during = { from = \"23:00\", to = \"23:30\" }",
    );
    let (told, _, _) = change_to(
        &keeping,
        latest,
        2,
        &BTreeSet::new(),
        ("change evening dim to eleven", "at eleven"),
    );
    let _ = take_said_line();
    assert_eq!(told["changed"], "evening-dim", "{told}");
    let packages = || -> Vec<String> {
        let mut found: Vec<String> = fs::read_dir(&keeping.directory)
            .unwrap()
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.ends_with(".tar"))
            .collect();
        found.sort();
        found
    };
    let named = || -> Vec<String> {
        let mut named: Vec<String> = made_skill::entries(&keeping.directory)
            .iter()
            .flat_map(|entry| {
                made_skill::packages_named(entry)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect();
        named.sort();
        named.dedup();
        named
    };
    // The latest old version and the skill's own, nothing else.
    assert_eq!(packages(), named());
    assert_eq!(packages().len(), 2, "{:?}", packages());
    assert!(crate::make_no_asks::clear(&keeping.directory).unwrap());
    let own = made_skill::entries(&keeping.directory)[0]["package"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(packages(), [own]);
    switches(false);
    let _ = fs::remove_dir_all(&root);
}

/// Twelve's P3 on #8008: with `LAMP_MAKE_NO_ASKS` off, a change that
/// clashes keeps its clash's question and the made one waiting for it.
#[test]
fn a_change_that_clashes_keeps_its_question() {
    let (root, keeping) = keeping("clash");
    made_first(&keeping);
    let twin = keeping.installed.join("rest-glow");
    fs::create_dir_all(&twin).unwrap();
    fs::write(
        twin.join("skill.toml"),
        "kind = \"installable\"\nversion = \"1.0.0\"\n\n[trigger]\ntopic = \"voice/state\"\n\
         field = \"state\"\nequals = \"resting\"\ncooldown = \"1h\"\n\n[[do]]\nlight = { colour \
         = \"blue\" }\n",
    )
    .unwrap();
    let (url, served) = service(vec![(200, later())]);
    let off = BTreeSet::new();
    let told = changed(
        &keeping,
        "evening dim",
        "at half past ten",
        || true,
        Some(&off),
        |words| {
            make_with(
                4,
                Some(LATER),
                words,
                &keeping,
                chat_at(url),
                NOW + 1,
                || None,
            )
        },
        crate::app_make::now_unix_ms(),
    );
    assert_eq!(served.join().unwrap(), 1);
    let _ = take_said_line();
    assert!(told[crate::made_clash::CLASH_ID].is_string(), "{told}");
    assert!(
        made_skill::entries(&keeping.directory)
            .iter()
            .any(|entry| entry["on"] != true && entry["name"] == "evening-dim"),
        "the made one waits for the choice"
    );
    let _ = fs::remove_dir_all(&root);
}
