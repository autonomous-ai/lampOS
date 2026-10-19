# Watch you leave's tests

**How they were made.** Not on a lamp: no lamp has said `room/stood_up` with
its side yet. Each case's `.jsonl` files were written by a script in the
format `lamp-bus record` writes: `room/stood_up` as the eyes say it (an event
good for 5 s, `sitting_s` and the side of the picture the sitter left by, or
none from the middle), `voice/state` and `body/hand`, each kept until
replaced. Each `expect.toml` sets the local time of the case's first message
(`clock`). Re-recording a case on a lamp, with its owner's yes, replaces the
written one.

**Run** with `lamp-skills test lampOS/skills/watch-you-leave` (or `cargo test
-p lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result (lamp-skills, #1669 (e)) |
|---|---|---|
| `watches-them-go-left` | The sitter leaves by the picture's left at 10 s | Pass: one `watch-left` at 10 s |
| `watches-them-go-right` | The same by the right | Pass: one `watch-right` at 10 s |
| `from-the-middle-it-does-not-move` | No side | Pass: nothing |
| `from-the-middle-keeps-its-ten-minutes` | No side at 10 s, then left at 300 s | Pass: one `watch-left` at 300 s; a middle departure starts no cooldown |
| `once-in-ten-minutes` | Departures at 10 s, 300 s and 700 s | Pass: 10 s and 700 s; 300 s is within the cooldown |
| `never-while-the-lamp-speaks` | The lamp speaking as they leave | Pass: nothing |
| `never-with-a-hand-on-it` | A hand on the lamp as they leave | Pass: nothing |
| `never-at-night` | From 23:30 | Pass: nothing |

The turn's size (60 counts of base yaw) and its direction on a real lamp are
for the arm hour: the sign follows a lamp whose `--gaze` yaw is positive.
