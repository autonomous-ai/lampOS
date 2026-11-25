# memory5 (kit-q-memory5): runner and judge spec for Fifth

Written 25 Nov 2026 for the first builder's 17:59 ruling on #8567. The runner and judge are Fifth's, built from this spec in Fifth's own language, and run on lamp-52e6 from the 27 Nov 07:00 gate. It describes what Friday's off-repo kit does (`run-memory5.sh`, `kit5.py`, `judge5.py`, `readback5.py`, `kept_guard.py`), plus the three steps that kit lacks: the test face's enrol, its removal, and the record count before and after.

**Data (the only inputs in git):**

| What | Where | Pin |
|---|---|---|
| Steps, in play order | `lampOS/tools/played/memory5/lines.tsv` on `lamp-voice` | commit `0c2a2bc4e0` |
| 15 checks | `lampOS/tools/played/memory5/checks.tsv` on `lamp-voice` | commit `0c2a2bc4e0` |
| One WAV per line (23) | `memory5/voices.tsv` + `memory5/SHA256SUMS`, PR #8650 (`friday/memory5-gate-voices`) | `SHA256SUMS` sha256 `b64a2cdb06d4…` |
| The WAVs | draft release `kit-memory5-clips-1125` (23 WAVs + `SHA256SUMS`, team only) | `gh release download kit-memory5-clips-1125 --repo autonomous-ai/lampOS -D <dir> && shasum -a 256 -c SHA256SUMS` |

A line's WAV file is `<first 16 hex of SHA-256(text)>.wav`, with `text` exactly as in `lines.tsv`. Every line uses one voice (ElevenLabs Sarah, `olivia`). Never fall back to macOS `say`: a missing WAV stops the run before anything plays.

**Lamp access.** Use ssh with `sudo -S`. Use the memory socket `/run/lampos-memory/memory.sock`, the eyes' socket `/run/lampos-eyes/who.sock` and the schedule socket `/run/lampos-schedule/call.sock`. Each takes one JSON line and gives one JSON line back, with a 20 s timeout. Use the `lamp-bus record` recorder for the bus. Never stop or restart `lamp-update`, never reboot, never open a `*.sqlite3` file, never `systemctl daemon-reload`, and never restart `lamp-voice` (see pitfall 2).

---

## 1. Preflight (nothing plays until every row is OK)

| # | Read | How | Stop the run (exit 3, nothing played) when |
|---|---|---|---|
| P1 | Host | `hostname` | not `lamp-52e6` |
| P2 | Build | `/var/lib/lampos-update/state.json` → `.installed.sequence` | saved only: the gate names the build |
| P3 | Units | `systemctl is-active lamp-update lamp-voice lamp-memory lamp-eyes lamp-schedule lamp-actions` | `lamp-voice` not active |
| P4 | Voice process | `systemctl show -p MainPID -p NRestarts -p ActiveEnterTimestamp lamp-voice` | saved. The put-back must find the same MainPID |
| P5 | Drop-ins | `ls /etc/systemd/system/lamp-voice.service.d /run/systemd/system/lamp-voice.service.d`; `systemctl show -p DropInPaths --value <unit>` for each unit above; `ls -d /run/friday-*` | any `zz-` drop-in (another builder's) |
| P6 | Exes and links | `readlink -f /proc/<MainPID>/exe` for each unit; `readlink -f /opt/lampOS/*/current` | saved for the put-back compare |
| P7 | Switches | `LAMP_*` from `/proc/<pid>/environ` of lamp-voice, lamp-actions and lamp-eyes | warn only. Each check that needs a switch reads INCONCLUSIVE when it is off (table in §3) |
| P8 | Walk file | sha256 of `/var/lib/lampOS/voice/onboarding/progress.json`, and whether its `owner` key is set | saved. The put-back must find the same sha (pitfall 3) |
| P9 | Eyes' people | `/var/lib/lampos-eyes/people.json`: the ids (`p-[0-9a-f]{16}(-[0-9]{1,3})?`) and whether any name is `Olivia`. Save ids and a name hash only, never names | file missing, or an `Olivia` already known |
| P10 | Owner id | the 52e6 owner's `p-…` (the walk file's owner, or the app). Do not use 06c1's `p-fc66f021c67d064c` | not found |
| P11 | Scopes' keys | every scope in the list below, read as in the next table | any read not `ok` |
| P12 | Word clash | the content words of every keep line (`remember` or `actually`), against every as-found entry of the test face, room and owner scopes (below) | any shared word: reword the line before the run |
| P13 | WAVs | every `say`/`mem` line's WAV present and `shasum -c` OK | any missing (exit 3) |
| P14 | Quiet and face | the gap gate (below), run before `q0` and before each `quiet` step | see the gap gate |

**Scopes.** `F` = the test face's id, read back after the enrol (§2), expected `p-b2471d941bf88836`. `everyone` = the room. `O` = the 52e6 owner. The rule scopes are `r-<F hex>`, `r-<O hex>` and `r-everyone`, where `p-<hex>` becomes `r-<hex>`.

**Reading every key, never only the 50-row page:**

| Scope | Request | Note |
|---|---|---|
| F, everyone, O | `{"op":"kept","person":S,"observations":true,"max_bytes":16384}` | Gives ids `{id,text,at_unix_ms,on?,last_said_unix_ms?}`. The server clamps `max_bytes` to 16384 and fills it **newest first**. If an as-found scope's text sum is near 16 KB, its oldest ids fall out of the window. Record the byte sum, and compare ids only after cleanup, when the window is back to the as-found one. |
| F, everyone, O | `{"op":"talk_notes","person":S,"limit":50}` | Talk-note ids, for the cleanup |
| r-F, r-O, r-everyone | `{"op":"kept","person":R,"max_bytes":16384}` | Rule ids |
| O records | `{"op":"service_records","person":O,"limit":50}`, then **for every key** `{"op":"service_records","person":O,"key":K}` | The list is capped at `LIST_MOST` = 50 (server side, newest change first). The set of keys is the union of the page's keys and the as-found key list from earlier runs or the app. If the page holds exactly 50 rows and no as-found key list is given, stop: the tail cannot be counted. Save `key|status|changed_unix_ms` for each key. |
| O contacts | `{"op":"service_contacts","person":O}` | Save `sha256(lower(name))[:12]:changed_unix_ms`, never the name |
| Schedule | `{"op":"list_set","after_id":A,"limit":8}`, paged until a page holds fewer than 8 | Reminder ids, for the cleanup |

The test face's scope `F` and `r-F` must be **empty before the enrol** on 52e6. If they are not, stop: 52e6 is meant to hold no person's records.

**The gap gate (the product's 60 s, not "60 s with no journal line").** Follow `journalctl -f -n 0 -o short-unix -u lamp-voice -u lamp-memory -u lamp-schedule -u lamp-actions` for the whole run, and keep it as `journal.txt`. A `quiet` step waits its `wait` s and then, for `q0`, `s3g_q`, `s3g_rest` and `s2g_q`, until **60 s** pass with none of these lines. Each one restarts the count:

- `heard [1-9][0-9]* words` (a line held as "not said to the lamp" counts: pitfall 1)
- `first answer audio`, `first audio byte`
- `lamp-voice: saying ` (a skill speaking)

Cap the extra wait at 120 s. Past the cap, the checks that rest on a new session (T2 for `s2g_q`, C23 and C44 for `s3g_rest`) read INCONCLUSIVE `no 60 s gap`. Stop if the journal has been stale for more than 15 s.

**The face-in-view check.** Before a scene opens with the face shown, the newest `faces in view N` line (voice, on the memory-scope line, `… ms ago, faces in view N`) or the eyes' `a face in N` must say 1 and be at most 5 s old. `S9` needs `faces in view 0`, with the face hidden. Record the newest line for each scene.

---

## 2. The test face's enrol (new: the kit has none)

**Who.** The made-up **Olivia**, a talking-face video on the Mac's built-in screen beside the lamp. No real person stands in. The video stays off-repo. Friday's driver is `face.sh talk|still|hide|status` (`~/friday-video-test/video-test/`, video `FACE_VIDEO`). Its status starts with `playing`, `paused` or `hidden`. `talk` = playing (mouth moving), `still` = paused frame shown, `hide` = window hidden.

| Step | Do | Expect |
|---|---|---|
| E1 | The gap gate, then `face talk` (about 0.35 of the frame tall). Record `humans/faces/tracked` for 4 s | exactly one face in ≥ 80 % of looks, never two |
| E2 | On the eyes' socket `/run/lampos-eyes/who.sock`: `{"remember":"Olivia","one_face":true}` (the voice's own call under `LAMP_NAMES=checked`, product on) | `{"remembered":"Olivia","replaced":[]}`. The eyes take a last look before they answer, so allow up to 20 s. Refusals: `{"error":"busy"}`, `"eyes_shut"`, `"not_a_name"`, `"known_as"`, `"near_to"`, `"not_like_them"`, `"full"`, or no face / too far / two faces. Any refusal stops the run (exit 3), with no retry past one |
| E3 | Read the id: `people.json` again; the new id is the one not in P9's ids | `p-b2471d941bf88836` = `"p-"` + SHA-256(`"olivia"`, trimmed and lowercased)[:8 bytes hex]. The eyes give it a `-N` suffix if that id is already someone's: then use the suffixed id as `F` throughout |
| E4 | `{"who":true}` on who.sock, with the face talking | `{"who":"Olivia","likeness":…}` (the eyes know her now) |
| E5 | `kept` of `F` and `r-F` again (§1) | both empty. Save them as the test face's as-found (empty) |

If the lamp's eyes run with `LAMP_REMEMBER_SPEAKER=on` (not in the product), a plain remember answers `{"error":"speaker_required"}`. Then stop and say so. Do not switch it off.

The enrol line is not one of the 23 played lines, so nothing is said aloud for it.

---

## 3. Per scene

**Line kinds** (`lines.tsv` columns: step, scene, face, kind, wait, text):

- `face`: `talk`, `still` or `hide`, set before the line. `-` leaves it as it is. After `talk`, wait until the status reads `playing` (6 × 0.5 s), then 2 s more. If it is not playing, try once more, then mark it `talk-frozen`.
- `quiet`: wait `wait` s, plus the gap gate where §1 says so.
- `say`: play the WAV (`afplay`, Mac volume at most 45), then wait `wait` s. If the wake word was misheard (heard-ok below), replay once.
- `mem`: run the **speak gate** before it, play it, wait `wait` s, then run **line-seen**. If a line reads VOID with no tool call, wait 5 s and say it once more, at most once per line, whatever the cause. A line VOID after a tool ran is never said again.
- `snap`: snapshot every scope (§1's `kept` and `talk_notes` for F, everyone, O, r-F, r-O and r-everyone, plus the schedule) into `snap_<text>.json`.

Write a mark for every played line: `tag, scene, start_epoch, end_epoch, face_state, words`. Every check finds its turn by these marks.

**Bus recording.** One recording for the whole set: `lamp-bus record <dir> voice/heard voice/said humans/faces/tracked --seconds <budget+600>`. Write its PID and stop only that PID (`kill -INT`), never by pattern.

**Speak gate** (before each `mem` line): the last 4 s of `humans/faces/tracked`.
- `talk`: ≥ 3 looks; ≥ 80 % of them with exactly one face, and none with two; one id; ≥ 2 looks with `speaking.state == "yes"`.
- `still`: the same single-face rule, with 0 looks reading `yes`.

If it fails, set the face again, wait 3 s and try once more. On a second fail, skip the line, write `gates.tsv`: `tag pre 2 skip …`, and its checks read VOID.

**Turn of a line** (the judge's unit): the `voice/heard` record in `[start-2, end+12]` whose text best matches the line (difflib ratio) gives `request`. `said` = every `voice/said` text with that `request`. `j` = the journal lines matching `\brequest <req>\b`.

**heard-ok** (after a `say`/`mem` line starting with `Lamp, `): read the journal since the line's start.

| Regex | Meaning |
|---|---|
| `request (\d+) heard (\d+) words` | heard |
| `request (\d+): its answer is played` | played |
| `request (\d+): (?:its answer is held back\|.*called not_for_me)` and `request (\d+): tools: .*calls=\S*not_for_me` | bad |
| `request (\d+): tools: .*?calls=(\S+)` | a tool ran: never replay |

The line is misheard when nothing was heard or nothing played (once bad requests are taken out). It is also misheard on noise (more than the line's words + 2 heard), or when cut short (fewer than words − 1), with runs of number words counted as one word.

**line-seen** (after a `mem` line): every request heard since start − 1 s must have `seen speaking it: the known person's face`, and none of `no face was seen speaking it|\bcarried:`. The one exception is the lone line, `the known person's face the one face in view and no other voice heard`, which is seen. Any `not_for_me` call makes the line unseen. Exit 0 seen; 1 VOID with no tool call; 3 VOID with a tool run; 2 not heard. Write `gates.tsv`: `tag post 1 seen|void|none req … calls=…`.

**Misheard turn** (judge side, INCONCLUSIVE): no request; or `j` matches `held back: said to someone it calls by name|held back: a reply to someone the room had just called|held back: nobody spoke to the lamp|called not_for_me`; or `said` is empty.

**Shared regexes (from `judge5.py`/`kit5.py`):**

```
DONT  = (?i)\b(don't|do not|haven't|have not|no record|not sure|nothing (on|about)|can't recall|no idea)\b
FACT  = lamp-memory: remember person=\S+ ok        # an entry is a kept fact if its at_unix_ms is within 1.0 s of one
NEW   = an entry of the snapshot's scope whose id is not in the preflight baseline (kept + talk ids)
```

### The steps

Approximate timings are the `wait` sums. The full play is about 15 min, under a 24 min run cap that always ends in the put-back.

| Scene | Step | Face | Kind | Wait | Line (clip) |
|---|---|---|---|---|---|
| pre | q0 | hide | quiet | 50 + gap | none |
| **S9** | s9_name | hide | say | 18 | "Lamp, what's my name?" (`abbe46a40fa22cff.wav`) |
| **S3g** | s3g_q | hide | quiet | 50 + gap | none |
| | s3g_01 | talk | say | 15 | "Lamp, I'm planning to repaint my kitchen next week." (`8fbfcd783d42c027`) |
| | s3g_02 | - | say | 15 | "Lamp, what do you think of teal for the kitchen walls?" (`b4b2f1d3fb3870fb`) |
| | s3g_03 | - | say | 15 | "Lamp, I have a dentist appointment tomorrow at ten, so I'll paint after that." (`9a4018fbf219d2f0`) |
| | s3g_04 | - | say | 12 | "Lamp, quiz me: what's the capital of Sweden?" (`e44eeefb04954393`) |
| | s3g_05 | - | say | 12 | "Lamp, quiz me: what's the capital of Ghana?" (`9c2b6dfbac9979b9`) |
| | s3g_06 | - | say | 12 | "Lamp, quiz me: what's the capital of Nepal?" (`bd6f2700c1008b26`) |
| | s3g_rest | still | quiet | 80 + gap | none (the observation pass runs after the conversation ends) |
| | s3g_snap | - | snap | 0 | `snap_s3g.json` |
| **S0g** | s0g_name | talk | mem | 16 | "Lamp, what's my name?" (`abbe46a40fa22cff`) |
| | s0g_k1 | - | mem | 18 | "Lamp, please remember that my cousin is called Ferdinand." (`ef01bbc66d0a4996`) |
| | s0g_k1_yes | - | mem | 12 | "Yes, please keep that." (`ad525cf40d8ea83a`) |
| | s0g_k2 | - | mem | 18 | "Lamp, please remember that my favourite jam is apricot." (`dab491919d513e66`) |
| | s0g_k2_yes | - | mem | 12 | "Yes, please keep that." (`ad525cf40d8ea83a`) |
| | s0g_t1 | - | mem | 18 | "Lamp, from now on, please keep your answers to me to one short sentence." (`4f05e435a6bda1da`) |
| | s0g_snap | - | snap | 0 | `snap_s0g.json` |
| **S13g** | s13g_keep | - | mem | 18 | "Lamp, please remember that my water bottle is green." (`d1fcec1a1d713ab1`) |
| | s13g_keep_yes | - | mem | 12 | "Yes, please keep that." |
| | s13g_ask_face | - | mem | 18 | "Lamp, what colour is my water bottle?" (`429e4e253a25787e`) |
| | s13g_hide | hide | quiet | 8 | none |
| | s13g_keep_hidden | - | say | 18 | "Lamp, please remember that my scooter is orange." (`4c1bdbe0f97931cf`) |
| | s13g_ask_hidden | - | say | 18 | "Lamp, what colour is my water bottle?" |
| | s13g_snap_hidden | - | snap | 0 | `snap_s13g_hidden.json` |
| | s13g_fix | talk | mem | 18 | "Lamp, actually, my water bottle is blue now." (`3106ce03c3ba2d33`) |
| | s13g_fix_yes | - | mem | 12 | "Yes, please keep that." |
| | s13g_snap_fix | - | snap | 0 | `snap_s13g_fix.json` |
| | s13g_forget_about | - | mem | 18 | "Lamp, forget what I said about my water bottle." (`633d8cbe68eaa676`) |
| | s13g_forget_yes | - | mem | 14 | "Yes, please forget it." (`27a9ef3dad6dc964`) |
| | s13g_snap_forget | - | snap | 0 | `snap_s13g_forget.json` |
| | s13g_after | - | mem | 18 | "Lamp, what colour is my water bottle?" |
| (S0g) | s0g_r1 | - | mem | 16 | "Lamp, what's my cousin called?" (`ae2e4fe6e3f53220`) |
| (S0g) | s0g_r2 | - | mem | 16 | "Lamp, what's my favourite jam?" (`0cf41520f907887d`) |
| **S2g** | s2g_q | still | quiet | 50 + gap | none (a new session for T2) |
| | s2g_t2 | talk | mem | 20 | "Lamp, tell me about the moon." (`946a4448dd5cb633`) |
| | s2g_blood | - | mem | 20 | "Lamp, what is my blood type?" (`23b6201544a217c7`) |
| | s2g_hide | hide | quiet | 8 | none |
| | s2g_room_plant | - | say | 20 | "Lamp, remember that the office plant gets watered on Fridays." (`c67467f2205b776c`) |
| | s2g_snap_room | - | snap | 0 | `snap_s2g_room.json` |
| | s2g_show | talk | quiet | 6 | none |
| | s2g_room_ask | - | mem | 22 | "Lamp, what do you know about the office plant?" (`21bfac1c8f93d963`) |
| | s2g_snap | - | snap | 0 | `snap_s2g.json` |

A scene that would start past the play budget (15 min; estimates S9 90 s, S3g 220, S0g 120, S13g 225, S2g 180) is skipped, and its checks read INCONCLUSIVE `SKIPPED: … test owed`. With no face video, every scene but S9 is skipped.

### The checks (`checks.tsv`), what each reads, and its verdict

A PASS or FAIL whose deciding lines (`checks.tsv` column `lines`) include one with a `gates.tsv` row of `skip` or `void` becomes **VOID**. `turn(x)` misheard or not played means INCONCLUSIVE.

| Check | Hard | Counts | Reads | PASS | FAIL | Else |
|---|---|---|---|---|---|---|
| N1 (S9) | **hard** | yes | turn `s9_name`. Evidence: `memory scope the room's` and `faces in view 0` | neither the test-face name nor the owner's typed name (`OWNER_NAME_CHECK`) in `said` (case-folded) | a name said | no name set: INCONCLUSIVE |
| R1 (S0g) | - | yes | turn `s0g_name` | `Olivia` in `said` | not in it | |
| R2 (S0g) | - | yes | FACT entries in F at `snap_s0g` matching `ferdinand`; turn `s0g_r1` | `ferdinand` in `said` | not kept, or kept and not told | |
| R3 (S0g) | - | yes | same with `apricot`, `s0g_r2` | `apricot` in `said` | as R2 | |
| T1 (S0g) | - | report | turn `s0g_t1` `j`: `calls=\S*keep_rule`; NEW ids in `r-F` at `snap_s0g` | both | either missing | `LAMP_MEMORY_RULES` off: INCONCLUSIVE |
| C1 (S13g) | - | yes | turn `s13g_ask_face`. Evidence: `memory told .*hits_path=(speaking\|face)` | `(?i)\bgreen\b` in `said` | DONT, or `(?i)\b(blue\|red\|black\|white)\b` | INCONCLUSIVE |
| C2 (S13g) | **hard** | yes | NEW `scooter` entries in F, everyone and O at `snap_s13g_hidden`; turn `s13g_ask_hidden`. Evidence: `told and kept as the room's`; `a remember about a person was not kept: speaker unknown` in [keep start, keep end + 25 s]; `memory had nothing told\|hits_path=room` | no scooter id in any of the three scopes, and no green told | a scooter id anywhere; or `\bgreen\b` in `said`; or `memory told .*hits_path=(speaking\|face)` on the ask | no snapshot, or misheard: INCONCLUSIVE |
| C5 (S13g) | - | yes | `lamp-voice provider: remember: replaces=` in [`s13g_fix` start, `s13g_fix_yes` end + 25 s]; NEW `bottle` entries in F at `snap_s13g_fix`. Evidence: `replaces=y .*answer=replaced` | the bottle entries hold `blue` and none hold `green` | both `green` and `blue` held | INCONCLUSIVE |
| C12 (S13g) | **hard** | yes | `forget called: source_id \S+, about [1-9][0-9]* words` in [`s13g_forget_about` start, `s13g_forget_yes` end + 25 s]; NEW `bottle` in F and everyone at `snap_s13g_forget` | the forget line found, and no bottle entry left | a bottle entry left | no forget line and nothing left: INCONCLUSIVE |
| B4848 (S13g) | **hard** | yes | turn `s13g_after` | C12 PASS and no `(?i)\b(green\|blue)\b` in `said` | a colour said | C12 not PASS: INCONCLUSIVE |
| C23 (S3g) | - | yes | `observation pass: proposed [0-9]+ kept [0-9]+` in [`s3g_01` start, `s3g_06` end + 120 s]; NEW entries in F at `snap_s3g` with `at_unix_ms ≥ s3g_01 start − 1 s`, matching `(?i)kitchen\|repaint\|teal\|dentist` | a pass line with `kept [1-9]`, and one such entry | the pass ran with none about the talk | `LAMP_MEMORY_OBSERVE` off, or no pass line: INCONCLUSIVE |
| C44 (S3g) | **hard** | yes | the same new entries; the words `[a-z]{4,}` of the six turns' `heard` text, minus `{lamp, that, this, with, have, next, week}` | every new entry shares one of those words | an entry shares none | no entries, or entries with no `kept [1-9]` pass: INCONCLUSIVE |
| T2 (S2g) | - | report | turn `s2g_t2`. Sentences: split on `(?<=[.!?])\s+`. Words: `[\w']+` | T1 PASS, ≤ 1 sentence and ≤ 25 words | more | T1 not PASS: INCONCLUSIVE |
| C30 (S2g) | **hard** | yes | turn `s2g_blood` | DONT in `said` | `(?i)\b(A\|B\|AB\|O)\s*(positive\|negative\|\+\|-)` | INCONCLUSIVE |
| C19 (S2g) | **hard** | yes | NEW `plant` entries in everyone at `snap_s2g_room`; turn `s2g_room_ask` | `(?i)plant\|water\|friday` in `said` and no FAIL phrase | `(?i)\byou (told\|said\|mentioned\|asked)\b` | `LAMP_MEMORY_ROOM_NOT_ASKER` off, or the plant not kept as the room's: INCONCLUSIVE |

**The set's verdict** (in this order):

1. Any of the **7 hard checks** (N1, C2, C12, B4848, C44, C30, C19) FAILs: **FAIL**.
2. Fewer than **12 of the 15 judged** (PASS + FAIL; T1 and T2 count here when decided): **VOID** (`memory5: VOID, test owed`).
3. The pool, PASS ÷ judged over the 13 counted checks (every check but T1 and T2), is under **0.90**: **FAIL**. With all 13 judged, one FAIL leaves 0.92 and passes; two leave 0.85 and fail.
4. A hard check not judged: **VOID**.
5. Otherwise **PASS**, and a PASS with an unclean put-back read-back (§4) becomes **VOID**: a kit fault, with the lamp held.

Output one line: `memory5: PASS|FAIL|VOID, <passed>/15`. Write `memory5.tsv` (check, scene, item, hard, counts, verdict, evidence) and `verdict.json` (`verdict, passed, total, judged, pool, hard "k/7", why, checks`).

---

## 4. Cleanup and put-back (always runs: on success, on a fail, on the run cap, on TERM)

| # | Do | Check |
|---|---|---|
| U1 | `face hide`. Stop the bus recorder by its PID. Fetch the journal since T0 − 5 s and the recording. Judge (§3) **before** any forget | |
| U2 | **Forget new entries by id.** For F and r-F: every id in `kept` (with observations) and `talk_notes` not in the baseline. For everyone, O, r-O and r-everyone: only ids not in the baseline **and** with `at_unix_ms ≥ T0` and text matching the words `ferdinand, apricot, jam, scooter, bottle, sweden, ghana, nepal, sentence, moon, teal, dentist, repaint, kitchen, plant`. Also use `{"op":"search","person":S,"query":W,"limit":50}` for each word, to find exchanges the `kept` list does not show. Send `{"op":"forget","person":S,"id":N}`, one per id | each answer `ok`; every as-found id of everyone and O still listed after each forget (stop forgetting at the first one gone) |
| U3 | `"selective":true` walks linked rows: it also takes any row that shares a distinctive word with the forgotten one, and any copy of it. Use it only through the guard: before a selective forget of a kept or observation id, read the scope live. Leave the id (record it as held, never forget it) if it has no content word, or if it shares a content word with a row the kit did not write. Content words are lowercased words of 3+ characters, not common words, with a final `s` dropped. Then remove the held ids by a plain forget by id, after a read shows no other row with their words | |
| U4 | **Remove the test face from the eyes:** `{"forget":"Olivia"}` on `/run/lampos-eyes/who.sock` | `{"forgot":true,"name_id_free":…,"id":"<F>"}` (`id` is given with `LAMP_MEMORY_STABLE_ID=on`, product on); `people.json` ids equal P9's again; `{"who":true}` is not Olivia |
| U5 | **Remove her from memory:** `{"op":"forget_person","person":F}` on the memory socket, after every r-F id has been forgotten by id (U2) | `kept` of F and r-F both return 0 entries; `talk_notes` of F returns 0 |
| U6 | Schedule: cancel by id any reminder not in the baseline with a kit word or `kind == "timer"`: `{"op":"cancel","id":N,"guard":{"store_id":…,"revision":…,"request_id":<32 hex>}}`, with the guard from `{"op":"status"}` | none expected: no step sets one |
| U7 | Mac volume back to min(as found, 45). Remove the lamp-side scratch directory | |
| U8 | **Recount every scope** (§1's reads again) | the table below |

**The read-back (`readback.md`/`readback.json`, `clean` = every row OK):**

| Row | OK when |
|---|---|
| Test face scope F, r-F | 0 entries, 0 talk notes (gone; they were empty before the enrol) |
| Eyes' people | ids == P9's; no `Olivia` |
| Room (everyone) ids | == as found (and every `ROOM_KEEP_IDS` id present) |
| Owner kept ids | == as found |
| Owner records, by key | every `key\|status\|changed_unix_ms` == as found. No key added, and none on the newest-first page with `changed_unix_ms ≥ T0` |
| Owner contacts | == as found |
| Rule scopes r-O, r-everyone | ids == as found |
| No kit ids left | the after ids minus the before ids are empty in every scope |
| Voice | same MainPID and NRestarts as P4; no `muted by the restart guard` since T0 |
| Walk file | sha == P8's; `owner` still set |
| Build, links, exes | == P2 and P6; `lamp-update` active |
| Drop-ins | == P5; no `zz-friday`; no `/run/friday-*` |
| Leak check | the run folder holds no e-mail-shaped token and no `\bOlivia\b` (mask the name in the logs) |

**Exit codes.** Friday's `run-memory5.sh` exits 2 (usage or env), 3 (preflight stopped, no WAV, word clash, base read-back not as found), 4 (a WAV missing mid-run, an unknown kind) or 0 (played; the verdict is in the line). Fifth's gate wrapper maps them to `gate-G5.sh`'s 0/10–13. This mapping is proposed; Fifth sets the meanings to `gate-G5.sh`'s own:

| Runner result | Wrapper exit |
|---|---|
| `memory5: PASS`, read-back clean | 0 |
| `memory5: FAIL` (a hard FAIL, or the pool < 0.90) | 10 |
| `memory5: VOID` (under 12 judged, a hard check not judged, or an unclean read-back) | 11 |
| Preflight stop or enrol refused (nothing played), runner exit 2/3 | 12 |
| Kit error mid-run (exit 4), or a put-back row not OK: **hold the lamp** until it is put right | 13 |

---

## 5. Pitfalls Friday hit

| # | Pitfall | What happened | Do instead |
|---|---|---|---|
| 1 | **Held lines break the 60 s gap** | The product's conversation gap (`CONVERSATION_GAP_US` = 60 s) restarts on any finished worded line, including one the lamp held as "not said to the lamp" (a bystander, office noise). The second route emits the transcript before the said-to-me hold. Friday's old `quiet62` waited for 62 s with no journal line, which is not what the product measures. Scored tries then had no new session | Use §1's gap gate: count from the last `heard [1-9][0-9]* words`, `first answer audio`, `first audio byte` or `lamp-voice: saying `. Check `faces in view 1` at the line's Start. A try whose request shows a reset inside the gap is VOID, and is never counted as a FAIL |
| 2 | **daemon-reload restarts the voice** | `systemctl daemon-reload` re-arms `lampos-setup-done.path` (`PathExists=/var/lib/lampos-setup/done`, `Unit=lampos.target`). The path still exists, so `lampos.target` starts again and pulls `lamp-voice` back up 0.46 s after a stop. `Restart=on-failure` is not the cause | memory5 needs no drop-in, so never run `daemon-reload` or restart the voice. If a future leg must, stop the voice, reload, wait 2 s, then stop it again if it came back. Confirm with `systemctl list-dependencies --reverse lamp-voice` and `journalctl -u lampos-setup-done.path` |
| 3 | **The walk file loses its owner** | When the voice starts while the eyes know nobody, the stock voice's owner refresh (at start, then every 10 s) sees an empty names list, posts `owner_gone`, and the walk drops `owner` from `/var/lib/lampOS/voice/onboarding/progress.json`. On 52e6 the eyes may know nobody but Olivia during the leg | Never restart the voice during the leg (pitfall 2). Remove Olivia from the eyes with the voice running, never across a voice start. Check the walk file's sha and `owner` in preflight and the read-back. If the eyes must restart, the eyes go first; wait until they know their people (≥ 1, or as found), then start the voice |
| 4 | **Forget-by-words skips shared words: forget by id** | A forget by the person's words (`forget what I said about …`, or the `about` route) leaves any row whose words are also in another row. `selective` forgets walk linked rows: a kept "Stop." took 4 of the test face's other items. In #8570's big forgets, 10 kit keeps were left behind because each shared a word with an older row | The put-back forgets every new entry by its id (§4 U2–U3), with the guard for `selective`. Then it recounts by key. It never forgets by words |
| 5 | **The paged list at 50** | `service_records` lists at most 50 (`LIST_MOST`, server side, newest change first), and `talk_notes`/`search` take `limit` 50. A "50 records" count is the page cap, not the total. `kept` is filled newest first up to 16 KB (`max_bytes` is clamped to 16384; with no `max_bytes` it is 4 KB) | Compare record **keys**, each read by `key`, never the count. Pass `max_bytes: 16384` to every `kept`. Compare ids, not lengths |

The test face's made-up name is never written to a posted file. Ids, counts and hashes only.
