# Sixth builder's work log

Sixth works on `lamp-sixth`, with one `sixth/<task>` branch per task. The
list and the answers belong to the first builder (Queue #705). Sixth has no
lamp of its own; a lamp it uses is lent through that lamp's `LAMP.md`, and
every result on a lamp below names it. Newest first.

## 2026-10-15 — #803 mark a bad moment — taken

- Taken by sixth, session 28a8aeae-5a22-451a-bcf2-3e20583e4452, no lamp.
  Branch `sixth/mark-weird`. The seventh asked on the issue about session
  records (#775); until answered, the narrowest reading: copy the window's
  records, no audio, nothing that rotates changed.

## 2026-10-15 — #754 second voice — merged

Ready for lamp-voice: yes (#778, merged 19:03 as `bd37eb808`)

- Merged by the first builder under the reviewer rule. Final head
  `5881a3f06`, 20 commits, each building on its own; ARM64 run
  38730179626 green on it.
- After the pull request opened, the reviewer found and sixth mended:
  history crossing persons (P1), a forgotten exchange coming back (P1),
  written turns losing their context (P2), a written turn's tool call
  counted as heard (P1), a pending written note not bound to its request
  and person (P1), the doc's lamp scope and 30 s queue (P3). Two rebases
  onto `lamp-voice` (#775's per-person sessions; keeper pause,
  `--addressed`, `Start::continues`) kept both sides.
- The switch-on minute on lamp-4ace is the first builder's. Latency
  follow-up: #779, after #803.

## 2026-10-15 — #754 second voice — pull request

Ready for lamp-voice: yes (#778), superseded by the entry above

- Task: #754, a second voice: the lamp still hears and answers when Gemini
  is down. Proposal, amendment (V1's config fields) and the raise beyond
  the switch are answered yes on the issue.
- Kind: off unless set (`LAMP_VOICE_SECOND_CONFIG`). Test that nothing
  changes without it:
  `coordinator::tests::second_route::without_a_second_route_the_packets_and_records_are_as_before`.
- Branch `sixth/second-voice`, 13 `lampOS:` commits on `origin/lamp-voice`,
  head `519197c5b`. Pull request #778.
- Gate on the Mac, from `lampOS/`: `cargo fmt --all --check` clean in every
  file touched (`crates/voice/src/agent_tasks.rs:396` is unformatted on
  `lamp-voice` itself); `cargo clippy -p lamp-voice --all-targets -- -D
  warnings` clean; `cargo build --workspace` clean; `cargo test -p
  lamp-voice` 988 passed, 17 failed, all in families that also fail on clean
  `lamp-voice` here (below); integration tests 193 passed.
- ARM64: run 38691709415 green on `cc935a7e9` (the same code before two
  test-only and format fixes); run 38691809959 on `f08d8eb79` failed in
  this task's own `a_spent_quota_moves_the_line…` (the run ended before the
  switch back), mended in `519197c5b`; run 38696765424 on it pending.
- On lamp-52e6 (voice lent by fifth, lamp-fifth `718a4a4ab` to `bb1fddc2e`),
  Gemini made unreachable, four played runs from a Mac beside the lamp. The
  first three found faults, each mended: the lamp's name heard as "Palin" or
  "Len" (now `keyterm=Lamp`), no history ("there" became the lamp's city),
  the relay's OpenAI speech not streaming (now ElevenLabs flash, 0.6 s to
  first audio), the level 6 dB low (now doubled). Run four: eight of eight
  answered, context kept, both light calls reached the executive; median
  4.3 s from the end of the line to the first sound, 3.0 to 3.6 s for a
  plain question. Raw readings: `docs/verification/754-lamp-52e6-20261015/`.
- Build for the lamp: `cargo zigbuild --release --target
  aarch64-unknown-linux-gnu.2.36 -p lamp-voice` on the Mac, SHA-256
  `9d10737d6cfcc82c1b6e7b111474701fe98df0d8ce173227d542385d1e988af9`; it ran on lamp-52e6 like the
  Docker build before it (all workers up, every library found). First
  install of a zigbuild binary on a lamp.
- Not shown: the agreed latency targets (proposed as #779, `sixth-P1`); a
  switch mid-line and Gemini's return played on a lamp (host tests only).

## 2026-10-15 — #754 second voice — taken

- Taken by sixth, session 69f1022b-ffa2-4b4f-a641-6093f9d1aad2, no lamp.
  Relabelled from `builder:third` by the first builder.

## Proposed tasks

| Id | What | Issue | Answer |
|---|---|---|---|
| sixth-P1 | The second voice's first words under 1.5 s | #779 | none yet |

## Found, not mine

- On this Mac, clean `lamp-voice` fails 25 tests of `lamp-voice` in a full
  parallel run (`coordinator::motor::*`, `coordinator::optional_ring*`,
  `provider_worker::tests::reminders::*`,
  `provider_worker::tests::review_2026_10_14::*`); they pass one at a time.
  Seen 2026-10-15 at `origin/lamp-voice` of that morning.
- `lamp-policy`'s `tests/program.rs:363`
  (`the_program_judges_what_the_voice_hears_and_prints_nothing_of_it`) failed
  once on the ARM64 runner (run 38686359454's first attempt) with no
  change to the crate; seventh's run 38686608332 failed the same hour in
  `tests/policy.rs:727`.
- `.github/workflows/lampos-arm64.yml` did not list `sixth/**` in the
  morning; runs were started by hand until a push run appeared.
