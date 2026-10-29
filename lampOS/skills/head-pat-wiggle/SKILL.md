---
name: head-pat-wiggle
description: When a hand that petted the lamp's head lifts and stays off a second, the lamp gives a small, quick, happy wiggle of its base, every pet, at most once in 4 seconds. Never while asleep, during setup or in quiet hours.
---

# Head pat wiggle

A pet is a hand sliding left and right across the lamp's head (docs/controls.md,
"A pet"). Controls says it on `controls/pet`, only with `LAMP_HEAD_PET=on`,
and says `ended` when the hand lifts.

When the hand lifts and stays off the head a second, the lamp is pleased: its base gives a small, quick
side-to-side, a little left, right and left again, then a softer sway, eased
in and out, in about 3 seconds, back where it began, still going as it
speaks. The head stays still. It is a few
degrees, a wag, not a dance.

It wiggles after every pet, never during one, nor when the hand comes back
within that second for the next (moving under a hand would push against
it; that pet's own end brings the wiggle), and at most once in 4 seconds, so petting on and
on is not motion on and on. Head pat's spoken line comes with the pet's
start, at most once in 10 seconds, so a pet always gets the wiggle and a
line at most every 10 seconds.

It stays still while the lamp sleeps (and for a pet that woke it), while an
onboarding step is pending, in quiet hours (22:00 to 07:00, or when the time
is not known), and while a hand pushes the body. It may wiggle while the lamp
speaks. The body itself plays it only standing, and drops it while it is held,
paused or busy with another gesture.

If asked about it: "When you pet my head, I wiggle a little. It means I
liked it."
