# Head pat's tests

**How they were made.** Not on a lamp: no lamp says `controls/pet` yet
(#3997, `LAMP_HEAD_PET` is off everywhere). Each case's `.jsonl` files were
written by a script in the format `lamp-bus record` writes: `controls/pet`
as Controls says a pet (told apart, then every 100 ms, each good for
500 ms, then `ended`); `voice/state` once a second; `expression/state`
every 400 ms, each good for 500 ms, with `asleep`; and, where a case needs
it, `onboarding/pending` every 500 ms. Each `expect.toml` sets the local time
of the case's first message (`clock`) and takes any of the seven lines, as
one is picked at each run. Re-recording a case on lamp-06c1, at its lamp
check, replaces the written one.

**Run** with `lamp-skills test lampOS/skills/head-pat` (or `cargo test
-p lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result (lamp-skills, #3997) |
|---|---|---|
| `a-pet-gets-one-line` | Awake and resting; a pet at 20 s | Pass: one line at 20 s |
| `at-most-once-a-minute` | Pets at 20 s, 50 s and 90 s | Pass: lines at 20 s and 90 s |
| `a-long-pet-gets-one-line` | One pet going on for 70 s | Pass: one line, at its start (`rearm`) |
| `never-while-asleep` | Asleep all through, a pet | Pass: nothing |
| `not-for-the-pet-that-woke-it` | Asleep until 19 s (the kept hand's wake), the pet told apart at 19.2 s; another pet at 90 s | Pass: nothing for the first, a line at 90 s |
| `never-during-setup` | An onboarding step pending all through, a pet | Pass: nothing |
| `a-lamp-done-with-setup-answers` | Onboarding says nothing pending, a pet | Pass: one line |
| `never-in-quiet-hours` | From 23:00, a pet | Pass: nothing, and nothing kept for the morning |
| `never-while-it-thinks` | Thinking from 15 s to 30 s, a pet at 20 s | Pass: nothing, and not later |
| `never-while-it-speaks` | Speaking from 15 s to 30 s, a pet at 20 s | Pass: nothing, and not later |
| `not-for-a-pet-that-cut-its-words` | Speaking until 19 s, when the pet's touch stopped it; the pet at 20.2 s | Pass: nothing, within 10 s of its words |
| `nothing-without-the-switch` | No `controls/pet` at all | Pass: nothing |

Each gate was checked to matter: with its `[[while]]`, `[quiet]`,
`cooldown` or `rearm` taken out, the case that holds it fails.
