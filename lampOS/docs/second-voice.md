# The second voice route

`crates/voice/src/second_config.rs`, `crates/voice/src/second_provider.rs`
and `second_provider/{net,services,worker}.rs`; the switch in
`crates/voice/src/coordinator/second.rs`. Issue
[#754](https://github.com/autonomous-ai/lampOS/issues/754). Status on
2026-10-15: host tests, the switch included, and four played runs on
lamp-52e6 with Gemini made unreachable (the last kept in
[verification](verification/754-lamp-52e6-20261015/README.md)): the
second route answered eight of eight questions there. A switch mid-line and
Gemini's return have run on the host only.

## Why

Step 11 of [brain-build.md](brain-build.md) is the second speech route:
speech to text, a text answer and speech synthesis, for when Gemini Live
cannot answer. Without it a refused quota or a Google outage leaves the
lamp with the "voice is down" state and nothing to say.

The route is a provider worker of its own, `lamp-voice worker second` (role
`second`). It speaks the same `ProviderInput` / `ProviderOutput` wire as the
Gemini worker and the fixture, so the coordinator treats it as one more
provider. Off unless configured: with no second-route file, nothing about
the voice service changes. With `LAMP_VOICE_ROUTE=second` it is the lamp's
only voice instead, and Gemini Live is not used ("Second only", below).

## The configuration

A private JSON file, named by `LAMP_VOICE_SECOND_CONFIG` in the voice
service's environment (`SECOND_CONFIG_ENV`). Unset or empty: the route is
off. The coordinator passes the path to the worker as
`worker second DIR PARENT WORKER PROVIDER_CONFIG SECOND_CONFIG [flags]`,
where the flags are the Gemini provider worker's, less `--keep-link`,
after this route's own `--wait-cue`, `--down-line`, `--only-voice`, one
of `--gemini-chat`, `--openrouter-chat` and `--deepseek-chat`,
`--gemini-speech`, and one of `--deepseek-search`, `--openrouter-search` and
`--exa-search` (each only when switched on, in that order; `RouteFlags` in
`second_provider/worker.rs`).

The field names are V1's (`system/domain/device.go`, `hal/models.py`), so
whatever carries V1's settings over can copy them. The one change: each
`*_api_key` becomes a `*_credential_file`. A key written in the JSON is
refused (unknown fields are refused).

| Field | Required | What |
|---|---|---|
| `llm_base_url` | yes | OpenAI-compatible base URL of the text model |
| `llm_model` | yes | model name, not empty |
| `llm_credential_file` | yes | file holding the text model's key |
| `stt_base_url` | no | the company's speech-to-text proxy; empty or absent: `llm_base_url` |
| `stt_credential_file` | no | its key; empty or absent: `llm_credential_file` |
| `stt_language` | no | sent as `language`; absent: `en` |
| `deepgram_credential_file` | no | when set, speech to text goes straight to Deepgram with this key, and the two `stt_*` above are not used |
| `tts_base_url` | no | empty or absent: `llm_base_url` |
| `tts_credential_file` | no | empty or absent: `llm_credential_file` |
| `tts_provider` | no | `openai` (the default) or `elevenlabs`; anything else is refused |
| `tts_voice` | no | the voice; absent: OpenAI `nova`, ElevenLabs `Rachel` |
| `tts_model` | no | the voice's model (letters, digits, `-` `.` `_`); absent: OpenAI `tts-1`, ElevenLabs `eleven_flash_v2_5`. `LAMP_SECOND_SPEECH_MODEL` goes over it (the owner on 19 Oct: "use v4 turbo", `eleven_v4_turbo`) |
| `llm_extra` | no | fields the chat request also carries, for an OpenAI-compatible endpoint that needs its own (DeepSeek's `{"thinking":{"type":"disabled"}}`, a model's `reasoning_effort`); at most 16, never `model`, `messages`, `stream`, `tools` or `tool_choice` |
| `gemini_base_url` | no | Google's Gemini API for `LAMP_SECOND_CHAT=gemini` and `LAMP_SECOND_SPEECH=gemini`; absent: `https://generativelanguage.googleapis.com/v1beta` |
| `gemini_credential_file` | — | no longer read, and refused by name if given: Gemini's key is the voice unit's credential `gemini.key` (below) |
| `gemini_chat_model` | no | absent: `gemini-3.6-flash` |
| `gemini_speech_model` | no | absent: `gemini-3.8-flash-lite-tts` |
| `search_url`, `search_credential_file` | no, together | the search service `LAMP_SECOND_SEARCH=exa` calls (`web_search`) |
| `search_extra` | no | fields of the search request of its own (Exa's `type`, say) |
| `openrouter_model` | no | the slug `LAMP_SECOND_CHAT=openrouter` asks, from OpenRouter's public model list; absent: `deepseek/deepseek-v4.1-flash-1017` |
| `openrouter_reasoning_effort` | no | `none`, `minimal`, `low`, `medium` or `high`; absent: `minimal` for a `google/` model, `none` for any other |
| `deepseek_model` | no | the model `LAMP_SECOND_CHAT=deepseek` asks of DeepSeek's own API; absent: `deepseek-flash` (DeepSeek-V4.1-Flash-1017). DeepSeek's peak hours, 01:00–04:00 and 06:00–10:00 UTC on weekdays, cost double; no switch for them |
| `deepseek_base_url` | no | the relay `LAMP_SECOND_CHAT=deepseek` and `LAMP_SECOND_SEARCH=deepseek` go through instead of DeepSeek's own API (#1934): `<base>/chat/completions`, `<base>/v1/tools/search` and `/v1/tools/fetch`, with the relay's credential (`llm_credential_file`) as the bearer, and no `deepseek.key`; absent: DeepSeek's own API with `deepseek.key`. `LAMP_SECOND_DEEPSEEK_BASE_URL` in the unit's environment goes over it |

OpenRouter's and DeepSeek's keys are not in this file: they are the voice
unit's systemd credentials, `openrouter.key` and `deepseek.key` in its
`CREDENTIALS_DIRECTORY` ("OpenRouter and DeepSeek", below). With
`deepseek_base_url`, DeepSeek's chat and search take the relay's credential
instead, so a lamp with only its relay key runs them (#1934, the lead's
task: lamps carry no provider keys). The journal then says DeepSeek is asked
"through the relay with its credential", with no URL or key, and
`probe-cascade` times `chat=deepseek-relay(…)` beside the direct one. The
relay's base is `https://campaign-api.autonomous.ai/api/v1/ai/v1/deepseek`,
with DeepSeek's own paths under it and the relay key as the bearer. It
passes the bodies and streams through unchanged, and answers `GET
<base>/models` for `LAMP_SECOND_CHAT_WARM`. A missing key is 401 and past
the lamp's limit 429. Search and fetch are 5 calls a second for the whole
relay (the backend on #1934, 21:0x).

The fallback is V1's: an empty or absent STT or TTS URL or key is the LLM's.
A Gemini model name is letters, digits, `-`, `.` and `_` only: it goes into
the request's URL.

Two of V1's values do not carry over as they are. Read from lamp-52e6's V1
`config.json` on 2026-10-15 (endpoints only):

- `llm_model` is `Auto-AI`, which the company relay serves only on its
  Anthropic-messages route; its `/chat/completions` answers 404 for it. Set
  an OpenAI chat model there instead: `gpt-4.1-mini` answered through the
  relay (first byte 0.32 s from the lamp, one probe).
- `tts_provider` is `gemini`, which this route does not take (it would fail
  with Gemini). Set `elevenlabs`. The relay's OpenAI `/audio/speech` answers
  only once the whole sentence is made: "Paris!" took 2.4 to 5.5 s to its
  first audio, a 4 s sentence 5.0 s (`tts-1`; `gpt-4o-mini-tts` 2.2 s). Its
  ElevenLabs path streams: 0.60 to 0.68 s to the first audio for the same
  sentences (`eleven_flash_v2_5`; probes from lamp-52e6 on 2026-10-15).

Rules the loader keeps (`load_from_path`):

- The JSON file and every credential file are bounded regular files owned
  by the service's user, mode 0600: a symlink or group or world access is
  refused, as for `provider.json` ([live-runtime.md](live-runtime.md),
  "Private configuration and commands"). The JSON is at most 16,384 bytes.
- A credential path must be absolute and the file not empty. Its trailing
  line ending is dropped. A key is never shown by `Debug`, and no error
  carries a key, a URL or a server's words.
- Every URL must be `https://` with a host, at most 2,048 bytes, with no
  `?`, `#` or `@`. HTTP redirects are not followed.

An example, with the company's endpoints:

```json
{
  "llm_base_url": "https://campaign-api.autonomous.ai/api/v1/ai",
  "llm_model": "<model>",
  "llm_credential_file": "/etc/lampOS/voice/second.key"
}
```

The first builder writes lamp-4ace's file from the company's endpoints.
Who writes it at onboarding is not built (below).

## What each client speaks

As V1's HAL speaks them (`hal/drivers/voice/stt/*`, `hal/drivers/voice/tts/*`).
Every connect, TLS included, is bounded at 10 s; a streamed response that
says nothing more for 30 s is a timeout. TLS is rustls with the `ring`
provider and the webpki roots, as `lamp-gemini` has it.

A base URL on `campaign-api.autonomous.ai` ending in `/ai` is given `/v1`
(V1's `_ensure_openai_v1`); any other is used as it is, less a trailing `/`.

- **Speech to text** (`net/stt.rs`): Deepgram's websocket protocol, with
  `Authorization: Token <key>`. Straight to `wss://api.deepgram.com/v1/listen`
  when `deepgram_credential_file` is set, otherwise to the proxy at
  `<base>/ws/audio/transcriptions` (the base made `wss://`). Query, V1's for
  nova models: `model=nova-3-general`, `encoding=linear16`,
  `sample_rate=16000`, `channels=1`, `language`, `smart_format=true`,
  `interim_results=true`, `endpointing=300` (V1's was 1500; see Latency
  targets), `vad_events=true`, and
  `keyterm=Lamp`, as V1 boosts the lamp's name: without it, on lamp-52e6,
  "Hey Lamp" came back as "Palin", "Palan" and "Len", and the model took
  the question for other people's talk. After it, one `keyterm` each for
  `LAMP_SECOND_KEY_TERMS` in the voice service's environment,
  comma-separated (at most 20, each at most 40 bytes of letters in any
  script, digits, spaces, `-`, `.` and `'`; refused at the worker's start
  otherwise, naming the variable), for the places people here ask about:
  on lamp-4ace on 19 Oct "Hanoi" was heard as "Hannover" twice and the
  weather came back for Hannover. It reaches the second route's worker
  alone, whose journal says `lamp-voice second: speech to text expects N
  words beside the name (LAMP_SECOND_KEY_TERMS)`; the stop words' stream
  keeps the name alone. **With `LAMP_SECOND_KNOWN_NAMES=on`** (#753), the
  names the lamp knows follow the unit's words, each its own `keyterm`:
  - the name typed at setup (`LAMP_PERSON_NAME`);
  - the agents it hands work to, "Claude Code", "Codex" and "OpenCode",
    when it has a tasks part (on lamp-4ace on 19 Oct "ask Codex" came back
    "Next to write").

  Never the known faces' names. The list of everyone the lamp remembers
  would leave the house with every line, including people who asked only
  to be recognised and haven't spoken (the reviewer on #1818). A name said
  aloud reaches Deepgram in the audio anyway, but the list is the owner's
  call (the first builder, 19 Oct 15:42). If the owner says yes, it comes
  back behind a switch of its own. `docs/eyes.md`'s rule stands: no print
  and no remembered name leaves the lamp.

  Each is a plain name, once, never "Lamp" again, at most 49 after it
  (Deepgram takes 100 and 500 tokens, and advises the 20 to 50 that matter
  most). They are read on a thread of the worker's own every minute
  (`second_provider::net::key_terms`), so a line never waits on a file.
  The journal says how many when the count changes (`lamp-voice second:
  speech to text listens for N names the lamp knows`), never the names.
  The lamp's zone is not one of them: `Asia/Ho_Chi_Minh` is Hanoi's too,
  so a place goes in `LAMP_SECOND_KEY_TERMS`. A keep-alive
  every 5 s. Only final segments are kept. At the end of the line, when
  the words are already settled (final segments cover all the audio sent,
  and no interim words or `SpeechStarted` came after them), they are the
  transcript at once (`Transcript::settled`). An earlier `speech_final`
  does not settle them: it marks a pause, and audio sent after it may
  still have words on their way ("set a timer" without "for twenty
  minutes", #878). Otherwise `{"type":"Finalize"}` asks for
  the final words at once, keeping the socket: final segments are taken
  until they cover all the audio sent, for up to 1 s (`FINALIZE`). An
  answer marked `from_finalize` that falls short of the audio sent does
  not end the line: `CloseStream` goes at once and its words are waited
  for. On lamp-52e6 (2026-10-16, #828) audio sent faster than speech, a
  held burst, got such an answer: "Hang" for "Hang on." (0.30 of 0.59 s)
  and "Lamp," for "Lamp, stop."; with `CloseStream` after it, 16 of 16
  rounds had the whole phrase, its segment starting where the answer
  ended, about 0.25 s later than the answer. **Unless what the answer
  leaves is the line's quiet end** (#1712, the first builder at 23:46):
  on lamp-4ace on 19 Oct, `Finalize` answered in about 280 ms on 131
  lines, but on 114 its answer reached about 0.48 s short of the audio
  sent, so the words waited for `CloseStream`: 652 ms after the line's
  end at the median, 1.7 s at p90, against 280 ms when it covered. The
  stream knows where the line's sound ends (`LineSound`): the end of its
  last block within 30 dB of the line's loudest block so far and over
  about -60 dBFS. The bar is the line's own, so a far voice's soft last
  word counts as sound (Jarvis on #1993); after the canceller, talk near
  the lamp is at -21 to -31 dBFS and the quiet between sounds near
  -75 dBFS (the first builder on #1712, 15:42). When the finals cover
  that end to within one block (`COVERED`, 0.05 s), with nothing pending,
  the answer's words are the line's at once. The journal adds
  `, the rest quiet: sound to S ms` to `words from=finalize`. Any sound
  past the answer is a word left and still waits for `CloseStream`:
  "Hang on."'s "on", a word loud for 0.10 s after a 40 ms gap, or one at
  -48 dBFS. A level that stays up after the words, a noisy room, keeps
  the line waiting as before. Only a request's line ends this way
  (`SttStream::finish`); a stop word's stream (`finished`) keeps
  `CloseStream`, since over the lamp the canceller can take a stop word
  under any bar (#828). If 1 s passes with no covering
  finals, `CloseStream` and up to 15 s in all for its final words, as
  before. `CloseStream` is sent in
  every case to close the socket, but waited on only in that fallback.

  **Flux on trial (`LAMP_SECOND_FLUX=on`, off unless set; the hearing half
  of #1712's plan, `net/stt/flux.rs`).** The same lines go to Deepgram's
  Flux, `wss://api.deepgram.com/v2/listen?model=flux-general-en&encoding=linear16&sample_rate=16000`,
  with the same `keyterm`s: the name, the unit's words and the names the
  lamp knows. Through the Autonomous proxy it's the proxy's own path,
  `…/ws/audio/transcriptions`, with Flux's query. The proxy passes
  `model=flux-general-en` on to Deepgram's `/v2/listen`: on lamp-4ace on
  19 Oct, with the lamp's own key, its handshake, `Connected` and
  `TurnInfo` came back, and a clean line was heard whole (the first
  builder, 15:54). Its `/v2/listen` paths are 404s. Flux hears English
  only: with an `stt_language` other than `en` or `en-*`, Nova-3 hears the
  lines and the journal says why once. So a Vietnamese lamp
  (`stt_language` `vi`, the only Vietnamese code Nova-3 has) hears with
  Nova-3 whatever `LAMP_SECOND_FLUX` says, since Flux has no Vietnamese
  (`flux-general-en`, and `-multi`'s ten languages). Its key terms go with
  it: Deepgram's keyterm prompting is for Nova-3 monolingual and
  multilingual alike (Deepgram's docs, read 19 Oct). Whether English names
  among them help a Vietnamese line is not measured. Host test
  (`stt::tests::vietnamese_keeps_off_flux_and_reaches_the_stream_with_its_key_terms`,
  a local server reading the stream's query): `vi` asks `nova-3-general`
  with `language=vi` and the key terms, with Flux on or off; English or
  unset with Flux on asks Flux. `LAMP_TRANSCRIPTION_LANGUAGE` is the Live
  route's transcript hint only: this route's language is `stt_language`
  (Fourth and the first builder on #1722, 22:52 and 22:54).

  Flux tells turns itself. Each `TurnInfo` carries one turn's words so
  far, and `EndOfTurn` ends the turn: by Flux's own model (`trigger`
  `model`), by `ForceEndTurn` (`manual`), or after 5 s of quiet
  (`timeout`). The coordinator still ends the line. Audio goes in 80 ms
  messages, as Deepgram advises. Flux's reference names no `KeepAlive`,
  so an idle socket is kept with 80 ms of quiet.

  At the end of the line:
  - **Settled:** the latest turn has ended and Flux's windows cover all
    the audio sent. Its words are the transcript at once.
  - **Otherwise `ForceEndTurn`,** waited for up to 1 s as `Finalize` is.
    Its `EndOfTurn` ends the wait, and so does the warning that no turn is
    open (`FORCE_END_TURN_NO_ACTIVE_TURN`).
  - **Unanswered:** `CloseStream`, which ends no turn, and each turn's
    latest words are the transcript.

  The stop words' and wait words' streams stay on Nova-3, since the switch
  reaches only the second route's worker. A flush on a Flux stream is a
  `ForceEndTurn` too, and its answer is never taken for the line's.

  The journal's line is Nova-3's, with Flux's account in place of the
  finals': "request N at the line's end: flux: turns=K, <no turn|the
  latest turn open|the latest turn ended by model|manual|timeout>, <the
  model ended no turn|the model ended turn I T ms before End>,
  settled=<bool>, heard to H of S ms sent[, force_end answered in A ms (end
  of turn|no turn)|, force_end unanswered], words
  from=<settled|force_end|close_stream>". T is how far ahead of the
  coordinator's end Flux's own end of turn came. That, beside the words,
  is what the trial is for.

  `lamp-voice stt-check` reads the unit's switches now. So
  `LAMP_SECOND_FLUX=on lamp-voice stt-check CONFIG WAV` hears a recorded
  line with Flux, and without the switch with Nova-3: the same line by
  both.

  Host tests on the paused clock: 0 ms to the words with the turn ended
  over all the audio, 100 ms with `ForceEndTurn` answered 100 ms later, and
  1 s + 800 ms when it isn't answered. Not yet run against Deepgram or on a
  lamp.
- **Text model** (`net/chat.rs`): `POST <base>/chat/completions`,
  `Authorization: Bearer <key>`, `"stream": true`, with `tools` when any are
  declared. They are declared as they stand at each request, as a Live
  session is told them when it opens, so the executive's actions come and
  go with the executive (one that starts after the voice is not missed).
  The answer is read as server-sent events and cut into sentences as it
  arrives (`net/sentences.rs`). It is whole at `[DONE]`, or at the
  stream's end after a chunk with a `finish_reason`; a stream that ends
  before either was cut, and fails the turn (`TurnFailed`, as any chat
  failure) rather than leaving a half answer as the whole one. A turn's
  first round that has not begun (no word, no call) within 3 s of speech to
  text giving the words is given up and the turn ends unanswered, nothing
  said (`BEGIN_WITHIN`; journal: `chat round 1 had not begun in … ms`): on
  lamp-4ace on 2026-10-16 first events came 1.6 to 20.5 s after the words,
  and an answer 12 s late is worse than none. A line given up so is no
  talk the lamp left alone: it was said to the lamp, so the room holds it
  as it holds the rule's holds, and it ends no conversation. Only a line
  the model answers with nothing (`not_for_me`) counts toward the two that
  end one (#1712, the first builder at 16:04). A round after a tool ran is
  not bounded: what was done (a timer set, the light changed) has to be
  said, or it is asked for and done again.
  A look's first round may take 5 s (`LOOK_BEGIN_WITHIN`): a picture is
  slower to read, and on lamp-52e6 one of two real looks went past 3 s.
  Past it the lamp says "I couldn't get a good look." (`COULD_NOT_LOOK`;
  journal: `look round had not begun in … ms`) rather than nothing after
  "Let me look." (the first builder, 2026-10-16 21:25). A full look
  (`LAMP_LOOK_FULL=on`) whose still never came arrives as the no-look note
  without `look`, and is a look all the same: 5 s, then the line.
  With `LAMP_SECOND_WAIT_CUE=on` (sixth-P3), a spoken turn whose first
  round has not begun 1.5 s after the words makes one soft sound ("Hmm.",
  "Mm." or "Hm.", the next never the last one played), and is then given up
  only at 4.5 s: a "hmm" followed by silence is the worst of both. The
  sounds are said once by this route's own voice at its first spoken turn,
  kept in memory, cut to 0.6 s with a 30 ms fade, at half the
  reply's level. They go as audio only: nothing is kept in the thread, the 120 s
  announcement still comes before the first real answer, and a written
  turn never makes one. On lamp-4ace on 2026-10-16, 9 of 10 backup turns
  waited past 1.5 s.
  With `LAMP_SECOND_DOWN_LINE=on`, a spoken turn whose first round has not
  begun within its bound, or whose chat refuses it before any word (a
  status: 401, 402, 429, a 5xx; #1844's B, then the turn fails as before),
  says "I can't think right now; give me a minute."
  (`DOWN_LINE`) instead of nothing, at most once a minute
  (`DOWN_LINE_EVERY`; the journal: `said it can't think right now`). The
  line is said once by this route's own voice after the first turn is out,
  never beside a turn's own chat and speech, while the relay answers, and
  kept in memory (at most 4 s), so it can still be said when the chat no
  longer answers. A render that fails is journaled (`the down line could
  not be said`) and tried again after the next turn. A minute counts from
  when it was played, not from a try that failed. It goes as audio only,
  like the wait's sound. A written turn, the lamp's own (a hello, a skill's
  say, the news), never says it: said unasked it reads as a broken machine
  (the reviewer on #1195). A look keeps its own line. A speech that never rendered (the relay down since
  start) leaves the turn silent, as before. On 2026-10-17 from 02:29 the
  relay's chat timed out at 40 s, then gave 401 after 60 s, while its
  speech to text answered in under a second (the sixth's relay log).
- **Speech** (`net/tts.rs`), 24 kHz mono 16-bit PCM:
  - `openai`: `POST <base>/audio/speech`, `Authorization: Bearer <key>`,
    model `tts-1`, `response_format: pcm`.
  - `elevenlabs`: `POST <base>/text-to-speech/<voice id>/stream?output_format=pcm_24000`,
    `xi-api-key: <key>`, model `eleven_flash_v2_5` (V1 used
    `eleven_v4_turbo`, 0.81 to 1.30 s to its first audio through the
    relay), speed 1.0. A base that is
    not an ElevenLabs host (`api.elevenlabs.io`, `api.us.…`, `api.eu.…`)
    gets `/elevenlabs` first, as V1's proxy path. A V1 voice name
    (`VOICE_IDS_BY_LANG`) is turned into its id; any other value is taken as
    an id.

  Text in `[...]` is not spoken. A sentence with nothing left to say sends
  no request. Every sample is doubled (+6 dB) and clipped at full scale:
  through the relay `tts-1` `nova` measured peak -7.3 dBFS and RMS -28.8
  dBFS, ElevenLabs Rachel peak -6.4 and RMS -23.5, and the second voice was
  heard much quieter than Gemini's.
- **Gemini's text model** (`LAMP_SECOND_CHAT=gemini`, `Chat::gemini`):
  Google's OpenAI-compatible endpoint, `POST <gemini base>/openai/chat/completions`,
  `Authorization: Bearer <key>`, `reasoning_effort: "minimal"`, streamed,
  with the lamp's tools and a look's picture as the configured chat sends
  them. A call's `extra_content` (Gemini's thought signature) is given back
  with the call in the next round, as Gemini 3 asks; two calls at one index
  with different ids are two calls; a call without an id is given one.
- **Gemini's speech** (`LAMP_SECOND_SPEECH=gemini`, `net/gemini_tts.rs`):
  `POST <gemini base>/models/<model>:streamGenerateContent?alt=sse`,
  `x-goog-api-key: <key>`, `responseModalities: ["AUDIO"]` and the Live
  route's own voice (`provider.json`'s `voice`, Kore on the lamps). One
  request a sentence or first clause, as for the other voices: Google takes
  no streamed text. The audio comes as base64 parts of server-sent events,
  24 kHz 16-bit PCM (another rate is refused), and is passed on as it
  comes, at its own level: no +6 dB, as it is Google's voice like Live's
  (not measured on a lamp).
- **OpenRouter's text model** (`LAMP_SECOND_CHAT=openrouter`,
  `Chat::openrouter`): `POST https://openrouter.ai/api/v1/chat/completions`,
  `Authorization: Bearer <key>`, the model a slug of its public list
  (`openrouter_model`), `provider: {"sort": "latency"}` (its providers
  tried fastest first) and `reasoning: {"effort": …}` (OpenRouter's docs,
  read 19 Oct). The lamp's tools and a look's picture (an `image_url` part,
  a base64 `data:` URL) go as for the relay. A round's `reasoning_details`
  are given back unchanged with its calls in the next round, as OpenRouter
  asks (`ChatEvent::Reasoning`).
- **DeepSeek's text model** (`LAMP_SECOND_CHAT=deepseek`, `Chat::deepseek`):
  `POST https://api.deepseek.com/chat/completions`, `Authorization: Bearer
  <key>`, `thinking: {"type": "disabled"}`, set always and never left to
  DeepSeek's default (on). Pictures as `image_url` parts; a model that takes
  none (`deepseek-v4-pro`, DeepSeek's pricing page) is never sent one: a
  look asked of it is told it saw nothing (`Services::sees`).
- **Web search** (`LAMP_SECOND_SEARCH`, `net/search.rs`), declared to the
  model beside the lamp's tools, within Live's bound of 32 functions (the
  search before a skill's last function, the fetch only with room left;
  `worker::with_search`). A turn waits 3 s for a call (`WITHIN`), then goes
  on with "the search did not answer in time".
  - `deepseek`: DeepSeek's Search & Fetch API (api-docs.deepseek.com/api/search,
    read 19 Oct). `web_search(query)` is `POST <deepseek>/v1/tools/search`
    with `{"query", "count": 3}`; each result's title, its `summary` (written
    for models) cut at 500 characters and its date go back. `fetch_page(url)`
    is `POST <deepseek>/v1/tools/fetch` with `{"url"}`; the page's title and
    its Markdown cut at 2,000 characters go back. $0.75 a thousand
    successful calls of each from 1 Nov 2026; 5 a second an account.
  - `openrouter`: OpenRouter's web plugin on Exa, `plugins: [{"id": "web",
    "engine": "exa", "mode": "instant", "max_results": 3}]` on the chat
    request, with `LAMP_SECOND_CHAT=openrouter` only. It searches before
    every answer, which every answer then waits for; OpenRouter's docs
    suggest its `openrouter:web_search` server tool instead, which lets the
    model choose (not built).
  - `exa`: the search service `search_url` names, in Exa's `/search` shape
    (`x-api-key`, `query`, `numResults` 3, text cut at 500 characters,
    `search_extra` added). Ready for Exa, whose key the lamp does not have.

  None was run against its service.

A service's answer becomes the turn's failure reason (`fault`): 401 and 403
Authentication, 402 QuotaExceeded, 429 RateLimited, 5xx ServerUnavailable,
other statuses ServerRejected; a connect timeout ConnectTimeout, any other
timeout ResponseTimeout.

## A turn on the second route

`Started` at once, and speech to text opened while the person is still
talking. Up to 30 s of microphone audio waits while it connects, the
length of the coordinator's line buffer, so a replayed line fits. At the
end of the line: the person's words, then the chat answer, each sentence
said as it is complete while the next is still being written. Sentence
order is kept. A written turn is the same from the chat on.

- **Each stage after the words is journaled, without words:** `chat round
  1 first event in … ms`, then `first piece to the voice … ms after the
  words were heard, K words` (the first clause or sentence asked of the
  voice), then `first answer audio … ms after the words were heard`. The
  first piece's time less the first event's is the wait for the clause.
  The first audio's time less the first piece's is the voice's own. The
  coordinator's `--playout-start-ms` (350 on the units) comes after (#1712,
  the first builder at 17:27: the chat's first clause about 0.9 s and the
  voice's first audio about 0.3 s, but the first answer audio 1.4 to 3.9 s
  after the words).
  Each chat request is journaled too, without words: `chat request B
  bytes, M messages, T tools; its answer began in A ms; the last request S
  s before` (or `the first request of this client`). A is the response's
  headers. A gap over the client's 90 s pool keep means a new connection
  (the first builder at 18:26: the first event came in 0.9 to 1.9 s on
  lamp-4ace, 0.7 s from the probe).
  Each piece asked of the configured voice (OpenAI's or ElevenLabs) is
  journaled at its first audio byte, without words: `request R ElevenLabs
  piece K: C chars, sent S ms after the words were heard on a reused|new
  connection; its first audio byte A ms after it went`. C counts the text
  sent; A is from the request's going to the body's first byte. At the
  turn's end, one line for its pieces: `request R ElevenLabs: B of N pieces
  began, U on a reused connection, C chars; the first piece's first audio
  byte A ms after it went, the slowest M ms` (`never came` when the first
  never began). "Reused" is read, not guessed: the response's local socket
  (hyper's `HttpInfo`) is one an earlier response or warming came on (the
  last 8 kept). The client speaks HTTP/1.1 only, so pieces asked while one
  still downloads each open their own connection. Gemini's voice is not
  journaled this way (#1712, the first builder at 19:57: `eleven_v4_turbo`'s
  first sound came about 690 ms after its first piece went on lamp-4ace,
  request 2: 2185 to 2875 ms after the words; Flash 306 ms from the probe).
  After the first request, the line also says whether its tools and its
  first message are the last request's, hashed: `; its tools and first
  message as the last request's`, or which of the two is not. A service's
  prompt cache holds only a prefix that is the same as before.
  Each answer is journaled when it ends: `chat answer: its first words or
  call F ms and its end E ms after it was asked; P prompt tokens, H from the
  cache, M not; C completion tokens`. H is DeepSeek's
  `prompt_cache_hit_tokens`, or `prompt_tokens_details.cached_tokens` in
  OpenAI's form. DeepSeek is asked for the count
  (`stream_options.include_usage`), and other services' is told when they
  send it, else `no usage given` (#1712, the first builder at 18:56: the
  request was 31 KB with 31 tools, and its first event came in 976 to
  1296 ms).
- **With `LAMP_SECOND_STEADY_FIRST=on` a spoken turn's request begins with
  what stays** (off unless set; #1712, the first builder at 18:56). The
  system message keeps only the character and the skills. The memory note,
  the time, the room's facts and the thread go in a second system message
  after the exchanges, before the turn's notes and words (`Told::after`).
  So the request's prefix, the character, the tools and the exchanges, is
  the same from one turn to the next while the person goes on, and the time
  changes it only after them. Off, it's all in the one system message, the
  time right after the character. A text's request is unchanged. Each turn's
  journal says how much the cache held (above): the A/B is the played check
  both ways, by H and by the first event's time.
- **With `LAMP_SECOND_FEW_TOOLS=on` a spoken turn's chat is given its few
  functions first** (off unless set; #1712, the first builder at 18:56:
  "light, volume, timer, search, look, tasks"). The others are behind one,
  `more_functions`, whose description names them (`worker::few_tools`).
  - **Given at once** (`given_at_once`):
    - the light's and the volume's;
    - the timer's and the reminders';
    - the tasks';
    - the web search and its fetch;
    - the look;
    - the decline (`not_for_me`), which a busy room needs at every turn;
    - `use_skill`, which the skills' lines in the instruction name;
    - setup's walk (`setup_step`, `redo_setup`), which a setup talk calls at
      every turn;
    - the memory's `remember`, `recall` and `forget`, asked of a lamp all
      day. Behind `more_functions`, on lamp-4ace at 00:29 on 20 Oct,
      "remember my violet umbrella is in the car" and "forget that" were
      answered in words, nothing kept or forgotten, while `recall`, which
      the memory note names, was called (the first builder on #1722, 00:34).
  - **Behind `more_functions`:** `forget_everything`, the faces', the time elsewhere
    (`the_time`), a mark for review, the stop of reports, and any skill
    function given on its own.
  - **Once the model calls it in a turn**, the route answers it itself, with
    nothing run ("Every function is given to you now…"), and that turn's
    next round is given every function. A round costs about a second, only
    for those.
  - **The first request is smaller** and the same from turn to turn for a
    prompt cache. The journal's `chat request … T tools` line shows the
    count. A text's chat and the probe are given every function.
- **With `LAMP_SECOND_CHAT_WARM=on` the chat's connection is opened as a
  line begins** (off unless set), as Gemini's voice is, when no request went
  in the last 60 s (`chat::WARM_AFTER`). It's a GET of the service's
  `models`, read whole, so the line's chat goes on a connection already
  made rather than making one after the words. The journal says `the
  chat's connection opened ahead in T ms (HTTP S)`. The client's pool keeps
  an idle connection 90 s, so the lamp's first line after a quiet minute
  paid for the handshake (the first builder at 18:26: the chat's first
  event 0.9 to 1.9 s on lamp-4ace, 0.7 s from the probe). A warming that
  fails changes nothing; the chat connects as before.
- **With `LAMP_SECOND_SPEECH_WARM=on` the configured voice's connection is
  opened as a line begins** (off unless set, as the chat's switch is; its
  own switch, since `LAMP_SECOND_CHAT_WARM` names the chat), when no piece
  or warming went in the last 60 s (`tts::WARM_AFTER`, the chat's). Only
  when the configured voice is the lamp's one voice; Gemini's is warmed
  whenever it is. It's a GET of `<base>/models` with the key
  (ElevenLabs: `<base>/elevenlabs/models` on the proxy, `/v1/models`
  direct), read whole (at most 256 KiB), so the answer's first piece goes
  on a connection already made. No text is sent, so no characters are
  billed: ElevenLabs bills the text of speech requests, and `GET
  /v1/models` lists models. The Autonomous proxy has no route for it (404,
  `Cannot GET …`, read from the Mac on 19 Oct) and keeps the connection
  open after it, so the warming still opens it. The journal says
  `ElevenLabs: the speech connection opened ahead in T ms (HTTP S)`, and the
  first piece's line says `reused`. A warming runs beside the line and
  never holds a piece up: a piece asked while it is in flight opens its own
  connection, and one that fails changes nothing.
- **The answer's first piece goes after a few words** if no clause or
  sentence ended sooner: 4 with the configured voice
  (`CONFIGURED_FIRST_WORDS`; the first builder at 18:26: a 9-word first
  clause went to ElevenLabs 2.8 s after the words), and 6 with Gemini's
  (`FIRST_WORDS`). `LAMP_SECOND_FIRST_WORDS=N` sets it, from 0 (whole
  clauses only) to 20, for an A/B.
- **Each line's speech-to-text socket is journaled as it is taken:**
  `request N: speech to text opened in C ms, A ms of the line's sound
  waiting` (or `kept ready`). The line's sound comes in 10 ms blocks from
  its start and waits until the socket is open, then goes at once (#1712,
  the first builder at 00:25: on lamp-4ace 8 of 12 lines' words came 0.9
  to 2.3 s after the line, `Finalize`'s answer reaching only 0.3 to 1.2 s
  of the 1.1 to 2.8 s sent; Deepgram was still hearing the burst).
- **Whose words a line's are, counted, never the words** (#1712, the
  first builder at 03:48: on lamp-4ace one line said once became two
  requests of 6 words each, the second begun with nothing played). When a
  line's words are final, after its `heard N words` line, the journal
  says `request N: of its W words, K in request M's words it cut short, J
  in the lamp's answer to request A so far` (each part only when there is
  one; `tell_whose_words`). Words are compared whole, as their letters and
  digits in lower case (`shared_words`). The same words again read as a
  replay of the line before; the lamp's own as an echo of its answer; few
  of either as someone speaking. The route keeps the words of its latest
  answer sent to the voice, with its request, for this (`lamp_said`).
- **With `LAMP_SECOND_STT_WARM=on`, the next line's socket is kept open**
  (off unless set; `Engine::keeping_a_listener_ready`). As a line takes its
  socket, the next is opened in the background and kept with `KeepAlive`
  (every 5 s, `KEEP_ALIVE`). The next line takes it at once, so its sound
  goes live from its first block. Its bounds (the first builder at 00:31):
  - **at most 3 minutes** (`KEPT_FOR`), then it is closed, so a lamp
    nobody talks to keeps nothing open; a line after that opens its own;
  - **closed at once when listening stops** (the privacy switch, a mute)
    or the microphone's privacy generation changes (`Engine::listening`,
    from each authority the worker installs), and never used for a line of
    another generation; no audio is sent on it before a line takes it;
  - **one found closed** (`Listener::open`, `SttStream::still_open`) is
    dropped without a word, and the line opens its own as before.

  The cost is one idle socket to the relay's proxy during a conversation:
  Deepgram bills the audio sent, and `KeepAlive` sends none.
- **With `LAMP_SECOND_PREPARED=on` the chat is begun on the line's early
  words** (off unless set; #1712, the first builder at 17:02: the route's
  first word came about 3.4 s after a line, Live's 2.4 s, and the bar is
  under 2.0 s). Speech to text tells the turn the words it has before they
  are final (`net::Early`). With Nova-3 that's at the line's end, the
  finals and the last interim's words while `Finalize`'s answer is still to
  come. With Flux (`LAMP_SECOND_FLUX=on`) it's before the line's end: Flux
  is asked for its eager turn ends (`eager_eot_threshold=0.5`), each
  `EagerEndOfTurn`'s words are early words, and `TurnResumed` takes them
  back, dropping what was begun. Round 1 of the chat is asked on them at once (`Prepared`), its
  events kept, with nothing said and no tool run. When the words are final
  it's kept only if all three hold:
  - the words are the early ones, as letters, digits and spaces in any case;
  - the turn's system message and history are as they were;
  - no note goes beside the words, but the name note on words that name the
    lamp, which tells nothing new.

  Otherwise it's dropped and round 1 is asked as before. Neither the rule
  nor a tool runs early, since both change what the lamp knows or does.
  The cost is a dropped round now and then, and one more of the system
  message's memory recalls for each line it's begun on. The journal says
  `request N: its chat is begun on the line's early words`, then `… is
  kept, begun T ms ago` or `… is dropped: <why>`. In a host test on the
  paused clock, with a 400 ms `Finalize` answer and a chat's 600 ms to its
  first event, the first answer audio came 600 ms after the line's end,
  against 1,000 ms without it.
- **The chat starts without `CloseStream`'s round trip when it can.** It
  never starts before the coordinator's end of the line: speech to text's
  own end of an utterance comes after 300 ms of silence and would answer a
  person still thinking. But when final segments covering all the audio
  came before the line ended, the words held are the
  transcript, and the chat starts as soon as the end of the line does.
  Otherwise `Finalize` asks for the final words and they are taken as
  soon as they come, up to 1 s; a proxy that does not answer it falls back
  to `CloseStream`'s answer, up to 15 s in all. Host tests on the paused
  clock (`net/stt.rs`), against a scripted 800 ms `CloseStream` round
  trip: 0 ms to the words with finals covering the audio before the end,
  0 ms with an at-once answer to `Finalize`, 50 ms + 800 ms when that
  answer is short of the audio sent (the rest from `CloseStream`), and
  1 s + 800 ms when `Finalize` is ignored. Two more hold the whole line in both arrival
  orders: an utterance ended before more audio was sent, and an older
  final arriving after `Finalize` and before its answer. On lamp-52e6 (2026-10-15 23:30, four played questions) the
  company proxy passed `Finalize`: every turn took `words from=finalize`,
  and the last words came 246 to 321 ms after the line's end, against 564
  to 662 ms with `CloseStream` at 23:00. Why a turn waited is
  journaled once, without words, when the words are in: "lamp-voice second: request N at the line's end:
  settled=<bool>, speech_final=<bool>, finals cover <ms|none> of <ms> ms
  sent, pending=<none|interim D ms after the last final|speech_started D
  ms after the last final>, last final <T ms before End|none>[, finalize
  answered in <A> ms to <E> ms|, finalize unanswered], results <N> with
  words, <M> without, words from=<settled|finalize|close_stream>[+interim]",
  the last naming which path gave the words. The results count every
  result the service sent, interim and final: none with words says the
  service heard no words at all (#1712: on lamp-4ace on 19 Oct at 15:21 the
  owner's line had finals covering 3.07 of 3.79 s and no words). `+interim`
  says the last interim result's words were kept, its finals having come
  without them. D is audio time from the last final's end to the pending interim's end or
  `SpeechStarted`'s timestamp (negative: inside audio a final already
  covers); T is wall time since that final arrived. A is how long
  `Finalize`'s answer took, and E where in the audio it ends: short of the
  ms sent, the rest came from `CloseStream` (not told when the words had
  settled and no `Finalize` went). `CloseStream`'s words are taken once
  they cover all the audio sent, without waiting for the socket's close
  (#1712: on lamp-4ace on 19 Oct most lines took `close_stream`, 0.8 to
  1.0 s after the line, against 275 ms for the one that took `finalize`).

- **The first words go early.** Until some of the answer is on its way, a
  clause of at least four words ending in a comma, semicolon or colon is
  said on its own (`Sentences::clause`).
- **The chat is given the person's last exchanges.** Up to six, each said
  within five minutes, as user and assistant messages after the system
  message: a Live session keeps its own, and "and how many people live
  there?" needs the one before (on lamp-52e6, without it, "there" became
  the lamp's own city). They are read from the thread (`Thread::recent`),
  under the same lock as its recall and after its forgetting and reload
  (`thread::current`), so they are only the person's at the lamp and never
  something forgotten. The route keeps no copy of its own.
  Its calls are not kept, so while a task this voice prepared waits for
  the person's yes (`agent_tasks::pending_note`, within `CONFIRM_WITHIN`),
  a spoken request is told so beside its words, with the tasks part on:
  without it, on lamp-4ace on 19 Oct, the yes was taken as a new
  hand-over, asked about again and never started.
- **A tool call knows whether it was heard.** The engine passes `heard`
  to the tool runner: true in a spoken turn, false in a written one, as the
  Live route does, so a yes to prepared work cannot come in a turn the lamp
  wrote itself (`agent_tasks::may_confirm`).
- **A written request stays with its answer as a note.** When the lamp
  begins a turn itself, the request's text is kept on that exchange in the
  thread (`Thread::written`, a `written` field in the file) and given to the
  next chat as a system note ("A note you were given in writing, not words
  anyone said: ..."), never as a user message. A person who said nothing
  sends no empty user message. A Live session's recall is unchanged: it is
  not read the note. While it waits for its answer the note is bound to its
  request and to the person the turn is answered for: only that answer
  takes it, an answer to a later request (the written one was retired or
  failed) drops it, and a forgetting drops it too.
- **A request going on from one given up is asked as one.** When a
  request's end went early (`--early-end-ms`) and the person spoke again,
  the successor's start carries `continues_cut_off`. Gemini, which heard
  both, is told so (`CUT_OFF_NOTE`). This route never heard the first one
  inside the second: the chat is asked with the given-up request's words
  before the successor's, while what is told as heard stays the
  successor's own words. Only the words of the request begun just before
  it, in the same session, are taken, and the words last heard are dropped
  at every start, so none are kept for a later request. A request given up
  before its speech to text finished has no words to give: its successor is
  asked with its own words alone (the reviewer on #1168).
- **A cut-in, a line that may follow on, and a bare call are told as on
  the Live route** (#1712, item 4 of the first builder's 20:15), each as a
  system message just before the person's words, after the stop's:
  - **A cut-in** (`LAMP_CUT_IN_TOLD=on`, the worker's `--cut-in-told`): a
    line begun over the lamp's audible answer (`over_the_lamps_voice`),
    and not the first after a stop, which is told the stop instead, is
    told `cut_in_told::NOTE`. It is known from the line's start, so a chat
    begun on its early words (`LAMP_SECOND_PREPARED`) is asked with it
    and kept; a note known only later (a stop's, a bare call's, a
    follow-on's) still drops that chat.
  - **A line that may follow on** (`LAMP_FOLLOWS_ON=on`, the Start's
    `follows_on`): told `follows_on::NOTE`, unless its End marks a short
    sound or the rule judged it in a busy room (`Heeding::judged_busy`;
    with no rule, this process's room now).
  - **A bare call** (`--call-wait-ms`, `ProviderInput::Called`, which this
    route dropped before): told `CALLED_NOTE`, so the model answers as
    someone who hears their name ("Yes?"). A lone name often comes back
    with no words; such a line is asked as `Lamp.`, the name the
    coordinator heard, rather than left as a sound. Without `Called`, no
    words are still no line.
  - The journal says each: "it began over the lamp's answer, and the
    model is told so", "the model is told it may follow on from what the
    lamp said" (or "… in a busy room; the model is not told …"), "only
    the lamp's name was said, and the model is told so", and "… no words
    of it came back: asked as the name".
- **In a busy room, a line going on waits for the Policy**
  (`LAMP_GOING_ON_ASKS=on`, with the rule and the bus, #1522; #1712, item
  4): a line the rule would let out only as a conversation going on, and
  nothing else (no name, call, face, close voice, question of the lamp's
  or remark), waits for the Policy's attention decision on it, as on the
  Live route. It is played on `answer`, a `stay_quiet` said back to the
  lamp or `look_up`, held on any other `stay_quiet`, and played with no
  decision in `policy_holds::WAIT` (2 s), so a Policy that is down does not
  make the lamp deaf. The rule says the line waits
  (`second_provider::WAITS_FOR_POLICY`), and the turn asks it again every
  50 ms (`Heeding::policy_now`), never blocking the worker's one thread; a
  chat begun on the line's early words goes on meanwhile. A decision
  already there is used at once, so most lines wait for nothing: the
  Policy decides as the line's words end, and this route's words are final
  about 0.6 s after. The journal says "its answer waits for the Policy's
  decision", then "the Policy's decision came after N ms" or "no decision
  of the Policy's came in N ms".
- **The first request after a stop is told of it.** With
  `LAMP_STOP_TOLD=on`, the first request after a hand or the head stopped
  the lamp carries `after_stop`. Gemini is told so with `STOPPED_NOTE`. This
  route gives the same note as a system message just before the person's
  words, in that chat only, so the stopped story is not begun again.
  A request with `after_stop` that is never asked (it heard nothing, was
  held as a short sound or not said to the lamp, or was replaced before its
  chat) leaves the note to the request begun right after it in the same
  session, so a cough after a stop does not use it up (the reviewer on
  #1175). It is told once, with the chat that is asked, and again to a
  request going on from that one (`continues_cut_off`), which was given up
  after it was asked. It is never told more than 2 min after the request
  that carried it began (`TELL_STOP_WITHIN_US`, as the coordinator).
- **Each turn is one person's**, as a Live session is since #775: before
  the chat, the thread is opened for whoever the eyes count at the lamp
  (`Sessions::open`, read only as the turn opens: someone else known after
  10 s with the one before unnamed, or at once when the one before was
  last named 30 s ago or more), and that person's kept things are asked
  for. With
  Days memory, the note is asked once per run of turns for one person.
- **Each turn is logged** as "lamp-voice second: request N ...": the words
  heard and how long after the line, the state of the words at the line's
  end (above), each chat round's first event (the first text or tool
  calls: a role-only chunk carries neither and is not an event), the
  tools called, the first answer audio, and any failure.

- Up to four chat requests in a turn, tool rounds included (`MAX_ROUNDS`).
- A call to `not_for_me` ends the turn silently, before the unfinished last
  words are said.
- A due reminder is deferred (`ReminderDeferral::Disabled`): this route
  does not speak the lamp's own requests.
- A failure is `TurnFailed` at the stage the coordinator accepts: InputOpen
  while the line is coming, AwaitingResponse before any answer audio,
  Responding after.

## What a person hears

When the coordinator switches (below):

- **Switched at once:** a turn Gemini fails before any of its answer was
  heard, whatever the fault (a quota, a refusal, a rate limit, a server
  rejection, a dropped connection), or a link that changes during the line.
- **Switched on a response timeout:** not sooner than Gemini's
  `response_timeout_ms`, which cannot go below 2 s (`config.rs`). A turn
  switched this way cannot meet the 1.2 s target below. With
  `no_reply_after_ms` set, slowness ends as Unanswered, which does not
  switch.
- **Nobody repeats themselves:** the line is replayed to the second route
  from its start.
- **The lamp never answers twice:** Gemini's late answer to that turn is
  dropped.
- **One sentence:** "My voice sounds a little different just now." is said
  before the first answer when the route has not answered in the last 120 s
  (`ANNOUNCEMENT`, `ANNOUNCE_AFTER`), because the voice cannot be Gemini's.
  It is not kept in the thread, and it is heard, not written: no
  transcript of it goes to the coordinator, so an answer's opening words,
  which it reads for a look ("Let me look.") and for the body's gesture,
  are the model's. Each sentence after it is said after a
  space, so the lamp's words read "just now. Hmm," (#1014). Never said
  when this route is the only voice ("Second only", below).
- **As quiet as Gemini:** with `LAMP_ATTENTION` on, a spoken turn's words
  are judged by the Live route's own rule for whom it answers
  (`crate::attention`, [attention.md](attention.md)) before the chat is
  asked (`worker::Heeding`). It is given the room as Gemini's worker
  publishes it (`room/state`), the lamp's name as `lamp-name` hears it
  (`name/heard`, dated by the wall clock, in the window of the line's first
  and last microphone packets, so a replayed line keeps its time), whether
  the eyes see someone turned to the lamp, the thread (a conversation
  going on, the lamp's last line a question), and with
  `LAMP_ATTENTION=busy-near` how far the room microphone rose over its
  quiet during the line (`room/sound`, sampled every 100 ms on a thread of
  its own, as the Live worker samples it). Words it holds end the turn
  after them, unanswered. Each turn is judged as a session's first, except
  one going on from the request begun just before it in the same session
  (`continues`, the same utterance after the lamp ended it early, with
  `LAMP_CUT_KEEPS_NAME=on`), when the rule judged that one last: it keeps
  what the rule knew of it, its name, words so far and whether it was
  close, as on the Live route (`Attention::continued_at`). A request going
  on from one the rule never judged (replaced before its words came back)
  is judged fresh, never as going on from an older question (the reviewer
  on #1226). `over_the_lamp` (a line begun over the
  lamp's reply) is used by nothing here: this route runs no same-voice
  check and no staff note, the two things it guards on the Live route. On
  lamp-4ace on 2026-10-16 this route answered "Me." and half a question
  between colleagues while Gemini's session was renewed; both are held now.
  With `LAMP_REPORTS=on`, a held line whose heard words name `report`,
  `reports`, `sharing`, `sending` or `data` (`attention::HELD_ASKED_WORDS`,
  #1925; any other is sent nowhere) is still asked once, told only the
  functions done though a line's answer is held
  (`attention::DONE_THOUGH_HELD`, today `reports_stop`), with a note that
  nothing said now is heard (`HELD_NOTE`), as the Live route's model hears
  every line and its `reports_stop` is done though held (the first builder
  on #1722, 16:44). With `LAMP_SECOND_SCOPED_CONTEXT=on` that round is
  told for the line's admitted person or room alone, as every other turn
  is, never for the session's sticky person
  ([memory.md](memory.md#second-route-context-scope)). Only those calls are run, with no round after; any
  words are dropped and nothing is played. The turn ends when that round
  does, given up after `HELD_WITHIN` (6 s); a line begun meanwhile replaces
  it, and words carried from a line it cut short are answered beside it.
  The round costs one more chat request per held line naming reports, only
  with the switch.
- **Back to Gemini:** silently, at the next idle moment with Gemini Ready,
  or at the start of the next turn if the room never goes quiet. Nothing
  is said about it.

## Same character, memory, thread and tools

`worker::serve`:

- **Character:** the instruction of the Live route's `provider.json`
  (`ProviderConfig::instruction`, the version without a camera), then the
  time and place (the time as the request is built, "Second only" below),
  then the memory note (the
  Days note frozen for the process, or the memory writer's, read for each
  turn), then the conversation thread in the room the rest leave of
  16,384 bytes. Built when each turn's chat begins.
- **Thread:** the same file as the Gemini worker (`--thread-file`).
  Before each turn the route takes the file if Gemini wrote it since, and
  after each turn it writes the file whole. Gemini does the same at each
  connect. The rule is in [conversation-thread.md](conversation-thread.md),
  "Two voice routes, one file". The memory-forget path is honoured first.
- **Journal:** each completed turn goes to the memory's journal as the
  Live route's do (`remember::finished`).
- **Tools:** the same declarations (`tools::declared`), mapped to the
  OpenAI `tools` schema. Each call runs through `tools::answer` and
  `tools::run_call`, the one function the Gemini worker also uses
  (`ToolDesk::called`), so the light, schedule, memory, tasks and eyes
  behave the same. The Gemini worker's log line ("request N asked for X;
  the tasks part answered in N ms; ...") is unchanged; this route logs the
  same words after "lamp-voice second:", ending "the answer was sent".
- `LAMP_MEMORY_DAYS=on` reaches this worker as it reaches the Gemini one
  (`process.rs`).

## What the rest of the lamp sees

The route's audio goes through the same speaker worker, so the echo
canceller and "spoken over: it stops" behave as now. The ring, the body and
Expression follow the same provider states. Its turns are journalled like
any other. Shown with peer processes in `coordinator/tests/second_route.rs`
(below): a turn the second route answers leaves the speaker the same
authorities, the ring the same phases and the trace the same finished-turn
record as one Gemini answers, and spoken over it stops the same way.

## The trade-off

With the company's defaults the route goes through the same relay as
Gemini. It then covers a Google-side outage, quota or refusal, not an
outage of the relay. A lamp with its own keys set (its own endpoint, or a
`deepgram_credential_file`) is covered for both. This is the intended
default.

## Latency targets

Agreed on the issue, from the end of the line to the lamp's first sound.
**Not met on lamp-52e6** (2026-10-15, eight played questions,
[verification](verification/754-lamp-52e6-20261015/README.md)): median 4.3 s
to the first write to the speaker, 3.0 to 3.6 s for a plain question with no
slow chat. The chat through the relay takes 1.2 s at best to its first
event, with spikes to 7.7 s; speech to text gives its last words 0.6 to 0.9
s after the line; speech starts 0.4 to 0.6 s after the chat's first event.
Since then (#779) speech to text's last words are asked for with
`Finalize` at the line's end. On lamp-52e6 at 23:00 the journal showed that
no final segment had arrived by the end of any of four lines (`finals cover
none`), even with `endpointing` at 300 ms, so the words always waited for
`CloseStream`'s final; with `Finalize` at 23:30 they came 246 to 321 ms
after the end, about 0.34 s sooner on every turn. End of the line to the
first sound was then 3.4, 4.4, 6.0 (a tool round) and 2.8 s: the rest is the
chat through the relay, 1.0 to 2.7 s to its first words, whose own spread is
larger than anything left in this code. From the lamp, 20 requests each,
the relay's `gpt-4.1-mini` took 1.1 to 1.2 s (p50) to its first words, with
the 9.6 KB persona and tools no slower than one line.

The targets, as revised by the first builder on #779 for a voice used only
while Gemini is down:

- End of the line to the first sound: p50 ≤ 2.5 s, p95 ≤ 4 s.

Not yet met on lamp-52e6 (median about 3.9 s at 23:30). The relay's speed
and a direct provider key are the owner's and the relay team's.

A switch on a response timeout cannot meet the first (above).

## Timing the first words (#828)

`lamp-voice stt-check CONFIG WAV [ROUNDS] [--held-ms MS]` streams a recorded
line (16 kHz mono 16-bit) to this route's speech-to-text, as `CONFIG` (the
file `LAMP_VOICE_SECOND_CONFIG` names) sets it up, and prints one line a
round:
- `connect_ms`: opening the stream;
- `first_words_ms`: from the first audio queued to the stream to the first
  result with words parsed from it, interim or final
  (`SttStream::first_words`);
- `audio_ms`: how long queueing the line took;
- `final_after_audio_ms`: from the line's last audio queued to its final
  words;
- `words`: the final words.

What it times is the client's queue to a parsed result. That isn't the
moment audio leaves on the wire, and it isn't how long a stop takes on a
lamp, which also has the pause, the decision and the speaker.

The first `MS` of the line go at once as one message, standing in for a
paused reply's held audio (#828, step 2: a stop word heard during the
pause). The route sends 20 ms frames, and a pause that sends its held audio
the route's way would send those frames back to back. The rest of the line
goes at the pace it was said, in 20 ms frames. Three rounds and nothing
held, unless said; at most 20 rounds and 1,600 ms held. It reads the
credential file the config names, so on a lamp it runs as the voice's user
or root.

The sixth builder measured the relay with it on lamp-52e6 on 16 October,
with synthetic voices (#828, 08:22). No words come before a line ends: the
first came 0.22 to 0.30 s after its audio ended, and 0.24 to 0.38 s after a
600 ms burst. A burst lost the end of "Hang on." and "Lamp, stop.", which
came from `Finalize`, fixed in #956.

## Timing each answer for the A/B (`answer_first_audio`, `answer_stages`)

The session's trace (`session.jsonl`) gets two records for a request, so an
A/B against Live times the answer and never a cue. On 19 Oct the route's
"first sound" had been its wait cue at 1.5 s, and the answer came 4.0 to 4.5
s after the words.

- **`answer_first_audio`**, on both routes, once a turn:
  `{"kind":"answer_first_audio","turn":N,"at_us":…,"after_decision_ms":M}`.
  - It's written when the answer's first audio reaches the coordinator
    (`AnswerAudio` in `coordinator.rs`). On the Live route that's the turn's
    first audio. On the second route it's the first audio after the turn's
    first words: a wait cue, the down line and the announcement come with no
    words, and each sentence of the answer comes with its words before its
    audio.
  - `after_decision_ms` is from the end-of-speech decision for that turn
    (`local_endpoint`). It's null for a turn with no decision, the lamp's
    own.
- **`answer_stages`**, on the second route, for every request before its
  end:
  `{"kind":"answer_stages","turn":N,"at_us":…,"from":"line_end","decided_at_us":…,"words_ms":…,"cue_ms":…,"chat_first_ms":…,"first_answer_audio_ms":…}`.
  - Each stage is in ms from the end of the line as the second worker heard
    it: its audio queue closing, or for a written request or a reminder, when
    it began.
  - The stages are speech to text's words; a wait cue begun; the chat's first
    event in round 1, text or a call; and the answer's first audio sent.
  - A stage not reached is null. A request whose chat never began still sends
    its words and cue times (`ProviderOutput::Stages`).
  - `decided_at_us` is the end-of-speech decision's time on the
    coordinator's clock, to line the two records up.

The battery's tool counter reads the journal. Without
`LAMP_POLICY_HOLDS_ACTIONS`, a call the Live route would hold for the
Policy is said on this route as that route says one it lets run:
`request N asked for X; run after 0 ms: this route holds no call for the
Policy` (`worker::run_after`). With the switch, the bus and the route's
rule (`LAMP_ATTENTION`), this route holds those calls as Live does, by the
Policy's decision about the turn. Its gate then says Live's own lines with
the real wait ([attention.md](attention.md#on-the-second-route-1712)).

## Coordinator switch

`coordinator/second.rs`. With `LAMP_VOICE_SECOND_CONFIG` set in the
service's (`serve`) environment and the Gemini source, `spawn_rig` starts
the standby `second` worker just before the privacy worker, with
`second::arguments`: the Gemini worker's arguments, the second
configuration after the first, less `--keep-link`. Its readiness is taken
and its authority kept fresh during the rest of startup.

- **One active route.** `Rig::second` holds the standby worker, its own
  link register and output credit. A switch swaps all three with
  `rig.provider`, `cloud` and the output flow, so every provider path runs
  unchanged against the other route. The standby gets every authority;
  its link reports go to its register; its output is drained, credited and
  dropped (how a late Gemini answer is lost).
- **At rest** (no turn in flight, nothing queued): to the second route
  while Gemini is not Ready and has not ended, as a stand-in only (below);
  back to Gemini, silently, on the first idle tick its link is Ready again
  or has ended (so Gemini's end still ends the service).
- **While Gemini reconnects** (#960): with no turn in hand when Gemini's
  link drops (lamp-4ace, `server_go_away`, Ready again after 1.9 s), the
  second route only stands in, and a request admitted meanwhile is held
  for Gemini as at startup, up to `STARTUP_HOLD_US`; the second route is
  given it only when the hold is over or Gemini's link has ended. A line
  Gemini loses mid-line (09:03, `write_timeout`) still goes to the second
  route at once: Gemini's worker has retired that request with its turn
  (`TurnLost` raises `failed_through`), so a replay of it to Gemini's new
  session would be dropped; going back to Gemini mid-line would need a new
  request. The next turn goes to Gemini once it is Ready (#962). Until
  #960, a request after Gemini's first Ready went to the second route at
  once (#885's review).
- **A new turn on the second route** goes to Gemini when Gemini's link is
  Ready, before anything of it is sent (`second::flush`): its queued
  commands are rebound to Gemini's session, recorded as
  `voice_route_switched` to `live`, `live_link_ready`, with the turn.
  Between turns is not the only way back: on lamp-4ace (2026-10-16
  09:03–09:06, #960) a busy room left no idle tick, and every turn went
  to the second route with Gemini up.
- **At startup**, before Gemini's first Ready of this voice process, the
  second route only stands in (that swap and the swap back are not
  recorded): a request admitted on it is held, nothing of it sent, for up
  to `STARTUP_HOLD_US` (lampOS#779). If Gemini's link is Ready in that
  time the turn is rebound to Gemini's session and its whole line goes
  there; if not, or once Gemini's link has ended, the second route is given
  the whole line, as a switch, even when Gemini's Ready comes in the same
  tick. The hold is derived from Gemini's audio age bound
  (`lamp_gemini::MAX_INPUT_AGE`, 2.5 s) less the line's lead-in (the
  detector's 30-block pre-roll, 300 ms) and a 300 ms margin: 1.9 s. A line
  held longer would replay a first block older than the bound, which
  Gemini's worker refuses as stale, and the worker ends (lampOS#892). It
  was 3 s when the four lamp-52e6 runs of PR #885 were measured: runs 1, 3
  and 4 (held 1.0 to 1.2 s) fall inside the new hold; run 2 (held 1897 ms)
  is at its edge and would now go to the second route if held 3 ms
  longer. One hold only: a request after that goes as
  at rest. A held turn cancelled and replaced before the next tick's
  upkeep gives up its hold there and then, so its replacement is held
  afresh rather than sent to the second route. After Gemini's first Ready
  nothing is held and an outage switches at once, as above: the first
  Ready is latched as it is observed (`CloudAvailability::was_ready`), so a
  Ready and its loss in one control drain still count, and a hold then in
  hand goes to the second route at once.
- **Mid-turn**, while Gemini is active, nothing of its answer was heard and
  the line was kept whole: a `TurnFailed` of that turn, or a link change
  during the line (the request is marked `movable`, so the gate leaves it
  for the switch). The turn is rebound to the second route's session and
  its line queued again: Start, every sent block from 1, the unsent ones
  (a held line's replay tail not yet sent to Gemini included), then what
  is queued, End among them. A reminder turn is not moved.
- **The line** (`second::Line`) is what the one send site sent Gemini for
  the turn in hand: in memory only, never on disk or the bus, at most 30 s
  of 16 kHz mono (beyond that it is dropped and the turn is not moved),
  cleared when the line is answered or abandoned and at once when an
  authority without its owner or with the microphone closed is published
  (mute, privacy, cancellation).
- **Pace.** A replay to the second route goes eight commands a tick,
  before what was queued since; the second worker takes eight inputs a
  tick, as the Gemini worker does, and holds 30 s of input while its
  speech-to-text connects. A held line given to Gemini goes two commands a
  tick (`LIVE_REPLAY_PER_TICK`, about ten times the pace of speech), with
  what is queued since joining it: Gemini's link takes input into a queue
  of 32 (`lamp_gemini::INPUT_QUEUE_CAPACITY`) and gives the connection up
  as `Backpressure` (reported `local`) when it is full. Sent at eight a
  tick, a line held 1.9 s overran it 37 ms after Gemini's first Ready on
  lamp-52e6 (PR #885 run 2, lampOS#892), and the second route answered.
  The pace alone does not hold through a write that stalls: a 90 ms stall,
  within the link's 200 ms write bound, still filled the queue at two a
  tick. So, with a second route configured, the coordinator gives Gemini's
  provider worker `--wait-for-room`, and the worker offers a command only
  when the queue has room (`InputSender::has_room`); one that finds it full waits, first in
  line, with nothing more read from the coordinator, for up to
  `ROOM_WAIT_US` (250 ms, past the write bound), and then goes, a queue
  still full being the link's failure as before. What the coordinator
  sends meanwhile waits in its own socket and queue, in order. The End a
  cancellation owes an open request waits for room the same way (the
  retirement itself stays immediate), and nothing of the next request is
  offered before it. This changes how Gemini's worker meets a full queue
  for all its input, not only a replay, but only on a lamp with a second
  route.
- **Without a configured route** `Rig::second` is `None`, the three
  intercepts (the send site, the output receipt, the gate's `movable`) are
  the old calls, Gemini's worker is not given `--wait-for-room` (a full
  queue ends its connection as it always has), and nothing else changes.
- Trace: `voice_route_switched` (`to`, `why`, `turn`, `replayed_commands`,
  and the failure's `stage` and `reason`); `voice_route_held` (`turn`,
  `held_ms`, `to`: `live` or `second`) when a startup hold ends; the standby's startup link
  record carries `"route":"second"`; an Ended of the standby, `"standby":true`;
  `voice_route_dropped` (`status`, `active`) when its worker exited.
- **Held up, the second worker goes on** (#960): authority that lapsed in
  its queue while it was held up (lamp-4ace's SD/MMC stalls, #784) is
  skipped, as the Live worker does, and the controller's heartbeat is
  judged once the queue is empty (or after 1.5 s of backlog). Before, a
  stall while finishing a turn ended it with "second route privacy or
  input lease expired", and the voice with it (lamp-4ace, 2026-10-16
  08:47).
- **A second worker that exits is dropped** (#960): checked once a tick
  before anything else of the tick touches it (`second::dropped_if_gone`).
  Standing by, the route goes and Gemini goes on. Active, Gemini's worker
  is put back first and the turn the route held ends
  (`second_route_failed`) as a turn the provider interrupted does: its
  unsent input and unwritten speech go, and the rest of a line still
  being said is drained rather than taken for a new request; the next
  turn is Gemini's. The 50 ms health check and the authority sends leave
  the second worker to this, in whichever slot it is; Gemini's worker,
  waiting while the second route is active, stays mandatory, so its exit
  still ends the voice (`coordinator::mandatory`). Recorded as
  `voice_route_dropped` (`status`, `active`). An authority that cannot
  reach it is not an error either. A worker that dies inside the very
  tick that touches it still ends the voice, as before.

## Second only

The owner, 19 October: Gemini Live is expensive; this route is cheaper and
slower; test it alone and compare the two routes' latency, an A/B on
lamp-4ace. `LAMP_VOICE_ROUTE`, in the voice service's environment, picks the
lamp's voice (`coordinator/second.rs`, `routes`):

- **Unset, empty or `gemini`:** Gemini Live, with this route standing by
  when `LAMP_VOICE_SECOND_CONFIG` is set: everything above, unchanged.
- **`second`:** this route is the lamp's only voice from the first turn. It
  needs `LAMP_VOICE_SECOND_CONFIG`.
- **Anything else, or `second` without that configuration:** the voice does
  not start. `run_serve` refuses before anything starts, never falling back
  to Gemini, and the journal says why, either
  `lamp-voice: LAMP_VOICE_ROUTE=second needs LAMP_VOICE_SECOND_CONFIG, the second route's configuration: the voice does not start, and does not fall back to Gemini Live`
  or `lamp-voice: LAMP_VOICE_ROUTE is gemini or second, not "…": the voice does not start`.
  A value is taken exactly: `Second` or ` second` is refused. A bounded run
  (`lamp-voice directed`) has no second route, and refuses `second`.

**The lamps' voice since 19 October** (the owner, #1712). The product unit
(`deploy/product/lamp-voice.service`), lamp-52e6's, the repo's lamp-4ace
unit and the attended pilot set `LAMP_VOICE_ROUTE=second`, as lamp-4ace runs
it: `LAMP_VOICE_SECOND_CONFIG=/etc/lampOS/voice/second.json`,
`LAMP_SECOND_CHAT=deepseek`, `LAMP_SECOND_SEARCH=deepseek`,
`LAMP_SECOND_SPEECH_MODEL=eleven_v4_turbo`, the down-line and the wait cue.
Live stays a per-lamp setting: a drop-in with `LAMP_VOICE_ROUTE=gemini`.

**A product lamp needs nothing a person adds** (the first builder at 21:08
and 21:14). Every service goes through the Autonomous relay with the one key
the lamp already has:
- DeepSeek's chat and search go through the relay
  (`LAMP_SECOND_DEEPSEEK_BASE_URL=https://campaign-api.autonomous.ai/api/v1/ai/v1/deepseek`,
  #1939), with second.json's `llm_credential_file`, so the unit loads no
  credential.
- Speech to text and the voice fall back to the text model's base and key,
  as on V1.

It needs two files in `/etc/lampOS/voice`, root's and 0600:

| File | Placed by | What reads it |
|---|---|---|
| `second.json` | install, and an update round before it writes a voice unit that names it, from `deploy/product/settings/second.json` (the relay's `…/api/v1/ai`, `gpt-4.1-mini`, ElevenLabs), kept up to date while it is install's own; a lamp's own is kept. On a lamp whose updater predates the round's step, the voice places the same bytes as it starts when the file is missing (`second_config::place_product_if_missing`; updates.md) | `LAMP_VOICE_SECOND_CONFIG` |
| `credential` | setup (the user's key), or install from V1's config; the same file `provider.json` names | second.json's `llm_credential_file`, for every service |

Without either, the voice does not start, and the route never falls back to
Live. `lamp-selftest`'s `voice_route` part names what is missing (`selftest.md`).

lamp-4ace's repo unit takes DeepSeek through the relay too (the owner,
21:3x: "instead of using their direct deepseek key"), and loads only
Gemini's key (`LoadCredential=gemini.key:/etc/lampOS/voice/second-llm.key`).
The A/B of DeepSeek direct against the relay takes `deepseek.key` in
`probe-cascade`'s own transient unit, never in the voice's. Its own
second.json names `second-llm.key`, `second-stt.key` and `second-tts.key`,
which hold the relay key.

With `second`, the journal says once at start:
`lamp-voice: the voice route is second only (LAMP_VOICE_ROUTE=second): speech to text, a text model and speech answer every turn; Gemini Live is not used`.

**Telling the sessions apart.** With `second`, and only then, the session's
`run_start` record carries `"voice_route":"second"` (`Routes::mark`), and
voice-eval's importer keeps it in the imported attempt's evidence
(`evidence.voice_route`); an unknown value is refused. `provider_kind` stays
`gemini`: the kind of provider that may answer every turn, so the A/B's
sessions import against the same scenarios (the reviewer on #1686).

**What leaves the lamp, on every turn:** each admitted line's audio goes to
speech to text, its words (and a look's picture) to the text model and the
answer's text to speech, as in "A turn on the second route" above, now on
every turn and not only after a take-over
([service-privacy.md](service-privacy.md), "Which service hears a line").

**How.** The route starts in the state a take-over leaves, with no Gemini
behind it. `spawn_rig` starts the `second` worker where Gemini's would be,
in `rig.provider` (`Routes::workers`), with `second::arguments` (Gemini's
worker's arguments and both configurations, less `--keep-link` and
`--wait-for-room`) and `--only-voice`; `Rig::second` is `None`. No Gemini
worker is started, so no Gemini session is opened. Its readiness is taken
during startup as Gemini's is (`StartupEvents::poll`). Every turn, spoken
requests, the lamp's own written turns, reminders, called and named turns
and looks, then goes to it through the paths written for `rig.provider`, as
after a take-over: authority, leases, cut-ins and duplex, the say service,
notes, the tools declared, and the voice state, Expression and the ring on
the bus. Nothing is held, replayed or switched, and no `voice_route_*`
record is written.

- **The link, when it is the only voice** (#1844, the first builder's yes,
  19 Oct; `second_provider/link.rs`). Before, it said Ready once at start
  and never again, so with the Wi-Fi off every turn failed in silence and
  `lamp/online` stayed true. Now a turn that fails on the network (a
  connection refused or reset, a connect that timed out, an answer that
  never came: `Transport`, `ConnectTimeout`, `ResponseTimeout`, at any of the
  three services) checks whether the services' hosts answer at all: a bare
  TCP connection to the chat's, the speech's, speech to text's and Gemini's
  host at once, nothing sent, any answering within 2 s enough
  (`PROBE_WITHIN`). One answering: a service was slow or refused, and
  nothing changes (a status, 401, 429 or 5xx, never checks: the service
  answered). None: the link is Unavailable with the turn's fault, and the
  check runs every 30 s (`PROBE_EVERY_US`, as `--reach-probe`) until one
  answers, when it is Ready again in a new session. The journal says
  `lamp-voice second: no service host answers after a turn failed on the
  network: the link is down, checked every 30 s` and `… a service host
  answers again: the link is back`. So the offline logic follows it as it
  follows Gemini's ([offline.md](offline.md)): with `LAMP_OFFLINE_VOICE=on`
  the offline line once and the breath, `lamp/online` false from health and
  Expression's offline face, and "I'm back online." after; offline speech is
  a local-only turn that never reaches this worker, which is why the check
  runs on its own. The first turn of an outage still fails as before (it is
  what finds it). A standby's link is unchanged.
- **Its exit ends the voice**, as Gemini's does (`coordinator::mandatory`):
  it is the only voice. A standby's exit is still dropped, and Gemini goes
  on.
- **Its stop is Gemini's (#1620).** On a stop (SIGTERM, a mute, a fault)
  `Rig::shutdown` sends it Stop with the other workers and holds it to the
  same shared deadline, exit recheck, kill, reap and cleanup accounting
  (`ForcedStop`/`WorkerExit` under its own role, `second`). Speech to text,
  the chat and the voice are tasks inside this one process, not children of
  their own; the endpoint is the session's, on either route. Tested
  mid-request and for a worker that ignores Stop
  (`second_route::a_stop_while_the_second_route_alone_is_mid_request_is_clean`,
  `a_second_worker_that_ignores_stop_is_killed_and_named_as_geminis_is`).
  A stop during a blocking call no longer holds it: `serve` ends its tokio
  runtime with `worker::finish` (`shutdown_background()`), since a plain
  drop waited, with no bound, for running `spawn_blocking` work (a tool
  call, up to `tools::ASK_DEADLINE`, 1.5 s; a text's tool call; a DNS
  lookup) and outlasted the shared 300 ms, so the worker was killed and
  cleanup failed. The process exits right after; blocking work left
  detached dies with it, as the Live worker's detached call threads do
  (`a_stop_during_a_slow_tool_call_lets_the_second_worker_exit_within_its_deadline`:
  1.50 s before, within 300 ms after).
- **Its start never waits for a service (#1567).** The worker says its link
  is Ready, session 1, as it starts (`worker::run`), before any service is
  asked; speech to text and the chat connect per line. A refused or silent
  Deepgram, DeepSeek or ElevenLabs fails each line (`TurnFailed`, with the
  fault of `second_provider::fault`), never readiness, so the voice runs
  past the 25 s budget with `LAMP_START_OFFLINE` on or off; the switch has
  nothing to wait for here
  (`the_second_route_alone_with_its_services_refused_stays_up_past_the_startup_budget`,
  `the_second_workers_link_is_ready_before_any_service_is_asked`).
- **No "My voice sounds a little different just now."** `--only-voice` turns
  the announcement off (`Engine::announcing`): there is no other voice to
  differ from, and it would be the first thing heard in every answer after
  two quiet minutes, ahead of the model's words.
- **A due reminder is said**, as the Live route says it, with
  `LAMP_SCHEDULE_SPEAK` on (`Engine::saying_reminders`): the chat is given
  the Live route's own note for it (`provider_worker::reminder_note`, the
  item's words as data), and what it says is the reminder; a call it makes
  is not run, as the Live route's tool gate holds it, and an item the Live
  route refuses (empty, over 300 bytes, a control character) is refused the
  same way. Standing by, a reminder is still deferred
  (`ReminderDeferral::Disabled`), for Gemini to say once it is back. The
  journal: `lamp-voice second: due reminders are said on this route, the
  lamp's only voice`.
- **The voice's own sayings are said**, as on Live (the first builder's
  20:15 on #1722): task news told unasked and progress news (each a
  written turn of the news's note, at the same quiet moment), a skill's say
  (`LAMP_SAY`, answered `spoken` on the same proof), and news asked about
  (`check_agent_task`), whose key the turn takes out of the answer before
  its model reads it and sends as `NewsAnswered` (`docs/tasks.md`). So the
  tasks part is told a finished task was heard once its turn is heard
  whole, and closes its harness, as on Live. Tests:
  `coordinator::tests::second_route::news_is_told_once_at_the_next_quiet_moment_on_the_second_route_as_on_live`
  and `a_skills_say_is_said_and_answered_spoken_on_the_second_route_as_on_live`
  (the loop, both routes), and `second_provider`'s
  `news_asked_about_rides_on_its_turn_and_its_key_is_never_the_models` and
  `news_asked_by_voice_through_this_route_is_put_on_the_turn`.
- **The hums, as on Live** (the first builder's 21:51 on #1722). With
  `LAMP_OTHER_LANGUAGE=sorry` (`--ask-unclear`) the route's rule
  (`worker::Heeding`, the Live rule's own `crate::attention`) holds a line
  of a conversation heard mostly in another script, as a number alone, or
  as a cut-in over the lamp's answer with no English word
  (`End { cut_in }`, told to the rule before it judges). The turn then sends
  `ProviderOutput::Unclear` before its `TurnComplete`, with no chat asked
  (`Engine::asking_again`). So the coordinator asks "Sorry?" in writing, and
  hums for an empty one with `LAMP_UNCLEAR_HUM=on`, as for Live. With
  `LAMP_MISHEARD_HUM=on` a misheard cut-in is sent with `cut_in: true`: the
  lamp hums its "hm?" with no model turn, and the first request asked within
  10 s is told it wasn't caught (`misheard_hum::NOTE`), once. Journal:
  `lamp-voice second: request N was held as unclear (why): …`. Tests:
  `worker::tests::a_line_heard_as_nothing_the_conversation_means_is_held_as_unclear`,
  `second_provider`'s
  `a_line_held_as_unclear_is_told_and_a_misheard_cut_in_is_noted_next`, and
  the loop's
  `an_empty_sorry_in_the_loop_hums_with_the_switch_and_only_then` and
  `with_the_switch_a_misheard_cut_in_hums_and_sends_no_model_turn`, each run
  on Live and on the second route alone.
- **A holding-only answer is asked to go on**, as on Live
  (`LAMP_HOLDING_ANSWER=on`; the first builder's 22:54 on #1722). A spoken
  turn whose whole answer only asks the person to wait ("One moment."),
  with nothing called, sends `HoldingAnswer` before it completes, and the
  coordinator asks the route once to go on
  ([holding-answer.md](holding-answer.md), "On the second route").
- **A line lost on the network is asked once more** (`LAMP_RESEND_LOST_LINE=on`,
  as on Live; the first builder's 00:02 on #1722), as the lamp's only voice.
  - **When:** a spoken line whose round-1 chat failed before its first
    event (no word said, nothing called), with a fault `link::lost` counts:
    `Transport` or `ConnectTimeout`, the faults the link's own check runs
    for, less `ResponseTimeout`, which Live's `resend::mendable` holds
    would only stay silent again.
  - **How:** the turn waits for the link, asking every 250 ms whether any
    service host takes a bare connection, as the link's check does
    (`link::any_answers`). If one does within `resend::RESEND_WITHIN_US`
    (5 s) of the failure, the same messages are asked once more, round 1's
    bounds running from then.
  - **Otherwise** it fails as before, and the link's check and down line
    follow. The turn's `TurnFailed`, which starts that check, waits only
    those 5 s, so the two never act on one failure at once.
  - **Never:** twice, after a word or a call, for a status (a refusal is no
    network loss), for the lamp's own written turn, or once a newer line has
    begun. That line replaces the turn (`Engine::begin`).
  - **No second "sorry":** the down line is said only for a refusal or a chat
    not begun in time, never for these.
  - Journal: `lamp-voice second: request N lost on the network (…): the link
    is back, asked once more`, or why it is not asked again.
  - Tests: `second_provider`'s
    `a_line_lost_on_the_network_is_asked_once_more_when_the_link_is_back`
    and `a_line_is_not_asked_again_after_words_a_call_a_refusal_or_a_newer_line`
    (paused clock), and `link`'s
    `a_line_is_lost_only_on_the_network_and_never_for_a_silent_answer`.
- **Its input is taken late**, in order and from the parent's boot, as the
  Live worker's is since 40de19327 (`WorkerChannels::taking_late_input`): its
  input is checked against fresh authority when used, and its end would end
  the voice. Standing by it keeps the bound, as before.
- `LAMP_SECOND_WAIT_CUE`, `LAMP_SECOND_DOWN_LINE`, `LAMP_SECOND_CHAT` and
  `LAMP_SECOND_SPEECH` work as with a standby.

**What differs from Gemini Live**, for the A/B to watch. All of it is as
after a take-over, but now for every turn:

- **Latency** is this route's (Latency targets, above): speech to text's last
  words, the chat's first words through the relay, then the first speech.
  A turn leaves the same `turn_finished` record on both routes.
- **The room is this route's own** (#1712: on 19 Oct the route alone missed
  two calls and answered a bystander, every line judged in an unknown room,
  since nothing published `room/state`). The second worker keeps the room in
  its process's `room_told::RoomTold`, as the Live worker keeps its own:
  - **fed by the engine:** a line begun (`heard`, and `began_over_the_lamp`
    for one begun over the lamp's voice) and more of it (`hearing`); its end,
    or its end as a sound; the rule's verdict (`judged`); the lamp speaking an
    answer (`spoke`), or its own turn or a reminder (`spoke_unasked`); a call
    that does something (`answered_by_a_call`, not a lookup); and a line left
    unanswered, by the rule or by the model;
  - **read by the rule** (`worker::Heeding::as_the_only_voice`): the room
    `at` the moment, and no conversation going on once people have talked
    past the lamp. The rule is set as the Live worker sets its own: remarks
    held (`LAMP_REMARKS`), and with `LAMP_SPOKEN_TO` only talk between
    people counted, by day too;
  - **told what the lamp did** (`Engine::telling_what_it_did`,
    `worker::Heeding::lamp_did`), as the Live worker tells its rule: a
    task, a timer or a reminder asked of its parts (`attention::worked`),
    so a question about its work later is for it, and its own turn
    beginning to be heard (`attention::news_told`), so a reply to its news
    is. Without it, on lamp-4ace on 19 Oct, every line 104 s after a
    hand-over was held as no conversation, where Live answered all five;
  - **told and kept:** `room/state` on the bus at each tick, and the room's
    file (`LAMP_ROOM_FILE`) kept about once a second and taken back at the
    start, as on the Live route.
  - **one session, as Live's** (#1712, the first builder at 16:43): the
    rule judges the route's lines as one session's, not each as a session's
    first, so a line past the session's first two is trusted as on Live. A
    new one begins as Live renews its own: for a new person at the lamp,
    and after 8 lines held in a row, at most every 15 minutes.
  - **a conversation goes on from a call** (#1712): a line that called the
    lamp for itself (its name, its words, a face turned to it), let out,
    carries the person's next lines for 15 s (`CALL_CARRIES_FOR`), and ends
    the count of talk past the lamp before it (`RoomTold::called`). The
    lamp's own words carry them for 45 s, as on Live. 15 s is enough for
    the owner's lines every 6 s, and a sound half a minute after a call the
    lamp never answered isn't carried (the first builder at 17:26).
    On lamp-4ace on 19 Oct the owner said a line every 6 s, and 5 of the 10
    let out never got their first word out before the next line's Start.
    So the lamp never "spoke", the office's talk before still counted, and
    the owner's lines without the name were held as no conversation. Two
    lines left alone after the call still end it. When none is going on,
    the journal says why, without words: `request N: no conversation going
    on: it was last heard saying words S s ago, it was last called C s ago,
    K lines left alone since it last spoke` ("it was never …" when never).
  - **in a busy room, this route asks more than Live** (#1712, the first
    builder at 18:57). In ten minutes of a busy office on lamp-4ace, 15
    lines were let out as the conversation going on, and about 10 of the 15
    answers went to office talk: DeepSeek declines far less than Gemini.
    So in a busy room:
    - a conversation goes on only 15 s from the end of the lamp's own last
      words (`BUSY_GOING_ON_FOR`) or the person's call. The end is when the
      words' audio, sent packet by packet, will have played, each packet
      after the one before (`Did::Said`); the thread keeps only when the
      answer began (the first builder at 19:21: "And what about tomorrow?"
      came 18 s after the weather line, after a long answer, and was held);
    - a line let out only as it going on must be a question, three words or
      more, or an answer to the lamp's own question, such as the hand-over's
      "yes" (`BUSY_SHORT`);
    - a line let out only by someone turned to the lamp must ask it
      something (`BUSY_TURNED`);
    - its name, its words calling it, a tap, or being said close to it let
      a line out as before.

    In a quiet room it is as on Live, and one more thing: a conversation is
    also going on for 20 s after the end of the lamp's words
    (`AFTER_THE_LAMPS_WORDS`, follows-on's `WITHIN_US`), whenever the answer
    began (`worker::going_on_from`). In a quiet room only: a busy room still
    goes on only by the thread's window or the call, and then 15 s after
    the words (`went_on_told`). With the 20 s there too, an answer of 30 s
    or more let office talk out in the 15 s after it (the first builder's
    agent on #2032, 03:09). The thread's 45 s count from an
    answer's start, so a story told for 40 s or more used them up while it
    was told. On lamp-4ace on 20 Oct at 01:47 and 01:49, two hand-overs said
    as stories ended ("Hey Lamp, ask Claude Code to write a two-line
    poem…", "… ask Codex to write a haiku…") lost "Hey Lamp" to the lamp's
    own voice and were held as no conversation, "it was last heard saying
    words, to their end, 0 s ago" (the first builder at 01:50 and 02:04 on
    #1712). The journal's "no conversation going on" line says when the
    lamp was "last heard saying words, to their end," and adds ", in a busy
    room, where it goes on 15 s".
  - **the journal names the sign that let a line out** (#1712, the first
    builder at 17:49). With `--spoken-to`, the rule's first reason can be
    one that lets nothing out by itself, the session trusted or the room
    not known to be busy. The line goes out only with one more sign, and
    `its answer is played:` names that sign (`Attention::sign`): its name
    heard, a tap, its words calling the lamp, someone turned to it, said
    close to it, or a conversation going on. On lamp-4ace on 19 Oct, the
    office's short lines while the owner sat facing the lamp were journaled
    as let out by trust; the turned face had let them out. The face rule
    stays as Live has it (the first builder at 17:53). The model is to
    decline what isn't for it, as Gemini does.

  Not here, as they have no part on this route: the Policy's hold of a line
  going on (`LAMP_GOING_ON_ASKS`, which also ends a busy room sooner once
  answered) and another voice's line (the same-voice check). The busy-room
  battery on lamp-4ace is its check.
- **How to take what it heard** (`worker::SPOKEN_NOTE`, spoken turns
  only, after the character): the ears turn some words into others, names
  and places most of all; a question plainly about the weather, a price, a
  time or a place is answered for the likeliest meaning, here first, with
  a search if needed, and asked back only when it cannot be guessed; greet
  and say goodbye for the time of day; a poem has at least four lines. From
  the first builder's scoring of 19 Oct: "what is the weather like in
  Hanoi" was answered "You asking what the weather's like here today?",
  "Night!" came at 1 pm (the character's example), and a poem had two
  lines. A text is typed, not heard, and is not told it. With a memory
  (`LAMP_MEMORY_SOCKET`), `worker::MEMORY_CALLS_NOTE` follows: to remember
  call `remember` ("saying you will is not keeping it"), to forget
  something they told it call `forget`, with both sides as examples in one
  line: "please forget that" or "forget what I just told you" is a forget,
  "forget it, it doesn't matter" or "never mind" about the conversation is
  not (the reviewer and the first builder on #2007, 01:17 and 01:18: a
  wrong forget deletes what they asked the lamp to keep). Worded only
  against "forget it", the note left "Hey Lamp, please forget that." in
  words on lamp-4ace, nothing forgotten (the first builder at 02:05). For
  an earlier conversation, call `recall`. The memory note
  itself names only `recall`, and on lamp-4ace on 20 Oct DeepSeek answered
  "remember …" and "forget that" in words (#1722, 00:34).
- **The time, as it is asked** (`local_time::told_asked_here`, spoken
  turns and texts): each request is built as it is asked, so it is told
  the time then, exactly and with the words a person says it in, "It is
  7:49 in the evening, Monday 19 October 2026. Said aloud: seven
  forty-nine in the evening.", and to answer the time, the date and "how long
  until" from it. A Live session is told the time it opened and to call
  `the_time` first, which on this route cost a round: on lamp-4ace on
  19 Oct "what time is it?" called it, and that round's first event came
  2100 ms later; with `LAMP_SECOND_FEW_TOOLS`, `the_time` behind
  `more_functions`, two rounds. So `the_time` is here only for another
  place's time (`worker::time_elsewhere`): with `LAMP_TIME_PLACES` it is
  told `worker::TIME_ELSEWHERE` and needs its place; without it, it is
  not given. Whether DeepSeek then answers with no call is not yet shown
  on a lamp. All this is with `LAMP_CLOCK` on; with it off, the request is
  told the time as a Live session without it is, roughly
  (`local_time::told`), as before (the first builder on #1921).
- **The lamp's place** (`LAMP_PLACE`, "Hanoi, Vietnam"): told beside the
  time, "You are in Hanoi, Vietnam. When someone asks about the weather,
  the news or anything nearby without naming a place, they mean Hanoi,
  Vietnam.", and with
  `LAMP_TIME_PLACES` the time is said as that place's ("Here in …"), not
  the zone's city. A zone names one city for a whole country: lamp-4ace,
  in Hanoi, keeps `Asia/Ho_Chi_Minh`. Letters in any script, digits,
  spaces, `,` `-` `.` `'`, at most 60 bytes; anything else is no place. It
  reaches the second route's worker alone, whose journal says `lamp-voice
  second: the lamp's place is … (LAMP_PLACE)`.
- **A line let out only as the conversation going on** (with
  `LAMP_SPOKEN_TO` and `not_for_me` offered, `LAMP_TOOLS=on`): no name,
  nobody turned to the lamp, not said close to it, no answer to its own
  question (`attention::Attention::played_on_the_conversation_alone`). It is
  asked with `worker::CONVERSATION_ALONE` beside its words: it may be
  people's talk, and if it is plainly said to someone else or a remark to
  the room, the model calls `not_for_me`; a follow-up is the model's. The
  journal adds `, only as the conversation going on: the model is told so`
  to `its answer is played`. Gemini Live hears who is talking to whom; this
  route's model reads one line of words, and on lamp-4ace on 19 Oct it
  answered colleagues' talk after a named request six times, where Live
  answered it once. Each answer kept the conversation going, and with no
  line left unanswered the room never became busy. In such a line the
  schedule's `set_timer`, `set_reminder` and `cancel_reminder`
  (`policy_holds::SCHEDULE_HELD`) are not run (`Engine::holding_calls`,
  `worker::Heeding::holds_call`): the model is told `policy_holds::HELD`,
  as the Live route tells it of a call the Policy holds, and the journal
  says `request N asked for set_timer: held, nothing shows it was said to
  the lamp`. At 13:03 on 19 Oct "Yes, give me five minutes", said to
  someone else, set a five-minute timer. And such a line of one or two
  words, with no question in it and not a short word said to the lamp
  (`not_speech::short_word`: "louder", "thanks", "stop"), is held before
  the model sees it (`worker::LONE_WORDS`): "What's the price of gold?"
  heard as "Surprise" was answered "Oh! I do like a surprise." "And
  Paris?" is a question and goes on.
- **Said by name, or perhaps not for the lamp** (`worker::call_note`,
  `Engine::noting_whether_called`). Each spoken line gets one note beside
  its words. A line whose words or the lamp's name heard in its sound
  called the lamp (`Attention::called_for_itself`) gets "this was said to
  you by name: answer it" (`BY_NAME_NOTE`). Every other line, a face alone
  included, gets the decline note (`DECLINE_NOTE`): much of what reaches it
  is not said to it, and a line plainly people talking to each other, a
  remark to the room, a call, a video or a noise is left alone with
  `not_for_me`, which is right, not rude. A line let out by a conversation
  alone has `CONVERSATION_ALONE` in its place. Without the rule, the line's
  own words decide (`called_by_name`). The first builder on #1712: at 17:26
  DeepSeek answered what the rule let out where Gemini declined, and at
  17:56 it declined "Lamp, what time is it?". The count on played lines is
  `called not_for_me` against `first answer audio`.
- **A line going on from the last one hears its sound first.** The
  coordinator ends a line at a short pause, so the owner's "Lamp, … what's
  the weather?" came as pieces of about 1.3 s, each a speech-to-text stream
  of its own, and the piece with the name got no words (lamp-4ace, 19 Oct,
  16:37 to 16:45: four such pieces around lamp-name's hits, `results 0
  with words`). The engine keeps the last line's sound, its last 1.5 s
  (`KEPT_SAMPLES`): carried whole, 2 to 4 s of it held each line's words
  back 1 to 2.5 s while Deepgram worked through it (18:56). Only the name
  cut off on its own carries: a piece that came back with no words, or with
  the lamp's name alone (`attention::only_the_name`: "Lamp.", "Hey Lamp,",
  a word written for the name, greetings at most). One whose other words
  came back carries nothing. With `LAMP_SECOND_STT_WARM=on` the name's
  piece comes back as "Lamp", where a socket opened for it heard nothing.
  On lamp-4ace on 20 Oct, "Lamp" alone was held as a short sound and the
  rest for no name: 4 of 6 held, twice (#2002's P1, the first builder at
  01:23). The name's words then begin the next
  line's, so it is judged and asked whole ("Lamp. what time is it?"), and
  its sound is not sent again: `request N goes on from request M: its 1
  words, the lamp's name alone, begin it; its sound is not sent again`. A
  line in the same session that
  the coordinator says continues it, or that begins within 1.5 s of its end
  when it was not answered (`CARRY_AFTER_END`), takes that piece's time,
  so a name lamp-name heard in it counts for the line. Its sound is sent
  again only when lamp-name heard no name there (`Engine::knowing_the_name_heard`,
  `worker::Heeding::name_heard_since`, read from the bus's last hearing):
  sent again, 1.5 s of it still held the words back about 2 s (the first
  builder at 19:21). With the name heard, `request N goes on from request
  M: the name heard in it counts; its sound is not sent again`; without,
  that sound goes into the line's stream first:
  `request N goes on from request M: its <ms> ms of sound go first`. Its
  window then begins where the first piece's did, so a name lamp-name heard
  there counts for it, and the old words are not put before its own as well.
  The sound is let go 2 s after its line ended (`KEEP_AFTER_END`), at the
  next start, and never crosses a session.
- **Words cut short by the next line.** A new line ends the turn in
  flight, by the engine's own start and by the coordinator's newer owner,
  before anyone knows whether the new line is for the lamp. A spoken
  request still being answered, none of its answer heard, is cut short:
  `request N: ended before its answer: a newer line began; its words wait
  for that line's verdict`. If that line is held (no words, a short sound,
  or the rule), its turn answers the words cut short in its place, within
  30 s (`CARRY_WITHIN`): `request M is held, so request N's words, cut
  short by it, are answered in its place`. If it is let out, it is answered
  itself and they are let go. On lamp-4ace on 19 Oct at 15:56 the weather
  and a race result were searched, then ended by the owner's next line,
  which was held, and were never said.
- **The name note** (`LAMP_NAME_NOTE`) is given as on the Live route
  (`worker::Heeding::telling_the_name`): a line the lamp's name began, heard
  by `lamp-name` at `called::LEAST_SCORE` or more no more than
  `called::BEFORE_US` before the line, is asked with the note beside its
  words. With `LAMP_NAME_NOTE_BUSY=face`, in a busy room only with someone
  turned to the lamp or the name heard surely; the journal says why not.
  So is the staff note (`LAMP_STAFF_NOTE`).
- **The look by its own call.** With a full look (`LAMP_EYES_LOOK_FILE` and
  `LAMP_LOOK_FULL`), spoken turns are offered `take_a_look`
  (`second_provider::LOOK_CALL`, `worker::with_look`; texts are not) and
  told "When you would look, call take_a_look" (`worker::LOOK_CALL_NOTE`).
  The call asks the coordinator for the turn's full look (`LookAsked`, as a
  skill's look step does), the lamp says "Let me look." if the model said
  nothing, and the turn ends with no further round; the picture comes in
  the look turn after it. In a turn the lamp wrote itself the look is
  dropped (`LookAsked` with why). The journal says `request N called
  take_a_look: the look is asked of the coordinator`. The look by the
  words "Let me look." stays beside it (`speak_up::asks_to_look`); both
  ask for the same turn's look, which is taken once. The first builder on
  #1712: a phrase rule is easy to trip.
- **Live route only, so not on this route:** the same-voice
  check,
  the head tap
  and `--addressed` for whom it answers, sight and camera facts, and
  `LAMP_MEMORY_RULES`, which reaches Gemini's worker only (`process.rs`).
- **The air (`LAMP_AIR_FACTS=on`, #1712's map item 6):** as on the Live
  route (`crate::air_told`), `air/state` is read every 5 s on the bus
  (`worker::air_task`, a reading older than 30 s says nothing), and once the
  room has felt stuffy for ten minutes a spoken request is told so beside
  its words (`Engine::noting_the_air`, `worker::air_noting`), not in a busy
  room, then not again for three hours. A written turn, the lamp's own,
  never is. The journal: `request N is told the air is stuffy`. Its room
  is the one this route keeps (`room/state`); the room's quiet or busy is
  in each turn's system message with `LAMP_ROOM_FACTS`, as before.
- **`voice/link`** (`LAMP_VOICE_LINK`) is this route's link, Ready from its
  start. As a standby it stays Ready: an outage shows only as failed turns.
  As the only voice it goes down and back with the network (below, "The
  link, when it is the only voice").
- With `--camera-config` the camera worker still starts for Gemini's picture
  context, which nothing reads here. lamp-4ace's unit sets none.

Host tests: `coordinator::second::tests` (the values, the refusals, the
workers each value starts) and `coordinator::tests::second_route`
(`the_second_route_alone_answers_a_spoken_request_and_no_gemini_worker_starts`:
nothing reaches a Gemini worker, the second route is sent what Gemini would
have been, and the speaker's authorities, the ring's phases and the kinds
and fields of every record are a Gemini run's;
`the_second_route_alone_exiting_ends_the_voice_as_gemini_does`), the startup
poll (`startup_takes_the_second_route_alone_as_the_provider`), the early
refusal (`run_serve_refuses_a_wrong_route_before_anything_starts`: no session
folder made), `run_start`'s route
(`run_start_names_the_second_route_alone_and_nothing_else`) and its import
(voice-eval's `an_imported_attempt_says_when_the_second_route_answered_alone`),
the engine (`the_only_voice_never_says_it_sounds_different`,
`the_only_voice_says_a_due_reminder_with_the_live_routes_note_and_no_tools`)
and the input taken late
(`the_second_routes_input_takes_a_message_late_only_as_the_only_voice`). Not
shown on a lamp.

## Gemini's voice and text model, called directly

The owner, 19 October (06:4x): ElevenLabs costs too much; use Gemini's own
speech, "just not gemini live", at Live's latency, "the same latency or
almost the same", and with Live's voice. The research of that morning
picked Gemini 3.8 Flash-Lite TTS in Kore (half ElevenLabs Flash's price a
minute of speech, rated above it; its time to first audio is not published,
and developer reports put it at 1 to 3 s) and Gemini 3.6 Flash with thinking
at its floor as the text model (0.80 to 0.83 s to the first token,
published, from the US). Two switches, each off unless set, in the voice
service's environment, on this route standing by or alone:

- **`LAMP_SECOND_SPEECH=gemini`:** speech is Gemini's speech model
  (`gemini_speech_model`, absent `gemini-3.8-flash-lite-tts`), in the Live
  route's own voice (`provider.json`'s `voice`: Kore on the lamps, as the
  product's `deploy/product/settings/voice-provider.json` sets it), one
  request a sentence or first clause, its audio passed on as it comes. The
  journal at start: `lamp-voice second: the voice is Gemini (<model>, voice
  Kore), the lamp's one voice (LAMP_SECOND_SPEECH=gemini)`. The answer's first
  piece goes once six words have come whole (`FIRST_WORDS`,
  `Engine::cutting_first_words`), if no sentence or clause ended sooner,
  and the rest of that sentence follows as one: Gemini takes no streamed
  text, so its first sound otherwise waited for the whole first sentence.
  Other voices keep whole sentences and clauses. Each sentence's
  time to its first audio is journaled, with its length and how many were
  asked beside it, never its words: `lamp-voice second: Gemini's speech
  began in <ms> ms (<n> characters, <k> more asked and not begun)`. As a
  spoken line begins, the connection the answer's first sentence takes is
  opened ahead with a `GET` of the model, unless one was used in the last
  20 s (`GeminiTts::warm`, `WARM_AFTER`): `lamp-voice second: Gemini's
  speech connection opened ahead in <ms> ms (HTTP <status>)`. The probe
  from lamp-4ace on 19 Oct put a new connection at 50 to 500 ms more to
  the first audio than one kept (first run against the next two). On
  lamp-4ace at 13:03 on 19 Oct, through the relay, the chat's first event
  to the first audio was 1.89 s against about 0.65 s with ElevenLabs: these
  lines are to tell where it goes.
- **`LAMP_SECOND_CHAT=gemini`:** the text model is Gemini
  (`gemini_chat_model`, absent `gemini-3.6-flash`) through Google's
  OpenAI-compatible endpoint, called directly, not through the relay, with
  thinking `minimal`, the lamp's tools (and their thought signatures given
  back) and a look's picture. The journal at start: `lamp-voice second: the
  text model is Gemini (<model>, thinking minimal), called directly
  (LAMP_SECOND_CHAT=gemini)`.

Either is refused at start with a value it does not take (unset or empty
is off; `LAMP_SECOND_CHAT` also takes `openrouter` and `deepseek`, below):
`lamp-voice: LAMP_SECOND_SPEECH is gemini, elevenlabs, openai or nothing,
not "…": the voice does not start`. Both need `LAMP_VOICE_SECOND_CONFIG`,
and do nothing without it.

**One voice per lamp** (the owner, 19 Oct: "some lamps ship with Gemini,
some with ElevenLabs; it's a config, but it should be one voice per lamp").
`LAMP_SECOND_SPEECH` names the lamp's one voice: `gemini`, `elevenlabs` or
`openai`; unset, `second.json`'s `tts_provider`. Every sentence, the wait's
sounds, the down line, reminders and the skills' say (a written turn) are
that voice's alone (`services::in_one_voice`). A provider chosen over the
configured one takes its own default voice, since `tts_voice` names one of
the configured provider's, and the chosen voice's model is
`LAMP_SECOND_SPEECH_MODEL`'s. No sentence goes to another voice: one the
voice fails to start (a refusal, a quota, a broken connection, an answer
with no sound, or no first sound within 4 s, `START_WITHIN`) is asked of it
once more, `lamp-voice second: <voice> failed to start a sentence (…);
asked once more`; failing again it is not said, `<voice> failed again (…);
the sentence is not said, and the turn goes on`, and the turn goes on with
the next sentence. Before #1844's C the second failure failed the whole
turn, against what this said, and a host that took the connection and never
answered cost the connection's 10 s and its response's, twice: now at most
8 s a sentence. A sentence begun and then lost fails as any does. The journal at start names the voice: `the voice is ElevenLabs,
model eleven_v4_turbo, the lamp's one voice`.

**The key.** The voice unit's credential `gemini.key`
(`LoadCredential=gemini.key:/etc/lampOS/voice/gemini.key`), read as
OpenRouter's and DeepSeek's are (below, `second_config::GEMINI_CREDENTIAL`).
One there but not private, or a link, is refused, never passed over.
Without it, the Live route's own key, but only when `provider.json` goes
to Google directly (its endpoint is Google's own and its `authentication`
the API key: `ProviderConfig::google_credential_file`). A relay's key is
never sent to Google. On the lamps `provider.json` goes through the company
relay, so a switch with no `gemini.key` keeps the second worker from
starting, and so the voice: `lamp-voice: LAMP_SECOND_CHAT=gemini and
LAMP_SECOND_SPEECH=gemini call Google directly and need a Google API key:
the voice unit's credential gemini.key (LoadCredential=gemini.key:<file>);
the Live route's key goes to a relay and is not given to Google`. Until 19
Oct the key was `gemini_credential_file` in the second route's
configuration; a configuration that still names it is refused by name, so
it is moved, not left behind. No key is written in a log, a test or this
page.

**A search begun from the words.** With a search on, a spoken request that
plainly asks for something new (news, a score or result, a price or rate,
opening hours, the weather: a question, or "tell me", "check", "look up")
starts its search from the words heard, the lamp's name left off and the
month and year after them, as its chat begins (`worker::early_query`,
`Engine::hearing_words`). The request's first `web_search` takes that
search's answer, whatever query the model gives (`worker::Early`); a
later one runs as asked. The journal says `request N: a search begins
from the words heard (<n> characters)` and `request N web_search answered
by the search begun <ms> ms before the call`. So the search runs while
the model's first round does, which took 0.74 to 0.92 s to its first
event on lamp-4ace on 19 Oct; the search test's first audio was 2.6 to
4.4 s.

**The weather, asked ahead** (`LAMP_SECOND_WEATHER=on`, off unless set;
the first builder on #1712, 15:18). The lamp's place (`LAMP_PLACE`, or else
the place its zone names) is looked up once by its first part at
Open-Meteo's geocoding (`geocoding-api.open-meteo.com`), then the weather
now, today and tomorrow is asked of `api.open-meteo.com` every 15 minutes,
with no key (`net::weather`). While it is under an hour old the model is
told it beside the time: "The weather where you are, as of N minutes ago:
In Hanoi now: 31°C, partly cloudy, feels like 36°C, humidity 70%. Today
26 to 33°C, … Use it when someone asks about the weather here; never bring
it up unasked." So "what's the weather" needs no search. Only the place's
name and its coordinates leave the lamp. The journal: `the weather in
Hanoi, Vietnam is asked of Open-Meteo every 15 minutes`, `the weather in
… came in N ms` on the first and after a failure, and a failure once until
the next success. It reaches the second route's worker alone
(`process.rs`).

**A dated search.** The search function's description (`net::search::DESCRIPTION`)
asks for the month and year the model was told in a query for news,
results and prices: on lamp-4ace on 19 Oct "Who won the last Formula One
race?" found last season's.

**Through the company relay.** The relay serves Gemini's speech at
`https://campaign-api.autonomous.ai/api/v1/ai/v1/google-search/v1beta`
(V1's path, `hal/drivers/voice/tts/gemini.py`), with the relay's key in
`x-goog-api-key` as this route sends it: then `gemini.key` is the relay's
key (on lamp-4ace `LoadCredential=gemini.key:/etc/lampOS/voice/second-llm.key`),
never a Google key. From lamp-4ace on 19 October at 12:1x (the first
builder, an 82-character sentence, 3 runs each, all HTTP 200 with
`audio/l16; rate=24000`): `gemini-3.8-flash-lite-tts` first audio 840,
571 and 793 ms, `gemini-3.8-flash-tts` 1443, 948 and 1212 ms.
`LAMP_SECOND_GEMINI_BASE_URL` and `LAMP_SECOND_SPEECH_MODEL` in the voice
service's environment go over `gemini_base_url` and
`gemini_speech_model`, so a battery's unit sets them without touching
`second.json` (unset or empty, the file's stay; refused at the worker's
start as the file's are, naming the variable). They reach the second
route's worker alone (`process.rs`), whose journal then says `lamp-voice
second: Gemini at <url>, voice <model>`.

**The bar**, measured on lamp-4ace on 19 October with Gemini Live, from the
end-of-speech decision to the first sound: chat lines median 1.70 to
1.81 s, tool turns 2.02 to 2.21 s, looks 1.1 to 1.3 s, all lines median
2.0 s and p90 2.6 to 2.9 s. Not yet measured on this route with these
switches; the probe below is the first step.

**Ready, without keys:** a fast host of DeepSeek's weights (Baseten) is an
OpenAI-compatible chat, set by `llm_base_url`, `llm_model`,
`llm_credential_file` and `llm_extra`, and a look's picture goes to it as an
image part, as it goes to the relay. Exa is `LAMP_SECOND_SEARCH=exa`
(`search_url`, "What each client speaks"). Neither was run against its
service.

## OpenRouter and DeepSeek

The owner, 19 October (08:5x and 09:0x): test DeepSeek's models and its web
search, through OpenRouter with the key the Policy already uses for Jev, and
on DeepSeek's own API. Off unless set, on this route standing by or alone:

- **`LAMP_SECOND_CHAT=openrouter`:** the text model through OpenRouter,
  `openrouter_model` (absent: `deepseek/deepseek-v4.1-flash-1017`, the
  dated 17 October snapshot the owner picked, pinned by its slug in
  OpenRouter's public `GET /api/v1/models` of 19 Oct, never the moving
  alias `~deepseek/deepseek-flash-latest`; the second candidate is
  `google/gemini-3.6-flash`). The journal at start: `lamp-voice second: the
  text model is <slug> through OpenRouter, the fastest provider first,
  reasoning <effort> (LAMP_SECOND_CHAT=openrouter)`.
- **`LAMP_SECOND_CHAT=deepseek`:** the text model on DeepSeek's own API,
  `deepseek_model` (absent: `deepseek-flash`, which DeepSeek serves with
  DeepSeek-V4.1-Flash-1017; its docs give no dated name to ask for), thinking
  off. `deepseek-v4-pro` (V4-Pro-0813, no pictures) is a probe candidate:
  set, it is never sent a look's picture.
- **`LAMP_SECOND_SEARCH`:** `deepseek` (DeepSeek's Search & Fetch as
  `web_search` and `fetch_page`, with any chat), `openrouter` (OpenRouter's
  web plugin on Exa, with `LAMP_SECOND_CHAT=openrouter` only, refused at
  start otherwise) or `exa` (`search_url`). The journal at start:
  `lamp-voice second: the model may search the web (web_search, fetch_page;
  LAMP_SECOND_SEARCH)`.

Any other value of either is refused at start, and the voice does not start:
`lamp-voice: LAMP_SECOND_CHAT is gemini, openrouter, deepseek or nothing,
not "…": the voice does not start`.

**The keys** are the voice unit's systemd credentials, never in a
configuration file, a log, a test or this page. The unit loads each with
`LoadCredential=` (the files stay root's alone; systemd gives the service
its own copy), the coordinator passes the unit's `CREDENTIALS_DIRECTORY` to
the second worker alone (`process.rs`, an absolute path only), and the
worker reads `openrouter.key`, `deepseek.key` or `gemini.key` there as it
reads its other keys (`second_config::unit_credential`). A switch whose credential is
missing keeps the worker, and so the voice, from starting, naming the
credential: `lamp-voice: the voice unit's credential openrouter.key: …`. A
drop-in for lamp-4ace's unit:

```ini
[Service]
LoadCredential=openrouter.key:/etc/lampOS/policy/jev.key
LoadCredential=deepseek.key:/etc/lampOS/voice/deepseek.key
LoadCredential=gemini.key:/etc/lampOS/voice/gemini.key
Environment=LAMP_SECOND_CHAT=deepseek
Environment=LAMP_SECOND_SEARCH=deepseek
Environment=LAMP_SECOND_SPEECH=gemini
```

OpenRouter's key is the one the Policy already uses for Jev (its unit
loads `/etc/lampOS/policy/jev.key` as `jev.key`); DeepSeek's and Gemini's
are put on the lamp by the owner. `LoadCredential=` of a file that isn't
there keeps the unit from starting, so the Gemini line goes in only once
its key is placed.

### Timing the candidates (`probe-cascade`)

`lamp-voice probe-cascade PROVIDER_CONFIG SECOND_CONFIG [RUNS] [--look JPEG]`
(`second_provider/probe.rs`; with `--wire`, the next section) times the
route's candidates from where it
runs, with the character of `provider.json` and one fixed question, and
with `--look` once more with that JPEG (at most 184 KiB) as the question's
picture, to each model that takes one:

- **chat:** the configured chat; Gemini's (with a Google key); through
  OpenRouter `deepseek/deepseek-v4.1-flash-1017` (reasoning `none`) and
  `google/gemini-3.6-flash` (`minimal`), and `openrouter_model` if it is
  another; on DeepSeek's own API each model its `GET /models` lists (at most
  eight; `deepseek-flash` and `deepseek-v4-pro` if the list fails), thinking
  off and on;
- **search:** OpenRouter's web plugin on its first candidate (to the first
  event and the first chunk with the search's results) and DeepSeek's
  search (to its answer, with the number of results);
- **speech:** the configured voice and Gemini's, one fixed sentence each.

No tools are declared. Five runs unless said, at most 50. One line a
candidate a run, milliseconds and counts only (never the words asked,
heard or said, a key or a URL):

- chat: `first_ms` (the first text or call), `clause_ms` (the first clause
  or sentence the route would say, `Sentences`), `done_ms`, and with a
  search `searched_ms`;
- search: `first_result_ms`, `results`;
- speech: `first_audio_ms`, `done_ms`, `audio_ms` (the audio's length);

then each candidate's median and 90th percentile. A candidate without its
key is left out, saying why. Each time runs from its request's start; it is
not the lamp's end-of-speech-to-sound time, which also has speech to text's
last words and the playout hold. Any other OpenAI-compatible endpoint is
timed by giving it a configuration of its own as `SECOND_CONFIG`. It reads
the credential files the configurations name and the unit's credentials,
so on a lamp it runs under systemd with them:

```sh
systemd-run --pty --wait \
  -p LoadCredential=openrouter.key:/etc/lampOS/policy/jev.key \
  -p LoadCredential=deepseek.key:/etc/lampOS/voice/deepseek.key \
  -p LoadCredential=gemini.key:/etc/lampOS/voice/gemini.key \
  /opt/lampOS/voice/current/lamp-voice probe-cascade \
  /etc/lampOS/voice/provider.json /etc/lampOS/voice/second.json 20 --look /run/lampos-eyes/sight.jpg
```

### Where the relay's time goes (`probe-cascade --wire`)

`lamp-voice probe-cascade PROVIDER_CONFIG SECOND_CONFIG [RUNS] --wire`
(`second_provider/probe/wire.rs`) times a chat request step by step, from
the lamp's side, for DeepSeek through the relay and on its own API (#1934;
the first builder on #1722, 00:56). On lamp-4ace the relay's answers began
about 0.6 s later than DeepSeek's own. Each request is the route's own as
`Chat` builds it (`hosted.deepseek_model`, thinking off), made by hand on a
connection of its own:

- **dns, tcp, tls:** the name looked up, the TCP connect and the TLS
  handshake, each on its own;
- **send:** the request handed to the kernel, its body whole. On a large
  request part of the upload may still be going, and is counted in the
  first byte;
- **first byte:** from then to the response's first;
- **events:** from the same point to the stream's first and second `data:`
  events, the gap between them (`gap_ms`), and to its first words or call
  (`words_ms`);
- **done:** to the stream's end.

Each run asks each target two requests:
- **lamp:** a spoken turn's size: the character, a turn's changing part,
  and every function a turn declares, the actions part's read from
  `LAMP_ACTIONS_DIR` as `probe-tools` reads them. The route's own was
  31 KB with 31 functions on lamp-4ace; the probe prints its own `bytes`.
- **small:** one short line and no functions.

Two things it is not (the first builder's agent on #2024):
- **The lamp's request is smaller than a real turn's.** It has no memory
  note, facts, time line or the route's notes. Read its `bytes=` beside the
  route's own `chat request` journal line for the same lamp, so a size
  effect isn't read short.
- **Every request is on a new connection.** It pays `tcp_ms` and `tls_ms`
  each time, where the route reuses a kept one. So `done_ms` is the
  request's, not a turn's. The difference line keeps `first_byte` apart
  from the connect.

How to read it:
- **A first byte late by the same on both sizes:** the network or the
  relay's own wait.
- **Late on the lamp's only:** the request's size.
- **A late first byte with the first two events close together:** the
  relay holding the stream back.
- **A first byte on time and a late first event:** the relay answering
  its headers at once and holding the stream back.

The relay is `deepseek_base_url` (or `LAMP_SECOND_DEEPSEEK_BASE_URL`) with
the route's credential. DeepSeek's own API takes the unit's `deepseek.key`.
A target without its URL or key is left out, saying so.

**On one kept connection** (`--wire --kept`, the first builder on #1934,
20 Oct 04:16): each target and size is asked three requests on one
connection (`Arm`):
- **first:** it makes the connection, so it pays all the steps above.
- **straight:** right after, with no connect, TLS or slow start.
- **idle:** after 5 s with nothing sent (`IDLE`). This is what a real turn
  pays on the route's kept connection. Unless
  `net.ipv4.tcp_slow_start_after_idle` is 0, the kernel restarts slow start
  after idle, so a request over about 13 KB waits a round trip again.

Each request has its own changing part, as turns do. An answer is read to
its body's end, so the connection can carry the next request. Only a 200 is
timed. A refused answer, and every arm after an answer that doesn't keep the
connection, is counted `closed`, since a real turn would then pay a new
connection (the reviewer on #2044).

The lines are `wire kept target=… size=… run=… arm=…`, then each arm's
medians. `wire kept idle-less-straight target=… size=…` is what restarting
slow start costs a turn, and `wire kept relay-less-direct size=… arm=…` is
the relay's share on a kept connection.

**The output:** one line per request:
`wire target=relay size=lamp run=1 bytes=… status=200 dns_ms=… tcp_ms=…
tls_ms=… send_ms=… first_byte_ms=… first_event_ms=… second_event_ms=…
gap_ms=… words_ms=… done_ms=… events=…`. Then each target's and size's
medians (and the 90th percentile of the first byte, the first event and
the words), and,
with both targets, the relay's medians less DeepSeek's own, step by step
(`wire relay-less-direct size=lamp tcp_ms=+… tls_ms=+… …`).

A failed step is told by its name and kind only (`tcp: TimedOut`). Like
the rest of the probe it prints counts and milliseconds only, never the
words asked or answered, a key, a host or a URL. Five runs unless said, at
most 50. `systemd-run` doesn't carry the voice unit's environment, so the
relay's URL is given as lamp-4ace's unit has it. Without it, and with no
`deepseek_base_url` in `second.json`, the relay is left out and only
DeepSeek's own API is timed (the reviewer on #2024):

```sh
systemd-run --pty --wait \
  -p LoadCredential=deepseek.key:/etc/lampOS/voice/deepseek.key \
  -E LAMP_SECOND_DEEPSEEK_BASE_URL=https://campaign-api.autonomous.ai/api/v1/ai/v1/deepseek \
  -E LAMP_ACTIONS_DIR=/run/lampos-actions \
  /opt/lampOS/voice/current/lamp-voice probe-cascade \
  /etc/lampOS/voice/provider.json /etc/lampOS/voice/second.json 10 --wire
```

`--wire --kept` takes the same command with `--kept` after `--wire`, at most
20 runs (`MOST_KEPT_RUNS`). Its 10 runs ask 120 requests and wait 5 s 40
times, about 4 minutes.

### How much the prompt cache holds (`probe-cache`)

`lamp-voice probe-cache PROVIDER_CONFIG SECOND_CONFIG [RUNS]` (1 to 20,
three unless said) settles where DeepSeek puts the functions in its prompt,
which `LAMP_SECOND_STEADY_FIRST` depends on (#1712, the first builder at
18:56). It asks DeepSeek's `hosted.deepseek_model`, thinking off, with the
unit's `deepseek.key`. Each run asks each arm twice, 5 s apart (`PAUSE`).
The second request's changing part is not the first's, as the time and the
room change between turns:

- **same:** the whole request twice, unchanged: what the cache holds at
  most;
- **one:** the character, then the changing part, in one system message, as
  today;
- **steady:** the character alone, an exchange, then the changing part as a
  system message, as `LAMP_SECOND_STEADY_FIRST=on` lays it out;
- **steady-bare:** `steady` with no functions, to count them.

The functions are the voice's own, as a spoken turn declares them: the
decline, the time, the memory's, the faces', a mark, setup's walk and the
stop of reports. The actions part's light and volume aren't among them. The
changing part is the probe's own, about 1 KB.

Each arm's line gives both requests' `first_event_ms`, `prompt_tokens` and
`cached_tokens`. Then come the medians of the second requests, the functions'
size (steady less steady-bare), and a verdict:
- **the functions are held with what comes before the changing part:**
  steady's cached tokens reach steady-bare's plus 80% of the functions. The
  switch lets the cache hold the character, the functions and the exchanges.
- **otherwise, they're not held before the changing part.** DeepSeek puts
  them after every system message, so the changing part has to leave the
  system messages for them to be held.
- A third line says when today's layout (one) holds them too: DeepSeek puts
  them first.

It prints counts and milliseconds only. Run it like `probe-cascade`:

```sh
systemd-run --pty --wait \
  -p LoadCredential=deepseek.key:/etc/lampOS/voice/deepseek.key \
  /opt/lampOS/voice/current/lamp-voice probe-cache \
  /etc/lampOS/voice/provider.json /etc/lampOS/voice/second.json 3
```

### Which function each follow-up line calls (`probe-tools`)

`lamp-voice probe-tools PROVIDER_CONFIG SECOND_CONFIG [RUNS]` (1 to 20, 3 by
default; `second_provider/probe/tools.rs`). It was built for one fault the
four-arm test found (the first builder on #1722, 20:22). On lamp-4ace at
20:08 and 09:12, after "make your light a little dimmer" and "a little
dimmer still", DeepSeek answered "Hey Lamp, turn yourself down a bit." with
`light_dimmer`, then "turn yourself back up a little" with `volume_up`: the
lamp ended 20 steps louder. Live calls `volume_down`, then `volume_up`.

- **The conversation:** the character and a turn's changing part, then
  `followups.sh`'s five lines as played (`LINES`): the four above and "Hey
  Lamp, make your light a little brighter." (`light_brighter`), which checks
  that the note doesn't push a light line onto the volume.
  Each line is asked with the lines before it as right history: the right
  call, the actions part's short answer to it (`LAMP_SHORT_DONE=on`, as on
  the lamps), and a word or two. So each line is judged on its own.
- **The functions:** the voice's own, as `probe-cache` declares them, and
  the actions part's from its catalog in `LAMP_ACTIONS_DIR`
  (`/run/lampos-actions`), the way the voice reads them. On a lamp that's
  the light and volume with their installed descriptions. Without them it
  refuses to run.
- **The arms**, on DeepSeek's own API (`hosted.deepseek_model`, thinking
  off, the unit's `deepseek.key`):
  - `today`: every function;
  - `few`: narrowed as `LAMP_SECOND_FEW_TOOLS=on` narrows a spoken turn's;
  - `note`: today's, with one sentence after the changing part (`NOTE`):
    "'Turn yourself up' or 'turn yourself down' always means your voice
    (volume_up, volume_down), whatever was said about your light before."

It prints, for each arm and line, how many requests called each function
first (`none` for an answer in words) and how many were right. Then it names
the arms that were right every time on every line. Counts only, no words.

**Then the memory's nine lines** (`MEMORY_LINES`: `trials.sh forget`'s
four, the first builder on #1722, 00:34: on lamp-4ace "remember my violet
umbrella is in the car" and "forget that" were answered in words, with
every function given, while Live called `remember` and `forget`). In order:
- "Hey Lamp, remember my violet umbrella is in the car." (`remember`);
- "What was the name of that café I mentioned last week?" (counted);
- "Oh, forget it, never mind." and "Never mind, forget I asked.",
  each right when it does **not** call `forget`, asked while the umbrella is
  kept (the reviewer and the first builder on #2007);
- "What do you remember about my umbrella?" (`recall`);
- "Hey Lamp, please forget that." (`forget`), the trial's own line;
- "What about my umbrella?" (counted);
- "Remember my keys are on the hook by the door." (`remember`);
- "Actually, forget what I just told you." (`forget`), straight after it.

The counted lines are right in words or a `recall` that finds nothing. Each
is asked with the ones before as right history, with every function, in
two arms:
- `memory-today`: the spoken character (`SPOKEN_NOTE`), the changing part
  and the memory note as `LAMP_MEMORY_DAYS` tells it (`DAYS_NOTE`, which
  names only `recall`);
- `memory-note`: the same with `worker::MEMORY_CALLS_NOTE`, which names
  `remember`, `forget` and `recall`.

Its verdict names the arms right every time on the judged lines.

**Then the sayings' three lines** (`SAYING_LINES`, the first builder's
01:04 and 02:44 on #1722): the lines of #2001's lamp check
(`second-trio.sh`) that rest on what the model says, each judged on its
words, so a model or wording change that breaks one shows here without the
lamp:
- `asked-again` (#1967): mid-story, the coordinator's written ask-again turn
  (`speak_up::ASK_AGAIN_NOTE`): right is a short question ("Sorry?", at
  most three words), nothing called, nothing of the story;
- `after-the-hum` (#1967, `LAMP_MISHEARD_HUM`): "What's seven times
  eight?" with `misheard_hum::NOTE` beside it, as the route puts a note:
  right is 56 in words, nothing called, the story not taken up again;
- `go-on` (#1986, `LAMP_HOLDING_ANSWER`): after "One moment.", the
  coordinator's written turn (`holding_answer::NOTE`): right is 391 in
  words, nothing called, never another holding phrase. Each number and
  name is matched as a whole word ("56" isn't in "156").

Each is asked with the spoken character and the turn's changing part, with
every function. It prints `right=N/M` per line and a verdict naming any line
not right every time; counts only, never the words. The lost line asked
again (#2001) asks the same chat again, so its check is the engine's tests.

**Then memory's quality** (`probe/quality.rs`, #1693, the first builder at
03:05 on #1722). There are six cases, each a fresh session told what the lamp
keeps as its frozen snapshot (with source ids), under the memory note and
`MEMORY_CALLS_NOTE` as a spoken turn has them:
- **`preference-used`:** "How do I take my coffee?" with "Takes coffee with
  milk and no sugar." kept. It's right with **no call**, and words that say
  "milk" (a whole word).
- **`correction-first`:** "Actually, I take it with milk now." with "Takes
  coffee black." kept. It's right when its first call is `forget` or
  `recall`, never a second `remember`, since nothing replaces a kept thing in
  one call.
- **`correction-kept`:** the same, after a `forget` of id 7 answered. It's
  right when it calls `remember`.
- **`earlier-today`:** "What did we talk about earlier today?" It's right
  with `recall`.
- **`earlier-today-said`:** the same, after a `recall` that found, as it
  answers someone known, theirs marked ("book the dentist for Friday") and
  one of the room's, whose speaker is unknown ("Don't tell Minh, his
  surprise party is on Sunday"). It's right with no further call, and words
  that say "dentist" and neither "party" nor "surprise".
- **`never-mind`:** "Forget it, never mind." with something kept. It's right
  with anything but `forget` or `remember`.

Each case prints `right=N/M`. The bar is the last line: every case right
every time. Counts only, no words. Run it like `probe-cache`, where
lamp-actions runs:

```sh
systemd-run --pty --wait \
  -p LoadCredential=deepseek.key:/etc/lampOS/voice/deepseek.key \
  /opt/lampOS/voice/current/lamp-voice probe-tools \
  /etc/lampOS/voice/provider.json /etc/lampOS/voice/second.json 20
```

## Not done

- **Gemini Terminal** (a wrong key or configuration) still ends the voice
  service as before. The second route covers Unavailable and failed turns
  only.
- A link change in the very first tick of a line, before the line is
  marked `movable`, abandons the line as before.
- **Onboarding writing `second.json`:** waits on the owner's choice of how a
  lamp is set up. Not built.
- **`gemini` and `piper` as `tts_provider`:** refused; Gemini's speech is
  `LAMP_SECOND_SPEECH=gemini` instead (above); `piper` on the device is a
  later piece.
- **The cascade's overlaps** the research named for Live's short-line time
  (ending speech to text early and starting the chat before the lamp is
  sure; warm connections kept open; a search started beside round 1;
  weather in the prompt) are not built. Connections are kept between
  requests only as the HTTP client's pool keeps them.
- **The room on the second route alone** ("Second only", above) is built;
  the busy-room battery on lamp-4ace is its check.
- The services are not probed at startup: the link says Ready and a service
  that cannot be reached fails that turn (`ponytail:` in `worker.rs`).

## Open limits

Reported by the builders:

- **Facts not told:** the worker accepts the Gemini worker's flags, but
  sight and the camera facts are not told to the text model (the air is,
  above). A
  written turn with `look` (the lamp taking a look) sends the eyes'
  newest picture with its words, as an OpenAI image part (a JPEG `data:`
  URL) to this route's `llm_base_url` (the company relay and the model
  behind it, not Gemini; Google's Gemini API with `LAMP_SECOND_CHAT=gemini`), read from `LAMP_EYES_LOOK_FILE` with the Live route's bounds (1 s
  and 64 KiB; with `LAMP_LOOK_FULL`, a full look's still and 184 KiB). The
  picture goes in that turn's first chat round only, never into a round
  after a tool, the thread or a later turn; a picture the chat refuses is
  asked once more with the no-look note, within the look's 5 s bound.
  With no fresh picture, or no look file, the Live route's no-look note
  goes in place of the words (`speak_up::NO_LOOK_NOTE`): the model is told
  it saw nothing, never left to describe a picture it was not given
  (sixth-P2, #1044). With `LAMP_SESSION_FACT` and `LAMP_ROOM_FACTS` on,
  each turn's system message carries the facts a Live session is told, in
  its words, after the memory note and before the thread's recall: the
  session fact when no conversation is going on, and the room as Gemini's
  worker publishes it (`room/state`) when it is known and said in the last
  3 s (`bus_out::room_now`; a worker that has stopped leaves its last room
  kept, however old). Air facts wait for a stuffy spell of ten
  minutes, which this route, used only while Gemini is away, does not
  follow.
- **Short sounds, by words alone:** with `LAMP_SHORT_SOUNDS=quiet-room-checked`
  and `LAMP_ATTENTION` on, a turn whose end is marked as a short sound let
  through is answered only when what was heard is, whole, a short word said
  to the lamp; otherwise it ends after its words, unanswered (#800,
  [not-speech.md](not-speech.md#a-room-sound-let-through-and-answered-800)).
  Being close to the lamp or facing it cannot be seen here, so it does not
  count as it does on the Live route.
- **The rule for whom it answers, without two signs:** a tap on the head
  and whether it was said from close (`--addressed`) are not given to it,
  so a line in a busy room that only those would let through on the Live
  route is held here. Both are off on lamp-4ace. Like the Live route, it
  does not wait for the Policy's attention decision, which holds only
  actions.
- **Deepgram direct uses nova-3 on `/v1/listen`.** V1's default
  (`hal/drivers/voice/stt/deepgram.py`) is `flux-general-en` on Deepgram's v2
  protocol.
- **Simplifications marked `ponytail:`:**
  - sentences: a fixed English abbreviation list and "stop then
    whitespace"; quotes after the stop, ellipses and non-Latin stops are not
    handled;
  - speech: every `[...]` is dropped, where V1 drops only its ElevenLabs
    emotion tags;
  - a model still calling tools after four rounds ends the turn with what
    it has said.

## Verification

Host tests: `second_config::tests` (the fallback, Deepgram chosen by its
file, an inline key refused, an unknown `tts_provider` refused, a readable
credential refused, plain `http` refused); the clients' tests on recorded
bytes and a local server (`net/*`); the engine's (`second_provider/tests.rs`);
the worker's authority and a turn kept in the thread, its file and the
journal (`worker.rs`); the thread reload (`thread::tests`).

The switch: `coordinator::second::tests` (the line's 30 s bound, its
request, its clearing on mute or a closed microphone and once answered or
abandoned, the worker arguments) and `coordinator::tests::second_route`,
the production `conversation()` with peer processes on real local IPC,
stepped on the scenario clock so a busy host cannot age a lease
(lampOS#890): without a route nothing of it is seen and a route standing by leaves
Gemini's packets and the kinds and fields of every record unchanged; a
spent quota moves the whole line and the rest of the lamp sees the same
turn; a link dropped mid-line loses nothing; Gemini's audio after its
failure is never played; the next turn goes back to Gemini once Ready;
spoken over, the second route's answer stops as Gemini's does. The
comparison across runs is of kinds, fields and sequences, not bytes:
times and boots differ from run to run.

Not shown on the host: the real second worker behind the switch (the
tests' second route is a peer). On a lamp: a switch mid-line and Gemini's
return, and the latency targets, which lamp-52e6 did not meet (above).
