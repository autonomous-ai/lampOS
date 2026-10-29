# Head pat

A hand petting the lamp's head gets a short response, at most once in 10 s
(#3997, Part of #4133). The base's happy wiggle 2 s after the hand lifts, every
pet, at most once in 4 s, is the skill beside it, `head-pat-wiggle`: its
own skill since a skill's `[[while]]` conditions hold for all its triggers,
and the wiggle need not wait for a quiet voice. See `SKILL.md` for what it
does and `tests/README.md` for its cases.

- **Needs** `LAMP_HEAD_PET=on` in `lamp-controls` (docs/controls.md, "A
  pet"), on in the product: without it `controls/pet` is never said and this
  skill never runs.
- **Core, in the product** (`install::PRODUCT_SKILLS`) since #4133's design
  owner's yes after the lamp check on lamp-06c1 (29 Oct).
- **The response is chosen in `skill.toml`**, not in code: keep one of its
  three `[[do]]` steps.
  - A spoken line (the default): `say = { text = [...] }`, one of the lines
    at random but never the one said last, said as written through the
    voice's say service. Change the words freely, 2 to 16 different lines.
  - **How often** is `cooldown` in `[trigger]`: 10 s since 29 Oct (15 s
    after the lamp check on lamp-06c1, where a second pet within the minute
    got nothing and read as "pet not working"; then 10 s, with the wiggle
    answering every pet); the design owner and the growth lead tune it here.
  - A non-word giggle: put `sounds/giggle.wav` here (24 kHz mono 16-bit, at
    most 3 s, quiet: the voice's own sound check) and use `sound = "giggle"`.
  - A purr with light: `cue = "hum"` with a warm breathing light; add
    `light = true` to `[permissions]`.
  `lamp-skills test` on this folder after a change runs its cases; a changed
  response changes what the `say` cases expect.
- **When it stays silent:** asleep (and the pet that woke it), the whole setup
  walk (`onboarding/pending`: an invitation or `walk`, #4367), quiet hours (22:00 to 07:00, or the time not known), the
  voice listening, thinking or speaking or within 3 s of it. A pet held
  back is let go, never answered late.
- **Latency:** the line is a say at priority 8 (a second's quiet since the
  last turn) that expires if it can't start within 3 s, so it is said at
  once or not at all, never late (lamp-voice's
  `a_pets_line_is_said_at_once_or_dropped_never_late`); a spoken text costs
  about a second of model time. To be measured on lamp-06c1 from the
  `controls/pet` message to the first sound.
