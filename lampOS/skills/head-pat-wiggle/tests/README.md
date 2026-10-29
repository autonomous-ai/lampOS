# Head pat wiggle's tests

**How they were made.** Not on a lamp: no lamp says `controls/pet` yet
(#3997, `LAMP_HEAD_PET` is off everywhere). Each case's `.jsonl` files were
written by Head pat's script, in the format `lamp-bus record` writes:
`controls/pet` as Controls says a pet (told apart, then every 100 ms, each
good for 500 ms, then `ended`; 2 s long unless said); `voice/state` once a
second; `expression/state` every 400 ms, each good for 500 ms, with
`asleep`; and, where a case needs it, `onboarding/pending` every 500 ms and
`body/hand`; `controls/state` every 100 ms, good for 300 ms, ready, the
head's pads touched from each pet's contact start (1.2 s before it is told
apart) to its end. Each `expect.toml` sets the local time of the case's first
message (`clock`). Re-recording a case on lamp-06c1, at its lamp check,
replaces the written one.

**Run** with `lamp-skills test lampOS/skills/head-pat-wiggle` (or `cargo
test -p lamp-skills`, which runs every skill's cases). The move itself is
held to the body's check and to half its caps by
`head_pat_wiggle_is_small_quick_and_the_bases_alone`.

| Case | What it holds | Result (lamp-skills, #3997) |
|---|---|---|
| `a-pet-end-gives-a-wiggle` | Awake; a pet from 20 s to 22 s | Pass: one wiggle at 24 s, 2 s after the lift, none during the pet |
| `every-pet-gets-a-wiggle` | A pet every 6 s from 20 s to 92 s, the pads free 2.8 s between | Pass: 13 wiggles, 2 s after each end, 24 s to 96 s |
| `quick-pets-wiggle-once-the-hand-stays-off` | 1 s pets every 1.5 s, ending 21 s to 36 s, the pads free 0.3 s between | Pass: none under the hand coming back (#4324), one at 38 s |
| `a-pet-back-within-two-seconds-gets-none` | A pet ends at 22 s, the next touches the pads at 23.5 s and ends at 26.7 s | Pass: none for the first, one at 28.7 s |
| `a-long-pet-wiggles-once-as-it-ends` | One pet from 20 s to 90 s | Pass: one wiggle, at 92 s |
| `nothing-while-the-hand-stays` | A pet with no `ended` (its last message runs out) | Pass: nothing |
| `never-while-asleep` | Asleep all through, a pet | Pass: nothing |
| `not-for-the-pet-that-woke-it` | Asleep until 19 s, a pet from 19.2 s; another from 90 s | Pass: nothing for the first, a wiggle at 94 s |
| `never-during-setup` | An onboarding step pending all through, a pet | Pass: nothing |
| `a-lamp-done-with-setup-wiggles` | Setup done: nothing pending and `walk` false, a pet | Pass: a wiggle at 24 s |
| `never-in-a-setup-step-without-an-invitation` | The walk under way (`walk` true) in a step led by talk, nothing pending, a pet | Pass: nothing |
| `never-in-quiet-hours` | From 23:00, a pet | Pass: nothing, and nothing kept for the morning |
| `wiggles-while-it-speaks` | Speaking from 15 s to 30 s, a pet from 20 s | Pass: a wiggle at 24 s |
| `never-while-a-hand-pushes-the-body` | `body/hand` pushed from 15 s to 30 s, pets from 20 s and 40 s | Pass: a wiggle at 44 s only |
| `nothing-without-the-switch` | No `controls/pet` at all (`controls/state` as ever) | Pass: nothing |

Each gate was checked to matter: with the trigger's `equals = true`, a
`[[while]]`, `[quiet]`, `cooldown`, the sleep's `for` or `pads_free`'s
`for` changed, the case that holds it fails (`for = "1s"` fails
`a-pet-back-within-two-seconds-gets-none`). `pet_ended`'s `rearm` is kept for
safety: Controls says `ended` once a pet, so no case depends on it.
