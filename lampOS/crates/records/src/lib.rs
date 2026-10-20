//! Opt-in retention for recognized, closed voice service records.
//!
//! The inventory is fixed before consulting the live-process guard. Current
//! voice producers exclusively create output directories, so a later start
//! cannot acquire an inventoried directory. This is not a sandbox for a
//! malicious privileged process; configured roots have trusted owners.

pub mod config;
pub mod forward;
pub mod guard;
pub mod json_file;
pub mod plan;
pub mod report;
pub mod run;
pub mod store;

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, DeserializeSeed, SeqAccess, Visitor},
};
use std::{fmt, io, marker::PhantomData, path::PathBuf};

pub const MAX_REPORT_ITEMS: usize = 16_384;
pub const MAX_REPORT_ITEM_BYTES: usize = 1_024;
pub const MAX_REPORT_ERROR_BYTES: usize = 4_096;
pub const MAX_PROCESS_EXAMPLES: usize = 16;
/// Added optional report fields keep this version. New required fields or
/// enum values, or changes to a field's meaning, require a schema bump. So
/// does an added optional field that makes the result worse than its known
/// fields show (a skipped root, a partial count): an older reporter ignores
/// it and would show that report as ok.
pub const REPORT_SCHEMA_VERSION: u32 = 1;

/// Reasons a pinned process could no longer own userspace session writes.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessSkipReason {
    Gone,
    Zombie,
    Exiting,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ProcessSkip {
    pub pid: u32,
    pub reason: ProcessSkipReason,
}

fn process_examples<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<ProcessSkip>, D::Error> {
    sequence::<D, ProcessSkip, MAX_PROCESS_EXAMPLES>(deserializer)
}

/// Observations from up to two process passes, not unique process counts.
/// Examples exclude known kernel threads and never include process arguments.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct ProcessObservations {
    pub gone: u32,
    pub zombie: u32,
    pub exiting: u32,
    pub kernel_threads: u32,
    #[serde(deserialize_with = "process_examples")]
    pub examples: Vec<ProcessSkip>,
}

impl ProcessObservations {
    fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn record(&mut self, pid: u32, reason: ProcessSkipReason) {
        let count = match reason {
            ProcessSkipReason::Gone => &mut self.gone,
            ProcessSkipReason::Zombie => &mut self.zombie,
            ProcessSkipReason::Exiting => &mut self.exiting,
        };
        *count = count.saturating_add(1);
        if self.examples.len() < MAX_PROCESS_EXAMPLES {
            self.examples.push(ProcessSkip { pid, reason });
        }
    }

    fn validate(&self) -> io::Result<()> {
        // Each of the guard's two passes accepts at most 65,536 processes.
        let total = u64::from(self.gone)
            + u64::from(self.zombie)
            + u64::from(self.exiting)
            + u64::from(self.kernel_threads);
        if total > 131_072
            || self.examples.len() > MAX_PROCESS_EXAMPLES
            || self
                .examples
                .iter()
                .any(|item| item.pid == 0 || item.pid > i32::MAX as u32)
        {
            return Err(config::invalid("invalid process observations"));
        }
        for (reason, count) in [
            (ProcessSkipReason::Gone, self.gone),
            (ProcessSkipReason::Zombie, self.zombie),
            (ProcessSkipReason::Exiting, self.exiting),
        ] {
            if self
                .examples
                .iter()
                .filter(|item| item.reason == reason)
                .count()
                > count as usize
            {
                return Err(config::invalid("process examples exceed observed count"));
            }
        }
        Ok(())
    }
}

fn process_observations<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<ProcessObservations, D::Error> {
    let value = ProcessObservations::deserialize(deserializer)?;
    value.validate().map_err(de::Error::custom)?;
    Ok(value)
}

// These limits apply while deserializing, not after allocating arbitrary JSON
// arrays. The JSON reader separately bounds encoded bytes. An escaped string
// may use the decoder's scratch buffer before this visitor receives it, but is
// never retained in the report if it exceeds its decoded UTF-8 byte limit.
struct Text<const MAX: usize>(String);

impl<'de, const MAX: usize> Deserialize<'de> for Text<MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TextVisitor<const MAX: usize>;
        impl<const MAX: usize> Visitor<'_> for TextVisitor<MAX> {
            type Value = Text<MAX>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a string of at most {MAX} UTF-8 bytes")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if value.len() > MAX {
                    return Err(E::custom("report string exceeds its byte limit"));
                }
                Ok(Text(value.to_owned()))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                if value.len() > MAX {
                    return Err(E::custom("report string exceeds its byte limit"));
                }
                Ok(Text(value))
            }
        }
        deserializer.deserialize_str(TextVisitor::<MAX>)
    }
}

struct RejectExtra;

impl<'de> DeserializeSeed<'de> for RejectExtra {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, _: D) -> Result<Self::Value, D::Error> {
        Err(de::Error::custom("too many report entries"))
    }
}

fn sequence<'de, D, T, const MAX: usize>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Sequence<T, const MAX: usize>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const MAX: usize> Visitor<'de> for Sequence<T, MAX> {
        type Value = Vec<T>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "an array of at most {MAX} report entries")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
            // Do not reserve from a deserializer's untrusted size hint.
            let mut values = Vec::new();
            loop {
                if values.len() == MAX {
                    // Determine whether the array ended without deserializing
                    // an extra, possibly huge, nested value.
                    let _ = access.next_element_seed(RejectExtra)?;
                    return Ok(values);
                }
                match access.next_element()? {
                    Some(value) => values.push(value),
                    None => return Ok(values),
                }
            }
        }
    }
    deserializer.deserialize_seq(Sequence::<T, MAX>(PhantomData))
}

fn report_roots<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<RootReport>, D::Error> {
    sequence::<D, RootReport, { config::MAX_ROOTS }>(deserializer)
}

fn report_items<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    sequence::<D, Text<MAX_REPORT_ITEM_BYTES>, MAX_REPORT_ITEMS>(deserializer)
        .map(|items| items.into_iter().map(|item| item.0).collect())
}

fn report_error<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Option::<Text<MAX_REPORT_ERROR_BYTES>>::deserialize(deserializer)
        .map(|value| value.map(|value| value.0))
}

fn report_schema<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let schema = u32::deserialize(deserializer)?;
    if schema != REPORT_SCHEMA_VERSION {
        return Err(de::Error::custom("unsupported report schema"));
    }
    Ok(schema)
}

fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn report_fingerprint<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = Text::<64>::deserialize(deserializer)?.0;
    if !valid_fingerprint(&value) {
        return Err(de::Error::custom("invalid configuration fingerprint"));
    }
    Ok(value)
}

fn valid_mode(value: &str) -> bool {
    matches!(value, "apply" | "dry_run")
}

fn report_mode<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = Text::<7>::deserialize(deserializer)?.0;
    if !valid_mode(&value) {
        return Err(de::Error::custom("invalid report mode"));
    }
    Ok(value)
}

fn report_path<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
    let value = PathBuf::from(Text::<4_096>::deserialize(deserializer)?.0);
    config::absolute_path(&value).map_err(de::Error::custom)?;
    Ok(value)
}

fn report_unit<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = Text::<128>::deserialize(deserializer)?.0;
    config::unit_name(&value).map_err(de::Error::custom)?;
    Ok(value)
}

/// Recognize a newer report without interpreting its nested schema. The
/// existing envelope and encoded-file bound still apply; arbitrary JSON
/// carrying only a version number is not a cleanup report to move aside.
pub(crate) fn newer_report_schema(bytes: &[u8]) -> io::Result<Option<u64>> {
    fn roots<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<de::IgnoredAny>, D::Error> {
        sequence::<D, de::IgnoredAny, { config::MAX_ROOTS }>(deserializer)
    }
    #[derive(Deserialize)]
    struct Envelope {
        schema_version: u64,
        #[serde(rename = "config_fingerprint", deserialize_with = "report_fingerprint")]
        _fingerprint: String,
        #[serde(rename = "at_unix_s")]
        _at: u64,
        #[serde(rename = "mode")]
        _mode: Text<128>,
        #[serde(rename = "enabled")]
        _enabled: bool,
        #[serde(rename = "roots", deserialize_with = "roots")]
        _roots: Vec<de::IgnoredAny>,
    }
    let envelope: Envelope = serde_json::from_slice(bytes)
        .map_err(|_| config::invalid("unrecognized cleanup report"))?;
    Ok((envelope.schema_version > u64::from(REPORT_SCHEMA_VERSION))
        .then_some(envelope.schema_version))
}

/// A report at this build's schema from a newer build that writes a `mode`
/// this one doesn't know (#1680, `docs/updates.md`): the mode is a plain
/// word, lowercase letters and `_`, at most 32, and with this build's own
/// mode in its place every other field reads as this build's report. Set
/// aside whole like a newer schema, never taken as cleanup evidence, so
/// the cleanup runs. A malformed mode, or anything else this build can't
/// read, is still not a cleanup report.
pub(crate) fn newer_report_mode(bytes: &[u8]) -> bool {
    let Ok(mut report) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return false;
    };
    let plain = report["mode"].as_str().is_some_and(|mode| {
        !valid_mode(mode)
            && (1..=32).contains(&mode.len())
            && mode
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    });
    if !plain || report["schema_version"] != REPORT_SCHEMA_VERSION {
        return false;
    }
    report["mode"] = "apply".into();
    serde_json::from_value::<RunReport>(report).is_ok()
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Disk {
    pub total_bytes: u64,
    /// Space available to an ordinary writer, excluding filesystem reserves.
    pub available_bytes: u64,
}

/// Persisted reports tolerate added fields; serialization writes only this
/// build's fields. Known fields, enum values and all bounds remain checked.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunReport {
    #[serde(deserialize_with = "report_schema")]
    pub schema_version: u32,
    /// SHA-256 of the serialized retention configuration, as lowercase hex.
    /// The reporter must compare it with the current configuration, not merely
    /// trust that this syntactically valid digest belongs to that configuration.
    #[serde(deserialize_with = "report_fingerprint")]
    pub config_fingerprint: String,
    pub at_unix_s: u64,
    #[serde(deserialize_with = "report_mode")]
    pub mode: String,
    pub enabled: bool,
    #[serde(deserialize_with = "report_roots")]
    pub roots: Vec<RootReport>,
    #[serde(default, deserialize_with = "report_error")]
    pub error: Option<String>,
}

impl RunReport {
    /// Check the same syntax and resource limits for values built in memory.
    /// Counters describe observations and are not reconstructed or corrected.
    pub fn validate(&self) -> io::Result<()> {
        if self.schema_version != REPORT_SCHEMA_VERSION {
            return Err(config::invalid("unsupported report schema"));
        }
        if !valid_fingerprint(&self.config_fingerprint) {
            return Err(config::invalid("invalid configuration fingerprint"));
        }
        if !valid_mode(&self.mode) || self.roots.len() > config::MAX_ROOTS {
            return Err(config::invalid("invalid report mode or root count"));
        }
        if self
            .error
            .as_ref()
            .is_some_and(|value| value.len() > MAX_REPORT_ERROR_BYTES)
        {
            return Err(config::invalid("report error exceeds its byte limit"));
        }
        for root in &self.roots {
            root.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RootReport {
    #[serde(deserialize_with = "report_path")]
    pub root: PathBuf,
    #[serde(deserialize_with = "report_unit")]
    pub unit: String,
    pub before_bytes: u64,
    pub planned_bytes: u64,
    pub removed_bytes: u64,
    pub remaining_bytes: u64,
    pub limit_bytes: u64,
    #[serde(deserialize_with = "report_items")]
    pub active: Vec<String>,
    #[serde(deserialize_with = "report_items")]
    pub planned: Vec<String>,
    #[serde(deserialize_with = "report_items")]
    pub removed: Vec<String>,
    #[serde(deserialize_with = "report_items")]
    pub refused: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "ProcessObservations::is_empty",
        deserialize_with = "process_observations"
    )]
    pub processes: ProcessObservations,
    pub disk: Option<Disk>,
    #[serde(default, deserialize_with = "report_error")]
    pub error: Option<String>,
}

impl RootReport {
    pub fn validate(&self) -> io::Result<()> {
        config::absolute_path(&self.root)?;
        config::unit_name(&self.unit)?;
        self.processes.validate()?;
        for items in [&self.active, &self.planned, &self.removed, &self.refused] {
            if items.len() > MAX_REPORT_ITEMS
                || items.iter().any(|item| item.len() > MAX_REPORT_ITEM_BYTES)
            {
                return Err(config::invalid(
                    "report entries exceed their count or byte limit",
                ));
            }
        }
        if self
            .error
            .as_ref()
            .is_some_and(|value| value.len() > MAX_REPORT_ERROR_BYTES)
        {
            return Err(config::invalid("report error exceeds its byte limit"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::{cell::Cell, io::Read, rc::Rc};

    #[test]
    fn process_observations_are_optional_for_old_receipts_and_empty_output_is_unchanged() {
        let old = root();
        let parsed: RootReport = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(parsed.processes, ProcessObservations::default());
        assert_eq!(serde_json::to_value(parsed).unwrap(), old);
    }

    #[test]
    fn process_examples_are_bounded_and_remain_in_a_failed_root_report() {
        let mut value: RootReport = serde_json::from_value(root()).unwrap();
        for pid in 1..=100 {
            value.processes.record(pid, ProcessSkipReason::Gone);
        }
        value.processes.record(101, ProcessSkipReason::Zombie);
        value.processes.record(102, ProcessSkipReason::Exiting);
        value.processes.kernel_threads = 20;
        value.error = Some(
            "process 103 protection is incomplete: live process has no inspectable arguments"
                .into(),
        );
        value.validate().unwrap();
        assert_eq!(value.processes.gone, 100);
        assert_eq!(value.processes.examples.len(), MAX_PROCESS_EXAMPLES);
        let bytes = serde_json::to_vec(&value).unwrap();
        let decoded: RootReport = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.processes, value.processes);
        assert_eq!(decoded.error, value.error);
        assert!(bytes.len() < 2048);
    }

    #[test]
    fn process_report_counts_examples_and_unknown_reasons_fail_closed() {
        let mut root = root();
        let mut observations = ProcessObservations::default();
        observations.record(42, ProcessSkipReason::Exiting);
        let normal = serde_json::to_value(&observations).unwrap();
        for value in [
            json!({"gone": 131073, "zombie": 0, "exiting": 0, "kernel_threads": 0, "examples": []}),
            json!({"gone": 0, "zombie": 0, "exiting": 0, "kernel_threads": 131073, "examples": []}),
            json!({"gone": 0, "zombie": 0, "exiting": 0, "kernel_threads": 0, "examples": [{"pid": 42, "reason": "gone"}]}),
            json!({"gone": 1, "zombie": 0, "exiting": 0, "kernel_threads": 0, "examples": [{"pid": 0, "reason": "gone"}]}),
            json!({"gone": 1, "zombie": 0, "exiting": 0, "kernel_threads": 0, "examples": [{"pid": 2147483648u32, "reason": "gone"}]}),
            json!({"gone": 1, "zombie": 0, "exiting": 0, "kernel_threads": 0, "examples": [{"pid": 42, "reason": "unknown"}]}),
        ] {
            root["processes"] = value;
            assert!(serde_json::from_value::<RootReport>(root.clone()).is_err());
        }
        root["processes"] = normal;
        serde_json::from_value::<RootReport>(root.clone()).unwrap();
        root["processes"]["exiting"] = json!(MAX_PROCESS_EXAMPLES + 1);
        root["processes"]["examples"] = json!(vec![
            ProcessSkip {
                pid: 42,
                reason: ProcessSkipReason::Exiting
            };
            MAX_PROCESS_EXAMPLES + 1
        ]);
        assert!(serde_json::from_value::<RootReport>(root).is_err());
    }

    fn root() -> Value {
        json!({
            "root": "/var/lib/lampOS/voice/sessions",
            "unit": "lamp-voice.service",
            "before_bytes": u64::MAX,
            "planned_bytes": u64::MAX,
            "removed_bytes": u64::MAX,
            "remaining_bytes": u64::MAX,
            "limit_bytes": u64::MAX,
            "active": [], "planned": [], "removed": [], "refused": [],
            "disk": {"total_bytes": u64::MAX, "available_bytes": u64::MAX},
            "error": null
        })
    }

    fn report() -> Value {
        json!({
            "schema_version": 1,
            "config_fingerprint": "0123456789abcdef".repeat(4),
            "at_unix_s": u64::MAX,
            "mode": "apply",
            "enabled": true,
            "roots": [root()],
            "error": null
        })
    }

    #[test]
    fn valid_report_round_trips_without_imposing_counter_relationships() {
        let original = report();
        let report: RunReport = serde_json::from_value(original.clone()).unwrap();
        report.validate().unwrap();
        assert_eq!(serde_json::to_value(&report).unwrap(), original);
        assert_eq!(report.roots[0].remaining_bytes, u64::MAX);
        let mut disabled = original;
        disabled["enabled"] = false.into();
        disabled["mode"] = "dry_run".into();
        disabled["roots"] = json!([]);
        disabled.as_object_mut().unwrap().remove("error");
        serde_json::from_value::<RunReport>(disabled)
            .unwrap()
            .validate()
            .unwrap();
    }

    #[test]
    fn fingerprint_is_required_and_exactly_lowercase_sha256_hex() {
        let mut value = report();
        value.as_object_mut().unwrap().remove("config_fingerprint");
        assert!(serde_json::from_value::<RunReport>(value).is_err());
        for digest in [
            String::new(),
            "a".repeat(63),
            "a".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
            "é".repeat(32),
            format!("{}\n", "a".repeat(63)),
        ] {
            let mut value = report();
            value["config_fingerprint"] = digest.clone().into();
            assert!(serde_json::from_value::<RunReport>(value).is_err());
            let mut in_memory: RunReport = serde_json::from_value(report()).unwrap();
            in_memory.config_fingerprint = digest;
            assert!(in_memory.validate().is_err());
        }
    }

    #[test]
    fn schema_mode_and_numeric_types_fail_closed() {
        for (field, bad) in [
            ("schema_version", json!(0)),
            ("schema_version", json!(2)),
            ("mode", json!("")),
            ("mode", json!("APPLY")),
            ("mode", json!("other")),
            ("at_unix_s", json!(-1)),
            ("at_unix_s", json!(0.5)),
        ] {
            let mut value = report();
            value[field] = bad;
            assert!(
                serde_json::from_value::<RunReport>(value).is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn next_release_fields_are_ignored_at_every_persisted_level_and_not_written() {
        let mut current = report();
        current["roots"][0]["processes"] = json!({
            "gone": 1, "zombie": 0, "exiting": 0, "kernel_threads": 0,
            "examples": [{"pid": 42, "reason": "gone"}]
        });
        let mut later = current.clone();
        for pointer in [
            "",
            "/roots/0",
            "/roots/0/disk",
            "/roots/0/processes",
            "/roots/0/processes/examples/0",
        ] {
            later.pointer_mut(pointer).unwrap()["later"] =
                json!({"kind": "future_detail", "values": [1, null, "new"]});
        }
        let decoded: RunReport = serde_json::from_value(later).unwrap();
        decoded.validate().unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), current);
    }

    #[test]
    fn added_fields_do_not_hide_unknown_enum_values_or_missing_required_fields() {
        for reason in [json!("later_reason"), json!({"kind": "later_reason"})] {
            let mut value = report();
            value["later"] = true.into();
            value["roots"][0]["processes"] = json!({
                "gone": 1, "zombie": 0, "exiting": 0, "kernel_threads": 0,
                "examples": [{"pid": 42, "reason": reason, "later": true}],
                "later": true
            });
            assert!(serde_json::from_value::<RunReport>(value).is_err());
        }
        for field in ["config_fingerprint", "mode", "enabled", "roots"] {
            let mut value = report();
            value["later"] = true.into();
            value.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<RunReport>(value).is_err());
        }
    }

    #[test]
    fn newer_schema_requires_the_bounded_report_envelope_but_does_not_decode_its_roots() {
        let mut later = report();
        later["schema_version"] = json!(2);
        later["mode"] = json!("later_mode");
        later["roots"][0]["processes"] = json!({"later_reason": true});
        let bytes = serde_json::to_vec(&later).unwrap();
        assert_eq!(newer_report_schema(&bytes).unwrap(), Some(2));
        assert!(serde_json::from_slice::<RunReport>(&bytes).is_err());
        for (field, value) in [
            ("schema_version", json!("2")),
            ("config_fingerprint", json!("not a fingerprint")),
            ("at_unix_s", json!(-1)),
            ("mode", json!(17)),
            ("mode", json!("m".repeat(129))),
            ("enabled", json!(null)),
            ("roots", json!(vec![json!({}); config::MAX_ROOTS + 1])),
        ] {
            let mut invalid = later.clone();
            invalid[field] = value;
            assert!(newer_report_schema(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
        assert!(newer_report_schema(br#"{"schema_version":2}"#).is_err());
        assert!(newer_report_schema(&bytes[..bytes.len() - 1]).is_err());
        later["schema_version"] = json!(u64::MAX);
        assert_eq!(
            newer_report_schema(&serde_json::to_vec(&later).unwrap()).unwrap(),
            Some(u64::MAX)
        );
        assert_eq!(
            newer_report_schema(&serde_json::to_vec(&report()).unwrap()).unwrap(),
            None
        );
    }

    #[test]
    fn root_and_all_record_arrays_have_exact_count_bounds_even_for_empty_strings() {
        let mut value = report();
        value["roots"] = Value::Array(vec![root(); config::MAX_ROOTS]);
        serde_json::from_value::<RunReport>(value.clone()).unwrap();
        value["roots"].as_array_mut().unwrap().push(root());
        assert!(serde_json::from_value::<RunReport>(value).is_err());

        for field in ["active", "planned", "removed", "refused"] {
            let mut value = root();
            value[field] = Value::Array(vec![Value::String(String::new()); MAX_REPORT_ITEMS]);
            let parsed: RootReport = serde_json::from_value(value.clone()).unwrap();
            parsed.validate().unwrap();
            value[field].as_array_mut().unwrap().push("".into());
            assert!(
                serde_json::from_value::<RootReport>(value).is_err(),
                "{field}"
            );
        }
    }

    struct Counted<'a> {
        input: &'a [u8],
        read: Rc<Cell<usize>>,
    }

    impl Read for Counted<'_> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let count = self.input.read(output)?;
            self.read.set(self.read.get() + count);
            Ok(count)
        }
    }

    #[test]
    fn overflow_stops_before_reading_or_allocating_the_extra_value_or_tail() {
        let prefix = format!("{{\"active\":[{}", "\"\",".repeat(MAX_REPORT_ITEMS));
        // The next value is both invalid and large. The cardinality error must
        // win before deserialization reads that value or the remaining array.
        let input = format!("{prefix}{{{}", "private invalid tail".repeat(100_000));
        let read = Rc::new(Cell::new(0));
        let result = serde_json::from_reader::<_, RootReport>(Counted {
            input: input.as_bytes(),
            read: Rc::clone(&read),
        });
        let error = result.unwrap_err();
        assert!(error.to_string().contains("too many report entries"));
        assert!(read.get() <= prefix.len() + 2, "read {} bytes", read.get());
        assert!(read.get() < input.len() / 10);

        let prefix = format!(
            "{{\"roots\":[{}",
            format!("{},", root()).repeat(config::MAX_ROOTS)
        );
        let input = format!("{prefix}{{{}", "invalid".repeat(10_000));
        let read = Rc::new(Cell::new(0));
        let error = serde_json::from_reader::<_, RunReport>(Counted {
            input: input.as_bytes(),
            read: Rc::clone(&read),
        })
        .unwrap_err();
        assert!(error.to_string().contains("too many report entries"));
        assert!(read.get() <= prefix.len() + 2);
    }

    #[test]
    fn record_strings_are_bounded_by_decoded_utf8_bytes() {
        for field in ["active", "planned", "removed", "refused"] {
            let mut value = root();
            value[field] = json!(["é".repeat(MAX_REPORT_ITEM_BYTES / 2)]);
            serde_json::from_value::<RootReport>(value.clone()).unwrap();
            value[field] = json!([format!("{}a", "é".repeat(MAX_REPORT_ITEM_BYTES / 2))]);
            assert!(serde_json::from_value::<RootReport>(value).is_err());
        }
        let input = format!(
            "{{\"refused\":[\"{}\"]}}",
            "\\u00e9".repeat(MAX_REPORT_ITEM_BYTES / 2 + 1)
        );
        assert!(
            serde_json::from_str::<RootReport>(&input)
                .unwrap_err()
                .to_string()
                .contains("byte limit")
        );
    }

    #[test]
    fn paths_units_and_error_strings_have_byte_limits() {
        for (field, fits, too_long) in [
            (
                "root",
                format!("/{}", "x".repeat(4_095)),
                format!("/{}", "x".repeat(4_096)),
            ),
            (
                "unit",
                format!("{}.service", "x".repeat(120)),
                format!("{}.service", "x".repeat(121)),
            ),
            (
                "error",
                "é".repeat(MAX_REPORT_ERROR_BYTES / 2),
                "é".repeat(MAX_REPORT_ERROR_BYTES / 2 + 1),
            ),
        ] {
            let mut value = root();
            value[field] = fits.into();
            serde_json::from_value::<RootReport>(value.clone())
                .unwrap()
                .validate()
                .unwrap();
            value[field] = too_long.into();
            assert!(
                serde_json::from_value::<RootReport>(value).is_err(),
                "{field}"
            );
        }
        for path in ["relative", "/a/../sessions", "/a\n/sessions"] {
            let mut value = root();
            value["root"] = path.into();
            assert!(serde_json::from_value::<RootReport>(value).is_err());
        }
        let mut value = report();
        value["error"] = "x".repeat(MAX_REPORT_ERROR_BYTES + 1).into();
        assert!(serde_json::from_value::<RunReport>(value).is_err());
    }

    #[test]
    fn values_built_in_memory_follow_the_same_resource_limits() {
        let mut value: RunReport = serde_json::from_value(report()).unwrap();
        value
            .roots
            .resize(config::MAX_ROOTS + 1, value.roots[0].clone());
        assert!(value.validate().is_err());
        value.roots.truncate(1);
        value.roots[0].refused = vec![String::new(); MAX_REPORT_ITEMS + 1];
        assert!(value.validate().is_err());
        value.roots[0].refused = vec!["x".repeat(MAX_REPORT_ITEM_BYTES + 1)];
        assert!(value.validate().is_err());
        value.roots[0].refused.clear();
        value.roots[0].error = Some("x".repeat(MAX_REPORT_ERROR_BYTES + 1));
        assert!(value.validate().is_err());
    }
}
