# The lamp's small body

`lamp-body` (crate `lamp-body`, `src/body.rs`, `src/gesture.rs`,
`src/bin/lamp-body.rs`) moves the lamp's head a few degrees for what the lamp
is doing: a lift and a tilt while someone speaks to it, a look away while it
has not answered yet, small nods on its own voice while it speaks, and back to
where it was found between turns. It is not the qualified motion worker
described in [motor-worker.md](motor-worker.md) and replaces nothing there. It
exists so that a lamp folded in its rest pose can be expressive within limits
narrow enough to check by eye.

Given a pose to stand in, it also raises the lamp on its shoulder and elbow
when someone speaks to it and folds it down again when it has been left alone
(see "Standing up"). Without one it is exactly the body described first.
Given eyes, it turns the head to the face they see while the lamp listens or
speaks (see "Looking at a face").

Status on 2026-10-13: the head motion has run on lamp-4ace for a night of
conversation (see "Second and later runs on the lamp"). Standing up and
looking at a face are built and run against simulated servos; neither has yet
run on a lamp.

## What it may do

- It commands three joints: head pitch (servo 5), head roll (servo 4) and
  base yaw (servo 1). The shoulder and the elbow are read on every step and,
  unless it was given a pose to stand in, never written.
- It writes three registers: the goal position (42) of those three joints,
  in one sync write, and once at start their goal speed (46) and acceleration
  (41), so that the servo itself is slow whatever this process sends. Unless
  it was given a pose to stand in, `--torque on` or `--overload release`, it
  writes torque only off, on all five, as a run ends on a surprise or a start
  is refused for heat or load (see "When it stops"), and on to a head or
  base found without torque on a lamp found standing, to fold it (below). With
  `--force-limit` it writes one more, once at start: the force limit (48)
  of the joints that carry the lamp (below, "The force its servos are
  allowed").
- It starts only if all five joints answer, the three it commands have torque
  on, a clear status byte, a temperature and a load under their bounds, and
  the head's pitch is found between 1,760 and 2,020. A head pressed on the
  base (found at 2,095 on lamp-4ace on 2026-10-12) is refused, with nothing
  written, unless it was given `--wake on` (see "Waking"). A joint found at
  62 or warmer, or over its start load, is refused with torque off on all
  five and nothing else written: such a joint is pushing at something, and
  left held it would push on and be refused at every start until a power cut
  (base yaw on lamp-52e6, 2026-10-16: load 212, 65 °C, refused at every
  restart). A joint found at 62 or warmer is first read again, up to three
  times and before anything is written, until two reads in a row agree
  within 3 degrees (`TEMPERATURE_JUMP`): one glitch on the first read would
  otherwise let go of a lamp found standing, and it falls. Only the joints
  judged here are read for it: the three it commands, and the shoulder and
  the elbow of a lamp given `--stand` (a folded lamp's warm arm is not read
  again). Two reads that agree start as usual unless both are hot (62 then
  61 starts; 70 then 70 is refused). Reads that never agree are acted on
  with the last hot one, so one low glitch on a read again does not let a
  real heat in (70, then 5, 70 and 70 is refused with nothing moved). A read
  again that fails after a hot one is acted on with that hot one. A read
  over 100 (`TEMPERATURE_MAX`) is a glitch: never heat and never agreeing,
  so 150, 150, then 45 and 45 starts as usual. Over 100 on every read is a
  temperature that cannot be read: the run is refused (`would not start:
  temperature unreadable at start: wrist_pitch reads 150`), with nothing
  written and nothing let go unless the lamp is found standing (below).
  Only the arm of a lamp given `--stand` over 100 on every read is judged
  too warm (below). The reads again are in the end report (`hot_rereads`, per joint). Heat that is acted on is
  let go as before, where the lamp stands, arm up or not.
- A lamp given `--stand` and found up on its way (a twentieth of the way or
  more, both arm joints within 80 counts of it) whose start is refused with
  nothing written is not left up held, where every restart would refuse it
  again and nothing would lower it. It folds down first, as when asked to
  stop: at its own pace, every arm goal through the guarded write, at most
  50 counts a write, within the time a stop allows. Then all five are let
  go and the run ends on the refusal. This covers a commanded joint over
  100 on every first read (its temperature is not judged while it folds),
  and one found with torque off or a status flag. One without torque is
  taken hold of where it is, goal first, as `--torque on` does, to come
  down with the arm. One with a status flag (the servo's own protection),
  or without torque and over 100 on every read (perhaps its own heat
  cut-out), is left as it is: written no goal, profile or torque on, since
  a goal turns such a servo back on, while the arm folds without it
  (`a_head_found_without_torque_and_unreadable_is_left_while_the_arm_folds`).
  Started again, it is found folded and refused, with
  nothing left powered
  (`a_lamp_found_standing_whose_start_is_refused_folds_then_lets_go`). It
  is refused as before, arm up and held, when the arm itself cannot be
  folded: its profile read fails, it has torque off, another flag, or an
  overload that stays, or the head does not take hold. A joint found hot or
  loaded still lets go of all five at once.
- With `--torque on` (off unless given) it first takes hold of a joint it
  would otherwise refuse because its torque is off: the three it commands
  always, the shoulder and the elbow only for a lamp given `--stand`. A
  servo whose torque went off keeps an old goal (on lamp-52e6 on 2026-10-14,
  after the old runtime stopped, all five had torque off and goals 18 to 39
  counts from where they were), and torque on alone would send it there. So
  for each such joint it writes the goal (42) as the position just read,
  then torque on (40), each acknowledged. About 100 ms later it reads all
  five again; a joint it gave torque that moved more than 12 counts ends the
  run refused, its goal written where it went ("wrist_roll moved 30 counts
  when its torque came on"). Otherwise it starts as usual. The report counts
  the torque writes (`torque_writes`) and names the joints
  (`torque_given`).
- Every goal is within a fixed reach of its home: pitch 45 counts up and 15
  down, roll 30 either way, yaw 40 either way (11.4 counts to a degree). Home
  is the pose given with `--home PITCH,ROLL,YAW`, in encoder counts, or else
  the pose found at start. A joint found more than 60 counts from a given
  home is a reason not to start. From anywhere nearer, the head first eases
  home at the same speed as every other move, and until it is inside the
  reach it is never moved away from it. The head's pitch goal is also never over 2,030: on lamp-4ace
  the head crossed 1,766 to 2,043 with no load and met the base near 2,045 to
  2,056. That window, 1,740 to 2,030 while the head is beside the base, is
  lamp-4ace's, in absolute counts; `--pitch-window LOW,HIGH` gives a lamp
  its own (below, "Running it", and "lamp-52e6").
- The commanded pose moves at 50 counts a second at most (about 4 degrees a
  second). That is the speed the head was moved at when it was backed off the
  base on lamp-4ace on 2026-10-12 (47 counts a second at most), which it
  followed within 13 counts on the servo profile written here (goal speed 150,
  acceleration 50). No goal moves more than two counts between two writes,
  20 ms apart.

## When it stops

After every step it reads all five joints. The run ends, the present positions
are written as the three goals so that nothing keeps pushing, then, as V1,
torque off (40, 0) is written once to each of the five so that the lamp is
let go, and nothing is written after that, when:

| What is seen | Bound |
| --- | --- |
| Load on a commanded joint, six steps in a row (120 ms), for the fourth time in a minute, or again within two seconds of the joint giving way | pitch 240, roll 150, yaw 220 |
| A commanded joint away from its goal for ten steps | 30 counts |
| Any joint's status byte different from the one found at start | any change |
| The shoulder or the elbow away from where it was found, in a body that does not stand | 6 counts |
| A commanded joint's temperature register, on every step for a second | 62 |
| A read, or a write, that fails on every step for half a second (25 steps) | missing, late, malformed or unexpected bytes |
| An arm goal outside its joint's window, or too far in one step (below) | refused before it is written, at once |

The same let-go ends every run that stops on a surprise, not only one in
the loop: taking hold, a profile or force limit that does not write or read
back at start, a wake that does not reach home, and a shoulder that does
not come down when it is lowered all end with torque off on all five, after
the hold where there is one. Every way a run ends passes through one place
(`run_alive`) that does this. It is not done when the body is asked to
stop (unless `--let-go-at-stop on`, below), on a refusal to start (one for a hot or loaded joint has let go
already, one of a lamp found standing lets go once it has folded, and one of a
lamp found folded with its head or base out of range lets go and waits to be
set straight under `--refused-let-go`, on by default, below), or
under `--overload release`, where torque is already off. A
standing lamp whose shoulder or elbow cannot be read (`Stop::BadTemperature`,
below), or is found too warm, is not let go up, where it falls, nor left up
held, where it may heat on: it folds down first, then is let go.

Another surprise while a standing lamp folds down (asked to stop, or
folding for one of the stops above) does not let it go mid-air: a joint
too hot, loaded, off its goal, drifting, or a temperature that cannot be
read (that joint no longer judged) leaves it folding, within the time a stop allows, and the run then ends on what it
was folding for, or else on that surprise, and lets go folded
(`a_surprise_while_a_standing_lamp_folds_keeps_it_folding_then_lets_go`:
the head at 70, or reading 150 and 5 by turns, as it folds). A wire lost
for half a second, an arm goal the guard refuses, a position that cannot
be read, and a hand or `--overload release` letting go still end it at
once. A status byte that changes is the servo's own protection, and a goal
turns a servo that cut itself out back on: that joint is written no goal
from then on, not even the hold at the end. A head or base joint so
flagged is left as it is while the rest folds; a flagged shoulder or
elbow, without which the arm cannot fold, lets go of the lamp at once
(`a_flag_while_a_standing_lamp_folds_writes_that_joint_nothing_more`).

A load over its bound for six steps does not by itself end the run. The
joint gives way: its goal becomes where it is, and nothing is asked of it for
two seconds while the other joints go on. Then it moves again from there. If
it still reads over the bound six steps after it stopped pushing, something
is pushing it and the run ends; so it does the fourth time any joint gives
way within a minute. Each time is counted in the report (`eased`) and written
to the log. Until 2026-10-13 the first such load ended the run, and with it
all motion until someone started the body again: on lamp-4ace that happened
twice in a quarter of an hour, at 217 and at 190 on the head joint, with
nothing in its way either time.

The bounds for the head and the base were 170 and 150 until 2026-10-13.
With everything above in place the head still gave way now and then when
it began to lift from rest, reading 173 to 203 for a little over 120 ms
before it moved, and the base once at 208, turning back in the middle of
its look away. Nothing was in the way of either. Since a joint that pushes
without moving is put to rest within half a second whatever it reads, the
bound no longer has to stop that; it is there for a joint that is pushed
hard, and 240 and 220 are a quarter of what these servos can exert. To
start at all a joint must still be found under the old bounds (170, 150,
150): at rest it should be carrying next to nothing.

While the wire is failing nothing new is asked of the joints, and the
write that failed is tried again on the next step. Until 2026-10-13 three
failed reads in a row, or one failed write, ended the run. On lamp-4ace
that took one stall of the whole machine: at 04:00:40, with the lamp idle,
the ring's write overran its 10 ms budget and in the same second the body
found 41 bytes of a late reply on the wire and stopped after its third
missed read. It had run for 36 minutes without a fault.

A status flag that was already latched at start on a joint it does not command
is not a reason to stop or to refuse: lamp-4ace's elbow has carried `0x20`
since the head test of 2026-10-12. A change of that byte is.

It also ends on SIGINT or SIGTERM, after easing back to its home (three
seconds at most), and prints one JSON report either way.

### Every arm goal inside its window

Every goal of the shoulder and the elbow is checked in one place,
`Bus::goal_refused` in `src/body.rs`, before anything is written. Goals reach
the wire only through `Bus::send_goals` (every step, lowering a shoulder,
every hold) and `Bus::write_goal`, one joint's goal by itself (taking hold of
a joint found without torque, telling an overloaded one to stop pushing),
and both ask it first. Head and base goals are asked too, and only to be 0
to 4,095: their goals are bounded where they are made, and the head is sent
home in one write at the end. With `LAMP_BODY_OWN_RANGES=on` (#3558; off
unless exactly `on`) the body also reads, read-only at every start, the
lower and upper limits each of the five servos stores (registers 9 and 11,
which V1's calibration wrote on its lamps), says them in its report
(`own_ranges`), and bounds the lamp by them as V1 bounds every goal:
- a head or base goal must then be inside its joint's own range less 20
  counts either side (`OWN_RANGE_MARGIN`), or the run ends on `Stop::Window`
  as for an arm goal; except that a joint lying past that window (its last
  good read, else the goal last written) may be sent back toward it: a goal
  between where it lies and the window, and inside the servo's own range, is
  taken, while one farther out, past the range or out the other side is
  still refused (#3504,
  `with_own_ranges_a_goal_back_toward_the_window_is_taken`);
- a hold where a head or base joint already is (taking hold of it, the
  write-back after, a joint that has arrived or settled at rest given where
  it is, every stop-pushing hold when a wake, a lowering or the run ends)
  is checked against the servo's own range itself, not that window:
  holding is not motion, so a roll resting at 2,410 under a stored 2,413
  still takes hold, and is not clamped, which would move it
  (`with_own_ranges_a_roll_resting_near_its_stop_is_taken_hold_of_at_start`,
  `with_own_ranges_a_stop_pushing_hold_near_a_stop_is_written`); motion
  from there walks it back inside, as the eased way home at the end does
  (`with_own_ranges_a_joint_arrived_near_a_stop_rests_there`);
- a `--home`, a pitch window or a `--stand` pitch, roll or yaw outside that
  same window, or a `--folded-arm` or `--stand` shoulder or elbow outside
  the servo's own range, is refused at start, writing nothing (a folded arm
  rests on its mechanical stop, which may lie past the servo's stored
  limit: lamp-52e6's factory fold put the shoulder at 2,038, under its limit
  2,047, so a folded arm may be up to `Standing::FOUND_SLACK` (80) outside
  the range, a standing arm is checked against the range itself, and the
  arm's goals keep their own window);
- a joint that stores the whole turn, 0 to 4,095 (lamp-52e6's elbow), was
  never calibrated by V1 and keeps today's guard; its range can only be
  swept by hand (`lamp-body-measure sweep`, below), which nothing reads yet;
- a stored limit past 4,095, or a lower limit above the upper, is a failed
  read: the lamp does not start (`first read: <joint> stores position
  limits …`), writing nothing, rather than run on a range clamped wider
  (#3504). With a pitch window of its own, the head pitch servo's limits
  are read once and serve both that window's check and its own range.

It is a bound, never a remap: poses and clips stay shared. Off, the limits
are not read and the body runs exactly as before
(`without_own_ranges_the_servos_limits_change_nothing`). `lamp-body-measure`
writes every servo's limits into `lamp.env` as a comment, so factory step 1
shows them. An arm goal must be inside its joint's
window: the joint's way between `--folded-arm` and `--stand`,
with 420 counts either side (the most a gesture adds to it, 360, and 60 to
spare). On lamp-4ace that is shoulder 1,628 to 3,077 and elbow 1,627 to
3,022. A pose whose window would reach past 0 or 4,095 (an arm count of
`--folded-arm` or `--stand` under 420 or over 3,675) is refused, so no
window is cut at the seam
(`an_arm_pose_whose_window_would_reach_the_seam_is_refused`). It must also be within 50 counts of the arm goal last written (a rise
moves it a few counts a step, a gesture 21, and an arm let go by a hand
goes back down to its resting goal at 50 a step), or lie between that goal
and where the joint was last read, if that reading was within 50 of the one
before it: a hold where it is, as when it gives way to a hand, is taken hold
of or stops pushing, never asks it past either. A goal that is neither is
not written. The guard only stops: what makes the arm's goals never asks
it. Each step, after the rise or fold, a gesture, a hold or its release,
the rest and a shift have made them, `Bus::eased` moves each arm goal at
most 50 counts from the one last written unless the guard takes it as it
is. A goal outside the joint's window, or more than 400 counts away
(`ARM_EASE_REACH`: what a hold let go needs, a gesture's 360 at most; the
largest over 378 simulated runs of every clip after a held arm on both
lamps' flags was 318), is not eased toward: the guard refuses it at once
(`an_arm_goal_outside_its_window_is_not_eased_toward`,
`an_arm_goal_farther_than_a_hold_release_is_refused_not_eased_toward`).
Nor is any goal of a folded arm at rest: it has no hold to let go, and its
way back from a hand is ramped already
(`a_resting_arm_goal_more_than_a_step_away_is_refused_not_eased_toward`). On lamp-52e6 on 2026-10-17 a standing shoulder held at 2,609, short of
its pose, was asked 2,683 at once by a nod, and the lamp was let go
(`a_standing_session_never_moves_an_arm_goal_more_than_a_step_a_write`, with
lamp-4ace's and lamp-52e6's flags, and
`a_nod_right_after_a_hand_lets_go_of_the_arm_does_not_stop_the_lamp`). A
goal the guard refuses is not written. The run ends on `Stop::Window` at that step, never retried as
a failed write nor reported as `Stop::Bus`, and lets go as any surprise
does, after the hold, which writes the arm where it was last read and is
checked the same way. A hold refused is not written, and the run ends on
what ended it; at the end of a run asked to stop, a refused write ends it
on `Stop::Window` instead of `Cancelled`. That last write keeps torque on,
so when it is made (not for a lamp that did not get down in time, which is
left as it is) the arm may only be left within 50 counts of its last goal: a
reading farther off, even a steady one under 300, is not written, and the
run ends on `Stop::Window` and lets go
(`a_steady_wrong_elbow_reading_before_cancelled_is_not_written_at_the_exit`). The same holds when the folded arm comes
to rest: its resting goal, which keeps torque on, is where it reads only
within 50 counts of the goal it had, or farther while it pushes there (load
over 100, an arm resting short of its fold) and came there without a jump
over 50 from one read to the next since it last stood or rose (a misread
jumps:
`a_steady_wrong_elbow_reading_under_load_as_the_arm_comes_to_rest_is_not_its_resting_goal`),
and otherwise that goal
(`an_arm_resting_short_of_its_fold_and_pushing_is_left_where_it_lies`,
`a_steady_wrong_elbow_reading_as_the_arm_comes_to_rest_is_not_its_resting_goal`). The end report names the joint,
the goal, the window and the goal it came from:

```text
"stop":{"kind":"window","joint":3,"goal":4095,"window":[1627,3022],"from":2047}
```

Why, lamp-4ace on 2026-10-17 at 01:58. A body that may stand took the arm's
goals at start from the servos' goal registers (40 to 47, read with the
profile), raw. The elbow's register held 37,639 (0x9307). Folded at rest the
body sends the arm's goals back every step, the anti-tip cut clamped the
37,639 to 4,095 inside its model and wrote 4,095, and the elbow, found at
2,049, was driven to the seam and ended at 4 (`stop: tracking, joint 3, goal
4095, position 4`). Since then a register is taken only if it is 0 to 4,095
and within 80 counts (`Standing::FOUND_SLACK`) of where the joint is found;
otherwise the arm's goal is where it is. The anti-tip model refuses a count
outside 0 to 4,095 instead of clamping it (below). What wrote 37,639 into
that register is not known: the body is the only thing in lampOS that writes
goals. The tests are `a_goal_register_far_from_the_arm_is_not_taken_as_its_goal`,
`an_elbow_goal_register_past_its_range_is_never_written_as_a_goal`,
`an_arm_goal_outside_its_window_never_reaches_the_wire`,
`an_arm_joint_taken_hold_of_outside_its_window_is_not_given_that_goal`,
`an_arm_goal_is_never_more_than_a_step_from_the_last_unless_it_holds_where_the_arm_is`,
`a_head_goal_past_the_servos_range_never_reaches_the_wire` and
`the_hold_after_a_surprise_writes_the_arm_where_it_was_last_read`.

### Every position read checked before it is used

Every position the body reads passes one place, `Bus::judge_positions` in
`src/body.rs`, before anything uses it. A position is a bad read if it is
outside 0 to 4,095, or more than 300 counts (`READ_JUMP`) per 20 ms step
since the last read from the joint's last good reading, and as far from the
goal last written to it. The steps are counted since the last read, not the
last good one, so a bad value that stays does not become good by staying. An STS3215 turns at most about 3,000 counts a second unloaded; at 4,096
a step is 82 counts, and 300 is near four times that. A bad read is counted
in the end report (`bad_reads`, yaw, shoulder, elbow, roll, pitch) and
replaced by the joint's last good reading, so it never becomes a goal, a
hold, a resting goal or where the arm slumped. With no good reading yet (the
first read) the read fails. Three bad reads of one joint in a row end the
run on `Stop::BadRead`, which lets go as any surprise does and names the
joint in the end report and the journal (`stopped: elbow read 37639 three
times in a row, not where it can be (last good 2049)`):

```text
"stop":{"kind":"bad_read","joint":3,"position":37639,"from":2049}
```

Why, lamp-4ace on 2026-10-17 at 01:58:39. The body ended on `Cancelled`, and
its exit sent the arm's last positions back as goals, raw. One elbow reading
of 37,639 went out as the elbow's goal. The tests are
`an_elbow_read_past_its_range_just_before_cancelled_is_never_written_as_a_goal`,
`one_bad_read_of_a_joint_is_counted_and_skipped`,
`three_bad_reads_of_a_joint_in_a_row_stop_the_body_naming_it` and
`an_elbow_read_far_off_in_the_last_step_is_not_written_back_at_the_exit`
(2,049 read as 2,949 from the last step on).

### Every temperature read checked before it is used

Temperatures pass one place too, `Bus::judge_temperatures` in
`src/body.rs`, right after the positions of the same read and before a
peak (`peak_temperature`, `arm_peak_temperature`) or the hot-servo stop uses
them. A temperature is a bad read if it is over 100 (`TEMPERATURE_MAX`), or
farther from the joint's last good one than 3 (`TEMPERATURE_JUMP`). A real
rise moves the good one with it a read at a time: on lamp-52e6 on
2026-10-17, over 632 pairs of the arm's logged standing steps 20 to 30 s
apart (readings under 58), none rose more than a degree, 3 degrees a minute
at most. A bad one is counted in the end report as `bad_temperatures` (per
joint, in the order of `bad_reads`) and replaced by the joint's last good
one, so it is never a peak, and the heat streak holds on it for that step,
neither adding nor resetting (`Bus::temperature_held`). Before this hold, a
replaced read carried the last good value into the streak: one read of 63
let in at a true 60, then the register at 59, made 50 hot steps of one hot
read and let the whole lamp go (the seventh's re-check of #1236, 2026-10-17
08:37). The 25-read settle below now covers that case by itself, but the
hold still matters when real hot reads lead into a glitch: 30 hot reads at
a true 62, a 65 let in, then 24 replaced reads at a true 61 would reach 55
hot steps without it, on the head as on a standing arm. A bad value settles, and becomes the good one, once half of
`temperature_steps` (25) reads each agree within 2 degrees
(`TEMPERATURE_SETTLE`) with the last that agreed, skipping up to 5 outliers
among them (`TEMPERATURE_SETTLE_SKIPS`: 25 of 30): a glitch is skipped, not
a reason to start again, since lamp-52e6's elbow glitches on one read in 27,
close to a window of 25. So a servo really at 70 after a low first read,
one rising 5 degrees a second after it (with a 150 every 20 reads), a jump
to 70 that stays, or a steady 150 still stops on heat, about half a second
later than without the check; one 150 is still dropped. Each settle is in
the end report, `temperature_settles` (joint, last good, settled; the first
16), so a stop on a steady 150 says it settled there rather than looking
like heat that was read. A good read no longer ends a settle in progress: it counts as one of
the settle's skipped outliers, so a value read on 25 of 30 reads, the good
one on the rest, still settles; half and half never does. A bad read within 10 degrees of the last good one, both
under 100 (`TEMPERATURE_FLAP`), is a flap: a register reading two
plausible values by turns. lamp-4ace's shoulder on 2026-10-17 read about 55
and 59 to 61 by turns (its settles 4 to 6 apart) and ended at 15:25:38 on
`BadTemperature` (`temperature 55, from 60`) after 618,587 steps, every
read under 62: each good read cleared the settle, so the other value never
settled while the bad reads in a row climbed to 50. A flap is counted in
`bad_temperatures` and still feeds a settle. Only a flap that repeats, the
third in a row each within 2 degrees of the last (`TEMPERATURE_FLAP_AGREE`,
`TEMPERATURE_SETTLE`) and within 10 reads of it (`TEMPERATURE_FLAP_READS`),
is kept out of `BadTemperature`, and counted per joint in the end report's
`temperature_flaps` (a part of `bad_temperatures`); it holds the heat streak, as a replaced
read does, and is heat evidence: it is judged on the higher of it and the good one (the
cautious side for heat), and the good reads in the 10 after it on it too.
A lone flap, a glitch or noise inside the window, is a bad read as on
lamp-voice: a joint near 70 read as 64 to 70 at random, its last good 60
coming back one read in 8, still ends on `BadTemperature` within about 3 s
(`lone_flaps_around_a_good_value_that_returns_still_stop`, the reviewer's
probes on #1347), while lone glitches between good reads never add up. A flap holds only next to its good value: a read with no
good read in the last 10 is a bad read as before, toward
`BadTemperature`, so a sensor whose reads all sit 4 to 10 from a stale good
one (last good 60, then 64 and 70 by turns) still ends within about a
second (`reads_within_the_flap_window_of_a_stale_good_one_still_stop`, the
reviewer's case on #1347). So lamp-52e6's glitches, one read in 27 at random, never chain
into heat: a cool head at 56 whose glitches read 57 to 66 runs ten
minutes (`a_cool_joint_glitching_within_the_flap_window_runs_ten_minutes`,
the eighth's probe on #1347, which judging on the highest flap ever seen
stopped on `Temperature` 65). A real flap over the limit, 60 and 64 by
turns, is judged 64 and stops on `Temperature`, but its streak runs at
half rate (the flap reads hold), so the stop comes at about 100 reads
rather than 50
(`a_shoulder_flapping_between_55_and_61_runs_20_minutes_judged_at_61`,
`a_joint_flapping_between_60_and_64_stops_on_heat`). A read over 100 or
more than 10 from the good one is judged as before. With no good one yet (the first read) a reading is
taken as it is: one over 100 is counted, and a hot one is read again before
it is acted on (above). A glitch, or glitches between good reads however
many, never stop the run: the hot-servo stop needs 50 hot steps in a row,
and letting go of a lamp for a sensor glitch is worse than the glitch. But
bad reads of one joint in a row, less one for each good read, end the run
at 50 (`temperature_steps`) on `Stop::BadTemperature`, as three bad
positions do, naming the joint (`stopped: head_pitch temperature unreadable
(it reads 5, last good 41)`). A good read takes one off rather than
starting the count again, so 49 bad and one good, again and again, still
ends it within about a second more, where starting again took 44.7 s, and
one bad read in 27 never adds up (ten simulated minutes at that rate, at
random, run on). For the head or the base the run ends there and the
whole lamp is let go, as for any surprise (one folding down while it
stands keeps folding first, see "When it stops"). For the shoulder or
the elbow of a lamp that may stand, it first folds down as when asked to
stop, at its own pace and its arm goals at most 50 counts a write,
that joint's temperature no longer judged; once folded (or after the time
a stop allows) the run ends on `BadTemperature` and the whole lamp is let go,
folded. Started again with the elbow still unreadable, the arm is down and
let go, never left up with torque on
(`a_standing_lamp_whose_elbow_temperature_is_unreadable_folds_then_lets_go`;
folded, it is let go at once,
`a_folded_lamp_whose_elbow_temperature_is_unreadable_lets_go`). The stop
in the report:

```text
"stop":{"kind":"bad_temperature","joint":5,"temperature":5,"from":41}
```

A temperature that cannot be read that long is not known to be cool: a
joint can warm behind it while its last good reading is cool (the seventh's
10:12 re-check of #1270: the last good 40, the servo warming at 5 degrees a
second behind 60 reads of 150 and 5 by turns, then a glitch on one read in
27, ran on at a true 80). How often, on lamp-52e6 on
2026-10-17 from 02:42 to 07:35: of 1,839 logged standing steps (one every
30 s, and each change of mood), the shoulder read 60 or over on 23 (about 5
an hour) and the elbow on 19 (about 4 an hour), about one logged step in
a hundred; the head's temperatures are not logged.

It is a count apart from `bad_reads` because it is not the same fault: the
position in the same reply was good, so a position-misread count stays
what it was and the two can be told apart in a report.

Why, lamp-52e6 on 2026-10-17: every run that stood ended on
`arm_peak_temperature [150,150]` (and lamp-4ace's 01:58 report on
`peak_temperature [150,55,150]`), while most logged steps read 43 to 54. A
few logged steps read 60 to 150 on one joint, the step before and after
back near 47, never over 150. It is not a misframed reply: a reply is taken
only with its checksum, its ID asked once, exactly 15 bytes of telemetry
and nothing else on the wire before the request or after the last reply
(`protocol.rs` `StatusParser::drain` and `ReplyTracker::match_reply`,
`Bus::expect_quiet` and `Bus::read`), and the positions and loads in those
same replies were where the joint was. It is what the servo's temperature
register (63) said. The tests are
`a_shoulder_reading_150_degrees_once_is_counted_and_never_a_peak`,
`bad_temperatures_in_a_row_do_not_stop_the_run`,
`a_joint_that_really_warms_still_stops_after_its_hot_steps`,
`a_joint_really_at_70_after_a_first_read_of_40_still_stops_on_heat`,
`a_jump_that_stays_becomes_the_reading_and_stops_on_heat`,
`a_glitch_of_32_between_47_and_48_is_dropped`,
`a_small_glitch_near_the_limit_then_a_flicker_down_is_not_heat`,
`a_fast_rise_after_a_low_first_read_still_stops_near_the_limit`,
`a_joint_warming_behind_bad_reads_stops_as_unreadable_within_a_second`,
`a_hot_joint_whose_reads_never_settle_stops_as_unreadable`,
`a_cool_joint_glitching_one_read_in_27_runs_on_for_a_minute`,
`a_cool_joint_glitching_at_random_one_read_in_27_runs_ten_minutes`,
`garbage_with_one_good_read_in_50_still_stops_within_seconds`,
`a_true_value_settles_through_glitches_after_a_low_first_read`,
`a_hot_first_read_that_does_not_repeat_starts_as_usual`,
`a_hot_first_read_that_repeats_is_refused_as_before` and
`one_hot_reading_while_it_moves_is_not_heat` (lamp-4ace's yaw reading 72
for one step, now a bad read).

## What moves it

The voice service names a mood and a voice level; the body decides the rest.
`lamp-voice serve` sends one datagram to the path in `LAMP_BODY_MOOD_SOCKET`,
when that is set, on every change of mood and otherwise every 40 ms:

```json
{"mood":"resting|listening|thinking|speaking|reading","level":0}
```

`level` is the voice in play on the scale of the ring's face (see
[ring-choreography.md](ring-choreography.md)): the person's while listening,
the lamp's own while speaking. While the lamp has stopped speaking for someone
who spoke over it (a duplex yield in force) the mood sent is `listening` with
that person's level, so the head turns its attention to them at once and goes
back to `speaking` if the reply resumes. And `speaking` is sent from the
moment the answer's first audio arrives, a few hundred milliseconds before
it is played, so that the head is back on the person when the first word
comes and is not making its largest move while the lamp speaks.

The head also settles when the talk around the lamp is not for it. After
two turns in a row that the lamp did not answer, `listening` and `thinking`
are no longer sent and the head stays at rest: a lamp beside two people
talking to each other looks up at the first sentences and then leaves them
to it. The ring still shows that it hears. The first answer the lamp does
give brings the head back, from the moment that answer arrives, and so
does half a minute without a turn. Nothing comes back, and a body that is absent,
slow or stopped changes nothing in the voice service. A mood older than 1.5 s
is rest. Anything that is not exactly that object is ignored.

| Mood | Wanted pose, as offsets from where the head was found |
| --- | --- |
| resting | none |
| listening | pitch 18 up, roll 26 to one side; the side alternates between exchanges |
| thinking | pitch 30 up, yaw 36 to the side it was tilted from, roll 12 the other way. With a wider reach this is where it ends, not where it begins: see "A glance first" |
| speaking | pitch 8 up, and a nod of up to 7 either way when its voice is louder or quieter than its last second |

The commanded pose follows the wanted one with no overshoot, inside the speed
and reach above.

### A glance first

The look away while the lamp thinks begins as a glance and deepens only if
the thinking goes on. A person asked something easy flicks their eyes
aside and is back as they begin to answer; asked something hard, they
look away for as long as it takes.

- At once: the base turns 36 counts, a little over three degrees, and
  the lift and the tilt go the same share of their way.
- For the first 0.8 s it stays there.
- If the lamp is still thinking, the head goes the rest of the way (the
  lift and the tilt) over the next 4 s, eased at both ends. **The base
  holds the glance:** it turns no further while the lamp thinks.

Within the folded reach (yaw 40) the glance is the whole look away, so a
lamp started without `--reach` moves exactly as it did. With lamp-4ace's
yaw reach of 250 the base stops at the glance's 36 counts; until 20 Oct it
went on to the whole look away, 225 counts, near 20 degrees.

**Why the base holds** (#1722, 20 Oct): on lamp-4ace a slow first answer
(3.5 s, a cold connection) let the base turn 82 counts more from about 2 s
into the wait, and the microphone took the servo for someone speaking: a
line of 0 words held the answer 1.9 s, and once 6 words were answered in
place of the request. A person thinking a while looks away with the eyes
and the head, not the whole body, and the base is the lamp's loud joint
(Thirteen and Fourth; the first builder's yes, 05:21). Tests:
`gesture.rs`'s `the_look_away_is_a_glance_that_deepens_only_while_the_thinking_goes_on`
(the base's yaw stays the glance through 4.8 s, the head goes on) and
`body/tests.rs`'s `a_wider_reach_and_speed_can_be_chosen_only_inside_their_caps`
(six seconds of thinking at a reach of 200: the base 30 to 50 counts, the
head 90 or more).

It was changed on what the joints showed. On lamp-4ace on 2026-10-14,
with `body/joint_states` recorded through three short played exchanges
and nobody in the office, the lamp had thought for 1.16 to 1.39 s when
its voice began. Its base was by then 94 to 133 counts round (8 to 12
degrees), and it was back and still 0.68 to 1.24 s after the first sound.
The three answers lasted 0.78 to 1.68 s: the lamp spent most of each
turning back to the person it was answering, and one had ended before it
was back.

The same three exchanges played again the same night with the glance,
measured the same way (the base's angle from where it was when the lamp
stopped listening, at moments counted from `voice/state` turning to
speaking):

| How far the base was from where it listened | Before | With the glance |
| --- | --- | --- |
| As its voice began | 8.3 to 11.7 degrees | 1.4 to 3.2 degrees |
| 0.6 s into the answer | 3.9 to 9.2 degrees | 0.4 to 2.2 degrees |
| The farthest at any moment of the answer | 8.3 to 11.9 degrees | 1.4 to 3.2 degrees |

Three answers each way, a played voice, joints sampled every 80 ms. What
it does not show: how the glance looks. Nobody has watched it, and a turn
of three degrees may be too small to read as looking away at all. The
base also does not come all the way back: it rests where it is once it is
within 20 counts of where it is wanted, as every joint does, so after an
exchange it can sit up to two degrees from where it began.

**The pose's speed is what lets a nod show** (24 Oct): the offsets Expression asks of the head, the mood
poses and gaze moves go through the pose filter at `--speed`, with a software acceleration of six times it (720 counts
a second squared at 120, 1,800 at 300). Leans keep their 1.5 s and the arm's stand and fold `--stand-seconds`.
lamp-4ace's joints, read off `body/joint_states` ten times a second through one spoken answer each: at `--speed 120`,
two head swings of 2 degrees or more, its posture moving and nothing else; at 300, six, of 3 to 7 degrees, the nods. No
false cut-in was heard in either. Body's exit reports over 17.6 minutes at 300 and 43 at 120 that evening: the head
pitch's highest single 20 ms step read 328 and 281, roll 32 at both, the base 196 and 200, and the arm's shoulder and
elbow 376 and 488 against 396 and 488. Single steps over the pitch's 240 come at both speeds; the strain bounds act on
six steps in a row (pitch, roll and yaw 240, 150 and 220; the arm 560 under `--force-limit 600`), and at 300 nothing
was eased and nothing retreated. The product runs 300, `Tuning::SPEED_CAP`. Folded, the head reaches 15 counts below
home (`--reach`); stood up, its downward reach grows toward the upward one (`bounds()`), so a nod dips from wherever the
speaking posture holds the head.

A joint that has arrived rests, in any mood: once the commanded pose has
been within 6 counts of the wanted one and slower than 15 counts a second
for 0.3 s, the joint's goal becomes its own position, and it is asked
nothing more until the wanted pose is 20 counts or more from where it is. A
servo asked for less than it can move pushes and stays where it is. So a nod
smaller than 20 counts does not happen, and at the reach lamp-4ace runs
with that is all of them: the head comes back to the person as the answer
arrives and then keeps still while the lamp talks. That is also when its
microphone needs it still ([duplex-listening.md](duplex-listening.md),
"Seven blocks"). Nodding as it speaks is still in the gestures and comes
back with a head joint that starts for a small move, or a reach wide enough
to make every nod a large one. At the reach used without `--reach` the
lift for listening is 18 counts and does not happen either; the tilt, the
look away and the way back do.

## Waking

No power cut has been tried. On lamp-52e6 on 2026-10-14, folded, with its
voice stopped, the body was started twice with `--wake on` and a home it was
not found at: once with the base's home 300 counts round (found at 2,024,
home 2,335), once back again (found at 2,318, home 2,035). It woke the base
311 counts in 24.3 s and 283 counts in 22.2 s, the head's roll the 8 counts
it was off with it, with no missed read, no joint giving way, and loads of
at most 127, 52 and 112 (pitch, roll, yaw). Each run then went on until it
was stopped. That the joints never moved away from home on the way is shown
by the tests below, not by these runs, whose reports give where each began
and ended. Every other sentence of this section is what the code does
against simulated servos.

On lamp-4ace on the same day, 15:16 to 15:20 (first builder, build
`dd336aef…` of `001bf889f`), folded, its voice running, the same test
with lamp-4ace's unit options less its standing ones: **the wake ended
itself both ways before reaching home.** Toward a base home 286 counts
round (found at 2,029, home 2,315), the base turned 72 counts and stuck at
2,101 while its goal went on to 2,135: off its goal by more than 30 for
ten steps, so the run held and ended (`"stop": {"kind": "tracking",
"joint": 1}`), at a load of 148. The other way (home 1,729, 300 counts),
the base turned 189 counts and the shoulder moved from 2,050 to 2,057 as
it went, one more than the 6 a bystander may drift (`"kind": "drift",
"joint": 2`). No read was missed and the peak loads were 130, 56 and 148.
In between, the ordinary start (a joint found within the 250 counts it
may start from) brought the base 177 counts home and the head's pitch 87
counts, at the body's own speed, with no check tripping. Not tried on
lamp-4ace: its own case, the head found pressed on the base at 2,095,
which needs the head's torque let go. The service runs with `--wake on`
since 15:21: a wake that ends early ends no worse than the refusal
before it, and each of systemd's three restarts begins from where the
last one held.

Again on lamp-4ace that evening, 19:47 to 19:49 (first builder), with the
body then running there (`638cb63c…`, built from `fc22bb107`, which has
the fifth builder's wake mends), folded, its voice running: **the wake
reached home both ways.** Run by hand with the unit's options less its
standing, gaze and idle ones and a base home 292 counts round (found at
2,023, home 2,315): it said it woke the base, the head's pitch and its roll
"in 22.9 s", and held the base at 2,289, 26 short of its goal, at a load
of 116 (a read the report of that run would have had more of was lost: the
second run's folder replaced it). Then the service itself, started at its
own home (base 2,015) with the base found at 2,289, brought it to 2,027,
262 counts, and was running as usual 21.6 s after it started. A second
hand run toward a home 300 counts the other way (1,715) was refused, as it
should be, because the base was then 574 counts from it, past the 400 a
wake brings back: the test's own mistake, not the body's.

After a real unplug on lamp-52e6 on 2026-10-14, every servo read torque 0
and a goal register of 0, all five, when power came back. The head had
drooped in the cut, its pitch from 1,771 to 2,022 to 2,034 (toward the
base, some 250 counts), and the yaw and the roll had moved 10 to 80. Lifting
it from there, the head did not follow a goal a little ahead: a goal 40
counts up moved it 7 counts and it stuck at a load of 144; on a ramp of 260
counts in 3 s it lagged its goal by 40 to 80 counts at loads of 200 to 320
and stopped 37 short of the end (pitch gain P 32). The first wake, found at
yaw 1,958, shoulder 2,045, elbow 2,083, roll 2,100, pitch 2,034, ended after
a few seconds on the 30-count tracking bound (goal 2,000, head 2,032), and
its report said `"woke":null`, since only a finished wake was reported.
That is why a lifting head is led and given room, below. The fix has run
only against simulated servos, including that found pose with lamp-52e6's
options.

A lamp whose power comes back is found wherever it fell or was pushed: on
lamp-4ace the head rests pressed on the base (2,095 on 2026-10-12, above
the start window), and a lamp may also be found turned, or with its arm
somewhere a hand left it. Without `--wake on` the body refuses such a lamp
as before. With it, and with a `--home` given (without one, where the lamp
is found is its home and there is nothing to wake to):

- It reads every servo before moving any, takes hold of joints without
  torque if told to (`--torque on`), and checks torque, status, warmth and
  load exactly as before. Waking changes only the two checks of where the
  joints are.
- A head found outside its start window, or a commanded joint found farther
  from its home than it may start (60 counts folded, or the widest of
  `--reach`), but no more than 400 counts from it, is not refused. Before
  the body runs, the three joints are brought to their home the way
  something alive wakes, not the way a machine homes (the repository's
  rule 8): first a stir of up to 24 counts toward home (80 for a head that
  lifts and for the base either way: their goals must lead them by about
  that much before they move at all; on lamp-4ace the base, gain P 16,
  stuck at 2,101, 34 behind a goal moving at the wake's speed, after 72
  counts of a 286-count wake),
  a second's pause,
  then the rest of the way, each eased at both ends and timed so that the
  joint with the farthest to go moves at 20 counts a second at most, two
  fifths of the body's own speed. Every goal starts where the joint is, so
  nothing jumps; each joint only ever moves from where it was found toward
  its home, so the head found on the base only ever rises away from it.
  The head's pitch and roll go first; the base turns only once they have
  arrived or settled (below), with its own stir and pause. On lamp-52e6 on
  2026-10-16, slumped with its head resting on its own base, the three
  woke together and the base, dragging the head across the base's top,
  read a load of 224 over its 220 and the run ended; by hand the base
  turns freely. The head pressed on the base at 2,095 on lamp-4ace's home
  of 1,950 takes about 12 s; lamp-52e6's head drooped to 2,034 with its
  base 77 off, about 28 s; a base turned 300 counts, about 24 s. A head
  and a base each 400 off take about a minute.
- On the way it reads all five joints every step. A changed status byte, a
  shoulder or an elbow that moves more than 15 counts (on lamp-4ace the
  shoulder swayed 7 as the base turned 189 counts of its wake, one over the
  run's 6, which ended that wake on `drift`), a commanded joint over
  its load bound for six steps (400 for a head that lifts, which read up to
  320 doing it, otherwise the run's own), off its goal by more than 100 for
  ten steps while the goal is on its way, or a wire that fails for half a
  second ends the run, holding each joint where it is. Once every goal is
  home the wake waits for the joints: it is over when each is within the
  run's own 30 counts of its goal or has settled. A joint farther than 30
  that has not come 3 counts nearer in half a second has settled if it is
  within 50 counts of home, and otherwise ends the run. The run then takes
  over the goals the wake left, except that a joint settled more than 30
  short runs from where it is: its goal is written as its position, so that
  nothing keeps pushing, and its home moves there by as much, so that moods
  are laid on the pose it has and the gap is not tried again. The 50 is
  from lamp-52e6 (below): from below, these servos do not close the last
  30 to 40 counts. A step whose write or read failed
  does not advance the wake, so the next goal cannot jump. Asked to stop
  while waking, it holds where it is and ends.
- Farther than 400 counts from its home it still refuses, saying why
  ("base_yaw is 450 counts from its home, more than waking brings back
  (400)"). 400 is about 35 degrees: a head on the base was 145 from
  lamp-4ace's home, and lamp-52e6's home pitch of 1,771 would put its head
  on the base some 300 away. Farther than that, a joint is somewhere no lamp
  has been seen. The whole 400 at the wake's speed takes 31 s for a
  joint, the head's and then the base's about a minute, inside the
  90 s systemd gives a service to say it is ready; the body says it is ready
  only once it has woken and its loop runs.
- On a lamp given `--stand`, an arm found with its elbow folded (within 80
  counts) and its shoulder up is lowered first, before the head and the
  base wake: the shoulder alone, eased down onto its folded count at 40
  counts a second at most, the elbow, the head and the base held where
  they are. That is how a power cut leaves a standing lamp: on lamp-52e6 on
  2026-10-14 the shoulder stayed up at 2,633 on its gears and the elbow
  fell to 2,074, folded; eased down by hand from there over 15 s (39 a
  second on average, 58 at its fastest) the shoulder reached 2,059 at a load
  of 172 at most. 40 is a little under that at its fastest: 580 counts take
  about 22 s, so with the head's wake after it the lamp is up inside 90 s.
  Every step all five are read: a changed status byte, another joint moving
  more than 30 counts for ten reads in a row (one read off is not taken
  as a move, and a hold never asks for it), the held head, base or elbow
  over its own load bound (the run's, `load`: 240, 150, 220 for the head's
  pitch, roll and the base; the elbow's `--arm-load`) for six steps, the
  shoulder over its `--arm-load` for six steps,
  more than 160 from its goal for half a second, or stopping farther than
  80 from folded once its goal is there, or a wire failing for half a
  second ends the run, holding all five where they are. The report says
  what it did (`lowered`, as `woke`). Then the lamp may stand as it would
  have. Any other arm off its way is not lowered: which way down from there
  keeps the head clear of the base and the desk is not known, and these
  two joints carry the lamp, so the lamp runs folded and says why
  (`standing_refused`: "... off the way between folded and stood up;
  waking does not lower an arm found off its way"). The head still wakes.
- In the run itself, with `--wake on` only: a commanded joint farther than
  30 counts from its goal but no farther than 50, under its load bound,
  whose goal has not changed and which has not moved 3 counts in half a
  second rests where it is, instead of the run ending on tracking: its goal
  becomes its position and later moods are laid from there, as for a joint
  a hand left (`rested_short` counts the times). It is the same weight as
  above: on lamp-52e6, woken with its shoulder up, the head held 30 to 37
  counts short of goals of 1,759 and 1,775 and the run ended on tracking
  three times in a row. Without `--wake on` such a joint ends the run as
  before.
- The report says what the wake did (`woke`: the joints it moved, where
  each was found, where their goals had got to, where they were when it
  ended (`settled`) and the seconds it took),
  whether it finished or not, and the journal has a line with the same when
  the body ends.

A lamp found where it may start writes exactly what it wrote without
`--wake on`.

The led wake was then run on lamp-52e6 from a real droop (found yaw 1,979,
roll 2,101, pitch 2,029): it woke for 20.8 s at peak loads of 232, 140 and
76, the head lifted 226 counts and stayed 32 short of home (the yaw 11, the
roll 17), and that build ended there on the stopped-short rule ("tracking",
goal 1,771, head 1,803). Settling within 50 of home, above, is the answer
to that; it has run only against simulated servos. Not shown: that the
head stays where it settled once its goal is its position, nor whether a head pressed on the base reads
under the start load bound (170), which waking does not relax; nor how the
wake looks to somebody beside the lamp. Folded with its torque switched off
by hand, lamp-52e6's head did not droop in 10 s; it drooped in the cut.

## Standing up

Not yet run on a lamp. Everything in this section is what the code does
against simulated servos.

With `--stand PITCH,ROLL,YAW,SHOULDER,ELBOW` (which needs `--home` and
`--folded-arm SHOULDER,ELBOW`, all in encoder counts) the body also commands
the shoulder (servo 2) and the elbow (servo 3):

- A mood other than rest raises the lamp from its folded pose to the one
  given. It stays up while such moods keep arriving and for `--awake-seconds`
  (90 without it) after the last, then folds down again. Asked to stop, it
  folds down first; that may take 3 s and twice the way.
- A lamp with eyes (see "Looking at a face") also rises for somebody who
  comes to sit in front of it, without a word: a face in six looks running,
  about a second and a half at the eyes' four looks a second, counts as company, and company keeps it awake exactly as
  being spoken to does. So it does not fold away from a person who is still
  there, and folds once they have been gone for `--awake-seconds`. A face
  for less than that, somebody walking past, wakes nothing. Not yet seen on
  a lamp: whether a folded lamp-4ace's camera sees a person at the desk at
  all.
- The way is one number from 0, folded, to 1, stood up. It moves at most the
  whole way in `--stand-seconds` (5 without it; 2 to 20), reaching that speed
  in half a second, and the two joints follow it eased at both ends, so
  neither starts or stops with a jerk. `--stand-share PERCENT` ends the way
  early: at 10 the shoulder moves about a degree and a half. It is there to
  bring a lamp up in steps the first time.
- The head keeps its folded pose until the arm has made a quarter of its way
  and is back at it before the arm comes down that far. Then its home moves
  to the standing pose's head pitch, roll and yaw, every gesture stays an
  offset from that home, and the head is carried along with it. The window
  that keeps the head off the base applies only while the head is beside the
  base, that is in that first quarter.
- Stood up, the head has room it does not have beside the base: it may look
  down as far as it may look up, and with `--stand-roll N` it may tilt N
  counts either way, both growing with how clear of the base it is. That
  is what gives a standing lamp the tilt it listens with (85% of that
  reach, to alternate sides), which a folded lamp-4ace cannot make. On
  lamp-4ace itself it is left off: tried on 2026-10-13 with 110 counts,
  the roll servo moved freely once the lamp stood, but its axis is the
  round head's own, so nothing is seen from outside and only the camera's
  picture rolls, by about nine degrees. The
  head's pitch is never asked for less than 1,670, just above that servo's
  own limit, whatever a mood, the eyes and a gesture add up to.
- The goals of all five joints go out in one sync write.
- An overload flag (`0x20`) latched on the shoulder or the elbow before the
  run leaves that servo holding with a fraction of its torque. Such a joint
  cannot carry the lamp, so at start the body writes torque on (register 40)
  to it once, and if the flag is still there, off and on. If it stays, the
  body does not stand and runs as a folded lamp; the report says why
  (`standing_refused`) and counts the writes (`torque_writes`). Besides
  `--torque on` (in "What it may do"), this is the only write of torque
  there is.
- It also does not stand, and says why, when it was given no home, when
  either joint has torque off, another flag, or is at 62 or warmer, or when
  the two are not found within 80 counts of one point on the way between
  folded and stood up. Found part of the way up, on that way, it takes the
  lamp as it is and stays up as if it had just been spoken to; but found
  up with only an arm at 62 or warmer (or over 100 on every first read),
  it folds down first, as when asked to stop, and is let go folded, on
  `Stop::Temperature`
  (`a_lamp_found_standing_with_a_warm_or_unreadable_elbow_folds_then_lets_go`).

A shoulder or an elbow that reads over its load bound (`--arm-load`, 950
and 950 without it) for six steps, is more than 160 counts from its goal for
half a second, or reads 62 or warmer for a second sends the lamp back down
the way it came. It rests there for a minute whatever it is told, and after
the third time inside half an hour (`Standing::RETREAT_WINDOW`) for half an
hour (`Standing::LONG_REST`), and then it may rise again; the report lists
each time (`retreats`). While it rests so, its health is a warning, "its arm
sat down for a strain three times in half an hour: it rests folded and may
rise again in N min" (`Glimpse::resting_s`), never an error, so the lamp
does not droop as unwell. Until 24 Oct the third time was for the rest of the
run: lamp-4ace sat down for three strains of the elbow (loads 572, 573 and
586 against 560) in half an hour on 20 Oct and never stood again in a
five-day run, its gestures all dropped as "not standing". Only an arm still over its load or off its goal two seconds
later ends the run, held where it is. The load bound is high on purpose:
the old runtime's notes say these two joints read 830 to 1,000 in free
motion on another lamp, so following and warmth are the real guards.

The pose lamp-4ace is given is the old runtime's idle pose
(`hal/recordings/idle.csv`, first frame): head pitch 1,772, shoulder 2,657,
elbow 2,602, with this lamp's own roll and yaw. Its folded arm reads 2,048
and 2,047. On 2026-10-09 the old runtime saved a pose of this lamp with the
shoulder at 2,658 and the elbow at 2,650, so it has stood there; nothing
else about the pose has been checked on this unit, and the old runtime
reached it by moving all five joints together over five seconds.

## Looking at a face

Not yet run on a lamp. This is what the code does against simulated servos
and a simulated camera.

With `--gaze-socket PATH` and `--gaze YAW,PITCH` the body takes where a face
is from `lamp-eyes` ([eyes.md](eyes.md)) on that socket: one datagram a look,
`{"face":{"x":..,"y":..,"size":..,"turned":..}}` or `{"face":null}`. Anything
else is ignored, and only the newest waiting counts.

Which face that is, is the eyes' choice: the largest near one, or, with
`LAMP_TURN_TO_ANSWERED=on` in the eyes' unit, the one seen asking while the
lamp answers it ([eyes.md](eyes.md), "Turning to the one it answers").

A skill's look (`lamp-skills`, "look at the face") comes on the same socket
in the same shape, and the body cannot tell it from the eyes' own: it is
taken or dropped by the same rules, and a look alone never stands a folded
lamp up. A face across the room is not a reason to get up; being spoken to,
somebody sitting in front of it for a second, or a gesture asked is. When
whether a face in view is looked at changes, the journal says so once, with
why (paused, asleep, folded or folding, neither listening nor speaking), and the
face line every five seconds says it too:

```text
lamp-body: step N a face is not looked at: folded or folding
lamp-body: step N a face is looked at
```

The camera is in the head, so turning toward a face moves the picture too.
The gaze (`src/gaze.rs`) therefore works in small corrections with stillness
between them:

- A sight counts only if no joint has been asked to move for 0.4 s, the lamp
  is listening or speaking, and it is not folded or folding. A picture taken
  while the head moved shows where the face was.
- After three such sights in a row with a face in them it takes the middle
  one, across and down separately (their mean until 2026-10-13, when one
  stray face at the corner of a picture moved lamp-4ace's head).
  A face within 0.15 of the middle across and 0.20 down is being looked at
  already. While the lamp speaks those are 0.35 and 0.40, since its head
  should be still then for its microphone.
- Otherwise it adds 70% of what would bring the face to the middle to an
  offset of base yaw and head pitch: `--gaze` says how many counts of each
  bring a face from the right edge, and from the bottom edge, to the middle,
  and their signs say which way each joint moves the picture. Under the whole
  amount on purpose: a number set too high still settles.
- The offset is added to what the mood wants and goes through the same
  window, speed and load rules as every gesture. It is bounded by
  `--gaze-reach` (250 and 120 counts without it), and sideways that may be
  wider than the reach of the poses: the head turns further to a person
  than it does to think. On lamp-4ace on 2026-10-13 people standing at the
  lamp were further round than the 250 counts (22 degrees) it could then
  turn, the face staying at 0.44 to 0.75 of the half-picture off centre.
  The old runtime's recorded moves turn the base 42 degrees either way, and
  the gaze may now be given up to 500 counts (44 degrees). Then it waits to
  be still again.
- It never searches. With no face in the picture nothing is added, and when
  none has been seen for six seconds, or the lamp folds down, the offset goes
  back to nothing at 60 counts a second.

So a person who shifts in their chair is followed in a second or two, in one
or two moves; the look away to think, and back, happens as before, and the
head returns to where the face was.

The two numbers of `--gaze` have to be found on the lamp: nothing here knows
how wide its camera sees or which way its joints turn the picture. The old
runtime's own notes give its camera anything from 60 to 123 degrees across.

Found on lamp-4ace on 2026-10-13, standing, by telling the body a face was to
one side (a small script sending the eyes' datagram, no camera involved) and
comparing one frame taken by hand before and after: 250 counts more of base
yaw moved the scene 110 of the picture's 320 pixels of half-width to the
left, and 120 counts more of head pitch moved it 52 of 240 pixels of
half-height up. So a half-picture is worth 727 counts of yaw and 553 of
pitch, both positive, near the middle of a lens that sees about 128 degrees
across: `--gaze 727,553`. The body made the two corrections asked of it
(160 and then to its reach of 250) and held. No face was in front of the
lamp for any of this: its eyes have looked at an empty office for several
minutes without reporting one, and found one in a photo file on the lamp's
own processor in 50 ms.

The same day, with a photo of a face filling the screen of the computer the
lamp stands in front of: its eyes reported the face in 66 of 66 looks, an
eighth of the picture high. With the gaze first pushed 250 counts aside
the face was at -0.27 across; the next time the lamp listened the gaze went
to 113 (250 less 0.27 of 727 times 0.7) and the face to -0.11, inside what
counts as looked at. It has not yet been seen with a living person in front
of it.

## Turning to a call

With `--look-at-name on` the body reads `name/heard` on the bus
([name.md](name.md)): the lamp's name, heard, subscribed to as `body`
whether or not `LAMP_BUS` is on; a bus it cannot use is said once in its
log and no call is taken. Off unless given, and without
it a call changes nothing the body writes (test
`without_the_switch_a_called_lamp_writes_exactly_as_one_never_called`).

- A hearing whose name began more than 1.5 s before it arrived is not a call.
  One while the voice says it speaks is not either, and nor is one within
  ten seconds of the last call taken.
- A call is taken as being spoken to for two seconds: a folded lamp rises,
  the head takes its listening pose, and the gaze turns it to a face it sees
  at once (above, "Looking at a face"), as when it is listened to; the
  voice's own listening carries on from there if the person goes on. A lamp
  that is folded looks at nobody until it stands.
- With no face in the last look, the lamp glances once it stands: the base
  turns about 60 counts (five degrees) to one side and the other and back,
  in a second (`clips::CALLED`). It waits as long as rising takes; a face
  seen before or during it ends it, and the gaze has them. It takes the place
  of a shift at rest, never of another gesture.
- It never searches: one glance, then it lets it go.

On lamp-52e6 on 2026-10-14, with calls published by hand and nobody in view
([verification](verification/look-at-name-lamp-52e6-20261014/)): standing,
the base began its glance 135 to 204 ms after the call and reached about
3.5 degrees one way and 5 the other; folded, the lamp rose at once and
glanced 6.8 s later, once up. Said aloud from a Mac beside it, with
`lamp-name` running, the voice had already gone to listening (and lifted
the head) 0.45 s into the line; the name was heard 0.55 to 0.95 s after
"Hey Lamp" ended, in four lines of six, and each time the base began its
glance 83 to 135 ms later. A call with somebody in view has not been seen
on a lamp.

## Leaning toward the person while it speaks

For #933. From the front, where the person sits, the head is a disc facing
them: a few degrees of pitch or roll cannot be seen (lamp-4ace,
2026-10-16). What reads is the base turning and the arm leaning. So a
lamp standing at its full height leans toward the person while it speaks:
the shoulder and the elbow come `LEAN` = 45 and 60 counts (about four and
five degrees) from the pose it stands in, eased in as the answer starts
and eased out as it ends. On by default; there is no flag.

- **Which way.** Down on both, the way V1's nod (`nod.csv`) takes them as
  the head dips toward the person.
- **How far.** That nod goes to 77 and 120 counts and keeps the lamp's
  weight inside V1's `motion.max_cog_offset_mm` (22 mm). About half of each
  moves the centre of gravity a few millimetres, whichever way the shared
  calibration has the joints (worked out with V1's
  `hal/drivers/motors/recording_stability.py` on `robots/lamp/urdf`).
- **How fast.** At least 1.5 s each way with an eased start and end, and
  longer if the elbow would otherwise go faster than the pose follows at
  (`--speed`): at 120 that is under 60 counts a second at its fastest.
- **When not.** Not while the lamp rises or folds, not while a gesture
  plays (the gesture moves the arm itself; the lean eases out and back in
  after), and not after it slumped. While the wire fails the lean is kept as
  it stands, a held place too: nothing new, and nothing dropped and taken
  again once the wire answers
  (`a_wire_failing_mid_lean_keeps_the_lean_as_it_stands`). Every arm check stays as it was: its load bound, its tracking, its warmth, and
  `--limp` and `--overload release`.
- **A hand on the arm.** Under `--give-way`, a shoulder or an elbow that
  stops pushing against a hand is held where the hand has it. While the
  lamp wants to lean and the hand still reads on that joint (a load over
  its stall bound), the lean stays exactly where it was, neither in nor
  out, so the lamp does not lean into the hand again. The hold is taken
  into the lean once the lean eases out (the answer over, a gesture,
  folding), or once the lean is wanted and nothing reads on the joint any
  more, including a hold made before the answer. From then on the joint
  goes from where it was held toward where the lean wants it. That place
  is kept as an offset from the pose, so it folds with the rest of the
  arm. It moves no more than the lean's own step, and not at all toward a
  hand that still reads on it, except once the lamp is leaving: no stall
  check runs then, so a hand keeps nothing and the arm folds to its folded
  pose. So no goal jumps back into the lean, none
  goes past the held place toward a hand that is there, and a hand that
  has gone does not keep the next answer from leaning
  (`an_arm_held_while_it_speaks_is_not_leaned_into_again`,
  `a_lean_eases_out_from_where_a_hand_held_it`,
  `a_lean_eases_out_from_a_hand_still_there_after_the_answer`,
  `a_hand_letting_go_mid_ease_out_leaves_no_jump`,
  `a_held_shoulder_eases_out_as_an_elbow_does`,
  `a_held_lean_folds_with_the_arm_without_a_jump`,
  `a_hand_gone_does_not_keep_the_next_answer_from_leaning`,
  `a_hold_from_before_the_answer_does_not_keep_it_from_leaning`,
  `a_hand_on_the_elbow_does_not_take_the_fold_past_its_pose`). In
  #1010's reviews the held place was first kept by the hold alone, which
  let go 20 counts into the ease-out and jumped 40 counts back into the
  hand. Then it was kept as a fixed count, which stayed put through a fold
  and jumped 134–156 counts at the end, and was held again with no hand
  there, so the next answer did not lean.

With Expression on, a turn of the base that it asks while the lamp speaks,
a sway between sentences, is taken as any other turn it asks: within 70
counts, around the turn the pose already wants, at the speed the pose
follows at, and not for a base that has met resistance (see below). The
body takes it from `base_yaw` in `expression/body`; no new field.

Against simulated servos (`a_speaking_lamp_leans_in_and_back_and_its_base_sways_for_expression`,
`a_base_that_meets_a_hand_is_not_swayed_on_by_expression`): standing at
2,650 and 2,600, speaking for six seconds, the arm reached 2,605 and 2,540
and was back at 2,650 and 2,600 within four seconds of listening again; a
turn of 52 counts asked for 1.5 s moved the base's goal 35 to 52 and back,
no goal moving more than 3 counts in a step; and a base that met
resistance as the turn was asked stayed where it gave way for the two
seconds after.

**On lamp-52e6, 2026-10-16, 11:48**, body `92634401` (this and the reading
pose below, built from `fifth/reading-pose`), with its unit's flags
unchanged, and the lamp made to answer "Lamp, can you tell me a short
story?" from a Mac beside it. Its body journal, every half second:
speaking from 11:48:28.1 to 11:48:35.2, the arm's goals went from 2,657
and 2,602 to 2,645 and 2,586 at 0.5 s, 2,612 and 2,542 at 1.5 s, and held
there; the joints read 2,609 and 2,537 by 5.5 s (the shoulder 14 to 18
counts short of its goal for the first four seconds), at loads of 24 to
84. No retreat, stall or slump. The base did not sway: Expression on that
lamp still asks its sway of the head's roll (`sway_roll`), which this
lamp's reach of one count does not take. A Mac camera beside the lamp saw
it from the side; nobody has yet judged from the front how the lean reads.

## Reading: a look down at the desk

For #933 and the Desk tutor skill (#941). A skill that wants the lamp to
read a page in front of it sends `{"mood":"reading","level":0}` to the
mood socket, and says it again at least once every 1.5 s
(`MOOD_STALE`) for as long as the lamp reads. The body then:

- **Looks down.** The head's pitch goes as far down as its bounds let it,
  which stood up is as far as it may look up (`--reach`'s first figure: 150
  counts, about 13 degrees, on lamp-52e6), from the pose it stands in. No
  more: the bounds, `--pitch-window` beside the base and the pitch floor
  are all as before, and the pose is followed at `--speed`. The eyes do not
  lift it to a face and Expression does not nod it while it reads; the
  base's turn is as in any other mood.
- **Leans forward** as it does while it speaks (`LEAN`, above), with the
  same conditions but one: a gesture does not take the lean away. A
  skill's `acknowledge` or `curious` at the page is laid over the lean,
  which is held exactly where it is while the gesture plays and goes on
  easing in after, so the lamp keeps looking at the work instead of
  pulling back for two or three seconds and leaning in again (#1260
  review, 09:19). Held, the pose V1's anti-tip rule checks the gesture
  over as it begins (the lean included) is the pose it plays over: a
  gesture that would tip over it is refused whole, as any other is, and
  each step is still cut at the lean-back bound and eased at most
  `ARM_GOAL_STEP`. Speaking, a gesture still lets the lean go: that lean
  is toward a person, and the gesture is the move made at them instead.
  The gesture's pitch is laid over the reading pitch after the bounds,
  kept only from the pitch floor and the seam, so the whole of it is
  made: `curious` lifts 58 counts above it and dips 190 below;
  `acknowledge`'s pitch only lifts (28 counts), its nod is the arm's.
- **Holds** it, and comes back to the voice's mood once the skill stops
  saying `reading`: the pose is followed back at the same speed.
- **Stands up for it** if it is folded, as for any mood but rest.
- **Turns toward a place, if asked** (#1679, "light my book"):
  `{"mood":"reading","level":0,"toward":X}`, X from -1 (the picture's left
  edge) to 1 (its right), turns the base as the gaze would turn it to a face
  seen there: 70% of X times `--gaze`'s yaw for a half-picture, within
  `--gaze-reach`'s yaw (`Gaze::toward`), added to the base's turn of the pose
  and followed at `--speed` through the same window and rules as every pose.
  Held while `reading` with it is said; a `reading` without it, or the pose
  let go, takes it away at the same speed. Nothing without `--gaze`, which
  alone says how far a place in the picture is, and nothing in any other
  mood or paused. A `toward` past -1 to 1 turns nothing. The place is the
  model's, from the voice's look at the desk (Point the light,
  `skills/point-light`): the eyes find faces only. A body before this
  refuses the whole datagram, `toward` and all, so the lamp's body and its
  skills go on together. Against simulated servos
  (`a_reading_lamp_turns_toward_the_place_it_is_asked_and_back`): X = -0.5
  with a 342-count half-picture turns the base 120 counts from where plain
  reading has it, at most 3 counts a step, and back after; nothing without
  a gaze or listening; past a reach of 100, 100. Not run on a lamp.
- **Never toward a person** (#1738, "look for my keys"): a `toward` on the
  side of the picture where the eyes last saw a face, within the 6 s the
  gaze keeps a face before it lets go (`LOST_US`), is not turned to: the
  base stays where plain reading has it, the head over the desk, and the
  journal says once per refused place `not turned toward X: a face is seen
  on that side; the head stays over the desk`. A face within 0.1 of the
  middle (`FACE_SIDE`) is on neither side; a face on the other side, or
  none, changes nothing (`Gaze::face_toward`). So the search skill's "never
  looks a person over" is the body's rule, not only the model's note.
  Against simulated servos
  (`a_search_does_not_turn_toward_the_side_a_face_is_seen_on`): a face at
  -0.5 keeps a -0.5 look exactly where plain reading is; a face at 0.5, or
  none, turns it the same 120 counts. Not run on a lamp.

While a skill says `reading`, the voice's own moods on the same socket
(`resting` between turns, sent every 40 ms) do not take the pose from it,
or the two would take turns at the head every step; once it is unsaid for
1.5 s they count again. A paused lamp rests as before, reading or not.
Of the moods that reach the body within one step, a `reading` counts
even when the voice's `resting` came after it, so every `reading` a skill
sends renews the hold (`of_this_step` in `src/bin/lamp-body.rs`). Before
that, on lamp-52e6 at 14:30 with `reading` every second, the lamp fell to
rest twice in ten seconds when the two met in one step. After it, body `93a64bd1` at
14:36 to 14:40, `reading` every second for 20 to 30 s, four runs: with
the voice idle, twice with a Mac asking the lamp for a story (the voice
held both answers back, a busy room), and once with a second sender
saying `speaking` every 40 ms as well: `reading` without a break each
time, ending 1.5 s after the last one.

Against simulated servos
(`a_reading_lamp_looks_down_at_the_desk_holds_it_and_comes_back`): stood
up at 1,900, the head's goal went down to 2,048 to 2,050 and stayed there
for the six seconds of reading, under an asked Expression lift of 40
counts; the arm leaned by `LEAN`; afterwards both were back where the lamp
listened, no goal moving more than 3 counts in a step.

A gesture over reading, with lamp-4ace's and lamp-52e6's flags
(`a_reading_lamp_keeps_its_lean_through_a_gesture`,
`a_gesture_asked_as_the_reading_lean_eases_in_holds_it_where_it_was`,
`a_reading_lamp_makes_the_whole_of_a_gestures_pitch`): `acknowledge`
dips the elbow from the lean's 2,542 to about 2,220, never lifts the arm
above the lean, and leaves it at 2,612 and 2,542 on every step after
(before, it went back to 2,657 and 2,602 and leaned in again over 1.5 s);
asked while the lean is still easing in, the arm ends the gesture where
it began. The head's goal goes 50 or more counts above the reading pitch
and 180 or more below it for `curious`. No arm goal moves more than 50 a
write, no `Window` stop, nothing refused or cut by the anti-tip rule, no
pose past 22 mm. Speaking, the same `acknowledge` still lets the lean go:
the elbow is back at its stand pose (2,602) once it is over, then leans in
again (`a_speaking_lamp_still_lets_its_lean_go_for_a_gesture`).

**On lamp-52e6, 2026-10-16, 11:51**, body `92634401`, unit flags
unchanged, `reading` sent every half second for 14 s by a small script on
the lamp while the voice went on sending its own moods. The head's goal
went from 1,670 (lifted to a face) to 1,919 in 2.5 s and the joint read
1,919, three counts short of the stood pose's 1,772 plus 150; the arm
leaned to 2,612 and 2,542 and read 2,610 and 2,536, loads at most 48. The
mood stayed `reading` throughout, and went to rest 0.6 s after the script
stopped. The base, which the eyes had turned 380 counts toward a face,
came back toward straight ahead at about 60 counts a second while it read:
reading is not attending, so the eyes let the face go.

What the camera saw, measured on the lamp and never copied off it: the
eyes' frame (`/run/lampos-eyes/sight.jpg`, 320 by 240, not the full look's
1,280 by 960) in four bands from top to bottom, mean brightness and a
crude texture (mean difference between neighbouring pixels):

| | top | | | bottom |
|---|---|---|---|---|
| before, head lifted to a face | 146, 2.8 | 169, 7.4 | 79, 13.2 | 61, 14.1 |
| reading, 6 s | 151, 8.0 | 74, 13.3 | 78, 12.4 | 145, 9.2 |
| reading, 11 s | 166, 7.9 | 81, 13.4 | 73, 11.5 | 131, 8.0 |
| 8 s after | 174, 3.5 | 130, 14.6 | 70, 14.7 | 84, 8.8 |

So the view moved down: a bright surface, the desk, filled the bottom
quarter while it read. Whether a page there is readable at 1,280 by 960
needs a person at the lamp to look at a full look taken in this pose;
frames do not leave the lamp, so there is no photograph here.

## What Expression may add

On lamp-4ace since 2026-10-14, 10:05, switched on. What it did there is
at the end of this section.

The part that plans what the lamp shows ([expression.md](expression.md))
may ask the body to add a little to the pose it is in: a nod while the
person pauses is the first thing it is for. With `LAMP_EXPRESSION_BODY=on`
in the body's unit (and the bus on), the body listens to `expression/body`
(`src/expression_in.rs`). Off by default.

- **What is taken.** The head's pitch, the head's roll and the base's
  turn, in radians, turned into counts (652 to a radian). A positive pitch
  dips the head, as in the body's own poses. The shoulder and the elbow
  are the arm that carries the lamp and are not moved for expression (the
  body leans them itself while the lamp speaks, above). A
  gaze is not taken: the body already looks at the face its own eyes
  report. A request for stillness adds nothing.
- **How it is applied.** Added to the pose the mood wants, before
  anything else is done with that pose. So it goes through everything the
  mood's pose goes through: each joint's reach, the speed and
  acceleration the pose is followed at (120 counts a second on lamp-4ace),
  the rest of a joint that has arrived, and the hold after resistance. It
  is cut first to 60, 45 and 70 counts (pitch, roll, turn: about five,
  four and six degrees), whatever was asked. Nothing is added while a
  gesture plays, and a lamp at rest is not stirred by it (but see "A
  glance from rest" below).
- **With the gaze.** The base's turn from Expression (#933's sway between
  sentences, out at most about 52 counts from centre and back) is added to
  the turn the body's own gaze has, never instead of it. While Speaking
  the gaze corrects only a face more than 0.35 of a half-picture off the
  middle, and only from a head still for 0.4 s. On lamp-52e6
  (`--gaze 782,557`) 52 counts move a face about 0.07, so a face within
  about 0.28 of the middle is never pushed past 0.35 and the sway is left
  alone. A face further out than that is not corrected during a sway
  either: the sway moves the goal through its whole 1.6 s, and the gaze
  waits for a head still for 0.4 s. Expression's sway relies on this 0.35.
- **Which request counts.** One that passes its own check, is of this
  start of the machine (the kernel's boot identity), has not run out, was
  not made ahead of the body's clock, lasts no longer than a request may
  (a fifth of a second), and is newer than the one in hand from the same
  start of the planner. Anything else is not taken, and the body says so
  in its journal for the first three and then ever more seldom.
- **When the planner stops.** Its last request runs out 0.2 s later and
  the pose is the mood's own again, followed back at the same speed.
- **Never waited for.** A thread of its own takes requests from the bus;
  the body's loop looks at what has come and goes on.

The body's report at the end of a run has `expression_steps`: the steps
in which something was added.

Against simulated servos, a lamp standing and listening, asked for a dip
of 40 counts for 0.6 s: the head's goal dipped 24 to 41 counts and was
back within 6 of where it listened from, no goal moving more than 3
counts in a step, and neither the tilt nor the turn moved. Asked for 500
counts of dip and of turn, it dipped no more than 60 and never under its
own reach, and turned 60 to 70. Asked while at rest, nothing was written.

**On lamp-4ace, 2026-10-14, 10:09, without a sound.** The lamp was made to
stand and listen through its mood socket (a socket of its own for the
trial, so that the voice's own word of its mood did not mix with it), and
its head was asked for five dips the way Expression asks: requests on
`expression/body` every 20 ms, each lasting a fifth of a second, easing
out to an offset and back (`cargo run -p lamp-body --example
expression_ask`). The joints were recorded from `body/joint_states`, one
reading every 80 ms.

| Asked | The goal dipped | The head itself dipped | Farthest, after the asking began | Back within 6 counts |
|---|---|---|---|---|
| 40 counts, out in 0.4 s and back in 0.4 s | 28 | 20 (1.8 degrees) | 0.72 s | 1.12 s |
| the same | 24 | 17 (1.5 degrees) | 0.65 s | 1.13 s |
| the same | 25 | 30 (2.7 degrees) | 0.73 s | it came to rest 11 counts from where it began |
| 55 counts, out in 0.5 s, held 0.15 s, back in 0.5 s | 37 | 26 (2.3 degrees) | 0.78 s | 1.42 s |
| the same | 49 | 38 (3.3 degrees) | 0.78 s | 1.89 s |

The base's turn did not move. The body took every request (it says when
it does not) and met no resistance.

What that shows: the way from a request on the bus to the head moving
works on the lamp, the head makes about half to two thirds of a dip
asked at this pace, it is at its farthest about 0.3 s after the request
is, and a dip and its way back take a little over a second. The head
does not start until 20 counts are asked of it, which is the first
150 ms of an eased move, and then it has to catch up. It can end up to
12 counts from where it began: a joint that is within 20 counts of
where it is wanted rests there.

What it means for a nod: the built-in one (23 counts, over 0.32 s) cannot
be made by this head. And a nod that takes a second cannot be made inside
a pause in someone's speech here, because 0.6 s of quiet ends their turn;
what this head can do is nod once as someone finishes, on its way to
looking aside to think. That needs the planner to carry a move begun
while listening on into thinking.

Not shown: what it looks like. Five photographs were taken during the
trial, one every five seconds; a move of a second and three degrees
needs film, or someone watching.

### A glance from rest

Task 32 (#710), on lamp-52e6 since 2026-10-14, 18:11, for the trial only;
on unless given `off` (since 2026-10-15; it was off unless set). With `--rest-glance on` (and `LAMP_EXPRESSION_BODY=on`), a
lamp at rest takes what Expression asks of its head, so that a resting
lamp lifts its head at a sudden sound and lets it go (the repository's
rule 8). Without it, a lamp at rest is not stirred, exactly as before.

- **What is taken.** The head's pitch and roll only, cut first to 50 and
  25 counts (`REST_GLANCE_REACH`, about four and two degrees) and then to
  each joint's own reach and bounds; the roll is held to the reach it has
  folded or stood (one count on lamp-52e6). Never the base's turn, never
  the arm.
- **From where it rested.** Taken out of rest, the glance is laid on where
  the head and the base came to rest, not on home, and the head goes back
  there when the requests run out, at the speed the pose follows at, and
  rests again. The base is not written. Rising or folding, or a mood other
  than rest, ends the glance and the lamp goes on as before. Where it
  rested is where it has rested for a second, or, as a glance begins,
  where a head already still at rest is held: the factory's self-test may
  begin before that second and asks its next move within it, and laid on
  the pose instead, lamp-52e6's head was not led back (no lead in the
  journal) and rested 17 below (#2818).
- **Led from below.** A head lifting from rest has its goal 40 counts
  ahead (`GLANCE_LIFT_LEAD`), never past its bounds: from below,
  lamp-52e6's head did not start for a goal 30 ahead. So is a head coming
  back up after a glance dipped it, for as long as it reads more than 6
  counts below its way back: unled, lamp-52e6's stopped 26 short of where
  it rested and rested there, and the factory's `head_move` failed (#2818).
  Led without coming a count closer for 1 s (`GLANCE_LEAD_STILL_STEPS`),
  it is led no more until it is back: a glance is judged as a gesture, so
  nothing else ends a lead into a hand or a stall. A head creeping back
  against its weight, with pauses, keeps its lead: cut after 200 ms,
  lamp-52e6's rested 15 to 19 low. The journal says when a lead starts and
  ends, and why (`glance lead starts` / `ends: arrived` / `ends: no
  progress for 1 s` / `ends: the glance is over`), with the head, its way
  and its goal.
- **Judged as a gesture.** While it glances and for half a second after,
  a head that lags or strains is let go as in a gesture, and the run goes
  on. The load, warmth and wire checks stay.

Against simulated servos, at rest for eight seconds and asked for 500
counts up, 500 of tilt and 40 of turn for 1.5 s: the goal rose 80 to 90
(50 and the lead), the tilt stayed within its one count, the turn did not
move, and four seconds later every goal was within 6 of where it rested.
Without the switch, the same run writes exactly what a run asked nothing
writes. Stood, with a head that stops 26 short of any goal it lifts to,
dipped 40 for a second: back within 2 counts of where it rested in 3 s
(26 short without the lead back).

**On lamp-52e6, 2026-10-14, 18:02 to 18:11**
([readings](verification/rest-glance-lamp-52e6-20261014/README.md)),
requests published by hand on `expression/body` in the shape of
lamp-4ace's "glance at a sound" (0.07 rad up in 250 ms, 0.03 rad of
tilt, back by 1.6 s):

- **Standing, at rest.** The head rose 40 counts (1,736 to 1,696, about
  3.5 degrees) within half a second, held, came back to 1,733 to 1,740 and
  rested; the roll moved its goal by one count, the base and the arm not
  at all.
- **Folded, at rest.** The head did not move: its goal may not go above
  `pitch_window` (1,740) while folded, 30 counts over where lamp-52e6's
  head rests (1,770), and a goal 30 ahead does not lift it. A folded
  lamp-52e6 glances only if that window is widened, which is not done
  here. With this lamp's own window (`--pitch-window 1670,1995`, task 38,
  "lamp-52e6" below) the same request at 20:41 lifted the folded head 57
  counts (1,764 to 1,707, about 5 degrees) within half a second; it was
  back at 1,758 two seconds later, roll goal one count, base and arm not
  written.

**On lamp-4ace, 2026-10-14, 18:40 to 18:44** (the first builder,
[readings](verification/rest-glance-lamp-4ace-20261014/README.md)), on by
default in its unit since 18:41. A minute with the switch off changed
nothing. A knock and a bang played from a loudspeaker a metre away did not
reach the ears' 30 dB threshold, so nothing was asked. One sudden sound
published by hand on `ears/sudden`, the lamp folded at rest: Expression
asked for its glance, and the head lifted 49 counts (1,952 to 1,903, about
4.3 degrees), highest about 1.2 s after, and was back within 6 counts a
second later; roll, base and arm as before. lamp-4ace's home pitch is 210
counts inside the folded window. Seen once, not explained: about 7 s
later the resting head was held 4 to 5 counts from its goal with its load
rising to 163, until its goal moved 8 counts and the load fell to 25.
Whether a folded lamp-52e6 should have its own window: #710's answer is a
lamp's own `--pitch-window` (#719, task 38), in lamp-52e6's unit since
2026-10-14 20:43.

## Gestures

Not yet run on a lamp. This is what the code does against simulated servos.

A gesture is a short recorded move of the whole lamp: a nod, a shake of the
head, a greeting, a laugh, a shy dip, a stretch (`src/clips.rs`, with the
frames in `src/clips/data.rs`). Each is frames of offsets in encoder counts
for head pitch, head roll, base yaw, shoulder and elbow, a fixed time apart,
beginning and ending at nothing. They were recorded on the old runtime
(`hal/recordings/NAME.csv`) and are generated from those files by
`crates/body/tools/generate_clips.py`. These seven keep 35 to 60% of each one's size, and every clip is
slowed until no joint is asked to go faster than 900, 1,300, 1,000, 800
and 800 counts a second:

| Clip | Lasts | Mostly |
| --- | --- | --- |
| `nod` | 3.1 s | the arm bows 93 and 120 counts and comes back, the head with it |
| `headshake` | 5.1 s | head roll 135 either way, base yaw 85 |
| `greeting` | 5.0 s | shoulder up 221, elbow 153, head lifts 123 |
| `laugh` | 4.4 s | head back 134, shoulder back 111 |
| `shy` | 6.0 s | head dips 108, elbow folds 198 |
| `stretching` | 6.5 s | shoulder up 296, elbow 339 |
| `idle` | 10.0 s | shoulder and elbow sway about 40 either way, the head lifts 47 |

Added on 2026-10-16 (the V1 moves judged on lamp-4ace, `docs/skills.md`, "V1's moves and emotions: what we take"):
each is the most active stretch of its recording of the length given, begun at its stillest frame nearby, scaled
into the reach the test below allows (at most 80% of the recording), ended at nothing over its last half second and
slowed under the same speeds. They are what skills and expressions ask for by name. Filmed from the iMac on
lamp-4ace at 10:4x: `acknowledge`, `curious`, `happy`, `excited` and `caring` read from the front; `surprised`,
`confused` and `sleepy` are small there.

| Clip | From | Lasts | Mostly |
| --- | --- | --- | --- |
| `acknowledge` | `acknowledge` | 2.0 s | a quick bow: shoulder 222, elbow 325 |
| `curious` | `curious` | 3.0 s | a lean toward you: head 190, base 142, shoulder 133, elbow 142 (reworked) |
| `happy` | `happy_wiggle` | 4.0 s | a wiggle: elbow 335, head 190, shoulder 171, base 142 (reworked) |
| `caring` | `sad` | 5.0 s | a slow droop: elbow 333, head 157 |
| `surprised` | `shock` | 2.0 s | a recoil: head back 190, elbow 200, shoulder 195 (reworked for the tip line) |
| `confused` | `confused` | 3.0 s | a puzzled dip and turn: head 190, base 142 (reworked) |
| `sleepy` | `sleepy` | 9.2 s | a slow droop with a catch |
| `excited` | `excited` | 4.0 s | two quick bounces up: shoulder 248, elbow 219 (reworked) |
| `scanning` | `scanning` | 9.1 s | a look round, roll 191, yaw 123: only when a person asks |
| `dance_chill` | `music_chill` | 9.1 s | base sweeps about the standing pose, 142 either way (reworked) |
| `dance_groove` | `music_groove` | 8.1 s | a shoulder bounce about the standing pose, 85 back and 78 forward, elbow 158 (reworked) |
| `dance_hype` | `music_hype` | 7.0 s | the whole lamp, fastest of the five (reworked) |
| `dance_waltz` | `music_waltz` | 9.6 s | a slow lean on the elbow, 276 (reworked) |
| `dance_jazz` | `music_jazz` | 9.1 s | a head bob, 191 (reworked) |

Reworked on 2026-10-16 after a person judged every clip on lamp-06c1 (#995): `curious`, `happy`,
`confused`, `excited` and the five dances are not scaled as a whole. Each joint has its own gain, as large as
its reach and its speed allow (1.6 at most), times the clip's emphasis, so that one joint leads and the rest
support it and none is slowed by more than a few per cent (`dance_waltz`, x1.06); the generator's `REWORKED` holds them. They carry no head roll: a head that
does not tilt standing does not tilt in a gesture (above), and on lamp-06c1 `body/joint_states` showed `confused`
and `headshake` command none of the 189 and 135 counts they ask, so a recording's roll is moved onto head pitch
and base yaw. `dance_chill`'s base and `dance_groove`'s shoulder swing about the standing pose (the middle of
the swing eased in and out over 0.6 s, `CENTRED`), and `excited` is its bounce twice (`REPEATS`), as the person
asked after a second look. `excited`'s shoulder emphasis is 0.75 so that, through `lamp-lean` with V1's model
(`deploy/lamp-4ace/lean-first-software.json`, lamp-4ace's stand), its centre of mass stays within about 18 mm (23.3 at
1.0). `surprised` keeps its full head recoil with the shoulder at 0.6, 18.0 mm (25.7 before), and `dance_groove`
is 8.8: every clip is under V1's 22 mm line (#1072) with a margin. The other clips are as before, byte for byte.

The voice service asks for one on the mood socket, `{"gesture":"nod"}`, and
so does a skill (`lamp-skills`); a name that is not a clip is dropped and
said. It asks at most once an answer, from the
first words of what the lamp itself says (`crates/voice/src/body_link.rs`,
`cue_for`): a nod with "yes", "sure", "of course", "okay"; a shake with "no",
"nope", "I don't think"; a greeting with "hello", "hi", "good morning"; a
laugh with "haha"; the shy dip with "thank you". Most answers begin with
none of these and get no gesture. "No problem" and "no worries" get none.
The same in Vietnamese: a nod with "ừ", "dạ", "vâng", "đúng rồi", "có chứ";
a shake with "không", "chưa"; a greeting with "chào", "xin chào"; a laugh
with "hihi"; the shy dip with "cảm ơn"; none for "không sao" and "không có
gì", nor for a bare "có", which begins too many sentences that are not a
yes.

Since 16 October (#985) the kept V1 emotions (above) are asked for the same
way, checked before the nod and the shake: `surprised` with "wow", "whoa",
"no way", "trời ơi"; `caring` with "oh no", "I'm sorry", "sorry to hear",
"oh dear", "that sounds hard", "tiếc quá"; `confused` with a bare "Sorry?",
"Pardon?" or "Huh?" (the lamp asking again, #930), not with "Sorry, I missed
that"; `curious` with "hmm", "ooh", "interesting", "tell me more";
`acknowledge` with "got it", "on it", "will do", "done", "được rồi", "xong
rồi"; `happy` with "yay", "congratulations", "awesome", "that's great",
"that's fantastic", "wonderful", "that's amazing", "that is incredible",
"tuyệt", "hay quá" (a bare "amazing" or "incredible" is not one); `sleepy` with "good night", "sleep well", "chúc ngủ ngon". "No way!",
a shake before, is now a surprise.

What the body does with the request:

- A gesture plays only on a lamp standing still at its full height. Asked
  of a folded lamp that may stand (`--stand`), it stands up for it first,
  through the same way up as for any mood other than rest (with its
  anti-tip and arm-load rules), and stays up `--awake-seconds` after; the
  gesture waits for as long as rising takes and then three seconds more.
  Asked of one still rising (for a mood, or for a gesture asked just
  before), it waits for the rest of the rise, `(1 - stand) × --stand-seconds`
  and never less than the gesture it replaces, and then three seconds
  more. Not stood up for, and dropped: a lamp paused (`lamp/pause`), one
  a hand on the head put to sleep (`dropped: asleep`, "Asleep" below),
  stopping, held by a hand where a joint gave way, or that let go under
  overload or a hand and lies where it slumped (until a mood or somebody
  in front of it stands it up), and one that may not stand. A gesture
  asked while another plays is dropped too, or, under `--arbitration on`,
  weighed by who asks ("Who may ask over whom" below). Paused after a gesture was
  taken, the awake window it opened is given back, so the lamp folds as a
  paused body does: before it began, the gesture is dropped (`dropped:
  paused`); once it plays, it ends first and the lamp folds after it.
- The body has no idea of night, or of Expression's sleep: a gesture from
  outside stands a folded lamp up at any hour. What keeps a sleeping lamp
  down at night is only the Policy's unprompted question ("not late at
  night or early in the morning", `crates/policy/src/moment.rs`), which a
  skill such as Stretch break is asked under `LAMP_SKILLS_ASK=policy` with
  the hour known (`lamp-policy --utc-offset-minutes` or `--zone`). With
  `LAMP_SKILLS_ASK=skip`, nothing does.
- Each gesture asked, taken or dropped, is one journal line with why; one
  refused by the anti-tip rule is `dropped: it would tip`, besides its own
  line ("Not tipping over" below), and the gesture it was making when the
  run stops ends with `gesture over, as the run stopped`:

  ```text
  lamp-body: step N gesture stretching taken: it was folded and stood up for it
  lamp-body: step N gesture nod taken: it plays
  lamp-body: step N gesture nod dropped: paused
  lamp-body: step N gesture nod dropped: a hand holds it
  lamp-body: step N gesture nod dropped: it has let go and lies limp
  lamp-body: step N gesture nod dropped: it let go and lies where it slumped
  lamp-body: step N gesture nod dropped: it does not stand this run
  lamp-body: step N gesture nod dropped: another gesture plays
  lamp-body: step N gesture nod dropped: another was asked after it
  lamp-body: step N gesture nod dropped: stale, it was not standing 3 s after it could have been
  lamp-body: step N gesture nod dropped: a joint could not keep up with it
  lamp-body: step N gesture past_the_line dropped: it would tip
  lamp-body: step N gesture over, as the run stopped
  lamp-body: step N gesture stretching dropped: held by a hand (asked by skill:stretch-break)
  lamp-body: step N gesture stretching dropped: busy, conversation (asked by skill:stretch-break)
  lamp-body: step N gesture stretching ended: by conversation
  lamp-body: step N gesture stretching ended: by a hand
  lamp-body: step N gesture stretching ended: by a hand (the hand from a test input, --hold-from)
  lamp-body: dropped from the mood socket, not a mood or a gesture this body has: {"gesture":"dance"}
  ```

  The same line is said at most once in five seconds, and the next time
  with how many like it were held back (`(N more like it before this)`), so
  a skill that asks in a loop cannot fill the journal. A gesture taken stays
  up for the whole of it: the lamp's awake window is stretched to the end
  of the clip, so it never folds mid-gesture.

  Until 16 October a gesture asked of a folded lamp was forgotten without a
  word: on lamp-4ace a skill's `stretching` at `stand 0.00` did nothing and
  the journal said nothing.
- Its offsets are laid over whatever the mood and the eyes have the lamp
  doing, outside the filter that follows the mood, and leave the lamp where
  it was. They change by at most 24, 34, 26, 21 and 21 counts a step and
  take no joint further than 220, 220, 170, 360 and 360 counts.
- While one plays, and for half a second after, a joint of the head may be
  150 counts from its goal and read a load of 700, and the shoulder and
  elbow may be 260 from theirs. A quick move lags and reverses hard. If a
  joint goes past even that for its usual number of steps, the gesture is
  let go, its offsets go back to nothing at the same bounded speed, and the
  lamp carries on; the run does not end, and the report counts it
  (`gestures`, `gestures_dropped`).
- The eyes correct nothing while a gesture plays, since the picture is
  moving.
- One any frame of which would take the lamp more than 22 mm off its
  base's axis is refused whole ("Not tipping over" below).

With `--idle on` a lamp standing at rest shifts a little by itself now and
then: anything alive that stands with nothing to do does. Not on a clock
(until 2026-10-16 it was every 18 s, `idle` after `idle` all afternoon on
lamp-4ace, #705): each wait is drawn at random from 40 s to 3 min. A lamp
left alone folds `--awake-seconds` after the last mood or face, and folded
it never shifts: that is how it is still with nobody near. Each shift is
one of five small moves (`crate::idle`), the one made last a third as
likely as each other: not ruled out, as with only two to choose from that
would be strict turns, a pattern of its own.

| Move | Lasts | Mostly |
| --- | --- | --- |
| `idle` | 10.0 s | the old runtime's idle loop at half its size (table above); the only one that moves the arm, so not while a hand holds the shoulder or the elbow |
| `tilt_left`, `tilt_right` | 3.2 s | head roll 25 to one side, the head lifting 10; only on a head that tilts standing (`--stand-roll` past `--reach`'s roll), as elsewhere the roll is left out and both are the same lift under a degree. No deployed unit gives `--stand-roll`, so there the moves are `idle`, `look_up` and `dip` |
| `look_up` | 3.2 s | the head lifts 50 |
| `dip` | 3.2 s | the head lowers 40, a breath let out |

The four head moves stay inside the reach of a glance at rest
(`REST_GLANCE_REACH`) and move neither the base nor the arm; they pass
V1's anti-tip rule with the built-in clips (`cog.rs`). A `--moves` file
may not take their names. No shift
while it is folded, rising or folding, while a mood other than rest is
heard (someone talking to it, a call taken as listening), while a call or
another gesture plays or waits, while the head or the base has given way to
a hand or is in one, or while stillness is asked (Expression's `hold` for
someone working under the light, or a pause); any of these lets a shift
under way go and starts the wait again, so no shift comes right after it.

A shift and Expression's rest layer under `--rest-glance` (its breath and
drift, renewed every 20 ms at rest) are one layer, not two:

- A shift starts and ends where the rest offsets hold the head, with no
  dip back to plain rest before or after it; the breath goes on under it
  at half its size.
- One quiet window: no shift starts within 30 s of a drift
  (`SHIFT_QUIET`), and Expression begins no drift within 30 s of the body
  moving by itself (`BODY_QUIET_US`, `docs/expression.md`). A drift does
  not start the wait over; a shift that falls due inside the window waits
  for its end. The body tells a drift from the breath by the offsets'
  average over a breath (5.2 s) moving 5 counts, which it sees a couple of
  seconds into the drift: a shift begun in those seconds plays on, the two
  together as one move.

A gesture that is asked for replaces a shift. Each run draws from its own
seed (the clock at start; tests pass one with `Standing::seeded`). The
report counts shifts apart from gestures (`idles`); the journal names each
(`gesture look_up`).

A head that does not tilt when it stands (`--stand-roll` not given) does not
tilt in a gesture either: the roll in a clip is left out. On lamp-4ace that
joint turns a round head about its own axis.

The gestures need the servos' own speed bound out of the way:
`--servo-speed 1000`. The shoulder's and the elbow's are written as 800 or
more for a lamp that stands.

### Moves from files

With `--moves DIR` (#940, [skills.md](skills.md)) the body also reads the
moves in that folder once at start (`src/moves.rs`), one a file,
`NAME.json`:

```json
{"name": "wiggle", "step_ms": 750, "frames": [[0, 0, 0, 0, 0], [4, 0, 2, 6, 6], [0, 0, 0, 0, 0]]}
```

Each frame is offsets in encoder counts for head pitch, head roll, base
yaw, shoulder and elbow, as the clips above. A move is asked for by name on
the mood socket as they are (`{"gesture":"wiggle"}`) and played exactly as a
gesture: only on a standing lamp, with the same bounds, drop rules and
report counts. It is held to what the generator promises the clips:

- a name of 1 to 32 of `a-z`, `0-9`, `-` and `_`, not one of the body's
  own gestures (nor `called`), and not another file's (the first file in
  name order keeps it);
- `step_ms` from 40 to 1000, and 1.5 to 10 s in all;
- the first and the last frame nothing;
- no joint farther than 200, 200, 150, 350 and 350 counts, nor asked to
  go faster than 900, 1,300, 1,000, 800 and 800 counts a second (with a
  count of rounding at each end of a step).

A file that breaks any of these, is not JSON of exactly these three fields
or is longer than 256 KiB is refused, and its reason is said in the log.
The others are still taken. They are read in the order of their file names;
Mac `._` files and anything hidden are passed over. A folder of more than
64 moves, or more than 1,024 entries of any kind, is refused whole in one
line and not read through, so the start's work stays bounded and no move
plays or not by how many others are there. A folder that
cannot be read is said once, and the body runs with its own gestures. The
start says how many moves it took and their names.

A skill's own move (`do.body.move`, [skills.md](skills.md)) comes whole on
the mood socket instead, in one datagram of at most 16 KiB:
`{"move":{"name":"wiggle","step_ms":750,"frames":[...]}}`. The body holds it
to the same rules (`moves::check`), and
plays it as a gesture: the same drop rules (paused, held by a hand, slumped,
another playing), the same journal line of why it was taken or dropped, the
same smoothing and speed limits, and V1's anti-tip rule below as it begins.
One it cannot play is dropped, and the journal says
`dropped from the mood socket, a move this body cannot play: move "NAME": WHY`.
One with the name of a `--moves` file's move is dropped, and the journal says
`a --moves file's move has the name`, so one name never means two moves. A
datagram of 16 KiB or more (`recv` cuts it there) is dropped as
`a datagram longer than 16 KiB, not read`. The body keeps one sent move a
name: the same move sent again plays what was kept, and a changed one (the
skill edited) replaces it. Past 64 names, a move of a new name is dropped
until the body restarts; past 64 replacements in a run (each replaced move
stays in memory, as it may still be playing), a changed move is dropped the
same way, and the kept ones still play.

### Who may ask over whom (#940)

A gesture or a move on the mood socket may say who asks: `{"gesture":"nod","source":"voice","priority":"conversation"}`,
or `"source"` and `"priority"` beside `"move"`. `source` is free text for
the journal (`skill:<name>`, `voice`); `priority` is one of `person`,
`safety`, `conversation`, `skill` and `idle`, in that order
([skills.md](skills.md), `lamp_msgs::Priority`, the same order as the
ring's). With neither, it is `conversation`, as every caller was before.
Any other priority, or any other field, is not read and is dropped as `not
a mood or a gesture this body has`. Callers may stamp always; each body
decides by its own switch: only with `--arbitration on` (off unless given)
is who asks weighed, and off, `source` and `priority` (even `null`) are
read and not weighed, so a stamped gesture plays as an unstamped one; off,
`priority` is kept only for an unprompted gesture to give way (below).
So the callers' stamping and every lamp's switch need not flip together.
`lamp-skills` stamps `skill:<name>` at `skill` with `LAMP_BODY_REQUESTS=on`
([skills.md](skills.md), #1252); the voice sends its gestures unstamped,
so they are the conversation's.

- Safety wins without asking: the anti-tip rule, the guard on every arm
  goal (`ARM_GOAL_STEP`) and the stop for a hot or loaded joint act on
  whatever plays, as before.
- A person's hand wins next: a gesture asked while a hand holds a joint
  where it gave way is dropped as `held by a hand`, and one asked of a
  lamp that slumped is dropped as before.
- A gesture asked while another plays, at a higher priority than the one
  playing, ends it (`ended: by conversation`): its offsets ease back to
  nothing over 0.7 s (`EASE_OUT`), from the speed they had, on a cubic that
  slows into the end, so the change a step grows and shrinks by a count or
  two and never snaps back or stops dead (in the test, a stretch ended at
  its widest: the shoulder 1, 2, 2, 4 … 13 … 3, 2, 1 counts a step). Never
  more than the gesture's own step (24, 34, 26, 21 and 21 counts), the arm's
  through the same guarded write as every arm goal; the new one begins once
  they are back at nothing. Ended mid-swing, it carries on past the
  gesture's own farthest before it turns, as anything moving does: over
  every frame of every built-in clip, by up to 50 counts of the head's
  roll (headshake), 32 of the shoulder (laugh), 28 of the elbow (nod), 18
  of the head's pitch and 16 of the base's turn (reviewer, 09:46 on
  #1239); never past `GESTURE_REACH`, and V1's 22 mm cut holds through it.
  A slump during the ease ends it with everything else, so the lamp taken
  up two minutes later carries no gesture's offsets. Not yet looked at on
  a lamp: it is, on lamp-52e6, a headshake ended mid-swing among it,
  before the switch goes on. Asked at the same priority or
  lower, it is dropped, naming who holds the body (`dropped: busy,
  conversation`). The body's own turn to a call is the conversation's.
- Of several gestures read in one step (the socket is read up to 32
  datagrams a step), the body is handed the highest priority, the newest
  on a tie, and each other one is said `lamp-body: gesture NAME dropped:
  busy, person` (who holds it). Off, the newest, as before.
- The same holds for one waiting for the lamp to stand: a gesture asked
  lower than the one waiting is dropped; one asked as high or higher takes
  its place, as before.
- A shift at rest (`--idle`) never starts while a gesture plays or waits,
  as before, and any gesture asked ends one.
- An unprompted gesture, asked at `skill` or `idle`, is an invitation: it
  ends the moment a conversation begins, the mood turning from anything
  else to `listening`, `thinking` or `speaking` (a call taken counts),
  `ended: by conversation`; or a hand comes on the head (a joint gave way
  and is held), `ended: by a hand`. Whatever `--arbitration`. Checked every
  motor step, no wait: the step that reads the mood is the first of the
  same 0.7 s ease out as above, and `body/gesture` says it as any `ended`.
  One of the conversation's, safety's or a person's plays on; one at
  `skill` or `idle` still waiting to begin is dropped as `busy,
  conversation`. It is the turn into a conversation that ends it: a
  gesture a skill starts while the lamp already listens, or on the very
  step it turns to listen, plays on, which no skill does, since they ask
  only at rest. One still waiting when a hand comes is dropped with it, so
  it never begins after the ease out. On lamp-c0a4 on 21 October a
  skill's `stretching` played on 3 s after the person spoke. A hand on the arm is left to the arm's
  own rules (in the tests the lamp folds away from it, which ends the
  gesture with no `ended` line). After a hand has shown this once on a
  lamp, a test input can stand in for it (`--hold-from`, below in "Giving
  way to a hand"; off in every product unit).
- With `LAMP_GESTURE_PAUSE=on` (#2602; off unless exactly `on`), turning to
  `listening` only **pauses** it: eased to a hold where it is over the
  same 0.7 s, its own clock standing still, as a creature stops
  mid-stretch to hear. It **ends** when the lamp answers, the mood
  `thinking` or `speaking`, or when the hold reaches 3 s, `ended: by
  conversation`, eased out as above. If the mood goes back to `resting`
  within 3 s with no answer (music, a cough, a line not said to the lamp),
  it **goes on** from where it was held: its clip is taken up from the
  point the hold carried it to, and the arm eases into it from still over
  0.7 s, never swinging back to where it paused. A
  conversation begun straight in `thinking` or `speaking` ends it at once,
  and a hand on the head ends it at once, paused or not. `body/gesture`
  says only the end; a pause and a going-on are not said, since nothing
  that reads it has a word for them. It goes on in the product after one
  lamp run with someone at the arm (the first builder, 22 Oct).

The journal line says who asked, when the datagram said (`(asked by
skill:stretch-break)`). Without the switch, a gesture asked while another
plays is dropped whoever asks, as before.

#### Who asks, from the kernel: `--caller-caps on` (#3504, phase 0)

Any process in group `lampos` may write to the mood socket, and a datagram
may say any `source` and `priority`. With `--caller-caps on` (off unless
given) the body asks the kernel who sent each datagram (`SO_PASSCRED`,
`lamp_ipc::caller`) and holds a gesture or a move to the most its systemd
unit may ask at, whatever it says, if it runs as that unit's user
(`lamp_msgs::MOST_BY_UNIT`):

| Unit | Runs as | At most |
|------|---------|---------|
| `lamp-voice` | root | person |
| `lamp-cloud`, `lamp-actions` | lampos | conversation |
| `lamp-skills`, `lamp-control` | lampos | skill |
| `lamp-addons`, `lamp-expression` | lampos | idle |
| root's other helpers | root | what they ask |
| anyone else, or a known unit run as another user | | idle |

A unit is named only from `/system.slice/` or `/lampos.slice/`, so a login
user's own `systemd-run --user --unit=lamp-voice` is nobody's. Its
`source` becomes the unit's name before what it said
(`lamp-skills/skill:dance`, `unknown`, `root`). A cut is said once a minute
per unit: `lamp-body: lamp-skills asked at person, held to skill`. The cap
holds whatever `--arbitration`, since a skill's gesture asked at
`conversation` would otherwise outlast a conversation beginning. Moods are
taken as before. The gaze socket takes a sight only from `lamp-eyes` or
`lamp-skills` run as `lampos`, or from root, and drops any other, said once
a minute. A datagram too long for the body's buffer is refused whole. No
product unit passes it yet.

#### Held by lamp-control: `--lease-socket` (#3504, phase 0)

The lamp programming protocol's server, `lamp-control`, keeps its own
leases, clients and the 10-minute cap. The body knows one holder only.
With `--lease-socket PATH` the body listens there for one stream at a time,
and lets one in only if the kernel names it `lamp-control.service` run as
`lampos`, or root (`lamp_ipc::caller`). A second, or anyone else, is
answered `refused` with why and closed. Lines of JSON each way
(`lamp_body::lease`):

| Asked | Answered |
|-------|----------|
| `{"hold":{"for_ms":N}}`, N from 1 to 30000: taken or renewed | `{"lease":"held"}`, or `suspended` or `lost` as below |
| `{"let_go":true}` | `{"lease":"free"}` |
| anything else, or a line over 1024 bytes | `{"lease":"refused","why":...}` |

Said unasked when it changes:
- `{"lease":"suspended","why":"conversation"}` while the mood is listening,
  thinking or speaking, and `{"lease":"held"}` again after.
- `{"lease":"lost","why":"hand|slumped|private|paused|asleep|lapsed"}`: a
  hand on a joint, the body let go under load or by a hand (its torque
  off) or let go waiting for a hand to set its pose right
  (`--refused-let-go`), the person's privacy mute, a pause, a hand's sleep,
  or a hold not renewed within its `for_ms`. These six are a closed set
  (`lease::LOST_WHYS`: `Said::lost` asserts each why is in it, so a
  seventh fails the first test that says it): a new one is added to
  lamp-control's `LeaseWhy` in the same change, or the door would keep a
  lease the body has lost (Fourteen on #3900). It is not taken back when that
  ends; the holder asks again. A hold asked while one of these lasts is
  answered `lost` with why. A run that ends with a holder connected tells
  it `lost`/`slumped` first when the body gave out (an overload a second
  time within ten minutes), then closes.

Its holder gone, the lease ends at once, so the lamp's own life is back
within a step if `lamp-control` stops, and within 30 s if it hangs. A
holder that doesn't read its answers is taken as gone too: an answer that
can't be written at once ends the lease. The door never holds the body's
step: each step lets in or refuses at most 4 new callers and reads at most
4 KiB of its holder, and a line past 1024 bytes ends the lease with
`refused`. The socket is mode 0660. Without the bus, the person's privacy
mute can't end a lease, and the body says so at start. While
held, the body takes no shift at rest and nothing from Expression (its
offsets and the rest glance), and a standing lamp stays up past its
`--awake-seconds`, as with someone in front of it. A hold never stands a
folded lamp up by itself: a gesture asked does that, as any gesture does.
Gestures and moves still come on the mood socket, at the caps above. No
product unit passes it yet.

The holder read-budget fixture queues more than 4 KiB with one nonblocking
write before reading its peer. It checks the line cutoff, the 4 KiB read
budget, and that every remaining queued byte is drained before EOF, without
waiting for space in an unread socket.

## Not tipping over (V1's anti-tip rule)

The body keeps V1's anti-tip rule (#641, #642 in the first lamp software;
`src/cog.rs`). It works out how far the lamp's centre of gravity sits off
its base's axis. The model is V1's own (`robots/lamp/urdf/lamp.urdf`, in
`lamp-lean`'s form as `deploy/lamp-4ace/lean-first-software.json`, which
agrees with V1's `cog_offset_mm` to 0.03 mm). The limits are also V1's:

| V1 | Limit | What the body does |
| --- | --- | --- |
| `check_stable`, `SAFETY.md` `motion.max_cog_offset_mm` | 22 mm off the axis, in any direction | A gesture, built in, from `--moves` or a skill's move, is checked frame by frame on the pose the lamp is in when it would begin (the idle shift too). If any frame goes past 22 mm, the whole gesture is refused and nothing moves. One that comes within 85% of 22 mm plays, and a line says so. |
| `limit_lean_back` in `/servo/move` and `nudge` | 22 mm behind the axis | Every step while a gesture plays, and while the lamp rises, folds or is folded. |
| `limit_lean_back` in `distribute_pitch`, `MAX_LEAN_BACK_MM` | 16 mm behind the axis | Every step of a lamp standing still at its full height: the mood's pose, the eyes, Expression, and anything else laid on the pose. |

V1 also cut a step short the same way. Halving ten times, it found the
largest share of the step from the last goals that stays inside the limit,
and wrote that. A pose already past the limit may only come forward. "Behind"
means along the arm's own heading, with base yaw left out. While a step is
held short, what follows the mood is held there with it, and a rise or fold
is taken back to where the arm stopped (the lamp then counts as not yet
stood up), so nothing jumps when the rule lets go. A lamp held at the line
is journaled once: the run of cuts ends only once what is asked is 1 mm
inside it. The check is made once, where the goals of every
step are made, just before they are written. That covers the mood's pose,
the eyes, Expression, gestures, moves and standing up, and anything that
later adds to the same goals. The other goal writes in `src/body.rs` move
nothing new, so they are not checked:

- the wake, which takes the head from where it was found to its folded home;
- lowering a shoulder found up onto its folded count;
- taking hold of, or stopping, a joint where it is read;
- resting a joint where it arrived;
- writing home at the end of a run of a lamp that is down.

By this model a folded lamp sits 18 mm (lamp-4ace) to 20 mm (lamp-52e6)
behind its axis. Standing it sits 2.7 mm in front. That is why the 16 mm
bound applies only to a lamp that stands. A test (`cog::tests`) checks that
every built-in clip passes the rule on both lamps' standing poses. No frame
may come within 85% of 22 mm, and no frame may lean the lamp back past 16 mm.
A regenerated clip therefore cannot be refused at runtime. No clip is let
off: since #1058 trimmed `surprised`, `excited` and `dance_groove`, every clip
is 18 mm or less at its worst frame (`surprised`, 18.0 mm), and none leans
the lamp back more than 8.8 mm (`dance_groove`). The journal has one line each time the
rule acts. A refused gesture's line names the clip, its worst frame and the
millimetres:

```text
lamp-body: step N anti-tip: REFUSED gesture <clip>: frame <n> takes the centre of gravity <mm> mm off the base's axis, over V1's 22 mm (1 so far)
lamp-body: step N anti-tip: cut a move short that would lean the lamp back 26.9 mm, over its 22 mm (V1's rule; 1 so far)
```

The report counts these under `tipping`. The masses in the model are V1's
estimates, 1.49 kg against the 7 kg in the spec. V1 also scored its 22 mm on
motor values it read as degrees ([lean.md](lean.md)). So these millimetres
are V1's numbers on V1's model. They are not a measurement of any lamp.

A pose with a count outside 0 to 4,095 is no pose a servo can be in. The
model takes it as infinitely far off the axis, and a step toward it is not
taken at all; it is not clamped to the seam, which is how an elbow goal of
37,639 was written as 4,095 on lamp-4ace (above, "Every arm goal inside its
window").

## Running it

```text
lamp-body --device /dev/EXPLICIT_SERVO_ALIAS --unit-id PROVISIONED_UNIT --lock-directory /home/WORKER/private-motor-locks --mood-socket /run/lampos-body/mood.sock [--home PITCH,ROLL,YAW] [--stand PITCH,ROLL,YAW,SHOULDER,ELBOW --folded-arm SHOULDER,ELBOW] [--gaze-socket /run/lampos-body/gaze.sock --gaze YAW,PITCH]
```

Reach and speed can be chosen at start, each inside a cap fixed in the code:

Since 2026-10-15, `--wake`, `--rest-glance`, `--look-at-name` and `--idle`
are on unless given `off`: each passed its minute on lamp-4ace and runs
there (`docs/joining.md`, "The switch is for that first minute"). For those
four rows, "Without it" is what `off` leaves.

| Option | Meaning | Without it | Cap |
| --- | --- | --- | --- |
| `--reach UP,DOWN,ROLL,YAW` | farthest a goal may be from home, in counts | 45,15,30,40 | 180,25,100,250 |
| `--speed N` | fastest the commanded pose moves, counts a second | 50 | 300 |
| `--servo-speed N` | goal speed written to the three servos | 150 | 1,000 |
| `--servo-acceleration N` | acceleration written to them | 50 | 150 |
| `--stand P,R,Y,S,E` with `--folded-arm S,E` | the pose it stands in and the arm's folded one, in counts | never stands | shoulder and elbow each at most 1,400 from folded |
| `--stand-share N` | how much of the way up it goes, percent | 100 | 1 to 100 |
| `--stand-seconds N` | fastest the whole way is travelled | 5 | 2 to 20 |
| `--awake-seconds N` | how long it stays up after the last mood that was not rest | 90 | 5 to 3,600 |
| `--arm-load S,E` | load that sends it back down | 950,950, or 40 under `--force-limit` when that is given | 50 to 1,000, and under `--force-limit` |
| `--force-limit N` | the most force the base's turn and, for a lamp that may stand, the shoulder and the elbow may push with, of 1,000 | not written: each servo keeps what it holds | 300 to 1,000 |
| `--give-way return\|stay\|off` (default return) | what a joint does after it gave way, and a joint that pushes without moving told to stop (below, "Giving way to a hand") | `off`: as before the option: no stall rule, a joint that gave way always comes back | return, stay or off |
| `--limp on\|off` (default on while giving way) | a hand forcing a joint past V1's contact floor slumps the lamp (below) | `off`: gives way without the slump | on or off; on needs `--give-way` |
| `--torque on` | take hold of a joint found without torque: its goal written as where it is, then torque on, and refused if it then moves more than 12 counts | a joint without torque is refused, and torque is not written | on or off |
| `--wake on` (default) | with `--home`, a joint found out of where it may start but within 400 counts of home is brought home slowly before anything else; a shoulder left up with its elbow folded is lowered first; in the run, a joint held 30 to 50 counts short of its goal, still, rests where it is (see "Waking") | such a joint is refused | on or off |
| `--let-go-at-stop on\|off` (default off; the factory's, #1682) | asked to stop (SIGINT or SIGTERM), a lamp that got down, its head within 20 counts of home and its arm of its folded counts (inside the tracking bound and the arm's goal step, so an arm left a little short of folded keeps holding), has its last goals written and is then let go, torque off on all five, so that `lamp-body-measure` can read it at rest; a standing lamp folds first, as always, and one that did not get down in time keeps holding (`asked_to_stop_standing_it_folds_first_and_only_then_lets_go_with_let_go_at_stop`, `asked_to_stop_folded_at_rest_it_lets_go_only_with_let_go_at_stop`, `asked_to_stop_with_its_elbow_short_of_folded_it_keeps_holding_with_let_go_at_stop`). A let-go run (with it, no `--home`, so no `--stand`, and no `--torque on`), all it writes being torque off, may take `--folded-arm SHOULDER,ELBOW` alone; then it is not refused for a head found outside its start window (lamp-0c89 rested at pitch 1755, under 1760, held with torque on) when its shoulder and elbow are each found within 20 counts of those, the same bound the let-go at a stop uses: it is let go at once, torque off on all five and no goal written, stays let go until asked to stop, and its report says why in `let_go_outside_window`. Its arm found standing, no `--folded-arm`, or any other run is refused there as before, nothing written (`a_let_go_run_outside_the_window_with_its_arm_folded_writes_torque_off_only`, `a_let_go_run_outside_the_window_standing_or_without_folded_counts_is_refused`, `a_run_that_is_not_only_a_let_go_outside_the_window_is_still_refused`). The folded counts must come from a read the person confirmed was folded, never from this run's own read of where the arm is now. The product's unit never passes it | a stop keeps the lamp held, torque on, until the power goes | on or off |
| `--refused-let-go on\|off` (default on, #2602) | a lamp found folded whose start is refused only because the head or the base is found out of where it may start (the head outside its start window when not waking, a joint farther from home than the start reach, or than `WAKE_REACH` (400) when waking) is let go instead of left as it was: torque off on the three it commands, and on the shoulder and the elbow too with `--stand`, no goal written. Each step while let go (every 100 ms) is watched with what it asks of a person, so the body's diagnostic on the bus is an error with that sentence and state `starting`, sent once per change, not with each read (#2938, the Reviewer-lead's P3 on #2940): every waiting step is passed on, the report goes when what it says changes, and the same again only every 2 s (`WAITING_RESEND`), so that `lamp-health`, which takes a part as stale after 5 s, still has it fresh (`a_body_let_go_reports_once_per_change_and_slowly_again`); the voice says the sentence aloud once, and again when called by name, with `LAMP_BODY_ASK=on` ([speaking-first.md](speaking-first.md)) ("My head is tipped too far. Please move it gently back toward the middle, and I will start by myself."; "twisted" for the roll, "My base is turned too far" for the yaw), the journal says it once, and the first watched step says READY to systemd so the wait is not killed as a slow start. Once pitch, roll and yaw have been inside the start's own bounds, and all five still (3 counts a read at most), for ten reads in a row (a second), it takes hold where they are (goal, then torque, as `--torque on`) and starts as it would have, `--wake` bringing the head home slowly, each wake step watched; a joint that moves as it is taken hold of is let go again and waited for. Only what gravity already holds is let go (#2938): a head found tipped up out of range (its pitch under the range, the side it lifts toward, against its weight) keeps its torque, since let go it would drop onto the base or a hand; the roll, the base and a folded arm are let go, and it says "My head is tipped up too far, and I am holding it so it does not drop. I need someone to help me set it straight." (`refused_with_its_head_tipped_up_it_keeps_holding_the_head`). Ten minutes after it first let go in the run (`WAIT_ASKING`; a failed take-hold does not restart it), nobody having set it straight, it stops asking: it stays let go, reads every 0.5 s instead of 100 ms, and its diagnostic says "Nobody has set me straight, so I have stopped asking. I am let go, and I will start by myself once I am set straight."; set straight then, it still starts (`nobody_setting_it_straight_it_stops_asking_and_reads_less`). **A joint still held while the rest waits is judged by the run's own bounds** (El's P2 on #2940): a head tipped up keeps its pitch held, so that pitch counts steps over its load bound (`load_steps`) and at its temperature bound (`temperature_steps`) as the run does, and over either the wait ends on that stop (`Load` or `Temperature`) rather than pushing or heating on unchecked (`a_head_held_while_it_waits_is_judged_by_the_runs_load_and_heat`). **A shoulder that stops short while lowered on waking** (El's lamp-06c1 case, 22 Oct: lowered from 2,149 for 2,050, settled 2,140, past `Standing::FOUND_SLACK`, so every start ended on `Tracking` and systemd gave up after three) is let go too: the shoulder and the elbow, the head held where it is and judged by the same bounds, asking "My arm is not folded all the way down. Please fold it gently down, and I will start by myself.", and once a hand has folded it (the shoulder within `FOUND_SLACK` of folded, all still, for a second) it takes hold and starts again, the head judged then as any start's (`a_shoulder_that_stops_short_while_lowered_is_let_go_and_waited_for`). Any other stop while lowering ends the run as before. **A start after a wait is watched from its first step** (the Reviewer-lead's P2 on #3168): the wait has sent READY=1, so systemd's 5 s watchdog is armed; a shoulder lowered then is watched each step, and the run's schedule counts from the steps already taken, so its first step is due a tick later, never the wait's steps later (`a_shoulder_lowered_in_a_start_after_a_wait_is_watched_each_step`, `after_a_wait_its_run_is_watched_without_a_gap`; lamp-52e6's `Watchdog timeout` 4.4 s after taking hold). Asked to stop while let go, it ends let go (`Cancelled`). A lamp found up, a home given outside the start window, or any other refusal is refused as before. The exit report's `refused_let_go` holds what it last asked (`refused_for_its_pose_it_lets_go_says_so_and_stays_let_go_while_out_of_range`, `set_back_in_range_it_takes_hold_where_it_is_and_starts_by_itself`, `found_standing_out_of_range_it_is_refused_as_before_and_not_let_go`, `the_ranges_waited_for_are_the_starts_own_bounds`). It does not walk a joint home from farther than `--wake` already does: within 400 counts that is waking's job | refused with torque left as it was: on lamp-e82d a head knocked 1,060 counts from home kept torque on, systemd gave up after three starts, and only a power cut let a hand set it straight | on or off |
| `--overload release` | any joint at a load of 800 (80% of `--force-limit` for a joint held to one) for 1.5 s: torque off on all five for 120 s, then held where they are and the head brought home slowly; a second time within ten minutes ends the run, torque off (see "Letting go under overload") | torque is written off only as a run ends on a surprise | release |
| `--gaze-socket PATH` with `--gaze Y,P` | where the eyes report, and the signed counts of yaw and pitch that a half-picture is worth; the socket without `--gaze`, `--gaze-reach` given or not (a factory `lamp.env` before its gaze is measured, as the product unit passes it) is gaze off, said at start (thirteen-P1) | never looks | 900 either way |
| `--gaze-reach Y,P` | the farthest the gaze takes yaw and pitch; the yaw may be wider than `--reach`'s | 250,120 | 500, 200 |
| `--look-at-name on` (default) | the lamp's name heard on the bus taken as being spoken to for two seconds, and with nobody in view one small glance once it stands (see "Turning to a call") | a call is not taken | on or off |
| `--idle on` (default) | a standing lamp at rest shifts a little now and then | off | on or off |
| `--arbitration on` | a gesture or a move may say who asks (`source`, `priority`), and one asked higher ends the one playing (see "Who may ask over whom") | as before: `source` and `priority` are ignored, and a gesture asked while another plays is dropped | on or off |
| `--caller-caps on` | who asks is the kernel's word: a gesture or a move is held to its unit's most, and the gaze socket takes sights from the eyes, skills or root only (see "Who asks, from the kernel") | as before: any process in group `lampos` may ask at any priority and send a sight | on or off |
| `--lease-socket ABSOLUTE_PATH` | lamp-control may hold the body for up to 30 s at a time, renewed, while the lamp's own life stands back (see "Held by lamp-control") | no lease: nobody can ask the lamp's own life to stand back | a short absolute path |
| `--hold-from ABSOLUTE_FILE` | a test input, for tests on a lamp only: while the file's first word is `held`, the head's pitch is taken as held by a hand (see "Test input: --hold-from" under "Giving way to a hand") | off: only a joint that gave way is a hand. No product unit passes it | an absolute file; `held`, or anything else for no hand |
| `--stand-roll N` | the head's tilt reach once the lamp has stood up, where that is more than the roll of `--reach` | no more than folded | 200 |
| `--pitch-window LOW,HIGH` | the window the head's pitch goal is kept in while the head is beside the base, this lamp's own; checked at start against the head pitch servo's own limits (registers 9 and 11), and the run refused with nothing written if it is not inside them | lamp-4ace's, 1,740 to 2,030, and the servo's limits are not read | LOW at least 1,670 (the head's floor) and under HIGH, and around `--home`'s pitch |

Every gesture is a share of the reach (listening lifts 40% of the way up and
tilts 85% of the roll; thinking lifts 70%, turns 90% of the yaw and rolls 40%
the other way; speaking sits 20% up and nods 15% of the way either
side), so a wider reach makes every gesture larger. The head's pitch window,
the load, tracking, drift, status and heat rules do not change with it.

It takes the same exclusive ownership of the servo port as
`lamp-body-inspect`, so neither can run while the other does. Stop every
other owner of the port first.

**`lamp-body-measure`** (#1682) is the factory's per-lamp step: the same
read-only rest read, made into this lamp's `lamp.env` (`lamp_body::poses`):
`--home` and `--folded-arm` as found, `--stand` as lamp-4ace's with this
lamp's roll and yaw, the default `--pitch-window` checked against the pitch
servo's limits, `--torque on`, and `--gaze` left for its own measurement.
`--home`'s pitch is brought inside the body's home range, where the body
must find the head to start (`Limits::FOLDED.pitch_home`, 1760..2020 as the
code reads it): a rest pitch within `poses::HOME_PITCH_TOLERANCE` (20) counts
outside it is the head resting on its stop and takes the nearest edge, said
in a comment line of the file (lamp-0c89, #2396, rests at 1755 and gets the
low edge). Farther out it is refused, a head off its stop, and the refusal
says to fold the head onto its stop and measure again. The body's own start
check is unchanged: it also reads the live pitch, which lies outside the
range, and lets it through only to wake it home, so the clamp helps a body
that wakes. `lamp-body` wakes unless given `--wake off`, so the product unit,
the factory's stands and the seated template unit all do; neither file says
`--wake` (the product unit and the stands say it themselves, and a second
one is refused). A run with `--wake off` still refuses a head on its stop
under the range. `lamp-body-measure` writes the file only with `--write`, never over one
already there, and refuses a read taken with torque on (install.md, "At the
factory, once per lamp"). A read that stops short says why in its refusal:
the port's preparation first (its lock folder must be the running user's or
root's, mode 0700, with no group- or world-writable folder above it), then
the plan's fault, then the port's cleanup.

With `--seated` (#2163) the same read makes a seated file: `--home` as found,
the default `--pitch-window` (checked the same way) and `--torque on`, and
nothing else. It has no `--stand` and no `--folded-arm`, so a body given it
commands the head and the base only. Without a stand, `--torque on` takes hold
of those three only. The body never writes a goal to the shoulder or the
elbow, and writes torque to them only to let go (off on all five, at a
surprise, an overload or a limp), as V1 does. `--torque on` is needed: the
lamp was read with torque off, and a body without it refuses to start.
The seated file is for the generic package's `lamp-body` unit, which is how a
V1 lamp moves to V2. The product unit passes the stand's own options
(`--stand-share`, `--stand-seconds`, `--awake-seconds`, `--idle`), which need
a `--stand`, so it refuses a seated file. A product lamp is never seated: its
file is the factory's, written after its watched stand. Both are tested from
the unit files themselves. That file is for a lamp moved to V2 whose first stand
no person has confirmed yet. Its standing file comes later, from the
factory's watched stand. A read with torque on is refused here too: the tool
never turns torque off to get a read, since an arm that is up would drop
(Fifth on #2163).

**`lamp-body-measure sweep`** (#3504, #3558's read side) records each
joint's range of motion by hand, as V1's `record_ranges_of_motion()` did:
the same read-only plan, run again and again on one open port
(`linux::inspect_repeatedly`) for `--seconds N` (1 to 600) while a person
moves each joint from one end to the other. It writes no register; every read
must show torque off on all five, or the sweep stops there, refused with
why. Each joint's reads are unwrapped one onto the next
(`poses::ArcTracker`): a jump of more than half a turn between two reads is
the 0/4095 seam crossed, not a move, and a read farther than
`poses::SWEEP_OUTLIER` (512 counts) from the read after it is a single
misread and dropped. So a joint is an arc, the positive way from one end to
the other, not a min and max: hw-first's elbow on lamp-3e83 read 3 to 4,095
as a min and max (#3558), where the arc is its two ends either side of the
seam, said as `3900-300 (across the seam)`; a whole turn or more is "a full
turn". It prints each arc with its span in counts and degrees, and a comment
line, `# Swept by hand: yaw 855-3283, shoulder 2056-3989, elbow 3900-300
(across the seam), roll 1223-3237, pitch 1470-3103.` With `--write FILE` it
appends that line to a `lamp.env` already there that has no sweep line yet,
and changes nothing else in it (never creates the file, never touches
`BODY_POSES`). Nothing reads the line yet: it is evidence for a per-unit
bound on the joints whose servos store 0 to 4,095 (the shoulder and elbow
V1 never calibrated), as #3558 asks
(`a_sweep_across_the_seam_keeps_both_ends`, `a_single_misread_is_dropped`,
`a_whole_turn_or_more_is_a_full_turn`, `jitter_at_rest_is_a_short_arc`,
`the_sweep_line_is_appended_once_to_an_existing_file`). Stop the body and
every other bus owner first.

**The first stand of a moved lamp, confirmed at the lamp** (#2396; the
first builder on #1711, 21 Oct). `lamp-body-measure first-stand`
(`crates/body/src/first_stand.rs`, run by `deploy/units/lamp-first-stand.service`
as root) is how a seated lamp gets its standing file without the factory:

- **The ask.** Once a face at least 0.12 of the picture's height has been in
  the eyes' presence file for 5 s, the lamp says "I'd like to stretch for
  the first time. When you're ready, say yes, or rest your hand on my
  head." through the
  voice's say service (`LAMP_SAY=on` in the generic voice unit). The say
  service says it only after 15 s of quiet. Nothing is asked in the run's
  first 2 minutes, so an update's 45 s check that every part runs never
  meets the body stopped.
- **The yes.** A touch on the head pads or a button, held 1 s, read from
  `controls/state` (`head_active`, `buttons[].pressed`). It counts only if
  begun once the ask's words have ended (a hand on the head while the lamp
  speaks, a pet, is no answer): a hand already on the head must be lifted
  and put back. An ask not said to its end is asked again later, and the
  journal says why (`dropped (user_interrupted)`, `expired`).
- **Or a spoken yes** (the owner, 22 Oct: a customer doesn't know to touch):
  a line on `voice/heard`, its pieces joined and judged whole once it
  finishes, within the same window. Every piece of it must have been heard
  (the bus stamp) at least 1.5 s after the ask's words ended, so the lamp's
  own words and their echo never count, nor a line begun while it spoke; a
  line that is a run of the ask's own words is passed over too, and so is a
  line begun within 1.5 s of anything else the lamp says (`voice/said`) or of
  the voice last seen speaking (`voice/state`), so the tail runs from the end
  of what it plays. The
  ask ends on words that aren't a yes. The line must be short (5 words at
  most), no question, open with yes, yeah, yep, yup, sure, okay, ok, ready or
  go, and hold **only** those words, "please", "now", and the whole phrases
  "I'm ready", "do it", "let's go" and "go ahead"; and be more than "okay" or
  "ok", which are only "I heard you". "Yeah, I'm ready" and "Yes, go ahead"
  are a yes; "Yes?", "Go to sleep", "Okay, bye", "Yeah, nah", "okay hold on",
  a line cut off as "okay let's" and a lone "okay" are not (El on #2893 and
  #3093; `first_stand::is_yes`). A face must be near when it is heard. And, since a
  yes starts the arm (the first builder on #2602, 22 Oct 12:0x), **never in a
  busy room** (`room/state`): an ask made in one says only "Rest your hand on
  my head when you're ready", and takes the touch alone; nor once another
  line was heard in the window (a second voice, or "yes, wait"; an empty
  line is none), nor after
  the window (`first_stand::SpokenYes`). A yes not taken has a journal line
  saying why.
- **A beat before moving.** Once the yes is taken, by touch or by voice, the
  lamp says "Okay, here I go.", waits 1 s, and then starts. The words are read in
  memory and never journaled. It needs the voice on the bus (`LAMP_BUS=on`),
  `voice/heard` and `voice/said` read as the subscriber `first_stand`;
  without them, or with `LAMP_FIRST_STAND_SPOKEN=off` in the unit's
  environment, the ask is the touch-only one and the touch alone is the yes.
- **The stand.** `steps.sh`'s body step:
  1. `lamp-body.service` is stopped and the lamp is read at rest. A lamp
     held with torque on is first let go by a body run.
  2. The arm stands at 30% and then at 100%, by `lamp-body` run by hand as
     the body's user, with `--wake on` and the same options.
  3. The shoulder and elbow of its last full stand are written into
     `/etc/lampOS/body/lamp.env`, never over one already there.
  4. `lamp-body.service` is started again, on that file.
- **A second touch while it moves stops it.** The body is sent SIGTERM,
  eases back and lets go, and nothing is written.
  - Controls that aren't known for 1 s while it moves stop it the same way,
    since a touch could no longer be seen.
  - The yes hand must be off the head and buttons before anything moves
    (up to 10 s, or nothing moves), so any touch while it moves is a stop.
  - The body and its read run as the body's user through `setpriv`, which
    execs them (`setpriv` is util-linux's, on the lamps' Debian base): the SIGTERM
    reaches the body itself, never a `runuser` that
    passes it on and kills the body 2 s later, mid-fold (Jarvis on #2541).
    However its run ends, a body still running is sent SIGTERM and given
    20 s to ease back, fold (about 6 s) and let go; only past that is it
    killed. So none is left holding the servo lock or cut short.
  - Its folder is a new one in the unit's private `/tmp`.
  - `lamp.env` is synced to disk before it is linked into place.
- **No yes, nothing moves.** Without a yes, touched or said, within 2 minutes the lamp
  waits. It asks at most twice, 10 minutes apart. After that a person asks
  again by removing the record and restarting the unit.
- **The record.** `/var/lib/lampos-body/first-stand.json`: `asks`,
  `outcome` (`no_answer`, `stood`, `stopped` or `failed`), `at_unix_ms`,
  and `shoulder` and `elbow` once it stood. The app and the rollback read it
  there.
- **It always ends well.** A failed stand leaves the lamp seated and the
  unit active (`RemainAfterExit`).
- **Not built yet.** The app showing the same line has no path on a moved
  lamp, which has no `lamp-cloud`.

The body does **not** read that file one release forward, on purpose
(#1680's one exception, a safety rule): an option a newer measure wrote that
this build does not read still stops the body, and the journal names it and
says why ("lamp-body: /etc/lampOS/body/lamp.env: --…: an option this build
does not read, written by a newer measure; the body stays still until this
lamp is updated"). A pose option may be a bound the arm needs, as
`--pitch-window` once was; a body that stands still until it is updated is
the safe failure, not one that moves on bounds it knows to be incomplete
(the first builder on #2071, 20 Oct 06:28).

**`lamp-body-mood`** (#1682) tells a body one mood on its `--mood-socket`,
as the voice does: one `{"mood":WORD,"level":LEVEL}` datagram (level 128
unless given). The factory's steps use it to stand a body run by hand on a
private socket. It never opens the servo port.

### Reading a run without stopping it

Two lines let a run be read from the journal while it goes on (#705), since
at rest the step lines come only every thirty seconds:

- **A fold's end**, once a fold reaches folded: how long from the first step
  the arm came down, where the shoulder and the elbow were told to go against
  where they are, the head and base the same, and whether it sat down for a
  strain since it was last folded, on the way up, standing or on the way
  down (`FoldSaid`, `src/bus_out.rs`). A fold that rises again
  before it is down is not said.

  `lamp-body: step 351 folded in 6.0 s; arm goals [2657, 2602] ended at [2650, 2602]; head and base goals [1968, 2069, 2015] at [1950, 2043, 2015]; nothing retreated since it was last folded`
- **The reads not used so far**, at the first step and every five minutes:
  the exit report's `bad_reads` per joint and its count of retreats
  (`ReadsSaid`).

  `lamp-body: step 15010 bad reads so far: base_yaw 0, base_pitch 0, elbow_pitch 0, wrist_roll 3, wrist_pitch 1; retreats so far 0`

Both are on, with no switch.

### Reading its exit report

`lamp-body exit-report` reads a line or a whole journal on stdin, finds the
newest exit report the body printed (`{"port":…,"body":{…}}`), and prints it
as plain lines for the morning report. It never opens the port. Call the
binary the unit itself runs, by its full path: lamp-4ace's unit names a hash
folder (`/opt/lampOS/body/<sha256>/lamp-body`) and has no `current`, the
product unit names `/opt/lampOS/body/current/lamp-body`. It needs a body
built with `exit-report` (#1364): an older one takes `exit-report` as a bad
option and exits 2 with its usage. Read the journal with `sudo` and
`-o short-iso`, so the `at:` line says when the run stopped (with `-o cat`
it is unknown):

```
BODY=$(systemctl show -p ExecStart --value lamp-body | sed -n 's/^{ path=\([^ ;]*\).*/\1/p')
sudo journalctl -u lamp-body --since yesterday -o short-iso | "$BODY" exit-report
```

Exit codes: 0 the newest report was read; 1 there is none on stdin; 3 the
newest could not be read (cut short by journald or a kill mid-write): it says
so with its `at:`, and the previous readable report follows labelled
"previous readable report, not the newest", never in its place; 4 stdin
cannot be read.

lamp-4ace's 15:25:38 exit of 2026-10-17 reads as below (the test fixture,
`lamp_4ace_1525_reads_as_plain_lines`; its `peak_temperature` and
`goal_writes` were not quoted from the lamp and are made up):

```
at: 2026-10-17T15:25:38+0700 lamp-4ace lamp-body[811]:
stop: base_pitch (joint 2): temperature unreadable; last read 55, last good 60
stood: yes, steps 618587
bad_temperatures: base_yaw 817, base_pitch 9354, elbow_pitch 2356, wrist_roll 448, wrist_pitch 323
temperature_flaps: not in this report
temperature_settles: 10: base_pitch 59->55, base_pitch 61->55, …
peak_temperature: wrist_pitch 50, wrist_roll 48, base_yaw 47
arm_peak_temperature: base_pitch 62, elbow_pitch 58
bad_reads: base_yaw 0, base_pitch 0, elbow_pitch 0, wrist_roll 0, wrist_pitch 0
goal_writes: 618001
```

It reads the report by `BodyReport`'s own field names (`src/exit_report.rs`).
Every field but `stop` may be missing and then says "not in this report": a
body built before `bad_reads`/`bad_temperatures` (2026-10-17 08:25),
`temperature_settles` (10:34) or `temperature_flaps` (#1364) still reads
(`a_body_older_than_the_temperature_fields_still_reads`, a real lamp-52e6
exit of 2026-10-14). A field renamed in `BodyReport` fails
`a_joint_flapping_between_60_and_64_stops_on_heat`, which reads a real report
back and requires every line by name.

## On the bus

With `LAMP_BUS=on` in its unit the body publishes on the lamp's bus
([bus.md](bus.md)) under the name `body` (`src/bus_out.rs`). Without it,
or with a bus it cannot use (said once in its log), it runs exactly as
before.

| Topic | Type | When | What |
| --- | --- | --- | --- |
| `body/joint_states` | `lamp_msgs/JointState` | every fourth step: 12.5 a second | Where each joint is, how fast it is turning, how hard its servo is working, and where it was last asked to be |
| `diagnostics` | `lamp_msgs/Diagnostic` | once a second, and once more when it stops; let go for its pose, once per change and the same again every 2 s | How the body is, under the name `body` |
| `body/hand` | `lamp_msgs/Hand` | with every step passed on | Whether a hand pushes it (below, "A hand on the bus") |
| `body/gesture` | `lamp_msgs/BodyGesture` | an event, each time, whatever the step | What became of a gesture: `{"name", "state"}`, `began` when the body begins it, `over` when it ends (played out, let go as it played, replaced by another, whose `began` follows its `over`, or ended by the run stopping: a goal refused, a surprise, asked to), `dropped` when one asked is dropped before it began (paused, a hand, held by a hand or busy under arbitration, another playing or asked after it, slumped, stale, refused whole by the anti-tip rule as `dropped: it would tip`) or one playing is ended for a higher ask (`ended: by WHO`, #1239), which no `over` follows. A gesture still waiting to begin when the run stops is not in the body's last step, so nothing is said of it. Several in one step are each said. Said where the journal says `gesture NAME`, `gesture over` and `gesture NAME dropped: ...` (one `GestureSaid` reads both), so the two never differ, but for the journal saying a repeated drop once in five seconds. Read-only: it asks nothing and changes no motion. A gesture let go as it played (a joint could not keep up) began, so it says `over`, not `dropped`, though its journal line says `dropped: a joint could not keep up with it`. One the publishing thread has no room for is let go and counted apart from the steps (`gestures_let_go`); lamp-body's last line says both counts. Under `--arbitration on` (#940), a `dropped` also says whose it was, why and who holds the body, so the asker that lost is told: `source`, the name its datagram gave (kept only up to 64 printable bytes, inline, so the motor loop never allocates; a longer one is not kept); `why`, the journal's own reason (`dropped: busy, skill`, `ended: by conversation`); and `by`, the holder's name, only when a skill or the conversation that named itself holds the body, never for a hand, a pause or a lamp that doesn't stand. The body carries each name with its priority (`playing_by`, `pending_by`), and who wins, and every goal it writes, is as without them. Of two asks read in the same step, the one dropped as it is read (`kept_of`) is told on `body/gesture` too, with its asker and the one that keeps the body as `by` (the reviewer on #1473: two skills asking at once). Each is left out when not known, and with the switch off the message is byte for byte `{"name", "state"}` |

The joints are `base_yaw`, `shoulder`, `elbow`, `head_roll`, `head_pitch`,
in the order of their servos' numbers, 1 to 5.

- **Angles** are radians from the middle of each servo's turn (count
  2,048), growing the way the counts do: 1,024 counts is a quarter turn.
  They are not angles from the lamp's home pose, and their signs are the
  servos', not a convention of the lamp's.
- **`velocity`** is worked out from two positions 80 ms apart. The first
  message has none.
- **`effort`** is the share of its full strength each servo reports
  (its load register over 1,000), from 0 to 1 and without a direction. It
  is not a torque in newton metres, which is what ROS's field of that
  name holds.
- **`commanded`** is the goal the body last wrote; for a shoulder or an
  elbow it never wrote, where it found them.
- A message means nothing a second after it was sent, so a body that has
  stopped is not read as where the lamp is.

The report is `ok` with no message, or a warning:

| Level | Message | When |
| --- | --- | --- |
| `warn` | `shoulder is warm: it reads 55, and the body stops at 62` | Any of the five joints reads 55 or more. The body ends its run, or sends the arm down, at 62 |
| `warn` | `a joint met resistance and stopped pushing, 2 times in the last minute` | A commanded joint gave way within the last minute |
| `warn` | `stopped, as it was asked to` | Its last word after SIGINT or SIGTERM |
| `error` | `stopped: head_pitch was pushed against (load 260)`, and one such sentence for each other reason in "When it stops" | Its last word when it stopped itself or would not start |

Its facts are the mood, how far it has stood up, the warmest joint and
its reading, and how often a joint gave way in the last minute and since
it started.

**Paused.** Whether or not it publishes, a body that can reach the bus
reads `lamp/pause` at most once a second ([keeper.md](keeper.md), "Named
states and the pause"). Paused, every mood is rest and nothing else asked
of it is taken, and it sees nobody, so a standing lamp folds when its awake
time runs out even with somebody in front of it; its report says `state`
`paused`. Nothing kept on the
topic, or a bus it cannot reach, is not paused.

**Asleep** (#1362). With `LAMP_HEAD_TOUCH=on`, a body that can reach the
bus also reads `voice/mute` every quarter of a second, and at once when the
voice speaks to it while it is taken to be asleep. While the voice says a
hand held on the head put the lamp to sleep (its mute with the reason
`sleep`, [alive.md](alive.md)), every mood is rest, it sees nobody, it is
held still and a gesture asked is dropped (`dropped: asleep`). A standing
lamp folds now, over `--stand-seconds` (6 on the product), even with
somebody in front of it and whatever its awake time, once a gesture from
outside already playing has ended: the slow fold is the sleep a person
sees, as Expression's sleeping pose alone moves the head 0.023 rad. The
head still takes Expression's sleeping breath. Woken by a tap, the voice
says it is open again, and the body stands up at once, the lift, before
any mood of the voice's; it then stays up for its awake time as after a
mood. Only the voice's word is taken (the topic's publisher `voice`): the
same from another part, a mute for another reason, a value older than its
three seconds, nothing kept or a bus it cannot reach is not asleep, and is no wake
either: a folded lamp stays down until a mood or somebody in front of it
stands it up. Without the switch nothing is read.

The loop that talks to the servos never waits for the bus. It hands
every fourth step to a thread of its own, which has room for sixteen; a
step that finds no room is let go and counted, and the count is written
to the log when the body ends.

Not shown: this has run on a development Mac against a bus in a
temporary directory (six tests, one of which offers 40,000 steps to a
full thread and holds each to under a millisecond). The program that
owns the servos is built for Linux only and was not run there with the
bus. Which way each angle's sign runs on the lamp has not been looked at.

## First run on a lamp

lamp-4ace, 2026-10-12 15:18 UTC, the build of `6dfc18bb8`, started by hand
while a played conversation ran. It found pitch 2,000, roll 2,043, yaw 2,015,
wrote the four profile registers the roll and yaw servos lacked, and moved for
66 steps: 59 goal writes, pitch to 1,973, yaw to 2,033, no missed read, peak
loads 104, 96 and 88. Then it stopped itself on `temperature`, joint 1: the
yaw servo's temperature register read 72 on one step. It had read 45 at start
and read 44, 44 and 45 on three reads at rest within the next two minutes, so
that was one reading taken while the motor drew current, not heat. The goals
were held at 1,973, 2,046 and 2,033, where the joints were.

Two changes followed. A temperature now ends the run only when it is read on
every step for a second, and the highest reading is reported either way. And
the home can be given: a stop that holds the head where it is no longer moves
the pose the next start works around. The load bounds were 160, 110 and 110
with two steps; they are 170, 150 and 150 with three, above what this run
read.

## Second and later runs on the lamp

Same evening, with `--home 1950,2043,2015`.

- With `--reach 100,15,50,120 --speed 120 --servo-speed 300
  --servo-acceleration 60` it stopped itself 14 s after start on `load`,
  joint 4: asked to tilt, the roll servo stayed at 2,047 (four counts from
  where it was found) while its load rose to 156. In its first run it had also
  stayed within three counts, at a load of 96. In this folded pose the head
  does not roll that way. Roll is switched off there since (`--reach
  100,15,1,120`); which way it is blocked, and by what, has not been looked
  at.
- With roll off, across four played exchanges (two lines it ignored, two it
  answered) the body's own log shows: listening, pitch 1,950 to 1,911 in
  about a second; thinking, pitch to 1,878 and yaw from 2,023 to 2,108 one
  time and to 1,920 the next; speaking, yaw back and pitch moving between
  1,919 and 1,947 with the voice; then home. No stop, no missed read, loads at
  or under 84, and a commanded joint never more than 18 counts behind its goal
  while moving. The yaw servo comes to rest 8 to 15 counts short of its goal
  and holds there at a load of 44 to 72; roll rests 4 counts off at 28.
- Photographs from the office iMac, about a metre away, show the head turned
  during thinking and facing the camera during listening and speaking.

Since then a joint that has rested for a second within 24 counts of its goal
is given its own position as the goal, so that nothing pushes at rest.

It then ran for about 1 h 40 min of played exchanges without a stop, until
it was stopped by hand, and for 11 minutes after it was started again. Then
it stopped itself on `load`, joint 5: the head, coming down from its thinking pose as an answer
began and nodding on the first syllable, read 217 on the third of three
steps over 170. It was within 6 counts of where it had been and 19 from its
goal, the other joints read 28 and 56, and nothing was in its way. Peak
load on the yaw joint in that run was 104. From this: the nod is 15% of the
reach where it was 25% and follows the voice a third more slowly, the
commanded pose changes speed at six times the top speed a second where it
was eight, and the load rule wants six steps in a row, not three.

Three minutes into the next run it stopped the same way, at 190, the head
9 counts short of its thinking pose with the base turned to one side; it
had reached that pose at a load of 74 twenty minutes before. A photograph
shows nothing against the head. What resists it there is not known. Since
then a joint that meets resistance gives way and goes on (see "When it
stops").

That showed what the resistance was. In the next run, 18 minutes in, the
head gave way three times in 35 s and the run ended on the fourth: each
time it sat at 1,936 or 1,937 with its goal 3 to 8 counts above, the goal
of a small nod, and did not move while its load climbed from 48 past 170
in a second and a half. The nods had been made smaller the same night and
had gone under what the joint can move. Since then a joint that has
arrived rests until it is asked for a move large enough to make. Twelve
counts was not enough: over the next ten minutes the head gave way four
more times, each time at rest at 1,937 and asked to lift 11 or 12 counts,
which it did not begin to do while its load passed 170. It lifts 40 from
rest without reading over 90. The bound is 20 counts now.

## The force its servos are allowed (lamp-4ace, 2026-10-14)

The old runtime capped the force of the three joints that carry the lamp
(the base's turn, the shoulder and the elbow) at 600 of 1,000 while it
ran, so that the arm gives way sooner against a hand, and measured what
that cost in lag ([v1-verdicts.md](v1-verdicts.md), HAL-4.5). This body
writes no such cap, and what the servos hold had never been read.

It was read on 2026-10-14 at 07:48, with the body stopped for the few
seconds it took and nothing written: one read instruction for each
register and servo, from a script on the lamp, each reply checked by its
checksum. The names are the servo's published register table's, not
checked against this firmware.

| Register | Base turn | Shoulder | Elbow | Head roll | Head pitch |
|---|---|---|---|---|---|
| 16, the force limit it starts with | 1,000 | 1,000 | 1,000 | 1,000 | 1,000 |
| 48, the force limit now | 1,000 | 1,000 | 1,000 | 1,000 | 1,000 |
| 40, torque on | 1 | 1 | 1 | 1 | 1 |
| 36, the load at which it protects itself, in hundredths | 80 | 80 | 80 | 80 | 80 |
| 35, for how long, in hundredths of a second | 200 | 200 | 200 | 200 | 200 |
| 34, the force it falls back to then, in hundredths | 20 | 20 | 20 | 20 | 20 |
| 28, the current at which it protects itself | 310 | 310 | 310 | 310 | 310 |

So every joint may push with all it has. What makes the arm give way
today is this program alone (a load over its bound for six steps, a
joint off its goal for half a second, warmth: "Standing up" above), and
behind it the servo's own protection, which lets go only after two
seconds over 80%.

What the lamp's own movements use: the run of the body that ended with
that reading had every stand-up and gesture of a night's played tests in
it (204,330 steps, no retreat). Its report gives the highest load each
joint read: 428 at the shoulder and 490 at the elbow, and 229, 60 and
156 at the three joints of the head and base. A cap of 600 is above all
of them, so on this unit it would cost ordinary movement nothing, and it
would take four tenths off what the arm can push with against a hand or
an edge.

The cap is `--force-limit N`, off unless given. At start, after the
servos' speed and before anything moves, the body writes it (register
48, two bytes) to the base's turn, and to the shoulder and the elbow if
the lamp may stand, and reads it back from each; a joint that reads back
anything else ends the run there, with nothing moved. The servo keeps it
until it loses power and then starts again with its own. The load bound
comes down with it: a joint capped at 600 never reads 950, so a bound
left there would be a guard switched off. Unless `--arm-load` says
otherwise the bound is 40 under the limit (560 for 600, over the 490 this
lamp's own movements reached), and an `--arm-load` at or over the limit
is refused. The report has `force_writes` and `force`.

Two tests against the simulated servos: the write byte for byte, to
which joints, before the first goal, and nothing written without the
option; and a servo that does not keep the limit, which ends the run
before anything moves. One for the option.

**On lamp-4ace since 08:01 on 2026-10-14, at 600.** One scene, played to
the body with no sound, once without the cap and once with it: the body
was told on its own socket what the voice would tell it (listening for
eight seconds, so that it stands; thinking; then speaking, with a nod, a
shake of the head, a stretch and a laugh asked for), while its joints
were recorded from the bus at 12.5 a second and the lamp was photographed
every five seconds.

| | Without the cap | With it at 600 |
|---|---|---|
| Highest load read: base turn, shoulder, elbow | 192, 368, 481 | 192, 316, 438 |
| Farthest from its goal: base turn, shoulder, elbow, in counts | 76, 95, 68 | 72, 81, 58 |
| The shoulder's way up, 609 counts | 9.2 s | 9.3 s |
| Gestures begun, and let go | | 3, 0 |
| Sent back down, or met resistance | | never |

The body's own report of the run with the cap, which reads every step
and not twelve times a second: 3 writes of the limit, 600 read back, the
elbow's highest load 508 and the shoulder's 340, nothing dropped. Read
afterwards with the body stopped and nothing written: the base's turn,
the shoulder and the elbow hold 600 and will start again with 1,000; the
head's two joints hold 1,000. In the photographs it stands upright and
moves as it did.

So on this unit the cap costs its own movements nothing that one scene
can show, and the elbow's 508 is 52 under the bound that sends the arm
back down.

**Not shown:** that the arm gives way against a hand, which is the point
of it and needs a hand; a cold lamp, a warm one, or another unit, where
the same movements may ask for more; and anything about leaning: this
body still has no check that a pose keeps the lamp on its base. One
oddity in the same report: the highest warmth read was 150 for
two of the three head and base joints, which no servo here has been; a
single bad reading counted as a peak is the likely cause, and the guard
that acts on warmth needs a whole second of it.

## Giving way to a hand

Not yet tried by a hand on any lamp. Everything in this section is what
the code does against simulated servos.

V1's lessons, kept by the fifth builder's person: a short bump should
give way and come back gently; a person who holds or adjusts the head for
longer than about a second wants it to stay where they put it, and on V1
a hand held for that second made the joint go limp; the joints that carry
the lamp's weight (the shoulder, the elbow) must never go limp, since on
V1 releasing them dropped the arm with nobody touching it; and a joint
held still against its goal must not cook.

`--give-way return|stay|off`, `return` unless given (since #703: shown by
hand on lamp-52e6 on 2026-10-15, see
`docs/verification/give-way-hand-lamp-52e6/`). With `--give-way off` the body
does exactly what it did before the option, write for write.

**Both modes: a joint that pushes without moving is told to stop.** Any
of the five joints the body writes (the three it commands; the shoulder
and the elbow of a lamp given `--stand`) whose goal is more than 6 counts
from where it is, whose load is over 100 and which moves no more than 6
counts for 1.5 s, is given its own position as its goal (not one that gave
way and may be in a hand: held where a hand left it, it would never come
back). A folded arm at rest (its goal kept where it came to rest) has
nothing to push at but a hand: it is held where the hand has it for two
seconds, as a joint that gave way is, and then its goal goes back to where
it rested, the hand given way to again if still there. On lamp-52e6 on
2026-10-15 a hand lifted the folded elbow 103 counts; held there for good,
the next start found it 99 counts off its way and the lamp would not
stand. Otherwise it is held there
until what is wanted of it moves 20 counts or more from what was wanted
when it stalled (a new mood, the arm rising or folding), and then follows
again. Torque is never written off for that. A joint that stops again within
6 counts of where it last stopped is held again (it still stops pushing,
which `--overload release` counts on) but not counted in `stalls` and not
logged; a joint that has moved freely away, unloaded, for a second forgets
that place, so a later stall there is counted. On lamp-4ace on 2026-10-15 a standing shoulder
resting short of its goal under its own weight was counted and logged twice
a minute with nobody touching the lamp (67 lines in 28 minutes), each time
a scene moved its goal a little and it stopped at the same place. A hand that
takes a joint elsewhere and stops it there is counted as before. Why these
numbers:

- 1.5 s is longer than the rules already there take to stop a joint
  pushing at rest (arrived for 0.3 s, at rest for 1 s), so it catches only
  what they leave pushing, and shorter than the servos' own protection
  (80% load for 2 s, then 20% of their force: on the shoulder or the elbow
  that is the arm dropping), so this acts first. Heat comes from minutes
  of pushing, not seconds: the elbow on lamp-4ace reached 69 °C one day
  with the head resting on the base.
- 100 of 1,000 is over what a joint at rest at its goal reads on
  lamp-4ace (the head 68, the base resting short 44 to 72) and under the
  100 to 150 that lamp-52e6's shoulder pushes with for as long as it
  stands, 22 to 34 counts short of its goal.
- A standing shoulder or elbow carries the arm, so for those two the
  bound is the load each read once the lamp stood up (`stand` settled)
  plus 100, never under 100; rising, folding or folded it is 100 again.
  On lamp-4ace from 2026-10-15 to 16 the shoulder stood at a load of 112
  (elbow 23) and its own weight made 426 shoulder `stalls` in 3 h 50 min
  at rest. 100 over is twice the 50 lamp-52e6's standing shoulder wanders
  (100 to 150) and far under a hand stopping the arm, which reads hundreds
  (the arm's own bound, `--arm-load`, is 950). The head keeps 100.
  That load is learned only while it reads 200 (100 + 100) or less, over
  what a free-standing arm carries (lamp-52e6 100 to 150, lamp-4ace 112):
  an arm already held by a hand or an obstacle when `stand` settles reads
  more, and learning that would lift its bound out of reach of the same
  push. Until it stands under 200 its bound stays 100.

The report counts each joint's (`stalls`: head pitch, head roll, base yaw,
shoulder, elbow) and the log says each one with the joints' positions and
loads.

**Both modes: a hand never ends the run.** For the three commanded
joints. A joint held more than its tracking (30 counts) off its goal for
120 ms while its load is over 100 gives way as one over its load bound
does: lamp-52e6's base turned 38 counts by a hand on 2026-10-15 read 120,
under the 220 that bound is. And while a joint that gave way is held, its
load and tracking are judged over a second instead of 120 ms and half a
second, since that is what a hand moving or holding it looks like: still
in the hand a second on, it gives way again where it is. Each step that
still reads the hand (its load over the bound, or the joint moved off where
it was put) keeps it held at least a second more, so one quiet read in
between is not the hand gone: on lamp-52e6 on 2026-10-19 a base held gently
read 172 to 196 with quieter reads between, was sent home into the hand
every two seconds, and the person felt it stiffen and jerk (#1115). The
bound is the lower of the joint's own and 100 only once the joint is more
than 15 counts from where its hold began (or where a hand left it, under
`stay`); still within that, only its own bound counts, so a joint resting
still at its own load over 100 is let go after its ease and goes back to
its moods and home, as any eased joint does, unless it went over the full
bound in this hold and still presses over its own rest load (by its load
alone, not by how far it stands from where it first gave): a still press at
300 easing to 130 on the head's 240, from a rest of 25, stays a hand and
its hold renews against the lower bound. On lamp-06c1 on 23 Oct the
head pitch read 124 to 136 at rest, its position unmoving all night, and
judged against 100 it was held for good, out of moods and home. On 22
Oct it gave way because its goal wandered 20 to 25 counts off it while it
never moved: its hold begins where it stood, not at that goal, unless it
moved from where it last tracked, as a hand moves it. A hand
that moves the joint, or presses past its own bound, still keeps it held.
On lamp-52e6 the
same morning, before this, a hand tilting the head took it 45 counts past
where it had given way and the run ended on tracking, and the third such
end in ten minutes left the lamp without a body. Under `--give-way` the
fourth give in a minute does not end the run either: a person may move
it as often as they like.

**`return`: a joint that gave way comes back,** as without the option
("When it stops"), once the hand has let go.

**`stay`: moved by a hand, it stays where it was put.** As above, and:

- In a hand for a second in a row (its load over its bound, or over 100
  if that is lower and it is off where its hold began, or it kept more than 15 counts off the goal it is held
  at), and it has been moved more than 15 counts from where it gave way
  (from its goal, for a joint that gave way because a hand had already
  moved it off it): a hand moved it. On lamp-52e6 a hand lifting the head
  6 degrees read 100 to 130, under the head's 240, and gave way 30 counts
  in; judged against 240 and from there, the head came back instead of
  staying. Judged against 100 it came back again (run 4): held 30 to 40
  counts off its goal, the light head on its soft servo read only 90 to
  130, around the bound and never over it for a second. A joint resting
  at its goal is never taken for a hand. Where it
  is becomes where it is left, inside the same reach and window as every
  goal (a hand that takes it past the reach leaves it at the edge, where
  the servo takes it at its own speed), and what the lamp is doing then
  is laid from there: later moods are relative to it. It is held two
  seconds more, so a hand still on it can go on moving it. The report
  counts each (`kept_where_put`) and the log says it.
- The same second and it has not moved: something holds it; it gives
  way again where it is, as with `return`.
- A bump that does not move it, or a hand that moves it for under a
  second, gives way and comes back as with `return`. A hand that lets go
  before the second sees the joint go back at the servo's own speed to
  where it gave way, and then to the pose its mood wants.

**`--limp on` (with `--give-way`; on unless given while it gives way, since #703, and off with `--give-way off`): a hand makes the
lamp slump, as V1 did.** The hardware team's requirement: the lamp must
slump rather than let a servo be damaged. Under a hand that would strain a
servo it lets go, as the old runtime did, and never holds on. Until 2026-10-14 a joint that gave way only had its
goal set to where it was, its torque left on. On lamp-52e6 that day, run
with `--give-way stay --overload release`, a person could not push the
head at all and feared breaking it: the journal shows the roll moved from
2,019 to 1,958 at a load of 256 and the yaw from 2,035 to 1,986 at 188,
each giving way by its goal while the servo held stiff. On V1 a hard hand
made the lamp go limp at once.

V1 (`robots/lamp/servo_overload.json`, `hal/board/servo_overload.py` and
`hal/drivers/motors/animation_service.py` on `main`) stopped a joint
whose load read over its contact floor (base yaw 55%, head roll 75%, head
pitch 65%) for 0.05 s, pinning it where it was; if it was still pushed
for `release_after_s` (1.0 s), by a load over the floor that stopped it
or held more than `CONTACT_RELEASE_LAG` (1.0 degree) off where it was
pinned, the overload cut-off took over and torque went off on every
servo, so the lamp slumped, and was taken again 120 s later
(`retry_s`). The shoulder and the elbow had no fixed contact floor there
(their off-playback floors were load 57% and a lag of 14 and 12
degrees), but the cut itself dropped them with the rest. V1's overload
rule alone (80% for 1.5 s) is `--overload release` here, and under a
600 force limit it does not trip on the base: what made V1 go limp under
a hand was this contact path. This body does the same, and its slump is
the one `--overload release` makes ("Letting go under overload"):

- **Stopping.** Head pitch over 650, head roll over 750 or base yaw over
  550 of 1,000 (with `--force-limit`, the yaw's floor is 55% of that
  limit) for 3 steps (60 ms), or over the joint's own bound for six steps
  as before, whichever comes first, and the joint stops where it is: its
  goal becomes its position, as above ("When it stops").
- **Letting go.** Only a joint stopped by its contact floor, as only
  V1's contact stop handed over to the cut-off; one that gave way by
  `--give-way`'s lower bounds gives way and comes back (or stays) as
  above, never slumps the lamp. On lamp-52e6 on 2026-10-15 a head lifting
  itself after a slump, 37 counts short of its goal at a load of 144,
  gave way by the lower bound, read as a degree off where it gave, and
  made the lamp slump again every two minutes with nobody touching it.
  Still pushed in the second after it stopped (50 steps in a row): its load over the lower of the bound that stopped it and its
  floor, or more than 11 counts (about a degree) from where it stopped.
  Then the lamp lets go as under overload: torque off on all five, the
  lamp slumps, nothing is written for 5 s, then every joint is held
  where it lies, a shoulder left up over a fallen elbow is lowered, and
  the head and the base are brought home slowly, as after an overload. V1
  rested 120 s here too, its `retry_s`, which is for a servo cooling
  from overload; a hand is not heat, and a lamp limp for two minutes
  after a push reads as broken, where a creature pushed hard flops and
  gathers itself within seconds (the first builder on #780). 5 s is a
  little over V1's contact pause (`pause_s`, 3 s). The servos do not
  need longer: through run 8 on lamp-52e6 (2026-10-15 14:14 to 14:21,
  the slump included) the shoulder and the elbow read 44 to 48 °C, far
  under the 62 at which the body stops, and a hand on the base for a
  second or two warms nothing. A slump from overload (`--overload
  release`) still rests 120 s. The
  report counts it in `released` and, per joint, in `released_by_hand`
  (pitch, roll, yaw); the log says so. A first version (2026-10-15 13:41
  on lamp-52e6) let go of the base alone: the lamp did not slump, as the
  person noticed, unlike V1.
- **A hand never ends the run.** Pushed again once it is up, or while it
  is brought up (a load or a tracking that would end the wake), it lets
  go again, as often as it is pushed: the rule that ends the run on a
  second trip within ten minutes is `--overload release`'s alone. On
  lamp-52e6 the first version's run ended on the base's load (236) with a
  hand still on it after it was taken again.
- **Found far off at start.** A head roll or a base further from home
  than waking brings back (400), as a hand may leave it after a slump, is
  taken hold of where it lies and the lamp runs from there, its home moved
  by as much, as V1 took hold where the joints lay; never the head's
  pitch, which carries the head. Without `--limp on` it is refused as
  before (on lamp-52e6 the base 545 counts off was refused at every
  restart, and the lamp had no body).
- **A shorter push** (a bump, or a hand that lets go within the second)
  gives way and comes back as before, its torque never touched: with the
  torque on, the servo pulls the joint back to where it stopped, and it
  is then asked for what its mood wants.
- **Asked to stop** while slumped, the body leaves the lamp as it lies,
  torque off, as under overload.

`--overload release` (V1's overload rule, below) is unchanged. There is no chime: the body has no speaker.

Five tests against the simulated servos and one for the option: a short
push gives way and comes back, and so does a head moved for half a
second; a head moved 30 counts by a hand for 1.5 s stays there while the
lamp rests and the next mood lifts it 40 counts from there, no goal
jumping; a head pushed 150 counts up with a reach of 100 is kept at 100;
a head stopping 27 counts short of its goal at a load of 130 is told to
stop 75 to 78 steps after it stopped moving and is not pushed at again
while the mood is the same, and without the option goes on pushing; a
standing shoulder 30 counts short at 150 is held where it is, and no
torque write is ever anything but on; told to come back, a head tilted 60
counts by a hand at a load of 240 and held for three seconds, and a base
turned 40 counts at 120, give way, the run goes on, and each is back
where it was once let go, while without the option the same hand ends the
run; a folded elbow lifted 200 counts by a hand for three seconds is
given way to and is back where it rested once let go, and without the
option it has no stall rule and its goal never leaves where it rested;
a light head lifted 40 counts at 120 and held a second and a half stays
at 1,920 when told to stay, and is back at 1,960 when told to come back.

Four tests for `--limp on`: a base turned 30 counts by a hand at 600 and
held makes the lamp slump about a second after it stopped (torque off to all
five, byte for byte), nothing is written for 5 s (and it is taken hold
of within a second after), and each joint is then held where it lies; pushed again once up, or while it is brought
up, it slumps again and the run goes on; without `--give-way` and
`--limp` the run ends on its load with no torque ever written. A base
found 545 counts from home starts held where it lies with `--limp on`
and is refused without it. A base pushed for 0.7 s gives way and comes
back, its torque never written; 700 on the head's pitch for 80 ms gives
way only with `--limp on`; an elbow pushed at 900 for 2 s is never
let go; and after a slump a head that sank 60 counts and lifts itself
home 37 counts short at 144 does not make the lamp slump again.

**Not shown:** that any of it is right against a hand. Whether a real
hand reads over the load bounds and the floors while it moves a joint
(V1's floors were set on lamp-a0ae and lamp-52e6 with its own loop),
how the lamp looks to the person while it slumps and is taken again, whether 1.5 s and 100
suit lamp-4ace's or lamp-52e6's servos, and what the temperature of a
joint held for minutes does with and without it. The measurements task 24
asks for (by hand, standing and folded, through a kitchen scale) have not
been made.

### Test input: --hold-from (#2668)

`--hold-from ABSOLUTE_FILE` is for tests on a lamp only, so a row a hand
on the head has shown once can run again with no person there. No product
unit passes it, and a test checks every file under `deploy/`
(`nothing_deployed_passes_hold_from`). The file's first word is read at
most every 100 ms (`hold_in::LOOK_EVERY`, a small read, never waited on):
`held` is the head's pitch held by a hand; `free`, any other word, an
empty file or none is no hand, and a file removed is a hand let go.

While it says `held`, the body takes the head as held exactly where a
joint that gave way would count (`in_hand_now`): a skill's or idle
gesture playing ends `ended: by a hand`, eased out, one waiting or asked
is dropped as held (`dropped: held by a hand`, or `dropped: a hand holds
it` with `--arbitration` off), no shift at rest starts, and the head's
pitch keeps the goal it has, asked nothing, as a hand's hold pins it.
Nothing of the giving way itself runs: no torque is written, no servo's
load or tracking is judged differently, nothing is counted as given way
or released by a hand, and the other joints follow the mood as ever. When
it says anything else the hand is gone on the next step, and the head
moves on from where it was held, no jump. It holds through a pause or a
hand's sleep, as a real hand would.

The journal says at start `lamp-body: hold from a test input: FILE
(--hold-from, a test flag no product unit passes)`, then `step N hold from
a test input: the head's pitch held by a hand` and `... the hand lets go`
as it changes, and each `ended`/`dropped` line about a hand while it holds
ends `(the hand from a test input, --hold-from)`, so a lamp's result can
name it. `body/gesture` says the same as for a real hand, and `body/hand`
says `pushed` while it holds (#2904), so what a push ends, a skill's
search and the voice's, can be run from it. Tests:
`a_hand_from_the_test_input_is_said_as_a_push`,
`a_skill_gesture_playing_ends_by_a_hand_from_the_test_input_eased_out`,
`a_skill_gesture_still_waiting_is_dropped_by_a_hand_from_the_test_input`,
and `hold_in`'s own for the file.

## Letting go under overload

Not yet tried on any lamp, by a hand or otherwise. Everything in this
section is what the code does against simulated servos.

`--overload release`, off unless given. Without it the body does exactly
what it did before, write for write, and writes torque off to all five
only as a run ends on a surprise ("When it stops"; with `--limp on` a hand makes the same slump: "Giving way to a
hand").

The hardware team's view, given through the owner's builder: the old
runtime's answer to a servo pushed hard is the best protection these
servos have. That runtime (`hal/drivers/motors/overload.py`,
`robots/lamp/servo_overload.json` on `main`) cut torque when a joint read
at least 800 of 1,000 for 1.5 s, kept it off for 120 s and then tried
again. V1 measured what it guards against: free motion peaks at 87% for
under 0.3 s; a hand holding a joint reads 94 to 100%
([v1-verdicts.md](v1-verdicts.md), HAL-4.4). This body does the same:

- **When.** Any of the five joints reads a load of at least 800 on 75
  steps in a row (1.5 s). A step whose read or write failed breaks the
  row, as in V1. A joint held to a force limit (`--force-limit`) cannot
  read much more than that limit, so 800 would never trip on it; V1 says
  so of itself ("a force-limited joint can never read the 800 that trips
  the cut-off"). For those joints (the base's turn, and the shoulder and
  the elbow of a lamp that may stand) the level is 80% of the limit, the
  same share of what the joint may push with that 800 is of a joint
  without one: 480 at lamp-4ace's 600. lamp-4ace's own movements reached
  508 at the elbow for moments in a scene with the cap; none held 480 for
  1.5 s, but nothing longer than one scene has been read.
- **Letting go.** Nothing more is asked of the joints. Torque off
  (register 40, 0) is written to all five, each acknowledged; one that
  does not acknowledge is tried again on the next step. The lamp slumps.
  The report lists each time (`released`: the joint, the load that
  tripped it, the seconds since the body started) and the journal has a
  line when it lets go and one when it takes hold again.
- **Resting, 120 s.** It reads all five every step, so the report and the
  bus go on showing positions, status and warmth, and tells systemd it is
  alive. Moods, gestures and what the eyes see are taken and dropped. No
  goal is written: a goal written to one of these servos turns its torque
  back on (lamp-52e6). Asked to stop (SIGINT, SIGTERM), it ends there with
  torque left off; so does a wire that fails for half a second.
- **Taking the lamp up again.** Every joint is taken hold of exactly as
  `--torque on` does at start: its goal written as where it is, then
  torque on, each acknowledged; one that then moves more than 12 counts
  ends the run. What the servos read now is what later reads are judged
  against, since torque off and on may change a flag. A commanded joint
  left with a flag ends the run. Then the head, its roll and the base are
  brought home along the `--wake` path, whether or not `--wake` was given,
  exactly as after a power cut ("Waking"): a stir, a pause, then the rest
  at 20 counts a second at most, never away from home; a head that drooped
  and must lift has its goal led up to 80 counts ahead of it, may lag up
  to 100 on the way, and one that stops within 50 counts of home is woken
  where it settles, the run then starting from there with its home moved
  by as much (`home` in the report). The wake's record is in `released`
  (`woke`), not in the start's `woke`. Before that, a shoulder left up
  over an elbow that fell folded (how a standing lamp slumps: on
  lamp-52e6 on 2026-10-16 the elbow fell from 2,595 to 2,080 in the rest
  and the shoulder stayed at 2,650) is lowered onto its folded count
  exactly as after a power cut ("Waking"), whether or not `--wake` was
  given, the head, the base and the elbow held where they lie; `lowered` in the
  report is then that one. The lamp is then folded: it counts as folded
  (`standing 0.00` on the bus, no `stand 1.00` in the journal while it is
  let go or taken up), its head is brought to its folded home, and the
  next mood other than rest, or a person in front of it, stands it by the
  ordinary way up with every check that way has. Without this, that run
  on lamp-52e6 refused to stand for good and sat 15 minutes with its
  shoulder up and its elbow down, its head pushing at a home that does
  not fit that arm, until a restart. Every step of this lowering (some
  22 s) is a step of the run and is watched, so systemd hears from the
  body each step (`WatchdogSec=5`); at start the same lowering comes
  before READY=1, when the watchdog is not yet armed. The read of the
  arm's torque before it, if it fails, is tried again for half a
  second, as any read of the run. A lowering that fails holds every joint where it is
  and ends the run, and the whole lamp is then let go (torque off), as at
  start. A stop asked during this lowering (SIGTERM, some 22 s) does neither:
  it holds every joint where it is with torque on, the shoulder part way
  down over a folded elbow, and the next start lowers it the rest of the
  way (`--wake`, "Waking"). The held joints' loads during it are kept with the
  run's peaks (`peak_load`, and the elbow's in `arm_peak_load`), so a run
  shows how near they came to their bounds. After a hand's slump (`--limp on`), a hand that stops it (a load or a tracking), presses the held head, base or elbow over its load bound for as long as the run allows (a load), or moves one more than its tracking bound for as many reads in a row as the run allows (a drift; a single read off is not a hand and is never held to), makes the lamp let
  go again instead (after an overload the same stop ends the run), and the step that does so writes no goal first: the
  arm is never sent toward the pose it stood in before the slump. Otherwise the arm is not lifted by that: it stays where
  it slumped until a mood or a person in front of the lamp next asks it
  to stand, and then rises by the ordinary way up from where it is, if
  that is on the way between folded and stood up; if not, the lamp runs
  folded and says why (`standing_refused`), as at start.
- **A second time within ten minutes** of the first, it lets go as before
  and the run ends there, torque off (`overload` in the report): a joint
  that keeps fighting should be looked at, not tried again.

**With `--give-way`.** Both rules time 1.5 s, and the order is this. A
commanded joint over its load bound (150 to 240) for 120 ms gives way long
before either (V1's contact floor of 550 to 750 for 60 ms, too); with
`--limp on` one still pushed a second later makes the lamp slump, as
this rule would. The stall rule
counts from a load of 100 while the joint does not move, and a load
climbing to 800 passes 100 first, so the stall rule normally acts first
and the joint stops pushing. A joint told to stop on the very step it
would trip starts its count again, and trips only if it stays loaded for
another 1.5 s: something is pushing it. A standing arm over its own load
bound for 120 ms is sent back down first; at 1.5 s, before the 2 s that
would end the run, it lets go. So on the head and the base this rule is
the last of the guards, and on the shoulder and the elbow, which carry
the lamp, it is the one that acts on a sustained push.

Three tests against the simulated servos: an elbow at 800 for 1.6 s lets
go of all five, the torque-off packets byte for byte; nothing is written
for 120 s although the lamp is spoken to; each joint is then given its
goal where it slumped and then its torque; a head that drooped 250 counts,
and like lamp-52e6's stops 32 short of a goal it lifts to, is lifted a
count a write at most, never away from home, and woken where it settles,
its home moved 32; the arm stays where it slumped; and the same load
again some 7 s after it woke ends the run with torque off. 800 for 1.4 s
does nothing, and the writes are those of a run without the option. A
standing lamp's elbow at 500 under a force limit of 600 lets go, and ends
with torque off when asked to stop while it rests; without the limit 500
does nothing. One for the option.

**Not shown:** that any of it is right on a lamp. No hand has tried it;
whether a slumping lamp falls somewhere it should not (on V1 an arm let
go dropped with nobody touching it, which is why `--give-way` never
lets go of the shoulder or the elbow), how far the head drops onto the base and whether the
wake then brings it back as tested (after a power cut on lamp-52e6 the
head drooped some 250 counts, and a standing lamp's shoulder stayed up
at 2,633 while the elbow fell folded: an arm left like that is off its
way, so a standing lamp that lets go would most likely run folded
afterwards), and what heat does: whether 1.5 s at
80% warms a joint at all, and whether two minutes off cools one. The
level of 80% of a force limit is reasoned, not measured.

## lamp-52e6

The fifth builder's lamp, from 2026-10-14. The old runtime was stopped and
disabled there the same morning; the units it runs are in
[`deploy/lamp-52e6/`](../deploy/lamp-52e6/README.md). How it differs from
lamp-4ace, as read on it with the old runtime stopped:

| | lamp-4ace | lamp-52e6 | Where it was read |
|---|---|---|---|
| Joints found at rest: yaw, shoulder, elbow, roll, pitch | 2023, 2052, 2047, 2053, 1950 | 2035, 2053, 2069, 2035, 1771 | `lamp-body-inspect`, torque off |
| Torque when the body starts | on, left so by the old runtime | off on all five once the old runtime stopped; the body refused ("torque is not on") | `lamp-body-inspect`; the body's own refusal |
| `--home` | 1950, 2043, 2015 | 1771, 2035, 2035 | found at rest. lamp-4ace's head rests leaning toward the base; this one's rests at the old runtime's rest pose (1770), which is why 1950 is refused here |
| `--folded-arm` | 2048, 2047 | 2053, 2069 | found at rest |
| `--stand` | 1772, 2043, 2015, 2657, 2602 | 1772, 2035, 2035, 2657, 2602 | lamp-4ace's pose with this lamp's roll and yaw |
| `--gaze` | 727, 553 | 782, 557 | measured here the same way, below |
| Servo limits in each servo's memory (yaw, shoulder, elbow, roll, pitch) | pitch's lower limit 1657; the rest not read | 809–3239, 2047–3799, 0–4095, 1065–2413, 1451–3067 | read without moving |
| Gains each servo holds, P, D, I (yaw, shoulder, elbow, roll, pitch) | not read | 16/32/0, 16/32/0, 32/32/10, 16/32/0, 32/32/10: the old runtime's, kept in the servos; this body writes none | read with the body stopped |
| Head roll, folded | meets something four counts from where it rests | moves 23 counts one way and 27 the other from 2,040 and stops 13 to 17 short of a goal 40 away, at a load of 64 to 80 | a goal stepped by hand with the body stopped |
| The shoulder standing | | rests 22 to 34 counts short of 2657, pushing at a load of 100 to 150 for as long as it stands | the body's log, two runs |
| The elbow folded | | at its goal, still reading a load of 120 to 140 | the body's log, two runs |

What follows from it:

- **Torque.** With nothing of the old runtime left to give its servos
  torque, the body takes hold itself: `--torque on` sets the goal of a joint
  found without torque to where it is, then writes torque on. Tried on
  2026-10-14 with the yaw and roll servos' torque switched off and their
  goals 12 and 16 counts from where they stood: without the option the body
  refused ("wrist_roll torque is not on"); with it, two torque writes, both
  joints read the same position after as before (2061 and 2049), and the
  run went on for 282 steps until it was stopped. A goal written to these
  servos while their torque is off turns their torque back on, so a stale
  goal cannot be made by hand that way; the 12 and 16 counts were the ones
  the servos held.
- **The start window.** The head must be found between 1,760 and 2,020; on
  this lamp it rests at 1,770 to 1,773, ten counts inside. The pitch floor
  of 1,670 is lamp-4ace's servo limit; this lamp's is 1,451, so the floor
  takes a few counts off the standing lamp's thinking lift (asked 1,667, held
  at 1,680) and nothing else.
- **The folded head's window.** lamp-4ace's window (1,740 to 2,030) left
  this head, resting at 1,764 to 1,800, 30 counts of lift, too little to
  start it from below. Measured on 2026-10-14 at 20:30 with the body
  stopped, the lamp folded, only the head's pitch moved, a count at a time
  at about six counts a second with its force held to half: going down it
  met the base at 2,010 (still from 2,009 to 2,013 while its goal went on
  to 2,029, its load rising from 20 to 76, and stopped there); going up it
  rose freely from 1,811 to 1,474, a load of 76 to 120 all the way, as far
  as the servo's own lower limit (1,451) allows. So its own window is
  `--pitch-window 1670,1995`: 15 counts short of the base, as lamp-4ace's
  2,030 is of its 2,045, and the head's floor at the top. It holds
  `--home`'s 1,771. FOLDED's start window (1,760 to 2,020) is kept: a head
  found past 1,995 is brought into the window at the body's speed. In the
  unit since 20:43
  ([readings](verification/pitch-window-lamp-52e6-20261014/README.md)).
- **The gaze.** Measured as on lamp-4ace, with its eyes stopped and the face
  told to the body on its own socket: a face at 0.5 across brought 254
  counts of yaw, and the picture (640 by 480, one frame before and one
  after, its shift found by phase correlation and checked by eye) moved 104
  of 320 pixels to the left; a face at 0.3 down brought 116 counts of pitch
  and the picture moved 50 of 240 pixels up. So a half-picture is 782 counts
  of yaw and 557 of pitch, both positive. Once each.
- **Driven by mood.** With its voice stopped and a script telling the body
  what the voice would (listening 6 s, thinking 4 s, speaking 5 s with a
  nod once, resting 4 s, three times; then left at rest until it folded,
  spoken to until it stood, and left again), twice, 13:46 to 13:51 on the
build it was installed with and 14:05 to 14:11 on this one with
`--torque on`: eleven exchanges, five nods asked for and played, folded
four times and stood up twice. No stop, no joint gave way, no arm was sent
back down. Thinking turned the base 170 to 190 counts and it was back
within 15 counts of where it listened about two seconds after the voice
began. The warmth guard read single steps of 65 to 109 on joints that read
46 to 50 either side of them, as on lamp-4ace, and never acted on them.

The raw readings are in
[`verification/body-lamp-52e6-20261014/`](verification/body-lamp-52e6-20261014/README.md).

**Not shown on this lamp:** that a power cut leaves the servos as stopping
the old runtime did (task 23); how the arm gives way against a hand (task
24); the head's roll in use: it moves when folded here, but stays at a reach
of one count as on lamp-4ace until a tilt is chosen and watched; and
anything about leaning.

## How the head follows (lamp-52e6, 2026-10-14)

Read from the servos themselves, about every 16 ms, with the body stopped
and the lamp standing, at the profile the body writes (goal speed 1000,
acceleration 60). Once each.

| Joint, goal moved | First movement | 90% of the way | Farthest | Comes to rest |
|---|---|---|---|---|
| Head pitch, 40 down | 85 ms | 181 ms | 108% | 3 counts past |
| Head pitch, 40 up | 95 ms | 727 ms | 98% | 1 count short |
| Head roll, 40 either way | 116 to 118 ms | never | 52 to 60% | 16 to 19 counts short |
| Base yaw, 60 | 114 ms | never | 83% | 10 counts short |

Roll and yaw hold a proportional gain of 16 (pitch 32); a servo with it
stops well short of a small goal and pushes there, which is why the body
gives a joint that has arrived its own position.

A nod, head pitch down and back, asked of the servo directly:

| Asked | Reached | When | Back within 3 counts |
|---|---|---|---|
| 25 counts (2.2 degrees), held 0.15 or 0.3 s | 12 to 14 counts, once 24 | 146 to 283 ms | 0.27 to 1.2 s |
| 45 counts (4 degrees), held 0.15 s | 34 to 35 counts (3 degrees) | 179 to 210 ms | 0.31 to 0.37 s |
| 45 counts, held 0.3 s | 38 to 41 counts | 158 to 212 ms | 0.62 s, or not within the second watched |

The acceleration written (60, 100 or 150) changed little; the highest load
was 320, under the 600 the joints that carry the lamp are allowed. So the
quickest nod this head shows clearly is about 3 degrees down and back in
about a third of a second, when the servo is asked 45 counts for 0.15 s. The
body itself is slower than that: its commanded pose moves at `--speed`
(120 counts a second on this lamp), so 45 counts take 0.375 s each way
before the servo is involved, and anything under 20 counts from where a
joint rests is not asked at all.

**Not shown:** whether such a nod looks soft and natural to a person (no one
has watched it), its cost in heat over many nods, and the folded lamp's
nods. The raw readings are in
[`verification/head-follow-lamp-52e6-20261014/`](verification/head-follow-lamp-52e6-20261014/README.md).

## Verification without a lamp

Host tests, `cargo test -p lamp-body` (266 passed, 0 failed, on macOS):
`body::tests` runs the engine against five simulated servos on a simulated
clock. They show that it writes only the three packets above, byte for byte;
that over a scripted exchange every goal names servos 5, 4 and 1, stays within
reach and moves at most two counts per write, and that the goals end exactly
where they were found; that it refuses each start condition with nothing
written, but torque off on all five for a joint found hot or loaded; that an
obstacle ten counts into a tilt ends the run within a few steps with the goal
held at the obstacle and then torque off on all five; that a new flag on the shoulder, a
changed flag or a nine-count move on the elbow, a head servo that stays hot,
and a silent wire each end it, while one hot reading does not; that a head
found 43 counts from a given home is brought there in small steps and never
away from it, and one found 100 counts away is refused; and that a mood that
stops arriving returns it home. With `--torque on`: that three joints found
without torque and with stale goals each get the goal, then the torque, byte
for byte, and none moves; that a joint whose goal write is not kept moves 30
counts and ends the run refused before any other motion; and that the
shoulder and the elbow are taken hold of on a lamp given `--stand` and never
written on one that is not. With `--wake on`: that a head found pressed on
the base at 2,095 rises out of it, never toward the base and never more
than a count a write, in a little over eight seconds, and then runs inside
its window; that a base found 300 counts round comes home without a goal
moving away from home, the way itself never over 20 counts a second; that
one found 450 away is refused with nothing written, as is a head on the base
without a home given; that an arm off its way is never written and the lamp
runs folded, saying why; and that a lamp found where it may start writes
exactly what it writes without the option. And lamp-52e6 as found after its power cut (torque off and goal 0 on all five, head drooped to 2,034), with its unit's options and `--stand` and `--torque on`, in a simulation whose head lags 37 counts at a load of 320 while it lifts: the head is lifted home, never toward the base, and the run goes on; a head that will not lift ends the wake held where it is, the report saying how far its goal got, and then lets go of all five. And a head that stops 32 short of home settles there: its goal and its home become where it is, and the run goes on. With `--wake on` too: lamp-52e6 as found unplugged standing (shoulder 2,633, elbow 2,074) has its shoulder lowered first, only ever down, under a count a write, the elbow never asked to move, and then stands when spoken to; without the option it is refused standing as before. A shoulder that does not come down as it is lowered ends the run held where it is, then torque off on all five. A head held 35 short at rest rests where it is, where without the option the run ends on tracking. A base that sticks 34 behind its goal is led round and settles 34 short, running from there; a shoulder that sways 7 counts while the base wakes is let, and one moved 20 ends the run.
`gesture::tests` bounds every wanted pose. `body_link::tests` in `lamp-voice`
checks the datagram.

Standing up, in the same simulation with a shoulder and an elbow that follow
their goals: the five-joint write and the torque write byte for byte; that a
lamp spoken to rises to the pose and, left alone, folds down to exactly
where it was found, with every write naming servos 5, 4, 1, 2 and 3, the arm
never outside the way between folded and stood up and no arm goal moving
more than six counts in a write; that the elbow's old flag costs one torque
write to the elbow alone; that a head whose standing pitch is toward the
base stays inside its folded reach and window until the arm has made a
quarter of its way; that a shoulder strained for a second on the way up
sends the lamp down, where it stays though it is still spoken to; that an
elbow strained for longer ends the run; that a lamp found standing is taken
as it is and folds down when asked to stop, and one found off the way is
not stood; and that a flag that will not clear, or no home, leaves it a
folded lamp that runs as before.

Looking at a face (`gaze::tests`, and two in `body::tests` with a simulated
camera in the simulated head): that it turns to a face in two to six
corrections and then keeps still; that a `--gaze` set half again too high
still settles, and the reach is never passed; that nothing is corrected from
a moving picture, while it thinks or rests, or for a face a little off the
middle while it speaks; that with nobody there it does nothing, holds for a
few seconds when someone leaves and then lets go, and lets go when it folds;
that the head of a folded lamp does turn to a face while it listens, inside
its reach, and comes home; and that without a face, or without eyes, the
goals written are exactly those of a body that has none.

Gestures (`clips::tests`, several in `body::tests`, and `body_link::tests` in
`lamp-voice`): every clip begins and ends at nothing and stays inside its
speed and size; a standing lamp asked to nod makes the clip's move, for the
clip's time, with no goal jumping, and stands afterwards exactly as it
stood; a gesture asked of a rising lamp, even one asked as the rise
begins or a second one asked while it rises for the first, waits for the
rest of the rise and begins when it stands, and one the lamp never stands
up for goes stale and is said; one asked of a folded lamp at rest stands it
up and then plays, and one asked of a paused, hand-held or slumped lamp is
dropped with its line and the lamp stays as it was; paused after it was
taken, it is dropped before it begins, or plays to its end, and the lamp
folds; a head joint that strains
ends the gesture and not the run; which first words get which gesture, that
one is asked once an answer, and the datagram's exact text.

Not shown by any of this: what a gesture looks like, sounds like or does to
the joints on a real lamp, at any of these speeds; that the head turns to a
real face, since no
camera was involved and the two numbers of `--gaze` are not known for any
lamp; anything about standing on a real lamp, where
the weight of the head, the slack in the joints and the loads these two
servos read have not been seen by this code; that the simulated servo resembles a real one (its
speed register is taken as counts a second, which is not established); that
roll and yaw are clear of the base and the arm in the folded pose, which no
Rust code has moved; that the load bounds suit the head's own weight (the head
at rest at 2,001 on lamp-4ace reads a load of 68, and read up to 116 while it
was moved there at about 47 counts a second); what the servos sound like to
the microphones.

## A hand on the bus

With `LAMP_BUS=on`, lamp-body says on `body/hand` (`lamp_msgs::Hand`,
latest, kept 1 s) whether a hand is pushing it now, with every step it
passes on (every 80 ms): `pushed` is true while a head or base joint met
resistance and is held where it gave way, and a hand is on it. A push
starts on a step where the joint is more than 15 counts (`HAND_MOVED`)
from where it first gave way, reads a load over its own bound or, under
`--limp on` (the default), over its contact floor (a pitch pressed at 675
in a glance, under the glance's 700), or, pinned under `--limp on`, is
pushed as the slump judges it (a load over the lower of its bound and its
contact floor, or more than 11 counts, `LIMP_LAG`, off where it gave), so
a push that slumps the lamp is always said. Being moved that far is a
push even under the joint's load bound and contact floor, and "where it
first gave" is kept across the holds of one push: a joint that gives way
again within a hold's length (`EASE_STEPS`, 2 s) of its last hold keeps
where it first gave. A hand on lamp-52e6 (2026-10-27) turned the base about
30 counts a hold, each a stop-pushing of its own at loads of 104 to 160
under its 220; from where each hold began it never moved 15 counts, and
`pushed` was never said. Once started, it stays while
the joint is more than 15 counts off where it first gave, or reads over
half its bound and over its own resting load by a quarter of its bound (a
press at 300 easing to 130 on a pitch bound 240, that read 25 before, is
still a push), and it ends on the first step that is neither (the next
`body/hand` after a release says it is gone). A hand that keeps the joint
off, re-held where it is each second, stays a push until it lets go;
`since_us` is when that push began, on the body's
own clock (0 when not pushed). A joint held at its rest that reads only its
own weight and jitters a few counts is not a push, whichever lamp's
calibration: lamp-06c1's head, held where it gave way to its own push at its
rest, read 124 to 136 there and said `pushed` 8 times a second with nobody
at the lamp (2026-10-23). It adds a publication and changes nothing the
body does.

What it can feel: a load over a joint's bound, with or without
`--give-way`; with `--give-way` also a joint held off its goal at a load
over 100 (a light hand on the base) and a hand that keeps it held. Without
`--give-way` a hand still on the joint after it gave way ends the run
(`load`), so `pushed` lasts until then. The shoulder and the elbow are not
in it: a hand on the arm makes the lamp sit down, which the body's report
says. Eight tests: steps say a hand only while one is on a joint, and never
without one; a resting joint jittering inside that band, at any rest and
resting load, never says one, while a hand moving it or pressing past its
bound still does; a hand keeping a joint 40 counts off where it gave,
pressing under its bound, says one on every step of the hold and none
once it lets go (pitch and base); a pitch pressed over its contact floor
in a glance says one until it slumps; a base turned 33 counts a hold over
two holds, pressed under its bound, says one in the second hold and none
once the hand has gone, while one such turn held still does not; the bus
says it with each step passed on and keeps when the push began.

## Preparing to unplug (#1674)

With `LAMP_UNPLUG=on`, the body accepts the core skill's kept request,
immediately closes the wake window and folds through its existing guarded
trajectory. It reports Ready only from fresh physically settled samples, not
from pause or the trajectory fraction. The inhibit lasts until reboot, including
across a body-service restart. See [the protocol and proof](unplug.md); no lamp
validation is claimed yet.
