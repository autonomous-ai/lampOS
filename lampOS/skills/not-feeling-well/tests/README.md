# Not feeling well's tests

**How they were made.** Not on a lamp (#1681). The four `quiet_ms` cases
(#1681's third PR) model each part reporting every second and `lamp/health`
every 5 s only, its `quiet_ms` worked out as `lamp-health` does. Each case's `.jsonl` files
were written by a script in the format `lamp-bus record` writes, as Light
out's were: `lamp/health` every 5 s and at once when its level changes,
each message good for 15 s (`expires_ms`), as `lamp-health serve` says it
([health.md](../../../docs/health.md)); `ok` outside the case's error, and
`error` ("body: the servo port does not answer") inside it. `voice/state`
is kept until replaced, `resting` unless the case says otherwise, half a
second off the health messages so that no two share a time. Each
`expect.toml` sets the local time of the case's first message (`clock`):
the skill keeps still from 22:00 to 08:00. Re-recording a case on a lamp,
with its owner's yes, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/not-feeling-well` (or `cargo
test -p lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result (lamp-skills, #1681) |
|---|---|---|
| `droops-once-an-error-holds-half-a-minute` | From 10:00, `error` from 60 to 300 s | Pass: `sleepy` and a pale breath once at 90 s; given back at 305 s, 10 s (the gap) after the last `error` at 295 s |
| `quiet-on-a-short-error` | From 10:00, `error` from 60 to 80 s | Pass: nothing |
| `quiet-while-a-talk-goes-on` | From 10:00, `error` from 60 to 300 s; a talk from 50.5 to 400.5 s | Pass: nothing: the error ended before the talk did |
| `waits-for-the-talk-to-end` | From 10:00, `error` from 60 to 300 s; a talk from 50.5 to 150.5 s | Pass: nothing in the talk; once at 155 s, the first health after it (`wait = "1h"`); given back at 305 s |
| `quiet-at-night` | The first case from 23:00 | Pass: nothing |
| `waits-out-the-night` | From 07:50, `error` from 60 to 900 s | Pass: nothing in the night; once at 08:00 (600 s), the chance kept; given back at 905 s |
| `ends-when-health-goes-silent` | From 10:00, `error` from 60 s; the last health at 200 s; `voice/state` every 10 s from 200.5 s | Pass: once at 90 s; given back at 220.5 s, the first message after the health ran out at 215 s |
| `droops-once-a-part-that-was-well-goes-quiet` | From 10:00, every part well; the eyes' last report at 59.8 s, back at 500.8 s (a crashed part, `quiet_ms`) | Pass: `sleepy` and a pale breath once at 360 s (300.2 s unheard); given back at 510 s, 10 s after the last health with `quiet_ms` (500 s) |
| `quiet-on-a-short-restart` | From 10:00, the body restarts five times, gone 6 to 9 s each (stale, then `ok` again) | Pass: nothing (`quiet_ms` at most 5.4 s) |
| `quiet-on-a-part-never-heard-from` | From 10:00, the eyes expected and never heard from (missing, the lamp `stale` for 10 min) | Pass: nothing (no `quiet_ms`) |
| `droops-once-for-a-part-that-errs-and-then-stops` | From 10:00, the body says `error` from 60.6 s, its last report at 119.6 s | Pass: once at 95 s (the first `error` health at 65 s); nothing at 5 min of its silence: a part that keeps its `error` is not in `quiet_ms` |

The light is named as asked, `no_brighter = true` with it (#3691): its level
against the person's light is made as the run begins, from `light/state`,
which a replay has none of. That it is never brighter (white 1 over a person
at level 1, as written at 2 and up, nothing over a light that is off) is
`glow`'s unit tests in `crates/skills`.

The `sleepy` look has no case: whether the droop and the pale breath read as a
lamp a little unwell, not a status light, is for the lamp check. The 30 s and the
10 s gap are not measured against real failures yet.
