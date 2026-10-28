# Guard mode's tests

**How they were made.** Not on a lamp: guard mode has not run on one. Each
case's `.jsonl` files were written by a script in the format `lamp-bus
record` writes, as Light out's were: `room/presence` once a second, each good
for 3 s, someone until 20 s after the last face or voice, as the eyes say it
with `LAMP_PRESENCE=on`; `humans/faces/tracked` once a second, half a second
off, a face 0.2 of the picture's height while someone is near; the person's
ask on `voice/heard` at 2 s; the ring's word that the mark shows,
`light/reached`, at 4.35 s, in every case where the mark is asked for but
one; in one case a sample of `voice/privacy`, the
voice's lease, once a second; and in the two #2562 cases `ears/classes`
around 660 s, a door scored 0.91 at 660 s with the lamp's own voice clear.
The three cases named "real" hold real recordings' scores on `ears/classes`
instead: ESC-50 clips (CC BY-NC), scored on a Mac by the pinned YAMNet
through lamp-ears' own frontend and model, one patch each 0.5 s, numbers
only and no audio kept; a file scores higher than a room would through a
speaker and a microphone. What the eyes know of the face at the lamp is
not on the bus (it is the eyes' known file, which the program reads), so a
case says it as `[[known]]`: from `at`, that name, or with none, nobody
known. Each `expect.toml` sets the local time of the first message
(`clock = "14:20"`), and the model calls `keep_watch` at 4 s. The mark is
`light`, the mark given back `ends`, a text `text`, the one turn of the
head to an arrival `body.gaze` and "I'm off guard" aloud `say`. A replay
takes arrivals by sound as the program does with `LAMP_GUARD_SOUNDS=on`.

**Run** with `lamp-skills test lampOS/skills/guard-mode` (or `cargo test -p
lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result |
|---|---|---|
| `no-visit-is-counted-before-the-ring-shows-its-mark` | As the case below, with no `light/reached`: a ring that never says the mark shows (reviewer-two on #1937) | Pass: the mark at 4 s, and nothing else: the visit at 14:31 is not counted or texted |
| `a-visit-gives-one-text-with-its-time-and-length` | Asked at 14:20; the desk empty from 49 s; someone at 14:31 (660 s) for two minutes, a face it doesn't know | Pass: the mark at 4 s; the head turns to the face once at 661.5 s (#2562); one text at 799 s, when they've gone: "Someone I don't know was at your desk at 14:31, for about 2 minutes." |
| `a-door-with-no-face-texts-nothing` | #2562: asked at 14:20; the desk empty from 49 s; a door at 14:31 (660 s), and no face, voice or anyone after | Pass: the mark at 4 s, and nothing else: a sound alone is nobody coming in |
| `a-door-then-a-face-is-one-arrival` | #2562: as above, then a face it doesn't know at 668.5 s that stays at the desk to 739 s | Pass: the mark at 4 s; the head turns to the face once at 668.5 s; one text at 760 s: "Someone I don't know came in at 14:31 and was here for about 2 minutes." |
| `a-real-knock-with-no-face-texts-nothing` | #2562: as `a-door-with-no-face-texts-nothing`, with a real knock (ESC-50 `1-103999-A-30`): door 0.162 at 659.5 s, 0.944 at 660 s (YAMNet's Knock; its Door alone read 0.648), then 0 to 663.5 s | Pass: the mark at 4 s, and nothing else |
| `a-real-knock-then-a-face-is-one-arrival` | #2562: as `a-door-then-a-face-is-one-arrival`, with the same real knock | Pass: the mark at 4 s; the head turns to the face once at 668.5 s; one text at 760 s: "Someone I don't know came in at 14:31 and was here for about 2 minutes." |
| `real-footsteps-then-a-face-is-one-arrival` | #2562: as above, with real footsteps (ESC-50 `1-155858-A-25`): footsteps 0.364 at 659 s rising to 0.603 at 660 s and 0.622 at 660.5 s, then under 0.3 to 663 s | Pass: the same as the real knock's: one head turn at 668.5 s and one text at 760 s, "came in at 14:31" |
| `a-second-visit-within-ten-minutes-is-not-texted` | Visits at 14:31, 14:36 and 14:45, a minute each, heard but no face | Pass: texts at 739 s and 1579 s; none for the second, 300 s after the first text |
| `your-face-back-ends-it` | A, whose face the eyes know, asks and leaves; back at 14:40 and known at 1202 s | Pass: the mark at 4 s; the head turns to them at 1200.5 s, before they are known; the mark given back at 1202 s and "I'm off guard." said aloud (#2562); no text |
| `someone-known-is-said-as-known-never-by-name` | A asks and leaves; B, also known, at the desk for three minutes | Pass: the head turns to B once at 661.5 s; one text at 859 s, "Someone you've introduced me to was at your desk at 14:31, for about 3 minutes."; no name |
| `the-privacy-switch-closed-ends-it` | The switch closed from 300 s for 30 s, and again from 600 s | Pass: at 300.25 s the mark given back and one text, "Guard is off: the privacy switch is closed." (#2562); nothing at the second closing: it is off until asked again |
| `presence-after-the-switch-closed-is-no-visit` | The switch closed from 380 s to 500 s while `room/presence` still says someone at 400-470 s (a presence the eyes would not give: reviewer-two's P3 on #1807); open, someone at 700-770 s | Pass: the mark given back and the switch's one text at 380.25 s, and nothing after: no visit is counted once the switch has ended it |
| `never-on-from-a-text` | The tool called with nothing heard at the lamp, as a typed message would; a visit after | Pass: nothing, no mark and no text |
| `stop-gives-the-mark-back` | On, then "stop" at the lamp and `turn = "off"` at 300 s; a visit after | Pass: the mark given back at 300 s; the visit after is not texted |
