# Release qualification after the integration landing

PR [#2462](https://github.com/autonomous-ai/lampOS/pull/2462), head
`1399893f45101fdb181b9615fce31daf523034a8`, is ready for reviewer integration.
Publication remains disabled. This evidence supersedes the earlier head's
qualification without changing or removing its records.

[Native, artifact, x86 and general CI receipt](native.json) binds all results
to their workflow logs and checkout identities. [Fresh host evidence](https://github.com/autonomous-ai/lampOS/blob/1399893f45101fdb181b9615fce31daf523034a8/lampOS/docs/verification/update-release-host-20261021.json)
records 35 publisher, 12 provenance and 7 real-updater CLI passes, a fresh
Rust 1.96 updater build, actionlint and the workflow checks. The rebase
preserves updater level 5, the reset-plan documentation and commit-specific
CI concurrency; the five Python files are unchanged from the reviewed head.

## Native results

[ARM64 run 39613083839](https://github.com/autonomous-ai/lampOS/actions/runs/39613083839)
and [x86 Clippy run 39613083789](https://github.com/autonomous-ai/lampOS/actions/runs/39613083789)
tested merge `be308f4c5e5c058fb07f89303abbd75e37ddc6f4` against integration
base `64b2cb461bc593461b69779c515deea6813797b3`.

- Nextest: `9621 tests run: 9621 passed (1 leaky), 63 skipped`, 159.372 seconds,
  154 binaries; two own-harness binaries skipped by that profile run separately.
- Four doctests and 12 own-harness tests passed; relay signals passed separately.
  Nested test stdout is not counted a second time. Total counted Rust passes: 9,637.
- Native Python suites: `Ran 35 tests in 0.292s`, `Ran 12 tests in 0.862s`,
  `Ran 7 tests in 0.416s`; each ended `OK`.
- Strict all-workspace/all-target x86 Clippy passed without Rust warnings.
  Its system-package certificate rehash warning is recorded separately.
- [General CI run 39613073681](https://github.com/autonomous-ai/lampOS/actions/runs/39613073681)
  passed Go, Web and Python jobs, each checking out the exact branch head.

The known `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`
leaked-process diagnostic and four unique Cargo example filename-collision
warnings are preserved. This is the PR profile, not the full nightly realtime
qualification. The normal PR cache path ran; cold publication preparation and
the protected signing/publication job were skipped.

## Artifact verification

The downloaded ZIP is **112,532,033 bytes**, SHA-256
`ae7ac15cd9cafbccda07b4750773cabce49e8ba1a73f49d7b9ac970866e3d3b4`,
matching GitHub metadata and the upload log. All **50 members** were streamed
without extraction or execution; all **49 payload SHA-256 checksums** match.
Membership is derived from classification and exporter blobs at the tested
merge: **47 binaries, two selected examples, one checksum file**. The receipt
records every member's size and hash, both source blob identities and CRC checks.

Uncompressed artifact: **304,473,020 bytes**. The publisher's retained subset
is **31 binaries / 218,565,328 bytes**: 13 core, 16 optional and two provisioning
binaries. That subtotal excludes generated units, skills, checksums and signed
envelopes; it is not a complete release size. Sixteen development binaries and
two CI examples are omitted from publication.

## Limits and handoff

No environment, key, repository variable, release or device was changed.
Host and native rehearsals use disposable keys and simulated GitHub. Real-key
internet delivery, quiet recovery, whole-lamp acceptance and activation remain
with the owner and the wider OTA handoff. Publication stays disabled. Only
an actual reviewer/DevOps merge completes Second's workflow/docs slice of
[#1812](https://github.com/autonomous-ai/lampOS/issues/1812).
