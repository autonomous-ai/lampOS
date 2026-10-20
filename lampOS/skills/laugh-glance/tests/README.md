# Laugh glance's tests

**How they were made.** Not on a lamp: no lamp publishes `ears/classes` yet
(#1923). Each case's `.jsonl` files were written by a script in the format
`lamp-bus record` writes: `ears/classes` every 500 ms, each good for 1.5 s,
with a laughter score of 0.9 while someone laughs and 0.02 otherwise;
`humans/faces/tracked` once a second, one face or none; and `voice/state`,
kept until replaced. Each `expect.toml` sets the local time of the case's
first message (`clock`). Re-recording a case on a lamp, with its owner's
yes, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/laugh-glance` (or `cargo test
-p lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result (lamp-skills, #1669) |
|---|---|---|
| `glances-once-at-a-laugh` | A face in view; laughter from 10 to 13 s | Pass: one glance at about 10 s |
| `once-a-burst-and-not-again-within-five-minutes` | Laughter at 10 s, 14 s and 60 s | Pass: one glance; 14 s and 60 s are within the cooldown |
| `one-score-over-the-bar-is-a-laugh` | One laughter score of 0.35 | Pass: one glance at 10 s (one window is enough) |
| `not-at-a-score-under-the-bar` | One laughter score of 0.25 | Pass: nothing (`above = 0.3`) |
| `nobody-in-view` | Laughter, no face | Pass: nothing |
| `never-while-the-lamp-speaks` | Laughter while the lamp speaks | Pass: nothing |
| `never-at-night` | From 23:30, laughter | Pass: nothing |
| `laughter-on-and-on-gets-one-glance` | Laughter without a lull from 10 s to 330 s | Pass: one glance; none at 310 s when the cooldown ends (`rearm`) |
| `a-laugh-with-nobody-in-view-spends-nothing` | A laugh with nobody at 10 s; a face from 30 s, a laugh at 60 s | Pass: one glance about 60 s; the first spent no cooldown |
| `never-over-its-own-sound` | Laughter while `own_voice` is `present` | Pass: nothing |

The bar, one window over 0.3, is set from free-to-use laughs and people
talking scored off the lamp (#1669, `docs/sound-events.md`); the lamp check
confirms it. How the glance looks, and how it sits beside
Expression's own startled glance at a sudden sound, is for the lamp check.
