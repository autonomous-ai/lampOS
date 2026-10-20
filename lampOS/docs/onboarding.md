# The first five minutes

`crates/voice/src/onboarding.rs`, #841. Status on 2026-10-15: built behind
a switch, host tests only. Not yet tried on a lamp or with a person.

## What it is for

A new lamp knows nothing: not where its person sits, not their name or
face, not what they would rather it did. On lamp-4ace a person had to guide
it for twenty minutes. Here the lamp leads that itself, by voice, right
after setup (#806), one short step at a time. It should feel like meeting a
new creature, not filling in a form.

## The steps

The runner in the voice's coordinator owns the order and the timing. Every
line is the model's: each step begins as a written note in place of speech
(`ProviderInput::Written`, as the hello in [speaking-first.md](speaking-first.md)),
and only when the lamp is at rest (nothing heard, nothing owed, a provider
ready, not paused) and a face is in front of it.

| # | Step | The lamp | Ends when | Kept |
|---|---|---|---|---|
| 1 | `where_you_sit` | asks them to sit where they usually sit and look at it | the eyes' largest fresh face (`humans/faces/tracked`) is at least 0.14 of the picture's height with its centre within 0.25 of the middle, held 3 s while the head is still and no hand is on the lamp (every head joint, `base_yaw`, `head_pitch`, `head_roll` on `body/joint_states`, within 6 counts of its goal, slower than 25 counts a second, and the same goals as the last reading; `body/hand` not pushed; each reading fresh, within 0.7 s; any motion, or a reading stale or missing once there is any sign of a body, starts the 3 s again, and a hand not freshly released counts as still there. Any sign stays: either topic present at all (fresh, stale, unparseable, or a read that failed) or any `body/` topic or declaration on the bus. Only a lamp with no body at all goes without that gate): done, and the lamp says so. Short of that, after 8 s of quiet, one note says what is missing: no face, "a bit closer", or "turn me a little toward you". After 60 s: failed, and the lamp says they can turn it toward their seat later or redo the setup | the framing (size, centre) |
| 2 | `who_you_are` | asks what to call them; after a clear yes, `remember_my_face` ([eyes.md](eyes.md)) | the model calls `setup_step` | the face in the eyes' people file |
| 3 | `how_you_sound` | placeholder | — | — |
| 4 | `touch` | placeholder | — | — |
| 5 | `privacy` | placeholder | — | — |
| 6 | `giving_way` | placeholder | — | — |
| 7 | `preferences` | asks about quiet hours and whether it may say hello, one at a time; keeps each as a rule with `keep_rule` (#813) when the provider worker has both `LAMP_MEMORY_DAYS=on` and `LAMP_MEMORY_RULES=on` ([memory.md](memory.md), "What a person teaches and corrects"); without either, or with the legacy memory, there is no `keep_rule` and the model keeps them with `remember` if it has a memory at all and says that is everything | the model calls `setup_step` | the rules, in memory |

A placeholder is the step of a part another builder owns: hearing (fourth,
#753), head touch (third, #711), the privacy switch (third, #795), and
whether the body gives way (fifth, #780). It is recorded as `not_available`
and passed over without a word: four "that isn't ready yet" in a row would
sound like a machine. Each is marked `PLACEHOLDER (#841)` in the code.

A step the model leads ends with `setup_step {step, outcome}`, outcome
`done`, `skipped` (they asked to skip it) or `failed` (it could not be
done; the model then tells them what to try later). Only the step being
led is taken; a report of any other is refused and recorded
(`onboarding_report_refused`). `where_you_sit` is done only by what the
eyes measure: the model may only skip it, and the worker refuses any other
outcome with words it can say. Never reported, a step is
failed after 90 s once the room has been quiet for 20 s, and the walk goes
on. Nobody in front of the lamp for 30 s: the step being led is dropped and
asked again when someone is back. The keeper's pause
([keeper.md](keeper.md)) stops it the same way: nothing is begun while
paused, and the step being led is asked again afterwards.

## The body says it too

A step that ends done asks the body for one move, by the path the answers'
nods take (`LAMP_BODY_MOOD_SOCKET`, `{"gesture":…}`): a nod when it sees them
well where they sit, a greeting when it knows their name and face, a nod
for any later step. Skipped or failed: none. No body, or the keeper's
pause: none, and none is kept for later (`body_gesture_asked` with
`"why":"onboarding"` in the record). The body has no lean yet, so a
greeting stands in for it.

## A step that was not ready, offered later

A step recorded `not_available` stays so in the file. When a later voice
has it, the runner offers just that step once the walk is over, at a rest
with someone there, never in the middle of another step. It is led like
any step to its end. Offered and left unfinished (they walked off, the
pause), it is offered again no sooner than a day later
(`offered_at_unix_s`). Its note is its owner's to word, beginning "one more
thing I can do now"; until then a placeholder note says so.

## Starting and redoing

It starts the first time the voice runs with the switch on and no step kept
yet, and someone is at the lamp. A walk left half done (they walked off,
the voice restarted) goes on at the first step with no outcome.

"Lamp, let's redo the setup": the model calls `redo_setup`, every outcome
is forgotten, and the walk begins again at the next rest. Skipping one
step is `setup_step … skipped`; skipping the rest is that for each step.

## Where it is kept

`LAMP_ONBOARDING_FILE` (JSON, mode 0600, written whole and renamed): each
step's outcome, when, and step 1's framing. Only the coordinator writes it.
The provider worker, which runs the model's functions, appends what the
model reported to a mailbox beside it (`FILE.told`), and the coordinator
takes that in twice a second. Once lamp-settings (#808) is merged this
moves there. Nothing leaves the lamp.

Each step is also in the session record: `onboarding_step` (asked, or its
outcome and framing), `onboarding_guide` (a word on where to move it, with
the face), `onboarding_stopped` (paused or nobody there),
`onboarding_redo` and `onboarding_finished`.

## The switch

`LAMP_ONBOARDING=on` and an absolute `LAMP_ONBOARDING_FILE` in the voice
service's environment. The coordinator passes the file to the provider
worker as `--onboarding-file`, and only then are `setup_step` and
`redo_setup` declared to the model. Without both: no function, no file
read or written, no turn
(`tools::tests::without_the_onboarding_switch_nothing_changes`,
`onboarding::tests::the_switch_is_on_only_with_on_and_an_absolute_file`).
While the lamp is leading a step it does not also say the hello.

### The nudge (`LAMP_ONBOARDING_NUDGE=on`)

Off unless set; off, the walk is as above. Row 10 try 4 on lamp-52e6 (the
second route, DeepSeek) found two things it changes:

- **A step the model ends is nudged once.** The model heard the name in
  lines of 4 and 10 words and called no function: it answered in talk. The
  coordinator tells the walk of each line the person finished (the input
  transcript). Once `who_you_are` has one line with words after its note,
  or `preferences` two (it asks two things), and the step is still open,
  the walk gives one note at the next quiet moment (3 s) to call
  `setup_step` now. The one for `who_you_are` says to wait first if the
  face question is still unanswered. Once per ask; the 90 s fail is as
  before.
- **"Who you are" falls back to the name heard.** Still not reported at a
  quiet moment at least 10 s after that note (so an answer on the face can
  come first), and the first line after the lamp spoke its question was one
  or two words (after an "I'm") with no question, yes, no, greeting or
  thanks: the walk has the model ask "Did I get that right, NAME?", unless
  its face question is still unanswered. The yes, no and greeting words
  are English only. A short yes line keeps
  the step done, as the model's `done` would (the face is kept only by
  `remember_my_face`); a no has it ask once more for the name, and then
  the step goes on as before, failing at 90 s. The name is kept in memory
  only, never in the session record.
- **"Where you sit" takes a steady face after a guide.** The face was seen
  in 39 of 40 looks, but the body's gaze kept nudging the head, so it was
  never still for 3 s. After a guide, any face whose centre stays within
  0.1 each way and its size within 20% of where the hold began, for 3 s,
  is done where it is, its framing kept, whether or not the head is still;
  a hand on the lamp (or one not freshly released) still holds it open. A
  face that moves starts the 3 s again where it is now, and one that never
  stays fails at 60 s as before. Before a guide, the rule is as above.
- **The walk waits for someone near before it asks.** Row 10 try 5 on
  lamp-52e6: with the seat empty, a person walking by in the background
  started `where_you_sit`, and its guides went on. Its first ask now also
  needs a face at least 0.14 high (the hello's near bar; seated, try 5's
  face was 0.14 to 0.18) seen without a break for 2 s. Before that the
  lamp says nothing (`with_the_nudge_the_walk_waits_for_a_near_face_that_stays`).

Each step the model ends also gets one `onboarding_nudge` record when it is
over: `answers` (lines with words), `nudged`, `reported` (the model called
`setup_step`), `name_asked_back` and `name_confirmed`; counts and booleans,
no words.

## Tests

`onboarding::tests`, with a scripted room and face: the steps walked in
order and kept across a restart; a step skipped and the walk redone; a
step that fails saying what to do and moving on; the pause and an empty
room stopping the step being led; the nearest face as its framing.

## Not shown

A lamp run with a person (lamp-52e6, by the coordinator), which is to
measure: seconds per step from asked to outcome, steps redone or skipped,
step 1's guiding notes before the framing held and the face size and
centre reached, whether every line was the model's and short, and the
whole time. Whether the model calls `setup_step` reliably, and whether
"turn me a little toward you" without a side is enough for a person.
