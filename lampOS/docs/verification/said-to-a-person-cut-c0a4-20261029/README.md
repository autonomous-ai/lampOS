# #3945 (b), cut lines on "Ma'am" from the Mac on lamp-c0a4 (29 Oct, 14:39–14:47Z)

No person was at the lamp. Every line was a synthetic voice played from the
Mac, which sits close to the lamp.

- **Lamp:** lamp-c0a4, with its installed product units. lamp-voice ran from
  a local ARM64 build of this branch's change (`73d51756bb`, before its rebase
  onto the newer lamp-voice, which touches only lamp-cloud and a bus test)
  as a /run drop-in, with `LAMP_NAME_SAID_TO_A_PERSON=on`. The unit already
  sets `LAMP_NAME_SECOND_SIGN=on` and `LAMP_ATTENTION=busy-near`. Speaker at
  40%, cloud on. The lamp was restored after.
- **Played** (`plays-1.txt`, `plays-2.txt`, UTC, when each line began): the
  cut line "Ma'am, could" + a 0.7 s or 1.0 s pause + "you pass me the salt,
  please? Sir, what time is this?", in four synthetic voices, about 22 s
  apart, then one control line ("Lamp, what time is it in London?"). Run 1
  with Mac output at 60%, run 2 at 70%. No extra gain.
- **`journal-1.txt`, `journal-2.txt`:** `journalctl -u lamp-voice -u
  lamp-name` for each run, as the lamp wrote it, in the lamp's local time
  (+07:00). The journal never holds the words heard; the one quoted line is
  the lamp's own fixed stop answer.

**Run 1 (60%).** All eight cut lines were held (requests 4–16): "Ma'am"
alone as a short sound, the rest as "nobody spoke to the lamp", and one
"Sir, …" line as "said to someone it calls by name". The control
(request 19) was answered, "its name … counts by: its words calling it, its
hearing at the line's start". lamp-name made no hearing over the bar on
any "Ma'am", so the new cut-line path was not reached.

**Run 2 (70%).** It began 39 s after run 1's control was answered, so its
first lines fell inside that conversation:

- Requests 20–22 are one cut line. lamp-name heard "Ma'am" near the bar
  (0.66), and `LAMP_NAME_NEAR`'s first-word check said it wasn't the name.
  Request 22 ("you pass me the salt …") was let out as "a conversation with
  the lamp is going on". Request 23 ("Sir, …") was held, "no conversation
  going on" (38 s since), and request 22's words were answered in its place
  (`LAMP_JOIN_CUT_WORDS`).
- That answer kept the conversation going: request 24 ("Ma'am") and 25 were
  let out as a conversation going on; 25 was taken as a stop said over the
  lamp's answer. Request 26 ("Sir, …") was held as said to someone named, and
  25's words were answered in its place.
- Request 28: a hearing over the bar (0.95) began 2,082 ms into the line, not
  at its start, so it counted for nothing alone; the line was let out as a
  conversation going on. Requests 30, 31 and 42 the same.
- Requests 32–40 were held as "a busy room". The control (request 41) was
  answered, "the text shows it was called".

**Read.** Neither run reached the new path: no hearing over the bar at a
"Ma'am" line's start. The fix is shown by its unit tests, which rebuild the
cut "Ma'am" line from the person's earlier run in #3945 that was let out. Run 2 shows a
separate case, outside this change: a line said to someone else, inside a
conversation still going on, is answered, and a held "Sir, …" line makes the
cut words before it answered in its place. Noted on #3945.
