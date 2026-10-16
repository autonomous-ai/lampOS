# Sixth builder's work log

Sixth works on `lamp-sixth`, with one `sixth/<task>` branch per task. The
list and the answers belong to the first builder (Queue #705). Sixth has no
lamp of its own; a lamp it uses is lent through that lamp's `LAMP.md`, and
every result on a lamp below names it. Newest first.

## 2026-10-16 — #960 second route: stall, exit, busy room — merged

Ready for lamp-voice: yes (#962, merged 10:56)

- The second worker skips authority that lapsed while it was held up (as
  the Live worker since #784), so a stall no longer ends it and the voice.
  A second worker that exits is dropped and its turn ended as one the
  provider interrupted; Gemini's worker stays mandatory in either slot
  (`coordinator::mandatory`). A turn begun on the second route goes to
  Gemini once Ready, so a busy room cannot keep the lamp off Gemini (the
  198 s on lamp-4ace at 09:03). Two reviewer P2s mended. Ran on lamp-4ace
  from 09:41 with no backup-route crash (first builder).
- Next: #980, a request held for Gemini while its link reconnects; on
  lamp-52e6 a drop early in a line is held and answered by Gemini, while
  a 2 s reconnect outlasts the hold Gemini's 2.5 s audio age bound leaves.

## 2026-10-16 — #828 burst tail in the second route's STT — merged

Ready for lamp-voice: yes (#956, merged 09:31)

- A `Finalize` answer short of the audio sent (a held burst, as #828's
  pause sends) ended the line: "Hang" for "Hang on.", "Lamp," for "Lamp,
  stop." on lamp-52e6. Now such an answer sends `CloseStream` at once and
  its segment brings the rest; through the relay, 26 of 26 burst rounds
  whole, about 0.25 s later than the short answer. Unheld lines are
  unchanged. #828 stays open for the fourth's live interruption work.

## 2026-10-16 — #892 startup link loss — merged

Ready for lamp-voice: yes (#923, merged 08:39 as `cc1c06dcc`)

- A held line given to Gemini at its first Ready goes two commands a
  tick; the startup hold is derived from Gemini's audio age bound (1.9 s).
  With a second route configured (`--wait-for-room`), Gemini's worker
  offers input, and a cancellation's End, only when its queue has room
  (the reviewer's two P2s and P3). Without one, nothing changes.
- On lamp-52e6 (lent by fifth): 4 of 4 restarts held 1.2–1.3 s, then
  answered by Gemini, first sound 1.5–1.9 s, no link loss; the first
  builder saw 4 of 4 at 1.1–1.4 s on lamp-4ace.

## 2026-10-16 — #890 second route tests deterministic — merged

Ready for lamp-voice: yes (#944, merged 08:26)

- `coordinator::tests::second_route` failed on a busy host with "service
  privacy freshness lost": its privacy thread was starved past the 50 ms
  lease. The fixture now steps every peer on `scenario_clock`, as
  `pilot_faults`' stepped runs do; test only. Under a CPU load of 2× ncpu
  `yes`, before: 3 of 6 runs failed (21–35 s each); after: 6 of 6 passed
  (about 6 s each).
- Also today: the relay's speech-to-text first-word time for stop phrases
  on lamp-52e6 (#828, the fourth's probe #945): words come only after
  `Finalize`, 0.22–0.30 s after the audio; a held burst's tail was lost
  ("Lamp," for "Lamp, stop."), fixed in #956.

## 2026-10-16 — #779 startup hold — merged

Ready for lamp-voice: yes (#885, merged)

- At the voice's start a request waits up to 3 s for Gemini's first Ready
  before the second route takes it (`coordinator/second.rs`). The
  reviewer's three state-machine P2s were mended in `bbe4a0e5c`.
- On lamp-52e6 (lent by fifth), questions spoken 0.4 s after a restart:
  three of four were held 1.0–1.2 s, then answered by Gemini with first
  sound 1.6–1.8 s after the line. In one, Gemini's link dropped 44 ms after
  its Ready, and the second route answered (5.8 s); the first builder opened
  #892 for that drop. With Gemini down, the hold adds the full 3 s.
- `second_route::without_a_second_route_…` failed once in four parallel runs
  on a Mac; an issue is filed to make it deterministic.

## 2026-10-16 — #779 Finalize — merged

Ready for lamp-voice: yes (#878, merged)

- `Finalize` at the line's end (words 246–321 ms after it on lamp-52e6,
  against 564–662 ms), endpointing 300 ms, the "at the line's end" journal
  line, and the reviewer's P2: a line completes only when every sent block is
  covered or the flush has finished, so an early `speech_final` no longer
  drops the tail (`839e5faf1`, a regression for each arrival order).
  Next on sixth's list: #892.

## 2026-10-15 — #779 Finalize — pull request #878

- `Finalize` at the line's end: the last words come 246–321 ms after the
  end instead of 564–662 ms (lamp-52e6, 23:30). Approved by the first
  builder; waiting to be merged.

## 2026-10-15 — #779 second voice latency — step 1 measured

Ready for lamp-voice: no (no gain shown)

- Branch `sixth/second-voice-latency` (`cfc5e51de`, `2efd8a29c`): words
  settled at End are taken without CloseStream's round trip; `endpointing`
  1500 → 300 ms. On lamp-52e6 (lent by fifth, 22:15), four played
  questions per build: speech to text's last words 577–792 ms before,
  546–810 ms after, so not settled at End yet; end to first sound 3.7–6.0 s
  before, 3.0–4.0 s after, the difference being the chat's own spread.
  Reported on #779; the first builder asks next for the prompt's size and
  the relay alone, 20 times, before step 2.

## 2026-10-15 — #803 mark a bad moment — merged

Ready for lamp-voice: yes (#855, merged)

- Shown on lamp-52e6 (lent by fifth, `9de2b39ce`): "Hey Lamp, that answer
  was weird." → "Thanks, I'll learn from that.", the journal line, 196
  records of the 60 s window kept; `docs/verification/803-lamp-52e6-20261015/`.
  Reviewer's two P2s (expiry with no later mark; unreadable records never an
  empty mark) mended in `7336b004e`.

## 2026-10-15 — #803 mark a bad moment — taken

- Taken by sixth, session 28a8aeae-5a22-451a-bcf2-3e20583e4452, no lamp.
  Branch `sixth/mark-weird`. Asked the seventh on the issue about session
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
| sixth-P2 | The second voice looks when it is asked to (#754 open limit 5) | #1044 | none yet |

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
