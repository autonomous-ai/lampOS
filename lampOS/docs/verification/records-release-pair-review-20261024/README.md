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

## Correction read: `6f79d33d6`

The Rust cleanup P2 is closed. The predicate now requires the exact owned
unit path, unified-v2 mount evidence, and either group absence or an empty
parent plus one valid `populated 0`. Malformed, duplicate, missing, oversized
and linked proof refuses. `stop` requires the proof before setting stopped;
fixture deletion remains after every owned stop and command cleanup succeeds.

Independently extracted the four exact std-only helper functions and six
new tests, adding only std imports, and compiled them with Rust 1.96.0 on
the Mac: **6/0**. The bound source hash, command and boundary are in
`corrected-cgroup-controls.json`; source and raw output are retained. This is
a disposable-filesystem predicate test, not Mac systemd/cgroup execution.
Current shell/jq controls pass **52/0**, both shell syntax checks and diff
whitespace checks pass.

Fresh dedicated native run **40060176957** is terminal success on this head:
**3/0**, six ordinary controls filtered, 45.53 seconds. Third re-executed
the current terminal schema over all three receipts; verified four collected
file hashes, 69 complete command stdout/stderr hash pairs and all six external
owned-unit exit proofs. Five source-map pairs match over 10,323 paths; 14
primary git-blob hashes match the authored source, and preview `9d4c2fc51`
has the exact authored tree. `corrected-native-verification.json` retains
these bindings and every top-level artifact-file hash.

The private pair works both directions; the old shared-layout reader refuses,
then the current reader resumes. This is actual released-executable/systemd
CI evidence, not signature-trust, full-rotation or lamp qualification.
The cancelled historical run and earlier workflow/lint failures remain
separate; ordinary ARM64 and Linux Clippy are still distinct required gates.
