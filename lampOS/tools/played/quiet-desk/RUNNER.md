# quiet-desk (kit-q-quiet-desk): runner and judge spec

Written 26 Nov 2026 for the first builder's #7814 ask (10:46): memory as one person alone at a quiet desk meets it. The set is played on lamp-06c1, booked on #6831, in two legs back to back: stable 16102, then `lamp-voice`'s head, with the switches as the product unit has them. Scores per scene go on #7814, as machine lines only (§3).

The runner and judge are off-repo, like memory5's. Their mechanics are memory5's (`lampOS/tools/played/memory5/RUNNER.md`): the sockets, the gap gate, the face-in-view check, the speak gate, line-seen, heard-ok, the turn of a line, the shared regexes and the pitfalls. It uses memory5's voices format (`voices.tsv` + `SHA256SUMS`, PR #8650), with no gate on that PR. This file says only where quiet-desk differs, and what it scores. The put-back (§4) is written out in full here and uses none of memory5's U2 or U3.

**Data (the only inputs in git):**

| What | Where |
|---|---|
| Steps, in play order (40) | `lampOS/tools/played/quiet-desk/lines.tsv` |
| 25 checks | `lampOS/tools/played/quiet-desk/checks.tsv` |
| One WAV per `say`/`mem` line | not yet rendered. A later PR adds `quiet-desk/voices.tsv` and `SHA256SUMS`; the WAVs go to a team-only draft release, as memory5's do |

A line's WAV file is `<first 16 hex of SHA-256(text)>.wav`, with `text` exactly as in `lines.tsv` (`printf %s "$text" | shasum -a 256 | cut -c1-16`). Every line is the made-up test face Olivia's voice (ElevenLabs Sarah, as memory5). Never fall back to macOS `say`: a missing WAV stops the run before anything plays.

**Lamp access.** As memory5: ssh with `sudo -S`, the memory, eyes' and schedule sockets, one JSON line each way, a 20 s timeout, `lamp-bus record` for the bus. Never stop or restart `lamp-update` or `lamp-voice`, never reboot, never open a `*.sqlite3` file, never `systemctl daemon-reload`.

**The run's config (off-repo, never in git):** `OWNER_ID` (06c1's owner's `p-…`), `ROOM_KEEP_IDS` (with 17), the leg's build sequence.

---

## 1. Preflight (nothing plays until every row is OK)

memory5's P1 to P14, with these changes:

| # | Change |
|---|---|
| P1 | Host is `lamp-06c1` |
| P2 | `.installed.sequence` is the leg's build: `16102` for the stable leg, the head's sequence for the head leg. Stop when it is not. The runner never installs a build; the booked hold sets it |
| P9 | `Olivia` **must** already be known: on 06c1 the test face stays enrolled between kits. Save her id as `F` (expected `p-b2471d941bf88836`, or its `-N` suffix). Stop when she is not known. There is no enrol and no removal (memory5's §2, U4 and U5 do not apply) |
| P10 | `O` is the config's `OWNER_ID`. Stop when it is not set |
| P11 | Save **F's as-found ids** (kept with observations, talk notes) and `r-F`'s, as the other scopes'. They are never forgotten. Also save, for F, the room, O, r-F, r-O and r-everyone, the newest exchange id as found: page `{"op":"exchanges_after","person":S,"after":A,"limit":20}` until `more` is false. Save `key\|status\|changed_unix_ms` of every O service record, each read by `key` (memory5's P11) |
| P11b | The room's (`everyone`) entry **17** is present and in `ROOM_KEEP_IDS`. It is never forgotten |
| P12 | Word clash: the **fact words** only (`anna, hannah, lilac, dentist, fourth, oat, biscuit, march, lisbon, priya`) against every as-found entry of F, the room and O. Any match stops the run. `dentist` is in memory5's lines and its cleanup list: an as-found dentist row (a memory5 put-back not finished) is a clash |
| P13 | Every `say`/`mem` line's WAV present and `shasum -c` OK |

The gap gate runs before `q0` and before QD12 (§2).

## 2. Per scene

Line kinds, the speak gate, line-seen, marks and the bus recording are memory5's §3. These are added:

- **Unnamed asks.** Each scene's tell is said by name (`Hey Lamp, …`); its yes and its ask are said without it, as follow-ups inside the open conversation. The ask lands 30 to 60 s after its tell (tell 14 s + yes 18 s). heard-ok's wake-word replay applies to lines starting `Lamp, ` or `Hey Lamp, ` only. An unnamed line whose turn is misheard (memory5's misheard turn: `held back: nobody spoke to the lamp`, no request, or nothing said) reads INCONCLUSIVE `follow-up not taken`, and is said once more by the `mem` rule (5 s later, still inside the window), never twice.
- **A yes only when asked.** A `_yes` line (`qd1_yes` … `qd9_yes`) is said only when the turn before it asked for it. The lamp asked when its `voice/said` for the tell has a question with `(?i)\b(remember|keep|save|note|change|update|forget)\b` in it. When it did not ask (it kept quietly, or answered without asking), the yes is not said, its wait is kept so the ask's timing holds, and its mark reads `yes-not-asked`. That is not a VOID. A reminder offer at QD3 (`(?i)remind` with none of those words) gets no yes, so the kit never sets a reminder.
- **The line to someone else** (`qd10_other`) is a `say`: no speak gate, no line-seen, no replay. Its request is expected to be held (`held back: said to someone it calls by name`); a held line still restarts the 60 s gap (memory5 pitfall 1).
- **A new session for QD12.** The thread's second route is told a person's last exchanges (voice `thread.rs`: `EXCHANGES` = 12, `REMEMBERED_FOR` = 600 s; an exchange older than that is not told to a new session). So the answers of A1 to A8 can come from the conversation itself, and are `report` (in-conversation). QD12 must not: `qd12_q` waits its 500 s, then on until **630 s** after `qd8_ask`'s end mark (the last exchange with Hannah; `qd3_ask`, with Friday, is older), and then the gap gate (60 s with none of its lines). Every exchange that could tell Hannah or Friday is then past `REMEMBERED_FOR`, whether or not it is among the last 12, and no new exchange is made in the quiet. If the 630 s cannot be met inside the play budget, QD12 is skipped (`SKIPPED: … test owed`), never played early.

| Scene | Steps | What it plays |
|---|---|---|
| pre | q0 | 30 s quiet with the face hidden, plus the gap gate |
| QD1 | qd1_tell, _yes, _ask | sister Anna: tell, yes, ask back |
| QD2 | qd2_* | favourite colour lilac |
| QD3 | qd3_* | the dentist on Friday at half past nine |
| QD4 | qd4_* | desk on the fourth floor |
| QD5 | qd5_* | tea with oat milk, no sugar |
| QD6 | qd6_* | the neighbour's dog Biscuit |
| QD7 | qd7_*, qd_snap_keep | mum's birthday on 9 March; `snap_keep.json` |
| QD8 | qd8_fix, _yes, qd8_snap, qd8_ask | the correction "No, it's Hannah, not Anna"; `snap_fix.json` |
| QD9 | qd9_forget, _yes, qd9_snap, qd9_ask | "forget my favourite colour", then asked again; `snap_forget.json` |
| QD10 | qd10_other, qd10_snap, qd10_ask | a line to Priya (a flight to Lisbon), never for the lamp; `snap_other.json`; then asked by name |
| QD11 | qd11_never | a brother never told |
| QD12 | qd12_q, _sister, _dentist, _colour, qd12_snap | a new session, about 10.5 min after QD8: Hannah, Friday, and the colour still gone; `snap_end.json` |

The waits sum to about 18 min a leg; with the gap gates, face waits and snapshots, `qd8_ask` ends at about 560 s and QD12 plays at about 1190–1250 s. Cap a leg's play at **22 min**: QD12 must wait out the thread's history window (`REMEMBERED_FOR` 600 s past the last of the 12 exchanges that held Hannah), so a 20 min cap would skip QD12 and leave A12c, a hard check, unjudged (VOID); the cap always ends in the put-back. A scene that would start past the cap is skipped, and its checks read INCONCLUSIVE `SKIPPED: … test owed`.

## 3. Scoring and the report

Every line is scored in one of the first builder's four words, the `outcome` column of `checks.tsv`:

| Outcome | Means | Read from |
|---|---|---|
| answered | the reply holds the expected fact (or, for A9, A11, A12c, a don't-know), and for a fact, `MEM`: the journal shows memory carried it into that turn | the turn's `voice/said`, and `request <q>: memory told … hits_told=[1-9]… hits_path=(speaking\|face)` or `request <q>: tools: … calls=…recall` |
| kept | a NEW entry in F holds the fact (memory5's FACT/NEW: a `lamp-memory: remember person=<F> ok` line within 1 s of the entry's `at_unix_ms`) | the named snapshot |
| left alone | nothing NEW in any scope at `snap_other` and `snap_end`, and the Priya line held or not answered | the snapshots, the line's `j` |
| forgotten | `forget called: source_id \S+, about [1-9][0-9]* words`, and no entry with the fact left in F, the room's or O's scope | `snap_forget.json`, then A9 and A12c |

Fact matches are case-folded whole words: `anna`, `hannah`, `lilac`, `dentist`, `friday`, `fourth|4th`, `floor`, `oat`, `biscuit`, `march`, `ninth|9th|\b9\b`, `lisbon|flight|priya|sunday`. "Hannah, not Anna" is right: a check fails on Anna only when Anna is given as the name (`anna` with no `hannah`, or `(is|called|named) anna`). The correction (K8) reads `lamp-voice provider: remember: replaces=` in [`qd8_fix` start, `qd8_yes` end + 25 s], with evidence `replaces=y .*answer=replaced`, as memory5's C5. A fact said without `MEM` is INCONCLUSIVE `not carried by memory`, never a PASS.

**The set's verdict** (in this order, memory5's rules):

1. Any of the **5 hard checks** (F9, A9, L10, A11, A12c) FAILs: **FAIL**.
2. Fewer than **20 of the 25 judged** (PASS + FAIL; the 9 report checks A1 to A8 and A10 count here when decided): **VOID** (`quiet-desk: VOID, test owed`).
3. The pool, PASS ÷ judged over the **16 counted** checks (K1 to K9, F9, A9, L10, A11, A12a, A12b, A12c), is under **0.90**: **FAIL**. With all 16 judged, one FAIL leaves 0.94 and passes; two leave 0.875 and fail.
4. A hard check not judged: **VOID**.
5. Otherwise **PASS**; with an unclean put-back read-back it becomes **VOID**, and the lamp is held.

Output one line per leg: `quiet-desk <leg>: PASS|FAIL|VOID, <passed>/25`. Write `quiet-desk.tsv` (check, scene, outcome, hard, counts, verdict, evidence) and `verdict.json` per leg, locally.

**What is posted on #7814 is machine-only.** A table of scene × leg (stable 16102, head), each cell the scene's checks with their outcome and PASS, FAIL, VOID or INCONCLUSIVE. For a miss: the check, the request numbers, the entry ids and counts, and only journal lines of these forms, with anything after a `:` that is quoted text cut: `request N: memory told|had nothing told …` (counts and paths), `request N: tools: … calls=…`, `request N: held back: <reason>`, `request N heard N words`, `forget called: source_id …, about N words`, `remember: replaces=… answer=…`, `lamp-memory: remember person=<F|room|owner> ok`. Never `voice/said` or `voice/heard` text, entry text, or a person id other than F's. A face misread can put an owner's turn in a kit window; this keeps their words off the issue.

## 4. Cleanup and put-back (always runs, after each leg: on success, a fail, the cap, or TERM)

The put-back forgets only what the kit itself made, each id tied to one of the kit's own turns. It never forgets by word, never searches by word, never uses `selective`, and never uses `exchanges_forget` or a forget by `about` words.

**Kit turns.** The request numbers of the kit's played lines: for each mark, the `voice/heard` request in [start − 2 s, end + 12 s] whose text matches the line (memory5's turn of a line). Only these requests, and the journal's lines for them, tie an id to the kit.

| # | Do | Check |
|---|---|---|
| U1 | `face hide`. Stop the bus recorder by its PID, never by pattern. Fetch the journal since T0 − 5 s and the recording. Judge (§3) **before** any forget | |
| U2 | **Kit ids, by id only.** Read every scope again (§1's reads). An id is the kit's when it is NEW (not in the as-found ids, or an exchange id past the as-found newest), and: **kept or observation entry** — its `at_unix_ms` is within 1 s of a `lamp-memory: remember person=<S> ok` line, or of an `observation pass` line, that follows a kit turn within 25 s, and its text holds a fact word; **rule entry** — within 1 s of a kit turn's `calls=…keep_rule`; **exchange** — its `at_unix_ms` is inside a kit turn's window and its `heard` matches that kit line's text (difflib ratio ≥ 0.8, compared locally). In O, r-O and r-everyone the same ties apply, and nothing else: no fact word alone ever selects an id there | |
| U3 | **Dry run.** Write the list (scope, id, kind, the tying request, the first 40 bytes of its text) to the run's local log, never posted. Any NEW id that is not tied to a kit turn is **left**, listed as `untied`, and the read-back reads not clean: a person reads it before anything more is forgotten | |
| U4 | **Forget each listed id** with `{"op":"forget","person":S,"id":N}`, one per id, F's first | each answer `ok`; after each forget, every as-found id of F, the room (17 first) and O still listed, and the as-found newest exchange of each scope still there. Stop at the first one gone, and hold the lamp |
| U5 | **Service records.** A NEW O record key (`changed_unix_ms ≥ T0`) is the kit's only when its change follows a kit turn's tool call within 25 s. Before removing it, read **every** O key (memory5's P11 reads) and check that `{"op":"service_records_forget","person":O,"about":W}`'s words W match that key's record and no other key's: as on 25 Nov, by key, never by the count. If W would reach any as-found key, leave it, list it, and hold the lamp | each removed key gone; every as-found key's `key\|status\|changed_unix_ms` unchanged |
| U6 | **Schedule.** Cancel by id a reminder not in the baseline whose `request_id` or creation follows a kit turn's tool call within 25 s: `{"op":"cancel","id":N,"guard":{"store_id":…,"revision":…,"request_id":<32 hex>}}`, the guard from `{"op":"status"}` (none expected: the kit says no yes to a reminder offer) | |
| U7 | Mac volume back to min(as found, 45). Remove the lamp-side scratch directory. The test face stays enrolled: no eyes forget, no `forget_person` | |
| U8 | **Recount every scope** (§1's reads again) | the table below |

**The read-back (`readback.md`/`readback.json`, `clean` = every row OK):**

| Row | OK when |
|---|---|
| Test face F, r-F | ids == F's as-found ids (kept, observations, talk notes); none added, none lost |
| Exchanges, every scope | the as-found newest id of each scope still present; no NEW exchange tied to a kit turn left |
| Eyes' people | ids == P9's; Olivia still known, same id |
| Room (everyone) ids | == as found; entry **17** present |
| Owner kept ids | == as found |
| Owner records, by key | every as-found `key\|status\|changed_unix_ms` unchanged, each read by `key` (the list is capped at 50: compare keys, never the count); no kit key left |
| Owner contacts | == as found |
| Rule scopes r-O, r-everyone | ids == as found |
| Untied ids | none listed in U3 |
| Schedule | ids == as found |
| Voice, walk file, build, links, drop-ins | memory5's rows (same MainPID and NRestarts; walk file sha and `owner`; no `zz-friday`, no `/run/friday-*`) |
| Leak check | the posted text holds no e-mail-shaped token, no `\bOlivia\b`, no `voice/said` or entry text, and no person id but F's |

A NEW owner or room entry made by the owner's own talk during the run is untied, so it is never forgotten: it is listed (ids only) and left for the owner. The head leg starts only after the stable leg's read-back is clean, and its preflight must find the same as-found ids.

## 5. Notes

- **Kit words.** `dentist` is named in the brief, so the set keeps it, though memory5 also uses it (S3g, and its U2 list). Never run memory5 and quiet-desk on one lamp without the first one's put-back read clean in between: memory5's word-based U2 would take quiet-desk's dentist row. The other facts (Anna, Hannah, lilac, Biscuit, oat milk, Lisbon, Priya) are used by no other kit in `lampOS/tools/played/`.
- **The line to Priya** is the test face's own voice, talking to someone the lamp does not see. A lamp that answers the Priya line fails L10, whether or not it keeps it; its answer to `qd10_ask` is A10's (report).
- **MEM is a lower bound.** The hits line gives counts, not ids, so `MEM` shows memory told something into the turn, not which entry. Together with the K check (the entry kept) and QD12's new session, it rules out an answer read off the thread.
- All names and facts are made up. The test face's name is never written to a posted file: ids, counts and hashes only.
