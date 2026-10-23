# Existing relay PID failure: historical receipt and source audit

The retained 22 October Voice-eval run reports the same test, source line and
failure type as the current required reader gate:

```text
relay_hardening::a_closed_runner_transport_stops_the_runtime_and_its_workers
crates/voice-eval/tests/it/relay_hardening.rs:126:54
called `Result::unwrap()` on an `Err` value: ParseIntError { kind: Empty }
```

The old command was `cargo test -p lamp-voice-eval`, exit 101: 607 passed and
one failed across its completed targets; the integration target was 233/1/0.
Its runner recorded Git HEAD `a503d70f034990c72abe10ed55239372b7de8f23` with
the memory-note working-tree changes present. It was **not a clean base run**
and has no exact immutable tested commit. The evidence was published in
[the memory-note receipt](https://github.com/autonomous-ai/lampOS/blob/fdf8914f338dd0c1888be2581d90275e106a0cbe/lampOS/docs/verification/memory-note-retry-20261022/README.md),
before the boot-clock change.

The complete original combined output is retained byte-for-byte in
[`raw/historical-voice-eval.log.txt`](raw/historical-voice-eval.log.txt):
63,242 bytes, SHA256
`2e9c9978c13665730c2e201becb3a0416b8845333e863accd5fac5a01792f767`.
The working copy and the preserved `before-serial-eval` copy match exactly.
[`historical-observation.json`](historical-observation.json) projects the exact
command, exit/counts, recorded source state, host and provenance. The focused
diagnostic is in [`historical-failure-excerpt.txt`](historical-failure-excerpt.txt).

All three relevant Git blobs—`tests/it/relay_hardening.rs`, product
`src/physical/session.rs`, and `tests/it/common/mod.rs`—are byte-identical at
the historical recorded HEAD, published receipt commit, current base
`e5cd73aca116e6bb52fbc41fdd0094b8bf10f5bc`, and boot candidate
`0f2e6439e018680af247080983c9ec0c258c3229`. Exact hashes and the boot patch's
eight paths are in [`source-identities.json`](source-identities.json). The boot
patch changes no Voice-eval path. This is source equality, not compiled-binary
identity.

The current integration failure is 237/1/0 with the same line/type. Its excerpt
and the SHA256 of the complete current reader stdout are in
[`current-observation-link.json`](current-observation-link.json). The parent
and receipt agent own publication of that complete terminal gate; it is not
duplicated or reclassified as passing here.

## What this establishes and does not

This is an earlier independent observation of the same failure, predating the
boot-clock change. It is stronger than merely noticing unchanged source.
Neither failed observation is waived. No new execution was needed or performed.

The historical receipt lacks a complete source census and executable hash. It
records only three changed Voice-file hashes; two do not equal the later
rebased memory commit. Do not relabel its run as that later committed head or
claim compiled identity. Neither old nor current log captures the shell's
redirection/write interleaving, so the precise cause of the current failure
remains unproven. The historical serial pass is not offered as qualification.

## Source boundary

`relay()` starts a closer thread that drops the runner stream once its chosen
file exists (lines 53–61). The failing shell script starts a worker and runs
`echo $! > worker.pid` (lines 203–206). Redirection can create the file before
the PID bytes are written. Existence alone therefore does not prove PID
publication completed.

The unchanged product `run()` uses its system-clock entry point. The input
reader's EOF disconnects the channel; the owner observes that at lines
1379–1383, and the non-serve branch at lines 1391–1400 kills and reaps the
process group through `kill()` (lines 782–789). This can terminate the fake
shell before it finishes writing the PID. After all preceding original
assertions have passed, `read_pid()` parses the now-stable empty file and
panics. That is a source-supported possible ordering, not a recorded trace of
either failure. Private directories include the process ID and an atomic
counter; no cross-test pathname collision is visible in this fixture.

[`prospective-design.md`](prospective-design.md) proposes a separate completion
marker and one bounded adversarial old/new control. It is a design only: no
patch, claim, worktree edit, Cargo command or test was performed. Recent retained
Queue comments and the Tests log contain no exact fixture claim; the bounded
search scope and its limits are in [`ownership-read.json`](ownership-read.json).
