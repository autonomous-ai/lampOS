//! The Skills Store's lamp side (`docs/skills.md`, "From the Skills Store",
//! #1722): a package fetched from a URL by root's manage helper and
//! installed only when it is signed with the Store's key. Every
//! request here goes straight to the helper's own code (`package::asked`),
//! or to the real `lamp-skills asked` binary, with no `lamp-cloud` before
//! it: the helper refuses a bad package on its own.
//!
//! The Store is a local server speaking plain HTTP, reached through
//! `lamp-update`'s own HTTP code (`fetch::get_over`) as `lamp-update`'s
//! tests reach theirs: what is tested is the exchange, the redirects and
//! the package, not TLS.
use lamp_skills::package::{self, Entry, MANIFEST, Manifest, SIGNATURE, Store, sha256_hex};
use lamp_update::fetch::{Channel, Fetch, Stream, get_over};
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

fn skills_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills")
}

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The Store's key, from a seed: `which` tells two keys apart.
fn key(which: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[which; 32]).unwrap()
}

/// The Store's public key written as a lamp keeps it
/// (`/etc/lampOS/skills/store-key.pub`): 32 bytes in base64 on one line.
fn key_file(dir: &Path, key: &Ed25519KeyPair) -> PathBuf {
    use base64::Engine;
    let path = dir.join("key.pub");
    fs::write(
        &path,
        format!(
            "{}\n",
            base64::engine::general_purpose::STANDARD.encode(key.public_key().as_ref())
        ),
    )
    .unwrap();
    path
}

#[test]
fn store_readiness_requires_its_own_key_and_an_explicit_on() {
    assert_eq!(package::STORE_KEY, "/etc/lampOS/skills/store-key.pub");
    let dir = tempfile::tempdir().unwrap();
    let update = dir.path().join("update");
    let store = dir.path().join("skills/store-key.pub");
    fs::create_dir_all(&update).unwrap();
    fs::create_dir_all(store.parent().unwrap()).unwrap();
    let update_key = key_file(&update, &key(1));
    // No build's key in this skills folder: the factory's file alone.
    let folder = dir.path().join("no-build-key");
    assert!(!package::store_ready(Some("on"), &folder, &store));
    fs::write(&store, "not a key").unwrap();
    assert!(!package::store_ready(Some("on"), &folder, &store));
    let separate = key_file(dir.path(), &key(2));
    fs::copy(separate, &store).unwrap();
    for switch in [None, Some("off"), Some("ON"), Some("")] {
        assert!(!package::store_ready(switch, &folder, &store));
    }
    assert!(package::store_ready(Some("on"), &folder, &store));
    assert_ne!(package::store_key(&store), package::store_key(&update_key));
    fs::remove_file(&store).unwrap();
    assert!(!package::store_ready(Some("on"), &folder, &store));
}

/// Every name in a folder, hidden ones too.
fn left(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A skills folder with the lamp's core Dance in it, and beside it the
/// state folder [`ask`] gives the helper, with the owner's yes to Desk
/// tutor's camera kept: its package asks the camera (#3504's piece C).
fn skills(dir: &Path) -> PathBuf {
    let skills = dir.join("skills");
    copy(&skills_root().join("dance"), &skills.join("dance"));
    let state = skills.with_extension("state");
    fs::create_dir_all(&state).unwrap();
    lamp_skills::keep_names(
        &state.join(lamp_skills::CAMERA_FILE),
        &["desk-tutor".to_owned()].into(),
    )
    .unwrap();
    skills
}

/// Desk tutor's folder, installable, in `dir`.
fn tutor(dir: &Path) -> PathBuf {
    let skill = dir.join("desk-tutor");
    copy(&skills_root().join("desk-tutor"), &skill);
    skill
}

/// Desk tutor at `version`, packed and signed with `key`: the package's
/// bytes.
fn packed_at(dir: &Path, key: &Ed25519KeyPair, version: &str) -> Vec<u8> {
    let skill = tutor(&dir.join(format!("src-{version}")));
    let toml = fs::read_to_string(skill.join("skill.toml")).unwrap();
    assert!(toml.contains("version = \"1.0.0\""), "{toml}");
    fs::write(
        skill.join("skill.toml"),
        toml.replace("version = \"1.0.0\"", &format!("version = \"{version}\"")),
    )
    .unwrap();
    let path =
        package::pack_signed(&skill, &dir.join(format!("out-{version}")), Some(key)).unwrap();
    fs::read(path).unwrap()
}

/// Desk tutor packed, signed with `key` or not: the package's bytes.
fn packed(dir: &Path, key: Option<&Ed25519KeyPair>) -> Vec<u8> {
    let out = dir.join(format!("out-{}", key.is_some()));
    let path = package::pack_signed(&tutor(&dir.join("src")), &out, key).unwrap();
    fs::read(path).unwrap()
}

/// A tar entry's header, its name written as given, unchecked: a package
/// made by someone who means harm.
fn header(path: &str, size: usize, kind: tar::EntryType) -> tar::Header {
    let mut header = tar::Header::new_ustar();
    header.set_entry_type(kind);
    header.set_size(size as u64);
    header.set_mode(0o644);
    header.as_old_mut().name[..path.len()].copy_from_slice(path.as_bytes());
    header.set_cksum();
    header
}

/// A tar of these entries, in order: plain files, or a link to `target`.
fn tar_of(entries: &[(&str, Vec<u8>, Option<&str>)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, bytes, link) in entries {
        match link {
            None => {
                let header = header(path, bytes.len(), tar::EntryType::Regular);
                builder.append(&header, &bytes[..]).unwrap();
            }
            Some(target) => {
                let mut header = header(path, 0, tar::EntryType::Symlink);
                header.set_link_name(target).unwrap();
                header.set_cksum();
                builder.append(&header, &[][..]).unwrap();
            }
        }
    }
    builder.into_inner().unwrap()
}

/// A package by hand: a manifest of `listed` for `name`, signed with `key`,
/// and the entries `entries` after it, as given.
fn by_hand(
    key: &Ed25519KeyPair,
    name: &str,
    listed: &[(&str, &[u8])],
    entries: &[(&str, Vec<u8>, Option<&str>)],
) -> Vec<u8> {
    let manifest = serde_json::to_vec(&Manifest {
        format: package::FORMAT,
        name: name.into(),
        version: "1.0.0".into(),
        files: listed
            .iter()
            .map(|(path, bytes)| Entry {
                path: (*path).to_owned(),
                size: bytes.len() as u64,
                sha256: sha256_hex(bytes),
            })
            .collect(),
    })
    .unwrap();
    let signature = package::signature_for(&manifest, key).unwrap();
    let mut all = vec![(MANIFEST, manifest, None), (SIGNATURE, signature, None)];
    all.extend(entries.iter().cloned());
    tar_of(&all)
}

/// A Store on a free local port, giving each of lamp-skills' requests the
/// next answer as written, and how many it took. Another program on the host
/// can connect to the port too, a client of whatever server had it before: a
/// `node` agent's `GET /v1/models` once counted as a second fetch (#1569 row
/// 61). So only a request from lamp-skills' fetch (`User-Agent:
/// lamp-update`) is answered and counted.
///
/// It stops after 3 s with nothing more asked, or at [`Served::join`]: the
/// test's asks have returned by then, so a fetch of theirs has connected,
/// and one more look takes it.
fn serve(answers: Vec<Vec<u8>>) -> (u16, Served) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = Arc::clone(&stop);
    let thread = thread::spawn(move || {
        let mut taken = 0;
        let until = std::time::Instant::now() + Duration::from_secs(3);
        for answer in answers {
            let mut socket = loop {
                // Read before the look, so the look comes after the stop.
                let stopping = stopped.load(Ordering::SeqCst);
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket.set_nonblocking(false).unwrap();
                        // A stranger that says nothing can't hold the Store.
                        socket
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        let mut request = Vec::new();
                        let mut byte = [0u8; 1];
                        while !request.ends_with(b"\r\n\r\n") {
                            if socket.read_exact(&mut byte).is_err() {
                                break;
                            }
                            request.push(byte[0]);
                        }
                        if String::from_utf8_lossy(&request).contains("User-Agent: lamp-update") {
                            break socket;
                        }
                    }
                    Err(_) if !stopping && std::time::Instant::now() < until => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => return taken,
                }
            };
            taken += 1;
            let _ = socket.write_all(&answer);
        }
        taken
    });
    (port, Served { stop, thread })
}

/// The Store [`serve`] runs.
struct Served {
    stop: Arc<AtomicBool>,
    thread: thread::JoinHandle<usize>,
}

impl Served {
    /// How many requests the Store answered, once it is stopped.
    fn join(self) -> thread::Result<usize> {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.join()
    }
}

/// A server on a free local port that answers `answer` to whatever
/// connects, at once and without reading: plain HTTP where TLS is spoken.
/// It answers everyone for 3 s, so a stranger first (#1569 row 61) still
/// leaves one for lamp-skills.
fn answers_at_once(answer: Vec<u8>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    thread::spawn(move || {
        let until = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < until {
            match listener.accept() {
                Ok((mut socket, _)) => {
                    socket.set_nonblocking(false).unwrap();
                    let _ = socket.write_all(&answer);
                }
                Err(_) => thread::sleep(Duration::from_millis(5)),
            }
        }
    });
    port
}

/// A 200 with `body`.
fn ok(body: &[u8]) -> Vec<u8> {
    let mut answer =
        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    answer.extend_from_slice(body);
    answer
}

/// The Store at `port` over plain TCP, as if it were TLS.
struct Local(u16);

impl Channel for Local {
    fn fetch(&mut self, url: &str, most: u64, out: &mut dyn Write) -> Result<u64, Fetch> {
        let port = self.0;
        get_over(url, most, out, Duration::from_secs(5), |_, _| {
            TcpStream::connect(("127.0.0.1", port))
                .map(|socket| Box::new(socket) as Box<dyn Stream>)
                .map_err(|error| Fetch::Unreachable(error.to_string()))
        })
    }
}

const URL: &str = "https://store.example/skills/desk-tutor-1.0.0.tar";

/// `request` asked of the helper's own code over `skills`, its packages
/// from the Store at `port`, under the key at `key_file`: its answer and
/// whether it was done.
fn ask(
    skills: &Path,
    key_file: &Path,
    port: u16,
    request: serde_json::Value,
) -> (serde_json::Value, bool) {
    ask_waiting(skills, key_file, port, request, package::ASKED_LOCK_WAIT)
}

/// [`ask`], with the helper waiting at most `wait` for a held folder (the
/// lamp's `ASKED_LOCK_WAIT`).
fn ask_waiting(
    skills: &Path,
    key_file: &Path,
    port: u16,
    request: serde_json::Value,
    wait: Duration,
) -> (serde_json::Value, bool) {
    let mut channel = Local(port);
    let mut output = Vec::new();
    let done = package::asked(
        skills,
        // No skill the lamp made is asked for here.
        &skills.with_extension("made"),
        &skills.with_extension("state"),
        wait,
        Some(Store {
            key_file: key_file.to_path_buf(),
            channel: &mut channel,
        }),
        format!("{request}\n").as_bytes(),
        &mut output,
    )
    .unwrap();
    (serde_json::from_slice(&output).unwrap(), done)
}

fn install(url: &str) -> serde_json::Value {
    serde_json::json!({ "install": { "url": url } })
}

fn refused(answer: &serde_json::Value) -> &str {
    answer["refused"]
        .as_str()
        .unwrap_or_else(|| panic!("not refused: {answer}"))
}

#[test]
fn a_package_signed_with_the_store_key_installs_and_is_listed() {
    let dir = tempfile::tempdir().unwrap();
    let skills = skills(dir.path());
    let channel = key(1);
    let key_file = key_file(dir.path(), &channel);
    let bytes = packed(dir.path(), Some(&channel));
    let (port, store) = serve(vec![ok(&bytes)]);
    let (answer, done) = ask(
        &skills,
        &key_file,
        port,
        serde_json::json!({ "install": { "url": URL, "sha256": sha256_hex(&bytes) } }),
    );
    assert!(done, "{answer}");
    let said = answer["done"].as_str().unwrap();
    assert!(
        said.starts_with("installed desk-tutor 1.0.0 (installable) from its package"),
        "{said}"
    );
    let ours =
        lamp_update::manifest::fingerprint(channel.public_key().as_ref().try_into().unwrap());
    assert!(
        said.contains(&format!("signed with the Store's key {ours}")),
        "{said}"
    );
    assert_eq!(store.join().unwrap(), 1);
    assert_eq!(left(&skills), ["dance", "desk-tutor"]);
    // The skill keeps the package's sha256, the catalogue's, for the app.
    let kept = package::installed_from(&skills.join("desk-tutor")).unwrap();
    assert_eq!(kept.sha256, sha256_hex(&bytes));
    assert_eq!(kept.manifest.version, "1.0.0");
    let listed = package::list(&skills).unwrap();
    assert!(
        listed
            .lines()
            .any(|line| line.starts_with("desk-tutor") && line.contains("installable")),
        "{listed}"
    );
    // Open to the parts, which run as another user.
    let mode = fs::metadata(skills.join("desk-tutor/SKILL.md"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o644);
    // Without `sha256` too, and its next fetch replaces it whole.
    let (port, _) = serve(vec![ok(&bytes)]);
    let (answer, done) = ask(&skills, &key_file, port, install(URL));
    assert!(done, "{answer}");
    assert!(
        answer["done"]
            .as_str()
            .unwrap()
            .starts_with("replaced desk-tutor 1.0.0 with 1.0.0"),
        "{answer}"
    );
    assert_eq!(left(&skills), ["dance", "desk-tutor"]);
}

/// #3504's piece C: a package from the Store that asks the camera's
/// picture is refused, nothing written, until the owner's yes to it is
/// kept (`{"camera": NAME}`), then installs. A remove takes the yes back,
/// so the same package put on again is refused again.
#[test]
fn a_package_that_asks_the_camera_installs_only_once_the_owner_said_yes() {
    let dir = tempfile::tempdir().unwrap();
    let skills = skills(dir.path());
    let camera = skills
        .with_extension("state")
        .join(lamp_skills::CAMERA_FILE);
    fs::remove_file(&camera).unwrap();
    let channel = key(1);
    let key_file = key_file(dir.path(), &channel);
    let bytes = packed(dir.path(), Some(&channel));
    let (port, store) = serve(vec![ok(&bytes), ok(&bytes), ok(&bytes), ok(&bytes)]);
    let (answer, done) = ask(&skills, &key_file, port, install(URL));
    assert!(!done);
    assert_eq!(
        refused(&answer),
        "not installed: desk-tutor: it asks the camera, and the owner hasn't said yes"
    );
    assert_eq!(left(&skills), ["dance"]);
    let (answer, done) = ask(
        &skills,
        &key_file,
        port,
        serde_json::json!({ "camera": "desk-tutor" }),
    );
    assert!(done, "{answer}");
    let (answer, done) = ask(&skills, &key_file, port, install(URL));
    assert!(done, "{answer}");
    assert_eq!(left(&skills), ["dance", "desk-tutor"]);
    // A Store update of the same skill keeps its yes.
    let (answer, done) = ask(&skills, &key_file, port, install(URL));
    assert!(
        answer["done"]
            .as_str()
            .is_some_and(|said| said.starts_with("replaced desk-tutor")),
        "{answer}"
    );
    assert!(done);
    assert_eq!(fs::read_to_string(&camera).unwrap(), "desk-tutor\n");
    // Removed, its yes goes with it: put on again by the same name, it
    // asks again.
    let (answer, done) = ask(
        &skills,
        &key_file,
        port,
        serde_json::json!({ "remove": "desk-tutor" }),
    );
    assert!(done, "{answer}");
    assert_eq!(fs::read_to_string(&camera).unwrap(), "");
    let (answer, done) = ask(&skills, &key_file, port, install(URL));
    assert!(!done);
    assert_eq!(
        refused(&answer),
        "not installed: desk-tutor: it asks the camera, and the owner hasn't said yes"
    );
    assert_eq!(left(&skills), ["dance"]);
    assert_eq!(store.join().unwrap(), 4);
}

/// Unsigned, signed with another key, a signature that is not one, a
/// manifest changed after it was signed, a file changed, and a package
/// whose bytes are not the `sha256` asked for: each refused with the skills
/// folder made read-only, so a refusal that is not the signature's or the
/// hash's would show it reached a write.
#[test]
fn an_unsigned_wrongly_signed_or_tampered_package_is_refused_before_anything_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let skills = skills(dir.path());
    let channel = key(1);
    let key_file = key_file(dir.path(), &channel);
    let good = packed(dir.path(), Some(&channel));
    let unsigned = packed(dir.path(), None);
    let other = packed(&dir.path().join("other"), Some(&key(2)));
    let tutor = tutor(&dir.path().join("hand"));
    let skill_md = fs::read(tutor.join("SKILL.md")).unwrap();
    let toml = fs::read(tutor.join("skill.toml")).unwrap();
    let files: [(&str, &[u8]); 2] = [("SKILL.md", &skill_md), ("skill.toml", &toml)];
    let entries = || {
        vec![
            ("desk-tutor/SKILL.md", skill_md.clone(), None),
            ("desk-tutor/skill.toml", toml.clone(), None),
        ]
    };
    let signed = by_hand(&channel, "desk-tutor", &files, &entries());
    // The manifest changed after signing: one byte of it, in place.
    let mut manifest_changed = signed.clone();
    let at = manifest_changed
        .windows(7)
        .position(|window| window == b"\"1.0.0\"")
        .unwrap();
    manifest_changed[at + 1] = b'2';
    // A file changed, its manifest and signature left as they were.
    let mut file_changed = by_hand(&channel, "desk-tutor", &files, &entries());
    let at = file_changed
        .windows(skill_md.len().min(32))
        .position(|window| window == &skill_md[..skill_md.len().min(32)])
        .unwrap();
    file_changed[at] ^= 0x20;
    let garbage = {
        let manifest = serde_json::to_vec(&Manifest {
            format: package::FORMAT,
            name: "desk-tutor".into(),
            version: "1.0.0".into(),
            files: Vec::new(),
        })
        .unwrap();
        tar_of(&[
            (MANIFEST, manifest, None),
            (
                SIGNATURE,
                b"{\"key\": \"0123456789abcdef\", \"signature\": \"AAAA\"}".to_vec(),
                None,
            ),
        ])
    };
    let cases: Vec<(&str, Vec<u8>, Option<String>, &str)> = vec![
        ("unsigned", unsigned, None, "it carries no lamp-skill.sig"),
        ("another key", other, None, "signed with key "),
        (
            "not a signature",
            garbage,
            None,
            "signed with key 0123456789abcdef",
        ),
        (
            "manifest changed",
            manifest_changed,
            None,
            "lamp-skill.sig does not match lamp-skill.json",
        ),
        (
            "a file changed",
            file_changed,
            None,
            "its size or sha256 is not what lamp-skill.json says",
        ),
        (
            "not the sha256 asked",
            good.clone(),
            Some("0".repeat(64)),
            "the package's sha256 is not the one asked for",
        ),
    ];
    fs::set_permissions(&skills, fs::Permissions::from_mode(0o555)).unwrap();
    for (what, bytes, sha256, why) in cases {
        let (port, _) = serve(vec![ok(&bytes)]);
        let request = match sha256 {
            None => install(URL),
            Some(sha256) => serde_json::json!({ "install": { "url": URL, "sha256": sha256 } }),
        };
        let (answer, done) = ask(&skills, &key_file, port, request);
        assert!(!done, "{what}: {answer}");
        assert!(refused(&answer).contains(why), "{what}: {answer}");
        assert!(
            refused(&answer).starts_with("not installed: "),
            "{what}: {answer}"
        );
    }
    fs::set_permissions(&skills, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(left(&skills), ["dance"]);
    // The same signed package, untouched, installs: the cases differ only
    // in what was changed.
    let (port, _) = serve(vec![ok(&signed)]);
    let (answer, done) = ask(&skills, &key_file, port, install(URL));
    assert!(done, "{answer}");
}

#[test]
fn an_over_size_package_or_a_path_trick_link_or_core_name_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let skills = skills(dir.path());
    let channel = key(1);
    let key_file = key_file(dir.path(), &channel);
    let tutor = tutor(&dir.path().join("hand"));
    let skill_md = fs::read(tutor.join("SKILL.md")).unwrap();
    let toml = fs::read(tutor.join("skill.toml")).unwrap();
    let victim = dir.path().join("victim");

    // Over the size a package may be, said by its length: not read.
    let too_big = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
        package::MOST_PACKAGE_BYTES + 1
    )
    .into_bytes();
    // And with no length, cut off as it passes the bound.
    let mut endless = b"HTTP/1.1 200 OK\r\n\r\n".to_vec();
    endless.resize(
        endless.len() + package::MOST_PACKAGE_BYTES as usize + 4096,
        b'x',
    );
    for answer in [too_big, endless] {
        let (port, _) = serve(vec![answer]);
        let (answer, done) = ask(&skills, &key_file, port, install(URL));
        assert!(!done);
        assert!(refused(&answer).contains("larger than"), "{answer}");
    }

    // Signed, and each holding an entry no skill may: a way out of its
    // folder, an absolute path, a hidden name, a link.
    let listed: [(&str, &[u8]); 2] = [("SKILL.md", &skill_md), ("skill.toml", &toml)];
    let base = || {
        vec![
            ("desk-tutor/SKILL.md", skill_md.clone(), None),
            ("desk-tutor/skill.toml", toml.clone(), None),
        ]
    };
    let mut tricks = Vec::new();
    for (entry, link) in [
        ("desk-tutor/../victim", None),
        ("desk-tutor/../../victim", None),
        ("/tmp/victim", None),
        ("desk-tutor/.hidden", None),
        ("desk-tutor/look", Some("/etc/shadow")),
        ("desk-tutor/up", Some("../../victim")),
    ] {
        let mut entries = base();
        entries.push((entry, b"x".to_vec(), link));
        tricks.push((entry, by_hand(&channel, "desk-tutor", &listed, &entries)));
    }
    // A name that clashes with the lamp's core Dance, and a package that
    // says it is core itself.
    let dance_md = String::from_utf8(skill_md.clone())
        .unwrap()
        .replacen("name: desk-tutor", "name: dance", 1)
        .into_bytes();
    let dance = by_hand(
        &channel,
        "dance",
        &[("SKILL.md", &dance_md), ("skill.toml", &toml)],
        &[
            ("dance/SKILL.md", dance_md.clone(), None),
            ("dance/skill.toml", toml.clone(), None),
        ],
    );
    let core_toml = String::from_utf8(toml.clone())
        .unwrap()
        .replacen("kind = \"installable\"", "kind = \"core\"", 1)
        .into_bytes();
    let says_core = by_hand(
        &channel,
        "desk-tutor",
        &[("SKILL.md", &skill_md), ("skill.toml", &core_toml)],
        &[
            ("desk-tutor/SKILL.md", skill_md.clone(), None),
            ("desk-tutor/skill.toml", core_toml.clone(), None),
        ],
    );
    for (what, bytes, why) in [
        (
            "a core skill's name",
            dance,
            "dance is a core skill on this lamp",
        ),
        ("says it is core", says_core, "says kind = \"core\""),
    ] {
        let (port, _) = serve(vec![ok(&bytes)]);
        let (answer, done) = ask(&skills, &key_file, port, install(URL));
        assert!(!done, "{what}: {answer}");
        assert!(refused(&answer).contains(why), "{what}: {answer}");
        assert_eq!(left(&skills), ["dance"], "{what}");
    }
    for (what, bytes) in tricks {
        let (port, _) = serve(vec![ok(&bytes)]);
        let (answer, done) = ask(&skills, &key_file, port, install(URL));
        let why = refused(&answer);
        assert!(
            why.contains("a skill's paths are at most 4 parts")
                || why.contains("a second folder beside desk-tutor")
                || why.contains("a link, a device or another entry that is not a plain file"),
            "{what}: {why}"
        );
        assert!(!done, "{what}: {answer}");
        assert!(
            refused(&answer).starts_with("not installed: "),
            "{what}: {answer}"
        );
        assert!(!victim.exists(), "{what}");
        assert_eq!(left(&skills), ["dance"], "{what}");
    }
    // Dance is the lamp's own, as it was.
    assert!(
        fs::read_to_string(skills.join("dance/skill.toml"))
            .unwrap()
            .contains("kind = \"core\"")
    );
}

#[test]
fn only_an_https_url_is_fetched_and_a_redirect_only_to_https() {
    let dir = tempfile::tempdir().unwrap();
    let skills = skills(dir.path());
    let channel = key(1);
    let key_file = key_file(dir.path(), &channel);
    let bytes = packed(dir.path(), Some(&channel));
    let (port, store) = serve(vec![ok(&bytes)]);
    let long = format!("https://store.example/{}", "a".repeat(package::MOST_URL));
    for url in [
        "http://store.example/desk-tutor.tar",
        "file:///tmp/desk-tutor.tar",
        "ftp://store.example/desk-tutor.tar",
        "https://",
        "https:///desk-tutor.tar",
        "https://someone@store.example/desk-tutor.tar",
        "https://store.example/desk tutor.tar",
        "https://store.example/desk-tutor.tar\n",
        "HTTPS://store.example/desk-tutor.tar",
        "/tmp/desk-tutor.tar",
        long.as_str(),
    ] {
        let (answer, done) = ask(&skills, &key_file, port, install(url));
        assert!(!done, "{url}");
        assert_eq!(
            refused(&answer),
            format!("not installed: {}", package::NOT_A_STORE_URL),
            "{url}"
        );
    }
    let (answer, _) = ask(
        &skills,
        &key_file,
        port,
        serde_json::json!({ "install": { "url": URL, "sha256": "ABC" } }),
    );
    assert_eq!(
        refused(&answer),
        format!("not installed: {}", package::NOT_A_SHA256)
    );
    // Not one of them reached the Store: its one answer is still there.
    let (answer, done) = ask(&skills, &key_file, port, install(URL));
    assert!(done, "{answer}");
    assert_eq!(store.join().unwrap(), 1);

    // A redirect to http, or to a file on the lamp, is not followed.
    for to in ["http://store.example/x.tar", "file:///etc/shadow"] {
        let redirect = format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\n\r\n")
            .into_bytes();
        let (port, store) = serve(vec![redirect, ok(&bytes)]);
        let (answer, done) = ask(&skills, &key_file, port, install(URL));
        assert!(!done, "{to}");
        assert!(refused(&answer).contains("is not an https URL"), "{answer}");
        assert_eq!(store.join().unwrap(), 1, "{to}");
    }
    // A request that is not an install's shape is no request.
    for request in [
        serde_json::json!({ "install": { "url": URL, "also": 1 } }),
        serde_json::json!({ "install": { "url": 7 } }),
        serde_json::json!({ "install": { "sha256": "0" } }),
        serde_json::json!({ "install": { "url": URL, "name": 7 } }),
        serde_json::json!({ "install": URL }),
        serde_json::json!({ "install": { "url": URL }, "remove": "dance" }),
    ] {
        let (answer, done) = ask(&skills, &key_file, port, request.clone());
        assert!(!done, "{request}");
        assert_eq!(refused(&answer), package::NOT_A_REQUEST, "{request}");
    }
    // No key on the lamp: nothing is fetched.
    let (port, store) = serve(vec![ok(&bytes)]);
    let (answer, done) = ask(&skills, &dir.path().join("no-key.pub"), port, install(URL));
    assert!(!done);
    assert!(refused(&answer).contains("the Store's key"), "{answer}");
    assert_eq!(store.join().unwrap(), 0);
}

/// The real `lamp-skills asked`, as systemd starts it for a connection,
/// asked straight with nobody before it: a URL that is not https, and an
/// https one whose server speaks no TLS, are refused and nothing is
/// written; with no Store's key on the lamp, nothing is fetched.
#[test]
fn the_real_helper_refuses_on_its_own() {
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().unwrap();
    let skills = skills(dir.path());
    let key_file = key_file(dir.path(), &key(1));
    let ask_binary = |line: String, store: Option<&str>| {
        let (mut ours, theirs) = UnixStream::pair().unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-skills"));
        command.env_remove(package::STORE_SWITCH);
        command.env(package::ANY_ASKER, "on");
        if let Some(store) = store {
            command.env(package::STORE_SWITCH, store);
        }
        let mut child = command
            .args(["asked", "--dir"])
            .arg(&skills)
            .arg("--key-file")
            .arg(&key_file)
            .stdin(Stdio::from(std::os::fd::OwnedFd::from(
                theirs.try_clone().unwrap(),
            )))
            .stdout(Stdio::from(std::os::fd::OwnedFd::from(theirs)))
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // The command holds the child's ends until it is dropped: drop it,
        // or the read below never sees the end.
        drop(command);
        ours.write_all(line.as_bytes()).unwrap();
        ours.shutdown(std::net::Shutdown::Write).unwrap();
        let mut answer = String::new();
        ours.read_to_string(&mut answer).unwrap();
        let status = child.wait().unwrap();
        (
            serde_json::from_str::<serde_json::Value>(answer.trim()).unwrap(),
            status.success(),
        )
    };
    let (answer, success) = ask_binary(
        format!("{}\n", install("http://127.0.0.1/desk-tutor.tar")),
        Some("on"),
    );
    assert!(!success);
    assert_eq!(
        refused(&answer),
        format!("not installed: {}", package::NOT_A_STORE_URL)
    );
    // A server that answers in plain HTTP where TLS is spoken.
    let port = answers_at_once(ok(b"not a package"));
    let (answer, success) = ask_binary(
        format!(
            "{}\n",
            install(&format!("https://127.0.0.1:{port}/desk-tutor.tar"))
        ),
        Some("on"),
    );
    assert!(!success);
    assert!(
        refused(&answer).starts_with("not installed: the package was not fetched"),
        "{answer}"
    );
    assert_eq!(left(&skills), ["dance"]);
    // A missing or malformed Store key keeps the real helper off even
    // with its switch on. The update key is never substituted.
    let saved_key = fs::read(&key_file).unwrap();
    let (port, served) = serve(vec![ok(b"never asked for")]);
    for malformed in [None, Some("not a key")] {
        if let Some(text) = malformed {
            fs::write(&key_file, text).unwrap();
        } else {
            fs::remove_file(&key_file).unwrap();
        }
        let (answer, success) = ask_binary(
            format!("{}\n", install(&format!("https://127.0.0.1:{port}/x.tar"))),
            Some("on"),
        );
        assert!(!success);
        assert_eq!(refused(&answer), package::STORE_OFF);
    }
    assert_eq!(served.join().unwrap(), 0);
    // Removal still works with an invalid key and Store explicitly on.
    package::install(&skills, None, &tutor(&dir.path().join("keyless-remove"))).unwrap();
    let (answer, success) = ask_binary(
        format!("{}\n", serde_json::json!({ "remove": "desk-tutor" })),
        Some("on"),
    );
    assert!(success, "{answer}");
    assert_eq!(left(&skills), ["dance"]);
    fs::write(&key_file, saved_key).unwrap();
    // The Store's switch off in the helper's own environment (unset, or
    // anything but `on`): an install is refused as `lamp-cloud` refuses
    // it, whoever reached the socket, and nothing is fetched; a remove
    // still runs (the reviewer-lead on #1983).
    let (port, served) = serve(vec![ok(b"never asked for")]);
    for store in [None, Some("off"), Some("ON"), Some("")] {
        let (answer, success) = ask_binary(
            format!(
                "{}\n",
                install(&format!("https://127.0.0.1:{port}/desk-tutor.tar"))
            ),
            store,
        );
        assert!(!success, "{store:?}");
        assert_eq!(refused(&answer), package::STORE_OFF, "{store:?}");
    }
    assert_eq!(served.join().unwrap(), 0);
    package::install(&skills, None, &tutor(&dir.path().join("by-hand"))).unwrap();
    assert_eq!(left(&skills), ["dance", "desk-tutor"]);
    let (answer, success) = ask_binary(
        format!("{}\n", serde_json::json!({ "remove": "desk-tutor" })),
        None,
    );
    assert!(success, "{answer}");
    assert_eq!(left(&skills), ["dance"]);
}

/// Only the key in the Store's own file is taken: a package signed with the
/// update key is refused as any other key's, and a new `store-key.pub` (a
/// rotation) moves which key is taken at the next install, nothing
/// restarted.
#[test]
fn only_the_store_key_in_its_file_is_taken_and_a_new_file_rotates_it() {
    let dir = tempfile::tempdir().unwrap();
    let skills = skills(dir.path());
    let (update, old, new) = (key(1), key(2), key(3));
    let fingerprint = |key: &Ed25519KeyPair| {
        lamp_update::manifest::fingerprint(key.public_key().as_ref().try_into().unwrap())
    };
    let store_file = key_file(dir.path(), &old);
    let install_signed = |key: &Ed25519KeyPair, at: &str| {
        let bytes = packed(&dir.path().join(at), Some(key));
        let (port, _) = serve(vec![ok(&bytes)]);
        ask(&skills, &store_file, port, install(URL))
    };
    let refused_as_other =
        |answer: serde_json::Value, done: bool, signer: &Ed25519KeyPair, store: &Ed25519KeyPair| {
            assert!(!done, "{answer}");
            assert!(
                refused(&answer).contains(&format!(
                    "signed with key {}, and this lamp takes skills signed with the Store's key {}",
                    fingerprint(signer),
                    fingerprint(store)
                )),
                "{answer}"
            );
        };
    let (answer, done) = install_signed(&update, "update-1");
    refused_as_other(answer, done, &update, &old);
    assert_eq!(left(&skills), ["dance"]);
    let (answer, done) = install_signed(&old, "old-1");
    assert!(done, "{answer}");
    // The Store's key rotated: a new file in the same place.
    assert_eq!(key_file(dir.path(), &new), store_file);
    let (answer, done) = install_signed(&old, "old-2");
    refused_as_other(answer, done, &old, &new);
    let (answer, done) = install_signed(&update, "update-2");
    refused_as_other(answer, done, &update, &new);
    let (answer, done) = install_signed(&new, "new");
    assert!(done, "{answer}");
    assert!(
        answer["done"].as_str().unwrap().contains(&format!(
            "signed with the Store's key {}",
            fingerprint(&new)
        )),
        "{answer}"
    );
}

/// The Store's key as a build ships it (#3692): `store-key.pub` in the
/// skills folder wins over the factory's file; a key file of two lines (a
/// rotation) takes either key, by the signature's fingerprint, and no
/// third; the update key is never taken; a damaged one refuses every
/// install and never falls back to the factory's key. The file is no
/// skill: the loader and the list pass over it, and no install, remove or
/// sweep of the manage helper takes it away.
#[test]
fn a_builds_key_in_the_skills_folder_wins_and_rotates_and_a_damaged_one_refuses() {
    use base64::Engine;
    let dir = tempfile::tempdir().unwrap();
    let skills = skills(dir.path());
    let (update, factory, new, newer, other) = (key(1), key(2), key(3), key(4), key(5));
    let line = |key: &Ed25519KeyPair| {
        format!(
            "{}\n",
            base64::engine::general_purpose::STANDARD.encode(key.public_key().as_ref())
        )
    };
    let fingerprint = |key: &Ed25519KeyPair| {
        lamp_update::manifest::fingerprint(key.public_key().as_ref().try_into().unwrap())
    };
    let factory_file = key_file(dir.path(), &factory);
    let build_file = skills.join(package::BUILD_STORE_KEY);
    let mut at = 0;
    let mut install_signed = |key: &Ed25519KeyPair| {
        at += 1;
        let bytes = packed(&dir.path().join(format!("at-{at}")), Some(key));
        let (port, _) = serve(vec![ok(&bytes)]);
        ask(&skills, &factory_file, port, install(URL))
    };
    let taken = |(answer, done): (serde_json::Value, bool), key: &Ed25519KeyPair| {
        assert!(done, "{answer}");
        assert!(
            answer["done"]
                .as_str()
                .unwrap()
                .contains(&format!("signed with the Store's key {}", fingerprint(key))),
            "{answer}"
        );
    };
    let refused_as = |(answer, done): (serde_json::Value, bool), said: &str| {
        assert!(!done, "{answer}");
        assert!(refused(&answer).contains(said), "{answer}");
    };
    let ready = || package::store_ready(Some("on"), &skills, &factory_file);

    // No build's key: the factory's is used.
    assert!(ready());
    taken(install_signed(&factory), &factory);
    // A build's key wins: the factory's is refused, the build's taken.
    fs::write(&build_file, line(&new)).unwrap();
    refused_as(
        install_signed(&factory),
        &format!(
            "signed with key {}, and this lamp takes skills signed with the Store's key {}",
            fingerprint(&factory),
            fingerprint(&new)
        ),
    );
    taken(install_signed(&new), &new);
    // A rotation: two lines, either key taken by its fingerprint, no third,
    // and never the update key.
    fs::write(&build_file, format!("{}{}", line(&newer), line(&new))).unwrap();
    taken(install_signed(&new), &new);
    taken(install_signed(&newer), &newer);
    let both = format!(
        "the Store's key {} or {}",
        fingerprint(&newer),
        fingerprint(&new)
    );
    refused_as(install_signed(&other), &both);
    refused_as(install_signed(&update), &both);
    refused_as(install_signed(&factory), &both);
    // A damaged build's key refuses every install, the factory's key's
    // among them, and is said with its name.
    for damaged in [
        String::new(),
        "not a key\n".to_owned(),
        format!(" {}", line(&new)),
        format!("{}\n{}", line(&new), line(&newer)),
        format!("{}{}{}", line(&new), line(&newer), line(&other)),
        line(&new).replace('=', ""),
    ] {
        fs::write(&build_file, &damaged).unwrap();
        assert!(!ready(), "{damaged:?}");
        for signer in [&factory, &new] {
            let said = format!(
                "the Store's key ({})",
                skills.join(package::BUILD_STORE_KEY).display()
            );
            let answer = install_signed(signer);
            refused_as(answer.clone(), &said);
            refused_as(answer, "the factory's key is not taken in its place");
        }
    }
    // No skill: the loader, the list and the manage helper pass it over.
    fs::write(&build_file, line(&new)).unwrap();
    assert!(
        lamp_skills::load_each(&skills)
            .iter()
            .all(|(folder, _)| !folder.ends_with(package::BUILD_STORE_KEY))
    );
    let listed = package::list(&skills).unwrap();
    assert!(!listed.contains("store-key"), "{listed}");
    let (answer, done) = ask(
        &skills,
        &factory_file,
        0,
        serde_json::json!({ "remove": package::BUILD_STORE_KEY }),
    );
    assert!(!done, "{answer}");
    let (answer, done) = ask(
        &skills,
        &factory_file,
        0,
        serde_json::json!({ "remove": "desk-tutor" }),
    );
    assert!(done, "{answer}");
    assert_eq!(left(&skills), ["dance", "store-key.pub"]);
    assert_eq!(fs::read_to_string(&build_file).unwrap(), line(&new));
}

/// A Store package never puts a skill back to an older version, so an old
/// package the channel once signed can't be replayed onto a lamp: the
/// older is refused with the folder as it was; the same version (a
/// reinstall) and a newer one are taken (the reviewer-lead on #1983).
#[test]
fn an_older_version_is_refused_and_the_same_or_a_newer_one_taken() {
    let dir = tempfile::tempdir().unwrap();
    let skills = skills(dir.path());
    let channel = key(1);
    let key_file = key_file(dir.path(), &channel);
    let at = |version: &str| packed_at(dir.path(), &channel, version);
    let (older, same, newer) = (at("1.2.3"), at("1.10.0"), at("2.0.0"));
    let installed = |skills: &Path| {
        fs::read_to_string(skills.join("desk-tutor/skill.toml"))
            .unwrap()
            .lines()
            .find(|line| line.starts_with("version"))
            .unwrap()
            .to_owned()
    };
    let (port, _) = serve(vec![ok(&same)]);
    let (answer, done) = ask(&skills, &key_file, port, install(URL));
    assert!(done, "{answer}");
    assert_eq!(installed(&skills), "version = \"1.10.0\"");
    // Older: 1.2.3 is under 1.10.0 as numbers, though not as text.
    let (port, _) = serve(vec![ok(&older)]);
    let (answer, done) = ask(&skills, &key_file, port, install(URL));
    assert!(!done, "{answer}");
    assert_eq!(
        refused(&answer),
        "not installed: desk-tutor 1.10.0 is on this lamp, and this package is the older 1.2.3: \
         a skill is never put back to an older version"
    );
    assert_eq!(installed(&skills), "version = \"1.10.0\"");
    assert_eq!(left(&skills), ["dance", "desk-tutor"]);
    // The same: a reinstall.
    let (port, _) = serve(vec![ok(&same)]);
    let (answer, done) = ask(&skills, &key_file, port, install(URL));
    assert!(done, "{answer}");
    assert!(
        answer["done"]
            .as_str()
            .unwrap()
            .starts_with("replaced desk-tutor 1.10.0 with 1.10.0"),
        "{answer}"
    );
    // Newer.
    let (port, _) = serve(vec![ok(&newer)]);
    let (answer, done) = ask(&skills, &key_file, port, install(URL));
    assert!(done, "{answer}");
    assert!(
        answer["done"]
            .as_str()
            .unwrap()
            .starts_with("replaced desk-tutor 1.10.0 with 2.0.0"),
        "{answer}"
    );
    assert_eq!(installed(&skills), "version = \"2.0.0\"");
    // An installed one whose version can't be ordered is not replaced by a
    // Store package until the person removes it.
    let toml = fs::read_to_string(skills.join("desk-tutor/skill.toml")).unwrap();
    fs::write(
        skills.join("desk-tutor/skill.toml"),
        toml.replace("version = \"2.0.0\"", "version = \"two\""),
    )
    .unwrap();
    let (port, _) = serve(vec![ok(&newer)]);
    let (answer, done) = ask(&skills, &key_file, port, install(URL));
    assert!(!done, "{answer}");
    assert!(
        refused(&answer).contains("can't be compared with 2.0.0"),
        "{answer}"
    );
}

/// The catalogue's entry, `{name, version, sha256, url}` (the first
/// builder's 22:23 on #1722): each field checked against what is fetched,
/// the hash before the package is read, the name and version against the
/// signed manifest.
#[test]
fn a_catalogue_entry_installs_only_the_package_it_names() {
    let dir = tempfile::tempdir().unwrap();
    let skills = skills(dir.path());
    let channel = key(1);
    let key_file = key_file(dir.path(), &channel);
    let bytes = packed(dir.path(), Some(&channel));
    let entry = |name: &str, version: &str| {
        serde_json::json!({ "install": {
            "name": name, "version": version, "sha256": sha256_hex(&bytes), "url": URL,
        } })
    };
    for (request, why) in [
        (
            entry("plant-water", "1.0.0"),
            "the package is desk-tutor, not the plant-water asked for",
        ),
        (
            entry("desk-tutor", "1.0.1"),
            "the package is desk-tutor 1.0.0, not the version 1.0.1 asked for",
        ),
        (entry("Desk-Tutor", "1.0.0"), package::NOT_A_NAME),
        (entry("desk-tutor", "1.0"), package::NOT_A_VERSION),
    ] {
        let (port, _) = serve(vec![ok(&bytes)]);
        let (answer, done) = ask(&skills, &key_file, port, request);
        assert!(!done, "{answer}");
        assert_eq!(refused(&answer), format!("not installed: {why}"));
        assert_eq!(left(&skills), ["dance"]);
    }
    let (port, _) = serve(vec![ok(&bytes)]);
    let (answer, done) = ask(&skills, &key_file, port, entry("desk-tutor", "1.0.0"));
    assert!(done, "{answer}");
    assert_eq!(left(&skills), ["dance", "desk-tutor"]);
}

/// While another run holds the skills folder, an install from the Store
/// waits for it at most its wait (the lamp's `ASKED_LOCK_WAIT`, here a
/// short one) once the package is fetched and checked, then refuses busy
/// with nothing written, as a remove does; with the fetch's own bound that
/// is under `lamp-cloud`'s 40 s for an install. A hold given back within the
/// wait lets the install go on.
#[test]
fn an_install_waits_for_a_held_folder_only_so_long_then_writes_nothing() {
    use std::time::Instant;
    let dir = tempfile::tempdir().unwrap();
    let skills = skills(dir.path());
    let channel = key(1);
    let key_file = key_file(dir.path(), &channel);
    let bytes = packed(dir.path(), Some(&channel));
    let held = fs::File::open(&skills).unwrap();
    held.lock().unwrap();
    let (port, _) = serve(vec![ok(&bytes)]);
    let wait = Duration::from_millis(300);
    let asked = Instant::now();
    let (answer, done) = ask_waiting(&skills, &key_file, port, install(URL), wait);
    let waited = asked.elapsed();
    assert!(!done);
    assert_eq!(refused(&answer), package::BUSY);
    assert!(
        waited >= wait && waited < wait + Duration::from_secs(1),
        "{waited:?}"
    );
    assert_eq!(left(&skills), ["dance"]);

    let giving_back = thread::spawn(move || {
        thread::sleep(Duration::from_millis(500));
        held.unlock().unwrap();
    });
    let (port, _) = serve(vec![ok(&bytes)]);
    let (answer, done) = ask(&skills, &key_file, port, install(URL));
    giving_back.join().unwrap();
    assert!(done, "{answer}");
    assert_eq!(left(&skills), ["dance", "desk-tutor"]);
}
