use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

const CARD: &str = "---\nname: tea-tip\ndescription: Call when someone asks for a tea tip.\n---\n\n# Tea tip\n\nSECRET-WORDS-IN-THE-FILE\n";
const TOML: &str = "kind = \"installable\"\nversion = \"1.0.0\"\ntool = \"tea_tip\"\n[permissions]\nspeak = true\n[[do]]\nsay = \"A tip.\"\n";

/// A folder of the test's own, with a skills folder and a pushed one.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("upload-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("skills")).unwrap();
    std::fs::create_dir_all(dir.join("pushed")).unwrap();
    dir
}

/// The Store's key, written where `store_keys` reads it.
fn store_key(dir: &Path) -> ring::signature::Ed25519KeyPair {
    use ring::signature::KeyPair;
    let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    std::fs::write(
        dir.join("store-key.pub"),
        base64::engine::general_purpose::STANDARD.encode(key.public_key().as_ref()),
    )
    .unwrap();
    key
}

/// The file's `upload_id`: its SHA-256's first 16 hex.
fn id(bytes: &[u8]) -> String {
    lamp_skills::package::sha256_hex(bytes)[..16].to_owned()
}

/// The backend's message's inner `data` for `bytes`, as Backend3 gives it.
fn data(bytes: &[u8], format: &str) -> Value {
    json!({
        "upload_id": id(bytes),
        "name": "tea-tip.md",
        "version": "",
        "format": format,
        "sha256": lamp_skills::package::sha256_hex(bytes),
        "size": bytes.len(),
        "content_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

/// A local backend answering `get-message` once with `data` for `kind`;
/// the request's head handed back.
fn backend(kind: &str, data: Value) -> (String, mpsc::Receiver<String>) {
    backend_after(kind, data, None)
}

/// [`backend`], answering only once `gate` says so: the fetch is in flight
/// until then.
fn backend_after(
    kind: &str,
    data: Value,
    gate: Option<mpsc::Receiver<()>>,
) -> (String, mpsc::Receiver<String>) {
    let body = json!({"status": 1, "data": {"cmd": "data", "kind": kind, "data": data}})
        .to_string()
        .into_bytes();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if stream.read_exact(&mut byte).is_err() {
                break;
            }
            head.push(byte[0]);
        }
        let _ = tx.send(String::from_utf8_lossy(&head).into_owned());
        if let Some(gate) = gate {
            let _ = gate.recv();
        }
        let mut answer = format!(
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        answer.extend_from_slice(&body);
        let _ = stream.write_all(&answer);
    });
    (base, rx)
}

/// A stand-in lamp-voice on a short path: each line asked, and the canned
/// answers in turn.
fn voice(name: &str, answers: Vec<Value>) -> (PathBuf, mpsc::Receiver<Value>) {
    let dir = PathBuf::from(format!("/tmp/up-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("make.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let (seen, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for answer in answers {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            seen.send(serde_json::from_str(&line).unwrap()).unwrap();
            writeln!(stream, "{answer}").unwrap();
        }
    });
    (socket, lines)
}

fn uploads(dir: &Path, base: &str, socket: &Path) -> Arc<Uploads> {
    let mut uploads = Uploads::new(
        base,
        "k-secret",
        "d-42",
        &dir.join("skills"),
        &dir.join("pushed"),
        &dir.join("store-key.pub"),
        socket,
    );
    uploads.https_only = false;
    uploads.settle_every = Duration::from_millis(10);
    uploads.settle_within = Duration::from_secs(5);
    uploads.retry_within = Duration::from_secs(5);
    Arc::new(uploads)
}

fn kept() -> Value {
    json!({"kept": {"name": "tea-tip", "description": "Only when you ask for it.",
        "version": "1.0.0", "sha256": "a".repeat(64),
        "uses": {"light": false, "body": false, "topics": []}}})
}

/// A tar of `entries`, as a package holds them: plain files, ustar.
fn tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (path, data) in entries {
        let mut header = [0u8; 512];
        header[..path.len()].copy_from_slice(path.as_bytes());
        header[100..107].copy_from_slice(b"0000644");
        header[108..115].copy_from_slice(b"0000000");
        header[116..123].copy_from_slice(b"0000000");
        header[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
        header[136..147].copy_from_slice(b"00000000000");
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        header[148..156].copy_from_slice(b"        ");
        let sum: u32 = header.iter().map(|&b| u32::from(b)).sum();
        header[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        out.extend_from_slice(&header);
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.resize(out.len() + 1024, 0);
    out
}

/// A package of `tea-tip` with `files`, signed with `key` if given.
fn package(files: &[(&str, &[u8])], key: Option<&ring::signature::Ed25519KeyPair>) -> Vec<u8> {
    let manifest = json!({
        "format": 1,
        "name": "tea-tip",
        "version": "1.0.0",
        "files": files.iter().map(|(path, data)| json!({
            "path": path,
            "size": data.len(),
            "sha256": lamp_skills::package::sha256_hex(data),
        })).collect::<Vec<_>>(),
    })
    .to_string()
    .into_bytes();
    let mut entries: Vec<(String, Vec<u8>)> = vec![("lamp-skill.json".into(), manifest.clone())];
    if let Some(key) = key {
        entries.push((
            "lamp-skill.sig".into(),
            lamp_skills::package::signature_for(&manifest, key).unwrap(),
        ));
    }
    for (path, data) in files {
        entries.push((format!("tea-tip/{path}"), data.to_vec()));
    }
    let entries: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(path, data)| (path.as_str(), data.as_slice()))
        .collect();
    tar(&entries)
}

/// The kind is answered only with its switch: `can` doesn't name it, and
/// an app that asks anyway is told it isn't on this lamp.
#[test]
fn without_the_switch_the_kind_is_not_answered() {
    let mut lamp = crate::answer::tests::Fake::default();
    assert!(!crate::answer::can(&lamp).contains(&KIND));
    let kind = format!("{KIND_PREFIX}{}", id(CARD.as_bytes()));
    assert_eq!(
        crate::answer::answer(&mut lamp, &kind, &json!({})),
        Err(crate::wire::not_here(&kind))
    );
    let dir = folder("switch");
    lamp.upload = Some(uploads(&dir, "http://127.0.0.1:9", Path::new("/nowhere")));
    assert!(crate::answer::can(&lamp).contains(&KIND));
    // Only `skills.upload.` and 16 lowercase hex.
    for other in [
        "skills.upload",
        "skills.upload.ABCDEF0123456789",
        "skills.upload.0123",
    ] {
        assert_eq!(id_of(other), None, "{other}");
    }
}

/// A bare SKILL.md: asked for by its kind with the lamp's key and id, its
/// file taken from the message, size, SHA-256 and id checked, handed to
/// the voice as a bare card, and `success` only once the voice says a
/// spoken yes put it on. The journal names no word or name of the file.
#[test]
fn a_bare_skill_md_is_taken_from_the_message_and_succeeds_only_once_on() {
    let dir = folder("bare");
    let upload_id = id(CARD.as_bytes());
    let (base, heads) = backend(
        &format!("{KIND_PREFIX}{upload_id}"),
        data(CARD.as_bytes(), "skill_md"),
    );
    let (socket, seen) = voice(
        "bare",
        vec![
            kept(),
            json!({"state": "kept"}),
            json!({"state": "asked"}),
            json!({"state": "on"}),
        ],
    );
    let uploads = uploads(&dir, &base, &socket);
    let (result, journal) = uploads.run(&upload_id);
    let kept = result.unwrap();
    assert_eq!(kept["added"], "file");
    assert_eq!(kept["uploaded"], true);
    assert_eq!(kept["upload_id"], upload_id.as_str());
    let head = heads.recv().unwrap();
    assert!(
        head.starts_with(&format!(
            "GET /devices/get-message?kind=skills.upload.{upload_id} "
        )),
        "{head}"
    );
    assert!(head.contains("authorization: Bearer k-secret"), "{head}");
    assert!(head.contains("x-device-id: d-42"), "{head}");
    assert_eq!(
        seen.recv().unwrap(),
        json!({"upload": {"skill_md": CARD}}),
        "a bare card: no skill.toml"
    );
    let journal = journal.join("\n");
    for secret in ["SECRET-WORDS", "tea-tip", "k-secret"] {
        assert!(!journal.contains(secret), "{secret}: {journal}");
    }
    // Asked until it is on: kept, asked, then on.
    let settled = uploads.settle(kept).unwrap().unwrap();
    assert_eq!(settled["state"], "on");
    for _ in 0..3 {
        assert_eq!(
            seen.recv().unwrap(),
            json!({"added": {"sha256": "a".repeat(64)}})
        );
    }
}

/// Kept but left off at the lamp: `failure`, said plainly, so the backend
/// deletes the file; and with no ask at all within the wait, too.
#[test]
fn left_off_or_never_asked_is_a_failure() {
    let dir = folder("left");
    let (socket, _seen) = voice("left", vec![json!({"state": "left_off"})]);
    let uploads = uploads(&dir, "http://127.0.0.1:9", &socket);
    let kept = json!({"sha256": "a".repeat(64)});
    assert_eq!(uploads.settle(kept.clone()), Err(LEFT_OFF.to_owned()));
    let mut waits = Uploads::new(
        "http://127.0.0.1:9",
        "k",
        "d",
        &dir.join("skills"),
        &dir.join("pushed"),
        &dir.join("key"),
        Path::new("/nowhere/make.sock"),
    );
    waits.settle_every = Duration::from_millis(10);
    waits.settle_within = Duration::from_millis(50);
    assert_eq!(waits.settle(kept), Err(NEVER_ASKED.to_owned()));
}

/// A package's two files go to the voice as they are; its refusal comes
/// back as the failure.
#[test]
fn a_package_goes_to_the_voice_and_its_refusal_is_the_failure() {
    let dir = folder("package");
    let bytes = package(
        &[
            ("skill.toml", TOML.as_bytes()),
            ("SKILL.md", CARD.as_bytes()),
        ],
        None,
    );
    let upload_id = id(&bytes);
    let (base, _) = backend(
        &format!("{KIND_PREFIX}{upload_id}"),
        data(&bytes, "package"),
    );
    let (socket, seen) = voice(
        "package",
        vec![json!({"refused": "it starts on its own, which a skill added from a file can't"})],
    );
    let (result, _) = uploads(&dir, &base, &socket).run(&upload_id);
    assert_eq!(
        result,
        Err("it starts on its own, which a skill added from a file can't".to_owned())
    );
    assert_eq!(
        seen.recv().unwrap(),
        json!({"upload": {"skill_md": CARD, "skill_toml": TOML}})
    );
}

/// The file must be the one uploaded: another SHA-256, size, an id that
/// isn't the kind's or the file's, or a format that isn't what it is, is
/// refused before anything is read of it.
#[test]
fn a_sha256_size_or_id_that_does_not_match_is_refused() {
    let bytes = CARD.as_bytes();
    let upload_id = id(bytes);
    assert!(decoded(&data(bytes, "skill_md"), &upload_id).is_ok());
    let changed = |field: &str, value: Value| {
        let mut data = data(bytes, "skill_md");
        data[field] = value;
        decoded(&data, &upload_id)
    };
    let mismatch = Err(MISMATCH.to_owned());
    assert_eq!(changed("sha256", json!("b".repeat(64))), mismatch);
    assert_eq!(changed("size", json!(bytes.len() - 1)), mismatch);
    assert_eq!(changed("upload_id", json!("0123456789abcdef")), mismatch);
    assert_eq!(
        changed(
            "content_base64",
            json!(base64::engine::general_purpose::STANDARD.encode(b"another file"))
        ),
        mismatch
    );
    // The kind's id, not the file's.
    assert_eq!(
        decoded(&data(bytes, "skill_md"), "0123456789abcdef"),
        mismatch
    );
    assert_eq!(changed("format", json!("zip")), Err(NOT_A_SKILL.to_owned()));
    // Said a package, sent a SKILL.md: refused through the whole of it.
    let dir = folder("format");
    let (base, _) = backend(&format!("{KIND_PREFIX}{upload_id}"), data(bytes, "package"));
    let (result, _) = uploads(&dir, &base, Path::new("/nowhere/make.sock")).run(&upload_id);
    assert_eq!(result, Err(NOT_A_SKILL.to_owned()));
}

/// Too big: past 1 MiB said, or in base64 past what 1 MiB is, refused
/// before it is decoded.
#[test]
fn a_file_too_big_is_refused() {
    let big = vec![b'-'; MOST_BYTES as usize + 1];
    let upload_id = id(&big);
    assert_eq!(
        decoded(&data(&big, "skill_md"), &upload_id),
        Err(TOO_BIG.to_owned())
    );
    // Said small, sent far bigger: refused before it is decoded.
    let bigger = vec![b'-'; 2 * MOST_BYTES as usize];
    let mut said_small = data(&bigger, "skill_md");
    said_small["size"] = json!(10);
    assert_eq!(decoded(&said_small, &id(&bigger)), Err(TOO_BIG.to_owned()));
    // Its message within the fetch's own bound, for this kind only.
    assert!(MOST_MESSAGE_BYTES > MOST_BYTES.div_ceil(3) * 4);
}

/// Never as the Store's: a package with the Store's signature is refused
/// and pointed at the Store; one signed otherwise goes on to the voice
/// unsigned.
#[test]
fn a_store_signed_package_is_refused() {
    let dir = folder("store");
    let key = store_key(&dir);
    let files = [
        ("skill.toml", TOML.as_bytes()),
        ("SKILL.md", CARD.as_bytes()),
    ];
    let signed = package(&files, Some(&key));
    assert_eq!(
        files_of(&signed, &dir.join("skills"), &dir.join("store-key.pub")),
        Err(FROM_THE_STORE.to_owned())
    );
    let other = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[9; 32]).unwrap();
    let not_the_stores = package(&files, Some(&other));
    assert_eq!(
        files_of(
            &not_the_stores,
            &dir.join("skills"),
            &dir.join("store-key.pub")
        ),
        Ok((Some(TOML.to_owned()), CARD.to_owned()))
    );
}

/// A bare `SKILL.md` saved with Windows line endings is read as a card,
/// as one with `\n` is, not sent down the package path.
#[test]
fn a_bare_card_with_crlf_is_read_as_a_card() {
    let dir = folder("crlf");
    let crlf = CARD.replace('\n', "\r\n");
    assert_eq!(
        files_of(
            crlf.as_bytes(),
            &dir.join("skills"),
            &dir.join("store-key.pub")
        ),
        Ok((None, crlf.clone()))
    );
    assert_eq!(
        files_of(
            CARD.as_bytes(),
            &dir.join("skills"),
            &dir.join("store-key.pub")
        ),
        Ok((None, CARD.to_owned()))
    );
}

/// No code: a package with `run.py` is refused, and so is one with any
/// file but the two.
#[test]
fn a_package_with_run_py_or_other_files_is_refused() {
    let dir = folder("code");
    let skills = dir.join("skills");
    let key = dir.join("store-key.pub");
    let with_code = package(
        &[
            ("skill.toml", TOML.as_bytes()),
            ("SKILL.md", CARD.as_bytes()),
            ("run.py", b"print('hi')"),
        ],
        None,
    );
    assert_eq!(files_of(&with_code, &skills, &key), Err(NO_CODE.to_owned()));
    let with_sound = package(
        &[
            ("skill.toml", TOML.as_bytes()),
            ("SKILL.md", CARD.as_bytes()),
            ("sounds/ding.wav", b"RIFF"),
        ],
        None,
    );
    assert_eq!(
        files_of(&with_sound, &skills, &key),
        Err(OTHER_FILES.to_owned())
    );
    assert_eq!(
        files_of(b"not a skill", &skills, &key),
        Err(NOT_A_SKILL.to_owned())
    );
}

/// A name the lamp holds is refused before the voice is asked: on the lamp
/// (core, Store, made) or pushed.
#[test]
fn a_taken_name_is_refused() {
    let dir = folder("taken");
    std::fs::create_dir_all(dir.join("skills").join("tea-tip")).unwrap();
    let why = held(CARD, &dir.join("skills"), &dir.join("pushed")).unwrap();
    assert!(why.contains("already on this lamp"), "{why}");
    std::fs::remove_dir_all(dir.join("skills").join("tea-tip")).unwrap();
    std::fs::create_dir_all(dir.join("pushed").join("tea-tip")).unwrap();
    assert!(held(CARD, &dir.join("skills"), &dir.join("pushed")).is_some());
    std::fs::remove_dir_all(dir.join("pushed").join("tea-tip")).unwrap();
    assert_eq!(held(CARD, &dir.join("skills"), &dir.join("pushed")), None);
    // Through the whole of it: the voice is never asked.
    std::fs::create_dir_all(dir.join("skills").join("tea-tip")).unwrap();
    let upload_id = id(CARD.as_bytes());
    let (base, _) = backend(
        &format!("{KIND_PREFIX}{upload_id}"),
        data(CARD.as_bytes(), "skill_md"),
    );
    let (result, journal) = uploads(&dir, &base, Path::new("/nowhere/make.sock")).run(&upload_id);
    let why = result.unwrap_err();
    assert!(
        why.contains("already on this lamp") && why.contains("tea-tip"),
        "{why}"
    );
    // The app is told the name; the journal never (El's P2 on #5080).
    let journal = journal.join("\n");
    assert!(!journal.contains("tea-tip"), "{journal}");
    assert!(journal.contains(REFUSED_BY_ITS_CHECKS), "{journal}");
}

/// lamp-voice's own refusal, which may name the skill, goes whole to the
/// app and as a fixed reason to the journal; a fixed reason of this module
/// is journaled as it is.
#[test]
fn a_refusal_naming_the_skill_never_reaches_the_journal() {
    let dir = folder("refused");
    let upload_id = id(CARD.as_bytes());
    let (base, _) = backend(
        &format!("{KIND_PREFIX}{upload_id}"),
        data(CARD.as_bytes(), "skill_md"),
    );
    let (socket, _seen) = voice(
        "refused",
        vec![json!({"refused": "a skill called tea-tip is already kept here"})],
    );
    let (result, journal) = uploads(&dir, &base, &socket).run(&upload_id);
    assert_eq!(
        result.unwrap_err(),
        "a skill called tea-tip is already kept here"
    );
    let journal = journal.join("\n");
    assert!(!journal.contains("tea-tip"), "{journal}");
    assert!(journal.contains(REFUSED_BY_ITS_CHECKS), "{journal}");
    assert_eq!(journaled(TOO_BIG), TOO_BIG);
    assert_eq!(journaled(LEFT_OFF), LEFT_OFF);
    assert_eq!(journaled("tea-tip is on"), REFUSED_BY_ITS_CHECKS);
}

/// Taken `starting`, one fetch at a time, its result kept for the app as
/// the same kind: here `failure`, the backend having no file to give.
#[test]
fn it_is_taken_starting_and_its_result_kept_for_the_app() {
    let dir = folder("start");
    let uploads = uploads(&dir, "http://127.0.0.1:9", Path::new("/nowhere"));
    let kind = format!("{KIND_PREFIX}0123456789abcdef");
    uploads.start(&kind).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut done = uploads.take();
    while done.is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        done = uploads.take();
    }
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].kind, kind);
    assert_eq!(done[0].result, Err(NO_MESSAGE.to_owned()));
    let reply = done[0].reply(&crate::wire::Identity::default(), "now");
    assert_eq!(reply["status"], "failure");
    assert_eq!(reply["kind"], kind.as_str());
    // Owed back first, then taken again.
    uploads.owe(done);
    assert_eq!(uploads.take().len(), 1);
    assert!(uploads.start("skills.upload").is_err());
}

/// The results ready within 10 s, or none.
fn done_within(uploads: &Uploads) -> Vec<Outcome> {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut done = uploads.take();
    while done.is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        done = uploads.take();
    }
    done
}

/// `uploads` keeping what is in flight in `state`.
fn uploads_kept_in(dir: &Path, base: &str, socket: &Path, state: &Path) -> Arc<Uploads> {
    let mut made = Arc::into_inner(uploads(dir, base, socket)).unwrap();
    made.state = Some(state.to_owned());
    Arc::new(made)
}

/// What the state file holds: never the skill's name or its words, only
/// the four fields, and only this process's user may read it.
fn state_file_holds_no_words(path: &Path) -> Value {
    use std::os::unix::fs::PermissionsExt;
    let text = std::fs::read_to_string(path).unwrap();
    for secret in [
        "tea-tip",
        "tea_tip",
        "SECRET-WORDS",
        "Tea tip",
        "Only when you ask",
        "k-secret",
    ] {
        assert!(!text.contains(secret), "{secret}: {text}");
    }
    let mode = std::fs::metadata(path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let read: Value = serde_json::from_str(&text).unwrap();
    for entry in read.as_array().unwrap() {
        let mut keys: Vec<&str> = entry
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["kind", "sha256", "state", "upload_id"]);
    }
    read
}

/// The same kind again while it is in flight (fetched, then waiting for the
/// yes, until its result is acked) is answered `starting` again and joins
/// it, never `BUSY`, so the backend keeps the copy the first is fetching;
/// one result settles both. Another file while one is fetched is `BUSY`
/// (El's P3s on #5080). The state file never names the skill.
#[test]
fn the_same_upload_again_joins_the_one_in_flight() {
    let dir = folder("again");
    let state = dir.join(STATE_FILE);
    let upload_id = id(CARD.as_bytes());
    let kind = format!("{KIND_PREFIX}{upload_id}");
    let (gate, held) = mpsc::channel();
    let (base, heads) = backend_after(&kind, data(CARD.as_bytes(), "skill_md"), Some(held));
    let (socket, seen) = voice("again", vec![kept(), json!({"state": "left_off"})]);
    let uploads = uploads_kept_in(&dir, &base, &socket, &state);
    uploads.start(&kind).unwrap();
    // The backend has been asked and holds its answer: in flight.
    heads.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(uploads.start(&kind), Ok(()), "the same kind joins");
    assert_eq!(
        uploads.start(&format!("{KIND_PREFIX}0123456789abcdef")),
        Err(BUSY.to_owned()),
        "another file while one is fetched"
    );
    let pending = uploads.pending();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        (pending[0].state.as_str(), &pending[0].sha256),
        (FETCHING, &None)
    );
    state_file_holds_no_words(&state);
    gate.send(()).unwrap();
    let done = done_within(&uploads);
    assert_eq!(done.len(), 1, "one result for both");
    assert_eq!(done[0].kind, kind);
    assert_eq!(done[0].result, Err(LEFT_OFF.to_owned()));
    // Handed once, asked once.
    assert!(seen.recv().unwrap().get("upload").is_some());
    assert!(seen.recv().unwrap().get("added").is_some());
    // Until the broker acks it, still in flight: the same kind again joins
    // it, with no second fetch.
    assert_eq!(uploads.start(&kind), Ok(()));
    let pending = uploads.pending();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].state, ASKING);
    assert_eq!(pending[0].sha256, Some("a".repeat(64)));
    assert_eq!(state_file_holds_no_words(&state)[0]["state"], ASKING);
    assert!(uploads.take().is_empty());
    uploads.acked(&done[0]);
    assert!(uploads.pending().is_empty());
    assert!(!state.exists(), "nothing in flight, no file");
}

/// A restart while it waits for the yes: the state file carries it on, it
/// is asked about by its SHA-256, and the app still hears `success`; a
/// record that isn't this module's is dropped.
#[test]
fn a_restart_while_asking_still_tells_the_app() {
    let dir = folder("restart-ask");
    let state = dir.join(STATE_FILE);
    let asked = format!("{KIND_PREFIX}fedcba9876543210");
    std::fs::write(
        &state,
        json!([
            {"upload_id": "fedcba9876543210", "kind": asked, "sha256": "b".repeat(64), "state": ASKING},
            {"upload_id": "0000000000000000", "kind": "skills.upload.1111111111111111", "sha256": null, "state": FETCHING},
            {"upload_id": "2222222222222222", "kind": "skills.upload.2222222222222222", "sha256": "tea-tip", "state": ASKING},
        ])
        .to_string(),
    )
    .unwrap();
    let (socket, seen) = voice(
        "restart-ask",
        vec![json!({"state": "asked"}), json!({"state": "on"})],
    );
    let uploads = uploads_kept_in(&dir, "http://127.0.0.1:9", &socket, &state);
    uploads.resume();
    let kinds: Vec<String> = uploads.pending().into_iter().map(|p| p.kind).collect();
    assert_eq!(
        kinds,
        std::slice::from_ref(&asked),
        "the unsound two dropped"
    );
    state_file_holds_no_words(&state);
    let done = done_within(&uploads);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].kind, asked);
    let told = done[0].result.clone().unwrap().unwrap();
    assert_eq!(told["state"], "on");
    assert_eq!(told["upload_id"], "fedcba9876543210");
    assert_eq!(told["sha256"], "b".repeat(64));
    assert_eq!(told.get("name"), None, "the state file never had it");
    for _ in 0..2 {
        assert_eq!(
            seen.recv().unwrap(),
            json!({"added": {"sha256": "b".repeat(64)}})
        );
    }
    uploads.acked(&done[0]);
    assert!(!state.exists());
}

/// A restart while it is fetched: fetched again (the backend still holds
/// it, no result having been acked), handed on and settled as at first.
#[test]
fn a_restart_while_fetching_fetches_it_again() {
    let dir = folder("restart-fetch");
    let state = dir.join(STATE_FILE);
    let upload_id = id(CARD.as_bytes());
    let kind = format!("{KIND_PREFIX}{upload_id}");
    std::fs::write(
        &state,
        json!([{"upload_id": upload_id, "kind": kind, "sha256": null, "state": FETCHING}])
            .to_string(),
    )
    .unwrap();
    let (base, heads) = backend(&kind, data(CARD.as_bytes(), "skill_md"));
    let (socket, seen) = voice("restart-fetch", vec![kept(), json!({"state": "on"})]);
    let uploads = uploads_kept_in(&dir, &base, &socket, &state);
    uploads.resume();
    let done = done_within(&uploads);
    assert!(heads.recv().unwrap().contains(&kind));
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].result.clone().unwrap().unwrap()["state"], "on");
    assert_eq!(seen.recv().unwrap(), json!({"upload": {"skill_md": CARD}}));
    assert_eq!(uploads.pending()[0].state, ASKING);
    state_file_holds_no_words(&state);
    uploads.acked(&done[0]);
    assert!(!state.exists());
}

/// A restart while it is fetched, lamp-voice not yet up (a reboot): the
/// hand-over is tried again until it answers, the file fetched once.
#[test]
fn a_restart_before_the_voice_is_up_hands_it_over_once_it_is() {
    let dir = folder("restart-early");
    let state = dir.join(STATE_FILE);
    let upload_id = id(CARD.as_bytes());
    let kind = format!("{KIND_PREFIX}{upload_id}");
    std::fs::write(
        &state,
        json!([{"upload_id": upload_id, "kind": kind, "sha256": null, "state": FETCHING}])
            .to_string(),
    )
    .unwrap();
    let (base, heads) = backend(&kind, data(CARD.as_bytes(), "skill_md"));
    let socket = PathBuf::from(format!(
        "/tmp/up-restart-early-{}/make.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    let uploads = uploads_kept_in(&dir, &base, &socket, &state);
    uploads.resume();
    assert!(heads.recv().unwrap().contains(&kind));
    std::thread::sleep(Duration::from_millis(200));
    let (up, seen) = voice("restart-early", vec![kept(), json!({"state": "on"})]);
    assert_eq!(up, socket);
    let done = done_within(&uploads);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].result.clone().unwrap().unwrap()["state"], "on");
    assert_eq!(seen.recv().unwrap(), json!({"upload": {"skill_md": CARD}}));
    assert!(heads.try_recv().is_err(), "fetched once");
}

/// LAMP_UPLOAD_TO_AGENT: off, a bare SKILL.md past 8 KiB is refused for
/// its length as before, and nothing reaches the voice; on, the same 4 KB
/// photobooth-like file goes to the voice whole, and its grants come back
/// for the app. Past 64 KiB it is refused even on.
#[test]
fn an_agent_skills_file_is_taken_to_64_kib_only_with_its_switch() {
    let body = "Take three photos with a countdown, flash the light, then post the strip to \
                the chat through the bot API at https://api.example.invalid/. "
        .repeat(80);
    let card = format!("---\nname: photo-booth\ndescription: Takes a photo strip.\n---\n{body}");
    assert!(card.len() > MOST_FILE_BYTES && card.len() < MOST_AGENT_BYTES);
    let run = |name: &str, card: &str, to_agent: bool, answers: Vec<Value>| {
        let dir = folder(name);
        let upload_id = id(card.as_bytes());
        let (base, _) = backend(
            &format!("{KIND_PREFIX}{upload_id}"),
            data(card.as_bytes(), "skill_md"),
        );
        let (socket, seen) = voice(name, answers);
        let mut uploads = Uploads::new(
            &base,
            "k-secret",
            "d-42",
            &dir.join("skills"),
            &dir.join("pushed"),
            &dir.join("store-key.pub"),
            &socket,
        );
        uploads.https_only = false;
        uploads.to_agent = to_agent;
        (uploads.run(&upload_id).0, seen)
    };
    let (off, seen) = run("agent-off", &card, false, vec![]);
    assert_eq!(off.unwrap_err(), TOO_LONG);
    assert!(seen.try_recv().is_err());
    let mut answer = kept();
    answer["kept"]["grants"] = json!(["light", "speak", "camera", "net"]);
    let (on, seen) = run("agent-on", &card, true, vec![answer]);
    let on = on.unwrap();
    assert_eq!(on["grants"], json!(["light", "speak", "camera", "net"]));
    assert_eq!(seen.recv().unwrap(), json!({"upload": {"skill_md": card}}));
    let long = format!("{card}{}", "x".repeat(MOST_AGENT_BYTES));
    let (past, _) = run("agent-past", &long, true, vec![]);
    assert_eq!(past.unwrap_err(), AGENT_TOO_LONG);
}
