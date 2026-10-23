# Third's actual-updater release-pair review

Assigned by First on Queue #3126, 24 October 00:03. Read #3278 at
`7ebb876be004a84981cc71bf8b01e3eb3a06eb4c`; carried to
`baed91ead4cc9712ad053090fa2724b05897e2ce` on `1291ef1f4`.
All 255 authored paths retain identical bytes and Git modes, with one
unchanged commit by range-diff. Only the five upstream Eyes paths differ.

**READ-OK `baed91ead`: no P1/P2.** Current-head native and Linux formatting
evidence are verified below. First owns the final latest-base check and merge.

## Source and evidence

Read all of `release_pair.rs`, `release_pair/run.sh`,
`release_pair/systemctl.sh`, the documentation delta and retained receipts.
Both fixture shell syntax checks pass; source/docs diff check passes. No
artifact test, build, Docker command or device action was run by Third.

The verified release executables create their own journals and cross-version
Apply/Back requests. The writer stops at a handshake before the first ordinary
restart, after candidate settings/units/links are verified. The fixture checks
its executable and process-group ownership before SIGKILL, retains the journal
unchanged, and invokes the other release's CLI for recovery. Assertions cover
old settings bytes/modes, units, links, selected binaries, state canary,
saved restart order, inactive Schedule and refusal of the failed candidate.
Container removal is restricted to the fixture's owner label and bounded.

[Independent evidence audit](evidence-audit.txt): all **234 raw-file**
hash/size bindings, **166 snapshot assertions**, and **three source hashes**
verify. Both manual cases' terminal/container/helper statuses support **2/0**,
distinct from the ordinary suite's two intentional external-artifact skips.
Original filesystem modes are producer-recorded, not reconstructed from
the committed files' Git modes. The back/previous records plus the fixture
bind the old data; there is no standalone pre-execution filesystem snapshot.

P3: the receipt claims a manifest-entry comparison but retains only the public
envelope digest, without its envelope/decoded updater entry or comparison
record. Retain that small public provenance record. This does not undermine
the verified asset identity or recovery assertions and does not hold the PR.
Reviewer-two separately noted the missing source-after timeline record.

Resolved after the read: Second retained the public envelope, decoded
manifest and updater comparison at `a3c8aa660`. Third verified the envelope
digest, decoded the base64 payload, matched the decoded file and updater
entry, and checked the explicit `signature_verified: false` limit. The
provenance P3 is closed without another experiment. The source-after
timeline limitation is now stated explicitly in Second's native receipt.

## Independently checked native and artifact identity

GitHub metadata confirms updater asset SHA-256 and size for releases 6408
(`f731555d…`, 4,690,504 bytes) and 6449 (`c953a32e…`, 4,739,096 bytes).
Run 39973722029 reports successful build and publisher jobs on `be16c3327`.
This is consistency with published assets, not independent release-key trust.

Primary logs for reviewed `7ebb876be`: ARM64 **39992469455**, **11,068 passed
(one Voice LEAK), 0 failed, 71 skipped**; process harness **12/0**. Strict
workspace Clippy **39992469415** passes. Both check merge `88771df4e`, whose
tree `0701c61c8109074ec25bb4c9e3c5c6bad3111991` equals the authored tree.
[Run metadata](reviewed-arm64.json) is retained. Full downloaded logs stay
in local scratch; these results qualify the reviewed head, not the later carry.

Second's exact-head Linux formatting receipt at `37306496f` matches the
reviewed tree/subtree and 6,963 before/after file hashes; its command log,
exit zero, restricted source-only container and owner-checked cleanup agree.
The disclosed first setup failure happened before Cargo and is not a test pass.

## Final carried-head qualification

Primary logs for `baed91ead`: ARM64 **39993981374**, **11,069 passed including
one Voice LEAK / 0 failed / 71 skipped**, process harness **12/0**. Strict
workspace Clippy **39993981366** passes. Both checked merge `6637984494`, whose
repository tree `642c8009948a5ecd7af819a49226c9f450d762a2` equals the PR head.
[Terminal metadata](final-arm64.json) and [raw log hashes](native-log-hashes.json)
are retained; the full logs remain in local scratch.

Second's final Linux formatting receipt `6baa7314c` passes on `baed91ead`.
Third reconstructed all **6,963** regular-file hashes from the exact Git
archive: manifest `c1bdfbd65317a4f88f716e79399f02e57c13e4b3e306a351b584aa34f62cc96f`
matches both recorded manifests. Source trees, command result, normal
terminal exit and owner-checked removal agree. No fixture execution was
repeated by Third. The recorded base is current `1291ef1f4` at this read.

One process-kill boundary per direction with fake systemctl/health does not
qualify physical power loss, production systemd, flash durability, attended
motion or independent signature trust. Shell wait status 137 is not a direct
`WIFSIGNALED` observation. First retains final merge and acceptance disposition.
