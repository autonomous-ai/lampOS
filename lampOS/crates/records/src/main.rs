include!("application.rs");

struct SystemSchedule;

impl ReportSchedule for SystemSchedule {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn wait(&mut self, _cycle: ReportCycle, remaining: Duration) -> io::Result<()> {
        std::thread::sleep(remaining);
        Ok(())
    }
}

fn reporting(config: PathBuf, result: PathBuf, seconds: Option<u64>) -> io::Result<()> {
    reporting_with(config, result, seconds, &mut SystemSchedule, Reporter::new)
}

fn command(command: Command) -> io::Result<u8> {
    command_with_reporting(command, reporting)
}

fn main() -> ExitCode {
    entrypoint(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_records::json_file::REPORT_BYTES;
    fn args(value: &str) -> io::Result<Command> {
        arguments(
            &value
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        )
    }
    #[test]
    fn check_defaults_to_dry_and_never_accepts_a_live_override() {
        assert_eq!(
            args("check /etc/lampOS/records.json").unwrap(),
            Command::Check {
                config: "/etc/lampOS/records.json".into(),
                apply: false,
                report: None,
            }
        );
        for value in [
            "check /a --live none",
            "check /a --ignore-live",
            "check /a --report /r",
            "check /a --apply --apply",
            "check relative",
            "check /a --apply --report",
        ] {
            assert!(args(value).is_err(), "{value}");
        }
        assert!(args("check /a --apply --report /run/records/last.json").is_ok());
    }
    #[test]
    fn reporter_has_explicit_bounded_test_lifetime() {
        assert!(args("report /config /last --seconds 1").is_ok());
        for value in [
            "report /config /last --seconds 0",
            "report /config /last --seconds 86401",
            "report /config /last extra",
        ] {
            assert!(args(value).is_err());
        }
    }

    #[test]
    fn output_cannot_overwrite_config_or_an_unrelated_file() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let dir = fs::canonicalize(env::temp_dir())
            .unwrap()
            .join(format!("lamp-records-cli-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let config_path = dir.join("records.json");
        let bytes = serde_json::to_vec(&Config::customer_default()).unwrap();
        fs::write(&config_path, &bytes).unwrap();
        let result = command(Command::Check {
            config: config_path.clone(),
            apply: true,
            report: Some(config_path.clone()),
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&config_path).unwrap(), bytes);
        let unrelated = dir.join("unrelated.json");
        fs::write(&unrelated, b"{}").unwrap();
        assert!(
            command(Command::Check {
                config: config_path,
                apply: true,
                report: Some(unrelated.clone())
            })
            .is_err()
        );
        assert_eq!(fs::read(unrelated).unwrap(), b"{}");
        let reserved_config = dir.join(".last.json.lamp-records-tmp");
        fs::write(&reserved_config, &bytes).unwrap();
        fs::set_permissions(&reserved_config, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(
            command(Command::Check {
                config: reserved_config.clone(),
                apply: true,
                report: Some(dir.join("last.json"))
            })
            .is_err()
        );
        assert_eq!(fs::read(reserved_config).unwrap(), bytes);
        assert!(!dir.join("last.json").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn old_result_does_not_match_a_changed_age_policy_or_enable_switch() {
        let config = Config::customer_default();
        let result = synthetic(&config, 10, None);
        assert_eq!(result.config_fingerprint, config.fingerprint());
        let mut changed = config.clone();
        changed.enabled = true;
        assert_ne!(result.config_fingerprint, changed.fingerprint());
        changed = config;
        changed.roots[0].max_age_seconds += 1;
        assert_ne!(result.config_fingerprint, changed.fingerprint());
    }

    #[test]
    fn oversized_result_leaves_readable_failure_and_next_cleanup_can_replace_it() {
        use lamp_records::{Disk, RootReport};
        use std::{fs, os::unix::fs::PermissionsExt};
        let dir = fs::canonicalize(env::temp_dir())
            .unwrap()
            .join(format!("lamp-records-receipt-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join("last.json");
        let config = Config::customer_default();
        let mut result = synthetic(&config, 10, None);
        result.roots.push(RootReport {
            root: config.roots[0].path.clone(),
            unit: config.roots[0].unit.clone(),
            before_bytes: 0,
            planned_bytes: 0,
            removed_bytes: 0,
            remaining_bytes: 0,
            limit_bytes: config.roots[0].max_bytes,
            active: vec![],
            planned: vec![],
            removed: vec![],
            refused: vec!["x".repeat(1024); 9000],
            processes: lamp_records::ProcessObservations::default(),
            disk: Some(Disk {
                total_bytes: 100,
                available_bytes: 100,
            }),
            error: None,
        });
        result.validate().unwrap();
        persist(&config, &path, &mut result);
        assert!(result.error.is_some());
        assert_eq!(result.roots[0].refused.len(), 9000);
        let failure: RunReport = json_file::read(&path, REPORT_BYTES).unwrap();
        assert!(failure.error.is_some());
        assert!(failure.roots.is_empty());
        assert_eq!(failure.config_fingerprint, config.fingerprint());
        assert_eq!(
            lamp_records::report::diagnostic(Some(&failure), 10).level,
            lamp_msgs::Level::Error
        );
        let config_path = dir.join("records.json");
        json_file::write(&config_path, &config).unwrap();
        assert_eq!(
            command(Command::Check {
                config: config_path,
                apply: true,
                report: Some(path.clone()),
            })
            .unwrap(),
            0
        );
        let recovered: RunReport = json_file::read(&path, REPORT_BYTES).unwrap();
        assert!(recovered.error.is_none());
        assert!(!recovered.enabled);
        assert!(!json_file::temporary_path(&path).unwrap().exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failure_receipt_cannot_overwrite_an_unknown_supported_value() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let dir = fs::canonicalize(env::temp_dir())
            .unwrap()
            .join(format!("lamp-records-unrecognized-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join("last.json");
        let config = Config::customer_default();
        let mut result = synthetic(&config, 10, None);
        let mut unknown = serde_json::to_value(&result).unwrap();
        // Not a plain word, so not a newer build's mode, which is set aside
        // instead (#1680): a value this build can't take.
        unknown["mode"] = "Later Mode!".into();
        let bytes = serde_json::to_vec(&unknown).unwrap();
        fs::write(&path, &bytes).unwrap();
        persist(&config, &path, &mut result);
        assert!(result.error.is_some());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    /// The privacy case of #1505: with cleanup on, a newer build's report at
    /// the result path is set aside and one `check --apply` still removes a
    /// session past `max_age_seconds`. Only the live set is injected; the
    /// real guard needs Linux systemd and the voice's unit, so the binary
    /// cannot remove a session in a test
    /// (`real_cli_apply_with_unknown_service_cannot_delete_any_fixture_record`).
    #[test]
    fn cleanup_on_sets_aside_a_newer_report_and_still_removes_an_expired_session() {
        use std::{
            collections::BTreeSet,
            fs::{self, File, FileTimes, OpenOptions},
            io::Write,
            os::unix::fs::{OpenOptionsExt, PermissionsExt},
        };
        if super::test_openat2::store_cannot_run_here() {
            return;
        }
        fn private_dir(path: &Path) {
            fs::create_dir(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let dir = fs::canonicalize(env::temp_dir())
            .unwrap()
            .join(format!("lamp-records-newer-cleanup-{}", std::process::id()));
        private_dir(&dir);
        let mut config = Config::customer_default();
        config.enabled = true;
        config.roots[0].path = dir.join("sessions");
        config.bus_root = dir.join("bus");
        private_dir(&config.roots[0].path);
        // Last written 100 s after the epoch: far past the age limit.
        let id = format!("{:032x}", 1);
        let session = config.roots[0].path.join(&id);
        private_dir(&session);
        private_dir(&session.join("metadata"));
        let old = FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(100));
        for name in ["events-0.jsonl", "health.json"] {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(session.join("metadata").join(name))
                .unwrap();
            file.write_all(b"recorded").unwrap();
            file.set_times(old).unwrap();
        }
        for path in [session.join("metadata"), session.clone()] {
            File::open(path).unwrap().set_times(old).unwrap();
        }
        let config_path = dir.join("records.json");
        json_file::write(&config_path, &config).unwrap();
        let report_path = dir.join("last.json");
        let mut newer = serde_json::to_value(synthetic(&config, 17, None)).unwrap();
        newer["schema_version"] = 2.into();
        newer["mode"] = "later_mode".into();
        newer["roots"] = serde_json::json!([{"later_kind": "new_enum_value"}]);
        let bytes = serde_json::to_vec_pretty(&newer).unwrap();
        fs::write(&report_path, &bytes).unwrap();
        let code = check_with(
            config_path,
            true,
            Some(report_path.clone()),
            |config, now, apply| run::execute_with(config, now, apply, |_, _| Ok(BTreeSet::new())),
        )
        .unwrap();
        assert_eq!(code, 0);
        assert!(!session.exists(), "the expired session must be removed");
        let archives: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(Result::unwrap)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("last.json.newer-2-")
            })
            .collect();
        assert_eq!(archives.len(), 1);
        assert_eq!(fs::read(archives[0].path()).unwrap(), bytes);
        let receipt: RunReport = json_file::read(&report_path, REPORT_BYTES).unwrap();
        assert!(receipt.enabled);
        assert_eq!(receipt.config_fingerprint, config.fingerprint());
        assert_eq!(receipt.roots[0].removed, vec![id]);
        fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
#[path = "../tests/support/openat2.rs"]
mod test_openat2;

#[cfg(test)]
mod application_schedule_tests;

#[cfg(test)]
mod fixture_protocol;
#[cfg(test)]
mod fixture_schedule;
#[cfg(test)]
mod fixture_schedule_tests;
