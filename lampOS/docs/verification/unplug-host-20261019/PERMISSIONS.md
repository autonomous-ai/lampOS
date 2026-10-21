# Integration with core/store permissions (#1698), 19 October 2026

The three existing unplug commits were rebased onto upstream
`55271a2de` (which includes #1698), producing checkpoint `36b76292e`.
This note covers the subsequent permission integration correction, whose source
and checked consumers are pinned by [permissions-source-sha256.txt](permissions-source-sha256.txt).
The original full host gate, integration gate, identity gate and their manifests
are historical snapshots and remain unchanged. Their totals are not claimed as
a full workspace retest of this source.

## Integration changes

- Preserve upstream package/core-store modules, loader and runtime permission
  enforcement, tool-name ownership and ordinary skill behavior.
- Prepare-to-unplug declares `kind = "core"`, version `1.0.0`, body and speech
  permission, and reads of the kept body request plus Body, Expression and Ring
  completion topics. It remains absent from the catalog when `LAMP_UNPLUG` is
  not `on`; no product unit enables it.
- The loader requires the canonical skill/tool names and Core kind. The
  composite `PrepareUnplug` call requires body, speech and all four topic
  grants. Runtime admission, the request path and delayed readiness speech
  recheck those grants; failure speech separately requires speech permission.
- Respect upstream's captured-grant lifetime: catalog reload/removal changes
  future runs, not the grants of a run already admitted. Tests pin this behavior.
  No new generation, revocation, socket or Voice protocol was introduced.
  Physical readiness remains independently live and expiring in Voice.
- Package installation/removal tests confirm this is a product core skill;
  an installable definition with canonical names cannot use the fold operation.
  Trusted local definitions and IPC callers are still the trust boundary; this
  is not cryptographic authentication or exclusion of arbitrary bus writers.

## Checks and results

Commands ran from `lampOS/`, using its pinned toolchain and existing target,
with `RUSTC_WRAPPER= CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`.
Socket/process tests ran outside the filesystem sandbox.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| `cargo clippy -p lamp-skills -p lamp-voice -p lamp-update --all-targets -- -D warnings` | Passed; final run after fixture fixes |
| `cargo test -p lamp-skills -- --test-threads=1` | Library 17, binary 6 and package 12 passed; integration failures retained below |
| `cargo test -p lamp-skills --test skills -- --test-threads=1` | Corrected integration suite: 89 passed, zero failed or ignored |
| `cargo test -p lamp-voice --lib unplug -- --test-threads=1` | 10 passed |
| `cargo test -p lamp-voice --lib say:: -- --test-threads=1` | 36 passed |
| `cargo test -p lamp-update --test install --test functions -- --test-threads=1` | Install 48, functions 2 passed |

Skills covers 124 distinct tests across those successful suites. The library,
binary and package code/tests were unchanged by the final fixture corrections;
they were not rerun after the integration suite passed. The two Voice filters
overlap and are not summed as distinct tests. No ignored test is counted.

## Failures retained and corrected

The first full Skills run found two fixture/integration issues: the upstream
core inventory omitted the new seventh core skill, and the new package test
had not created its required installation directory. Both were corrected.
A second full run passed those but the newly added default-off process fixture
incorrectly waited for an empty `tools.json`; upstream removes an empty catalog.
The corrected fixture installs ordinary core Dance alongside unplug, waits for
its real catalog and verifies that only Dance is advertised, the unplug call
is refused and no body request is published. The complete 89-test integration
suite then passed. No production check was weakened to fix these tests.

One accidental test invocation from the parent directory began compiling with
the wrong toolchain context; it was interrupted (exit 130) and contributes no
result above. Subsequent commands used `lampOS/` as required. The existing
target was approximately 4.5 GB, with 22 GiB available, when checked during this
run; no other checkout or device was cleaned or changed.

No full workspace, native ARM64, acoustic or real-lamp result is claimed here.
