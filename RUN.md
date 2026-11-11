# Kit 5194: the played before/after of LAMP_OPENS_FURTHER on lamp-06c1

`K=/private/tmp/claude-501/-Users-macbookpro-orca-workspaces-autonomous-os-lamp-jarvis/c9e8347e-3e4a-4167-8838-79b9a1c0744f/scratchpad/kit5194`

**The done-when (#5194):** a played before/after on lamp-06c1 with 20 everyday lines in the testers' kind of talk. With the switch on, at least 8 of 20 answers carry a fitting opening, 0 of the command, stop and busy-room lines do, and the PM reads the openings in the transcript as natural.

**The switch:** `LAMP_OPENS_FURTHER=on` on lamp-voice, from #5235, merged as `29cc36da91` in train 678. It is V1's "Leave a soft door, not a questionnaire" (`robots/lamp/SOUL.md`). The voice journals one line per spoken request:
- `lamp-voice second: request N: the model is told its answer may end with one short opening (LAMP_OPENS_FURTHER)`, or
- `… request N: no opening further: <why> (LAMP_OPENS_FURTHER)`.

## What runs

- **Stock build, no binaries staged.** The only change is a `/run` drop-in, `/run/systemd/system/lamp-voice.service.d/zz-kit5194.conf`, holding `Environment=LAMP_OPENS_FURTHER=on`, between the two passes. A reboot clears it.
- **Restarts.** The only unit restarted is lamp-voice: once when the switch goes on, and once at put-back. Each time it is stopped alone first and checked for `Result=success` (a clean SIGTERM releases the restart guard), then started.
- **Never touched:** lamp-update and lamp-update-back are never stopped, and the kit never takes the updater's lock or touches a `current` link. setup, switch and put-back wait up to 20 min for an update round, then refuse (exit 2, OWED).
- **Two passes of 28 rows, the same rows each pass.** First with the switch off (stock), then on:

| Rows | Kind | What is played (voice Karen, `say -r 185`, every line opens with "Lamp,") |
|---|---|---|
| E01–E20 | 13 `feel` lines, 7 `plain` | Everyday lines. In the `feel` lines a feeling or the person's own day sits under the ask: wiped out after work, a friend's birthday, an exam tomorrow, a new baby, no sleep, a boss who yelled, a sick dog, an interview, missing mum, cooking for six, a rainy week. The `plain` lines are weather, cups in a litre, a movie, Japanese thanks, the capital of Canada, octopuses and the blue sky. A door on a `plain` line counts toward the 8, but it is shown apart as a possible nag. |
| C1–C4 | command | "set a timer for five minutes", "cancel the timer", "lights off", "turn the light back on". C2 and C4 undo C1 and C3. |
| S1, S2 | stop | A long ask ("tell me a long story about a lighthouse keeper" / "…the history of tea"). 4 s into the lamp's speaking, "Lamp, stop." / "Stop, Lamp.". The row's reply is what comes after the stop. |
| B1, B2 | busy room | `talk.aiff` (night 7's two-voice room talk, Samantha and Daniel, 176 s) starts at `afplay -v 0.6`. 30 s later, over it: "Lamp, what's the capital of Japan?" (a short factual check) and "Lamp, I'm so tired after today. What time is it?" (a feeling, in a busy room). |

- **Every row is its own conversation.** After a reply ends, the driver waits 50 s before the next row. The voice's conversation window, `GOING_ON_FOR`, is 45 s, so the gates "at most two openings in a conversation" and "never right after an opening" never join two rows.
- **How a reply is read:**
  - The bus tap of `voice/said` gives the lamp's text and request number. `voice/heard` ties each request to the played line.
  - The journal gives the gate line per request.
  - `room/state` gives quiet or busy.
  - If `voice/said` has nothing in a row's window, the voice session's `transcript` events are used instead (`q.sh events`, marked `events`).
- **Where things are recorded.** The recorders are transient units `kit5194-said|heard|vstate|room|mute|follow|checks`, writing to `/run/kit5194/logs`. put-back stops them.
- **Time:**
  - A row is about 75 s: clip, reply, 50 s gap.
  - A stop row is about 90 s; a busy row is about 110 s.
  - A pass is about 38 min.
  - Setup takes about 5 min, the switch 1 min, put-back 2 min.
  - **About 1 h 30 min in all**, so post the HOLD for 1 h 45 min.
  - The driver will not play on a weekday from 12:00 to 14:00, when the office rests (`LAMP_PLAY_AT_LUNCH=yes` overrides).

## Commands, in order

**0. Before (main session).**
- Post the HOLD on **#3336**:
  `HOLD lamp-06c1 · Jarvis · HH:MM–HH:MM (GMT+7) · #5194 played before/after of LAMP_OPENS_FURTHER: stock build + a /run drop-in on lamp-voice (Environment= only), lamp-voice restarted twice (switch, put-back), lamp-update untouched; 56 lines from the Mac speaker at 45 (kit5194.tgz <sha12>)`
- Wait for any earlier holder's `released` before touching the lamp.
- Post `PLAYING lamp-06c1 · #5194 · HH:MM` when the driver starts.
- Run the preflight. It is read-only. It must end `PREFLIGHT OK`: the installed commit carries `29cc36da91`, the voice is not muted, the lamp is not asleep, and lamp-voice has no `LAMP_OPENS_FURTHER` yet.
  ```sh
  zsh $K/mac/pre.zsh
  ```
- Check the clips (they are already rendered; `zsh $K/mac/render.zsh` remakes any missing one):
  ```sh
  (cd $K/clips && shasum -a 256 -c --quiet SHA256SUMS) && echo clips ok
  ```

**1. Stage (Mac → lamp `/run/kit5194`).** No AppleDouble files.
```sh
zsh $K/mac/stage.zsh      # rewrites lamp/SHA256SUMS, sh -n each script, builds $K/lamp5194.tgz with COPYFILE_DISABLE=1 --exclude='._*'
sshpass -p orangepi scp $K/lamp5194.tgz blue-lamp:/tmp/lamp5194.tgz
sshpass -p orangepi ssh blue-lamp "echo orangepi | sudo -S -p '' sh -c 'mkdir -p /run/kit5194 && tar -xzf /tmp/lamp5194.tgz -C /run/kit5194 && rm /tmp/lamp5194.tgz && find /run/kit5194 -name \"._*\"; ls /run/kit5194'"
```

**2. Setup (lamp awake, daytime).** It changes no unit. It records the before-state and starts the recorders.
```sh
sshpass -p orangepi ssh blue-lamp "head -c 300 /var/lib/lampos-update/state.json"      # installed.sequence
sshpass -p orangepi ssh blue-lamp "echo orangepi | sudo -S -p '' sh /run/kit5194/setup.sh <installed.sequence>" 2>&1 | tee $K/setup.out
```

**Setup refuses, having changed nothing, when any of these holds:**
- the sums are wrong, or there are `._` files;
- the sequence you typed is wrong;
- lamp-update is busy for 20 min;
- **the voice's binaries have no `LAMP_OPENS_FURTHER` and no "may end with one short opening" line** (a build older than #5235);
- lamp-voice is inactive, has someone's `/run` drop-ins, or already runs with the switch;
- the voice is muted, or **the lamp is asleep at start**;
- there is no lamp-bus;
- `/run/kit5194/state` exists.

**What setup records:**
- the installed build and the voice's binary path and sha256;
- argv, env, DropInPaths;
- the sha256 of every file in `/run/…/lamp-voice.service.d` and `/etc/…/lamp-voice.service.d`;
- links, units and `/run` units.

`setup.out` must end with `setup done … The switch is OFF (stock).`

**3. The driver (Mac).** It plays the off pass, runs `switch.sh on`, plays the on pass, then puts back, copies the logs and analyses them.
```sh
nohup zsh $K/mac/driver.zsh > $K/driver.out 2>&1 & echo $! > $K/driver.pid
tail -f $K/driver.log      # PLAY lines; touch $K/STOP to stop early (it then puts back)
```
- Before each row, the driver checks `voice/mute`. A muted voice stops the rows and puts back.
- `switch.out` must end with `SWITCH-ON-OK`, and should show `voice second worker: says LAMP_OPENS_FURTHER is on`.
- If the switch fails, `switch.sh` puts back by itself, and there is no on pass.
- To play only some rows or one pass: `ROWS="E01 E02" PASSES=on zsh $K/mac/driver.zsh`. A run of only the on pass still runs `switch.sh on` first.

**4. Put-back (by the driver; or by hand).**
```sh
sshpass -p orangepi ssh blue-lamp "echo orangepi | sudo -S -p '' sh /run/kit5194/putback.sh" | tee $K/putback.out
```
**What it does:**
1. Waits for lamp-update to be quiet.
2. Stops the recorders.
3. Removes the drop-in, and the folder if the kit made it.
4. Runs daemon-reload.
5. If the switch was on, restarts lamp-voice: stop alone, `Result=success`, then start.

**`putback.out` must show these SAME lines:**
- `SAME voice drop-in files (sha256, byte-equal)`;
- `SAME voice DropInPaths`, `SAME kit drop-in gone`;
- `SAME voice binary path`, `SAME voice binary sha256`;
- `SAME voice argv`, `SAME voice env`;
- `SAME LAMP_OPENS_FURTHER gone from the voice`;
- `SAME current links`, `SAME /run units`, `SAME /etc drop-ins`;
- `SAME active lamp-* units`, `SAME failed lamp-* units`;
- `SAME kit recorders stopped`;
- `SAME voice listening_ready`, `SAME voice/mute`.

It must also show `UPDATER self.json: none; state.json unchanged` and `PUTBACK-OK`, and its **last line must be `VOICE OK: not muted, listening.`**

**If the voice is muted:**
- If the restart guard muted it, put-back tries **one** clean voice restart (#4332).
- If it is still muted, put-back exits 3 with a loud `!!! THE VOICE IS MUTED`, and the driver writes `$K/VOICE-MUTED`. **Tell the person at once** (it needs a head tap) and say so in `released`.

**If `OWED`**, run put-back again once lamp-update is quiet.

**5. After.** Copy the logs (the driver does: `$K/logs/logs`, `$K/logs/state`). Then remove the kit and post `released` on #3336:
```sh
sshpass -p orangepi ssh blue-lamp "echo orangepi | sudo -S -p '' rm -rf /run/kit5194"
```
`released lamp-06c1 · Jarvis · HH:MM · #5194 before/after played, put back: drop-in gone, voice on stock (sha same), voice listening_ready, voice/mute false`

**6. Analysis (Mac; the driver runs it too).**
```sh
python3 -I $K/mac/analyze.py $K      # writes $K/transcript.tsv and $K/report.md
```

**`transcript.tsv`** has one line per row and pass. Its columns are:
- `row`, `switch` (off/on), `kind`, `line`, `heard`, `reply`;
- `has_door` (y/n), `door_kind` (question/offer/noticing);
- `gate` (the journal's `told` or `no: why`), `room`, `read` (said / said,unmatched / events / none), `requests`;
- **`pm_read`, left blank for the PM.**

**`report.md`** gives:
- off and on counts: doors on the 20 everyday lines (feel and plain apart), doors on the 8 zero-lines, no-reply rows;
- the bar's verdict on the counts;
- the gate tallies (any off row with a gate line means the switch leaked);
- every row's reply.

## Known limits

- **The has_door heuristic only counts.** It looks at the last sentence: a `?`, an offer opening as in `opens_further.rs`, or a short noticing ("that sounds…", "good luck", "hope…"). A clarifying question counts as a door, and a door phrased some other way is missed. The PM's read decides fit and naturalness.
- **Replies come from `voice/said`.** The voice publishes it only with `LAMP_BUS=on` (the product sets it). Setup refuses if `voice/state` writes nothing, but `voice/said` is first seen in the first row. If it stays empty, the session events are the fallback (`q.sh events` at each pass end).
  - A fixed line that is not the provider's text, such as a stop's "Okay, I'll stop there." (`LAMP_SPOKEN_STOP_ENDS`), may be on neither. Then a stop row reads `(no reply)`, which counts as no door. Check the stop rows' journal (`logs/logs/journal.txt`) by hand.
- **The busy rows depend on the room being judged busy.** The `room` column shows `room/state` at the line. In night 7, `talk.aiff` once woke the lamp by a false name hit. The analyzer ties replies to the played line through `voice/heard`, and marks the rest `unmatched`.
  - If the lamp holds the busy line back altogether (no reply), the row is a trivial 0. Say so in the post.
- **The same lines are played twice.** In the on pass the lamp's memory may recall the off pass ("you mentioned your exam"). The PM should read such a thread back as memory, not as the switch.
- **The timer.** If C2 fails to cancel C1's timer, the timer rings about 5 min later, during an E-row of the same pass. The analyzer ties replies by `voice/heard`, so the ring itself does not count, but note it.
- **Clocks.** Each row stamps the lamp's own clock (`q.sh now`) just before the clip, so there is no Mac-to-lamp offset. The clip itself starts about 0.1–0.3 s later.
- **No shellcheck on this Mac.** Every script passed `sh -n`, `bash -n` and `zsh -n`.
