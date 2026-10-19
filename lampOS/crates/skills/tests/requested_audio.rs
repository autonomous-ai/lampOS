//! Private authorization is deterministic; no microphone, output or sleeps.
use lamp_msgs::{
    REQUESTED_AUDIO_LEASE_US, REQUESTED_AUDIO_START_US, RequestedAudioContext,
    RequestedAudioOperation, RequestedAudioPhase,
};
use lamp_skills::{
    Engine, load, load_all, load_each, package,
    requested_audio::{Authority, MOST_OFFERS, SCAN_FRESH_US, enabled},
    tools_file_with_requested_audio,
};
use serde_json::json;
use std::{fs, path::Path};

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const TOML: &str = r#"
kind = "installable"
version = "1.0.0"
tool = "play_fixture"
[permissions]
speak = true
[[do]]
say = "Say briefly that you will play the requested sound."
[[do]]
after = "say"
requested_audio = "sounds/fixture.wav"
"#;

fn wav(path: &Path, value: i16) {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 24_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..240 {
        writer.write_sample(value).unwrap();
    }
    writer.finalize().unwrap();
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let skill = dir.path().join("audio-fixture");
    fs::create_dir_all(skill.join("sounds")).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: audio-fixture\ndescription: Play the requested local fixture.\n---\n",
    )
    .unwrap();
    fs::write(skill.join("skill.toml"), TOML).unwrap();
    wav(&skill.join("sounds/fixture.wav"), 100);
    (dir, skill)
}

fn context(call: u64, issued_us: u64) -> RequestedAudioContext {
    RequestedAudioContext {
        worker_boot: "voice-fixture".into(),
        provider_session: 1,
        request: 1,
        privacy_generation: 0,
        call_id: format!("call-{call}"),
        issued_us,
    }
}

fn authority(path: &Path, at: u64) -> Authority {
    let skill = load(path).unwrap();
    let mut authority = Authority::new(true, "skills-fixture".into());
    authority.replace(&[skill], at).unwrap();
    authority
}

#[test]
fn validates_and_packs_while_off_but_never_advertises_or_runs_media() {
    let (dir, path) = fixture();
    let skill = load(&path).unwrap();
    assert!(skill.requested_audio().is_some());
    assert!(
        tools_file_with_requested_audio(std::slice::from_ref(&skill), false)["tools"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        tools_file_with_requested_audio(std::slice::from_ref(&skill), true)["tools"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut engine = Engine::new(vec![skill.clone()], true);
    assert!(engine.tool("play_fixture", &json!({}), 1).is_err());
    assert!(
        engine
            .tool_with_requested_audio("play_fixture", &json!({}), 1, true)
            .is_ok()
    );
    assert!(
        engine
            .tool_with_requested_audio(
                "play_fixture",
                &json!({"requested_audio_context":context(1, 1)}),
                1,
                true
            )
            .is_err()
    );
    let mut off = Authority::new(false, "off".into());
    off.replace(&[skill], 1).unwrap();
    assert!(off.offer("audio-fixture", context(1, 1), 1).is_err());
    let output = dir.path().join("packages");
    fs::create_dir(&output).unwrap();
    assert!(package::pack(&path, &output).is_ok());
    assert!(enabled(Some("on")));
    for value in [None, Some("off"), Some("true"), Some("ON"), Some("on ")] {
        assert!(!enabled(value));
    }
}

#[test]
fn catalog_body_metadata_follows_filtered_duplicate_tool_winners() {
    let (dir, path) = fixture();
    // An unused body grant is legal, and must still restrict this tool
    // when advertised. It must not escape the media-off filter.
    fs::write(
        path.join("skill.toml"),
        TOML.replace("speak = true", "speak = true\nbody = true"),
    )
    .unwrap();
    let media = load(&path).unwrap();
    let ordinary = |name: &str, tool: &str, body: bool, enabled: bool| {
        let path = dir.path().join(name);
        fs::create_dir(&path).unwrap();
        fs::write(
            path.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {name}\n---\n"),
        )
        .unwrap();
        let action = if body {
            "body = { gesture = \"stretching\" }"
        } else {
            "say = \"Answer briefly.\""
        };
        fs::write(
            path.join("skill.toml"),
            format!(
                "kind = \"installable\"\nversion = \"1.0.0\"\ntool = \"{tool}\"\nenabled = {enabled}\n[permissions]\nspeak = true\nbody = {body}\n[[do]]\n{action}\n"
            ),
        )
        .unwrap();
        load(&path).unwrap()
    };
    // load_each rejects duplicate names. The public catalogue helper can
    // also receive a caller's list, where the first emitted tool wins.
    let inventory = [
        ordinary("disabled-body", "disabled_stretch", true, false),
        ordinary("quiet-first", "quiet_note", false, true),
        ordinary("body-shadowed", "quiet_note", true, true),
        media,
        ordinary("media-fallback", "play_fixture", false, true),
        ordinary("body-first", "small_stretch", true, true),
        ordinary("quiet-shadowed", "small_stretch", false, true),
    ];
    assert_eq!(
        tools_file_with_requested_audio(&inventory[..4], false),
        json!({"tools":[{"name":"quiet_note","description":"quiet-first"}]}),
        "disabled, shadowed and media-off definitions cannot add body metadata"
    );
    assert_eq!(
        tools_file_with_requested_audio(&inventory, false),
        json!({
            "tools":[
                {"name":"quiet_note","description":"quiet-first"},
                {"name":"play_fixture","description":"media-fallback"},
                {"name":"small_stretch","description":"body-first"}
            ],
            "body":["small_stretch"]
        }),
        "media off selects the ordinary fallback without inheriting the omitted grant"
    );
    assert_eq!(
        tools_file_with_requested_audio(&inventory, true),
        json!({
            "tools":[
                {"name":"quiet_note","description":"quiet-first"},
                {"name":"play_fixture","description":"Play the requested local fixture."},
                {"name":"small_stretch","description":"body-first"}
            ],
            "body":["play_fixture", "small_stretch"]
        }),
        "a later duplicate cannot add or remove the winning definition's body grant"
    );
}

#[test]
fn refuses_every_ungranted_or_non_tool_shape_and_unknown_values() {
    let (_dir, path) = fixture();
    let variants = [
        TOML.replace("speak = true", "speak = false"),
        TOML.replace("tool = \"play_fixture\"", ""),
        TOML.replace("after = \"say\"", "after = \"later\""),
        TOML.replace("after = \"say\"", ""),
        TOML.replace("[[do]]\nsay", "[[do]]\nlook = true\nsay"),
        TOML.replace(
            "requested_audio =",
            "light = {colour = \"warm\"}\nrequested_audio =",
        ),
        format!("{TOML}\n[[do]]\nsay = \"another line\"\n"),
        format!("{TOML}\n[trigger]\ntopic = \"fixture/value\"\n[ask]\npolicy = \"hello\"\n"),
        TOML.replace("kind = \"installable\"", "kind = \"future_kind\""),
        TOML.replace(
            "say = \"Say briefly that you will play the requested sound.\"",
            "say = { from = \"future\" }",
        ),
        TOML.replace(
            "requested_audio =",
            "future_action = true\nrequested_audio =",
        ),
        TOML.replace(
            "Say briefly that you will play the requested sound.",
            &"x".repeat(241),
        ),
    ];
    for variant in variants {
        fs::write(path.join("skill.toml"), &variant).unwrap();
        assert!(load(&path).is_err(), "accepted {variant}");
    }
}

#[test]
fn requested_audio_refuses_fully_granted_limits_and_routine_steps() {
    let (_dir, path) = fixture();
    let granted = TOML.replace(
        "speak = true",
        "speak = true\nbody = true\noutput_limits = true\ntopics = [\"voice/state\"]",
    );
    let limited = format!(
        "{granted}\n[limits]\nduring = {{ from = \"22:00\", to = \"06:00\" }}\nlight_channel_max = 24\n"
    );
    // This is otherwise a valid, fully granted cap declaration. The media
    // shape must reject it before the runtime can install automatic caps.
    let control = limited.replace(
        "[[do]]\nafter = \"say\"\nrequested_audio = \"sounds/fixture.wav\"\n",
        "",
    );
    fs::write(path.join("skill.toml"), control).unwrap();
    assert!(load(&path).unwrap().limits.is_some());
    let mut variants = vec![limited];
    for step in ["say =", "requested_audio ="] {
        for extra in ["cue = \"hum\"", "rest = { op = \"cancel\" }"] {
            variants.push(granted.replace(step, &format!("{extra}\n{step}")));
        }
    }
    for variant in variants {
        fs::write(path.join("skill.toml"), &variant).unwrap();
        let why = load(&path).unwrap_err();
        assert!(
            why.starts_with("requested_audio needs a tool alone"),
            "{variant}: {why}"
        );
    }
}

#[test]
fn requested_audio_refuses_fully_granted_shows_and_choice_steps() {
    let (_dir, path) = fixture();
    let granted = TOML.replace("speak = true", "speak = true\nlight = true");
    fs::create_dir(path.join("shows")).unwrap();
    fs::write(
        path.join("shows/slow.toml"),
        "[[light]]\ncolour = \"warm\"\nlevel = 2\nfor = \"2s\"\n\n[[light]]\ncolour = \"blue\"\nlevel = 2\nfor = \"2s\"\n",
    )
    .unwrap();
    let choice = "[choice]\nname = \"scene\"\nabout = \"The requested show.\"\none_of = [\"calm\"]\ndefault = \"calm\"\n";
    // The new scene fields are valid with their grants: the default choice
    // selects this show after its ordinary confirmation, with media off.
    let control = granted
        .replace("[permissions]", &format!("{choice}[permissions]"))
        .replace(
            "requested_audio = \"sounds/fixture.wav\"",
            "if_choice = [\"calm\"]\nshow = \"slow\"",
        );
    fs::write(path.join("skill.toml"), control).unwrap();
    let show = load(&path).unwrap();
    assert!(show.requested_audio().is_none());
    let calls = Engine::new(vec![show], true)
        .tool("play_fixture", &json!({}), 1)
        .unwrap()
        .calls;
    assert!(calls.iter().any(|call| {
        matches!(call, lamp_skills::Call::Show { name, frames } if name == "slow" && frames.len() == 2)
    }));
    // Neither step of the exact media form may inherit a show or a choice
    // selector. In particular, a valid show must not be accepted and then
    // silently skipped by the runtime's private-offer branch.
    for step in ["say =", "requested_audio ="] {
        for extra in ["show = \"slow\"", "if_choice = [\"calm\"]"] {
            let mut variant = granted.replace(step, &format!("{extra}\n{step}"));
            if extra.starts_with("if_choice") {
                variant = variant.replace("[permissions]", &format!("{choice}[permissions]"));
            }
            fs::write(path.join("skill.toml"), &variant).unwrap();
            let why = load(&path).unwrap_err();
            assert!(
                why.starts_with("requested_audio needs a tool alone"),
                "{variant}: {why}"
            );
        }
    }
}

#[test]
fn disabled_media_reserves_its_tool_without_advertisement_or_authority() {
    let (dir, path) = fixture();
    let live = load(&path).unwrap();
    fs::write(path.join("skill.toml"), format!("enabled = false\n{TOML}")).unwrap();
    let disabled = load(&path).unwrap();
    assert!(!disabled.enabled);
    let inventory = load_each(dir.path());
    assert_eq!(inventory.len(), 1);
    assert!(!inventory[0].1.as_ref().unwrap().enabled);
    let (loaded, errors) = load_all(dir.path());
    assert!(loaded.is_empty() && errors.is_empty(), "{errors:?}");
    let listed = package::list(dir.path()).unwrap();
    assert!(
        listed
            .lines()
            .any(|line| { line.starts_with("audio-fixture ") && line.contains("disabled;") }),
        "{listed}"
    );
    assert!(listed.ends_with("The skills here give the model no function.\n"));
    for enabled in [false, true] {
        assert!(
            tools_file_with_requested_audio(std::slice::from_ref(&disabled), enabled)["tools"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    let mut engine = Engine::new(vec![live.clone()], true);
    assert!(
        engine
            .tool_with_requested_audio("play_fixture", &json!({}), 1, true)
            .is_ok()
    );
    engine.replace(vec![disabled.clone()]);
    assert!(
        engine
            .tool_with_requested_audio("play_fixture", &json!({}), 2, true)
            .is_err()
    );
    let mut authority = Authority::new(true, "skills-fixture".into());
    authority.replace(&[live], 1).unwrap();
    let offer = authority.offer("audio-fixture", context(1, 1), 1).unwrap();
    // Exercise the public authority directly, even if its caller supplies
    // the complete inventory instead of load_all's enabled definitions.
    authority.replace(&[disabled], 2).unwrap();
    assert!(
        authority
            .authorize(&offer, RequestedAudioPhase::Start, 2)
            .is_err()
    );
    assert!(authority.offer("audio-fixture", context(2, 2), 2).is_err());

    let other = tempfile::tempdir().unwrap();
    let package_out = other.path().join("packed");
    fs::create_dir(&package_out).unwrap();
    assert!(package::pack(&path, &package_out).is_ok());
    let collision = other.path().join("audio-copy");
    fs::create_dir(&collision).unwrap();
    fs::write(
        collision.join("SKILL.md"),
        "---\nname: audio-copy\ndescription: Check the disabled tool reservation.\n---\n",
    )
    .unwrap();
    fs::write(
        collision.join("skill.toml"),
        "kind = \"installable\"\nversion = \"1.0.0\"\ntool = \"play_fixture\"\n[permissions]\nspeak = true\n[[do]]\nsay = \"Answer briefly.\"\n",
    )
    .unwrap();
    let why = package::install(dir.path(), &collision).unwrap_err();
    assert!(
        why.contains("the installable skill audio-fixture's"),
        "{why}"
    );
}

#[test]
fn paths_links_and_modified_assets_are_refused() {
    let (_dir, path) = fixture();
    let original = load(&path).unwrap();
    for relative in [
        "../escape.wav",
        "/tmp/escape.wav",
        "sounds/../fixture.wav",
        "sounds/.hidden.wav",
        "fixture.mp3",
    ] {
        fs::write(
            path.join("skill.toml"),
            TOML.replace("sounds/fixture.wav", relative),
        )
        .unwrap();
        assert!(load(&path).is_err(), "accepted {relative}");
    }
    fs::write(path.join("skill.toml"), TOML).unwrap();
    assert!(original.requested_audio().unwrap().unchanged());
    // Same length, same valid amplitude: actual bytes still invalidate it.
    wav(&path.join("sounds/fixture.wav"), 101);
    assert!(!original.requested_audio().unwrap().unchanged());
    fs::rename(
        path.join("sounds/fixture.wav"),
        path.join("sounds/real.wav"),
    )
    .unwrap();
    std::os::unix::fs::symlink("real.wav", path.join("sounds/fixture.wav")).unwrap();
    assert!(load(&path).is_err());
}

#[test]
fn exact_context_offer_and_unknown_private_enums_are_checked() {
    let (_dir, path) = fixture();
    let mut authority = authority(&path, 1_000_000);
    let offer = authority
        .offer("audio-fixture", context(1, 1_000_000), 1_000_001)
        .unwrap();
    let op = json!({"op":"audio_authorize","offer":offer,"phase":"start"});
    let parsed: RequestedAudioOperation = serde_json::from_value(op.clone()).unwrap();
    parsed.check().unwrap();
    for (field, value) in [
        ("phase", json!("future")),
        ("op", json!("audio_future")),
        ("extra", json!(true)),
    ] {
        let mut wrong = op.clone();
        wrong[field] = value;
        assert!(serde_json::from_value::<RequestedAudioOperation>(wrong).is_err());
    }
    assert!(
        serde_json::from_value::<RequestedAudioOperation>(
            json!({"op":"audio_finish","offer":offer,"outcome":"future"})
        )
        .is_err()
    );
    let mut wrong = offer.clone();
    wrong.context.privacy_generation += 1;
    assert!(
        authority
            .authorize(&wrong, RequestedAudioPhase::Start, 1_000_002)
            .is_err()
    );
    let mut wrong = offer.clone();
    wrong.sha256 = "0".repeat(64);
    assert!(
        authority
            .authorize(&wrong, RequestedAudioPhase::Start, 1_000_002)
            .is_err()
    );
    authority
        .authorize(&offer, RequestedAudioPhase::Start, 1_000_002)
        .unwrap()
        .check(&offer, 1_000_002)
        .unwrap();
}

#[test]
fn renewal_never_revives_a_lease_and_terminal_call_cannot_be_reissued() {
    let (_dir, path) = fixture();
    let mut authority = authority(&path, 1_000_000);
    let original = context(1, 1_000_000);
    let offer = authority
        .offer("audio-fixture", original.clone(), 1_000_000)
        .unwrap();
    let lease = authority
        .authorize(&offer, RequestedAudioPhase::Start, 1_000_001)
        .unwrap();
    let lease = authority
        .authorize(&offer, RequestedAudioPhase::Renew, lease.until_us - 1)
        .unwrap();
    assert!(
        authority
            .authorize(&offer, RequestedAudioPhase::Renew, lease.until_us)
            .is_err()
    );
    assert!(
        authority
            .authorize(&offer, RequestedAudioPhase::Start, lease.until_us)
            .is_err()
    );
    let mut changed = original;
    changed.issued_us += 1;
    changed.privacy_generation += 1;
    assert!(
        authority
            .offer("audio-fixture", changed, 1_000_001)
            .is_err()
    );
    assert!(
        authority
            .offer("audio-fixture", context(2, 1_000_000), 1_000_002)
            .is_ok()
    );
}

#[test]
fn a_second_start_and_finish_are_terminal() {
    let (_dir, path) = fixture();
    let mut authority = authority(&path, 1);
    let offer = authority.offer("audio-fixture", context(1, 1), 1).unwrap();
    authority
        .authorize(&offer, RequestedAudioPhase::Start, 1)
        .unwrap();
    assert!(
        authority
            .authorize(&offer, RequestedAudioPhase::Start, 2)
            .is_err()
    );
    assert!(
        authority
            .authorize(&offer, RequestedAudioPhase::Renew, 3)
            .is_err()
    );
    let offer = authority.offer("audio-fixture", context(2, 1), 1).unwrap();
    authority.finish(&offer).unwrap();
    assert!(
        authority
            .authorize(&offer, RequestedAudioPhase::Start, 2)
            .is_err()
    );
    assert!(authority.finish(&offer).is_err());
}

#[test]
fn original_start_and_finish_boundaries_never_move() {
    let (_dir, path) = fixture();
    let mut authority = authority(&path, 1);
    assert!(authority.offer("audio-fixture", context(1, 2), 1).is_err());
    let offer = authority.offer("audio-fixture", context(1, 1), 1).unwrap();
    let scan = authority.scan();
    authority.scanned(&scan, true, offer.start_before_us);
    assert!(
        authority
            .authorize(&offer, RequestedAudioPhase::Start, offer.start_before_us)
            .is_err()
    );
    assert!(
        authority
            .offer("audio-fixture", context(2, 1), 1 + REQUESTED_AUDIO_START_US)
            .is_err()
    );
    let start = offer.start_before_us + 1;
    authority.scanned(&scan, true, start);
    let offer = authority
        .offer("audio-fixture", context(3, start), start)
        .unwrap();
    let original = (
        offer.issued_us,
        offer.start_before_us,
        offer.finish_before_us,
    );
    authority
        .authorize(&offer, RequestedAudioPhase::Start, start)
        .unwrap();
    let mut now = start + REQUESTED_AUDIO_LEASE_US / 2;
    while now < offer.finish_before_us {
        authority.scanned(&scan, true, now);
        let lease = authority
            .authorize(&offer, RequestedAudioPhase::Renew, now)
            .unwrap();
        assert!(lease.until_us <= offer.finish_before_us);
        now += REQUESTED_AUDIO_LEASE_US / 2;
    }
    authority.scanned(&scan, true, offer.finish_before_us);
    assert!(
        authority
            .authorize(&offer, RequestedAudioPhase::Renew, offer.finish_before_us)
            .is_err()
    );
    assert_eq!(
        original,
        (
            offer.issued_us,
            offer.start_before_us,
            offer.finish_before_us
        )
    );
}

#[test]
fn replacement_revocation_restart_and_stale_scans_fail_quiet() {
    let (_dir, path) = fixture();
    let mut current = authority(&path, 1);
    let offer = current.offer("audio-fixture", context(1, 1), 1).unwrap();
    let scan = current.scan();
    current.replace(&[], 2).unwrap();
    assert!(!current.scanned(&scan, true, 3));
    assert!(
        current
            .authorize(&offer, RequestedAudioPhase::Start, 3)
            .is_err()
    );
    current.replace(&[load(&path).unwrap()], 4).unwrap();
    assert!(current.offer("audio-fixture", context(1, 1), 4).is_err());
    let mut restarted = authority(&path, 1);
    assert!(
        restarted
            .authorize(&offer, RequestedAudioPhase::Start, 2)
            .is_err()
    );
    let offer = current.offer("audio-fixture", context(2, 4), 4).unwrap();
    assert!(
        current
            .authorize(&offer, RequestedAudioPhase::Start, 4 + SCAN_FRESH_US)
            .is_err()
    );
    let scan = current.scan();
    current.scanned(&scan, true, 5 + SCAN_FRESH_US);
    assert!(
        current
            .authorize(&offer, RequestedAudioPhase::Start, 5 + SCAN_FRESH_US)
            .is_err()
    );
}

#[test]
fn in_place_permission_edit_and_asset_edit_revoke_existing_offers() {
    let (_dir, path) = fixture();
    for edit_permission in [true, false] {
        fs::write(path.join("skill.toml"), TOML).unwrap();
        let mut authority = authority(&path, 1);
        let offer = authority.offer("audio-fixture", context(1, 1), 1).unwrap();
        let scan = authority.scan();
        if edit_permission {
            fs::write(
                path.join("skill.toml"),
                TOML.replace("speak = true", "speak = false"),
            )
            .unwrap();
        } else {
            wav(&path.join("sounds/fixture.wav"), 200);
        }
        assert!(!scan.unchanged());
        assert!(!authority.scanned(&scan, false, 2));
        assert!(
            authority
                .authorize(&offer, RequestedAudioPhase::Start, 3)
                .is_err()
        );
    }
}

#[test]
fn the_bounded_table_refuses_instead_of_evicting_replay_protection() {
    let (_dir, path) = fixture();
    let mut authority = authority(&path, 1);
    for id in 0..MOST_OFFERS {
        let offer = authority
            .offer("audio-fixture", context(id as u64, 1), 1)
            .unwrap();
        authority.finish(&offer).unwrap();
    }
    assert!(
        authority
            .offer("audio-fixture", context(MOST_OFFERS as u64, 1), 1)
            .is_err()
    );
    assert!(authority.offer("audio-fixture", context(0, 2), 2).is_err());
}

struct Service {
    child: Child,
    tools: std::path::PathBuf,
    clock: std::path::PathBuf,
    stderr: std::path::PathBuf,
    _runtime: tempfile::TempDir,
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn exchange(path: &Path, value: serde_json::Value) -> serde_json::Value {
    let mut stream = UnixStream::connect(path).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    writeln!(stream, "{value}").unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap_or_else(|why| panic!("{line:?}: {why}"))
}

impl Service {
    fn start(root: &Path, switch: &str) -> Self {
        Self::with_limits(root, switch, false)
    }

    fn with_limits(root: &Path, switch: &str, limits: bool) -> Self {
        let plain = root.join("plain-fixture");
        fs::create_dir(&plain).unwrap();
        fs::write(
            plain.join("SKILL.md"),
            "---\nname: plain-fixture\ndescription: Give a short answer.\n---\n",
        )
        .unwrap();
        fs::write(plain.join("skill.toml"), "tool = \"plain_fixture\"\n[permissions]\nspeak = true\n[[do]]\nsay = \"Answer briefly.\"\n").unwrap();
        // A separate short path keeps macOS Unix socket paths within bounds.
        let runtime = tempfile::Builder::new()
            .prefix("audio-svc-")
            .tempdir_in("/tmp")
            .unwrap();
        let tools = runtime.path().join("t");
        let clock = runtime.path().join("c");
        let bus = runtime.path().join("b");
        let actions = runtime.path().join("a");
        let stderr = runtime.path().join("stderr");
        let child = Command::new(env!("CARGO_BIN_EXE_lamp-skills"))
            .args([
                "--dir",
                root.to_str().unwrap(),
                "--tools-dir",
                tools.to_str().unwrap(),
                "--bus-root",
                bus.to_str().unwrap(),
                "--clock-control",
                clock.to_str().unwrap(),
                "--actions-socket",
                actions.to_str().unwrap(),
                "--zone",
                "UTC",
                "--boot-id",
                "audio-service-host",
            ])
            .env_clear()
            .env("LAMP_REQUESTED_AUDIO", switch)
            .env("LAMP_SKILL_ROUTINES", if limits { "on" } else { "off" })
            .env("LAMP_OUTPUT_CAPS", if limits { "on" } else { "off" })
            .env("LAMP_SKILLS_ASK", "skip")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(fs::File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        let mut service = Self {
            child,
            tools,
            clock,
            stderr,
            _runtime: runtime,
        };
        service.until_catalog(|tools| tools.iter().any(|tool| tool["name"] == "plain_fixture"));
        service
    }

    fn until_catalog(&mut self, predicate: impl Fn(&[serde_json::Value]) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(bytes) = fs::read(self.tools.join("tools.json"))
                && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
                && let Some(tools) = value["tools"].as_array()
                && predicate(tools)
            {
                return;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "skills exited before readiness"
            );
            assert!(
                Instant::now() < deadline,
                "skills catalog readiness timed out"
            );
            // A bounded readiness poll, never an assumption that time made
            // the child ready; the positive catalog condition is required.
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn call(&self, request: serde_json::Value) -> serde_json::Value {
        exchange(&self.tools.join("call.sock"), request)
    }

    fn offer(&self, call: u64) -> lamp_msgs::RequestedAudioOffer {
        let response = self.call(json!({
            "op":"call", "name":"play_fixture", "args":{},
            "requested_audio_context":context(call, lamp_ipc::monotonic_us())
        }));
        assert_eq!(response["ok"], true, "{response}");
        serde_json::from_value(response["result"]["requested_audio"].clone()).unwrap()
    }

    fn until_refused_reloads(&mut self, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let log = fs::read_to_string(&self.stderr).unwrap();
            if log
                .matches("reload refused: at most eight skills may own output limits")
                .count()
                >= count
            {
                return;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "skills exited before reload refusal: {log}"
            );
            assert!(Instant::now() < deadline, "reload was not refused: {log}");
            // The refusal is the barrier; elapsed time alone never passes.
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn real_call_socket_keeps_legacy_tools_and_requires_private_spoken_context() {
    let (_dir, path) = fixture();
    let mut service = Service::start(path.parent().unwrap(), "on");
    assert_eq!(
        service.call(json!({"op":"call","name":"plain_fixture","args":{}}))["ok"],
        true
    );
    assert_eq!(
        service.call(json!({"op":"call","name":"play_fixture","args":{}}))["ok"],
        false
    );
    let ctx = context(1, lamp_ipc::monotonic_us());
    assert_eq!(
        service.call(
            json!({"op":"call","name":"play_fixture","args":{"requested_audio_context":ctx}})
        )["ok"],
        false
    );
    let response = service
        .call(json!({"op":"call","name":"play_fixture","args":{},"requested_audio_context":ctx}));
    assert_eq!(response["ok"], true, "{response}");
    let offer: lamp_msgs::RequestedAudioOffer =
        serde_json::from_value(response["result"]["requested_audio"].clone()).unwrap();
    assert_eq!(offer.context, ctx);
    assert_eq!(offer.issued_us, ctx.issued_us);
    assert_eq!(
        offer.start_before_us,
        ctx.issued_us + REQUESTED_AUDIO_START_US
    );
    assert_eq!(
        service.call(json!({"op":"audio_authorize","offer":offer,"phase":"future"}))["ok"],
        false
    );
    let response = service.call(json!({"op":"audio_authorize","offer":offer,"phase":"start"}));
    assert_eq!(response["ok"], true, "{response}");
    assert!(
        response["result"]["until_us"].as_u64().unwrap()
            <= lamp_ipc::monotonic_us() + REQUESTED_AUDIO_LEASE_US
    );
    assert_eq!(
        service.call(json!({"op":"audio_finish","offer":offer,"outcome":"cancelled"}))["ok"],
        true
    );
    assert_eq!(
        service.call(json!({"op":"audio_authorize","offer":offer,"phase":"renew"}))["ok"],
        false
    );
    let mut duplicate = ctx;
    duplicate.issued_us = lamp_ipc::monotonic_us();
    assert_eq!(
        service.call(
            json!({"op":"call","name":"play_fixture","args":{},"requested_audio_context":duplicate})
        )["ok"],
        false
    );

    let response = service.call(json!({"op":"call","name":"play_fixture","args":{},"requested_audio_context":context(2, lamp_ipc::monotonic_us())}));
    assert_eq!(response["ok"], true, "{response}");
    let offered = response["result"]["requested_audio"].clone();
    fs::write(
        path.join("skill.toml"),
        TOML.replace("speak = true", "speak = false"),
    )
    .unwrap();
    exchange(&service.clock, json!({"skip_us":2_000_001}));
    service.until_catalog(|tools| tools.iter().all(|tool| tool["name"] != "play_fixture"));
    assert_eq!(
        service.call(json!({"op":"audio_authorize","offer":offered,"phase":"start"}))["ok"],
        false
    );
}

#[test]
fn real_service_switch_off_hides_and_refuses_media_with_valid_context() {
    let (_dir, path) = fixture();
    let service = Service::start(path.parent().unwrap(), "off");
    let catalog: serde_json::Value =
        serde_json::from_slice(&fs::read(service.tools.join("tools.json")).unwrap()).unwrap();
    assert_eq!(catalog["tools"].as_array().unwrap().len(), 1);
    assert_eq!(
        service.call(json!({"op":"call","name":"plain_fixture","args":{}}))["ok"],
        true
    );
    assert_eq!(service.call(json!({"op":"call","name":"play_fixture","args":{},"requested_audio_context":context(1, lamp_ipc::monotonic_us())}))["ok"], false);
}

#[test]
fn refused_cap_reload_keeps_audio_generation_but_asset_scan_still_revokes_it() {
    let (dir, path) = fixture();
    let mut service = Service::with_limits(dir.path(), "on", true);
    let before = service.offer(1);
    for number in 0..9 {
        let name = format!("cap-fixture-{number}");
        let folder = dir.path().join(&name);
        fs::create_dir(&folder).unwrap();
        fs::write(
            folder.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Check bounded cap reloads.\n---\n"),
        )
        .unwrap();
        fs::write(
            folder.join("skill.toml"),
            r#"
[trigger]
topic = "voice/state"
field = "state"
equals = "resting"
[ask]
policy = "nudge"
[permissions]
topics = ["voice/state"]
body = true
output_limits = true
[limits]
during = { from = "22:00", to = "06:00" }
light_channel_max = 24
[[do]]
rest = { op = "cancel" }
"#,
        )
        .unwrap();
        assert!(load(&folder).unwrap().is_routine());
    }
    let plain = dir.path().join("plain-fixture/skill.toml");
    fs::write(
        &plain,
        fs::read_to_string(&plain)
            .unwrap()
            .replace("Answer briefly.", "A replacement answer."),
    )
    .unwrap();
    exchange(&service.clock, json!({"skip_us":2_000_001}));
    service.until_refused_reloads(1);

    let after = service.offer(2);
    assert_eq!(before.definition_generation, after.definition_generation);
    assert_eq!(before.sha256, after.sha256);
    let response = service.call(json!({"op":"audio_authorize","offer":before,"phase":"start"}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(
        service.call(json!({"op":"audio_finish","offer":before,"outcome":"cancelled"}))["ok"],
        true
    );
    // The rejected batch did not partially install the Engine definitions.
    let plain = service.call(json!({"op":"call","name":"plain_fixture","args":{}}));
    assert_eq!(plain["result"]["next"], "Answer briefly.", "{plain}");

    // Preserve metadata to prove this path uses the byte scan even while
    // the previously refused batch has an unchanged folder fingerprint.
    let asset = path.join("sounds/fixture.wav");
    let metadata = fs::metadata(&asset).unwrap();
    wav(&asset, 200);
    fs::OpenOptions::new()
        .write(true)
        .open(&asset)
        .unwrap()
        .set_modified(metadata.modified().unwrap())
        .unwrap();
    let changed = fs::metadata(&asset).unwrap();
    assert_eq!(changed.len(), metadata.len());
    assert_eq!(changed.modified().unwrap(), metadata.modified().unwrap());
    exchange(&service.clock, json!({"skip_us":2_000_001}));
    service.until_refused_reloads(2);
    assert_eq!(
        service.call(json!({"op":"audio_authorize","offer":after,"phase":"start"}))["ok"],
        false
    );
    let refused = service.call(json!({
        "op":"call", "name":"play_fixture", "args":{},
        "requested_audio_context":context(3, lamp_ipc::monotonic_us())
    }));
    assert_eq!(refused["ok"], false, "{refused}");
    let catalog: serde_json::Value =
        serde_json::from_slice(&fs::read(service.tools.join("tools.json")).unwrap()).unwrap();
    assert!(
        catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "play_fixture")
    );
}
