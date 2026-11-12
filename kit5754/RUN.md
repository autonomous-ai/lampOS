# Kit 5754: rows 3 and 5 for comfort-break and meal-check, on lamp-52e6

This kit is for lampOS #5754. Whoever plays it, a person or an agent, is called "the player" below.

`K=/private/tmp/claude-501/-Users-macbookpro-orca-workspaces-autonomous-os-lamp-jarvis/c9e8347e-3e4a-4167-8838-79b9a1c0744f/scratchpad`. The kit is `$K/kit5754.tgz`; check its sha256 against the one V2 cleared.

## The rows (the PM's 12:1x answer on #5754)

| Row | What | Done when |
|---|---|---|
| 3, nobody at the lamp | comfort-break and meal-check, with nobody at the desk for each skill's full window | no `Triggered` and no `say` in `skills/events`, and nothing spoken, while the real voice is listening (so any `say` would be heard and logged) |
| 5, the words | comfort-break with someone at the lamp (a made-up face on the Mac screen, aimed at the lamp) | its spoken line captured in full from `voice/said` |

## What the skills do (origin/lamp-voice)

- **comfort-break** is a core skill. It is on through `[present]`, because the product's lamp-skills unit sets `LAMP_NUDGES_PRESENT=on`. It fires when one face has been at the desk (`near = 0.14`) for 90 min, with a 2 min `gap`. It also needs the eyes' `room/activity` `sitting_s` above 5399, nobody else in view for 1 min, the voice resting 5 s and not speaking for 30 s, and the person not head down. Further limits: `[active]` 09:00–20:00, `[quiet]` 22:00–08:00, `wait` 30 min, `cooldown` 90 min, 3 a day, and the wellbeing budget of 2 an hour at least 30 min apart. Its line is worded by the voice and says only "a short break".
- **meal-check** is installable (1.0.1, from the Store). It fires after 75 min at the desk inside lunch (11:30–13:30) or dinner (18:30–20:30), with the same conditions except `sitting_s`. Its `[quiet]` is 22:00–08:00, and it asks once per window, at most 2 a day.
- **Nobody at the lamp:** a `present` trigger never holds, so there is no `Triggered`, no `Asked`, no `Done` and no `<skill> not run` line. lamp-skills journals every event as `lamp-skills: <skill> <Stage>: <why>`.

## How time is compressed

- **Row 3 (MODE=fast, the default):** `row3.sh` restarts lamp-skills on its own stock binary with one `/run` drop-in, `zz-kit5754.conf`. The drop-in is the unit's own `ExecStart` plus `--clock-control /run/lampos-skills/kit5754-clock.sock`, which is the test clock in docs/skills.md ("A test's clock", `crates/skills/src/clock.rs`). `kit5754-skip` then skips the skills' clock by 125 min, in 60 s steps 1 s apart, with nobody at the desk. That covers comfort-break's 90 min `for` plus its 30 min `wait`, and meal-check's 75 + 30. 60 s of real time follows. The drop-in is then removed and lamp-skills restarted on its found argv. The row takes about 6 minutes.
  - **The wall clock is not skipped.** `[active]`, `[quiet]` and meal-check's windows stay real, so the row must run inside a meal window for meal-check to count. `row3.sh` refuses to start otherwise.
  - **The voice is the real one and is not touched:** a `say` would be spoken and land on `voice/said`.
- **Row 3 (MODE=real, the fallback):** no drop-in and no restart. It records from 11:30 to 13:31 (or 18:30 to 20:31), which covers both skills' full windows on the wall clock.
- **Row 5 can't be compressed.** comfort-break's `[present]` also needs the eyes' `sitting_s` above 5399. The eyes count it in real seconds (the product sets `LAMP_ACTIVITY=place`), and skipping lamp-skills' clock does not move it. So the face sits for 90 real minutes, as #5555 did.
- `lamp-bus replay` of the skills' own `tests/` cases isn't used. On the live bus a replay of `humans/faces/tracked` would interleave with the real eyes, and the skills' own test runner (`lamp-skills test`) uses a stand-in voice, not the real one.

## Why #5555's words were empty, and how row 5 gets them

- **Why they were empty:** `voice/said` carries the lamp's words in pieces with `finished: false`. The second route (ElevenLabs; "43 chars in 2 ElevenLabs pieces" on #5555) ends every line with one more piece that is **empty and `finished: true`** (`crates/voice/src/second_provider.rs`: "a last piece, empty and finished"). The `Speech` doc in lamp-msgs says the finished piece is "the whole of what was said". A reader that kept only that row got `""`. That is the likely reason #5555's export showed empty text.
- **How row 5 gets them:** `lamp-bus record` keeps every `voice/said` piece (payload in base64). `kit5754.py` finds comfort-break's `triggered` and `done` events on `skills/events` and takes the first request whose first piece came after the trigger. It joins **all** that request's pieces in order and prints each piece with `--pieces`. The voice's journal adds nothing here: it gives only char counts.

## Steps

### 0. Before (main session)

- **V2's kit yes** on this kit's sha256.
- **The 52e6 holder's GO.** The rows run after that holder's own switch-on rows. If 52e6 has a shared kit lock, take it the way the other 52e6 kits do. This kit takes no lock of its own.
- **Lifting the off file needs the 52e6 holder's explicit yes.** 52e6's `/var/lib/lampos-skills-state/off` holds comfort-break as a person's choice. Without `LIFT_OFF=yes`, setup stops and changes nothing. With it, setup saves the original and its sha256, lifts only the `comfort-break` line, and put-back writes the file back byte for byte, checked by sha256.
- **Post the HOLD on #3336:**

```
HOLD lamp-52e6 · <player> · HH:MM until about HH:MM (GMT+7) · #5754 rows 3 and 5 (comfort-break, meal-check): installed build, no overlays; row 3 restarts lamp-skills on its stock binary with one /run drop-in adding --clock-control (its own clock skipped 125 min), row 5 is real time with a made-up face on the Mac screen; the skills' off file lifted for comfort-break only with the 52e6 holder's yes and put back by sha256; a lamp-side dead-man puts back at the hard end (kit5754.tgz <sha12>, V2's yes)
```

### 1. Push the kit (Mac → lamp)

Use the address or alias you normally use for 52e6 (`<52e6>` below).

```sh
shasum -a 256 $K/kit5754.tgz                         # must match V2's yes
scp $K/kit5754.tgz root@<52e6>:/tmp/kit5754.tgz
ssh root@<52e6> 'mkdir -p /run/kit5754 && tar -xzf /tmp/kit5754.tgz -C /run && rm /tmp/kit5754.tgz && find /run/kit5754 -name "._*"'
```

The last command must print nothing. If you re-pack the kit, use `COPYFILE_DISABLE=1 tar --exclude='._*' -czf ...`.

### 2. Setup (root on the lamp, 08:00–22:00 only)

```sh
head -c 300 /var/lib/lampos-update/state.json; echo      # installed.sequence
LIFT_OFF=yes sh /run/kit5754/setup.sh <installed.sequence>   # LIFT_OFF=yes only with the 52e6 holder's yes
```

Setup stops before changing anything in any of these cases:

- the sequence you typed is wrong;
- an update round has been busy for 20 min, or `self.json` is there;
- there are /run drop-ins on lamp-skills;
- lamp-skills runs without `LAMP_NUDGES_PRESENT=on`;
- comfort-break is in the off file and `LIFT_OFF` isn't `yes`;
- meal-check is in the off file;
- the voice is muted;
- it's quiet hours.

What setup does:

- Writes the before-state to `/root/kit5754`.
- Starts the recorders (`kit5754-rec`: `skills/events`, `voice/said`, `voice/state`, `voice/mute`, `humans/faces/tracked`; `kit5754-journal`: lamp-skills, plus the voice's readiness and mute lines). `voice/heard` is never recorded.
- Arms the dead-man `kit5754-deadman`, which runs `putback.sh` at `HARD_END_MIN` (default 300 min).
- With `LIFT_OFF=yes`, lifts comfort-break and waits for lamp-skills' `the skills turned off read again`.

Setup restarts nothing. It must end with `setup done`. If it prints `NOTE: no listening_ready ... (rotated out?)`, the voice's own line has left the journal; put-back then reads that check as a NOTE, not SAME.

### 3. meal-check through the app's Store Add (a person, on a phone)

- Ask the person to open the app, go to Store → meal-check → **Add** on this lamp. Do this after setup, so put-back knows meal-check wasn't there before.
- Check on the lamp that it was added and loaded:

```sh
ls /var/lib/lampOS/skills/meal-check/skill.toml && journalctl -u lamp-skills -n 80 -o cat | grep 'loaded \[' | tail -1 | grep -o '"meal-check"'
```

If nobody can do the Add, row 3 still plays for comfort-break, and its meal-check line reads **OWED**.

### 4. Row 5, the words (real time, about 95 min)

- **When:** start between 09:00 and 10:30, or 14:00 and 16:30. With meal-check installed, `row5.sh` refuses a start whose 90 min end falls in 12:45–14:00 or 19:45–21:00, because meal-check would take the wellbeing group's turn first.
- **The face:** put the made-up face (a generated face, no real person) on the Mac screen, aimed at the lamp, at the desk distance the other 52e6 kits use. Nobody else may be in the camera's view. No sound is played from the Mac. If any is, the Mac volume stays at 45 or below.
- **Post the PLAYING line on #3336:**

```
PLAYING lamp-52e6 · <player> · HH:MM until about HH:MM (GMT+7) · #5754 row 5, comfort-break's words: a made-up face on the Mac screen, no person, no Mac sound; installed build, lamp-skills on its stock argv; comfort-break's off line lifted (the 52e6 holder's yes); line due about HH:MM; hard end HH:MM (dead-man) (kit5754.tgz <sha12>, V2's yes)
```

- **Run it:**

```sh
systemd-run --unit=kit5754-row5 --collect sh /root/kit5754/row5.sh
tail -f /root/kit5754/logs/row5.out          # a line every 5 min; ends with "ROW5 end"
```

- It stops at once if there is no face at the desk.
- It ends 45 s after `lamp-skills: comfort-break Done`, or after `WAIT_MIN` (130 min).
- Then hide the face.

### 5. Row 3, nobody at the lamp (fast: about 6 min)

- **When:** inside a meal window, 11:30–13:20 or 18:30–19:48, so meal-check's window is open. comfort-break's `[active]` covers both.
- **Before you start:** the Mac's face is hidden and nobody sits at the desk. `row3.sh` stops if the eyes see a face at the desk.
- **Post the PLAYING line on #3336:**

```
PLAYING lamp-52e6 · <player> · HH:MM until about HH:MM (GMT+7) · #5754 row 3, nobody at the lamp (comfort-break, meal-check): no face, nobody at the desk; lamp-skills restarted on its stock binary with one /run drop-in adding --clock-control, its own clock skipped 125 min in 60 s steps, then the drop-in removed and lamp-skills restarted on its found argv; the real voice listening (kit5754.tgz <sha12>, V2's yes)
```

- **Run it:**

```sh
systemd-run --unit=kit5754-row3 --collect -E MODE=fast sh /root/kit5754/row3.sh
tail -f /root/kit5754/logs/row3.out          # ends with "ROW3 end"
```

- **Fallback, MODE=real:** start it between 11:30 and 11:35 (or 18:30 and 18:35). It records until 13:31 (or 20:31) with no drop-in.
- Rows 3 and 5 can be played in either order. Neither may overlap the other.
- To stop either row early: `touch /root/kit5754/STOP`.

### 6. Put-back (root on the lamp)

```sh
sh /root/kit5754/putback.sh                  # KEEP_MEAL=yes sh ... keeps a meal-check the person added
```

What put-back does:

- Waits for lamp-update to be quiet. Exit 2, **OWED**, means nothing was changed: run it again later.
- Stops the kit's units and the dead-man.
- Removes row 3's drop-in if it is still there, and then restarts lamp-skills.
- Removes a meal-check that wasn't on the lamp at setup, unless `KEEP_MEAL=yes`.
- Writes the off file back byte for byte, with its owner and mode, and waits for lamp-skills to read it again.
- Waits 5 s, then checks:

| Check | Must read |
|---|---|
| lamp-skills binary, argv, env, ExecStart | `SAME` |
| clock drop-in gone, /run units, /etc drop-ins, current links | `SAME` |
| active and failed lamp-* units | `SAME` |
| off file sha256, owner/mode | `SAME` (byte-equal to the saved original) |
| `lamp-skills keeps off:` | the same list as before the kit |
| skills folder | `SAME` (meal-check removed), or a NOTE with `KEEP_MEAL=yes` |
| kit units gone | `SAME` |
| voice/mute | `SAME` (`"muted":false`) |
| voice listening_ready | `SAME`: the voice's current run said it (the kit never restarts the voice); a `NOTE` only if it had already left the journal at setup |
| UPDATER | `self.json: none; state.json unchanged` |
| `._ files` | `0` |

The last line is `PUTBACK-OK: …` or `PUTBACK-DIFF: <what>`.

### 7. Verdict (Mac)

```sh
mkdir -p $K/kit5754-out && scp -r root@<52e6>:/root/kit5754 $K/kit5754-out/
python3 -I $K/kit5754/kit5754.py $K/kit5754-out/kit5754 --pieces | tee $K/kit5754-out/verdict.txt
ssh root@<52e6> 'rm -rf /root/kit5754 /run/kit5754'
```

**Before posting, read the words.** The voice may address a known face by name. Replace any person's name with `[name]`. No names or handles go in any post.

### 8. QUIET on #3336, then the result on #5754

```
QUIET lamp-52e6 · <player> · HH:MM (GMT+7) · #5754 rows 3 and 5: <the ROW3 and ROW5 lines>. Put-back: <the PUTBACK line>. FREE.
```

## Pass and fail lines (exactly as `kit5754.py` prints them)

- **Row 3, PASS:** `ROW3 <skill> PASS (fast: 125 min skipped on lamp-skills' own clock in 60 s steps, HH:MM:SS-HH:MM:SS real[, inside the lunch window]): nobody at the desk (0 faces at the desk in N looks), no Triggered, Asked or Done on skills/events, no not-run line, so no say; the voice listening and not muted throughout`
- **Row 3, FAIL:** `ROW3 <skill> FAIL (...): with nobody at the desk it still did something: <stage at time: why | the not-run line>`
- **Row 3, VOID:** `ROW3 <skill> VOID (...): <a face at the desk in N of M looks | the voice was muted | no voice/state | only N min of its clock passed, under its 120/105 min (for + wait) | not inside a meal window>`
- **Row 3, not installed:** `ROW3 meal-check OWED: not loaded on the lamp in the row (installed through the app's Store Add first)`
- **Row 3 notes:** `ROW3 NOTE voice/said: nothing at all was said in the row`, or `ROW3 NOTE something else was said in the row (request N, HH:MM:SS): "<text>"` (another part speaking, not these skills)
- **Row 5, PASS:** `ROW5 comfort-break PASS: Triggered HH:MM:SS (humans/faces/tracked has held for 5400 s), Done HH:MM:SS (look at the face: told the body; say: spoken); request N, C chars in P pieces (+1 empty end piece): "<the whole line>"`
- **Row 5, rule 8:** `ROW5 RULE8 ok: N sentence(s); names no bathroom, toilet or health word (list in kit5754.py)`, or `ROW5 RULE8 CHECK: the line names [...]: read it before posting`
- **Row 5, FAIL:** `ROW5 comfort-break FAIL: no Triggered in N min with a face at the desk (...)`, or `... Triggered ... but no Done in the row`, or `... Done ... says "<why>", not spoken`
- **Row 5, words FAIL:** `ROW5 comfort-break WORDS FAIL: Done HH:MM:SS says spoken, but voice/said carried no piece after HH:MM:SS`, or `... request N had P pieces, all empty`
- **Row 5, VOID:** `ROW5 comfort-break VOID: no Triggered in N min, and the face was not held at the desk (...)`; any `STOP` in a row's output also counts as VOID

## Safety rules

- **lamp-update:** never stopped, restarted or masked. Neither is lamp-update-back. The updater's lock is never taken and a `current` link is never touched. Setup and put-back wait for a quiet updater.
- **Only lamp-skills is restarted,** and only by row 3 (twice) or by a put-back that finds row 3's drop-in. The voice, eyes and body are never restarted.
- **Drop-ins:** /run only, `zz-kit5754.conf` on lamp-skills, in row 3 alone.
- **Quiet hours:** every script refuses to run from 22:00 to 08:00.
- **The Mac:** no sound is played. If any is, the volume stays at 45 or below.
- **Archives:** any tar pushed to the lamp is built with `COPYFILE_DISABLE=1 tar --exclude='._*'`.
- **Not recorded:** no memory or user data is touched, and `voice/heard` (people's words) is not recorded.

## Known limits

- **Restarts forget snoozes.** Row 3 fast restarts lamp-skills twice. A restart forgets a "not today" snooze said on the lamp that day, since snoozes live in memory only; an off kept in the file stays. Setup and a normal put-back restart nothing.
- **Row 5 uses one of comfort-break's 3 a day** (daily.json). That count is not put back: it starts again at local midnight.
- **Row 3 fast compresses only the skills' own timers.** The meal window is real. MODE=real is the literal full window.
- **The helper binaries:**
  - `lamp-bus` is kit5526's aarch64 cross-build (the bus crate, only reading and recording here).
  - `kit5754-skip` is a std-only Rust helper cross-built on the Mac (`src/kit5754-skip.rs`). It writes `{"skip_us": N}` lines to the clock socket and reads the `{"now_us": …}` answers.
- **Not yet played on a lamp.**
  - The scripts pass `dash -n`.
  - `kit5754.py` was checked on made-up recordings, both PASS and FAIL.
  - `kit5754-skip` was checked against a stand-in socket on the Mac.

## Files

| File | What |
|---|---|
| `setup.sh`, `putback.sh`, `row3.sh`, `row5.sh`, `common.sh` | run as root on the lamp |
| `lamp-bus`, `kit5754-skip` | aarch64 helpers |
| `kit5754.py` | the verdict lines, on the Mac |
| `src/` | the skip helper's source |
| `SHA256SUMS` | checked by setup |
