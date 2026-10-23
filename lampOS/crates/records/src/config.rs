//! One explicit file owns retention limits; unknown kinds cannot become paths.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io, path::Path};

pub const DEFAULT_BYTES: u64 = 500_000_000;
pub const DEFAULT_AGE_SECONDS: u64 = 14 * 24 * 60 * 60;
pub const MAX_ROOTS: usize = 4;

/// The schema this build writes and reads.
pub const SCHEMA: u32 = 1;

/// `records.json`. A field a newer build writes is read past and named once
/// ([`read`], #1680); a root `kind` this build doesn't know is refused.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub schema_version: u32,
    /// No default-on behavior, even if a timer has been installed.
    #[serde(default)]
    pub enabled: bool,
    pub bus_root: std::path::PathBuf,
    pub roots: Vec<RootConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RootConfig {
    pub kind: Kind,
    pub path: std::path::PathBuf,
    pub unit: String,
    pub max_bytes: u64,
    pub max_age_seconds: u64,
    /// Explicit per-root opt-in; omission keeps the historical private layout.
    #[serde(default)]
    pub session_read_group: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    VoiceServiceMetadata,
}

/// `records.json` as this build reads it, one release forward (#1680).
#[derive(Debug)]
pub enum Read {
    /// This build's schema: the configuration, and the journal's line
    /// naming the fields this build does not read, if any.
    Current(Config, Option<String>),
    /// A newer schema: this build prunes nothing with it. Its number, and
    /// its `bus_root` when that is still a bounded absolute path, so the
    /// reporter can say on the bus that the cleanup is paused.
    Newer {
        schema: u64,
        bus_root: Option<std::path::PathBuf>,
    },
}

/// `bytes`, `records.json` at `path`. A newer `schema_version` is told
/// apart before anything else is read, and never pruned with: what a bump
/// means is not this build's to guess, and a cleanup deletes. At this
/// build's schema, a field it doesn't know is read past and named
/// ([`crate::forward`]); a file that doesn't parse, a bad known value (a
/// root `kind` it doesn't know) and a key-like field are refused.
pub fn read(path: &Path, bytes: &[u8]) -> io::Result<Read> {
    #[derive(Deserialize)]
    struct Envelope {
        schema_version: u64,
        #[serde(default)]
        bus_root: Option<serde_json::Value>,
    }
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|error| crate::forward::unreadable(path, &error))?;
    if envelope.schema_version > u64::from(SCHEMA) {
        let bus_root = envelope
            .bus_root
            .as_ref()
            .and_then(serde_json::Value::as_str)
            .map(std::path::PathBuf::from)
            .filter(|root| absolute_path(root).is_ok());
        return Ok(Read::Newer {
            schema: envelope.schema_version,
            bus_root,
        });
    }
    let (config, unread) = crate::forward::read(path, bytes)?;
    Ok(Read::Current(config, unread))
}

pub fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

/// Reject ambiguous spellings rather than normalizing them after validation.
pub fn absolute_path(path: &Path) -> io::Result<()> {
    let text = path
        .to_str()
        .ok_or_else(|| invalid("paths must be UTF-8"))?;
    if !path.is_absolute()
        || text.len() > 4096
        || text.ends_with('/')
        || text.chars().any(char::is_control)
        || text
            .split('/')
            .skip(1)
            .any(|c| c.is_empty() || c == "." || c == "..")
        || text.split('/').count() > 65
    {
        return Err(invalid("paths must be bounded absolute normal paths"));
    }
    Ok(())
}

pub fn unit_name(unit: &str) -> io::Result<()> {
    if !unit.ends_with(".service")
        || unit.len() <= ".service".len()
        || unit.len() > 128
        || !unit
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.@".contains(&b))
        || !unit.as_bytes()[0].is_ascii_alphanumeric()
    {
        return Err(invalid("unit must be an explicit system .service name"));
    }
    Ok(())
}

/// A local group name, never a numeric ID, path, or NSS query.
fn session_read_group_name(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name.len() > 128
        || !name.as_bytes()[0].is_ascii_alphabetic()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(invalid(
            "session_read_group must be a bounded local group name",
        ));
    }
    Ok(())
}

/// Resolve once before opening a root. Read only the bounded local group file;
/// libc/NSS resolution can contact a network or block without a deadline.
pub(crate) fn session_reader_gid(name: &str) -> io::Result<u32> {
    session_read_group_name(name)?;
    let file: std::fs::File = rustix::fs::open(
        "/etc/group",
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?
    .into();
    if !file.metadata()?.is_file() {
        return Err(invalid("local group source must be a regular file"));
    }
    session_reader_gid_from(name, file)
}

fn session_reader_gid_from(name: &str, source: impl io::Read) -> io::Result<u32> {
    use io::Read;
    const MAX_GROUP_BYTES: u64 = 1024 * 1024;
    session_read_group_name(name)?;
    let mut bytes = Vec::new();
    source.take(MAX_GROUP_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_GROUP_BYTES {
        return Err(invalid("local group source exceeds its byte bound"));
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| invalid("local group source is not UTF-8"))?;
    let mut gid = None;
    for line in text.lines() {
        let mut fields = line.split(':');
        if fields.next() != Some(name) {
            continue;
        }
        let _password = fields
            .next()
            .ok_or_else(|| invalid("malformed local group"))?;
        let number = fields
            .next()
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|id| *id != u32::MAX)
            .ok_or_else(|| invalid("invalid local group ID"))?;
        if fields.next().is_none() || fields.next().is_some() || gid.replace(number).is_some() {
            return Err(invalid("local group must have one unique exact entry"));
        }
    }
    gid.ok_or_else(|| invalid("session-reader group is absent from the local group source"))
}

impl Config {
    pub fn fingerprint(&self) -> String {
        // Struct field order is stable; no maps or secrets occur in this file.
        // forward::read must serialize even a known null policy. Keep the
        // historical fingerprint bytes when the additive policy is absent.
        #[derive(Serialize)]
        struct FingerprintRoot<'a> {
            kind: Kind,
            path: &'a Path,
            unit: &'a str,
            max_bytes: u64,
            max_age_seconds: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            session_read_group: Option<&'a str>,
        }
        #[derive(Serialize)]
        struct Fingerprint<'a> {
            schema_version: u32,
            enabled: bool,
            bus_root: &'a Path,
            roots: Vec<FingerprintRoot<'a>>,
        }
        let fingerprint = Fingerprint {
            schema_version: self.schema_version,
            enabled: self.enabled,
            bus_root: &self.bus_root,
            roots: self
                .roots
                .iter()
                .map(|root| FingerprintRoot {
                    kind: root.kind,
                    path: &root.path,
                    unit: &root.unit,
                    max_bytes: root.max_bytes,
                    max_age_seconds: root.max_age_seconds,
                    session_read_group: root.session_read_group.as_deref(),
                })
                .collect(),
        };
        let bytes =
            serde_json::to_vec(&fingerprint).expect("retention configuration is serializable");
        format!("{:x}", Sha256::digest(bytes))
    }

    pub fn customer_default() -> Self {
        Self {
            schema_version: SCHEMA,
            enabled: false,
            bus_root: "/run/lampos-bus".into(),
            roots: vec![RootConfig {
                kind: Kind::VoiceServiceMetadata,
                path: "/var/lib/lampOS/voice/sessions".into(),
                unit: "lamp-voice.service".into(),
                max_bytes: DEFAULT_BYTES,
                max_age_seconds: DEFAULT_AGE_SECONDS,
                session_read_group: None,
            }],
        }
    }

    pub fn validate(&self) -> io::Result<()> {
        if self.schema_version != SCHEMA || self.roots.is_empty() || self.roots.len() > MAX_ROOTS {
            return Err(invalid("schema_version must be 1, with one to four roots"));
        }
        absolute_path(&self.bus_root)?;
        for (index, root) in self.roots.iter().enumerate() {
            absolute_path(&root.path)?;
            // This is an adapter for one writer's sessions, never a generic
            // cleanup of /var/lib, the journal, or a memory database directory.
            if root.path.file_name().is_none_or(|name| name != "sessions")
                || root.path.components().count() < 4
                || root.max_bytes == 0
                || root.max_bytes > 1_000_000_000_000
                || root.max_age_seconds == 0
                || root.max_age_seconds > 366 * 24 * 60 * 60
            {
                return Err(invalid(
                    "voice root must end in /sessions; limits must be positive and bounded",
                ));
            }
            unit_name(&root.unit)?;
            if let Some(group) = &root.session_read_group {
                session_read_group_name(group)?;
            }
            if self.bus_root.starts_with(&root.path) || root.path.starts_with(&self.bus_root) {
                return Err(invalid("bus and retention roots must be separate"));
            }
            for other in &self.roots[..index] {
                if root.path.starts_with(&other.path)
                    || other.path.starts_with(&root.path)
                    || root.unit == other.unit
                {
                    return Err(invalid("retention roots must not overlap or reuse a unit"));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn customer_defaults_are_explicit_and_off() {
        let mut value = serde_json::to_value(Config::customer_default()).unwrap();
        value.as_object_mut().unwrap().remove("enabled");
        let config: Config = serde_json::from_value(value).unwrap();
        assert!(!config.enabled);
        assert_eq!(config.roots[0].max_bytes, 500_000_000);
        assert_eq!(config.roots[0].max_age_seconds, 1_209_600);
        config.validate().unwrap();
        let mut changed = config.clone();
        changed.enabled = true;
        assert_ne!(config.fingerprint(), changed.fingerprint());
        changed = config.clone();
        changed.roots[0].max_age_seconds += 1;
        assert_ne!(config.fingerprint(), changed.fingerprint());
    }

    /// The product's file, as install places it at `/etc/lampOS/records.json`
    /// (`deploy/product/settings/records.json`): valid, on, and bounding
    /// the voice's sessions at 2.0 GB and two weeks. Without it the timer
    /// runs nothing on a product lamp (Twelve on lamp-cb94, 17 Oct).
    #[test]
    fn the_products_config_is_valid_and_bounds_the_voices_sessions() {
        let config: Config = serde_json::from_str(include_str!(
            "../../../deploy/product/settings/records.json"
        ))
        .unwrap();
        config.validate().unwrap();
        assert!(config.enabled);
        assert_eq!(config.roots.len(), 1);
        let root = &config.roots[0];
        assert_eq!(
            root.path,
            std::path::Path::new("/var/lib/lampOS/voice/sessions")
        );
        assert_eq!(root.unit, "lamp-voice.service");
        assert_eq!(root.max_bytes, 2_000_000_000);
        assert_eq!(root.max_age_seconds, 1_209_600);
    }

    fn path() -> &'static Path {
        Path::new("/etc/lampOS/records.json")
    }

    /// A kind this build doesn't know is refused, so it can never become a
    /// path; a field it doesn't know is read past, named, and enables
    /// nothing (#1680).
    #[test]
    fn unknown_fields_and_kinds_do_not_enable_generic_cleanup() {
        let mut value = serde_json::to_value(Config::customer_default()).unwrap();
        value["roots"][0]["kind"] = "memory".into();
        let error = read(path(), &serde_json::to_vec(&value).unwrap()).unwrap_err();
        assert!(
            error.to_string().contains("not read (data error at line 1"),
            "{error}"
        );
        assert!(!error.to_string().contains("memory"), "{error}");
        let mut value = serde_json::to_value(Config::customer_default()).unwrap();
        value["delete_everything"] = true.into();
        value["roots"][0]["also"] = "/".into();
        let Read::Current(config, unread) =
            read(path(), &serde_json::to_vec(&value).unwrap()).unwrap()
        else {
            panic!("this build's schema");
        };
        assert_eq!(
            config.fingerprint(),
            Config::customer_default().fingerprint()
        );
        assert_eq!(
            unread.as_deref(),
            Some(
                "/etc/lampOS/records.json: fields this build does not read, ignored: delete_everything, roots[0].also"
            )
        );
    }

    /// #2052's audit, the records row (#1680): the product's `records.json`
    /// one release ahead. A newer field at this schema is read past and
    /// named; a newer schema is told apart before anything else is read,
    /// and nothing is pruned with it; a key-like field is refused by name.
    /// Before, every cleanup run failed on any of them.
    #[test]
    fn the_products_config_one_release_ahead_is_read_or_told_newer() {
        let product: serde_json::Value = serde_json::from_str(include_str!(
            "../../../deploy/product/settings/records.json"
        ))
        .unwrap();
        let ours: Config = serde_json::from_value(product.clone()).unwrap();
        let mut newer = product.clone();
        newer["roots"][0]["min_free_bytes"] = 1_000_000_000_u64.into();
        let Read::Current(config, unread) =
            read(path(), &serde_json::to_vec_pretty(&newer).unwrap()).unwrap()
        else {
            panic!("this build's schema");
        };
        config.validate().unwrap();
        assert_eq!(config.fingerprint(), ours.fingerprint());
        assert!(
            unread
                .unwrap()
                .ends_with("ignored: roots[0].min_free_bytes")
        );
        newer["schema_version"] = 2.into();
        newer["roots"][0]["kind"] = "memory_days".into();
        let Read::Newer { schema, bus_root } =
            read(path(), &serde_json::to_vec(&newer).unwrap()).unwrap()
        else {
            panic!("a newer schema");
        };
        assert_eq!(schema, 2);
        assert_eq!(bus_root.as_deref(), Some(Path::new("/run/lampos-bus")));
        // A bus it can't take is none: the reporter has nowhere to say it.
        newer["bus_root"] = "run/relative".into();
        assert!(matches!(
            read(path(), &serde_json::to_vec(&newer).unwrap()).unwrap(),
            Read::Newer {
                schema: 2,
                bus_root: None
            }
        ));
        let mut key = product;
        key["relay_token"] = "sk-not-shown".into();
        let error = read(path(), &serde_json::to_vec(&key).unwrap())
            .unwrap_err()
            .to_string();
        assert!(error.contains("relay_token is named like a key"), "{error}");
        assert!(!error.contains("sk-not-shown"), "{error}");
        for broken in [&b"{"[..], br#"{"schema_version":"sk-not-shown"}"#] {
            let error = read(path(), broken).unwrap_err().to_string();
            assert!(error.contains("not read ("), "{error}");
            assert!(!error.contains("sk-not-shown"), "{error}");
        }
    }

    #[test]
    fn broad_ambiguous_and_overlapping_paths_are_rejected() {
        for path in [
            "/",
            "/var/lib",
            "/sessions",
            "sessions",
            "/a/../sessions",
            "/a/./sessions",
            "/a//sessions",
            "/a/sessions/",
        ] {
            let mut config = Config::customer_default();
            config.roots[0].path = path.into();
            assert!(config.validate().is_err(), "{path}");
        }
        let mut config = Config::customer_default();
        config.roots.push(config.roots[0].clone());
        assert!(config.validate().is_err());
        config.roots[1].unit = "different.service".into();
        config.roots[1].path = config.roots[0].path.join("a/sessions");
        assert!(config.validate().is_err());
    }

    #[test]
    fn malformed_service_and_limits_are_rejected() {
        for unit in [
            "*.service",
            "--all.service",
            ".service",
            "a\nb.service",
            "a.socket",
        ] {
            assert!(unit_name(unit).is_err());
        }
        for unit in ["lamp-voice.service", "voice@desk.service"] {
            unit_name(unit).unwrap();
        }
        let mut config = Config::customer_default();
        config.roots[0].max_bytes = 0;
        assert!(config.validate().is_err());
        config.roots[0].max_bytes = 1;
        config.roots[0].max_age_seconds = u64::MAX;
        assert!(config.validate().is_err());
    }

    fn session_read_group_config(value: serde_json::Value) -> io::Result<Read> {
        let mut config = serde_json::to_value(Config::customer_default()).unwrap();
        config["roots"][0]["session_read_group"] = value;
        read(path(), &serde_json::to_vec(&config).unwrap())
    }

    #[test]
    fn session_read_group_is_a_recognized_explicit_per_root_policy() {
        let Read::Current(config, unread) =
            session_read_group_config("peek-session-readers".into()).unwrap()
        else {
            panic!("current configuration schema");
        };
        assert!(
            unread.is_none(),
            "a security policy cannot be silently ignored"
        );
        config.validate().unwrap();
        let written = serde_json::to_value(&config).unwrap();
        assert_eq!(
            written["roots"][0]["session_read_group"],
            "peek-session-readers"
        );
        assert_ne!(
            config.fingerprint(),
            Config::customer_default().fingerprint()
        );
        assert!(
            !config.enabled,
            "permission policy does not activate retention"
        );
    }

    #[test]
    fn session_read_group_omitted_or_null_keeps_the_private_default() {
        let default = Config::customer_default();
        let value = serde_json::to_value(&default).unwrap();
        assert!(
            value["roots"][0]
                .get("session_read_group")
                .is_none_or(serde_json::Value::is_null)
        );
        let Read::Current(omitted, unread) =
            read(path(), &serde_json::to_vec(&value).unwrap()).unwrap()
        else {
            panic!("current schema");
        };
        assert!(unread.is_none());
        assert_eq!(omitted.fingerprint(), default.fingerprint());
        let historical = br#"{"schema_version":1,"enabled":false,"bus_root":"/run/lampos-bus","roots":[{"kind":"voice_service_metadata","path":"/var/lib/lampOS/voice/sessions","unit":"lamp-voice.service","max_bytes":500000000,"max_age_seconds":1209600}]}"#;
        assert_eq!(
            default.fingerprint(),
            format!("{:x}", Sha256::digest(historical))
        );

        let Read::Current(explicit, unread) =
            session_read_group_config(serde_json::Value::Null).unwrap()
        else {
            panic!("current schema");
        };
        assert!(
            unread.is_none(),
            "explicit private policy is recognized too"
        );
        assert_eq!(explicit.fingerprint(), default.fingerprint());
    }

    #[test]
    fn session_read_group_rejects_malformed_names_and_wrong_json_types() {
        for value in [
            serde_json::json!(""),
            serde_json::json!(" reader"),
            serde_json::json!("reader "),
            serde_json::json!(".."),
            serde_json::json!("reader/other"),
            serde_json::json!("reader\nother"),
            serde_json::json!("x".repeat(129)),
            serde_json::json!(1),
            serde_json::json!(true),
            serde_json::json!([]),
            serde_json::json!({}),
        ] {
            let refused = match session_read_group_config(value.clone()) {
                Err(_) => true,
                Ok(Read::Current(config, _)) => config.validate().is_err(),
                Ok(Read::Newer { .. }) => false,
            };
            assert!(
                refused,
                "malformed session-reader group policy accepted: {value}"
            );
        }
    }

    #[test]
    fn session_read_group_applies_to_one_root_and_fingerprints_each_group() {
        let mut value = serde_json::to_value(Config::customer_default()).unwrap();
        let mut second = value["roots"][0].clone();
        second["path"] = "/var/lib/other-voice/sessions".into();
        second["unit"] = "other-voice.service".into();
        value["roots"].as_array_mut().unwrap().push(second);
        value["roots"][0]["session_read_group"] = "peek-session-readers".into();
        let Read::Current(first, unread) =
            read(path(), &serde_json::to_vec(&value).unwrap()).unwrap()
        else {
            panic!("current schema");
        };
        assert!(unread.is_none());
        first.validate().unwrap();
        let written = serde_json::to_value(&first).unwrap();
        assert_eq!(
            written["roots"][0]["session_read_group"],
            "peek-session-readers"
        );
        assert!(
            written["roots"][1]
                .get("session_read_group")
                .is_none_or(serde_json::Value::is_null)
        );
        value["roots"][0]["session_read_group"] = "different-session-readers".into();
        let Read::Current(changed, unread) =
            read(path(), &serde_json::to_vec(&value).unwrap()).unwrap()
        else {
            panic!("current schema");
        };
        assert!(unread.is_none());
        changed.validate().unwrap();
        assert_ne!(
            first.fingerprint(),
            changed.fingerprint(),
            "reader group is part of deletion policy identity"
        );
    }

    #[test]
    fn session_read_group_never_makes_an_unknown_root_kind_or_unsafe_path_valid() {
        for (field, value) in [("kind", "memory"), ("path", "/var/lib")] {
            let mut config = serde_json::to_value(Config::customer_default()).unwrap();
            config["roots"][0]["session_read_group"] = "peek-session-readers".into();
            config["roots"][0][field] = value.into();
            let refused = match read(path(), &serde_json::to_vec(&config).unwrap()) {
                Err(_) => true,
                Ok(Read::Current(config, _)) => config.validate().is_err(),
                Ok(Read::Newer { .. }) => false,
            };
            assert!(refused, "session group widened the supported root contract");
        }
    }
    #[test]
    fn session_read_group_local_resolver_is_exact_unique_and_bounded() {
        let resolve = |s: &str| session_reader_gid_from("peek-readers", s.as_bytes());
        assert_eq!(
            resolve("other:x:10:\npeek-readers:x:412:one,two\n").unwrap(),
            412
        );
        for text in [
            "peek-readers-other:x:412:\n",
            "peek-readers:x:412:\npeek-readers:x:412:\n",
            "peek-readers:x:-1:\n",
            "peek-readers:x:4294967295:\n",
            "peek-readers:x:1",
            "peek-readers:x:1::extra",
        ] {
            assert!(resolve(text).is_err(), "{text:?}");
        }
        assert!(session_reader_gid_from("peek-readers", &[b'x'; 1024 * 1024 + 1][..]).is_err());
        assert!(session_reader_gid_from("peek-readers", &[255][..]).is_err());
    }

    #[test]
    fn session_read_group_local_resolver_propagates_read_failure() {
        struct Failed;
        impl io::Read for Failed {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("injected read failure"))
            }
        }
        assert!(session_reader_gid_from("peek-readers", Failed).is_err());
    }
}
