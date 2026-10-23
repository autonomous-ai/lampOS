# Skills early-refusal fixture: controls and local gates

PR [#3277](https://github.com/autonomous-ai/lampOS/pull/3277), Part of #1685. Authored head `1806a691d9dc8f93c61c116793f9c68230448c17` has parent `be16c3327524ccef8a1908a3b8ebf40c16efba8a` and changes only `crates/skills/tests/it/package.rs` and `docs/skills.md` under lampOS. Latest-base fetch before commit found no drift.

The local Mac controls and gates used that parent plus the two frozen snapshots. All 10,055 tracked bytes/modes bind to the later commit; no claim is made that a precommit binary embedded the later Git head. Exact committed-head Linux formatting ran separately. Source identities, replayable scratch patches and assertions are in [source-binding.json](source-binding.json) and [controls.json](controls.json).

| Observation, executed once | Passed / failed / ignored | Exit | Result |
| --- | --- | --- | --- |
| Old fixture, child exit before write | 0 / 1 / 0 | 101 | Original write unwrap: OS32 `BrokenPipe`; 304 filtered |
| Candidate, same positive exit barrier | 1 / 0 / 0 | 0 | OS32 `BrokenPipe`, OS57 `NotConnected`, exact refusal, failed child status, preserved skill; 305 filtered |
| Uninstrumented `package::` | 23 / 0 / 0 | 0 | All selected package cases; 283 filtered |
| Local Mac full Skills, default parallel | 513 / 0 / 1 | 0 | All 514 compiled cases accounted for; zero filtered or unrun |
| Mac fmt / strict Skills Clippy / workspace build | — | 0 / 0 / 0 | All completed |
| Exact-head Linux x86 fmt | — | 0 | Source unchanged; no compilation, tests or ARM build |

The new permanent control waits on the exact child's `try_wait` exit event, with a 20-second watchdog and 10 ms poll. The baseline keeps its original unconditional write unwrap after the same barrier. Both controls observed child exit status 1 before writing; both PIDs were absent afterward without the runner sending a signal. The candidate collected the reply, status and preserved file before judging transport results. Authorized request write/half-close unwraps and callback order remain strict. Both scratch worktrees restored their exact initial files; shipping source bytes stayed frozen. No control or failed test was retried.

The [complete small native streams](native-raw-streams.json), including the original failure and successful control diagnostics, are lossless UTF-8 envelopes with original byte counts and SHA-256. [Compiled census](compiled-census.json) and [all terminal rows](local-gates.json) bind the actual selection. [Commands](commands.json) retain bounds and actual exits. The full Skills command is `cargo test -p lamp-skills --offline --locked --no-fail-fast`, with `RUST_TEST_THREADS` unset, jobs4/debug0/incremental0; Rust1.96 and executable architecture are recorded. Focused and control artifact commands use the exact binaries produced by the recorded no-run Cargo command.

[Linux formatting](linux-formatting.json) used the existing cached Linux amd64 image, rootless user1000, no network or shared target, and `cargo fmt --all --check`. Its 14.524-second timing includes container setup, extraction, formatting and hashing. The one ephemeral hostname in its stdout and the repository cwd in command metadata are explicitly projected; original hashes/offsets are retained. Native streams are unchanged. Full compiler-message JSON, source maps/archive and raw image metadata remain external, with their [digests and availability limits](external-raw-index.json); no executable or Python is published.

This proves the early-refusal fixture failure mode and its narrow repair, not the original remote run's exact scheduling. There is no speedup, device or ARM result in this receipt. Required PR CI and assigned reviews remain separate; no other failed gate is waived. [Validation](validation.json) covers JSON, raw reconstruction, census, links, whitespace and manifest identity.
