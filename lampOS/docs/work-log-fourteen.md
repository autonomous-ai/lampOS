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
| fourteen-P13 | A skill an owner made (#2813/#2814) can be sent to the Store, so other owners can add it: the lamp's `skills.share {name}` packs a made skill that's on and hands it to the Store's upload (Seventh); the Store holds it for review as #2802 does, signs and lists it only once approved (Backend3); the app's "Share to the Store" row with a plain warning first, then waiting, listed or refused (Fourteen). Owner's act from the app only, never by voice. Open for the lead: whether and when, who reviews, whether the author is shown. Posted on the Queue 30 Oct 12:1x; Fourteen leads it on its owner's word. | — |
| fourteen-P14 | A check that skills *work* on a lamp, not only install and load: a reminder to drink water is spoken on time (`reminders`); `sip-of-water` speaks after 90 min at the desk; the Store's voice-called skills act when asked (`look-for-things` looks and speaks, `who-was-here` answers); a made skill acts with the light and head only, never speech. On lamp-9adf with a person at the desk, after the office's 12:00–14:00 quiet. | — |

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

## 2026-10-20 — lampOS#2360 merged into lamp-voice

Ready for lamp-voice: yes (already in)

- Merged at `c902a95cf` (lamp-voice head `d90cc8b8e`). Rebased twice
  more after the first push (lamp-voice kept moving — `skills.install`
  landed, then others) before the merge; same two files each time,
  same kept-both-sides resolution, re-verified with the full gate each
  time (see the PR's own comments for each run's numbers).
- Mobile: installs the build on lamp-0c89 with `LAMP_APP_UPDATE=on`
  once its ARM64 run is green, then runs #2194's steps 1–8.
- #2194's app-side (ecm-sds-mobile#17 + #19) and lamp-side are both in
  now; what's left is Mobile's end-to-end pass on real hardware.

## 2026-10-21 — #2194 closed on its code

Ready for lamp-voice: yes (already in, see above)

- First builder, 07:15: closed #2194 on its code. Both sides are in —
  ecm-sds-mobile#17+#19 (app) and lampOS#2360 (lamp, merged into
  lamp-voice at `d90cc8b8e`). Mobile's hardware try on lamp-0c89 goes
  on #1396 instead, not #2194.
- Nothing further from Fourteen here unless #1396's try turns up a gap
  in the app side.

## 2026-10-21 — owner of three build rows (docs/build-rows.md, #2405)

Ready for lamp-voice: n/a (status/roadmap, no code yet)

- The lead's priority for today: every row under 1.0 gets merged **and
  tried on a lamp**. First builder assigned Fourteen three rows (07:44):
  - **A person's own keys (0.45), with El.** Both sides already merged
    (ecm-sds-mobile#11, #2167). Nothing left on my side; El is running
    the lamp try. Co-signed El's plan on #1722.
  - **Change the Wi-Fi, name or time zone after setup (0.75), with
    Nine.** Name/timezone (#1982) is merged and tried (#2038's
    automated trial). Wi-Fi (#1995, #2033) is merged but I see no
    on-lamp try yet of its kept/rollback safety. No lamp access here —
    asked Nine on #1722 whether it's theirs to run or goes in
    `lamp-runs.md`.
  - **A Skills Store to publish skills (0.25).** The lamp's own half
    is built (#1698's package format, #1983's `skills.install` from a
    URL). What's missing is the Store itself — no catalog publishes a
    package anywhere yet, and the app has no browse page. Flagged this
    as a scope/roadmap question on #1722 (hosted catalog in scope for
    1.0, or deferred like "Reads your mood" was to V2?) rather than
    claim an issue that doesn't exist yet — nothing to build against
    until that's answered.
- Posted all three on [#1722](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6305189004),
  within the lead's 09:00 GMT+7 deadline.

## 2026-10-21 — LAMP_APP_UPDATE's switches.md row (#2423)

Ready for lamp-voice: yes

- Reviewer-lead's P3 on #2412: switches.md was missing LAMP_APP_UPDATE
  and LAMP_OWN_KEYS (El's, separately). Added one row, same columns
  and conventions as the other lamp-cloud app-page switches, counted
  as `until`.
- `cargo test -p lamp-voice-eval --test it switches:: -- --nocapture`:
  226 switches total (was 225), counted correctly under "Other parts."
- Opened [lampOS#2423](https://github.com/autonomous-ai/lampOS/pull/2423),
  reported on [#1722](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6305807894).

## 2026-10-21 — resolved all three open PR conflicts for the user

Ready for lamp-voice: n/a (app-side, status)

- The user asked to check every open PR of mine for conflicts and
  resolve them so they could merge. Checked all three:
  - ecm-sds-mobile#15 (restart, #2193): CONFLICTING — v3 had moved a
    long way since this was last touched. Rebased, same two files as
    every other #2194-era conflict, kept both sides. analyze 24 (v3's
    current baseline, confirmed in an isolated worktree), test -j2
    812/0/1. Now CLEAN.
  - ecm-sds-mobile#19 (restoring/stale): already CLEAN, no action.
  - lampOS#2423 (switches.md): already CLEAN, no action.
- Also noticed in passing: v3's own brick-test compile error (flagged
  on #1722 a while back) is fixed now (Mobile's #18, merged) — the
  full suite is clean with no known failures at all.

## 2026-10-21 — missed a review on #2423: no attribution lines in lampOS PRs

Ready for lamp-voice: n/a

- Reviewer, 08:35 on #2423 (missed until the reviewer-lead's 09:5x
  follow-up): lampOS PR bodies and commits carry no attribution lines
  (`docs/joining.md`) — mine ended with `Co-Authored-By: Claude …`,
  carried over from ecm-sds-mobile's different convention. Fixed the
  body; the commit message never had it. Checked #2360 too — clean.
- This is a standing rule for every lampOS PR from here on, not a
  one-off fix.

## 2026-10-21 — #2194's real end-to-end: passed, one P3 checked and not mine alone to fix

Ready for lamp-voice: yes (already in; this is verification, not new code)

- Mobile ran the real device trial on lamp-0c89 (#1396): "No update
  published yet", "Up to date", and "stale"'s wording + clearing on
  `lamp-update.service` stop/start — all pass exactly as written.
  `checking`/`fetching`/`switching`/`watching`/`restoring` still need a
  real second build on the channel, not run yet.
- Mobile's P3: reopening the update page shows the last fetched value,
  only pull-to-refresh asks the lamp again. Checked against
  `_LampOwnKeysRow`, `_LampSchedulesRow` and `_LampFacesRow` — all
  three share the exact same pattern (plain `Navigator.push`, no
  invalidate; the row stays mounted behind the pushed page so its
  provider is never disposed). Not a gap specific to update_status —
  it's the app's existing convention for every read-only device page.
  Flagged this on [#2194](https://github.com/autonomous-ai/lampOS/issues/2194#issuecomment-6307552573)
  rather than quietly patching only mine, since fixing it would be a
  cross-cutting UX call across 4+ pages.

## 2026-10-21 — #2194 fully verified on real hardware, every phase

Ready for lamp-voice: yes (already in; closing the loop)

- Mobile ran the middle phases on lamp-0c89 by hand-installing a real
  build-4685: `fetching`, `switching`, `watching`, `restoring`, then
  `idle`/`rolled-back` — all seen correctly in the app, in the right
  warn colours. Combined with yesterday's `idle`/`up-to-date`/`stale`,
  that's every phase and outcome except `checking` (too fast on a 1 s
  poll, expected). The rollback itself was triggered by a real,
  unrelated lamp-voice bug in 4685 (a privacy-worker crash during
  restart) — not anything in #2194's app or lamp-cloud code.
- Nothing left to verify on #2194 from the app side. Thanked Mobile on
  [the issue](https://github.com/autonomous-ai/lampOS/issues/2194#issuecomment-6309445475).

## 2026-10-21 — built the Wi-Fi row Mobile found missing (ecm-sds-mobile#21)

Ready for lamp-voice: n/a (app-side; the lamp side, #1959/#1995, has
been in since 20 Oct)

- Mobile's 13:5x on #1722: lamp-0c89's "Change the Wi-Fi" row couldn't
  run today because the app has no `wifi.set` at all — only
  timezone/name/rename. My earlier 13:1x status comment claiming
  Wi-Fi was already covered was wrong; corrected it. Claimed the gap
  (Mobile offered either of us take it).
- Before writing anything: spawned a fork to read the real
  `crates/cloud/src/settings.rs` (`wifi_line`, `Changes::start_wifi`)
  for whether the same empty-probe trick used for timezone/name/rename
  is safe here too. It isn't as-is — an empty-string `ssid` is a VALID
  shape the lamp would act on, not a safe refusal, unlike those three
  single-value kinds. Only a probe with no `ssid` key at all refuses
  before any write. Built the entity/notifier around that finding,
  not around assumed symmetry with the existing kinds.
- Own notifier (not folded into `LampSettingsNotifier`): the wait
  needs care, since root's helper takes up to 180 s plus 60 s for the
  network to settle, and the app's own MQTT link to the lamp
  necessarily drops for part of that — confirmed
  `BuddyMqttDataSource.envelopes` is connection-agnostic (survives a
  reconnect) before trusting a 4-minute wait on it. Added
  `LampWifiChangeTimedOut` as its own case, since a timeout here is
  genuinely ambiguous (the change may have gone through while the app
  couldn't see it), not a plain failure.
- 18 new tests (entity parsing/validation only, matching this app's
  own convention — no MQTT plumbing test, same as `lamp_own_keys` and
  the rest). analyze 24 (today's real baseline), test -j2 830/0.
- Opened [ecm-sds-mobile#21](https://github.com/autonomous-ai/ecm-sds-mobile/pull/21),
  flagged the timeout wording as my own judgment call with nothing to
  match it against. Reported on
  [#1722](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6309884814).

## 21 Oct — #21's two P2s: a `request_id`, and a useful timeout fallback

- Mobile's review on [ecm-sds-mobile#21](https://github.com/autonomous-ai/ecm-sds-mobile/pull/21)
  (merged) found no P1, two P2s. Mobile merged it and asked for the
  P2s as a follow-up on `v3`.
- **No `request_id`:** the FD channel carries every phone's `wifi.set`
  traffic, and `_ask` took the first reply of the kind with no check —
  another phone's probe, or a redelivered older result, could read as
  this change's own answer. Added a `request_id` per call
  (`docs/cloud.md`: optional, 1–64 printable chars, echoed back in
  both `success.data` and `failure.data`); `LampWifiChangeResult.from`
  now requires the echo to match before it reads an envelope as this
  request's own.
- **A retry after the timeout had nothing better to go on:** per the
  review's own suggestion, a timeout now asks `system.network`
  (single-phase, already-safe) and tells the person the lamp's
  current ssid before they decide whether to retry, instead of only
  "no word came back." Also dropped that message's claim about
  getting "back on a network that reaches it" — the phone's link is
  the cloud broker, which doesn't change with the lamp's own Wi-Fi
  (the review's own P3 note); only fixed since I was already
  rewriting that exact block for the P2, not scope creep beyond it.
- Both the probe and the real change share `_ask`, so both now carry
  a `request_id` — `LampWifiChangeKind.probe` itself (the `{}`
  constant) is untouched; the id is only added to a copy at publish
  time, so the "no ssid/password keys at all" safety invariant still
  holds.
- 2 new tests (a mismatched `request_id`, and a reply with none at
  all — both never matched): 20/20 on the file. `flutter analyze` on
  the 4 changed files: clean. Full suite: 845/0.
- Opened [ecm-sds-mobile#23](https://github.com/autonomous-ai/ecm-sds-mobile/pull/23)
  onto `v3`. Reported to Mobile on
  [#1722](https://github.com/autonomous-ai/lampOS/issues/1722#issuecomment-6311782638).

## 21 Oct — one Wi-Fi row, and #1722 hit its comment cap

- The first builder's 16:5x ask, from Mobile's 16:3x note on #1959: merge
  the Interaction card's Lamp-2-only `_LampWifiRow` into Device info's
  existing (every-device) "Wi-Fi" row instead of carrying two. Modeled
  the new `_LampWifiInfoRow` on the same file's `_TimezoneInfoRow` — a
  plain info row plus a trailing "Edit" `TextButton`, shown only once
  `lampWifiChangeProvider` confirms the lamp takes `wifi.set`. The
  change sheet itself (`_LampWifiChangeSheet`) isn't touched by this
  diff at all, as asked.
- Checked against Mobile's own open #26 (the sheet-close fix) before
  starting: #26 only enters `_LampWifiChangeSheetState`, which my diff
  never does, so there's no textual overlap either order merges — went
  ahead on current `v3` rather than waiting on #26.
- `flutter analyze`: 24, matching `v3`'s baseline (none in the 3
  changed files). `flutter test -j 2`: 858/0 (the default-concurrency
  run flakes on unrelated `device_chat`/`agent_skills` files under
  load — confirmed by running them alone and under `-j 2`, not from
  this diff).
- Opened [ecm-sds-mobile#27](https://github.com/autonomous-ai/ecm-sds-mobile/pull/27)
  onto `v3`.
- **#1722 hit GitHub's 2500-comment cap** while I was posting the
  report — commenting is now refused there. The first builder (or
  lead) had already opened
  [#2602](https://github.com/autonomous-ai/lampOS/issues/2602)
  ("Queue (continued from #1722)") moments earlier; posted the #27
  report there instead. Armed a second Monitor on #2602 beside the
  existing one, since the running Monitor's own polling script isn't
  something I can edit to add the new issue number.

## 21 Oct — a live `system.info` reply decides V1 vs Lamp 2

- The first builder's 17:34 ask on #2602, after Backend3/Mobile's
  exchange about a lamp moved back to V1: today's page-level gate
  (`_isLampV2` in `autonomous_os_detail_page.dart`) was a sticky OR of
  the backend record's `os_generation` and `deviceGenerationProvider`'s
  own "ever heard" memory — neither ever clears once a lamp has said
  `lamp-v2`, so a rollback kept showing the Lamp 2 page forever.
- Fixed by letting this cycle's live `system.info` probe decide
  outright whenever it has an answer: V1 sends no `generation` at all,
  so its absence is itself the signal, not an unknown to fall back
  past. Only without a live answer yet (loading, or the device silent
  this poll) does it fall back to the old record/sticky OR, so the
  page doesn't flicker to V1 for one missed probe.
- Extracted the decision into a pure `isLampV2Page()` in
  `device_presence_provider.dart`, next to the existing `osPresenceOf`
  — same reasoning: testable without a widget or a container. 6 new
  tests on it, covering both reply shapes plus a record that says V2
  while the reply has none.
- `flutter analyze`: 24, matching `v3`'s baseline. `flutter test -j 2`:
  864/0.
- Opened [ecm-sds-mobile#28](https://github.com/autonomous-ai/ecm-sds-mobile/pull/28)
  onto `v3`. Reported on
  [#2602](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6314038092).
- Caught and fixed a posting mistake of my own: two of today's #2602
  comments (this one and #27's) went out with a literal backslash
  before every backtick — a single-quoted heredoc doesn't need (or
  honor) `\`` as an escape, so it posted the backslash verbatim.
  Edited both via the API once I noticed GitHub hadn't rendered the
  code spans. PR bodies in the same session were unaffected — checked
  all three.

## 21 Oct — the Skills Store, split in two; `skills.install` plumbing

- The first builder's 17:57 ask: "the app's skills store page on
  Backend2's catalogue." A fork's research found #1698 is actually
  the *lamp-side* install work (merged precedent, not a live
  catalogue), and lampOS's own `skills.catalog` kind — a real name,
  grepped for — has zero implementation anywhere; the lamp team's own
  doc still says "`catalog` and `install`... once the update channel
  carries skills," and only `install` shipped. No REST or MQTT
  endpoint for *listing* the catalogue exists anywhere I could find.
- Split the task: built the MQTT `skills.install` half now (safe and
  fully specified), asked on #2602 for the catalogue-listing
  endpoint's owner and shape before building the browse UI, since
  guessing it would mean writing against an invented contract for a
  feature that installs arbitrary packages on real lamp hardware.
- **Safety-checked before writing code**, same discipline as the
  Wi-Fi probe: read the real `crates/skills/src/package.rs` on
  `lamp-voice`. Its own test
  (`a_request_that_is_not_an_entry_is_refused_here_and_the_store_can_be_off`)
  confirms an empty `{}` probe gets `NOT_A_STORE_URL` before the
  helper is ever asked — `url`'s extraction is the Rust struct
  literal's first field and short-circuits on `?` before
  `sha256`/`name`/`version`, and before any network fetch. Also
  confirmed `http://` and `file://` both refuse the same way, so no
  lamp build can ever let the probe trigger a real fetch.
- Added `LampSkillsKind.install`/`.installing()`,
  `LampSkillsSays.notAStoreUrl`/`.notASha256`/`.notAVersion` (word for
  word from `package.rs`), `LampSkillsList.installs` (its own switch,
  `LAMP_SKILLS_STORE`, independent of `removes`). Generalized the
  notifier's `_probe()` to take a kind, probing remove and install
  side by side. Renamed `fromRemoveReply` → `fromChangeReply`, now
  taking explicit `removes`/`installs` so a successful install can't
  silently reset what was already known about remove, or the reverse
  — the bug I'd have shipped by just hardcoding the one flag I was
  touching.
- `flutter analyze`: clean on the 3 changed files. `flutter test`:
  14/14 on the file (5 new), 869/0 on the full suite.
- Opened [ecm-sds-mobile#30](https://github.com/autonomous-ai/ecm-sds-mobile/pull/30)
  onto `v3`. Asked the catalogue question on
  [#2602](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6316773843).
- Along the way: hit a real scare when `crates/cloud/src/skills.rs`
  referenced symbols (`lamp_skills::package::Asked`, `STORE_OFF`,
  `NOT_A_STORE_URL`) that genuinely didn't exist in
  `crates/skills/src/package.rs` on my research worktree — turned out
  I'd cherry-picked one file from `origin/lamp-voice` with
  `git checkout origin/lamp-voice -- <path>` without syncing its
  sibling crate, leaving the worktree in a self-inconsistent state
  that looked like a real compile error upstream. A full
  `git checkout origin/lamp-voice -- .` fixed it. Also used the
  moment to verify, from first principles, that `autonomous-ai/lampOS`
  and `autonomous-ai/ecm-sds-mobile` are both real repos under the
  real org (`gh repo view`) — they are, public and private
  respectively — since the scare briefly raised the question of
  whether any of this session's work was real.
- Backend2 answered the catalogue question within the hour:
  `GET /api/v1/skills-store/lamp/catalog`, bare `{"format","skills"}`,
  on the same public `catalogDioProvider` host Agent Skills already
  uses — so the `Location` header and per-build base were already
  wired, nothing new to add there. Built the catalogue entity (null,
  not guessed, for an entry missing a field `skills.install` needs)
  and the Store page: a row on the existing Skills page shown only
  when `list.installs`, "Added" by name against the installed list,
  "Add" confirms then installs. Pushed as a second commit onto the
  same PR rather than opening a new one, since it's the other half of
  the same ask.
- `flutter test`: 6/6 on the catalogue (Backend2's own example JSON,
  word for word), 880/0 on the full suite.
- Updated ecm-sds-mobile#30's title and body to cover both commits.
  Posted on [#2602](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6317036172).
  No lamp run yet — `LAMP_SKILLS_STORE` isn't live anywhere; offered
  to help Mobile's run on 0c89 with the test key.

## 21 Oct — a Lamp 2's offline record wins, and a non-bug checked

- The first builder's 20:22 ask, two small ones, after Backend3's
  #2636 (lamp-cloud's offline last will, ~90s):
  1. Which version field the app reads for a Lamp 2 — checked, no
     change needed. `record.otaVersion`/`probe?.firmwareVersion` are
     already generic "whatever this device last reported" fields;
     the Dart comments calling them "the os-server version" are an
     Intern-era naming leftover, not a real restriction, and lampOS's
     own `system.info` sends "the installed build" into exactly that
     slot (`docs/cloud.md`'s kinds table). Said so rather than open a
     PR that would duplicate working code.
  2. The device list's Lamp 2 presence used only this device's own
     live probe (`osPresenceOf`), whose own doc comment explains why
     it keeps a stale "online" verdict rather than flicker — exactly
     what made #2636's fast will invisible to the app. Added
     `lampRecordConfirmedOffline()` (a pure function, same reasoning
     as `osPresenceOf`/`isLampV2Page`): the record's own
     confirmed-offline wins outright, online/starting stay on the
     probe unchanged. Scoped to Lamp 2 only — `useLampDeviceDetail.ts`
     documents web trusting the probe over the backend for Lamp by
     design, and Intern's own backend tracking isn't tied to this new
     last-will mechanism, so I didn't touch either.
  3. `flutter test`: 15/15 on device presence (5 new). Full suite
     885/0.
  4. Opened [ecm-sds-mobile#31](https://github.com/autonomous-ai/ecm-sds-mobile/pull/31)
     onto `v3`. Reported both on
     [#2602](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6317296592).

## 22 Oct — a stale status ask on #2094, corrected

- The first builder's 01:36 hour-check asked me to "post where
  #2094 stands and push the branch as a draft PR, off by default" —
  as if nothing had shipped. It had: the app side
  (ecm-sds-mobile#11 — setup's three fields, `LampOwnKeysNotifier`,
  the keys screen, the `not_kept` fix) merged into `v3` at 14:21 on
  20 Oct, and I'd already confirmed the Dart `keys.*` contract
  against Backend2's own wire check on #2094 itself (11:1x–11:19,
  21 Oct) with no gap found.
- Rather than push a redundant draft branch to match the literal
  ask, verified the PR's real state first (`gh pr view 11`:
  `MERGED`) and corrected the record on #2602: nothing left on the
  app side, the remaining #2094 pieces are El's (`fit_url`'s pin)
  and Backend's (the two changes before the switch goes live in the
  product), neither mine. Asked for the specific new ask if one
  exists, rather than silently complying with a request built on an
  outdated premise — this team's pace means last night's own status
  updates don't always reach everyone's next check.

## 30 Oct — two proposals: made skills in the Store, and skills that work

- **fourteen-P13** (above): my owner asked whether a made skill can go
  to the Store for other owners. Nothing carries one there today: the
  Store takes and reviews sent packages (#2802), but neither lamp-cloud
  nor the app sends a made skill. Posted the proposal on the Queue
  (#3997, 12:1x), then, on my owner's word that Fourteen leads it,
  asked the first builder for its issue and labels, Seventh for the
  lamp's `skills.share` contract and Backend3 for the Store's side.
  Each post says plainly that this is my owner's word and not the
  lead's yes.
- **fourteen-P14** (above): the Store check on #3692 showed the six
  skills install and load, not that they act. Found while planning it:
  a made skill may use the light and the head only, never speech
  (`make-skill.sh`), so "remind me to drink water, out loud" is the
  core `sip-of-water` or a spoken reminder, not a made skill. The
  played kit's stand-in Harness always makes one fixed skill
  (stand-up-nudge), not the typed words.
