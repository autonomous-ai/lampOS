mod artifacts;
mod host;
mod inventory;

use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, String>;
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
static SERIAL: Mutex<()> = Mutex::new(());
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub enum CaseKind {
    OldReadsCurrent,
    CurrentReadsOld,
    SharedRefusal,
}
impl CaseKind {
    fn name(self) -> &'static str {
        match self {
            Self::OldReadsCurrent => {
                "records_6408_prunes_6449_private_sessions_and_preserves_live_invocation"
            }
            Self::CurrentReadsOld => {
                "records_6449_prunes_6408_private_sessions_and_preserves_live_invocation"
            }
            Self::SharedRefusal => "records_6408_refuses_6449_shared_layout_and_6449_can_resume",
        }
    }
    fn tag(self) -> &'static str {
        match self {
            Self::OldReadsCurrent => "old",
            Self::CurrentReadsOld => "current",
            Self::SharedRefusal => "shared",
        }
    }
}
struct Case {
    base: PathBuf,
    output: PathBuf,
    case: CaseKind,
    number: u64,
    pair: artifacts::Pair,
    units: Vec<host::Unit>,
    log: Vec<Value>,
    observations: Vec<Value>,
    until: Instant,
    created: bool,
    cleaned: bool,
    preflight_verified: bool,
}
fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(error)?;
    if bytes.len() > 1024 * 1024 {
        return Err("fixture JSON evidence exceeds 1 MiB bound".into());
    }
    inventory::private_file(path, &bytes)
}
fn append_unit(output: &Path, name: &str) -> Result<()> {
    let path = output.join("owned-units.json");
    let mut units: Vec<String> = match inventory::read(&path, 16384) {
        Ok(bytes) => {
            let v: Value = serde_json::from_slice(&bytes).map_err(error)?;
            if v["version"] != 1 {
                return Err("wrong ownership ledger version".into());
            }
            serde_json::from_value(v["units"].clone()).map_err(error)?
        }
        Err(_)
            if fs::symlink_metadata(&path)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            Vec::new()
        }
        Err(reason) => return Err(reason),
    };
    let prefix = format!("lamp-records-pair-{}-", std::process::id());
    if units.len() >= 24
        || units
            .iter()
            .any(|s| !s.starts_with(&prefix) || !s.ends_with(".service"))
        || units.iter().any(|s| s == name)
    {
        return Err("unexpected/non-owned ownership ledger".into());
    }
    units.push(name.to_owned());
    let temporary = output.join("owned-units.pending.json");
    write_json(&temporary, &json!({"version":1,"units":units}))?;
    fs::rename(&temporary, &path).map_err(error)?;
    fs::File::open(output)
        .map_err(error)?
        .sync_all()
        .map_err(error)
}
impl Case {
    fn new(case: CaseKind, output: PathBuf, pair: artifacts::Pair) -> Self {
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        Self {
            base: Path::new("/run").join(format!(
                "lamp-records-release-pair-{}-{}",
                std::process::id(),
                number
            )),
            output,
            case,
            number,
            pair,
            units: Vec::new(),
            log: Vec::new(),
            observations: Vec::new(),
            until: Instant::now() + Duration::from_secs(150),
            created: false,
            cleaned: false,
            preflight_verified: false,
        }
    }
    fn initialize(&mut self) -> Result<()> {
        host::preflight(&mut self.log)?;
        self.preflight_verified = true;
        inventory::private_dir(&self.base, 0o700)?;
        self.created = true;
        for name in ["bin", "control", "bus", "voice"] {
            inventory::private_dir(&self.base.join(name), 0o700)?;
        }
        let shared = matches!(self.case, CaseKind::SharedRefusal);
        if shared {
            fs::set_permissions(self.base.join("voice"), fs::Permissions::from_mode(0o710))
                .map_err(error)?;
        }
        inventory::private_dir(&self.root(), if shared { 0o750 } else { 0o700 })?;
        // A real local group, no account creation or NSS. Both writer/reader
        // must independently validate this existing root/GID0 entry.
        let groups = inventory::read(Path::new("/etc/group"), 1024 * 1024)?;
        let root_entries = std::str::from_utf8(&groups)
            .map_err(error)?
            .lines()
            .filter(|l| l.starts_with("root:"))
            .collect::<Vec<_>>();
        if root_entries.len() != 1 || root_entries[0].split(':').nth(2) != Some("0") {
            return Err("root local group is absent/ambiguous".into());
        }
        inventory::private_file(&self.base.join("control/provider.json"),br#"{"endpoint":"wss://127.0.0.1:9","model":"fixture","voice":"Aoede","credential_file":"/nonexistent-unused-fixture-key"}"#)?;
        inventory::private_file(
            &self.base.join("control/mute.json"),
            br#"{"version":1,"clock_id":"records-pair-native","muted":true,"through_us":0}"#,
        )?;
        Ok(())
    }
    fn root(&self) -> PathBuf {
        self.base.join("voice/sessions")
    }
    fn writer(&self) -> artifacts::Artifact {
        match self.case {
            CaseKind::CurrentReadsOld => self.pair.old_voice.clone(),
            _ => self.pair.current_voice.clone(),
        }
    }
    fn produce(&mut self) -> Result<PathBuf> {
        self.time_left()?;
        let artifact = self.writer();
        let bin = self.base.join("bin/lamp-voice");
        if !bin.exists() {
            fs::copy(&artifact.path, &bin).map_err(error)?;
            fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).map_err(error)?;
        }
        if artifacts::digest(&bin)? != artifact.sha256 {
            return Err("copied actual Voice artifact changed".into());
        }
        let mut unit = host::Unit::new(format!(
            "lamp-records-pair-{}-{}-{}.service",
            std::process::id(),
            self.case.tag(),
            self.units.len()
        ));
        unit.reserve(&mut self.log)?;
        // Persist ownership BEFORE systemd-run can create anything. Always
        // cleanup outside the test may use only these exact reserved names.
        append_unit(&self.output, &unit.name)?;
        self.units.push(unit);
        let index = self.units.len() - 1;
        let root = self.root();
        let config = self.base.join("control/provider.json");
        let mute = self.base.join("control/mute.json");
        let bus = self.base.join("bus");
        let session = self.units[index].start(
            host::Producer {
                binary: &bin,
                config: &config,
                root: &root,
                mute: &mute,
                bus: &bus,
                shared: matches!(self.case, CaseKind::SharedRefusal),
            },
            &mut self.log,
        )?;
        self.observations.push(json!({"producer_release":artifact.release,"artifact":artifact,"unit":self.units[index].name,
            "invocation":self.units[index].invocation,"metadata_ready":true,"muted":true,"hardware_or_provider_worker":false}));
        Ok(session)
    }
    fn time_left(&self) -> Result<Duration> {
        self.until
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| "case original 150 s work ceiling reached".into())
    }
    fn config(&self, unit: &str) -> Result<PathBuf> {
        let path = self.base.join("control/records.json");
        let mut root = json!({"kind":"voice_service_metadata","path":self.root(),"unit":unit,"max_bytes":1,"max_age_seconds":86400});
        if matches!(self.case, CaseKind::SharedRefusal) {
            root["session_read_group"] = "root".into();
        }
        write_json(
            &path,
            &json!({"schema_version":1,"enabled":true,"bus_root":self.base.join("bus"),"roots":[root]}),
        )?;
        Ok(path)
    }
    fn records(
        &mut self,
        artifact: &artifacts::Artifact,
        config: &Path,
        apply: bool,
        label: &str,
    ) -> Result<Value> {
        let mut args = vec!["check".into(), config.display().to_string()];
        let result = self.base.join(format!("control/{label}.json"));
        if apply {
            args.extend([
                "--apply".into(),
                "--report".into(),
                result.display().to_string(),
            ]);
        }
        let command = host::command(
            &artifact.path,
            &args,
            self.time_left()?.min(Duration::from_secs(60)),
            &mut self.log,
        )?;
        let report: Value = serde_json::from_slice(&command.stdout)
            .map_err(|_| "actual Records did not emit a complete JSON report".to_owned())?;
        if report["schema_version"] != 1
            || report["enabled"] != true
            || report["roots"].as_array().map(Vec::len) != Some(1)
        {
            return Err("actual Records report shape is not accepted release schema1".into());
        }
        let expected_exit = if report["error"].is_null() && report["roots"][0]["error"].is_null() {
            1
        } else {
            2
        };
        if command.code != Some(expected_exit) {
            return Err(
                "actual Records exit disagrees with its expected warning/refusal report".into(),
            );
        }
        if apply {
            let stored: Value =
                serde_json::from_slice(&inventory::read(&result, 1024 * 1024)?).map_err(error)?;
            if stored != report {
                return Err("persisted actual report disagrees with stdout".into());
            }
        }
        self.observations.push(
            json!({"label":label,"reader_release":artifact.release,"reader_sha256":artifact.sha256,
            "exit":command.code,"actual_report":report}),
        );
        Ok(report)
    }
    fn negatives(&self) -> Result<(PathBuf, inventory::Snapshot)> {
        let outside = self.base.join("outside");
        inventory::private_dir(&outside, 0o700)?;
        inventory::private_file(
            &outside.join("sentinel"),
            b"synthetic-outside-must-remain\n",
        )?;
        let unknown = self.root().join("fffffffffffffffffffffffffffffffe");
        inventory::private_dir(&unknown, 0o700)?;
        inventory::private_file(&unknown.join("unexpected"), b"unknown-layout\n")?;
        symlink(
            &outside,
            self.root().join("fffffffffffffffffffffffffffffffd"),
        )
        .map_err(error)?;
        Ok((outside.clone(), inventory::snapshot(&outside)?))
    }
    fn exercise(&mut self) -> Result<()> {
        self.initialize()?;
        let closed = self.produce()?;
        self.units[0].stop(&mut self.log)?;
        let shared = matches!(self.case, CaseKind::SharedRefusal);
        let (closed_bytes, closed_proof) = inventory::session(&closed, shared, true)?;
        let live = self.produce()?;
        let (_, live_proof) = inventory::session(&live, shared, false)?;
        let closed_id = closed
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("bad actual closed ID")?;
        let live_id = live
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("bad actual live ID")?;
        if closed_id == live_id || !host::valid_id(closed_id) || !host::valid_id(live_id) {
            return Err("actual systemd invocation IDs were not distinct".into());
        }
        self.observations.push(json!({"closed_producer_inventory":closed_proof,"active_producer_inventory":live_proof}));
        let (outside, outside_before) = self.negatives()?;
        let config = self.config(&self.units[1].name)?;
        let root_before = inventory::snapshot(&self.root())?;
        if shared {
            let old = self.pair.old_records.clone();
            let refused = self.records(&old, &config, true, "old-shared-refusal")?;
            let r = &refused["roots"][0];
            if r["error"].as_str().is_none_or(str::is_empty)
                || r["removed_bytes"] != 0
                || !array(r, "planned")?.is_empty()
                || !array(r, "removed")?.is_empty()
            {
                return Err("6408 did not truthfully refuse shared root without pruning".into());
            }
            // Root0750 refusal happens before its proc guard. This negative
            // case alone is not evidence that systemd ownership was checked.
            inventory::unchanged_except_live(
                &root_before,
                &inventory::snapshot(&self.root())?,
                live_id,
            )?;
        }
        let reader = match self.case {
            CaseKind::OldReadsCurrent => self.pair.old_records.clone(),
            _ => self.pair.current_records.clone(),
        };
        let dry = self.records(&reader, &config, false, "dry")?;
        success(&dry, closed_id, live_id, closed_bytes, false)?;
        inventory::unchanged_except_live(
            &root_before,
            &inventory::snapshot(&self.root())?,
            live_id,
        )?;
        let applied = self.records(&reader, &config, true, "applied")?;
        success(&applied, closed_id, live_id, closed_bytes, true)?;
        if closed.exists() || !live.exists() {
            return Err("actual deletion/live protection does not match report".into());
        }
        if inventory::snapshot(&outside)? != outside_before {
            return Err("outside link target was changed".into());
        }
        let after = inventory::snapshot(&self.root())?;
        inventory::live_objects_preserved(&root_before, &after, live_id)?;
        for (name, node) in &root_before {
            if name.starts_with("ffffffffffffffffffffffffffffff") && after.get(name) != Some(node) {
                return Err("unknown/linked negative was changed".into());
            }
        }
        if after.keys().any(|n| n.starts_with(".lamp-records-trash/")) {
            return Err("successful apply left unexpected quarantine content".into());
        }
        self.units[1].remains_live(&mut self.log)?;
        let _ = inventory::session(&live, shared, false)?;
        let muted: Value = serde_json::from_slice(&inventory::read(
            &self.base.join("control/mute.json"),
            4096,
        )?)
        .map_err(error)?;
        if muted["muted"] != true {
            return Err("actual software mute changed during cleanup fixture".into());
        }
        self.pair.verify()?;
        self.observations.push(json!({"private_or_current_shared_cleanup":true,"closed_removed":1,"live_protected":1,
            "outside_unchanged":true,"unknown_and_symlink_unchanged":true,"removed_allocated_bytes":closed_bytes,
            "shared_rollback_retention_supported":!shared,"no_full_rotation_or_hardware_acceptance":true}));
        Ok(())
    }
    fn cleanup(&mut self) -> Result<()> {
        let mut errors = Vec::new();
        for unit in self.units.iter_mut().rev() {
            if let Err(reason) = unit.stop(&mut self.log) {
                errors.push(reason);
            }
        }
        if !host::commands_cleaned(&self.log) {
            errors.push("owned command process-group cleanup is unconfirmed".into());
        }
        if !errors.is_empty() {
            return Err(errors.join("; "));
        }
        if self.created {
            fs::remove_dir_all(&self.base).map_err(error)?;
            self.created = false;
        }
        self.cleaned = true;
        Ok(())
    }
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    value[key]
        .as_array()
        .ok_or_else(|| format!("missing actual report array {key}"))
}
fn success(report: &Value, closed: &str, live: &str, bytes: u64, apply: bool) -> Result<()> {
    let r = &report["roots"][0];
    if !report["error"].is_null() || !r["error"].is_null() {
        return Err("actual Records refused; not successful cleanup evidence".into());
    }
    if !array(r, "active")?.contains(&json!(live))
        || array(r, "planned")? != &vec![json!(closed)]
        || r["planned_bytes"] != bytes
    {
        return Err("actual fixed inventory/live plan differs".into());
    }
    let expected = if apply { vec![json!(closed)] } else { vec![] };
    if array(r, "removed")? != &expected || r["removed_bytes"] != if apply { bytes } else { 0 } {
        return Err("actual removal/accounting differs".into());
    }
    let before = r["before_bytes"].as_u64().ok_or("missing before bytes")?;
    let remaining = r["remaining_bytes"]
        .as_u64()
        .ok_or("missing remaining bytes")?;
    if remaining
        != before
            .checked_sub(if apply { bytes } else { 0 })
            .ok_or("bad report subtraction")?
    {
        return Err("actual remaining byte accounting differs".into());
    }
    Ok(())
}
impl Drop for Case {
    fn drop(&mut self) {
        if !self.cleaned
            && let Err(reason) = self.cleanup()
        {
            eprintln!("release-pair emergency cleanup failed: {reason}");
        }
    }
}

pub fn run(kind: CaseKind) {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let output = inventory::evidence_root().expect("explicit native fixture evidence root");
    let pair = match artifacts::Pair::load() {
        Ok(pair) => pair,
        Err(reason) => {
            let receipt = json!({"version":1,"test":kind.name(),"result":"failed",
                "error":reason,"phase":"artifact_preflight","cleanup_confirmed":true,"units_started":0});
            write_json(&output.join(format!("{}.json", kind.name())), &receipt)
                .expect("artifact failure receipt");
            println!(
                "release_pair_case {}",
                json!({"test":kind.name(),"result":"failed","cleanup_confirmed":true})
            );
            panic!("actual release artifact preflight failed: {reason}");
        }
    };
    let mut case = Case::new(kind, output.clone(), pair);
    let observed = catch_unwind(AssertUnwindSafe(|| case.exercise()));
    let outcome = match observed {
        Ok(v) => v,
        Err(_) => Err("fixture panicked; preserved cleanup/evidence".into()),
    };
    if outcome.is_err() {
        for unit in &case.units {
            let _ = host::command(
                Path::new("/usr/bin/journalctl"),
                &[
                    "--no-pager".into(),
                    "--output=short-monotonic".into(),
                    "--lines=40".into(),
                    "--unit".into(),
                    unit.name.clone(),
                ],
                host::QUERY,
                &mut case.log,
            );
        }
    }
    let cleanup = case.cleanup();
    let passed = outcome.is_ok() && cleanup.is_ok();
    let receipt = json!({"version":1,"test":kind.name(),"result":if passed{"passed"}else{"failed"},
        "expected_pair":[6408,6449],"artifacts":[case.pair.old_records,case.pair.current_records,case.pair.old_voice,case.pair.current_voice],
        "architecture":std::env::consts::ARCH,"native_root_systemd":case.preflight_verified,
        "error":outcome.as_ref().err(),"cleanup_error":cleanup.as_ref().err(),"cleanup_confirmed":cleanup.is_ok(),
        "case_number":case.number,"observations":case.observations,"commands":case.log,
        "command_log_is_bounded_prefixes":true,"command_log_may_be_compacted":case.log.len()==128,
        "no_guard_override":true,"signature_trust_verified":false,"full_rotation_or_device_acceptance":false});
    let receipt_path = output.join(format!("{}.json", kind.name()));
    write_json(&receipt_path, &receipt).expect("persist bounded terminal case receipt");
    println!(
        "release_pair_case {}",
        json!({"test":kind.name(),"result":if passed{"passed"}else{"failed"},"cleanup_confirmed":cleanup.is_ok()})
    );
    assert!(
        passed,
        "actual release pair failed: {:?}; cleanup {:?}",
        outcome.err(),
        cleanup.err()
    );
}
