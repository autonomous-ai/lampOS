# Guard mode

While you're out, the lamp tells you by text when someone is at your desk:
"Someone was at your desk at 14:31, for about 2 minutes." An installable
skill (`skills/guard-mode/`), off until you turn it on. Built to the first
builder's design on [#1685](https://github.com/autonomous-ai/lampOS/issues/1685)
(19 Oct, 12:08), from the owner's decision that guard mode is an installable
skill (19 Oct, 07:1x).

**Status:** built, with tests; not tried on a lamp, and out of the product's
install until it is (`install::NOT_PRODUCT_SKILLS`). **Its texts reach no
phone yet:** they go on `messages/out` on the lamp's bus, and nothing carries
that topic off the lamp until `lamp-cloud` takes it and the app team's push
relay delivers it (mobile-P1; [messages.md](messages.md), "Unasked texts").

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
- **It counts from when you leave.** A visit counts only once the desk has
  been empty since you asked (`room/presence` says nobody). A visit is
  someone at the lamp: from when `room/presence` says someone (or a face at
  least 0.10 of the picture's height is seen) until it says nobody again,
  which the eyes say 20 s after the last face or voice.
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
- **It ends** when you say stop (`turn = "off"`, which needs no voice, so the
  app could stop it too), or when the eyes know your own face at the lamp
  again after you'd left: the mark is given back, and no text is sent for
  you coming back. **Or by itself,** 12 hours after you asked (`[watch]
  most`, 1 to 24 h), with one text, "I've stopped keeping an eye on things."
  So a watch nobody stopped doesn't text you about yourself the next morning
  (the peer read's P3 on #1807). If the eyes didn't know your face when you asked, the
  lamp asks you to tell it to stop when you're back.
- **The privacy switch wins.** With the switch closed (`voice/privacy`'s
  `physical_muted`), the lamp can't see: one text, "I can't see right now,
  the privacy switch is closed.", once each closing, and quiet while it
  stays closed. A closing within a minute of the last line isn't texted
  again, so a switch flicked to and fro isn't a stream of texts (the peer
  read's P3 on #1807: a second closing was never told). While it is closed the watch takes nothing it is told about the
  desk, whoever says it: no visit begins or ends and none is texted, and
  the asker's face ends nothing. So the promise is the watch's own, not
  only the eyes closing with the switch (reviewer-two's P3 on #1807).
- **Rule 8.** It doesn't turn to scan the room, flash or make a sound. The
  lamp does what it always does, and answers anyone who speaks to it as
  usual; asked, it says it's keeping an eye on things.

## What it reads, and what it never does

Its `[permissions]` (#1698), and the program holds it to them where each
thing leaves `lamp-skills`:

| Permission | For |
|---|---|
| `topics`: `room/presence` | whether anyone is at the lamp: counts and times, no picture |
| `topics`: `humans/faces/tracked` | the faces' sizes, for a face near |
| `topics`: `voice/heard` | that someone spoke just before the call: asked out loud |
| `topics`: `voice/privacy` | the privacy switch, looked at as it goes (the voice says it a hundred times a second, so it is read, never listened to) |
| `known_faces` | whether the face at the lamp is one the eyes know, and whether it's yours, from the eyes' known file (`lamp-eyes --known-file`, read by `lamp-skills --known-file`); the name read there stays in the program, never in a text, an event or the journal |
| `messages` | one short line on `messages/out` |
| `light` | only its faint mark |

No picture: it reads none, keeps none and sends none. No body, no sound.

**A picture in the text** (the owner's open question on #1685) is not built.
It would leave the lamp, which loosens privacy. If wanted, it would be a
separate setting the person turns on themselves, with the risk said plainly
where they turn it on; it is not part of this skill.

## How it's built

A watch is the runtime's kind for a skill that is on only while a person has
asked for it ([skills.md](skills.md), "A watch"): `[watch]` in `skill.toml`
instead of triggers and steps, its tool's `[choice]` exactly `on` and
`off`. The logic is `crates/skills/src/watch.rs`, the program's wiring
`crates/skills/src/main.rs` (`watch_call`, `watched`, `watch_look`), and the
text's type `lamp_msgs::MessageOut` (`crates/msgs/src/chat.rs`).

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
`lampos`, and the file is 0600). The product's `lamp-skills.service` doesn't
pass `--known-file` yet: there, the watch knows no face, says "someone I
don't know" or "someone", and ends only when you say stop.

## Tests

- `crates/skills/src/watch.rs`: never on by itself (nothing it sees turns it
  on; refused with nothing heard, heard too long ago, or no mark); one text
  for a visit with its time and length; a second visit within 10 minutes
  not texted; the face of the person who asked ending it, only after they
  left, and nobody else's; known or not, never a name; the privacy switch's
  one line, and nothing about the desk taken while it is closed; off when
  asked; nothing counted once the ring hasn't confirmed the mark for 30 s,
  and counting again at its next word
  (`a_mark_the_ring_has_not_confirmed_for_thirty_seconds_counts_nothing`).
- `skills/guard-mode/tests/`: nine written cases replayed through the
  watch ([tests/README.md](../skills/guard-mode/tests/README.md)). A
  replay looks at no time between messages, so one `light/reached` holds
  there: the 30 s rule and the 12 hours are the program's, tested in
  `watch.rs` and `skills.rs`.
- `crates/skills/tests/skills.rs`: the loader's refusals for a watch (a
  trigger, a choice other than on and off, a missing permission or topic, a
  mark that isn't faint and steady, a `[[do]]`, an unknown key such as
  `picture`), and the program on a bus of its own: a call with nothing
  heard refused, the mark on `light/request`, a visit's text on
  `messages/out`, and the asker's face back in the known file ending it and
  giving the mark back; and the ring going quiet while the mark is renewed:
  after 30 s a visit isn't counted or texted, and the ring's next word
  counts again
  (`guard_mode_counts_nothing_once_the_ring_has_not_confirmed_its_mark_for_30_s`).
  The program tests' stand-in ring says the mark still shows after each
  skip of the clock past 30 s, as a ring does after each renewal.
- `lamp_msgs::chat::tests::a_text_sent_unasked_is_one_short_line_from_its_sender`.

## Not shown, and open

- Nothing ran on a lamp: the cases are written, not recorded.
- Delivery to a phone (above).
- How the mark and Light out when you leave share the ring when both are
  installed: both ask at skill priority, and which the ring shows is the
  ring's arbitration. Not tried.
- A visit is texted when it ends, so someone who stays an hour is told
  after the hour.
- The eyes recognise a face at most every 2 s and only near: a known person
  seen briefly may be said as someone the lamp doesn't know.
- A typed call that comes within 30 s of someone speaking at the lamp would
  turn it on; the mark still shows it.
