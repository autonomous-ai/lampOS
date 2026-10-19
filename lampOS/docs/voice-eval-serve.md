# Serve-session conversation runner

Status 2026-10-11 (Agent 2): implemented and tested offline only. The runner
has driven a fake `lamp-voice serve` through the real `lamp-session` relay on one
Mac and a Linux VM. It has **not** run against a Lamp, a real provider or the
iMac speakers. Nothing below is an acoustic, latency or quality result.

One attempt is one supervised `lamp-voice serve` runtime and one provider
session. It carries a whole pre-registered conversation of up to 20
context-linked turns. Every stimulus is cached speech: the runner never renders
audio and never replays a prompt. Every attempt, including a failed or
withheld one, stays in the ledger.

## Running it

Physical (operator-authorized; same readiness checks as `physical-run`): put
`"serve_config": "/private/provider.json"` in the physical config in place of
`provider_config` or the fixture pair. Serve requires loudspeaker replay
through the prepared player and refuses raw audio diagnostics.

```sh
cargo run --release -p lamp-voice-eval -- physical-run --config FILE --plan fixtures/voice-eval-serve-v1.json \
  --cache .cache/acoustic --manifest M... --out NEW_DIR --scenario serve-conversation [--execute]
```

Runtime policy is forwarded typed and validated. The fields are
`turn_taking`, `reference_clock`, `reference_recovery`, `playout_start_ms` and
`provider_continuation`, in the config or as the same `lamp-session` flags.

- Each value is parsed with lamp-voice's own types.
- The whole set is checked with lamp-voice's own `DirectedOptions` rules, on the
  Mac and again in the relay before any child starts. For example, bounded
  recovery without a masking policy is refused.
- An omitted field passes nothing, so the default child argv is unchanged.
  Fixture runs refuse these fields.

**The voice route.** A `serve` or `directed` run starts the voice on the
second route (Deepgram, DeepSeek, ElevenLabs) unless it names Gemini Live:
the owner, 19 Oct 19:3x, every voice test runs on the second route, and Live
only for a comparison the first builder names on #1712.

- `voice_route` is `"second"` (the default) or `"live"` (`--voice-route` to
  `lamp-session`). The relay sets `LAMP_VOICE_ROUTE` (`second`, or `gemini`
  for live) in the runtime's environment, not its argv.
- `runtime_env` holds the `LAMP_*` settings the voice reads, copied from the
  lamp's voice unit (`--runtime-env NAME=VALUE`, each name once). The second
  route needs `LAMP_VOICE_SECOND_CONFIG`, and takes its switches
  (`LAMP_SECOND_CHAT`, `LAMP_SECOND_SEARCH`, `LAMP_SECOND_SPEECH`, ...) from
  here. `LAMP_VOICE_ROUTE` itself is set only by `voice_route`.
- A run on the second route without `LAMP_VOICE_SECOND_CONFIG` is refused on
  the Mac before anything starts on the lamp. lamp-voice would refuse it at
  start too. A fixture run refuses both fields.
- **Keys:** the second route reads DeepSeek's key (`deepseek.key`) from the
  voice unit's systemd credentials (`CREDENTIALS_DIRECTORY`). A `lamp-session`
  started over plain SSH doesn't have them, so the `lamp_command` must start it
  where they are. No physical run on the second route has been made yet; the
  first one settles how (#1712).

`fixtures/voice-eval-serve-pilot-config.example.json` holds the installed
pilot's settings, the kept-start candidate, the second route's settings and
an external room recorder:

- noise suppression off;
- `duplex`; the installed template uses `v1-baseline`;
- capture reference clock;
- reference recovery off;
- 200 ms playout start;
- ring ceiling 12;
- kept-start continuation.

It contains no secrets: the SSH target is a placeholder, and the provider path
is the template's path, not its content. The serve child then runs:

```text
lamp-voice serve PROVIDER OUT --noise-suppression off --turn-taking duplex --reference-clock capture
  --reference-recovery off --playout-start-ms 200 --provider-continuation kept-start
  --cue-socket WORK/cue.sock --ring-channel-ceiling 12
```

Offline, with no device and no audio, using the fake serve runtime, the real
relay and a fake prepared player whose "first copy" is a room datagram:

```sh
cargo run -p lamp-voice-eval -- serve-offline --out NEW_DIR --cache .cache/acoustic --manifest M... \
  [--plan FILE] [--fake-arg --fake-reopen-ms --fake-arg 700] [--player-fault 2:late:150]
```

`fixtures/voice-eval-serve-v1.json` is the reusable cached-scene example. Its
eight turns are:

1. A question at listening readiness.
2. A follow-up.
3. A second follow-up.
4. An interruption during the third answer.
5. A follow-up.
6. A correction on that follow-up's local endpoint.
7. A change of topic.
8. A last question.

Each follow-up starts on `next_turn_ready`. Existing plans, the catalog and
their hashes are unchanged; the default plan still hashes to `e9dad734…`.

## Readiness and the start grant

- **Before the trigger wait.** The runner arms each stimulus first. It calls the
  observer's `play_prepared` in-process: no process is created at the grant. The
  call loads and verifies the WAV and opens the output in silence. The ready
  receipt's `source_sha256` must equal the cached asset's identity.
  - Process creation, WAV preparation and report persistence are therefore
    never between readiness and the start.
  - A player that fails to prepare, or reports another source, withholds that
    step as `player_failed`.
- **Triggers.** They are events only:
  - `listening_ready`, `speaker_first_write` (for overlap).
  - `next_turn_ready`: availability reopened and the link ready after every new
    turn finished, with no other input since.
  - `local_endpoint`.
- **Final dispatch check.** It drains everything already relayed and rechecks
  every precondition.
- **The grant.** The runner sends exactly one grant, whose expiry is the
  original lateness bound on the player host's CLOCK_MONOTONIC:
  `mapped(Lamp event) − clock uncertainty + configured delay + 100 ms`.
  - The Lamp event time is translated through the ping/pong clock map. The
    runner takes the earliest Mac time the map allows; it never assumes the
    Lamp's monotonic epoch is the Mac's.
  - No fresh budget is added after transport, receipt or any other delay. A
    receipt-time trigger (no Lamp event time) or a missing clock map withholds
    the step.
- **At the first source copy.** The player re-reads the clock and refuses at or
  after expiry (`start_expired`). A refused or failed delivery is polled while
  the runner waits for the next trigger. It becomes a runner `evidence_lost`
  event, and every later turn is withheld.
- **What the start evidence is.** The step's start is replaced by the player's
  first-copy time. That is still not an acoustic onset; room audio stays a
  separate boundary.

In serve mode, listening readiness comes only from the accepted schema-1 cue,
which carries the runtime's own event time. A stdout readiness line has only
the relay's receipt time and may be buffered, so it is kept as a diagnostic
(`stdout_readiness_relay_us`) and never triggers a step. The grant receipt
keeps a signed `margin`: `headroom_at_grant_us` and `expired_at_send`. A send
delayed past expiry shows as negative headroom and is refused by the player at
its first copy; it never wraps around into a positive margin.

`local_endpoint` binds to the one turn admitted since the previous step.

- **Not dispatched:** a second admission, or the turn finishing or speaking
  before its endpoint.
- **Withheld at dispatch:** the turn has output or a finish, or any new
  candidate or admission arrived after the endpoint.
- **Declaration only:** the plan marks such a scene as a fresh request, but the
  runner does not reset turn ownership or context. Whether the runtime
  superseded the turn before output is judged from the trace; the dispatch
  qualifies nothing.

## Stop, reaping and failure provenance

Every serve session ends through one relay path. The runner's `{"type":"stop"}`
line, a closed runner transport and the hard deadline (`seconds +
allowance`) all start it.

1. SIGTERM goes only to the runtime the relay started, the leader of the
   process group it created. This matches the pilot unit's coordinator-first
   `KillMode=mixed`; there is no group-wide SIGTERM.
2. After a 10 s grace (`SERVE_STOP_GRACE_US`; longer than the pilot unit's
   5 s `TimeoutStopSec`), the relay kills that group.
3. The relay then confirms the group is gone.

`session_end.serve` keeps these separately:

- `stop_cause`: `runner_stop`, `transport_closed`, `deadline`, or
  `runtime_exited` when the runtime ended by itself;
- the runner's stop reason;
- request, signal and exit times;
- `forced_kill` and `group_reaped`;
- the metadata summary.

A runtime that sends its own `run_end`, or reports a terminal provider link,
is waited for (10 s) rather than stopped, so its own end keeps its provenance.

On the Mac, a serve attempt is certified only after the outer transport has
been reaped with a successful exit. The transport is the `lamp-session` relay,
normally over SSH. The runner waits for it for at most 5 s after closing its
input. A nominal runtime end (exit 0, acknowledged metadata) does not
certify the attempt if the transport then exits nonzero, by signal, or has
to be killed at that bound.

- The runtime's outcome stays in `session_end`.
- The transport's outcome is recorded separately: `transport_exit`,
  `transport_status` and `transport_forced_kill`.

Room evidence is certified separately, and never by a process exit. The pilot
launches the observer with `open -n -W`, whose exit status is `open`'s, not the
observer's. The attempt keeps the observer's own terminal report
(`room_audio.observer_terminal`). When it is not a verified `captured_unscored`
recording, the attempt carries the following:
- `room_recording: invalid` with the status and fault detail;
- a withheld `evidence_missing` finding;
- the line `Independent room recording: **invalid** (...)` on its page;
- `room_recording` state counted by the report and by the CLI summary line.

Lifecycle success and room-evidence validity stay distinct: neither implies the
other.

**Room coverage.** A prompt is played only while the room recording covers it,
in every physical mode:
- **Recorder exit.** If the recorder has exited (any status, including `open
  -W`'s 0), that becomes a runner `evidence_lost` event. This and every later
  prompt are withheld, rechecked at the final dispatch itself.
- **Declared end.** When the recorder's ready receipt declares its capture end
  (`planned_capture_end_host_monotonic_ns`, written by the observer), a prompt
  is refused unless its speech plus the scenario's observation window
  (`observe_ms`, 8 s for the serve plan) ends before that time. A 120 s capture
  inside a 240 s scenario therefore withholds the prompts it cannot cover
  instead of playing them unrecorded.

- **Grant cap.** A granted first copy may start later than its request,
  until the scenario's original expiry. The grant's expiry is therefore the
  earlier of that original expiry and the latest start the declared end still
  covers. No slack is added. Each grant receipt records both bounds under
  `coverage`.
- **Actual start.** At the end, every delivered prompt's actual start (the
  first source copy, or a direct player's start request) is checked against
  its latest covered start. An overrun is kept in `coverage_violations`.

The room evidence keeps the ready receipt, the declared end, `coverage_lost`
(`prompt_not_covered`, `declared_end_reached` or `recorder_exited_early`) and
`coverage_violations`. Two cases can leave granted speech unrecorded:
`recorder_exited_early`, and any coverage violation. The WAV is kept, but it
is scored as `room_recording: invalid` (`ended_during_session` or
`prompt_outside_recording`). Its annotations are refused for timing and for
reviewed content alike, and it has no listening template. A recording without a declared end is covered only by exit
detection, polled every 20 ms in serve.

Lamp-4ace's first eight-step run shows this. The runtime and transport exited
0. The room recording was `invalid_capture`: `queue_full` dropped 13,824 of
15,840,000 requested 48 kHz frames, with one writer sequence gap. The whole WAV
stays invalid and is never offered for listening.

### Startup clock map and preparation

A cue is accepted only if its 100 ms expiry is still ahead at the runner,
including the clock map's uncertainty. That bound and the stale-cue refusal
are unchanged. Two startup rules keep the decision on live evidence:

- **Only live pings define the map.** The runner writes ping 1 when it starts
  the transport, so it waits while SSH connects. Its round trip is connection
  setup, not the live path. Only pings written after the relay's
  `session_start` is taken may define the map, and one is sent at once then.
- **Cues are taken while a player prepares.** A prepared player's ready
  receipt is awaited within its unchanged 10 s bound in 2 ms polls. Between
  polls the runner takes every relayed line and keeps pinging, so a cue is
  judged when it arrives. Its events are kept in order for the runner.
- **No cue is published before a live map.** The relay's receipt is not
  proof at the runner. A cue taken before the first live map waits, as does
  any cue behind one already waiting. It keeps its original event, send,
  relay and runner receipt times and its order (at most 64 wait).
  - **Release.** The first live sample decides waiting cues in order. Each is
    published only if the map's upper bound at that decision is before the
    original expiry; equality refuses.
  - **Hold limit.** A cue still waiting 100 ms of runner time after its receipt
    is refused. The relay showed it arrived no earlier than its event, so it
    has certainly expired. This is a proof, not a new deadline.
  - **Retention.** Cues still waiting at the end are kept as refused receipts.
    Nothing is restamped or extended.

Each cue receipt keeps the mapping its decision used under `freshness`:
offset, uncertainty, decision time, time held, upper Lamp-time bound, expiry
and margin. Every ping is kept in `clock_samples` with its round trip and
whether it was usable.

Lamp-4ace attempt wdqcviuk showed the failure. Its first player took ~293 ms to
prepare. The first `input_availability` cue was refused as stale including
clock uncertainty, and the runner stopped the runtime before readiness. Under
the final mapping that cue was only ~21.5 ms old, so the refusal is proven but
real staleness is not; the mapping used at the decision was not retained.
`tests/serve_session.rs` reproduces this through the real relay with a 400 ms
transport start and a 2.3 s preparation. The old order fails with that exact
refusal; the current order plays both prompts.

The following are relay errors, so the attempt is aborted, never scored as a
clean conversation:

- a nonzero or signalled runtime exit, even after complete, acknowledged
  metadata (the runner checks the exit status as well);
- a forced kill;
- processes left in the group;
- a closed transport;
- the deadline;
- a group still present 2 s after the end.

After exit, the relay streams `metadata/events-{0..3}.jsonl` as trace lines,
oldest generation first:

- Each segment must begin with a valid `service_segment` header.
- Generations must be contiguous.
- `service_sequence` must advance by one.
- `health.json` must report a finished writer.
- The runtime's `service_metadata_finished` line must acknowledge every event,
  with nothing dropped, oversized, unconfirmed or degraded.

Overwritten history (generation 0 gone), gaps or a missing acknowledgment are
missing evidence: they are reported and the attempt is not scored.

## Service cycles

A physical mute ends the runtime cycle. A later cycle has a new cue boot.

- **Finding retained.** A new boot with a contiguous cue sequence is still a
  CueOrder rejection ("context continuity lost"). The conversation's remaining
  turns are withheld at once and the attempt is aborted.
- **Restart verification.** The attempt's `serve_continuity` evidence records:
  - every observed boot change and every service cycle record;
  - whether the runtime's own metadata verifies a restart: cycle 1 muted, and
    cycle 2 started no later than the first new-boot cue.
- **Scoring.** Only the first cycle's records are scored; the rest are counted.

## Cue contract this relies on

In addition to the proposed `input_availability`, `provider_link` and
`input_candidate` kinds (see [directed cues](voice-eval-directed-cues.md)):

- **Startup history.** Before a boot's listening readiness, a state record
  already older than the 100 ms cue bound is startup history. It is not sent and
  latches no fault; it is counted (`historical_state_skipped`).
- **Current link state.** At readiness the coordinator publishes the link state
  it just validated, `provider_link_current`. It is a fresh `provider_link`
  cue, never a re-stamped old record.

Both are separate lamp-voice proposal commits.

## Offline controls

These ran through the real relay process, lamp-voice's own `CueSink` and the
fake serve runtime and player, one attempt at a time. The 100 ms bound was
unchanged; no lease was widened.

| Control | Result |
| --- | --- |
| Normal 8 turns | 8/8 injected; each grant sent with 92–99 ms of the original bound left; first copy before expiry; correction superseded the follow-up before output (fake); SIGTERM to exit 12.5 ms; 105 metadata records, no gaps; group reaped |
| Delayed readiness (700 ms reopen) | each follow-up dispatched on the reopened availability, ≥ 690 ms after the finish |
| Extra candidate during the wait | `precondition_lost`; rest withheld; nothing granted |
| Stale, duplicate, reordered, foreign-boot, dropped cue | rejected cue retained; conversation ended (`session_ended`); attempt aborted |
| Provider outage 800 ms | dispatched on the recovery's link-ready event |
| Provider terminal | `provider_unavailable`; runtime exited by itself (`runtime_exited`, exit 1) |
| Player first copy 150 ms late | `start_expired`, zero copied; later turns withheld; no second admission |
| Player prepare failure | `player_failed` → `delivery_failed`; rest withheld |
| Runtime stopped externally | `session_ended`; `runtime_exited`; clean metadata |
| Physical mute and new cycle | withheld at the new boot; continuity lost; restart verified from metadata |
| SIGTERM ignored | forced kill after the 10 s grace; attempt aborted, never clean |
| Room recorder ends early | `evidence_lost` (room recorder); rest withheld |
| Recorder exits 0 with `invalid_capture` | `room_recording: invalid`, terminal report quoted; acoustics unscored; no listening template (`tests/report_validity.rs`) |
| Relay: stop / transport closed / deadline / SIGTERM ignored | each cause, signal, exit, forced kill and reap recorded separately |

## Limits

- The fake runtime is a policy simulation of serve's shape, not lamp-voice. The
  real serve path additionally needs the physical privacy worker, Gemini and
  hardware.
- The prepared observer has not run in this runner against the iMac speakers.
  Main's separate real-iMac check covered the observer itself.
- Every cached fresh-request scene starts its speech 1 s into the clip. With
  about 1.1 s from endpoint to first ALSA write, a correction dispatched on the
  endpoint can be heard after the reply started; the trace decides which
  happened.
- A mute or stop is visible live only as cancellation, lost availability or
  the next boot. Service-cycle records reach the runner only in the final
  metadata.
- The room recorder runs for its configured length; the runner waits for it
  at the end. Prompts it cannot cover are withheld (see room coverage above);
  a recorder that declares no end is guarded only by its exit.

Main integration also samples the decision clock after parsing each cue, including
each member of a held startup batch. A pause between decisions cannot let a later
cue reuse an earlier freshness result. Original event, relay and runner receipt
times remain unchanged; a deterministic two-cue regression crosses the second
expiry and refuses readiness.

## Microphone comparison option

The physical configuration can set `"microphone": "mic1"` to forward the typed
`--microphone mic1` option to lamp-voice. Omission leaves the original command
unchanged and uses its mic2 default. No mixer is changed by the runner. Use an
independent room recorder, retain the selected role in the report, and keep this
cohort separate from mic2 and direct-human results. See [microphone topology](microphone-topology.md).

## Scoring the native conversation (Lamp-4ace)

The first native run's original evaluation is retained as failed. Its
cancellation findings disagreed with the raw order. Turn 3's `user_interrupted`
finish came 153 µs before turn 4's admission (same boot, generation 7 → 9),
while the coffee candidate preceded the revoke by the 470 ms duplex yield. The
old matcher looked for the candidate within 20 ms. As a result it reported:
- an unexplained cancellation;
- a truncated sunset answer and a missed coffee interruption;
- a false interruption of the pending turn 5 by its own `local_endpoint`
  correction.

The current rules are in [the main runbook](voice-eval.md). Re-scoring the
retained run with `evaluate RUN_DIR --plan fixtures/voice-eval-serve-v1.json`
writes a new evaluation beside the original. With the current rules:
- the sunset reply is yielded as planned and the coffee interruption is scored;
- turn 5 is a planned supersession before output;
- turn 1's 30 ms supply gap remains a playback-gap defect;
- the endpoint boundary states the runtime's 1,200 ms silence wait.

`tests/report_validity.rs` holds the exact-order reconstruction, with negative
controls: no admission, an admission 25 ms late, another boot, a stale
generation, an earlier turn, a correction bound to another turn and an ordinary
request.
