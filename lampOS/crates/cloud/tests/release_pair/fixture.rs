#[path = "broker.rs"]
mod broker;
#[path = "support.rs"]
mod support;
use broker::Broker;
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use support::*;

const NAME: &str = "provenance-kit";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Case {
    root: PathBuf,
    artifacts: Vec<Artifact>,
    deadline: Instant,
}
impl Case {
    fn new(name: &str, writer: u64, reader: u64) -> Self {
        isolation();
        let parent = PathBuf::from(
            std::env::var_os("LAMP_COMPAT_OUTPUT").expect("explicit evidence parent"),
        );
        assert!(parent.is_absolute() && fs::symlink_metadata(&parent).unwrap().is_dir());
        let root = parent.join(format!(
            "cv-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(
            root.as_os_str().as_encoded_bytes().len() <= 48,
            "short private bus path"
        );
        directory(&root);
        write(&root.join("transcript.jsonl"), b"");
        for p in ["sources", "packages", "credentials", "cloud-state", "bus"] {
            directory(&root.join(p));
        }
        let artifacts = [6408, 6498]
            .into_iter()
            .flat_map(|r| {
                [
                    Artifact::new(&root, "skills", r),
                    Artifact::new(&root, "cloud", r),
                ]
            })
            .collect::<Vec<_>>();
        json_file(
            &root.join("case.json"),
            &json!({"test":name,"writer":writer,"reader":reader,"skills_stages":{"pack-a":writer,"install-a":writer,"pack-b":reader,"install-b":reader},"cloud_stages":{"opposite-cloud":reader,"original-cloud":writer,"return-cloud":reader},"artifacts":artifacts.iter().map(Artifact::receipt).collect::<Vec<_>>(),"isolation_marker":MARKER,"network":"loopback only; no default route","skills_path":SKILLS,"off_path":format!("{OFF_PARENT}/off"),"identical_provenance_parser":true,"distinct_cloud_payloads":true,"excluded":["accounts and remote MQTT","HTTP/provider","hardware","Store signature trust","updater","new schema policy","power-loss durability"]}),
        );
        println!("Cloud provenance release-pair evidence: {}", root.display());
        directory(Path::new("/var/lib/lampOS"));
        directory(Path::new(SKILLS));
        directory(Path::new(OFF_PARENT));
        write(&Path::new(OFF_PARENT).join("off"), b"");
        Self {
            root,
            artifacts,
            deadline: Instant::now() + Duration::from_secs(120),
        }
    }
    fn artifact(&self, part: &str, release: u64) -> &Artifact {
        self.artifacts
            .iter()
            .find(|a| a.part == part && a.release == release)
            .unwrap()
    }
    fn check(&self) {
        assert!(Instant::now() < self.deadline, "original case deadline");
    }
    fn cli(&self, release: u64, stage: &str, args: &[&str]) -> String {
        self.check();
        let artifact = self.artifact("skills", release);
        let mut child = Owned::spawn(&self.root, stage, artifact, args, &[]);
        let status = child.wait(self.deadline.min(Instant::now() + Duration::from_secs(10)));
        let stdout = read(&self.root.join(format!("{stage}.stdout")), MAX);
        let stderr = read(&self.root.join(format!("{stage}.stderr")), MAX);
        record(
            &self.root,
            json!({"event":"cli_complete","stage":stage,"stdout_sha256":sha(&stdout),"stderr_sha256":sha(&stderr)}),
        );
        assert!(
            status.success(),
            "actual released CLI failed: {}",
            String::from_utf8_lossy(&stderr)
        );
        artifact.verify();
        String::from_utf8(stdout).unwrap()
    }
    fn source(&self, label: &str, version: &str) {
        let parent = self.root.join("sources").join(label);
        directory(&parent);
        let p = parent.join(NAME);
        directory(&p);
        write(&p.join("SKILL.md"),b"---\nname: provenance-kit\ndescription: A synthetic package provenance fixture.\n---\n# Provenance fixture\n");
        write(&p.join("skill.toml"),format!("kind = \"installable\"\nversion = \"{version}\"\nenabled = true\ntool = \"fixture_provenance\"\n\n[permissions]\nspeak = true\n\n[[do]]\nsay = \"Synthetic inert definition {version}.\"\n").as_bytes());
        write(
            &p.join("notes.txt"),
            format!("Synthetic asset {version}.\n").as_bytes(),
        );
    }
    fn install(&self, release: u64, label: &str, version: &str) -> String {
        let dest = format!("packages/{label}");
        directory(&self.root.join(&dest));
        let source = format!("sources/{label}/{NAME}");
        let package = format!("{dest}/{NAME}-{version}.tar");
        assert_eq!(
            self.cli(
                release,
                &format!("pack-{label}"),
                &["pack", "--out", &dest, &source]
            ),
            format!("packed {package}\n")
        );
        let path = self.root.join(&package);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        let bytes = read(&path, MAX);
        let inputs = tree(&self.root.join("packages"));
        let installed = self.cli(
            release,
            &format!("install-{label}"),
            &["install", "--dir", SKILLS, &package],
        );
        assert!(
            installed.starts_with("installed ") || installed.starts_with("replaced "),
            "actual CLI success"
        );
        assert!(
            installed.contains(NAME)
                && installed.contains("from its package")
                && installed.contains("it carries no signature"),
            "actual CLI package identity and unsigned boundary"
        );
        assert_eq!(
            tree(&self.root.join("packages")),
            inputs,
            "released install modified its input"
        );
        let provenance = read(&Path::new(SKILLS).join(NAME).join(".package.json"), MAX);
        let value: Value = serde_json::from_slice(&provenance).unwrap();
        assert_eq!(value["sha256"], sha(&bytes));
        assert_eq!(value["manifest"]["name"], NAME);
        assert_eq!(value["manifest"]["version"], version);
        record(
            &self.root,
            json!({"event":"actual_package_installed","release":release,"version":version,"package":package,"package_bytes":bytes.len(),"package_sha256":sha(&bytes),"provenance_sha256":sha(&provenance),"inspection":"generic JSON only; actual Cloud consumer tested next"}),
        );
        sha(&bytes)
    }
    fn checked_list(&self, broker: &mut Broker, stage: &str, version: &str, hash: Option<&str>) {
        self.check();
        let before = tree(Path::new(SKILLS));
        let inputs = tree(&self.root.join("sources"));
        let packages = tree(&self.root.join("packages"));
        let off = tree(Path::new(OFF_PARENT));
        snapshot(&self.root, &format!("{stage}-before"), &before);
        let reply = broker.list(stage);
        json_file(&self.root.join(format!("{stage}-reply.json")), &reply);
        let data = &reply["data"];
        assert_eq!(data["version"], 1);
        assert_eq!(data["left_out"], 0);
        let rows = data["skills"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0],
            json!({"name":NAME,"title":"Provenance fixture","description":"A synthetic package provenance fixture.","group":"installed","version":version,"sha256":hash,"can_remove":true,"can_turn_off":true,"off":false,"loads":true,"why":null,"may_use":[{"name":"speak","says":"speaks, or plays a short sound","privacy":null}]})
        );
        assert_eq!(
            tree(Path::new(SKILLS)),
            before,
            "Cloud read mutated installed files"
        );
        assert_eq!(
            tree(&self.root.join("sources")),
            inputs,
            "Cloud read mutated source inputs"
        );
        assert_eq!(
            tree(&self.root.join("packages")),
            packages,
            "Cloud read mutated package inputs"
        );
        assert_eq!(
            tree(Path::new(OFF_PARENT)),
            off,
            "Cloud read mutated Skills off state"
        );
        snapshot(&self.root, &format!("{stage}-after"), &before);
        record(
            &self.root,
            json!({"event":"cloud_provenance_checked","stage":stage,"version":version,"sha256":hash,"installed_and_inputs_unchanged":true,"row_still_loaded":true}),
        );
    }
    fn reader(&self, release: u64, stage: &str, version: &str, hash: &str, negative: bool) {
        self.check();
        let listener = Broker::listener();
        let port = listener.local_addr().unwrap().port();
        let creds = self.root.join("credentials").join(stage);
        directory(&creds);
        json_file(
            &creds.join("account"),
            &json!({"device_id":"compat-dummy","mqtt_endpoint":"127.0.0.1","mqtt_port":port,"mqtt_username":"fixture","mqtt_password":"fixture-only","fa_channel":"compat/fa","fd_channel":"compat/fd","llm_base_url":""}),
        );
        write(&creds.join("key"), b"");
        write(&creds.join("build"), b"{}");
        let state = self.root.join("cloud-state").join(stage);
        directory(&state);
        let bus = self.root.join("bus").join(stage);
        directory(&bus);
        let artifact = self.artifact("cloud", release);
        let mut child = Owned::spawn(
            &self.root,
            stage,
            artifact,
            &["--bus-root", bus.to_str().unwrap()],
            &[
                ("LAMP_CLOUD", "on"),
                ("LAMP_SKILLS_PAGE", "on"),
                ("CREDENTIALS_DIRECTORY", creds.to_str().unwrap()),
                ("STATE_DIRECTORY", state.to_str().unwrap()),
            ],
        );
        let mut broker = Broker::accept(listener, &self.root, stage, &mut child, self.deadline);
        self.checked_list(
            &mut broker,
            &format!("{stage}-positive"),
            version,
            Some(hash),
        );
        if negative {
            let path = Path::new(SKILLS).join(NAME).join(".package.json");
            let original = read(&path, MAX);
            for kind in ["malformed", "unknown-field"] {
                let changed = if kind == "malformed" {
                    b"{\"sha256\":".to_vec()
                } else {
                    let mut v: Value = serde_json::from_slice(&original).unwrap();
                    v["future_provenance_field"] = json!(true);
                    serde_json::to_vec(&v).unwrap()
                };
                fs::write(&path, &changed).unwrap();
                record(
                    &self.root,
                    json!({"event":"synthetic_negative_metadata","stage":stage,"kind":kind,"sha256":sha(&changed),"not_a_released_writer":true}),
                );
                self.checked_list(&mut broker, &format!("{stage}-{kind}"), version, None);
                fs::write(&path, &original).unwrap();
            }
            self.checked_list(
                &mut broker,
                &format!("{stage}-restored"),
                version,
                Some(hash),
            );
        }
        child.alive();
        child.stop();
        drop(broker);
        artifact.verify();
        let _ = read(&self.root.join(format!("{stage}.stdout")), MAX);
        let _ = read(&self.root.join(format!("{stage}.stderr")), MAX);
        assert!(
            fs::read_dir(&state).unwrap().next().is_none(),
            "no login rotation/report state written"
        );
    }
}

pub fn run(writer: u64, reader: u64, name: &str) {
    let case = Case::new(name, writer, reader);
    case.source("a", "1.0.0");
    case.source("b", "1.1.0");
    let source_root = case.root.join("sources");
    for (path, node) in tree(&source_root) {
        fs::set_permissions(
            source_root.join(path),
            fs::Permissions::from_mode(if node.bytes.is_some() { 0o444 } else { 0o555 }),
        )
        .unwrap();
    }
    let sources = tree(&source_root);
    json_file(&case.root.join("sources-before.json"), &tree_json(&sources));
    let first = case.install(writer, "a", "1.0.0");
    case.reader(reader, "opposite-cloud", "1.0.0", &first, true);
    let next = case.install(reader, "b", "1.1.0");
    case.reader(writer, "original-cloud", "1.1.0", &next, true);
    case.reader(reader, "return-cloud", "1.1.0", &next, false);
    assert_eq!(tree(&source_root), sources, "immutable source definitions");
    json_file(&case.root.join("sources-after.json"), &tree_json(&sources));
    for artifact in &case.artifacts {
        artifact.verify();
    }
    let transcript = read(&case.root.join("transcript.jsonl"), 1024 * 1024);
    let events = String::from_utf8(transcript)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .collect::<Vec<_>>();
    let spawned = events
        .iter()
        .filter(|v| v["event"] == "spawn")
        .collect::<Vec<_>>();
    let reaped = events
        .iter()
        .filter(|v| v["event"] == "reaped")
        .collect::<Vec<_>>();
    assert_eq!(spawned.len(), 7);
    assert_eq!(reaped.len(), 7);
    for s in spawned {
        assert_eq!(
            reaped
                .iter()
                .filter(|r| r["stage"] == s["stage"] && r["pid"] == s["pid"])
                .count(),
            1
        );
    }
    assert!(!events.iter().any(|v| v["event"] == "failure_cleanup"));
    assert_eq!(
        events
            .iter()
            .filter(|v| v["event"] == "cloud_provenance_checked")
            .count(),
        9
    );
    assert_eq!(
        events
            .iter()
            .filter(|v| v["event"] == "synthetic_negative_metadata")
            .count(),
        4
    );
    // Delete only paths this case created, after all seven owned children reaped.
    fs::remove_dir_all("/var/lib/lampOS").unwrap();
    fs::remove_dir_all(OFF_PARENT).unwrap();
    for p in ["/var/lib/lampOS", OFF_PARENT] {
        assert_eq!(
            fs::symlink_metadata(p).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
    }
    json_file(
        &case.root.join("result.json"),
        &json!({"test":name,"result":"passed","children_reaped":7,"fixed_paths_removed":true,"artifacts_unchanged":true,"synthetic_metadata_controls":4,"cloud_queries":9,"actual_released_writers_and_readers":true,"provenance_parser_changed":false}),
    );
}
