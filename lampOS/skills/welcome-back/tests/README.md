# Welcome back's tests

**Run** with `lamp-skills test lampOS/skills/welcome-back` (or `cargo test -p
lamp-skills`, which runs every skill's cases). Times are from each case's
first message.

| Case | How it was made | What it holds | Result (with #1140) | Fails when |
|---|---|---|---|---|
| `back-after-ten-minutes` | Synthetic: `lamp-bus pub --at-us` on a private bus (#938) | Nobody near for ten minutes (a face across the room at 0.10), someone sits down at 600 s; away and back within and after the cooldown | Pass: a hello at 603 s and 2,403 s | (unchanged by #1140) |
| `there-at-start-then-talking` | Synthetic, as above | Someone there from the start; the lamp speaks at 440–445 s; back at 450 s; back again at 900 s | Pass: one hello, at 903 s: the 450 s arrival is 5 s after the lamp spoke | (unchanged by #1140) |
| `back-at-the-desk-while-the-room-talks` | **Recorded on lamp-06c1**, 2026-10-16 17:50:00–17:58:00 (#995), `lamp-bus record humans/faces/tracked voice/state`, cut to the window | The person sits down at 17:56:39.4 (399.3 s; face 0.21 to 0.31 of the picture's height) while the office talks near the lamp (`listening`, `thinking` for 1 to 4 s every 5 to 60 s), and the lamp speaks at 406.2–408.4 s, most likely to them | Pass: nothing. The lamp is not 30 s without speaking before their face leaves the picture, about 25 s after they sat | the lamp's own speech is not counted: a hello at 413.5 s, to someone it has just talked with |
| `the-room-talks-the-lamp-does-not` | **Derived** from the recorded case: the lamp's one reply (3 `speaking` messages, 406.2–408.4 s) turned into `thinking`, as talk it let pass | The same arrival, the office's talk only | Pass: a hello at 413.5 s (17:56:53.6), the first 5 s of rest | without `wait`: nothing |
| `the-room-never-rests-within-the-minute` | Synthetic, as above (#1140) | Nobody for 400 s; someone sits down at 400 s and stays to 520 s; the room talks from 400 to 470 s, never resting 5 s (listening 2 s, resting 2 s) | Pass: nothing, not even once the room rests at 470 s | `wait = "10m"`: a hello at 475 s |
| `gone-before-the-room-rests` | Synthetic, as above (#1140) | Nobody for 400 s; someone sits down at 400 s while the room talks (to 440 s) and leaves at 431 s | Pass: nothing | without the 5 s quiet: a hello at 403 s. A held chance is taken only on a message with a near face, so with no one there after 431 s it passes with `gap = "60s"` too |

Before #1140 the recorded case expected nothing for another reason: the
chance of an arrival was spent when the voice had not rested 30 s at that
moment, which in an office it seldom has, so nobody arriving was greeted.
Now the room's talk counts 5 s and the lamp's own speech 30 s, and the chance
waits up to a minute while the person stays.

The hello is asked with `expires_ms = 60000` (#2861): its say lives a
minute, as the chance's `wait` does, so a turn the lamp takes as it is asked
and the voice's 15 s quiet after it leave it time to begin.

The synthetic cases' faces are one box, 0.25 of the picture's height, every
second; `voice/state` every second. The recordings hold boxes and ids, no
pictures, and no `voice/heard`.
