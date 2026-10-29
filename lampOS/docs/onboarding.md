# The first five minutes

`crates/voice/src/onboarding.rs`, #841. The privacy, near-call (#1803) and giving-way (#1800) steps remain behind
the onboarding switch. Device qualification is outstanding; local evidence
and rollout limitations are recorded with each PR.


> **PARKED (#3702), 28 Oct:** giving way is out of the onboarding walk by the owner's call. While `onboarding::giving_way::PARKED` is true, lamp-voice sets the step unavailable (`set_giving_way_available` in coordinator.rs), so it is kept `not_available` and never offered; touch is followed by room sounds. The model is told nothing of it either: `setup_step` and `redo_setup` leave `giving_way` out of their steps and their words, and the skill's `SKILL.md` description no longer names it (lamp-29e7's re-test, 29 Oct 09:39: told of "the body giving way" by `setup_step`'s words, the model asked "push gently against my arm or head, and I'll give way" itself and reported the step skipped; test `a_parked_step_is_never_told_to_the_model`). Its code, words and tests stay, and `skill.toml` keeps it in `order` (an installed lamp-skills reads the same keys); to put it back, set `PARKED` to false and restore the `SKILL.md` description as its PARKED note says.

## What it is for

A new lamp knows nothing: not where its person sits, not their name or
face, not what they would rather it did. On lamp-4ace a person had to guide
it for twenty minutes. Here the lamp leads that itself, by voice, right
after setup (#806), one short step at a time. It should feel like meeting a
new creature, not filling in a form.

## The steps

The installed core skill (`skills/onboarding/skill.toml`) owns the step order,
first and repeat notes, re-offer interval, nudge notes and answer counts, and
completion and coaching notes. The voice's coordinator owns the physical
checks, cancellation and interaction timing. Every line is the model's: each step begins as a written note in place of speech
(`ProviderInput::Written`, as the hello in [speaking-first.md](speaking-first.md)),
and only when the lamp is at rest (nothing heard, nothing owed, a provider
ready, not paused) and a face is in front of it.

| # | Step | The lamp | Ends when | Kept |
|---|---|---|---|---|
| 1 | `where_you_sit` | first says this is a short setup to follow, with other questions after it, then asks them to sit where they usually sit and look at it; that intro is said once a walk, and every later ask of the step (nobody there for 30 s, the pause, a restart mid-step) says only the step's short `reoffer` line. The intro counts as said only when its turn was heard: the coordinator reports how it played, as it does an invitation's, and only whole (or played through with a gap) keeps `opened`; cut, or never said, the next ask says the intro again, and after three such asks it is kept as said anyway, so it is never repeated forever (`onboarding_opener` record, the Reviewer-lead's P3 on #4095). Never said (none of it played), the step is asked again at once, with the intro, while it goes on, as how you sound and giving way offer an unsaid invitation again; cut (someone spoke over it, or a touch stopped it), it is not: the step goes on from what the eyes see, and the intro waits for the step's next ask (`"asked_again"` in the record, jarvis's P3 on #4120). A redo of the whole setup is a new walk, so it opens with the intro again; a redo of this step alone says the short line | the eyes' largest fresh face (`humans/faces/tracked`) is at least 0.14 of the picture's height with its centre within 0.35 of the middle (the lead's chair at about 1 m, row 10 on lamp-52e6 on 21 October, gave size 0.15, x 0.05, y -0.26: a seated person is a little high in a lamp's picture), held 3 s while the head is still and no hand is on the lamp (every head joint, `base_yaw`, `head_pitch`, `head_roll` on `body/joint_states`, within 6 counts of its goal and slower than 25 counts a second, or trailing the body's own move of those goals (within a second of the goal's last change, between where the move began and the goal give or take 6 counts, and not turning away from it): its own gaze ramps the goals while it listens and speaks, and that is the lamp looking, not someone moving it; a joint past the goal, behind where the move began or turning away is moved by something else (8080 on lamp-52e6 failed the step at 60 s in 2 of 5 walks with a good face held, #4034); `body/hand` not pushed; each reading fresh, within 0.7 s; any motion, or a reading stale or missing once there is any sign of a body, starts the 3 s again, and a hand not freshly released counts as still there. Any sign stays: either topic present at all (fresh, stale, unparseable, or a read that failed) or any `body/` topic or declaration on the bus. Only a lamp with no body at all goes without that gate): done, and the lamp says so. While the step is led the voice tells the body it listens between turns (`Onboarding::looking`), so the body's gaze turns the head to the face in small corrections, as a creature looks at someone, before anything is asked of the person. Short of that, after 8 s of quiet, one note (only one, ever) says what is missing: no face, or "move me closer to you" / "turn my head toward you until I'm facing you", each with "or stay as you are". "Turn me" waits until a face big enough has been in view, looked at, for 5 s: the lamp asks to be moved only when its own look still has the person off the middle. After that note any face held 3 s the same way is done where it is, its framing kept: a person sitting beside the lamp (row 10 on lamp-52e6, the face at x 0.12, y -0.63) would just say "there's fine". After 60 s: failed, and the lamp says they can turn it toward their seat later or redo the setup | the framing (size, centre) |
| 2 | `who_you_are` | asks what to call them; after a clear yes, `remember_my_face` ([eyes.md](eyes.md)) when the voice has it (`LAMP_EYES_WHO_SOCKET`, and eyes that recognise); without it, the name only, never `remember` | a name heard ("call me …", "my name is …", "I'm …", "I am …", "it's …", or a name alone, one or two words and no question) and then a yes ("yes", "that's fine", …, and "no problem", "I don't mind", "of course", "go ahead", whose no word is no no, when the phrase leads the line or the line is four words or fewer, so "I'll go ahead and sit down" is no yes; #3883, a tester's 7071 lamp took 5 minutes on 27 October) heard after it: the walk ends the step itself, but only on a lamp that keeps no face (no `LAMP_EYES_WHO_SOCKET`); with a face to keep, that yes is most often the face question's, and the step ends when the face is saved (below), by the model's report, or 15 s after the latest yes with no face saved, once the room has been quiet 3 s (`FACE_SAVE_WAIT_US`, timed from the latest yes since the first is often the name's "Marie, right?"; `START_QUIET_US`: a refused save, the eyes away), done with the name only; with a face to keep, a no after the name never ends the step or takes the name back: like a yes it restarts that 15 s wait, so a no to "Marie, right?" waits for the new name (a line that names someone, "No, it's Mary.", corrects it), and the face's no, of any length, ends it done with the name only once the wait and the quiet are over (lamp-29e7, 28 Oct, ended on the yes with no face kept; the Reviewer-lead's P2s on #3992) (`onboarding_heard`; row 10 on lamp-52e6, 21 October: named and agreed, never reported). The word after "I'm" and the others is no name when it is fine, good, okay, ok, not, sure, here, ready, sorry, just, back, busy, tired, the lamp's name ("lamp") or a yes or no word; a name alone also never is what, huh, hmm, um, uh, hey, hi or hello. Without a face to keep, a no after a name takes it back. The model's `setup_step` call is a second way in, as soon as the name (and the face, if asked) is settled. Each `setup_step` call is a journal line with its step, outcome and answer ("lamp-voice: onboarding: setup_step step … outcome …: noted" or "refused (…)"), and a report the walk does not take says which step is led; a name heard and a no that takes it back are journal lines too, without the name (#3883). A face `remember_my_face` saves while this step is led is a third: the eyes' `ok` ends it as done through the same mailbox, so a chat that saves the face never leaves the step without an outcome (#3740, a tester's 7071 lamp on 27 October); a refused save changes nothing. A face saved before this step is asked (while an earlier step is led, as on that 7071 lamp, where the name was told and the face kept while `where_you_sit` was asked) ends it as done when it comes up, without asking it, and the record says `"before_asked": true` (#3640). That is the evidence the HOLDs on #3640 asked for: no heard words do it, so a short answer to an earlier step ("Like this", "My desk", "I'm sitting") never counts as a name. The walk keeps only a flag, in memory, never the name; a face `forget_my_face` forgets, or a "forget everything" that forgot the face too, a redo of the walk or of this step, or a voice restart lets it go, and the step is then asked as before. Nobody is known yet in this step, so every save there is the room's: a visitor's "remember my face" during it ends the step too, which the scope can't tell apart (the Reviewer-lead on #3746). The walk's record names it (`onboarding_face_saved`), and with the nudge on its `onboarding_nudge` line says `"reported": false`, since the model made no `setup_step` call. A name heard with no yes is done, not failed, at the 90 s give-up. The name's words are not kept: the walk's record says `"name_kept": false`, as the model's own call keeps none. This reading is for the nudge off: with the nudge on (`LAMP_ONBOARDING_NUDGE`), the step ends only through the nudge's own path, where the name is asked back and kept on a yes (`onboarding_nudge`, `"name_confirmed": true`). The two never both end it (the first builder on #2526) | the face in the eyes' people file, when asked and agreed; and the name saved with the face that ended this step, as `owner` in the onboarding file (#4118: who set the lamp up, named by default with `LAMP_PERSON_NAME_BY_DEFAULT` on a lamp with no typed name), gone on any face forgotten by voice, within 10 s once the eyes no longer know them (the app's face removal too), a whole redo or a reset; otherwise nothing |
| 3 | `how_you_sound` | the core onboarding skill requests a short prompt: from their usual seat, say "Hey Lamp" the way they'd get its attention, a few times, each later prompt answering the last call (#1803, #3702, below); then one to say a sentence from there in their own words | the name part: three accepted calls (each heard by lamp-name at 0.95 or more, or by speech to text as the name alone, #3923) make a bar and its file is written; five prompts can't reach three, or the calls spread over 10 dB: no bar. Then the sentence part, whose outcome is the step's: a line heard with words, said close, its words covering it: done; short of that, coached once ("a little louder" or "a little closer"), then done or failed | the prompts and accepted tries (`rise_db`, `score` when lamp-name heard it, `heard_by`: `name` or `text`); the bar in `/var/lib/lampOS/voice/near-call.json`; the sentence's prompt count only |
| 4 | `touch` | says exactly "OK, next step. Tap the top of my head once, then let go." (see "Exact lines" below), then stays quiet | a head-pad touch from Controls (`controls/touch`, `lamp_msgs/TouchEvent` from `controls`, `source: head`, enabled; `lamp-controls --touch`) after the ask: done. Short of that, after 8 s of quiet, one reminder, "I didn't feel that. Touch the very top of my head with your fingertip, once."; 60 s after the ask with no touch felt: `later`, said with the `later` line (#3996). Either end is said (`touch_done`, or the `later` line) before the next step is asked. The model may only skip it. Not available without the voice's bus (`LAMP_BUS=on`) or the subscription to `controls/touch`; the base strip stays off (#1891) | the outcome only |
| 5 | `privacy` | led first, before step 1, whenever it is available (see "Physical privacy step" below); the core onboarding skill requests a short invitation naming the verified switch location | an invitation begun, a fresh physical close (even while it is said), the voice's verified microphone cleanup, then a fresh same-source open and normal readiness; with eyes that give receipts on the bus, also their receipt that the camera closed for this close (#1802) | bounded attempt evidence and completion identity |
| 6 | `giving_way` | the core onboarding skill requests a short invitation, exactly "Hold my base with one hand. With the other, tilt my head gently to one side. Let go when I say so."; a second try's is `give_again`, "I didn't feel my head move. …" (#1800, below). A push is answered at once ("That's it, now let go."), and a minute with none a few seconds in is prompted once (#3702) | from the invitation's start to a minute after its playback, the body says a hand pushed it, a head joint moved more than the body's `HAND_MOVED` (15 counts) from where it was asked to be with its goal held there, and then the hand let go: done; two invitations without that: failed if the body felt a push, `later` if it felt none (#3996); no body, or the keeper's pause while it is led: `not_available` | the outcome only |
| 6½ | `room_sounds` | only on a lamp with no privacy slide (`LAMP_PRIVACY_CONTROLS=no-slide`, exactly): asks exactly "May I notice music and laughter in the room? Nothing is recorded. Just say yes or no." (see "Room sounds" below); elsewhere `not_available`, unspoken | the model calls `setup_step`: `done` only after a clear yes, `skipped` for a no; never reported, `failed` after 90 s as any step | a yes alone, as `/var/lib/lampOS/consent/room-sounds.json` |
| 6¾ | `face_expressions` | only with `LAMP_FACE_EXPRESSIONS_ASK=on` (a lamp whose eyes read how a face seems): asks exactly "May I notice when people look happy or upset, so I can answer more warmly? A yes covers everyone at this lamp. I never say it aloud and keep no pictures. Just say yes or no." (see "Face expressions" below); elsewhere `not_available`, unspoken | as `room_sounds`: `done` only after a clear yes, `skipped` for a no, `failed` after 90 s | a yes alone, as `/var/lib/lampOS/consent/face-expressions.json` |
| 7 | `preferences` | asks about quiet hours and whether it may say hello, one at a time; keeps each as a rule with `keep_rule` (#813) when the provider worker has both `LAMP_MEMORY_DAYS=on` and `LAMP_MEMORY_RULES=on` ([memory.md](memory.md), "What a person teaches and corrects"); without either, or with the legacy memory, there is no `keep_rule` and the model keeps them with `remember` if it has a memory at all and says that is everything | both answers heard: the quiet hours (a time, a part of the day but not one right after "good", so "Good evening, Lamp." is a greeting, "after"/"before", or any no word, "no idea" included, which counts as "none") and the hello (a line with "hello", "hellos", "hi" or "greet" and a yes or a no in it, "Yes, say hello", "Please don't greet me"; or a yes or a no once the quiet hours are in, where a line that greets takes only a plain yes, so "That's fine." answers and "Hi, I'm fine." doesn't. A greeting or an opener, "Hello, Lamp.", "Hi, how are you?", is someone saying hello, not an answer: on lamp-52e6 on 27 October "Hello, Lamp." ended the step and the lamp asked again); the walk ends the step itself (`onboarding_heard`). Or the model calls `setup_step` as soon as it has heard the second answer, before it says anything else (row 10 on lamp-52e6: asked and answered, never reported) | the rules, in memory |

A placeholder is the step of a part another builder owns. It is recorded as
`not_available` and passed over without a word: repeated "that isn't ready
yet" in a row would
sound like a machine. Each is marked `PLACEHOLDER (#841)` in the code.

A step the model leads ends with `setup_step {step, outcome}`, outcome
`done`, `skipped` (they asked to skip it) or `failed` (it could not be
done; the model then tells them what to try later). Only the step being
led is taken; a report of any other is refused and recorded
(`onboarding_report_refused`). `where_you_sit` is done only by what the
eyes measure, `touch` only by what Controls feels, and `how_you_sound`,
`privacy` and `giving_way` only by their measured evidence: the model may
only skip them, and the worker refuses any other
outcome with words it can say. `setup_step`'s words also tell the model
that the runner asks each step in its own words and counts every try: it
never names, announces, explains or asks a step itself, and never counts
calls (lamp-29e7's re-test, 29 Oct: with the runner's calls unheard, the
model said "That's twice now", "that's three", and led a "touch" step of
its own while how you sound was still led). Never reported, a step is
failed after 90 s once the room has been quiet for 20 s, and the walk goes
on.

**`later`: a step nobody tried (#3996, the first builder's yes).** A first-time
person who says nothing usually didn't understand, so a step that ends with
**no attempt** is `later`, not `failed`, and the lamp says one exact line,
the skill's `later`: "No problem, we'll try that one later." `failed` stays
for a step that was tried and didn't work, and still says what to try. Only
the voice's evidence decides it, never the model (`setup_step` takes no
`later`):

| Step | No attempt means |
|---|---|
| `who_you_are`, `preferences` | no line with words heard in any of its asks (`Onboarding::heard`, `heard_line`) |
| `how_you_sound` | no name heard in any call prompt and no sentence line, and no call already kept |
| `touch` | no touch felt: its 60 s give-up is now always `later` |
| `giving_way` | no push said by the body (`body/hand` pushed) in either try, while the step is led, inside a try's minute or not |
| `privacy` | no switch close seen in either invitation (it was `skipped`) |

`where_you_sit` is unchanged (a face held in view is an attempt), and so are
`room_sounds` and `face_expressions`, where silence is a no that leaves each
off. The attempts seen are kept in `progress.json`'s `tried` (step names, until
the step ends), so a voice restarted after a push or words still ends that
step `failed`, never `later`; an older voice ignores the field. **On file, an older build still reads it:** a build from before `later`
has no such outcome, and its storage refuses a file it can't parse, so a
lamp the updater put back (#2810) would never go on with onboarding until a
redo. A `later` step is written as the outcome that build gave such a step
(`skipped` for privacy, `failed` for every other), with `"later": true`
beside it, which that build reads past; this build reads the pair back as
`later` (`onboarding::steps_file`). Its `later_offers` and
`offered_at_unix_s` are read past or defaulted the same way. **Offered again:** once the walk is over, never inside another step,
with someone in front and the room quiet, as a `not_available` step is: first
`later_after_s` (1800 s, the same sitting) after the walk's last outcome, then
once more `reoffer_after_s` after that offer. Each is counted in the step's
`later_offers` in `progress.json`, kept through an offer that ends
`not_available` (an invitation left unsaid). Such a step's next offer, as a
`not_available` step's is, counts too, and past two it is never asked again
(the first builder's P2 on #4012), so the cap holds. The re-offer
opens with one exact line naming the step, the step's `later_reoffer` ("Earlier
we skipped how you call me. Let's try it now.", the first builder's P2 on
#4012), then its `reoffer` note, in one go with no yes or no to judge; a file
without a `later_reoffer` says only the `reoffer`. Its own tries are given back (giving way's two, privacy's
two invitations, and how you sound's prompts). A re-offer that is tried and
done is `done`. One left silent again stays `later` with its count. After two
re-offers it stays `later` for good, so it never nags. "Redo the setup"
still redoes everything. **A `later` step counts as not done** in #3530's rows
and the onboarding quality row. Nobody in front of the lamp for 30 s: the step being led is dropped and
asked again when someone is back. The keeper's pause
([keeper.md](keeper.md)) stops it the same way: nothing is begun while
paused, and the step being led is asked again afterwards.

## Installed walk data

Voice reads `/var/lib/lampOS/skills/onboarding/skill.toml` once when its service
invocation begins, before opening audio. The same immutable definition is used
by every walk and mute/reopen cycle in that invocation; conversation turns do
not read the file. A later skill-only install, edit, or restored missing file
needs an operator-managed Voice restart to activate it. The updater installs
product skill files before restarting changed binaries, so the initial binary
and data rollout has the data in place first; a skill-only update deliberately
does not restart Voice.

The manifest must be a regular file, not a link, at most 65,536 bytes; a FIFO
never waits for a writer. It must be enabled and `kind = "core"`, with the
`onboarding` core grant and checked `[onboarding]` data. Missing, unreadable,
disabled or invalid data disables this service's automatic walk and invitations and emits one fixed
journal diagnostic. There is no built-in word or order fallback. Physical
privacy waiting, microphone cleanup and the onboarding storage owner still
run. Existing `setup_step` and `redo_setup` provider tools still follow
`LAMP_ONBOARDING` and its file setting; they are not removed by this failure.
Repair the file and restart Voice to recover the automatic walk.

`order` names each of the nine stable step IDs exactly once; `steps` provides
`first` and `reoffer` notes for every ID. `reoffer_after_s` is 1 to 31,536,000
(the shipped value is 86,400), and so is `later_after_s` (shipped 1,800). Only `who_you_are` and `preferences` may have a
`nudge`, with `words` and `after_answers` from 1 to 8. `where_you_sit`,
`how_you_sound`, `touch` and `giving_way`, and no other step, may have a
`skipped` line (#3769, "Exact lines" below); without one, as in an older
updater's built-in file, the skip is said by nobody. Every note is 1 to 900
bytes with no control characters. Only the privacy step's notes have one
`{location}` slot, and only `words.who_confirm` has one `{name}` slot; other
notes have no slots. Consent remains a code-checked mechanism: both `first`
and `reoffer` for `who_you_are` must retain the exact clause
`only after a clear yes call remember_my_face`; both notes for `room_sounds`
and for `face_expressions` must retain `done only after a clear yes`. Voice's installed-file loader and
Skills loading/packaging reject a missing or altered clause. Surrounding
words remain editable. The shipped notes are unchanged. The privacy notes
fit a 1,024-byte written turn with the
verified location's maximum 160 Unicode characters. An expanded name
confirmation over 1,024 bytes is omitted, without cutting the name or claiming
it was confirmed. Filled data is never interpreted as another slot.

`[onboarding.words]` requires `sit_no_face`, `sit_closer`, `sit_turn`,
`sit_done`, `sit_failed`, `who_again`, `touch_remind`, `touch_done`,
`touch_failed`, `later`, `privacy_done`,
`privacy_done_camera`, `privacy_failed`, `near_again`, `near_missed`, `near_done`,
`near_failed`, `sentence_first`, `sentence_louder`, `sentence_closer`,
`sentence_again`, `sentence_done`, `sentence_failed`, `give_again`,
`give_done`, `give_failed` and `who_confirm`. Unknown or missing data fields are refused,
except #3769's `touch_done`, `touch_failed` and `give_again`, #3702's `near_missed`, and #3996's `later` and
`later_after_s` (1800 s without it), which are optional: an
older updater's `install` writes its own built-in onboarding file without them,
and the walk still runs, saying what it said before (no line at a touch's end,
the first invitation again on a second try; Reviewer-two's P2 on #3781).

Skills uses the same validator when loading or packaging this core skill.
Changing order or words cannot add a mechanism, grant physical authority,
remove privacy's priority or change proof, timeout and cancellation rules.
The shipped data reproduces the previous order and notes byte for byte,
except the measured steps' lines, which are exact sentences since #3769
("Exact lines" below); `onboarding/legacy_words.rs` is a test-only oracle,
absent from production.

### Exact lines (#3769)

Where you sit, how they sound (the call and the sentence), touch, privacy's
result and giving way say lines written once in the skill, each note
`(Say only this, nothing before or after it: "…")`, so the model never words
them itself; the steps the model leads (who you are, room sounds, face
expressions, preferences) give it each line to say exactly, in the step's own
note, so no new skill key is needed. No line says the arm or another step's
words (`no_step_says_another_steps_words`). Each
step has what to do, what to do when it didn't work, and that it worked,
said before the next step's invitation:

| Step | What to do | Didn't work | Worked | Gave up |
|---|---|---|---|---|
| `touch` | `first`: "OK, next step. Tap the top of my head once, then let go." (`reoffer`: "One more thing: …") | `touch_remind`: "I didn't feel that. Touch the very top of my head with your fingertip, once." | `touch_done`: "Got it, I felt that." | `touch_failed`: "I didn't feel a tap this time. That's fine, you can redo the setup later. Just ask me to redo the setup." |
| `where_you_sit` | `first`: "Let's get set up first! It'll only take a few minutes. Please follow my instructions step-by-step, and once we're done, you can ask me anything. Sit where you usually sit and look at me. I'll tell you when I see you."; `reoffer`, any later ask in the walk (the intro is said once a walk, kept as `opened` in the progress file so a restart doesn't replay it; a redo of the whole setup clears it): "Sit where you usually sit and look at me. I'll tell you when I see you."; `later_reoffer`, said in its place on a later visit (the step is never `later`): "One more thing: sit where you usually sit and look at me. I'll tell you when I see you." | `sit_no_face`: "I can't see your face yet. …"; `sit_closer`: "You're a little far. Move me closer to you, or stay as you are."; `sit_turn`: "I can see you at the edge of my view. Turn my head toward you until I'm facing you, or stay as you are." | `sit_done`: "Got it, I can see you well there." | `sit_failed`: "I still can't see you well there. That's fine, you can turn me toward your seat later." |
| `how_you_sound`, the call | `first`/`reoffer`: "Now I'll learn how you call me. When I stop talking, say 'Hey Lamp' from your seat, the way you'd get my attention. I'll ask until I've heard you three times."; `near_again`, the prompt after an accepted call (#3702): "Got it. Say 'Hey Lamp' once more." | `near_missed`, the prompt after a call not caught (#3702): "I didn't catch that. Say 'Hey Lamp' again, from your seat." | `near_done`: "Got it, that's three." | `near_failed`: "I couldn't learn how you call me this time. That's fine, you can redo the setup later. Just ask me to redo the setup." |
| `how_you_sound`, the sentence | `sentence_first`: "Now say any sentence to me, the way you'd talk to me." | `sentence_louder`, `sentence_closer`, `sentence_again`: "Say another sentence from your seat, a little louder." and the like | `sentence_done`: "Got it, I hear you well from there." | `sentence_failed`: "I couldn't hear you well enough this time. I'll get used to your voice as we talk." |
| `privacy`, its result | (its invitation: "Find my privacy switch {location}. Close it, then open it again.") | (a second invitation) | `privacy_done`: "Got it, the switch closes my microphones." (`privacy_done_camera`: "… and my camera.") | `privacy_failed`: "The switch check didn't finish. You can try it again later by redoing the setup. Just ask me to redo the setup." |
| `giving_way` | `first`/`reoffer`: "Hold my base with one hand. With the other, tilt my head gently to one side. Let go when I say so."; `give_felt`, at once on a try's first push: "That's it, now let go." | `give_again`, the second try's invitation within the walk (a re-offer later opens as the first): "I didn't feel my head move. Hold my base with one hand. With the other, tilt my head gently to one side. Let go when I say so."; `give_prompt`, once, 8 s into a minute with nothing felt: "Hold my base with one hand. With the other, tilt my head gently to one side, then let go." (it ends like `give_more`: `give_felt` plays once a try, so a push after this prompt gets no "now let go") | `give_done`: "Got it, my head moved with your hand."; `give_more`, each push let go too soon: "A little more, then let go." | `give_failed`: "I didn't feel my head move this time. That's fine, you can redo the setup later. Just ask me to redo the setup." |
| `who_you_are`, led by the model | `first`/`reoffer`: "What should I call you?" (then, with `remember_my_face`, "May I remember your face, so I know you next time?") | `who_again`: "Sorry, what should I call you?" | no fixed line: the model greets them by name itself, and a fixed one would be a third greeting (#4151) | skipped or failed: "That's fine. You can tell me your name any time." |
| `room_sounds`, led by the model | `first`/`reoffer`: "May I notice music and laughter in the room? Nothing is recorded. Just say yes or no." | an answer it can't tell, asked once: "Sorry, was that a yes or a no?" | after its `setup_step`: "Got it, I'll notice music and laughter." after a yes, "Okay, I won't." otherwise | (90 s, `failed`, unsaid) |
| `face_expressions`, led by the model | `first`/`reoffer`: "May I notice when people look happy or upset, …? … Just say yes or no." | as `room_sounds`: "Sorry, was that a yes or a no?" | after its `setup_step`: "Got it, I'll notice how people look." after a yes, "Okay, I won't." otherwise | (90 s, `failed`, unsaid) |
| `preferences`, led by the model | `first`/`reoffer`: "Are there hours when I should keep quiet? …", then "May I say hello when you sit down?" | an answer it can't tell, asked once: "Sorry, could you say that again?" | after its `setup_step`: "Got it, that's everything. You can ask me to redo the setup any time." | (90 s, `failed`, unsaid) |

Giving way's lines never mention sleep: the step never puts the lamp to
sleep (#3747).

**A fixed line is said even when the chat is late.** These lines still go to
the model as a written turn, and on the second route a turn whose chat has
not begun in time used to end unanswered, the walk's line unsaid (lamp-52e6,
28 Oct 16:42, `how_you_sound`: three lines ended `chat round 1 had not begun
in 3000 ms; the turn ends unanswered`). Now a written turn whose note is the
fixed form, `(Say only this, nothing before or after it: "…")` with nothing
after the quote but `)` or a trailer that only forbids (` Do not …)`,
privacy's), says the quoted words as they are through the route's own speech
when its first round has not begun within the bound (journal: `the
onboarding line's own words are said`; `onboarding::fixed_words`), or when
that round fails before any word, whatever the error (lamp-52e6, 29 Oct
07:35: the model service out of credit answered HTTP 402 in 0.9 s, and
every `how_you_sound` prompt ended with nothing said; journal: `chat round 1
failed; the onboarding line's own words are said`). It is an
ordinary spoken reply to the coordinator, so the invitation's delivery
(`Whole`, `Gapped`, `Cut`, `Unsaid`), the yield to a person speaking and
the turn's bookkeeping are as for any reply. A line that asks the model to do
more (`who_you_are`, preferences, a confirm: "Ask exactly this … When they
have answered …") keeps the model path and ends unanswered (or failed) as before; so do
a person's line, a look and an add-on's quoted line.

The conversational steps keep their tool calls and consent clauses, but say
their questions exactly: `who_you_are` asks "What should I call you?", then
"May I remember your face, so I know you next time?", and on a skip or a
failure says only "That's fine. You can tell me your name any time."
(`who_again`: "Sorry, what should I call you?"). `preferences` asks "Are there
hours when I should keep quiet? For example, 'after ten at night', or 'no'.", then "May I say hello when you sit down?",
and ends with "Got it, that's everything. You can ask me to redo the setup any
time." `room_sounds` and `face_expressions`
ask their questions exactly too (see the step table), each ending "Just say yes
or no.", and keep their consent clauses.

**A skip is said from the record.** The model may only skip `where_you_sit`,
`how_you_sound`, `touch` and `giving_way`. For those four, the walk itself says
the step's `skipped` line once the skip is in the record, before the next
step, and `setup_step`'s answer tells the model to say nothing about it
(`{"ok": true, "note": "I say that it is skipped myself. …"}`). So the step
named is the step skipped: on lamp-29e7 on 27 October the model said "I've
skipped touch" when the record had skipped how they sound. The lines are "Okay,
we'll skip where you sit for now.", "Okay, we'll skip how you call me for
now.", "Okay, we'll skip the touch for now." and "Okay, we'll skip my head
giving way for now." A privacy skip keeps `privacy_failed`. The
conversational steps' skips stay the model's, as their notes ask.
Host targets are under 50 ms for the startup read and validation and under
1 ms for a cached note selection. These are local boundaries, not spoken or
lamp latency measurements; the unchanged walk still needs its lamp run.

## Room sounds (#1722 item 1)

On a lamp with no privacy slide, music and laughter (`ears/classes`,
#1669) are off until the person says yes; on a slide lamp they stay on
behind the switch, as built, and this step is never offered. The voice
reads `LAMP_PRIVACY_CONTROLS` once when the walk is made; only the exact
value `no-slide` makes the step available, and all of it stays behind
`LAMP_ONBOARDING` like every step.

The step is asked once. Any kept outcome ends it: a yes (`done`), a no
(`skipped`) or silence (`failed` after 90 s, as any step) is in the
progress file, so a restart, a mute or a rebuilt walk does not ask again.
A no and silence store nothing more: the step's outcome and time only,
as every declined step. Only a yes writes a record for lamp-ears, and it
holds the answer and its time, never words or sound.

The record, for lamp-ears (`onboarding/storage.rs`, `room_sounds`):

- Path: `/var/lib/lampOS/consent/room-sounds.json` (`ROOM_SOUNDS_PATH`),
  outside the voice's 0700 folder, which lamp-ears (`lampos`) cannot
  enter. `deploy/product/lampos.conf` makes `/var/lib/lampOS/consent`
  0750 root:lampos.
- Written whole by the onboarding storage worker (temporary file, mode set
  after it is made since the voice's `UMask=0077`, group set, synced,
  renamed, folder synced): 0640 root:lampos, the group's id read from
  `/etc/group` by name at the voice's start. With no `lampos` group the
  voice says so once and writes it 0600 root's, which lamp-ears reads as
  no, the safe side. A write that fails is said once and leaves no file
  (no again); the progress stands.
- Written only by the progress write that first keeps the yes; removed by
  any progress write or start whose `room_sounds` outcome is not `done`.
  It is never written back from the progress, so once removed it stays
  gone until a new yes.
- Shape: `{"version":1,"room_sounds":"yes","at_unix_s":<when the yes was kept>}`.
- Read rule: yes only when the file is a JSON object whose `room_sounds`
  is exactly `"yes"`. Missing, unreadable, unparseable, over 4096 bytes,
  or anything else is no. Read one version ahead: `version` and any other
  field are ignored.

Taking the yes back: it is withdrawn by `redo_setup` (the progress
emptied, the file removed on that write), by a forget of everything
(`forget_everything`, `remember.rs`: whoever asks, the household's yes
goes with their memory, with a journal line naming the file only), and by
the factory reset (the reset plan wipes the file). There is no separate
"stop noticing music" yet.

## Face expressions (#2872)

lamp-eyes reads how the near face seems (`humans/seems`) only with a kept
yes ([privacy-switch.md](privacy-switch.md), "Face reads stay off without a
yes"), on every lamp, slide or not. It is a yes for this lamp, covering
everyone it sees: one file, not one per person, so whoever answers answers
for the household, and the ask says so. With
`LAMP_FACE_EXPRESSIONS_ASK=on` (read once when the walk is made, behind
`LAMP_ONBOARDING`) the walk asks for it once, after room sounds; without it
the step is `not_available` and unspoken. Its words and its place in
the order are the onboarding skill's (`[onboarding.steps.face_expressions]`
in `skills/onboarding/skill.toml`), with the same code-checked
`done only after a clear yes` clause as room sounds; the switch is code.

Everything else is the room sounds step's, above: asked once, any kept
outcome ends it, only a yes writes a record, and `redo_setup`, a forget of
everything and the factory reset remove it. The record is
`/var/lib/lampOS/consent/face-expressions.json` (`FACE_EXPRESSIONS_PATH`,
beside `room-sounds.json`, written the same way and with the same mode and
group), shaped `{"version":1,"face_expressions":"yes","at_unix_s":<when>}`,
and read by lamp-eyes' `seems::face_expressions_yes` with the same rule:
yes only when `face_expressions` is exactly `"yes"`.

Taking the yes back is as for room sounds: a forget of everything, a redo
of the step (`redo_setup`), or the factory reset. There is no separate
"stop noticing my face" yet.

## Movement during onboarding

Onboarding does not request nods or gestures for placement, identity, touch,
privacy or preferences. Only the giving-way step asks the body for one move, a `nod`, once
it is done, after the hand has let go. The step itself asks the body for
nothing: the body gives way to a hand by its own rule (`--give-way`,
[body.md](body.md)); ordinary safety and physical controls keep their reactions.

## A step that was not ready, offered later

A step recorded `not_available` stays so in the file. When a later voice
has it, the runner offers just that step once the walk is over, at a rest
with someone there, never in the middle of another step. It is led like
any step to its end. Offered and left unfinished (they walked off, the
pause), it is offered again no sooner than a day later
(`offered_at_unix_s`), using the skill's `reoffer_after_s` (one day in the
shipped data). Its note is that step's `reoffer` in the installed skill;
the shipped touch re-offer ("One more thing: …") introduces the newly available ability, and
so does where you sit's, without the setup intro: its `reoffer` is also its short line
within a walk, so a later visit says its `later_reoffer` instead ("One more thing: sit
where you usually sit…", jarvis's P3 on #4095). A new key would be refused by an older
build reading the file; an older build says the short line, as before. Each
other shipped re-offer matches its former invitation.

### While the model service refuses (#4265)

With `LAMP_SECOND_REFUSED_HOLD=on`, while the model service refuses the
lamp's turns (failure-notices.md, "The model service refusing"), the walk
begins nothing while turns are held and the step being led is not failed.
Once a minute the hold lifts and the walk goes on by itself. A step offered
as an invitation (privacy, how_you_sound, where_you_sit's opener) is
re-offered then, that invitation being the minute's try, and the model's
answer to it ends the refusal; only a line said in its own words on a failed
chat (#4263, `OwnWordsSaid`) does not. A step asked as a note (who_you_are,
preferences, touch, where_you_sit's ask) is asked again only after
`STEP_GIVE_UP_US` (90 s) and quiet, `MOST_UNSAID` (two) times at most. A step
that would end `failed` or `not_available` meanwhile is kept as `later`
instead, with one journal line, and offered again only after the skill's
`later_after_s` (30 minutes): a long refusal leaves setup's steps `later`. A
`later` re-offer cut that way gets its count back, and is offered again by
the `later` rules above. Off unless set.

## Why a step is not_available

Each measured step needs these, by switch, checked at every poll of the walk:

- `how_you_sound`: `LAMP_SAY=on` (the say socket made), `LAMP_BUS=on` with
  the bus up when the session starts (the ears' subscriptions to
  `name/heard`, `ears/level` and `voice/state`), and onboarding storage
  started.
- `giving_way`: `LAMP_SAY=on`, `LAMP_BUS=on` with the bus up when the
  session starts (the feel's subscriptions to `body/joint_states` and
  `body/hand`), `LAMP_BODY_MOOD_SOCKET` (the body link), and onboarding
  storage started.
- `touch`: `LAMP_BUS=on` and the walk's own subscription to `controls/touch`;
  it is not part of the record below.

The session record `onboarding_inputs` names which one is false:
`{"kind":"onboarding_inputs","says","bus","ears","body","feel","storage","at_us"}`,
booleans only, no paths, recorded while the walk exists, once at the first
poll and again whenever one of them changes (row 10 on lamp-52e6, where
both steps were `not_available` with every switch set).

## Starting and redoing

It starts the first time the voice runs with the switch on and no step kept
yet, and someone is at the lamp. Each step begins after 3 s of quiet: no voice
the detector calls speech, words or none, no words heard and none of the
lamp's own, as [speaking-first.md](speaking-first.md) ("When") counts it. A
request that heard no words holds it only through its speech, not through
its end's silence or the wait for an answer (on lamp-52e6 on 21 October the
lamp's fan had kept the privacy step from ever being offered). A walk left half done (they walked off,
the voice restarted) goes on at the first step with no outcome.

`redo_setup` names what to do again. "Lamp, let's redo the setup" is
`step: all`: every outcome is forgotten, and the walk begins again at the
next rest, a new walk that opens with the setup intro again. "Can we redo the touch step?" is `step: touch`: only that step's
outcome and its own counts (how you sound's prompts and tries, giving way's
tries) go, and every other step keeps what it ended with. Privacy is never
redone alone; it goes with `all`. A call that names no step, or a step the
walk doesn't know, reopens nothing and tells the model to ask which (on
lamp-06c1 on 28 Oct "redo the touch step" reached a redo with no step and
reopened every step). Skipping one step is `setup_step … skipped`; skipping
the rest is that for each step.

## Where it is kept

`LAMP_ONBOARDING_FILE` (JSON, mode 0600, written whole and renamed): each
step's outcome, when, step 1's framing, and `opened` once "where you sit"
has said its intro this setup/redo and it was heard. Only the coordinator writes it.
The provider worker, which runs the model's functions, appends what the
model reported to a mailbox beside it (`FILE.told`), and the coordinator
takes that in twice a second. Once lamp-settings (#808) is merged this
moves there. Nothing leaves the lamp. A step this build does not know (a
later build's, after a roll-back) is read past and kept as it is: the file
is not refused, and the walk goes on with the steps it knows (#2110).

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
"turn my head toward you until I'm facing you" without a side is enough for a
person.

## Physical privacy step (#841)

Privacy is an explicitly measured step, not a model-reported success. The
`onboarding` core skill, granted `core = ["onboarding"]` in `[permissions]`
(the runtime knows it by that declaration, not its name), reads
`onboarding/pending` and asks the existing say
service for the exact invitation token. The runner retains the words, evidence
and persistence; there is no second conversational runner and no automatic
voice fallback when the skill is absent. Stale, skipped, redone or restarted
attempts cannot borrow another invitation. This bridge adds no model tool and
grants no microphone, camera or body authority.

Privacy is led first, before step 1, whenever it is available, though its
record keeps the table's place. It is available only with a verified switch
location (below) and Controls' privacy receipts, `LAMP_PRIVACY_CONTROLS=on`
(the `privacy_bus.rs` worker): the GPIO
privacy worker sends no receipt, and without one no invitation could ever be
offered, so the step is `not_available` and the walk goes on. The product's
`lamp-voice.service` doesn't set `LAMP_PRIVACY_CONTROLS`.

With the receipt source on, a receipt that never shows the switch open and
fresh (faulted, stale, a reader that stopped) would hold the walk at privacy
with nothing offered. That wait is bounded by the sitting step's give-up time
(60 s, `SIT_GIVE_UP_US`), counted only while nothing is offered or in an
attempt: past it privacy is recorded `not_available`, the journal gets one
`onboarding_stopped` row (`why: no_open_privacy_receipt`) and the walk goes on.
The next open, fresh receipt makes privacy available again and it is offered
then, as privacy always is once available. Never done: the bound adds no
evidence. A restart re-arms the wait.

The runner gives the model two short sentences to say and nothing else,
about 3-5 s spoken: "Find my privacy switch <verified location>. Close it,
then open it again." (`onboarding.steps.privacy` in the skill data; closed =
microphones off, so the words end with the lamp hearing). The attempt is
armed as the invitation's turn begins (its words taken for that turn, the
`onboarding_privacy_armed` row), not when the turn ends: a fresh close from
then on counts, even while the lamp is still saying it. That close mutes the
lamp as any physical mute does, ends the voice cycle with the close as its
candidate and is neither a failed try nor a new offer; the next cycle
completes on the fresh open as below. On the lead's 52e6 run (21 October) the
person closed the switch 13 s after the offer while a longer invitation was
still being said, nothing was armed yet, and the step was offered again and
skipped. Words are given only for a baseline the attempt can arm with
(`privacy::armable`); if arming still fails as the turn begins, the words
already being said spend no try: the same try is offered again once the
switch reads open (an `onboarding_privacy_arm_failed` row), and the offer's
90 s give-up bounds a failure that keeps coming. A close stamped before the
invitation began is not its answer. An
invitation that ends without being heard whole (cut off, a playback gap: the
turn's shared `heard_whole` proof) drops the armed attempt and can be retried
once, then skipped. One of which nothing played at all (`no_audio_answer`, or
a turn that ended unsettled before its first audio: the route failed, the
service stopped, a privacy gap) was never said, so it is no try (#3754). A
failure notice played in place of words that never came is none of the
invitation's audio either (lamp-52e6, 29 Oct 07:35: a chat refused with HTTP
402 played only the notice, and every prompt was taken as cut): it is
offered again once the voice can speak, `MOST_UNSAID` (2) times at most, then
the step is `not_available`, never `failed`, with an
`onboarding_privacy_try` row whose `why` is `"invitation not said"`. Giving
way and how you sound do the same with their own rows. An invitation
played to its end with a gap in its playback and nobody's turn taken over it
(`Delivery::Gapped`: on lamp-52e6 on 28 October the lamp yielded to its own
voice and resumed, every word said, #4035) was heard: privacy keeps its
attempt armed as after a whole one, and how you sound opens its try's window
(below). Giving way, before any push began, takes it as one not said, under
the same `MOST_UNSAID` bound and counter, its row's `why` `"the invitation
was played with a gap"`. A model-led step whose
90 s run out after its conversation ended (20 s of quiet) is asked again too,
twice at most, and fails only after that. Software mute, sleep, an initial open level,
and model `setup_step done` are not evidence of physical close followed by
open.

Controls' existing fence provides the publisher, reader and acquisition
identity through the same privacy worker channel. Close observation only
captures bounded evidence in memory. The outer service persists a close only
after the voice's own revocation and verified teardown. That proves the
microphones closed. The camera (#1802): on every mute, before any record or
onboarding work, the voice tells the eyes `{"camera":false}` (and a voice
cycle that ends any other way says it as it goes, `EyesLink` on drop), so the
camera no longer waits out the eyes' 1.5 s permit timeout. When a close is
taken as evidence and the eyes' `eyes/camera_closed` publisher is alive on
the bus, the voice also sends `{"close_receipt":N}` (N a fresh random token,
kept with the close in `privacy-pending.json` as `camera`). The eyes answer
on `eyes/camera_closed` (`lamp_msgs::CameraClosed`, `{"close":N,"released":
bool}`, kept as the topic's latest) once the capture device's stop has
returned with the camera shut, never while it is open. On reopening, Done
needs that receipt: from the source `eyes`, for this N, `released: true`,
and stamped within 2 s of the ask (`CAMERA_RECEIPT_MS`). Then Done's note
claims the microphones and the camera. A receipt for another close, one
that says the stop faulted, a late one or none makes the step Failed
(`onboarding_privacy_camera`, `not_proven`, with why): the camera was to be
proven and was not. Without eyes configured (`LAMP_EYES_PERMIT_SOCKET`
unset), or without their receipt publisher on the bus (eyes without
`LAMP_BUS=on`, or older eyes), the camera is `not_available` and Done claims
the microphones only, as before. Whether it was asked (`onboarding_privacy_camera`,
`asked` or `not_available`) is written to the session's records by the mute
itself, since the mute ends the cycle at once. Muting never waits for the eyes; only the
proof reads their receipt, and only when the switch opens again. What a
receipt shows is that the eyes' process stopped and closed the V4L2 device
without a fault (STREAMOFF, buffers freed, file closed), not that the
sensor or its light lost power. A close read just before the
invitation began is the person acting early: the attempt is
offered again (within the two-try budget), not failed. The normal no-audio
privacy wait is unchanged. A later voice cycle can complete the step only
with fresh, advancing evidence from the same physical source and its normal
input-readiness gate. Source/clock changes, failed cleanup and missing evidence
leave the step incomplete. An attempt expires after five minutes, evaluated
on reopening; expiry never changes the switch or capture authority.

The product reserves `/var/lib/lampOS/voice/onboarding/` as private state, with
`progress.json` as the canonical progress path. Pending proof and bounded
retry metadata stay beside it. Completion is saved by attempt identity before
pending evidence is erased, so a crash does not repeat an acknowledgment.
A new service invocation discards unfinished close proof, and any
`privacy-pending.json.next-*` or `progress.storage-next` a crash left
mid-write, before its first privacy wait; normal physical-mute cycles within that service retain it.
Software mute or an unreadable source discards prior proof after cleanup. A
failed discard disables onboarding proof for that service while leaving the
normal privacy wait unchanged. Factory reset wipes all contents, including
temporary/mailbox files, and preserves the installed core skill. There is no implicit import from arbitrary
legacy environment paths.

The step also needs `LAMP_ONBOARDING_HARDWARE_PROFILE` naming an absolute path
to a hardware-owner supplied regular JSON file:

```json
{"privacy_switch_location":"<verified location on this hardware>"}
```

This is a schema example, not a location to deploy. No profile is supplied or
guessed by default. The loader rejects symlink files, malformed/unknown fields,
files above 4096 bytes, and empty/control-containing location phrases or
phrases over 160 characters. Missing valid location evidence keeps privacy
unavailable. Read the profile once in storage initialization, not on the interaction loop.
The product unit sets `LAMP_ONBOARDING=on` (`deploy/product/lamp-voice.service`)
but no `LAMP_ONBOARDING_HARDWARE_PROFILE`, so on a product lamp the privacy
step stays unavailable and its lines are never said (#4141).

Immediate feedback remains independent of progress writes. A single storage
worker belongs to the whole service, including its physical-mute cycles;
progress, invitation budget, checkpoint and mailbox operations use its bounded
ordered queue. The interaction loop only submits or polls without waiting.
There is one operation in flight and bounded pending state, not one thread per
write. Late completions cannot restore an invalidated attempt: an epoch fence
hides old results immediately and a durable invalidation runs after any old
write before the tutorial resumes. No worker completion grants capture
authority. A slow or failed store can defer tutorial progress while normal
voice and physical privacy keep running. A new invitation is not published
before its budget commit, and Done is not acknowledged before its matching
progress commit. The provider keeps appending its bounded mailbox messages in
its separate process; the runner reads them only through the storage worker. The target is acknowledgment within
500 ms after accepted proof. Full device latency, slow-storage responsiveness,
and the physical close/open walkthrough remain unverified until measured on an
exclusively leased lamp. Local tests do not establish those hardware results.

Known limit: the say service trusts the skill name a process gives. A process
in the `lampos` group that reads the invitation token from the bus could ask
for the same invitation; it gets no data, only the lamp saying the invitation
the runner already offered.

## How you sound (#1803)

The person calls the lamp by name from their usual seat; the lamp learns how
loud that call is at the room microphone, for the busy-near rule's bar
(`lamp-near-call`, [attention.md](attention.md), Fourth's #753). The runner is
`crates/voice/src/onboarding/near.rs`, modelled on the privacy step: one exact
invitation token per prompt on `onboarding/pending` (`step: how_you_sound`),
asked for by the same core skill (a step it does not know, from a newer voice,
is read as `unknown` and its token still asked for: the say service decides), and only the prompt's complete playback opens
a try. A prompt counts once its budget commit lands, before it is published;
one dropped before the model took it (nobody there, the pause) is given back.
One cut before it played whole (someone spoke over it: an office's voices, or
the person answering early, as on lamp-52e6's row 10, try 1), or answered
over its last word (below), is offered again with its own number and spends
nothing, twice in a step at most (`MOST_REOFFERS`, kept through absence); a
third such prompt counts. Its record
says `offered_again`. A prompt said through a playback gap with nobody's turn
taken over it (`Delivery::Gapped`, #4035: on lamp-52e6 prompt 1 was "cut"
three times by the lamp yielding to its own voice and the step failed early)
was heard, so it opens its try's window as a whole one does, and a call or a
sentence in it is a try (lamp-29e7's re-test, 29 Oct 09:36:27: prompt 3
played with a gap opened no window, the next two calls went to the chat as
"answered as a call", and the model counted them itself). With nothing heard
in the window (no name, no line), it spends neither the budget nor these two
re-offers: it is offered again as a prompt never said, `MOST_UNSAID` times
at most, then the step is `not_available`; its row's `why` is `"the prompt
was played with a gap"`. Tests:
`a_call_after_a_prompt_said_with_a_gap_is_a_try`,
`a_sentence_after_a_prompt_said_with_a_gap_is_a_try`,
`a_prompt_said_with_a_gap_and_no_call_after_it_spends_nothing`.

**What each prompt says (#3702).** The tester on lamp-29e7 (28 October)
said a bare "Lamp" four times, never told what to say or how many calls were
wanted. The first prompt names the phrase the name detector knows ("Hey
Lamp", [name.md](name.md)) and says it asks until it has heard three (`TRIES`). Each later call
prompt answers the last try judged in this voice cycle: accepted,
`near_again` ("Got it. Say 'Hey Lamp' once more."); not accepted (any refusal, or answered
early), `near_missed` ("I didn't catch that. Say 'Hey Lamp' again, from your
seat."), never the first prompt again. Before any try is judged (a first
prompt cut, or a new voice cycle), and after the person left, a pause or a
mute, the first line is said. The phrase is
never a prompt's last words: a name begun in its last 0.5 s would be taken
for an early answer. `near_missed` is optional, as #3769's lines are;
without it a miss says the first line. The sentence part keeps its own prompts.
Tests: `each_call_prompt_answers_the_last_try_and_only_the_first_explains`;
`every_way_a_person_calls_gets_its_line` ("Hey Lamp", "Hey, Lamp" and a bare
"Lamp", heard or missed, at a 4, 22 and 30 dB rise, and a sentence without
the name: a heard call is "Got it. Say 'Hey Lamp' once more." at any loudness, since a try
does not judge loudness, and anything else "I didn't catch that. …");
`a_call_over_the_prompt_is_asked_again_with_the_same_line`.

**A try.** At the moment the prompt's whole playback is known, the runner reads
the Unix clock: that is the prompt's end. The coordinator's monotonic clock is
used only for the try's deadline and to catch a clock jump; it is never taken
for a Unix time. Evidence comes from the bus, dated by its headers:
`name/heard` (a hearing begins at the header's time less `ago_ms`),
`ears/level` (mic1's level and the room's quiet, every 100 ms) and
`voice/state` (when the lamp spoke). Collection runs to 7 s after the prompt's
end (the 6 s window plus the 1 s rise of a hearing begun at its very end),
keeping 1.5 s of earlier history so the half second of the lamp's own speech
before a hearing is known; the try is judged 0.3 s after that for delivery.
That 7.3 s is only for a try where no sure call is heard. A sure call (a
`name/heard` at 0.95 or more begun in the window, or over the prompt's last
word) is judged as soon as its own second of rise is in, 1.3 s after it began,
about 0.7 s after its final hearing; and the next prompt, its answer ("Got it. Once more." or
"I didn't catch that. …"), waits only 0.5 s of quiet (`ANSWER_QUIET_US`),
not the walk's 3 s, so it follows the call as a reply would (the
Reviewer-lead's P3 on #4001: a call 1 s into the window was answered about
6 s later). A hearing under 0.95 is no call alone and
may still count by its words, so it is judged at 7.3 s as before, and its
line's words still pending then (`TEXT_WAIT_US`, below) hold the judgement
until they come or the wait ends. It is never judged early: its line's End,
which says the words are on their way, comes 0.4 to 1.5 s after its speech,
after its second of rise (Thirteen's finding on #4014). The rules are the
same; only a second name heard after the verdict is past the try, not a
`TwoHearings` refusal. Tests:
`a_heard_call_is_answered_at_once_not_after_seven_seconds`,
`with_nothing_heard_the_try_is_judged_at_seven_seconds`,
`a_weak_hearing_with_words_pending_waits_for_them`,
`a_weak_hearing_whose_line_ends_late_counts_by_text`.
`lamp_near_call::near_try` judges it: one hearing in the 6 s at 0.95 or more,
mic1's largest rise over its quiet in the second after it began, and the lamp
not speaking from 0.5 s before the hearing to 1 s after (speech elsewhere in
the window does not spoil it). The prompt itself counts as the lamp speaking
until its end, so a name begun in the half second after it is refused as the
lamp speaking (its own tail or echo).

**A call heard by speech to text counts too** (#3923). lamp-name misses some
voices on some lamps: on lamp-29e7 (build 8001) the tester said "Lamp" the
same way four times and only prompt 2 counted; 1, 3 and 4 were refused "the
name wasn't heard" with `heard: []`, while the second route's speech to text
heard each bare "Lamp". So a line told the second route as the step's call
(`StepCall`, below) whose words were the lamp's name alone (`name_alone`:
"Lamp", "Lamp.", "Hey Lamp", "Hey, Lamp.", "Hey Lamp!") is told back as
`ProviderOutput::StepCallHeard`, before its idle `TurnComplete`. The
coordinator kept when that line's speech began and ended (its candidate's
trigger, and its End; not the oldest speech block kept, which can be a sound
from before the line: on c0a4 that start was 2.5 s early) and hands them to the step (`Onboarding::near_text_call`; record
`onboarding_text_call` with `taken`, and `began_unix_ms` or the `why` it was
not taken), which maps it to the Unix clock by the
prompt's end read on both clocks, as `holds_reply` does, and keeps it as
evidence only while the name part listens for a call. `near_try_with_said`
then counts it as a hearing: the same 6 s window, one call a try (two
separate calls, by either path, are two hearings), the same lamp-spoke guard,
and the rise from mic1's largest level over its quiet in the second after the
line began, `own_voice` readings left out. A line and a hearing begun within
1 s of each other (`ONE_CALL_MS`), or a hearing begun inside the line however
far in (a slow "Hey, … Lamp", or a cough before it that speech to text left
out), are one call; so are two hearings of the detector begun within 1.5 s
(`ONE_HEARING_MS`), or both inside one such line (c0a4: one "Hey Lamp" heard
twice, 1.1 s apart), at the first one's start and the surer score; calls
said further apart stay two (so a quick "Lamp. Lamp." under 1.5 s is one
try, not refused as heard more than once): a hearing at 0.95 or more
stays the detector's (`heard_by: name`, its score kept); one under 0.95 that
the line confirms counts as `heard_by: text`. A hearing under 0.95 with no
such line is still refused as unsure: the detector's bar is not lowered. A
try heard by text keeps no score (`score` absent, `heard_by: text`); an older
file, with a score and no `heard_by`, reads as heard by the name, and the bar
file and the kept tries accept either (`Try::heard_surely`). A line of the
name begun over the prompt's last word is answered early, as a hearing is
(below). A sound with no words, "Lamp is broken", or any other words never
come back as `StepCallHeard`, so they are never a call by text. When a call's
line began in the window and ended with its words not back yet, the try's
judgement waits for them at most 1.5 s more (`TEXT_WAIT_US`): a "Lamp" begun
near 6 s has its words back about a second later; once its turn ends
without `StepCallHeard` (its words were more than the name, or none), the try
waits no longer. Only that turn's own end clears it: a finished transcript
names no line, and an earlier line's late words would have emptied the call
line's place (the Reviewer-lead on #4006). A slow "Hey, … Lamp" whose pause ended early
(`--early-end-ms`) and went on as a new turn keeps the start of the line
given up (`a_slow_call_whose_pause_ended_early_keeps_its_start`). Known limit: a call heard only by text starts at the line's trigger,
a few blocks into its speech, so a breath or a sound just before "Lamp" moves its rise
window earlier, and the call's loudest moment may fall past it. Tests:
`lamp_near_call::tests::a_line_heard_as_the_name_alone_counts_with_its_rise`,
`without_a_line_of_the_name_alone_the_detectors_bar_is_unchanged`,
`a_line_and_a_hearing_begun_within_a_second_are_one_call`,
`three_calls_heard_by_text_alone_make_the_bar`,
`old_and_text_files_read_and_odd_mixes_do_not`;
`onboarding::near_flow_tests::a_call_heard_only_by_text_counts_with_its_rise_and_says_so`,
`a_call_heard_by_text_and_by_the_detector_is_one_try`,
`a_call_by_text_begun_over_the_prompt_is_offered_again`,
`a_call_begun_late_has_its_words_waited_for_a_moment`,
`three_calls_heard_only_by_text_keep_a_bar`;
`second_provider::tests::a_line_with_no_words_is_never_told_as_the_steps_call`;
`coordinator::tests::second_route::a_bare_name_heard_by_text_reaches_the_step_as_its_call`.

**A call begun over the prompt's last word is not a try.** People start
answering as the lamp finishes: on lamp-52e6 (row 10, tries 2 and 4) the name
began 237 to 478 ms before the prompt's end. A name heard at 0.95 or more,
begun up to 0.5 s before the end (`OVER_PROMPT_MS`, the call wait's
`CALL_BEFORE_US`), whose final hearing came after it, is the person answering
early: the voice holds its reply as to any call (`call_began_ms`), but it is
not measured. Its sound sits under the lamp's own voice, and the ears flag
`own_voice` for 0.5 s past the end (`lamp-ears`, `own_tail_ms`), so the second
after it would give the room after the call, not the call; nor is it told from
the lamp's own last word heard as its name (`lamp-name`'s `not_self.rs`). So
the prompt is offered again with its own number and spends nothing, under the
same `MOST_REOFFERS` as a cut prompt; once those are used it counts as a try
not accepted. Its record says `why: answered_early` and `offered_again`. One
begun earlier, or over before the end, is not the call. Tests:
`a_name_begun_over_the_prompts_last_word_is_offered_again_not_measured`,
`the_lamps_own_last_word_or_echo_heard_as_its_name_is_not_a_call`.

A try is refused, and the person asked again, when the Unix clock moved more
than 1 s apart from the monotonic one over the try, when a source began again
or a `name/heard` was lost during it, or when any message during it was
invalid (a score that is not finite or outside 0 to 1, an impossible
`ago_ms`, a level outside -120..0 dBFS). A valid score under 0.95 stays a
hearing, so the try says it was unsure. No sound, no mic2 ratio and no voice
identity is used or kept.

**Every try says why in the journal** (#3504). Each `onboarding_near_try` and
`onboarding_sentence_try` record, accepted or not, is also one lamp-voice
journal line, `lamp-voice onboarding: how_you_sound try: {…}`: the prompt,
`accepted`, `why` (or the sentence's `short`), and the numbers that decided
it. A name try lists every name heard as `heard: [[ms from the prompt's end,
score], …]`, every line heard by text as the name alone as `said: [ms from
the prompt's end, …]` (#3923) and, when refused, when the lamp spoke as `spoke: [[from, to],
…]` (ms from the prompt's end; the prompt itself is the span ending at 0);
an accepted one adds `rise_db`, `score` (null when heard by text) and
`heard_by` (`name` or `text`), so a run shows which path counted. A sentence try has `words`, `speech_ms`,
`rise_db`, `bar_db` and `said_close`. The session record rolls over and the
journal is rotated, so read it soon after the walk.

**The reply is held.** An answer that starts within a second of the name
spoils the try, and a model hearing its name answers ("Yes?"). So while a
try's window is open, from the prompt's end to 7 s on or its verdict, the voice holds its
reply to the call: an utterance in it that is only the name ends unanswered
when it ends, however long it runs past the window, as a sound does. Only the
name: the step must have heard it (`name/heard`, in the window, beginning no
more than 0.5 s before the utterance's first speech, as the call wait's
`CALL_BEFORE_US`), and the utterance holds no more speech than a call (1.2 s,
`CALL_SPEECH_BLOCKS`, #782). Anything else said in the window, "skip that"
with no name or "Lamp, can we skip this?" with more than the name, goes on as
any other turn (the reviewer on #1964). The rule is the step's own
(`Onboarding::near_holds_reply`, `NearFlow::holds_reply`), read by
`coordinator.rs`'s `near_call_window` on every block of a turn and at its End,
and the `AdmissionStep::End` arm drops the turn. Its End never goes to the
model and nothing of an answer is played, on Gemini Live and on the second
route (`LAMP_VOICE_ROUTE=second`) alike, since both are the coordinator's
`rig.provider`. Its record says `onboarding_near_call` (the endpoint's field
and the turn's outcome), not `not_speech`, so it is not counted as a short
sound. On the second route a line told as the step's call (`StepCall`, below) is
not held (#3923): the route never answers the name alone, and its End must
go on for its words to come back as the step's call by text. On c0a4 (head
9d4d3e0d9) each call began about 0.8 s after its prompt and lamp-name heard
something of it, so it was held and dropped at its End: the route logged
no words for it ("speech to text kept ready" and nothing at the line's
end); a "Hey Lamp" after such a line (request 7) was answered, likely with
the held line's kept sound going on into it. On Gemini Live the hold
stands. A line whose Start came before the step waited for a call is told
as its call once the step does, before its End (record `onboarding_step_call`
with `late`; c0a4, 28 October, "Hey Lamp." answered as a turn), and the step
counts a try's evidence and calls while its prompt is claimed or its try is
open, whatever the walk's lead says between ticks. The coordinator keeps the
last four call lines, each in its own place, so a call whose words come back
after the next line began still reaches the step (c0a4, "Hey, Lamp." left
to the step with `said: []`). Tests:
`a_line_begun_before_the_step_waits_is_told_as_its_call_once_it_does`,
`a_call_whose_words_come_back_after_the_next_line_began_still_reaches_the_step`,
`a_call_begun_after_the_prompt_and_heard_by_the_detector_still_reaches_the_step_by_text`.
Outside the window, and with no near-call try open, nothing changes;
Stop, mute and physical privacy are not touched. Tests:
`coordinator::tests::second_route::a_call_in_the_near_call_window_is_not_answered_on_either_route`,
`coordinator::tests::second_route::a_line_that_is_more_than_the_name_is_answered_in_the_near_call_window`,
`onboarding::near_flow_tests::the_reply_is_held_only_in_the_try_window_and_the_held_call_counts`.

**On the second route, only the name is never a turn** (#3504). On lamp-52e6
on 27 October each "Lamp" the person called was also taken by the second
route as a line of its own and answered, and the answer cut the walk's next
prompt: three of six tries were "not played whole". So while the name part
waits for a call, from its prompt handed to the model, through its playing,
to its try's judgement (`NearFlow::waits_for_call`), every line admitted is
told the second route as `ProviderInput::StepCall` right after its Start
(`coordinator.rs`, `step_call`; record `onboarding_step_call`), whether or
not lamp-name's hearing has come yet. When its words are only the lamp's
name (`second_provider.rs`'s `name_alone`, the rule of
`LAMP_SECOND_NAME_CARRIES`: "Lamp.", "Hey Lamp!"), or the name was heard and
no words came back, the second route asks no chat, answers nothing, keeps it
out of the thread and carries it to no later line ("only the lamp's name,
while how_you_sound waits for a call: left to the step, not answered"). The
try is measured from `name/heard` as before, and, when the words were the
name alone, from the line itself too: the route also sends `StepCallHeard`
(#3923, "A call heard by speech to text counts too" above). A line with no
words, which only the detector's call made one, is not told: the detector
heard it. Any other words ("Lamp, what
time is it?") are a turn as now, and a prompt such a line cuts is offered
again as any cut prompt. Outside the step nothing is told. The Live worker
takes nothing from `StepCall`; there the coordinator's hold above stands.
Only the invitation's own turn can mark a prompt played whole
(`invitation_finished` is bound to it), so an answer never does. The places
that decide whether a line heard during the walk becomes a turn are:
`commit_admitted_input`'s `CountOnly` (the sentence), the `StepCall` after it
(the call), `near_call_window` on a turn's blocks and at its End (the
coordinator's hold), and the second route's turn task (`count_only` and
`step_call` marks, before its name carry). `continue_after_early_end` tells
no `StepCall`; it runs only with `--early-end-ms`, which the product unit
does not pass. Tests:
`second_provider::tests::only_the_name_while_the_step_waits_for_a_call_is_not_a_turn`,
`coordinator::tests::second_route::a_line_begun_while_the_step_waits_for_a_call_is_told_the_second_route`.

**Each prompt's end is one journal line**: `lamp-voice onboarding:
how_you_sound prompt N played whole (call)`, or `cut`, or `with a gap`, or
`(sentence)`, at the
moment its playback is known whole or cut. A test driven from outside times
its answer by it.

**The name part's end.** At most five prompts, and the part stops asking
once five can't reach three accepted (`asks_again`). Three accepted calls make
the bar (`near_rise`: the quietest less 3 dB, held to 16–24 dB); the bar and
its tries are written to `/var/lib/lampOS/voice/near-call.json` (0600, whole),
and only after that write completes is the name part done
(`onboarding_near_call`, `done`, and the lamp says so). Too few accepted calls,
or calls more than 10 dB apart, make no bar: any earlier file is removed, the
part has failed (`onboarding_near_call`, `failed`), and the rule keeps its
18 dB. Either way the step goes on to its sentence part.

**The sentence part** (the first builder's ruling (a), 20 October). The lamp
asks the person to say a sentence from where they sit, in their own words: a
prompt like the name's, with its own budget of two (`sentence_prompts`, its
invitations numbered 6 and 7 after the name's five). Its window runs 15 s
from the prompt's end. A line that begins in it is the sentence, and is
measured if it ends in it too (`coordinator.rs`, `sentence_window`;
`Onboarding::sentence_window`). Its Start is followed at once, before any of
its sound, by `CountOnly` to the provider. Unlike a name in the near-call
window, its End goes on, so the words come back, and its answer is held: the
turn ends `onboarding_sentence` as its finished words arrive, or as the first
of an answer shows, its words or its audio. The second route
(`LAMP_VOICE_ROUTE=second`) never asks its model for a count-only line, so
there is no answer; Gemini Live may begin one first. Nothing of it is played. It is done only on the voice's own
evidence (`near.rs`, `sentence_judge`):

- **heard with words**: its finished transcript has at least one word that
  asks something (`wait_words::words_in`: fillers, "okay" and hums don't count);
- **said close, for this room**: the room microphone's largest rise over its
  quiet during the line (`room_floor::Rise::most_since`, the record's
  `rise_db`) at the sentence's bar or more (`sentence_bar`, the record's
  `bar_db`): this person's quietest accepted call from the seat less 3 dB, as
  their bar is made, so it is relative to this room's quiet at this hour;
  with no call accepted, 6 dB (`SENTENCE_NO_CALL_DB`); never over the near
  rule's bar (`attention::near_rise_db()`: 18 dB, or this home's bar when
  `LAMP_ATTENTION_NEAR_CALIBRATED` is on), and never under 3 dB
  (`SENTENCE_LOWEST_DB`). On lamp-52e6 in an office (row 10) the call rose
  3.9 dB and a normal sentence (8 words in 1.95 s) fell short of the flat
  18 dB; there the bar is 3 dB. A far, quiet line still falls short: people
  farther off rose 9.8 to 15.1 dB in a quiet room, under its 18 dB, and a line
  under 3 dB in the office. The no-call bar is not the near rule's 18 dB,
  which is a call's: a sentence from the seat rose 9.2 dB on lamp-52e6 (row
  10) and 10.7 to 11.0 dB on lamp-c0a4 (#2844), so on 27 October a person on
  52e6 whose calls were all refused failed the whole step on their sentence
  (#3504). 6 dB is 52e6's seat sentence less 3 dB, as a bar is made from
  calls; the same voice from twice as far rises about 4.5 dB, under it, and
  the lamp's own voice gives no rise at all. A raised voice from across the
  room (9.8 to 15.1 dB) is not refused by level, here or at the 3 dB a lamp
  gets when its calls are accepted (#2844). Tests:
  `in_a_loud_room_the_sentence_is_judged_against_the_persons_own_calls`,
  `calls_answered_at_the_prompts_end_leave_no_bar_and_the_seats_sentence_still_passes`;
- **its words covering it**: at least one word for each second of the line's
  speech (`MS_A_WORD`, half the pace of ordinary speech; the line's speech is
  its blocks the detector scored as speech, 10 ms each). Fewer is a fragment.

The try is judged as soon as a line with at least one word is in, or 3 s
past the window with none. A line whose words came back none ("okay", a hum,
the room) does not end the try: a later line in the window replaces it, and
it is judged only if nothing else comes. Short of the evidence, it is coached once: "a little louder" when
the level fell short or nothing was said, "a little closer" when the words
didn't come back or didn't cover it. The second try ends the part: done or
failed. Failed, the lamp says "I'll get used to your voice as we talk."

**The step's outcome is the sentence part's.** The name part is independent:
its outcome is its bar file, written or removed, which #1892 reads as before.
A step `done` with no bar (nobody called the name) and a step `failed` with a
bar both happen. The model can only skip the step
(`setup_step how_you_sound skipped`); a skip leaves an earlier bar as it is.
Redo forgets the prompts and tries of both parts. Nobody there for 30 s or the
pause drops the prompt or try in flight, and the sentence part goes on when
someone is back. A voice restart keeps the committed prompts and accepted
tries: the name part's bar is written again from them and the sentence part
goes on with its prompts left (the second then only asks again: what fell
short is not kept); with both spent and none judged, it has failed.

**Nothing of the sentence is kept.** Neither its words nor its sound, on
either route. The coordinator gathers every piece of its words in memory
(Gemini Live gives them in pieces) and passes only their count, once the
last piece is in; it publishes none of them on `voice/heard`, none of the
model's answer on `voice/said`, and records no `transcript` for either in
the session record (the journal). In the provider's process, `CountOnly`
(`thread::count_only`, cleared by the next Start) keeps every piece of what
was heard, and the lamp's answer to it, out of the conversation thread: so
out of its file, the memory's journal (`remember`), the next line's
exchange and every later prompt and history built from the thread. The
second route also never asks its model for it, and a line going on from it
does not begin with its words. The session record has `onboarding_sentence_line` (`speech_ms`, `rise_db`),
`onboarding_sentence_words` (`words`, a count) and `onboarding_sentence_try`
(`accepted`, `short`, the same numbers, `bar_db` and `said_close`); `progress.json` keeps only
`sentence_prompts`. Tests:
`coordinator::tests::second_route::a_sentence_in_its_window_is_counted_and_not_answered_on_either_route`,
`coordinator::tests::second_route::the_sentences_words_and_its_answer_are_counted_only_on_either_route`,
`provider_worker::tests::the_sentence_after_count_only_never_reaches_the_live_thread`,
`second_provider::tests::the_sentence_is_heard_for_its_count_and_never_asked_of_the_model`,
`second_provider::worker::tests::the_sentence_is_kept_nowhere_and_the_next_line_carries_none_of_it`
(each with a planted marker word),
`onboarding::near_flow_tests` (the sentence tests at its end). Fourth's #1892 reads the bar
(`LAMP_ATTENTION_NEAR_CALIBRATED`, off by default) and the factory reset
removes the file; this step only writes it.

**Kept and checked.** `progress.json` gains `near_call_prompts` (at most 5),
`near_call_tries` (at most 3, never more than the prompts) and
`sentence_prompts` (at most 2). Every load and commit checks each kept try
(finite rise 0–120 dB, score 0.95–1) and that a kept `done` had its sentence
asked (`sentence_prompts` 1 or more); a file that fails is refused whole, as
any malformed progress is. The bar's own file is checked by its reader.

**Latency and the loop.** From the prompt's end to the outcome: the try is
judged in memory on the coordinator's loop (at most 256 buffered numbers)
7.3 s after the prompt; the next prompt waits for the usual 3 s of quiet. After
the third accepted call, the bar's write (`lamp_near_call::write`, a
synchronous write, fsync and rename) runs on the onboarding storage worker; the
loop only submits it and polls. The step is recorded `done` on the poll after
that write completes and is then committed by the same worker. Nothing on that
path waits on a file: a held write leaves capture, the privacy switch and Stop
running (`a_held_near_call_write_does_not_interrupt_capture_privacy_or_stop`,
`a_held_near_call_write_never_holds_the_loop_and_nothing_is_done_before_it`).
The bus is drained without waiting, at most every 20 ms.

Tests: `onboarding::near_flow_tests`, `onboarding::storage::tests::the_near_call_bar_is_written_or_removed_only_on_the_worker`
and the coordinator's held-write test; the record is in
[verification/onboarding-near-call-host-20261019](verification/onboarding-near-call-host-20261019/README.md),
and the sentence part's in
[verification/onboarding-how-you-sound-sentence-host-20261020](verification/onboarding-how-you-sound-sentence-host-20261020/README.md).
Not shown: a lamp run with a person.

## Giving way (#1800)

The lamp invites its person to move its head gently by hand, gives way, and
says so. The invitation asks the person to hold the base with one hand and tilt
the head with the other, so the push moves the head: a push into the arm
doesn't move the head joints. The runner is
`crates/voice/src/onboarding/giving_way.rs`, modelled on
the near-call step: one exact invitation token on `onboarding/pending`
(`step: giving_way`), asked for by the same core skill, and only the
invitation's complete playback starts a try's minute. The body is read from
the moment the invitation is taken to be said, though: a person pushes as soon
as they are asked, while the lamp is still saying it, and does not push again.
On lamp-29e7 (28 Oct, #3702) the body said the hand 15 s before the try
opened, and the two minutes after it had no push. A push from the
invitation's start is the try's, judged the same way; one done before the
invitation ends is said once it has. It runs the same on Gemini Live and
on the second route (`LAMP_VOICE_ROUTE=second`): the invitation is a written
turn to the coordinator's `rig.provider`, whichever route that is.

**The rule it uses is the body's** (Fifth's #780, [body.md](body.md), "Giving
way to a hand" and "A hand on the bus"): a joint that gave way and is then more
than `HAND_MOVED` counts (15, `lamp_body::HAND_MOVED`, used by
`in_hand_after` in `crates/body/src/body.rs`) from where it gave way was moved
by a hand; one within `HAND_MOVED` of its goal is not in a hand. No new number
is made. From the bus, in the minute after the invitation's end (Unix
milliseconds by the headers, as near-call's; the body's own `since_us` is its
monotonic clock and is never read as a time):

Any of the three head joints counts: the base turning (`base_yaw`, lamp-52e6)
or the head tilting (`head_pitch`, the run that passed on lamp-29e7, 28 Oct),
so the invitation names no direction ("any way you like").

1. `body/joint_states` with no hand on it: each head joint's goal (`base_yaw`,
   `head_pitch`, `head_roll`), where it was asked to be. The base is a head
   joint here: asked to "take its head, move it a little", the person on
   lamp-52e6 (27 Oct, #3504) turned only the base, 30 holds all on
   `base_yaw`. The goal judged against is the oldest of the last second with
   no hand said (`BEFORE_MS`, the body's second in a hand), not the last one:
   the body gives way where the hand already has it, its goal there, a step
   before it says the hand, so the last goal before the push is already the
   hand's and a turn held there read as no move.
2. `body/hand` says `pushed`.
3. While it does, a head joint is more than `HAND_MOVED` from that goal and its
   own goal is within `HAND_MOVED` of where it is: the hand moved it and the
   body holds it there (it gave way, under `return` and `stay` alike; a body
   that pushes back keeps its goal and is not counted).
4. `body/hand` says not pushed: done, and the lamp says so.

**Never a silent minute** (#3702): on a tester's lamp the ring sat orange and
the lamp silent through the minute while the person did not know whether it
felt them. The first push of a try owes `give_felt` at once; a try's minute
with nothing felt after `PROMPT_AFTER_US` (8 s, touch's `GUIDE_EVERY_US`), owes
`give_prompt`, once a try; a push let go before the head moved `HAND_MOVED`
owes `give_more`, "A little more, then let go.", each time. The body says a
hand once its load passes its bound or a joint is 15 counts (about 1.3°) off
its goal, so a gentle nudge is enough and no hold is needed: "That's it, now
let go." is the cue that it was enough. Too hard is never a failure (the body
gives way), so no line warns of it. The prompt also comes 8 s after a push
that was not enough, once a try. An invitation cut by the person talking while
they push (`Delivery::Cut` with a push felt) keeps the push and opens the
minute; a push still on at the minute's end is waited for `LET_GO_US` (5 s)
to be let go (the Reviewer-lead's P2 and P3 on #4002). One cut, or played
with a gap, before any push began spends no try (#4035: on lamp-52e6 on 28
October the invitation was "not played whole" twice, the lamp yielding to its
own voice, and the step failed with no push asked for): it is said again as
an invitation not said, `MOST_UNSAID` times at most, then the step is
`not_available`, never `failed`. Test:
`an_invitation_cut_with_no_push_is_said_again_and_spends_no_try`. The count
is the lead's: when the step is led anew (offered again later, or asked once
the person is back) it starts again, and a re-offer is dated
(`offered_at_unix_s`) so the next waits `reoffer_after_s`, as any step's
(Third's P3 on #4045). A re-offer that ends `not_available` again keeps that
date, for every step (the Reviewer-lead's P2 on #4045). Tests:
`a_step_left_for_later_by_gaps_counts_its_unsaid_anew_when_offered_again`,
`a_re_offer_left_for_later_again_waits_the_skills_interval`. A newer line owed replaces one not yet said (done replaces the felt
line). The words are the skill's (optional, as #3769's: an older file says
none of them). Latency from the body's push to the line: the body says the hand
on its next offered step (every 80 ms); the voice drains the bus at most every
20 ms (`LOOK_EVERY_US`), and the line is owed on that same loop pass and handed
to the provider as a written turn on the next onboarding poll, once nothing
else is being said, so at most about 100 ms plus the provider's first audio;
a push during the invitation is answered as soon as it ends.

The journal says what the step saw, as `lamp-voice: giving_way: ...`: the
try opening, each push the body says, and on its release the decision with
the head joint farthest from its goal before (one held where the hand has it
first), in counts: where it is, how far, its goal before and its goal now; and
a minute ending with how many pushes the body said; the try opening says how
many came while the invitation was said. A try that fails says
whether the body never said a hand, or a hand that did not move the head that
far or was not held there.

A push that does not move the head that far (a bump), a hand still on it when
the minute ends, a reading from before the invitation ended, from another
publisher, unparseable, non-finite or with a missing head joint, a body that
began again or lost a message mid-push (that push is not judged), and a Unix
clock more than 1 s apart from the monotonic one over the try, are not done. A
minute without it asks once more; two invitations without it: failed, and the
lamp says they can redo the setup later. The minute is the one "where you sit"
gives (`SIT_GIVE_UP_US`), and two invitations are the privacy step's budget.

The model can only skip it (`setup_step giving_way skipped`); a skip, a redo,
nobody there for 30 s or the keeper's pause drops the try in flight, so later
evidence is not counted. Without a body (`LAMP_BODY_MOOD_SOCKET` unset), the bus or the
say service it is `not_available`, and the pause while it is led records
`not_available` too; either is offered again later as any such step. The
invitation budget is kept with the walk: `progress.json` gains
`giving_way_tries` (at most 2, a file with more is refused whole), committed
before each invitation is published, as near-call's prompts are; an offer
never handed to the model gives its try back. So a voice restart or a mute's
end, which rebuild the walk, cannot ask a third time: a walk rebuilt with both
spent fails the step. A redo starts the count again. Evidence
is judged as it is drained from the bus (at most 32 a topic every 20 ms, no
waiting); the outcome is then kept by the storage worker like any other, and a
held write neither holds the loop nor says done before it lands.

Tests: `onboarding::giving_way_flow_tests` (a fake body on a scripted clock:
done only by a hand that moves the head and lets go, the model refused; a hand
still on, two unfelt tries failing; a bump, a body pushing back, a restart, a
forged publisher and early evidence not done; a clock jump; no body and the
pause `not_available`; a skip and a redo ignoring later evidence; a held
progress write; a rebuilt walk not asking a third time; a corrupt kept count
refused; the base turned about 32 counts a hold from 1,903, as on lamp-52e6
on 27 Oct, with the hand said from the second hold as the body says it since
#3676: done after two holds and after the day's five to 1,744, and not done
while the body never says the hand; #3702's: a push while the invitation is
said is the try's, done let go before or after it ends; nothing felt is
prompted once at 8 s and nothing else is said in the minute; a push that did
not move the head is answered and asked for a little more; each of the
three head joints done and the arm alone not; a late push a second before
the minute's end done; no push at all prompted each try, then the plain fail
line; each line pinned). The record is in
[verification/onboarding-giving-way-host-20261019](verification/onboarding-giving-way-host-20261019/README.md).
No coordinator test drives an onboarding invitation through the say service on
either route (privacy and near-call have none either). Not shown: a lamp run
with a person, and the latency from the hand letting go to the lamp's word.
