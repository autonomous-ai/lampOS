# Names

Written 2026-10-13.

A folder under `crates/`, the Cargo package, the program it builds and, where
there is one, the systemd unit all carry the same plain name. The name says
what the thing does for the lamp, in the words the owner uses for it.

## Renamed on 2026-10-13

| Was | Is now | What it is |
|---|---|---|
| `crates/live`, `lamp-live`, `lamp_live`, `lamp-live.service` | `crates/voice`, `lamp-voice`, `lamp_voice`, `lamp-voice.service` | The voice: it listens, talks with the model, speaks and drives the light ring. |
| `crates/endpoint`, `lamp-endpoint`, `lamp_endpoint`, program `lamp-endpoint-worker` | `crates/turn-end`, `lamp-turn-end`, `lamp_turn_end`, program `lamp-turn-end` | Judges, in its own process, whether a person has finished speaking. |
| `crates/endpoint-ipc`, `lamp-endpoint-ipc`, `lamp_endpoint_ipc` | `crates/turn-end-wire`, `lamp-turn-end-wire`, `lamp_turn_end_wire` | What the voice and the turn-end program say to each other, and the voice's side of it. |
| `crates/motor`, `lamp-motor`, `lamp_motor` | `crates/body`, `lamp-body`, `lamp_body` | The body: the servos, their limits and the motions. |
| programs `lamp-motor-inspect`, `lamp-motor-worker` | `lamp-body-inspect`, `lamp-body-worker` | The first reads the servos without moving them; the second is the worker the voice can start. `lamp-body` kept its name. |
| `crates/interaction`, `lamp-interaction`, `lamp_interaction` | `crates/turn-owner`, `lamp-turn-owner`, `lamp_turn_owner` | Who owns the current turn and may still speak, light or move for it. |
| `crates/acoustic`, `lamp-acoustic`, `lamp_acoustic` | `crates/sound-fixtures`, `lamp-sound-fixtures`, `lamp_sound_fixtures` | Fixed desk scenes and cached voices for repeatable sound tests. |
| `lamp-live-pilot@.service` | `lamp-voice-pilot@.service` | The attended pilot's unit template. |

## Added since

| Name | What it is |
|---|---|
| `crates/actions`, `lamp-actions`, `lamp-actions.service` | Carries out what the lamp is asked to do to itself ([actions.md](actions.md)). Other documents and drawings call this role the executive. |
| `crates/bus`, `lamp-bus` | How the lamp's programs tell each other what they sense and make of it, and the tool for looking at it ([bus.md](bus.md)). Not a service: a directory and a library. |
| `crates/msgs`, `lamp-msgs` | The message types the bus's topics carry, defined once ([msgs.md](msgs.md)). A library. |
| `crates/memory`, `lamp-memory`, `lamp-memory.service` | What the lamp remembers from one day to the next ([memory.md](memory.md)). |
| `crates/policy`, `lamp-policy` | Decides what the lamp does. Its first decision is whether what was heard was said to the lamp, made in shadow mode: said on the bus and written down, with nothing acting on it ([policy.md](policy.md)). Unit `lamp-policy.service` (the product's and lamp-4ace's); the product's acts on its hello decisions (`--act hello`), not on this one. |

## Deliberately not renamed

- `crates/observer`, `lamp-observer` and `tools/observer-bundle`: the built
  app holds a macOS permission that was granted to its identity.
- The word "endpoint" where it means a network address: `GOOGLE_ENDPOINT`,
  `validate_endpoint`, a relay endpoint.
- Rust identifiers, modules, command-line flags, environment variables, JSON
  field names, trace event kinds and record kinds that contain the old words,
  such as `endpoint_session`, `EndpointRig`, `"endpoint_policy"`,
  `--motor-worker` and `LAMP_ENDPOINT_WORKER`. Recorded data and outside tools
  read them.
- The notes between agents in `coordination/` and `HANDOFF.md`, and the dated
  evidence in `docs/verification/`. They say what was run and installed at the
  time, under the names of the time, and some of those files are pinned by
  their SHA-256.
- The plans `fixtures/voice-eval-v1.json` and `fixtures/voice-eval-v2.json`
  (a plan's hash goes into every record made with it) and the example
  `crates/voice-eval/tests/fixtures/recording-import-examples.json`, a byte
  copy of the one in `coordination/`.
- Names that belong to something else: the crate `lamp-endpoint-eval` on
  another branch, the stored bundle `lamp-endpoint-check-b1f43e50a.tar` and
  the worker inside it, and an old temporary directory in
  `docs/motor-worker.md`.
- Document file names such as `docs/live-runtime.md` and
  `docs/motor-worker.md`.
- Crates whose names already fit: gemini, audio, camera, ring, eyes, bus,
  memory, ipc, environment, voice-eval. The runtime directories
  `lampos-voice`, `lampos-body` and `lampos-eyes` fit too.

## On the lamp

lamp-4ace keeps the old unit and program names until the next deploy. The
unit files and the runbook under `deploy/` already use the new ones, and so
must the deploy scripts kept beside the working copies.
