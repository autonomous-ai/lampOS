# Plan: sticky face identity for per-person memory (keep SFace)

Status: **draft 2, revised 2026-11-19; built in #7933 to these rules (§8).**
For builder agents. On 20 Nov at 01:55 the plan's owner restored these
rules on #7933 ("I want all my original rule back"), overruling the first
builder's 19 Nov 20:00, 21:14 and 22:14 rulings.
The owner approved the goal and decisions (§2) on 2026-11-18. Draft 1 was
written against `ec8317fe2e` (16 Oct), 6,678 commits behind
`origin/lamp-voice`, and its root cause no longer matches the code. This
draft is checked against `origin/lamp-voice` at `8db1f3ef6f` (18 Nov).
Re-check line numbers before editing. Paths are under `lampOS/`.
Background: [face-recognition-v1-v2-comparison.md](face-recognition-v1-v2-comparison.md).

## 0. Review of draft 1

### What we keep

| Draft 1 part | Why it still holds |
|---|---|
| The problem: a known person is often treated as nobody, so their memory isn't reached | Still reported. The cause moved (§1), but the symptom is the same |
| §2 decisions: keep SFace, cut false rejection, single-user focus, no face check on confirmations, privacy rule unchanged | Owner's call; nothing in today's code contradicts them |
| §3 target behaviour | Still the right goal |
| T2: log each recognition, never a name | Partly exists (§1.4); the missing fields are still worth adding |
| T4: another known person switches only on 2 looks | The eyes still switch on a single print |
| T7: a lower bar for someone matched recently, set from data | Still useful; tune it from the T0 data |
| T8: better SFace input | Unchanged, later |
| §6 gates, §7 metrics | Kept, with one metric added |

### What is outdated

| Draft 1 claim | Today (`8db1f3ef6f`) |
|---|---|
| Memory uses `remember::person_now()` on every call (`remember.rs:448`) | `person_now()` (`remember.rs:6165`) is only a fallback when a call has no lease. Every heard line on the product route gets a per-line scope lease (`second_provider.rs:5270-5323`, `memory_scope.rs`) |
| Exchanges are journalled with whoever is current when they finish | `finished()` (`remember.rs:6373-6393`) files each exchange by its own `whose`, for Days and the legacy writer |
| `lamp-eyes` deletes the known file on any single miss | `LAMP_FACE_KNOWN_HOLD=on` (#4493) holds the name 4 s through a miss (`who.rs:1955`, `main.rs:2963-3058`) |
| `Presence::seen` switches person at once | It switches only after `SOMEONE_ELSE_AFTER` = 10 s (`known_faces.rs:4071`) |
| T1: switch memory to the session's person | The journal part is done. The read and keep parts are governed by the lease rules (§1.2), not `person_now()` |
| T5: follow the face between prints, new work | Mostly built: `LAMP_FACE_REFOUND=on` (followed ids, 0.5 s retry) and `LAMP_FACE_KNOWN_ON_ID` (#7424, built, **off** in the product) |
| T6: re-file a conversation's early lines, open design point | Built as `LAMP_MEMORY_FOLLOWS_FACE=on` with lamp-memory's `adopt` (`days.rs:2668-2677`), within limits (§1.5) |
| Code refs at `ec8317fe2e` | All line numbers in draft 1 are stale |

### What changes in this draft

1. **New T0, first: measure why lines go to the room.** No tool counts it
   today, and the most likely cause (an empty snapshot) writes no log line.
   Every later task is sized from this data.
2. **T1 is rewritten:** hold the conversation's person when the eyes'
   snapshot is empty, building on the existing `LAMP_FACE_GAP_HOLD` instead
   of `person_now()`.
3. **T3 is rewritten:** lengthen and widen the eyes' existing 4 s hold
   rather than build a new one.
4. **T5 becomes "turn on `LAMP_FACE_KNOWN_ON_ID` after its row".**
5. **T6 becomes "widen `FOLLOWS_FACE`".**
6. **New T9:** carried lines are never journalled, so a long conversation
   carried as the person's doesn't grow their memory.
7. Forget rules and private read-backs are stated as unchanged (§2).

## 1. How it works today, and where it fails

### 1.1 The path from a face to a memory scope

| Step | Code | Notes |
|---|---|---|
| Eyes print the near face every 2 s | `who.rs:1887` `RECOGNISE_EVERY` | SFace, match bar 0.363 |
| A match writes the `known` file; the name is held 4 s through a miss | `who.rs:1955` `KNOWN_HOLD`, `main.rs:2963-3058` | Within the hold, an unmatched print ≥ 0.30 to the held person on the same or a followed id keeps the name |
| The voice reads `known`; it is fresh for 3 s | `known_faces.rs:2049` `FRESH` | |
| Each heard line gets a lease from `FaceCache::snapshot` | `second_provider.rs:5270-5323`, `memory_scope.rs:419-441` | Empty once `known` is stale or gone |
| The lease's scope is narrowed by the per-line sight rules | `memory_scope.rs:829-843` | §1.2 |
| Recall, keep, forget and the journal use the lease's scope | `tools.rs`, `remember.rs:6373-6393` | |
| The session note says who is at the lamp, from the live `known` file | `known_faces.rs:4128` | The model can be told "you know X's face" while the line's scope is the room's |

### 1.2 Every way a line ends up the room's while a known person is there

| Cause | Switch (product) | Logged today |
|---|---|---|
| **Snapshot empty:** the eyes' 4 s hold lapsed, an unknown print under 0.30 or a new id let go, or nobody near | none | **No** (only with `LAMP_FACE_GAP_HOLD` on, `second_provider.rs:5290-5299`) |
| Another face seen speaking the line | `LAMP_SECOND_SPEAKING_FACE` on | `second_provider.rs:9690` |
| No face seen speaking it | `LAMP_PERSON_SCOPE_SEEN` on | `:9690`, `:9686` |
| One face in view, but under 3 looks, or another face in a look | `LAMP_PERSON_SCOPE_ONE_FACE` (+`_READS`) on | `:9649`, `:9655` |
| Another voice heard while the line is judged | one-face rule | `:10488` |
| Carry ended (another voice, or none heard) | `LAMP_CONVERSATION_CARRIES_PAST_SILENT` on | `:10426`, `:10469` |
| No eyes reader | — | `:9869` |
| Small or unsure faces aren't counted at the desk | `LAMP_FACES_AT_DESK_ONLY` on | counts only |

`LAMP_FACE_GAP_HOLD` (12 s, held lines never read memory) exists but is
**off** in the product (`memory_scope.rs:29-47`).

### 1.3 Most likely causes, to confirm with T0

1. **The empty snapshot.** 4 s of eyes' hold plus 3 s of freshness is about
   7 s. A head turned for longer, a print under 0.30, or a passer-by's
   unknown print drops the person, and it isn't logged.
2. **The per-line sight rules.** A line can be left as the room's even with
   the person in view: their mouth wasn't seen, there were too few looks,
   or another voice was heard.
3. **Carried lines are never journalled** (`:9738`, `:9778`, `:9790`). A
   conversation kept as the person's by the carry rules reads their memory,
   but doesn't add to it.

### 1.4 What the eyes log today

`lamp-eyes: recognition best {best} height {tallest:.3} …`
(`main.rs:2910-2913`) and a gap line (`:2954`). They don't log eye distance,
`turned`, YuNet score, second-best likeness or print time.

### 1.5 Late recognition today

`LAMP_MEMORY_FOLLOWS_FACE=on` (`follows_face.rs:1-28`). When a face is saved
or becomes known in a conversation that began as the room's, lamp-memory's
`adopt` moves the caller's earlier lines (room ids 1–64) to them
(`remember.rs:13863`, `:13936`, `:13991`). A line counts as the caller's by
voice print, or by one speaking face. The anchor line must have been seen
spoken by that one face. Logged at `:14084` and `:14006`/`:14058`.

## 2. Decisions (owner, 2026-11-18)

- **Keep OpenCV SFace.** V1's EdgeFace has a licence problem for commercial
  use. No model swap.
- **Cut false rejection.** Once someone has been recognised, the lamp should
  be hard to make lose them.
- **Single-user focus.** Someone else stepping in mid-conversation is rare.
  The accepted risk, in full: until another *known* face is matched or the
  conversation goes quiet, someone else at the lamp is answered from the
  current person's memory and can hear what it recalls, and their talk is
  kept as the current person's.
- **Confirmations are not changed.** No face check is added to any
  confirmation.
- **Privacy rule is not changed.** Only people who asked to be remembered
  are known. No pictures are kept, and no prints leave the lamp.

Added in draft 2, to keep the plan inside rulings already made:

- **Forgets keep today's rule.** A forget, correction or rule still needs the
  person's own face seen saying it, or the single-item yes (#5627,
  `tools.rs:2275-2398`). A held identity never runs a forget.
- **Private read-backs keep their own checks.** The food diary, mail,
  run-write read-backs and the like keep their "owner alone in view"
  checks. A held identity is never evidence that the owner is present or
  alone.
- **The no-asks rule (first builder, 18 Nov 18:41)** applies: nothing here
  adds a question to the person.
- **Memory changes are read by the memory owner (Friday)**, since they own
  lamp-memory.

## 3. Target behaviour

1. One match at any point in a conversation is enough to make it that
   person's: session, recall, keep and journal. Forget keeps its own rule.
2. Missed prints, no face, a turned head and unknown faces on the same
   followed face never drop a known person.
3. Only a confident match of a **different enrolled person** switches the
   person. Otherwise the scope falls back to the room only after the quiet
   rule: 2 min with nothing said, or 30 s with no face.

## 4. Tasks

Each task is one PR with its own tests and doc update, and its own switch,
on only when exactly `on`, until its row passes.

### T0. Count why a line goes to the room (first; independent)

- **Voice:** log the one silent case, a heard line admitted as the room's
  because the snapshot was empty, with the reason (stale `known`, no
  `known`, never known this conversation). Use the existing style of
  `second_provider.rs:9690`, with no name.
- **Eyes:** add to the recognition line (`main.rs:2910`) eye distance,
  `turned`, YuNet score, second-best likeness and print time. No name.
- **A daily count:** lines left as the room's while a known person was in
  the conversation, by reason (§1.2), counts only. Run it on lamp-06c1 for
  a day before T1.
- **Tests:** a formatter test for each line; no name in the output.
- **Docs:** `docs/eyes.md` "Who is there", `docs/memory.md` "Second route
  context scope".

### T1. Hold the conversation's person when the snapshot is empty (after T0)

- Build on `LAMP_FACE_GAP_HOLD` (`memory_scope.rs:29-47`), not
  `person_now()`.
  - Widen its window from 12 s to the session's rule: `GONE_AFTER` 30 s
    with no face, and `QUIET_BEFORE_ROOM` 2 min with nothing said
    (`known_faces.rs:4063`, `thread.rs:87`).
  - Hold only while no other known face is matched, and no other voice or
    face was seen speaking.
- Today a gap-held line never reads memory (`ScopeSnapshot::held`). Under
  the single-user decision, a held line **reads and keeps** as the person's
  and is journalled as theirs. It still runs **no forget, correction or
  rule**.
- **Its own switch** (for example `LAMP_FACE_HOLD_CONVERSATION`), or a
  widening of `LAMP_FACE_GAP_HOLD` named in the PR. Friday's read is
  needed.
- **Tests:**
  - (a) the person matched, then `known` gone for 25 s mid-conversation:
    recall and the journal stay theirs;
  - (b) no face for 31 s: the room's;
  - (c) another known person matched: theirs at once;
  - (d) a forget on a held line is refused as today;
  - (e) nobody ever known: unchanged.
- **Docs:** `docs/memory.md` "A known face lost for a moment", and the
  switches row.

### T2. Merged into T0

### T3. Lengthen the eyes' hold (after T0, with T1)

- `KNOWN_HOLD` is 4 s (`who.rs:1955`). Lengthen it to about 30 s while the
  held face stays on the same or a followed id, so `known` stays fresh with
  T1.
- **An unknown print on the held, followed face doesn't let go.** Only a
  new id, or nobody near for `GONE_AFTER`, clears the name. Today an
  unknown print under 0.30 lets go at once (`main.rs:2986-2992`).
- **A new unknown face away from the held place is nobody near there**
  (the owner's call on #7933, 20 Nov, `Held::keeps_away`). A print that
  matches nobody at the real bar, on a new id more than `JUMP_SHARE` of the
  held box's width from the held face's last box, keeps the name and
  renews nothing: no follow, no carry, so the hold still ends 30 s from
  the last sighting of the held face. That 30 s is the existing
  `STICKY_HOLD`, the owner-approved bound (20 Nov); no new window or cap.
  A new id within `JUMP_SHARE` follows or lets go as before, and a far
  face that matches someone goes through T4. lamp-06c1 at 443fea27: a
  covered held face (box 0.40 to 0.50 across) left the detector and a
  small face at 0.13 to 0.17 across, on a new id, let the name go.
- Size both from T0's data.
- **Tests:** match, misses, a print under 0.30 on the same id, then 10 s of
  no face, then a match: the name holds throughout. No face for 31 s
  clears it.
- **Docs:** `docs/eyes.md` "Held through a miss".

### T4. Only another known person switches, on 2 looks (with T3)

- Today a single print of person B switches (`who.known = found`,
  `main.rs:2992`). Require B matched on 2 looks within about 4 s, each
  above the bar and above A's likeness.
- The voice's `Presence` already waits 10 s (`known_faces.rs:4071`); leave
  it.
- **Tests:** one stray B print keeps A; two B prints switch to B.

### T5. Turn on `LAMP_FACE_KNOWN_ON_ID` (after T3)

- It's built (#7424) and off in the product: "The lamp keeps your name while
  you sit at the desk, through a moment it reads your face poorly."
- Run its row on lamp-06c1 with T0's counts before and after, then the
  switch-on PR.
- New code only if T0 shows a gap it doesn't cover.

### T6. Widen `FOLLOWS_FACE` for late recognition (after T1)

- `FOLLOWS_FACE` moves the caller's earlier lines only when the anchor line
  was seen spoken by one face, or a voice print matches (§1.5).
- Under the single-user decision, consider also moving a room-started
  conversation's lines when the person is first matched alone in view, with
  no other voice in that conversation.
- Friday decides, since it changes lamp-memory's `adopt` use. Propose on
  the PR before building.
- **Tests:** a room conversation, then a lone match after 2 exchanges:
  those exchanges move to the person.

### T7. A lower bar for someone matched recently (after T0 data)

- The eyes already use 0.30 to *keep* a held name (`HOLD_LIKENESS`). This
  task uses a lower bar to *match again* within N minutes of a match, only
  when no other enrolled person scores higher.
- Set the bar and N from T0's data on lamp-06c1, and record the numbers in
  the PR.
- Also measure false accepts between enrolled people in the same household,
  not only misses.
- **Tests:** 0.32 after a recent match counts; 0.32 with no recent match
  doesn't; 0.32 when B scores 0.33 doesn't.
- **Built:** a rematch is held, not a full match (§8). While someone is
  held, the lower bar may only match the one held again: a rematch of
  another person is not taken (not a match, not a takeover look, no let-go
  by itself); another person still needs T4's two looks (the owner's call,
  20 Nov, after leg 1b on lamp-06c1).

### T8. Better SFace input (later, unchanged)

- **Enrolment spread:** take the 5 prints at least ~0.5 s apart, reject a
  print with cosine > 0.9 to one already taken, and stretch the window
  (e.g. up to 8 s). Keep the `no_face` / `too_far` / `two_faces` answers.
  Update `known_faces::DEADLINE` and the model-facing words if the window
  changes.
- **Quality check before printing:**
  - minimum eye distance in px
  - `|turned|` limit
  - YuNet score
  - sharpness of the aligned crop
  - reject faces clipped by the frame edge

  A frame that fails is "can't tell": it never counts as a miss for T3/T5.
- **Resolution:** recognise from a 640×480 frame. Map YuNet marks back to
  that frame and align from it; detection stays on the 320 copy. Measure
  the decode and CPU cost on the lamp (A523) and keep the 250 ms look
  cadence.
- Each bullet can be its own PR.

### T9. Journal carried lines as the person's (after T0; Friday's call)

- Lines carried as a known person's conversation (`CONVERSATION_CARRIES`,
  `_CARRIED_MEMORY`) read their memory but are never journalled
  (`second_provider.rs:9738`, `:9778`, `:9790`). So their memory doesn't
  grow from them.
- Under the single-user decision, propose journalling them in the person's
  record. Keep "never forgets" as it is.
- Do it only if T0 shows carried lines are a large share.

## 5. Not in scope

- Replacing SFace, or adding a landmark or speaker model. CAM++ stays off
  for licence reasons.
- Any face or voice check on confirmations.
- A stranger bank, or recognising people who did not ask.
- Changing forget rules or private read-back checks.

## 6. Gates and rules for builders

- Rust only, English only in lampOS notes and docs.
- After code changes, update `docs/eyes.md` and/or `docs/memory.md` and the
  `docs/switches.md` rows to match the code.
- Narrow local gates; CI runs the full suites.
  - Voice: `cargo test -p lamp-voice <touched modules>` (for example
    `memory_scope::`, `known_faces::`, `second_provider::`, `remember::`).
  - Any change to `remember.rs`, `memory_scope.rs` or code `DaysClient`
    calls: also run the `lamp-memory` targets that drive the voice:
    `--test voice`, `--test voice_people`, `--test voice_submit`,
    `--test voice_timing`.
  - Eyes: `cargo test -p lamp-eyes`.
  - `cargo clippy -p <touched crate> --all-targets -- -D warnings` and
    `cargo fmt --check`.
- On the shared Mac:
  - Run cargo at `-j 3` / `--test-threads=4`.
  - Go through `~/.claude/friday-watch/gate-lock.sh`, and never run a
    crate's full suite under the lock.
  - Stop only PIDs you started.
- Lamp checks on lamp-06c1 need a posted hold. Put back anything installed
  for a test, and check `listening_ready`.
- Every switch is `on` only and off in the product until its row passes.
  Then a switch-on PR.
- Commit and PR rules as in `CLAUDE.md` / `lampOS/docs/joining.md`.

## 7. How we know it worked

Measured on lamp-06c1 with T0's counts, before T1 and after each step:

| Metric | Target |
|---|---|
| Lines left as the room's while a known person was in the conversation, by reason | Falls after each step; the "snapshot empty" share near 0 after T1+T3 |
| Share of a known person's turns whose recall and journal used their scope | ≥ 95 % |
| A known person treated as nobody while talking | None in a 10-minute session with natural head turns |
| A wrong person switch (B for A) | None in the same session |
| A forget run on a held line | None |
| Time from sitting down to first match | Report median and worst |

## 8. Build record (#7933)

Built in one PR, as the owner set it on #7933 (20 Nov, 01:55). That call
overrules the first builder's rulings of 19 Nov:

- **20:00:** a print under the match bar dropped the held name at once;
- **21:14:** no lower bar after a recent match (T7 removed);
- **22:14:** another enrolled person's first look dropped the held name.

The privacy HOLD's points of 15:45 that the owner's rules leave no room for
go with them: a 2 min total from the last match (P1), and a held name read
as theirs only with a near face in view (P3).
It also overrules #7814's condition that a line held with no face reads
nothing. The rows those rulings were drawn from (lamp-06c1, 19 Nov) are
kept on #7933: row 3 at 88ef4580 matched an unenrolled face as the owner at
0.33 in 2 of 63 prints; row 4 at ad548096 kept the owner's name 8.8 s on
another enrolled face. They are the accepted risk of §2, as the owner wrote it.

| Task | Switch (product) | Built as |
|---|---|---|
| T0 | none | `who::recognition_told`, `memory_scope::room_line` |
| T1 | `LAMP_FACE_NO_FACE_HOLD` (on) | `memory_scope::Gap::no_face`: 30 s with no face, 2 min quiet; the held line is `ScopeSnapshot::sticky`, read, kept and journalled as theirs, no forget, correction, rule or yes, never the owner |
| T3 | `LAMP_FACE_STICKY` (on) | `who::Held`: any print or none on the held face's own or followed id keeps the name; each sighting renews `STICKY_HOLD` (30 s); a new id or nobody near for 30 s clears it; a new unknown id away from the held box (past `JUMP_SHARE`) is nobody near at their place, renewing nothing (`Held::keeps_away`) |
| T4 | `LAMP_FACE_STICKY` | `who::sticky_look`, `who::Takeover`: one stray print of another enrolled person keeps the one held, two within 4 s switch |
| T5 | `LAMP_FACE_KNOWN_ON_ID` (on) | built in #7424; subsumed under T3 |
| T6 | `LAMP_FACE_STICKY` with `LAMP_MEMORY_FOLLOWS_FACE` | `follows_face::theirs_alone`: only a match anchors it |
| T7 | `LAMP_FACE_STICKY` | `who::Recent`: 0.30 within 10 min of a match, when nobody else scores higher. A rematch is held, not a full match (the held mark, `who::keep_found`): read and kept, no forget, correction or rule, never the owner, no `adopt` anchor; only `SAME_PERSON` is a real match (Friday's P1, 20 Nov 03:16). While someone is held, only that person: a rematch of another is not taken (`who::held_rematch`) |
| T9 | `LAMP_CARRIED_JOURNAL` (on) | `worker::keep_out_scoped` |

Also under `LAMP_FACE_STICKY`: a newly seen face (a new id, a face after
none, a box jump) is printed at once while a name is held (`who::NewFace`),
which only speeds recognition. T8 is not built.

### Row 5 finding (lamp-06c1, 20 Nov, at ba7984d3)

Legs E (lower face covered 30 s) and H (held anchor under a partial cover)
failed for one cause. The detector lost the covered face for 0.2 to 3 s at
a time, and each return was a new passing id: f43 to f50 in E, f19 in H.
A gap's print follows a new id only when it reads the last held print at
`SAME_PERSON`, and a covered print read it at 0.161 (E) and 0.103 (H).
So T3's "a new id clears" let the name go:

- **E:** the name went to another enrolled person on one print, at 0.364.
- **H:** it went to nobody for 6 s, and the voice's empty snapshot read that
  file correctly.

The gap line logged "kept on its id (sticky)" on those checks. It now says
what the check did (`who::gap_told`).

| | E (covered) | F-enrolled (a real swap) |
|---|---|---|
| Track id | new each return (f43 to f50) | the same (f22) |
| Best likeness (the other enrolled person) | 0.338 to 0.499 | 0.556 to 0.619 |
| The held person, at most (second best) | 0.066 to 0.171 | 0.177 to 0.212 |
| Alike to the last held print | 0.161 | 0.056 |
| Eye distance | 25.7 to 28.7 px | 15.7 to 16.4 px |
| Detector score | 0.60 to 0.81 | 0.85 to 0.89 |

Built from it:

- **On, no switch:** a T7 rematch never counts as a takeover look.
- **Behind its own switch, on in the product by the owner's call (20 Nov): `LAMP_FACE_FOLLOW_NEAR_NEW_ID`.**
  A new id in the held face's place (within `JUMP_SHARE` of its last box),
  with the sitter check saying the sitter is not standing up, is followed.
  It changes T3.
  **Follow gap (`LAMP_SESSION_PERSON`, on in the product, the memory
  owner's choice "B with 5s", 23 Nov).** With it on, the new id follows only
  within `who::FOLLOW_GAP`, 5 s from the last look that saw the face (every
  250 ms, `Held::saw_near_at`, not the 2 s recognition check): the 0.2 to
  3 s losses above, plus a margin. A new face sitting down in the place
  later is not followed, so it is never named as the held person. Off, any
  gap within the sticky hold follows, as before.
- **Behind its own switch, on in the product by the owner's call (20 Nov): `LAMP_FACE_TAKEOVER_SCORE`.**
  A takeover look needs a detector score of at least `PERSON_SCORE` (0.8).
  It is a new cap with a thin margin: one covered print scored 0.81.

The follow alone still lets E switch on two looks (0.499, then 0.391,
3.6 s apart), so E needs both switches.

Leg 4's bar overlap is information under T7, which says to measure the false
accepts. Own p05 was 0.288 against an unenrolled maximum of 0.316. 3 of 104
unenrolled prints became rematches, and each is written held.
