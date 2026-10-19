# Music sway's tests

**How they were made.** Not on a lamp: no lamp publishes `ears/classes` yet
(#1923). Each case's `.jsonl` files were written by a script in the format
`lamp-bus record` writes: `ears/classes` every 500 ms, each message good for
1.5 s (`expires_ms`), as lamp-ears says it with `LAMP_EARS_CLASSES=on`
(`crates/ears/src/classes.rs`), with a music score of 0.85 while music plays
and 0.05 otherwise; and `voice/state`, kept until replaced. Each
`expect.toml` sets the local time of the case's first message (`clock`).
Re-recording a case on a lamp, with its owner's yes, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/music-sway` (or `cargo test -p
lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result (lamp-skills, #1669) |
|---|---|---|
| `sways-once-after-twenty-seconds-of-music` | From 10:00, music for five minutes, then quiet | Pass: one sway at 20 s, none again while the same music goes on |
| `a-quiet-bar-does-not-end-the-music` | Music with three quiet seconds at 10 s | Pass: still one sway at 20 s (`gap = "5s"`) |
| `not-for-a-short-clip` | Ten seconds of music | Pass: nothing |
| `never-while-the-lamp-speaks` | Five minutes of music while the lamp speaks | Pass: nothing |
| `never-at-night` | From 23:30, five minutes of music | Pass: nothing |
| `never-with-a-hand-on-it` | Five minutes of music, a hand on the lamp from 5 s | Pass: nothing |

The threshold (0.6) is provisional: no score is measured on a lamp yet
(`docs/sound-events.md`). How the sway looks is for the lamp check.
