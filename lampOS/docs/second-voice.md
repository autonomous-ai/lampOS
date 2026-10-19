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

OpenRouter's and DeepSeek's keys are not in this file: they are the voice
unit's systemd credentials, `openrouter.key` and `deepseek.key` in its
`CREDENTIALS_DIRECTORY` ("OpenRouter and DeepSeek", below).

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
  ended, about 0.25 s later than the answer. If 1 s passes with no
  covering finals, `CloseStream` and up to 15 s in all for its final
  words, as before. `CloseStream` is sent in
  every case to close the socket, but waited on only in that fallback.
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
  and an answer 12 s late is worse than none. A round after a tool ran is
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
  begun within its bound says "I can't think right now; give me a minute."
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
- **Back to Gemini:** silently, at the next idle moment with Gemini Ready,
  or at the start of the next turn if the room never goes quiet. Nothing
  is said about it.

## Same character, memory, thread and tools

`worker::serve`:

- **Character:** the instruction of the Live route's `provider.json`
  (`ProviderConfig::instruction`, the version without a camera), then the
  time and place as a Live session is told them, then the memory note (the
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

The battery's tool counter reads the journal: a call the Live route would
hold for the Policy is said on this route as that route says one it lets
run: `request N asked for X; run after 0 ms: this route holds no call for
the Policy` (`worker::run_after`). This route holds none.

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

- **Its exit ends the voice**, as Gemini's does (`coordinator::mandatory`):
  it is the only voice. A standby's exit is still dropped, and Gemini goes
  on.
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
  lines. A text is typed, not heard, and is not told it.
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
  `LAMP_CUT_IN_TOLD`, `LAMP_FOLLOWS_ON`, `LAMP_GOING_ON_ASKS`, the head tap
  and `--addressed` for whom it answers, sight, camera and air facts, and
  `LAMP_MEMORY_RULES`, which reaches Gemini's worker only (`process.rs`).
- **`voice/link`** (`LAMP_VOICE_LINK`) is this route's link, Ready from its
  start: a relay outage shows only as failed turns.
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
  request a sentence or first clause, its audio passed on as it comes. A
  sentence Gemini cannot start (a refusal, a quota, a broken connection, or
  an answer with no sound) is said by the configured voice
  (`tts_provider`, ElevenLabs on lamp-4ace): `lamp-voice second: Gemini's
  speech failed (…); ElevenLabs says the sentence`. A sentence Gemini began
  and then lost fails as any voice's does. The journal at start: `lamp-voice
  second: speech is Gemini's (<model>, voice Kore), ElevenLabs for a
  sentence it cannot start (LAMP_SECOND_SPEECH=gemini)`. The answer's first
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
`lamp-voice: LAMP_SECOND_SPEECH is gemini or nothing, not "…": the voice
does not start`. Both need `LAMP_VOICE_SECOND_CONFIG`, and do nothing
without it.

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
(`second_provider/probe.rs`) times the route's candidates from where it
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
  sight, the camera and air facts are not told to the text model. A
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
