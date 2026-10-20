# Directed conversation scheduling cues

`lamp-session` creates a fresh private work directory and binds its same-user
Unix datagram socket before starting either `lamp-voice directed` or
`lamp-voice directed-fixture`. Both commands receive `--cue-socket ABS_PATH`.
Directed cues do not enable PCM diagnostics; that remains a separate explicit
option. A runtime without this CLI capability fails instead of silently using
fixed delays. Provider-kind, fault-injection, verified cached-source and
reviewed-room-evidence restrictions remain in force.

The schema-1 cue channel is scheduling evidence only. Each datagram is at most
512 bytes and contains its session boot, monotonic sequence, event and send
timestamps, an expiry exactly 100 ms after the event, optional capture/reference
epochs, and an immutable turn/generation for owned events. Supported kinds are
`listening_ready`, `input_admitted`, `local_endpoint`, `speaker_first_write`,
`speech_retired`, `turn_completed`, `cancelled`, and `run_end`. A cancellation preserves the
optional original `turn_finished.outcome` in `reason`; an omitted reason stays
unknown. The cue does not supply acoustic input boundaries, input transcript,
or proof of a complete or correct audible answer.

Compatibility is envelope-level, not whole-stream compatibility: an older
schema-1 cancellation without `reason` remains parseable. The new ordering
checks require `input_admitted`, `local_endpoint`, playback occurrence IDs and
`turn_completed` from the updated producer,
so deploy this runner and runtime together. An older stream missing those
owner/phase cues is withheld or invalidated, never silently treated as a
supported directed conversation.

The runner validates the envelope, consecutive sequence and nondecreasing send
times, stable boot and per-turn generation, admission before endpoint, endpoint
before playback, and a matching playback occurrence before its retirement.
`speaker_first_write` and `speech_retired` carry a positive immutable
`playback_sequence`. Several strictly increasing occurrences may belong to the
same original turn/generation; only one may be active. Retirement must match
the active sequence and cannot close a later occurrence. `turn_completed`
requires the original generation, exact `outcome` and `playback_gaps` count.
Audio completion requires all known occurrences to have retired. A completion
with missing fields is invalid; absent fields never become success or zero gaps. Child-observation event times
may arrive after newer coordinator timestamps; they are preserved, not required
to be globally sorted. Cancellation may precede playback, or follow complete
audio retirement while its provider turn remains active. It rejects a revived
cancelled or completed owner, unknown owner, duplicate phase or changed lineage.
Retirement ends one occurrence; another positive sequence may start for the
same active turn. Only whole-turn completion or cancellation is terminal. The
Lamp relay receipt must be at or after send and strictly before expiry. For
owned cues, a ping/pong clock mapping is also required: the mapped upper bound
of the runner receipt, including clock uncertainty, must precede expiry.
Readiness can additionally arrive on stdout after the relay has acknowledged
its requested cue capability. That readiness line never substitutes for a
missing retirement or playback-start cue.

The raw accepted or rejected cue envelopes, Lamp relay receipt times and runner
receipt times are retained in `evidence.cue_receipts`; `session_start` records
mode and actual runtime arguments. Sequence/identity/freshness or malformed
transport failures invalidate the attempt. A cue failure immediately ends
stimulus scheduling with an explicitly synthetic `cue_protocol_invalid`
run-end event in the runner clock domain, while cleanup still collects the
actual runtime trace. All attempted or withheld steps remain in the ledger.
The final trace can explain a missed trigger but cannot retroactively authorize
a stimulus.

The scenario's delay is measured from its matching software cue. For example,
`rapid-follow-up` in the new v2 plan targets 150 ms after the original turn's
whole-turn completion;
`topic-change` targets 1,200 ms after its first accepted speaker write and still
requires that reply to be playing. These are software scheduling offsets,
not measured silence gaps or acoustic interruption latency. Missing or stale
cues withhold the follow-up; there is no fixed-delay fallback. Existing runner
lateness checks remain active; overlap uses the currently active occurrence,
including a second or later generation. Zero-tolerance overlap intervals are
half-open: a stimulus at the exact retirement/cancellation timestamp is outside
playback, and a zero-length interval never proves overlap. Explicit ring timing
tolerance remains separate. Silence between occurrences does not
satisfy the speaking precondition. A correct relay/fake test
cannot qualify microphone pickup, speech quality, source identity, semantic
correctness, audibility, or end-to-end latency. Those still require valid room
recordings and independent review.

## Proposed readiness cues

The runner's `next_turn_ready` wait needs three more schema-1 kinds from
`fixture_provider::CueSink::observe_event`. They are proposed in a separate
lamp-voice commit with no coordinator change (branch `lamp-a2-readiness-cues`)
and are not yet integrated. The runner accepts them strictly. All three are unowned: no `turn`
and no `generation`, and the existing envelope, sequence, boot and freshness
checks apply.

| Kind | From trace record | Extra field |
| --- | --- | --- |
| `input_availability` | `turn_taking_availability` | `available`: its `prospective_input_available` (required) |
| `provider_link` | `provider_ready`, `provider_unavailable`, `provider_terminal` | `link`: `ready`, `unavailable` or `terminal` (required) |
| `input_candidate` | `input_candidate` | none; the candidate identity stays in the final trace |

`available` is valid only on `input_availability`, and `link` only on
`provider_link`.

Each producer record already has `at_us`. A provider link record's `at_us`
is the provider worker's own observation, which can be older than the cue
freshness bound. Its cue therefore uses the record's
`coordinator_observed_at_us`, when the coordinator learned it. The runner
orders by cue sequence, not by timestamp. A `turn_taking_availability` record
without `prospective_input_available` latches the usual cue fault; it is never
guessed. lamp-voice already records availability and the link before
`listening_ready`, so a runtime that sends these cues is recognized from the
start of every attempt. Deploy the producer and the runner together, as for
the other schema-1 kinds.

### Startup history and current link state (proposed fix)

Root reproduced a defect in the first proposal. A valid provider Ready is
retained while later workers start (`StartupEvents`) and recorded only at
conversation entry. If that startup takes longer than 100 ms, the Ready is too
old to send. The sink then latched `cue_invalid_or_unavailable` and published
nothing more, not even `listening_ready`. Two separate lamp-voice proposal
commits correct this without weakening packet freshness or identity checks.

1. **Startup history.** Before a boot's own listening readiness, a
   `provider_*` or `turn_taking_availability` record already older than the cue
   bound is startup history:
   - it is not sent and latches no fault;
   - it is counted in `historical_state_skipped`;
   - it is never re-stamped.

   Future-stamped records, foreign or stale identities, and every record after
   readiness keep the strict bound. A new boot starts a new startup.
   `coordinator/tests/startup_cues.rs` runs this through the production
   `StartupEvents` retention, conversation-entry recording and the real sink.
   On the unfixed sink the 120 ms case failed with exactly the reported latch;
   the 20 ms control, the post-readiness stale record and the future-stamped
   record behaved the same before and after.
2. **Current link state.** When `conversation()` records `listening_ready`, it
   has just validated `cloud.available()`. It now first records
   `provider_link_current` with the same `at_us`, `state: "ready"` and the
   ready session. The sink publishes it as a fresh `provider_link` cue under
   the strict bound. This is a newly validated current-state publication, not
   cached history, and it is the only coordinator change. Without it, a slow
   startup would leave the runner without link evidence, and `next_turn_ready`
   would be `trigger_unavailable`.

## Offline regression path

`tests/it/physical_loopback.rs` starts the real `lamp-session` subprocess and a
`fake-lamp-voice` subprocess. The fake supports both runtime command forms,
uses the actual live `CueSink`, and receives declared stimulus intervals through
a private fake-room socket. Cached synthetic tones validate the cache and
orchestration paths only: no CPAL/ALSA device, provider connection, SSH command,
TTS, microphone or speaker is opened by these tests.

Fake-only fault options interpose one bounded local datagram shim:
`--fake-cue-fault drop-retirement` omits the first retirement cue, while
`stale-retirement` forwards that original datagram 150 ms late without changing
its timestamps, sequence or 100 ms expiry. The shim retains at most one delayed
512-byte datagram and drains at most 64 per tick. Nominal tests do not use it.
`drop-completion` and `stale-completion` apply the same rules to whole-turn
completion. Each retains its final trace event while proving that a second
stimulus was never started. `--fake-generations two` splits the first scripted
fake reply into a 500 ms segment and a second segment after a 200 ms simulated
provider delay; it changes only fake timing, creates no audio and is recorded
in the invoked arguments. The loopback tests use the real producer and relay
to verify two occurrences, follow-up after whole completion, and interruption
during the second occurrence. Additional pure tests reject schema aliasing,
missing or zero identities, changed generations, duplicates, missing sequence
numbers, invalid phase order, and receipt at the exact expiry boundary.

Run the finite process/socket tests serially to avoid unnecessary host
scheduling contention:

```sh
cargo test --locked --offline -p lamp-voice-eval --lib
cargo test --locked --offline -p lamp-voice-eval --test it physical_loopback:: -- --test-threads=1
```

Local scheduling assertions are test sanity bounds, not hardware latency gates.
No Rust runtime behavior or threshold is changed by this runner implementation.

## Historical scoring

The original `fixtures/voice-eval-v1.json` bytes and its `speech_retired` triggers
remain unchanged and parseable. `voice-eval-v2.json` is a new default plan with
`turn_completed` follow-ups; its distinct identity/hash prevents silently
relabeling historical runs. A v1 trigger still means the first matching occurrence
retirement, so it is not qualified as a whole-answer follow-up.

Offline import can read an unambiguous legacy single occurrence without a
sequence when an actual terminal completion and matching retirement exist.
Recognized start, retirement and terminal records require a positive turn and
an original timestamp. Missing time invalidates the named turn; missing identity
invalidates the attempt without assigning it to another owner. Such records are
retained, and invalid attempts are excluded from every success/latency aggregate.
Multiple missing/mixed IDs, stale or duplicate retirement, or incomplete playback
at claimed completion invalidate the evidence while retaining the attempt.
An old retirement followed only by cancellation never proves full completion.
First-word latency uses the first accepted write; software exposure sums all
occurrence intervals and excludes inter-generation silence. Cancellation clips
that exposure at the cancellation timestamp: cancelled accepted tails are separate
receipts and this number is neither optical nor acoustic duration. Ring and
overlap checks inspect every occurrence. Semantic correctness and audible
completion still require reviewed, validated room evidence.
