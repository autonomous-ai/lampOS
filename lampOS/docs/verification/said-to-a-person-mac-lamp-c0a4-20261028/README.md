# #3945, "Ma'am" lines from the Mac on lamp-c0a4 (28 Oct, 16:08–16:13Z)

No person was at the lamp. Every line was a synthetic voice played from the
Mac, which sits close to the lamp.

- **Lamp:** lamp-c0a4, with its installed product units. lamp-voice ran from
  a local ARM64 build of lamp-voice `08fd8f9e2b` (it has #4082) as a /run
  drop-in, with `LAMP_NAME_SAID_TO_A_PERSON=on`. The unit already sets
  `LAMP_NAME_SECOND_SIGN=on` and `LAMP_ATTENTION=busy-near`. Speaker at 40%,
  cloud on. The lamp was restored after.
- **Played** (`plays.txt`, UTC, when each line began): six synthetic voices ×
  three lines, about 14 s apart, then one control line ("Lamp, …"). Mac
  output at 60%, no extra gain.
- **`journal.txt`:** `journalctl -u lamp-voice -u lamp-name` from 16:08:40Z,
  as the lamp wrote it, in the lamp's local time (+07:00). The journal never
  holds the words heard.

**Read:** one hearing at or over the bar during a line, voice 2's "Ma'am,
could you pass me the salt please?" at 0.92, was answered (request 2: "its
name was heard in the room's sound"). The switch's first-word check never
shows for it. Request 8 shows that check ("its first word, set off, doesn't
sound like it"); request 2 does not. Four more of the 18 lines were answered:
request 6 as "the first thing said after the lamp's own news" (request 5 was
the lamp's own news), and requests 4, 8 and 9 as a conversation going on. So
the answered lines are requests 2, 4, 6, 8 and 9. Requests 3 and 7 were held
as said to someone named, and 11 as a short sound. A second hearing over the
bar, 0.95 at 23:11:48, went with request 10, which heard no words, so nothing
was answered. The control (request 12) was answered, "the text shows it was
called". The switch's hold line ("its first word, set off, is said to a
person") does not appear. Why request 2's first word wasn't checked, or what
else let it out (the Mac is close, `busy-near`), isn't in the journal. See
#3945. Counts corrected after Reviewer-two's read on #4170.
