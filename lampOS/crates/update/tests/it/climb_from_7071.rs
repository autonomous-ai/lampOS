//! A lamp still on the 7071 image (#3689) reads every build with 7071's own
//! updater, level 5, whose rules are on lamps and can no longer change. It
//! opens the channel's envelope and, as each build needs a newer updater,
//! takes only the `lamp-update` the build carries, then that one reads the
//! rest (the climb). This publishes a build as CI does (`lampos-publish.py`:
//! `lamp-update manifest` over the repository's own deploy and skills, then
//! `lamp-update sign`) and opens it with a copy of 7071's rules, from
//! `aa9ec9e27a`'s `manifest.rs` (`signed`, `too_old`, `is_folder_url`,
//! `check_hash_and_size`). Another envelope key or format, a larger
//! envelope, a `min_updater` of 5 or less (7071 would then read the whole
//! manifest, and refuse what it doesn't know), or a carried `lamp-update`
//! 7071 cannot read would leave every 7071 lamp refusing each build instead
//! of climbing.
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::{
    digest::{SHA256, digest},
    rand::SystemRandom,
    signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey},
};
use serde::Deserialize;
use std::{fs, path::Path, process::Command};

/// 7071's `FORMAT`, `UPDATER`, `ENVELOPE`, `MOST_ENVELOPE_BYTES` and
/// `MOST_FILE_BYTES`.
const FORMAT: &str = "lampos-update/1";
const UPDATER: u64 = 5;
const ENVELOPE: &str = "lampos-manifest.json";
const MOST_ENVELOPE_BYTES: usize = 512 * 1024;
const MOST_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// What CI publishes as (`lampos-publish.py`'s `CHANNEL` and release tag).
const CHANNEL: &str = "lamp-voice";
const SEQUENCE: u64 = 9000;
const FILES_URL: &str =
    "https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice-9000/";

/// 7071's `Envelope`: a key it doesn't know is refused.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: String,
    key: String,
    manifest: String,
    signature: String,
}

/// 7071's `Needs`, read before anything else of the manifest.
#[derive(Deserialize)]
struct Needs {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    sequence: Option<u64>,
    #[serde(default)]
    min_updater: Option<u64>,
    #[serde(default)]
    files_url: Option<String>,
    #[serde(default)]
    files: Vec<serde_json::Value>,
}

/// 7071's `File`: a file with another field is not one it can take.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    name: String,
    sha256: String,
    size: u64,
}

fn sha256_hex(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 7071's `is_token`.
fn is_token(text: &str) -> bool {
    (1..=40).contains(&text.len())
        && text.as_bytes()[0].is_ascii_lowercase()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// 7071's `is_folder_url`.
fn is_folder_url(url: &str) -> bool {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("file:///"));
    rest.is_some_and(|rest| {
        url.ends_with('/')
            && !rest.contains("..")
            && !rest.contains(['?', '#', '\\', ' ', '@'])
            && rest.len() < 1024
    })
}

fn lamp_update(arguments: &[&str]) {
    let out = Command::new(env!("CARGO_BIN_EXE_lamp-update"))
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "lamp-update {}: {}{}",
        arguments[0],
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_build_published_as_ci_does_is_climbed_by_7071s_updater() {
    let dir = tempfile::tempdir().unwrap();
    let stage = dir.path().join("build");
    fs::create_dir(&stage).unwrap();
    // The binaries' bytes don't matter here, only that they are carried.
    for name in ["lamp-update", "lamp-voice"] {
        fs::write(stage.join(name), format!("{name}, built for this test\n")).unwrap();
    }
    published_and_climbed(dir.path(), &stage, FILES_URL, &[]);
}

/// Publishes the binaries in `stage` as CI does, with `more` options to
/// `lamp-update manifest`, and opens the envelope as 7071 does: it climbs.
/// The public key the build is signed for.
fn published_and_climbed(dir: &Path, stage: &Path, files_url: &str, more: &[&str]) -> [u8; 32] {
    let lamp_os = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let key_file = dir.join("signing.key");
    fs::write(&key_file, STANDARD.encode(document.as_ref())).unwrap();
    let public: [u8; 32] = Ed25519KeyPair::from_pkcs8(document.as_ref())
        .unwrap()
        .public_key()
        .as_ref()
        .try_into()
        .unwrap();
    let path = |path: &Path| path.to_str().unwrap().to_owned();
    let sequence = SEQUENCE.to_string();
    let commit = "0123456789abcdef".repeat(3)[..40].to_owned();
    lamp_update(
        &[
            &[
                "manifest",
                "--binaries",
                &path(stage),
                "--deploy",
                &path(&lamp_os.join("deploy")),
                "--skills",
                &path(&lamp_os.join("skills")),
                "--channel",
                CHANNEL,
                "--sequence",
                &sequence,
                "--commit",
                &commit,
                "--files-url",
                files_url,
            ][..],
            more,
        ]
        .concat(),
    );
    lamp_update(&[
        "sign",
        "--key-file",
        &path(&key_file),
        "--manifest",
        &path(&stage.join("manifest.json")),
        "--out",
        &path(&stage.join(ENVELOPE)),
    ]);

    // As 7071's `signed`.
    let bytes = fs::read(stage.join(ENVELOPE)).unwrap();
    assert!(
        bytes.len() <= MOST_ENVELOPE_BYTES,
        "the envelope is {} bytes, and 7071 reads one of at most {MOST_ENVELOPE_BYTES}",
        bytes.len()
    );
    let envelope: Envelope = serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("7071 cannot read the envelope: {error}"));
    assert_eq!(envelope.format, FORMAT);
    assert_eq!(envelope.key, sha256_hex(&public)[..16]);
    let manifest = STANDARD.decode(&envelope.manifest).unwrap();
    let signature = STANDARD.decode(&envelope.signature).unwrap();
    UnparsedPublicKey::new(&ED25519, public)
        .verify(&manifest, &signature)
        .expect("7071 checks the signature over the manifest's bytes");

    // As 7071's `too_old`: a build for a newer updater, and the
    // `lamp-update` it carries.
    let needs: Needs = serde_json::from_slice(&manifest).unwrap();
    let needed = needs.min_updater.expect("a min_updater");
    assert!(
        needed > UPDATER,
        "min_updater {needed}: 7071 would read the whole manifest, not climb"
    );
    assert!(needs.channel.as_deref().is_some_and(is_token));
    assert_eq!(needs.channel.as_deref(), Some(CHANNEL));
    assert_eq!(needs.sequence, Some(SEQUENCE));
    assert!(needs.files_url.as_deref().is_some_and(is_folder_url));
    assert_eq!(needs.files_url.as_deref(), Some(files_url));
    let carried: Vec<File> = needs
        .files
        .into_iter()
        .filter_map(|file| serde_json::from_value::<File>(file).ok())
        .filter(|file| file.name == "lamp-update")
        .collect();
    let [carried] = &carried[..] else {
        panic!("7071 finds {} lamp-update it can take", carried.len());
    };
    let binary = fs::read(stage.join("lamp-update")).unwrap();
    assert_eq!(carried.sha256, sha256_hex(&binary));
    assert_eq!(carried.size, binary.len() as u64);
    assert!((1..=MOST_FILE_BYTES).contains(&carried.size));
    public
}

/// The far jump (#5012): a lamp still on the 7071 image, its settings
/// writer that image's updater, meets first a build published as CI does
/// that needs updater 10 and writes `store.env` (the lamp's is install's,
/// off). 7071 climbs to the `lamp-update` the build carries; that one
/// confirms itself by a dry round, the same round `check` runs, and it must
/// end without waiting for the writer, which only a round that applies
/// refreshes. Here that `check` is run with the built `lamp-update` from its
/// folder on a lamp's tree.
#[test]
fn the_updater_7071_climbs_to_for_a_build_writing_store_env_is_not_held_for_the_writer() {
    let dir = tempfile::tempdir().unwrap();
    let stage = dir.path().join("build");
    fs::create_dir(&stage).unwrap();
    let binary = fs::read(env!("CARGO_BIN_EXE_lamp-update")).unwrap();
    fs::write(stage.join("lamp-update"), &binary).unwrap();
    fs::write(
        stage.join("lamp-voice"),
        "lamp-voice, built for this test\n",
    )
    .unwrap();
    let files_url = format!("file://{}/", stage.display());
    let public = published_and_climbed(dir.path(), &stage, &files_url, &["--min-updater", "10"]);
    let manifest: serde_json::Value = serde_json::from_slice(
        &STANDARD
            .decode(
                serde_json::from_slice::<Envelope>(&fs::read(stage.join(ENVELOPE)).unwrap())
                    .unwrap()
                    .manifest,
            )
            .unwrap(),
    )
    .unwrap();
    const STORE_ENV: &str = "/etc/lampOS/skills/store.env";
    let carried = manifest["settings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|setting| setting["path"] == STORE_ENV)
        .expect("the build carries store.env");
    let off = "LAMP_SKILLS_STORE=off\n";
    assert_ne!(carried["text"], off);

    // The lamp: the climbed lamp-update in its folder, the older writer,
    // and install's store.env, recorded.
    let root = dir.path().join("lamp");
    let put = |path: &str, bytes: &[u8]| {
        let at = root.join(path.trim_start_matches('/'));
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(at, bytes).unwrap();
    };
    let folder = format!("/opt/lampOS/update/{}", sha256_hex(&binary));
    let climbed = root.join(format!("{}/lamp-update", folder.trim_start_matches('/')));
    put(&format!("{folder}/lamp-update"), &binary);
    fs::set_permissions(
        &climbed,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    put(
        "/usr/local/lib/lampos/lamp-update-back",
        b"lamp-update of the 7071 image",
    );
    put(STORE_ENV, off.as_bytes());
    fs::create_dir_all(root.join("etc/systemd/system")).unwrap();
    let state = serde_json::json!({
        "installed": {"sequence": 1, "commit": format!("{:040}", 1)},
        "previous": null,
        "refused": [],
        "made": [],
        "settings": {STORE_ENV: sha256_hex(off.as_bytes())},
    });
    put(
        "/var/lib/lampos-update/state.json",
        state.to_string().as_bytes(),
    );
    put(
        "/etc/lampOS/update/key.pub",
        STANDARD.encode(public).as_bytes(),
    );
    let channel = dir.path().join("channel");
    fs::create_dir(&channel).unwrap();
    fs::copy(stage.join(ENVELOPE), channel.join(ENVELOPE)).unwrap();

    let out = Command::new(&climbed)
        .args([
            "check",
            "--channel-url",
            &format!("file://{}/", channel.display()),
            "--lamp",
            "lamp-4ace",
            "--root",
            root.to_str().unwrap(),
        ])
        .env("LAMP_UPDATE_SELF", "on")
        .env("LAMP_UPDATE_APPLY", "on")
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success() && !said.contains("settings writer is older"),
        "{:?}: {said}",
        out.status
    );
}
