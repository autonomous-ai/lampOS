# quiet-desk (kit-q-quiet-desk): runner and judge spec

Written 26 Nov 2026 for the first builder's #7814 ask (10:46): memory as one person alone at a quiet desk meets it. The set is played on lamp-06c1, booked on #6831, in two legs back to back: stable 16102, then `lamp-voice`'s head, with the switches as the product unit has them. Scores per scene go on #7814, with the journal lines of any miss.

The runner and judge are off-repo, like memory5's. Their mechanics are memory5's (`lampOS/tools/played/memory5/RUNNER.md`, PR #8650): the sockets, the gap gate, the face-in-view check, the speak gate, line-seen, heard-ok, the turn of a line, the shared regexes and the pitfalls. This file says only where quiet-desk differs, and what it scores.

**Data (the only inputs in git):**

| What | Where |
|---|---|
| Steps, in play order (40) | `lampOS/tools/played/quiet-desk/lines.tsv` |
| 25 checks | `lampOS/tools/played/quiet-desk/checks.tsv` |
| One WAV per `say`/`mem` line | not yet rendered. A later PR adds `quiet-desk/voices.tsv` and `SHA256SUMS`; the WAVs go to a team-only draft release, as memory5's do |

A line's WAV file is `<first 16 hex of SHA-256(text)>.wav`, with `text` exactly as in `lines.tsv` (`printf %s "$text" | shasum -a 256 | cut -c1-16`). Every line is the made-up test face Olivia's voice (ElevenLabs Sarah, as memory5). Never fall back to macOS `say`: a missing WAV stops the run before anything plays.

**Lamp access.** As memory5: ssh with `sudo -S`, the memory, eyes' and schedule sockets, one JSON line each way, a 20 s timeout, `lamp-bus record` for the bus. Never stop or restart `lamp-update` or `lamp-voice`, never reboot, never open a `*.sqlite3` file, never `systemctl daemon-reload`.

---

## 1. Preflight (nothing plays until every row is OK)

memory5's P1 to P14, with these changes:

| # | Change |
|---|---|
| P1 | Host is `lamp-06c1` |
| P2 | `.installed.sequence` is the leg's build: `16102` for the stable leg, the head's sequence for the head leg. Stop when it is not. The runner never installs a build; the booked hold sets it |
| P9 | `Olivia` **must** already be known: on 06c1 the test face stays enrolled between kits. Save her id as `F` (expected `p-b2471d941bf88836`, or its `-N` suffix). Stop when she is not known. There is no enrol and no removal (memory5's §2, U4 and U5 do not apply) |
| P10 | `O` is 06c1's owner, `p-fc66f021c67d064c` |
| P11 | Save **F's as-found ids** (kept with observations, talk notes) and `r-F`'s, as the other scopes'. They are never forgotten |
| P11b | The room's (`everyone`) entry **17** is present. Add it to `ROOM_KEEP_IDS`. It is never forgotten, by id or by a selective walk |
| P12 | Word clash: the content words of every tell and the fix (`qd1_tell` … `qd8_fix`) against every as-found entry of F, the room and O. Any shared word stops the run (reword first). `dentist` is in memory5's lines and its cleanup list: an as-found dentist row (a memory5 put-back not finished) is a clash, and stops the run |
| P13 | Every `say`/`mem` line's WAV present and `shasum -c` OK |

The gap gate runs before `q0` and before `qd12_q` (a new session for QD12).

## 2. Per scene

Line kinds, the speak gate, line-seen, marks and the bus recording are memory5's §3. Two additions:

- **Unnamed asks.** Each scene's tell is said by name (`Hey Lamp, …`); its yes and its ask are said without it, as follow-ups inside the open conversation. The ask lands 30 to 60 s after its tell (tell 14 s + yes 22 s). heard-ok's wake-word replay applies to lines starting `Lamp, ` or `Hey Lamp, ` only. An unnamed line whose turn is misheard (memory5's misheard turn: `held back: nobody spoke to the lamp`, no request, or nothing said) reads INCONCLUSIVE `follow-up not taken`, and is said once more by the `mem` rule (5 s later, still inside the window), never twice.
- **The line to someone else** (`qd10_other`) is a `say`: no speak gate, no line-seen, no replay. Its request is expected to be held (`held back: said to someone it calls by name`); a held line still restarts the 60 s gap (memory5 pitfall 1).

| Scene | Steps | What it plays |
|---|---|---|
| pre | q0 | 50 s quiet with the face hidden, plus the gap gate |
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
| QD12 | qd12_q, _sister, _dentist, _colour, qd12_snap | a new session after the gap: Hannah, Friday, and the colour still gone; `snap_end.json` |

The waits sum to about 12 min a leg, plus the gap gates. Cap a leg at 20 min; the cap always ends in the put-back. A scene that would start past the budget is skipped, and its checks read INCONCLUSIVE `SKIPPED: … test owed`.

## 3. Scoring

Every line is scored in one of the first builder's four words, the `outcome` column of `checks.tsv`:

| Outcome | Means | Read from |
|---|---|---|
| answered | the lamp's reply to the ask holds the expected fact (or, for QD11, a don't-know) | the turn's `voice/said` |
| kept | a NEW entry in F holds the fact (memory5's FACT/NEW: a `lamp-memory: remember person=<F> ok` line within 1 s of the entry's `at_unix_ms`) | the named snapshot |
| left alone | nothing NEW in any scope, and the line held or not answered | `snap_other.json`, the line's `j` |
| forgotten | `forget called: source_id \S+, about [1-9][0-9]* words`, and no entry with the fact left in F, the room's or O's scope | `snap_forget.json`, then the later asks |

Fact matches are case-folded whole words: `anna`, `hannah`, `lilac`, `dentist`, `friday`, `fourth|4th`, `floor`, `oat`, `biscuit`, `march`, `ninth|9th|\b9\b`, `lisbon|flight|priya|sunday`. DONT is memory5's regex. The correction (K8) reads `lamp-voice provider: remember: replaces=` in [`qd8_fix` start, `qd8_yes` end + 25 s], with evidence `replaces=y .*answer=replaced`, as memory5's C5.

**The set's verdict** (in this order, memory5's rules):

1. Any of the **5 hard checks** (F9, A9, L10, A11, A12c) FAILs: **FAIL**.
2. Fewer than **20 of the 25 judged** (PASS + FAIL; A10 counts here when decided): **VOID** (`quiet-desk: VOID, test owed`).
3. The pool, PASS ÷ judged over the 24 counted checks (all but A10), is under **0.90**: **FAIL**. With all 24 judged, two FAILs leave 0.92 and pass; three leave 0.875 and fail.
4. A hard check not judged: **VOID**.
5. Otherwise **PASS**; with an unclean put-back read-back it becomes **VOID**, and the lamp is held.

Output one line per leg: `quiet-desk <leg>: PASS|FAIL|VOID, <passed>/25`. Write `quiet-desk.tsv` (check, scene, outcome, hard, counts, verdict, evidence) and `verdict.json` per leg. The #7814 report is a table of scene × leg (stable 16102, head), each cell the scene's checks as answered / kept / left alone / forgotten with PASS or FAIL, then the journal lines of every miss. Mask the test face's name in every posted line.

## 4. Cleanup and put-back (always runs, after each leg: on success, a fail, the cap, or TERM)

| # | Do | Check |
|---|---|---|
| U1 | `face hide`. Stop the bus recorder by its PID, never by pattern. Fetch the journal since T0 − 5 s and the recording. Judge (§3) **before** any forget | |
| U2 | **Forget every kit id by id.** For F and r-F: every id in `kept` (with observations, `max_bytes` 16384) and `talk_notes` that is **not in F's as-found ids** (P11). For the room, O, r-O and r-everyone: only ids not in the baseline, with `at_unix_ms ≥ T0` and text matching `anna, hannah, sister, lilac, colour, dentist, fourth, floor, desk, oat, tea, sugar, biscuit, dog, neighbour, birthday, march, mum, lisbon, flight, priya, sunday, brother`. Also `{"op":"search","person":S,"query":W,"limit":50}` per word, for exchanges `kept` does not show. One `{"op":"forget","person":S,"id":N}` per id | each answer `ok`; every as-found id of F, the room and O still listed after each forget (stop at the first one gone, and hold the lamp) |
| U3 | `"selective":true` only through memory5's guard (U3 there). Never on an id that shares a content word with an as-found row of F, the room or O, and never anywhere near room entry 17 | |
| U4 | Schedule: cancel by id any reminder not in the baseline with a kit word or `kind == "timer"` (`qd3_yes` may set a dentist reminder) | |
| U5 | Mac volume back to min(as found, 45). Remove the lamp-side scratch directory. The test face stays enrolled: no eyes forget, no `forget_person` | |
| U6 | **Recount every scope** (§1's reads again) | the table below |

**The read-back (`readback.md`/`readback.json`, `clean` = every row OK):**

| Row | OK when |
|---|---|
| Test face F, r-F | ids == F's as-found ids (kept, observations, talk notes); none added, none lost |
| Eyes' people | ids == P9's; Olivia still known, same id |
| Room (everyone) ids | == as found; entry **17** present |
| Owner kept ids | == as found |
| Owner records, by key | every `key\|status\|changed_unix_ms` == as found, each read by `key` (the list is capped at 50: compare keys, never the count) |
| Owner contacts | == as found |
| Rule scopes r-O, r-everyone | ids == as found |
| No kit ids left | after ids minus before ids are empty in every scope |
| Schedule | ids == as found |
| Voice, walk file, build, links, drop-ins | memory5's rows (same MainPID and NRestarts; walk file sha and `owner`; no `zz-friday`, no `/run/friday-*`) |
| Leak check | the run folder holds no e-mail-shaped token and no `\bOlivia\b` |

The head leg starts only after the stable leg's read-back is clean, and its preflight must find the same as-found ids.

## 5. Notes

- **Kit words.** `dentist` is named in the brief, so the set keeps it, though memory5 also uses it (S3g, and its U2 list). Never run memory5 and quiet-desk on one lamp without the first one's put-back read clean in between: either kit's cleanup would take the other's dentist row. The other facts (Anna, Hannah, lilac, Biscuit, oat milk, Lisbon, Priya) are used by no other kit in `lampOS/tools/played/`.
- **A dentist reminder.** If the lamp offers a reminder at `qd3_tell`, the yes may set one. That is not a miss of K3; U4 cancels it by id.
- **The line to Priya** is the test face's own voice, talking to someone the lamp does not see. A lamp that answers it fails L10, whether or not it keeps it.
- All names and facts are made up. The test face's name is never written to a posted file: ids, counts and hashes only.
