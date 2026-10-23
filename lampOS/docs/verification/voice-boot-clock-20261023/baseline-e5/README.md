# e5cd73aca full Voice baseline: completed failure

**Ready: NO.** One fixed default-parallel Voice package observation on exact base
`e5cd73aca116e6bb52fbc41fdd0094b8bf10f5bc` completed with exit **101**:
**3,860 passed / 10 failed / 15 ignored**. All six target summaries and all
3,885 case identities bind to their compiled/listed inventories. All 86 named
boot-clock cases passed. Compilation and source restoration succeeded; this
remains a failed test observation, with no waiver and no retry.

This receipt reads only completed baseline evidence. It does not include or
qualify the separate active boot AFTER run (`0f2e6439e018680af247080983c9ec0c258c3229`).
Its source, outputs and target were not inspected during this audit.

The exact command was:

```text
cargo test --offline --locked -p lamp-voice --no-fail-fast --message-format=json-render-diagnostics
```

It ran once from the frozen phone worktree's lampOS directory, after its two
owned files were temporarily restored to exact base blobs. Default test
parallelism remained enabled. Command wall time was **618.682987 seconds**,
including compilation. Compiler stderr reports `5m 08s`; this rounded compiler
message is not a separately measured compile wall time. The command had a
1,800-second external watchdog and did not time out.

| Target | Passed | Failed | Ignored | Reported target seconds |
| --- | ---: | ---: | ---: | ---: |
| lamp-voice:lib:lamp_voice | 3580 | 9 | 15 | 288.13 |
| lamp-voice:bin:lamp-voice | 1 | 0 | 0 | 0.0 |
| lamp-voice:test:it | 269 | 1 | 0 | 10.36 |
| lamp-voice:example:render_line_second | 4 | 0 | 0 | 0.0 |
| lamp-voice:example:stop_words_stt_replay | 6 | 0 | 0 | 0.18 |
| lamp-voice:doctests | 0 | 0 | 0 | 0.0 |

[Results](results.json), [target summaries](targets.json) and all
[86 selected outcomes](affected-86.json) preserve execution separately from
listing. Per-case elapsed times were not emitted. Do not infer individual case
durations from target times or the command wall time.

## Failures

The complete failure texts and source locations are in [failures.json](failures.json)
and the full human stdout payload. Eight library failures are in the motor
fixtures: five at `coordinator/motor/tests.rs:134` report a stepped-child timeout;
two at line 883 report qualification expiry before startup; one at line 1362
reports stale/invalid offer, placement or qualification evidence. The remaining
library failure is the privacy publisher case at `privacy_bus.rs:1313:25`.
Integration fails the light-call latency assertion at `tests/it/light_call.rs:90:5`.
Every failure remains unclassified and unwaived here. No cause is inferred from
a diagnostic's wording or a later pass.

## Comparison with the earlier frozen receipt

[Source and boundary comparison](comparison/source-and-boundaries.json) records
full file hashes and Git blob IDs for f477d6c, da595851 and this e5 baseline.
`privacy_bus.rs` (63,219 bytes) and `tests/it/light_call.rs` (3,214 bytes) are each
byte-identical across all three revisions; their source paths and assertion
locations did not move. The complete repository snapshots, binaries and case
inventories differ: this baseline has 3,885 cases versus the old pair's 3,843.

- Privacy passed on old f477 and failed on old da595. It now fails on unchanged
  e5 at the same line with the exact message `the host stalled in 10 of 10
  attempts: privacy publisher unavailable`. This is evidence that this boundary
  can occur without the boot fixture patch on the later base. The diagnostic
  does not prove host-load causation or the cause of the earlier occurrence.
- Light-call fails the unchanged `<25 ms` assertion in all three observations.
  The code selects element 10 of the sorted 20 durations (upper middle):
  48.529192 ms on f477, 98.366350 ms on da595, and 38.435873 ms here. All three
  full arrays and exact failure excerpts are retained. These are not matched
  host conditions or physical-device latency results.

This comparison does **not** classify all five earlier candidate-only library
failures as inherited, and does not waive them. The other four are not explained
by these two repeated boundaries. No extra test was executed for this audit.

## Source, restoration and binary proof

[Source proof](source-proof.json) verifies all **9,992 tracked paths**, including
6,649 lampOS paths, root robots/lamp inputs and the literal tracked symlink.
An independent reconstruction from the exact Git tree matched every recorded
path, mode, size and content hash and recomputed the full base aggregate.
Mtime refresh preserved that aggregate, as did execution and census collection.

The borrowed phone worktree's saved two-file candidate was restored exactly,
including permissions. Its complete 9,992-path candidate aggregate was separately
reconstructed from the base and the saved two snapshots and matched the recorded
restoration. This restoration identity is the phone candidate, **not** the boot
AFTER tree. No repository, live source or build target was touched by packaging.

[Artifacts](artifacts.json) records all 13 Cargo executable artifacts and their
captured byte lengths/SHA256s, target/profile fields and source aggregate. Five
are listed test executables and have actual execution census proof; the other
eight are ordinary compiled executable artifacts, not missing test runs.
No assertion is made about current bytes in the reused target directory.
[Environment](environment.json) records the frozen compiler/ISA and overrides.
Rust reports x86_64-apple-darwin and the test files are Mach-O x86_64; physical
hardware and translation state were not established.

## Census, parser and raw evidence

[Census bindings](census.json) reference complete raw `--list` and `--ignored
--list` outputs for all five test executables. Listing runs no test case.
[Parser proof](parser-proof.json) preserves all three ` - should panic` display
annotations and their unique compiled identities. Normalization applies only
when the raw display is absent and removing that one suffix resolves exactly
one compiled identity. Every row and ignored identity then matches its census;
no row is silently dropped. An in-memory missing-identity control is rejected.
The earlier parser replay retains its actual failures and is labeled as parser
work, not new test execution.

[Commands](commands.json) contains all 17 command records, exact arguments,
stdout/stderr bindings, status and times. The entire non-Cargo stdout and full
stderr are preserved losslessly, including every failure and progress row.
Only complete Cargo machine-record lines, all before the first test suite,
are omitted from the portable stdout. Their original byte ranges, hashes and
the complete original stream's hash/path are explicit. Every retained byte
range maps back to the original combined stream; no test text is removed.
Compiler stderr and normalized exact artifact/compilation evidence remain.

[Raw payloads](raw-payloads.json) gives byte offsets, lengths and hashes for all
streams, inventories and historical failure excerpts. Slice those exact payload
bytes; wrapper markers are not part of the original output. The one transformed
stdout is explicitly mapped in commands.json. Shared empty/identical identity
streams are deduplicated without changing bytes. All other streams are exact.

[External files](external-files.json) pins the original sealed receipt, large
source maps, redundant metadata, omitted Cargo machine stream and historical
receipt by path, size and SHA256. Full source maps are reproducible from the
pinned Git tree and saved overlay identities; their original map files remain
external. This availability limit is explicit. Local PATH and host-node details
are omitted. No Python, executable, archive or Cargo machine stream is packaged.

[validation.json](validation.json) and [SHA256SUMS](SHA256SUMS) record verification.
Publication remains with the parent; no full-package failure is waived here.
