# Light out's tests

**How they were made.** Not on a lamp: no lamp has published
`room/presence` yet (#1677). Each case's `.jsonl` files were written by a
script in the format `lamp-bus record` writes, as Stuffy air's longer cases
were: `room/presence` once a second, each message good for 3 s
(`expires_ms`), as the eyes say it with `LAMP_PRESENCE=on`
(`crates/eyes/src/away.rs`: someone within 20 s of the last face or voice,
else `nobody_s` from it, or from the start when there was none), and
`voice/state` kept until replaced, `resting` unless the case says otherwise,
half a second off the presence messages so that no two share a time. Each
`expect.toml` sets the local time of the case's first message (`clock`): the
skill keeps still from 23:00 to 06:00. Re-recording a case on a lamp,
with its owner's yes, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/light-out` (or `cargo test -p
lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result (lamp-skills, #1677) |
|---|---|---|
| `goes-out-after-ten-minutes-and-comes-back-when-you-sit-down` | From 10:00, a face until 30 s, nobody from 50 s (counted from 30 s), someone back at 900 s | Pass: out over 30 s at 630 s, 600 s after the last face; given back at 900 s. Pins the ten minutes to the second |
| `waits-for-the-lamp-to-finish-speaking` | From 10:00, nobody; `voice/state` speaking from 598.5 to 640.5 s; someone back at 720 s | Pass: nothing while it speaks, out at 641 s, the first presence after it rests (`wait = "1h"`); given back at 720 s |
| `waits-out-the-night` | From 05:45, nobody; someone back at 960 s | Pass: nothing at 05:55, in the night; out at 06:00 (900 s) with the chance kept; given back at 960 s |
| `never-at-night` | From 22:58, nobody; someone back at 660 s | Pass: nothing: the ten minutes come at 23:08, and someone back at 23:09 spends the chance |
| `stays-on-when-asked-to-keep-the-light-on` | As the first case, with `memory/rules` saying `keep_light_on: true` from 0.25 s ("keep the light on") | Pass: nothing goes out. With the rule false the case fails: the light goes out at 630 s |
| `comes-back-when-the-eyes-stop-looking` | From 10:00, nobody; the presence stops after 700 s (a mute); `voice/state` every 10 s from 700.5 s | Pass: out at 600 s; given back at 710.5 s, the first message after the last presence ran out at 703 s |

The fade itself, the ring easing into the ember over 30 s and back in about
half a second, is the ring's, tested in `crates/voice/src/ring_face.rs`;
how it looks is for the lamp check ([presence.md](../../../docs/presence.md)).
