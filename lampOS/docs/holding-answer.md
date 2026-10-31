# Finishing a holding-only answer (#1579)

With `LAMP_HOLDING_ANSWER=on`, a Gemini reply to heard input that ends after
saying only a short holding phrase can receive one written continuation on the same
session: “Go on: you said you would, so say it now.” The switch is off by
default. This repairs the conversation already in progress; it starts no
independent skill or background conversation.

## What exists and what is reused

V1's `hal/realtime/voice_agent/gemini_live.py` checks whether a spoken
response completed the request when extended thinking is enabled. Its
`response_outcome.py` treats a promise or acknowledgement as incomplete;
routing, interruption, rejection and playback acknowledgements constrain
fallback. It can hand unresolved work to the main route. It does not have
this exact holding-phrase continuation.

Keep those exclusions and a bounded attempt. Reuse V2's provider lineage,
written-turn owner, speaker retirement, capture freshness and one-second
quiet check. The continuation is deliberately narrower than V1's semantic
outcome check: no extra model, dependency, network request for classification,
replacement session or repeated retry. A fixed English phrase list may miss
other ways of asking someone to wait.

## Which answers qualify

The provider keeps at most 256 bytes of transcript for its current request
while the switch is on. It concatenates streaming fragments and checks the
whole answer only at an idle `TurnComplete`. A generation end or a non-idle
completion is not enough: substantive words in a later generation still
disqualify the answer. Overflow, failure, interruption or a new request
discards the candidate. Every function call disqualifies it, including a
silent call or one later held or rejected. An answer held by attention or
the tool policy does not qualify. Reminder output is excluded in the worker;
the coordinator also excludes every turn started by the lamp itself, including
reminders, skills, news and camera follow-ups.

Matching ignores ASCII case, surrounding terminal punctuation and repeated
whitespace. The complete text must be one of:

- “let me think”, “let me think for a moment”, “let me think for a second”;
- “one moment”, “just one moment”, “one second”, “just a second”, “just a moment”;
- “give me a second”, “give me a moment”;
- “let me check”, “let me check that”, “let me check that one”, “let me
  check on that”, “let me look that up”, “let me look it up”, “let me find
  out”: a check promised with no call. On lamp-4ace on 31 Oct “Let me check
  that one.” about an Aeron chair was the whole answer, with no search
  (#4684's line 3); the continuation lets the search be made.

A quotation, any added words, or a phrase followed by the answer does not
match. “Let me see” is excluded even on its own: it can invite someone to
show a drawing or object instead of promising an answer. No transcript is
added to the coordinator's diagnostic record.

## When the continuation may start

The coordinator accepts the classification only for its current request
and confirmed provider session, and pins the opportunity to that worker's
incarnation. Equal session numbers on different voice routes do not match.
The queued continuation cannot be held, rebound or replayed on a fallback
route. It waits for successful completion and all
normal speaker retirement receipts. A silent, failed or gapped reply gets
no continuation. A hand stop, mute, capture gap, replacement session or new
turn clears the opportunity; an opportunity that cannot start within three
seconds after the spoken reply settles expires.

Before creating its written turn, the coordinator requires fresh capture,
listening readiness, idle admission with no prospective input, an empty
provider queue, no speaker work or playback tail, and at least one second
since the last detected speech and the current listening cycle's start.
For this quiet check, validated capture frames keep their original speech
probability even when busy-room or faint-sound admission suppresses them.
This conservatively includes the lamp's own voice: it can add a second after
the holding phrase rather than hiding a person's speech under that phrase.
Nothing starts while the keeper has paused the voice. The normal privacy,
cancellation and output checks still apply after the note is queued. A
holding-only response to this note cannot trigger a second consecutive note.

The note takes the existing written-turn path. Its diagnostic event,
`holding_answer_nudged`, names the original turn, next turn and provider
session. This records a queued continuation, not evidence that anybody
heard an answer. The second voice route does not produce this classification.

## On the second route

The same switch works on the second voice route (the first builder's 22:54
on #1722); the worker reads Gemini's worker's `--holding-answer` as it reads
every other flag (`Engine::holding_answers`). The route answers each turn
whole, so the check is made there once the turn's words are out:

- **Qualifies:** a spoken request, with nothing called in any round, whose
  words said, all of its sentences joined, match the same list
  (`holding_answer::holding_only`).
- **Then** `ProviderOutput::HoldingAnswer` goes for the request and the
  session it was begun in, before its idle `TurnComplete`, as the Live
  worker sends it. The coordinator's continuation is the same: once the
  room is quiet, one written turn of the note to the same voice.
- **Never:** after a call (a timer may be set; a look asked by its call
  counts, so the look is never answered twice), with more words, for the
  lamp's own written turn, or a turn that failed.

Tests: `second_provider`'s
`a_holding_only_answer_is_told_so_the_route_is_asked_to_go_on` (the route),
and `coordinator::tests::second_route::a_holding_answer_is_asked_to_go_on_once_on_the_second_route_as_on_live`
(the loop, on Live and on the second route alone, and nothing with the
switch off).

## Verification and latency boundary

The host tests drive the real provider worker with a scripted Gemini
transport and step the coordinator's owner clock. They cover chunked holding
phrases, substantive continuations, function calls, disabled behavior, the
one-second boundary, stale capture, pending output/input, mute, session
replacement, route changes and the one-attempt limit. The worker and owner
checks use separate fixtures; they do not establish a complete process-level
conversation round trip. A stepped production-loop fixture retires the
holding reply through local speaker receipts, delivers a capture-gap event
half a second later, reopens capture and keeps fresh quiet for another second.
It reaches the loop's cancellation branch and checks that no continuation
is queued or sent; the same fixture without the gap must queue exactly one.
Pure argument tests also pin the worker flag to the exact `on` value.
The continuation adds no sleep or
timer: once its quiet and ownership conditions hold, it is queued in that
coordinator pass. The local target is one normal pass after those conditions
hold; the scripted check has zero added clock time at that boundary.

Not shown: acoustic timing, recognition accuracy, or whether Gemini supplies
the promised answer. The first builder's lamp check is five rounds of the
hand-over scenes with the switch off and on, counting holding-only turns
and measuring from the holding phrase's last audible word to the promised
answer's first audible word. No lamp settings or deployments are changed here.
