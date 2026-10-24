use ring::digest::{SHA256, digest};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        process::{CommandExt, ExitStatusExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const SKILLS: &str = "/var/lib/lampOS/skills";
pub const OFF_PARENT: &str = "/var/lib/lampos-skills-state";
pub const MARKER: &str = "/run/lamp-cloud-pair-isolated";
pub const MARKER_BYTES: &[u8] = b"lamp-cloud-provenance-pair network-none tmpfs-v1\n";
pub const MAX: u64 = 256 * 1024;

pub fn sha(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn directory(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
pub fn file(path: &Path) -> File {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
}
pub fn write(path: &Path, bytes: &[u8]) {
    let mut f = file(path);
    f.write_all(bytes).unwrap();
    f.sync_all().unwrap();
}
pub fn read(path: &Path, limit: u64) -> Vec<u8> {
    let m = fs::symlink_metadata(path).unwrap();
    assert!(
        m.is_file() && m.nlink() == 1 && m.len() <= limit,
        "bounded plain file {}",
        path.display()
    );
    let mut bytes = Vec::new();
    File::open(path)
        .unwrap()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() as u64 <= limit);
    bytes
}
pub fn json_file(path: &Path, value: &Value) {
    write(path, &serde_json::to_vec_pretty(value).unwrap());
}
pub fn record(root: &Path, value: Value) {
    let path = root.join("transcript.jsonl");
    assert!(
        fs::metadata(&path).unwrap().len() < 1024 * 1024,
        "bounded transcript"
    );
    let mut f = OpenOptions::new().append(true).open(path).unwrap();
    serde_json::to_writer(&mut f, &value).unwrap();
    f.write_all(b"\n").unwrap();
    f.flush().unwrap();
}

pub fn isolation() {
    assert_eq!(std::env::consts::OS, "linux");
    assert_eq!(std::env::consts::ARCH, "aarch64");
    assert_eq!(read(Path::new(MARKER), 128), MARKER_BYTES);
    let marker = fs::symlink_metadata(MARKER).unwrap();
    assert_eq!(marker.uid(), 0);
    assert_eq!(marker.mode() & 0o022, 0, "root-owned isolation marker");
    let interfaces: Vec<_> = fs::read_dir("/sys/class/net")
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        interfaces,
        [std::ffi::OsString::from("lo")],
        "network-none container, loopback only"
    );
    let routes = fs::read_to_string("/proc/net/route").unwrap();
    assert!(
        routes
            .lines()
            .skip(1)
            .all(|l| l.split_whitespace().nth(1) != Some("00000000")),
        "no default route"
    );
    assert!(
        fs::read_to_string("/proc/self/mountinfo")
            .unwrap()
            .lines()
            .any(|l| {
                let Some((left, right)) = l.split_once(" - ") else {
                    return false;
                };
                left.split_whitespace().nth(4) == Some("/var/lib")
                    && right.split_whitespace().next() == Some("tmpfs")
            }),
        "fixed lamp paths must live on a private /var/lib tmpfs"
    );
    assert!(fs::symlink_metadata("/var/lib").unwrap().is_dir());
    for path in ["/var/lib/lampOS", OFF_PARENT] {
        assert_eq!(
            fs::symlink_metadata(path).unwrap_err().kind(),
            std::io::ErrorKind::NotFound,
            "fixed paths must be absent before creation"
        );
    }
}

#[derive(Clone)]
pub struct Artifact {
    pub release: u64,
    pub part: &'static str,
    pub source: PathBuf,
    pub path: PathBuf,
    pub pin: &'static str,
    pub size: usize,
    source_mode: u32,
}
impl Artifact {
    pub fn new(root: &Path, part: &'static str, release: u64) -> Self {
        let (pin, size) = match (part, release) {
            ("skills", 6408) => (
                "2c6bc8f34c9fd4a56784a54fa2d03b1e76be9d1833ccf37db905f88dba67d3d5",
                8_807_104,
            ),
            ("skills", 6498) => (
                "87e500127793db88df8bcd7af793879faeee95f36a8e0d2ef8483c59fef26984",
                8_786_048,
            ),
            ("cloud", 6408) => (
                "9f3abad362555ad11177bdf41706e91d740bca8689928a8555c4ce3f257fc664",
                10_802_168,
            ),
            ("cloud", 6498) => (
                "6d4d566d837dad9df137b475557f330deb62a84405128b879686368a9de30bf4",
                10_804_480,
            ),
            _ => panic!("unsupported fixed release"),
        };
        let source = PathBuf::from(
            std::env::var_os(format!("LAMP_COMPAT_{}_{release}", part.to_uppercase()))
                .expect("explicit pinned artifact"),
        );
        assert!(source.is_absolute());
        let path = root.join(format!("lamp-{part}-{release}"));
        let source_mode = fs::symlink_metadata(&source).unwrap().mode() & 0o7777;
        let a = Self {
            release,
            part,
            source,
            path,
            pin,
            size,
            source_mode,
        };
        let bytes = a.checked(&a.source);
        write(&a.path, &bytes);
        fs::set_permissions(&a.path, fs::Permissions::from_mode(0o555)).unwrap();
        a.verify();
        a
    }
    fn checked(&self, path: &Path) -> Vec<u8> {
        let bytes = read(path, self.size as u64);
        assert_eq!(bytes.len(), self.size);
        assert_eq!(sha(&bytes), self.pin);
        assert_eq!(&bytes[..6], b"\x7fELF\x02\x01");
        assert_eq!(&bytes[18..20], &[0xb7, 0]);
        bytes
    }
    pub fn verify(&self) {
        assert_eq!(self.checked(&self.source), self.checked(&self.path));
        assert_eq!(
            fs::symlink_metadata(&self.source).unwrap().mode() & 0o7777,
            self.source_mode
        );
        assert_eq!(
            fs::symlink_metadata(&self.path).unwrap().mode() & 0o7777,
            0o555
        );
    }
    pub fn receipt(&self) -> Value {
        json!({"release":self.release,"part":self.part,"sha256":self.pin,"bytes":self.size})
    }
}

pub struct Owned {
    pub child: Child,
    root: PathBuf,
    stage: String,
    reaped: bool,
}
impl Owned {
    pub fn spawn(
        root: &Path,
        stage: &str,
        artifact: &Artifact,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Self {
        artifact.verify();
        let child = Command::new(&artifact.path)
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C")
            .envs(env.iter().copied())
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::from(file(&root.join(format!("{stage}.stdout")))))
            .stderr(Stdio::from(file(&root.join(format!("{stage}.stderr")))))
            .process_group(0)
            .spawn()
            .unwrap();
        let owned = Self {
            child,
            root: root.into(),
            stage: stage.into(),
            reaped: false,
        };
        record(
            root,
            json!({"event":"spawn","stage":stage,"pid":owned.child.id(),"artifact":artifact.receipt(),"argv":args,"environment":env,"direct_private_executable":true}),
        );
        owned
    }
    fn note(&mut self, status: ExitStatus) {
        self.reaped = true;
        record(
            &self.root,
            json!({"event":"reaped","stage":self.stage,"pid":self.child.id(),"code":status.code(),"signal":status.signal()}),
        );
    }
    pub fn wait(&mut self, until: Instant) -> ExitStatus {
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                self.note(status);
                return status;
            }
            assert!(Instant::now() < until, "bounded child exit: {}", self.stage);
            thread::sleep(Duration::from_millis(5));
        }
    }
    pub fn stop(&mut self) {
        self.alive();
        self.child.kill().unwrap();
        let status = self.wait(Instant::now() + Duration::from_secs(3));
        assert_eq!(status.signal(), Some(9));
        assert_eq!(status.code(), None);
    }
    pub fn alive(&mut self) {
        if let Some(status) = self.child.try_wait().unwrap() {
            self.note(status);
            panic!("reader exited before the cut: {}", self.stage);
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        let _ = self.child.kill();
        let until = Instant::now() + Duration::from_secs(3);
        let mut status = None;
        while Instant::now() < until {
            match self.child.try_wait() {
                Ok(Some(s)) => {
                    self.reaped = true;
                    status = Some(s);
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(5)),
                Err(_) => break,
            }
        }
        // Fallible logging during unwinding cannot cause a second panic.
        if let Ok(mut f) = OpenOptions::new()
            .append(true)
            .open(self.root.join("transcript.jsonl"))
        {
            let _ = writeln!(
                f,
                "{}",
                json!({"event":"failure_cleanup","stage":self.stage,"pid":self.child.id(),"reaped":self.reaped,"code":status.and_then(|s|s.code()),"signal":status.and_then(|s|s.signal())})
            );
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Node {
    pub mode: u32,
    pub bytes: Option<Vec<u8>>,
    inode: u64,
    device: u64,
    modified: (i64, i64),
}
pub type Tree = BTreeMap<String, Node>;
pub fn tree(root: &Path) -> Tree {
    fn visit(root: &Path, path: &Path, out: &mut Tree) {
        let m = fs::symlink_metadata(path).unwrap();
        assert!(m.is_file() || m.is_dir());
        assert!(out.len() < 128);
        let name = if path == root {
            ".".into()
        } else {
            path.strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned()
        };
        let bytes = m.is_file().then(|| read(path, MAX));
        out.insert(
            name,
            Node {
                mode: m.mode() & 0o7777,
                bytes,
                inode: m.ino(),
                device: m.dev(),
                modified: (m.mtime(), m.mtime_nsec()),
            },
        );
        if m.is_dir() {
            for e in fs::read_dir(path).unwrap() {
                visit(root, &e.unwrap().path(), out);
            }
        }
    }
    let mut out = Tree::new();
    visit(root, root, &mut out);
    out
}
pub fn tree_json(t: &Tree) -> Value {
    json!(t.iter().map(|(name,n)| json!({"path":name,"mode":n.mode,"inode":n.inode,"device":n.device,"mtime":n.modified,"bytes":n.bytes.as_ref().map(Vec::len),"sha256":n.bytes.as_ref().map(|b|sha(b))})).collect::<Vec<_>>())
}
pub fn snapshot(root: &Path, label: &str, t: &Tree) {
    let dest = root.join(format!("snapshot-{label}"));
    directory(&dest);
    for (name, n) in t {
        if name == "." {
            continue;
        }
        match &n.bytes {
            Some(b) => write(&dest.join(name), b),
            None => directory(&dest.join(name)),
        }
    }
    json_file(&dest.join("files.json"), &tree_json(t));
}
