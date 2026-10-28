# #3945 on lamp-c0a4, 28 Oct 2026

What ran: lamp-c0a4 on its product units (7071), with lamp-voice replaced for
the test by a local ARM64 build of `0bd5317fe4` (this change before its clean
rebase onto lamp-voice), through a `/run` drop-in that added
`LAMP_NAME_SAID_TO_A_PERSON=on`. Lamp speaker 40%, cloud on. The drop-in was
removed after and the product voice restarted; one test-input tap reopened the
microphone after each restart (the restart guard).

`journal.txt` is lamp-voice's journal from 15:58:30 to 16:06:30 (+07),
filtered to the second route's request lines, the name lines and the
microphone lines. Nothing else is edited out.

The lines, in order:

1. A synthetic voice played from the Mac (Mac system volume 70% for this test):
   "Ma'am, could you pass me the salt please?" three times, then "Sir, what
   time is it?" (requests 1 to 5). lamp-name raised no hearing on these.
   Request 1 was answered because a face was turned to the lamp.
2. A person, facing the lamp: "Lamp, what time is it in London?" and "Hey
   Lamp, are you there?" (requests 6, 7): heard, answered.
3. A person, turned away: "Ma'am, could you pass me the salt please?" three
   times (requests 8 to 12). Request 9 is #3945's case: lamp-name heard the
   name at 1.00 at the line's start, and the switch held it ("is said to a
   person ("ma'am") ... not counted"). The others raised no hearing.
