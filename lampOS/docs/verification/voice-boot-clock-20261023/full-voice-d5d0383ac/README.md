# #3190 current-base native Voice gate

**Ready for lamp-voice: no.** One complete default-parallel Voice package run on `d5d0383accafac8aa54c352eba65b1b0d5a44d9d`, based on `cf462adb04f64d699c226dd3b1781c0ad927068f`, finished with **3,921 passed, 3 failed and 15 ignored**. Formatting and strict Voice Clippy passed. All 86 clock cases and the separate LoadingChild case passed. The three other failures remain unwaived; their cause is not established. No candidate run was repeated.

| Target | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Voice library | 3,642 | 1 | 15 |
| Voice binary | 1 | 0 | 0 |
| Integration `it` | 268 | 2 | 0 |
| `render_line_second` example | 4 | 0 | 0 |
| `stop_words_stt_replay` example | 6 | 0 | 0 |
| Doctests | 0 | 0 | 0 |

All six targets, including the zero-test doctest target, have complete result-to-inventory bindings. The five executed binaries contain 3,939 cases. The current library has 3,658 identities: 37 additions and no removals relative to the older focused 3,621 inventory. The old count was not imposed on this changed base. [Results](results.json) retain all 87 selected outcomes, failed names, ignored names, target counts and display-annotation mappings; [census](case-census.json) binds the complete current inventories to the captured binaries and raw lists.

## Actual failures

- `provider_worker::tests::held_consent::asked_about_reports_after_a_held_stop_it_says_who_and_when`: `held_consent.rs:326` expected the `sending` field to be `false`; it was `Null` because the reply said the reports part was not running. Source inspection finds the fixture publishes its 15-second state before the rig acquires the shared test lock and refreshes only on a request; this read-only reports-state scenario sends no request. The output contains no state-age timestamp proving expiry, so that ordering is a possible mechanism, not a diagnosis.
- `fixture_provider::stale_cue_and_peer_loss_latch_invalid_without_blocking_or_retrying`: `fixture_provider.rs:691` failed `sink.scan(&trace, AT).is_some()`. This first scan uses a fresh sink, a valid fixed timestamp and a dropped peer. There is no elapsed-time wait at this assertion. No syscall or file-descriptor-lifetime diagnostic explains the missing observed fault.
- `light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`: `light_call.rs:90` requires the sorted sample at index `len / 2` to be below 25 ms. For these 20 samples the upper-middle value was **57.810245 ms**. All 20 `Outcome::Done` checks passed. Startup readiness precedes timing and teardown follows it; each measured interval surrounds `Executive::call(light_colour)`. The complete 20-value array is retained.

[Failure records](failures.json) preserve the exact diagnostics and their byte offsets/hashes in the original stdout. [All three asserting files](failure-source-boundaries.json) are byte-identical between this head and its exact base. The integration tests use the normal Voice library, excluding its `cfg(test)` clock and LoadingChild code. These source facts do not establish the cause of the unscheduled failures. A separate clean-base comparison is being prepared; it has not been executed in this receipt. Earlier failed gates also remain unchanged.

## Commands and source identity

The ordered commands were:

```
cargo fmt --all --check
cargo clippy -p lamp-voice --all-targets --offline --locked --message-format=json-render-diagnostics -- -D warnings
cargo test --offline --locked -p lamp-voice --no-fail-fast --message-format=json-render-diagnostics
```

They finished respectively with exit 0 in 13.477148 s, exit 0 in 91.935472 s, and exit 101 in 412.365950 s. The last time includes compilation and the full package command. Each ran once. `RUST_TEST_THREADS` was unset and no thread override was passed. MAIN and Cargo CPU were exclusive to this run; four compiler jobs, dev/test debug0, incremental0 and offline dependencies were retained. The tool launched with authorized local-socket permissions; Unix datagram and IPv4 loopback listener preflights passed before Cargo. No watchdog fired. No readers or workspace build ran in this piece.

The clean committed tree is `278baa2eace61124fe255386404dc84410b446a8`. All 10,026 tracked repository paths, including root fixtures and symlink targets, were hashed before and after; the aggregate remained `688c434f7b2bde1c21313c601e990c279d49da4f66fbce4e78e8c48b15b67d6b`. Worktree mtime refresh preserved content and modes. The carry was conflict-free with equal range-diff, and all 1,834 original current-base assertion bodies remained byte-identical. [Source proof](source-proof.json) records the eight authored files, three asserting files, immutable source maps, carry, runner and final restoration identity.

[Artifacts](artifacts.json) preserve captured executable hashes and Cargo artifact metadata. The test command compiled 13 executable artifacts; compilation of an example is not a claim that it ran. Five test executables and doctests were actually launched. Rust 1.96.0 reports `x86_64-apple-darwin`, and `file` identifies Mach-O x86_64 executables. Physical hardware and translation mode were not established. These are laptop results, with no device or product-latency qualification. Linux/ARM64 results are separate.

## Lossless output and decoding

[Commands](commands.json) records all 19 commands, including compiler identity and the post-run `file`/inventory queries. [Raw index](raw-index.json) covers all 38 stdout/stderr streams. Every human output byte is retained in [the JSON string envelope](human-streams.txt.json); only complete recognized Cargo machine JSON records are omitted, with their original stream/range hashes retained. Each full inventory appears once in this raw bundle, rather than being duplicated as another large name array. Actual test rows remain in the full human test output.

To decode the envelope, parse JSON and encode its `data` string as UTF-8 without adding a newline. Verify `decoded_bytes` and `decoded_sha256`. Raw-index `decoded_offset` values then address those decoded bytes. Each retained range must match its original source span and SHA256. Closing wrapper markers lie outside original bytes. Full Cargo machine streams, complete source maps and verifier scripts remain external at the recorded paths/hashes; their portability limit is explicit. No binary or script is included here.

[Validation](validation.json) and [SHA256SUMS](SHA256SUMS) seal the receipt. MAIN/CPU were released after terminal census and source checks; packaging reread captured evidence only.
