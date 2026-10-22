# Music sway's tests

**How they were made.** Not on a lamp: no lamp publishes `ears/classes` yet
(#1923). Each case's `.jsonl` files were written by a script in the format
`lamp-bus record` writes: `ears/classes` every 500 ms, each message good for
1.5 s (`expires_ms`), as lamp-ears says it with `LAMP_EARS_CLASSES=on`
(`crates/ears/src/classes.rs`), with a music score of 0.85 while music plays
and 0.05 otherwise (a mute: `unavailable`, with no scores); and `voice/state`, kept until replaced. Each
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
| `a-second-piece-within-half-an-hour-gets-nothing` | A minute of music, six minutes quiet (rearmed), two minutes of a second piece | Pass: one sway at 20 s only (`cooldown = "30m"`) |
| `music-under-the-bar-gets-nothing` | Five minutes of music scored 0.55 | Pass: nothing (`above = 0.6`) |
| `a-mute-in-the-music-starts-it-over` | 12 s of music, 10 s muted (`unavailable`), then music | Pass: one sway at 42 s, not 20 s (the mute is past `gap = "5s"`) |
| `piano-the-voice-takes-for-speech` | lamp-52e6's own scores from the played test (22 Oct, 10:35): 90 s of a piano from the Mac speaker, the hold ripe at 57 s; a written `voice/state` of eight short turns over it (2.5 s listening, 1.5 s thinking), one from 55 s to 59 s, as the voice took the piano for speech (its recording was not kept: 71 resting, 23 listening and 14 thinking messages while it played) | Pass: one sway at 59 s, when that turn ends (`wait = "1m"`). Without the `wait`, nothing: the chance held back at 57 s was spent and the rearm line disarmed the piece, which is what the played test saw |
| `never-to-its-own-music` | Five minutes of music with `own_voice` `present` (the lamp plays it), the voice resting | Pass: nothing (#2833) |
| `room-music-as-its-own-ends` | The lamp's own music for 40 s, then room music to 300 s | Pass: one sway at 40 s, the chance held back at 20 s kept by `wait` |
| `room-music-after-its-own-piece` | The lamp's own music for two minutes, six quiet minutes, then two minutes of room music | Pass: one sway at 500 s, 20 s into the room's music; its own piece spent no cooldown |
| `jazz-through-lamp-52e6s-microphone` | lamp-52e6's own scores (#2399): 20 s of an empty office, 120 s of a jazz vocal track, 30 s of a line over it; the voice resting beside it | Pass: one sway at 43 s, 22 s into the track (its dips under 0.6 stay within the gap) |

The threshold (0.6, 20 s, a 5 s gap) holds through lamp-52e6's microphone
(#2399, 21 Oct): the piano and jazz tracks from a speaker beside it each got
one sway, and its empty office and a line over the music none.
`jazz-through-lamp-52e6s-microphone` is that run's own scores. How the
sway looks is for the lamp check.
