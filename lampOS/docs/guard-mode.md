# Guard mode

While you're out, the lamp tells you by text when someone is at your desk:
"Someone was at your desk at 14:31, for about 2 minutes." (Every line it says is its own, in its `skill.toml`'s `[watch.words]`, #940.) An installable
skill (`skills/guard-mode/`), off until you turn it on. Built to the first
builder's design on [#1685](https://github.com/autonomous-ai/lampOS/issues/1685)
(19 Oct, 12:08), from the owner's decision that guard mode is an installable
skill (19 Oct, 07:1x). Items 2 to 6 of [#2562](https://github.com/autonomous-ai/lampOS/issues/2562)'s
design add arrivals by sound, turning it on from the app, one turn of the
head to an arrival, "I'm off guard", and the privacy switch ending it.
Naming a known person (its item 1, the first builder's ruling of 22 Oct)
is behind its own switch, `LAMP_GUARD_NAMES`, off unless set: without it
nobody is named.

**Status:** built, with tests; not tried on a lamp, and out of the product's
install until it is (`install::NOT_PRODUCT_SKILLS`). **Its texts reach the
phone only with `LAMP_MESSAGES=on`** in `lamp-cloud`: they go on
`messages/out` on the lamp's bus, and `lamp-cloud` carries each up as the
backend's unasked news, which it keeps for the owner and pushes to their
phones ([messages.md](messages.md), "Unasked texts"). Off unless set; not
yet seen on a lamp.

## What it does

- **On only when asked, out loud.** "Lamp, keep an eye on things while I'm
  out." The model calls `keep_watch` with `turn = "on"`. The watch turns on
  only if the newest thing the lamp heard, within 30 s of the call
  (`[watch] asked_within`), asks for it in words: one of `[watch] if_heard`
  ("keep an eye", "keep watch", "watch my desk", "guard", "while I'm out",
  and a few more) among its whole words, any case. So a typed message from
  the app can't turn it on, nor can the model's own call after talk of
  something else (the peer read's P3 on #1807). It also needs its mark to be
  showable. Any voice at the lamp can ask, not only yours: the texts still
  go only to you, and the mark shows. Tying the call to the very turn that
  asked waits for the voice to give a skill's call its request number. Refused, the model is told why in words the lamp
  can say ("I keep an eye on things only when someone asks me out loud, at
  the lamp."). It never turns itself on: a watch has no trigger, and a load,
  a reload or a restart of `lamp-skills` leaves it off.
- **Off when you're back, in any words** (#2827, with
  `LAMP_GUARD_WORDS_V2=on` in `lamp-skills`). The model reads `keep_watch`
  in `lamp-skills`' `tools.json` (`/run/lampos-skills/tools.json`), which
  the voice declares as it is: the skill's `SKILL.md` description and its
  `[choice]` `about`. Those put on first and name "keep an eye" for it, and
  on lamp-06c1 (22 Oct 09:14) "I'm back. You can stop keeping an eye on
  things." was called `{"turn":"on"}`. With the switch, the tool's
  description and its choice put off first: off when you say you're back or
  here now, thank it for watching, or say it can stop or there's no need,
  even in "keep an eye" or "watch" words ("I'm back, you can stop keeping
  an eye on things", "thanks, I'm here now", "no need to keep watch"); on
  only when you ask it to start (`watch::WORDS_V2_DESCRIPTION`,
  `watch::WORDS_V2_TURN`). Without it, both are as the skill says, byte for
  byte. Only the words change: `[watch] if_heard` still gates on. Not yet
  measured on the real model.
- **It says so, and shows it.** The model says in one short line that it's
  on. While it's on, the ring keeps a faint mark, blue at level 1, asked of
  the ring at skill priority on `light/request` and renewed every 10 s, so
  nobody is watched without a sign. Without the ring's light requests
  (`LAMP_LIGHT_REQUESTS=on`) it refuses to turn on. **While the mark isn't
  shown, nothing is counted** (the peer read's P2 on #1807): until the ring
  says it is shown (`light/reached`), no visit begins, ends or is texted,
  and the journal and `skills/events` say so. That holds from the moment
  it's turned on, before the ring's first word, so a ring that never
  answers (no ring worker, the voice down) never has anyone watched
  without a sign (reviewer-two on #1937). It holds later too: the ring
  says the mark shows again after each renewal, about every 10 s, and
  when it has said nothing for 30 s (the ring worker stopped, or it forgot
  the claim, with no word that the mark is outranked), the mark is taken
  as hidden. The journal and `skills/events` say "its mark has not been
  confirmed for 30 s: no visit counted", and it counts again at the
  ring's next word (Third's privacy read on #1963). The ring says nothing
  inside a turn, whose face has colours of its own, so a turn longer than
  30 s pauses the counting the same way until the next renewal after it.
  When the ring says the mark
  is outranked (`light/outranked`), the first builder's four rules (#1937):
  1. **Your own light over it pauses it.** Say "turn the light off" on the
     way out, and the lamp makes no claim over your light and counts
     nothing. It texts you once: "I've paused watching while your light is
     off."
  2. **Another program's light over it** (Light out's, as the desk empties)
     **is asked over again:** the mark is given back and asked for with a
     new claim, at most once every 30 s. A light that outranks even that
     stays on top, and nothing is counted.
  3. **Paused, it goes on only when you ask again in words** ("keep an eye
     on things"): asking is your yes to the mark over your light. The mark
     is asked for anew, and it counts once the ring shows it.
  4. **Your light going on alone doesn't resume it:** it stays paused until
     you ask, or until its 12 hours end it.

  So the mark never replaces a light you just chose unasked, and the lamp
  never watches unseen.
- **From the owner's app** (#2562, with `LAMP_GUARD_FROM_APP=on`).
  `guard.on` and `guard.off` from the app, as `skills.remove` comes: on the
  lamp's own account channel only (`online::commanded`), so the owner's
  alone, and handed by `lamp-cloud` to `lamp-skills`' call socket
  (`/run/lampos-skills/call.sock`) as
  `{"op":"guard","turn":"on","deadline_us":...}`. `lamp-skills` takes that
  op only with the same switch in its own unit and only from `lamp-cloud`'s
  unit, by the peer's credentials (`SO_PEERCRED`) and cgroup, as the manage
  helper tells its askers; any other peer is refused, "guard mode is turned
  on and off from afar only by the owner's app", and a tool's call can't
  name the app's kinds.
  `deadline_us` is when `lamp-cloud` stops waiting for the answer: the
  moment it sent the op plus its 2 s (`watch::FROM_APP_WAIT`, shared by
  both programs), on the host's shared monotonic clock
  (`lamp_ipc::monotonic_us`, `CLOCK_MONOTONIC`). The call socket answers one
  connection at a time, and a voice call ahead can hold it 2 to 3 s, so an
  op can arrive after the app was already told nothing was turned. Read at
  or past its deadline, when it is read and again before the main loop
  turns anything, the op is let go, "guard.on let go: it waited N ms behind
  another call, past the app's 2 s", with no mark and nothing counted, so
  the lamp and the app never disagree. An op with no `deadline_us`, from an
  older `lamp-cloud`, is let go too; that costs nothing, since the switch is
  off by default. No voice is needed,
  but the mark rule holds: the mark is asked of the ring and nothing is
  counted until the ring confirms it. Turned on from the app, no face is
  the asker's, so no face ends it. Paused under the person's own light, it
  goes on only when asked at the lamp ("Guard is paused while your light is
  off: ask at the lamp for it to go on."). The app is answered
  `{"guard": "on"}` or `{"guard": "off"}`, or why not.
- **It counts from when you leave.** A visit counts only once the desk has
  been empty since you asked (`room/presence` says nobody). A visit is
  someone at the lamp: from when `room/presence` says someone (or a face at
  least 0.10 of the picture's height is seen) until it says nobody again,
  which the eyes say 20 s after the last face or voice.
- **Arrivals by sound** (#2562, with `LAMP_GUARD_SOUNDS=on`). The ears'
  room sound classes (`ears/classes`, `lamp_msgs::SoundClasses`, with
  `LAMP_EARS_CLASSES=on` in `lamp-ears`) now score a door and footsteps too
  (YAMNet's `Walk, footsteps`, and for a door the highest of `Door`,
  `Sliding door`, `Slam` and `Knock`: a knock on a door is AudioSet's
  `Knock`, not `Door`; `Tap`, which desk taps and typing raise, is left
  out). With nobody at the desk, a door
  or footsteps scored at least 0.5 in a window the lamp didn't speak into
  is someone come in only when a face (anywhere in view, or a known one) or speech
  (`voice/heard`, or the classes' speech at least 0.5) follows within 30 s:
  the visit then begins at the sound, and its text says so: "Someone I
  don't know came in at 14:31 and was here for about 2 minutes." **A sound
  alone texts nothing**: after 30 s it is let go, and the journal says
  nobody came in. The thresholds are not measured on a lamp. Without the
  switch the watch takes nothing from `ears/classes`, and `lamp-skills`
  listens to it only for another skill that reads it.
  A window the lamp didn't speak into: its classes' `own_voice` is `clear`,
  or `unknown` and the watch's own reading of `voice/state` says the lamp
  wasn't speaking over it. `lamp-ears` says `present` (the lamp spoke into
  it, never taken) or `unknown`, never `clear`, since its speaking flag
  maps a stale voice state to false (#2714). So `lamp-skills` keeps since
  when every `voice/state` reading, each within 3 s of the one before
  (the voice says it once a second), said anything but `speaking`, on the
  same monotonic µs clock as the window: a window that starts before that
  is taken, and one with the voice speaking in it, no fresh reading, or no
  reading at all is not. `clear`, from an older or later producer, is
  taken as before.
- **The head turns once** (#2562, rule 8). As a visit's first face is
  seen, one gaze to the largest face in view, on the body's gaze socket as
  a skill's look at a face sends it, then the body rests. No patrol, no
  scan, and no turn again within 2 minutes of the last, whoever comes. The
  ears give no direction, so an arrival heard but with no face turns
  nothing. A person coming back to the desk is turned to as well, before
  their face is known.
- **One text a while.** When a visit ends, one line on `messages/out`, at
  most one every 10 minutes (`[watch] every`): a visit that ends within 10
  minutes of the last text is not texted, and the journal says so. The time
  is the lamp's local time when the visit began (`--zone`), left out when
  the lamp doesn't know it; the length is rounded to the minute ("for under
  a minute", "for about a minute", "for about 2 minutes").
- **Known or not, never who.** "Someone you've introduced me to" when the
  eyes knew a face at the desk that isn't yours, "Someone I don't know" when
  a face was seen and not known, and "Someone" when there was a voice or a
  presence but no face near. Never a name: in a shared home, a name would
  tell you whom another person visited.
- **By name, only with a yes** (#2562 item 1, `LAMP_GUARD_NAMES=on` in
  `lamp-skills`, off unless set). A face the eyes know is named only when
  its person said yes to being named: the eyes' `tellable` file beside
  their known file, the same yes Who was here reads
  (`lamp_skills::known::tellable_in`). "Minh came in at 18:10 and was here
  for about 2 minutes." Two known people at one visit, one who said yes
  and one who didn't: "Minh and someone the lamp knows came in at 18:10
  and were here…" (`[watch.words]`' `named_and_known`; two names or three
  are joined by `and`, "Minh and Lan"; a visit keeps at most 3). More than
  one person says "were" (`visit_many`, `came_in_many`): "Minh and Lan were
  at your desk…"; one says "was", as before. Known but no yes stays
  "someone you've introduced me to", a stranger "someone I don't know".
  **Read as the text goes,** never as the face was seen: a yes taken back,
  or a forget, before the visit's text is sent names nobody in it. A name
  comes only from the eyes' known file, a face they know, never from a
  voice or from what someone says. The name is in the text alone: the
  journal and `skills/events` say "naming someone the eyes say said yes",
  never the words. Without a known file the switch names nobody, and the
  journal says so at start. **Checked again as it leaves the lamp:** a
  named text goes on `messages/out` with its words as they read with names
  off (`MessageOut::unnamed`) and who it names as the eyes' person ids,
  never names (`named`). `lamp-cloud` reads the eyes' `tellable` again just
  before it sends the text to the broker (`LAMP_CLOUD_KNOWN_FILE`, the same
  `lamp_msgs::known` reading), and sends the named words only if each one
  named still said yes; a yes taken back by any of them, a forget, or a
  file it can't read sends the unnamed words, as does a text it holds while
  the broker is away or sends again after a failed or lost send. So a text
  waiting on the bus or in `lamp-cloud` never names someone who took their
  yes back. And it goes named only straight to the wire: with a send of
  `lamp-cloud`'s own still not acknowledged by the broker, the MQTT client
  could hold it behind that one past the eyes' word, so it goes unnamed then
  too ([messages.md](messages.md), "Unasked texts"). **Too long
  named:** three long names can make the line longer than a text may be
  (280 bytes); then the unnamed words go alone, not nothing.
- **It ends** when you say stop (`turn = "off"`, which needs no voice), or
  `guard.off` from the app, or when the eyes know your own face at the lamp
  again after you'd left: the mark is given back, no text is sent for you
  coming back, and the lamp says aloud, as written, "I'm off guard." (the
  voice's say service, priority 5, spent if not begun within 30 s; #2562).
  **Or by itself,** 12 hours after you asked (`[watch] most`, 1 to 24 h),
  with one text, "I'm off guard: it's been 12 hours." So a watch nobody
  stopped doesn't text you about yourself the next morning (the peer read's
  P3 on #1807). If the eyes didn't know your face when you asked, the lamp
  asks you to tell it to stop when you're back.
- **The privacy switch ends it** (#2562). With the switch closed
  (`voice/privacy`'s `physical_muted`), the lamp can't see: the watch is
  off at once, its mark given back, with one text, "Guard is off: the
  privacy switch is closed.", and nothing more while the switch stays
  closed or is flicked again: it is off until asked for again. It doesn't
  go on while the switch is closed, by voice or from the app ("The privacy
  switch is closed, so I can't see: I won't keep an eye on things."). Once
  it has ended nothing about the desk is taken, whoever says it, so the
  promise is the watch's own, not only the eyes closing with the switch
  (reviewer-two's P3 on #1807). The switch is looked at about once a second
  and before each turn on.
- **Rule 8.** It doesn't patrol, scan the room or flash; its one sound is
  "I'm off guard", and its one move a single turn of the head to someone
  coming in. The lamp does what it always does, and answers anyone who
  speaks to it as usual; asked, it says it's keeping an eye on things.

## What it reads, and what it never does

Its `[permissions]` (#1698), and the program holds it to them where each
thing leaves `lamp-skills`:

| Permission | For |
|---|---|
| `topics`: `room/presence` | whether anyone is at the lamp: counts and times, no picture |
| `topics`: `humans/faces/tracked` | the faces' sizes, for a face near, and the largest face's place, for the one turn of the head |
| `topics`: `voice/heard` | that someone spoke just before the call: asked out loud |
| `topics`: `voice/privacy` | the privacy switch, looked at as it goes (the voice says it a hundred times a second, so it is read, never listened to) |
| `topics`: `ears/classes` | a door, footsteps and speech, as scores, with `LAMP_GUARD_SOUNDS=on` (#2562); no sound is read or kept |
| `topics`: `voice/state` | that the lamp wasn't speaking over a window the ears scored with its own voice unknown (#2562) |
| `known_faces` | whether the face at the lamp is one the eyes know, and whether it's yours, from the eyes' known file (`lamp-eyes --known-file`, read by `lamp-skills --known-file`); the name read there stays in the program, never in an event or the journal, and in a text only with `LAMP_GUARD_NAMES=on` and the eyes' `tellable` yes, read as it goes (#2562) |
| `messages` | one short line on `messages/out` |
| `light` | only its faint mark |
| `body` | only one turn of the head to an arrival, at most one in 2 minutes |
| `speak` | only "I'm off guard", as written |

No picture and no recording: it reads none, keeps none and sends none, of
video or of audio. A stranger is never more than "someone I don't know".

**A picture in the text** (the owner's open question on #1685) is not built.
It would leave the lamp, which loosens privacy. If wanted, it would be a
separate setting the person turns on themselves, with the risk said plainly
where they turn it on; it is not part of this skill.

## How it's built

A watch is the runtime's kind for a skill that is on only while a person has
asked for it ([skills.md](skills.md), "A watch"): `[watch]` in `skill.toml`
instead of triggers and steps, its tool's `[choice]` exactly `on` and
`off`. The logic is `crates/skills/src/watch.rs`, the program's wiring
`crates/skills/src/main.rs` (`watch_call`, `app_call`, `app_turn`,
`watched`, `watch_look`), the app's side `crates/cloud/src/guard.rs`, and
the text's type `lamp_msgs::MessageOut` (`crates/msgs/src/chat.rs`).

```toml
[watch]
every = "10m"
asked_within = "30s"
mark = { colour = "blue", level = 1 }
```

On a lamp it needs the eyes' `room/presence` (`LAMP_PRESENCE=on`), the ring's
light requests (`LAMP_LIGHT_REQUESTS=on` in the voice and `lamp-skills`), and
for the known-face words and your face ending it, `lamp-skills --known-file
/run/lampos-eyes/known` beside `lamp-eyes --known-file` (both run as
`lampos`, and the file is 0600). The product's `lamp-skills.service` gives
it as `Environment=LAMP_SKILLS_KNOWN_FILE=/run/lampos-eyes/known`, not as the
flag: `--known-file` is as new as guard mode, and an older `lamp-skills`
would end at its start on it (#2157), where it ignores the line. Without the
file, the watch knows no face, says "someone I don't know" or "someone", and
ends only when you say stop. Arrivals by sound need `LAMP_EARS_CLASSES=on`
in `lamp-ears` and `LAMP_GUARD_SOUNDS=on` in `lamp-skills`; turning it on
from the app, `LAMP_GUARD_FROM_APP=on` in both `lamp-cloud` and
`lamp-skills` (`lamp-cloud`'s unit may reach `/run/lampos-skills`). The
turn of the head needs the body's gaze socket (`--gaze-socket`), and "I'm
off guard" the voice's say service (`LAMP_SAY=on`).

## Tests

- `crates/skills/src/watch.rs`: never on by itself (nothing it sees turns it
  on; refused with nothing heard, heard too long ago, or no mark); one text
  for a visit with its time and length; a second visit within 10 minutes
  not texted; the face of the person who asked ending it, only after they
  left, and nobody else's; known or not, never a name; off when
  asked; nothing counted once the ring hasn't confirmed the mark for 30 s,
  and counting again at its next word
  (`a_mark_the_ring_has_not_confirmed_for_thirty_seconds_counts_nothing`).
  #2562's: `a_door_with_no_face_within_thirty_seconds_texts_nothing`,
  `a_door_then_a_face_gives_one_arrival` (and speech after footsteps),
  `a_second_arrival_within_two_minutes_does_not_turn_the_head`,
  `guard_from_the_app_is_on_without_a_voice_and_counts_only_once_its_mark_shows`,
  `only_lamp_clouds_unit_is_the_app`,
  `sounds_are_taken_only_with_the_switch_and_the_lamps_voice_clear`
  (unknown with the voice resting over the window, then a face, is one
  arrival; unknown while it spoke, with a stale state or none, and
  present, are none; clear still is),
  `the_privacy_switch_closed_ends_it_with_one_text`,
  `once_the_switch_has_ended_it_nothing_about_the_desk_is_taken`, and
  "I'm off guard" in `the_face_of_the_person_who_asked_ends_it`.
  #2562 item 1's names:
  `two_known_one_tellable_names_one_and_says_someone_the_lamp_knows_for_the_other`
  (and both named, neither, the order seen),
  `with_names_off_nobody_is_ever_named`,
  `a_yes_taken_back_before_the_text_goes_names_nobody` (a no and a
  forget), `a_voice_alone_never_names`,
  `a_watch_without_known_faces_names_nobody` and
  `the_named_lines_have_only_their_own_slots`; the named text's `unnamed`
  and `named` in the first;
  `names_too_long_for_one_line_send_the_unnamed_text_not_nothing`.
  `crates/cloud/src/unasked.rs`,
  `a_named_text_goes_named_only_at_once_and_is_held_unnamed`;
  `crates/cloud/tests/it/texts.rs`,
  `a_named_text_goes_named_only_while_its_yes_stands_as_it_goes` (on a real
  bus: named with the yes; the yes taken back before lamp-cloud first
  reads the waiting text, then sent connected, unnamed; held while away,
  unnamed; no known file, unnamed);
  `crates/cloud/src/online.rs`,
  `a_named_text_behind_an_unacknowledged_publish_goes_unnamed` (the real
  client and event loop, a loopback broker with a receive maximum of 1 and
  `info`'s PUBACK held: unnamed; nothing ahead: named);
  `lamp_msgs::chat::tests::a_named_text_carries_its_unnamed_words_and_older_readers_are_unaffected`,
  `lamp_msgs::known::tests::still_tellable_is_every_one_named_now_and_nothing_unverifiable`.
- `crates/skills/tests/it/skills.rs`,
  `guard_modes_new_words_come_only_with_their_switch` (#2827): with
  `LAMP_GUARD_WORDS_V2` on, `keep_watch`'s description and `turn` choice in
  the tools file are the new words, off first, inside the provider's 1024
  and 256 bytes; off, they are `SKILL.md`'s and `skill.toml`'s byte for
  byte; other skills' tools unchanged either way.
- `skills/guard-mode/tests/`: eleven written cases replayed through the
  watch, arrivals by sound taken as with `LAMP_GUARD_SOUNDS=on` ([tests/README.md](../skills/guard-mode/tests/README.md)). A
  replay looks at no time between messages, so one `light/reached` holds
  there: the 30 s rule and the 12 hours are the program's, tested in
  `watch.rs` and `skills.rs`.
- `crates/skills/tests/it/skills.rs`: the loader's refusals for a watch (a
  trigger, a choice other than on and off, a missing permission or topic, a
  mark that isn't faint and steady, a `[[do]]`, an unknown key such as
  `picture`), and the program on a bus of its own: a call with nothing
  heard refused, the mark on `light/request`, a visit's text on
  `messages/out`, and the asker's face back in the known file ending it and
  giving the mark back, with "I'm off guard" asked of a stand-in say
  service; and the ring going quiet while the mark is renewed:
  after 30 s a visit isn't counted or texted, and the ring's next word
  counts again
  (`guard_mode_counts_nothing_once_the_ring_has_not_confirmed_its_mark_for_30_s`).
  The program tests' stand-in ring says the mark still shows after each
  skip of the clock past 30 s, as a ring does after each renewal.
- `crates/skills/tests/it/skills.rs`,
  `guard_mode_from_the_app_is_the_owners_alone_and_the_switch_closed_ends_it`
  (#2562): the `guard` op refused without `LAMP_GUARD_FROM_APP=on` and from
  a peer that isn't `lamp-cloud`'s unit, a tool's call naming `guard.on`
  refused; an op with no `deadline_us`, one past it, and one read past it
  behind a slow call holding the socket let go, with no mark and guard
  still off; from the app's link, on with its mark, and the switch closed
  ending it with one text, and refusing it on again while closed.
- `crates/cloud/src/guard.rs`,
  `guard_from_the_app_is_handed_to_lamp_skills_call_socket`, the op
  carrying `deadline_us` 2 s after it was sent;
  `crates/skills/src/watch.rs`,
  `the_apps_op_past_its_deadline_or_with_none_is_let_go`; `guard.on` and
  `guard.off` not on this lamp without the switch
  (`unmuting_from_the_app_and_what_this_lamp_lacks_are_refused_plainly`);
  `lamp-cloud`'s unit reaching the call socket
  (`the_unit_may_reach_lamp_skills_call_socket_where_lamp_skills_makes_it`).
- `lamp_msgs::sound_classes::tests::scores_without_a_door_or_footsteps_read_as_none`.
- `lamp_ears::classes::tests::a_knock_a_slam_or_a_sliding_door_is_the_door_and_a_tap_is_not`.
- `lamp_msgs::chat::tests::a_text_sent_unasked_is_one_short_line_from_its_sender`.

## Not shown, and open

- Nothing ran on a lamp: the cases are written, not recorded.
- Delivery to a phone: built (`LAMP_MESSAGES=on`, [messages.md](messages.md)),
  not yet seen on a lamp and a phone.
- How the mark and Light out when you leave share the ring when both are
  installed: both ask at skill priority, and which the ring shows is the
  ring's arbitration. Not tried.
- A visit is texted when it ends, so someone who stays an hour is told
  after the hour.
- The eyes recognise a face at most every 2 s and only near: a known person
  seen briefly may be said as someone the lamp doesn't know.
- A typed call that comes within 30 s of someone speaking at the lamp would
  turn it on; the mark still shows it.
- #2562: the door and footsteps scores and their 0.5 are not measured in a
  room (a real knock and real steps by lamp-06c1's mic owe the bar's
  check: ESC-50 clips through a laptop speaker read under 0.01); the app's `guard.on` and the turn of the head are not yet seen on a
  lamp. On a host other than Linux no peer is `lamp-cloud`, so the `guard`
  op is refused there unless a test sets `LAMP_SKILLS_ANY_ASKER=on`.
- Names (#2562 item 1): not seen on a lamp. A text says "was here" also
  for two people.
