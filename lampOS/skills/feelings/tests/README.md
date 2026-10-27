# Feelings' tests

**How they were made.** Not on a lamp, and not recorded: each case's
`.jsonl` files were written by a script in the format `lamp-bus record`
writes (#3691). `mood/state` every second, each good for 2.5 s, with `felt`
named in the spans the case gives (glad and lively for 20 s, shorter than
the 90 s Mood holds them, except where a case says; low for its two-minute
spell); `humans/faces/tracked` once a second,
one face or none; `voice/state`, kept until replaced; and, in one case,
`room/activity`. Each `expect.toml` sets the local time of the case's first
message (`clock`). The lamp check (Mood and Tone on, the skill on by hand,
the video face) comes before the skill leaves `PRODUCT_SKILLS_OFF`;
re-recording a case there, with its owner's yes, replaces the written one.

A relative light's case names its `for` and, with a breath, its
`breathe_ms`; the level and colour it becomes are made against the light
shown as the run begins, which a replay has none of. That the ring keeps it
under the night's caps is `ring_caps`' host test in `crates/voice`.

**Run** with `lamp-skills test lampOS/skills/feelings` (or `cargo test -p
lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result |
|---|---|---|
| `glad-lifts-and-breathes-warm` | A face, the voice resting, glad from 40 s to 60 s | Pass: `glad-lift` and a warm 6 s breath for 12 s, once, at 40 s |
| `lively-perks-and-breathes-quicker` | Lively from 40 s to 60 s | Pass: `perk` and a 4 s breath for 8 s, once, at 40 s |
| `low-droops-and-dims` | Low from 40 s for two minutes | Pass: `low-droop` and a steady light a level dimmer for 20 s, once for the spell |
| `waits-for-the-person-to-finish` | Glad at 42 s while the voice listens, 40 s to 50 s | Pass: once, at 55 s, after 5 s resting |
| `a-glad-line-the-lamp-answers` | A glad line at 40 s, the lamp answering 42 s to 50 s, glad held 90 s as Mood holds it | Pass: once, at 80 s, 30 s after it spoke |
| `waits-for-the-lamp-to-finish-speaking` | Low at 42 s while the lamp speaks, 40 s to 50 s | Pass: once, at 80 s, 30 s after it spoke |
| `a-talk-past-the-wait-gives-none` | Low at 42 s, the lamp speaking until 110 s | Pass: nothing, past the minute it waits |
| `once-in-twenty-minutes` | Glad at 40 s, 10 min and 21 min 40 s | Pass: at 40 s and 1300 s, not at 10 min |
| `two-an-hour` | Glad, lively and low, 5 minutes apart | Pass: the first two; the low gets nothing |
| `quiet-at-night` | Glad from 22:00 | Pass: nothing |
| `asleep-gives-none` | Glad with Mood asleep (by hand) | Pass: nothing |
| `head-down-gives-none` | Glad while the person is head down | Pass: nothing |
| `stale-mood-gives-none` | Glad at 20 s, held back, Mood silent after 27 s | Pass: nothing |
| `a-bang-or-the-air-gives-nothing` | Energy up 0.25 and down 0.08, no `felt` | Pass: nothing |
| `nobody-in-view-is-not-standing` | Glad from a touch, nobody in view | Pass: nothing |
| `waits-for-the-lamp-to-stand` | Glad at 40 s, a face from 45 s | Pass: once, at 55 s |

The moves have no case of their own: whether each reads as glad, lively or
low, small and natural, is for the lamp check to judge.
