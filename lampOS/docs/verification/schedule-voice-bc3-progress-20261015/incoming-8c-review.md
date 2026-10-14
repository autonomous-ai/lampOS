Incoming integration review, 2026-10-15

Frozen HEAD: `1a38f2f3e6b8afd1e7896761a0da6149a2400f67` plus uncommitted
Task 31. Incoming: `8c1d8eecde3c2416480c87ca6fd40de66bae1796`.
Source review only; no checkout, merge, source edit, Cargo, or device action.

One concrete incoming defect: early-end speech does not refresh news presence.

- In incoming `coordinator.rs:3543-3545`, a successful early End records
  `said_turn` for an utterance with at least 30 speech blocks but leaves the
  new `said_at_us` unset. At the eventual local endpoint, `early.confirm`
  returns `Some` (`:3687-3691`), so the only assignment to `said_at_us`
  (`:3696`) is skipped. News now requires that timestamp or a fresh face
  (`:4244-4248`). With early-end enabled and no camera presence, genuine
  conversations therefore cannot establish/refresh the two-minute presence
  window; news can remain silent from startup or after the last ordinary
  endpoint ages out. This is in fd1a273ee, independent of Task 31.
- Minimal correction belongs with First's news change: refresh the timestamp
  wherever the qualifying heard turn is recorded, using the existing
  `sent_at` for early End, or record it at confirmed local End independent
  of whether End was sent early. Keep restarts, short sounds and autonomous
  Written/Reminder turns from setting it.
- Needed regression: a qualifying early-ended heard utterance with no face
  opens/refreshes news presence; startup alone, a short sound and a spoken
  reminder alone do not. Existing early-end tests do not assert this new
  presence state. Do not alter timing assertions or timeouts to address it.

Integration seams:

- `git diff 1a38f2f3e..origin/lamp-voice -- <three Voice files> | git apply
  --check` passed, as did the equivalent check for `lampOS/crates/body`.
  These were read-only checks against the dirty frozen tree: no textual
  conflict found. Task 31's reminder insertion is separate from the three
  incoming coordinator edits.
- Preserve the distinct clocks: `said_at_us` is person-presence evidence;
  `last_turn_us` still gates news quiet time; Task 31's `last_speech_us` and
  fresh capture evidence still gate reminders. Reminder PCM must not renew
  `said_at_us`. Keep the existing order: promised camera look, SecondLook,
  reminder, greeting, task news.
- `agent_tasks` changes only the successful confirmation response from
  `started` to `handing_over` and its wording/test. Task 31 does not parse
  that response; CHECK declarations/routing and the reminder tool veto are
  unaffected. The failed-hand-over news uses existing Tasks dispatch
  `state`/`confirmed`/`finished` fields and stays in the background reader.
- The Body change is independent of Task 31: no Voice, Msgs, Schedule or
  Actions wire change. Its new `--look-at-name` flag defaults off. Added
  Body struct fields have no Task 31 construction sites. Linux-only Body
  startup still needs the final native build/test gate; host review cannot
  establish servo behavior.

After integrating once, focused checks from `lampOS/`:

```sh
cargo test -p lamp-voice task_news::tests
cargo test -p lamp-voice agent_tasks::tests
cargo test -p lamp-voice coordinator::tests::early_end
cargo test -p lamp-voice coordinator::tests::reminders
cargo test -p lamp-voice tasks_then_schedule_flags_keep_both_owners_and_their_declarations
cargo test -p lamp-body
```

Include the new presence regression if that upstream defect is repaired.
These commands were suggested, not executed. Incoming changes do not touch
the reminder provider/ledger paths; their frozen-gate evidence remains
useful, but it is not an exact-head pass. Finish the current gate as evidence
for its frozen source, batch accepted upstream integration, and run the
required full native workspace gate on the final head. No reason found to
repeat the full host gate after each individual upstream commit.
