# Current integration checkpoint, 20 October 2026

Music #1757 is not ready yet. This folder records the fresh integration after
#2157 landed. Earlier foundations and native runs remain historical evidence.

- [Preserved preflight failures](preflights.json): missing current Reports
  fixture fields, then a genuine recording-gap completion failure. Neither
  failure is relabelled as a passing gate.
- [Current source snapshot](music-source-snapshot.json) and [changed inputs](music-inputs.json):
  frozen on `dfc5f73c00dbf10a65b668156914d44a19fb4575`, including the
  separate cancellation/completion race fix found in review.
- [Consumer coverage](music-consumers.json): five changed crates, 29 reverse
  consumers, with all changed-name grep consumers covered. Full unfiltered
  suites are running; source-only builds retain the documented Update fixture
  exception. Final results and native CI will be recorded separately.

The new regression set preserves Policy holds and approval, held consent,
retry identity, face narrowing, warm name carry, search(found), and the
seven-second Written wait. A Written call still carries no spoken Music
authority. Completion now requires the ticket's successful atomic transition,
not merely a playback check made before a concurrent cancellation.

The feature remains off unless explicitly enabled. No device action,
acoustic measurement, musical-quality judgment or physical qualification is
claimed. The host example will be rendered, packed and installed only into a
disposable directory; generated assets will not enter Git.

The [earlier complete snapshot](music-before-latest-base.json), commit
`e1c3226665bbe01dab6dcec678d426d1d3f59d08` on `bc31e93d85`, passed 7,992 tests
with zero failures, 32 ignored and none filtered, plus formatting, Clippy and
workspace build. All 2,114 source hashes match both copies after the gate.
It predates the newer base and terminal-transition correction.

The [new candidate's focused results](music-current-focused.json) are 12 voice
integration, 49 audio and 24 Skills tests, zero failures, plus formatting and
strict workspace Clippy. Its host handoff was 199 microseconds first use and
194 repeated, confirmation retirement to authored speaker IPC acceptance.
This is not acoustic onset. The full consumer and example stages are running.
