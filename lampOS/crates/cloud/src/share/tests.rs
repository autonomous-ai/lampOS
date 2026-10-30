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
    skill(&places, "water", "Water", "made: here\n", MADE);
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
    let (boundary, body) = multipart("water-1.0.0.tar", b"TAR", Some("Lan"));
    let text = String::from_utf8(body).unwrap();
    assert!(text.starts_with(&format!("--{boundary}\r\n")));
    assert!(text.contains("name=\"file\"; filename=\"water-1.0.0.tar\"\r\nContent-Type: application/x-tar\r\n\r\nTAR\r\n"));
    assert!(text.contains("name=\"author\"\r\n\r\nLan\r\n"));
    assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    let (_, body) = multipart("w.tar", b"TAR", None);
    assert!(!String::from_utf8(body).unwrap().contains("author"));
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
        &json!({"name": "water", "author": "Lan"}),
        &scratch("up"),
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
        share(&taken, &places, &json!({"name": "water"}), &scratch("no")),
        Err("the Skills Store said: skill_name_taken".into())
    );
    // A refusal the Store gives with a 200 and "status": 0 is one too.
    let (full, _) = store(
        200,
        r#"{"status":0,"message":"Request failed","error_message":"3 are waiting already"}"#,
    );
    assert_eq!(
        share(&full, &places, &json!({"name": "water"}), &scratch("no2")),
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
        head.starts_with("GET /api/v1/skills/mine HTTP/1.1"),
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
            &json!({"name": "water"}),
            &scratch("unread")
        ),
        Err(UNREAD.into())
    );
}
