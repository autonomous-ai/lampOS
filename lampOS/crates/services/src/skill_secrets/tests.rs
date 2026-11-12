use super::*;
use std::os::unix::fs::MetadataExt;

const VALUE: &str = "test-value-123";

/// A store and a skills folder with `photo-booth` needing two names.
fn place() -> (tempfile::TempDir, Secrets) {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    let folder = skills.join("photo-booth");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("skill.toml"),
        "kind = \"installable\"\nsecrets = [\"GEMINI_API_KEY\", \"TELEGRAM_BOT_TOKEN\"]\n",
    )
    .unwrap();
    let secrets = Secrets::new(&dir.path().join("store"), &skills);
    (dir, secrets)
}

fn ask(line: Json) -> Ask {
    let object = line.as_object().unwrap();
    Ask::parse(object["op"].as_str().unwrap(), object).unwrap()
}

fn set(skill: &str, name: &str, value: &str) -> Ask {
    ask(json!({"op": SET_OP, "skill": skill, "name": name, "value": value}))
}

#[test]
fn names_are_read_from_the_ways_a_file_names_a_variable() {
    let text = "---\nname: photo-booth\n---\n\
                Set GEMINI_API_KEY in the app.\n\
                key = os.environ[\"TELEGRAM_BOT_TOKEN\"]\n\
                chat = os.getenv('TELEGRAM_CHAT_ID')\n\
                curl -H \"Authorization: Bearer $OPENAI_API_KEY\" https://x\n\
                const k = process.env.STRIPE_SECRET;\n\
                export WEATHER_CITY=Hanoi\n\
                The environment variable SLACK_WEBHOOK is read too.\n\
                Writes to $OUTPUT_DIR and reads $HOME and $PATH.\n\
                Say HELLO WORLD in CAPS, with an API call.\n\
                LAMP_CONTROL_TOKEN and PYTHONPATH are the lamp's own.\n";
    assert_eq!(
        names_in(text),
        [
            "GEMINI_API_KEY",
            "TELEGRAM_BOT_TOKEN",
            "TELEGRAM_CHAT_ID",
            "OPENAI_API_KEY",
            "STRIPE_SECRET",
            "WEATHER_CITY",
            "SLACK_WEBHOOK",
        ]
    );
    // A name in plain words with no variable's form and no secret's part
    // is no name; nor a value.
    assert!(names_in("Take a PHOTO and SAY cheese.").is_empty());
    assert!(names_in("Ask for the API key and the TOKEN, set the PAT.").is_empty());
    assert!(names_in("GEMINI_API_KEY=test-value-123").contains(&"GEMINI_API_KEY".to_owned()));
}

#[test]
fn a_name_is_plain_and_never_the_runs_own() {
    for good in ["GEMINI_API_KEY", "ABC", "A1_B2"] {
        assert!(name_ok(good), "{good}");
    }
    for bad in [
        "AB",
        "gemini_api_key",
        "1ABC",
        "_ABC",
        "A-B-C",
        "PATH",
        "HOME",
        "LD_PRELOAD",
        "LAMP_CONTROL_TOKEN",
        "HERMES_HOME",
        "PYTHONPATH",
        "HTTPS_PROXY",
        "TZ",
        "BASH_ENV",
        &"A".repeat(65),
    ] {
        assert!(!name_ok(bad), "{bad}");
    }
    assert!(value_ok(VALUE) && value_ok(&"x".repeat(4096)));
    for bad in ["", "a\nb", "a\rb", "a\0b", "a\u{1b}b", &"x".repeat(4097)] {
        assert!(!value_ok(bad), "{bad:?}");
    }
}

#[test]
fn needed_is_the_skill_tomls_secrets() {
    let (_dir, secrets) = place();
    assert_eq!(
        needed(&secrets.skills, "photo-booth").unwrap(),
        ["GEMINI_API_KEY", "TELEGRAM_BOT_TOKEN"]
    );
    assert_eq!(needed(&secrets.skills, "nothing-here"), Err(NO_SKILL));
    assert_eq!(needed(&secrets.skills, "../etc"), Err(BAD_SKILL));
    assert_eq!(
        needed_of_toml("secrets = [\"PATH\", \"GOOD_KEY\", \"GOOD_KEY\", 3]"),
        ["GOOD_KEY"]
    );
}

#[test]
fn a_value_is_kept_root_only_and_only_its_name_said() {
    let (_dir, secrets) = place();
    let (told, said) = secrets.answer(&set("photo-booth", "GEMINI_API_KEY", VALUE));
    assert_eq!(
        told.unwrap(),
        json!({"skill": "photo-booth", "needed": ["GEMINI_API_KEY", "TELEGRAM_BOT_TOKEN"], "set": ["GEMINI_API_KEY"]})
    );
    // The journal's line: the name and the length, never the value.
    assert_eq!(said, "skill photo-booth: GEMINI_API_KEY kept (14 bytes)");
    let path = secrets.dir.join("photo-booth.json");
    let file = fs::metadata(&path).unwrap();
    assert_eq!(file.mode() & 0o777, 0o600);
    assert_eq!(fs::metadata(&secrets.dir).unwrap().mode() & 0o777, 0o700);
    assert_eq!(file.uid(), rustix::process::geteuid().as_raw());
    assert!(fs::read_to_string(&path).unwrap().contains(VALUE));
    // Nothing is left beside it.
    assert_eq!(fs::read_dir(&secrets.dir).unwrap().count(), 1);
    // The list says names only.
    let (listed, said) = secrets.answer(&ask(json!({"op": LIST_OP, "skill": "photo-booth"})));
    assert!(!listed.unwrap().to_string().contains(VALUE));
    assert!(!said.contains(VALUE));
    // The `Debug` of an ask hides it.
    assert!(!format!("{:?}", set("photo-booth", "GEMINI_API_KEY", VALUE)).contains(VALUE));
}

#[test]
fn a_name_not_needed_or_a_bad_value_is_refused() {
    let (_dir, secrets) = place();
    let (told, _) = secrets.answer(&set("photo-booth", "OTHER_API_KEY", VALUE));
    assert_eq!(told, Err(NOT_NEEDED));
    let (told, _) = secrets.answer(&set("no-such", "GEMINI_API_KEY", VALUE));
    assert_eq!(told, Err(NO_SKILL));
    let parse = |line: Json| {
        let object = line.as_object().unwrap().clone();
        Ask::parse(object["op"].as_str().unwrap(), &object)
    };
    assert_eq!(
        parse(
            json!({"op": SET_OP, "skill": "photo-booth", "name": "GEMINI_API_KEY", "value": "a\nb"})
        ),
        Err(BAD_VALUE)
    );
    assert_eq!(
        parse(json!({"op": SET_OP, "skill": "photo-booth", "name": "GEMINI_API_KEY", "value": ""})),
        Err(BAD_VALUE)
    );
    assert_eq!(
        parse(json!({"op": SET_OP, "skill": "photo-booth", "name": "PATH", "value": VALUE})),
        Err(NOT_NEEDED)
    );
    assert_eq!(
        parse(json!({"op": LIST_OP, "skill": "Photo"})),
        Err(BAD_SKILL)
    );
    assert_eq!(
        parse(json!({"op": LIST_OP, "skill": "photo-booth", "value": VALUE})),
        Err(BAD_FIELDS)
    );
    assert!(!secrets.dir.join("photo-booth.json").exists());
}

#[test]
fn unset_drop_and_a_skill_gone_take_the_values() {
    let (_dir, secrets) = place();
    secrets
        .answer(&set("photo-booth", "GEMINI_API_KEY", VALUE))
        .0
        .unwrap();
    secrets
        .answer(&set("photo-booth", "TELEGRAM_BOT_TOKEN", VALUE))
        .0
        .unwrap();
    let (told, _) = secrets.answer(&ask(
        json!({"op": UNSET_OP, "skill": "photo-booth", "name": "GEMINI_API_KEY"}),
    ));
    assert_eq!(told.unwrap()["set"], json!(["TELEGRAM_BOT_TOKEN"]));
    // Unset again: nothing changes, and it is no failure.
    let (told, _) = secrets.answer(&ask(
        json!({"op": UNSET_OP, "skill": "photo-booth", "name": "GEMINI_API_KEY"}),
    ));
    assert_eq!(told.unwrap()["set"], json!(["TELEGRAM_BOT_TOKEN"]));
    // Removed from the app: its file goes.
    let (told, _) = secrets.answer(&ask(json!({"op": DROP_OP, "skill": "photo-booth"})));
    assert!(told.is_ok());
    assert!(!secrets.dir.join("photo-booth.json").exists());
    // Removed some other way: the next ask drops its file.
    secrets
        .answer(&set("photo-booth", "GEMINI_API_KEY", VALUE))
        .0
        .unwrap();
    fs::remove_dir_all(secrets.skills.join("photo-booth")).unwrap();
    secrets.sweep();
    assert!(!secrets.dir.join("photo-booth.json").exists());
}

#[test]
fn the_same_name_on_again_as_another_package_keeps_none() {
    let (_dir, secrets) = place();
    let record = |sha: &str| {
        fs::write(
            secrets.skills.join("photo-booth").join(".package.json"),
            format!("{{\"sha256\":\"{}\"}}", sha.repeat(64)),
        )
        .unwrap();
    };
    record("a");
    secrets
        .answer(&set("photo-booth", "GEMINI_API_KEY", VALUE))
        .0
        .unwrap();
    let (told, _) = secrets.run("{\"op\":\"run_secrets\",\"skill\":\"photo-booth\"}");
    assert_eq!(told["values"]["GEMINI_API_KEY"], VALUE);
    record("b");
    let (told, _) = secrets.run("{\"op\":\"run_secrets\",\"skill\":\"photo-booth\"}");
    assert_eq!(told["values"], json!({}));
    assert!(!secrets.dir.join("photo-booth.json").exists());
}

#[test]
fn the_run_is_given_the_values_and_told_what_is_missing() {
    let (_dir, secrets) = place();
    let line = "{\"op\":\"run_secrets\",\"skill\":\"photo-booth\"}\n";
    let (told, said) = secrets.run(line);
    assert_eq!(
        told,
        json!({"ok": true, "values": {}, "missing": ["GEMINI_API_KEY", "TELEGRAM_BOT_TOKEN"]})
    );
    assert_eq!(said, "a skill's run: photo-booth given 0 key(s), 2 missing");
    secrets
        .answer(&set("photo-booth", "GEMINI_API_KEY", VALUE))
        .0
        .unwrap();
    let (told, said) = secrets.run(line);
    assert_eq!(told["values"], json!({"GEMINI_API_KEY": VALUE}));
    assert_eq!(told["missing"], json!(["TELEGRAM_BOT_TOKEN"]));
    assert!(!said.contains(VALUE));
    // A skill that needs none, and a line that is not the run's.
    fs::create_dir_all(secrets.skills.join("plain")).unwrap();
    assert_eq!(
        secrets
            .run("{\"op\":\"run_secrets\",\"skill\":\"plain\"}")
            .0,
        json!({"ok": true, "values": {}, "missing": []})
    );
    assert_eq!(
        secrets
            .run("{\"op\":\"run_secrets\",\"skill\":\"photo-booth\",\"x\":1}")
            .0["error"],
        BAD_FIELDS
    );
    assert_eq!(
        secrets.run("{\"op\":\"run_secrets\",\"skill\":\"..\"}").0["error"],
        BAD_SKILL
    );
}
