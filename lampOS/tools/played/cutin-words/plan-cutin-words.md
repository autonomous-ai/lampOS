# #3997 step 2: the wordless cut-in hold, A/B on lamp-52e6

**What it answers.** On lamp-4ace (product unit, 9063) an answer was cut by the lamp's own tail, scored as speech,
with no words heard. The first builder's yes (#3997, 31 Oct 07:07) is an A/B of the product set against
`+LAMP_STOP_WORDS=heard +LAMP_BACKCHANNEL_HOLDS=on`. With both, a due commit waits up to 500 ms for the yield's words
and resumes the answer if they come back empty (`coordinator.rs:6616-6640`, `duplex.rs:548-581`, `:12822-12830`).

Pass needs all three:
- real cut-ins 4/4;
- 0 wordless commits;
- time to yield unchanged.

**Kit.** `row-cutin-words.sh`, scored by `score-cutin-words.py`. It is based on `tools/played/cutin.sh`. Run it
with Mac voices only and nobody in the room (it takes `lamp-lock` itself). Run the DRY matrix first (below).

## Running it
From a checkout, on any Mac, with only these set:
```
cd lampOS/tools/played/cutin-words
canned-cutin/matrix.sh        # DRY: no lamp, no audio, no post; every row must PASS
L="ssh orangepi@lamp-4ace.local" SEQ=<installed> HARD_END=HH:MM NO_PERSON=yes OUT=~/out-cutin ./row-cutin-words.sh
# 5 valid tries per arm in both rows:
C_TRIALS=5 O_TRIALS=5 L="ssh orangepi@lamp-4ace.local" SEQ=<installed> HARD_END=HH:MM NO_PERSON=yes OUT=~/out-cutin ./row-cutin-words.sh
```
- **The lamp** is the first label of L's host (`lamp-4ace`); `LAMP=` overrides it. It names the lock
  (`../../lamp-lock.sh`), the posts and the other-lamp gate, which skips the kit's own lamp.
- **Who:** `WHO=` (default `Sixth`) heads the PLAYING and QUIET posts and the loans lines; the lock holder is it in
  lower case.
- **Tries:** `C_TRIALS` (4) and `O_TRIALS` (3) are the valid tries per arm; `C_MAX` (`C_TRIALS` + 2) and `O_MAX`
  (`O_TRIALS` + 1) the most tries. `NEED_MIN` follows them (44 at the defaults, 56 at 5 and 5: book an hour).
- **Clips** are rendered once by `tts.sh` (ElevenLabs through the relay, key in `LAMP_RELAY_KEY_FILE`, default
  `~/.config/lamp-relay.key`) and cached in `WAVS` (default `~/.cache/lampos-cutin-words`), named by a hash of the
  voice and the text, so the same text plays on both arms. The texts are `HELLO`, `ASKS`, `CUTS` and `ENDLINE` in
  the kit. DRY needs no clips.
- **It plays** through `../scenario.py` and `../lamp_ssh` (`KIT=` overrides the directory), with a 10 s connect time
  (`LAMP_SSH_TIMEOUT`).
- **Levels** (`LEVEL_DB` −18, `SYSVOL` 90) were set on Sixth's Mac speaker. Another Mac plays louder or softer: each
  arm's hello must be heard, or the kit stops.
- **Levelling:** with ffmpeg/ffprobe (and bc) on PATH, ffmpeg levels the clips. Without them, `afconvert`
  (built into macOS) converts each clip to 16-bit WAV, and a python3 stdlib gain brings it to the same mean volume,
  hard-limited at 0.97 (it measures −18.0 dB, as ffmpeg does). A TTS stand-in may write AIFF (`say`) or WAV. With
  `WAVS_LEVELLED=yes`, the clips in `WAVS` are already at level: they are only converted to 16-bit WAV.
- **Another /run drop-in** on lamp-voice stops the run, unless it is named in `ALLOW_DROPINS` (space-separated, e.g.
  `ALLOW_DROPINS="zz-product-u.conf"`). An allowed drop-in stays, and the put-back removes only the kit's. It must sort
  before `zzzz-sixth-cutin.conf`, so that the kit's Environment lines win; otherwise the run stops. Its ExecStart, if
  any, counts as the installed one. The log names the allowed drop-ins, and the arm proofs still apply.
- **Needs** on the Mac: bash, python3, gh (logged in; it posts on #3336), and ssh to the lamp with sudo there.
  ffmpeg is optional (see Levelling).

## Arms
- **P:** the product unit as installed, untouched.
  - **Proof:** lamp-voice's `/proc/<pid>/exe` is the installed path (`readlink -f` of the unit's ExecStart), and
    neither `LAMP_STOP_WORDS` nor `LAMP_BACKCHANNEL_HOLDS` is in its environ. If the product unit already set one of
    them, this stops the run, since P would not be the product set.
- **W:** the same installed lamp-voice, plus a `/run` drop-in (`zzzz-sixth-cutin.conf`). The drop-in carries only
  `Environment=LAMP_STOP_WORDS=heard` and `Environment=LAMP_BACKCHANNEL_HOLDS=on`. There is no ExecStart override, and
  no binary is staged.
  - **Proof:** after the restart, the journal has `stop words: heard, in a yield` and
    `backchannel holds: a yield's commit waits`, and not `backchannels are held only with`. The exe is the installed
    path, and the environ has exactly `LAMP_STOP_WORDS=heard` and `LAMP_BACKCHANNEL_HOLDS=on`.
- **Order:** P, then W. That is one restart in, and the put-back's restart out.

## Rows per arm (the fewest that decide the verdict)
| Row | What plays | Valid | At most |
|---|---|---|---|
| C, real cut-ins | A long-story ask, then 4 s into the answer `Stop.` / `Wait, hold on.` / `Stop, stop.` / `Wait, hold on.` (try *i* plays the same clip on both arms) | 4 | 6 |
| O, the own tail | A long-story ask, then 90 s of answer with no sound from the Mac, then `Stop, stop.` to end it | 3 | 4 |

Time to yield needs no row of its own. It is row C's `#PLAY_US` to its `duplex_yield`.

**Levels.** A quiet room and no bed. The asker is at the kit's usual level: file −18 dB at Mac volume 90 (mic p90
−31 on 30 Oct). Speaker is set to 50%. There is no calibration and no tap tool.

## How a try is scored (`score-cutin-words.py`, from the session's events file)
- **Yield, end and decision:** the same as `cutin.sh`. A yield's decision is the commit or resume that its own
  `duplex_yield_end` names (same `started_at_us` and owner). Onset is `#PLAY_US`, the lamp's clock just before afplay,
  0.1–0.3 s early on both arms alike.
- **Row C:**
  - A try is valid if the answer had begun before the line and had not ended before it.
  - A real cut-in counts only when its yield ends in `commit`. "No yield within the clip + 1.5 s" is a miss, and so
    is a yield that resumed.
  - Logged with each try: `held_ms`, and the `stop_words` count.
  - A commit before the line is a wordless commit. Its try is not valid.
- **Row O:**
  - Every `duplex_commit` from the answer's first write to the end line's `#PLAY_US` is a wordless commit, because
    nothing played.
  - A try is valid if it committed, or if the answer ran at least 30 s.
  - Also logged: yields, resumes, the resumes held for words that came empty, and minutes of answer.
- **Reads:** per try, a filtered read of the events file on the lamp, with only the kinds the score uses (176 KB
  against 5.7 MB for the whole file on 30 Oct). At each arm's end, the whole events file (gzip) and the arm's
  journal are fetched read-only to `OUT/arm-<A>-*`. The journal's counts are logged as a cross-check: "0 words",
  "held then committed" and "held as a yield with no words".

## Verdict (printed at the put-back, from whatever the tally holds)
Checked in this order:
1. **VOID:** either arm has fewer than 4 valid C tries or fewer than 3 valid O tries.
2. **FAIL:** a valid W cut-in did not commit. That is the named risk: a real cut-in whose words came back empty and
   resumed.
3. **FAIL:** W made any wordless commit, in row O or before a cut-in.
4. **FAIL:** the time to yield changed. The medians of W and P differ by more than 100 ms (`YIELD_TOL_MS`, 10
   blocks). The hold only delays the commit, so the yield should not move.
5. **NO REPRO:** W passes 2–4, but P made no wordless commit either. Then W's 0 is not evidence for the hold. It is
   reported as that, not as a pass.
6. **PASS.**

Each arm's line also gives:
- the cut-ins and their decisions;
- onset to yield per try, and the median;
- wordless commits (row O, and before a cut-in);
- held-and-resumed yields;
- the commits' `held_ms` (expected ≤ 500 on W, 0 on P).

## Setup and gates (from row-3967d, the reviewed kit)
- **Lock and posts:** the kit takes `lamp-lock`. It posts PLAYING and QUIET on #3336. QUIET is posted from the EXIT
  trap by a detached poster, so a kit that lamp-lock TERMs and then KILLs still posts what ran and the put-back
  result.
- **Before starting:** the camera check (`NO_PERSON=yes`), no `lamp-update` switch in the last 3 min, and no foreign
  `/run` drop-in on lamp-voice.
- **Before every try:**
  - SEQ is checked, with one retry when the read comes back empty.
  - No afplay may be running on the Mac. DRY reads `DRY_STRAY` instead, since it plays nothing (at 07:21 another
    builder's afplay stopped a DRY).
- **After every try:** a relay HTTP 429 stops the run.
- **Each arm:** "hello" must be heard.
- **Hard end** (`HARD_END=HH:MM`):
  - The kit refuses to start with under 44 min left.
  - It stops between tries once under 3 min remain before `HARD_END − 2`.
  - A lamp-side dead-man (`systemd-run --on-active`) runs the put-back at `HARD_END − 2`.
- **Put-back:** one script on the lamp, `/run/sixth-cutin-putback.sh`. It runs from the exit trap, or in its own
  systemd unit on a signal, and from the dead-man too. It removes the drop-in (one restart) and sets Speaker to 50%.
  It then checks that lamp-voice is active, runs the installed exe, and has neither switch in its environ, and that no
  drop-in is left. It writes the loans line, and disarms the dead-man only when everything is clean. The Mac's
  volume is restored.

## Lamp minutes (from row-3967d's measured 30 Oct run; this kit has no bed and no taps)
- **Setup** (unit checks, dead-man, PLAYING, Speaker): **1 min**.
- **Per arm:**
  - The switch (drop-in, 20 s settle, proofs), the hello, and the fetch at the arm's end: **1.5 min**.
  - **Row C try:**
    - ask to first audio about 15 s;
    - 4 s of answer;
    - the line's scenario: ssh, 1.5 s clip, 8 s, and its own events read, about 22 s;
    - the story's end about 5 s, the filtered read and the 429 check about 4 s.
    - About **60 s** a try, bounded at 76 s (row-3967d's B row measured 76 s a try with its taps).
  - **Row O try:** 15 + 90 + 22 + up to 33 s for the story's end + 4 = about **136 s**, bounded at 150 s.
  - **At most:** 6 × 76 s + 4 × 150 s + 1.5 min = **19.1 min**.
  - **Expected:** 4 × 60 s + 3 × 136 s + 1.5 min = **12.3 min**.
- **The arithmetic:**
  - `NEED_MIN` = 1 + 2 × 19.1 + 5 min margin = 44.2, so **44**.
  - Expected for the whole run: 1 + 2 × 12.3 ≈ **26 min**.
- **Book 45 minutes** (`HARD_END` = start + 45).

## Canned DRY matrix
`canned-cutin/make.sh OUTDIR` builds the canned events: one directory per scenario, with `PC`, `PO`, `WC` and `WO`
on the DRY clock. `canned-cutin/matrix.sh` builds them in `M` (default `$TMPDIR/dry-cutin-m`), runs every case with
no lamp, no audio, no clips and no post, and prints one line per case. Seen on 31 Oct 14:4x from a fresh checkout:
**32/32 PASS**, and no Python traceback in any run.

| Case | Knob | Expected line |
|---|---|---|
| pass | `DRY_CASE=pass` | `VERDICT: PASS (W: cut-ins 4/4, 0 wordless commits against P's 3, median onset->yield 420 ms against 400)` |
| no repro | `DRY_CASE=norepro` (P row O no commit) | `VERDICT: NO REPRO …` |
| wordless on W | `DRY_CASE=wordless` | `VERDICT: FAIL (W made 3 wordless commits)` |
| cut-in held and resumed | `DRY_CASE=missed` (words 0) | `VERDICT: FAIL (a real cut-in on W did not commit: resume,…)` |
| slower yield | `DRY_CASE=slowyield` (700 vs 400 ms) | `VERDICT: FAIL (time to yield changed …)` |
| void, no answer begun | `DRY_CASE=void` | `VERDICT: VOID (valid tries: P C 4, P O 3, W C 0, W O 3 …)` |
| void, asks unanswered | `DRY_NOANSWER=WC` | the same VOID |
| 5 tries per arm | `C_TRIALS=5 O_TRIALS=5` | `VERDICT: PASS (W: cut-ins 5/5, 0 wordless commits against P's 5, …)` |
| camera | `NO_PERSON=` | `would stop here: manual check …` |
| lamp-update switched | `DRY_SWITCHED=…` | `would stop here: lamp-update switched …` |
| foreign drop-in | `DRY_FOREIGN=zz-cutin-check.conf` | `would stop here: another /run drop-in …` |
| allowed drop-in | `DRY_FOREIGN=zz-product-u.conf ALLOW_DROPINS=zz-product-u.conf` | `allowed /run drop-ins (kept, ALLOW_DROPINS): zz-product-u.conf`, then `VERDICT: PASS` |
| allowed plus another | `DRY_FOREIGN='zz-product-u.conf zz-cutin-check.conf'`, the same allow | `would stop here: another /run drop-in on lamp-voice: zz-cutin-check.conf` |
| allowed, sorts after the kit's | `DRY_FOREIGN=zzzzz-late.conf ALLOW_DROPINS=zzzzz-late.conf` | `would stop here: allowed drop-in zzzzz-late.conf sorts after …` |
| no ffmpeg | PATH without ffmpeg/ffprobe, `say` clips (AIFF, `canned-cutin/say-tts.sh`), `DRY_TTS=yes` | `clips: levelled by python3`, `VERDICT: PASS`, every clip −18.0 dB |
| pre-levelled | the same, `WAVS_LEVELLED=yes` | `clips: levelled by none` |
| hard end, refusal | `HARD_END=now+16` | `would stop here: the hard end … leaves 16 min, under the 44 min` |
| hard end, stop | the same | `stopped at the hard end … before WO try 3` |
| SEQ read empty once / twice / moved | `DRY_SEQFAIL=1` / `2` / `DRY_SEQ=9070` | the retry line / `the lamp left 9063 (now )` / `(now 9070)` |
| exe not installed | `DRY_EXE=…` | `arm P: lamp-voice runs …, not the installed …` |
| P environ has a switch | `ARMS=P DRY_ENVIRON='1 0'` | `arm P: lamp-voice has LAMP_STOP_WORDS or …` |
| W environ short | `ARMS=W DRY_ENVIRON='0 1'` | `arm W: … are not both (and only) in lamp-voice's environ` |
| W startup: holds not on | `DRY_STARTUP=` the "held only with" line | `arm W: no 'stop words: heard, in a yield' line` |
| W startup: stop words only | `DRY_STARTUP=` the stop-words line | `arm W: no 'backchannel holds' line` |
| hello | `DRY_HELLO=fail` | `arm P: hello not heard` |
| stray afplay | `DRY_STRAY=99999` | `FAIL a stray afplay before PC 1`, then a QUIET with "stopped early" |
| put-back failed | `DRY_PUTBACK='PUT-BACK FAILED: drop-in-left'` | QUIET `Put-back: FAILED …`, and `PUT-BACK FAILED:` exit 3 |
| own lamp PLAYING | `STUB_PLAYING=52e6` (the kit's L is lamp-52e6) | not refused: `VERDICT: PASS …` |
| TERM mid-try | `DRY_REALNAP=1`, `kill -TERM` | QUIET within 2 s (0.09 s), with the put-back through `systemd-run` |
| TERM then KILL (lamp-lock) | group TERM, KILL 1 s later, `DRY_GH=stub` | gh stub got `issue comment 3336 … QUIET … stopped by TERM` |

The score was also run on the real 30 Oct events (`out-3967d8`, arm N). It agrees with `score-3967.py` per try:
- N1A 1 and 3 were cut by the bed, read here as 1 wordless commit each;
- N1B 2 went on, read here as no yield;
- N1B 4 ended before the phrase.

## Open questions
- **Which rule:** Twelve (07:12) reads 4ace's strings as rule (b), 40 still blocks after ≥ 10 plain `Over`, not (a).
  The hold covers both, since `wordless` holds any `due` commit, so the A/B is the same either way. Only the
  description changes.
- **Repro on 52e6:** P may show no wordless commit in 3 × 90 s. NO REPRO is then the honest result. Raise
  `O_TRIALS`/`O_MAX` if the first builder wants a longer own-tail row (each try is about 2.3 min).
- **Slot and posts:** the slot is asked on #1711 (the first builder: 52e6's slots are the lead's verifiers'). PLAYING
  and QUIET go on #3336, as in row-3967d. Also check that lamp-4ace's iMac is not playing in earshot.
