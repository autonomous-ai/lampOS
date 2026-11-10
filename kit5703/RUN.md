# Kit 5703: the played row for lampOS #5703 (lamp-body stands from a found pose)

This kit is for lampOS #5703 at head `5caac04a46496e245a723b1fbddc8859d1f18925`. The person who plays it with their hands (Fifth on lamp-52e6, or another holder) is called "the player" below. The builder who sets it up and puts it back is called "the operator". They can be the same person.

`K=/private/tmp/claude-501/-Users-macbookpro-orca-workspaces-autonomous-os-lamp-jarvis/c9e8347e-3e4a-4167-8838-79b9a1c0744f/scratchpad`. The kit is `$K/kit5703.tgz`; check its sha256 against the one posted with the kit.

## The row (the first builder's ask on #5703, 19:1x, comment 6739812163)

| Row | What | Kit step | Done when |
|---|---|---|---|
| 1 | Three power-ups in poses from El's sweep, including the drooped one and a nearly folded arm. The lamp stands within the start time. | `row.sh 1a`, `row.sh 1b`, `row.sh 2` (the third pose is row 2's) | READY under 180 s, the journal's "it folds first" line, no load, strain or overload stop, no systemd restart, and then it stands when spoken to |
| 2 | A fold that would tip waits for a hand. The lamp says so, or stays quiet because `LAMP_BODY_ASK` is unset in the product. A hand folds the arm, and the lamp stands. | `row.sh 2` | "it lets its arm go and waits for a hand to fold it" in the journal, the arm not moved by the lamp, quiet (or spoken, if `LAMP_BODY_ASK=on`), "set straight: taken hold" after the hand, then it stands |
| 3 | Nothing touches the desk or the hand on the way up. It looks like a lamp getting up, not a machine homing: no jerk, no sweep. | asked y/n at the end of every `row.sh` | the player's own judgement, two y/n questions per row |
| 4 | Core rows 1–4 after. | by voice, after the last power-up row (below) | each answered as in joining.md |

`row.sh` prints `RESULT row X: row1 PASS|FAIL | row2 … | row3 …` with the timings, and appends it to `/root/kit5703/logs/results.txt`. `putback.sh` prints that file again at the end.

## What is staged

- **`lamp-body`**, from this head's CI artifact: "lampOS on ARM64" run 42590674657 for `5caac04a46`, artifact `lampos-arm64-5caac04a46496e245a723b1fbddc8859d1f18925`. It is ELF 64-bit aarch64 Linux, sha256 `c2f6b9528960accd9ae2edf35c83c0c7b50b307a0ec9d9933e16ac410bd01886`, the same as the artifact's own SHA256SUMS line.
- **Only lamp-body.** The keeper's change (`Notify::extend_timeout`, `EXTEND_TIMEOUT_USEC`) is in the `lamp-keeper` library, which is linked into lamp-body (`crates/body/Cargo.toml`: `lamp-keeper = { path = "../keeper", default-features = false }`). The staged lamp-body contains the string `EXTEND_TIMEOUT_USEC`; the `lamp-keeper` binary does not use it.
- **The drop-in** `/etc/systemd/system/lamp-body.service.d/zz-kit5703.conf`:
  - `ExecStart=` is the found unit's own ExecStart, with only `/opt/lampOS/body/current/lamp-body` changed to `/var/lib/kit5703/lamp-body`. `$BODY_POSES` and `%H` stay as they were.
  - `TimeoutStartSec=180`, as #5703's product unit has.
  - It is in /etc, not /run, so a real power cut (`row.sh X power`) comes back on the staged binary.
- **Where the binary goes:** `/var/lib/kit5703/lamp-body`. lamp-body runs as `lampos`, so the binary can't be in `/root` (not traversable by lampos) or `/run` (may be noexec). setup.sh checks that lampos can exec from /var/lib.
- **A dead-man timer**, `kit5703-deadman.timer`, runs `putback.sh deadman` at the hard end (default 240 min after setup). It is `Persistent=true` and enabled into `timers.target`, so it also runs after a power cut if the hard end passed while the lamp was off.
- **What the kit never touches:** lamp-update and lamp-update-back are never stopped or restarted, and the updater's lock is never taken. setup, row and putback wait or refuse while an update round runs. The only unit the kit restarts is lamp-body. The voice is never restarted by the kit; only a real power cut restarts it.

## Why the stand-in counts, and when a real power cut is needed

A plain `systemctl restart lamp-body` is **not** a power-up:
- Asked to stop, the body eases home and keeps its torque "until the power goes" (body.md, "When it stops"; `--let-go-at-stop` is off in the product), and a standing lamp folds first.
- The next start finds every joint held. A held arm can't be posed by hand.
- #5703 lets an arm go for a hand only when the shoulder and the elbow were both **found torque-off**. A held arm whose fold would tip is refused and kept held (`a_held_arm_whose_fold_would_tip_keeps_holding_and_is_not_let_go`). So row 2 can't be played on a plain restart.

**The kit's stand-in (`row.sh X`, the default)** adds the product's own park before the power goes:
- `lamp-body-measure let-go`, as the body's user, the same call `lamp-setup shutdown-now` makes (shutdown.md). It writes torque off on all five, only for a lamp that is down, and reads each joint's torque back.
- The player then poses the limp lamp, with a live read-only readout from `lamp-body-inspect` (no register written, no torque, no motion).
- Then lamp-body starts and finds every joint torque-off where a hand left it, which is what it finds at power-up. That is the input #5703's code acts on: `arm_found_off`, take-hold, `fold_plan`, the anti-tip gate.

**What the stand-in does not cover:**
- **Goal registers.** After a real power-up they read 0, not the last positions. The body writes each goal as its position before it turns torque on, so this should not matter, but only a power cut shows it.
- **The start under a booting lamp's load.** Every other part starts at the same time.
- **How a powered-off arm settles.**

So play rows 1a, 1b and 2 with the stand-in, then **play 1a once more with a real power cut** (`row.sh 1a power`, then `row.sh 1a after`) if the holder agrees. 1a is the slowest start, about 103 s in the simulator and 108.7 s in El's sweep, so it is the row that tests the 180 s backstop and the `EXTEND_TIMEOUT_USEC` chain. If the holder doesn't want a power cut, say "stand-in only" in the PASS post. The first builder's ask says "a power-up", so it is their call whether the stand-in is enough.

The power cut uses the product's safe-off, `lamp-setup shutdown`, which never pulls the plug on a running lamp:
- It says "Going to sleep now. Unplug me when my light turns green.", folds the lamp, lets it go, and lights the ring green.
- It refuses while an update installs (it holds lamp-update's lock).
- The voice is stopped first, so its restart guard does not mute it at the next power-on (shutdown.md, `VOICE_FIRST`).

## Safety (the player reads this first)

- **The arm moves by itself.** At every start in this kit, lamp-body may lift the head, then fold the elbow, then the shoulder, slowly (40 counts a second, about 3.5 degrees a second), one joint at a time, then wake the head. In row 2 it may let the arm go limp instead.
- **Keep your hands off the lamp except when the script asks you to pose it or fold it.** When it asks you to take your hands off, take both off and step back to arm's length before you press Enter.
- **Clear the space.** Nothing within an arm's reach of the lamp on the desk: no cup, no phone, no cable over the arm's path. Put the lamp's base squarely on the desk, away from the edge. There is no collision model (#5703's PR body): if the lamp hits something, it is because something was in the way.
- **Row 2 may drop the arm.** The lamp lets its arm go limp. It may sag or fall under its own weight, and the lamp may rock. Keep one hand near the head (not touching it) to catch it.
- **To stop it:** in the kit's terminal press Ctrl-C, then run `systemctl stop lamp-body`. The body finishes or holds where it is within about 3 s and never jumps (a standing lamp folds as it stops).
- **If something is about to be pinched or hit**, pull the power plug. The lamp goes limp at once, so support the head with your other hand as you pull. Tell the operator: a pulled plug needs a check of the SD card and the voice after boot.
- **If a start fails**, systemd starts lamp-body again by itself after 15 s (`Restart=on-failure`), and the arm may move again then. Stay clear until the operator says it is settled.
- **Speak or point, never push.** A hand pushing the moving arm is a load stop, and the body then lets the whole lamp go limp.

## The poses (El's 572-start sweep on #5703)

El's sweep (comment of 13:20, #5703) ran 572 starts on lamp-52e6's poses:
- shoulder 1,633 to 3,073 in steps of 120;
- elbow 1,649 to 2,899 in steps of 125;
- head pitch 1,771, 2,030, 2,250 and 2,420;
- yaw and roll 2,035.

Counts are lamp-52e6's, in joint order **yaw, shoulder, elbow, roll, pitch**. lamp-52e6's own poses are home pitch 1,771, folded arm 2,053/2,069 and stand 2,657/2,602. One count is about 0.09 degrees, and a higher head pitch is the head further down. I ran each pose below through this head's own simulator (`powered_up` and `fold_plan` in `crates/body/src/body/tests.rs`, as a throwaway test, not committed).

| Row | Counts (52e6) | In plain words | Simulator at 5caac04a46 |
|---|---|---|---|
| 1a, the drooped one | `[2035,2593,2899,2035,2420]` | The lamp drooped: the arm up near its standing height (shoulder about 6 degrees under it), the elbow bent well past where it stands (about 26 degrees past), and the head hanging down hard (about 57 degrees below home). This is El's slowest start. | It lifts the head to 2,030, folds the elbow, then the shoulder, and wakes. READY at 102.8 s (El's sweep: 108.7 s). It stands. |
| 1b, a nearly folded arm | `[2035,2113,2024,2035,1771]` | Nearly folded: the lamp at rest, but the shoulder lifted a little (about 5 degrees, 60 counts) and the elbow pushed a little past its rest (about 4 degrees, 45 counts). Head at home. This is El's P2 pose and the test `a_nearly_folded_arm_off_its_way_folds_and_stands`. | It folds the elbow, then the shoulder. READY at 4.4 s. It stands. |
| 2, a Tips pose | `[2035,2833,2524,2035,2030]` | The arm raised past its standing height (shoulder about 15 degrees past), the elbow just short of where it stands (about 7 degrees short), and the head tipped down to where it meets the base when folded (2,030, about 23 degrees below home). | `Tips`: "folding it from here takes its centre of gravity 19 mm off its base's axis, farther than found or folded (18 mm)". READY at 0.1 s, then `ARM_FOLD`: it waits for a hand. |

**The Tips pose** is El's "shoulder 2833 with the elbow 2274–2899" case, only 1 mm over the bound. The PR asks the row to check whether the bound plus `FOLD_LEAN_SLACK` is stricter than the real lamp needs, and row.sh asks the player whether the lamp looked at risk of tipping. In the simulator, shoulder 2,833 with the head at home (1,771) **folds** and stands rather than tipping. Every elbow from 2,274 to 2,899 is `Tips` only with the head at 2,250 or 2,420, and at 2,030 for elbow 2,524 or 2,899. Pitch 2,030 is used here so that, once the arm is folded, the head is within the wake's 400 and the lamp stands. At 2,250 it would also ask a hand to lift the head (#2602), a second wait. A clearer Tips pose, if the player wants one, is `[2035,3000,3000,2035,1771]`: 28 mm against 20 mm, the test `a_fold_path_over_the_anti_tip_bound_is_refused_and_nothing_moves`.

**On a lamp other than 52e6**, row.sh moves each pose onto that lamp's own `lamp.env` poses:
- yaw, roll and pitch by the offset from `--home`;
- shoulder and elbow by the offset from `--folded-arm`, in the direction of `--stand`.

It prints the target counts next to the live ones. On 52e6 the targets equal the counts above. A pose within 40 counts of the target is shown `ok`; 80 counts or more off asks before going on. If the arm won't stay where it is put and settles a little lower, that is fine: a power cut leaves it so too, and the journal's "found shoulder … elbow … head pitch …" line records where it really was.

## Steps

### 0. Before (the operator, on the Mac)

1. **Agreements:**
   - V2's kit yes on this kit's sha256.
   - The lamp holder's GO, with their hands. On 52e6 that is Fifth.
   - The holder's yes for one real power cut (row 1a power), or "stand-in only".
2. **The installed build:**
   - Read the lamp's `head -c 300 /var/lib/lampos-update/state.json` (installed sequence and commit).
   - Then on the Mac, in a lampOS checkout with origin fetched, run:

```sh
I=<installed commit>
git log --oneline $I..5caac04a46 -- lampOS/crates/body lampOS/crates/keeper lampOS/crates/msgs lampOS/crates/bus lampOS/crates/ipc   # what the staged body brings beyond the installed build
git log --oneline 5caac04a46..$I -- lampOS/crates/body lampOS/crates/keeper lampOS/crates/msgs lampOS/crates/bus lampOS/crates/ipc   # body changes the installed build has and the staged body lacks
```

   - The first list should be #5703 plus lamp-voice body changes the lamp would get anyway.
   - If the second list has a body change that matters to the row, say so in the HOLD.
   - Nothing in either list may change the bus's message format. If one does, stop and ask the first builder.
3. **Post the HOLD on #3336:**

```
HOLD lamp-<id> · <player> · HH:MM until about HH:MM (GMT+7) · #5703 played row with hands: lamp-body from 5caac04a46's ARM64 artifact (run 42590674657, sha256 c2f6b952…) staged by an /etc drop-in (ExecStart + TimeoutStartSec=180), lamp-body restarted only, lamp-update untouched; three power-ups by the product's let-go park (+ one real power cut if agreed), then core rows 1–4; put back by sha256, dead-man at the hard end (kit5703.tgz <sha12>)
```

### 1. Push the kit (Mac → lamp)

```sh
shasum -a 256 $K/kit5703.tgz                      # must match the posted sha256
scp $K/kit5703.tgz root@<lamp>:/tmp/kit5703.tgz
ssh root@<lamp> 'mkdir -p /run/kit5703 && tar -xzf /tmp/kit5703.tgz -C /run && rm /tmp/kit5703.tgz && find /run/kit5703 -name "._*"'
```

The last command must print nothing. If you re-pack the kit, use `COPYFILE_DISABLE=1 tar --exclude='._*' -czf ...`.

### 2. Setup (the operator, root on the lamp)

```sh
head -c 300 /var/lib/lampos-update/state.json; echo      # installed.sequence
sh /run/kit5703/setup.sh <installed.sequence>
```

**Setup stops, having changed nothing, when any of these holds:**
- the sums are wrong, or the staged lamp-body is not the artifact's sha256;
- the sequence you typed is wrong;
- an update round has been busy for 20 min;
- lamp-body is not active, or has /run drop-ins, or its ExecStart is overridden or not `/opt/lampOS/body/current/lamp-body …`;
- `lamp.env` has no `--home`, `--folded-arm` or `--stand` (a seated lamp);
- lampos can't exec from /var/lib;
- the voice is muted;
- `/root/kit5703` or `/var/lib/kit5703` is left from an earlier run.

**What setup records first, in `/root/kit5703`:**
- the found ExecStart (`execstart.txt`);
- the running binary's path and sha256 (`stock-exe.txt`, `stock-sha.txt`);
- the installed build (`build.txt`, `state-before.json`) and the `current` links;
- `TimeoutStartUSec`, the /etc drop-ins, the active and failed lamp units, the voice's mute and readiness.

**Then it:**
- stages the binary;
- writes the drop-in;
- arms the dead-man;
- checks `TimeoutStartUSec` is `3min`;
- restarts lamp-body once.

A standing lamp folds as it stops, then starts from folded as every day. Setup checks that the running binary is `/var/lib/kit5703/lamp-body` with the staged sha256, and that its start had no stop line. It ends with `setup done`.

### 3. The power-up rows (the player at the lamp; the operator can drive the terminal)

Run each in an interactive ssh session (`ssh -t`). Each asks before anything moves.

```sh
sh /root/kit5703/row.sh 1a     # drooped
sh /root/kit5703/row.sh 1b     # nearly folded
sh /root/kit5703/row.sh 2      # Tips: waits for a hand
```

**Each row, in order:**
1. **Stop.** On Enter, lamp-body stops (it folds if standing). Its exit report is logged.
2. **Let go.** The product's let-go makes the lamp limp. If the let-go says the lamp is not down, the row stops and starts lamp-body again: do not pose it.
3. **Pose.** The player poses it by hand. Enter re-reads the joints and shows each against the target; `g` goes on.
4. **Hands off.** The player takes both hands off and steps back, then presses Enter.
5. **Start.** lamp-body starts. The script prints the journal lines as they come:
   - "found shoulder … elbow … head pitch …: it folds first: …";
   - or "…: it lets its arm go and waits for a hand to fold it";
   - any stop.

   It times READY from systemd's own monotonic stamps (`InactiveExitTimestampMonotonic` to `ActiveEnterTimestampMonotonic`).
6. **Row 2 only:**
   - The script asks whether the lamp left its arm alone, whether it stayed quiet (`LAMP_BODY_ASK` is read from the voice's environment), how the arm dropped, and whether it looked like tipping.
   - Then the player folds the arm by hand, holds it still a second, and takes their hands away.
   - The lamp should log "set straight: taken hold where it is, starting" and wake.
7. **Stand.** The player says "Lamp, what time is it?" (or the Mac says it). The lamp should rise to stand while it listens; it is asked y/n.
8. **Row 3.** Two y/n questions: did anything touch, and did it look like a lamp getting up. Then one free line.
9. **Result.** `RESULT …` with PASS or FAIL for rows 1, 2 and 3, and the timings.

**Starts and restarts:**
- Each row's stop and start counts against the unit's `StartLimitBurst=3` in 10 min. row.sh runs `systemctl reset-failed lamp-body` before each start so that the third row in ten minutes is not refused.
- A row left midway (Ctrl-C after the let-go) leaves lamp-body stopped and the lamp limp. Start it again with hands clear (`systemctl start lamp-body`), or run putback.sh.

**The real power cut, if agreed (row 1a again):**

```sh
sh /root/kit5703/row.sh 1a power   # type 'shutdown': it says its line, folds, goes limp, ring green
# ring green -> unplug -> set the pose by hand with the power off -> hands off -> wait 5 s -> plug in -> step back
sh /root/kit5703/row.sh 1a after   # once ssh works again: times READY from this boot's lamp-body start
```

### 4. Core rows 1–4 (after the last power-up row, lamp on the staged body)

**Where the core rows are defined:**
- **joining.md ("The PM's promote"):** "Every `PASS` also covers the core rows: a light colour change, a one-minute timer set and rung, 'what reminders do I have?', 'Stop' over the lamp's talk, and one ordinary question."
- **How they are numbered:** on #5385 the PM's 10343 tally counts "core rows 1–4" as the four other than the spoken stop. Fourteen's 10343 post (#5385, comment 6653794555) failed "core row 5, the stop during a long answer".
- **No kit for them:** there is no core-row kit in the scratchpad, so they are played by voice as Fourteen played them.

**The four rows:**

| Core row | Say | Pass |
|---|---|---|
| light colour | "Lamp, turn the light blue." | `light_colour`, the ring turns blue, "Done." Put it back after: "Lamp, turn the light back to normal." |
| one ordinary question | "Lamp, what is the capital of Australia?" | answered (Canberra) |
| reminders | "Lamp, what reminders do I have?" | `list_reminders`, answered |
| one-minute timer | "Lamp, set a timer for one minute." | `set_timer`, "Timer set. It rings at …", and it rings a minute later |

To read the tool calls afterwards:

```sh
journalctl -u lamp-voice --since "-15 min" --no-pager | grep -E 'light_colour|list_reminders|set_timer|listening_ready|muted' | tail -20
```

Also check that the body stays up while the lamp talks: no `lamp-body` stop lines in `journalctl -u lamp-body --since "-15 min"`.

### 5. Put back (the operator)

```sh
sh /root/kit5703/putback.sh
```

**What it does:**
1. Waits for lamp-update to be quiet. Exit 2 is OWED, with nothing changed.
2. Removes the drop-in and the dead-man.
3. Runs daemon-reload.
4. Restarts lamp-body on stock. A standing lamp folds first: hands clear.
5. Removes the staged binary.

**Then it checks, SAME or DIFF:**
- the running binary's sha256 equals the stock one recorded at setup;
- the binary path, argv, ExecStart and `TimeoutStartUSec` are as found;
- lamp-body is active, and started with no "will not stand" and no wait;
- the drop-in, the timer and the binary are gone;
- the /etc drop-ins, `current` links, active and failed lamp units are as found;
- the voice is not muted and said `listening_ready` in its current run.

It prints the rows played, then **`PUTBACK-OK`** or **`PUTBACK-DIFF: …`**.

**If the voice is muted:**
- **`muted_by` is `restart_guard`** (possible only after the power cut): a clean `systemctl stop lamp-voice; systemctl start lamp-voice` ends it (#4332). Do that only with the holder's yes; the kit never restarts the voice itself.
- **Muted by a person:** the mute is theirs. Leave it.

Copy `/root/kit5703/logs` to the Mac, then run `rm -rf /root/kit5703 /run/kit5703`. Post `released` on #3336.

### 6. Report on #5703

Post `PASS` or `FAIL` for the played row, with:
- each row's `RESULT` line: READY seconds, found pose, folds-first or wait line, and the player's touch and lamp-like answers;
- stand-in or power cut for each;
- the Tips pose's drop and tip-risk answers (for the `FOLD_LEAN_SLACK` question);
- core rows 1–4;
- `PUTBACK-OK`.
