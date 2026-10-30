# Voice routes: one contract, several experiences

Draft design, 24 Oct 2026. Read against `lamp-voice` at `1d915d44c`
(24 Oct 07:41 +07). Nothing was built or run for it: it is a code survey
followed by a proposal. Paths are relative to `lampOS/`. A reference like
`coordinator/second.rs:231` is under `crates/voice/src/` unless another crate
is named.

The owner's ask: let a lamp run one of several whole voice experiences, and
let the person choose, for example in the Autonomous app:

| Route | What it is | State |
|---|---|---|
| `cascade` | Speech to text (Deepgram), a text model (DeepSeek), speech (ElevenLabs). Today's "second route" and the product's voice since 19 Oct | Built |
| `gemini-live` | Google Gemini Live, speech in and speech out | Built; per-lamp drop-in |
| `openai-live` | OpenAI GPT-Live-1, full duplex, with "client delegation" so our own model and tools can sit behind it | Not built |
| `dessa` | Block's Dessa, a full-duplex speech-to-speech research preview | No API: placeholder only |

---

## Part 1. What the code does today

### 1.1 How the route is chosen

`LAMP_VOICE_ROUTE` in the voice service's environment, read when the service
starts (`coordinator/second.rs:155`, `routes()` at `:231`):

| Value | Result (`Routes`, `:159`) | Worker in `rig.provider` | Standby |
|---|---|---|---|
| unset, empty or `gemini` | `Routes::Live` | `provider` (Gemini Live) | `second`, if `LAMP_VOICE_SECOND_CONFIG` is set |
| `second` | `Routes::SecondOnly(config)` | `second` (the cascade), with `--only-voice` | none |
| `second` without `LAMP_VOICE_SECOND_CONFIG` | refused | the voice does not start, and never falls back to Live | |
| anything else | refused | "LAMP_VOICE_ROUTE is gemini or second, not …" | |

- `run_serve` refuses a bad route, or a bad `LAMP_SECOND_*` choice, before
  anything starts (`coordinator.rs:2445-2475`). It also places the product's
  `second.json` if the unit names it and it is missing
  (`second_config.rs:307`).
- `spawn_rig` reads the route again and starts the worker for it
  (`coordinator.rs:3820-3850`, `Routes::workers` at `second.rs:194`). The
  cascade's worker gets the Live worker's arguments, its own configuration
  and its route flags (`second.rs:314`), minus `--keep-link` and
  `--wait-for-room`, which are only for Gemini's link.
- The product unit sets `LAMP_VOICE_ROUTE=second`,
  `LAMP_VOICE_SECOND_CONFIG=/etc/lampOS/voice/second.json`, `LAMP_SECOND_CHAT=deepseek`,
  `LAMP_SECOND_SEARCH=deepseek`, `LAMP_SECOND_SPEECH_MODEL=eleven_v4_turbo`
  and a dozen more `LAMP_SECOND_*` switches
  (`deploy/product/lamp-voice.service:70-114`). Live is "a per-lamp drop-in"
  with `LAMP_VOICE_ROUTE=gemini`.

**What changes with the route:**

| | `gemini` (Live) | `second` (cascade) |
|---|---|---|
| Worker process | `lamp-voice worker provider DIR …` (`main.rs:691`) | `lamp-voice worker second DIR … PROVIDER_CONFIG SECOND_CONFIG [route flags]` (`main.rs:706`) |
| Config files | `/etc/lampOS/voice/provider.json` (endpoint, model, voice, credential; `deploy/product/settings/voice-provider.json`) | `provider.json` too (the character, `config.rs:401-415`), plus `second.json` (`deploy/product/settings/second.json`; fields in `second_config.rs:14-80`) |
| Credentials | `provider.json`'s `credential_file`, the relay key `/etc/lampOS/voice/credential`; the endpoint is the relay's Gemini websocket | `second.json`'s `llm_credential_file`, the same relay key, used for every service (STT and TTS fall back to the LLM's base and key, `second_config.rs:430-435`). DeepSeek is reached through the relay by deriving `…/api/v1/ai/v1/deepseek` from `llm_base_url` (`relay_deepseek`, `:109`). Unit credentials (`CREDENTIALS_DIRECTORY`) reach only this worker (`process.rs:1440`): `gemini.key`, `openrouter.key`, `deepseek.key` for non-relay choices, and the person's own keys with `LAMP_OWN_KEYS=on` (`second_config.rs:537-582`, `docs/own-keys.md`) |
| Environment | the shared opt-ins | the shared opt-ins plus 5 cascade-only opt-ins, the credentials folder and a list of 54 names (`second_route_worker_environment`, `process.rs:1453-1521`) |
| Journal at start | — | `SECOND_ONLY_LINE` (`second.rs:305`) |
| Session record | `run_start` as before | `run_start` has `"voice_route":"second"` (`Routes::mark`, `second.rs:297`); voice-eval keeps it (`crates/voice-eval/src/events.rs:36-40`) |
| Worker exit | ends the voice | ends the voice when it is the only voice; a standby's exit is dropped (`mandatory`, `coordinator.rs:1722`) |
| Selftest | `voice_route` says `not_fitted` | `voice_route` checks `second.json`, every key file it names and every `LoadCredential=` source (`crates/selftest/src/voice_route.rs:203-260`) |

**Live with a standby** is a third behaviour hidden inside `gemini`: the
cascade stands by and takes over while Gemini's link is not Ready, at startup
(a held line waits up to `STARTUP_HOLD_US`, about 1.9 s) and mid-turn (a line
is replayed from `Line`, at most 30 s of audio, memory only)
(`coordinator/second.rs:1-29, :37-70`). That logic is written against
Gemini's own limits (`lamp_gemini::MAX_INPUT_AGE`, its input queue).

### 1.2 The contract a route meets today

There is no written contract. In practice it is the provider wire, which
three implementations speak: the Live worker (`provider_worker.rs`), the
cascade (`second_provider.rs`, "Engine speaks the provider protocol", `:5`)
and the test fixture (`fixture_provider.rs`).

**Process and channels.** The coordinator starts the worker with a cleared
environment holding only what its role is given (`worker_command`,
`process.rs:908`) and two local datagram channels (`transport.rs`):

- *Control, coordinator to worker* (`wire.rs:24`): `Authority { snapshot }`
  (the turn owner's permits: microphone, speaker, camera, privacy
  generation), `ProviderOutputCapacity { through }` (output credit), `Stop`.
- *Control, worker to coordinator* (`wire.rs:143`): `Ready`,
  `ProviderLink { link }` (Ready, Unavailable, … with a session number;
  `provider_link.rs`), `VisualContext`, `Fault { code }`.
- *Data in*: `ProviderInput` (`provider_worker.rs:674`). *Data out*:
  `ProviderOutput` (`provider_worker.rs:866`).

**Inputs** (every one carries `request` and `privacy_generation`):

| Input | Meaning | Who uses it |
|---|---|---|
| `Start` | a line admitted; `session` chosen at admission; flags `continues`, `continues_cut_off`, `over_the_lamp`, `after_stop`, `follows_on`, `over_the_lamps_voice`, `heard_of_cut` (notes the model is told) | both |
| `Audio` | 16 kHz mono i16 blocks of 160 samples (`crates/audio/src/lib.rs:33-35`), with sequence and read time | both |
| `End` | the line is over; `sound` (sent only for the model to judge), `cut_in` | both |
| `Called` | the line was only the lamp's name and the wait ran out | both |
| `Paused` | ask speech to text for the words so far (`LAMP_SECOND_EARLY_FINALIZE`) | cascade only; "The Live worker takes nothing from it" |
| `CountOnly` | onboarding's sentence: count the words, keep nothing | both |
| `Written` | a turn the lamp begins itself (hello, say service, task news, a look); `look`, `search` (cascade only), `quoted` (cascade only) | both |
| `Reminder` | a due reminder's text; "data, never permission to run tools" | both |

**Outputs:**

| Output | Required today? | Notes |
|---|---|---|
| `Started` | yes, before any output that needs it | the cascade sends it at once (`second_provider.rs:41-49`). A request may instead fail `NotDelivered` without it: the coordinator takes `NotDelivered` only before `Started` and refuses it after (`coordinator/cloud.rs:283-303`), so a worker that never delivered must not invent a `Started` (reviewer-two on #3321) |
| `Audio` | yes | 24 kHz mono i16, at most 960 samples a packet, at most 30 s of PCM queued or reserved at once (not a reply's whole length), and two events of credit, which counts transcripts and lifecycle events too (`provider_flow.rs:6-7, 73-116`); checked in `coordinator.rs:7029-7049` |
| `Transcript` | yes | the person's words with `request: None`, the answer's text with the request |
| `TurnComplete { idle }` | yes | |
| `TurnFailed { stage }` | yes | `NotDelivered`, `InputOpen`, `AwaitingResponse`, `Responding` (`provider_worker.rs:1090`); the coordinator's handling is in `coordinator/cloud.rs` |
| `GenerationComplete` | both send it | ignored by the coordinator (`coordinator.rs:7113`) |
| `Unclear`, `HoldingAnswer`, `RequestedAudio`, `LookAsked`, `NewsAnswered`, `ReminderDeferred` | optional, both | |
| `Stages`, `WordsEnd`, `LookNoCall` | optional, cascade only | "The Live route sends none" |
| `Interrupted`, `InputDiscarded`, `OwnershipAssumed`, `Ended` | Live's link semantics | the cascade never sends `Interrupted` or `Ended` |

**Turns, stops and barge-in belong to the coordinator.** It owns the
microphones, the activity detector, the end of a line (`--endpoint
smart-turn-fast`), admission, and the duplex mask that lets a person cut in
over the lamp (`turn_taking.rs`). Gemini Live is run with its own activity
detection off: `"automaticActivityDetection": {"disabled": true}`,
`"activityHandling": "START_OF_ACTIVITY_INTERRUPTS"`
(`crates/gemini/src/wire.rs:647-651`), so the lamp's `Start` and `End` are
Gemini's turns. A stop by hand or head is a cancel of the turn owner
(`cancel_reply`, `coordinator.rs:13886`) published as a new authority
snapshot; output for a turn that no longer owns the speaker is refused at the
playback boundary. A new `Start` supersedes the old request: the cascade
keeps "at most one turn at a time" and dropping a turn "stops its chat and
speech at once" (`second_provider.rs:1658-1667`).

**Tools and function calls.** One module, `tools.rs`, declares and runs every
function (the lamp's own, the actions part's `tools.json` and `call.sock`,
skills, schedule, memory, tasks): `declared` (`:520`), Gemini Live's
32-function bound (`within_bound`, `:562`), `run_call` (`:1362`). The bound
is the route's own: the cascade's is 48 (`second_provider.rs:179-187`), and
its worker takes `tools::uncut`, adds its own search and look, and fits that
(`second_provider/worker.rs:1140-1167`); its search and `more_functions`
are answered in the worker, outside `run_call` (reviewer-two on #3321). The
cascade maps the declarations to the OpenAI `tools` schema
(`docs/second-voice.md:1720-1742`). The types are Gemini's
(`lamp_gemini::{Tool, ToolCall}`, `tools.rs:32`).

**The Policy gate.** With `LAMP_POLICY_HOLDS_ACTIONS=on` an action waits for
the Policy's attention decision for its turn (`policy_holds.rs`). The cascade
has its own copy of the gate, "as it holds them on the Live route"
(`second_provider/policy.rs:1-8`).

**Whom the lamp answers.** The attention rule (`attention.rs:1-43`) runs
inside each worker: on Live against the service's text of what it heard
(which arrived before the first answer sound by at least 0.32 s), on the
cascade against speech to text's words before the chat is asked
(`second_provider.rs:28-35`). A held answer is dropped in the worker.

**Say cues, reminders, task news.** The say service (`say.rs:1-70`) and the
schedule and tasks parts reach the route as `Written` or `Reminder` turns,
behind the coordinator's gates. A skill's own sound, a built-in cue and a
recorded line are played by the coordinator and never reach a route. An
add-on's quoted line needs a turn asked with no functions, so it is refused
on anything but the cascade (`quoted_refused`, `coordinator.rs:12693`).

**Requested audio.** The worker reports a Skills socket's offer as
`RequestedAudio`; the coordinator plays it under its own final guards
(`requested_audio.rs:1-5`).

**Looks.** `take_a_look` and a skill's `"look": true` come back as
`LookAsked`; the coordinator takes the full look and sends a `Written { look }`
turn; the worker attaches the eyes' newest frame (`--eyes-look`). On Live
the worker may also offer a frame as a person begins to speak, at most every
30 s (`sight.rs`), and stream camera context (`visual_context.rs`), both
through `lamp_gemini::Supervisor::try_image`.

**Memory and the person.** Inside each worker: the per-request memory scope
(`memory_scope.rs`; bound per request in `provider_worker.rs:5340-5390`), read
from the eyes' known file; the memory note, the journal
(`remember::finished`) and the forget paths. Shared files: the conversation
thread (`docs/conversation-thread.md:306`, "Two voice routes, one file") and
the character (`ProviderConfig::instruction`, "The same for every voice
route", `config.rs:401-415`).

**Privacy and mute.** Every input carries its privacy generation; the
coordinator clears a held `Line` at once on mute (`second.rs:24-29`); a
physical or software mute ends the service and it restarts after verified
cleanup (`service_privacy.rs`). What leaves the lamp differs by route
(`docs/service-privacy.md:218`; `docs/second-voice.md`, "Second only").

**Telemetry.** The coordinator writes the session's events
(`record(trace, …)`): `provider_first_audio`, `answer_first_audio` (route
aware, `coordinator.rs:7092`), `answer_stages` from `Stages` (cascade only,
`:7139-7153`), `run_start`. The link drives the offline line, its breath and
`lamp/online`; the cascade derives its link from probes of its hosts
(`second_provider/link.rs:1-15`).

**How behaviour reaches a worker.** `provider_arguments`
(`coordinator.rs:3733-3797`) passes 37 groups of flags to whichever worker
runs, and both parse them into the same `Files` (`provider_worker.rs:259`).
Environment opt-ins: 24 go to both workers; 5 opt-ins, the credentials folder and the 54-name list go
to the cascade only, 1 (`LAMP_MEMORY_EARLY_WORDS`) to Live only
(`process.rs:1070-1535`). So flags are shared, but each worker decides which
it honours.

### 1.3 Feature parity

Method: the rows of the two voice tables in `docs/switches.md` (lines
103-344, 226 `LAMP_*` switches), sorted by what their own text says about
routes, then each borderline row read by hand. Approximate, as the doc is a
census of the code, not the code.

| Scope | Count | Main examples |
|---|---|---|
| Cascade only | **64** | `LAMP_SECOND_SCOPED_CONTEXT` (per-person context, the lead's decision of 21 Oct), `LAMP_SECOND_SPEAKING_FACE`, `LAMP_SECOND_WAIT_CUE`, `LAMP_SECOND_DOWN_LINE`, `LAMP_SECOND_EARLY_FINALIZE`, `LAMP_SECOND_UNFINISHED_WAIT`, `LAMP_SECOND_SKILL_LOOK` / `LOOK_NOTE` / `LOOK_WALK`, `LAMP_ADDON_SAY`, `LAMP_OTHERS_ANSWER`, `LAMP_GOING_ON_SAME_ONE`, `LAMP_JOIN_CUT_WORDS`, `LAMP_TURNED_SPEAKING`, `LAMP_MEMORY_FOLLOWS_FACE`, `LAMP_MEMORY_OBSERVE`, `LAMP_FORGET_ASKED`, `LAMP_TASKS_NAMED_ASK`, `LAMP_PLACE`, `LAMP_SECOND_WEATHER`, the provider and speed switches |
| Both routes | **32** | `LAMP_ATTENTION_NEAR_CALIBRATED`, `LAMP_ADDRESSED`, `LAMP_NAME_OR_FOLLOW_UP`, `LAMP_GOING_ON_ASKS`, `LAMP_CUT_IN_TOLD`, `LAMP_HOLDING_ANSWER`, `LAMP_MISHEARD_HUM`, `LAMP_OTHER_LANGUAGE`, `LAMP_POLICY_HOLDS_ACTIONS`, `LAMP_HEALTH_FACTS`, `LAMP_TIME_PLACES`, 14 `LAMP_MEMORY_*` switches |
| Live only | **5** | `LAMP_REMARKS`, `LAMP_TRANSCRIPTION_LANGUAGE`, `LAMP_MEMORY_EARLY_WORDS`, `LAMP_LIVE_SCOPED_SESSION`, `LAMP_PROVIDER_WRITES` |
| No route named | 125 | the coordinator's own (turn-taking, stop words, privacy, the ring, say, offline, endpoint) and switches shared by flags (`LAMP_SPOKEN_TO`, `LAMP_TOOLS`, `LAMP_NAME_NOTE`, task news, schedule) |

Since the cascade became the product's voice on 19 Oct, features land there
first. Of the product unit's 88 `LAMP_*` settings, 13 are cascade-only
(`LAMP_SECOND_SCOPED_CONTEXT`, `SPEAKING_FACE`, `WAIT_CUE`, `DOWN_LINE`,
`EARLY_FINALIZE`, `PLAYOUT_START_MS` and the provider and speed choices). A
lamp switched to `gemini` today quietly loses them; its per-person context
would rest on `LAMP_LIVE_SCOPED_SESSION`, which is off and has no lamp run.

The coordinator also checks the route by its worker's name in four places:
the playout start (`playout_for`, `coordinator.rs:175`), the A/B's first
audio (`:7092`), the open name (`:7406`) and quoted says (`:12693`). Texts
from the app are answered only by the cascade's worker, so with `gemini` and
no standby every text is refused (`coordinator.rs:3824-3831`).

### 1.4 Are the cascade's parts pluggable?

Yes, behind one seam, chosen by environment at start.

- **The seam.** `Services` (`second_provider/services.rs:17`): `listen()`
  (a `Listener`, `:101`: `send`, `finish`, `early`, `said`, `open`),
  `chat(messages)`, `speak(text)`, plus warm-ups and relay hooks. `Net`
  (`:152`) wraps the real clients; tests use fakes.
- **Choice.** `RouteFlags` (`second_provider/worker.rs:420`) from
  `LAMP_SECOND_CHAT` (`gemini`, `openrouter`, `deepseek`; unset = the
  `second.json` chat), `LAMP_SECOND_SPEECH` (`gemini`, `elevenlabs`,
  `openai`; unset = `second.json`'s `tts_provider`) and `LAMP_SECOND_SEARCH`
  (`deepseek`, `openrouter`, `exa`), each a table of word and worker flag
  (`:437-451`); anything else is refused at start (`from_values`, `:501`).
  `services()` (`:888-985`) builds `Net` and picks the keys.
- **Text model.** One OpenAI-compatible streaming client, `Chat`
  (`net/chat.rs:224`), with constructors `new` (the `second.json` endpoint),
  `gemini`, `openrouter`, `deepseek` (`:278, :293, :312, :355`). *A new
  model:* an OpenAI-compatible one is **S** (a constructor, a `ChatRoute`
  word, its key or relay path, whether it `sees` pictures, its cache
  fields). One with another protocol is **M** (a client that yields the same
  `ChatEvent`s, tool calls included).
- **Speech to text.** Deepgram's websocket protocol only, to Deepgram or
  through the Autonomous relay (`Stt`, `second_config.rs:216`; `net/stt.rs:1-3,
  575-576`), with Flux as a model switch. *A new service:* **M**, a
  `Listener` with interim words (`early`, `said`) and a final, plus config
  fields and a key.
- **Speech.** `TtsProvider { OpenAi, ElevenLabs }` (`second_config.rs:222`;
  request shapes in `net/tts.rs:134-145, 424-425`) and Gemini's own voice
  (`net/gemini_tts.rs`), 24 kHz packets out. *A new voice:* **S to M**, a
  request and a decoder to 24 kHz mono, plus its relay route.

---

## Part 2. Design

### 2.1 Goals and limits

- One list of routes, one contract, one place for behaviour that must not
  fork. A feature is written once and every route that can carry it does.
- A route is a whole experience: listening, the answer and the voice. The
  coordinator, the Policy, tools, memory and privacy stay the lamp's.
- The person can switch from the app; the lamp restarts its voice cleanly
  and says nothing it shouldn't while it does.
- Not in scope: running two routes at once for one person, or switching
  mid-conversation.

### 2.2 The route registry

One table, in `crates/voice-processes` beside `WorkerRole`
(`crates/voice-processes/src/lib.rs:92`), which already names the voice's
roles "in one place" so the records guard and the voice can't drift. The
selftest and the setup helper read it from there.

| Route | Accepted values | Worker role | Config | Credentials (through the relay by default) | Standby | Selftest |
|---|---|---|---|---|---|---|
| `cascade` | `cascade`, `second` (kept forever: lamps' drop-ins and tests say it) | `second` | `provider.json` (character) + `second.json` + `LAMP_SECOND_*` choices | relay key `/etc/lampOS/voice/credential`; own keys optional | none | today's `voice_route` checks |
| `gemini-live` | `gemini-live`, `gemini`, unset or empty (as today) | `provider` | `provider.json` | relay key, relay's Gemini websocket | `cascade` if `LAMP_VOICE_SECOND_CONFIG` | new: `provider.json` and its `credential_file` (today `not_fitted`) |
| `openai-live` | `openai-live` | new `openai-live` | `provider.json` (character) + new `openai-live.json` (endpoint, model, voice, delegation mode, credential file) | relay key, through a new relay route (backend work); a direct key only in a probe's transient unit | none at first | new: its config and key |
| `dessa` | `dessa` | none | none | none | none | `fail`: "not available yet" |

Each entry also says:
- **Capabilities** (2.3): what the route carries, read by the coordinator in
  place of `role == "second"`.
- **What leaves the lamp**, for `docs/service-privacy.md` and the app's
  screen.
- **Offered**: whether the app may list it (2.6).

`dessa` is accepted only so the app and the docs have a name to show; the
voice refuses to start on it with one journal line, as it refuses an unknown
route today, and `lamp-selftest` says why. It gets a worker when an API
exists.

`run_start` says `voice_route` for every route, not only the cascade, and
voice-eval's importer learns the new values (`crates/voice-eval/src/events.rs:36`,
`physical/session.rs:108`).

### 2.3 The common contract

Three layers, each with one owner.

**1. The coordinator owns the room** (unchanged, never in a route):
microphones, echo, the activity detector, the end of a line, admission and
the duplex mask, stop words and hand and head stops, turn ownership and the
playback guard, privacy and mute, the ring and expression, the say service's
gates, requested audio's playback, taking looks, the offline line, the
session's events and the bus.

**2. Shared turn services, outside every route.** Today these modules are
shared but each worker wires them itself, and the cascade has copies (the
Policy gate). They become one library with one entry point per job, called
by every route's worker:

| Service | Today | One entry point |
|---|---|---|
| Whom the lamp answers | `attention.rs`, run on Live's transcript and on the cascade's words separately | `judge(words, turn facts) -> answer / hold / ask again`, given a route's words for a request, before any sound is let out |
| The Policy gate | `policy_holds.rs` and `second_provider/policy.rs` | one gate for every action a route runs |
| Tools | `tools.rs` (Gemini's `Tool` type) | neutral `ToolSpec` / `Call` types, mapped by each route to its service's schema; **each route declares its own function bound** (Live 32, the cascade 48) and its worker-local answers (the cascade's search and `more_functions`), so moving tools to the shared library drops none of the cascade's |
| Memory and the person | `memory_scope.rs`, `remember.rs`, `known_faces.rs`, wired per worker | `RequestScope` bound at `Start`, the note, the journal, the forget paths, one call each |
| Notes beside a line | name note, staff note, session fact, room, air, health, seems, cut-in, after-stop | one builder that returns the notes for a request; a route only places them |
| Character and thread | `config.rs:401-415`, the thread file | unchanged: already shared |
| Texts from the app | the cascade worker's chat (`messages.rs`) | a text brain any route may run (2.4), so texts don't need the cascade |

**3. Each route owns only what is its own:** turning the line into words or
an understanding, getting the answer, and speaking it in its voice.

**The wire contract, version 1.** Written down in one module
(`route_contract.rs`) and in `docs/voice-routes.md`, from today's enums:

- Required in: `Start`, `Audio`, `End`, `Called`, `CountOnly`, `Written`
  (with `look`), `Reminder`. Required out: `Started` (before any output that
  needs it; a request that was never delivered fails `NotDelivered` without it),
  `Audio`, `Transcript` (the person's words and the answer's), `TurnComplete`,
  `TurnFailed` with a stage the coordinator accepts; `Ready` and
  `ProviderLink` on control.
- Rules every route keeps: one turn at a time and a new `Start` drops the old
  one's work at once; nothing of an older privacy generation is sent
  anywhere; no input is replayed across a privacy change; a request's output
  carries its request; the route never assumes it may speak (the coordinator's
  guard decides); failures are classified by stage.
- Optional, by capability: `Paused`, `Written.search`, `Written.quoted`,
  `Stages`, `WordsEnd`, `LookAsked`, `LookNoCall`, `RequestedAudio`,
  `HoldingAnswer`, `Unclear`, `NewsAnswered`, `ReminderDeferred`.
- `Ready` gains the route's capability list (`exact_words`, `early_words`,
  `stages`, `search_looks`, `quoted_says`, `texts`, `sight`, `holds_answer`,
  …). Worker and coordinator are the same binary, so the field is safe to
  add. The coordinator's four `role == "second"` checks and the texts check
  read capabilities instead.
- Neutral types: `Tool`, `ToolCall`, `LinkFailure`, `Context` move out of
  `lamp_gemini`'s vocabulary into the contract module, so a third route
  doesn't depend on Gemini's crate.

**A conformance suite.** One table of scenes run against each worker with
fake services: a line answered; a cut-in mid-answer; a hand stop; a mute
mid-line; a written turn with a look; a reminder; a tool call held by the
Policy; a call during a superseded turn (never run); link lost before and
after the first audio; a quoted say. The fixture, `coordinator/tests/second_route.rs`
and the Live worker's tests are the start.

### 2.4 How full-duplex routes plug in

**Gemini Live already does.** It is a duplex service run in the lamp's
turns: its activity detection is off and the lamp's `Start`/`End` are its
`activityStart`/`activityEnd` (`crates/gemini/src/wire.rs:647-651`). Our turn
rules, the attention rule (on its input transcript) and the tool desk all
apply. `gemini-live` needs parity work, not a new shape.

**GPT-Live is different.** From OpenAI's guides (read 24 Oct,
[live conversations](https://developers.openai.com/api/docs/guides/live-conversations),
[live delegation](https://developers.openai.com/api/docs/guides/live-delegation)):
it listens and speaks at once and decides turns itself; the guide describes
no way to switch its own turn-taking off, only
`session.input_audio.mute`/`unmute`; transcripts come as
`session.input_transcript.delta` and `session.output_transcript.delta` with
no completed-turn event; audio over a websocket is PCM16 at 24 kHz by
default, both ways; in client delegation (`delegation.type: "client"`, the
default) it sends `session.delegation.created` with an id and no request
text, and the client answers with `session.commentary.append` (said aloud,
"may paraphrase") and `session.thinking.append` (quiet context), each at most
500 tokens; `session.instructions.append` can interrupt its speech but
cancels no backend work; the mode is fixed per session. The repo's own
research has it at 0.8 to 1.3 s published, US and Europe only, $0.05 a
minute billed by the second
(`docs/faster-answers.md:613, 649-656`).

So `openai-live` plugs in like this:

1. **Lamp-gated listening (the first mode).** The worker keeps input muted
   and unmutes it only for an admitted line: `Start` unmutes and sends the
   line's audio (16 kHz resampled to 24 kHz), `End` mutes again. GPT-Live
   then hears what Gemini hears today and nothing else, so the coordinator's
   turn-taking, admission and the privacy promise ("each admitted line's
   audio goes to …") stand. A cut-in is a new admitted `Start` (unmute)
   while the coordinator has already cut the old answer's playback.
2. **Our rule before any sound.** The worker feeds `input_transcript.delta`
   to the shared attention judge and holds the request's output audio until
   it has judged, as the Live route does. Whether its transcript comes before
   its first audio, as Gemini's does by 0.32 s, must be measured first.
3. **Our model and tools behind it (client delegation).** On
   `session.delegation.created` the worker builds the request from the line's
   transcript (it has the request's Start and End), then runs the shared text
   brain: the cascade's chat-with-tools loop without speech to text or speech
   (DeepSeek through the relay, the tool desk, the Policy gate, the request's
   memory scope). It returns each result once it stands alone as
   `session.commentary.append` and progress as `session.thinking.append`.
   A superseded or stopped request's task is dropped, and its calls are not
   run, as today (`provider_worker.rs`, `retired`). Memory notes and the
   person's name go in as `session.thinking.append` at the line's Start; the
   character as instructions at session start (its 16,384-token limit fits
   our 16,384-byte character file).
4. **Ending a turn.** With no turn-complete event, the worker sends
   `TurnComplete` after the answer's audio has been idle for a bound and no
   delegation is open. This is a guess the coordinator will act on, so the
   bound is measured, not chosen.
5. **What it can't promise.** Commentary is paraphrased, so a reminder's
   words, an add-on's quoted line and onboarding's sentence are not said
   verbatim: the route declares `exact_words: false`, and the coordinator
   refuses a quoted say there as it does on Live today. Reminders and task
   news are still said, in its words.
6. **Its voice.** A switch to it changes the lamp's voice (today one voice
   per lamp, `second_provider/worker.rs:399-403`). The app says so.

**A second mode later, "open duplex":** stream every unmuted second and let
GPT-Live take turns itself, the lamp's rules then gating only output and
actions. It changes what leaves the lamp (all room audio while unmuted, not
admitted lines) and the turn rules the product is tuned on, so it needs the
owner's and the privacy review's yes, and its own played tests.

**Dessa:** no API, so nothing to plug in. When one exists it takes the same
questions: can the lamp gate its listening, can our tools sit behind it,
does it say exact words.

### 2.5 How a user switches

The same path the person's own keys already use (`docs/own-keys.md`;
`crates/setup/src/keys.rs`; `crates/cloud/src/settings.rs`):

1. **The app** sends `voice.route.get` and `voice.route.set {"route",
   "request_id"?}` to `lamp-cloud`, answered `starting`, then `success` or
   `failure`, as `keys.set` is. `get` returns the route, the offered routes
   and why any is not.
2. **`lamp-cloud` writes nothing.** It hands one line to root's helper
   (`lamp-setup change`, or a `lamp-setup voice` beside it) on its systemd
   socket; the helper answers only `lamp-cloud`'s cgroup, as the keys helper
   does.
3. **The helper checks** the route is in the registry and offered, and that
   its files and keys are there (the selftest's checks, called as a library),
   then writes one drop-in, `/etc/systemd/system/lamp-voice.service.d/route.conf`,
   with `Environment=LAMP_VOICE_ROUTE=<route>` and that route's own tier-3
   switches only, and runs `daemon-reload`. The helper refuses any other
   name, so `route.conf` can never carry a privacy switch (reviewer-three on
   #3321). `route.conf` is in both reset plans
   (`setup/fixtures/reset-plan-*.json`), as the own-keys folder is, so a
   reset lamp never sends the next owner's lines to the provider the last
   owner chose. A lamp's own drop-ins already outlive updates
   ("A lamp that needs a switch off says so in lamp-voice.service.d/",
   `deploy/product/lamp-voice.service:3`).
4. **A quiet restart.** It waits for quiet as an update does (voice resting,
   nobody heard, nothing said for 45 s; `change::wait_for_quiet`,
   `crates/update/src/quiet.rs:18`), at most 30 minutes, then restarts
   `lamp-voice`.
5. **Proof, or back.** `success` only once the new voice's `run_start` says
   the new `voice_route` and it is listening. If it doesn't start (the voice
   never falls back by itself, #1712), the helper puts the old drop-in back,
   restarts again and answers `failure` with why.

**What is lost on a switch:** the turn in hand (the wait for quiet avoids
most), the provider's session context (the thread file carries the talk
over, "Two voice routes, one file"), the per-chat text exchanges kept in
memory for the app's texts, warm connections and the prompt cache (the first
answers are slower), the face cache, which starts cold after a voice start
(`memory_scope.rs:1-13`), and say asks in flight (their askers are told
`dropped`). Reminders, memory, the room file, mute state and skills are
kept. The lamp's voice itself changes.

**What a switch sends:** what is kept (memory, the thread file, the room
file) is also what the new provider receives from its first turn. "What
leaves the lamp" in `docs/service-privacy.md`, and the app's screen, say so
before `voice.route.set` (reviewer-three on #3321).

### 2.6 The parity matrix and the bar for offering a route

- **Make it from the code.** Each switch row in `docs/switches.md` gets a
  **Routes** column (`all`, or the routes named); voice-eval's switches test
  (which already checks every row's **Counted as**) also checks the column
  against the registry's capabilities. The matrix page is generated, not
  kept by hand.
- **Tier 1, must have before the app lists a route:** privacy (only admitted
  lines leave the lamp; mute ends the session; generations honoured); stops
  and cut-ins within the coordinator's bounds; the attention rule and
  `LAMP_SPOKEN_TO`; per-person memory scope **with the speaking face**, or
  every turn the room's, and the forget paths (reviewer-three on #3321: on
  the product these are one decision, `LAMP_SECOND_SCOPED_CONTEXT` beside
  `LAMP_SECOND_SPEAKING_FACE`, and a route with scope but no speaking face
  would keep a bystander's line as the known person's); the Policy
  gate; the tools (light, volume, schedule, memory, tasks, skills, the time);
  reminders, task news and the say service's written turns; looks; requested
  audio; the offline line and `lamp/online`; texts from the app (through the
  shared text brain); the first-audio events for the A/B; a selftest check.
- **Tier 2, should have:** the wait cue or the route's own sign it heard; the
  down line; early words; holding answers; said-as-heard; skill looks and
  the look walk.
- **Tier 3, the route's own:** provider and speed switches.
- **The proof:** the played tests (`pauses.sh`, `plain.sh`, `busy.sh`,
  `hold.sh`, `stories.sh`, `checks.sh`) and real people's lines, each route
  in the same hour, with the median end of line to first sound and the cost
  per hour of use. A route below the cascade on answered lines is not offered.

Today `gemini-live` misses tier 1 on per-person context (only through
`LAMP_LIVE_SCOPED_SESSION`, never run), on the speaking face (a switch to
`gemini` quietly loses `LAMP_SECOND_SPEAKING_FACE`), and on texts (only with
the cascade standing by).

### 2.7 Phases

**Phase 1. Make the contract explicit around the two routes we have.**

| Step | Effort | Risk |
|---|---|---|
| 1a. The registry in `voice-processes`; new names with `second` and `gemini` kept; `run_start` marks every route; voice-eval reads the new values | S | a renamed value breaks an import or a lamp's drop-in: keep the old words |
| 1b. Capabilities in `Ready`; the coordinator's `role == "second"` checks and the texts check read them | S | a check missed: grep for `"second"` in `coordinator*` and add a test per capability |
| 1c. The contract module, `docs/voice-routes.md`, neutral types, the conformance suite over fixture, cascade and Live | M | the suite tests what we wrote, not the services: keep the played tests as the judge |
| 1d. Shared turn services: one attention entry, one Policy gate, one memory binding, one note builder, the text brain out of the cascade worker | L | the product's voice changes by accident: refactor only, no behaviour change, ARM64 run and played tests per merge train; the cascade first |
| 1e. The **Routes** column and its check; the matrix generated | S-M | rows judged wrong by hand: read each borderline row's code path |
| 1f. `gemini-live` to tier 1 (per-person context, texts without a standby) or marked experimental | M | Live's cost; its parity runs need lamp time |

**Phase 2. `openai-live`.**

| Step | Effort | Risk |
|---|---|---|
| 2a. A probe (`probe-openai-live`, as `probe-cascade`) in a transient unit: first sound after a muted-again line, whether it answers after `mute`, transcript before audio, a cut-in, a delegation round trip, `session.usage.updated` | S | its answers decide the whole design (open questions 1-4) |
| 2b. A relay route for GPT-Live's websocket (backend team) | M | outside this repo; lamps carry no provider keys |
| 2c. A client crate (`crates/openai-live`) and the worker role, contract v1, lamp-gated listening, link states and failure stages | L | turn ends are guessed; sessions open all day may bill all day: open per conversation, close when quiet |
| 2d. Client delegation on the shared text brain | M | paraphrase; duplicate actions if a request is rebuilt twice: one task per delegation id, results checked before retry |
| 2e. Tier 1 and the played tests, cost per hour | M | latency from the customers' region unknown (our customers speak English, the owner, 24 Oct) |

**Phase 3. The user's switch.** `voice.route.get/set` in `lamp-cloud`, the
root helper, the drop-in, the quiet restart, proof and rollback, the
selftest, `docs/cloud.md` and `docs/setup.md`; the app's screen in the app's
own repository. **M.** Risks: a switch at the wrong moment (the quiet wait),
a route that won't start (rollback), a person surprised by a new voice (the
app says so first).

**Phase 4. `dessa`** when Block publishes an API: a probe, then the same
steps as phase 2. Effort unknown.

### 2.8 Open questions

1. Does GPT-Live answer a line whose input is muted after it, or does it
   need trailing silence or an instruction? This decides lamp-gated
   listening (probe 2a).
2. Does its input transcript reach us before its first audio, so our rule
   can judge before any sound? By how much?
3. What exactly is billed by the minute: the open session or voice time?
   Must the session close between conversations, and what does reopening
   cost in latency?
4. Is client delegation's paraphrase acceptable for reminders, task news and
   skills' says, or do those need our own speech (and then two voices)?
5. Texts from the app: should every route run the shared text brain (one
   more model connection, but texts work on any route), or are texts only
   on routes that carry them?
6. Should `gemini-live` keep the cascade standing by, and should
   `openai-live` get one? The take-over logic is written for Gemini's limits.
7. Which routes does the app offer before 27 Nov, and is a route the app
   lists a promise of parity or allowed as "experimental"?
8. Whose decision is "open duplex" (all room audio while unmuted)? It changes
   `docs/service-privacy.md`'s promise.
9. One voice per lamp: does a route switch keep the lamp's voice where it
   can (the cascade's voice is chosen; Live's and GPT-Live's are theirs)?
10. Does the setting live in a drop-in only, or also in `crates/settings`'
    declared store, with the drop-in as its applied form?
