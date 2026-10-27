//! Immutable ordinary before-images for a future independent writer (#3689).
//!
//! **Unwired:** captures are not authenticated plans or restoration authority.
//! The caller establishes trusted directory ancestry/ownership and excludes
//! writers. Only a fixed artifact in that private directory can be published.
use crate::{
    install, manifest,
    writer_files::{Entry, Identity, Observation},
    writer_refresh::Artifact,
    writer_settings::{MOST_OLD_BYTES, RawOwnership},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use rustix::fs::{AtFlags, FileType, Mode, OFlags, RenameFlags, Stat};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use std::{
    collections::BTreeSet,
    fmt,
    fs::{File, Permissions},
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

pub const FILE: &str = "writer-preimages.json";
const STAGE: &str = ".writer-preimages.stage";
pub const MOST_BUNDLE_BYTES: usize = 16 * 1024 * 1024;
const MOST_FILE_BYTES: usize = crate::writer_files::MOST_BYTES;
const MOST_BASE64: usize = MOST_FILE_BYTES.div_ceil(3) * 4;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn failure(error: impl Into<io::Error>, message: &'static str) -> io::Error {
    io::Error::new(error.into().kind(), message)
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Serialize)]
#[serde(transparent)]
struct Text<const N: usize>(String);
impl<'de, const N: usize> Deserialize<'de> for Text<N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Bounded<const N: usize>;
        impl<'de, const N: usize> Visitor<'de> for Bounded<N> {
            type Value = Text<N>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded text")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if value.len() > N {
                    return Err(E::custom("text bound exceeded"));
                }
                Ok(Text(value.to_owned()))
            }
        }
        deserializer.deserialize_str(Bounded::<N>)
    }
}
#[derive(Serialize)]
#[serde(transparent)]
struct List<T, const N: usize>(Vec<T>);
impl<'de, T: Deserialize<'de>, const N: usize> Deserialize<'de> for List<T, N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Bounded<T, const N: usize>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>, const N: usize> Visitor<'de> for Bounded<T, N> {
            type Value = List<T, N>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded sequence")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut items = Vec::new();
                for _ in 0..N {
                    match seq.next_element()? {
                        Some(item) => items.push(item),
                        None => return Ok(List(items)),
                    }
                }
                if seq.next_element::<de::IgnoredAny>()?.is_some() {
                    return Err(de::Error::custom("sequence bound exceeded"));
                }
                Ok(List(items))
            }
        }
        deserializer.deserialize_seq(Bounded::<T, N>(std::marker::PhantomData))
    }
}

#[derive(Serialize, Deserialize)]
enum Op {
    #[serde(rename = "ordinary_preimages")]
    Ordinary,
}
#[derive(Serialize, Deserialize)]
struct Document {
    version: u32,
    op: Op,
    transaction: Text<64>,
    entries: List<Encoded, 128>,
}
#[derive(Serialize, Deserialize)]
struct Encoded {
    path: Text<511>,
    // An array binds both selected values, including explicit None, without
    // allowing a missing optional field to silently become a default record.
    records: [Option<Text<64>>; 2],
    ancestors: List<Stamp, 64>,
    before: Before,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Before {
    Present {
        bytes: Text<MOST_BASE64>,
        identity: Stamp,
    },
    MissingLeaf,
    MissingAncestor {
        component: usize,
    },
}

// A streaming tagged reader avoids serde's internal-tag Content buffering
// of ignored future arrays/objects before bounded field validation.
impl<'de> Deserialize<'de> for Before {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            Kind,
            Bytes,
            Identity,
            Component,
            #[serde(other)]
            Other,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Kind {
            Present,
            MissingLeaf,
            MissingAncestor,
        }
        struct Tagged;
        impl<'de> Visitor<'de> for Tagged {
            type Value = Before;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a bounded before-image object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut kind: Option<Kind> = None;
                let mut bytes: Option<Text<MOST_BASE64>> = None;
                let mut identity: Option<Stamp> = None;
                let mut component: Option<usize> = None;
                while let Some(field) = map.next_key()? {
                    match field {
                        Field::Kind => {
                            if kind.is_some() {
                                return Err(de::Error::duplicate_field("kind"));
                            }
                            kind = Some(map.next_value()?);
                        }
                        Field::Bytes => {
                            if bytes.is_some() {
                                return Err(de::Error::duplicate_field("bytes"));
                            }
                            bytes = Some(map.next_value()?);
                        }
                        Field::Identity => {
                            if identity.is_some() {
                                return Err(de::Error::duplicate_field("identity"));
                            }
                            identity = Some(map.next_value()?);
                        }
                        Field::Component => {
                            if component.is_some() {
                                return Err(de::Error::duplicate_field("component"));
                            }
                            component = Some(map.next_value()?);
                        }
                        Field::Other => {
                            let _: de::IgnoredAny = map.next_value()?;
                        }
                    }
                }
                match kind.ok_or_else(|| de::Error::missing_field("kind"))? {
                    Kind::Present if component.is_none() => Ok(Before::Present {
                        bytes: bytes.ok_or_else(|| de::Error::missing_field("bytes"))?,
                        identity: identity.ok_or_else(|| de::Error::missing_field("identity"))?,
                    }),
                    Kind::MissingLeaf
                        if bytes.is_none() && identity.is_none() && component.is_none() =>
                    {
                        Ok(Before::MissingLeaf)
                    }
                    Kind::MissingAncestor if bytes.is_none() && identity.is_none() => {
                        Ok(Before::MissingAncestor {
                            component: component
                                .ok_or_else(|| de::Error::missing_field("component"))?,
                        })
                    }
                    _ => Err(de::Error::custom(
                        "unsupported before-image field combination",
                    )),
                }
            }
        }
        deserializer.deserialize_map(Tagged)
    }
}

// Private wire representation: the observer gains no serde or mutation API.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp {
    device: u64,
    inode: u64,
    links: u64,
    length: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    modified_seconds: i64,
    modified_nanos: i64,
    changed_seconds: i64,
    changed_nanos: i64,
}
impl From<Identity> for Stamp {
    fn from(i: Identity) -> Self {
        Self {
            device: i.device,
            inode: i.inode,
            links: i.links,
            length: i.length,
            mode: i.mode,
            uid: i.uid,
            gid: i.gid,
            modified_seconds: i.modified_seconds,
            modified_nanos: i.modified_nanos,
            changed_seconds: i.changed_seconds,
            changed_nanos: i.changed_nanos,
        }
    }
}
impl From<Stamp> for Identity {
    fn from(i: Stamp) -> Self {
        Self {
            device: i.device,
            inode: i.inode,
            links: i.links,
            length: i.length,
            mode: i.mode,
            uid: i.uid,
            gid: i.gid,
            modified_seconds: i.modified_seconds,
            modified_nanos: i.modified_nanos,
            changed_seconds: i.changed_seconds,
            changed_nanos: i.changed_nanos,
        }
    }
}
impl Stamp {
    fn check(&self, directory: bool) -> io::Result<()> {
        let kind = if directory {
            FileType::Directory
        } else {
            FileType::RegularFile
        };
        if self.mode & !0o177777 != 0 {
            return Err(invalid("invalid preimage identity"));
        }
        // Apple RawMode is u16; Linux RawMode is already u32. Check the
        // narrowing conversion without a lossy cast or an identity conversion.
        #[cfg(target_os = "linux")]
        let mode = self.mode;
        #[cfg(not(target_os = "linux"))]
        let mode = self
            .mode
            .try_into()
            .map_err(|_| invalid("invalid preimage identity"))?;
        if FileType::from_raw_mode(mode) != kind
            || self.links == 0
            || (!directory && self.links != 1)
            || !(0..1_000_000_000).contains(&self.modified_nanos)
            || !(0..1_000_000_000).contains(&self.changed_nanos)
        {
            return Err(invalid("invalid preimage identity"));
        }
        Ok(())
    }
}

/// Owned immutable selected input, not an approved action or desired text.
pub struct Preimage {
    path: String,
    recorded: Option<String>,
    writing: Option<String>,
    before: Observation,
}
impl Preimage {
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn recorded(&self) -> Option<&str> {
        self.recorded.as_deref()
    }
    pub fn writing(&self) -> Option<&str> {
        self.writing.as_deref()
    }
    pub fn before(&self) -> &Observation {
        &self.before
    }
}
impl fmt::Debug for Preimage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Preimage")
            .field("ancestor_count", &self.before.ancestors.len())
            .field(
                "old_byte_count",
                &match &self.before.entry {
                    Entry::Present { bytes, .. } => bytes.len(),
                    _ => 0,
                },
            )
            .finish()
    }
}

/// Retains original encoded bytes, including unknown nonsemantic fields.
/// No field/record mutation or reserialization interface is provided.
pub struct Bundle {
    transaction: String,
    entries: Vec<Preimage>,
    encoded: Vec<u8>,
    artifact: Artifact,
}
impl fmt::Debug for Bundle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreimageBundle")
            .field("entry_count", &self.entries.len())
            .field("encoded_byte_count", &self.encoded.len())
            .finish()
    }
}
impl Bundle {
    /// Syntax validation is not proof of freshness: the future
    /// authenticated journal caller must establish uniqueness and provenance.
    pub fn from_capture(transaction: &str, captured: &RawOwnership<'_>) -> io::Result<Self> {
        if !digest(transaction) || captured.entries().len() > manifest::MOST_SETTINGS {
            return Err(invalid("invalid preimage inputs"));
        }
        let mut old_bytes = 0usize;
        let mut entries = Vec::new();
        for entry in captured.entries() {
            let before = entry.before();
            let encoded = match &before.entry {
                Entry::Present { bytes, identity } => {
                    old_bytes = old_bytes
                        .checked_add(bytes.len())
                        .filter(|n| *n <= MOST_OLD_BYTES)
                        .ok_or_else(|| invalid("preimage old byte budget exceeded"))?;
                    if bytes.len() > MOST_FILE_BYTES {
                        return Err(invalid("preimage file bound exceeded"));
                    }
                    Before::Present {
                        bytes: Text(STANDARD.encode(bytes)),
                        identity: (*identity).into(),
                    }
                }
                Entry::MissingLeaf => Before::MissingLeaf,
                Entry::MissingAncestor { component } => Before::MissingAncestor {
                    component: *component,
                },
            };
            entries.push(Encoded {
                path: Text(entry.setting().path.clone()),
                records: [
                    entry.recorded().map(|s| Text(s.to_owned())),
                    entry.writing().map(|s| Text(s.to_owned())),
                ],
                ancestors: List(before.ancestors.iter().copied().map(Stamp::from).collect()),
                before: encoded,
            });
        }
        let doc = Document {
            version: 1,
            op: Op::Ordinary,
            transaction: Text(transaction.to_owned()),
            entries: List(entries),
        };
        let mut output = Capped(Vec::new());
        serde_json::to_writer(&mut output, &doc)
            .map_err(|_| invalid("preimage encoding exceeds its bound"))?;
        let expected = Artifact::of(&output.0);
        Self::decode(output.0, transaction, &expected)
    }
    pub fn transaction(&self) -> &str {
        &self.transaction
    }
    pub fn entries(&self) -> &[Preimage] {
        &self.entries
    }
    pub fn encoded(&self) -> &[u8] {
        &self.encoded
    }
    pub fn artifact(&self) -> &Artifact {
        &self.artifact
    }

    /// Verify exact identity/size BEFORE parsing; identity is not authentication.
    pub fn decode(encoded: Vec<u8>, transaction: &str, expected: &Artifact) -> io::Result<Self> {
        check_expected(transaction, expected)?;
        if encoded.len() as u64 != expected.size
            || manifest::sha256_hex(&encoded) != expected.sha256
        {
            return Err(invalid("preimage artifact identity mismatch"));
        }
        let doc: Document =
            serde_json::from_slice(&encoded).map_err(|_| invalid("invalid preimage encoding"))?;
        if doc.version != 1 || doc.transaction.0 != transaction {
            return Err(invalid("unsupported preimage version or transaction"));
        }
        let mut paths = BTreeSet::new();
        let mut total = 0usize;
        let mut root = None;
        // Validate all claimed byte budgets and identities before any base64
        // decode buffer is allocated. Unknown nonsemantic fields stay in encoded.
        for entry in &doc.entries.0 {
            let path = &entry.path.0;
            let parts = path.strip_prefix('/').map_or(0, |s| s.split('/').count());
            if path.len() > 511
                || parts > 64
                || !manifest::is_setting_path(path)
                || !paths.insert(path.clone())
            {
                return Err(invalid("invalid or duplicate preimage path"));
            }
            for (n, value) in entry.records.iter().enumerate() {
                if value
                    .as_ref()
                    .is_some_and(|v| !(digest(&v.0) || n == 0 && v.0 == install::SET_ASIDE))
                {
                    return Err(invalid("invalid selected preimage record"));
                }
            }
            let first = entry
                .ancestors
                .0
                .first()
                .ok_or_else(|| invalid("preimage lacks root identity"))?;
            if root.is_some_and(|r| r != *first) {
                return Err(invalid("preimage roots disagree"));
            }
            root = Some(*first);
            for stamp in &entry.ancestors.0 {
                stamp.check(true)?;
            }
            let count = match &entry.before {
                Before::Present { bytes, identity } => {
                    identity.check(false)?;
                    if identity.length > MOST_FILE_BYTES as u64 || bytes.0.len() > MOST_BASE64 {
                        return Err(invalid("preimage file bound exceeded"));
                    }
                    total = total
                        .checked_add(identity.length as usize)
                        .filter(|n| *n <= MOST_OLD_BYTES)
                        .ok_or_else(|| invalid("preimage old byte budget exceeded"))?;
                    parts
                }
                Before::MissingLeaf => parts,
                Before::MissingAncestor { component } => {
                    if *component >= parts.saturating_sub(1) {
                        return Err(invalid("invalid missing preimage component"));
                    }
                    component + 1
                }
            };
            if entry.ancestors.0.len() != count {
                return Err(invalid("invalid preimage ancestor count"));
            }
        }
        let mut entries = Vec::with_capacity(doc.entries.0.len());
        for entry in doc.entries.0 {
            let before = match entry.before {
                Before::Present { bytes, identity } => {
                    let wanted = identity.length as usize;
                    if bytes.0.len() != wanted.div_ceil(3) * 4 {
                        return Err(invalid("invalid preimage encoded length"));
                    }
                    // The library's documented conservative decode buffer is
                    // bounded to the claimed length plus at most two bytes.
                    let mut decoded = vec![0; base64::decoded_len_estimate(bytes.0.len())];
                    let length = STANDARD
                        .decode_slice(bytes.0.as_bytes(), &mut decoded)
                        .map_err(|_| invalid("invalid preimage base64 or length"))?;
                    if length != wanted {
                        return Err(invalid("invalid preimage byte length"));
                    }
                    decoded.truncate(length);
                    Entry::Present {
                        bytes: decoded,
                        identity: identity.into(),
                    }
                }
                Before::MissingLeaf => Entry::MissingLeaf,
                Before::MissingAncestor { component } => Entry::MissingAncestor { component },
            };
            let [recorded, writing] = entry.records;
            entries.push(Preimage {
                path: entry.path.0,
                recorded: recorded.map(|s| s.0),
                writing: writing.map(|s| s.0),
                before: Observation {
                    ancestors: entry.ancestors.0.into_iter().map(Identity::from).collect(),
                    entry: before,
                },
            });
        }
        Ok(Self {
            transaction: doc.transaction.0,
            artifact: Artifact::of(&encoded),
            encoded,
            entries,
        })
    }
}
fn check_expected(transaction: &str, artifact: &Artifact) -> io::Result<()> {
    if !digest(transaction)
        || !digest(&artifact.sha256)
        || artifact.size == 0
        || artifact.size > MOST_BUNDLE_BYTES as u64
    {
        return Err(invalid("invalid preimage artifact expectation"));
    }
    Ok(())
}
struct Capped(Vec<u8>);
impl Write for Capped {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .0
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > MOST_BUNDLE_BYTES)
        {
            return Err(invalid("preimage encoded byte bound exceeded"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Fixed-name private artifact I/O, anchored to an already trusted directory.
/// Leaf owner/mode checks do not authenticate privileged ancestors or callers.
pub struct Store {
    directory: File,
}
impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreimageStore").finish_non_exhaustive()
    }
}
#[derive(Clone, Copy)]
enum Published {
    StageSynced,
    Renamed,
    DirectorySynced,
}
impl Store {
    pub fn open(folder: &Path) -> io::Result<Self> {
        let directory = File::from(
            rustix::fs::open(
                folder,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|e| failure(e, "cannot open private preimage directory"))?,
        );
        let meta = directory
            .metadata()
            .map_err(|e| failure(e, "cannot inspect private preimage directory"))?;
        if !meta.is_dir()
            || meta.mode() & 0o7777 != 0o700
            || meta.uid() != rustix::process::geteuid().as_raw()
        {
            return Err(invalid(
                "preimage directory is not private and caller-owned",
            ));
        }
        Ok(Self { directory })
    }
    fn absent(&self, name: &str) -> io::Result<()> {
        match rustix::fs::statat(&self.directory, name, AtFlags::SYMLINK_NOFOLLOW) {
            Err(rustix::io::Errno::NOENT) => Ok(()),
            Err(e) => Err(failure(e, "cannot inspect private preimage name")),
            Ok(_) => Err(invalid("private preimage name already exists")),
        }
    }
    /// No replace/cleanup/retry. Failed acknowledgement grants nothing, even
    /// after rename/dir sync; retain bytes and reconcile with expected identity.
    pub fn publish(&self, bundle: &Bundle) -> io::Result<Artifact> {
        self.publish_inner(bundle, |_| Ok(()))
    }
    fn publish_inner(
        &self,
        bundle: &Bundle,
        mut checkpoint: impl FnMut(Published) -> io::Result<()>,
    ) -> io::Result<Artifact> {
        self.absent(FILE)?;
        self.absent(STAGE)?;
        let mut file = File::from(
            rustix::fs::openat(
                &self.directory,
                STAGE,
                OFlags::WRONLY
                    | OFlags::CREATE
                    | OFlags::EXCL
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK
                    | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(|e| failure(e, "cannot create private preimage stage"))?,
        );
        file.set_permissions(Permissions::from_mode(0o600))
            .map_err(|e| failure(e, "cannot protect private preimage stage"))?;
        file.write_all(bundle.encoded())
            .map_err(|e| failure(e, "cannot write private preimage stage"))?;
        file.sync_all()
            .map_err(|e| failure(e, "cannot sync private preimage stage"))?;
        self.directory
            .sync_all()
            .map_err(|e| failure(e, "cannot sync private preimage stage directory"))?;
        checkpoint(Published::StageSynced)?;
        rustix::fs::renameat_with(
            &self.directory,
            STAGE,
            &self.directory,
            FILE,
            RenameFlags::NOREPLACE,
        )
        .map_err(|e| failure(e, "cannot publish private preimage artifact"))?;
        checkpoint(Published::Renamed)?;
        self.directory
            .sync_all()
            .map_err(|e| failure(e, "cannot sync private preimage directory"))?;
        checkpoint(Published::DirectorySynced)?;
        Ok(bundle.artifact().clone())
    }
    /// Integrity-only read. After an uncertain publication acknowledgement,
    /// success is not a new durability or admission acknowledgement. The future
    /// caller must separately reconcile and sync/journal before any grant.
    pub fn load(&self, transaction: &str, expected: &Artifact) -> io::Result<Bundle> {
        check_expected(transaction, expected)?;
        self.absent(STAGE)?;
        let before = rustix::fs::statat(&self.directory, FILE, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|e| failure(e, "cannot inspect private preimage artifact"))?;
        if FileType::from_raw_mode(before.st_mode) != FileType::RegularFile
            || before.st_nlink != 1
            || before.st_mode & 0o7777 != 0o600
            || before.st_uid != rustix::process::geteuid().as_raw()
            || before.st_size < 0
            || before.st_size as u64 != expected.size
        {
            return Err(invalid("invalid private preimage artifact metadata"));
        }
        let file = File::from(
            rustix::fs::openat(
                &self.directory,
                FILE,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|e| failure(e, "cannot open private preimage artifact"))?,
        );
        let opened = rustix::fs::fstat(&file)
            .map_err(|e| failure(e, "cannot inspect opened preimage artifact"))?;
        if !same_stat(&before, &opened) {
            return Err(invalid("preimage artifact changed before reading"));
        }
        let mut bytes = vec![0; expected.size as usize + 1];
        let mut used = 0;
        let mut reader = &file;
        while used < bytes.len() {
            match reader.read(&mut bytes[used..]) {
                Ok(0) => break,
                Ok(n) => used += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(failure(e, "cannot read private preimage artifact")),
            }
        }
        let after = rustix::fs::fstat(&file)
            .map_err(|e| failure(e, "cannot inspect read preimage artifact"))?;
        if !same_stat(&opened, &after) || used as u64 != expected.size {
            return Err(invalid("preimage artifact changed while reading"));
        }
        bytes.truncate(used);
        Bundle::decode(bytes, transaction, expected)
    }
}
fn same_stat(a: &Stat, b: &Stat) -> bool {
    a.st_dev == b.st_dev
        && a.st_ino == b.st_ino
        && a.st_mode == b.st_mode
        && a.st_nlink == b.st_nlink
        && a.st_size == b.st_size
        && a.st_uid == b.st_uid
        && a.st_gid == b.st_gid
        && a.st_mtime == b.st_mtime
        && a.st_mtime_nsec == b.st_mtime_nsec
        && a.st_ctime == b.st_ctime
        && a.st_ctime_nsec == b.st_ctime_nsec
}

#[cfg(test)]
mod tests;
