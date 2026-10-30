//! Publish and subscribe between the lamp's processes, with no process in
//! the middle. See `docs/bus.md`.
//!
//! The lamp is several processes, each somebody's own, and the rule is that
//! one of them failing takes none of the others down. A broker would be the
//! one process all of them need. So a topic here is a directory, a
//! subscriber is a datagram socket in it, and a publisher sends each message
//! to every socket it finds there without waiting for any: a subscriber that
//! is slow, stopped or dead loses messages and nobody else notices.
//!
//! What is lost is not hidden. Every message carries its publisher's count,
//! and a subscriber is told with each one how many it missed before it.
//!
//! ```no_run
//! use lamp_bus::{Bus, Kind};
//! use std::time::Duration;
//!
//! let bus = Bus::from_environment()?;
//! let mut servos = bus.publisher("servos", Kind::Latest, "body")?;
//! servos.publish_json(98_123_456, &serde_json::json!({"yaw": 1990}))?;
//!
//! let mut follow = bus.subscribe("servos", "follow")?;
//! if let Some(message) = follow.recv(Duration::from_millis(100))? {
//!     println!("{} missed, yaw {}", message.gap, message.json()?["yaw"]);
//! }
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! A publisher can also say what its topic carries, and a subscriber what it
//! expects: the two are then held to the same type, the subscriber learns
//! when the source has gone or the topic is late, and a message that has
//! outlived its time is not handed over.
//!
//! ```no_run
//! use lamp_bus::{Bus, Declaration, Kind, Notice};
//! use std::time::Duration;
//!
//! let bus = Bus::from_environment()?;
//! let joints = Declaration::new("lamp_msgs/JointState", 1, Kind::Latest)
//!     .every(Duration::from_millis(50))
//!     .lasting(Duration::from_millis(500));
//! let mut body = bus.publisher_declared("body/joint_states", &joints, "body")?;
//! body.publish_json(98_123_456, &serde_json::json!({"position": [0.1]}))?;
//!
//! let mut follow = bus.subscribe_typed("body/joint_states", "follow", "lamp_msgs/JointState")?;
//! // In the subscriber's loop: a message if there is one, then what has
//! // changed about the topic since it last asked.
//! if let Some(message) = follow.recv(Duration::from_millis(100))? {
//!     println!("{}", message.json()?);
//! }
//! for notice in follow.notices() {
//!     match notice {
//!         Notice::SourceGone => println!("the body has gone"),
//!         Notice::Late => println!("no joints for 100 ms"),
//!         _ => {}
//!     }
//! }
//! # Ok::<(), std::io::Error>(())
//! ```
mod clock;
mod declare;
mod live;
pub mod logging;
mod name;
mod publish;
mod subscribe;
mod wire;

pub use clock::{BOOT_ID_PATH, kernel_boot_id, monotonic_us};
pub use declare::Declaration;
pub use live::Source;
pub use name::MOST_TOPIC_BYTES;
pub use publish::{MOST_SUBSCRIBERS, Published, Publisher, SubscriberTotals, Totals};
pub use subscribe::{Notice, Statistics, Subscriber};
pub use wire::{EXPIRY_GRACE_MS, Header, JSON, Kind, MOST_BYTES, Message, NONE, Now, StampClock};

use std::{
    env, fs, io,
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, PermissionsExt},
        net::UnixDatagram,
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// Where the topics are when nothing else is said: memory, not a disk, and
/// empty after a restart of the lamp.
pub const DEFAULT_ROOT: &str = "/run/lampos-bus";
/// The environment variable that says otherwise.
pub const ROOT_VARIABLE: &str = "LAMP_BUS_ROOT";

/// How long a publisher may go without showing itself alive before it is
/// taken for dead, unless the `Bus` is told another.
pub const DEFAULT_LEASE: Duration = Duration::from_secs(3);

/// The longest path a subscriber's socket may have. A Unix socket's address
/// holds 104 bytes on macOS and 108 on Linux, the last of them a NUL, and
/// what is tried on one must hold on the other.
pub const MOST_SOCKET_PATH_BYTES: usize = 103;

/// Sockets and the `latest` file: the owner and the group, nobody else.
const FILE_MODE: u32 = 0o660;

#[derive(Clone, Debug)]
pub struct Bus {
    root: PathBuf,
    directory_mode: u32,
    rescan_every: Duration,
    lease: Duration,
    clocks: clock::Clocks,
}

/// One subscriber's socket as a topic's `subs/` shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listener {
    /// The socket without `.sock`: the subscriber's name, its process and
    /// a count.
    pub socket: String,
    /// There is a process behind it. A socket with none was left by a
    /// subscriber that died, and the topic's next publish removes it.
    pub listening: bool,
}

/// One topic as the directories show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Topic {
    pub name: String,
    /// Sockets in `subs/` with a process behind them.
    pub subscribers: usize,
    /// Sockets left by a subscriber that died. The topic's next publish
    /// removes them.
    pub stale: usize,
    /// There is a `latest` file.
    pub latest: bool,
}

impl Bus {
    /// The bus under `root`. Nothing is created until somebody publishes a
    /// value that is kept, or subscribes.
    pub fn new(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        // A socket is addressed by its path, and a relative one would mean
        // another place to every process with another working directory.
        if !root.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("bus root {} is not an absolute path", root.display()),
            ));
        }
        Ok(Self {
            root,
            directory_mode: 0o770,
            rescan_every: Duration::from_millis(200),
            lease: DEFAULT_LEASE,
            clocks: clock::Clocks::default(),
        })
    }

    /// The bus under `LAMP_BUS_ROOT`, or under `/run/lampos-bus`.
    pub fn from_environment() -> io::Result<Self> {
        match env::var_os(ROOT_VARIABLE) {
            Some(root) if !root.is_empty() => Self::new(root),
            _ => Self::new(DEFAULT_ROOT),
        }
    }

    /// The mode of every directory this bus creates, whatever the umask.
    pub fn directory_mode(mut self, mode: u32) -> Self {
        self.directory_mode = mode;
        self
    }

    /// How long a publisher goes on with the subscribers it knows before it
    /// reads the directory again: the longest a new subscriber waits for
    /// its first message, and how often a subscriber with nothing waiting
    /// looks at `latest`.
    pub fn rescan_every(mut self, every: Duration) -> Self {
        self.rescan_every = every;
        self
    }

    /// How long a publisher may go without showing itself alive before it
    /// is taken for dead and its entry removed (three seconds unless said).
    /// A publisher refreshes its entry every third of this, and once a
    /// second at the least. Every process on one root has to use the same
    /// lease: with a shorter one a process takes its neighbours for dead.
    pub fn lease(mut self, lease: Duration) -> Self {
        self.lease = lease;
        self
    }

    /// Clocks of the caller's own in place of the host's: the wall clock in
    /// milliseconds since 1970, and CLOCK_MONOTONIC in microseconds. Every
    /// publisher and subscriber made from this bus reads them: a publisher
    /// for `at_unix_ms`, a subscriber for what has expired and what was
    /// late. For tests and simulators, which play a wall clock that a time
    /// server sets forward or back.
    pub fn clocks(
        mut self,
        wall_ms: impl Fn() -> u64 + Send + Sync + 'static,
        monotonic_us: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Self {
        self.clocks.set(wall_ms, monotonic_us);
        self
    }

    /// The boot this bus's processes take themselves to be in, in place of
    /// the kernel's ([`kernel_boot_id`]); `None` for none. For tests and
    /// simulators.
    pub fn boot_id(mut self, id: Option<&str>) -> Self {
        self.clocks.set_boot_id(id);
        self
    }

    /// The boot this bus takes itself to be in: what a publisher that
    /// stamps its monotonic clock says in `clock`, and what a subscriber
    /// compares a header's `clock` with.
    pub fn this_boot(&self) -> Option<&str> {
        self.clocks.boot_id()
    }

    /// What this bus's clocks say now, to judge a header by
    /// ([`Header::expired_by`]).
    pub fn now(&self) -> Now<'_> {
        self.clocks.now()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A publisher of `topic` called `source`. Touches nothing: a root that
    /// is not there yet is found, or not, with each publish. From its first
    /// message it is listed in the topic's `pubs/`.
    pub fn publisher(&self, topic: &str, kind: Kind, source: &str) -> io::Result<Publisher> {
        Publisher::new(self, topic, kind, source)
    }

    /// A publisher with an explicit clock for subscriber rescans and
    /// presence refreshes in a local fixture. Reads must be pure and in one
    /// nondecreasing Instant domain; the caller alone advances the clock.
    /// Construction does not sample it. Header UTC, filesystem lease times
    /// and real delivery keep their ordinary clocks and behavior.
    pub fn publisher_with_cadence_clock(
        &self,
        topic: &str,
        kind: Kind,
        source: &str,
        now: impl Fn() -> Instant + Send + Sync + 'static,
    ) -> io::Result<Publisher> {
        Publisher::new_with_cadence_clock(self, topic, kind, source, now)
    }

    /// A publisher that says what `topic` carries. The declaration is
    /// written to `<topic>/declaration`, every message names its type, and
    /// a declared lifespan is put in every header. It fails if the topic is
    /// already declared as another type or kind: two programs disagree
    /// about the topic, and the second is the one told.
    pub fn publisher_declared(
        &self,
        topic: &str,
        declaration: &Declaration,
        source: &str,
    ) -> io::Result<Publisher> {
        Publisher::declared(self, topic, declaration, source)
    }

    /// A declared publisher with the explicit cadence clock described by
    /// [`Self::publisher_with_cadence_clock`]. The clock is installed before
    /// the original first presence refresh, after the real declaration
    /// write. Invalid inputs and a failed declaration write do not sample it.
    pub fn publisher_declared_with_cadence_clock(
        &self,
        topic: &str,
        declaration: &Declaration,
        source: &str,
        now: impl Fn() -> Instant + Send + Sync + 'static,
    ) -> io::Result<Publisher> {
        Publisher::declared_with_cadence_clock(self, topic, declaration, source, now)
    }

    /// Bind a socket in the topic's `subs/`, creating the directories that
    /// are missing. Publishers already running find it at their next look.
    pub fn subscribe(&self, topic: &str, name: &str) -> io::Result<Subscriber> {
        Subscriber::new(self, topic, name, None)
    }

    /// Subscribe to `topic` expecting messages of `type_name`. It fails,
    /// with nothing created, if the topic is declared to carry another
    /// type. A topic that nobody has declared yet is subscribed to: its
    /// publisher may start later, and until then, and after, a message
    /// whose header names another type is passed over and counted
    /// (`Subscriber::wrong_type`).
    pub fn subscribe_typed(
        &self,
        topic: &str,
        name: &str,
        type_name: &str,
    ) -> io::Result<Subscriber> {
        Subscriber::new(self, topic, name, Some(type_name))
    }

    /// The topic's current value, if it keeps one. A value that has
    /// outlived its `expires_ms`, by the clock its header calls for
    /// ([`Header::expired_by`]), is returned all the same, with `expired`
    /// set: it is the last thing known, and the caller decides what an old
    /// value is worth.
    pub fn latest(&self, topic: &str) -> io::Result<Option<Message>> {
        name::topic(topic)?;
        let kept = wire::kept(&self.root.join(topic).join("latest"), topic)?;
        Ok(kept.map(|(header, payload)| Message {
            expired: header.expired_by(&self.now()),
            header,
            payload,
            gap: 0,
            publisher_restarted: false,
        }))
    }

    /// What the topic was declared to carry, if a publisher declared it.
    pub fn declaration(&self, topic: &str) -> io::Result<Option<Declaration>> {
        name::topic(topic)?;
        declare::read(&self.root.join(topic).join("declaration"))
    }

    /// The publishers of the topic that have shown themselves alive within
    /// the lease. Looking removes the entry of one that has not.
    pub fn publishers(&self, topic: &str) -> io::Result<Vec<Source>> {
        name::topic(topic)?;
        live::list(&self.root.join(topic).join("pubs"), self.lease)
    }

    /// The topic's subscribers, by socket, and whether each has a process
    /// behind it.
    pub fn subscribers(&self, topic: &str) -> io::Result<Vec<Listener>> {
        name::topic(topic)?;
        Ok(listeners(&self.root.join(topic).join("subs")))
    }

    /// Every topic that has a `subs/`, a `pubs/`, a `latest` or a
    /// declaration, by name: one that somebody has published, subscribed to
    /// or declared since the root was made.
    pub fn topics(&self) -> io::Result<Vec<Topic>> {
        let mut found = Vec::new();
        match self.look(&self.root, "", &mut found) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        found.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(found)
    }

    fn look(&self, directory: &Path, prefix: &str, found: &mut Vec<Topic>) -> io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let Ok(part) = entry.file_name().into_string() else {
                continue;
            };
            let name = format!("{prefix}{part}");
            if part.contains('/') || name::topic(&name).is_err() || !entry.file_type()?.is_dir() {
                continue;
            }
            let path = entry.path();
            let subs = path.join("subs");
            let latest = path.join("latest").is_file();
            if latest
                || subs.is_dir()
                || path.join("pubs").is_dir()
                || path.join("declaration").is_file()
            {
                let held = listeners(&subs);
                let subscribers = held.iter().filter(|held| held.listening).count();
                let stale = held.len() - subscribers;
                found.push(Topic {
                    name: name.clone(),
                    subscribers,
                    stale,
                    latest,
                });
            }
            // A directory that vanishes while it is read is not an error.
            let _ = self.look(&path, &format!("{name}/"), found);
        }
        Ok(())
    }
}

/// The sockets in `subs`, by name, and whether each has somebody behind
/// it. Connecting asks the kernel without sending anybody anything.
fn listeners(subs: &Path) -> Vec<Listener> {
    let Ok(entries) = fs::read_dir(subs) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let file = entry.file_name();
        // A subscriber's socket has its `.sock` name once it can be sent to.
        let Some(socket) = file.to_str().and_then(|file| file.strip_suffix(".sock")) else {
            continue;
        };
        if !entry.file_type().is_ok_and(|kind| kind.is_socket()) {
            continue;
        }
        let refused = UnixDatagram::unbound()
            .and_then(|probe| probe.connect(entry.path()))
            .is_err_and(|error| error.kind() == io::ErrorKind::ConnectionRefused);
        found.push(Listener {
            socket: socket.into(),
            listening: !refused,
        });
    }
    found.sort_by(|a, b| a.socket.cmp(&b.socket));
    found
}

/// Create `path` and whatever is missing above it, each with `mode`, and
/// with its parent's set-group-ID bit if the parent has one. A directory
/// that is already there is left as it is.
///
/// The bit is how programs of different users share a bus: the root is made
/// with it and a group they are all in, and Linux then gives everything
/// made below it that group. Setting a new directory's mode outright took
/// the bit off again: on lamp-4ace on 2026-10-13, the first time two users
/// used one root, a topic made by root could not be entered by the other
/// user, and a kept value written by root could not be read.
fn make_directory(path: &Path, mode: u32) -> io::Result<()> {
    const SET_GROUP_ID: u32 = 0o2000;
    let create = || match fs::DirBuilder::new().mode(mode).create(path) {
        // The umask may have taken bits away.
        Ok(()) => {
            let shared = path
                .parent()
                .and_then(|parent| fs::metadata(parent).ok())
                .map_or(0, |parent| parent.permissions().mode() & SET_GROUP_ID);
            fs::set_permissions(path, fs::Permissions::from_mode(mode | shared))
        }
        // Another process made it first.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        Err(error) => Err(error),
    };
    match create() {
        Err(error) if error.kind() == io::ErrorKind::NotFound => match path.parent() {
            Some(parent) => {
                make_directory(parent, mode)?;
                create()
            }
            None => Err(error),
        },
        done => done,
    }
}

#[cfg(test)]
mod directory_tests {
    #[test]
    fn a_new_directory_keeps_its_parents_shared_group_bit_and_has_none_without_it() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("lamp-bus-setgid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mode =
            |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o7777;
        // Without the bit on the root: exactly the mode asked for.
        super::make_directory(&root.join("plain/topic/subs"), 0o770).unwrap();
        assert_eq!(mode(&root.join("plain")), 0o770);
        assert_eq!(mode(&root.join("plain/topic/subs")), 0o770);
        // With it: every level below carries it on.
        let shared = root.join("shared");
        std::fs::create_dir(&shared).unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o2770)).unwrap();
        super::make_directory(&shared.join("voice/state/pubs"), 0o770).unwrap();
        for below in ["voice", "voice/state", "voice/state/pubs"] {
            assert_eq!(mode(&shared.join(below)), 0o2770, "{below}");
        }
        // One that is already there is left as it is.
        std::fs::set_permissions(shared.join("voice"), std::fs::Permissions::from_mode(0o750))
            .unwrap();
        super::make_directory(&shared.join("voice"), 0o770).unwrap();
        assert_eq!(mode(&shared.join("voice")), 0o750);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
