//! Private bounded persistence of observations, never admission or authority.
//!
//! The producer and its native Identity comparison are deliberately unchanged.
//! Decoding creates a different closed provider, never a reconstructed Stat.
use super::{
    Collector, ConfEntries, DirectoryFact, FrozenInputs, HeadInputs, Identity, Limits, Result,
    Text, TextFact, Witness, identity, key_path,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use rustix::fs::FileType;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
    ser::SerializeMap,
};
use std::{ffi::OsStr, fmt, io, os::unix::ffi::OsStrExt, path::Path};

const REFUSED: &str = "saved head inputs refused";
const SCHEMA: u32 = 1;
const OP: &str = "head_inputs";
const ENCODED: usize = 16 * 1024 * 1024;
const FACTS: usize = 256;
const WITNESSES: usize = 8192;
const NAMES: usize = 8192;
const RAW_BYTES: usize = 2 * 1024 * 1024;
const JSON_DEPTH: usize = 32;
const JSON_TOKENS: usize = 524_288;
const JSON_STRING: usize = 2 * 1024 * 1024;
const JSON_PRIMITIVE: usize = 128;
// Schema 1 caps are independent of a future producer-default expansion.
const WIRE_LIMITS: Limits = Limits {
    file: 1024 * 1024,
    bytes: 4 * 1024 * 1024,
    names: 128,
    name_bytes: 64 * 1024,
    keys: 256,
    path: 4096,
    components: 64,
    links: 16,
    work: 16 * 1024,
};

fn refused<T>() -> Result<T> {
    Err(REFUSED.into())
}
fn convert<T, U: TryFrom<T>>(value: T) -> Result<U> {
    U::try_from(value).map_err(|_| REFUSED.into())
}
fn add(value: &mut usize, amount: usize, cap: usize) -> Result<()> {
    let next = value.checked_add(amount).ok_or(REFUSED)?;
    if next > cap {
        return refused();
    }
    *value = next;
    Ok(())
}
fn reserve<T>(values: &mut Vec<T>, count: usize) -> Result<()> {
    values.try_reserve_exact(count).map_err(|_| REFUSED.into())
}

/// Portable selected fields, including signed device words and timestamps.
/// Wide device words cover Apple's signed dev_t and Linux's unsigned dev_t.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp {
    dev: i128,
    ino: u64,
    mode: u32,
    nlink: u64,
    uid: u32,
    gid: u32,
    rdev: i128,
    size: i64,
    mtime: i64,
    mtime_ns: u32,
    ctime: i64,
    ctime_ns: u32,
}

impl Stamp {
    fn of(identity: Identity) -> Result<Self> {
        let s = identity.0;
        let result = Self {
            dev: convert(s.st_dev)?,
            ino: convert(s.st_ino)?,
            mode: convert(s.st_mode)?,
            nlink: convert(s.st_nlink)?,
            uid: convert(s.st_uid)?,
            gid: convert(s.st_gid)?,
            rdev: convert(s.st_rdev)?,
            size: convert(s.st_size)?,
            mtime: convert(s.st_mtime)?,
            mtime_ns: convert(s.st_mtime_nsec)?,
            ctime: convert(s.st_ctime)?,
            ctime_ns: convert(s.st_ctime_nsec)?,
        };
        result.check()?;
        Ok(result)
    }

    fn check(&self) -> Result<()> {
        let device = |value| (-2_147_483_648..=18_446_744_073_709_551_615_i128).contains(&value);
        if !device(self.dev)
            || !device(self.rdev)
            || self.size < 0
            || self.mode & !0o177777 != 0
            || self.mtime_ns >= 1_000_000_000
            || self.ctime_ns >= 1_000_000_000
        {
            return refused();
        }
        // A checked mode conversion is independent of native Stat layout.
        let _ = FileType::from_raw_mode(convert(self.mode)?);
        Ok(())
    }

    fn kind(&self, expected: FileType) -> Result<()> {
        self.check()?;
        if FileType::from_raw_mode(convert(self.mode)?) != expected {
            return refused();
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Bounds {
    file: u32,
    bytes: u32,
    names: u32,
    name_bytes: u32,
    keys: u32,
    path: u32,
    components: u32,
    links: u32,
    work: u32,
}

impl Bounds {
    fn of(l: Limits) -> Result<Self> {
        let result = Self {
            file: convert(l.file)?,
            bytes: convert(l.bytes)?,
            names: convert(l.names)?,
            name_bytes: convert(l.name_bytes)?,
            keys: convert(l.keys)?,
            path: convert(l.path)?,
            components: convert(l.components)?,
            links: convert(l.links)?,
            work: convert(l.work)?,
        };
        result.native()?;
        Ok(result)
    }

    fn native(self) -> Result<Limits> {
        let l = Limits {
            file: convert(self.file)?,
            bytes: convert(self.bytes)?,
            names: convert(self.names)?,
            name_bytes: convert(self.name_bytes)?,
            keys: convert(self.keys)?,
            path: convert(self.path)?,
            components: convert(self.components)?,
            links: convert(self.links)?,
            work: convert(self.work)?,
        };
        let cap = WIRE_LIMITS;
        if l.file > cap.file
            || l.bytes > cap.bytes
            || l.names > cap.names
            || l.name_bytes > cap.name_bytes
            || l.keys > cap.keys
            || l.path > cap.path
            || l.components > cap.components
            || l.links > cap.links
            || l.work > cap.work
        {
            return refused();
        }
        Ok(l)
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Raw(Vec<u8>);
impl Serialize for Raw {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(&self.0))
    }
}

#[derive(Clone, PartialEq, Eq)]
enum SavedWitness {
    Directory(Stamp),
    Link { stamp: Stamp, target: Raw },
}
impl Serialize for SavedWitness {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_map(Some(match self {
            Self::Directory(_) => 2,
            Self::Link { .. } => 3,
        }))?;
        match self {
            Self::Directory(stamp) => {
                out.serialize_entry("kind", "directory")?;
                out.serialize_entry("stamp", stamp)?;
            }
            Self::Link { stamp, target } => {
                out.serialize_entry("kind", "link")?;
                out.serialize_entry("stamp", stamp)?;
                out.serialize_entry("target", target)?;
            }
        }
        out.end()
    }
}

#[derive(Clone, PartialEq, Eq)]
enum SavedText {
    Regular { text: String, stamp: Stamp },
    NullMask { leaf: Stamp, null: Stamp },
    MissingLeaf,
    MissingAncestor { suffix: Vec<Raw> },
}
impl SavedText {
    fn value(&self) -> Option<String> {
        match self {
            Self::Regular { text, .. } => Some(text.clone()),
            Self::NullMask { .. } => Some(String::new()),
            Self::MissingLeaf | Self::MissingAncestor { .. } => None,
        }
    }
}
impl Serialize for SavedText {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_map(None)?;
        match self {
            Self::Regular { text, stamp } => {
                out.serialize_entry("kind", "regular")?;
                out.serialize_entry("text", &STANDARD.encode(text.as_bytes()))?;
                out.serialize_entry("stamp", stamp)?;
            }
            Self::NullMask { leaf, null } => {
                out.serialize_entry("kind", "null_mask")?;
                out.serialize_entry("leaf", leaf)?;
                out.serialize_entry("null", null)?;
            }
            Self::MissingLeaf => out.serialize_entry("kind", "missing_leaf")?,
            Self::MissingAncestor { suffix } => {
                out.serialize_entry("kind", "missing_ancestor")?;
                out.serialize_entry("suffix", suffix)?;
            }
        }
        out.end()
    }
}

#[derive(Serialize)]
struct TextRow {
    path: Raw,
    witnesses: Vec<SavedWitness>,
    fact: SavedText,
}
#[derive(Serialize)]
struct DirectoryRow {
    path: Raw,
    witnesses: Vec<SavedWitness>,
    names: Option<Vec<Raw>>,
    missing: Option<SavedText>,
}
#[derive(Serialize)]
struct Portable {
    schema: u32,
    op: &'static str,
    root: Raw,
    root_stamp: Stamp,
    limits: Bounds,
    texts: Vec<TextRow>,
    directories: Vec<DirectoryRow>,
}

/// Separate restart provider: raw encoded bytes are retained without mutation.
pub(crate) struct SavedInputs {
    facts: Portable,
    encoded: Box<[u8]>,
}

mod coherence;
impl fmt::Debug for SavedInputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SavedInputs")
            .field("text_count", &self.facts.texts.len())
            .field("directory_count", &self.facts.directories.len())
            .field("encoded_bytes", &self.encoded.len())
            .finish_non_exhaustive()
    }
}
impl SavedInputs {
    /// Returns exact original bytes, including bounded future metadata.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "Staged exact-byte persistence API; no storage wiring exists"
        )
    )]
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.encoded
    }
}
impl HeadInputs for SavedInputs {
    fn text(&mut self, path: &Path) -> Result<Option<String>> {
        key_path(path, self.facts.limits.native()?).map_err(|_| REFUSED.to_owned())?;
        let key = path.as_os_str().as_bytes();
        self.facts
            .texts
            .binary_search_by(|row| row.path.0.as_slice().cmp(key))
            .map(|n| self.facts.texts[n].fact.value())
            .map_err(|_| REFUSED.into())
    }

    fn entries(&mut self, path: &Path) -> Result<Option<ConfEntries>> {
        key_path(path, self.facts.limits.native()?).map_err(|_| REFUSED.to_owned())?;
        let key = path.as_os_str().as_bytes();
        let n = self
            .facts
            .directories
            .binary_search_by(|row| row.path.0.as_slice().cmp(key))
            .map_err(|_| REFUSED)?;
        Ok(self.facts.directories[n].names.as_ref().map(|names| {
            let paths: Vec<_> = names
                .iter()
                .map(|name| Ok(path.join(OsStr::from_bytes(&name.0))))
                .collect();
            Box::new(paths.into_iter()) as ConfEntries
        }))
    }
}

#[derive(Default)]
struct Budget {
    facts: usize,
    witnesses: usize,
    names: usize,
    name_bytes: usize,
    raw: usize,
    text: usize,
    suffix: usize,
}
impl Budget {
    fn fact(&mut self, limits: Limits) -> Result<()> {
        add(&mut self.facts, 1, FACTS.min(limits.keys))
    }
    fn witness(&mut self) -> Result<()> {
        add(&mut self.witnesses, 1, WITNESSES)
    }
    fn raw(&mut self, bytes: &[u8], cap: usize) -> Result<()> {
        if bytes.len() > cap {
            return refused();
        }
        add(&mut self.raw, bytes.len(), RAW_BYTES)
    }
    fn name(&mut self, bytes: &[u8], limits: Limits) -> Result<()> {
        if bytes.is_empty()
            || bytes.contains(&0)
            || bytes.contains(&b'/')
            || matches!(bytes, b"." | b"..")
        {
            return refused();
        }
        add(&mut self.names, 1, NAMES)?;
        add(&mut self.name_bytes, bytes.len(), limits.name_bytes)?;
        self.raw(bytes, limits.path)
    }
    fn text(&mut self, bytes: &[u8], limits: Limits) -> Result<()> {
        if bytes.len() > limits.file {
            return refused();
        }
        add(&mut self.text, bytes.len(), limits.bytes)
    }
}

struct BoundedWriter(Vec<u8>);
impl io::Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self
            .0
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other(REFUSED))?;
        if next > ENCODED {
            return Err(io::Error::other(REFUSED));
        }
        if self.0.capacity() < next {
            let target = self
                .0
                .capacity()
                .max(1024)
                .saturating_mul(2)
                .min(ENCODED)
                .max(next);
            self.0
                .try_reserve_exact(target - self.0.len())
                .map_err(|_| io::Error::other(REFUSED))?;
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn component(bytes: &[u8]) -> Result<()> {
    // Unresolved link tails may contain '.' or '..'; their exact bytes matter.
    if bytes.is_empty() || bytes.contains(&0) || bytes.contains(&b'/') {
        return refused();
    }
    Ok(())
}
fn joined_name(path_bytes: usize, name_bytes: usize, cap: usize) -> Result<()> {
    if path_bytes
        .checked_add(1)
        .and_then(|n| n.checked_add(name_bytes))
        .ok_or(REFUSED)?
        > cap
    {
        return refused();
    }
    Ok(())
}

fn path_bytes(path: &Path, root: &Path, limits: Limits, budget: &mut Budget) -> Result<()> {
    key_path(path, limits).map_err(|_| REFUSED.to_owned())?;
    if !path.starts_with(root) {
        return refused();
    }
    budget.raw(path.as_os_str().as_bytes(), limits.path)
}

fn check_witnesses(
    values: &[Witness],
    root: Identity,
    limits: Limits,
    budget: &mut Budget,
) -> Result<()> {
    if values.is_empty()
        || values.len() > limits.components.checked_add(1).ok_or(REFUSED)?
        || !matches!(values.first(), Some(Witness::Directory(first)) if *first == root)
    {
        return refused();
    }
    let mut links = 0;
    for witness in values {
        budget.witness()?;
        match witness {
            Witness::Directory(stamp) => Stamp::of(*stamp)?.kind(FileType::Directory)?,
            Witness::Link { identity, target } => {
                links += 1;
                if links > limits.links || target.is_empty() || target.contains(&0) {
                    return refused();
                }
                Stamp::of(*identity)?.kind(FileType::Symlink)?;
                budget.raw(target, limits.path)?;
            }
        }
    }
    Ok(())
}

fn check_text(value: &Text, limits: Limits, budget: &mut Budget) -> Result<()> {
    match value {
        Text::Regular { text, identity } => {
            let stamp = Stamp::of(*identity)?;
            stamp.kind(FileType::RegularFile)?;
            if convert::<_, usize>(stamp.size)? != text.len() {
                return refused();
            }
            budget.text(text.as_bytes(), limits)
        }
        Text::NullMask { leaf, null } => {
            let leaf = Stamp::of(*leaf)?;
            let null = Stamp::of(*null)?;
            leaf.kind(FileType::CharacterDevice)?;
            null.kind(FileType::CharacterDevice)?;
            if leaf.rdev != null.rdev {
                return refused();
            }
            Ok(())
        }
        Text::MissingLeaf => Ok(()),
        Text::MissingAncestor { suffix } => {
            if suffix.len() < 2 || suffix.len() > limits.components {
                return refused();
            }
            let mut total = 0;
            let expanded = limits
                .path
                .checked_mul(limits.links.checked_add(1).ok_or(REFUSED)?)
                .ok_or(REFUSED)?;
            for part in suffix {
                let bytes = part.as_bytes();
                component(bytes)?;
                add(&mut total, bytes.len(), expanded)?;
                budget.raw(bytes, limits.path)?;
            }
            Ok(())
        }
    }
}

/// This first pass performs no collection/base64/output allocation. Codec
/// limits are deliberately narrower than every possible successful capture.
fn check_frozen(inputs: &FrozenInputs) -> Result<()> {
    let limits = Bounds::of(inputs.limits)?.native()?;
    let mut budget = Budget::default();
    key_path(&inputs.root_path, limits).map_err(|_| REFUSED.to_owned())?;
    budget.raw(inputs.root_path.as_os_str().as_bytes(), limits.path)?;
    Stamp::of(inputs.root_identity)?.kind(FileType::Directory)?;
    for (path, fact) in &inputs.texts {
        if path.as_os_str().as_bytes() == inputs.root_path.as_os_str().as_bytes() {
            return refused();
        }
        budget.fact(limits)?;
        path_bytes(path, &inputs.root_path, limits, &mut budget)?;
        check_witnesses(&fact.witnesses, inputs.root_identity, limits, &mut budget)?;
        check_text(&fact.text, limits, &mut budget)?;
        if let Some(directory) = inputs.directories.get(path)
            && (directory.names.is_some()
                || directory.missing.as_ref() != Some(&fact.text)
                || !matches!(&fact.text, Text::MissingLeaf | Text::MissingAncestor { .. })
                || directory.witnesses != fact.witnesses)
        {
            return refused();
        }
    }
    for (path, fact) in &inputs.directories {
        if path.as_os_str().as_bytes() == inputs.root_path.as_os_str().as_bytes()
            && (fact.names.is_none() || fact.witnesses.len() != 1)
        {
            return refused();
        }
        budget.fact(limits)?;
        path_bytes(path, &inputs.root_path, limits, &mut budget)?;
        check_witnesses(&fact.witnesses, inputs.root_identity, limits, &mut budget)?;
        match (&fact.names, &fact.missing) {
            (Some(names), None) => {
                if names.len() > limits.names {
                    return refused();
                }
                for (n, name) in names.iter().enumerate() {
                    if names[..n]
                        .iter()
                        .any(|prior| prior.as_bytes() == name.as_bytes())
                    {
                        return refused();
                    }
                    joined_name(
                        path.as_os_str().as_bytes().len(),
                        name.as_bytes().len(),
                        limits.path,
                    )?;
                    budget.name(name.as_bytes(), limits)?;
                }
            }
            (None, Some(missing @ (Text::MissingLeaf | Text::MissingAncestor { .. }))) => {
                check_text(missing, limits, &mut budget)?;
            }
            _ => return refused(),
        }
    }
    Ok(())
}

fn copy_bytes(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    reserve(&mut result, bytes.len())?;
    result.extend_from_slice(bytes);
    Ok(result)
}
fn saved_witnesses(values: &[Witness]) -> Result<Vec<SavedWitness>> {
    let mut result = Vec::new();
    reserve(&mut result, values.len())?;
    for value in values {
        result.push(match value {
            Witness::Directory(stamp) => SavedWitness::Directory(Stamp::of(*stamp)?),
            Witness::Link { identity, target } => SavedWitness::Link {
                stamp: Stamp::of(*identity)?,
                target: Raw(copy_bytes(target)?),
            },
        });
    }
    Ok(result)
}
fn saved_text(value: &Text) -> Result<SavedText> {
    Ok(match value {
        Text::Regular { text, identity } => SavedText::Regular {
            text: String::from_utf8(copy_bytes(text.as_bytes())?).map_err(|_| REFUSED)?,
            stamp: Stamp::of(*identity)?,
        },
        Text::NullMask { leaf, null } => SavedText::NullMask {
            leaf: Stamp::of(*leaf)?,
            null: Stamp::of(*null)?,
        },
        Text::MissingLeaf => SavedText::MissingLeaf,
        Text::MissingAncestor { suffix } => {
            let mut parts = Vec::new();
            reserve(&mut parts, suffix.len())?;
            for part in suffix {
                parts.push(Raw(copy_bytes(part.as_bytes())?));
            }
            SavedText::MissingAncestor { suffix: parts }
        }
    })
}

impl Portable {
    fn of(inputs: &FrozenInputs) -> Result<Self> {
        check_frozen(inputs)?;
        let mut texts = Vec::new();
        reserve(&mut texts, inputs.texts.len())?;
        for (path, fact) in &inputs.texts {
            texts.push(TextRow {
                path: Raw(copy_bytes(path.as_os_str().as_bytes())?),
                witnesses: saved_witnesses(&fact.witnesses)?,
                fact: saved_text(&fact.text)?,
            });
        }
        texts.sort_by(|a, b| a.path.0.cmp(&b.path.0));
        let mut directories = Vec::new();
        reserve(&mut directories, inputs.directories.len())?;
        for (path, fact) in &inputs.directories {
            let names = if let Some(names) = &fact.names {
                let mut copied = Vec::new();
                reserve(&mut copied, names.len())?;
                for name in names {
                    copied.push(Raw(copy_bytes(name.as_bytes())?));
                }
                Some(copied)
            } else {
                None
            };
            directories.push(DirectoryRow {
                path: Raw(copy_bytes(path.as_os_str().as_bytes())?),
                witnesses: saved_witnesses(&fact.witnesses)?,
                names,
                missing: fact.missing.as_ref().map(saved_text).transpose()?,
            });
        }
        directories.sort_by(|a, b| a.path.0.cmp(&b.path.0));
        Ok(Self {
            schema: SCHEMA,
            op: OP,
            root: Raw(copy_bytes(inputs.root_path.as_os_str().as_bytes())?),
            root_stamp: Stamp::of(inputs.root_identity)?,
            limits: Bounds::of(inputs.limits)?,
            texts,
            directories,
        })
    }
}

/// Encodes only native observations, not a SavedInputs (which must retain its
/// exact original bytes). Output has fixed field order and raw-byte key order.
#[allow(
    dead_code,
    reason = "Staged private read-only codec; no guard or storage wiring exists"
)]
pub(crate) fn encode(inputs: &FrozenInputs) -> Result<Vec<u8>> {
    let portable = Portable::of(inputs)?;
    let mut out = BoundedWriter(Vec::new());
    serde_json::to_writer(&mut out, &portable).map_err(|_| REFUSED.to_owned())?;
    lexical_bounds(&out.0)?;
    Ok(out.0)
}

/// A bounds scan only. Serde remains the authoritative JSON parser. This scan
/// allocates nothing, decodes no escapes and does not accept a document.
fn lexical_bounds(bytes: &[u8]) -> Result<()> {
    if bytes.len() > ENCODED {
        return refused();
    }
    let mut position = 0;
    let mut depth = 0_usize;
    let mut tokens = 0_usize;
    while position < bytes.len() {
        match bytes[position] {
            b'"' => {
                add(&mut tokens, 1, JSON_TOKENS)?;
                position += 1;
                let start = position;
                loop {
                    if position - start > JSON_STRING || position == bytes.len() {
                        return refused();
                    }
                    match bytes[position] {
                        b'"' => {
                            position += 1;
                            break;
                        }
                        b'\\' => {
                            position = position.checked_add(2).ok_or(REFUSED)?;
                            if position > bytes.len() {
                                return refused();
                            }
                        }
                        _ => position += 1,
                    }
                }
            }
            b'{' | b'[' => {
                add(&mut tokens, 1, JSON_TOKENS)?;
                add(&mut depth, 1, JSON_DEPTH)?;
                position += 1;
            }
            b'}' | b']' => {
                depth = depth.checked_sub(1).ok_or(REFUSED)?;
                position += 1;
            }
            b',' | b':' | b' ' | b'\t' | b'\r' | b'\n' => position += 1,
            _ => {
                add(&mut tokens, 1, JSON_TOKENS)?;
                let start = position;
                while position < bytes.len()
                    && !matches!(
                        bytes[position],
                        b'"' | b'{'
                            | b'['
                            | b'}'
                            | b']'
                            | b','
                            | b':'
                            | b' '
                            | b'\t'
                            | b'\r'
                            | b'\n'
                    )
                {
                    position += 1;
                    // serde_json's i128 path uses a temporary digit String.
                    if position - start > JSON_PRIMITIVE {
                        return refused();
                    }
                }
            }
        }
    }
    if depth != 0 {
        return refused();
    }
    Ok(())
}

struct KeySeed(&'static [&'static str]);
impl<'de> DeserializeSeed<'de> for KeySeed {
    type Value = Option<usize>;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        struct KeyVisitor(&'static [&'static str]);
        impl Visitor<'_> for KeyVisitor {
            type Value = Option<usize>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a field name")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
                Ok(self.0.iter().position(|field| *field == value))
            }
        }
        deserializer.deserialize_identifier(KeyVisitor(self.0))
    }
}

struct ChoiceSeed(&'static [&'static str]);
impl<'de> DeserializeSeed<'de> for ChoiceSeed {
    type Value = usize;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        struct ChoiceVisitor(&'static [&'static str]);
        impl Visitor<'_> for ChoiceVisitor {
            type Value = usize;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a supported semantic tag")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<usize, E> {
                self.0
                    .iter()
                    .position(|tag| *tag == value)
                    .ok_or_else(|| E::custom(REFUSED))
            }
        }
        deserializer.deserialize_str(ChoiceVisitor(self.0))
    }
}
fn mark<E: de::Error>(seen: &mut u64, key: usize) -> std::result::Result<(), E> {
    let bit = 1_u64
        .checked_shl(convert(key).map_err(|_| E::custom(REFUSED))?)
        .ok_or_else(|| E::custom(REFUSED))?;
    if *seen & bit != 0 {
        return Err(E::custom(REFUSED));
    }
    *seen |= bit;
    Ok(())
}
fn required<T, E: de::Error>(value: Option<T>) -> std::result::Result<T, E> {
    value.ok_or_else(|| E::custom(REFUSED))
}

#[derive(Clone, Copy)]
enum BytesKind {
    Raw,
    Name,
    Text,
    Suffix,
}
impl Budget {
    fn decoded(&mut self, length: usize, kind: BytesKind, limits: Limits) -> Result<()> {
        match kind {
            BytesKind::Text => {
                if length > limits.file {
                    return refused();
                }
                add(&mut self.text, length, limits.bytes)
            }
            BytesKind::Name => {
                add(&mut self.names, 1, NAMES)?;
                add(&mut self.name_bytes, length, limits.name_bytes)?;
                if length > limits.path {
                    return refused();
                }
                add(&mut self.raw, length, RAW_BYTES)
            }
            BytesKind::Raw => {
                if length > limits.path {
                    return refused();
                }
                add(&mut self.raw, length, RAW_BYTES)
            }
            BytesKind::Suffix => {
                if length > limits.path {
                    return refused();
                }
                let expanded = limits
                    .path
                    .checked_mul(limits.links.checked_add(1).ok_or(REFUSED)?)
                    .ok_or(REFUSED)?;
                add(&mut self.suffix, length, expanded)?;
                add(&mut self.raw, length, RAW_BYTES)
            }
        }
    }
}

/// The injected allocator is also the ordering-control seam in private tests.
/// Quotas are checked before this function invokes it. At most two extra bytes
/// per field are temporary base64 decode capacity, not stored content credit.
fn decode_raw_with(
    value: &str,
    kind: BytesKind,
    limits: Limits,
    budget: &mut Budget,
    allocate: impl FnOnce(usize) -> Result<Vec<u8>>,
) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(4)
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
    {
        return refused();
    }
    let padding = if value.ends_with("==") {
        2
    } else if value.ends_with('=') {
        1
    } else {
        0
    };
    let capacity = (value.len() / 4).checked_mul(3).ok_or(REFUSED)?;
    let length = capacity.checked_sub(padding).ok_or(REFUSED)?;
    budget.decoded(length, kind, limits)?;
    let mut output = allocate(capacity)?;
    if output.len() != capacity {
        return refused();
    }
    let actual = STANDARD
        .decode_slice(value, &mut output)
        .map_err(|_| REFUSED)?;
    if actual != length {
        return refused();
    }
    output.truncate(actual);
    Ok(output)
}
fn allocation(length: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reserve(&mut bytes, length)?;
    bytes.resize(length, 0);
    Ok(bytes)
}
struct RawSeed<'a> {
    budget: &'a mut Budget,
    limits: Limits,
    kind: BytesKind,
}
impl<'de> DeserializeSeed<'de> for RawSeed<'_> {
    type Value = Raw;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Raw, D::Error> {
        struct RawVisitor<'a>(RawSeed<'a>);
        impl Visitor<'_> for RawVisitor<'_> {
            type Value = Raw;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded base64 bytes")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Raw, E> {
                decode_raw_with(value, self.0.kind, self.0.limits, self.0.budget, allocation)
                    .map(Raw)
                    .map_err(|_| E::custom(REFUSED))
            }
        }
        deserializer.deserialize_str(RawVisitor(self))
    }
}

const ENVELOPE_FIELDS: &[&str] = &[
    "schema",
    "op",
    "root",
    "root_stamp",
    "limits",
    "texts",
    "directories",
];
struct Header {
    root: Raw,
    stamp: Stamp,
    limits: Bounds,
}
struct HeaderSeed;
impl<'de> DeserializeSeed<'de> for HeaderSeed {
    type Value = Header;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Header, D::Error> {
        struct HeaderVisitor;
        impl<'de> Visitor<'de> for HeaderVisitor {
            type Value = Header;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a saved head envelope")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Header, M::Error> {
                let mut seen = 0;
                let mut schema = None;
                let mut op = None;
                let mut root = None;
                let mut stamp: Option<Stamp> = None;
                let mut limits: Option<Bounds> = None;
                let mut small = Budget::default();
                while let Some(key) = map.next_key_seed(KeySeed(ENVELOPE_FIELDS))? {
                    if let Some(key) = key {
                        mark::<M::Error>(&mut seen, key)?;
                        match key {
                            0 => schema = Some(map.next_value::<u32>()?),
                            1 => op = Some(map.next_value_seed(ChoiceSeed(&[OP]))?),
                            2 => {
                                root = Some(map.next_value_seed(RawSeed {
                                    budget: &mut small,
                                    limits: WIRE_LIMITS,
                                    kind: BytesKind::Raw,
                                })?)
                            }
                            3 => stamp = Some(map.next_value()?),
                            4 => limits = Some(map.next_value()?),
                            _ => {
                                map.next_value::<IgnoredAny>()?;
                            }
                        }
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                if schema != Some(SCHEMA) || op != Some(0) || seen & 0x7f != 0x7f {
                    return Err(de::Error::custom(REFUSED));
                }
                let root: Raw = required(root)?;
                let stamp = required(stamp)?;
                let limits = required(limits)?;
                let native = limits.native().map_err(|_| de::Error::custom(REFUSED))?;
                let path = Path::new(OsStr::from_bytes(&root.0));
                key_path(path, native).map_err(|_| de::Error::custom(REFUSED))?;
                stamp
                    .kind(FileType::Directory)
                    .map_err(|_| de::Error::custom(REFUSED))?;
                Ok(Header {
                    root,
                    stamp,
                    limits,
                })
            }
        }
        deserializer.deserialize_map(HeaderVisitor)
    }
}

struct WitnessSeed<'a> {
    budget: &'a mut Budget,
    limits: Limits,
    allowed: bool,
}
impl<'de> DeserializeSeed<'de> for WitnessSeed<'_> {
    type Value = SavedWitness;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        if !self.allowed {
            return Err(de::Error::custom(REFUSED));
        }
        self.budget
            .witness()
            .map_err(|_| de::Error::custom(REFUSED))?;
        struct WitnessVisitor<'a>(WitnessSeed<'a>);
        impl<'de> Visitor<'de> for WitnessVisitor<'_> {
            type Value = SavedWitness;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a bounded witness")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                let mut seen = 0;
                let mut kind = None;
                let mut stamp: Option<Stamp> = None;
                let mut target = None;
                while let Some(key) = map.next_key_seed(KeySeed(&["kind", "stamp", "target"]))? {
                    if let Some(key) = key {
                        mark::<M::Error>(&mut seen, key)?;
                        match key {
                            0 => {
                                kind =
                                    Some(map.next_value_seed(ChoiceSeed(&["directory", "link"]))?)
                            }
                            1 => stamp = Some(map.next_value()?),
                            2 => {
                                target = Some(map.next_value_seed(RawSeed {
                                    budget: &mut *self.0.budget,
                                    limits: self.0.limits,
                                    kind: BytesKind::Raw,
                                })?)
                            }
                            _ => unreachable!(),
                        }
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                let stamp = required(stamp)?;
                match (kind, seen) {
                    (Some(0), 3) => {
                        stamp
                            .kind(FileType::Directory)
                            .map_err(|_| de::Error::custom(REFUSED))?;
                        Ok(SavedWitness::Directory(stamp))
                    }
                    (Some(1), 7) => {
                        stamp
                            .kind(FileType::Symlink)
                            .map_err(|_| de::Error::custom(REFUSED))?;
                        let target: Raw = required(target)?;
                        if target.0.is_empty() || target.0.contains(&0) {
                            return Err(de::Error::custom(REFUSED));
                        }
                        Ok(SavedWitness::Link { stamp, target })
                    }
                    _ => Err(de::Error::custom(REFUSED)),
                }
            }
        }
        deserializer.deserialize_map(WitnessVisitor(self))
    }
}
struct WitnessesSeed<'a> {
    budget: &'a mut Budget,
    limits: Limits,
}
impl<'de> DeserializeSeed<'de> for WitnessesSeed<'_> {
    type Value = Vec<SavedWitness>;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        struct WitnessesVisitor<'a>(WitnessesSeed<'a>);
        impl<'de> Visitor<'de> for WitnessesVisitor<'_> {
            type Value = Vec<SavedWitness>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded witnesses")
            }
            fn visit_seq<S: SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> std::result::Result<Self::Value, S::Error> {
                let mut values = Vec::new();
                let mut links = 0;
                while let Some(value) = seq.next_element_seed(WitnessSeed {
                    budget: &mut *self.0.budget,
                    limits: self.0.limits,
                    allowed: values.len() <= self.0.limits.components,
                })? {
                    if matches!(value, SavedWitness::Link { .. }) {
                        links += 1;
                        if links > self.0.limits.links {
                            return Err(de::Error::custom(REFUSED));
                        }
                    }
                    reserve(&mut values, 1).map_err(|_| de::Error::custom(REFUSED))?;
                    values.push(value);
                }
                if values.is_empty() {
                    return Err(de::Error::custom(REFUSED));
                }
                Ok(values)
            }
        }
        deserializer.deserialize_seq(WitnessesVisitor(self))
    }
}

struct SuffixSeed<'a> {
    budget: &'a mut Budget,
    limits: Limits,
}
impl<'de> DeserializeSeed<'de> for SuffixSeed<'_> {
    type Value = Vec<Raw>;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        self.budget.suffix = 0;
        struct PartSeed<'a> {
            budget: &'a mut Budget,
            limits: Limits,
            allowed: bool,
        }
        impl<'de> DeserializeSeed<'de> for PartSeed<'_> {
            type Value = Raw;
            fn deserialize<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> std::result::Result<Raw, D::Error> {
                if !self.allowed {
                    return Err(de::Error::custom(REFUSED));
                }
                let value = RawSeed {
                    budget: self.budget,
                    limits: self.limits,
                    kind: BytesKind::Suffix,
                }
                .deserialize(deserializer)?;
                component(&value.0).map_err(|_| de::Error::custom(REFUSED))?;
                Ok(value)
            }
        }
        struct SuffixVisitor<'a>(SuffixSeed<'a>);
        impl<'de> Visitor<'de> for SuffixVisitor<'_> {
            type Value = Vec<Raw>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a bounded unresolved suffix")
            }
            fn visit_seq<S: SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> std::result::Result<Self::Value, S::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element_seed(PartSeed {
                    budget: &mut *self.0.budget,
                    limits: self.0.limits,
                    allowed: values.len() < self.0.limits.components,
                })? {
                    reserve(&mut values, 1).map_err(|_| de::Error::custom(REFUSED))?;
                    values.push(value);
                }
                if values.len() < 2 {
                    return Err(de::Error::custom(REFUSED));
                }
                Ok(values)
            }
        }
        deserializer.deserialize_seq(SuffixVisitor(self))
    }
}

struct TextSeed<'a> {
    budget: &'a mut Budget,
    limits: Limits,
}
impl<'de> DeserializeSeed<'de> for TextSeed<'_> {
    type Value = SavedText;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        struct TextVisitor<'a>(TextSeed<'a>);
        impl<'de> Visitor<'de> for TextVisitor<'_> {
            type Value = SavedText;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a closed text fact")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                let mut seen = 0;
                let mut kind = None;
                let mut text = None;
                let mut stamp: Option<Stamp> = None;
                let mut leaf: Option<Stamp> = None;
                let mut null: Option<Stamp> = None;
                let mut suffix = None;
                while let Some(key) = map.next_key_seed(KeySeed(&[
                    "kind", "text", "stamp", "leaf", "null", "suffix",
                ]))? {
                    if let Some(key) = key {
                        mark::<M::Error>(&mut seen, key)?;
                        match key {
                            0 => {
                                kind = Some(map.next_value_seed(ChoiceSeed(&[
                                    "regular",
                                    "null_mask",
                                    "missing_leaf",
                                    "missing_ancestor",
                                ]))?)
                            }
                            1 => {
                                text = Some(map.next_value_seed(RawSeed {
                                    budget: &mut *self.0.budget,
                                    limits: self.0.limits,
                                    kind: BytesKind::Text,
                                })?)
                            }
                            2 => stamp = Some(map.next_value()?),
                            3 => leaf = Some(map.next_value()?),
                            4 => null = Some(map.next_value()?),
                            5 => {
                                suffix = Some(map.next_value_seed(SuffixSeed {
                                    budget: &mut *self.0.budget,
                                    limits: self.0.limits,
                                })?)
                            }
                            _ => unreachable!(),
                        }
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                match (kind, seen) {
                    (Some(0), 7) => {
                        let stamp = required(stamp)?;
                        let text: Raw = required(text)?;
                        stamp
                            .kind(FileType::RegularFile)
                            .map_err(|_| de::Error::custom(REFUSED))?;
                        if convert::<_, usize>(stamp.size)
                            .map_err(|_| de::Error::custom(REFUSED))?
                            != text.0.len()
                        {
                            return Err(de::Error::custom(REFUSED));
                        }
                        let text =
                            String::from_utf8(text.0).map_err(|_| de::Error::custom(REFUSED))?;
                        Ok(SavedText::Regular { text, stamp })
                    }
                    (Some(1), 25) => {
                        let leaf = required(leaf)?;
                        let null = required(null)?;
                        leaf.kind(FileType::CharacterDevice)
                            .map_err(|_| de::Error::custom(REFUSED))?;
                        null.kind(FileType::CharacterDevice)
                            .map_err(|_| de::Error::custom(REFUSED))?;
                        if leaf.rdev != null.rdev {
                            return Err(de::Error::custom(REFUSED));
                        }
                        Ok(SavedText::NullMask { leaf, null })
                    }
                    (Some(2), 1) => Ok(SavedText::MissingLeaf),
                    (Some(3), 33) => Ok(SavedText::MissingAncestor {
                        suffix: required(suffix)?,
                    }),
                    _ => Err(de::Error::custom(REFUSED)),
                }
            }
        }
        deserializer.deserialize_map(TextVisitor(self))
    }
}

fn checked_saved_path(path: &Raw, header: &Header) -> Result<()> {
    let value = Path::new(OsStr::from_bytes(&path.0));
    key_path(value, header.limits.native()?).map_err(|_| REFUSED.to_owned())?;
    if !value.starts_with(Path::new(OsStr::from_bytes(&header.root.0))) {
        return refused();
    }
    Ok(())
}
fn checked_saved_chain(chain: &[SavedWitness], header: &Header) -> Result<()> {
    if !matches!(chain.first(), Some(SavedWitness::Directory(stamp)) if *stamp == header.stamp) {
        return refused();
    }
    Ok(())
}

struct TextRowSeed<'a> {
    budget: &'a mut Budget,
    header: &'a Header,
}
impl<'de> DeserializeSeed<'de> for TextRowSeed<'_> {
    type Value = TextRow;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<TextRow, D::Error> {
        let limits = self
            .header
            .limits
            .native()
            .map_err(|_| de::Error::custom(REFUSED))?;
        self.budget
            .fact(limits)
            .map_err(|_| de::Error::custom(REFUSED))?;
        struct RowVisitor<'a>(TextRowSeed<'a>);
        impl<'de> Visitor<'de> for RowVisitor<'_> {
            type Value = TextRow;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a bounded text row")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<TextRow, M::Error> {
                let limits = self
                    .0
                    .header
                    .limits
                    .native()
                    .map_err(|_| de::Error::custom(REFUSED))?;
                let mut seen = 0;
                let mut path = None;
                let mut witnesses = None;
                let mut fact = None;
                while let Some(key) = map.next_key_seed(KeySeed(&["path", "witnesses", "fact"]))? {
                    if let Some(key) = key {
                        mark::<M::Error>(&mut seen, key)?;
                        match key {
                            0 => {
                                path = Some(map.next_value_seed(RawSeed {
                                    budget: &mut *self.0.budget,
                                    limits,
                                    kind: BytesKind::Raw,
                                })?)
                            }
                            1 => {
                                witnesses = Some(map.next_value_seed(WitnessesSeed {
                                    budget: &mut *self.0.budget,
                                    limits,
                                })?)
                            }
                            2 => {
                                fact = Some(map.next_value_seed(TextSeed {
                                    budget: &mut *self.0.budget,
                                    limits,
                                })?)
                            }
                            _ => unreachable!(),
                        }
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                if seen != 7 {
                    return Err(de::Error::custom(REFUSED));
                }
                let path = required(path)?;
                let witnesses = required(witnesses)?;
                checked_saved_path(&path, self.0.header).map_err(|_| de::Error::custom(REFUSED))?;
                if path.0 == self.0.header.root.0 {
                    return Err(de::Error::custom(REFUSED));
                }
                checked_saved_chain(&witnesses, self.0.header)
                    .map_err(|_| de::Error::custom(REFUSED))?;
                Ok(TextRow {
                    path,
                    witnesses,
                    fact: required(fact)?,
                })
            }
        }
        deserializer.deserialize_map(RowVisitor(self))
    }
}

struct NamesSeed<'a> {
    budget: &'a mut Budget,
    limits: Limits,
}
impl<'de> DeserializeSeed<'de> for NamesSeed<'_> {
    type Value = Vec<Raw>;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        struct NameSeed<'a> {
            budget: &'a mut Budget,
            limits: Limits,
            allowed: bool,
        }
        impl<'de> DeserializeSeed<'de> for NameSeed<'_> {
            type Value = Raw;
            fn deserialize<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> std::result::Result<Raw, D::Error> {
                if !self.allowed {
                    return Err(de::Error::custom(REFUSED));
                }
                let value = RawSeed {
                    budget: self.budget,
                    limits: self.limits,
                    kind: BytesKind::Name,
                }
                .deserialize(deserializer)?;
                component(&value.0).map_err(|_| de::Error::custom(REFUSED))?;
                if matches!(value.0.as_slice(), b"." | b"..") {
                    return Err(de::Error::custom(REFUSED));
                }
                Ok(value)
            }
        }
        struct NamesVisitor<'a>(NamesSeed<'a>);
        impl<'de> Visitor<'de> for NamesVisitor<'_> {
            type Value = Vec<Raw>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded raw names")
            }
            fn visit_seq<S: SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> std::result::Result<Self::Value, S::Error> {
                let mut names: Vec<Raw> = Vec::new();
                while let Some(name) = seq.next_element_seed(NameSeed {
                    budget: &mut *self.0.budget,
                    limits: self.0.limits,
                    allowed: names.len() < self.0.limits.names,
                })? {
                    if names.contains(&name) {
                        return Err(de::Error::custom(REFUSED));
                    }
                    reserve(&mut names, 1).map_err(|_| de::Error::custom(REFUSED))?;
                    names.push(name);
                }
                Ok(names)
            }
        }
        deserializer.deserialize_seq(NamesVisitor(self))
    }
}

struct OptionalNamesSeed<'a>(NamesSeed<'a>);
impl<'de> DeserializeSeed<'de> for OptionalNamesSeed<'_> {
    type Value = Option<Vec<Raw>>;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        struct OptionalVisitor<'a>(OptionalNamesSeed<'a>);
        impl<'de> Visitor<'de> for OptionalVisitor<'_> {
            type Value = Option<Vec<Raw>>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("names or explicit absence")
            }
            fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(None)
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(None)
            }
            fn visit_some<D: Deserializer<'de>>(
                self,
                d: D,
            ) -> std::result::Result<Self::Value, D::Error> {
                self.0.0.deserialize(d).map(Some)
            }
        }
        deserializer.deserialize_option(OptionalVisitor(self))
    }
}
struct OptionalMissingSeed<'a>(TextSeed<'a>);
impl<'de> DeserializeSeed<'de> for OptionalMissingSeed<'_> {
    type Value = Option<SavedText>;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        struct OptionalVisitor<'a>(OptionalMissingSeed<'a>);
        impl<'de> Visitor<'de> for OptionalVisitor<'_> {
            type Value = Option<SavedText>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an explicit missing fact or null")
            }
            fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(None)
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(None)
            }
            fn visit_some<D: Deserializer<'de>>(
                self,
                d: D,
            ) -> std::result::Result<Self::Value, D::Error> {
                let value = self.0.0.deserialize(d)?;
                if !matches!(
                    value,
                    SavedText::MissingLeaf | SavedText::MissingAncestor { .. }
                ) {
                    return Err(de::Error::custom(REFUSED));
                }
                Ok(Some(value))
            }
        }
        deserializer.deserialize_option(OptionalVisitor(self))
    }
}

struct DirectoryRowSeed<'a> {
    budget: &'a mut Budget,
    header: &'a Header,
}
impl<'de> DeserializeSeed<'de> for DirectoryRowSeed<'_> {
    type Value = DirectoryRow;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        let limits = self
            .header
            .limits
            .native()
            .map_err(|_| de::Error::custom(REFUSED))?;
        self.budget
            .fact(limits)
            .map_err(|_| de::Error::custom(REFUSED))?;
        struct RowVisitor<'a>(DirectoryRowSeed<'a>);
        impl<'de> Visitor<'de> for RowVisitor<'_> {
            type Value = DirectoryRow;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a bounded directory row")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                let limits = self
                    .0
                    .header
                    .limits
                    .native()
                    .map_err(|_| de::Error::custom(REFUSED))?;
                let mut seen = 0;
                let mut path = None;
                let mut witnesses = None;
                let mut names: Option<Vec<Raw>> = None;
                let mut missing: Option<SavedText> = None;
                while let Some(key) =
                    map.next_key_seed(KeySeed(&["path", "witnesses", "names", "missing"]))?
                {
                    if let Some(key) = key {
                        mark::<M::Error>(&mut seen, key)?;
                        match key {
                            0 => {
                                path = Some(map.next_value_seed(RawSeed {
                                    budget: &mut *self.0.budget,
                                    limits,
                                    kind: BytesKind::Raw,
                                })?)
                            }
                            1 => {
                                witnesses = Some(map.next_value_seed(WitnessesSeed {
                                    budget: &mut *self.0.budget,
                                    limits,
                                })?)
                            }
                            2 => {
                                names = map.next_value_seed(OptionalNamesSeed(NamesSeed {
                                    budget: &mut *self.0.budget,
                                    limits,
                                }))?
                            }
                            3 => {
                                missing = map.next_value_seed(OptionalMissingSeed(TextSeed {
                                    budget: &mut *self.0.budget,
                                    limits,
                                }))?
                            }
                            _ => unreachable!(),
                        }
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                if seen != 15 || names.is_some() == missing.is_some() {
                    return Err(de::Error::custom(REFUSED));
                }
                let path = required(path)?;
                let witnesses = required(witnesses)?;
                checked_saved_path(&path, self.0.header).map_err(|_| de::Error::custom(REFUSED))?;
                checked_saved_chain(&witnesses, self.0.header)
                    .map_err(|_| de::Error::custom(REFUSED))?;
                if path.0 == self.0.header.root.0 && (names.is_none() || witnesses.len() != 1) {
                    return Err(de::Error::custom(REFUSED));
                }
                if let Some(names) = &names {
                    for name in names {
                        joined_name(path.0.len(), name.0.len(), limits.path)
                            .map_err(|_| de::Error::custom(REFUSED))?;
                    }
                }
                Ok(DirectoryRow {
                    path,
                    witnesses,
                    names,
                    missing,
                })
            }
        }
        deserializer.deserialize_map(RowVisitor(self))
    }
}

struct TextRowsSeed<'a> {
    budget: &'a mut Budget,
    header: &'a Header,
}
impl<'de> DeserializeSeed<'de> for TextRowsSeed<'_> {
    type Value = Vec<TextRow>;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        struct RowsVisitor<'a>(TextRowsSeed<'a>);
        impl<'de> Visitor<'de> for RowsVisitor<'_> {
            type Value = Vec<TextRow>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded ordered text facts")
            }
            fn visit_seq<S: SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> std::result::Result<Self::Value, S::Error> {
                let mut rows: Vec<TextRow> = Vec::new();
                while let Some(row) = seq.next_element_seed(TextRowSeed {
                    budget: &mut *self.0.budget,
                    header: self.0.header,
                })? {
                    if rows.last().is_some_and(|last| last.path.0 >= row.path.0) {
                        return Err(de::Error::custom(REFUSED));
                    }
                    reserve(&mut rows, 1).map_err(|_| de::Error::custom(REFUSED))?;
                    rows.push(row);
                }
                Ok(rows)
            }
        }
        deserializer.deserialize_seq(RowsVisitor(self))
    }
}
struct DirectoryRowsSeed<'a> {
    budget: &'a mut Budget,
    header: &'a Header,
}
impl<'de> DeserializeSeed<'de> for DirectoryRowsSeed<'_> {
    type Value = Vec<DirectoryRow>;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        struct RowsVisitor<'a>(DirectoryRowsSeed<'a>);
        impl<'de> Visitor<'de> for RowsVisitor<'_> {
            type Value = Vec<DirectoryRow>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded ordered directory facts")
            }
            fn visit_seq<S: SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> std::result::Result<Self::Value, S::Error> {
                let mut rows: Vec<DirectoryRow> = Vec::new();
                while let Some(row) = seq.next_element_seed(DirectoryRowSeed {
                    budget: &mut *self.0.budget,
                    header: self.0.header,
                })? {
                    if rows.last().is_some_and(|last| last.path.0 >= row.path.0) {
                        return Err(de::Error::custom(REFUSED));
                    }
                    reserve(&mut rows, 1).map_err(|_| de::Error::custom(REFUSED))?;
                    rows.push(row);
                }
                Ok(rows)
            }
        }
        deserializer.deserialize_seq(RowsVisitor(self))
    }
}

struct FactsSeed<'a> {
    budget: &'a mut Budget,
    header: &'a Header,
}
impl<'de> DeserializeSeed<'de> for FactsSeed<'_> {
    type Value = (Vec<TextRow>, Vec<DirectoryRow>);
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        struct FactsVisitor<'a>(FactsSeed<'a>);
        impl<'de> Visitor<'de> for FactsVisitor<'_> {
            type Value = (Vec<TextRow>, Vec<DirectoryRow>);
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a saved head envelope")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                let mut seen = 0;
                let mut texts = None;
                let mut directories = None;
                while let Some(key) = map.next_key_seed(KeySeed(ENVELOPE_FIELDS))? {
                    if let Some(key) = key {
                        mark::<M::Error>(&mut seen, key)?;
                        match key {
                            5 => {
                                texts = Some(map.next_value_seed(TextRowsSeed {
                                    budget: &mut *self.0.budget,
                                    header: self.0.header,
                                })?)
                            }
                            6 => {
                                directories = Some(map.next_value_seed(DirectoryRowsSeed {
                                    budget: &mut *self.0.budget,
                                    header: self.0.header,
                                })?)
                            }
                            _ => {
                                map.next_value::<IgnoredAny>()?;
                            }
                        }
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                if seen != 127 {
                    return Err(de::Error::custom(REFUSED));
                }
                Ok((required(texts)?, required(directories)?))
            }
        }
        deserializer.deserialize_map(FactsVisitor(self))
    }
}

fn cross_list_coherence(texts: &[TextRow], directories: &[DirectoryRow]) -> Result<()> {
    let (mut text, mut directory) = (0, 0);
    while text < texts.len() && directory < directories.len() {
        let left = &texts[text];
        let right = &directories[directory];
        match left.path.0.cmp(&right.path.0) {
            std::cmp::Ordering::Less => text += 1,
            std::cmp::Ordering::Greater => directory += 1,
            std::cmp::Ordering::Equal => {
                if right.names.is_some()
                    || right.missing.as_ref() != Some(&left.fact)
                    || !matches!(
                        &left.fact,
                        SavedText::MissingLeaf | SavedText::MissingAncestor { .. }
                    )
                    || left.witnesses != right.witnesses
                {
                    return refused();
                }
                text += 1;
                directory += 1;
            }
        }
    }
    Ok(())
}

/// Exact expected identity is checked before parsing, not established here.
/// Neither a matching digest nor decoding authenticates root, origin or intent.
#[allow(
    dead_code,
    reason = "Staged private read-only codec; no guard or storage wiring exists"
)]
pub(crate) fn decode(
    bytes: &[u8],
    expected_sha: &[u8; 32],
    expected_size: u64,
) -> Result<SavedInputs> {
    if bytes.len() > ENCODED || convert::<_, u64>(bytes.len())? != expected_size {
        return refused();
    }
    let actual = ring::digest::digest(&ring::digest::SHA256, bytes);
    if actual.as_ref() != expected_sha {
        return refused();
    }
    // Ignored future metadata must not bypass whole-document UTF-8 validation.
    std::str::from_utf8(bytes).map_err(|_| REFUSED.to_owned())?;
    lexical_bounds(bytes)?;
    // A first streaming pass admits semantic header/widths before allocating
    // any fact tree. IgnoredAny builds no Value or serde Content tree.
    let mut parser = serde_json::Deserializer::from_slice(bytes);
    let header = HeaderSeed
        .deserialize(&mut parser)
        .map_err(|_| REFUSED.to_owned())?;
    parser.end().map_err(|_| REFUSED.to_owned())?;
    let limits = header.limits.native()?;
    let mut budget = Budget::default();
    budget.raw(&header.root.0, limits.path)?;
    let mut parser = serde_json::Deserializer::from_slice(bytes);
    let (texts, directories) = FactsSeed {
        budget: &mut budget,
        header: &header,
    }
    .deserialize(&mut parser)
    .map_err(|_| REFUSED.to_owned())?;
    parser.end().map_err(|_| REFUSED.to_owned())?;
    cross_list_coherence(&texts, &directories)?;
    let encoded = copy_bytes(bytes)?.into_boxed_slice();
    Ok(SavedInputs {
        facts: Portable {
            schema: SCHEMA,
            op: OP,
            root: header.root,
            root_stamp: header.stamp,
            limits: header.limits,
            texts,
            directories,
        },
        encoded,
    })
}

fn same_text(observed: &TextFact, saved: &TextRow) -> Result<bool> {
    Ok(saved_witnesses(&observed.witnesses)? == saved.witnesses
        && saved_text(&observed.text)? == saved.fact)
}
fn same_directory(observed: &DirectoryFact, saved: &DirectoryRow) -> Result<bool> {
    if saved_witnesses(&observed.witnesses)? != saved.witnesses
        || observed.missing.as_ref().map(saved_text).transpose()? != saved.missing
    {
        return Ok(false);
    }
    match (&observed.names, &saved.names) {
        (None, None) => Ok(true),
        (Some(observed), Some(saved)) => {
            // Recheck is a set comparison. Persisted replay order stays intact.
            let mut left: Vec<_> = observed.iter().map(|name| name.as_bytes()).collect();
            let mut right: Vec<_> = saved.iter().map(|name| name.0.as_slice()).collect();
            left.sort_unstable();
            right.sort_unstable();
            Ok(left == right)
        }
        _ => Ok(false),
    }
}
impl SavedInputs {
    /// Explicit fresh read-only comparison, never called by decode or replay.
    /// Root ancestry, exclusion and intent/output binding remain caller duties.
    #[allow(
        dead_code,
        reason = "Staged independent recheck; no guard or effects integration exists"
    )]
    pub(crate) fn revalidate(&self, root: &Path) -> Result<()> {
        if root.as_os_str().as_bytes() != self.facts.root.0.as_slice() {
            return refused();
        }
        let limits = self.facts.limits.native()?;
        let mut fresh = Collector::open(root, limits).map_err(|_| REFUSED.to_owned())?;
        if Stamp::of(fresh.root_identity)? != self.facts.root_stamp {
            return refused();
        }
        for saved in &self.facts.texts {
            let path = Path::new(OsStr::from_bytes(&saved.path.0));
            fresh.key(path).map_err(|_| REFUSED.to_owned())?;
            let observed = fresh.observe_text(path).map_err(|_| REFUSED.to_owned())?;
            if !same_text(&observed, saved)? {
                return refused();
            }
        }
        for saved in &self.facts.directories {
            let path = Path::new(OsStr::from_bytes(&saved.path.0));
            fresh.key(path).map_err(|_| REFUSED.to_owned())?;
            let observed = fresh
                .observe_directory(path)
                .map_err(|_| REFUSED.to_owned())?;
            if !same_directory(&observed, saved)? {
                return refused();
            }
        }
        if Stamp::of(identity(&fresh.root).map_err(|_| REFUSED.to_owned())?)?
            != self.facts.root_stamp
        {
            return refused();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
