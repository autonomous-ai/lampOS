//! The Skills Store's lamp side (`docs/skills.md`, "From the Skills Store",
//! #1722): a package fetched from a URL by root's manage helper and
//! installed only when it is signed with the update channel's key. Every
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

/// The update channel's key, from a seed: `which` tells two keys apart.
fn key(which: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[which; 32]).unwrap()
}

/// The channel's public key written as a lamp keeps it
/// (`/etc/lampOS/update/key.pub`): 32 bytes in base64 on one line.
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

/// A skills folder with the lamp's core Dance in it.
fn skills(dir: &Path) -> PathBuf {
    let skills = dir.join("skills");
    copy(&skills_root().join("dance"), &skills.join("dance"));
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
fn serve(answers: Vec<Vec<u8>>) -> (u16, thread::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let handle = thread::spawn(move || {
        let mut taken = 0;
        let until = std::time::Instant::now() + Duration::from_secs(3);
        for answer in answers {
            let mut socket = loop {
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
                    Err(_) if std::time::Instant::now() < until => {
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
    (port, handle)
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
    let mut channel = Local(port);
    let mut output = Vec::new();
    let done = package::asked(
        skills,
        // No skill the lamp made is asked for here.
        &skills.with_extension("made"),
        package::ASKED_LOCK_WAIT,
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
fn a_package_signed_with_the_update_channels_key_installs_and_is_listed() {
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
        said.contains(&format!("signed with the update channel's key {ours}")),
        "{said}"
    );
    assert_eq!(store.join().unwrap(), 1);
    assert_eq!(left(&skills), ["dance", "desk-tutor"]);
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
    assert!(
        refused(&answer).contains("the update channel's key"),
        "{answer}"
    );
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
    let (port, _) = serve(vec![ok(b"not a package")]);
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
    package::install(&skills, &tutor(&dir.path().join("by-hand"))).unwrap();
    assert_eq!(left(&skills), ["dance", "desk-tutor"]);
    let (answer, success) = ask_binary(
        format!("{}\n", serde_json::json!({ "remove": "desk-tutor" })),
        None,
    );
    assert!(success, "{answer}");
    assert_eq!(left(&skills), ["dance"]);
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
/// waits for it at most `ASKED_LOCK_WAIT` once the package is fetched and
/// checked, then refuses busy with nothing written, as a remove does; with
/// the fetch's own bound that is under `lamp-cloud`'s 40 s for an install.
/// A hold given back within the wait lets the install go on.
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
    let asked = Instant::now();
    let (answer, done) = ask(&skills, &key_file, port, install(URL));
    let waited = asked.elapsed();
    assert!(!done);
    assert_eq!(refused(&answer), package::BUSY);
    assert!(
        waited >= package::ASKED_LOCK_WAIT && waited < Duration::from_secs(5),
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
