# Stretch break's tests

**How they were made.** Not on a lamp: each case's `.jsonl` files were written
by `lamp-bus record` on a private bus while its messages were published with
`lamp-bus pub ... --at-us` at the case's times (as Welcome back's were, #938),
faces at least every 2 s where a condition reads them (the runtime's face
lifespan). Re-recording a case on a lamp, with its owner's yes, replaces the
synthetic one.

**Run** with `lamp-skills test lampOS/skills/<name>` (or `cargo test -p
lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result (lamp-skills, #950 at `a0b5f1a47`) |
|---|---|---|
| `acts-after-50-minutes` | A face for 55 minutes, gone twice (36 s at minute 20, 48 s at 35); `voice/state` resting | Pass: looks at the face and stretches once, at 50 minutes |
| `quiet-when-short-sitting` | A face for 40 minutes, then none to minute 55 | Pass: nothing |
| `quiet-until-they-leave` | A face for 3 hours, never leaving | Pass: once at 50 minutes, never again |
| `waits-for-the-talk-to-end` | A face for 58 minutes; `voice/state` listening at 48, speaking at 50, resting at 56 | Pass: nothing during the talk, then one stretch at the first face after it, 56.5 minutes (`wait = "10m"`, #1021). Was `quiet-while-talking`, when a chance held back was spent |
| `quiet-while-the-talk-goes-on` | The same, the talk never ending before the recording does | Pass: nothing |
| `across-the-room` | `acts-after-50-minutes` with the face 0.04 of the picture tall, someone across the room (lamp-4ace, 2026-10-16 15:16, when it fired for them) | Pass: nothing (`near = 0.14`); without `near` it stretched at 50 minutes |
| `waits-for-a-look-up` | `acts-after-50-minutes` with `room/activity` (#1678) written beside each face, `head_down` from minute 48 to 54 | Pass: nothing at 50 minutes, one stretch at the first face after 54. Without the `[[while]]` it stretches at 50 minutes |
| `lets-it-go-when-they-stay-head-down` | `quiet-until-they-leave` to minute 70, `room/activity` `head_down` from 45 to 65 | Pass: nothing: the chance at 50 waits its ten minutes and is let go, and it re-arms only when they leave. Without the `[[while]]` it stretches at 50 |
| `quiet-when-paused` | Not recorded: waits for the Policy's gate (#939) | Not run |

`quiet-when-paused` waits for the Policy's gate (#939), the one gate for
the pause: `lamp-skills test` skips the ask, so that case will fail until
the gate is in the replay. A `[[while]]` on `lamp/pause` would do it now, but
would keep the skill silent on a lamp where nothing publishes the pause.
