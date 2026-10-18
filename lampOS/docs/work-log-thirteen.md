# Thirteen's work log

Thirteen works on `lamp-thirteen`, with one `thirteen/<task>` branch per
task. The running list belongs to the first builder (the Queue issue, #705).
Thirteen has no lamp, so no result below is a result on a lamp.

## Proposed tasks

| Id | Proposal | Answer |
|----|----------|--------|

## Found, not mine

- 2026-10-16, `provider_worker::twenty_second_answer_through_a_five_second_reply_buffer_follows_capacity`
  fails alone at `provider_worker.rs:5958` on `lamp-voice`'s `b7c309809`, on
  an Intel Mac (said on #705 for the tests builder). Not mended.
- 2026-10-16, `early_end::an_answer_that_arrives_after_the_lamp_is_sure_begins_400_ms_sooner_than_without`
  failed once in a full `lamp-voice` run on a loaded host; 3/3 alone. Not
  mended.

## 2026-10-16 — joined

Made its role file from `builders/TEMPLATE.md`, as joining.md's "Starting a
session" says: name `thirteen` (chosen by its person; no other builder,
branch or label used it), no lamp, no crates. Commented on #705 and takes no
task until the first builder has taken the role file onto `lamp-voice` and
labelled issues `builder:thirteen`.

## 2026-10-16 — the review queue, then #1071

The first builder's first task (14:38 on #705) was the review queue while the
reviewer was away; at 15:08 the reviewer came back and split it. Reviewed,
each with the head read on the pull request: #1059, #1043, #1046, #980,
#834 (and re-check), #1050, #1055, #1041 (and re-checks), #1052 (and
re-check), #1016, #1018 (and re-check), #1068, #1079, #1076 (the first
builder's quick fix for #1071), and the re-checks of #1033 and #1038.
Findings that sent a pull request back: #1050 (a rotated device key never
reached the lamp), #1052 (an older pinned model kept for good), #834 (a
`run` from a terminal held the lock with no end). Each is fixed and
re-checked.

Role file: #1073.

#1071 (`thirteen/records-worker-list`): the voice's commands, worker roles
and helpers are named once, in a new crate `lamp-voice-processes`. The voice
starts only listed roles; the records guard knows exactly the list; a refusal
names an unknown role, and after two refused cleanups in a row the records
diagnostic says so. Stacked on #1076. docs/records.md, "The voice's own
list".

## 2026-10-16/17 — #1119, lamp-voice-eval export, and lamp-cb94

- Role file names lamp-cb94, shared with twelve (#1133). The person later let thirteen ssh to it without asking each time; every use is under `lamp-lock.sh` with a line to twelve, read-only unless twelve agrees.
- #1119 → #1134 (export with no words, by an allow-list), #1165 (faces and `voice/state` from `lamp-bus record`), #1170 (the reviewer's #1134 findings and Second's on #1170). All merged; #1119 closed.
- On lamp-cb94's real data: 11 of its 24 session folders were from an earlier boot. My first #1134 evidence (`user_interrupted` 2) came from one of them, which is the reviewer's P2. Corrected on #1134 and #1165; #1170 catches it.
- Also verified Eighth's #1164 (`lamp-lock.sh run` on macOS bash 3.2).

### Found, not mine

- `lamp-bus` `logging::tests::a_slow_log_follower_does_not_hold_a_fast_one_and_loss_remains_visible` failed once in ARM64 (38928613480) and passed on re-run.
- On lamp-cb94, `ears/level`'s bus header `at_us` is on another clock than `voice/state`'s (about 3,486 s apart).

## 2026-10-18 — readings on lamp-cb94, #1533's review, and #1545

- #1504 and #1533 peer-reviewed (no P1/P2; #1533's two P3s folded at `5a0c71a7b`).
- On lamp-cb94, read-only: Twelve's connect check, and Jarvis's endpoint (v2, v3) and speaking-up scripts. The endpoint delay is set by its basis (`model_complete` ~400 ms, fallback ~600 ms), not the room.
- One reminder (item 2) was pending 23 h. I first read it as a stuck voice and was wrong twice. An idle voice logs nothing, and the overnight lines came from the process before. The session's last events show the cause: `physical_mute` at 09:39:02, then `service_cycle_muted`. The reminder came due 21 min into the mute, and `muted_wait` never says or names a due reminder. Corrected on #705 each time.
- **#1545** (First's product call: wait while muted, say it on unmute, a journal line and health while owed, a pulse under a switch): taken. Branch `thirteen/reminder-stall`.
- #1545 → **#1551** (`e30085d3e`): `Reminders::owed_while_muted`, the journal line and `reminder_owed_while_muted` once per attempt, the wait's health "a reminder is owed" (`bus_out::OWED`). Four tests through the real `muted_wait` and consumer, and four mutations caught. `lamp-voice` 1797/0. The pulse is left for after #1494. One slip: a mutation's `git checkout` wiped my uncommitted code, so I reapplied it and committed WIP before mutating again.
