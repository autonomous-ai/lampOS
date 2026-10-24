//! Offline local-package CLI only. Today's Skills loader and package reader
//! never stand in for a released reader. Generic tar/JSON inspection records bytes;
//! it does not qualify Cloud's separate installed-provenance consumer.

use super::*;

const NAME: &str = "pair-kit";
const TOOL: &str = "fixture_package";
const HIDDEN: &str = "sleeping-kit";
const HIDDEN_TOOL: &str = "fixture_hidden";
const LIVE: &str = "installed";
const MANIFEST: &str = "lamp-skill.json";

#[derive(Debug, PartialEq, Eq)]
struct Kept {
    mode: u32,
    bytes: Option<Vec<u8>>,
}

type Tree = BTreeMap<String, Kept>;

fn tree(root: &Path) -> Tree {
    fn visit(root: &Path, dir: &Path, kept: &mut Tree) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            assert!(!metadata.is_symlink(), "fixture tree contains a link");
            assert!(metadata.is_file() || metadata.is_dir());
            assert!(kept.len() < 64, "bounded fixture tree");
            let bytes = metadata.is_file().then(|| {
                assert!(metadata.len() <= MAX_LOG);
                fs::read(&path).unwrap()
            });
            let relative = path.strip_prefix(root).unwrap().to_str().unwrap();
            assert!(
                kept.insert(
                    relative.into(),
                    Kept {
                        mode: metadata.permissions().mode() & 0o7777,
                        bytes,
                    },
                )
                .is_none()
            );
            if metadata.is_dir() {
                visit(root, &path, kept);
            }
        }
    }
    let metadata = fs::symlink_metadata(root).unwrap();
    assert!(metadata.is_dir() && !metadata.is_symlink());
    let mut kept = Tree::from([(
        ".".into(),
        Kept {
            mode: metadata.permissions().mode() & 0o7777,
            bytes: None,
        },
    )]);
    visit(root, root, &mut kept);
    kept
}

fn tree_receipt(kept: &Tree) -> Value {
    json!(kept.iter().map(|(path, entry)| {
        match &entry.bytes {
            Some(bytes) => json!({"path":path,"mode":entry.mode,"bytes":bytes.len(),"sha256":sha(bytes)}),
            None => json!({"path":path,"mode":entry.mode,"directory":true}),
        }
    }).collect::<Vec<_>>())
}

fn snapshot(case: &mut Case, stage: &str) -> Tree {
    let kept = tree(&case.root.join(LIVE));
    let root = case.root.join(format!("snapshot-{stage}"));
    directory(&root);
    for (path, entry) in &kept {
        if path == "." {
            continue;
        }
        match &entry.bytes {
            Some(bytes) => write(&root.join(path), bytes),
            None => directory(&root.join(path)),
        }
    }
    let receipt = tree_receipt(&kept);
    json_file(&root.join("files.json"), &receipt);
    case.record(json!({"event":"installed_snapshot","stage":stage,"files":receipt}));
    kept
}

fn new_case(writer: Release, reader: Release) -> Case {
    assert_eq!(std::env::consts::OS, "linux", "manual native fixture");
    assert_eq!(std::env::consts::ARCH, "aarch64", "manual native fixture");
    let parent = std::env::var_os("LAMP_COMPAT_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    assert!(parent.is_absolute() && parent.is_dir());
    let root = tempfile::Builder::new()
        .prefix("kp-")
        .tempdir_in(parent)
        .unwrap()
        .keep();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    for name in [LIVE, "sources", "packages", "negative", "inventory"] {
        directory(&root.join(name));
    }
    writer.copy(&root);
    reader.copy(&root);
    json_file(
        &root.join("case.json"),
        &json!({
            "kind":"local-package-loader","writer":writer.number(),"reader":reader.number(),
            "return_reader":writer.number(),"writer_sha256":writer.pin().0,
            "reader_sha256":reader.pin().0,"writer_bytes":writer.pin().1,"reader_bytes":reader.pin().1,
            "writer_source":writer.pin().2,"reader_source":reader.pin().2,
            "boundary":"actual released pack/install/list; no daemon, services, accounts or output",
            "identity":"direct private executable path; SHA-256 and size before and after; no live proc claim",
            "excluded":["Cloud provenance consumption","Store signatures and trust","updater install",
                "complete product folder set","physical output","power-loss durability"],
            "package_implementation_changed_between_releases":false
        }),
    );
    println!("skills package-pair evidence: {}", root.display());
    // Reuse ownership, transcript and artifact checks without the routine
    // constructor's bus/clock metadata. These two clock fields are never read.
    Case {
        transcript: output(&root.join("transcript.jsonl")),
        root,
        records: 0,
        deadline: Instant::now() + Duration::from_secs(90),
        offset: 0,
        day: 0,
    }
}

fn run(case: &mut Case, release: Release, stage: &str, args: &[&str], success: bool) -> String {
    assert!(Instant::now() < case.deadline, "case deadline");
    let executable = case.binary(release);
    let before = fs::read(&executable).unwrap();
    assert_eq!(sha(&before), release.pin().0);
    assert_eq!(before.len(), release.pin().1);
    let input = (args.first() == Some(&"install")).then(|| {
        let path = case.root.join(args[3]);
        let metadata = fs::metadata(&path).unwrap();
        assert!(metadata.len() <= MAX_LOG);
        (
            path,
            fs::read(case.root.join(args[3])).unwrap(),
            metadata.permissions().mode() & 0o7777,
        )
    });
    let child = Command::new(&executable)
        .args(args)
        .env_clear()
        .current_dir(&case.root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(output(
            &case.root.join(format!("{stage}.stdout")),
        )))
        .stderr(Stdio::from(output(
            &case.root.join(format!("{stage}.stderr")),
        )))
        .process_group(0)
        .spawn()
        .unwrap();
    // A fast CLI can exit before /proc is read. Own it immediately, use
    // direct exec of the pinned private copy, and retain its actual exit.
    let mut owned = OwnedChild {
        child,
        stage: stage.into(),
        root: case.root.clone(),
        reaped: false,
    };
    case.record(json!({"event":"spawn","stage":stage,"pid":owned.child.id(),
        "release":release.number(),"argv":args,"environment":{},"stdin":"null",
        "executable_sha256_before":sha(&before),"identity_basis":"direct private executable"}));
    let status = owned.wait(case);
    let after = fs::read(&executable).unwrap();
    assert_eq!(after, before, "executed artifact changed");
    if let Some((path, bytes, mode)) = input {
        assert_eq!(
            fs::read(&path).unwrap(),
            bytes,
            "install changed its input package"
        );
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o7777,
            mode
        );
        case.record(json!({"event":"package_input_unchanged","stage":stage,
            "path":args[3],"bytes":bytes.len(),"mode":mode,"sha256":sha(&bytes)}));
    }
    let stdout = text(&case.root.join(format!("{stage}.stdout")));
    let stderr = owned.log();
    case.record(
        json!({"event":"command_complete","stage":stage,"code":status.code(),
        "signal":status.signal(),"stdout_sha256":sha(stdout.as_bytes()),
        "stderr_sha256":sha(stderr.as_bytes()),"executable_sha256_after":sha(&after)}),
    );
    assert_eq!(status.signal(), None, "{stage}: {stderr}");
    assert_eq!(
        status.code(),
        Some(i32::from(!success)),
        "{stage}: {stdout}\n{stderr}"
    );
    if !success {
        assert!(
            stdout.is_empty(),
            "refused install reported success: {stdout}"
        );
    }
    stdout
}

fn source(case: &Case, label: &str, name: &str, version: &str, tool: &str, enabled: bool) {
    let parent = case.root.join("sources").join(label);
    directory(&parent);
    let folder = parent.join(name);
    directory(&folder);
    write(&folder.join("SKILL.md"), format!(
        "---\nname: {name}\ndescription: A synthetic local package compatibility fixture.\n---\n# Package fixture\n"
    ).as_bytes());
    write(&folder.join("skill.toml"), format!(
        "kind = \"installable\"\nversion = \"{version}\"\nenabled = {enabled}\ntool = \"{tool}\"\n\n[permissions]\nspeak = true\n\n[[do]]\nsay = \"Synthetic package {version}.\"\n"
    ).as_bytes());
    write(
        &folder.join("notes.txt"),
        format!("Synthetic inert asset for {name} {version}.\n").as_bytes(),
    );
}

fn freeze_sources(case: &mut Case) -> Tree {
    let root = case.root.join("sources");
    for (name, entry) in tree(&root) {
        fs::set_permissions(
            root.join(name),
            fs::Permissions::from_mode(if entry.bytes.is_some() { 0o444 } else { 0o555 }),
        )
        .unwrap();
    }
    fs::set_permissions(&root, fs::Permissions::from_mode(0o555)).unwrap();
    let kept = tree(&root);
    json_file(&case.root.join("sources-before.json"), &tree_receipt(&kept));
    kept
}

fn pack(
    case: &mut Case,
    release: Release,
    stage: &str,
    source: &str,
    name: &str,
    version: &str,
) -> String {
    let out = format!("packages/{stage}");
    directory(&case.root.join(&out));
    let path = format!("{out}/{name}-{version}.tar");
    assert_eq!(
        run(case, release, stage, &["pack", "--out", &out, source], true),
        format!("packed {path}\n")
    );
    let bytes = fs::read(case.root.join(&path)).unwrap();
    assert!(bytes.len() as u64 <= MAX_LOG);
    fs::set_permissions(case.root.join(&path), fs::Permissions::from_mode(0o444)).unwrap();
    case.record(json!({"event":"package_written","stage":stage,"path":path,
        "bytes":bytes.len(),"sha256":sha(&bytes),"retained_mode":0o444}));
    path
}

fn install(case: &mut Case, release: Release, stage: &str, package: &str) {
    let stdout = run(
        case,
        release,
        stage,
        &["install", "--dir", LIVE, package],
        true,
    );
    assert!(
        stdout.starts_with("installed ") || stdout.starts_with("replaced "),
        "{stdout}"
    );
    assert!(stdout.contains("from its package"), "{stdout}");
    assert!(stdout.contains("it carries no signature"), "{stdout}");
}

#[derive(Debug, PartialEq, Eq)]
struct Row {
    kind: String,
    version: String,
    may_use: String,
}

fn inventory(stdout: &str) -> (BTreeMap<String, Row>, Vec<String>) {
    let mut lines = stdout.lines();
    assert_eq!(
        lines.next().unwrap().split_whitespace().collect::<Vec<_>>(),
        ["NAME", "KIND", "VERSION", "MAY", "USE"]
    );
    let mut rows = BTreeMap::new();
    let mut summary = None;
    for line in lines {
        if line.starts_with("The skills here give the model ") {
            assert!(summary.replace(line).is_none());
            continue;
        }
        assert!(summary.is_none(), "unexpected text after inventory summary");
        let mut rest = line;
        let mut word = || {
            rest = rest.trim_start();
            let end = rest.find(char::is_whitespace).expect("inventory column");
            let value = rest[..end].to_owned();
            rest = &rest[end..];
            value
        };
        let name = word();
        let kind = word();
        let version = word();
        let row = Row {
            kind,
            version,
            may_use: rest.trim().into(),
        };
        assert!(rows.insert(name, row).is_none(), "duplicate inventory row");
    }
    let summary = summary.expect("inventory summary");
    let functions = if summary == "The skills here give the model no function." {
        Vec::new()
    } else {
        let (count, names) = summary
            .strip_prefix("The skills here give the model ")
            .unwrap()
            .split_once(" function")
            .unwrap();
        let names = names
            .strip_prefix("s (")
            .or_else(|| names.strip_prefix(" ("))
            .unwrap()
            .split_once("). A session takes 32,")
            .unwrap()
            .0;
        let names: Vec<_> = names.split(", ").map(str::to_owned).collect();
        assert_eq!(names.len(), count.parse::<usize>().unwrap());
        names
    };
    (rows, functions)
}

fn listed(case: &mut Case, release: Release, stage: &str, version: &str) {
    let before = tree(&case.root.join(LIVE));
    let stdout = run(case, release, stage, &["list", "--dir", LIVE], true);
    let (rows, functions) = inventory(&stdout);
    assert_eq!(
        rows,
        BTreeMap::from([
            (
                NAME.into(),
                Row {
                    kind: "installable".into(),
                    version: version.into(),
                    may_use: "speak".into()
                }
            ),
            (
                HIDDEN.into(),
                Row {
                    kind: "installable".into(),
                    version: "1.0.0".into(),
                    may_use: "disabled; speak".into()
                }
            ),
        ])
    );
    assert_eq!(functions, [TOOL]);
    assert_eq!(
        tree(&case.root.join(LIVE)),
        before,
        "list changed installed files"
    );
    case.record(
        json!({"event":"inventory_checked","stage":stage,"version":version,
        "enabled_functions":functions,"disabled_tool_absent":true,"installed_unchanged":true}),
    );
}

struct Package {
    manifest: Value,
    files: BTreeMap<String, Vec<u8>>,
}

impl Package {
    fn read(path: &Path) -> Self {
        let mut archive = tar::Archive::new(File::open(path).unwrap());
        let mut entries = BTreeMap::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            assert!(entry.header().entry_type().is_file());
            let path = entry.path().unwrap().to_str().unwrap().to_owned();
            assert!(entry.size() <= MAX_LOG && entries.len() < 16);
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            assert!(entries.insert(path, bytes).is_none());
        }
        let manifest = serde_json::from_slice(&entries.remove(MANIFEST).unwrap()).unwrap();
        Self {
            manifest,
            files: entries,
        }
    }

    fn write(&self, path: &Path, update_hashes: bool) {
        let mut manifest = self.manifest.clone();
        if update_hashes {
            let prefix = format!("{}/", manifest["name"].as_str().unwrap());
            manifest["files"] = json!(self.files.iter().map(|(path, bytes)| json!({
                "path":path.strip_prefix(&prefix).unwrap(),"size":bytes.len(),"sha256":sha(bytes)
            })).collect::<Vec<_>>());
        }
        let mut archive = tar::Builder::new(Vec::new());
        let manifest = serde_json::to_vec(&manifest).unwrap();
        for (name, bytes) in std::iter::once((MANIFEST, manifest.as_slice())).chain(
            self.files
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        ) {
            let mut header = tar::Header::new_ustar();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(bytes.len() as u64);
            header.set_mtime(0);
            header.set_uid(0);
            header.set_gid(0);
            header.set_cksum();
            archive.append_data(&mut header, name, bytes).unwrap();
        }
        write(path, &archive.into_inner().unwrap());
    }
}

fn installed(case: &mut Case, source: &str, name: &str, package: &str, stage: &str) {
    let expected = tree(&case.root.join(source));
    let mut actual = tree(&case.root.join(LIVE).join(name));
    let provenance = actual.remove(".package.json").unwrap();
    assert_eq!(provenance.mode, 0o644);
    let record: Value = serde_json::from_slice(provenance.bytes.as_ref().unwrap()).unwrap();
    let package_path = case.root.join(package);
    let read = Package::read(&package_path);
    assert_eq!(read.manifest["format"], 1);
    assert_eq!(read.manifest["name"], name);
    assert_eq!(
        record,
        json!({"sha256":sha(&fs::read(&package_path).unwrap()),"manifest":read.manifest})
    );
    assert_eq!(actual.len(), expected.len());
    for (path, entry) in expected {
        assert_eq!(actual[&path].bytes, entry.bytes, "installed {path}");
        assert_eq!(
            actual[&path].mode,
            if entry.bytes.is_some() { 0o644 } else { 0o755 }
        );
    }
    case.record(
        json!({"event":"installed_package_bytes_checked","stage":stage,
        "skill":name,"package_sha256":record["sha256"],"provenance_reader_executed":false}),
    );
}

fn negatives(case: &mut Case, writer: Release, reader: Release, package: &str, duplicate: &str) {
    let mut cases = Vec::new();
    for (name, why) in [
        ("format", "format 2 is not one this lamp reads (1)"),
        (
            "digest",
            "its size or sha256 is not what lamp-skill.json says",
        ),
        ("provenance", "none beginning with a dot"),
        ("field", "unknown field `future_option`"),
        ("kind", "kind = \"future\""),
        ("permission", "say needs speak = true"),
    ] {
        let mut broken = Package::read(&case.root.join(package));
        match name {
            "format" => broken.manifest["format"] = json!(2),
            "digest" => broken.files.get_mut("pair-kit/notes.txt").unwrap()[0] ^= 1,
            "provenance" => {
                broken
                    .files
                    .insert("pair-kit/.package.json".into(), b"{}".to_vec());
            }
            "field" | "kind" | "permission" => {
                let toml =
                    String::from_utf8(broken.files.remove("pair-kit/skill.toml").unwrap()).unwrap();
                let changed = match name {
                    "field" => format!("future_option = true\n{toml}"),
                    "kind" => toml.replacen("kind = \"installable\"", "kind = \"future\"", 1),
                    _ => toml.replacen("speak = true", "speak = false", 1),
                };
                broken
                    .files
                    .insert("pair-kit/skill.toml".into(), changed.into_bytes());
            }
            _ => unreachable!(),
        }
        let path = format!("negative/{name}.tar");
        broken.write(&case.root.join(&path), name != "digest");
        cases.push((path, why));
    }
    cases.push((
        duplicate.into(),
        "tool \"fixture_hidden\" is the installable skill sleeping-kit's",
    ));
    let frozen = tree(&case.root.join(LIVE));
    for (phase, release) in [("reader", reader), ("return", writer)] {
        for (number, (path, why)) in cases.iter().enumerate() {
            let stage = format!("negative-{phase}-{number}");
            run(
                case,
                release,
                &stage,
                &["install", "--dir", LIVE, path],
                false,
            );
            assert!(
                text(&case.root.join(format!("{stage}.stderr"))).contains(why),
                "{stage}: expected {why}"
            );
            assert_eq!(
                tree(&case.root.join(LIVE)),
                frozen,
                "{stage} replaced or left staging files beside valid skills"
            );
            case.record(json!({"event":"refused_unchanged","stage":stage,"reason":why,"input_sha256":sha(&fs::read(case.root.join(path)).unwrap())}));
        }
        listed(
            case,
            release,
            &format!("positive-after-{phase}-refusals"),
            "1.1.0",
        );
    }
    snapshot(case, "after-refusals");
}

fn not_loaded(case: &mut Case, writer: Release, reader: Release, package: &str) {
    run(
        case,
        writer,
        "inventory-positive-install",
        &["install", "--dir", "inventory", package],
        true,
    );
    let invalid = case.root.join("inventory/future-kit");
    directory(&invalid);
    write(
        &invalid.join("SKILL.md"),
        b"---\nname: future-kit\ndescription: A synthetic unsupported kind.\n---\n",
    );
    write(
        &invalid.join("skill.toml"),
        b"kind = \"future\"\nversion = \"1.0.0\"\n",
    );
    let before = tree(&case.root.join("inventory"));
    for (stage, release) in [("inventory-reader", reader), ("inventory-return", writer)] {
        let stdout = run(case, release, stage, &["list", "--dir", "inventory"], true);
        let (rows, functions) = inventory(&stdout);
        assert_eq!(
            rows,
            BTreeMap::from([
                (
                    NAME.into(),
                    Row {
                        kind: "installable".into(),
                        version: "1.1.0".into(),
                        may_use: "speak".into()
                    }
                ),
                (
                    "future-kit".into(),
                    Row {
                        kind: "installable".into(),
                        version: "1.0.0".into(),
                        may_use:
                            "not loaded: kind = \"future\": \"core\", \"installable\" or \"addon\""
                                .into()
                    }
                ),
            ])
        );
        assert_eq!(functions, [TOOL]);
        assert_eq!(tree(&case.root.join("inventory")), before);
        case.record(
            json!({"event":"not_loaded_row_checked","stage":stage,"cli_exit":0,
            "refused_skill":"future-kit","positive_functions":functions,"files_unchanged":true}),
        );
    }
}

pub fn packages(writer: Release, reader: Release) {
    let mut case = new_case(writer, reader);
    source(&case, "v1", NAME, "1.0.0", TOOL, true);
    source(&case, "v2", NAME, "1.1.0", TOOL, true);
    source(&case, "disabled", HIDDEN, "1.0.0", HIDDEN_TOOL, false);
    source(
        &case,
        "duplicate",
        "duplicate-kit",
        "1.0.0",
        HIDDEN_TOOL,
        true,
    );
    let sources = freeze_sources(&mut case);
    let first = pack(
        &mut case,
        writer,
        "pack-a",
        "sources/v1/pair-kit",
        NAME,
        "1.0.0",
    );
    let hidden = pack(
        &mut case,
        writer,
        "pack-hidden",
        "sources/disabled/sleeping-kit",
        HIDDEN,
        "1.0.0",
    );
    let duplicate = pack(
        &mut case,
        writer,
        "pack-duplicate",
        "sources/duplicate/duplicate-kit",
        "duplicate-kit",
        "1.0.0",
    );
    install(&mut case, writer, "install-a", &first);
    install(&mut case, writer, "install-hidden", &hidden);
    installed(&mut case, "sources/v1/pair-kit", NAME, &first, "install-a");
    installed(
        &mut case,
        "sources/disabled/sleeping-kit",
        HIDDEN,
        &hidden,
        "install-hidden",
    );
    let a = snapshot(&mut case, "writer");
    listed(&mut case, reader, "list-b", "1.0.0");
    install(&mut case, reader, "cross-install-b", &first);
    assert_eq!(
        snapshot(&mut case, "cross-reader"),
        a,
        "same package reinstall changed bytes or modes"
    );

    let next = pack(
        &mut case,
        reader,
        "pack-b",
        "sources/v2/pair-kit",
        NAME,
        "1.1.0",
    );
    assert_ne!(
        sha(&fs::read(case.root.join(&first)).unwrap()),
        sha(&fs::read(case.root.join(&next)).unwrap())
    );
    install(&mut case, reader, "replace-b", &next);
    installed(&mut case, "sources/v2/pair-kit", NAME, &next, "replace-b");
    installed(
        &mut case,
        "sources/disabled/sleeping-kit",
        HIDDEN,
        &hidden,
        "companion-after-replacement",
    );
    let b = snapshot(&mut case, "reader-replacement");
    listed(&mut case, writer, "list-return", "1.1.0");
    install(&mut case, writer, "cross-install-return", &next);
    assert_eq!(snapshot(&mut case, "return"), b);
    negatives(&mut case, writer, reader, &next, &duplicate);
    not_loaded(&mut case, writer, reader, &next);
    let after = tree(&case.root.join("sources"));
    json_file(&case.root.join("sources-after.json"), &tree_receipt(&after));
    assert_eq!(after, sources, "versioned fixture sources changed");
    case.finish();
}
