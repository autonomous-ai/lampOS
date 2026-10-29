# Head pat wiggle

When a hand that petted the lamp's head lifts and the head's pads stay free
1 s, the base gives a small, quick, happy side-to-side, every pet, at most once in 4 s (#3997, Part of #4133).
See `SKILL.md` for what it does and `tests/README.md` for its cases.

- **Goes with Head pat**, which says the line (at most once in 10 s). Its own
  skill because a skill's `[[while]]` conditions hold for all its triggers:
  the line waits for a quiet voice, the wiggle does not.
- **Needs** `LAMP_HEAD_PET=on` in `lamp-controls` (docs/controls.md, "A
  pet"): without it `controls/pet` is never said and this skill never runs.
- **Installable, not in the product** (`install::NOT_PRODUCT_SKILLS`) until
  its lamp check, with Head pat.
- **When:** two triggers. `pet_ended`, the pet's `ended` message (the hand
  lifted), opens the chance and does nothing itself; `pads_free`,
  `controls/state`'s `head_active` false for 1 s counted from it (`follows`,
  within 10 s, once a pet), takes it; `cooldown = "4s"`. Never during the
  pet, nor under a hand back within that second for the next: a move under
  a hand fights the person and can trip the stall and give-way rule
  (#4056), and the body stops nothing for a palm on the pads, which pushes
  no joint (the Reviewer-lead on #4324). A pet that comes back within the
  second gets none of its own; its end opens the chance again. A hand back
  later than that, while the 3.1 s move still turns, still meets it.
  `controls/state` must say `ready` (a pad not read is not a free one).
- **The move** is `moves/pat-wiggle.json`: base yaw only, 0 → +47 → -64 →
  +47 counts, quick and eased in (an envelope of √sin over 1.5 cycles of a
  sine), then a softer sway → +24 → -20 → 0, cosine-eased: 42 frames 75 ms
  apart, about 3.1 s. Head pat's line begins about 2.5 s after the pet (the
  voice makes its audio in about 1.6 s); the wiggle, begun about 1.3 s after
  the lift (1 s free and the body's 280–350 ms), turns through it. At most
  64 counts (about 5.6°, at 11.4 counts a degree), under half of
  `lamp_body::moves::FARTHEST`'s 150 for the base, and at most about 370
  counts a second, under `FASTEST`'s 1,000. Checked at load by the body's own
  `moves::check` (test `head_pat_wiggle_is_small_quick_and_the_bases_alone`).
  The body's `happy` clip is not used: 4 s of the whole lamp near every cap.
- **When it stays still:** asleep (and the pet that woke it), an onboarding
  step pending, quiet hours (22:00 to 07:00, or the time not known), a hand
  pushing the body (`body/hand`). The body plays it as a gesture: only
  standing, and not while held, paused, leased or busy with another.
- **Tune** the size in the move file (the test holds it to half the caps) and
  how often in `cooldown`; `lamp-skills test` on this folder runs its cases.
- **Latency:** sent to the body's `mood.sock` 1 s after the `ended` message
  (the pads free that long, read from `controls/state` every 100 ms); the
  body began a clip 280–350 ms after a send on a standing lamp (#1199): about
  1.3 s from the lift to the base moving.
  To be measured on lamp-06c1 from the lift to the base moving.
