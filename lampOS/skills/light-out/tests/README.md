# Light out's tests

**How they were made.** Not on a lamp. Each case's `.jsonl` files were
written by a script in the format `lamp-bus record` writes, as Stuffy air's
longer cases were: `room/presence` every 2 s, each message good for 3 s
(`expires_ms`), as the eyes say it with `LAMP_PRESENCE=on`
(`crates/eyes/src/away.rs`: someone within 20 s of the last face, voice,
touch or, in the dim, view changing, else `nobody_s` from it, or from the
start when there was none); the eyes say it once a second, and every 2 s
halves forty minutes' lines (#5746). `voice/state` is kept until replaced,
`resting` unless the case says otherwise, half a second off the presence
messages so that no two share a time. Each `expect.toml` sets the local
time of the case's first message (`clock`): the skill keeps still from
23:00 to 06:00. Re-recording a case on a lamp, with its owner's yes,
replaces the written one.

**Run** with `lamp-skills test lampOS/skills/light-out` (or `cargo test -p
lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result (lamp-skills, #5746) |
|---|---|---|
| `dims-after-forty-minutes-goes-out-five-later-and-comes-back-when-you-sit-down` | From 10:00, a face until 30 s, nobody from 50 s (counted from 30 s), someone back at 3000 s | Pass: dimmed two steps at 2430 s, 2400 s after the last face, held five minutes; out over a minute at 2730 s; given back at 3000 s |
| `comes-back-at-once-when-the-view-changes-in-the-dim` | From 10:00, nobody; at 2550 s, in the dim, someone by `moved` (the eyes saw the view change) | Pass: dimmed at 2400 s; given back at 2550 s, and nothing goes out |
| `waits-for-the-lamp-to-finish-speaking` | From 10:00, nobody; `voice/state` speaking from 2398.5 to 2440.5 s; someone back at 2800 s | Pass: nothing while it speaks, dimmed at 2442 s, the first presence after it rests (`wait = "1h"`), out at 2742 s; given back at 2800 s |
| `waits-out-the-night` | From 05:15, nobody; someone back at 3060 s | Pass: nothing at 05:55, in the night; dimmed at 06:00 (2700 s) with the chance kept, out at 3000 s; given back at 3060 s |
| `never-at-night` | From 22:30, nobody; someone back at 2460 s | Pass: nothing: the forty minutes come at 23:10, and someone back at 23:11 spends the chance |
| `stays-on-when-asked-to-keep-the-light-on` | As the first case, with `memory/rules` saying `keep_light_on: true` from 0.25 s ("keep the light on") | Pass: nothing dims or goes out |
| `comes-back-when-the-eyes-stop-looking` | From 10:00, nobody; the presence stops after 2500 s (a mute, in the dim); `voice/state` every 10 s from 2500.5 s | Pass: dimmed at 2400 s; given back at 2510.5 s, the first message after the last presence ran out at 2503 s; nothing goes out |

A face, a word, a touch and the view changing in the dim each giving the
light back, and the out five minutes after the dim, are also run by the
program itself on a stepped clock (`crates/skills/tests/it/skills.rs`,
`light_out_dims_five_minutes_then_goes_out_and_anyone_in_the_dim_brings_it_back`).

The fade itself, the ring easing into the ember over a minute and back in about
half a second, is the ring's, tested in `crates/voice/src/ring_face.rs`;
how it looks is for the lamp check ([presence.md](../../../docs/presence.md)).
