# One spoken turn through the core loop, from the code (2026-10-29)

Core-verifier, for #4305 item 1. A **static** trace: read from `lamp-voice` at
`dc8fdc69d2`, the product unit
`deploy/product/lamp-voice.service` (`LAMP_VOICE_ROUTE=second`: Deepgram,
DeepSeek through the relay, ElevenLabs `eleven_v4_turbo`; `LAMP_BUS=on`,
`LAMP_BUS_AUDIO=on`). Nothing was run on a lamp. The same turn read from a
lamp's journal and bus is still to come; the grep-able lines for it are given
at each hop.

A turn leaves two records: the systemd journal (the `eprintln` lines quoted),
and the session trace (`record()`, `coordinator.rs:3159` →
`session_state.rs:137`), JSON lines with a `kind` under
`/var/lib/lampOS/voice/sessions/<INVOCATION_ID>/metadata/events-*.jsonl`.
Most core hops show only in the second.

| # | Hop | Where | Journal / trace kind |
|---|---|---|---|
| 1 | Microphone, canceller, speech probability: the capture worker sends each 10 ms block as `MicrophoneFrame` (`wire.rs:382`) over private IPC, not the bus | `audio_workers.rs:441`, send `:1055` | `capture: microphone busy at start` (contention only) |
| 2 | The core receives frames; with `LAMP_BUS_AUDIO=on` republishes the post-canceller sound on `mic2/audio` (`lamp_msgs/Audio`, `json+pcm`, 20 ms) | `coordinator.rs:8265`, `:8396` → `bus_out.rs:231` | — |
| 3 | Wake: `lamp-name` subscribes `mic2/audio`, publishes `name/heard`, `name/rising`, `name/near`, `stop/heard`; the core waits on `name/rising` (`--call-wait-ms 2000`) | `name/src/main.rs:481-500`, `:733`; `coordinator.rs:6118`, `:13658` | `lamp-name: heard its name: score=`; kind `call_wait` |
| 4 | Admission: `owner.admit` (lamp-turn-owner), then `ProviderInput::Start` and the held audio | `coordinator.rs:13746` (from `:9574`), `:13824`, `:14607` | kinds `input_candidate`, `input_admitted`, `input_candidate_rejected` |
| 5 | End of speech: `TurnDetector` (600 ms) confirmed by `lamp-turn-end` (`--endpoint smart-turn-fast`), then `ProviderInput::End` | `activity.rs`; `coordinator/endpoint.rs:3`; `coordinator.rs:10076`, `:14677` | kinds `local_endpoint`, `input_discarded_endpoint` (no journal line) |
| 6 | Speech to text in `lamp-voice worker second`: Deepgram/relay websocket; the line comes back whole and the core publishes it on `voice/heard` | `second_provider/worker.rs:10032`; `second_provider.rs:3600`, `:4938`, `:4993`, `:5163`, `:5444`; `coordinator.rs:7901`, `:8152` | `request N: speech to text opened|kept ready in X ms`, `request N heard W words, the last X ms after the line ended`, `speech to text failed:` |
| 7 | Said to the lamp or not: verdict on `voice/turn`, room on `room/state`, the policy read from `policy/decisions` | `second_provider.rs:5785`, `:5976`; `second_provider/worker.rs:3486`, `:3504`, `:4146`, `:4161`; `second_provider/policy.rs:203` | `request N: its answer is held back:`, `was held as unclear (` |
| 8 | Chat (DeepSeek, relay) | `second_provider.rs:8086`, `:8505`, `:8532`, `:8596`, `:8774` | `chat round R first event in X ms`, `called`, `failed after`, `had not begun in` |
| 9 | Speech (ElevenLabs `pcm_24000`), sentence by sentence, 960-sample packets | `net/tts.rs:268`; `second_provider.rs:9347`, `:9363`, `:9417`, `:9490`, `:9687`, `:7214` | `first piece to the voice X ms after the words were heard`, `request N first answer audio X ms after the words were heard` |
| 10 | Core output: the lamp's text on `voice/said`; stage times; speaker chunks under an output permit | `coordinator.rs:7554`, `:7900`, `:7671`, `:14449` (from `:10409`), `:14531` | kinds `provider_first_audio`, `answer_stages` |
| 11 | Speaker worker writes ALSA, reports playback, sends the echo reference straight to capture | `audio_workers.rs:1375`, `:1460`, `:1813` | kinds `speaker_first_write`, `playback_gap`, `turn_finished`; journal only on faults |
| 12 | Sensed back: `voice/state` (1/s); `voice/timing`, `voice/activity`, `voice/playout` → `lamp-expression` → `expression/body` → `lamp-body`; `body/joint_states` read back by voice, expression, eyes | `coordinator.rs:12741`; `msgs/src/lib.rs:463`; `body/src/expression_in.rs:157`, `name_in.rs:23`; `voice/src/body_motion.rs:142` | — |

## Where the code and the docs differ

Filed as issues (links on #4305):

- `docs/architecture.md`: the provider is Gemini Live in the doc (`:61`,
  diagram `:131-160`, `:165`), the second route in the product; `lamp-name`
  and `lamp-turn-end` are missing from the map (`:192` lists end-of-turn
  inference as a candidate); the choreographer is "inside the core"
  (`:127-129`, `:218`) and "motor output is inactive" (`:16-17`,
  `:184-186`, `:75`), while `lamp-expression` and `lamp-body` are their own
  units driving the head in conversation.
- `docs/bus.md`: `name/heard`, `name/rising`, `stop/heard` (on the turn
  path) are not listed; the sound's `enc` is given as `pcm_s16le_16000_mono`
  (`:76-77`), the code writes `json+pcm` with a JSON header line; the
  publisher of `mic2/audio` is "the microphone's owner" (`:356`), it is the
  voice core and only with `LAMP_BUS_AUDIO=on`; "no part publishes them yet"
  (`:330`) and "no publisher calls `alive()` yet" (`:334`) are stale;
  `policy/decisions` and `body/joint_states` "read by nothing yet"
  (`:443-444`) are stale; `voice/heard` "in pieces" (`:405-407`) is one
  whole message per line on the second route.

Unsure, not filed: `architecture.md:235-236` says the core makes no cloud
call; the core can open speech to text itself (`stop_words.rs:1546`) with
`LAMP_STOP_WORDS=heard` or `LAMP_WAIT_KEEPS_ANSWER=words`, which the product
unit does not set.
