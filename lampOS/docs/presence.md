# Someone at the lamp, and the light going out when nobody is

Two pieces of #1677, built on 19 October 2026 and not yet run on a lamp:

- **`room/presence`**, one signal of whether anyone is at the lamp, said by
  the eyes. It is the signal the light reads, and the one Messages reads to
  send a task's news to the app instead of into an empty room (#1673: "use
  one presence signal for both").
- **Light out when you leave** (`skills/light-out/`), a skill: after forty
  minutes with nobody there, the light dims two steps and holds there for
  five minutes, then goes out over a minute, as something falling asleep
  would, and comes back as it was the moment someone sits down, speaks,
  touches it or, during the dim, moves in view (the owner, 19 Oct 07:2x, a
  core skill; #5746, the first builder's approval on #5765).

Both are off unless switched on (below). Unprompted behaviour follows
CLAUDE.md's rule 8 and [creature.md](creature.md): small, slow, never a
snap, never over anyone.

## What V1 did

V1's `hal/drivers/sensing/presence_service.py`, read on 19 October:

| V1 | Verdict |
|---|---|
| Presence from camera motion, and from the person's activity (a voice turn, a gesture) | Change: faces from the eyes and a voice heard. V2's eyes find faces, not motion; a voice heard covers someone out of the camera's view |
| Two stages: dim to 20% after 5 minutes (`HAL_IDLE_TIMEOUT_S`), off after 15 (`HAL_AWAY_TIMEOUT_S`) | Change: a dim after 40 minutes, held 5, then out over a minute (#5746). V2's eyes miss a reader whose face they can't find, so it waits far longer, and the dim is a soft cue to anyone still there before the light goes |
| Off is the strip cleared | Change: out is the ring's ember, never dark: the ring at rest is the sign the lamp can hear ([ring-choreography.md](ring-choreography.md), "The lamp's own light") |
| On motion or activity, the person's light restored, not while asleep, not when they had turned it off | Keep: the ring shows the person's own light again by itself when the skill's ask ends; a light already off is left alone |
| "Sleep owns the strip": nothing while the device sleeps | Keep: never at night, the lamp's sleep (below) |
| An away event for the agent to announce sleep "in a cozy, sleepy way" | Drop: it never says a word about it. A lamp that announces it is leaving the light is a machine |
| Guard mode keeps it awake | Not taken: V2 has no guard mode |

## `room/presence`

`lamp_msgs::RoomPresence` (`crates/msgs/src/presence.rs`), kept (`latest`),
said once a second while the eyes look and at once when `someone` changes,
each good for 3 s ([msgs.md](msgs.md)):

| Field | Meaning |
|---|---|
| `someone` | A face seen, a voice heard or the lamp touched, or, only in light-out's dim, the view changing, within the last 20 s (`RoomPresence::SOMEONE_WITHIN_S`: the voice's Arrival forgave a face lost for the same 20 s, until its hello was removed, #2825) |
| `nobody_s` | How long nobody has been at the lamp, in whole seconds, from the last of those, or from when the eyes began looking if none came since; 0 while someone is there |
| `stirred` | While nobody is there and light-out's light is out, the view changed in the last 3 s (#5746): light-out brings its dim back; absent (false) otherwise |
| `by` | While someone is there, what last showed it: `face`, `voice`, `touch` (a touch or a pet, #5746) or `moved` (the view changed in light-out's dim, #5746); absent while nobody is (and from an older eyes'). At each change to someone the eyes also write `lamp-eyes: someone at the lamp (face, 0.21 of the picture tall)`, `(voice)`, `(touch)` or `(the view changed in light-out's dim)` in their journal (the tallest face as `humans/faces/tracked` publishes it, clamped to the picture), so a run tells a passer-by from someone at the desk (the first builder's lamp check, 19 Oct: four `someone`s in 15 minutes of an office at lunch, cause unknown) |

Counts and times only: no picture, no face, nobody's identity.

**Who says it.** The eyes (`lamp-eyes`, `crates/eyes/src/away.rs`, through
`bus_out.rs`), with `LAMP_PRESENCE=on` beside `LAMP_BUS=on` in their unit.
They own the larger part of the evidence, the faces; the voice's lines come
to them on `voice/heard`, as they already do for the mouth
(`--mouth-on-heard`). A face is any face the eyes publish on
`humans/faces/tracked`, after their own still-face filter, near or far, as
Expression counts nobody for its sleep (`alive.rs`). `room/state` says how
the room sounds and `attention/addressed` one request at a time, so neither
carries this.

**Only a voice said to the lamp (`LAMP_PRESENCE_VOICE=addressed`, part 2,
the first builder's yes on 19 Oct).** By default every finished line on
`voice/heard` is someone for 20 s, so in an office chatter near the lamp
keeps the light on. With `LAMP_PRESENCE_VOICE=addressed` in the eyes' unit,
a voice counts only on a `voice/turn` the voice answered as said to the lamp
(`answered`, #1155), at that verdict's time; a turn it held back as not said
to it, and a line only heard, are nobody. `by` is still `voice`. The cost:
the verdict comes after the voice has judged the line, about 1 to 3 s after
the words end, so the light coming back on a voice alone, from out of view,
is that much slower; a face brings it back at once, and "Hey Lamp" from a
dark doorway is still answered. Off unless set: unset, it is `voice/heard`
as before.

**A far face counts only while it moves (`LAMP_PRESENCE_FAR=moving`, the
first builder on #1678, 20 Oct).** By default any face counts, near or far.
At lamp-75c9 on 20 Oct at 10:17 a steady face far behind the sitter, 0.06
of the picture tall at its right (someone in the office, or a picture),
held `room/presence` at someone for about 37 s after the sitter had left. At
home that keeps the light on. With `LAMP_PRESENCE_FAR=moving` in the eyes'
unit, a face under 0.14 of the picture's height (`away::NEAR_TALL`, the line
the activity judge and the still places use) counts only:
- within 4 s of a near face (`NEAR_LATELY_US`): the sitter the detector
  missed for a look, or someone walking away; or
- while its own centre, by its passing id, has moved 0.05 of the picture,
  across or down, within 10 s (`MOVED`, `MOVED_WITHIN_US`): someone walking
  past or coming to the lamp. A false face's box moves 0.01 to 0.02 look to
  look (`still.rs`), and so does a steady face far behind. Each face is
  judged on its own, so two steady far faces taking turns as the tallest,
  or a face found for one look elsewhere, are not a move (the reviewer on
  #2149); or
- while it's the sitter's at their place: within `activity::REACH` of the
  last near face's heights of the place's centre, and at least half that
  face's height (`AT_PLACE_TALL`), whatever its id. The place follows the
  sitter's face look to look, as the activity judge's `sitter_in` follows
  its sitter, and is forgotten 4 s after no such face is at it. That's the
  sitter leaning back (0.6 to 0.95 of their seated height at lamp-75c9) or
  sitting low (0.68). A small face far behind the chair, in line with the
  camera, is under half and doesn't count (the reviewer on #2183).

A near face always counts. The costs:
- Someone sitting still far from the lamp, at another desk, is nobody,
  which is what the light wants.
- A steady face behind the chair that is at least half the sitter's height
  (a big picture, or someone close behind them) keeps counting after they
  leave, for as long as it stays found. A small far face there doesn't.

**Why the place** (#1677's rerun, lamp-75c9, 20 Oct 11:55–12:04): sitting,
the face was 0.146 to 0.2 of the picture tall. Leaning back still, it was
0.12 to 0.14, under 0.14 in 242 of 341 looks, and its id changed from f1 to
f10 while the place held (0.52 to 0.55 across, 0.37 to 0.40 down). Counted
only by the line, presence said nobody 131 s into leaning back. The line
itself stays at 0.14, since a lower one would let a bigger far face in (the
first builder, 12:12 on #1677). Walking out, with the far face in view at
0.73, it said nobody 23.8 s after the last near face. Off unless set:
unset, every face counts as before.

**A touch or a pet is someone (#5746).** A hand on the lamp is someone at
it: the eyes take `controls/touch` and `controls/pet` (`lamp_msgs::TouchEvent`,
`HeadPet`, Controls') as a presence input, the way they take a voice, at the
message's time, `by = "touch"`. One whose pads are turned off (`enabled`
false) is nothing. Like a voice, a touch while the eyes are not looking (a
mute, the privacy switch) is nothing, and not counted once they look again
(below). So a touch resets `nobody_s`, and a touch during light-out's dim
brings the light straight back.

**The view changing, in light-out's dim and after its out (#5746, the
first builder's change on #5765, and the reviewer's P2 there).** A reader
the eyes can't find, who doesn't touch the lamp or speak, still turns
pages. While light-out's dim shows, the eyes count the view changing as
someone, `by = "moved"`, and the light comes back. While its out shows,
after its fade, the view changing is a *stir*: `room/presence` still says
nobody, with `stirred: true` for 3 s (`RoomPresence::STIRRED_FOR`), and
light-out brings its dim back, not the full light, and its five minutes
again, then the out (a reader the dim missed is never left at the ember;
the lead's head-down reader). At any other time the eyes never count it,
so movement alone never resets the forty minutes. *How the eyes know what
shows:* the ring's own words, nothing new on the bus. The ring says a
request's light `light/reached` after it eases in and after each renewal
(every 10 s), naming its asker, and says on `light/drawn` what it draws
(when it changes, and once a second at rest). Light-out's light shows while
the newest `light/drawn` is the light the ring last said reached for
`skill:light-out` (`change::DIM_SOURCE`): on for its dim, off for its out.
It is held until the ring draws something else, which the ring says at
once (the out asked, the light given back, the person's light over it),
and is never timed against the renewals, so a late or unsaid renewal can't
drop it (the reviewer's P2: a 15 s window against 10 s renewals failed
towards the dark). A ring that says nothing more leaves it held: a change
then brings light back, the safe way to be wrong. A change counts 3 s after
the dim first showed (`DIM_SETTLE_US`), and 65 s after the out first showed
(`OUT_SETTLE_US`: its minute of fade and the exposure following).
*What counts as a change:* `change.rs`'s own comparison of each glance with
the last one compared (a 32 by 24 grid of mean greys, each row's shift
taken out, each cell past 12 greys and 8 times its learned noise), run at
every glance with presence on so the room's noise is learned as the
detector's skip learns it; the out's dark picture is compared on its own,
begun afresh when the out shows, so its noise is learned through the fade.
Only a verdict in the dim or the out counts: 2 cells or more
(`VIEW_CELLS`), one fewer than the detector's 3, leaning towards a yes,
since a yes only brings light back and a no leaves a reader in the dark.
Only the yes leaves the eyes, as `room/presence`'s `by` or `stirred`: no
frame, no cell, no place.

**Never "nobody" when it is not known.** Nothing is said while the eyes are
not looking: the camera closed by the privacy switch, a mute or the voice
gone, or no look for 5 s (`away::BLIND_AFTER_US`, Expression's
`EYES_QUIET_US`). The last value then runs out 3 s later, and a reader knows
nothing. When looks come back the count of nobody begins again from then:
who came and went while the eyes were shut is not known. A gap of under 5 s
(a stalled read, the camera reopened at another size) counts on.

**Readers.**
- Light out when you leave: `nobody_s` above 2399, below.
- Who was here (`skills/who-was-here`, #1685), an installable skill: when
  a person asks "was anyone here while I was out?", the model is told the
  spells someone was at the lamp over the last 12 hours, and when the lamp
  could not tell (a gap past a value's 3 s), each with its length and how
  long ago (`crates/skills/src/visits.rs`, `[history]` in
  [skills.md](skills.md)). Times only, in `lamp-skills`' memory, a day at
  most, lost at a restart; a spell ends where `nobody_s` says the last face
  or voice was. A spell in which no value's `by` said a face, only a voice,
  is told as someone heard near the lamp, not seen (a passer-by, or someone
  out of view); a value with no `by` (an older eyes') counts as a face.
- Messages (#1673): "nobody for 5 minutes" is `someone` false and
  `nobody_s` at least 300, on a value that has not run out.

## Light out when you leave

`skills/light-out/skill.toml`, in `lamp-skills`' schema
([skills.md](skills.md)): a core skill (`kind = "core"`, version 1.0.0,
#1698) that may read `room/presence`, `voice/state` and `memory/rules` ("Keep
the light on", below) and use the light, and nothing more (`[permissions]`). A core skill comes with the lamp, so
this one is installed, and on in the product since #5746 (the lead's
call, 6 Nov, after #5505 and #5539): until then it was installed disabled
(`install::PRODUCT_SKILLS_OFF`), so presence on for Messages did not put the
light out on lamps where nobody had watched it. Its played row on lamp-52e6,
a reader with their head down not darkened, is owed. A test holds the install's skills to exactly the core ones.

| | Default | Why |
|---|---|---|
| When | `nobody_s` above 2399: nobody for 40 minutes (#5746, was 10) | Faces miss someone reading face down or turned to a second screen; a reader can sit 30 minutes and more without the eyes finding their face. A light that goes out on someone still working is the failure that reads as a machine, so it waits long, and a touch or a pet counts as someone as well as a face or a word. Messages' 5 minutes is its own line on the same signal |
| How | First the light two steps dimmer, held five minutes (`light = { dim = 2, for = "5m", until = "ends" }`), then the light shown, off, asked of the ring over a minute (`light = { out = true, over = "60s", until = "ends" }`) | A living thing lets the light go slowly: the dim is a soft cue to anyone still there, and the out is "like falling asleep, never snapping off" (the owner). Out is the ember. The out comes only if the dim was held its five minutes (`for` on a dim, [skills.md](skills.md)) |
| Stirred after the out | While the out shows, after its fade, the eyes seeing the view change (`stirred`, nobody still there) bring the dim back at once, not the full light, and its five minutes again, then the out (the reviewer's P2 on #5765) | The lead's head-down reader is never left at the ember: a page turned after the out is enough |
| Back | The moment someone is there: the first `someone` (a face at the eyes' next look, at most 600 ms with nobody in view, a voice heard, a touch or a pet, or, during the dim, the view changing) ends the skill's ask, in the dim before anything goes out, and the ring shows the person's own light again, waking over about half a second | The person's light was never touched: the colour and level they chose, kept across a restart with `LAMP_LIGHT_KEEP=on` |
| Once | Once each time the desk is left (`rearm = "ends"`) | It never goes out again until someone came back |
| Not in a turn | Only while `voice/state` is `resting`; forty minutes that come while the lamp speaks (a reminder into an empty room) wait up to an hour for it to rest (`wait = "1h"`) | |
| Not at night | Never starts from 23:00 to 06:00, the lamp's local time (`[quiet]`), Expression's night (`sleep.night` in the product catalogue) | The night is the lamp's sleep (below). A desk left since the night goes out at 06:00 |
| Ask | `[ask] policy = "light"`, an `unprompted` act for the Policy's gate; the product skips the ask (`LAMP_SKILLS_ASK=skip`) | |

**The ring's fade** (`lamp_msgs::LightRequest::fade_ms`, 1 to 60 s, #1677).
A skill's `out` step asks the ring on `light/request` for the light shown,
off, with its fade, renewed every 10 s while it holds. While that request is
the light shown, the face eases into it at rest with a time of a quarter of
the fade (`ring_face.rs`, `FADE_EASES`): 98% of the way at the fade's end,
the light falling as a share of itself, which the eye reads as an even
dimming. A turn's face over it comes as quickly as ever. When the request
ends the face settles back into the person's light as after a tap (about
half a second), waking rather than snapping on. A ring older than the field
ignores it and eases at its own 70 ms. Through the actions part (without
`LAMP_LIGHT_REQUESTS=on`) the step is left alone and the run says so: the
person's light is never turned off as theirs.

**Sleep owns the night.** The lamp sleeps two ways. Put to sleep from the
head (#711), the voice is muted with the reason `sleep`: the ring shows the
asleep face and the camera is closed, so nothing is said on
`room/presence`, the skill's ask ends, and nothing is out to give back.
Expression's night sleep (23:00 to 06:00, after 15 minutes with nobody) is
the head's pose only and leaves the light as it is; the skill does not
start then. A light that went out before 23:00 stays out until someone
comes, whatever the hour.

**"Keep the light on."** Said to the lamp ("keep the light on", "don't turn
your light off when I leave"), it is kept as a rule with the voice's typed
memory rules (`LAMP_MEMORY_RULES=on`): `keep_rule` with `about = "light"`
and `never`, kept in the room's scope (`r-everyone`) whoever says it, since
the light is one thing everyone at the lamp shares and it goes out only when
nobody is there (the first builder, 19 Oct). `memory/rules` then says
`keep_light_on: true`, and the skill's condition stops it:

    [[while]]
    topic = "memory/rules"
    field = "keep_light_on"
    not_equals = true
    unknown = "holds"

`unknown = "holds"` lets it run on a lamp without the rules switch, where
`memory/rules` says nothing; a value that has run out still does not hold.
With the switch on, the voice reads the room's rules once as it starts and
says them (`remember::tell_rules_at_start`, trying for up to a minute while
the memory starts), so the rule holds after a reboot before anyone has
spoken; a session that says its fact first is never replaced by it.
"You can turn your light off again" is `forget_rule` with `about = "light"`
and no number: every such rule of the room's goes, whoever asks (the room's
rules are not numbered for a known person). A person who asks for their
light while it is out gets it at once (the person's `light/command` is over
any skill's ask); said into a dark lamp, "keep the light on" is a voice
heard, so `someone` comes back and the skill's ask ends with it.

## Switching it on

1. `LAMP_PRESENCE=on` in `lamp-eyes`' unit, with `LAMP_BUS=on` (the
   product's has the bus). **On in the product since 2 Nov** (the first
   builder's yes on #4710), with `LAMP_PRESENCE_VOICE=addressed`: a skill
   made on the lamp on `room/presence` ("when I come back to my desk") ran
   only with it (lamp-9adf, 18:21). Light out reads it, on since #5746.
2. `LAMP_LIGHT_REQUESTS=on` in the voice's and `lamp-skills`' units (the
   product's have both), and `LAMP_LIGHT_KEEP=on` for the person's light
   across a restart.
3. `skills/light-out/` in `/var/lib/lampOS/skills`, enabled. Install puts
   it there as its own `skill.toml`, enabled, since #5746 (it was in
   `install::PRODUCT_SKILLS_OFF` until then).
4. `lamp-skills` with `--zone system` (the product's and lamp-4ace's) for
   the quiet hours; without a zone a skill with quiet hours never starts
   itself.

## Tests

- `crates/msgs`: the type, its check and its declaration (`tests/it/presence.rs`);
  `fade_ms`'s bounds and an older request without it (`arbitration.rs`).
- `crates/eyes`: `away.rs`'s judge (someone within 20 s, nobody counted from
  the last face or voice, nothing when the eyes stop, the count begun again
  after; a touch someone and the count begun from it, nothing while the eyes
  are shut, `a_touch_is_someone_and_starts_nobody_again_but_not_while_the_eyes_are_shut`;
  `the_view_changing_is_someone_by_moved`), `change.rs`'s
  `the_view_changing_counts_only_in_light_outs_dim` (another skill's light,
  the settle, a still room, a page, the dim unsaid, its out drawn, the light
  given back), and `bus_out.rs`'s `room/presence` on a real bus from looks and
  `voice/heard`; with `addressed`, a line heard and a turn held back nobody
  and an answered turn someone by voice
  (`with_addressed_only_an_answered_turn_is_someone`); a touch and a pet
  someone, the pads off nothing
  (`a_touch_or_a_pet_is_someone_and_starts_nobody_again`); a page turned
  nobody without the dim and someone by `moved` in it
  (`the_view_changing_is_someone_only_in_light_outs_dim`); the dim held through
  a late renewal (`change::the_dim_holds_through_a_late_renewal`); a stir after
  the out settles (`change::after_the_out_settles_the_view_changing_is_a_stir`)
  said at once for 3 s with nobody going on
  (`away::a_stir_is_said_at_once_for_three_seconds_and_nobody_goes_on`).
- `crates/voice`: the face fading into a light slowly, no frame falling by a
  tenth, a turn over it as quick as ever, the wake in about half a second
  (`ring_face.rs`); the ring giving the shown request's fade to the face,
  none under a self-test or once given back, a fade past a minute refused
  (`ring_light.rs`).
- `crates/skills`: the `out` step loaded and refused (`tests/it/skills.rs`), the
  program asking the ring for the light off with its fade and giving it back
  when the trigger ends, and leaving the light alone without the ring's
  switch; light-out itself on a stepped clock, the dim at forty minutes, the
  out five minutes on, and a face, a word, a touch or the view changing in
  the dim giving the light back with nothing out, and a stir after the out
  bringing the dim back and the out again five minutes later
  (`light_out_dims_five_minutes_then_goes_out_and_anyone_in_the_dim_brings_it_back`).
- `skills/light-out/tests/`, eight cases written by a script in the format
  `lamp-bus record` writes, `room/presence` every 2 s (each good for 3 s;
  half the eyes' once a second, so the forty minutes are half the lines):
  the dim at forty minutes, out five later and back when someone sits down;
  back at once when the view changes in the dim, nothing out; the dim back
  when the view changes after the out, out again five minutes later; waiting for
  the lamp to finish speaking; the night waited out to 06:00; nothing at
  night; back when the eyes stop looking in the dim; on when asked to keep
  the light on.

## The lamp check (Nine's)

On a lamp with the switches above, the office empty, nothing played:
1. Leave the desk: the light dims about 40 minutes after the last face and
   goes out 5 minutes later over about a minute; `skills/events` says
   `dim: asked of the ring at level N` and then
   `light out: asked of the ring, faded into over 60000 ms`.
2. Sit down: the light is back, as it was, within a second of being in
   view, measured from a camera's frames; then speak from out of the
   camera's view after it goes out again: back as well.
3. Read face down at the desk for 50 minutes, turning pages, no touch, no
   word: the light may dim, and comes back within 2 s of a page turned; it
   is never dark (#5746's B1, Fifth's kit).
4. Mute the lamp while it is out: the asleep face shows, `skills/events`
   says the ask ended within about 8 s (the eyes stop saying it 5 s after
   their last look, and the last value runs out 3 s later), and unmuting
   shows the person's light.
5. The eyes' cost with presence on: their CPU in an empty room, before and
   after, over 20 minutes.

## Not done, and what may be wrong

- A face that never moves (a poster, a photo on the desk, a face on a
  screen) keeps `someone` true, so the light never goes out there: the
  safe way to be wrong. The eyes leave such faces out only with
  `--leave-still-faces on` (off unless given, [switches.md](switches.md)).
  lamp-4ace's unit gives it and the product's does not, so on the product a
  still face keeps the light on (the reviewer on #1721).
- A voice from a call or a television counts as someone: the light stays on
  then, which is the safe way to be wrong.
- `light/reached` is said 350 ms into a fade, before the light is there; the
  eyes read it only for the dim, which has no fade.
- The view in the dim is judged against noise learned at every glance, so
  movement that goes on in one place for many minutes (a screen playing, a
  fan, a reader fidgeting in one spot the whole forty minutes) teaches its
  cells to be noise and may need a bigger change there to count; a page
  turned after stillness, or a shift in the chair, counts. A flicker the
  noise never settles to (a screen in view) keeps saying the view changed:
  the light then dims and comes back, and never goes out, the safe way to
  be wrong. Not yet measured on a lamp.
- The eyes know the dim and the out by their asker's name,
  `skill:light-out`: a skill of that name is the only one whose light the
  view changes.
- A dark room's own noise, or a screen in view, can stir the light back to
  its dim after the out; it then goes out again five minutes later. That
  is the safe way to be wrong, and its cost (a dim, then out, again) is for
  the row to measure.
- A restart of `lamp-skills` while the light is out lets the ask lapse
  within 15 s (the light comes back) and dims it again at its first
  `room/presence`, out five minutes later. A restart of the eyes begins the count of nobody
  again: the light comes back, and dims 40 minutes later.
- Messages' reading of the signal is Fourth's, in #1673.
