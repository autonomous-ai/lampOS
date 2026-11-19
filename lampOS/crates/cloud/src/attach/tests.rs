use super::*;
use std::io::Read;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::sync::mpsc;

/// A made-up photo: a JPEG's first bytes and some more, no face in it.
fn jpeg(len: usize) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes.resize(len, 7);
    bytes
}

const TEXT: &str = "Bea's soup: SECRET-WORDS-IN-THE-ATTACHMENT, two carrots.";

fn id_for(bytes: &[u8]) -> String {
    lamp_skills::package::sha256_hex(bytes)[..16].to_owned()
}

/// The backend's message's inner `data` for `bytes`, as the contract says.
fn data(bytes: &[u8], kind: &str, mime: &str) -> Value {
    json!({
        "id": id_for(bytes),
        "kind": kind,
        "mime": mime,
        "size": bytes.len(),
        "sha256": lamp_skills::package::sha256_hex(bytes),
        "sent_at_ms": 1_763_000_000_000_u64,
        "content_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

/// A local backend answering `get-message` once with `data` for `kind`;
/// the request's head handed back.
fn backend(kind: &str, data: Value) -> (String, mpsc::Receiver<String>) {
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

/// Attachments kept in `dir/inbox` and told on a bus under `dir/bus`.
fn inbox_at(dir: &Path, base: &str) -> Attachments {
    std::fs::create_dir_all(dir.join("inbox")).unwrap();
    let mut attachments = Attachments::new(base, "k-secret", "d-42", &dir.join("inbox"));
    attachments.https_only = false;
    attachments.bus = Some(lamp_bus::Bus::new(dir.join("bus")).unwrap());
    attachments
}

#[test]
fn the_kind_is_attach_and_sixteen_lowercase_hex() {
    assert_eq!(id_of("attach.0123456789abcdef"), Some("0123456789abcdef"));
    for kind in [
        "attach",
        "attach.",
        "attach.0123456789ABCDEF",
        "attach.0123456789abcde",
        "attach.0123456789abcdef0",
        "attach.../../etc/passwd",
        "skills.upload.0123456789abcdef",
    ] {
        assert_eq!(id_of(kind), None, "{kind}");
    }
}

#[test]
fn a_photo_is_fetched_with_the_lamps_key_kept_0600_and_told_by_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let photo = jpeg(4_096);
    let id = id_for(&photo);
    let kind = format!("{KIND_PREFIX}{id}");
    let (base, heads) = backend(&kind, data(&photo, "photo", "image/jpeg"));
    let attachments = inbox_at(dir.path(), &base);
    // An older attachment in the folder goes: only the newest is kept.
    std::fs::write(dir.path().join("inbox/fedcba9876543210"), b"older").unwrap();
    let bus = lamp_bus::Bus::new(dir.path().join("bus")).unwrap();
    let mut voice = bus
        .subscribe_typed(AttachRef::TOPIC, "voice", AttachRef::TYPE)
        .unwrap();
    let attach = attachments.run(&id).unwrap();
    let head = heads.recv().unwrap();
    assert!(
        head.starts_with(&format!("GET /devices/get-message?kind=attach.{id} ")),
        "{head}"
    );
    assert!(head.contains("authorization: Bearer k-secret"), "{head}");
    assert!(head.contains("x-device-id: d-42"), "{head}");
    let path = dir.path().join("inbox").join(&id);
    assert_eq!(attach.path, path.to_str().unwrap());
    assert_eq!(std::fs::read(&path).unwrap(), photo);
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!dir.path().join("inbox/fedcba9876543210").exists());
    let told = voice
        .recv(Duration::from_secs(2))
        .unwrap()
        .expect("attachments/arrived")
        .parse::<AttachRef>()
        .unwrap();
    assert_eq!(
        told,
        AttachRef {
            path: path.to_str().unwrap().to_owned(),
            id: id.clone(),
            kind: "photo".into(),
            mime: "image/jpeg".into(),
            bytes: 4_096,
            sent_at: 1_763_000_000_000,
        }
    );
}

#[test]
fn what_the_backend_gives_is_refused_out_of_its_bounds() {
    let photo = jpeg(1_000);
    let ok = data(&photo, "photo", "image/jpeg");
    let id = id_for(&photo);
    assert_eq!(
        decoded(&ok, &id).map(|(kind, mime, bytes, _)| (kind, mime, bytes)),
        Ok(("photo".into(), "image/jpeg".into(), photo.clone()))
    );
    let with = |field: &str, value: Value| {
        let mut data = ok.clone();
        data[field] = value;
        data
    };
    let big = jpeg(184 * 1_024 + 1);
    let png = [0x89, b'P', b'N', b'G', 0, 0];
    let latin = [b'c', b'a', 0xE9];
    for (data, at, why) in [
        // Its size, SHA-256 or id isn't what was said.
        (with("size", json!(999)), id.clone(), MISMATCH),
        (with("sha256", json!("0".repeat(64))), id.clone(), MISMATCH),
        (with("id", json!("0123456789abcdef")), id.clone(), MISMATCH),
        (ok.clone(), "0123456789abcdef".to_owned(), MISMATCH),
        (
            with("content_base64", json!("not base64!")),
            id.clone(),
            MISMATCH,
        ),
        // Too big for its kind, said or sent.
        (data(&big, "photo", "image/jpeg"), id_for(&big), TOO_BIG),
        (with("size", json!(184 * 1_024 + 1)), id.clone(), TOO_BIG),
        (
            data(TEXT.repeat(1_200).as_bytes(), "text", "text/plain"),
            id_for(TEXT.repeat(1_200).as_bytes()),
            TOO_BIG,
        ),
        // Neither a JPEG nor UTF-8 text.
        (data(&png, "photo", "image/png"), id_for(&png), NOT_TAKEN),
        (data(&png, "photo", "image/jpeg"), id_for(&png), NOT_TAKEN),
        (
            data(&photo, "pdf", "application/pdf"),
            id.clone(),
            NOT_TAKEN,
        ),
        (
            data(&latin, "text", "text/plain"),
            id_for(&latin),
            NOT_TAKEN,
        ),
        // Nothing to read.
        (with("content_base64", Value::Null), id.clone(), NO_MESSAGE),
    ] {
        assert_eq!(
            decoded(&data, &at).map(|_| ()),
            Err(why.to_owned()),
            "{data}"
        );
    }
    let text = data(TEXT.as_bytes(), "text", "text/markdown; charset=utf-8");
    assert_eq!(
        decoded(&text, &id_for(TEXT.as_bytes())).map(|(kind, mime, _, _)| (kind, mime)),
        Ok(("text".into(), "text/markdown".into()))
    );
}

#[test]
fn a_refused_attachment_leaves_nothing_and_the_journal_never_holds_its_words() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = TEXT.as_bytes();
    let id = id_for(bytes);
    let kind = format!("{KIND_PREFIX}{id}");
    // Taken: the journal says its id, kind, bytes and ms alone.
    let (base, _heads) = backend(&kind, data(bytes, "text", "text/plain"));
    let attachments = inbox_at(dir.path(), &base);
    let result = attachments.run(&id);
    assert!(result.is_ok(), "{result:?}");
    let line = journal_line(&id, &result, Duration::from_millis(812));
    assert_eq!(
        line,
        format!(
            "an attachment from the app: id {id}, text, {} bytes, kept and told in 812 ms",
            bytes.len()
        )
    );
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    for words in ["SECRET-WORDS", "soup", "Bea", &encoded[..12]] {
        assert!(!line.contains(words), "{line}");
    }
    // Refused, its SHA-256 not the one said: nothing is kept, and the
    // journal says only a fixed reason.
    let mut lied = data(bytes, "text", "text/plain");
    lied["sha256"] = json!("f".repeat(64));
    let (base, _heads) = backend(&kind, lied);
    let attachments = inbox_at(dir.path(), &base);
    std::fs::remove_dir_all(dir.path().join("inbox")).unwrap();
    std::fs::create_dir_all(dir.path().join("inbox")).unwrap();
    let result = attachments.run(&id);
    assert_eq!(result, Err(MISMATCH.to_owned()));
    assert_eq!(
        std::fs::read_dir(dir.path().join("inbox")).unwrap().count(),
        0
    );
    let line = journal_line(&id, &result, Duration::from_millis(5));
    assert_eq!(
        line,
        format!("an attachment from the app: id {id}, not kept: {MISMATCH}, in 5 ms")
    );
    // A reason that isn't one of this module's never reaches the journal.
    let line = journal_line(&id, &Err(TEXT.to_owned()), Duration::ZERO);
    assert!(!line.contains("SECRET-WORDS"), "{line}");
    // With no bus, nobody is told and nothing is kept.
    let (base, _heads) = backend(&kind, data(bytes, "text", "text/plain"));
    let mut attachments = inbox_at(dir.path(), &base);
    attachments.bus = None;
    assert_eq!(attachments.run(&id), Err(NOT_TOLD.to_owned()));
    assert_eq!(
        std::fs::read_dir(dir.path().join("inbox")).unwrap().count(),
        0
    );
}

#[test]
fn a_second_attachment_while_one_is_fetched_is_busy_and_results_wait_for_the_app() {
    let dir = tempfile::tempdir().unwrap();
    // A backend that never answers keeps the first in flight.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let attachments = Arc::new(inbox_at(dir.path(), &base));
    attachments.start("attach.0123456789abcdef").unwrap();
    assert_eq!(
        attachments.start("attach.fedcba9876543210"),
        Err(BUSY.to_owned())
    );
    assert_eq!(
        attachments.start("attach.nothex"),
        Err(crate::wire::not_here("attach.nothex"))
    );
    drop(listener);
    let until = Instant::now() + Duration::from_secs(40);
    let taken = loop {
        let taken = attachments.take();
        if !taken.is_empty() || Instant::now() > until {
            break taken;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0].kind, "attach.0123456789abcdef");
    assert_eq!(taken[0].result, Err(NO_MESSAGE.to_owned()));
    attachments.owe(taken);
    assert_eq!(attachments.take().len(), 1);
}
