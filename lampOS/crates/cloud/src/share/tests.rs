use super::*;
use crate::skills::tests::{places, skill};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

const MADE: &str = "kind = \"installable\"\nversion = \"1.0.0\"\n[permissions]\nbody = true";

/// A Store on a local port that answers one request with `status` and
/// `body`, and hands back the request as it came: its first line, its
/// headers and its body.
fn store(status: u16, body: &'static str) -> (Store, mpsc::Receiver<(String, Vec<u8>)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut read = Vec::new();
        let mut buf = [0u8; 65_536];
        let (head, length) = loop {
            let n = stream.read(&mut buf).unwrap();
            read.extend_from_slice(&buf[..n]);
            if let Some(end) = read.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&read[..end]).into_owned();
                let length = head
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())?
                    })
                    .unwrap_or(0);
                read.drain(..end + 4);
                break (head, length);
            }
        };
        while read.len() < length {
            let n = stream.read(&mut buf).unwrap();
            read.extend_from_slice(&buf[..n]);
        }
        let _ = stream.write_all(
            format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        );
        tx.send((head, read)).unwrap();
    });
    (
        Store {
            base,
            key: "k-secret".into(),
        },
        rx,
    )
}

/// The SHA-256 a preview of `name` gives, as the app sends it with Send.
fn previewed(places: &crate::skills::Places, name: &str, at: &str) -> String {
    let nowhere = Store {
        base: "http://127.0.0.1:9".into(),
        key: "k-secret".into(),
    };
    share(
        &nowhere,
        places,
        &json!({"name": name, "preview": true}),
        &scratch(at),
        "en",
    )
    .unwrap()["preview"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/lcsh-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_store_is_the_hosts_root_and_never_shows_its_key() {
    let account = |base: &str| Account {
        llm_base_url: base.into(),
        ..Default::default()
    };
    // A lamp's base, as setup keeps it: the routes sit at its host's root.
    let store = Store::of(&account("https://api.test/api/v1/ai/v1"), " k ").unwrap();
    assert_eq!(
        store.url("publish"),
        "https://api.test/api/v1/skills/publish"
    );
    assert_eq!(
        Store::of(&account("http://127.0.0.1:8080/api/v1/ai/v1/"), "k")
            .unwrap()
            .url("mine"),
        "http://127.0.0.1:8080/api/v1/skills/mine"
    );
    assert_eq!(Store::of(&account("not a url"), "k"), None);
    assert_eq!(store.key, "k");
    assert!(!format!("{store:?}").contains('k') || !format!("{store:?}").contains("\"k\""));
    assert!(format!("{store:?}").contains("<hidden>"));
    assert_eq!(Store::of(&account(""), "k"), None);
    assert_eq!(Store::of(&account("https://api.test/v1"), " "), None);
}

#[test]
fn an_author_is_a_first_name_on_one_line_or_none() {
    assert_eq!(author(&json!({})), Ok(None));
    assert_eq!(author(&json!({"author": "  "})), Ok(None));
    assert_eq!(author(&json!({"author": " Lan "})), Ok(Some("Lan".into())));
    assert_eq!(
        author(&json!({"author": "x".repeat(32)})),
        Ok(Some("x".repeat(32)))
    );
    for bad in [json!("x".repeat(33)), json!("Lan\nNguyen"), json!(7)] {
        assert_eq!(author(&json!({ "author": bad })), Err(BAD_AUTHOR.into()));
    }
}

#[test]
fn only_a_made_skill_that_is_on_can_be_shared() {
    let places = places("share-which");
    skill(&places, "water", "Water", "made: here\n", MADE);
    skill(&places, "tutor", "Desk tutor", "", MADE);
    skill(
        &places,
        "dance",
        "Dance",
        "",
        "kind = \"core\"\nversion = \"1.0.0\"\n[permissions]\nbody = true",
    );
    assert_eq!(
        made_here_and_on(&places, "water"),
        Ok(places.folder.join("water"))
    );
    assert_eq!(
        made_here_and_on(&places, "tutor"),
        Err(NOT_MADE_HERE.into())
    );
    assert_eq!(
        made_here_and_on(&places, "dance"),
        Err(NOT_MADE_HERE.into())
    );
    assert_eq!(
        made_here_and_on(&places, "nobody"),
        Err(crate::skills::NOT_A_NAME.into())
    );
    fs::write(&places.off, "water\n").unwrap();
    assert_eq!(made_here_and_on(&places, "water"), Err(NOT_ON.into()));
}

#[test]
fn the_package_is_the_skills_own_files_and_not_made_here_on_another_lamp() {
    let places = places("share-pack");
    skill(
        &places,
        "water",
        "Water",
        "made: here\nmade_by: app\n",
        MADE,
    );
    fs::write(places.folder.join("water/.package.json"), "{}").unwrap();
    let scratch = scratch("pack");
    let (file_name, tar) = pack(&places.folder.join("water"), &scratch).unwrap();
    assert_eq!(file_name, "water-1.0.0.tar");
    let at = scratch.join("read.tar");
    fs::write(&at, &tar).unwrap();
    let package = lamp_skills::package::read_package(&at).unwrap();
    fs::remove_file(&at).unwrap();
    assert_eq!(package.manifest.name, "water");
    assert_eq!(package.signature, None);
    let files: Vec<&String> = package.files.keys().collect();
    assert_eq!(files, ["SKILL.md", "skill.toml"]);
    let card = String::from_utf8(package.files["SKILL.md"].clone()).unwrap();
    assert!(!card.contains("made:"), "{card}");
    assert!(!card.contains("made_by"), "{card}");
    assert!(card.contains("name: water"), "{card}");
    // Nothing is left behind, and the lamp's own folder is untouched.
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
    assert!(
        fs::read_to_string(places.folder.join("water/SKILL.md"))
            .unwrap()
            .contains("made: here")
    );
}

#[test]
fn the_multipart_body_carries_the_file_and_the_author_when_given() {
    let (boundary, body) = multipart("water-1.0.0.tar", b"TAR", Some("Lan"), None, None);
    let text = String::from_utf8(body).unwrap();
    assert!(text.starts_with(&format!("--{boundary}\r\n")));
    assert!(text.contains("name=\"file\"; filename=\"water-1.0.0.tar\"\r\nContent-Type: application/x-tar\r\n\r\nTAR\r\n"));
    assert!(text.contains("name=\"author\"\r\n\r\nLan\r\n"));
    assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    let (_, body) = multipart("w.tar", b"TAR", None, None, None);
    let text = String::from_utf8(body).unwrap();
    assert!(!text.contains("author") && !text.contains("made_with"));
    // A pushed skill says it was made with the SDK, beside the author.
    let (boundary, body) = multipart("w.tar", b"TAR", Some("Lan"), Some("sdk"), None);
    let text = String::from_utf8(body).unwrap();
    assert!(text.contains("name=\"made_with\"\r\n\r\nsdk\r\n"));
    assert!(text.contains("name=\"author\"\r\n\r\nLan\r\n"));
    assert!(text.ends_with(&format!("--{boundary}--\r\n")));
}

#[test]
fn share_uploads_the_package_with_the_lamps_key_and_reads_back_pending() {
    let places = places("share-up");
    skill(&places, "water", "Water", "made: here\n", MADE);
    let (store, seen) = store(
        201,
        r#"{"status":1,"data":{"id":"s1","name":"water","version":"1.0.0","review_status":"pending","owner":"x"}}"#,
    );
    let sent = share(
        &store,
        &places,
        &json!({"name": "water", "author": "Lan", "sha256": previewed(&places, "water", "up-seen")}),
        &scratch("up"),
        "en",
    )
    .unwrap();
    assert_eq!(
        sent,
        json!({"sent": {"id": "s1", "name": "water", "version": "1.0.0", "review_status": "pending"}})
    );
    let (head, body) = seen.recv().unwrap();
    assert!(
        head.starts_with("POST /api/v1/skills/publish HTTP/1.1"),
        "{head}"
    );
    let head = head.to_lowercase();
    assert!(head.contains("authorization: bearer k-secret"), "{head}");
    assert!(
        head.contains("content-type: multipart/form-data; boundary="),
        "{head}"
    );
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("filename=\"water-1.0.0.tar\""));
    assert!(body.contains("name=\"author\"\r\n\r\nLan\r\n"));
}

#[test]
fn the_stores_refusals_are_passed_on_in_plain_words_never_the_key() {
    assert_eq!(
        reason(401, b"{\"error\":\"bad key k-secret\"}"),
        UNKNOWN_LAMP
    );
    assert_eq!(
        reason(
            409,
            br#"{"status":0,"message":"Request failed","error_code":"skill_name_reserved","error_message":"that name is a built-in skill's"}"#
        ),
        "the Skills Store said: that name is a built-in skill's"
    );
    assert_eq!(
        reason(
            409,
            br#"{"status":0,"message":"Request failed","error_code":"skill_name_taken"}"#
        ),
        "the Skills Store said: skill_name_taken"
    );
    assert_eq!(
        reason(429, br#"{"error":"5 a day from one lamp"}"#),
        "the Skills Store said: 5 a day from one lamp"
    );
    assert_eq!(reason(400, b"not json"), "the Skills Store said no (400)");

    let places = places("share-no");
    skill(&places, "water", "Water", "made: here\n", MADE);
    let (taken, _) = store(
        409,
        r#"{"status":0,"message":"Request failed","error_code":"skill_name_taken"}"#,
    );
    assert_eq!(
        share(
            &taken,
            &places,
            &json!({"name": "water", "sha256": previewed(&places, "water", "no-seen")}),
            &scratch("no"),
            "en"
        ),
        Err("the Skills Store said: skill_name_taken".into())
    );
    // A refusal the Store gives with a 200 and "status": 0 is one too.
    let (full, _) = store(
        200,
        r#"{"status":0,"message":"Request failed","error_message":"3 are waiting already"}"#,
    );
    assert_eq!(
        share(
            &full,
            &places,
            &json!({"name": "water", "sha256": previewed(&places, "water", "no2-seen")}),
            &scratch("no2"),
            "en"
        ),
        Err("the Skills Store said: 3 are waiting already".into())
    );
}

#[test]
fn shared_reads_mine_and_keeps_only_the_apps_fields() {
    let (store, seen) = store(
        200,
        r#"{"status":1,"data":{"data":[{"id":"s2","name":"a","version":"1.0.0","review_status":"rejected","review_note":"it names a person","owner_id":"o"},{"id":"s1","name":"b","review_status":"approved"},{"name":"no id"}],"total":3}}"#,
    );
    assert_eq!(
        shared(&store).unwrap(),
        json!({"shares": [
            {"id": "s2", "name": "a", "version": "1.0.0", "review_status": "rejected", "review_note": "it names a person"},
            {"id": "s1", "name": "b", "version": "", "review_status": "approved"},
        ]})
    );
    let (head, _) = seen.recv().unwrap();
    assert!(
        head.starts_with("GET /api/v1/skills/mine?limit=100 HTTP/1.1"),
        "{head}"
    );
    assert_eq!(
        rows(&json!([{"id": 3, "name": "c", "review_status": "pending"}]))[0]["id"],
        "3"
    );
}

#[test]
fn unshare_deletes_that_id_and_refuses_one_that_isnt() {
    let (store, seen) = store(200, r#"{"status":1,"data":null}"#);
    assert_eq!(
        unshare(&store, &json!({"id": "s-1_a"})),
        Ok(json!({"unshared": "s-1_a"}))
    );
    let (head, _) = seen.recv().unwrap();
    assert!(
        head.starts_with("DELETE /api/v1/skills/s-1_a HTTP/1.1"),
        "{head}"
    );
    for bad in [
        json!({}),
        json!({"id": ""}),
        json!({"id": "../mine"}),
        json!({"id": "x".repeat(65)}),
    ] {
        assert_eq!(unshare(&store, &bad), Err(BAD_ID.into()));
    }
}

#[test]
fn a_preview_shows_the_text_that_would_leave_and_sends_nothing() {
    let places = places("share-preview");
    skill(&places, "water", "Water", "made: here\n", MADE);
    // No Store at all: a preview never reaches one.
    let store = Store {
        base: "http://127.0.0.1:9".into(),
        key: "k-secret".into(),
    };
    let answer = share(
        &store,
        &places,
        &json!({"name": "water", "preview": true}),
        &scratch("preview"),
        "en",
    )
    .unwrap();
    let files = answer["preview"]["files"].as_array().unwrap();
    assert_eq!(answer["preview"]["name"], "water");
    let card = files
        .iter()
        .find(|file| file["path"] == "SKILL.md")
        .unwrap();
    assert!(!card["text"].as_str().unwrap().contains("made: here"));
    assert!(files.iter().any(|file| file["path"] == "skill.toml"));
    assert!(
        files
            .iter()
            .all(|file| !file["path"].as_str().unwrap().starts_with('.'))
    );
    assert_eq!(answer["preview"]["sha256"].as_str().unwrap().len(), 64);
}

/// The Reviewer-lead's P2 on #4948: the lamp sends only the bytes the owner
/// saw. A send with no SHA-256, or with the one of a skill since changed,
/// sends nothing; the same files give the same SHA-256 again.
#[test]
fn a_share_sends_only_the_package_its_preview_showed() {
    let places = places("share-seen");
    skill(&places, "water", "Water", "made: here\n", MADE);
    let first = previewed(&places, "water", "seen-1");
    assert_eq!(first, previewed(&places, "water", "seen-2"));
    let (store, sent) = store(
        201,
        r#"{"status":1,"data":{"id":"s1","name":"water","version":"1.0.0","review_status":"pending"}}"#,
    );
    assert_eq!(
        share(
            &store,
            &places,
            &json!({"name": "water"}),
            &scratch("seen-none"),
            "en"
        ),
        Err(NOT_SEEN.into())
    );
    // Made again between the preview and Send: another package.
    let folder = places.folder.join("water");
    let toml = fs::read_to_string(folder.join("skill.toml")).unwrap();
    fs::write(folder.join("skill.toml"), format!("{toml}\n# changed\n")).unwrap();
    assert_eq!(
        share(
            &store,
            &places,
            &json!({"name": "water", "sha256": first}),
            &scratch("seen-stale"),
            "en"
        ),
        Err(CHANGED.into())
    );
    assert!(sent.try_recv().is_err(), "nothing was sent");
    let now = previewed(&places, "water", "seen-3");
    assert!(
        share(
            &store,
            &places,
            &json!({"name": "water", "sha256": now}),
            &scratch("seen-ok"),
            "en"
        )
        .unwrap()
        .get("sent")
        .is_some()
    );
}

/// Mobile's P2 on #4965: the skill made again while its preview is read.
/// The files shown and the SHA-256 come from one copy, so they describe the
/// same bytes, and a send of the skill as it is now is refused.
#[test]
fn a_preview_shows_and_hashes_one_copy_even_when_the_skill_changes_meanwhile() {
    let places = places("share-snapshot");
    skill(&places, "water", "Water", "made: here\n", MADE);
    let folder = places.folder.join("water");
    let toml = fs::read_to_string(folder.join("skill.toml")).unwrap();
    let before = previewed(&places, "water", "snapshot-before");
    let (sha256, files) = seen(&folder, &scratch("snapshot-seen"), || {
        fs::write(folder.join("skill.toml"), format!("{toml}\n# made again\n")).unwrap();
    })
    .unwrap();
    // Both from the copy taken before the change.
    assert_eq!(sha256, before);
    let shown = files
        .iter()
        .find(|file| file["path"] == "skill.toml")
        .and_then(|file| file["text"].as_str())
        .unwrap()
        .to_owned();
    assert_eq!(shown, toml);
    // The skill now is another package: a send of what was seen is refused.
    let (store, sent) = store(
        201,
        r#"{"status":1,"data":{"id":"s1","name":"water","version":"1.0.0","review_status":"pending"}}"#,
    );
    assert_eq!(
        share(
            &store,
            &places,
            &json!({"name": "water", "sha256": sha256}),
            &scratch("snapshot-send"),
            "en"
        ),
        Err(CHANGED.into())
    );
    assert!(sent.try_recv().is_err(), "nothing was sent");
}

#[test]
fn a_sent_answer_with_no_id_is_unread_not_sent() {
    let places = places("share-unread");
    skill(&places, "water", "Water", "made: here\n", MADE);
    let (store, _) = store(201, r#"{"status":1,"data":{"name":"water"}}"#);
    assert_eq!(
        share(
            &store,
            &places,
            &json!({"name": "water", "sha256": previewed(&places, "water", "unread-seen")}),
            &scratch("unread"),
            "en"
        ),
        Err(UNREAD.into())
    );
}

/// A skill pushed from a computer, in the pushed folder beside the lamp's
/// own (`skills-dev`), with its `.pushed.json`.
fn pushed(places: &crate::skills::Places, name: &str, toml: &str) -> std::path::PathBuf {
    let dev = places.folder.with_file_name("skills-dev");
    fs::create_dir_all(&dev).unwrap();
    skill(
        &crate::skills::Places {
            folder: dev.clone(),
            ..places.clone()
        },
        name,
        "Wave",
        "",
        toml,
    );
    fs::write(
        dev.join(name).join(".pushed.json"),
        r#"{"computer":"laptop","client":"c_1","sha256":"ab","at_unix_ms":1}"#,
    )
    .unwrap();
    dev.join(name)
}

#[test]
fn a_pushed_skill_that_is_on_can_be_shared_and_a_held_or_off_one_cannot() {
    let places = places("share-pushed");
    let wave = pushed(&places, "wave", MADE);
    assert_eq!(shareable(&places, "wave"), Ok(wave.clone()));
    // Off: the owner turned it off.
    fs::write(&places.off, "wave\n").unwrap();
    assert_eq!(shareable(&places, "wave"), Err(NOT_ON.into()));
    fs::remove_file(&places.off).unwrap();
    // A skill that sees the picture is held until the owner says yes to it.
    let seen = pushed(
        &places,
        "watch",
        "format = 2\nkind = \"installable\"\nversion = \"1.0.0\"\n[permissions]\nbody = true\ncamera = true",
    );
    assert_eq!(shareable(&places, "watch"), Err(PUSHED_HELD.into()));
    fs::write(
        places.folder.with_file_name("skills-dev").join(".yes.json"),
        r#"[{"name":"watch","client":"c_1","sha256":"ab"}]"#,
    )
    .unwrap();
    assert_eq!(shareable(&places, "watch"), Ok(seen));
    // The page's row and Share agree: Share takes exactly the pushed rows
    // that load, are on, and are not held for the picture.
    for row in crate::skills::pushed_rows(&places, crate::skills::read_off(&places.off).as_ref()) {
        assert_eq!(
            shareable(&places, &row.name).is_ok(),
            row.loads && row.off == Some(false) && row.camera_allowed != Some(false),
            "{}",
            row.name
        );
    }
    // A skill that is not in either folder is refused as before, and the
    // lamp's own installed skills still are.
    assert_eq!(
        shareable(&places, "nobody"),
        Err(crate::skills::NOT_A_NAME.into())
    );
    skill(&places, "tutor", "Desk tutor", "", MADE);
    assert_eq!(shareable(&places, "tutor"), Err(NOT_MADE_HERE.into()));
    // A name that is no skill's name never reaches a path.
    assert!(shareable(&places, "../skills").is_err());
    // A pushed folder that is a link to a folder elsewhere is no pushed
    // skill: nothing outside `skills-dev` is ever packed.
    let outside = places.folder.with_file_name("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::copy(wave.join("SKILL.md"), outside.join("SKILL.md")).unwrap();
    fs::copy(wave.join("skill.toml"), outside.join("skill.toml")).unwrap();
    fs::copy(wave.join(".pushed.json"), outside.join(".pushed.json")).unwrap();
    std::os::unix::fs::symlink(
        &outside,
        places.folder.with_file_name("skills-dev").join("linked"),
    )
    .unwrap();
    assert_eq!(
        shareable(&places, "linked"),
        Err(crate::skills::NOT_A_NAME.into())
    );
}

#[test]
fn a_pushed_skill_goes_with_made_with_sdk_and_a_made_one_does_not() {
    let places = places("share-made-with");
    pushed(&places, "wave", MADE);
    skill(&places, "water", "Water", "made: here\n", MADE);
    for (name, field) in [("wave", true), ("water", false)] {
        let (store, seen) = store(
            201,
            r#"{"status":1,"data":{"id":"s1","name":"x","version":"1.0.0","review_status":"pending","owner":"x"}}"#,
        );
        share(
            &store,
            &places,
            &json!({"name": name, "sha256": previewed(&places, name, "mw-seen")}),
            &scratch("mw"),
            "en",
        )
        .unwrap();
        let (_, body) = seen.recv().unwrap();
        let body = String::from_utf8_lossy(&body);
        assert_eq!(
            body.contains("name=\"made_with\"\r\n\r\nsdk\r\n"),
            field,
            "{name}"
        );
        // Whatever the skill's folder holds as dot files never travels.
        assert!(!body.contains(".pushed.json") && !body.contains(".yes.json"));
    }
}

// ---- who can see a shared skill (#5209) ----

/// [`store`] under a name a test's own `store` variable can't hide.
fn stored(status: u16, body: &'static str) -> (Store, mpsc::Receiver<(String, Vec<u8>)>) {
    store(status, body)
}

/// A Store nothing is ever sent to: a request that reached it would fail as
/// unreachable, so a test that gets a different answer proves nothing left.
fn nowhere() -> Store {
    Store {
        base: "http://127.0.0.1:9".into(),
        key: "k-secret".into(),
    }
}

#[test]
fn a_mode_is_one_of_the_three_exactly_or_none_or_refused() {
    assert_eq!(visibility(&json!({})), Ok(None));
    assert_eq!(visibility(&json!({"visibility": null})), Ok(None));
    for mode in ["public", "unlisted", "private"] {
        assert_eq!(visibility(&json!({"visibility": mode})), Ok(Some(mode)));
    }
    for bad in [
        json!("Private"),
        json!("PUBLIC"),
        json!(" private"),
        json!("secret"),
        json!(""),
        json!(1),
        json!(true),
        json!(["private"]),
    ] {
        assert_eq!(
            visibility(&json!({"visibility": bad})),
            Err(BAD_VISIBILITY.into()),
            "{bad}"
        );
    }
}

#[test]
fn the_multipart_body_carries_a_mode_only_when_one_is_named() {
    let (boundary, body) = multipart("w.tar", b"TAR", None, None, Some("private"));
    let text = String::from_utf8(body).unwrap();
    assert!(text.contains("name=\"visibility\"\r\n\r\nprivate\r\n"));
    assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    for none in [
        multipart("w.tar", b"TAR", None, None, None).1,
        multipart("w.tar", b"TAR", Some("Lan"), Some("sdk"), None).1,
    ] {
        assert!(!String::from_utf8(none).unwrap().contains("visibility"));
    }
}

#[test]
fn a_preview_says_the_mode_and_the_lamps_line_for_it_and_sends_nothing() {
    let places = places("share-vis-preview");
    skill(&places, "water", "Water", "made: here\n", MADE);
    // No mode named: none said, none picked for the owner.
    let preview = share(
        &nowhere(),
        &places,
        &json!({"name": "water", "preview": true}),
        &scratch("vis-none"),
        "ko",
    )
    .unwrap();
    assert_eq!(preview["preview"]["visibility"], Value::Null);
    assert!(preview["preview"].get("visibility_note").is_none());
    // Each mode named: the mode, and the line in the owner's language.
    for (mode, language) in [("private", "en"), ("unlisted", "vi"), ("public", "ja")] {
        let preview = share(
            &nowhere(),
            &places,
            &json!({"name": "water", "preview": true, "visibility": mode}),
            &scratch("vis-named"),
            language,
        )
        .unwrap();
        assert_eq!(preview["preview"]["visibility"], mode);
        assert_eq!(
            preview["preview"]["visibility_note"],
            visibility_note(mode, language)
        );
        assert!(preview["preview"]["sha256"].as_str().unwrap().len() == 64);
    }
    // A mode that is none of the three is refused on a preview too.
    assert_eq!(
        share(
            &nowhere(),
            &places,
            &json!({"name": "water", "preview": true, "visibility": "secret"}),
            &scratch("vis-bad-preview"),
            "en",
        ),
        Err(BAD_VISIBILITY.into())
    );
}

#[test]
fn every_mode_has_a_line_in_every_language_and_english_for_any_other() {
    let mut seen = std::collections::HashSet::new();
    for language in ["en", "vi", "ja", "zh", "ko", "fr"] {
        for mode in VISIBILITIES {
            let line = visibility_note(mode, language);
            assert!(!line.trim().is_empty(), "{language} {mode}");
            assert!(seen.insert(line), "{language} {mode} repeats a line");
            if language != "en" {
                assert_ne!(line, visibility_note(mode, "en"), "{language} {mode}");
            }
        }
    }
    assert_eq!(seen.len(), 18);
    for other in ["", "de", "EN", "ja_JP", "en_GB"] {
        for mode in VISIBILITIES {
            assert_eq!(visibility_note(mode, other), visibility_note(mode, "en"));
        }
    }
    // Where the skill goes, that it is checked and who can see it, in
    // English; private never says it stays on the lamp.
    let private = visibility_note("private", "en");
    assert!(private.contains("goes to the Store") && private.contains("checked"));
    assert!(private.contains("only you"));
    assert!(visibility_note("unlisted", "en").contains("anyone with its link"));
    assert!(visibility_note("public", "en").contains("everyone"));
}

#[test]
fn a_send_names_the_mode_only_when_one_was_named_and_passes_the_stores_words_on() {
    let places = places("share-vis-send");
    skill(&places, "water", "Water", "made: here\n", MADE);
    let seen_sha = previewed(&places, "water", "vis-send-seen");

    // A mode named: it is uploaded, and the Store's line comes back with it
    // (beside the skill, as `data` carries it).
    let (store, seen) = stored(
        201,
        r#"{"status":1,"data":{"skill":{"id":"s1","name":"water","version":"1.0.0","review_status":"pending"},"visibility":"private","visibility_note":"Private: it goes to the Store."}}"#,
    );
    let sent = share(
        &store,
        &places,
        &json!({"name": "water", "sha256": seen_sha, "visibility": "private"}),
        &scratch("vis-send"),
        "en",
    )
    .unwrap();
    assert_eq!(sent["sent"]["visibility"], "private");
    assert_eq!(
        sent["sent"]["visibility_note"],
        "Private: it goes to the Store."
    );
    let (_, body) = seen.recv().unwrap();
    assert!(String::from_utf8_lossy(&body).contains("name=\"visibility\"\r\n\r\nprivate\r\n"));

    // The Store's words inside the skill are taken the same way.
    let (store, _) = stored(
        201,
        r#"{"status":1,"data":{"id":"s1","name":"water","version":"1.0.0","review_status":"pending","visibility":"unlisted","visibility_note":"Unlisted: by link."}}"#,
    );
    let sent = share(
        &store,
        &places,
        &json!({"name": "water", "sha256": previewed(&places, "water", "vis-send-seen2"), "visibility": "unlisted"}),
        &scratch("vis-send2"),
        "en",
    )
    .unwrap();
    assert_eq!(sent["sent"]["visibility"], "unlisted");
    assert_eq!(sent["sent"]["visibility_note"], "Unlisted: by link.");

    // No mode named: no field goes, so a new version keeps the owner's mode
    // at the Store, and nothing is said that the Store did not say.
    let (store, seen) = stored(
        201,
        r#"{"status":1,"data":{"id":"s1","name":"water","version":"1.0.1","review_status":"pending"}}"#,
    );
    let sent = share(
        &store,
        &places,
        &json!({"name": "water", "sha256": previewed(&places, "water", "vis-send-seen3")}),
        &scratch("vis-send3"),
        "en",
    )
    .unwrap();
    assert!(sent["sent"].get("visibility").is_none());
    assert!(sent["sent"].get("visibility_note").is_none());
    let (_, body) = seen.recv().unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("visibility"));

    // A mode that is none of the three never reaches the Store.
    assert_eq!(
        share(
            &nowhere(),
            &places,
            &json!({"name": "water", "sha256": previewed(&places, "water", "vis-send-seen4"), "visibility": "Private"}),
            &scratch("vis-send4"),
            "en",
        ),
        Err(BAD_VISIBILITY.into())
    );
}

#[test]
fn a_mode_is_changed_later_with_a_put_and_the_stores_refusals_are_told() {
    let (store, seen) = stored(
        200,
        r#"{"status":1,"data":{"id":"s1","visibility":"private","visibility_note":"Private: only you."}}"#,
    );
    assert_eq!(
        set_visibility(&store, &json!({"id": "s1", "visibility": "private"})),
        Ok(
            json!({"visibility": {"id": "s1", "visibility": "private", "visibility_note": "Private: only you."}})
        )
    );
    let (head, body) = seen.recv().unwrap();
    assert!(
        head.starts_with("PUT /api/v1/skills/s1/visibility HTTP/1.1"),
        "{head}"
    );
    let head = head.to_lowercase();
    assert!(head.contains("authorization: bearer k-secret"), "{head}");
    assert!(head.contains("content-type: application/json"), "{head}");
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({"visibility": "private"})
    );

    // The Store says nothing of the mode: the one asked for is told.
    let (store, _) = stored(200, r#"{"status":1,"data":null}"#);
    assert_eq!(
        set_visibility(&store, &json!({"id": "s-1_a", "visibility": "unlisted"})),
        Ok(json!({"visibility": {"id": "s-1_a", "visibility": "unlisted"}}))
    );

    // Someone else's skill, or none: the Store's word, plainly.
    let (store, _) = stored(
        404,
        r#"{"status":0,"message":"Request failed","error_code":"skill_not_found"}"#,
    );
    assert_eq!(
        set_visibility(&store, &json!({"id": "gone", "visibility": "public"})),
        Err("the Skills Store said: skill_not_found".into())
    );

    // A bad id, no mode or a bad one: refused before anything is sent.
    for (bad, why) in [
        (json!({"visibility": "private"}), BAD_ID),
        (json!({"id": "../x", "visibility": "private"}), BAD_ID),
        (json!({"id": "s1"}), NO_VISIBILITY),
        (json!({"id": "s1", "visibility": null}), NO_VISIBILITY),
        (json!({"id": "s1", "visibility": "secret"}), BAD_VISIBILITY),
    ] {
        assert_eq!(set_visibility(&nowhere(), &bad), Err(why.into()), "{bad}");
    }
}

#[test]
fn shared_keeps_the_stores_mode_and_line_when_it_says_them_and_drops_none() {
    let (store, _) = stored(
        200,
        r#"{"status":1,"data":{"data":[{"id":"s2","name":"a","version":"1.0.0","review_status":"approved","visibility":"private","visibility_note":"Private: only you.","owner_id":"o"},{"id":"s1","name":"b","review_status":"approved"}],"total":2}}"#,
    );
    let shares = shared(&store).unwrap();
    assert_eq!(shares["shares"][0]["visibility"], "private");
    assert_eq!(shares["shares"][0]["visibility_note"], "Private: only you.");
    assert!(shares["shares"][0].get("owner_id").is_none());
    assert!(shares["shares"][1].get("visibility").is_none());
    assert!(shares["shares"][1].get("visibility_note").is_none());
}

#[test]
fn the_share_kinds_are_the_four_and_visibility_is_the_one_that_says_the_field_is_taken() {
    assert_eq!(
        KINDS,
        [
            "skills.share",
            "skills.shared",
            "skills.unshare",
            "skills.visibility"
        ]
    );
}

/// #8610, El's P2.2 and the first builder's call 2 on #8688: a skill made
/// with code and kept before #8651 that a trigger starts and whose code
/// would speak still runs, but its preview and its send are refused in the
/// owner's plain line, never the agent's repair text; the same skill
/// told-only previews.
#[test]
fn a_kept_trigger_skill_that_would_speak_is_refused_in_a_plain_line() {
    lamp_skills::task::set_made_here(Some(true));
    let places = places("share-told-only");
    let timed = |name: &str, grants: &str| {
        let dir = places.folder.join(name);
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: Today's news.\nmade: here\n---\n\n# News\n\nMore.\n"
            ),
        )
        .unwrap();
        fs::write(
            dir.join("skill.toml"),
            format!(
                "version = \"1.0.0\"\ngrants = {grants}\n[trigger]\n\
                 topic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n\
                 [ask]\npolicy = \"nudge\"\n[permissions]\ntopics = [\"voice/state\"]\n\
                 [[do]]\ntask = {{ task = \"Sum up today's news\", run = \"scripts/main.py\" }}\n"
            ),
        )
        .unwrap();
        fs::write(dir.join("scripts/main.py"), "print('news')\n").unwrap();
        // Kept by the manage helper as a made package, not from the Store.
        fs::write(
            dir.join(lamp_skills::package::INSTALLED_FROM),
            serde_json::to_vec(&lamp_skills::package::Installed {
                sha256: "ab".repeat(32),
                manifest: lamp_skills::package::Manifest {
                    format: lamp_skills::package::FORMAT,
                    name: name.into(),
                    version: "1.0.0".into(),
                    files: Vec::new(),
                },
                from_store: false,
            })
            .unwrap(),
        )
        .unwrap();
        dir
    };
    let speaks = timed("speaks", r#"["speak"]"#);
    timed("told", r#"["volume"]"#);
    let (store, sent) = store(201, r#"{"status":1,"data":{"id":"s1"}}"#);
    for (at, data) in [
        ("preview", json!({"name": "speaks", "preview": true})),
        ("send", json!({"name": "speaks", "sha256": "ab".repeat(32)})),
    ] {
        let refused = share(
            &store,
            &places,
            &data,
            &scratch(&format!("told-{at}")),
            "en",
        );
        assert_eq!(refused, Err(SPEAKS_ON_ITS_OWN.into()), "{at}");
    }
    assert!(sent.try_recv().is_err(), "nothing was sent");
    // The repair text stays the agent's, from `lamp-skills pack` itself.
    let why = lamp_skills::package::pack(&speaks, &scratch("told-agent").join("out")).unwrap_err();
    assert!(
        why.contains(lamp_voice::made_skill::TOLD_ONLY_SHAPE),
        "{why}"
    );
    assert_eq!(previewed(&places, "told", "told-ok").len(), 64);
    lamp_skills::task::set_made_here(None);
}
