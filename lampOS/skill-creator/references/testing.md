# Testing a skill on the lamp's own recordings

A skill passes when, replayed against what the lamp actually sensed, it does
what it should and nothing else. Never claim a test you did not run.

## Recordings

The lamp's bus can be recorded and played back (`lamp-bus`, on the lamp or
any computer with the bus):

```sh
# On the lamp: record the topics the skill listens to, for as long as the case takes.
lamp-bus record tests/arrives-after-a-break humans/faces/tracked --seconds 900
# Elsewhere, or later: play it back, here 60 times as fast.
lamp-bus replay tests/arrives-after-a-break/humans_faces_tracked.jsonl --speed 60
```

A recording is one `.jsonl` file per topic (`/` becomes `_`), the topic's
declaration first. Record every topic the skill's trigger and conditions
name, and `voice/state` and `lamp/pause`. A case lasts as long as it needs
(a cooldown case may be two hours: replay plays it in seconds), but holds
nothing private: faces are boxes and ids, never pictures; leave
`voice/heard` (what someone said) out unless the skill needs it, and then
ask the owner.

## The test file

Each case in `tests/` is a folder with its recordings and `expect.toml`:

```
tests/
  acts-after-50-minutes/
    humans_faces_tracked.jsonl
    expect.toml
  quiet-when-just-done/
    humans_faces_tracked.jsonl
    expect.toml
```

```toml
[[expect]]
at = "50m"                 # optional: when, from the case's first message
within = "1m"              # optional; without it, exactly at
call = "body.gaze"         # body.gaze, body.gesture, body.move, body.mood, light or say
args = { look_at = "face" }

[[expect]]
call = "body.gesture"
args = { gesture = "stretching" }
```

The `[[expect]]` entries are **all** the calls the case may make, in order:
any other call fails it, so "exactly once" needs nothing more. Arguments left
out of `args` are not compared. The runtime gives: `body.gaze` `look_at`,
`x`, `y`, `size`; `body.gesture` `gesture`; `body.move` `move`; `light` `colour`, `level`, `for`,
or `dim`, `until`, or `out`, `over_ms`, `until`; `body.mood` `mood`, `for_ms`; `say` `note`; `look`, none; `ends`, none (a dim held `until = "ends"` given back when its trigger ends). A skill with `[quiet]` hours needs `clock = "10:00"` in the expect file, the local time of the case's first message: without it, it makes no call. For a skill started by its `tool` (someone asks), say when the model calls
it, from the case's first message; a case that is only that needs no
recording:

```toml
[[call_tool]]
name = "check_my_work"
at = "16s"
```

A case where the skill must do nothing has
no `[[expect]]`, only:

```toml
none = true
```

## The cases every skill needs

- **It acts**: the recording in which it must act, and what it calls.
- **It does not act**: at least one of nobody there, too soon after the last
  time (the cooldown, or before it re-arms), a person talking to the lamp
  (if the skill has a `[[while]]` on `voice/state`). The pause is the Policy's
  gate (#939), which a test skips: a pause case waits for that gate.

Run them with `lamp-skills test <folder>`: each case is replayed in the
program (not on a bus), every message in order of its time, with the
Policy's ask skipped. A case with no recording is counted and not run. Say in
`tests/README.md` which cases exist, how each was made, and which passed. Do
not write a test runner of your own. Before trusting a pass, change the
skill so it should fail (a shorter `for`) and see the case fail.

## Making a case without waiting for the room

A case can be made by publishing its messages with their times into a
private bus while recording it:

```sh
lamp-bus --root /tmp/case-bus record tests/acts-after-50-minutes humans/faces/tracked voice/state &
lamp-bus --root /tmp/case-bus pub humans/faces/tracked latest '{"faces":[]}' --type lamp_msgs/Faces --at-us 0
# ... one pub for each message, --at-us in microseconds from the case's start
kill %1
```

Publish faces at least every 2 s wherever a condition reads them (a face
lasts 2 s), as the lamp's eyes do.

Nothing checks a message's shape: a field written wrong never holds, and a
case built on a shape the lamp never sends passes anyway. Copy the shapes
below, or each type's fields from the lamp's `lampOS/docs/msgs.md` (the code
is `crates/msgs`). The ones skills use most, as `lamp-bus pub` takes them:

| Topic | `--type` | A message |
|---|---|---|
| `humans/faces/tracked` | `lamp_msgs/Faces` | `{"faces":[{"id":"a1","xmin":0.3,"ymin":0.2,"xmax":0.6,"ymax":0.6,"confidence":0.9}]}`, or `{"faces":[]}` for nobody |
| `voice/state` | `lamp_msgs/VoiceState` | `{"state":"resting"}` (`listening`, `thinking`, `speaking`) |
| `air/state` | `lamp_msgs/AirState` | `{"co2_ppm":1300,"feel":"stuffy","pm2_5":4.0,"humidity":50.0,"temperature":30.0}` |
| `schedule/due` | `lamp_msgs/ScheduleDue` | `{"stamp":{"clock_id":"case-clock","at_us":0,"sequence":1},"store_id":"0123456789abcdef0123456789abcdef","revision":1,"enabled":true,"ready":true,"pending":0,"waiting_for_clock":0}`; with `pending` above 0, also `"oldest":{"id":1,"delivery":1,"due_unix_ms":...,"late":false}` |

For any other topic, read its type in `docs/msgs.md` before writing a case. Say in `tests/README.md` that the case was
made this way, not recorded on a lamp.

## Recording a case on the lamp

Ask the owner first, say what will be recorded and for how long. Prefer
recordings the lamp already has; ask the person beside the lamp only for
what needs a person (sitting down, a hand).
