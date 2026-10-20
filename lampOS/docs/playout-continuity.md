# Continuous reply playout

Scope: `MAIN-042`. An opt-in start policy in `crates/audio/src/playout.rs`,
its replay regression, and the evidence behind it. Offline only: I touched
no device and no cloud, and nothing here is an acoustic measurement. Room
and reply audio stayed outside Git. The policy is not wired into the
runtime; the wiring is a [separate proposal](#integration).

## Answer

1. **The four gaps were late supply, and the lateness has a fixed size.**
   The service keeps a reply only 13 ms to 110 ms ahead of real time. Now
   and then one burst arrives about 300 ms later than that, and playout
   runs dry unless it started far enough behind the first audio to make up
   the difference. Coffee, ocean and the story's second gap are each one
   such burst.
2. **A start that waits for a measured margin rides every recorded one out.**
   On 23 recorded replies the current 200 ms gate runs dry in 9 and the
   continuity start in none. The cost is the first block: 137 ms later on
   average, nothing for 5 replies, 100 ms to 110 ms for coffee and 190 ms to
   200 ms for ocean.
3. **Two starts miss the 200 ms target.** One short reply with no late burst
   is held 200 ms to 210 ms longer. The story's second generation is held
   290 ms where today it is not held at all.
4. **It is a delay, not a free gain.** A plain 600 ms horizon applied to
   every generation also runs dry in none of the 23, at the same average
   delay. What the measured margin adds is a shorter worst case and an even
   reserve. That comparison is [below](#against-a-longer-plain-horizon).
5. **The 2.27 s pause in the story is the model reasoning**, after a spoken
   acknowledgement that the service itself marks as not the end of the
   interaction. No buffer is an answer to it. See
   [the pause](#the-pause-in-the-story).

## What the lamps recorded

### Sources

Eight bundles, 23 replies that played to their end, 2,888 enqueue records,
2,755,209 frames: 115 s of speech. A reply cancelled or cut off by the end
of a run is not in the set.

| Label | Bundle | Lamp | Start gate when recorded | Replies |
|---|---|---|---|---|
| `six` | `lamp-4ace-varied-12io918n`, runtime `6e6c58b42` | `4ace` | 200 ms | 7, from 6 turns |
| `ten` | `lamp-rust-v1-ten-evidence-20261011T070623Z`, trial 1 | `a0ae` | none | 10 |
| `pilot` | `lamp-rust-first-pilot-20261011T014304Z` | `a0ae` | none | 1 |
| `ns1`, `ns2` | `lamp-rust-nscheck-evidence-20261011T024032Z`, trials 1 and 2 | `a0ae` | none | 1 each |
| `coord` | `lamp-rust-coordinated-evidence-20261011T030937Z`, trial 1 | `a0ae` | none | 1 |
| `play1`, `play2` | `lamp-rust-playout-evidence-20261011T033405Z`, trials 1 and 2 | `a0ae` | 200 ms | 1 each |

`six/4` is the coffee reply, `six/5` the ocean reply, `six/6` the story's
introduction and `six/7` the story. The fixture
`crates/audio/tests/it/playout_continuity/replies.tsv` carries, per reply, the
enqueue times and frame counts, the completion, the recorded first write
and the recorded runs of zeros, with the SHA-256 of each bundle's
`runtime/events.jsonl`. It has no audio and no text.

Times are the coordinator's enqueue times. In the six-turn run the provider
worker had stamped each piece up to 28 ms earlier. Nothing here sees the
socket or the service.

### The lead

Take the audio received before a burst and subtract the time since the
reply's first audio. That is what would still be in hand when the burst
arrived, had playout begun with the first audio. I call it the service's
lead.

It settles within the first second of a reply and then barely moves:
in the eight-second `six/3` it stays between 42 ms and 65 ms for 23 bursts.
The median over each reply is 13 ms to 110 ms across the 23. Sixteen of
them are between 91 ms and 110 ms. The seven smaller ones all belong to
replies that open with a single frame and deliver the speech afterwards.

So a reply played from its first audio has about a tenth of a second in
hand, for as long as it lasts.

### Late bursts

Against its own reply's lead, a burst was late by 60 ms or more 25 times.

| Late by | Bursts | Replies | Earliest, into the reply |
|---|---|---|---|
| 89 ms to 172 ms | 17 | 9 | 1.3 s |
| 295 ms to 306 ms | 8 | 8 | 2.0 s |
| more | 0 | | |

Nothing fell between 172 ms and 295 ms. The eight large ones are 295, 299,
299, 299, 302, 303, 304 and 306 ms, in replies recorded on two lamps over
several hours. I do not know what in the service produces that figure.

A start can also stall before there is a lead to speak of: `ten/3`
delivered 50 ms of audio and then nothing more until 349 ms after the
first, and `ns2/1` 50 ms and then nothing more until 303 ms.

### The four gaps

| Gap | Lead | Late burst | Playout began | Ran dry for |
|---|---|---|---|---|
| Coffee, 6.7 s in | 44 ms | 303 ms | 190 ms after the first audio | 70 ms |
| Ocean, 8.6 s in | 104 ms | 299 ms | 47 ms after | 160 ms |
| Story, 30 ms in | none yet | second burst 131 ms behind | 7 ms after, not held | 140 ms |
| Story, 18.2 s in | 91 ms | 295 ms | the 140 ms above had moved it later | 70 ms |

Each is the lateness, less the lead, less how far behind playout already
was, plus the 10 ms block the queue withholds until completion, to the
nearest slot.

In the accepted render three of the four cut speech that is at -13 dBFS to
-21 dBFS in the 10 ms on either side: they are inside words. The fourth is
30 ms into the story.

### The replay reproduces them

The regression replays the recorded enqueues through the real `StartGate`
and a queue that follows the coordinator's rules: one 240-frame block per
10 ms slot, the last block withheld until the generation is complete.
Started where each lamp's speaker started:

- the four gaps above come back at the same block and the same length;
- so do the gaps of 22 of the 23 replies, 21 of the 23 recorded runs of
  zeros;
- in the 23rd, `coord/1`, one block moves between its two gaps: 80 ms and
  140 ms recorded, 90 ms and 130 ms replayed.

That is what makes the before-and-after figures below worth reading. It
does not make them device results.

## V1 at `d5efe9d7b`

Read at the pinned commit. V1 has no start buffer to bring across.
`voice_service.py` and `tts/service.py` are under `hal/drivers/voice/`;
`base.py` and `gemini_live.py` are under `hal/realtime/voice_agent/`.

| | V1 | Rust, integration `1a089863a` |
|---|---|---|
| Start | The first chunk is written as soon as the speaker is claimed: `voice_service.py:1739` to `1750`. No prebuffer, no target lead | Optional horizon, 0 ms to 200 ms |
| Reply queue | Unbounded: `base.py:59`. A 30 s cap applies only while the speaker cannot be claimed: `voice_service.py:1733` | Bounded reply queue |
| Device | PortAudio stream asked for at least 120 ms of latency: `tts/service.py:70`, `360` to `372`. Blocking writes in 40 ms slices: `:66`, `:107`, `:1382` | ALSA, 10 ms blocks, one chunk outstanding |
| Nothing to write | Nothing is written. The device underruns, and that is logged: `tts/service.py:149` to `154`. The idle keepalive stays out of the way while speaking: `:403` to `:409` | Zeros are accepted every slot and each run is traced with the queue's state |
| End of a reply | `generation_complete` ends it without waiting for `turn_complete`: `gemini_live.py:1617` to `1621`. No drain | Generation complete marks the final block, and its retirement is a receipt |
| Later output in the same turn | Held off the speaker until the continuation is confirmed: `gemini_live.py:1352` | Joins the same queue |

Two things follow.

- **V1's start is less protected than the Rust gate, not more.** Whatever
  kept V1 continuous in a given run was the service's lead, or was not
  recorded: V1 logs an underflow and keeps no trace of its length. I could
  not determine from the source how much its device buffer delays a start
  after an underrun; that is PortAudio and the driver.
- **The completion rule is already the same.** Ending a reply on
  generation complete is what the Rust queue does.

## The policy

`StartGate::continuous(Continuity)` hands over a generation's first block
when a block can be handed over and either the generation is complete, or
both of these hold:

- **Cover.** Time since the first audio plus audio in hand is at least
  `cover_us`. This is the plain gate's rule. It decides only when very
  little has arrived.
- **Margin.** Time since the first audio plus the measured lead is at least
  `margin_us`, or the time since the first audio has reached `hold_us`.

Once open it stays open, as today. Under continuity the first audio after a
generation's final block has been handed over starts a new hold. Audio that
arrives before that joins the generation being played, with no hold and no
break.

The lead is measured as described above, from the reply's own arrivals,
taking the largest value so far. An arrival counts as a new burst when it
comes 10 ms or more after the one before: within a burst the pieces were at
most 5.4 ms apart, and separate bursts at least 14.5 ms, apart from bursts
sent back to back.

### The three numbers

`Continuity::EVALUATED`. They are not a live default.

| | Value | From |
|---|---|---|
| `margin_us` | 340 ms | The largest late burst recorded, 306 ms, plus the withheld block, one slot, and 14 ms for hand-over, which took 1.2 ms to 7.8 ms after the enqueue in the six-turn run |
| `hold_us` | 290 ms | The margin less the smallest lead recorded in a reply that had audio by then, 44 ms |
| `cover_us` | 380 ms | The longest recorded stall at a start: 50 ms of audio and then nothing until 349 ms. With 50 ms in hand this cover releases the first block at 330 ms, which bridges it |

A start released by the hold bound has not seen its lead yet. It then has
290 ms plus whatever the service turns out to lead by. That assumes a lead
of at least 44 ms. It held in every recorded reply that had its audio by
then, and it is not a guarantee.

### What does not change

- **A complete reply starts at once**, however short.
- **Samples.** The gate owns no PCM. It answers yes or no, and the caller
  hands over its own queue in order. The replay checks every block against
  the provider's sequence.
- **Cancellation.** Held audio has not left the caller's queue, so there is
  nothing to flush. `reset` forgets the reply and keeps the policy.
- **Generation boundaries.** A generation's final block is still the
  caller's to mark and pad. The gate only learns that it left.
- **Memory.** The gate is 128 bytes of counters and allocates nothing. The
  hold adds at most 380 ms of audio to the queue before the first block.
  The largest queue in the replay is 24,000 frames, one second, against
  23,521 with the current gate.
- **The plain gate.** `StartGate::new(horizon)` behaves as before. It
  ignores generation ends, and measures the lead without using it.

## Before and after

Replay of the recorded supply, twenty slot phases per reply. "Current" is
the plain 200 ms gate, with the story's generation not held, as deployed in
`6e6c58b42`. "Least in hand" is the least audio that could still be handed
over at any slot before the reply was complete.

| Reply | Speech | Lead | Bursts late by 60 ms or more | Current: first block | Current: zeros | Continuity: first block | Added | Least in hand |
|---|---|---|---|---|---|---|---|---|
| `six/1` | 3.8 s | 47 ms | 128 ms at 3.2 s | 354 to 363 ms | 0 | 354 to 363 ms | 0 | 30 ms |
| `six/2` | 2.0 s | 97 ms | none | 130 to 139 ms | 0 | 290 to 299 ms | 160 ms | 210 ms |
| `six/3` | 8.2 s | 56 ms | none | 343 to 352 ms | 0 | 343 to 352 ms | 0 | 70 ms |
| `six/4` coffee | 8.2 s | 44 ms | 89 ms at 4.8 s, 303 ms at 7.0 s | 186 to 195 ms | 70 to 80 ms | 290 to 299 ms | 100 to 110 ms | 30 ms |
| `six/5` ocean | 10.0 s | 104 ms | 139 ms at 4.8 s, 131 ms at 5.8 s, 299 ms at 8.8 s | 40 to 49 ms | 160 to 170 ms | 233 to 242 ms | 190 to 200 ms | 30 ms |
| `six/6` | 1.6 s | 83 ms | none | 113 to 123 ms | 0 | 290 to 299 ms | 170 to 180 ms | 240 ms |
| `six/7` story | 22.6 s | 91 ms | six of 128 ms to 142 ms, 295 ms at 18.4 s | 0 to 9 ms | 210 to 220 ms | 290 to 299 ms | 290 ms | 80 ms |
| `ten/1` | 6.1 s | 98 ms | 133 ms at 4.2 s, 172 ms at 4.7 s | 90 to 99 ms | 0 | 290 to 299 ms | 200 ms | 210 ms |
| `ten/2` | 4.6 s | 95 ms | 95 ms at 1.8 s, 134 ms at 2.6 s, 299 ms at 3.8 s | 100 to 109 ms | 110 to 120 ms | 290 to 299 ms | 190 ms | 80 ms |
| `ten/3` | 3.8 s | 91 ms | 104 ms at 1.3 s, 299 ms at 2.0 s | 150 to 159 ms | 150 to 160 ms | 330 to 339 ms | 180 ms | 30 ms |
| `ten/4` | 4.2 s | 99 ms | 302 ms at 2.8 s | 150 to 159 ms | 60 to 70 ms | 290 to 299 ms | 140 ms | 80 ms |
| `ten/5` | 4.2 s | 97 ms | none | 70 to 79 ms | 0 | 251 to 261 ms | 180 to 190 ms | 330 ms |
| `ten/6` | 3.2 s | 110 ms | 304 ms at 2.7 s | 90 to 99 ms | 110 to 120 ms | 290 to 299 ms | 200 ms | 90 ms |
| `ten/7` | 5.7 s | 110 ms | 131 ms at 5.0 s | 90 to 99 ms | 0 | 290 to 299 ms | 200 ms | 230 ms |
| `ten/8` | 3.3 s | 96 ms | none | 130 to 139 ms | 0 | 290 to 299 ms | 160 ms | 250 ms |
| `ten/9` | 3.3 s | 14 ms | none | 388 to 397 ms | 0 | 388 to 397 ms | 0 | 30 ms |
| `ten/10` | 4.0 s | 93 ms | none | 150 to 159 ms | 0 | 290 to 299 ms | 140 ms | 230 ms |
| `pilot/1` | 1.4 s | 92 ms | none | 70 to 79 ms | 0 | 276 to 286 ms | 200 to 210 ms | 360 ms |
| `ns1/1` | 3.7 s | 13 ms | 127 ms at 2.9 s | 393 to 402 ms | 0 | 393 to 402 ms | 0 | 70 ms |
| `ns2/1` | 3.3 s | 96 ms | none | 150 to 159 ms | 110 to 120 ms | 303 to 312 ms | 150 to 160 ms | 80 ms |
| `coord/1` | 3.7 s | 96 ms | 306 ms at 2.9 s | 100 to 109 ms | 120 to 130 ms | 290 to 299 ms | 190 ms | 70 ms |
| `play1/2` | 2.0 s | 93 ms | none | 308 to 317 ms | 0 | 308 to 318 ms | 0 to 10 ms | 70 ms |
| `play2/4` | 2.0 s | 65 ms | none | 80 to 89 ms | 0 | 266 to 276 ms | 180 to 190 ms | 320 ms |

First block times are from the reply's first enqueue. Under continuity no
reply accepts a zero before its end, in any phase.

| | Current gate | Continuity |
|---|---|---|
| Replies that run dry | 9 of 23 | 0 of 23 |
| Zeros accepted mid-reply, worst phase | 1,190 ms | 0 |
| Added to a reply's first block, 22 replies | | 137 ms on average; the middle two at 160 ms and 180 ms |
| The most added | | 210 ms |
| Not delayed | | 5 replies, by 0 ms to 10 ms |
| Least audio in hand at any slot | 0 | 30 ms |

### Where it misses the target

The target was no starvation in the reproduced patterns with at most
200 ms added to the first start.

- **Coffee and ocean meet it**: 100 ms to 110 ms and 190 ms to 200 ms. Ocean
  is at the limit, and so are `ten/1`, `ten/6` and `ten/7` at 200 ms.
- **`pilot/1` is held 200 ms to 210 ms longer.** It is 1.4 s long and had
  no late burst, so the hold bought it nothing. No start can know that in
  advance.
- **The story's second generation is held 290 ms** where today it is not
  held. In hindsight it needed 204 ms, plus the withheld block and a slot.
  A 200 ms hold would have removed the gap at 30 ms and left one or two
  blocks of zeros at 18.2 s. This delay lands after the 2.27 s pause and
  makes it 2.56 s.

The five replies that pay nothing are those whose audio arrives late
anyway: one frame, then the speech about 310 ms to 390 ms after it.

### Negative controls

In `crates/audio/tests/it/playout_continuity/mod.rs`:

| Test | What it holds to |
|---|---|
| `a_short_reply_that_is_complete_is_not_held` | Under continuity a 200 ms reply and a 60 ms reply start the moment they are declared complete. The same 200 ms without the flag waits out the 290 ms hold |
| `a_burst_later_than_the_margin_still_starves_and_playout_resumes` | A scripted service with a 100 ms lead: a burst 300 ms late is ridden out, one 700 ms late leaves 370 ms of zeros under continuity and 610 ms under the current gate, and every sample is still played once |
| `a_reply_cancelled_while_held_has_handed_nothing_over` | 320 ms had arrived, none had left, the opening receipt is empty, and the next reply is held from its own first audio |
| `a_second_generation_is_held_again_and_every_sample_is_played_once` | The sixth turn as two generations through one gate: 239 frames of final padding after the introduction and no other zero |
| `no_recorded_start_is_held_past_the_bound` | No first block later than 380 ms after the first audio once one could be handed over, and never less than 30 ms in hand |

## Against a longer plain horizon

Your root-cause report modelled 400 ms and 600 ms horizons on the six-turn
run. On all 23 replies, with the same replay:

| Start | Run dry | Zeros | Added on average | Most added | Over 200 ms | Story's own start | Least in hand |
|---|---|---|---|---|---|---|---|
| Plain 200 ms, as deployed | 9 | 1,190 ms | 0 | 0 | 0 | not held | 0 |
| Plain 200 ms, each generation held | 9 | 1,030 ms | 0 | 0 | 0 | 160 ms later | 0 |
| Plain 400 ms, each generation held | 7 | 330 ms | 68 ms | 200 ms | 0 | 200 ms later | 0 |
| Plain 500 ms, each generation held | 3 | 120 ms | 95 ms | 210 ms | 1 | 300 ms later | 0 |
| Plain 600 ms, story not held | 1 | 220 ms | 138 ms | 240 ms | 3 | not held | 0 |
| Plain 600 ms, each generation held | 0 | 0 | 138 ms | 240 ms | 3 | 310 ms later | 10 ms |
| Continuity, 340 / 290 / 380 | 0 | 0 | 137 ms | 210 ms | 1 | 290 ms later | 30 ms |

**A plain 600 ms horizon, held again for each generation, does nearly as
well.** It costs the same on average. Holding the later generation again is
what removes the story's gaps in both.

The difference is in what each rule looks at. A horizon counts the audio in
hand, and how much is in hand depends on whether a burst has just landed.
So the same 600 ms leaves ocean with 10 ms in hand at its late burst and
`ten/6` with 130 ms, and holds `ten/6` and `ten/7` 240 ms longer. The
margin rule counts how far behind the delivery playout runs, which is the
quantity a late burst eats. It leaves 30 ms or more everywhere and its
longest hold is 380 ms against 600 ms.

If you prefer the plain rule, the same code gives it:
`Continuity { margin_us: 0, hold_us: 0, cover_us: 600_000 }` is a 600 ms
horizon that is held again for each generation. The regression runs that
setting too, in `a_600_ms_horizon_for_each_generation_also_starves_none`.
The other plain rows of the table are from the Python model only.

## Measured and not built: longer pauses in place of a longer start

Part of the margin could come after the start: play sooner, and when the
reply reaches one of its own pauses, hold a few blocks there. Zeros between
two quiet blocks lengthen a pause; they do not cut a word.

I modelled it on the same 23 replies with the level of each accepted block:
a pause is three blocks at or under -45 dBFS, and each may be lengthened by
at most 40 ms.

| Start | Run dry | Added to the first block, average | Most | Story's own start | Added inside pauses, per reply |
|---|---|---|---|---|---|
| Plain 200 ms and pauses to 320 ms | 5 | 0 | 0 | 160 ms later | 90 ms |
| 200 ms hold and pauses to 320 ms | 0 | 80 ms | 160 ms | 200 ms later | 64 ms |
| 240 ms hold and pauses to 340 ms | 0 | 105 ms | 180 ms | 240 ms later | 55 ms |

Pauses alone are not enough: late bursts come as early as 2.0 s, before a
reply has paused. With a 200 ms hold the model reaches zero starvation at
57 ms less start delay than continuity on average, and it brings the story
to the 200 ms target.

I did not build it. It needs the coordinator to show the gate each block
before handing it over, it puts deliberate zeros into the accepted stream
that the gap accounting would have to tell apart from starvation, and
whether a lengthened pause is inaudible is a listening question. It is the
next step if 137 ms is too much. These figures come from a Python model
that gives the same per-reply table as the Rust replay for the current
gate and for two continuity settings. The pause part itself has no Rust
counterpart.

## The pause in the story

Kept apart from starvation, as asked. In the sixth turn, from the first
provider event:

| Time | Event |
|---|---|
| 0 ms to 1,212 ms | Nine audio events: 1.61 s of speech, a one-sentence acknowledgement |
| 1,528 ms | `turnComplete` carrying `interactionStatus: IN_PROGRESS`. The trace has it as `provider_turn_complete` with `idle: false` |
| 1,750 ms | The acknowledgement's last sample retires |
| 4,010 ms | The first audio of the story |
| 4,019 ms | Its first block is accepted |
| 26,539 ms | `turnComplete`, idle |

Between 1,212 ms and 4,010 ms the service sent no audio at all: 2.80 s. The
2.269 s you measured is the part after the acknowledgement had finished
playing.

**Likely origin: the model's background reasoning.** Three things point
the same way.

- The session runs `gemini-3.8-live-extended-thinking` at thinking level
  `LOW`. Google describes that model as adding background reasoning
  ([configuration notes](gemini-comparison-config.md)).
- The service closed the acknowledgement with a completion that says the
  interaction is still in progress. V1 calls an utterance of this kind a
  filler (`gemini_live.py:952`) and keeps output that follows a completion
  off the speaker until the continuation is confirmed (`:1352`).
- The fifth turn shows the same wait with nothing spoken. A completion with
  `idle: false` arrived 1,965 ms before its first audio, and that turn's
  endpoint-to-first-audio time is 3,742 ms against 1,296 ms to 1,492 ms in
  the other five.

So the model sometimes covers its reasoning with a sentence and sometimes
waits in silence. It is not transport: nothing was late, because nothing
had been sent. A start buffer cannot shorten it, and continuity makes it
290 ms longer.

**One narrow remedy: say that an answer is still coming.** The coordinator
learns at 1,528 ms, 222 ms before the acknowledgement stops sounding, that
the reply is not over. `ProviderOutput::TurnComplete { idle: false }`
already carries it. Used as an explicit boundary it lets:

- the lamp keep its working cue through the pause, so the pause reads as
  thinking and not as a finished or broken reply;
- the scorer count the acknowledgement as an interim, and time the answer
  from the story's first audio.

Nothing in the transport or the gate needs to change for that, and no
provider setting. It does not shorten the wait. Whether the plain
`gemini-3.8-live` model avoids it is a model comparison I did not run.

## Integration

Not on this branch. `lamp-a1-playout-integration` is this branch plus one
commit to your files. Its commit is named in
[my status](../coordination/agent-1.md):

| File | Change |
|---|---|
| `crates/voice/src/options.rs` | `PlayoutStart` carries the choice. `--playout-continuity` selects `Continuity::EVALUATED` and excludes `--playout-start-ms`. `PlayoutStart::gate()` builds the gate, `policy()` reports it. One test |
| `crates/voice/src/coordinator.rs` | `append_provider_audio` calls `start.arrival(at, frames)`. `next_speaker_chunk` calls `start.generation_ended()` on the final block. The reply is built with `playout_start.gate()`. `run_start` and the start receipt gain the policy, the measured lead and the reason |
| `crates/voice/src/main.rs` | The usage line |
| `crates/voice/src/coordinator/tests/playout_continuity.rs` | New: four tests through the production queue |

26 changed lines in `coordinator.rs`. With neither flag, and with
`--playout-start-ms`, the gate is the plain one and behaves as before; your
twelve `playout_start` tests pass unchanged, among them
`continuous_generation_is_not_rebuffered_and_previous_final_cursor_still_blocks`.

The production-path tests drive the coordinator's own
`append_provider_audio`, `may_start` and `next_speaker_chunk` at the
recorded enqueue times:

| Test | Result |
|---|---|
| `recorded_supply_runs_dry_behind_the_horizon_and_not_behind_continuity` | Coffee: 70 ms at block 672 behind the horizon, none behind continuity, first block at 190 ms and 290 ms. Ocean: 10 ms and 160 ms, none, 40 ms and 240 ms. The same samples in the same order |
| `a_second_generation_is_held_again_through_the_production_queue` | Sixth turn: 140 ms and 80 ms of zeros today, none under continuity, each generation's first block 290 ms after its own first audio, 239 frames of padding after the introduction and no other zero before the story's end |
| `the_receipt_names_the_policy_the_lead_and_the_reason` | The start receipt, for both gates |
| `a_continuity_hold_mints_no_permit_and_leaves_the_pcm_alone` | `dispatch_speaker_chunk` sends nothing and changes nothing while held, and sends the first block once the reply is complete |

In the trace a plain gate now reports `measured_lead_us` as well. That is
the one visible change with the option off, and it is how the lead can be
watched on a device before the policy is tried.

## Cost

| | Host, x86_64, release | Linux ARM64, emulated, release |
|---|---|---|
| One call to the gate | 2.0 ns | 24 ns and 28 ns in two runs |
| Calls per second of speech | about 125: one decision per slot and about 25 arrivals | |
| The gate | 128 bytes, no allocation | the same |

The lamp's own processor was not measured. The emulated figure is user-mode
QEMU on this Mac, which is slower than the device.

## Verification

Commands, commits and counts are in
[my status](../coordination/agent-1.md): format, strict Clippy and tests on
the host, and strict Clippy, tests and a release build for Linux ARM64
under emulation. Emulation is not the lamp.

## Not established

- **Anything acoustic.** Whether the replies sound continuous, and what the
  later start does to a conversation, is your physical A/B.
- **That the next late burst is no larger than 306 ms.** Eight samples of
  one size, cause unknown. A larger one runs dry under any bounded start.
- **That the service keeps at least 44 ms of lead.** It did in 23 replies.
- **Supply on another day, network or model.** All 23 replies are from one
  day, two lamps, one model.
- **Timing below the slot.** The replay has nominal 10 ms slots and no
  ALSA, IPC or scheduling. Its 30 ms reserve is against 1.2 ms to 7.8 ms of
  recorded hand-over, not against a loaded processor.
- **The generation-complete flag.** It was not traced, so the replay uses
  the turn completion that follows it. Real completion is earlier, which
  can only release a start sooner.
- **Longer pauses.** Modelled, not built, not heard.
