# Fourteen's work log

Fourteen works on `lamp-fourteen`, with one `fourteen/<task>` branch per
task, in this repository or, for a task's app side, in
`autonomous-ai/ecm-sds-mobile`. It is the app team's second builder, beside
`mobile`; the two share the app's repository and keep their branches apart.
The running list belongs to the first builder (the Queue issue, #1722,
continued from #705). Fourteen has no lamp; no result below is a result on a
lamp.

## Proposed tasks

| Id | Proposal | Answer |
|----|----------|--------|
| fourteen-P1 | Should a Lamp 2 get the app's V1 Enroll (My Voice/My Face) screen? Face is already done lampOS's own way (`eyes::who`, spoken consent); voice has no by-name equivalent at all (`same-voice` is turn-continuity, not identity); `face.enroll` has no lamp-side kind. Proposed instead, if anything: a read-only "who the lamp remembers" row with removal only, adding stays spoken. | — |

## Found, not mine

## 2026-10-20 — joined, first as `mobile2`

Ready for lamp-voice: yes (the log only; the role file is already there)

- Joined as `mobile2` at 10:18 and said so on the Queue,
  [#1722](https://github.com/autonomous-ai/lampOS/issues/1722) at
  [6282530882](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6282530882):
  "new builder mobile2, role file at <link>, no tasks yet".
- **Named `fourteen`** by the first builder at 10:21 on #1722, since
  joining.md asks for a name of its own rather than a suffix on another
  builder's. Its role file went onto `lamp-voice` as
  [#2144](https://github.com/autonomous-ai/lampOS/pull/2144), merged 10:22,
  as [builders/fourteen.md](../builders/fourteen.md).
- `lamp-fourteen` from `origin/lamp-voice` at `239f3f197`, replacing the
  `lamp-mobile2` branch the first commit was pushed on. Nothing a lamp runs
  is on it.
- No lamp is Fourteen's. lamp-0c89 is `mobile`'s (the first builder, 10:21).
- Where `mobile` is working in the app, so the branches stay apart (Mobile,
  10:2x on #1722): `ecm-sds-mobile` draft #7 (#1683's setup sheet and Health
  reports row) and #8 (the Skills page), then the name and time zone from
  the Lamp 2 page on #1982's kinds.

## 2026-10-20 — #2094, the app side — next

Ready for lamp-voice: no (not started; the work is in the app's repository)

- Given by the first builder at 10:21 on #1722, labelled `builder:fourteen`
  on [#2094](https://github.com/autonomous-ai/lampOS/issues/2094) beside
  `builder:el`: a person's own DeepSeek and Deepgram keys. El has the lamp
  side; the design is on #2094 (09:42) with the first builder's three
  additions.
- Wanted in the app (`ecm-sds-mobile` `v3`): the three optional setup
  fields; a keys screen with `keys.set`, `keys.remove` ("use the lamp's own
  again") and `keys.get`; both answers shown ("that key works" or "that key
  was refused"); and one line when a key is kept, that the lamp's requests
  then go to the person's provider, not through Autonomous.
- A key is never stored or logged here; the backend passes it through
  unlogged. Off until the lamp side's `LAMP_OWN_KEYS` is on. The messages'
  shape is agreed with El on #2094 before any code.
- **Claimed** at 10:4x, as the app side only, after El said here at 10:38
  that a second line is right for an issue with two sides:
  `taken by fourteen · session ef888428 · no lamp · the app side`
  ([6282775472](https://github.com/autonomous-ai/lampOS/issues/2094#issuecomment-6282775472)).
  El's `status:taken` for the lamp side is left as it is, and no label of
  another builder's is touched.
- In the same comment, the shape as I read it, and **eight things the app
  needs settled**, each with a proposal so a "yes" is enough: `keys.get`
  naming the host, `not on this lamp` while `LAMP_OWN_KEYS` is off,
  `request_id` echoed, nothing within 15 s, one sentence in both `error` and
  `detail.message` (#1673's convention), when a kept key takes effect,
  `keys.remove`'s own sentence, and no `base_url` for Deepgram.
- The lamp's sentences are shown in the app as given, not re-worded, so the
  app's words cannot drift from the lamp's and from the journal's host line.
  That is my answer to the first builder's addition 1 (09:44), which asked
  the app's builder for that line.
- Waiting on El's exact `cloud.md` contract lines, due when its
  `el/own-keys` PR is up (its 10:38). No app code before them; the flow is
  taken as El's 10:00, field names may still change.

## 2026-10-20 — #1396's two left items — settled, none mine

Ready for lamp-voice: no (nothing to build)

- The first builder asked Mobile at 10:34 to settle #1396's two left items
  with Fourteen, on #1396. Mobile posted the split at 10:4x: both stay with
  it — the Skills page (`ecm-sds-mobile`#8, on lamp-0c89) and the name and
  time zone (`mobile/lamp-v2-settings`, on #1982's `device.rename`,
  `person.name.set`, `timezone.set`) — and Fourteen takes #2094 whole.
- Agreed as posted, at
  [6282779089](https://github.com/autonomous-ai/lampOS/issues/1396#issuecomment-6282779089):
  I take neither, so nothing is handed over before Mobile's 11:30. In the
  app I work on `fourteen/*` only.

## 2026-10-20 — #2094, the app side — scouted the convention

Ready for lamp-voice: no (nothing on `lamp-voice` yet; this repository's log
only, no app code written)

- The first builder, 11:05 on #2094, to both El and Fourteen ("quiet since
  10:39"): asked El to post the `keys.*` messages' exact shape, and said
  Fourteen then builds the app's screen against it, with a fake lamp in the
  app's tests, while El builds the lamp side.
- Cloned `autonomous-ai/ecm-sds-mobile` at `v3` into scratch (outside this
  repository, per joining.md: the app side is not committed here).
- Read `mobile/lamp-v2-settings`'s `LampSettingsNotifier` as the convention
  to mirror: `BuddyMqttDataSource` over the device's own MQTT channel, a
  riverpod notifier whose `build()` probes each kind and whose `change()`
  waits for the lamp's result, "not on this lamp" already a first-class
  case, and `lamp_settings_test.dart` driving it against a fake data
  source. Posted the match at
  [6283169318](https://github.com/autonomous-ai/lampOS/issues/2094#issuecomment-6283169318):
  a `LampOwnKeysNotifier` the same shape, `"keys.set"`/`"keys.remove"`/
  `"keys.get"` as its kinds, a `lamp_own_keys_test.dart` against a fake
  lamp.
- Also read `mobile/lamp-v2-reports`'s change to `autonomous_os_setup_page.dart`
  (`_askShareReports`, a modal sheet, `shareReports` added to the setup
  payload) as the pattern for the setup wizard's three optional fields, once
  their own UI placement is agreed.
- Not written: the entity's fields and the notifier's `change()`/`build()`
  bodies. Those depend on El's exact `keys.*` field names and answer
  strings, still not posted; a guess there would be thrown away the moment
  the shape differs, so none is committed.

## 2026-10-20 — #2094, the app side — two pieces built and pushed

Ready for lamp-voice: no (the app side only; nothing here changes
`lampOS`'s own crates or docs beyond this log)

- El posted the `keys.*` contract on #2094 at 11:11, then the authoritative
  table at 11:13 ("where this differs from your 10:39 summary, the table
  above is the one the lamp takes"), with a two-minute overlap between the
  two: 11:11's `keys.get` success is a nested `{"uses","since","host"}` per
  service, 11:13's is a flat `"own"|"lamp"` string with "never a key". Built
  against 11:13. El's 11:24 then confirmed it in so many words: "The 11:13
  table is the shape", with the nested fields added on top, additively, not
  replacing it.
- Cloned `ecm-sds-mobile`, branch `fourteen/own-keys` from `v3` at
  `712f8c8e`. Opened
  [ecm-sds-mobile#11](https://github.com/autonomous-ai/ecm-sds-mobile/pull/11),
  not a draft, two commits:
  1. The setup wizard's three optional fields (`deepseek_api_key`,
     `deepseek_base_url`, `deepgram_api_key`), asked right after the wizard
     learns a device is a Lamp 2, added to `buildSetupPayload` the same way
     `share_reports` (lampOS #1683) was: included only when non-empty, so a
     blank sheet changes nothing in the body.
  2. `LampOwnKeysNotifier`: `keys.set`/`keys.remove`/`keys.get` over the
     device's own MQTT channel (`BuddyMqttDataSource`, the same transport
     `LampSettingsNotifier` uses for #1982). Treated all three as two-phase
     (`starting` then `success`/`failure`), including `keys.get` — the
     11:13 table shows it with only a `success` row, but El's 11:24 confirms
     it also acks `starting`; using `request()` there would have read that
     ack as the final answer and silently reported both services as the
     lamp's own. `LampOwnKeysReply.from`, a pure envelope-to-reply function
     mirroring `LampSettingsResult.from`, is what the tests exercise — the
     MQTT socket itself is not unit-tested here, same as every other kind
     in this codebase.
- Not in #11, named there as the next piece: the settings-tab row and the
  dedicated page. Both touch `autonomous_os_detail_page.dart` and its
  settings tab, which Mobile's unmerged `lamp-v2-settings`/`lamp-v2-skills`/
  `lamp-v2-reports` are all editing right now (each adds its own `part of`
  file and a conditional row). Starting that once at least one has landed,
  rather than adding a fourth concurrent edit to the same few lines.
- Also named as known, deliberate gaps in #11: `keys.get`'s `since`/
  `deepseek_host` and `keys.set`'s success `code` (El's 11:24 additions)
  aren't surfaced in `LampOwnKeys` yet; setup's `GET /api/device/setup/
  status`'s `own_keys` readback isn't wired into the setup success screen.
  Both are additive, not reshapes, per how `LampOwnKeys.fromKeysGet` reads
  only the two fields it knows.
- Gate, against clean `v3` at `712f8c8e`: `flutter analyze` 29/29, identical
  to `v3` line for line, 0 in any file this touches. `flutter test` 704
  passed / 1 skipped / 1 failed, the one failure the same pre-existing
  `test/features/brick/key_insight_period_test.dart` load error as clean
  `v3` (also in Mobile's #1396 log). My 19 new tests are part of the 704.
  Ran the full suite three times across both pieces (once showed two extra
  failures that did not reproduce in isolation or on a second run — full-
  suite flakiness under load, not a regression; the two reruns both matched
  baseline exactly).
- Not shown: a real Lamp 2. No lamp is Fourteen's.

## 2026-10-20 — #2094, the app side — all three pieces up on ecm-sds-mobile#11

Ready for lamp-voice: no (the app side is in `ecm-sds-mobile`; nothing a
lamp runs is on this branch)

- Mobile merged ecm-sds-mobile#8 (skills) and #9 (names & time zone) into
  `v3` at `c5cf58d2`, which also brought `domain/constants/lamp_answers.dart`
  (`isNotOnThisLamp`) onto `v3` for the first time. Rebased
  `fourteen/own-keys` onto it (clean, no conflicts); the local
  `lampOwnKeysIsNotOnThisLamp` duplicate stays for now (folding it into the
  shared helper is a one-line follow-up, not blocking) but is no longer
  needed to avoid a cross-branch build dependency.
- **Piece 3, the API keys row and page**, now buildable against the real
  merged `_LampSkillsRow`/`_LampSettingsRows` convention instead of an
  unmerged branch's diff: one settings-tab row ("API keys", Lamp 2 only,
  mirrors `_LampSkillsRow`'s gating) opening a page with one tile per
  service; each tile's sheet gives a key (and, for DeepSeek, its base URL)
  or says "use the lamp's own again". `keys.set`'s kept sentence is shown
  verbatim (`showSuccessAtTop`), never re-worded. No new tests: private
  widgets in a `part of` file, which this codebase tests only at the
  pure-logic layer (piece 2's), same as skills' and settings' own rows.
- **Found, not mine:** `autonomous_os_detail_page.dart` imports
  `ui/helpers/lamp_enrollment.dart` twice (lines 68, 85), pre-existing on
  `v3` at `c5cf58d2` (confirmed on the untouched ref). Left alone rather
  than editing a file three other branches are concurrently touching.
- **A measurement mistake, caught before it reached the PR:** a `git
  checkout origin/v3 -- .` partial-restore left my own new test files in
  place against the reverted source, misreading as "32 issues on v3" (3
  phantom errors from a function signature that no longer matched). Redid
  the comparison with an isolated `git worktree` on `origin/v3`: the real
  baseline is 29/29, matching exactly.
- **The full-suite flakiness from earlier ticks is confirmed environmental,
  not mine:** two separate full runs each failed a different, non-
  overlapping handful of files purely on "loading" (never the same files
  twice, besides the one known pre-existing one), and every failing file
  passed cleanly both in isolation and on a `flutter test -j 2` (lower
  concurrency) run: 714 passed / 1 skipped / 1 failed, the one failure
  `test/features/brick/key_insight_period_test.dart`, same as `v3`. That
  run is what ecm-sds-mobile#11's body reports.
- `gh pr edit 11` with the three-piece body, gate numbers, both gaps named
  (piece 2's) and the found-not-mine note.

## 2026-10-20 — #2094 — El's lamp side (#2167) read against ecm-sds-mobile#11

Ready for lamp-voice: no (reading only; no code changed)

- El's lamp side is up for review as
  [#2167](https://github.com/autonomous-ai/lampOS/pull/2167). Read the
  diff against my own three assumptions rather than take the prose alone:
  `keys.get` really does ack `starting` (`online.rs` tests `KEYS_KINDS` for
  all three kinds alike, no exception for `get` — confirms the
  `_awaitTerminal`-for-`get` choice was needed, not a stray guard); the
  setup field names match exactly (`deepseek_api_key`/`deepseek_base_url`/
  `deepgram_api_key`); `keys.get`'s real `done` payload carries
  `since`/`deepseek_host` beside the two flat strings, which
  `LampOwnKeys.fromKeysGet` already ignores harmlessly. Posted at
  [6283717118](https://github.com/autonomous-ai/lampOS/issues/2094#issuecomment-6283717118).
- No change needed in ecm-sds-mobile#11 from this read. The one open gap
  (setup's `own_keys` status readback) is confirmed real and precisely
  specified (`setup/src/serve.rs`'s `Status.own_keys`), not a guess.

## 2026-10-20 — fourteen-P1 — asked, not started

Ready for lamp-voice: no (a question, no code)

- Asked on the Queue,
  [#1722](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6283796390),
  whether a Lamp 2 should get the app's V1 Enroll screen
  (`AutonomousOsDeviceEntity.isLamp` explicitly excludes a Lamp 2 today,
  with its own comment that none of it is lampOS's).
- Read before asking, not just asking blind: `crates/eyes/src/who.rs`
  already does face recognition the spoken way ("remember my face"), kept
  only for those who asked; the app's `face.enroll`/`face.owners`/
  `face.remove` kinds have no lamp-side implementation (`cloud/src/wire.rs`,
  `answer.rs` use `"face.enroll"` only as a generic "not on this lamp"
  example). Voice has no by-name equivalent: `same-voice` and
  `voice/src/same_voice.rs` answer "same voice as the ongoing
  conversation" (#829), not "whose voice, by name"; `speaking_faces.rs`
  (#1917) ties a line to whose face is seen speaking, so personalization
  already rides on face recognition rather than a separate voiceprint.
- Proposed, if the answer is "something, but not that screen": a read-only
  row of who the lamp remembers (name only, from `eyes::who`'s list),
  removal only from the app, adding staying spoken.
- No task taken; `builder:fourteen` has no such label, and a lamp-side
  read (if wanted) is outside "Crates: none" in my role file either way.

## 2026-10-20 — fourteen-P1 — a third mechanism found, flagged

Ready for lamp-voice: no (a comment, no code)

- Mobile posted `faces.add_photo` (ecm-sds-mobile#13, lampOS #1968/#1977/
  #1979): a photo from the app, kept only once the person says yes at the
  lamp — also explicitly "Lamp 2 stays out of Enroll" there. A third
  mechanism alongside the two fourteen-P1 already named (V1's old
  `face.enroll`, lampOS's spoken `eyes::who`), and the closest of the three
  to `eyes::who`'s own consent shape (a photo substitutes for the live
  capture; the lamp still confirms).
- Posted the connection on
  [#1722](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6284300313)
  rather than open a second question: one answer likely covers both.

## 2026-10-20 — fourteen-P1 — the face half answers itself, narrowed to voice

Ready for lamp-voice: no (a comment, no code)

- Ten's [#2199](https://github.com/autonomous-ai/lampOS/pull/2199) (`face.list`/`face.remove`, `LAMP_FACES_PAGE=on`) is being tried on
  **lamp-0c89, a Lamp 2**, alongside ecm-sds-mobile#13's photo add. Mobile's
  "Lamp 2 stays out of Enroll" (12:4x) turns out to mean the old V1 row,
  not the feature: a Lamp 2 is getting face recognition from the app
  through a new path, not `_openEnrollmentPage`'s `isLamp` gate — the
  shape fourteen-P1 guessed, and already Mobile/Ten/El's to build.
- Said so on
  [#1722](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6284476124)
  rather than leave a stale half of the question for the first builder to
  answer: narrowed fourteen-P1 to just voice (a by-name equivalent, or
  none planned, as distinct from `same-voice`'s turn-continuity check).

## 2026-10-20 — #2094 — the `not_kept` failure code, piece 4

Ready for lamp-voice: no (the app side; nothing a lamp runs is on this
branch)

- El, 12:59 on #2094, to Fourteen: a new `data.code`, `not_kept`, on
  `keys.set` and setup's `own_keys` — the service took and checked the
  key but the lamp failed to write it, so the person's key for that
  service (old or new) is gone and the lamp has already fallen back to
  its own. The reviewer's P3 on #2167 caught that `unreached`'s "nothing
  changed" was untrue for this case.
- The message itself needed no change: `error.message` is already shown
  verbatim regardless of code. What needed fixing: `LampOwnKeysNotifier`'s
  cached `keys.get` state, which stayed stale after any failed `set` —
  correct for every OTHER failure code (nothing changed on the lamp), but
  wrong for this one. `set()` now calls `refresh()` specifically when
  `error.code == 'not_kept'`, before rethrowing, so the page doesn't go on
  showing "own" for a key the lamp just dropped.
- One new pure-logic test (`LampOwnKeysReply.from` on a `not_kept`
  envelope). Gate: `flutter analyze` 29/29 unchanged; `flutter test`
  715/1 skipped/1 failed (the one pre-existing failure), `-j 2` for a
  trustworthy number as before.
- Pushed to `fourteen/own-keys` (`6f50a00f`), PR body updated with piece
  4, replied on
  [#2094](https://github.com/autonomous-ai/lampOS/issues/2094#issuecomment-6284576267).

## 2026-10-20 — #2193 and #2194 — given, claimed, both blocked for now

Ready for lamp-voice: no (claims and a scoping note only)

- The first builder, 13:35 on #1722, idle-nudge: after #2094's app side,
  take the Lamp 2 page's restart
  ([#2193](https://github.com/autonomous-ai/lampOS/issues/2193), Fifth's
  lamp side) and update status
  ([#2194](https://github.com/autonomous-ai/lampOS/issues/2194),
  Migrate's) in the app. Mobile has faces (#13) and reminders (#2214), so
  the pages split cleanly. Build each against its kind's
  `crates/cloud/tests/app_contract.rs` pin as it lands, and say which
  first.
- Claimed both, app side, as second lines beside Fifth's and Migrate's
  (same two-sided-issue shape as #2094):
  [#2193](https://github.com/autonomous-ai/lampOS/issues/2193#issuecomment-6285008066),
  [#2194](https://github.com/autonomous-ai/lampOS/issues/2194#issuecomment-6285008467).
  Said which first on
  [#1722](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6285011269):
  **#2193**, since #2194 waits on #2188/#2189 (both open, Migrate's).
- Neither kind is in `app_contract.rs`'s `READ` list yet — no app code
  before each pins.
- **#2193 is further back than the issue assumes.** Fifth (06:22–06:24Z):
  lamp-cloud can't reach the keeper today (no socket/bus path, root-only
  CLI) and can't read lamp-update's lock either; proposed a new root
  socket/helper and keeper verb, and asked the first builder whether
  `system.reboot` should restart `lampos.target` or reboot the machine —
  unanswered as of this entry.
- What's not blocked: placement. A Lamp 2 shows no restart/power row at
  all today — `_buildMaintenanceCard`'s `hasPower = value.isLamp ||
  value.isInternPro` excludes it, the same `isLamp` gate fourteen-P1 found
  excluding Enroll. Said so on #2193: a Lamp-2-only row, its own MQTT
  widget (V1's `_LampPowerControls` is HTTP via
  `standToEarnRepositoryProvider`, not reusable transport-wise) — but its
  label waits on the machine-vs-target answer, so nothing written yet.
- Both #2193 and #2194 are, for now, waiting on others. #2094's app side
  is otherwise complete (four pieces on ecm-sds-mobile#11, awaiting
  review). Nothing else of mine is unblocked at this moment; said so
  rather than invent filler.

## 2026-10-20 — #2094 — folded the own-keys isNotOnThisLamp duplicate

Ready for lamp-voice: no (the app side; nothing a lamp runs is on this
branch)

- Mobile, 13:4x on #1722, confirming the #2193/#2194 split: pointed at
  `isNotOnThisLamp` from `domain/constants/lamp_answers.dart` as the
  convention to use — already on `v3` since #8/#9 merged, which is exactly
  what `lamp_own_keys.dart`'s own doc comment had promised to fold into
  once it landed. Did it: dropped the local
  `lampOwnKeysIsNotOnThisLamp` duplicate, imported the shared one in the
  notifier and the test.
- Gate: `flutter analyze` 29/29 unchanged. `flutter test`: isolated runs of
  every touched/flagged file pass cleanly (13/13 on
  `lamp_own_keys_test.dart` alone); full-suite runs keep showing a
  different, non-overlapping file or two failing to *load* each time —
  this run's extras were `resting_light_test.dart` (untouched by this
  branch) on one `-j 2` pass and `autonomous_os_chat_store_test.dart`
  (also untouched) on a fully serial `-j 1` pass, neither reproducing in
  isolation. Environmental on this machine, not this change — the only
  failure that ever reproduces alone is the one pre-existing
  `key_insight_period_test.dart`.
- Pushed to `fourteen/own-keys` (`35ccd5ca`). Replied to Mobile on
  [#1722](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6285152973).

## 2026-10-20 — #2193 — the app side built and pushed

Ready for lamp-voice: no (the app side; nothing a lamp runs is on this
branch)

- Fifth's lamp side landed as
  [#2223](https://github.com/autonomous-ai/lampOS/pull/2223) (not merged):
  `lamp-cloud`'s `system.reboot` asks the keeper
  (`lamp-keeper restart-asked`), refused while an update installs,
  otherwise `success {"restarting": true}` before the lamp goes down. The
  machine-vs-`lampos.target` question Fifth asked is still unanswered.
- Read #2223's diff in full before building, the same discipline as El's
  #2167: `answer.rs` routes `"system.reboot" => lamp.restart().map(Some)`
  through the same synchronous match arm as `system.info`/`volume.get`, no
  `starting` ack, confirming `system.reboot` is single-phase — unlike
  `keys.*`, `BuddyMqttDataSource.request()` is the right call, not
  `_awaitTerminal`.
- That reading also showed the wire contract (kind, empty request,
  `success`'s shape, the failure strings) doesn't depend on the open
  machine-vs-target question at all — only the lamp's own internal
  mechanism differs, invisible to the app. So nothing was actually
  blocking the build once #2223 existed; the earlier "waits on the
  answer" read was wrong, corrected here rather than silently dropped.
- Built `lib/src/features/device/domain/entities/lamp_restart.dart`,
  `.../notifiers/lamp_restart_notifier.dart`, and
  `.../pages/autonomous_os_detail_page_lamp_restart.dart`: a `_LampRestartControls`
  row beside V1's `_LampPowerControls` in General's Maintenance card, gated
  on `isLampV2`, reusing `_LampPowerButton`'s look over
  `BuddyMqttDataSource` instead of V1's HTTP
  `standToEarnRepositoryProvider`. No new tests — nothing of its own to
  unit test.
- Branched clean from `v3` at `c5cf58d2` (`fourteen/lamp-restart`), not
  stacked on ecm-sds-mobile#11 — a separate task from #2094, kept
  separate. (Caught and fixed a `git checkout -b` mistake that first
  stacked it on #11's branch; rebased `--onto origin/v3` before pushing.)
- Opened
  [ecm-sds-mobile#15](https://github.com/autonomous-ai/ecm-sds-mobile/pull/15),
  not a draft. Gate: `flutter analyze` 29/29 against clean `v3`;
  `flutter test -j 2` 695/1 skipped/1 failed, the one pre-existing
  `key_insight_period_test.dart` failure, matching `v3` exactly.
- Reported on
  [#2193](https://github.com/autonomous-ai/lampOS/issues/2193#issuecomment-6285336496)
  and
  [#1722](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6285339069).
- #2094's app side (ecm-sds-mobile#11, four pieces) still awaits review.
  #2194 stays blocked on #2188/#2189, both still open.

## 2026-10-20 — #2094 — ecm-sds-mobile#11 merged

Ready for lamp-voice: no (the app side; nothing a lamp runs is on this
branch)

- ecm-sds-mobile#11 merged into `v3` at 14:21, by its person. The app side
  of #2094 is live: setup's three fields, `LampOwnKeysNotifier`, the API
  keys screen, and the `not_kept` fix, all in one piece now.
- The issue stays open: the lamp side (#2167, El) is not merged yet, and
  #2094 isn't done end to end until both are. Said so on
  [#2094](https://github.com/autonomous-ai/lampOS/issues/2094#issuecomment-6285673215)
  rather than close it myself — not my call, and only half the task is in.

## 2026-10-20 — #2194 — Migrate claimed the lamp side, waiting on the pin

Ready for lamp-voice: no (acknowledgment only, no code)

- Migrate, 16:1x on
  [#2194](https://github.com/autonomous-ai/lampOS/issues/2194#issuecomment-6287588341):
  taken — `update.status` read from `lamp-update`'s own records, no new
  state, pinned in `app_contract.rs`; will say here when the pin lands.
- Acknowledged at
  [6287598346](https://github.com/autonomous-ai/lampOS/issues/2194#issuecomment-6287598346):
  waiting on the pin, no app code before it (same discipline as #2094).
  #2188/#2189 still show open as of this entry, though their PRs (#2225,
  #2249) are approved and queued to merge.

## 2026-10-20 — peer reads of ecm-sds-mobile#13 and #14

Ready for lamp-voice: no (review comments, no code of my own)

- The first builder, 16:1x on #1722 (while waiting on #2194's pin): peer
  read Mobile's ecm-sds-mobile#13 (photo faces, `faces.add_photo`/
  `face.list`/`face.remove`) and #14 (reminders, `schedule.list`/
  `schedule.cancel`), run under that repo's gates, findings posted there.
- Did not take either PR's claimed numbers on faith: checked out each
  branch (merged with `v3`'s current head) and ran `flutter analyze` and
  `flutter test` myself. Both matched exactly: 28/28 analyze; 758 (#13)
  and 730 (#14) tests passed, 1 skipped, 1 failed — the same pre-existing
  `key_insight_period_test.dart` failure on each, nothing new.
- Also checked the single-reply (no `starting` ack) assumption both PRs
  make against `lamp-voice`'s real `crates/cloud/src/answer.rs`, not just
  their own descriptions — `faces.add_photo`, `face.list`, `face.remove`,
  `schedule.list`, `schedule.cancel` are all in the same synchronous match
  arm as `system.info`/`system.reboot`. Confirmed, not assumed, the same
  discipline I'd want applied to my own #2193/#2094 work.
- Findings:
  [#13](https://github.com/autonomous-ai/ecm-sds-mobile/pull/13#issuecomment-6287714904) —
  one P3 (a cosmetic ordering inconsistency in `LampPhotoFaces.withAnswer`:
  the new face goes first in `sent` but last in `listed`), no P1/P2.
  [#14](https://github.com/autonomous-ai/ecm-sds-mobile/pull/14#issuecomment-6287726226) —
  clean, nothing to add.
- Reported done on
  [#1722](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6287732887).
- In the same stretch: first builder answered #2223's open question (a
  machine reboot, not `lampos.target`) — doesn't change ecm-sds-mobile#15,
  confirmed earlier that the wire contract is identical either way.

## 2026-10-20 — the lead's new rule: `Fixes #N` / `Part of #N` in every PR

Ready for lamp-voice: no (a convention to follow from now, no code)

- The first builder, 16:40 on #1722, to every builder (the lead's rule,
  #1711): a PR that finishes an issue says `Fixes #N`; one that does part
  of it says `Part of #N` and what's left; a "done" claim on an issue says
  what was delivered and what wasn't.
- Applied it to [ecm-sds-mobile#15](https://github.com/autonomous-ai/ecm-sds-mobile/pull/15):
  already said "Part of" for #2193, but as `lampOS#2193` — not the form
  GitHub cross-repo-links. Corrected to `autonomous-ai/lampOS#2193` and
  spelled out why it says "Part of", not "Fixes": a PR in this repo can't
  auto-close an issue in `lampOS` either way (`Fixes`/`Closes` only works
  within one repo, same as joining.md already says), so #2193 closes once
  the lamp side (#2223) is in too, not on this merge alone.
- Left [ecm-sds-mobile#11](https://github.com/autonomous-ai/ecm-sds-mobile/pull/11)
  as it is: already merged before the rule, and editing a closed PR's body
  changes nothing mechanical. Future PRs follow the rule from here.

## 2026-10-20 — #2194: a Lamp 2's software update status, read only

Ready for lamp-voice: yes (app side; depends on the lamp side, #2271, being
in `lamp-voice` — tracking separately, not blocking this report)

- Taken off Migrate's note on #2194: against #2271's `update.status`, read
  the pin in `app_contract.rs`
  (`the_software_update_is_answered_from_lamp_updates_status`) before
  writing anything, then confirmed from `answer.rs` that it answers inline
  (single-phase, like `system.reboot`) and from `round.rs`'s `Outcome::kind`
  that `last.outcome`'s wire names are a closed set of 10 — gave
  `LampUpdateOutcome.other` a defensive fallback for a name not in that
  set, never reached against the lamp as it stands.
- Built `LampUpdateStatus`/`LampUpdatePhase`/`LampUpdateOutcome`/
  `LampUpdateBuild`/`LampUpdateResult`, a notifier, and
  `_LampUpdateStatusRow`/`_LampUpdateStatusPage`, gated on `isLampV2`, read
  only (unlike V1's app-triggered `_SoftwareUpdateButton`).
- Cut the branch fresh from `v3` this time (`fourteen/update-status`), not
  stacked on `fourteen/lamp-restart` — the mistake from #2193 I corrected
  once already, not repeated.
- [ecm-sds-mobile#17](https://github.com/autonomous-ai/ecm-sds-mobile/pull/17),
  "Part of" #2194 (closes once #2271 is in `lamp-voice`). Reported on
  [#2194](https://github.com/autonomous-ai/lampOS/issues/2194#issuecomment-6288886434)
  and [#1722](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6288889747).
- Side finding while comparing gate numbers against a clean `origin/v3`
  worktree: `test/features/brick/key_insight_period_test.dart` isn't flaky
  as I'd assumed in earlier reports here — it's a real compile error
  (`addUnlockEvent` missing a required `scope` argument), already on v3's
  own tip, unrelated to any of my three PRs. Flagged on #1722, not fixed
  (not mine — "Crates: none" and not app code I touched either).

## 2026-10-20 — #2194 follow-up: `restoring` and `stale`'s real meaning

Ready for lamp-voice: yes (same PR, ecm-sds-mobile#17, pushed as a second
commit)

- Migrate, on #2194: #2271's review (`9c95ccb28`) widened `now` with a
  sixth value, `restoring` (a rollback), and spelled out what `stale`
  means — `lamp-update` hasn't republished within its own 180 s lifetime
  (said again every 60 s), so `now` shouldn't be believed. Field names
  and types otherwise unchanged.
- Added `restoring` as its own `LampUpdatePhase`, with its own row/page
  wording ("Rolling back to the previous build…"), not folded into
  `unknown`.
- Along the way, found `LampUpdateStatus.warns` had a doc comment
  claiming staleness warns on its own, but the code never actually
  checked `stale` — a bug from the first pass, not something Migrate's
  note introduced. Fixed to match the doc, so the row itself turns
  amber on a stale reply, not only the detail page's separate "Stale"
  tile.
- Two new tests (a stale reply warning mid-round, `restoring`'s own
  wording); 14/14. Replied on
  [#2194](https://github.com/autonomous-ai/lampOS/issues/2194#issuecomment-6294569062).

## 2026-10-20 — #2194: the follow-up needed its own PR

Ready for lamp-voice: yes (same as the two entries above)

- Mobile, 22:3x, on #2194: ecm-sds-mobile#17 had already merged into
  `v3` before my follow-up commit (restoring/stale) was pushed, so the
  merged PR doesn't carry it. My mistake — pushed to the branch without
  checking the PR had closed first.
- Opened [ecm-sds-mobile#19](https://github.com/autonomous-ai/ecm-sds-mobile/pull/19),
  same branch, onto `v3`, carrying only that one commit — checked via
  `gh pr view --json commits,files` that it's exactly the one commit and
  the two files it touches, not a re-diff of #17.
- Replied on [#2194](https://github.com/autonomous-ai/lampOS/issues/2194#issuecomment-6294640963).

## 2026-10-20 — #2194: flagged the no-lamp gap for end-to-end

Ready for lamp-voice: n/a (status note, no code)

- First builder's 02:09 roll-call, to Fourteen: ecm-sds-mobile#19
  (`restoring`/`stale`) waits on Mobile's read; #2194 end to end waits on
  Migrate's #2271 rebase into `lamp-voice`.
- Confirmed both still open (#19 unmerged, #2271 unmerged).
- Flagged on [#2194](https://github.com/autonomous-ai/lampOS/issues/2194#issuecomment-6299047655)
  that Fourteen has no lamp (role file), so the end-to-end pass needs
  whoever has one — same pairing shape as #1968 (Mobile + Ten) — rather
  than waiting to discover that after #2271 lands. Offered to write the
  exact steps for whoever runs it.

## 2026-10-20 — #2194: wrote the end-to-end steps for Mobile, applied Mobile's P3

Ready for lamp-voice: yes (same branch, ecm-sds-mobile#19, pushed as a
third commit)

- First builder, 02:13, to Fourteen and Mobile: write the end-to-end
  steps on #2194 now, not after #2271, marking which need #2271 so
  Mobile can run the rest today.
- Read the real source before writing anything (no crate edits —
  "Crates: none"): `round.rs`'s phase transitions by line number,
  `Outcome::kind`'s unchanged 10-value set, `quiet.rs`'s 45 s
  `--quiet-wait-s`, and `msgs/src/lib.rs`'s `UpdateStatus::EVERY`
  (60 s) / `LASTING` (180 s) — confirmed Migrate's secondhand numbers
  were exactly right. Confirmed `lamp-update once --apply on` forces a
  real round on demand (no waiting for the 900 s `serve` loop), and
  that `restoring` is inherently staged (a newly-switched unit has to
  fail its 60 s watch), not something flipped on demand.
- Posted the steps on [#2194](https://github.com/autonomous-ai/lampOS/issues/2194#issuecomment-6299314217):
  one today-runnable step (no row before #2271), the rest gated on
  #2271 being live with `LAMP_APP_UPDATE=on`.
- Also applied Mobile's P3 from the ecm-sds-mobile#19 read: `summary`
  now says "The lamp's updater hasn't said anything lately" when
  `stale`, instead of parroting the possibly-wrong phase. 14/14,
  analyze 28 (baseline). Noted on the PR.

## 2026-10-20 — took over #2271's rebase (lampOS#2360)

Ready for lamp-voice: yes (new PR, different scope than my app-side
work — "Crates: none" set aside for this one explicit takeover, per
the first builder's instruction, not a change to my role)

- First builder, 03:22: Fourteen take #2271's rebase over — Migrate
  silent since 22:33, `CONFLICTING`, and #2194's own steps wait on it.
  Rebase onto lamp-voice's head, keep Migrate as author, open as a new
  PR that replaces it, fold in the reviewer-lead's P2s if still open,
  ask for a re-read of only what changed.
- Checked the PR thread myself before touching anything: the
  reviewer-lead's two P2s (17:08) were already fixed on Migrate's own
  `9c95ccb28` (confirmed from their 22:30 comment) — nothing to fold in
  there. Their three small P3s from that same comment were left open,
  deliberately: a rebase-only takeover isn't the place to extend
  someone else's design on crates that aren't mine.
- New worktree (`fourteen/update-status-bus`), one rebase, two
  conflicts (`answer.rs`, `docs/cloud.md`), both additive, kept both
  sides. No Rust toolchain existed on this machine; installed one
  (`rustup`) to actually verify rather than push blind: `cargo fmt
  --check` clean, `clippy -D warnings` clean on the three crates,
  `cargo test` 538/0/1, and Migrate's own ARM64-red fix (the
  voice-eval bus-topics census test) passes.
- Opened [lampOS#2360](https://github.com/autonomous-ai/lampOS/pull/2360),
  closed #2271 as superseded (not merged — nothing of mine goes in
  without the reviewer-lead's and first builder's own merge). Reported
  on [#1722](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6300939260)
  and [#2194](https://github.com/autonomous-ai/lampOS/issues/2194#issuecomment-6300942465).
