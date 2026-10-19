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
| `glances-once-at-a-laugh` | A face in view; laughter from 10 to 13 s | Pass: one glance at about 11 s |
| `once-a-burst-and-not-again-within-five-minutes` | Laughter at 10 s, 14 s and 60 s | Pass: one glance; 14 s is the same burst (`rearm`), 60 s is within the cooldown |
| `not-at-one-loud-syllable` | One laughter score | Pass: nothing (`for = "1s"`) |
| `nobody-in-view` | Laughter, no face | Pass: nothing |
| `never-while-the-lamp-speaks` | Laughter while the lamp speaks | Pass: nothing |
| `never-at-night` | From 23:30, laughter | Pass: nothing |

The threshold (0.5) is provisional: no score is measured on a lamp yet
(`docs/sound-events.md`). How the glance looks, and how it sits beside
Expression's own startled glance at a sudden sound, is for the lamp check.
