# #3945, a declined line on lamp-c0a4 with `LAMP_DECLINE_SILENT` off and on (29 Oct, 17:20–17:40Z)

No person was at the lamp. Every line was a synthetic voice played from the
Mac, which sits close to the lamp, at Mac output 60%, no extra gain.

- **Lamp:** lamp-c0a4, with its installed product units. lamp-voice ran from
  a local ARM64 build of this branch's change (`3247aa451e`, the same code
  before its rebase onto the newer lamp-voice) as a /run drop-in, with
  `LAMP_DECLINE_SILENT` set off or on in the drop-in. Speaker at 40%, cloud
  on. The lamp was restored after.
- **Played:** a call ("Lamp, what's a good name for a goldfish?"), then two
  remarks to a person ("Ma'am, did you see my keys anywhere?", "Sir, the
  meeting moved to Friday."):
  - **a** (`plays-a.txt`): three voices, each call followed by the remarks
    12 s apart, off then on;
  - **b** (`plays-b.txt`): two voices, on, the first remark about 46 s after
    the call;
  - **c** (`plays-c.txt`): one voice, on then off, the first remark about
    30 s after the call.
- **Journals:** `journalctl -u lamp-voice -u lamp-name` for each, in the
  lamp's local time (+07:00). They never hold the words heard.

**Read.**
- **The declines were silent and logged.** With the switch on, two remarks
  let out and declined by the model (`not_for_me`) said nothing:
  - a-on request 3, as "a conversation going on";
  - c-on request 20, as "the first thing said after the lamp's own news".

  Each journals `declined by the model: no conversation going on from it
  (LAMP_DECLINE_SILENT)`. With it off, the declines (a-off requests 2 and
  3, c-off request 3) were silent too: the model wrote no words before its
  call in any run here.
- **Not seen on the lamp:**
  - **Spoken words before a call:** no run reached the case where words
    stream before a `not_for_me` call, or a refusal is written as words.
  - **A decline carrying a conversation:** no run showed a line let out only
    because of an earlier decline. In a and c the second remark still came
    within the call's own conversation window. In b the first remark came
    after the window, so it was held, and nothing was declined.

  Both are covered by the unit tests in `second_provider/tests/decline_silent.rs`
  and `a_declined_line_leaves_no_conversation_going_on_for_the_next_line`.
- **Answered remarks:** in a-on (request 4) and in both halves of c
  (c-on request 18, c-off request 2), the model *answered* a remark to a
  person that was let out as a conversation going on. That is the model's
  choice on a line let out by the conversation window, not what this switch
  changes.
- **First audio on lines whose round 1 the switch holds:** the two answered
  remarks told the model "the model is told so" with the switch on came at
  1,693 ms and 1,904 ms (a-on request 4, c-on request 18). The off runs have
  no line of exactly that kind to compare with: a-off request 8 began over
  the lamp's answer, which the model is not told of, so it would not wait.
  The cost (round 1's whole reply before first audio) is left to measure;
  named calls don't wait: on 1,566, 1,404 and 1,972 ms, off 1,355, 1,850
  and 1,104 ms.
