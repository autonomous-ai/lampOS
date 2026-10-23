# Voice fixture events: terminal Linux CI and existing exact-base run

**Ready: no.** These native results do not waive the earlier shared-process
x86 or consumer command failures. Attribution, reviewer disposition and the
final current-parent qualification remain separate. No new CI run, rerun,
local Cargo command, source edit or lamp operation was performed for this audit.

PR [#3023](https://github.com/autonomous-ai/lampOS/pull/3023) authored head
`157b800a1c34af5e077607c43086ec6342a324fe` is based on `ae4ab0022a5716b8b393956d5e26fd1480aac350`. Its ARM64 and Clippy jobs both checked
synthetic merge `ae8167074c87b025bedbf48a0635350d828c7853`, whose parents are that base and authored head.
The checked and authored Git trees are identical:
`b9e010dff22b2984972bcc22d255bdea0e1e6b51`. The frozen comparison is against ae4ab0022; it is not a claim
about a later lamp-voice parent.

| Observation | Workflow / actual job | Actual checkout | Terminal result |
|---|---|---|---|
| Existing exact-base push, ARM run 6230 | [39873728333 / 120154110817](https://github.com/autonomous-ai/lampOS/actions/runs/39873728333/job/120154110817) | `ae4ab0022` itself | Both completed / success |
| Candidate PR, ARM run 6236 | [39875221807 / 120159079217](https://github.com/autonomous-ai/lampOS/actions/runs/39875221807/job/120159079217) | `ae8167074` merge of `157b800a1` into `ae4ab0022` | Both completed / success |
| Candidate PR, strict x86 Linux Clippy run 2238 | [39875221768 / 120159079166](https://github.com/autonomous-ai/lampOS/actions/runs/39875221768/job/120159079166) | Same `ae8167074` merge | Both completed / success |

All are attempt 1. A head-filtered query found one existing ARM run on the
exact base, the push above; it was already terminal and was downloaded once.
The separate Eyes PR is not an exact-base control and is not used as one here.
No workflow/job disagreement or head drift was observed for the candidate.

## What ran and what it shows

Both ARM logs record `PICKED` empty, release workspace tests, profile `pr`,
4-core parallel execution and partition `hash:1/1`. The candidate selector
chose all crates because the diff includes a non-Markdown verification file.
The base push naturally selected the whole workspace. Both ran:

```sh
cargo nextest run --locked --release --workspace --profile pr \
  --no-tests=pass -j num-cpus --partition hash:1/1 \
  -E 'not (binary_id(lamp-ipc::relay_signals) | binary_id(lamp-turn-end-wire::process))'
```

| Primary nextest result | Exact base | Candidate |
|---|---:|---:|
| Passed / failed / skipped | 10,799 / 0 / 67 | 10,799 / 0 / 67 |
| Summary duration | 187.700 s | 186.844 s |
| Voice library, including the leaky pass | 3,554 | 3,554 |
| Voice integration / main binary / examples | 301 / 1 / 10 | 301 / 1 / 10 |
| Voice total primary passes | 3,866 | 3,866 |

Both pass counts include the same **one LEAK**:
`same_voice::tests::each_line_the_check_says_is_said_to_its_journal` in
`lamp-voice`. Each side has 10,798 PASS rows plus that LEAK, with exactly
10,799 unique primary `(binary ID, test name)` identities. The sets are equal:
no added or removed executed identity. Starting inventories report 164
binaries and 2 skipped binaries; 144 distinct IDs have primary result rows.
The 67 skipped cases are not passes. The exact nextest configuration has the
same SHA-256 on base and authored head, recorded in `summary.json`.

The seven assigned cases pass on both sides:

| Case | Existing base | Candidate |
|---|---|---|
| `provider_worker::tests::requested_audio_crosses_the_live_worker_only_after_its_tool_reply` | PASS 0.052s | PASS 0.052s |
| `provider_worker::tests::policy_holds::a_timer_in_a_turn_that_follows_the_lamp_is_set_though_the_policy_said_stay_quiet` | PASS 0.108s | PASS 0.013s |
| `provider_worker::tests::reminders::a_reminder_rung_into_a_busy_room_is_no_answer_to_end_it_by` | PASS 0.028s | PASS 0.007s |
| `remember::tests::the_oldest_of_a_long_kept_list_is_found_and_the_rooms_are_marked` | PASS 0.007s | PASS 0.007s |
| `second_provider::worker::tests::stop_during_music_rpc_revokes_scope_and_finishes_without_waiting_for_the_reply` | PASS 0.010s | PASS 0.010s |
| `second_provider::worker::tests::the_lamps_own_answers_to_talk_keep_no_conversation_going` | PASS 0.007s | PASS 0.007s |
| `coordinator::tests::requested_audio::cancelled_gapped_or_expired_music_never_acknowledges_its_confirmation_news` | PASS 0.115s | PASS 0.115s |

The 40 selected Voice rows in the summary are all PASS on each side: the
seven assigned identities plus their affected DeclineMusic/warm-STT cases,
all 12 coordinator requested-audio cases and all 18 runtime cases, with
shared identities counted once. This is family coverage, not a claim that
every one of those test bodies was edited. The previously reported
`a_music_action_before_decline_keeps_its_single_offer_and_confirmation`
also passes on both sides; its exact rows are retained.

Each ARM job separately passed **4 doctests / 0 failed** (Bus 2, IPC 1,
Msgs 1), `relay_signals: ok`, and the process harness **12 / 0**. These are
separate from nextest. Offline publication prerequisite checks remain in
full logs and selected summary lines.
Clippy ran `cargo clippy --locked --workspace --all-targets -- -D warnings`.
Logs identify Rust 1.96.0, native `aarch64-unknown-linux-gnu` for ARM and
`x86_64-unknown-linux-gnu` for Clippy.

This is an **existing native pass/pass observation**, not a predeclared
shared-process macOS `cargo test --lib` pair. Nextest gives tests separate
processes. These two observations reproduce no failure and establish neither
a lower failure rate nor a latency improvement. The historical failed
parallel and consumer commands remain failed; the original records in
[#3023's source receipt](https://github.com/autonomous-ai/lampOS/blob/157b800a1c34af5e077607c43086ec6342a324fe/lampOS/docs/verification/voice-full-run-events-20261022/README.md)
and subsequent lamp-tests receipts remain applicable.
No separate Linux `cargo fmt` was run. Mac formatting and other local gates
belong to their existing receipts and were not rerun or upgraded here.

## Evidence and verification

- `raw-output.json` stores complete exact-base ARM, candidate ARM and candidate
  Clippy job logs as lossless UTF-8 strings, each with original bytes/SHA-256.
- Six complete workflow/job terminal responses, the base jobs listing and the
  head-filtered run listing are included byte-for-byte.
- `3023-checked-commit.json` proves merge parents/tree.
  `3023-artifacts.json` records the existing artifact archive's metadata;
  the ZIP was not downloaded. Its digest is not a verified hash for every
  compiled binary.
- `summary.json` includes checkout and command lines, all primary binary
  IDs/counts, selected exact test rows, separate harness counts and the
  canonical identity-set digest. These IDs name nextest targets, not
  byte-identical compiled test executables.
- `SHA256SUMS` covers every other appendix file. Run
  `shasum -a 256 -c SHA256SUMS` from this folder.

For every `logs` object in `raw-output.json`, UTF-8 encode its `text`; length
and SHA-256 must equal its `bytes` and `sha256`. This reconstructs original
logs without loss. Count primary rows only between the single Starting and
Summary lines named in `summary.json`, not repeated final output. No script,
binary, Cargo message stream or bulk source manifest is included.
