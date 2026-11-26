# A person's own keys (#2094, built, on in the product since #8367)

Product lamps answer through the Autonomous relay and hold no provider key of
the company's (the lead on #1711, 20 Oct 07:34). A person may bring their
own instead, as in V1: a **DeepSeek** endpoint and key for the text model,
and a **Deepgram** key for speech to text. Each is optional. With none, the
lamp uses the relay, as before. The design was agreed on #2094 (El 09:42,
the first builder 09:44, Third's privacy read 09:50).

**`LAMP_OWN_KEYS=on`, on in the product (#8367)** on three units:
`lamp-setup.service`, `lamp-cloud.service` and `lamp-voice.service`, the
voice's with the keys' folder (`ConfigurationDirectory=lampOS/voice/own`,
mode 0700, and `LoadCredential=own:/etc/lampOS/voice/own`, the lines
`deploy/product/own-keys-voice.conf` gave test lamps by hand, which a lamp
no longer needs). `lamp-cloud` also needs `LAMP_CLOUD=on` and
`LAMP_SETTINGS_CHANGE=on`, both on in the product. `install` places the
helper's units (`lamp-setup-keys.socket`, enabled, and
`lamp-setup-keys@.service`), and a round adds them to a lamp installed
before, starting the socket with the round (`added::to_start`). The helper itself doesn't read the switch (its unit
passes `--enable`): `lamp-cloud` answers "not on this lamp" with the switch
off, and only `lamp-cloud` may ask. With a unit's switch set off in a
drop-in (`LAMP_OWN_KEYS=off`), nothing changes:
setup checks and keeps nothing and says nothing new, the app's kinds are
"not on this lamp", and the voice reads no person's key even if one is
there.

## How a key arrives

- **At setup** (`crates/setup/src/serve.rs`): the setup request's optional
  `deepseek_api_key`, `deepseek_base_url` (an https address; absent:
  `https://api.deepseek.com`) and `deepgram_api_key`, hidden in `Debug`.
  Setup runs as root. It checks each after the home's Wi-Fi is joined and
  before the marker, so the voice's first start takes them. A key that's
  refused, can't be checked or doesn't fit never fails setup: the lamp uses
  the relay, and `GET /api/device/setup/status` says why in `own_keys`, by
  service (absent when none was given). The status's `takes_own_keys` says
  whether this switch is on, true or false, so the app asks for own keys
  only on a lamp that will use them.
- **From the app, after setup** (`docs/cloud.md`, "A person's own keys"):
  `keys.set`, `keys.remove` ("use the lamp's own again") and `keys.get`.
  `lamp-cloud` writes nothing: it hands one line to root's helper,
  `lamp-setup keys`, on `lamp-setup-keys.socket`.
- **Who may ask the helper:** every part runs as `lampos`, so the socket's
  group can't tell `lamp-cloud` from a skill. The helper reads its peer by
  `SO_PEERCRED` and answers only a process whose cgroup
  (`/proc/<pid>/cgroup`) is the system's place for `lamp-cloud.service`,
  `/lampos.slice/lampos-parts.slice/lamp-cloud.service` or one under it,
  which only root can move a process into. A `lamp-cloud.service` anywhere
  else, such as one a user's own manager made under `user@<uid>.service`, is
  refused (the reviewer's P3 1 on #2167). Any other peer is refused before
  its line is read (Third's P2 on #2094). Peer credentials are read on
  Linux only.
- **What the check can't stop:** a `lampos` process that can attach to
  `lamp-cloud` (same user, with `kernel.yama.ptrace_scope` at 0) can make
  it ask. The check holds against a part that connects itself, not against
  one that controls `lamp-cloud`.

## The messages

The contract posted on #2094 (11:13, with its additions after), which
`lamp-cloud` and the helper hold to:

| Kind | Asked | Success `data` |
|---|---|---|
| `keys.set` | `{"service": "deepseek" \| "deepgram" \| "elevenlabs", "key"?, "base_url"?, "voice_id"?, "request_id"?}` (`key` only absent for ElevenLabs' voice alone) | `{"service", "uses": "own", "code": "works" \| "works_no_tools", "answer"}` (#8735, below) |
| `keys.remove` | `{"service", "request_id"?}` | `{"service", "uses": "lamp"}` |
| `keys.get` | `{"request_id"?}` | `{"deepseek": "own" \| "lamp", "deepgram": "own" \| "lamp", "elevenlabs": "own" \| "lamp", "since": {"deepseek": RFC 3339 \| null, "deepgram": …, "elevenlabs": …}, "deepseek_host": host \| null, "voice_id": {"elevenlabs": id \| null}, "offers": [...]}` (#8367, below; `in_use` and each offer's `agent`, #8735 below) |

Each is `starting` at once, then one `success` or `failure` of the same kind
within 15 s; `request_id` comes back in `data` on each. A `failure` has the
person's words in `error` and `data.code`: `refused`, `voice_refused`
(ElevenLabs' voice ID not on that account), `unreached`,
`not_kept` (the key worked but couldn't be written: the lamp uses its own),
`bad_field` (the field named), `busy` (one key request at a time),
`not_allowed` (a peer other than `lamp-cloud`) or `unavailable` (the helper
not there or not answering). Without the switch: V1's "not on this lamp" at
once, with no `starting`. Setup's status says `own_keys` by service as
`{"code", "answer"}`.

## The check

One request to the service before a key is kept, at most 5 s, no redirect
followed, and the only request setup or the helper makes with the key:

| Service | Request | Key |
|---|---|---|
| DeepSeek | `GET <endpoint>/models` | `Authorization: Bearer <key>` |
| Deepgram | `GET https://api.deepgram.com/v1/auth/token` | `Authorization: Token <key>` |
| OpenRouter (#8420) | `GET https://openrouter.ai/api/v1/key`, then `GET https://openrouter.ai/api/v1/models/<model>/endpoints` | `Authorization: Bearer <key>` on the first alone |
| Gemini (#8420) | `GET https://generativelanguage.googleapis.com/v1beta/openai/models/<model>` | `Authorization: Bearer <key>` |
| Anthropic (#8420) | `GET https://api.anthropic.com/v1/models/<model>` | `x-api-key: <key>` and `anthropic-version: 2023-06-01`, no bearer |

- **2xx:** kept. The answer names where the person's data goes now:
  "that key works: your conversations now go to api.deepseek.com with your
  key: your words, what the lamp remembers that's sent with them, and what
  it sees when asked to look", or for Deepgram, "that key works: your speech
  now goes to Deepgram under your account and its retention terms".
- **401 or 403:** "that key was refused". Not kept, and a key kept before
  stays as it was.
- **Anything else, a redirect, a timeout or no network:** "the lamp couldn't
  reach DeepSeek to check that key; nothing changed". Not kept.
- **2xx but the key can't be written** (a full or failing disk): "the key
  could not be kept; the lamp uses its own now", code `not_kept`. Keeping
  DeepSeek's takes the old key away first, so the person's key for that
  service is removed as far as it can be, and the voice is restarted to
  take the lamp's own (the reviewer's P3 2 on #2167).

## Where it's kept

`/etc/lampOS/voice/own/`: `deepseek.key`, `deepseek.url` (the endpoint,
the old key removed first, so a key never sits beside another key's endpoint) and
`deepgram.key`. The folder is root's alone, 0700, each file 0600 (no group or other reads it), written whole and
renamed into place. The voice's drop-in loads the folder whole
(`LoadCredential=own:/etc/lampOS/voice/own`), so each file is the credential
`own_<file>` in its `CREDENTIALS_DIRECTORY`; `ConfigurationDirectory=` makes
the folder if it's missing, so an empty one is no credential, never a failed
start.

A key is never in `provider.json` or `second.json`, a journal line, an
error, a reply, a session file or the bus. The 5 s reset wipes the folder as
one plain path (`reset-plan-product.json`, the rule on #2060), and
`lamp-selftest`'s voice route takes a credential's folder as present.

## The route at its next start

`SecondConfig::with_own_keys` (`crates/voice/src/second_config.rs`), with
`LAMP_OWN_KEYS=on`:
- `own_deepseek.key` there: DeepSeek's chat, its search and the memory's
  observation pass go to the person's endpoint with their key, over
  `LAMP_SECOND_DEEPSEEK_BASE_URL`, `deepseek_base_url` or the voice's own
  default for the company relay; whichever of those it was is kept as the
  relay a refused key falls back to.
- `own_deepgram.key` there: speech to text goes straight to Deepgram.
- Otherwise the relay. One journal line each, naming whose key and the
  host, never the key or a path: `DeepSeek: the person's own key, at
  api.deepseek.com (LAMP_OWN_KEYS)`, `Deepgram: the relay, no key of the
  person's (LAMP_OWN_KEYS)`. A key that can't be read (not private, an
  endpoint that isn't https) is passed over for the relay, said by its
  error's kind.

After a change the helper restarts the voice (`systemctl try-restart`) once
the voice has rested 1 s with nobody heard and nothing said
(`change::KEY_CHANGE_QUIET`, the lead's call on #8710: not an update's
45 s, so the person's next words already go the new way; a turn in flight
is waited out, never cut), and no update is at work: it takes lamp-update's lock and holds it
through the restart (`change::quiet_and_unlocked`, as the zone's
`lamp-schedule` restart does, which still waits the 45 s), since a round puts its build back when the
voice it restarted changes process in its watch (`updates.md` step 9).
While an update holds the lock it looks again 10 s later, after the quiet
again. It waits at most 30 minutes in all; otherwise the voice takes it at
its next start. Each helper that waits holds one of the socket's 4 connections
(`MaxConnections=4`) while it does: in a busy room, a fifth `keys.*` within
that half hour, `keys.get` included, gets `unavailable`, "the lamp's keys
are not running", until one restart is done (the reviewer's P3 4 on
#2167). Left so: four changes in half an hour is not a person's use, and
handing the restart to one waiter would need a unit of its own.

The voice's start lines name the host each time, never the path: DeepSeek's
chat, search and observation pass say "with the person's own key, at
api.deepseek.com (LAMP_OWN_KEYS)", so a changed endpoint shows in every
session's record, not only the day it was set (Third's P3 on #2167).

**`voice-keys`:** with the switch on, the second route's worker reports the
part to `lamp-health` once a second while it runs, `ok` with `uses=own` or
`uses=lamp`, whether or not the person's key is kept. So a key kept and
then removed (the voice restarted without it) leaves a part still reporting,
never one quiet for good, which would keep the lamp `stale` and, after 5
minutes, droop "Not feeling well" (the reviewer's P2 on #2167). Its bus publisher is named `voice_keys`, as the bus takes no `-` in a
publisher's name; before #2167's fix it was `voice-keys`, which the bus
refused, so the part never reported at all. Turning the
switch off, or dropping the second worker (#960), says once that
`voice-keys` is stopping (`state` = `stopping`, #2415's P3): the second
worker says it at its start with the switch off, and the voice says it when
it drops the second worker with the switch on. `lamp-health` forgets the
part instead of counting it quiet, so the lamp doesn't droop "Not feeling
well" for it, and tracks it again when it next reports
([health.md](health.md), "Stopping on purpose").

**A key that later fails** (a rotation): a request the person's endpoint
fails in any way before its first word (refused with 401 or 403, a 429 or a
5xx, #8362, or any other failure: `services::gives_way`, below) goes through
the relay it went
over, with the relay's key, so the person never gets a failed answer. One
journal line names the status, or says no answer came. The `voice-keys` part
tells `lamp-health` "your DeepSeek key was refused, using the lamp's own"
for a 401 or 403, and "your own model couldn't answer, using the lamp's own"
for anything else, until the person's own model answers again. That covers
the spoken chat; the search and
the observation pass don't fall back yet. **The person's answer is then out
of date:** they were told "your conversations now go to <host>", while each
refused request goes through Autonomous's relay again. Only `lamp-health`'s
`voice-keys` warning says so; the app could show it beside the keys screen
(the reviewer's P3 5 on #2167).

## Your own voice: ElevenLabs (#8367, built, on in the product since #8367)

The person's own ElevenLabs API key and voice ID, set from the app on the
same `keys.*` path, speak the lamp's live speech in their voice. Agreed on
#8367 (Mobile 12:02, the first builder 12:03 and 12:18) on #8362's
contract (the first builder 11:44, Mobile 11:46). The same switch,
`LAMP_OWN_KEYS`, and nothing new with it off. Run on lamp-52e6 through the
relay (#8394's row, ROW 7116296452); not yet against ElevenLabs' own
service.

- **The messages:** `keys.get` gains `offers`, one entry per service the
  lamp takes a key for, `kind` `"model"` or `"voice"` and `fields` the
  plain fields beside the key:
  `{"service": "elevenlabs", "name": "ElevenLabs", "kind": "voice", "models": [], "base_url": false, "fields": ["voice_id"]}`,
  after DeepSeek's (`"kind": "model"`, `"base_url": true`) and Deepgram's.
  It also gains `"elevenlabs": "own" | "lamp"`, `since.elevenlabs` and
  `voice_id: {"elevenlabs": id | null}`. A voice ID isn't a secret and
  comes back; the key never does. `keys.set` takes
  `{"service": "elevenlabs", "key"?, "voice_id"?, "request_id"?}`: a key
  needs a voice ID with it the first time ("ElevenLabs needs a voice ID
  with its key"), and later keeps the voice kept; with no `key` only the
  voice changes, checked with the kept key, and only beside one, otherwise
  `bad_field`, "a voice ID is changed beside a kept ElevenLabs key: give the
  key too". A voice ID is 1 to 64
  letters and digits. `keys.remove` takes `elevenlabs`.
- **Where it's kept:** beside the others, `elevenlabs.key` and
  `elevenlabs.voice` in `/etc/lampOS/voice/own/`, root's, 0600, so the
  voice reads them as `own_elevenlabs.key` and `own_elevenlabs.voice`. Never
  in a reply, an error, a journal line or a report.
- **The check:** one request, `GET https://api.elevenlabs.io/v1/voices/<voice_id>`
  with `xi-api-key: <key>`, which tries the key and the voice at once. 2xx:
  kept, "that key and voice work: the lamp now speaks in your ElevenLabs
  voice, and what it says goes to ElevenLabs under your account and its
  terms". 401 or 403: "that key was refused". 400 or 404: "that key works,
  but that voice ID isn't on its ElevenLabs account; nothing changed",
  code `voice_refused`. Anything else: unreached, nothing changed.
- **The speech:** at the voice's next start, `with_own_keys` puts the
  person's key and voice ID on the route's ElevenLabs speech, straight at
  `https://api.elevenlabs.io/v1` instead of the relay, over the language's
  and the chosen voice (#5394), when the route speaks with ElevenLabs. It
  is never sent through the relay's near address (`relay_routable`), so the
  person's key goes to ElevenLabs' own API alone, the probes' included. One
  journal line, "ElevenLabs: the person's
  own key and voice (LAMP_OWN_KEYS)", never the key or the ID. The helper
  restarts the voice only once the lamp is quiet ("The route at its next
  start"), so the change is heard from the next thing the lamp says, and a
  sentence being spoken is never cut.
- **The cached fixed lines (`say/tts_cache.rs`, the Reviewer-lead's P2 on
  #8394):** the speech cache, in the voice's main process, reads the
  person's key and voice ID the same way (`with_own_keys`, `LAMP_OWN_KEYS`
  and the unit's credentials), so the lamp's fixed lines are warmed and
  played in the person's voice, one journal line, "the speech cache keeps
  its lines in the person's own voice, apart from the lamp's". Their key
  names the voice that rendered them: `own:<voice_id>@<address>`, the
  address the person's voice is asked at (ElevenLabs' own API, or a test
  lamp's `LAMP_OWN_ELEVENLABS_URL`), so they are never the lamp's lines,
  another voice ID's or another address's; the lamp's own lines keep their
  key as before. A new key with the same voice ID plays the lines kept, the
  same voice. The person's render has no fallback of its own: a refusal
  (401 to 404) is a render that failed, never kept under the person's key.
  The line is said again in the lamp's own voice and kept under the
  lamp's key, and every later line is the lamp's, kept or rendered, until
  the voice starts again; one journal line, "the speech cache: ElevenLabs
  refused the person's own key or voice (HTTP <status>)". A line not kept
  in the person's voice yet plays the lamp's kept one. With the key
  removed, the voice starts again in the lamp's voice and plays the lamp's
  kept lines. The app's voice preview (#7485) always uses the lamp's own
  key, never the person's.
- **The fallback:** the relay's address, key and voice are kept beside the
  person's. A piece ElevenLabs refuses for the person's key or voice (401,
  402, 403 or 404, a bad key, a voice gone, credits run out) is said in the
  lamp's own voice after one line, "Your ElevenLabs voice isn't working
  right now, so I'm back to my own voice.", never silent; every piece after
  goes to the lamp's voice until the voice restarts, and `voice-keys` tells
  `lamp-health` "your ElevenLabs key was refused, using the lamp's own".
  Once per process, whichever piece or voice sees the refusal first (one
  shared state, `net::tts::OwnVoiceLost`; #8394's lamp row saw one 401
  journalled three times in 11 ms): one journal line, "ElevenLabs refused
  the person's own key or voice (HTTP <status>); the lamp's own voice from
  now on", and the notice said once, with its own line, "the person's
  ElevenLabs voice isn't working: told once, in the lamp's own voice", with
  the first answer's piece after it, never a line rendered ahead with no
  turn (the Start's, kept for later). A piece asked again over HTTP
  (`speak_fresh`) takes the same path.
- **Any other failure** (#8394's P2, the first builder): a piece the
  person's voice fails to start for another reason, a 400 or 422 (a voice
  the model or the plan won't speak, which the check can't catch), a 429,
  a 5xx, a lost connection, or a first sound later than 4 s
  (`START_WITHIN`), is asked once more, as any piece is (#4338). If that
  fails too, the person's voice gives way: the piece is said in the lamp's
  own voice after the same notice, said once, and every piece after goes
  to the lamp's voice until the voice restarts, with one journal line,
  "the person's own ElevenLabs voice failed a sentence twice (<why>); the
  lamp's own voice from now on", never the key or the ID. Before, the
  piece was left unsaid, and a voice that kept failing left every reply
  silent until the voice restarted. A refusal (401 to 404) still goes to
  the lamp's voice at once, the person's never asked again, and only a
  refusal makes `voice-keys` warn. The lamp's own voice failing a piece
  after that is asked as any voice is, never more
  (`in_one_voice_near` in `second_provider/services.rs`).
- **Off:** the kinds are "not on this lamp", the helper refuses, and the
  voice reads neither file, so the route is as it was.
- **Test lamps only, `LAMP_OWN_ELEVENLABS_URL` (#8394's lamp row):** an
  https address that takes the place of ElevenLabs' own API for the
  person's key and voice, read in one place (`lamp_msgs::own_voice`) by
  both the keys helper's unit and the voice's. It sends the person's key
  to that address: never set on a customer's lamp. It is read only under
  its gate, `LAMP_VOICE_LOOPBACK_SERVICES=on`, the voice's switch for test
  addresses (#8394's P2, the first builder: alone, it could send a
  person's key to any https host, and "what it says goes to ElevenLabs"
  would no longer be true). No product unit or drop-in sets either, which
  the voice's tests read every unit for, and without the gate the switch
  is ignored, so the person's key goes to ElevenLabs' own API alone; a
  test lamp's row sets both, in the voice's and the keys helper's units.
  Set with its gate, the check is one word said in that voice,
  `POST <root>/text-to-speech/<voice_id>/stream`
  (the relay has no `/voices`: a 404 on 23 Nov), where `<root>` is the
  address as the voice speaks to it (`/elevenlabs` added off ElevenLabs'
  own hosts, after the `/v1` the company relay's `/ai` takes), and the
  speech goes to the same address. The voice says one more line, "the
  person's own voice goes to <host>, not ElevenLabs' own API
  (LAMP_OWN_ELEVENLABS_URL, test lamps only)", the host alone. Unset,
  empty, not https, or without the gate: ElevenLabs' own API, as above,
  byte for byte. With the relay's address, key and voice it lets the lamp
  row run the person's path through the relay with no key of anyone's.

**Tests:** `crates/setup/src/keys.rs`
(`the_elevenlabs_check_asks_for_the_voice_with_the_key_in_its_header`,
`a_switched_check_asks_the_given_address_for_one_word_with_the_key`),
`crates/msgs/src/own_voice.rs` (the switch, its gate,
`without_its_gate_the_switch_is_ignored`, its check and its root),
`crates/voice/src/second_provider/net/tts.rs`
(`the_own_voice_check_root_is_where_the_voice_speaks`),
`crates/setup/tests/it/keys.rs`
(`elevenlabs_is_offered_beside_the_model_services`,
`an_elevenlabs_key_and_voice_are_checked_kept_and_never_said`,
`a_voice_alone_changes_only_the_voice_beside_a_kept_key`, and the pin
`with_the_switch_off_an_elevenlabs_key_changes_nothing`),
`crates/cloud/src/settings.rs`
(`keys_line_passes_an_elevenlabs_key_and_voice_or_a_voice_alone`),
`crates/voice/src/second_config.rs`
(`the_persons_elevenlabs_key_and_voice_speak_in_place_of_the_lamps`,
`a_test_lamps_switch_sends_the_persons_voice_to_its_address`, and the
pins `without_its_gate_the_switch_leaves_the_persons_voice_at_elevenlabs`
and `no_product_unit_sets_the_own_elevenlabs_url`) and
`crates/voice/src/second_provider/net/tts.rs`
(`a_refused_own_voice_falls_back_to_the_lamps_with_one_line`,
`a_refusal_seen_by_many_pieces_and_voices_is_said_once`),
`crates/voice/src/second_provider/services.rs`
(`the_persons_voice_failing_twice_gives_way_to_the_lamps_own`, for a 422,
a 429, a 503 and a 401;
`the_persons_voice_too_slow_twice_gives_way_to_the_lamps_own`;
`the_lamps_own_voice_failing_too_is_asked_no_more`),
`crates/voice/src/second_provider/probe.rs`
(`an_own_voice_key_is_never_sent_to_the_relay`) and
`crates/voice/src/second_provider/worker.rs`
(`a_refused_own_voice_is_warned_to_lamp_health`). The lamp
test (#8367): a real key and voice ID entered from the app by that lamp's
person, a few turns in that voice, a bad key told plainly, and a remove
giving the lamp's own voice back.

## An OpenAI-compatible model (#8362 step 1b, built, on in the product since #8367)

A person's own model at any OpenAI-compatible endpoint (Grok and the like),
on the same `keys.*` path and the same switch, with the contract agreed on
#8362 (the first builder at 11:44, Mobile's shape at 11:46) and #8367's
`kind`. The tests that pin it:
`setup/tests/it/keys.rs` (`an_openai_compatible_*`), `setup/src/keys.rs`
(`an_openai_compatible_check_finds_the_model_in_the_endpoints_list`),
`cloud/src/settings.rs` (`an_openai_compatible_key_goes_to_the_helper_*`),
`voice/src/second_config.rs`
(`the_persons_openai_compatible_model_is_used_only_with_the_switch`) and
`voice/src/second_provider/tests/chat_second_try.rs`
(`the_persons_endpoint_down_or_spent_falls_back_to_the_lamps_own`),
`voice/src/second_provider/net/chat.rs`
(`an_own_compatible_deepseek_model_is_asked_with_thinking_off_and_no_other`).

- **`offers`** in `keys.get`, #8367's shape: `{"service":
  "openai_compatible", "name": "OpenAI-compatible", "kind": "model",
  "models": [], "base_url": true, "fields": ["model"]}`, after DeepSeek's,
  Deepgram's and ElevenLabs'. `models` is empty because the endpoint's own
  list decides; `fields` tells the app to ask for the model's name.
  `lamp-cloud` passes `model` (text, at most 128 characters) to the helper,
  which alone takes it, and only for `openai_compatible`.
- **`keys.set`** `{"service": "openai_compatible", "key", "base_url",
  "model"}`: all three needed. The endpoint is https only, with no user,
  query or fragment (`fit_url`); the model is a slug (letters, digits, `-`,
  `.`, `_`, `/`, `:`). Each refused as `bad_field` in fixed words before any
  check, nothing of the request said back.
- **The check:** one `GET <base_url>/models` with the key as the bearer, as
  DeepSeek's. It works only when the 2xx's `data[].id` lists the model
  (read within the same 5 s and at most 1 MiB); a list without it is "that
  endpoint has no model by that name" (`bad_field`), and the list itself is
  never said back. A list that can't be read is `unreached`, "the lamp
  couldn't reach that endpoint to check that key; nothing changed".
- **Kept:** `openai_compatible.key`, `.url` and `.model`, 0600, as
  DeepSeek's are. **One own model at a time:** keeping one takes the other
  model service's own key away, so `in_use.model` is the one kept and the
  voice never guesses.
- **`keys.get`** adds `openai_compatible` (`own` | `lamp`),
  `openai_compatible_host`, `model` (`{"<service>": id | null}`), `checked`
  (`{"<service>": "works" | "refused" | "unreached" | null}`, the last
  check's code, kept only once the keys' folder is there, so a refused
  first key leaves nothing on the lamp) and `in_use` (`{"model": service | null}`). Never the key.
- **The voice** (`with_own_keys`, `own_openai_compatible.key`, `.url`,
  `.model`): the spoken chat goes to the endpoint with the person's key and
  model, over the relay it falls back to, in one journal line by host and
  model: `OpenAI-compatible: the person's own key, at <host>, model <id>
  (LAMP_OWN_KEYS)`, said only when its key is there. It is asked plainly,
  with none of DeepSeek's own fields (`thinking`, `stream_options`), as
  another service may refuse a field it does not know, unless it is
  DeepSeek's (the model named `deepseek...`, the host `api.deepseek.com` or
  the relay's DeepSeek address, `Chat::own_compatible`): that one is asked
  as DeepSeek's own chat, thinking off, as DeepSeek thinks by default and
  refuses a forced `tool_choice` while thinking (a 400, kit-8420 on 24
  Nov). The
  lamp's own chat is kept as `Hosted::lamps_own`: DeepSeek's web search, the
  memory's observation pass and the conversation's summary stay on it, as
  only DeepSeek's API and the relay serve them, and so does the relay a
  refused or failed request goes to. An endpoint that isn't https, or a
  model file missing or malformed, is passed over for the relay, said by its
  error's kind.
- **Mid-use failure:** a 401 or 403 falls back to the relay as today; so
  do a 429 (a usage limit too) and a 5xx before any answer (the endpoint
  down or spent), for DeepSeek's own key too, told once to `lamp-health`'s
  `voice-keys` part, "your own model couldn't answer, using the lamp's own",
  and one journal line, never a silent turn. Any other failure before the
  first word gives way too (the first builder's gate on #8393, before the
  switch-on): a 402 for no credit, a 404 for a model gone, any other 400 (a
  picture to a text-only model, an effort it refuses), a link that fails or
  a timeout. One with no HTTP status is told as 1 (`NO_HTTP_ANSWER`), never
  0. A held line's request (`chat_without`) gives way the same. Fifth's
  three P2s on #8543 sharpen it:
  - **The first word, not the head:** the answer is read to its first text
    or call. A 200 whose stream then carries only an error, or ends with no
    word, gives way too.
  - **A hung endpoint:** the person's own model has 4 s for its answer's
    head (`OWN_HEAD_WITHIN`, the same 4 s the person's own voice has for its
    first sound). An endpoint that takes the connection and never answers
    then gives way to the lamp's own model at the relay, told as no answer.
    Before, it held the link until its 30 s read timeout. That is not a
    rescue on a product turn: its wait is 4.5 s from the words (the 1.5 s
    cue and 3 s after it, `BEGIN_WITHIN_AFTER_CUE`), 3 s with no cue clip,
    so the relay has 0.5 s at most, and its first answer comes at about
    2 s (p50 1.99 s, p90 3.47 s). A hung endpoint mostly still ends the turn
    unanswered; what it gains is a link freed at 4 s, the warning told, and
    the next turn's own model tried afresh. A shorter head bound would hand
    healthy but slow own models to the relay, so it stays at 4 s; #8476's
    row times the hung case on the lamp. This is the Reviewer-lead's must-fix
    on #8543, before the switch-on, with or without the second try.
    A head that comes in time keeps the answer with the person's model, and
    the wait for its first word is the stream's own. Where the turn's second
    try is on (`LAMP_SECOND_CHAT_SECOND_TRY`, 2.5 s with no head, in no
    product unit), it asks the lamp's own model at the relay, not the
    person's again.
  - **The warning:** it clears when the person's own model answers again. A
    failure with no HTTP status is told only once the relay has answered, so
    a lamp off its own Wi-Fi never blames the person's model.
  - **A request with a picture or kept calls** that the person's model
    refuses with a 400 now goes to the lamp's own first. The lamp's own
    model takes no picture either, so its text-only retry follows: one more
    round trip on that turn.
- **Off:** nothing new is read or said; today's behaviour as it is.

## Named model providers: OpenRouter, Gemini and Anthropic (#8420, built, on in the product since #8367)

A person's own model by its provider's name, on the same `keys.*` path and
switch as the OpenAI-compatible one, with the plan the first builder agreed
on #8362 (22:02, 23 Nov). OpenRouter is the first, Gemini the second and
Anthropic the third (below). One rule for all three: **the endpoint is
fixed** in the lamp, never taken from the app, so the person's key only ever goes to its
own provider (#2094's rule). The tests that pin it:
`setup/tests/it/keys.rs`
(`an_openrouter_model_is_kept_with_its_model_at_openrouters_own_endpoint`),
`setup/src/keys.rs`
(`an_openrouter_check_asks_for_the_key_then_the_models_endpoints`),
`cloud/src/settings.rs` (`an_openrouter_key_goes_to_the_helper_with_its_model`),
`voice/src/second_config.rs`
(`the_persons_openrouter_model_is_used_only_with_the_switch`) and
`voice/src/second_provider/services.rs`
(`the_persons_openrouter_model_is_asked_as_openrouters_chat_without_search`).

- **`offers`:** `{"service": "openrouter", "name": "OpenRouter", "kind":
  "model", "models": [], "base_url": false, "fields": ["model"],
  "endpoint": "https://openrouter.ai/api/v1"}`, after the OpenAI-compatible
  one. `endpoint` is new and only on a named provider: the app shows it
  filled in and not editable. `base_url` stays `false`, so the app's bool
  still holds.
- **`keys.set`** `{"service": "openrouter", "key", "model"}`: the model is
  needed ("an OpenRouter key needs its model") and is a slug as above
  (`openai/gpt-4o-mini`). Any `base_url` sent with it is refused before any
  check, "OpenRouter takes no endpoint" (`bad_field`), as Deepgram's is.
- **The check** (the table above), both requests within the one 5 s bound,
  no redirect followed. OpenRouter's model list is public and about 1 MiB,
  so it proves nothing about a key: `GET /key` with the bearer tries the
  key, 401 or 403 is "that key was refused" and nothing more is asked. Then
  `GET /models/<model>/endpoints`, a public read sent without the key: 2xx
  works, 404 is "OpenRouter has no model by that name" (`bad_field`),
  anything else "the lamp couldn't reach OpenRouter to check that key;
  nothing changed". Seen live on 24 Nov: a made-up key's `/key` is 401, a
  made-up model's endpoints 404.
- **Kept:** `openrouter.key`, `openrouter.model` and `openrouter.checked`,
  0600, with no endpoint file. It is one of the model services
  (`Service::MODELS`): keeping it takes DeepSeek's own and the
  OpenAI-compatible key away, with their endpoint and model files, and
  keeping either of those takes it away. `keys.get` adds `openrouter`
  (`own` or `lamp`), `since.openrouter`, `model.openrouter`,
  `checked.openrouter`, and `in_use.model` is `"openrouter"` while it is
  kept. Never the key.
- **The voice** (`with_own_keys`, `own_openrouter.key` and `.model`, read
  only with the switch on): the spoken chat goes to OpenRouter's own API
  with the person's key and model, asked as OpenRouter's chat
  (`Chat::openrouter_at`: the fastest provider first and the route's
  `openrouter_effort`), never with its web search, which stays the lamp's
  own. `Hosted::own_chat` says which chat the person's model is asked as.
  One journal line by name and model: `OpenRouter: the person's own key,
  model <id> (LAMP_OWN_KEYS)`; the start's line says `the person's own <id>
  with the person's own key, at openrouter.ai (LAMP_OWN_KEYS), through
  OpenRouter`. An OpenAI-compatible key, if one were somehow there too, is
  used over it. A model file missing or malformed is passed over for the
  relay, said by its error's kind. The search, the memory's passes and the
  fallback stay on the lamp's own, and any failure before the first word
  falls back to it as the OpenAI-compatible model's does.
- **Off:** nothing new is read or said.
- **Not yet run on a lamp:** a real key and model from the app, a few
  turns, a bad key and a made-up model told plainly, and a remove.

### Gemini (#8420, built, on in the product since #8367)

The second named provider, by the same rule: Google's OpenAI-compatible
endpoint, fixed. The tests that pin it: `setup/tests/it/keys.rs`
(`a_gemini_model_is_kept_with_its_model_at_geminis_own_endpoint`),
`setup/src/keys.rs`
(`a_gemini_check_reads_the_one_model_and_refuses_a_bad_key_by_its_400`),
`cloud/src/settings.rs` (`a_gemini_key_goes_to_the_helper_with_its_model`),
`voice/src/second_config.rs`
(`the_persons_gemini_model_is_used_only_with_the_switch`) and
`voice/src/second_provider/services.rs`
(`the_persons_gemini_model_is_asked_as_geminis_chat`,
`the_persons_gemini_key_refused_by_its_400_falls_back_and_a_plain_400_too`).

- **`offers`:** `{"service": "gemini", "name": "Gemini", "kind": "model",
  "models": [], "base_url": false, "fields": ["model"], "endpoint":
  "https://generativelanguage.googleapis.com/v1beta/openai"}`, after
  OpenRouter's.
- **`keys.set`** `{"service": "gemini", "key", "model"}`: the model is needed
  ("a Gemini key needs its model"), a name as above (`gemini-2.5-flash`).
  Any `base_url` is refused before any check, "Gemini takes no endpoint"
  (`bad_field`).
- **The check** (the table above), one request within 5 s, no redirect
  followed. Gemini answers a bad key with **400, not 401**: seen live on
  24 Nov with a made-up key, the OpenAI-compatible endpoint says `400`
  `{"error": {"message": "Please pass a valid API key", "status":
  "INVALID_ARGUMENT"}}`, with no `API_KEY_INVALID` (the native API's word,
  which is in its `details[].reason`). A 400 whose error message says "valid
  API key", or whose details name `API_KEY_INVALID`, is "that key was
  refused"; so are 401 and 403. Any other 400 is not the key's and is
  "couldn't reach Gemini". 2xx works when the answer's `id` is the model, as
  `models/<id>` or `<id>`; 404 is "Gemini has no model by that name"
  (`bad_field`). Seen live too: a made-up model asked with no key is 404.
  A real key with a made-up model isn't seen yet; the row reads it.
- **Kept:** `gemini.key`, `gemini.model` and `gemini.checked`, 0600, no
  endpoint file. One of the model services: keeping it takes DeepSeek's own,
  the OpenAI-compatible and OpenRouter's away, and keeping any of them takes
  it away. `keys.get` adds `gemini`, `since.gemini`, `model.gemini`,
  `checked.gemini`, and `in_use.model` is `"gemini"` while it is kept.
- **The voice** (`own_gemini.key` and `.model`, read only with the switch
  on): the spoken chat goes to Google's API as Gemini's chat
  (`Chat::gemini_own`, the lamp's `Chat::gemini` with its
  `reasoning_effort: minimal`). The journal's line is `Gemini: the person's
  own key, model <id> (LAMP_OWN_KEYS)`, and the start's ends `through
  Gemini`. An OpenAI-compatible or OpenRouter key, if one were somehow
  there too, is used over it.
- **The fallback:** as the other own models': any failure before the first
  word. Only the person's own Gemini chat reads a 400's body for the key
  (`net::Error::KeyInvalid`, told as 400).
- **`reasoning_effort: minimal`** is the lamp's floor for Gemini 3. A
  2.5-series model may answer 400 to it, which then falls back to the
  lamp's own, so that model would never answer: the row
  with the person's own 2.5 model reads it, and if it does, the field is
  dropped for models other than 3.
- **Not yet run on a lamp:** a real key and model from the app, as for
  OpenRouter.

### Anthropic (#8420, built, on in the product since #8367)

The third named provider, by the same rule: Anthropic's API, fixed. The
tests that pin it: `setup/tests/it/keys.rs`
(`an_anthropic_model_is_kept_with_its_model_at_anthropics_own_endpoint`),
`setup/src/keys.rs`
(`an_anthropic_check_reads_the_one_model_with_its_own_headers`),
`cloud/src/settings.rs` (`an_anthropic_key_goes_to_the_helper_with_its_model`),
`voice/src/second_config.rs`
(`the_persons_anthropic_model_is_used_only_with_the_switch`) and
`voice/src/second_provider/services.rs`
(`the_persons_anthropic_model_is_asked_with_its_max_tokens`,
`the_persons_anthropic_chat_warms_with_its_own_headers`,
`the_persons_anthropic_key_refused_falls_back_to_the_lamps_own`).

- **`offers`:** `{"service": "anthropic", "name": "Anthropic", "kind":
  "model", "models": [], "base_url": false, "fields": ["model"], "endpoint":
  "https://api.anthropic.com/v1"}`, after Gemini's.
- **`keys.set`** `{"service": "anthropic", "key", "model"}`: the model is
  needed ("an Anthropic key needs its model"), a name as above
  (`claude-sonnet-4-5`). Any `base_url` is refused before any check,
  "Anthropic takes no endpoint" (`bad_field`).
- **The check** (the table above), one request within 5 s, no redirect
  followed: the one model read with Anthropic's own headers, never a bearer.
  Seen live on 24 Nov with a made-up key: `401` `{"type": "error", "error":
  {"type": "authentication_error", "message": "invalid x-api-key"}}`; with
  a bearer instead, the same 401. Any 2xx works: Anthropic answers an alias
  (`claude-sonnet-4-5`) with its dated id, so the id isn't matched. 404 is
  "Anthropic has no model by that name" (`bad_field`); 401 and 403 are
  "that key was refused"; anything else is "couldn't reach Anthropic". A
  real key with a made-up model isn't seen yet; the row reads it.
- **Kept:** `anthropic.key`, `anthropic.model` and `anthropic.checked`,
  0600, no endpoint file. One of the model services: keeping it takes
  DeepSeek's own, the OpenAI-compatible, OpenRouter's and Gemini's away,
  and keeping any of them takes it away. `keys.get` adds `anthropic`,
  `since.anthropic`, `model.anthropic`, `checked.anthropic`, and
  `in_use.model` is `"anthropic"` while it is kept.
- **The voice** (`own_anthropic.key` and `.model`, read only with the
  switch on): the spoken chat goes to Anthropic's OpenAI-compatible
  endpoint, `https://api.anthropic.com/v1/chat/completions`, with the key
  as a bearer (a made-up one is answered there `401` `{"error": {"code":
  "authentication_error", "message": "Invalid Anthropic API Key"}}`, seen
  live on 24 Nov), as `Chat::anthropic`: plain OpenAI chat with
  `max_tokens: 4096`, which every Claude model takes. Its warm asks
  `GET /v1/models` with `x-api-key` and `anthropic-version`, never a
  bearer, and a warm's status is only the journal's: it never stands for
  a refused key. The journal's line is `Anthropic: the person's own key,
  model <id> (LAMP_OWN_KEYS)`, and the start's ends `through Anthropic`.
  Any other own model's key, if one were somehow there too, is used over
  it.
- **The fallback:** as the other own models': any failure before the first
  word (529, overloaded, and a 400 for a key with no credit among them)
  goes to the lamp's own model through the relay with one line.
- **Limits of the OpenAI-compatible endpoint** (Anthropic's docs call it a
  way to test, not for production): no prompt caching, so every turn pays
  for its whole prompt; `system` messages anywhere in the conversation are
  hoisted to the top and joined; an answer longer than 4096 tokens is cut.
  Streaming, tools, `tool_choice` and images as data URLs work. The native
  Messages API is for later, for subscriptions or if a row shows bad tool
  calls.
- **Not yet run on a lamp:** a real key and model from the app, as for
  OpenRouter.

## The lamp's agent (#8735, built)

With `LAMP_OWN_KEYS=on`, the lamp's agent (OpenClaw or Hermes) chats with
the person's own model too, through lamp-cloud's forwarder
(docs/cloud.md, "The person's own model"):

- **The same model as the voice:** the first kept model key in
  `lamp_msgs::own_model::PRECEDENCE`, the one order the keys helper, the
  voice and the forwarder all read: OpenAI-compatible, OpenRouter, Gemini,
  Anthropic, then DeepSeek. Each named service's endpoint is kept there
  once, too.
- **Only with tools:** only when that key's check at `keys.set` saw the
  model make a tool call, the second line of its `.checked` record `tools`
  (#8735's setup part). Kept before that check, or no tool call: the agent
  stays on the relay. A Deepgram or ElevenLabs key alone leaves the agent
  on the relay.
- **Not the relay's day:** these chats are not counted in the relay's day
  used, which bounds skills' runs (the lead on #8735).
- **A refused key:** 401 or 403 from the person's endpoint before anything
  is passed back sends that chat to the relay, said once in the journal.
- **How lamp-cloud gets the key:** by credential, as it gets the relay's
  key. Its unit hands in only each model service's `.key`, `.url`,
  `.model` and `.checked` from this folder, each as `own_<file>` with a
  blank fallback, so lamp-cloud starts on a lamp with none. The folder
  stays root's, 0700.
- **When a change takes effect:** lamp-cloud reads them at its start. After
  a person changes their model key, the helper restarts lamp-cloud (#8741),
  so **the cloud link drops for a few seconds on purpose**, and the agent
  uses the new route from then. An agent chat in flight is drained first,
  for up to 10 s; one still running then is cut, said in the journal.
- **An agent task across that restart:** a task is many chat rounds. The
  round in flight at the stop ends within the drain, or is cut, said. A
  round the agent sends in the few seconds before lamp-cloud listens again
  is refused by the system (the port is closed), and lamp-cloud can't count
  it. Then the task depends on the runtime. The settings lampOS writes for
  OpenClaw and Hermes (`crates/tasks/src/openclaw_config.rs`,
  `hermes_config.rs`) set no model retry. Whether OpenClaw or Hermes
  retries a refused model request is their own code's, not verified here.
  If it doesn't retry, the task fails, and lamp-tasks ends the run as
  failed with the runtime's reason (`direct.rs`, `after_wait`), which the
  person hears. At the stop, the journal says `the Hermes forwarder:
  stopped, N chats in flight ended, M cut; a chat sent before lamp-cloud is
  back is refused`.
- **Who can read the key:** lamp-cloud's credentials are readable by its
  user, `lampos`, as the relay's key is. The agent runtimes run as
  `lampos` too, so they can read the person's model key, as they can read
  the relay's key today.

## The agent's tool check (#8735, PR2: setup side)

The lamp's agent (OpenClaw, Hermes) breaks on a model that makes no tool
calls, so a model key is checked for them once, at `keys.set` and at setup
(the lead's answer to Q3 on #8735). lamp-cloud's forwarder (#8742) gets the
person's model key by `LoadCredential=`, which systemd copies only at its
start; the folder stays root's alone, 0700, each file 0600.

- **When:** after a model service's key check works (DeepSeek,
  OpenAI-compatible, OpenRouter, Gemini, Anthropic; never Deepgram or
  ElevenLabs), one more request within 6 s of its own (`TOOLS_WITHIN`, so
  both stay under `lamp-cloud`'s 15 s): `POST <base>/chat/completions` at
  the endpoint and model just checked, the key as the bearer, as the voice
  chats. Body: one user line, one tool `ping` with no arguments,
  `tool_choice: "required"`, `max_tokens: 256`, `stream: false`; DeepSeek's
  with `thinking: {"type": "disabled"}` (a forced `tool_choice` is refused
  while it thinks), Gemini's with `reasoning_effort: "minimal"`.
- **Works:** a 2xx whose `choices[0].message.tool_calls` is a non-empty
  list. Anything else (no tool call, an error status, a body that isn't
  JSON, no answer in 6 s) is `works_no_tools`.
- **`works_no_tools`:** the key is kept and the voice uses it as with
  `works`; the reply is `success` `{"service", "uses": "own", "code":
  "works_no_tools", "answer"}`, the answer "that key works for the lamp's
  voice: <where the data goes>. It made no tool call, so the lamp's agent
  stays on the lamp's own model".
- **Kept:** in the key's `<service>.checked`, a second line beside its
  code: `works` then `tools` or `no_tools`. `checked.<service>` still says
  the first line alone. A key kept before #8735 has no second line.
- **`keys.get`:** `in_use` is `{"model": service | null, "agent": "own" |
  "relay", "agent_why": code | null}`. `own` only when the in-use model key's
  tool check passed; otherwise `relay`, `agent_why` one of `no_model_key`,
  `tts_only` (an ElevenLabs key and no model key), `no_tool_calls`, `refused`
  (the last `keys.set` for that service was refused) or `unchecked` (kept
  before the tool check: set it again). Each `offers[]` entry adds `agent`:
  `false` for Deepgram and ElevenLabs and for a kept model key whose tool
  check failed, `true` otherwise. Never a key.
- **lamp-cloud restarted:** after a person sets or removes a model key, the
  helper restarts lamp-voice and lamp-cloud, so the cloud link drops for a
  few seconds on purpose; an ElevenLabs or Deepgram change restarts only the
  voice. #8742's forwarder finishes an agent turn in flight before it stops.
  In detail: after a `keys.set` that kept a model key, or a
  `keys.remove` of one, the helper restarts lamp-cloud
  (`systemctl try-restart lamp-cloud.service`) right after the voice, at the
  same quiet moment and under the same update lock (`keys::restart_after`),
  so the forwarder loads the change. That is after the voice has rested 1 s
  (`change::KEY_CHANGE_QUIET`, #8710), the reply already written and
  flushed to lamp-cloud before the wait starts. Its journal line: "lamp-cloud
  restarted to load the person's model key for the agent (its
  LoadCredential= is copied only at its start, #8742)". Never for an
  ElevenLabs or Deepgram key. Restarting lamp-cloud drops the app's link
  until it reconnects. Tested in `tests/update_lock.rs`
  (`a_model_key_change_restarts_the_voice_then_lamp_cloud_once_quiet`).
- **`lamp-cloud`** passes the helper's `done` through as it is
  (`settings::ask_keys`), under its 4096-byte read of a reply.
- **Tests:** `src/keys.rs` (`a_model_check_asks_once_for_a_tool_call`) and
  `tests/it/keys.rs`
  (`the_agent_uses_an_own_model_key_only_once_it_made_a_tool_call`). Not yet
  run against a real service.

## Updates

Read one version ahead (#1680): `serve.json`'s `own_keys` field (the
folder, default `/etc/lampOS/voice/own`) is dropped and named by an older
setup. A build rolled back to before #2094 has no reader for `LAMP_OWN_KEYS`
or the `own_*` credentials and uses the relay; the drop-in's
`LoadCredential=` is systemd's alone and changes nothing it reads. An older
`lamp-selftest`, though, says a credential's folder "is not a file".

## What the app's repo must do (not built here)

- Send the three setup fields when the person gives them, and show
  `own_keys`' answers from the setup status.
- A keys screen with `keys.set`, `keys.remove` and `keys.get`, showing the
  `starting` ack at once and the answer after, word for word, including
  where the person's data goes.
- Never store or log a key; the backend passes it through to the lamp and
  never stores or logs it.

## Tests

`cargo test -p lamp-setup` (`tests/it/keys.rs`, `tests/it/serve.rs`,
`tests/it/reset.rs`, `src/keys.rs`), `-p lamp-voice` (`second_config`,
`second_provider::worker`), `-p lamp-cloud` (`settings`, `online`) and
`-p lamp-selftest`: a key absent, present and removed (relay, own, relay);
a refused or unreachable key not kept; a key that worked but couldn't be
written said as `not_kept`, the old one gone and the voice restarted;
`voice-keys` still reporting after a key is removed; the check one request, the key in
its header alone, no redirect followed; only `lamp-cloud`'s cgroup, at the system's place for it, asks; a
planted `sk-…` key in no reply, error, status or journal line; the reset
wiping the folder; a later 401 answered through the relay with the warning;
the switch off changing nothing. Not yet run on a lamp: systemd 252's
folder credential names (`own_<file>`), the cgroup check under the real
socket, and the checks against DeepSeek's and Deepgram's own services.
