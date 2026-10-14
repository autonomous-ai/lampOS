# Task 31 rebase review

Read-only source review on the Darwin builder, against base
`dcc2e6b1ee67e11e2de89f893696432e47c4d8c5`. Compared the preserved
pre-rebase Task 31 files in
`/private/tmp/lamp-third-schedule-pre-rebase-dcc2e6b1e.tar` with the current
files and upstream's changes since `7fbb03e1f`.

No merge defect found in the requested Voice scope:

- Upstream's short-sound classification, `End.sound` propagation, optional
  endpoint record field, and waiting-pose suppression survive unchanged
  (`coordinator.rs:3793`, `:3808`, `:3822`, `:4182`). Ordinary endpoints retain
  `sound: false`.
- `ProviderInput::End` preserves default/omitted-false wire behavior
  (`provider_worker.rs:258`). `InputHandoff` still uses `ended_as_sound` for
  marked sounds and `ended` for speech (`:2483`).
- Reminder lineage selects `forward_reminder` (`provider_worker.rs:1184`,
  `:1302`). Reminder `InputStarted` does not call `RoomTold::heard` (`:1708`).
  The merged `Unanswered` arm skips both room updates and `UNANSWERED`
  increments for reminders while preserving upstream sound-versus-speech
  handling (`:1772`). Actual forwarded audio still clears `UNANSWERED` and
  calls `RoomTold::spoke` (`:1717`).
- Added regression coverage preserves omitted/true `End.sound` wire round
  trips (`provider_worker/tests/reminders.rs:96`) and distinguishes unanswered
  reminders, short sounds, genuine speech, and actual reminder audio (`:459`).
- Scanned 41 `ProviderInput::End` occurrences, including 29 complete
  shapes/initializers: none lacked `sound`. Cloud, fixture-provider, and the
  dated provider review test contain the expected upstream additions.

`git diff --check` passed. No Cargo or runtime tests were run by this reviewer;
the root builder owns those results. This review is source evidence only and
makes no claim about lamp behavior or native ARM64 qualification.

Reviewed SHA-256 values:

| File under `lampOS/crates/voice/src/` | SHA-256 |
| --- | --- |
| `coordinator.rs` | `70b0efbc0873df3a48542fb95b8794435393366ef1b526393aba169c79eef9ca` |
| `coordinator/cloud.rs` | `5527db8f8cddea4d20f560cdf61aa2bd3e854cb0e99761a4ca8b825e3bc5b44f` |
| `fixture_provider.rs` | `0df3856eb0ffddafb583c8cb6be0ddf10b9568db2d4853bc517d9b7057b631d9` |
| `provider_worker.rs` | `69c06a80c5f1a902718bbc38ea321ef97f4994379afc799248261dc1e14b22ef` |
| `provider_worker/tests/review_2026_10_14.rs` | `b41ab52fa316da58e2ce3fbb8a84ba0ade6c8591d08de5fdadbf393753d832a1` |
| `provider_worker/tests/reminders.rs` | `61bae532ae0f18b59a86592b99d987d3be70b31d227eb13ace3613320de952c0` |
| `room_told.rs` | `80bef90051ac6f59e2a88d27ddee981d576a447b21f23bd4d912fadb05f040a1` |
