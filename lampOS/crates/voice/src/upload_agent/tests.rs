use super::*;
use crate::make_skill_local::tests::{keeping, manage_helper};
use crate::upload_skill::{Place, take};
use serde_json::json;

/// A photobooth-like file as the owner's is: three photos with countdowns,
/// lines said and the light flashed, a strip made by an image API, posted
/// to a chat by a bot API; the key and the token asked of the person in
/// chat. About 4 KB, as the owner's is; no real chat or key in it.
fn photobooth() -> String {
    let mut card = String::from(
        "---\nname: photo-booth\ndescription: Takes three photos with a countdown and posts \
         them as a strip to my chat.\n---\n\n# Photo booth\n\n## Trigger phrases\n\n\
         - \"take a photo\"\n- \"photo booth\"\n- \"strike a pose\"\n\n## Steps\n\n",
    );
    for shot in 1..=3 {
        card.push_str(&format!(
            "{shot}. Say \"Photo {shot} of 3, get ready!\" and count down from three, one \
             number a second. Flash the light white on each number, then take a photo with the \
             camera and keep it in your own folder.\n"
        ));
    }
    card.push_str(
        "\n## The strip\n\nSend the three photos to the image API at \
         https://images.example.invalid/v1/strip with an HTTP POST, the API key in the \
         Authorization header. Save the strip it gives back.\n\n## Posting\n\nPost the strip \
         to the chat through the bot API, https://bot.example.invalid/sendPhoto, with the bot \
         token and the chat id.\n\n## Secrets\n\nAsk the person in the chat for the API key and \
         the bot token the first time; never write them down.\n",
    );
    while card.len() < 4_000 {
        card.push_str("\nKeep every line short and friendly, and say thank you at the end.");
    }
    card
}

fn place(root: &Path) -> Place {
    let place = Place {
        skills: root.join("skills"),
        pushed: root.join("pushed"),
    };
    std::fs::create_dir_all(&place.skills).unwrap();
    std::fs::create_dir_all(&place.pushed).unwrap();
    place
}

const NOW: u64 = 1_800_000_000_000;

fn switches(agent: bool, net: bool) {
    set_on_here(Some(agent));
    crate::skill_task_yes::set_net_here(Some(net));
}

/// The owner's 4 KB photobooth file becomes an agent skill: a voice tool
/// named for it, its note the file's description and trigger phrases, one
/// task that runs the file, kept whole as its instructions, and the grants
/// camera, light, speak and net read from it; said back with them, and its
/// first run asks the yes.
#[test]
fn the_photobooth_file_becomes_an_agent_skill_with_camera_light_speak_and_net() {
    switches(true, true);
    let card = photobooth();
    assert!((3_900..8_192).contains(&card.len()), "{}", card.len());
    let (files, grants) = files(&card).unwrap();
    assert_eq!(grants, ["light", "speak", "camera", "net"]);
    assert!(asks_yes(&grants));
    let table: toml::Table = toml::from_str(&files.toml).unwrap();
    assert_eq!(table["tool"].as_str(), Some("photo_booth"));
    assert_eq!(table["do"][0]["task"]["run"].as_str(), Some(INSTRUCTIONS));
    assert!(table.get("trigger").is_none() && table.get("permissions").is_none());
    assert_eq!(files.scripts, [(INSTRUCTIONS.to_owned(), card.clone())]);
    let described = made_skill::written_description(&files.card).unwrap();
    for phrase in [
        "take a photo",
        "photo booth",
        "strike a pose",
        "posts them as a strip",
    ] {
        assert!(described.contains(phrase), "{described}");
    }
    capped(&files.toml).unwrap();
    // Kept as an added skill, marked agent, its grants in its index entry.
    let (root, keeping) = keeping("agent-booth");
    let made = take(&place(&root), &keeping, None, &card, NOW).unwrap();
    assert_eq!(made.name, "photo-booth");
    assert_eq!(
        made_skill::agent_grants(&keeping.directory, &made.sha256).unwrap(),
        grants
    );
    let entry = &made_skill::entries(&keeping.directory)[0];
    assert_eq!(
        (entry["added"].clone(), entry["agent"].clone()),
        (json!("file"), json!(true))
    );
    let line = on_line(&made, &grants);
    assert_eq!(
        line,
        "I added photo booth from a file, not from the Skills Store, and it's on: only when you \
         ask for it, the lamp's agent follows the file, and it changes the light, speaks and \
         plays sounds, uses the camera and reaches the internet. The first time it runs, I'll \
         ask you to agree."
    );
    // Going on records no yes: its first run asks it.
    let said = went_on(&made, &grants, |_| {
        panic!("no yes is recorded for the camera or net")
    });
    assert_eq!(said, line);
    let _ = std::fs::remove_dir_all(&root);
    switches(false, false);
}

/// With the switch off, the same file is refused for its length as today
/// (a written skill's note is at most 900 bytes), and nothing is kept.
#[test]
fn with_the_switch_off_the_file_is_refused_for_length_as_today() {
    switches(false, true);
    let (root, keeping) = keeping("agent-off");
    let why = take(&place(&root), &keeping, None, &photobooth(), NOW)
        .unwrap_err()
        .0;
    assert!(why.contains("longer than 900 bytes"), "{why}");
    assert!(made_skill::entries(&keeping.directory).is_empty());
    let _ = std::fs::remove_dir_all(&root);
    // On, but with LAMP_TASK_NET off: a file that names a web API is
    // refused, never kept off the network it needs.
    switches(true, false);
    assert_eq!(files(&photobooth()).unwrap_err().0, NO_NET);
    // Past 64 KiB: refused even on.
    switches(true, true);
    let long = format!("{}{}", photobooth(), "x".repeat(MOST_BYTES));
    assert!(files(&long).unwrap_err().0.contains("longer than 64 KiB"));
    switches(false, false);
}

/// A short file with only the light and its voice goes on at once: its
/// task's yes is recorded as it goes on, and no yes is asked.
#[test]
fn a_short_file_with_only_light_and_speak_goes_on_at_once() {
    switches(true, false);
    let card = "---\nname: good-night\ndescription: Says good night and dims the light.\n---\n\n\
                Trigger phrases: \"good night lamp\".\n\nSay good night warmly, then glow the \
                light dim orange.\n";
    let (files, grants) = files(card).unwrap();
    assert_eq!(grants, ["light", "speak"]);
    assert!(!asks_yes(&grants));
    let (root, keeping) = keeping("agent-at-once");
    let made = take(&place(&root), &keeping, None, card, NOW).unwrap();
    let asked = manage_helper(&keeping.manage);
    made_skill::put_on(&keeping, &made).unwrap();
    assert_eq!(asked.join().unwrap()["install_made"], made.sha256.as_str());
    let mut recorded = Vec::new();
    let said = went_on(&made, &grants, |name| {
        recorded.push(name.to_owned());
        Ok(())
    });
    assert_eq!(recorded, ["good-night"]);
    assert!(!said.contains("agree"), "{said}");
    assert!(
        said.ends_with("it changes the light and speaks and plays sounds."),
        "{said}"
    );
    assert!(
        files.toml.contains("grants = [\"light\", \"speak\"]"),
        "{}",
        files.toml
    );
    let _ = std::fs::remove_dir_all(&root);
    switches(false, false);
}

/// Its first run asks the yes: the model's call of its tool is answered
/// with the say-back and the wait for the person's yes, and nothing runs;
/// once agreed, the call runs. Another skill's call is not touched.
#[test]
fn an_agent_skills_first_run_asks_the_yes_and_an_agreed_one_runs() {
    switches(true, true);
    crate::skill_task_yes::set_on_here(Some(true));
    let (root, keeping) = keeping("agent-first-run");
    let made = take(&place(&root), &keeping, None, &photobooth(), NOW).unwrap();
    let asked = manage_helper(&keeping.manage);
    made_skill::put_on(&keeping, &made).unwrap();
    asked.join().unwrap();
    let task = |approved: bool| crate::skill_task_yes::TaskAsks {
        name: "photo-booth".into(),
        sha256: "b".repeat(64),
        grants: vec![
            "light".into(),
            "speak".into(),
            "camera".into(),
            "net".into(),
        ],
        task: "Takes three photos with a countdown and posts them as a strip to my chat.".into(),
        package: None,
        from: FROM.into(),
        approved,
    };
    let asking = crate::make_skill_local::Asking {
        request: 7,
        heard: true,
        owner_hears: true,
        sends_here: true,
    };
    let pushed = root.join("pushed");
    let now = std::time::Instant::now();
    let told = first_run_with(
        &keeping.directory,
        "photo_booth",
        asking,
        &pushed,
        |_| Ok(Some(task(false))),
        now,
    )
    .unwrap();
    assert_eq!(told["task_waits_for_yes"], true, "{told}");
    let next = told["next"].as_str().unwrap();
    assert!(
        next.contains("comes from a file added in the app"),
        "{next}"
    );
    assert!(next.contains("reaches the internet"), "{next}");
    assert!(next.contains("It runs only once you agree."), "{next}");
    assert!(
        first_run_with(
            &keeping.directory,
            "photo_booth",
            asking,
            &pushed,
            |_| Ok(Some(task(true))),
            now
        )
        .is_none()
    );
    assert!(
        first_run_with(
            &keeping.directory,
            "light_on",
            asking,
            &pushed,
            |_| Ok(Some(task(false))),
            now
        )
        .is_none()
    );
    // Off: as today, the call runs as any skill's.
    set_on_here(Some(false));
    assert!(
        first_run_with(
            &keeping.directory,
            "photo_booth",
            asking,
            &pushed,
            |_| Ok(Some(task(false))),
            now
        )
        .is_none()
    );
    crate::skill_task_yes::set_on_here(None);
    let _ = std::fs::remove_dir_all(&root);
    switches(false, false);
}

/// No secret is the lamp's: what the voice tells lamp-cloud and the
/// journal never has the file's words, and the lamp's own files (its
/// `skill.toml` and card) carry none of the file's steps, keys or links.
#[test]
fn no_secret_or_file_words_are_written_by_the_lamp() {
    switches(true, true);
    let (root, keeping) = keeping("agent-no-secret");
    let place = place(&root);
    let asked = json!({"upload": {"skill_md": photobooth()}});
    let (told, _) = crate::upload_skill::line(&asked, true, true, &place, &keeping, NOW).unwrap();
    let told = told.to_string();
    for words in [
        "example.invalid",
        "Authorization",
        "bot token",
        "Photo 1 of 3",
    ] {
        assert!(!told.contains(words), "{words}: {told}");
    }
    assert!(
        told.contains("\"grants\":[\"light\",\"speak\",\"camera\",\"net\"]"),
        "{told}"
    );
    let (files, _) = files(&photobooth()).unwrap();
    for words in ["example.invalid", "Authorization", "API key", "chat id"] {
        assert!(
            !files.toml.contains(words) && !files.card.contains(words),
            "{words}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
    switches(false, false);
}

/// The grants are read conservatively: only what the file plainly asks.
#[test]
fn grants_are_read_conservatively() {
    switches(true, true);
    assert!(grants_read("Tell me a joke.").is_empty());
    // Playing a sound is its voice; hearing a clap is `sound`, which keeps
    // its yes, as the camera and net do.
    assert_eq!(grants_read("Play a chime sound."), ["speak"]);
    assert_eq!(grants_read("When I clap, glow."), ["light", "sound"]);
    assert!(asks_yes(&grants_read("When I clap, glow.")));
    assert!(!asks_yes(&["light".into(), "speak".into(), "body".into()]));
    assert_eq!(
        grants_read("Highlight the lightweight words."),
        Vec::<String>::new()
    );
    assert_eq!(grants_read("Fetch the URL and say it."), ["speak", "net"]);
    // A bare "api" or "url" is not the internet: a reach-out word or a
    // real link is.
    assert_eq!(grants_read("Say the URL of this page aloud."), ["speak"]);
    assert_eq!(grants_read("Glow like the API docs' blue."), ["light"]);
    assert_eq!(
        grants_read("Call the weather API and say it."),
        ["speak", "net"]
    );
    assert_eq!(
        grants_read("Glow, then see https://example.invalid/a."),
        ["light", "net"]
    );
    switches(false, false);
}

/// A file carrying a value that looks like a key or token is refused in
/// plain words, the value never in the refusal (the first builder's 19:44
/// ruling: secrets go in through the app, a follow-up). A link, a chat's
/// number or a long word is not one.
#[test]
fn a_file_carrying_a_key_or_token_is_refused_without_saying_it() {
    switches(true, true);
    let fake = ["FAKE", "Key", "0000", "1111", "abcd", "2222", "efgh"].concat();
    let card = format!("{}\nUse the API key {fake} for the strip.\n", photobooth());
    let why = files(&card).unwrap_err().0;
    assert_eq!(why, HAS_SECRET);
    assert!(!why.contains(&fake));
    assert!(!carries_secret(
        "Post to https://bot.example.invalid/bot123456/sendPhoto for chat -1001234567890."
    ));
    assert!(!carries_secret(
        "an_image_strip_generator_v2 is the strip's name"
    ));
    switches(false, false);
}

/// Heron's P2 on #6666: a key or token in a link's path is found too,
/// each part past the scheme and host and a bot's `<id>:<token>` checked
/// with the same rule. Made-up probe values only.
#[test]
fn a_key_or_token_in_a_link_is_refused_too() {
    switches(true, true);
    for link in [
        "https://hooks.slack.com/services/T0A1B2C3D/B0A1B2C3D/abcDEF1234ghiJKL5678mnoP",
        "https://discord.com/api/webhooks/123456789012345678/AbCdEf1234567890GhIjKl1234567890",
        "https://api.telegram.org/bot123456789:AAH1bc2DEf3GHi4JKl5MNo6PQr7STu8VWx/sendMessage",
        "https://maker.ifttt.com/trigger/lamp/with/key/dAbC1234efGH5678ijKL9012",
    ] {
        assert!(
            carries_secret(&format!("Post the photo to {link} when done.")),
            "{link}"
        );
        let card = format!(
            "{}
Post each strip to {link}.
",
            photobooth()
        );
        let why = files(&card).unwrap_err().0;
        assert_eq!(why, HAS_SECRET, "{link}");
        assert!(!why.contains(link));
    }
    // A value outside a link is still found whole, and a link with no key
    // in it, a chat's number or a long word still passes.
    let fake = ["FAKE", "Key", "0000", "1111", "abcd", "2222", "efgh"].concat();
    assert!(carries_secret(&format!("The key is {fake}.")));
    assert!(carries_secret(&format!("Use bot123:{fake} to send.")));
    for clean in [
        "Post to https://bot.example.invalid/bot123456/sendPhoto for chat -1001234567890.",
        "See https://example.invalid/docs/skills/photo-booth/strip-layout for the strip.",
        "an_image_strip_generator_v2 is the strip's name",
        "Read /var/lib/lampOS/skills/photo-booth/scripts/SKILL.md first.",
    ] {
        assert!(!carries_secret(clean), "{clean}");
    }
    switches(false, false);
}

/// The owner's ruling (13 Nov): a file that uses a linked service goes to
/// the lamp's agent end to end, with that service's grants as a voice-made
/// service skill's `do.task` carries them: `-read` and `-write` for each
/// service it names. Said back plainly, and its first run keeps the spoken
/// yes.
#[test]
fn a_file_that_uses_a_linked_service_gets_its_grants_with_the_switch_on() {
    switches(true, false);
    crate::made_skill::set_agent_services_here(Some(true));
    let card = "---\nname: morning-mail\ndescription: Reads my new mail aloud in the morning.\n\
                ---\n\nTrigger phrases: \"read my mail\".\n\nRead my unread Gmail from today \
                and say how many there are and who they are from. Reply to the newest one \
                saying thanks, I'll read it later.\n";
    let (made_files, grants) = files(card).unwrap();
    assert_eq!(grants, ["speak", "gmail-read", "gmail-write"]);
    assert!(asks_yes(&grants));
    assert!(
        made_files
            .toml
            .contains("grants = [\"speak\", \"gmail-read\", \"gmail-write\"]"),
        "{}",
        made_files.toml
    );
    assert!(capped(&made_files.toml).is_ok());
    assert_eq!(
        capped_with(&made_files.toml, false).unwrap_err().0,
        NO_SERVICES
    );
    let (root, keeping) = keeping("agent-mail");
    let made = take(&place(&root), &keeping, None, card, NOW).unwrap();
    assert_eq!(
        made_skill::agent_grants(&keeping.directory, &made.sha256).unwrap(),
        grants
    );
    let said = on_line(&made, &grants);
    assert!(said.contains("reads your Gmail"), "{said}");
    assert!(said.contains("each sent only on your yes"), "{said}");
    assert!(said.contains("I'll ask you to agree"), "{said}");
    let _ = std::fs::remove_dir_all(&root);
    // Off: a file naming a service is refused in plain words, nothing kept.
    crate::made_skill::set_agent_services_here(Some(false));
    assert_eq!(files(card).unwrap_err().0, NO_SERVICES);
    assert_eq!(grants_read(card), ["speak"]);
    crate::made_skill::set_agent_services_here(None);
    switches(false, false);
}

/// The services are read by their names alone, in any case, from a table
/// of names: each one named gets its read and its write; what the file does
/// with them is the agent's to judge, after the owner's yes.
#[test]
fn services_are_read_by_their_names() {
    assert!(services_read("Tell me a joke and glow.").is_empty());
    assert!(services_read("Send the photo to the image API.").is_empty());
    assert_eq!(
        services_read("Say my next calendar event."),
        ["google_calendar-read", "google_calendar-write"]
    );
    assert_eq!(
        services_read("Count my inbox. Send the count to the slack channel."),
        ["gmail-read", "gmail-write", "slack-read", "slack-write"]
    );
    assert_eq!(
        services_read("Save the photo to Google Drive."),
        ["google_drive-read", "google_drive-write"]
    );
    assert_eq!(
        services_read("List my open GitHub issues and the NOTION page."),
        ["github-read", "github-write", "notion-read", "notion-write"]
    );
    assert_eq!(
        services_read("Check monday.com boards."),
        ["mondaydotcom-read", "mondaydotcom-write"]
    );
    // Monday alone is a day, and a drive alone is not Google Drive.
    assert!(services_read("On Monday, drive to work.").is_empty());
}

/// A photobooth file that names its key and token as variables, as the
/// owner's names them, with no value.
fn photobooth_with_names() -> String {
    format!(
        "{}\n## Keys\n\nThe image API key is read from `$GEMINI_API_KEY`, and the bot's from \
         os.environ[\"TELEGRAM_BOT_TOKEN\"]. Set both in the app.\n",
        photobooth()
    )
}

/// `LAMP_SKILL_SECRETS`: the file's variables' names (never a value) are
/// its `skill.toml`'s `secrets`, said to the app with the upload, the
/// tool's note says a key is never taken by voice, and the cap holds them.
/// Off, a file naming one is refused in plain words, never a name or
/// value in the refusal, and a file naming none is as before.
#[test]
fn with_the_keys_switch_a_files_variables_are_its_secrets_by_name() {
    switches(true, true);
    set_secrets_here(Some(true));
    let (files, _) = files(&photobooth_with_names()).unwrap();
    assert!(
        files
            .toml
            .contains("secrets = [\"GEMINI_API_KEY\", \"TELEGRAM_BOT_TOKEN\"]\n"),
        "{}",
        files.toml
    );
    assert_eq!(
        secrets_of_toml(&files.toml),
        ["GEMINI_API_KEY", "TELEGRAM_BOT_TOKEN"]
    );
    assert!(capped(&files.toml).is_ok());
    assert!(
        files
            .card
            .contains("Never ask for, take or repeat a key or token by voice")
    );
    assert!(files.card.len() < 1_100);
    // The upload tells the app the names.
    let (root, keeping) = keeping("agent-secret-names");
    let asked = json!({"upload": {"skill_md": photobooth_with_names()}});
    let (told, _) =
        crate::upload_skill::line(&asked, true, true, &place(&root), &keeping, NOW).unwrap();
    assert_eq!(
        told["kept"]["secrets"],
        json!(["GEMINI_API_KEY", "TELEGRAM_BOT_TOKEN"]),
        "{told}"
    );
    // A cap that names a reserved one, or a name twice, is refused.
    for bad in [
        "secrets = [\"PATH\"]",
        "secrets = [\"GOOD_KEY\", \"GOOD_KEY\"]",
        "secrets = \"GOOD_KEY\"",
    ] {
        let toml = files.toml.replace(
            "secrets = [\"GEMINI_API_KEY\", \"TELEGRAM_BOT_TOKEN\"]",
            bad,
        );
        assert!(capped(&toml).is_err(), "{bad}");
    }
    // Off: a file naming a key is refused plainly; one naming none is as before.
    set_secrets_here(Some(false));
    let why = super::files(&photobooth_with_names()).unwrap_err().0;
    assert_eq!(why, NEEDS_SECRETS);
    assert!(!why.contains("GEMINI_API_KEY"), "{why}");
    let (off, _) = super::files(&photobooth()).unwrap();
    assert!(!off.toml.contains("secrets"));
    assert!(!off.card.contains("by voice"));
    let _ = std::fs::remove_dir_all(&root);
    set_secrets_here(None);
    switches(false, false);
}

/// With the keys switch, a file carrying a value is still refused, and
/// told plainly to take the value out and add it in the app.
#[test]
fn with_the_keys_switch_a_value_in_the_file_is_told_to_go_in_the_app() {
    switches(true, true);
    set_secrets_here(Some(true));
    let fake = ["FAKE", "Key", "0000", "1111", "abcd", "2222", "efgh"].concat();
    let card = format!("{}\nGEMINI_API_KEY={fake}\n", photobooth());
    let why = files(&card).unwrap_err().0;
    assert_eq!(why, HAS_SECRET_VALUE);
    assert!(why.contains("add the value in the app"));
    assert!(!why.contains(&fake));
    set_secrets_here(None);
    switches(false, false);
}

/// The voice never takes a key: an agent skill's tool called while a key
/// it needs isn't in the app is told, by name, to add it there, and
/// nothing runs; with them all set, or the switch off, it runs.
#[test]
fn a_tool_missing_a_key_says_to_add_it_in_the_app_and_never_by_voice() {
    switches(true, true);
    set_secrets_here(Some(true));
    let (root, keeping) = keeping("agent-keys-missing");
    let made = take(&place(&root), &keeping, None, &photobooth_with_names(), NOW).unwrap();
    let asked = manage_helper(&keeping.manage);
    made_skill::put_on(&keeping, &made).unwrap();
    asked.join().unwrap();
    let needed = json!(["GEMINI_API_KEY", "TELEGRAM_BOT_TOKEN"]);
    let told = keys_missing_with(&keeping.directory, "photo_booth", |skill| {
        assert_eq!(skill, "photo-booth");
        Some(json!({"skill": skill, "needed": needed, "set": ["GEMINI_API_KEY"]}))
    })
    .unwrap();
    let line = told["refused"].as_str().unwrap();
    assert!(
        line.contains("needs TELEGRAM_BOT_TOKEN added in the app"),
        "{line}"
    );
    assert!(
        line.contains("Never ask for the key or token by voice"),
        "{line}"
    );
    assert!(!line.contains("GEMINI_API_KEY"), "{line}");
    // All set: it runs.
    assert!(
        keys_missing_with(&keeping.directory, "photo_booth", |skill| Some(
            json!({"skill": skill, "needed": needed, "set": needed})
        ))
        .is_none()
    );
    // The lamp's services can't say: left to lamp-tasks.
    assert!(keys_missing_with(&keeping.directory, "photo_booth", |_| None).is_none());
    // Another tool, or the switch off: nothing asked.
    assert!(keys_missing_with(&keeping.directory, "other_tool", |_| panic!()).is_none());
    set_secrets_here(Some(false));
    assert!(keys_missing_with(&keeping.directory, "photo_booth", |_| panic!()).is_none());
    let _ = std::fs::remove_dir_all(&root);
    set_secrets_here(None);
    switches(false, false);
}
