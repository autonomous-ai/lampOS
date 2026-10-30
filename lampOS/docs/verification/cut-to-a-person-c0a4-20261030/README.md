# #3945, cut words answered in a held line's place, from the Mac on lamp-c0a4 (30 Oct, 04:48–05:29Z)

No person was at the lamp. Every line was a synthetic voice played from the
Mac, which sits close to the lamp.

- **Lamp:** lamp-c0a4, with its installed product units (they set
  `LAMP_JOIN_CUT_WORDS=on`, `LAMP_NAME_SECOND_SIGN=on` and
  `LAMP_ATTENTION=busy-near`). lamp-voice ran from a local ARM64 build of
  this change (`550c8ee069`, before its clean rebase onto lamp-voice
  `cd735246d7`, which adds only the language work, off without its file) as a
  /run drop-in. Off is the same as lamp-voice without this change: the new
  code runs only with the switch on. Speaker at 40%, Mac output at 60%, no
  extra gain, cloud on. The lamp was restored after.
- **Played** (`plays-off.txt`, `plays-on.txt`, UTC, when each part began):
  a call, "Lamp, what time is it in London?", then 42 s later, near the end of
  that conversation's window, "Ma'am, could" + 0.7 s + "you pass me the salt,
  please?" + 0.7 to 1.0 s (`G`) + "Sir, what time is this?". One voice.
- **`journal-off.txt`, `journal-on.txt`:** `journalctl -u lamp-voice -u
  lamp-name` for each run, as the lamp wrote it, in the lamp's local time
  (+07:00). The journal never holds the words heard.

**Off (`LAMP_NAME_SAID_TO_A_PERSON=off`), 3 rounds.** The third (G = 0.9 s)
reproduced #3945's run 2: request 51 ("you pass me the salt, please?") was
let out as a conversation going on, and ended before its answer when request
52 ("Sir, what time is this?") began. Request 52 was held as "said to someone
it calls by name, not to the lamp", and "request 52 is held, so request 51's
words, cut short by it, are answered in its place". In the other two rounds
the salt line was answered on its own (request 42) or held (request 47) before
"Sir, …" began.

**On (`LAMP_NAME_SAID_TO_A_PERSON=on`), 12 rounds.** The case was not reached:
in none did "Sir, …" begin before the salt line's answer had begun, so no
words were cut short before their answer and nothing waited for a held line's
verdict. "Sir, …" was held as "said to someone it calls by name" (requests 4,
30, 33), as "a reply to someone the room had just called" (21), or as "nobody
spoke to the lamp" (12, 26, 51), and the salt line was answered or held on its
own. Whether the cut comes in time depends on how fast the answer starts,
which a played clip can't set. The 8 more rounds below reached it once.

**On, 8 more rounds (`journal-on-2.txt`, `plays-on-2.txt`, 05:19–05:29Z,
G = 0.9 s).** The case was reached once, in the second round. Request 10
("you pass me the salt, please?") was let out as a conversation going on,
then "ended before its answer: a newer line began". Request 11 ("Sir, what
time is this?") was held as said to someone it calls by name. The lamp
wrote "10's words, cut short, are let go: they or request 11 open with
"sir", said to a person (LAMP_NAME_SAID_TO_A_PERSON)", and nothing was
answered in request 11's place. This build is `550c8ee069`, before the two
read fixes on the PR (cut words that call the lamp are kept; the let-go
words' unplayed answers leave the thread). Their journal line is the
older one.

**Seen as well, outside this change:** inside a conversation, "Sir, what time
is this?" and the salt line were let out and answered as "a conversation with
the lamp is going on" (requests 42, 47 in `journal-on.txt`), the case noted on
#3945 on 29 Oct.
