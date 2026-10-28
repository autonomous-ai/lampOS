//! Present-only projection of independently retained original product bytes.
//!
//! This is not a State loader, complete State validation, authentication,
//! publication or admission. Only the independently selected present raw-State
//! profile is supported. Missing input must never become an inferred default.
use super::{
    Artifact, MOST_COMPONENTS, ProductSelection, REFUSED, Refused, SelectedRecord, hex,
    known_artifact, manifest, require,
};
use serde::{
    Deserializer,
    de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use std::fmt;

const MOST_SOURCE: usize = 16 * 1024 * 1024;
const MOST_MAP: usize = 4096;
const MOST_MADE: usize = 4096;
const MOST_REFUSED: usize = 32;
const MOST_KEY: usize = 4096;
const JSON_DEPTH: usize = 32;
const JSON_STARTS: usize = 131072;
const JSON_STRING: usize = 64 * 1024;
const JSON_PRIMITIVE: usize = 128;
const FIELDS: &[&str] = &[
    "installed",
    "previous",
    "refused",
    "made",
    "settings",
    "settings_writing",
    "systemd_owed",
    "models",
    "models_before",
];

pub(crate) struct OriginalProductBefore {
    encoded: Vec<u8>,
    artifact: Artifact,
}
struct OwnedRecord {
    path: String,
    recorded: Option<String>,
    writing: Option<String>,
}
pub(crate) struct OwnedProductSelection<'source> {
    source: &'source OriginalProductBefore,
    for_sequence: u64,
    installed_sequence: Option<u64>,
    refused: bool,
    records: Vec<OwnedRecord>,
}

impl fmt::Debug for OriginalProductBefore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OriginalProductBefore")
            .field("encoded_bytes", &self.encoded.len())
            .finish()
    }
}
impl fmt::Debug for OwnedProductSelection<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnedProductSelection")
            .field("records", &self.records.len())
            .finish()
    }
}

fn copy_string(value: &str) -> Result<String, Refused> {
    let mut result = String::new();
    result.try_reserve_exact(value.len()).map_err(|_| Refused)?;
    result.push_str(value);
    Ok(result)
}

impl OriginalProductBefore {
    /// Retain exact independently hash-bound bytes, not yet a usable projection.
    #[allow(
        dead_code,
        reason = "Staged private product projection; no parent wiring"
    )]
    pub(crate) fn from_present(
        encoded: Vec<u8>,
        protected_raw_state: &Artifact,
    ) -> Result<Self, Refused> {
        require(
            encoded.len() <= MOST_SOURCE
                && known_artifact(protected_raw_state, MOST_SOURCE as u64)
                && protected_raw_state.size == encoded.len() as u64,
        )?;
        protected_raw_state
            .check(MOST_SOURCE as u64)
            .map_err(|_| Refused)?;
        let artifact = Artifact::of(&encoded);
        require(protected_raw_state.matches(&artifact))?;
        Ok(Self { encoded, artifact })
    }

    #[allow(
        dead_code,
        reason = "Staged private product projection; no parent wiring"
    )]
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.encoded
    }

    #[allow(
        dead_code,
        reason = "Staged private product projection; no parent wiring"
    )]
    pub(crate) fn artifact(&self) -> &Artifact {
        &self.artifact
    }

    /// Parameters are bounded requests, not authenticated manifest facts. The
    /// existing prepare must still verify the actual signed target and order.
    #[allow(
        dead_code,
        reason = "Staged private product projection; no parent wiring"
    )]
    pub(crate) fn select(
        &self,
        target_sequence: u64,
        ordered_paths: &[&str],
    ) -> Result<OwnedProductSelection<'_>, Refused> {
        require(
            target_sequence != 0
                && !ordered_paths.is_empty()
                && ordered_paths.len() <= manifest::MOST_SETTINGS,
        )?;
        // Validate the ENTIRE request before allocating or copying any path.
        for (i, path) in ordered_paths.iter().enumerate() {
            require(
                path.len() <= 511
                    && manifest::is_setting_path(path)
                    && path
                        .strip_prefix('/')
                        .is_some_and(|rest| rest.split('/').count() <= MOST_COMPONENTS)
                    && !ordered_paths[..i].contains(path),
            )?;
        }
        std::str::from_utf8(&self.encoded).map_err(|_| Refused)?;
        lexical_bounds(&self.encoded)?;
        let mut records = Vec::new();
        records
            .try_reserve_exact(ordered_paths.len())
            .map_err(|_| Refused)?;
        for path in ordered_paths {
            records.push(OwnedRecord {
                path: copy_string(path)?,
                recorded: None,
                writing: None,
            });
        }
        let mut parser = serde_json::Deserializer::from_slice(&self.encoded);
        let (installed_sequence, refused) = ProjectionSeed {
            records: &mut records,
            target_sequence,
        }
        .deserialize(&mut parser)
        .map_err(|_| Refused)?;
        parser.end().map_err(|_| Refused)?;
        Ok(OwnedProductSelection {
            source: self,
            for_sequence: target_sequence,
            installed_sequence,
            refused,
            records,
        })
    }
}

impl OwnedProductSelection<'_> {
    #[allow(
        dead_code,
        reason = "Staged private product projection; no parent wiring"
    )]
    pub(crate) fn borrowed_records(&self) -> Result<Vec<SelectedRecord<'_>>, Refused> {
        require(self.records.len() <= manifest::MOST_SETTINGS)?;
        let mut records = Vec::new();
        records
            .try_reserve_exact(self.records.len())
            .map_err(|_| Refused)?;
        for row in &self.records {
            records.push(SelectedRecord {
                path: &row.path,
                recorded: row.recorded.as_deref(),
                writing: row.writing.as_deref(),
            });
        }
        Ok(records)
    }

    #[allow(
        dead_code,
        reason = "Staged private product projection; no parent wiring"
    )]
    pub(crate) fn selection<'a>(
        &'a self,
        records: &'a [SelectedRecord<'a>],
    ) -> Result<ProductSelection<'a>, Refused> {
        require(records.len() == self.records.len())?;
        for (borrowed, owned) in records.iter().zip(&self.records) {
            require(
                borrowed.path == owned.path.as_str()
                    && borrowed.recorded == owned.recorded.as_deref()
                    && borrowed.writing == owned.writing.as_deref(),
            )?;
        }
        Ok(ProductSelection {
            artifact: &self.source.artifact,
            for_sequence: self.for_sequence,
            installed_sequence: self.installed_sequence,
            refused: self.refused,
            records,
        })
    }
}

// A SeqAccess/MapAccess may discover that an entry exists before calling its
// seed. Admission is inside deserialize, before decoding that entry's payload;
// an oversized extra entry is never parsed as IgnoredAny merely to count it.
struct CountedSeed<'a, S> {
    used: &'a mut usize,
    most: usize,
    inner: S,
}
impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for CountedSeed<'_, S> {
    type Value = S::Value;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        if *self.used >= self.most {
            return Err(de::Error::custom(REFUSED));
        }
        *self.used += 1;
        self.inner.deserialize(d)
    }
}

struct FieldSeed(&'static [&'static str]);
impl<'de> DeserializeSeed<'de> for FieldSeed {
    type Value = Option<usize>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        struct FieldVisitor(&'static [&'static str]);
        impl Visitor<'_> for FieldVisitor {
            type Value = Option<usize>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(REFUSED)
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if value.len() > MOST_KEY {
                    return Err(E::custom(REFUSED));
                }
                Ok(self.0.iter().position(|field| *field == value))
            }
        }
        d.deserialize_identifier(FieldVisitor(self.0))
    }
}
fn unique<E: de::Error>(seen: &mut u16, field: usize) -> Result<(), E> {
    let bit = 1u16
        .checked_shl(u32::try_from(field).map_err(|_| E::custom(REFUSED))?)
        .ok_or_else(|| E::custom(REFUSED))?;
    if *seen & bit != 0 {
        return Err(E::custom(REFUSED));
    }
    *seen |= bit;
    Ok(())
}

// Borrowed visitors avoid constructing a String for unselected data. JSON's
// escaped-string scratch is independently bounded by whole-input preflight.
struct StringSeed(usize);
impl<'de> DeserializeSeed<'de> for StringSeed {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        struct StringVisitor(usize);
        impl Visitor<'_> for StringVisitor {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(REFUSED)
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<(), E> {
                if value.len() > self.0 {
                    Err(E::custom(REFUSED))
                } else {
                    Ok(())
                }
            }
        }
        d.deserialize_str(StringVisitor(self.0))
    }
}

struct BuildSeed;
impl<'de> DeserializeSeed<'de> for BuildSeed {
    type Value = u64;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<u64, D::Error> {
        struct BuildVisitor;
        impl<'de> Visitor<'de> for BuildVisitor {
            type Value = u64;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(REFUSED)
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<u64, M::Error> {
                let mut count = 0;
                let mut seen = 0;
                let mut sequence = None;
                while let Some(field) = map.next_key_seed(CountedSeed {
                    used: &mut count,
                    most: MOST_MAP,
                    inner: FieldSeed(&["sequence", "commit"]),
                })? {
                    if let Some(field) = field {
                        unique(&mut seen, field)?;
                    }
                    match field {
                        Some(0) => sequence = Some(map.next_value::<u64>()?),
                        Some(1) => map.next_value_seed(StringSeed(JSON_STRING))?,
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                if seen != 3 {
                    return Err(de::Error::custom(REFUSED));
                }
                sequence.ok_or_else(|| de::Error::custom(REFUSED))
            }
        }
        d.deserialize_map(BuildVisitor)
    }
}
struct OptionalBuildSeed;
impl<'de> DeserializeSeed<'de> for OptionalBuildSeed {
    type Value = Option<u64>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        struct OptionalBuildVisitor;
        impl<'de> Visitor<'de> for OptionalBuildVisitor {
            type Value = Option<u64>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(REFUSED)
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(None)
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(None)
            }
            fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
                BuildSeed.deserialize(d).map(Some)
            }
        }
        d.deserialize_option(OptionalBuildVisitor)
    }
}

struct RefusalSeed(u64);
impl<'de> DeserializeSeed<'de> for RefusalSeed {
    type Value = bool;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<bool, D::Error> {
        struct RefusalVisitor(u64);
        impl<'de> Visitor<'de> for RefusalVisitor {
            type Value = bool;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(REFUSED)
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<bool, M::Error> {
                let mut count = 0;
                let mut seen = 0;
                let mut sequence = None;
                while let Some(field) = map.next_key_seed(CountedSeed {
                    used: &mut count,
                    most: MOST_MAP,
                    inner: FieldSeed(&["sequence", "commit", "why", "at_unix_ms"]),
                })? {
                    if let Some(field) = field {
                        unique(&mut seen, field)?;
                    }
                    match field {
                        Some(0) => sequence = Some(map.next_value::<u64>()?),
                        Some(1 | 2) => map.next_value_seed(StringSeed(JSON_STRING))?,
                        Some(3) => {
                            map.next_value::<u64>()?;
                        }
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                if seen != 15 {
                    return Err(de::Error::custom(REFUSED));
                }
                Ok(sequence.ok_or_else(|| de::Error::custom(REFUSED))? == self.0)
            }
        }
        d.deserialize_map(RefusalVisitor(self.0))
    }
}
struct RefusalsSeed(u64);
impl<'de> DeserializeSeed<'de> for RefusalsSeed {
    type Value = bool;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<bool, D::Error> {
        struct RefusalsVisitor(u64);
        impl<'de> Visitor<'de> for RefusalsVisitor {
            type Value = bool;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(REFUSED)
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<bool, S::Error> {
                let mut count = 0;
                let mut refused = false;
                while let Some(matches) = seq.next_element_seed(CountedSeed {
                    used: &mut count,
                    most: MOST_REFUSED,
                    inner: RefusalSeed(self.0),
                })? {
                    refused |= matches;
                }
                Ok(refused)
            }
        }
        d.deserialize_seq(RefusalsVisitor(self.0))
    }
}
struct MadeSeed;
impl<'de> DeserializeSeed<'de> for MadeSeed {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        struct MadeVisitor;
        impl<'de> Visitor<'de> for MadeVisitor {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(REFUSED)
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<(), S::Error> {
                let mut count = 0;
                while seq
                    .next_element_seed(CountedSeed {
                        used: &mut count,
                        most: MOST_MADE,
                        inner: StringSeed(MOST_KEY),
                    })?
                    .is_some()
                {}
                Ok(())
            }
        }
        d.deserialize_seq(MadeVisitor)
    }
}

struct PathSeed<'a>(&'a [OwnedRecord]);
impl<'de> DeserializeSeed<'de> for PathSeed<'_> {
    type Value = Option<usize>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        struct PathVisitor<'a>(&'a [OwnedRecord]);
        impl Visitor<'_> for PathVisitor<'_> {
            type Value = Option<usize>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(REFUSED)
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if value.len() > MOST_KEY {
                    return Err(E::custom(REFUSED));
                }
                Ok(self.0.iter().position(|row| row.path == value))
            }
        }
        d.deserialize_str(PathVisitor(self.0))
    }
}
struct ValueSeed<'a> {
    selected: Option<&'a mut Option<String>>,
    writing: bool,
}
impl<'de> DeserializeSeed<'de> for ValueSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        struct ValueVisitor<'a>(ValueSeed<'a>);
        impl Visitor<'_> for ValueVisitor<'_> {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(REFUSED)
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<(), E> {
                if let Some(selected) = self.0.selected {
                    if !hex(value, 64) && (self.0.writing || value != crate::install::SET_ASIDE) {
                        return Err(E::custom(REFUSED));
                    }
                    *selected = Some(copy_string(value).map_err(|_| E::custom(REFUSED))?);
                    Ok(())
                } else if value.len() <= JSON_STRING {
                    Ok(())
                } else {
                    Err(E::custom(REFUSED))
                }
            }
        }
        d.deserialize_str(ValueVisitor(self))
    }
}
struct OwnershipSeed<'a> {
    records: &'a mut [OwnedRecord],
    writing: bool,
}
impl<'de> DeserializeSeed<'de> for OwnershipSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        struct OwnershipVisitor<'a>(OwnershipSeed<'a>);
        impl<'de> Visitor<'de> for OwnershipVisitor<'_> {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(REFUSED)
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
                let mut count = 0;
                let mut seen = 0u128;
                while let Some(selected) = map.next_key_seed(CountedSeed {
                    used: &mut count,
                    most: MOST_MAP,
                    inner: PathSeed(&*self.0.records),
                })? {
                    let slot = if let Some(index) = selected {
                        let bit = 1u128
                            .checked_shl(
                                u32::try_from(index).map_err(|_| de::Error::custom(REFUSED))?,
                            )
                            .ok_or_else(|| de::Error::custom(REFUSED))?;
                        if seen & bit != 0 {
                            return Err(de::Error::custom(REFUSED));
                        }
                        seen |= bit;
                        let row = &mut self.0.records[index];
                        Some(if self.0.writing {
                            &mut row.writing
                        } else {
                            &mut row.recorded
                        })
                    } else {
                        None
                    };
                    map.next_value_seed(ValueSeed {
                        selected: slot,
                        writing: self.0.writing,
                    })?;
                }
                Ok(())
            }
        }
        d.deserialize_map(OwnershipVisitor(self))
    }
}

struct ProjectionSeed<'a> {
    records: &'a mut [OwnedRecord],
    target_sequence: u64,
}
impl<'de> DeserializeSeed<'de> for ProjectionSeed<'_> {
    type Value = (Option<u64>, bool);
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        struct ProjectionVisitor<'a>(ProjectionSeed<'a>);
        impl<'de> Visitor<'de> for ProjectionVisitor<'_> {
            type Value = (Option<u64>, bool);
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(REFUSED)
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut count = 0;
                let mut seen = 0;
                let mut installed = None;
                let mut refused = false;
                while let Some(field) = map.next_key_seed(CountedSeed {
                    used: &mut count,
                    most: MOST_MAP,
                    inner: FieldSeed(FIELDS),
                })? {
                    if let Some(field) = field {
                        unique(&mut seen, field)?;
                    }
                    match field {
                        Some(0) => installed = map.next_value_seed(OptionalBuildSeed)?,
                        Some(2) => {
                            refused = map.next_value_seed(RefusalsSeed(self.0.target_sequence))?
                        }
                        Some(3) => map.next_value_seed(MadeSeed)?,
                        Some(4) => map.next_value_seed(OwnershipSeed {
                            records: &mut *self.0.records,
                            writing: false,
                        })?,
                        Some(5) => map.next_value_seed(OwnershipSeed {
                            records: &mut *self.0.records,
                            writing: true,
                        })?,
                        // Known unrelated fields participate in duplicate
                        // checks, but their field-specific types are NOT
                        // validated by this projection profile.
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                if seen & ((1 << 2) | (1 << 3)) != ((1 << 2) | (1 << 3)) {
                    return Err(de::Error::custom(REFUSED));
                }
                Ok((installed, refused))
            }
        }
        d.deserialize_map(ProjectionVisitor(self))
    }
}

fn add(value: &mut usize, amount: usize, cap: usize) -> Result<(), Refused> {
    *value = value.checked_add(amount).ok_or(Refused)?;
    require(*value <= cap)
}

// Adapted from Head codec's lexical bounds, with this projection's OWN caps.
// This only counts starts/depth/encoded string content/primitive lengths;
// Serde remains authoritative for its syntax/types/EOF contract. Whole-byte
// UTF-8 is checked separately; ignored values are opaque and may retain lone
// escaped UTF-16 surrogates that a typed String/Value reader would refuse.
fn lexical_bounds(bytes: &[u8]) -> Result<(), Refused> {
    require(bytes.len() <= MOST_SOURCE)?;
    let mut position = 0;
    let mut depth = 0usize;
    let mut starts = 0usize;
    while position < bytes.len() {
        match bytes[position] {
            b'"' => {
                add(&mut starts, 1, JSON_STARTS)?;
                position += 1;
                let start = position;
                loop {
                    if position - start > JSON_STRING || position == bytes.len() {
                        return Err(Refused);
                    }
                    match bytes[position] {
                        b'"' => {
                            position += 1;
                            break;
                        }
                        b'\\' => {
                            position = position.checked_add(2).ok_or(Refused)?;
                            if position > bytes.len() {
                                return Err(Refused);
                            }
                        }
                        _ => position += 1,
                    }
                }
            }
            b'{' | b'[' => {
                add(&mut starts, 1, JSON_STARTS)?;
                add(&mut depth, 1, JSON_DEPTH)?;
                position += 1;
            }
            b'}' | b']' => {
                depth = depth.checked_sub(1).ok_or(Refused)?;
                position += 1;
            }
            b',' | b':' | b' ' | b'\t' | b'\r' | b'\n' => position += 1,
            _ => {
                add(&mut starts, 1, JSON_STARTS)?;
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
                    if position - start > JSON_PRIMITIVE {
                        return Err(Refused);
                    }
                }
            }
        }
    }
    require(depth == 0)
}
