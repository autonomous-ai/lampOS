//! Inventory, protect, plan, then apply to that inventory only.
use crate::{
    Disk, REPORT_SCHEMA_VERSION, RootReport, RunReport,
    config::{Config, RootConfig},
    guard, plan,
    store::{Inventory, Removal, Store},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    time::{Duration, Instant},
};

/// Hard work limits still apply when callers inject the filesystem or clock.
pub const RUN_BUDGET: Duration = Duration::from_secs(45);

pub trait Records {
    fn inventory(&mut self) -> io::Result<Inventory>;
    fn remove(&mut self, id: &str) -> io::Result<Removal>;
    fn disk(&self) -> io::Result<Disk>;
}

impl Records for Store {
    fn inventory(&mut self) -> io::Result<Inventory> {
        Store::inventory(self)
    }
    fn remove(&mut self, id: &str) -> io::Result<Removal> {
        Store::remove(self, id)
    }
    fn disk(&self) -> io::Result<Disk> {
        Store::disk(self)
    }
}

fn empty_root(config: &RootConfig) -> RootReport {
    RootReport {
        root: config.path.clone(),
        unit: config.unit.clone(),
        before_bytes: 0,
        planned_bytes: 0,
        removed_bytes: 0,
        remaining_bytes: 0,
        limit_bytes: config.max_bytes,
        active: vec![],
        planned: vec![],
        removed: vec![],
        refused: vec![],
        processes: crate::ProcessObservations::default(),
        disk: None,
        error: None,
    }
}

fn detail(error: impl std::fmt::Display) -> String {
    let mut text = error.to_string();
    let mut end = text.len().min(512);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

/// `protect` must establish complete live-writer evidence after inventory.
/// An error protects the whole root. The CLI supplies only the Linux guard;
/// injection exists for disposable-tree tests and does not expose an override.
pub fn root_with(
    config: &RootConfig,
    now_unix_s: u64,
    apply: bool,
    store: &mut impl Records,
    mut protect: impl FnMut() -> io::Result<BTreeSet<String>>,
    deadline: Instant,
) -> RootReport {
    let mut report = empty_root(config);
    let result = (|| -> io::Result<()> {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "run budget exhausted before inventory",
            ));
        }
        let inventory = store.inventory()?;
        report.refused = inventory.refused.into_iter().map(detail).collect();
        report.before_bytes = inventory.sessions.iter().try_fold(0_u64, |sum, session| {
            sum.checked_add(session.bytes)
                .ok_or_else(|| io::Error::other("record size overflow"))
        })?;
        report.remaining_bytes = report.before_bytes;
        report.disk = Some(store.disk()?);
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "run budget exhausted after inventory",
            ));
        }
        // Do not refresh inventory after this point. Voice exclusively creates
        // NEW_OUTPUT, so later starts cannot acquire an existing candidate.
        let live = protect()?;
        if live.len() > crate::MAX_REPORT_ITEMS {
            return Err(io::Error::other(
                "live protection set exceeds the report bound",
            ));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "run budget exhausted during live protection",
            ));
        }
        report.active = live.iter().cloned().collect();
        let plan = plan::make(
            &inventory.sessions,
            &live,
            now_unix_s,
            config.max_bytes,
            config.max_age_seconds,
        )?;
        report.planned_bytes = plan.remove_bytes;
        report.planned = plan.remove;
        report.refused.extend(plan.refused);
        if !apply {
            return Ok(());
        }
        let initial_bytes: BTreeMap<_, _> = inventory
            .sessions
            .iter()
            .map(|item| (&item.id, item.bytes))
            .collect();
        for id in &report.planned {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "run budget exhausted; remaining plan preserved",
                ));
            }
            match store.remove(id) {
                Ok(removed) => {
                    report.removed_bytes = report
                        .removed_bytes
                        .checked_add(removed.removed_bytes)
                        .ok_or_else(|| io::Error::other("removed byte count overflow"))?;
                    if removed.complete {
                        report.removed.push(id.clone());
                        report.remaining_bytes = report
                            .remaining_bytes
                            .saturating_sub(initial_bytes.get(id).copied().unwrap_or(0));
                    } else {
                        report.remaining_bytes =
                            report.remaining_bytes.saturating_sub(removed.removed_bytes);
                    }
                    if let Some(error) = removed.error {
                        report
                            .refused
                            .push(format!("{id}: partial removal: {}", detail(error)));
                    } else if !removed.complete {
                        report.refused.push(format!("{id}: incomplete removal"));
                    }
                }
                Err(error) => {
                    report.refused.push(format!("{id}: {}", detail(error)));
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        report.error = Some(detail(error));
    }
    // A full filesystem is reported even when live protection or cleanup
    // failed. This is measured capacity, not inferred from unlinked blocks.
    match store.disk() {
        Ok(disk) => report.disk = Some(disk),
        Err(error) => {
            report.disk = None;
            if report.error.is_none() {
                report.error = Some(detail(error));
            }
        }
    }
    report
}

pub fn execute(config: &Config, now_unix_s: u64, apply: bool) -> RunReport {
    execute_with(config, now_unix_s, apply, |root, observations| {
        guard::live_sessions(&root.unit, &root.path, observations)
    })
}

/// `execute` with live protection injected, for disposable-tree tests only,
/// as with `root_with`. The CLI calls `execute`, never this.
pub fn execute_with(
    config: &Config,
    now_unix_s: u64,
    apply: bool,
    mut protect: impl FnMut(
        &RootConfig,
        &mut crate::ProcessObservations,
    ) -> io::Result<BTreeSet<String>>,
) -> RunReport {
    let mut report = RunReport {
        schema_version: REPORT_SCHEMA_VERSION,
        config_fingerprint: config.fingerprint(),
        at_unix_s: now_unix_s,
        mode: if apply { "apply" } else { "dry_run" }.into(),
        enabled: config.enabled,
        roots: vec![],
        error: None,
    };
    if let Err(error) = config.validate() {
        report.error = Some(detail(error));
        return report;
    }
    // Installing a unit or accidentally passing --apply cannot enable it.
    if apply && !config.enabled {
        return report;
    }
    let deadline = Instant::now() + RUN_BUDGET;
    for root in &config.roots {
        if Instant::now() >= deadline {
            let mut value = empty_root(root);
            value.error = Some("run budget exhausted before opening root".into());
            report.roots.push(value);
            continue;
        }
        let mut observations = crate::ProcessObservations::default();
        let mut result = match Store::open(&root.path) {
            Ok(mut store) => root_with(
                root,
                now_unix_s,
                apply,
                &mut store,
                || protect(root, &mut observations),
                deadline,
            ),
            Err(error) => {
                let mut value = empty_root(root);
                value.error = Some(detail(error));
                value
            }
        };
        // Retain observed exits even if a later live process refuses the root.
        result.processes = observations;
        report.roots.push(result);
    }
    report
}

pub fn exit_code(report: &RunReport) -> u8 {
    if report.error.is_some() || report.roots.iter().any(|root| root.error.is_some()) {
        return 2;
    }
    if report.roots.iter().any(|root| {
        !root.refused.is_empty()
            || (report.mode == "apply" && root.remaining_bytes > root.limit_bytes)
            || root
                .disk
                .as_ref()
                .is_none_or(|disk| disk.total_bytes == 0 || disk.available_bytes == 0)
    }) {
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Session;
    use std::{cell::Cell, rc::Rc};

    struct Fake {
        entries: Vec<Session>,
        calls: Rc<Cell<usize>>,
        removed: Vec<String>,
        full: bool,
        failure: bool,
        partial: bool,
    }
    impl Records for Fake {
        fn inventory(&mut self) -> io::Result<Inventory> {
            assert_eq!(
                self.calls.replace(1),
                0,
                "inventory must happen exactly once before guard"
            );
            Ok(Inventory {
                sessions: self.entries.clone(),
                refused: vec![],
            })
        }
        fn disk(&self) -> io::Result<Disk> {
            Ok(Disk {
                total_bytes: 100,
                available_bytes: if self.full { 0 } else { 50 },
            })
        }
        fn remove(&mut self, id: &str) -> io::Result<Removal> {
            assert_eq!(self.calls.get(), 2);
            if self.failure {
                return Err(io::Error::from_raw_os_error(28));
            }
            self.removed.push(id.into());
            Ok(Removal {
                removed_bytes: if self.partial { 2 } else { 10 },
                removed_files: 1,
                complete: !self.partial,
                error: self.partial.then(|| "injected write failure".into()),
            })
        }
    }
    fn fixture() -> (RootConfig, Fake) {
        let entries = (1..=3)
            .map(|n| Session {
                id: format!("{n:032x}"),
                bytes: 10,
                newest_unix_s: n,
                pending: false,
            })
            .collect();
        (
            Config::customer_default().roots.remove(0),
            Fake {
                entries,
                calls: Rc::new(Cell::new(0)),
                removed: vec![],
                full: false,
                failure: false,
                partial: false,
            },
        )
    }
    #[test]
    fn dry_run_plans_but_never_mutates_and_protects_old_live_session() {
        let (config, mut store) = fixture();
        let calls = Rc::clone(&store.calls);
        let live = store.entries[0].id.clone();
        let report = root_with(
            &config,
            2_000_000,
            false,
            &mut store,
            || {
                assert_eq!(calls.replace(2), 1);
                Ok(BTreeSet::from([live.clone()]))
            },
            Instant::now() + RUN_BUDGET,
        );
        assert!(report.error.is_none());
        assert_eq!(report.planned.len(), 2);
        assert!(!report.planned.contains(&live));
        assert!(store.removed.is_empty());
        assert_eq!(report.remaining_bytes, 30);
    }
    #[test]
    fn unknown_protection_refuses_entire_root_but_reports_full_disk() {
        let (config, mut store) = fixture();
        store.full = true;
        let report = root_with(
            &config,
            2_000_000,
            true,
            &mut store,
            || Err(io::Error::other("service restarted")),
            Instant::now() + RUN_BUDGET,
        );
        assert!(report.error.as_deref().unwrap().contains("restarted"));
        assert!(report.planned.is_empty());
        assert!(store.removed.is_empty());
        assert_eq!(report.disk.unwrap().available_bytes, 0);
    }
    #[test]
    fn full_disk_is_not_permission_to_bypass_safe_mutation_failures() {
        let (config, mut store) = fixture();
        store.full = true;
        store.failure = true;
        let calls = Rc::clone(&store.calls);
        let report = root_with(
            &config,
            2_000_000,
            true,
            &mut store,
            || {
                calls.set(2);
                Ok(BTreeSet::new())
            },
            Instant::now() + RUN_BUDGET,
        );
        assert_eq!(report.refused.len(), 3);
        assert_eq!(report.removed_bytes, 0);
        assert_eq!(report.remaining_bytes, 30);
        assert!(report.removed.is_empty());
    }
    #[test]
    fn partial_unlinks_are_accounted_without_claiming_complete_session_removal() {
        let (config, mut store) = fixture();
        store.partial = true;
        let calls = Rc::clone(&store.calls);
        let report = root_with(
            &config,
            2_000_000,
            true,
            &mut store,
            || {
                calls.set(2);
                Ok(BTreeSet::new())
            },
            Instant::now() + RUN_BUDGET,
        );
        assert_eq!(report.removed_bytes, 6);
        assert_eq!(report.remaining_bytes, 24);
        assert_eq!(report.refused.len(), 3);
        assert!(report.removed.is_empty());
    }
    #[test]
    fn expired_budget_cannot_reach_protection_or_deletion() {
        let (config, mut store) = fixture();
        let report = root_with(
            &config,
            2_000_000,
            true,
            &mut store,
            || panic!("guard must not run"),
            Instant::now(),
        );
        assert!(report.error.unwrap().contains("budget"));
        assert!(store.removed.is_empty());
    }
    #[test]
    fn disabled_apply_does_not_even_open_a_configured_root() {
        let report = execute(&Config::customer_default(), 100, true);
        assert!(!report.enabled);
        assert!(report.roots.is_empty());
        assert!(report.error.is_none());
        assert_eq!(exit_code(&report), 0);
    }
}
