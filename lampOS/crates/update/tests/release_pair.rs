//! Explicit, offline compatibility checks of two published ARM64 executables.
//! The real CLI writes each journal/request and the other release recovers an
//! actual process kill. Only the host fixture and systemctl responses are fake.
//! No service, timer, physical power-cut or release-signature claim is made.
//! Run the two ignored tests with LAMP_COMPAT_6408 and LAMP_COMPAT_6449 naming
//! the pinned public artifacts. Docker uses its existing colima-lampos context,
//! a pinned image, no network, and no host mounts. All output is retained.
use lamp_update::{
    manifest::{self, File as BuildFile, Manifest, Setting, Unit},
    records::{Build, Records, State},
};
use ring::{rand::SystemRandom, signature::Ed25519KeyPair};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{self, Read, Write},
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const OLD_SHA: &str = "f731555d6b783a69eb00bc2ff774e8ad922988284c9f3a9114010fa75fa51778";
const CURRENT_SHA: &str = "c953a32e802f41dc1bc741bef1fe910faa286f27c6cb1a1988aad435c5331434";
const IMAGE: &str = "sha256:7dc3bb4b3b88bb00e16e37eeb5c98a74f9d926f956584eb8c009c9ca86023cd2";
const CONTEXT: &str = "colima-lampos";
const RECORDS: &str = "var/lib/lampos-update";
const WITHIN: Duration = Duration::from_secs(120);
const PARTS: &[&str] = &["air", "records", "schedule"];
const SETTINGS: &[(&str, &str, u32, &str, u32)] = &[
    (
        "/etc/lampOS/records.json",
        "{\"fixture\":\"before records\"}\n",
        0o640,
        "{\"fixture\":\"after records\"}\n",
        0o600,
    ),
    (
        "/etc/lampOS/controls/controls.json",
        "{\"fixture\":\"before controls\"}\n",
        0o600,
        "{\"fixture\":\"after controls\"}\n",
        0o644,
    ),
];
static COMMAND_ID: AtomicU64 = AtomicU64::new(0);

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        // Do not signal an exited/reaped child's recycled PID. Child caches
        // terminal status, and the CLI is never invoked through a shell.
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = self.0.kill();
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            match self.0.try_wait() {
                Ok(Some(_)) => return,
                Err(_) => break,
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        let _ = writeln!(
            io::stderr().lock(),
            "compatibility Docker CLI cleanup could not confirm reap within five seconds"
        );
    }
}

struct ResultOf {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn bounded_text(path: &Path) -> io::Result<String> {
    let mut text = String::new();
    File::open(path)?.take(65_536).read_to_string(&mut text)?;
    Ok(text)
}

fn docker(logs: &Path, args: &[OsString], within: Duration) -> Result<ResultOf, String> {
    let id = COMMAND_ID.fetch_add(1, Ordering::Relaxed);
    let stdout = logs.join(format!("command-{id}.stdout"));
    let stderr = logs.join(format!("command-{id}.stderr"));
    let mut child = Running(
        Command::new("docker")
            .args(["--context", CONTEXT])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::from(
                File::create(&stdout).map_err(|e| e.to_string())?,
            ))
            .stderr(Stdio::from(
                File::create(&stderr).map_err(|e| e.to_string())?,
            ))
            .spawn()
            .map_err(|e| format!("cannot start Docker CLI: {e}"))?,
    );
    let until = Instant::now() + within;
    let status = loop {
        if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() >= until {
            return Err(format!(
                "Docker command {args:?} exceeded {within:?}; stdout={} stderr={}",
                bounded_text(&stdout).unwrap_or_default(),
                bounded_text(&stderr).unwrap_or_default()
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Ok(ResultOf {
        status,
        stdout: bounded_text(&stdout).map_err(|e| e.to_string())?,
        stderr: bounded_text(&stderr).map_err(|e| e.to_string())?,
    })
}

fn arguments(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(OsString::from).collect()
}

fn checked(logs: &Path, args: &[OsString]) -> ResultOf {
    let result = docker(logs, args, WITHIN).unwrap_or_else(|why| panic!("{why}"));
    assert!(
        result.status.success(),
        "Docker {args:?}: {}\n{}\n{}",
        result.status,
        result.stdout,
        result.stderr
    );
    result
}

struct Container<'a> {
    name: String,
    owner: String,
    logs: &'a Path,
    attempted: bool,
}
impl Container<'_> {
    fn remove(&mut self) -> Result<(), String> {
        if !self.attempted {
            return Ok(());
        }
        // Even a timed-out create can have succeeded at the daemon. Inspect
        // our random name's label before removing anything by that name.
        let inspect = docker(
            self.logs,
            &arguments(&[
                "container",
                "inspect",
                "--format",
                "{{index .Config.Labels \"lamp.compat.owner\"}}",
                &self.name,
            ]),
            Duration::from_secs(20),
        )?;
        if !inspect.status.success() {
            // Absence is the sole successful no-op; daemon/context failures
            // must stay visible rather than being called successful cleanup.
            if inspect.stderr.contains("No such container")
                || inspect.stderr.contains("No such object")
            {
                self.attempted = false;
                return Ok(());
            }
            return Err(format!(
                "cannot inspect compatibility container for cleanup: {}",
                inspect.stderr
            ));
        }
        if inspect.stdout.trim() != self.owner {
            return Err("refusing to remove a container without this fixture's owner label".into());
        }
        let removed = docker(
            self.logs,
            &arguments(&["container", "rm", "--force", &self.name]),
            Duration::from_secs(30),
        )?;
        if !removed.status.success() {
            return Err(format!(
                "cannot remove compatibility container: {}",
                removed.stderr
            ));
        }
        self.attempted = false;
        Ok(())
    }
}
impl Drop for Container<'_> {
    fn drop(&mut self) {
        if let Err(why) = self.remove() {
            let _ = writeln!(
                io::stderr().lock(),
                "compatibility container cleanup failed: {why}"
            );
        }
    }
}

fn write(path: &Path, bytes: impl AsRef<[u8]>, mode: u32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn pin(variable: &str, expected: &str, destination: &Path) {
    let path = std::env::var_os(variable)
        .unwrap_or_else(|| panic!("manual fixture needs {variable}, the pinned ARM64 release"));
    let metadata = fs::metadata(&path).unwrap();
    assert!(metadata.is_file() && metadata.len() <= 128 * 1024 * 1024);
    let bytes = fs::read(path).unwrap();
    assert_eq!(
        manifest::sha256_hex(&bytes),
        expected,
        "{variable}: SHA-256"
    );
    assert!(bytes.starts_with(b"\x7fELF\x02\x01"), "ELF64 little-endian");
    assert_eq!(bytes.get(18..20), Some(&[0xb7, 0x00][..]), "AArch64");
    // Execute only our immutable copy of the verified bytes, not the supplied
    // path, which another process could replace after this check.
    write(destination, bytes, 0o555);
}

fn binary(part: &str, new: bool) -> String {
    format!(
        "#!/bin/sh\nprintf 'synthetic {part} {}\\n'\n",
        if new { 2 } else { 1 }
    )
}

fn unit_text(part: &str, new: bool) -> String {
    // The graphs deliberately differ. Recovery must use the old declaration
    // order (Air then Records), not the candidate order (Records then Air).
    let after = match (part, new) {
        ("records", false) => "After=lamp-air.service\n",
        ("air", true) => "After=lamp-records.service\n",
        _ => "",
    };
    format!(
        "[Unit]\nDescription=release-pair {} {part}\n{after}[Service]\nExecStart=/opt/lampOS/{part}/current/lamp-{part}\nRestart=on-failure\n",
        if new { "candidate" } else { "before" }
    )
}

struct Fixture {
    folder: PathBuf,
    payload: PathBuf,
    logs: PathBuf,
    initial_state: Value,
}

impl Fixture {
    fn new(writer: &str, reader: &str) -> Self {
        // Keep the complete input and output, even on assertion failure. No
        // release binary or raw container log is committed to the repository.
        let folder = tempfile::Builder::new()
            .prefix("lamp-release-pair-")
            .tempdir()
            .unwrap()
            .keep();
        println!("release-pair evidence: {}", folder.display());
        let payload = folder.join("case");
        let logs = folder.join("logs");
        fs::create_dir_all(&logs).unwrap();
        fs::create_dir_all(payload.join("out")).unwrap();
        pin("LAMP_COMPAT_6408", OLD_SHA, &payload.join("6408"));
        pin("LAMP_COMPAT_6449", CURRENT_SHA, &payload.join("6449"));
        let root = payload.join("root");
        let key_bytes = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(key_bytes.as_ref()).unwrap();
        write(
            &root.join("etc/lampOS/update/key.pub"),
            manifest::public_key_text(&key),
            0o644,
        );
        let mut state = State {
            installed: Some(Build {
                sequence: 1,
                commit: "a".repeat(40),
            }),
            ..State::default()
        };
        state
            .later
            .insert("release_pair_canary".into(), json!({"kept": true}));
        let mut settings = Vec::new();
        for &(path, before, mode, after, after_mode) in SETTINGS {
            write(&root.join(path.trim_start_matches('/')), before, mode);
            write(
                &payload.join("after").join(path.trim_start_matches('/')),
                after,
                after_mode,
            );
            state
                .settings
                .insert(path.into(), manifest::sha256_hex(before.as_bytes()));
            let mut setting = Setting::new(path, after);
            setting.mode = Some(format!("{after_mode:04o}"));
            settings.push(setting);
        }
        write(
            &root.join("etc/lampOS/unrelated.txt"),
            b"must stay byte-exact\n",
            0o640,
        );
        let mut files = Vec::new();
        let mut units = Vec::new();
        for part in PARTS {
            let name = format!("lamp-{part}");
            let before = binary(part, false);
            let after = binary(part, true);
            let old_hash = manifest::sha256_hex(before.as_bytes());
            write(
                &root.join(format!("opt/lampOS/{part}/{old_hash}/{name}")),
                &before,
                0o755,
            );
            symlink(&old_hash, root.join(format!("opt/lampOS/{part}/current"))).unwrap();
            write(
                &root.join(format!("etc/systemd/system/{name}.service")),
                unit_text(part, false),
                0o644,
            );
            write(&payload.join(format!("channel/{name}")), &after, 0o755);
            files.push(BuildFile {
                name: name.clone(),
                sha256: manifest::sha256_hex(after.as_bytes()),
                size: after.len() as u64,
            });
            let text = unit_text(part, true);
            let unit = Unit {
                lamp: "fixture".into(),
                unit: format!("{name}.service"),
                sha256: manifest::sha256_hex(text.as_bytes()),
                size: text.len() as u64,
            };
            write(
                &payload.join("channel").join(unit.file_name()),
                &text,
                0o644,
            );
            write(
                &payload.join(format!("after/etc/systemd/system/{name}.service")),
                &text,
                0o644,
            );
            units.push(unit);
        }
        // min_updater requires a carried updater. Self-update stays off, and
        // this fixture has no update/current link; it is never installed.
        let carried_updater = fs::read(payload.join("6449")).unwrap();
        files.push(BuildFile {
            name: "lamp-update".into(),
            sha256: CURRENT_SHA.into(),
            size: carried_updater.len() as u64,
        });
        write(&payload.join("channel/lamp-update"), carried_updater, 0o555);
        Records::new(root.join(RECORDS)).save_state(&state).unwrap();
        assert!(!root.join(RECORDS).join("journal.json").exists());
        assert!(!root.join(RECORDS).join("apply").exists());
        let build = Manifest {
            schema: manifest::SCHEMA,
            channel: "lamp-voice".into(),
            sequence: 2,
            commit: "b".repeat(40),
            files_url: "file:///case/channel/".into(),
            files,
            units,
            min_updater: Some(manifest::SETTINGS_UPDATER),
            models: Vec::new(),
            settings,
            image: Vec::new(),
            reset_plan: None,
            harness: false,
        };
        write(
            &payload.join("channel/lampos-manifest.json"),
            manifest::seal(&serde_json::to_vec(&build).unwrap(), &key).unwrap(),
            0o644,
        );
        write(
            &payload.join("run.sh"),
            include_str!("release_pair/run.sh"),
            0o555,
        );
        write(
            &payload.join("bin/systemctl"),
            include_str!("release_pair/systemctl.sh"),
            0o555,
        );
        write(
            &payload.join("bin/journalctl"),
            "#!/bin/sh\nprintf 'unexpected journalctl: %s\\n' \"$*\" >> /case/out/unexpected.log\nexit 90\n",
            0o555,
        );
        write(
            &payload.join("roles"),
            format!("{writer}\n{reader}\n"),
            0o444,
        );
        write(
            &payload.join("pins"),
            format!("{OLD_SHA}  /case/6408\n{CURRENT_SHA}  /case/6449\n"),
            0o444,
        );
        Self {
            folder,
            payload,
            logs,
            initial_state: serde_json::to_value(state).unwrap(),
        }
    }

    fn execute(&self) -> PathBuf {
        let image = checked(
            &self.logs,
            &arguments(&[
                "image",
                "inspect",
                "--format",
                "{{.Id}} {{.Os}}/{{.Architecture}}",
                IMAGE,
            ]),
        );
        assert_eq!(image.stdout.trim(), format!("{IMAGE} linux/arm64"));
        let owner = self
            .folder
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_ascii_lowercase();
        let mut container = Container {
            name: owner.clone(),
            owner,
            logs: &self.logs,
            attempted: true,
        };
        checked(
            &self.logs,
            &arguments(&[
                "container",
                "create",
                "--name",
                &container.name,
                "--platform",
                "linux/arm64",
                "--network",
                "none",
                "--init",
                "--pids-limit",
                "64",
                "--memory",
                "512m",
                "--tmpfs",
                "/run:rw,nosuid,nodev,mode=755",
                "--label",
                &format!("lamp.compat.owner={}", container.owner),
                "--env",
                "LAMP_PAIR_DISPOSABLE=1",
                "--entrypoint",
                "/bin/sh",
                IMAGE,
                "/case/run.sh",
            ]),
        );
        let configuration = checked(
            &self.logs,
            &arguments(&[
                "container",
                "inspect",
                "--format",
                "{{json .}}",
                &container.name,
            ]),
        );
        let configuration: Value = serde_json::from_str(&configuration.stdout).unwrap();
        assert_eq!(configuration["HostConfig"]["NetworkMode"], "none");
        assert!(configuration["HostConfig"]["Binds"].is_null());
        assert_eq!(configuration["Mounts"], json!([]));
        checked(
            &self.logs,
            &[
                "container".into(),
                "cp".into(),
                self.payload.clone().into_os_string(),
                format!("{}:/case", container.name).into(),
            ],
        );
        let start = docker(
            &self.logs,
            &arguments(&["container", "start", "--attach", &container.name]),
            WITHIN,
        );
        // Copy failures as well as successes before cleanup. A start transport
        // failure still unwinds through our owner-checked container removal.
        let terminal = checked(
            &self.logs,
            &arguments(&[
                "container",
                "inspect",
                "--format",
                "{{json .State}}",
                &container.name,
            ]),
        );
        let terminal: Value = serde_json::from_str(&terminal.stdout).unwrap();
        let output = self.folder.join("output");
        fs::create_dir_all(&output).unwrap();
        checked(
            &self.logs,
            &[
                "container".into(),
                "cp".into(),
                format!("{}:/case/out/.", container.name).into(),
                output.clone().into_os_string(),
            ],
        );
        container.remove().unwrap_or_else(|why| panic!("{why}"));
        write(
            &self.folder.join("terminal.json"),
            serde_json::to_vec_pretty(&terminal).unwrap(),
            0o600,
        );
        let start = start.unwrap_or_else(|why| panic!("{why}; evidence {}", self.folder.display()));
        println!(
            "{}",
            json!({"phase":"container", "exit":start.status.code(), "stdout":start.stdout, "stderr":start.stderr})
        );
        assert_eq!(terminal["Status"], "exited");
        assert_eq!(terminal["Running"], false);
        assert_eq!(terminal["OOMKilled"], false);
        assert_eq!(terminal["Error"], "");
        assert_eq!(
            terminal["ExitCode"],
            0,
            "evidence {}",
            self.folder.display()
        );
        assert!(
            start.status.success(),
            "container CLI must agree with the terminal state"
        );
        output
    }

    fn verify(&self, out: &Path, writer: &str, reader: &str) {
        assert!(
            !out.join("unexpected.log").exists(),
            "unexpected fake host command"
        );
        assert_eq!(
            fs::read_to_string(out.join("writer-wait-status"))
                .unwrap()
                .trim(),
            "137"
        );
        assert_eq!(
            fs::read_to_string(out.join("recovery-status"))
                .unwrap()
                .trim(),
            "1"
        );
        assert_eq!(
            fs::read_to_string(out.join("next-status")).unwrap().trim(),
            "1"
        );
        let journal_bytes = fs::read(out.join("cut/records/journal.json")).unwrap();
        let journal_sha = manifest::sha256_hex(&journal_bytes);
        assert_eq!(
            fs::read_to_string(out.join("journal-before-recovery.sha256"))
                .unwrap()
                .split_whitespace()
                .next(),
            Some(journal_sha.as_str())
        );
        let writer_sha = if writer == "6408" {
            OLD_SHA
        } else {
            CURRENT_SHA
        };
        assert_eq!(
            fs::read_to_string(out.join("writer-executable"))
                .unwrap()
                .split_whitespace()
                .next(),
            Some(writer_sha)
        );
        let journal = read_json(&out.join("cut/records/journal.json"));
        assert_eq!(
            journal["build"],
            json!({"sequence":2,"commit":"b".repeat(40)})
        );
        assert_eq!(journal["settings"], true);
        assert_eq!(
            journal["rollback_order"],
            json!([
                "lamp-air.service",
                "lamp-records.service",
                "lamp-schedule.service"
            ])
        );
        let units = journal["units"].as_array().unwrap();
        assert_eq!(units.len(), 3);
        for part in PARTS {
            let switching = units
                .iter()
                .find(|unit| unit["unit"] == format!("lamp-{part}.service"))
                .unwrap();
            assert_eq!(switching["was_active"], *part != "schedule");
            assert_eq!(
                switching["link"]["path"],
                format!("/opt/lampOS/{part}/current")
            );
            assert_eq!(
                switching["link"]["build"],
                manifest::sha256_hex(binary(part, false).as_bytes())
            );
        }
        for (phase, op) in [("forward", "apply"), ("recovery", "back")] {
            let request = read_json(&out.join(format!("{phase}-request.json")));
            let done = read_json(&out.join(format!("{phase}-done.json")));
            for document in [&request, &done] {
                assert_eq!(document["version"], 1);
                assert_eq!(document["sequence"], 2);
                assert_eq!(document["op"], op);
            }
            let helper = fs::read_to_string(out.join(format!("{phase}-helper"))).unwrap();
            assert_eq!(
                helper.trim(),
                if phase == "forward" { reader } else { writer }
            );
            assert_eq!(
                fs::read_to_string(out.join(format!("{phase}-helper-status")))
                    .unwrap()
                    .trim(),
                "0"
            );
            for &(path, before, _, after, _) in SETTINGS {
                let text = if phase == "forward" { after } else { before };
                assert_eq!(done["records"][path], manifest::sha256_hex(text.as_bytes()));
            }
        }
        let back = read_json(&out.join("forward-back.json"));
        assert_eq!(back["version"], 1);
        assert_eq!(back["sequence"], 2);
        assert_eq!(back["files"].as_array().unwrap().len(), SETTINGS.len());
        assert_eq!(
            fs::read(out.join("forward-back.json")).unwrap(),
            fs::read(out.join("recovery-back.json")).unwrap()
        );
        for (snapshot, new) in [("cut", true), ("recovered", false), ("next", false)] {
            let at = out.join(snapshot);
            for &(path, before, mode, after, after_mode) in SETTINGS {
                assert_file(
                    &at.join(path.trim_start_matches('/')),
                    if new { after } else { before },
                    if new { after_mode } else { mode },
                );
            }
            assert_file(
                &at.join("etc/lampOS/unrelated.txt"),
                "must stay byte-exact\n",
                0o640,
            );
            for part in PARTS {
                assert_file(
                    &at.join(format!("units/lamp-{part}.service")),
                    &unit_text(part, new),
                    0o644,
                );
                assert_eq!(
                    fs::read_to_string(at.join(format!("{part}.link")))
                        .unwrap()
                        .trim(),
                    manifest::sha256_hex(binary(part, new).as_bytes())
                );
                assert_file(
                    &at.join(format!("{part}.selected")),
                    &binary(part, new),
                    0o755,
                );
            }
            assert!(!at.join("records/self.json").exists());
            assert!(!at.join("records/apply/request.json").exists());
            assert!(!at.join("records/apply/done.json").exists());
        }
        let recovered = read_json(&out.join("recovered/records/state.json"));
        let mut without_refusal = recovered.clone();
        let refusal = without_refusal["refused"]
            .as_array_mut()
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(refusal["sequence"], 2);
        assert_eq!(refusal["commit"], "b".repeat(40));
        assert!(refusal["why"].as_str().unwrap().contains("cut short"));
        assert_eq!(
            without_refusal, self.initial_state,
            "settings, custom fields and installed build restored"
        );
        assert_eq!(recovered, read_json(&out.join("next/records/state.json")));
        assert!(!out.join("recovered/records/journal.json").exists());
        assert!(!out.join("next/records/journal.json").exists());
        let calls = fs::read_to_string(out.join("commands.log")).unwrap();
        let restarts: Vec<&str> = calls
            .lines()
            .filter(|line| line.contains(" restart "))
            .collect();
        assert_eq!(
            restarts,
            [
                "forward restart lamp-update-apply.service",
                "forward restart lamp-records.service",
                "recovery restart lamp-update-apply.service",
                "recovery restart lamp-air.service",
                "recovery restart lamp-records.service",
            ]
        );
        assert!(!calls.lines().any(|line| line.starts_with("next ")));
        assert!(
            fs::read_to_string(out.join("recovery.stderr"))
                .unwrap()
                .contains("cut short")
        );
        assert!(
            fs::read_to_string(out.join("next.stdout"))
                .unwrap()
                .contains("was put back")
        );
        println!(
            "{}",
            json!({"result":"passed", "writer":writer, "recoverer":reader, "release_6408_sha256":OLD_SHA, "release_6449_sha256":CURRENT_SHA, "cut":"first ordinary restart, after real journal/settings/unit/link writes", "writer_shell_wait_status":137, "signal_sent":"SIGKILL to verified owned process group", "recovery_exit":1, "next_round_exit":1, "network":"none", "systemctl":"fixture", "physical_power_cut":false})
        );
    }
}

fn assert_file(path: &Path, text: &str, mode: u32) {
    assert_eq!(
        fs::read(path).unwrap(),
        text.as_bytes(),
        "{}",
        path.display()
    );
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o7777,
        mode,
        "{}",
        path.display()
    );
}

#[test]
#[ignore = "needs the two pinned ARM64 artifacts and the owned offline Docker context"]
fn release_6408_recovers_a_cut_written_by_release_6449() {
    let fixture = Fixture::new("6449", "6408");
    let output = fixture.execute();
    fixture.verify(&output, "6449", "6408");
}

#[test]
#[ignore = "needs the two pinned ARM64 artifacts and the owned offline Docker context"]
fn release_6449_recovers_a_cut_written_by_release_6408() {
    let fixture = Fixture::new("6408", "6449");
    let output = fixture.execute();
    fixture.verify(&output, "6408", "6449");
}
