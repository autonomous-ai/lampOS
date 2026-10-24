//! Actual release readers and writers under the native host's real systemd.
//! Explicit qualification only: missing artifacts/privileges never mean success.
#![cfg(target_os = "linux")]

#[path = "support/release_pair/mod.rs"]
mod pair;

#[test]
#[ignore = "requires verified release artifacts and native root/systemd; run explicitly"]
fn records_6408_prunes_6449_private_sessions_and_preserves_live_invocation() {
    pair::run(pair::CaseKind::OldReadsCurrent);
}

#[test]
#[ignore = "requires verified release artifacts and native root/systemd; run explicitly"]
fn records_6449_prunes_6408_private_sessions_and_preserves_live_invocation() {
    pair::run(pair::CaseKind::CurrentReadsOld);
}

#[test]
#[ignore = "requires verified release artifacts and native root/systemd; run explicitly"]
fn records_6408_refuses_6449_shared_layout_and_6449_can_resume() {
    pair::run(pair::CaseKind::SharedRefusal);
}
