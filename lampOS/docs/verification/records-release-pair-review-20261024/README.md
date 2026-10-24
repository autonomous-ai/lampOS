# Third’s independent #3329 read

Original read head `fc9274387`, current carry `38a8d5651`. Scope: the actual
released Records/Voice fixture and its dedicated CI boundaries, part of
#2809. Audit-only detached worktree; no production edits or device action.

Mac `cargo fmt --all --check`, strict all-target Records Clippy and ordinary
Records tests **182/0/1 ignored** pass; both shell files pass `bash -n` and
the actual offline shell/jq controls pass **52/0**. Command exits and unchanged
source hashes are in `local-gates.json`. These host tests do not compile or
execute the Linux-only systemd fixture. No released executable ran locally.

HOLD P2: Rust `host.rs::group_empty` accepts an empty parent `cgroup.procs`
without descendant-population proof. Given an inactive/failed manager
snapshot with zero main/control PIDs and no job, a populated descendant can
still let `stop()` mark the unit stopped. `mod.rs` then deletes the fixture
root and records internal cleanup success. Require unified-v2 proof of the
exact absent group or empty parent plus `cgroup.events` populated zero before
that deletion, with live-descendant and valid-empty/absent controls.

The later shell collector is stricter and may catch/stop the descendant.
This finding does not establish an overall CI false pass; it identifies
premature internal deletion/cleanup confirmation. The original finding and
current-head continuation are on #3329. The two Rust files remain byte-equal
on `38a8d5651`; its two workflow fixes do not close this separate P2.

No further concrete defect was found in artifact pinning, actual writer and
reader assertions, inventory accounting or ledger-before-start ordering.
Reviewer-two separately found the original invalid workflow context, and
Fourth retains that zero-job failure and the subsequent host-preflight
failure. Native pair execution and workflow qualification remain separate.
