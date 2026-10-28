---
name: onboarding
description: Meet the lamp through short invitations, its physical privacy switch, calling it by name from the person's usual seat, and its head giving way to a hand.
---

The core onboarding skill invites; the system runner verifies physical evidence
and keeps progress. It never changes microphone or camera permission.

Its `skill.toml` contains the nine-step `order`, every first and re-offer
note, the re-offer interval, optional nudge notes and answer counts, and the
coaching and outcome notes under `[onboarding]`. The shipped notes and order
match the previous runner word for word. Voice checks and snapshots this data
once before audio opens; mute/reopen and repeated walks reuse it. A later
skill-only update or repair needs an operator-managed Voice restart. Missing
or invalid data disables only the walk until that restart, with no built-in
fallback; physical privacy and storage handling continue. See
[`docs/onboarding.md`](../../docs/onboarding.md#installed-walk-data) for the
schema, byte bounds, loading rules and activation boundary.

For privacy, the runner supplies the verified device profile's switch location.
If the location is missing, the step is unavailable: never guess where it is.
The invitation is one short sentence the runner gives, about 3-5 s spoken
("Try my privacy switch <location>: close it, then open it again."): say only
that. The person may close the switch while it is being said; that counts and
the switch stops the lamp's words. Never promise a spoken reply while it is
closed. The runner, not a model report, verifies close and reopen. Opening after five
minutes expires the attempt; it is not successful evidence. Other steps keep
the runner's 90-second timeout after 20 seconds of quiet. Skipping and redoing
remain the runner's existing tools, not new functions of this skill.

The runner also ends "who you are" on a name heard and a yes after it, and
"preferences" on both answers heard; call `setup_step` all the same, as the
notes say. A report for a step the runner has already ended changes nothing.

For how they sound, each invitation is one prompt, said exactly as the runner
words it: the first names the phrase to say from their usual seat ("Hey Lamp")
and that it asks three times; each later one answers the last call ("Got it.
Once more." or "I didn't catch that. …"). The phrase is never the prompt's
last words, so the lamp's own voice is not taken for the call. The runner, not a model report, judges each call
from the name heard and the room microphone's rise; it keeps only those numbers,
never sound. At most five prompts are asked; the model can only skip the step.

For giving way, the invitation asks the person to take the lamp's head gently,
move it a little and let go. The runner, not a model report, judges it from the
body's own word that a hand moved the head, the body held it there and the
hand let go. The skill asks nothing of the body; the model can only skip it.

Consent is a code-checked boundary in the editable notes. Keep the exact
`only after a clear yes call remember_my_face` clause in both `who_you_are`
notes, and `done only after a clear yes` in both `room_sounds` and both
`face_expressions` notes. Voice
and Skills refuse loading or packaging if any of these clauses changes.
