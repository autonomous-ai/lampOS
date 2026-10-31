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
| `stt_base_url` | no | the company's speech-to-text proxy; empty or absent: `llm_base_url`. The voice unit's `LAMP_SECOND_LISTENING_BASE_URL` goes over it (#2882) |
| `stt_credential_file` | no | its key; empty or absent: `llm_credential_file` |
| `stt_language` | no | sent as `language`; absent: `en` |
| `deepgram_credential_file` | no | when set, speech to text goes straight to Deepgram with this key, and the two `stt_*` above are not used |
| `tts_base_url` | no | empty or absent: `llm_base_url`. The voice unit's `LAMP_SECOND_SPEECH_BASE_URL` goes over it (#2882) |
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
| `deepseek_base_url` | no | the relay `LAMP_SECOND_CHAT=deepseek` and `LAMP_SECOND_SEARCH=deepseek` go through instead of DeepSeek's own API (#1934): `<base>/chat/completions`, `<base>/v1/tools/search` and `/v1/tools/fetch`, with the relay's credential (`llm_credential_file`) as the bearer, and no `deepseek.key`; absent: the company relay's own DeepSeek (`llm_base_url` on `campaign-api.autonomous.ai` gives `…/api/v1/ai/v1/deepseek`, #2978) when `llm_base_url` is on the company relay, else DeepSeek's own API with `deepseek.key`. `LAMP_SECOND_DEEPSEEK_BASE_URL` in the unit's environment goes over it. The product unit no longer sets that variable, so the relay's routes (`LAMP_RELAY_ROUTES`) may take the chat to a nearer address (#2882) |

With `LAMP_RELAY_ROUTES=on` (#2882) the second worker asks the relay which
of its services this lamp should reach at a nearer address:
`GET` of `relay/routes` under `llm_base_url`'s API base (`…/api/v1/ai/v1/relay/routes`
on the company relay) with the route's key, answered
`{"status":1,"data":{"chat":…,"speech":…,"listening":…,"gemini":…,"ttl_seconds":600}}`.
Each address is what the matching variable takes (`chat` as
`LAMP_SECOND_DEEPSEEK_BASE_URL`, ending `/deepseek`; `speech` as
`LAMP_SECOND_SPEECH_BASE_URL`; `listening` as
`LAMP_SECOND_LISTENING_BASE_URL`; `gemini` as `LAMP_SECOND_GEMINI_BASE_URL`,
the route's own Gemini, its chat and its voice, never Live's) and is checked as the file's URLs are; a
route on a host other than the company's is a complete API base, as those
settings are (`/v1` is added only to `campaign-api.autonomous.ai`'s `/ai`).
`null`, an address not to use, or a 404 is the service's own address; a
401 or any other failure keeps what was last read, and is asked again
within a minute. It is asked at start, every `ttl_seconds` (60 to 3600)
and as the link comes back, in the
background: a turn never waits for it. A turn's chat and speech take the
routes as they stood when it began and keep them to its end; listening
takes the latest at each utterance. The unit's own address wins: a service
whose variable is given is never routed, nor a chat that is not DeepSeek
with the relay's key, Gemini's voice, Deepgram's own listening, or a voice
or listening with another service's key from `second.json`; the route's
Gemini only when it already goes to the relay with the relay's key, never a
Google key or Google's own address. A near
service that fails on the link, times out, answers a 5xx or a 401 goes to
its own address for the rest of the turn (the chat's request sent again
there, the sentence said in the voice at its own, listening opened once
more at its own), journaled `route=fallback service=… why=…`, and that
service stays at its own address, in later turns and utterances too,
until the routes are next read, so a near host that never answers costs
one slow turn, not every one; the journal names which services are near,
never an address.

A person's own DeepSeek endpoint and key and Deepgram key, with
`LAMP_OWN_KEYS=on`, go over the relay and over `deepgram_credential_file`
at start: the voice unit's credentials `own_deepseek.key`,
`own_deepseek.url` and `own_deepgram.key`, never fields here
([own-keys.md](own-keys.md), #2094).

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
- A service that refuses a request (any status but 2xx) is named in the
  journal by its codes alone, read from the first 4 KiB of its answer
  within 1 s: Google's `error.status` and `error.details[].reason`,
  OpenAI's `error.code` and `error.type` (DeepSeek and OpenRouter answer
  in that form), the first item's when the answer is an array (Google's
  OpenAI-compatible endpoint answers `[{"error": …}]`). A value counts only if it reads as a code: one case,
  letters, digits and `_`, `.` or `-`, at most 40 characters and at most
  4 digits (a key in one case, as DeepSeek's and Deepgram's are, has
  more); at most four. `lamp-voice second: a service answered HTTP 400 (INVALID_ARGUMENT,
  API_KEY_INVALID)`, or `… HTTP 502, naming no code`. The caller's own
  line follows, as before (`… failed: service answered HTTP 400`). It is
  for telling a key from a model or a request when Gemini's chat answered
  400 ten times out of ten on 21 Oct.
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

## The owner's language (#4473 (a), the voice side)

The lamp speaks English, Vietnamese, Japanese, Chinese or Korean, as the
owner chooses (`crates/voice/src/language.rs`). The setting is one line,
`en`, `vi`, `ja`, `zh` or `ko`, in the file `LAMP_LANGUAGE_FILE` names; the
product's unit names `/etc/lampOS/setup/language`. Each process reads it as
it starts, so a change takes effect at the voice's next start. Writing it
from the app (`lamp-cloud`, root's change helper) is not built yet: its
contract is agreed on #4473 and #4474 first.

**English is unchanged.** No variable, no file, an empty file, `en` or a
value this build doesn't know (said in the journal) is English: the
speech-to-text query, the character, the voice and its model are
exactly what they were.

For `vi`, `ja`, `zh` or `ko`:

| Part | What changes | Where |
|---|---|---|
| Speech to text | `language` is the code, over `stt_language`; Flux hears English only, so Nova-3 hears it (`nova-3-general`, as for `vi` above) | `SecondConfig::with_environment` |
| The character | "Always speak English" becomes "Always speak Vietnamese", "Japanese", "Mandarin Chinese, in simplified characters" or "Korean"; a character without the rule gets it at its end. The call name stays "Lamp" | `ProviderConfig::instruction` |
| The voice (ElevenLabs only) | `eleven_flash_v2_5`, over `LAMP_SECOND_SPEECH_MODEL` (`eleven_v4_turbo`), with V1's first voice for the language (`system/domain/device.go`): Ngan (vi), Shizuka (ja), Amy (zh). V1 had no Korean voice: Korean keeps the lamp's own. Speech tags are then off, as for any model that does not read them | `SecondConfig::with_environment` |
| English-only gates | #926: a cut-in with no English word is no fragment in any of them; Vietnamese, written in the Latin script, still asks again for a turn heard in another script, while Japanese, Chinese and Korean have no `--ask-unclear` (their script is the conversation's own). The stop-word stream: Vietnamese keeps it, hearing in Vietnamese with #2939's Vietnamese backchannels; Japanese, Chinese and Korean, which it has no words for, have none | `not_speech::unclear_arguments_from_environment`, `not_speech::unclear_in_conversation`, `stop_words::listen`, the coordinator's stop stream |
| The lamp's name | heard as English hears it, unchanged, and also as the language's speech to text writes it (#4473: on lamp-52e6 on 30 Oct, with `ja`, "Hey Lamp. 今何時ですか？" came back as 「エイランク、今何時ですか。」 and was not answered). Each clause (split at any punctuation, 、 and 。 too) is read past greetings at its start; the name itself counts opening a clause, a stand-in (also an everyday word) only set off, alone in its clause past any greetings (a greeting alone is not enough: "Ê làm bài tập chưa?", 「エイランクだね。」 are none). `ja`: greetings ヘイ, エイ, a (Latin, 「Aランク。」 on lamp-52e6, #4554; 「Aランクだね。」 is none); name ランプ, らんぷ, not with more katakana or ー right after (「ランプシェード」 is none, 「ランプを消して」 a call); stand-ins ランク, マンプ (「ヘイ、マンプ。」 on lamp-52e6, #4554; 「マンプス」 is none), in any clause: 「今何時ですか。ランク。」 is a call. `zh`: greetings 嘿, 嗨; name 兰普, 蓝普, "Lamp" with no space after a greeting, and anything after it ("兰普现在几点" is a call). `ko`: greeting 헤이; name 램프, 렘프, "야" attached ("렘프야"); with a subject or object particle (가, 를, 이, 은, 는, 의, 도) it is talk about the lamp ("램프가 고장났어." is none). `vi`: greetings hey, hây, hê, ê, này; stand-ins lam, lăm, làm, lắm, lem, whole words, and everyday one-word replies, so only in the line's first clause, greeted ("Hây lăm, …", "Hây lăm.") or with more clauses after ("Làm, mấy giờ rồi?"); a bare one, greeted in a clause of its own or not ("Làm!", "Lắm.", "Này, làm.", "Nhiều không? Lắm."), is no call and never the name alone. Every language also takes "hey". The sound inside a word is no call (「クランクを回す」, 「캠프」). It reaches the wake and address checks (`called`, `called_by_name_on`, `called_by_real_name`, which takes the name and not a stand-in), `begins_with_the_name` (greeted or set off), `only_the_name`, and onboarding how_you_sound's call by text (`second_provider::name_alone`). The key term stays "Lamp" in every language | `attention::calls_as_written`, `attention::only_the_name_as_written` |
| The first route (Live) | stays English: only the `second` worker is given `LAMP_LANGUAGE_FILE` | `process::second_route_worker_environment` |
| The change said | at start, a language other than the one recorded in `/var/lib/lampOS/voice/language-said` (absent is English) is said once in its own words through the say service (skill `language`, priority 8, 10 minutes to begin), and recorded only once spoken; an English lamp never changed owes nothing | `language::announce` |

The speech cache's key (`say/tts_cache.rs`) holds the voice, model and
language, so a new language renders its lines again.

### The fixed lines (#4473 (c))

The lamp's fixed lines are said in the owner's language from the speech
cache (speaking-first.md, "A cached line"), with English as the fallback.
English is untouched: no line of the owner's is warmed or looked up, and
each line plays exactly as before.

| Line | English words | Today | In the owner's language |
|---|---|---|---|
| The shutdown's | `lamp_msgs::GOING_TO_SLEEP` | speech cache, warmed at start | cached `text` played in the owner's words (`Recordings::cached`) |
| A hand's hold's sleep line | `lamp_msgs::SLEEP_LINE` | speech cache, warmed at start (`LAMP_SLEEP_LINE`) | the owner's when kept, else the English kept line, else the breath |
| `go_to_sleep`'s line | `lamp_msgs::SLEEP_LINE` | the model told to say the words; with `LAMP_SLEEP_LINE=on` and the line kept, the hold's cached line in its place | the model told the owner's words (`sleep_asked::told`); the cached line as a hold's, when kept |
| Offline, service down, back online | `language::PROVIDER_UNAVAILABLE`, `VOICE_SERVICE_UNREACHABLE`, `BACK_ONLINE` | the notices release's English file | the cache's line when kept (`FailureNotices::owners_line`), the English file otherwise |
| No Wi-Fi, recovery hotspot | `language::WIFI_LOST`, `WIFI_RECOVERING` | the notices release's English file, asked by name through the say service | the same (`Recordings::samples`) |
| The second route's own words before a look, and before a search with `LAMP_SECOND_CHECKING_LINE` | `second_provider::LOOKING` ("Let me look."), `CHECKING` ("Let me check.") | said as the turn's text, in English | the owner's words (`language::fixed_in`), and the next round is told those words were said (Thirteen's P2 on #4707) |
| The second route's down lines, and a look that never began | `second_provider::DOWN_LINE` ("Sorry, my thoughts are slow just now. Ask me again in a moment."), `DOWN_LINE_AGAIN`, `DOWN_LINE_CANNOT`; `COULD_NOT_LOOK` ("I couldn't get a good look.") | the down lines rendered by this route's voice after a turn and kept in memory (`LAMP_SECOND_DOWN_LINE`, on in the product); the look's said as the turn's text (`LAMP_SECOND_LOOK`, on) | rendered and said in the owner's words (`DownLine`'s language, read at start; `language::fixed_in`) |
| The second route's answer to a stop said over it | `spoken_stop::LINE` ("Okay, I'll stop there.") | rendered by this route's voice and kept in the thread (`LAMP_SPOKEN_STOP_ENDS`, on) | the owner's words, rendered and kept in the thread (`language::fixed_in`); the stop itself is still heard by its English words (`spoken_stop::is_stop_request`) |
| The second route's line when a line by name gets no words twice | `second_provider::NO_WORDS_LINE` ("Sorry, I lost my words there. Could you ask me again?") | said as the turn's text (`LAMP_NO_WORDS_AGAIN`, off in the product) | the owner's words (`language::fixed_in`) |

The words are `language::FIXED_LINES`, one row per English line with its
Vietnamese, Japanese, Chinese and Korean; "Wi-Fi" and the pinhole's
"reset" stay as they are printed. At start the cache warms the owner's
lines first, sixteen today (`FIXED_LINES`), then the English two
(`TtsCache::warm_lines`), on its
own thread, every line tried again until all are kept, first after a
minute, then twice as long each time, at most hourly; a line that renders
empty or past 20 s is not kept and never asked for again in that run
(it stays English); so the first
start must be online for them, and until then each line is said in
English, read again from the file each time it is chosen
(`TtsCache::owners_kept`, never a render on the voice's loop). The cache
keeps up to 48 lines. A failure notice said from the cache keeps the
release's kind in its record, with `wav_sha256` `speech-cache:<key>`: no
notices kind is added, and the release is untouched, since an older voice
refuses a whole manifest with a kind it doesn't know
(failure-notices.md). The failed turn's `turn_lost` has no words of its
own and stays the release's file or the sigh. Onboarding's fixed lines
(the skill's words, said as they are when the chat fails, #4263) and the
language change line (already in its own words) are not in this table.
Tests: `language::tests::each_fixed_line_has_its_words_in_each_language`,
`tts_cache::tests::the_owners_language_warms_its_own_lines_first_and_english_is_as_today`,
`say::tests::the_owners_language_is_played_when_kept_and_english_otherwise`,
`failure_notice::offline_lines_are_said_in_the_owners_language_when_kept_and_english_otherwise`,
`notices::loading_tests::the_owners_language_adds_no_notice_kind_and_keys_the_product_words`,
`sleep_asked::tests::the_model_is_told_the_line_in_the_owners_language`,
`second_provider::tests::the_down_lines_are_said_in_the_owners_language`,
`a_stop_said_over_the_lamp_is_answered_in_the_owners_language`,
`a_look_that_never_began_is_said_in_the_owners_language` and
`no_words_twice_is_said_in_the_owners_language`. The route reads the
language once, at start, into one field (`Shared::fixed`), which a test sets
(`Engine::in_language`).
Not heard yet: no line has been rendered or listened to on a lamp, and the
words want a native speaker's check.

**Cost.** The repo's only ElevenLabs price is $0.00004 a character
(`fixtures/voice-eval-cost-prices.example.json`, list prices read on 19
Oct), and its note says `eleven_v4_turbo`'s own price was not read and
Flash's was used in its place. By the repo's numbers, a character costs the
same in every language; v4 Turbo's real price is not in the repo.

**Not measured yet:** whether Nova-3 takes `ja`, `zh` and `ko` through the
relay (`vi` is shown above), whether the key term "Lamp" helps or hurts a
line in them, and the first audio of each language on a lamp. Onboarding's
lines and words are still English (#4473 (b)); the other fixed lines are
above. Tests: `language::tests`,
`attention::tests::the_name_is_heard_as_each_language_writes_it`,
`second_config::tests::the_owners_language_sets_the_speech_and_english_changes_nothing`,
`stt::tests::the_owners_language_is_the_streams`,
`not_speech::tests::the_owners_language_is_its_own_not_another`,
`stop_words::tests::the_stop_words_listen_only_in_a_language_they_have_words_for`.
The coordinator's own streams, the stop words' and the wait words', hear the
language through one helper (`language::hearing_config_from_env`).

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
  - the agents it hands work to, "Claude Code", "Codex", "OpenCode" and "Devin",
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
  last block within 30 dB of the line's peak so far and over about
  -60 dBFS. The peak is the loudest median of five blocks in a row, so a
  loud sound up to two blocks long (a click, a tap, a double tap) doesn't
  raise the bar for the words after it (Jarvis on #1993 and #2046); a
  longer one, a cough, still does. The bar is the line's own, so a far
  voice's soft last word counts as sound (Jarvis on #1993); after the
  canceller, talk near
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
  settled=<bool>, heard to H of S ms sent[; closed by the service C ms
  after force_end, before its answer: <the same counts again, at the
  close>, W words][, force_end answered in A ms (end of turn|no turn)|,
  force_end unanswered], words
  from=<settled|force_end|close_stream|closed_early>". T is how far ahead of the
  coordinator's end Flux's own end of turn came. `closed_early` is Flux
  ending the stream after `ForceEndTurn` and before its answer, with no
  `CloseStream` asked, told as on the Nova route below: when, how far it
  heard and how many words, never the words. That, beside the words,
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
  "Let me look." (the first builder, 2026-10-16 21:25). A written
  onboarding line of the fixed form (`(Say only this, nothing before or
  after it: "…")`, `onboarding::fixed_words`) not begun in time, or whose
  first round fails before any word (an HTTP 402, lamp-52e6 29 Oct 07:35),
  says its quoted words as they are rather than nothing (journal: `the
  onboarding line's own words are said`; lamp-52e6, 28 Oct 16:42; `docs/onboarding.md`,
  "Exact lines"). One the walk marks as its own (`onboarding::own_words`,
  how_you_sound's sentence prompts, #3769) asks no chat at all: its words are said at
  once. A full look
  (`LAMP_LOOK_FULL=on`) whose still never came arrives as the no-look note
  without `look`, and is a look all the same: 5 s, then the line.
  With `LAMP_SECOND_WAIT_CUE=on` (sixth-P3), a spoken turn whose first
  round has not begun 1.5 s after the words makes one soft sound ("Hmm.",
  "Mm." or "Hm.", the next never the last one played), and is then given up
  only 3 s after that scheduled sound (`BEGIN_WITHIN_AFTER_CUE`), at 4.5 s
  from the words with the default cue time: a "hmm" followed by silence
  is the worst of both. The sounds are said once by this route's own
  voice at its first spoken turn,
  kept in memory, cut to 0.6 s with a 30 ms fade, at half the
  reply's level. They go as audio only: nothing is kept in the thread, the 120 s
  announcement still comes before the first real answer, and a written
  turn never makes one. On lamp-4ace on 2026-10-16, 9 of 10 backup turns
  waited past 1.5 s.
  With `LAMP_SECOND_CUE_AFTER_MS` (500 to 4,000 ms, off unless set), the
  sound comes that long after the words in place of 1.5 s; a value it
  can't take is said and left at 1.5 s. A ready sound also moves the
  first-chat deadline: 3,000 ms means a 6 s deadline from the words,
  4,000 ms means 7 s. With no sound ready at the cue time, the deadline
  is the same as with one (1.5 s + 3 s, 4.5 s at the default): before
  #3755 it was the later of that time and the plain 3 s, so the first turn
  after a restart, its clips not rendered yet and its chat connection
  coldest, had the least time and was given up at 3.0 s (the tester's
  7071 lamp, 27 Oct 13:53; on lamp-75c9 a cold chat's first event came
  1.2 s later than a warm one's). The sounds and the down lines are also
  rendered as the worker starts and as each line begins, not only once its
  words are heard, so the first turn's "hmm" is usually ready. These bounds end the wait for the first chat
  event, not for speech playback to begin. Fourth's breakdown of lamp-4ace's
  silences (#2602, 21 Oct 20:41): the first answer audio now comes at p50
  1.99 s and p90 3.47 s over 96 turns, so at 1.5 s the sound came in about
  40% of turns, most of them about to answer. The gap after it was 13 of
  the 21 gaps over 300 ms that resumed (terminal-event recount: 25 other
  gaps ended in cancellation, including one without a matching journal
  turn). Replayed over those
  turns, at 3.0 s it would come in 10, with 3 gaps over 300 ms after it.
  A person who only paused hears the sound and goes on over it (lamp-4ace,
  21 Oct 18:36: 130 ms after it began). A line begun over nothing of the
  lamp's but the last request's wait sound, sounding still or not, is
  taken as begun in the clear (`Shared::only_its_wait_sound`; the journal:
  `began over only request N's wait sound`): no cut-in, no cut-in note,
  counted as the room's talk, so the join (#2620) can take it as the
  request going on; but no same-voice print is asked for it, since the
  sound is in its first samples (`Heard::BeganOverTheWaitSound`): on
  lamp-52e6 on 21 Oct (22:14-22:16, the Mac's played follow-ups), three
  such lines read 0.78, 0.59 and 0.17 against the conversation's voice, and
  the one at 0.17 was held as another voice. Once an answer piece has
  sounded, a line over it is a cut-in as before.
  With `LAMP_SECOND_DOWN_LINE=on`, a spoken turn whose first round has not
  begun within its bound, or whose chat refuses it before any word with a
  status that passes (429 or a 5xx; #1844's B, then the turn fails as
  before), says "Sorry, my thoughts are slow just now. Ask me again in a
  moment."
  (`DOWN_LINE`) instead of nothing. Said again within a minute
  (`DOWN_LINE_EVERY`) of that line, it says the shorter "Still slow,
  sorry." (`DOWN_LINE_AGAIN`), and after that one `DOWN_LINE` again: never
  the same line twice in a row (#3755: "I can't think right now" read as
  broken to the tester on a 7071 lamp, 27 Oct 13:53, so a slow moment, not a
  fault, and each person who asks is answered, where it used to fall silent
  for the rest of the minute). Any other status before any word (401, 402,
  403, 400, 404: a wrong or expired key, no credit, a request refused)
  says "Sorry, I can't answer just now." (`DOWN_LINE_CANNOT`) instead, at
  most once a minute, since asking again won't mend it and the slow line
  would promise it would (the Reviewer-lead's P3 on #3771); its journal:
  `said it can't answer just now`. The journal: `said its thoughts are slow`,
  and `(again, the shorter line)` for the second. The three lines are
  rendered together, each until it is kept: one whose render failed is asked
  again after the next turn, and until the short one is kept the long one is
  said in its place, never silence (Third's P2 on #3771). With an owner's
  language other than English they are rendered in its words ("The fixed
  lines", #4473), as is `COULD_NOT_LOOK`. The line is said once by this route's own voice as the first spoken line
  begins (before its chat, #1195's run on lamp-52e6, 21 Oct: the first turn
  after a start was silent otherwise), or after a turn is out, while the
  relay answers. The first can overlap one turn once a start: a line said
  over the lamp's voice also begins one, and a short line can end before a
  slow relay's render does. It is
  kept in memory (at most 8 s, so the longest line, about 6 s in Japanese,
  is never cut before "ask me again"; the old 4 s cut the English on
  lamp-c0a4 on 30 Oct, Reviewer-three's P2 on #4723), so it can still be said when the chat no
  longer answers. A render that fails is journaled (`the down line could
  not be said`) and tried again after the next turn. A minute counts from
  when it was played, not from a try that failed. It goes as audio only,
  like the wait's sound. A written turn, the lamp's own (a hello, a skill's
  say, the news), never says it: said unasked it reads as a broken machine
  (the reviewer on #1195). A look keeps its own line. A speech that never rendered (the speech service down too since start)
  leaves the turn silent, as before. Lamp-52e6, 21 Oct: with the chat's relay
  hung the line was said at 3.0 s; a refused connection says nothing here, as
  a network loss is the link's check's (below). On 2026-10-17 from 02:29 the
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

  OpenAI and Gemini are given the text without anything in `[...]`;
  ElevenLabs is given it as written, tags and all, unless
  `LAMP_SECOND_SPEECH_TAGS=on` filters it (below). A sentence with nothing
  left to say sends no request. Every sample is doubled (+6 dB) and clipped at full scale:
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
  model beside the lamp's tools, within this route's own bound of 48
  functions (`second_provider::MAX_TOOLS`; Live's is 32, Gemini Live's own;
  the chat models take more, DeepSeek's API up to 128), cut
  in the order `worker::chat_cut` declares: the fetch, the time elsewhere,
  `reports_state` with `LAMP_REPORTS=on`, the skills' functions not a core
  skill's (the last first), the weather with `LAMP_SECOND_WEATHER=on`, then
  the search, and never a core skill's
  function, `use_skill`, `reports_stop` or the actions part's (#2614,
  #2644, `worker::chat_tools`). What is cut is said once, on a plain
  journal line (`past the bound of 48, left out: …`). On lamp-4ace on 22 Oct the lamp's own,
  its skills' and the search's came to 37, and Live's bound had left out the time, the
  light scenes and the search; at 37 the chat's first answer began a median 856 ms after
  it was asked against 1031 ms at 32 (6 and 210 requests, the request 5% larger, its
  functions in the cached prefix). A turn waits 3 s for a call (`WITHIN`), then goes
  on with "the search did not answer in time".
  - `deepseek`: DeepSeek's Search & Fetch API (api-docs.deepseek.com/api/search,
    read 19 Oct). `web_search(query)` is `POST <deepseek>/v1/tools/search`
    with `{"query", "count": 3}`; each result's title, its `summary` (written
    for models) cut at 500 characters and its date go back. `fetch_page(url)`
    is `POST <deepseek>/v1/tools/fetch` with `{"url"}`; the page's title and
    its Markdown cut at 2,000 characters go back. $0.75 a thousand
    successful calls of each from 1 Nov 2026; 5 a second an account.
  - **Prices** (Migrate's finding 5 on #3997: on lamp-29e7 on 30 Oct a
    searched $399 was given as the ErgoChair Pro's price, and argued for when
    the person said the page showed $499). `web_search`'s description says a
    price found may be out of date, that Autonomous's own products are priced
    only from autonomous.ai's own product page, and that someone looking at
    the page is believed, never argued with. A search whose query or results
    speak of a price (the word, "how much", Vietnamese's "giá" or "bao
    nhiêu", or an amount with `$ € £ ¥ ₫ ₹`, USD or VND) carries `note` saying the same (`SEARCHED_PRICE_NOTE`), and a
    result on autonomous.ai carries its `url`. Such a search also reads the
    first two of those pages itself, at once, as below, each within another
    3 s after the results, and carries the `price` of the first in the
    results' order that has one. Its `note` says to give that price for that
    product, never one in the results, and never a price for another of our
    products from a search, only from its own page (`SEARCHED_OWN_PRICE_NOTE`).
    With none read it is `SEARCHED_PRICE_NOTE` and `OWN_PRICE_UNREAD_NOTE`.
    So a search the model asks for can wait up to 6 s in all; one begun early
    from the words heard waits mostly behind the model's first round. The
    model needn't call `fetch_page` for it: on lamp-4ace on 31 Oct (build
    9063) "How much is the Autonomous ErgoChair Pro?" was searched, not
    fetched, and answered "449 euro on Autonomous's own page" from the
    results' text, where the page said $499 (the first builder on #4593).
    `fetch_page` of an autonomous.ai page (`autonomous.ai` or
    `www.autonomous.ai`, no port; never the relay's `campaign-api`) also
    reads the page itself, `GET https://www.autonomous.ai<path>`, beside the
    fetch and within the same 3 s. Either read drops a locale's first
    segment (`/de-DE/…`, `/en-GB/…`, which give 534.31 EUR and 466.8 GBP for
    the ErgoChair Pro on 31 Oct) for the default page, takes at most 2 MiB,
    and follows at most two
    redirects and only while they stay on autonomous.ai (on 30 Oct a
    trailing slash answered 308 and an old product name,
    `office-chairs/ergochair-pro`, 301, both to the page): the price is from the
    page's own schema.org data, the first `Product` or `ProductGroup`'s
    `Offer`s (`AggregateOffer`'s low and high too), never a related
    product's and never the page's text, as `price: {product, currency,
    price}` or `{…, from, to}` when its options differ (the ErgoChair Pro's
    page on 30 Oct: 499 USD, 410 KB), with `note` to give that price and
    believe the person (`OWN_PRICE_NOTE`). No price read, or offers in two
    currencies: `note` says to give none from elsewhere
    (`OWN_PRICE_UNREAD_NOTE`).
  - **Saying it checks** (`LAMP_SECOND_CHECKING_LINE=on`, off unless set;
    #4684). On lamp-4ace on 31 Oct (build `7fa975266f`, the first builder's
    run of #4684) "How much is the Autonomous ErgoChair Pro?" got its right
    price 4.5 s after the words, with nothing said before: round 1's
    `web_search` call came with no words, then the search, our page's read
    and round 2. With the switch, a round with no words of its own that
    calls `web_search` or `fetch_page`, with nothing said yet in the turn,
    is begun with the lamp's own "Let me check." (`CHECKING`), said as the
    search runs, and the next round is told it was said
    (`said_once_note`). It is the lamp's line, spoken only when a search
    really runs, so it never promises a search that isn't made (#4595's
    case, line 3 of the same run). Each search also says in the journal how
    long it took, and our page's read when there was one:
    `web_search answered in N ms; our page's price read in M ms (read)`.
  - `openrouter`: OpenRouter's web plugin on Exa, `plugins: [{"id": "web",
    "engine": "exa", "mode": "instant", "max_results": 3}]` on the chat
    request, with `LAMP_SECOND_CHAT=openrouter` only. It searches before
    every answer, which every answer then waits for; OpenRouter's docs
    suggest its `openrouter:web_search` server tool instead, which lets the
    model choose (not built).
  - `exa`: the search service `search_url` names, in Exa's `/search` shape
    (`x-api-key`, `query`, `numResults` 3, text cut at 500 characters,
    `search_extra` added). Ready for Exa, whose key the lamp does not have.

  DeepSeek's runs on every product lamp (`LAMP_SECOND_SEARCH=deepseek` in
  `deploy/product/lamp-voice.service`): on lamp-4ace from 19 Oct (#1712's
  last-season Formula One answer, which led to the dated query) and on
  lamp-29e7 on 30 Oct, where its `web_search` and `fetch_page` gave the
  searched $399 of Migrate's finding 5 (the price rules above).
  OpenRouter's and Exa's were never run against their services.

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
- **With `LAMP_SECOND_SAID_ONCE=on` words said before a call are not said
  again after it** (off unless set; the first builder at 06:56 on #1722). On
  lamp-4ace "Set a timer for one minute." got "A minute, starting now. One
  minute, counting from now — six forty-four and change.", 2 of the last 4
  runs. The round that called had said its words, and the next round, given
  the timer's result, confirmed again. With the switch, a round that said
  words before its calls tells the next round, as a system message after
  the calls' results, that the person heard them
  (`second_provider::said_once_note`). It is told to say only what the
  results add, such as the time it is set for or that it failed, or nothing.
  A round that said nothing before its calls, or a call written as words
  (which isn't said), tells nothing. The journal says `chat round N said
  words before its calls: the next round is told they were heard`. The
  check is on the lamp: the day lines' timer and reminder at 4 of 4 runs,
  one confirmation each.
- **With `LAMP_SECOND_SAY_WITH_CALL=on`, a call that changes something is
  answered at once with the acting part's own words, its result** (off
  unless set; #2135, the first builder's yes at 22:13 on 30 Oct). On
  lamp-4ace on 30 Oct (8999), `light_warmer`, `remember` and `set_timer`
  said nothing until their second chat round: round 1 took about 1.0–1.5 s
  to the call, and round 2 another 1.0–1.2 s, so 2.4–3.3 s to the first
  sound, against 1.54–1.62 s for plain answers (`Engine::saying_at_once`):
  - **A round with no words of its own whose calls all changed the light or
    the volume,** each one answered short by lamp-actions (its `say`, with
    `LAMP_SHORT_DONE=on` in `lamp-actions.service`), says the calls' own
    line ("Warmer.", "Done." when they differ) as soon as the calls are
    answered. A change that didn't happen ("already") carries no `say`, so
    nothing is said early and the next round says why.
  - **A round whose one call set a timer, committed,** says "Timer set. It
    rings at 7:52 pm." with the schedule's own `clock`, the local time it
    rings, on a 12-hour clock as the lamp's other words are, so the time
    said is the time set (`timer_line`, `spoken_clock`). One asked for as a
    reminder (the person's words say "remind") is named as one: "I'll remind
    you at 7:52 pm."
  - **With one ask, the turn ends there.** The next round is not asked, so
    it can't say the change again in other words. It still runs, told the
    lamp said it (`short_done_note`), when the person's words plainly hold
    more (`asks_more`): cut into clauses at a sentence's end (not a point
    between digits), a comma or semicolon, or "and", "also", "then" or
    "plus", with a clause of only thanks, a greeting, the name or "okay"
    left out and a call's own words (a reminder's `text`) taken as its ask,
    more clauses than the round's calls. So "brighter, and is it sunny?" and
    "dim the light, tell me a joke" get round 2 after the first words, and
    "It's too bright.", "Make it warmer and brighter." (two calls) and "Make
    it warmer. Thanks!" end with them (Reviewer-three's P2 on #4726).
  - **In English only:** those words are English, so with an owner's
    language chosen (`language::at_start()`: Vietnamese, Japanese, Chinese
    or Korean) such turns wait for the model's words as without the switch,
    and the journal says so at start (`worker::says_at_once`, Thirteen's P2
    on #4700).
  - **Never** for a keep or a forget beside the change (either can come back
    held for the person's yes, Reviewer-three's P2 on #4635), a call
    refused, failed, held or unchanged, two timers, a reminder at a day and
    time, or a round with words of its own. Those turns go on as without the
    switch.

  **Tried and dropped: the model's words with the call.** The switch first
  also asked the model to say its few words in the same reply as the call.
  On lamp-4ace on 31 Oct (build `4ac4cc2bdf`) they came 1 time in 8 acting
  turns (the first builder on #4635). On `65b10836e0` (11:29–12:04) they
  came every time, but before the result: "Warmer it is." then "Already at
  my cosiest orange, actually." And a reminder was said twice, "Timer set.
  It rings at 12:03 pm." then "Two minutes — I'll nudge you at 12:03." (the
  first builder on #4710). The light and timer turns were 0.6–1.4 s faster,
  at Live's pace. So only the result's own words are said early now, and a
  single ask ends with them.
- **With `LAMP_SECOND_SPEECH_TAGS=on` the voice may be given an emotion
  tag** (off unless set, on in the product unit from 28 Oct, the owner's
  pick at lamp-52e6; #2901, the lead's proposal on #1711, 22 Oct). The
  first builder's played test on lamp-4ace on 28 Oct (`rUTAGS-0904`):
  35 of 37 lines answered, both misses on the hearing side, first sound
  1.50 s median and 4.25 s at most against 8001's own 1.63 s and 4.95 s,
  no tag spoken as words. The
  spoken turns' character is followed by a short note
  (`speech_tags::NOTE`), only when the voice reads tags (below): the voice reads `[laughs]`, `[sighs]`,
  `[whispers]` and `[excited]`, to be used rarely, only where a person
  would make that sound, at most one a reply. Each piece of the reply goes
  through `speech_tags::Tags` before it is said. The voice is given only a
  listed tag, the first of the reply, and only when it is ElevenLabs with
  a model that reads them (`speech_tags::READING_MODELS`,
  `eleven_v4_turbo`): `LAMP_SECOND_SPEECH_MODEL` (or `tts_model`) set to
  it, and with `LAMP_SECOND_SPEECH_SOCKET=on` the socket's model
  (`LAMP_SECOND_SPEECH_SOCKET_MODEL`) too. The route's default,
  `eleven_flash_v2_5`, would say a tag as a word, so with it the chat is
  not told and every tag is dropped, as with OpenAI or Gemini. Any other
  `[...]` is dropped. A `[` left open at a piece's end (up to 24
  characters) waits for the next piece, so "[lau" then "ghs] ok" is said as
  `[laughs] ok`; a tag with no words after it waits for the next words. The
  lamp's words, which go to the transcript, the thread, the own-words list
  and the body, never carry a tag, so a tag never moves the body or the
  light. The journal says `speech tags: K given to the voice, D dropped`
  per reply, counts only. Off, a reply is said and written as the chat
  wrote it.
- **With `LAMP_NO_WORDS_AGAIN=on` a line said to the lamp by name never
  ends in silence from an empty answer** (off unless set; #3704). On
  lamp-52e6 on 27 Oct at 09:52 "Lamp, What's my name?" got an answer with
  no words, and the lamp said nothing: a round with no words and no call
  ends the turn, and the line was counted as left alone, like room talk.
  With the switch, a spoken line whose words begin with the lamp's name
  (`attention::begins_with_the_name`: "Hey Lamp …", or "Lamp" set off by a
  pause mark, "Lamp, …"; "Lamp what's my name" with no mark does not
  count), that is not a stop or "never mind" ("Lamp, stop." is rightly
  answered with nothing; the Reviewer-lead's P2 on #3712), whose
  round gives no words and calls nothing, with nothing said or called
  earlier in the turn, is asked once more, told it gave no words and to
  answer in a few words or say it can't
  (`second_provider::NO_WORDS_AGAIN_NOTE`). If that round has no words
  either, the lamp says "Sorry, I lost my words there. Could you ask me
  again?" (`second_provider::NO_WORDS_LINE`; in the owner's language when
  one is chosen, "The fixed lines"). A turn that called anything
  ends as before, since a call's answer may tell the model to say nothing
  (a held call, a search stopped by a hand, a stay after unplugging). A
  line without the name ends as before too: the model may leave room talk
  alone. The asked-again round has no first-event bound of its own (only
  round 1 has), so a second chat that stalls still ends in silence. The
  cost is one more chat round, about 0.8 s to its first event,
  and only on a turn that would otherwise have been silent. The worker
  says at start `a line begun with the lamp's name answered with no words
  and no call is asked once more, and said a short sorry if it has none
  again (LAMP_NO_WORDS_AGAIN)`; each such turn's journal says `no words
  and no call for a line begun with the lamp's name; asked once more`,
  then, if needed, `no words again …; said "Sorry, …"`, so the played
  tests can count empty answers. Test:
  `an_empty_answer_to_a_line_said_by_name_is_asked_again_never_silence`.
- **With `LAMP_SHORT_DONE=on` a light or volume ask that worked is
  answered with the call's own short line** (off unless set; #2877). On
  lamp-52e6 (Fifth's row 24, 22 Oct) the model still spoke about ten words
  after a light or volume call lamp-actions answered short, a median of 10
  off and 10 on: it didn't keep the short answer's instruction. With the
  switch, when every call of a turn's first calling round is answered with
  lamp-actions' short mark (`"next"`) and its own words (`"say"`:
  "Brighter.", "Dimmer.", "Warmer.", "Cooler.", "Louder.", "Quieter.", else
  "Done."; `lamp_actions::short_say`), the voice keeps that line and asks
  the next round anyway, told after the results that the lamp itself says
  the line, to answer only what else was asked, or nothing at all
  (`second_provider::short_done_note`). A request can hold more than its
  calls: "dim the light and tell me a joke" is `light_dimmer` alone in
  round 1, and the joke comes in round 2 (the Reviewer-lead's P2 on #2930).
  So the line is said only when that round gives no words and no calls;
  words or calls go on as without the switch, the line unsaid. Judging the
  heard words as light-or-volume-only was not taken: "and", "also", a
  question after the ask, any language, is a guess, and the model already
  read them. The cost is that round's wait: the line comes when the round
  ends empty, about a chat round's first event later (0.80 s median on
  lamp-4ace for the sentence after a light change), not at once; the light
  or volume change itself, the answer seen or heard, is not delayed. Two
  calls are one line: theirs when they agree, "Done." when they differ
  (`second_provider::short_done_line`). The turn goes on as without the
  switch, with no note, when any call was refused, failed, held or changed
  nothing ("already", which carries no `"say"`), when any call is anyone
  else's, when an earlier round called anything or a held forget ran
  before it, and when the model said any word beside its calls: words
  beyond the ask, a call written as words included. The worker gets the
  switch in its environment and says so at start: `a turn whose every call
  is a light or volume change answered short is said with the calls' own
  line when the next round adds nothing (LAMP_SHORT_DONE)`; each such turn's journal says
  `every call answered short; "Brighter." kept while the next round says
  whether more was asked`, then `added nothing to the calls answered
  short; said "Brighter."`. The check is Fifth's row 24 on lamp-52e6: the
  reply after "make it brighter" at most 2 words on, against about 10 off,
  the light changed in both; it now rests on the model keeping the note's
  "nothing at all", which the row measures.
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
    - any function a system or user message of the turn names as a whole
      word (`a_note_names`): a note that says "call remember_my_face", such as
      setup's who_you_are note or the name-told note, is given it at once.
      Behind `more_functions`, on lamp-52e6 (row 10), DeepSeek called plain
      `remember` for it and never closed the step, 2 tries of 3.
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
- **With `LAMP_SECOND_CHAT_KEPT=on` the chat's client keeps an idle
  connection in its pool at most 25 s** (off unless set; `net::CHAT_POOL_IDLE`),
  not reqwest's 90 s, counted from when the connection went back to the
  pool. TCP keepalive is reqwest's own either way (15 s, then 3 probes 15 s
  apart, and a 30 s TCP user timeout on Linux), so this changes only how
  long a connection is reused. Keepalive finds a path dropped in silence
  only after about 60 s (15 s, then 3 probes 15 s apart), past the 25 s cap,
  so the cap and #3107's ask again are the real cover (El on #3115). With
  the switch, at most one idle connection per host is kept, so a warm still
  running when a chat goes can't leave two. On lamp-4ace on 22 Oct three chat requests
  never reached the relay (Backend's 02:09 on #2602), and the turn ended
  unanswered at 4.5 s. That fits a request written onto a pooled connection
  the path had already dropped, though the cause isn't shown; this switch
  is the test of it. A line whose connection the pool has let go opens a new
  one, a TLS handshake before its first word (its cost not measured here).
  With the chat's warm also on, the warm comes from 20 s idle
  (`WARM_AFTER_KEPT`) rather than 60 s, counted from the last request; it
  may still reuse a pooled connection, and may still be opening when a short
  line ends, so it makes a ready connection likelier, not certain. The
  other clients are unchanged. The journal says so at start. Measured by
  `chat round 1 had not begun` counts against nights without it, with the
  gap since the last request from `chat request going out` (#3107).
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
- **A line begun with no speech connection known idle is warmed too,**
  though a piece went within the 60 s (#1722, the first builder at 01:52 on
  21 Oct: on lamp-4ace 20 of 221 first pieces went on a new connection, first
  audio about 600 ms against about 385 ms reused, and 13 of the 20 came within
  10 s of the piece before). The connections are HTTP/1.1, one request on
  each at a time, so the last answer's pieces still streaming hold theirs,
  and a piece dropped mid-body, as at a cut-in, closes its own. `Tts` keeps
  the latest connections' local sockets, each marked idle once its answer
  is read to its end (none with `connection: close`), in use while it
  streams, and forgotten when it is dropped before its end. At most one
  warming is in flight. With an idle connection known, nothing more goes.
  With the switch off nothing warms, and the pieces' connections are as
  before.
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
- **With `LAMP_SECOND_OWN_ECHO=on`, the lamp's own words heard back are
  held as its echo** (off unless set; #2997; `second_provider/own_echo.rs`).
  On lamp-4ace on 22 Oct (14:40–16:45) 8 of 214 lines were mostly the
  lamp's own words by the count above, and 5 of those 8 were answered. With
  the switch, a line begun while the lamp's answer could be heard (`Start`'s
  `over_the_lamps_voice`), or within 1.5 s (`TAIL_US`) after its words sent
  would have played to their end (the end the conversation goes on from,
  `Did::Said`), is judged against that answer, before the attention rule.
  A heard word is the lamp's own only inside a run of at least 2 heard
  words (`STRIP_RUN`) that the answer says one after another in the same
  order (the same `word_key` matching). The same words out of order are
  not its own, so this count K can be lower than the journal's count above,
  which matches words as a set.
  - **nothing but its own words, at least 3** (`HELD_LEAST`): its answer
    is held back, and the journal says `request N: its answer is held back:
    the lamp's own echo: of its W words, K in the lamp's answer to request
    A, begun over the lamp's voice` (or `within 1.5 s of the lamp's
    words`). Its verdict is held back; it gets no hum or ask-again, and
    words it cut short are answered in its place, as for any held line.
  - **one run of its own words at the line's start or end, the rest new
    words**: that run is taken out of what the model is sent, and the
    journal says `request N: K of its W words, the lamp's own answer to
    request A at its start` (or `end`) `, are taken out of what the model
    is sent`. Its own words anywhere else, at both ends, or one alone
    ("so", "no"), leave the line as heard. The heard line is still told as
    heard.
  - **A line that adds a word the lamp did not say is never held.** At
    most its own run at its start or end is taken out, as above.
  - **The person's line is sent as heard, never held nor cut,** when it
    begins later than the 1.5 s, has the lamp's name
    (`attention::called_by_name_on`, or only the name called), is a
    question of its own (`remarks::questions`, a `?` or a question word
    first, with at least one word not the lamp's: "Did you say five
    o'clock?"), begins after the lamp's words when they ended on a question
    ("Yes, start it." after "Shall I start it?"), or answers an offer in the
    answer's last sentence (`OFFERS`: "I can", "I could", "shall I", "should I", "want me
    to", "would you like", "do you want", "if you want",
    "if you'd like", "if you like"; "I can't" is none, and a sign-off such as "let me know if" is none). An offer covers a
    line begun after the lamp's words, and one over its voice that adds a
    word: "set a reminder for six" after "I can set a reminder for six." is
    answered. When such a line would have been held or cut, the journal
    says `…, but it answers the lamp's offer: answered as the person's` (or
    the guard that kept it).
  - **Still held:** a line over the lamp's voice that is nothing but its
    words, even an offer's or a question's ("shall I start the timer" over
    "Shall I start the timer?"), and a line of its words within the tail
    after an answer with no offer and no question. A person who repeats
    such an answer word for word within 1.5 s is held as its echo.
  Counts only, never words. Without the switch nothing of this runs.
- **With `LAMP_SECOND_STT_WARM=on`, the next line's socket is kept open**
  (off unless set; `Engine::keeping_a_listener_ready`). As a line takes its
  socket, the next is opened in the background and kept with `KeepAlive`
  (every 5 s, `KEEP_ALIVE`). The next line takes it at once, so its sound
  goes live from its first block. The first line's is opened before it
  (#2135, the first builder on #1722 at 01:15: on lamp-4ace the first
  request after a start waited 753 ms for its socket, with 1,050 ms of its
  sound waiting). At an authority the worker installs with listening
  allowed and nothing kept, `Engine::listening` opens one, once a privacy
  generation, so the first line after the voice starts, or after listening
  comes back, takes it (its journal line says `kept ready`); the journal
  says `speech to text's socket opened before the first line` when it
  does. Its bounds (the first builder at 00:31):
  - **at most 3 minutes** (`KEPT_FOR`), then it is closed, so a lamp
    nobody talks to keeps nothing open; a line after that opens its own.
    The opened-before-the-first-line one is no exception: the authorities
    that keep coming don't open another in the same generation, so the
    first line after 3 quiet minutes is still cold;
  - **closed at once when listening stops** (the privacy switch, a mute)
    or the microphone's privacy generation changes (`Engine::listening`,
    from each authority the worker installs), and never used for a line of
    another generation; no audio is sent on it before a line takes it;
  - **one found closed** (`Listener::open`, `SttStream::still_open`) is
    dropped without a word, and the line opens its own as before.

  The cost is one idle socket to the relay's proxy during a conversation,
  and for the first 3 minutes after the voice starts: Deepgram bills the
  audio sent, and `KeepAlive` sends none. Tests:
  `the_next_lines_listener_is_kept_ready_and_one_closed_since_is_not_used`,
  `a_kept_listener_goes_after_its_time_or_when_listening_stops` and
  `the_first_line_after_a_start_goes_to_a_socket_opened_before_it` (with
  `no_listener_is_opened_before_a_line_outside_a_runtime`).
- **With `LAMP_SECOND_UNFINISHED_WAIT=on` a line whose words so far end
  unfinished waits longer before its end** (off unless set; #3026). A
  line's end is decided by the coordinator's detector from the sound
  alone: the silence window, or the end-of-turn model's verdict
  (`activity::TurnDetector`); no rule looked at the words. In the owner's
  test on lamp-4ace on 22 Oct (19:04–19:08) 5 of 28 lines were cut at a
  mid-sentence pause ("Lamp, I'm thinking about … going for a run tomorrow
  morning, is that a good idea?") and the second half was answered alone.
  With the switch:
  - **the second route's worker reads each result's words as they come**
    (`Listener::said`, interim and final) while the line is heard, and
    tells the coordinator whenever the words so far come to end on an
    unfinished word, or no longer do (`ProviderOutput::WordsEnd`, the class
    only, never the words). Unfinished is one list
    (`unfinished_words::UNFINISHED_WORDS`): a joining word ("about", "the",
    "a", "and", "to", "so", "like", "but", "or", "of", "for", "with", "my",
    and Vietnamese "và", "của", "là", "thì", "mà", "để" for a route set to
    Vietnamese) or a filler ("um", "uh"), compared composed (NFC), so a
    decomposed "và" matches. No question words: interim results rarely
    carry the "?", and "tell me why" or "show me how" are whole requests.
    A last word followed by "?" or "!" was said as an end and is finished;
  - **the line's speech to text keeps the fillers** (#3126): Deepgram
    drops "um" and "uh" from its words unless asked, so on lamp-4ace all
    44 held ends were `joining` and none `filler`. With the switch a
    request's line asks for them (`filler_words=true`,
    `SttStream::connect_hearing_fillers` from `Net::listen`); the stop
    words' and the wait words' streams do not, nor Flux, which takes no
    such option. The fillers are only for this judgement: the line's final
    words, and its early words for a prepared chat, have them taken out
    (`unfinished_words::without_fillers`, in the worker as the words come
    back), so the join, the not-for-me words, the name checks, the thread
    and the model read the line as before. Without the switch nothing is
    asked and nothing is taken out;
  - **the detector holds an end that comes due while they do**
    (`TurnDetector::words_end`) for at most 1.3 s more of quiet
    (`unfinished_words::WAIT_BLOCKS`, the first builder's guess for a real
    pause is 1.2–1.5 s). Speech in that time goes on with the same line;
    words told finished meanwhile let it end at once; when the wait runs
    out the line ends as it would have. The 1.3 s is the line's, not a
    pause's: a hold let go by a noise leaves only what is left of it for
    the line's next end, and no end is held past the silence window plus
    1.3 s of quiet. So a person who truly stopped after "um" is answered
    at most 1.3 s more per line, never on top of Smart Turn's incomplete
    wait: an end the model held as incomplete (`model_incomplete`, 1.5 s)
    is not held again. A finished line ("What's the weather in Hanoi?")
    gets no extra wait. A call's wait that ran out (`--call-wait-ms`) is
    not held again;
  - **the journal says each held end, counts only**: `lamp-voice: request
    N: unfinished words (joining|filler) held its end N ms, more speech
    came: yes|no`; the endpoint record has `unfinished_wait_ms`, and its
    `includes_silence_wait_ms` counts the hold.

  The words come back from speech to text a moment after they are said;
  an end due before the last words are back is judged on the ones before
  them. Tests: `unfinished_words`'s own, and `tests/it/unfinished_wait.rs`
  (unfinished words wait and end within the bound, a finished question
  doesn't wait, a stop after "um" is answered at the bound, with Smart Turn
  too, a line's holds add up to the bound, words going on keep one line,
  and without words told nothing changes);
  `the_words_so_far_are_told_only_with_the_switch` and
  `a_line_ending_on_a_filler_is_told_and_the_filler_never_reaches_the_model`
  on the worker; `fillers_are_asked_for_only_with_the_unfinished_wait` on
  the query; `a_filler_ended_line_is_held_and_joined_with_what_follows` on
  the detector; `words_end_holds_the_lines_end_only_with_the_switch`
  through the coordinator.
- **With `LAMP_SECOND_EARLY_FINALIZE=on` a line's words are asked for at a
  pause** (off unless set; the first builder on #1722, 20 Oct 07:20 and
  07:36). On lamp-4ace Deepgram never ends a line itself: the lamp's quiet
  tail (its noise floor, the canceller's residue) isn't silence to its
  `endpointing=300`, so every line's end sent `Finalize` and waited
  270–300 ms, and the early words were a stale interim. With the switch:
  - **the coordinator's own quiet count** (`TurnDetector::pause_blocks`, no
    second detector) at 150 ms of a pause, once a pause and only after
    300 ms of speech since the last ask, so a halting speaker doesn't cut
    the service's segments every word, sends `ProviderInput::Paused`
    (`coordinator/early_finalize.rs`);
  - **the second route asks for the words so far** (`Listener::flush`,
    `SttStream::paused`, a `Finalize` with the stream kept open). Speech
    that goes on comes as new finals, appended in order;
  - **at the line's end**, if that answer came, its finals cover the
    line's sound (`covers_the_sound`, the rest quiet) and no block since
    the pause's `Finalize` was over the absolute floor (`SOUND_FLOOR_RMS`:
    a soft last word, however far under the line's peak, is kept, Jarvis on
    #2102), they are the line's words at once, with no second `Finalize`
    (`words from=flush` in the journal's end line). Otherwise the end asks
    as before;
  - **the pause's answer gives the line's early words** when it comes
    before the line's end, its finals cover the line's sound, and nothing
    over the floor has come since (the first builder on #1722, 20 Oct
    11:36). With `LAMP_SECOND_PREPARED` the chat is begun on them then,
    and kept when the line ends on the same words (`words from=flush`).
    How long before the end that is depends on when the line ends: on
    lamp-4ace's lines that ended on the pause's answer (#2135, 10:10–10:13)
    it came 158–233 ms before the end on six of seven, and 4 ms on one. A
    line that ends at 400 ms of quiet, with the answer later than 250 ms,
    gives none (the reviewer on #2171). The journal's "kept, begun N ms
    ago" gives each line's head start. Sound over the floor after the early
    words takes them back at once (`Early::Resumed`), so the chat begun on
    them stops rather than running on to the line's end; round 1 is asked
    on the line's final words as before. An answer only after the end gives no
    early words: the line's words are at once then anyway. Each such pause
    begins one chat, so a line that pauses twice and goes on may ask
    twice;
  - **a pause's answer that comes after the line's end** is taken the same
    way (#2135). On lamp-4ace the end comes 400 ms into the quiet
    (`smart-turn-fast`'s confident "finished"), 250 ms after the pause's
    `Finalize`, which Deepgram answers in 220–450 ms; on the first run 6
    of 19 lines said `words from=flush`. The end still asks, but the
    service answers in order, so the first `from_finalize` result while a
    pause's is due is the pause's. With the rest quiet and covered, its
    finals are the line's words, `words from=flush`, its answer timed from
    the pause's `Finalize`. After the end, a pause's answer is also told by
    where it ends: no further than its `Finalize`, where the line's own
    reaches past it, so a pause's answer that never came doesn't make the
    line's own taken for it (`a pause finalize never answered`). With a
    word after it, or short of the sound,
    the line's own answer is waited for as before, and that answer alone
    can send `CloseStream`;
  - **the end line says the pause:** `pause finalize at N ms` (where in
    the audio it went), `answered` or `unanswered at End`, `sound after
    it` (a block over the floor), and the loudest block after it in dBFS,
    from which a busy room's floor would be set. A line with no pause says
    nothing of one;
  - **not on Flux**, whose flush would end its turn (`ForceEndTurn`) and
    which ends its turns itself; **not the stop words' stream**, which
    never takes a pause's answer as its end. The Live worker takes nothing
    from `Paused`.
- **With `LAMP_SECOND_OPEN_NAME=on`, a line that ends before its name
  waits for it** (off unless set; #2135, the first builder on #1722, 21 Oct
  01:58 and 02:16). On lamp-4ace at 21:25 "Hey Lamp, my name is" and 400 ms
  of quiet ended a line (`model_complete`, 0.933). The name began 9 ms after
  the end. It was judged as a sound while the answer was awaited, and let go
  1.13 s after the end. The lamp answered "my name is" alone, and the person
  said it all again 14 s later. With the switch:
  - **What counts as before a name** (`known_faces::opens_a_name`): the
    final words end on "my name is", "my name's", "call me", "I'm called",
    "I am called", "the name is" or "the name's", or in Vietnamese "tên tôi
    là", "tôi tên là" or "gọi tôi là" (with "mình" or "em" for "tôi"), a
    trailing "um", "uh" or "ờ" allowed. Not after "what", "don't", "never"
    or "not". Not a question ("Do you know what my name is?"). Not "call
    me" in a line that opens with "can", "could", "will", "would" or
    "please" past a greeting and the lamp's name ("Can you call me", a
    call asked for, whose "?" speech to text often drops), while "You can
    call me" still opens a name (the reviewer's P3s on #2355). Not a bare
    "I'm" or "I am": "Yes, I am." ends a line.
  - **The second route holds the answer** (`Engine::waiting_for_a_name`):
    such a line is answered no sooner than `OPEN_NAME_WITHIN` (2.5 s) after
    its end. A newer line drops the request, and the wait with it; with
    none, it is answered as it is.
  - **The coordinator lets the name go on** (`coordinator/open_name.rs`).
    The route gives it the line's final words before the answer
    (`Transcript` with no request). When they end before a name and none of
    the turn's answer has been heard, a sound that began within 2.5 s of
    the turn's end isn't let go by the answer wait (#1123, with its words)
    or the body's motors. The keeper's pause, the busy-room hold and
    asker-first (#829) still judge it: they keep the room's next voice off
    an awaited answer, so a colleague's "…see you at lunch" begun in those
    2.5 s, in a busy room or well under the person's voice, is never taken
    as the name (the reviewer's P3 on #2355). In a quiet room a colleague
    at the person's own level could still be joined; the route's
    same-voice print would tell them apart, and is to be read here once
    #2341's one "someone else" check is in. Otherwise it is decided as any
    line is, and its Start says `continues_cut_off`. So the
    route puts the held line's words before its own (the `before` path),
    and the two are asked as one: "Hey Lamp, my name is Jim. Please
    remember it." Its `input_admitted` says `goes_on_from_its_name`, and
    the turn's `open_name` record says when it ended.
  - **Only while the second route answers** (`provider.role` is
    `second`): the Live route has no such hold.
  - **The window is its own, 2.5 s** (`OPEN_NAME_WITHIN`, longer than
    `CARRY_AFTER_END`'s 1.5 s for the kept-sound carry): on lamp-4ace at
    05:53 a name came about 2.05 s after "my name is" ended, past 1.5 s,
    and the body's move let it go; a person who says "my name is" and stops
    is about to say it, and may think a moment (the first builder on #1722,
    06:00).
  - **What it costs:** at most 2.5 s before the answer, on lines whose
    final words end before a name, and nothing on any other.
  - **What it doesn't do:**
    - A sound let go before the line's words came is not brought back. On
      a cold socket the words came 705 ms after the end, before the let-go
      at 1.13 s.
    - The turn-end model's decision is unchanged: the line still ends at
      "my name is".
    - A line cut mid-name ("Hey Lamp, my…", ended by a silence fallback)
      doesn't end on an opening, so nothing here covers it. On lamp-4ace
      at 03:11 one ended 0.66 s in while the body turned to the voice, and
      the rest of it was let go as the body's motors
      (`input_let_go_body_moving`); that's the next step (the first
      builder on #1722, 03:19).
  - **Tests:** `known_faces::tests::a_line_ending_before_its_name_is_told_apart`,
    `second_provider::tests::a_line_ending_before_its_name_waits_for_it`,
    `coordinator::open_name::tests` and, through the session,
    `coordinator::tests::second_route::a_name_said_just_after_a_line_that_ended_before_it_goes_on_from_it`
    (with and without the switch, and with `LAMP_WAIT_KEEPS_ANSWER=on`, where
    without the switch the name first waits as a sound), and
    `in_a_busy_room_a_sound_after_a_line_that_ends_before_its_name_is_held_as_before`
    (the busy-room hold kept, through a loop test's busy room,
    `TEST_ROOM_BUSY`). The body's motors' skip is not driven by a test: the
    session's fixture has no body.
  - **For a lamp:** played name lines that pause 400–800 ms before the
    name. Count the names that go on (`goes_on_from_its_name`) and the
    candidates let go after an end, with the switch and without. Then one
    in a busy room, someone else speaking within 2.5 s of "my name is": it
    is not joined.
- **With `LAMP_SECOND_NAME_CARRIES=on`, a line that is only the lamp's
  name waits for the line after it, which is then said by it** (off unless
  set; branch `first/name-carries`, 22 Oct). In played round r01 on
  lamp-4ace that night, "Lamp, I need a gift for my sister … What should I
  get her?" with a natural pause after "Lamp," came as pieces. The name's
  piece (heard as "Life." or "Louvre," for "Lamp", or as the name with
  more) was held, as a short sound or as said to nobody, and each later
  piece as said to nobody: the name was in the held piece. 4 of 38
  requests were lost so. "lamp" and "hey lamp" are short words already
  (`not_speech::SHORT_WORDS`), so a short sound held so was a misheard
  name or more than the name. `marks.called` (the coordinator heard the
  name) had only rescued a line with no words. With the switch:
  - **Which line waits:** its words are only `key_terms::NAME` ("Lamp"),
    any case and punctuation, "hey" or "ok" before it at most
    (`name_alone`); or they are short and begin with it (`name_first`):
    the name first, set off as a call (`attention::begins_with_the_name`,
    so not "Lamp is broken."), at most 3 words after it
    (`NAME_FIRST_MORE`) ("Lamp. So", "Lamp, can", "Hey Lamp, what's").
    Never a line whole as it is (`whole_already`, El's P2: "Lamp, stop."
    waited 2.5 s): a stop or "never mind" (`says_stop`, the stop words'
    list), a question past the name ("Hey Lamp, what's the time?"), or two
    words or more past the name ending a sentence ("Lamp, lights off.");
    this route has no turn-end verdict for its lines and no verb list, so
    that is the bound that keeps "Lamp. So" and "Lamp. So." waiting. In r01 such pieces were the ones held: "Lamp. So So
    I have a meeting…" was held as a short sound, then the rest for no
    name. Or the coordinator ended it as only the name
    (`ProviderInput::Called`), whatever its words. Such a line is never
    held as a short sound, and its answer waits up to `NAME_CARRIES_FOR`
    (2.5 s) after its end, as `OPEN_NAME_WITHIN`'s wait does. A line itself
    said by a name never waits again.
  - **A line begun in that wait is said by the name, only when one person
    said both** (El's P1s on the branch, 22 Oct). Its Start takes the wait
    (`Engine::name_said_before`); `begin` drops the name's request. The
    rule keeps both lines as a judgement keeps them, before either is
    judged (`Heeding::keep_unjudged`: the faces seen speaking it, the
    eyes' newest look, the room; no name hearing read), and tells whether
    one person said both by the join's own judgement
    (`Heeding::one_speaker`, `same_speaker`, a print still to come waited
    for up to `JOIN_WAIT`): one face seen speaking each, or prints at
    0.70 or more; failing those, its no sign of a second person (at most
    one face in each newest look, no other voice heard and no two faces
    seen speaking in the conversation) only when both were said in a
    quiet room, never an unknown or busy one, even with one face in view
    at each. Otherwise there's no carry: the line is judged as its own,
    with its own words and window, and held as it would be without the
    name; the name's words wait where words cut short do, answered in its
    place if it is held, and never joined to it (`name_refused`, El's P3:
    the join's own no sign of a second person takes one face in view at
    each in a busy room). Answered so, the name comes with a note that
    someone went on talking right after it, never what was said, which may
    be someone else's (`worker::NAME_THEN_HELD`), so the lamp says it didn't
    catch it and asks them to say it again: on lamp-4ace on 26 Oct the first line after a start, "Lamp." and
    then "Remember that the plant by the window needs water on Fridays." in
    a room not yet known to be quiet, got "Mm? I'm here." and the request
    was lost with nothing asked. With `LAMP_FIRST_SECONDS_QUIET=on` (off
    unless set, on in the product unit; #3711) that room is shown quiet
    from what the ears measure, never just unknown: a line kept for the
    join in a room unknown only for want of history
    (`RoomTold::unknown_for_want_of_history`: watching began under five
    minutes ago, nothing since left alone as talk or made it busy) is kept
    as said in a quiet room when the ears' own stream shows one
    (`Heeding::ears_show_quiet`): the room microphone's readings kept
    (`ears/level`, every 100 ms, the last 30 s) outside the lines said
    (0.3 s before each to 0.7 s after) and the lamp's own answers played
    (to 0.7 s after, `Heeding::played_over`, from `Did::Said`), at least
    10 of them up to the line's end, all under 6 dB over the room's own quiet
    (`FIRST_SECONDS_RISE_DB`), and that quiet under a loud room's -30 dBFS
    (`LOUD_ROOM_FLOOR_DB`). No one level serves every lamp: each lamp's
    quiet sits where its capture level puts it, and lamp-c0a4's quiet room
    read -45 dBFS on 27 Oct (7071), where a first fixed -46 dBFS held the
    join. The lines said are every line the listener opened, told as each
    ends (`Engine::telling_lines_heard`, `Heeding::line_heard`), a line of
    no words, a hum or a sound included: on lamp-c0a4 on 27 Oct (run 1 on
    #3864) a head tap's "hm?" heard back as a line of no words, never
    judged, rose in the stream before the name and held the join. A sound
    the lamp's speaker plays outside this route's answers (the
    coordinator's head-tap hum or a cue) is left out only when heard back
    as a line or while `voice/state` says it is speaking (`ears/level`'s
    `own_voice`). And no line since the start was in another voice or seen spoken
    by two faces, and the rule held no line since the start
    (`Heeding::showing_the_first_seconds_quiet`): with no voice check
    (`LAMP_SAME_VOICE_MODEL` unset), another person's line with no face
    seen speaking it shows no second person, and held as said to nobody it
    is no talk left alone for the room told (`RoomTold::held`), so the hold
    itself keeps the join off (#3801's P3). Only a held line whose words
    could be someone's request counts: two words or more, not a short word
    or hum (`HELD_LINE_WORDS`). A line of no words, a hum, the lamp's own
    "hm?" heard back after a head tap, or the room's sounds count for
    nothing; the lamp's own words heard back are held as its echo before
    the rule (lamp-c0a4's run 1 on #3864: two lines of no words after a
    restart's head tap held the join). A wait for the Policy is no hold,
    and the bare name waiting for its request is never judged. Once set,
    the held line's mark is never cleared while the room is unknown for
    want of history (up to five minutes after the start): one held line
    turns the join off for the rest of that window, and a bare name and
    its request said after it are held, so the person may have to say it
    again (the reviewer-lead's P3 on #3864). So the bare
    name and its request, nobody in view, are joined and answered once. A
    rise like talk or a television between the lines, a loud room's quiet,
    no fresh quiet or too few readings, talk left alone, a line held,
    another voice, or two faces in view keep it held; the rule's verdicts and the room told
    are as before. The journal says `request N: the room is not known yet
    after the start, and the ears' stream stayed within R dB of its quiet
    of F dBFS outside the lines, with no talk left alone, no line held and
    no other voice since: kept as said in a quiet room for a join
    (LAMP_FIRST_SECONDS_QUIET)`. Its levels' thread is started after
    `--addressed`'s looks, and reads them beside the levels whichever
    started first (`Heeding::levels_looks`), so `--addressed` and
    `LAMP_NAME_NEAR`'s face check keep their looks with it on. With no rule (`Engine::telling_one_speaker`
    unset), nothing carries. Carried, the line is never held as a short
    sound, and the rule is given its words with the name line's words before them when
    those begin with the name ("Lamp. So I have a meeting at three…"), or
    "Lamp, " for a name misheard ("Life."), unless its own words begin with
    the name already (the kept sound put the name's words first, within
    `CARRY_AFTER_END`), and its window from the name's first sound,
    so the name heard there counts for it and earns the name note
    (`LAMP_NAME_NOTE`), as a heard name does: that window, the name's
    words and `LAMP_BUSY_CALLED_STRICT`'s evidence go to no line that
    didn't show one speaker. It is asked with those words. A line said by
    the name with no words of its own is the name's line. The kept sound
    (`KeptSound`) never carries a waiting name's piece: it asks nothing
    of who spoke.
  - **A call whose words come late** (El's P2): when the next line begins
    before the coordinator's call (`Called`) has its words back, the
    call's turn is kept running for them (`Engine::keep_a_coming_name`,
    whether its newer owner's authority or that line's start comes first)
    and says nothing itself; its words go to the line once back, within
    2.5 s of its end, and are judged as above. Words later than that carry
    nothing. A name by its words alone, with no `Called`, is known only
    once they are back, so the kept sound's own carry is left as it was
    for it.
  - **Never carried** across sessions or privacy generations, after a
    stop, past the wait, or to any line but the one begun just after the
    name.
  - **With no line in the wait,** the name alone, or a `Called` line, is
    answered as a call: `CALLED_NOTE`, its words when they begin with the
    name or are more than one, the name (`CALLED_WORDS`) for one word
    misheard or none. A short line begun with the name is answered as it
    is, not told it was a call.
  - **The journal**, counts only, never the words: `request N: only the
    lamp's name: its answer waits for the line after it
    (LAMP_SECOND_NAME_CARRIES)` (for a short line, `its W words begin with
    the lamp's name and stop short: …`), then `request M is said by the
    name of request N: <the sign>`, or `request M is not said by the name
    of request N: <why>; it is judged as its own`, or `request N: no line
    came after its name: answered as a call` (`as it is`); a call kept for
    its words, and a start that may not carry the name, say so.
  - **What it costs:** a lone call, or a short line begun with the name
    that isn't whole ("Lamp, can"), is answered up to 2.5 s later than
    before; every other line is as it was.
  - **What it doesn't do:** the coordinator isn't told of the wait, so its
    answer wait (`LAMP_WAIT_KEEPS_ANSWER`) and the body's motors judge a
    sound begun in it as they judge any sound while an answer is awaited:
    a short one may be let go, and a line admitted after the 2.5 s is not
    carried. A short line that doesn't begin with the name ("So I") waits
    only with `Called`.
  - **With `LAMP_CALL_TAKES_NEXT_LINE=on` as well** (the coordinator; off
    unless set; the first builder, 24 Oct): once the second route's final
    words for the turn in hand are only the name
    (`attention::only_the_name`: "Lamp.", "Hey Lamp,", "Lamb."), its
    answer unheard, a sound begun within 2.5 s of its end is decided at
    once (`open_name::called`). The answer wait, with its words, and the
    body's motors don't let it go. It isn't marked as going on from the
    name's line, because the second route joins the two itself. On lamp-4ace on 24
    October "Lamp, {2 s} tell me a joke." was let go so ("quiet") in 2 of
    10 pause lines with `LAMP_WAIT_KEEPS_ANSWER=on`, and the name answered
    as a call ("Yes?"); with `words`, 1 of 10, its words not back in time.
    A short line begun with the name ("Lamp, can") is still not known to
    the coordinator. The journal says `request N: only the lamp's name: a
    sound begun within 2.5 s of its end takes the turn at once
    (LAMP_CALL_TAKES_NEXT_LINE)`, and `request M: the line after the bare
    name, taken at once`.
  - **Tests:** `second_provider::tests::name_carries` (a line 1.2 s, 2 s
    and 2.4 s after "Lamp." or a misheard name with `Called`; "Lamp. So"
    and the rest 1.0 s later, asked as one line; "So I" held as before; a
    lone call; a short sound that is no name; a line 3 s later; another
    session and a stop; two faces, two voices or no rule; a call's words
    back 0.6 s and 3 s after its end; "Lamp, stop." and "Lamp, lights
    off." at once; a refused name's words never joined; the switch off)
    and
    `worker::tests::the_lamps_name_carries_only_on_a_sign_of_one_speaker`
    (a busy room, two faces, no eyes in a quiet and an unknown room, a
    print under 0.30) and
    `a_line_kept_unjudged_is_read_for_one_speaker_as_a_judged_one`, and
    `right_after_a_start_the_ears_quiet_room_lets_a_bare_name_join_its_request`
    (`LAMP_FIRST_SECONDS_QUIET`: nobody in view and the ears' quiet room
    joined, lamp-c0a4's quiet of -45 dBFS too; another voice since the
    start, a loud room's quiet, a rise between the lines, talk left alone,
    no level or readings, two faces, or the switch off held).
  - **For a lamp:** played name lines with a 0.4–2.4 s pause after
    "Lamp,", heard right and misheard, lone calls, and a colleague's line
    within 2.5 s of a call, in a quiet and a busy room: no request lost,
    lone calls answered, nothing carried to another person.
- **With `LAMP_NAME_NEAR=on`, a first word like the name, heard near, is
  the name** (off unless set; branch `first/name-carries`, 22 Oct). In
  that night's played rounds on lamp-4ace, 24 of 132 lines that began with
  the name had it written as another word (Louvre 7, Month 4, Length 2,
  Life, Light, Lane, Lynn and others; Swedish and British voices most), and
  lamp-name said nothing at its bar of 0.9 for lines whose clean files
  scored 0.97 to 1.00. With the switch, in both halves:
  - **lamp-name says its near hearings** (`--near`, 0.5 with the switch)
    on `name/near`, when each is over, with its highest score; nothing it
    says elsewhere changes ([name.md](name.md)).
  - **The second route takes a first word as the name only with both
    signs** (`named_near`): it is set off (past a greeting at most, then a
    comma or a stop, or it is the whole piece, `first_word_set_off`) and
    sounds like the name (`sounds_like_the_name`: one syllable from an L
    to an M, P or N, never a person's name of that shape a colleague may
    call at a desk, `NAMES_OF_ITS_SHAPE` ("Lynn, pass me that"), or one of
    the words written for "Lamp" on lamp-4ace, `HEARD_FOR_THE_NAME`); and lamp-name's near hearing reached 0.5
    (`NAME_NEAR_LEAST`) from 0.3 s before the line's first sound to 1.3 s
    after (`Heeding::near_name`). A word of the shape alone
    (`LikeTheName::Shape`) also needs a close face turned to the lamp
    over the line (`Heeding::turned_to_the_lamp`), judged as
    `--addressed` judges one (`addressed::Looks::between`, Sixth's P2s on
    1600b0a69): the eyes' looks from 1.5 s before the line to its end,
    kept by `Heeding::looking_for_near`, at least two, a face of
    `CLOSE_FACE_HEIGHT` or more turned to it in `FACING_SHARE` (0.4) of
    them, so neither a far face nor one turning to it after the line
    counts. Then as the rule reads it: with `LAMP_TURNED_SPEAKING`, one
    seen silent through the line is nobody (the P3); with the eyes
    reading speaking (`LAMP_EYES_SPEAKING`), one turned to it must be seen
    speaking the line too, so the listener's face never carries a
    colleague's "Leon, can you…" (reviewer-three's P3). Without speaking
    faces read, the face turned to the lamp may be another's than the
    speaker's: a close listener facing the lamp while a close colleague,
    turned away, says "Leon, pass that" lets the name carry. It is the
    same limit `--addressed` has, and only `LAMP_EYES_SPEAKING` closes it
    (Sixth's P3 on c0890e8db). People's names
    have the shape ("Leon", "Lynne", "Laine", "Lon", "Lem"), and a list of
    them always trails (reviewer-three's P2 on 194accd24); the words
    written for "Lamp" need the near hearing alone. "Louvre" set off as a
    name calls the lamp by its text too, near hearing or not
    (`attention::STAND_ALONE_INS`, 25 Oct, [attention.md](attention.md)). A line that begins
    with an earlier piece's words (`before`) is never rewritten: its start
    is not that word's (the P3). Then "Lamp" takes the word's place, so
    the rule and the model read the call, and a lone one waits for the line
    after it as a name does (`LAMP_SECOND_NAME_CARRIES`). Either sign
    alone changes nothing: "Thanks," with a near hearing, or "Right, …"
    with none.
  - **The journal** says which signs held and the near score, never the
    words.
  - **What it costs:** offline, the played lines with no name reached 0.5
    in 1 of 24 (mid-line); on AMI the bare-name model is heard at 0.5 43
    times an hour against 10 at 0.9 ([bare-name.md](bare-name.md)), before
    the word's sign. To be counted on lamp-4ace against d01–d12 and the
    AMI side talk before the voice's half is turned on.
  - **Tests:** `second_provider::tests::name_near`,
    `worker::tests::a_near_hearing_counts_only_around_the_lines_start`,
    and lamp-name's
    `a_near_bar_says_hearings_under_the_bar_and_changes_nothing_else`.
- **With `LAMP_SECOND_QUIET_HELD=on` the quiet after a pause isn't sent**
  (off unless set; needs `LAMP_SECOND_EARLY_FINALIZE=on`; the backend on
  #2135 and the first builder on #1722, 21 Oct 13:1x). Deepgram bills each
  second of audio sent, and the wait before a line's end is decided (0.4 s
  for a confident line, 0.6 s on the window, up to 1.5 s for an unfinished
  one) was streamed as quiet. With the switch (`QuietHeld`, `net/stt.rs`):
  - **after a request's pause's `Finalize`**, blocks at or under the
    absolute floor (`SOUND_FLOOR_RMS`, about -60 dBFS) are held on the lamp,
    up to 10 s; that `Finalize` has already asked for everything before it;
  - **a block over the floor**, however soft, ends the hold: the held quiet
    goes first, in order, then the block, so the service's audio stays
    whole and no word is lost. The hold starts again only at the next
    pause;
  - **at the line's end** what is held is dropped, never sent, and the end
    line says `quiet held back N ms`. A pause's answer then covers all the
    audio sent, so such a line usually ends `settled`, with nothing more
    asked for;
  - **not the stop words' stream** (its flushes hold nothing) and **not
    Flux**. A busy room whose quiet is over the floor holds nothing, by
    design.
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
  message's memory recalls for each line it's begun on. **A known
  slowness:** with `LAMP_SECOND_SCOPED_CONTEXT=on` (#1839) a turn is told for
  its admitted scope, but the early round is begun from the unscoped system
  message, so every scoped turn drops it as "the conversation changed". That
  is safe, and no faster than without the switch (the merge of `lamp-voice`
  into #1712, 19 Oct). The journal says
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
  ms after the last final>, last final <T ms before End|none>[, pause
  finalize at <P> ms, answered|unanswered at End[, sound after it],
  loudest after it <L> dBFS][, a pause finalize never answered][; closed by
  the service <C> ms after finalize, before its answer: settled=…, finals
  cover … of … ms sent, …, <W> words][, finalize answered in <A> ms to <E>
  ms|, finalize unanswered], results <N> with words, <M> without, words
  from=<settled|flush|finalize|close_stream|closed_early>[+interim]",
  the last naming which path gave the words. `closed_early` is the service
  ending the stream after `Finalize` and before its answer, with no
  `CloseStream` asked: the turn goes on with the words so far, and the
  part in `;` says when, how far the finals reached at the close and how
  many words there were, never the words, so a line that lost its end shows
  (the reviewer on #1954). The results count every
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
- **The chat is given the person's last exchanges.** Up to twelve, each said
  within ten minutes (six and five minutes before #1693: a long talk's quiz
  rules left them), as user and assistant messages after the system
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
    words are still no line. A line already whole (`whole_already`: a
    question, a stop, or a sentence past its first word) is never a bare
    call: it is asked as it is, without the note (lamp-4ace, 25 Oct: the
    name heard in "Nice. And what was my sister's name again?" was told
    as a call, and the model answered "May.").
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
  A run ends when the person changes or the worker restarts, and, with
  `LAMP_MEMORY_REOPEN_QUIET=on` (off unless set, #1693 (1)), when no turn
  was told for ten minutes (`thread::REMEMBERED_FOR`, after which the
  thread's exchanges aren't told either). The next turn then asks the note
  again as a new session, so the same person back the next morning can be
  asked about an open thread (`LAMP_MEMORY_THREADS`). Quiet is counted from
  the last time a turn was told, so a long conversation, or one turn's
  several asks (its prepared chat, then its rounds), never reopens it. The
  ask is the one a person change already makes; the old session is closed
  by the memory's own queue, off the turn. Tests:
  `worker::tests::after_ten_quiet_minutes_the_same_persons_next_turn_asks_the_note_again`,
  and the thread told after a quiet in
  `worker::memory_switches::with_the_memorys_switches_the_second_route_forgets_tells_a_thread_and_lists_rules`.
  - **After something kept** (#2540, from Fourth's replay of the forgotten
    name on #1722): a `remember` the store keeps, told or not, marks the
    run's note (`DaysClient::kept_since_note`). The run's next turn asks it
    again, the same ask, so a name told mid-run is in that run's later
    turns, not only in a recall the model may never ask once the 300 s of
    exchanges have gone by.
  - **Its journal** says "something was kept: this turn asks the memory's
    note again", once for each turn that asks again.
  - **Not asked again:** a `remember` the store refused, and a recall. A
    confirmed forget follows the existing journal-resume policy
    (`wants_fresh_session`, `LAMP_MEMORY_JOURNAL_RESUMES` and its backoff).
  - **Asked as the line begins, never on the worker's loop** (lamp-4ace,
    22 Oct 17:45: asked as the line ended, the note held the worker's one
    thread about 300 ms, missed its turn by 60 ms, and none was asked again
    for 30 s). A line begins (the engine's `Start`) before the lamp knows
    it is for it, so the note is asked then only for the scope the last
    answered turn was told in, and only when it is due: a new run
    (`opens_anew`), a failed note that may be asked again, a quiet, or
    something kept. Someone else's talk in the room while the person talks
    to the lamp asks nothing. An answered line whose scope is another (the
    first turn, a person change, the line narrowed to the room when another
    face was seen speaking it) asks for its own scope as the turn is told,
    and a turn with no line (the lamp's own written ones) asks there too.
    The read runs on a thread of its own and may take up to 2 s
    (`NOTE_AHEAD_WITHIN`). At the line's end the turn waits for it off the
    loop, never longer than what is left of 300 ms
    (`remember::FIRST_READ`) from the ask, not 300 ms more from the end.
    A note asked as the turn is told waits up to 900 ms from that ask
    (`NOTE_TOLD_WITHIN`): it has no line's time to read in, and on
    lamp-4ace (48 h to 25 Oct) all 56 came late against 300 ms, in a
    median 443 ms and at most 610 ms, so a fresh session's first question
    was answered without what the lamp keeps.
    Only one read runs at a time: while one goes on no other starts, so a
    read the camera or the microphone voided holds the next back for up to
    its 2 s, and a turn whose own read cannot start keeps the note it had.
  - **A read is not the memory's until a turn takes it**
    (`remember::NoteRead`; the reviewer-lead's and reviewer-two's P2s on
    #3016). The read opens its snapshots in the store under a new session
    name and reads the rules, but the client's session, its person, its
    sources, rules, carried links and offers are as they were. Only an
    answered line's turn takes it (`NoteRead::adopt`, from `Sessions::taken`
    once the turn waited for it): the session before is closed then, the
    journal resumes then if it may, and the follow-up joins the note then.
    A complete answer stands for its scope, however late it came, until a
    turn takes it, for at most ten minutes (`RETAINED_FOR`); an answered
    turn for another scope lets it go. A read let go untold closes its own
    session in the store, and gives back the kept mark it took as it
    began, so the next read covers that keep; a read voided by a privacy
    change is let go so as soon as it ends, before any note is chosen,
    while it is read still the one reader. A worker stopping closes a
    read's session with the open one.
  - **The follow-up only for an answered turn** (`LAMP_MEMORY_THREADS`;
    reviewer-two's P2 on #3016): the memory counts an offer as it gives
    one, and the last of a thread's three offers records it as given, so
    a read as a line begins asks none. The answered turn that takes the
    read asks it as it waits for the note, off the loop, within 300 ms
    more (`FOLLOW_UP_WITHIN`, `NoteRead::follow_up`), and a read begun as
    the turn is told asks it with the note. A line never answered spends
    no offer of anyone's. An answered turn whose read is then refused (a
    forget in those few ms) has had its offer counted, unless it is dated
    (`LAMP_MEMORY_EVENTS`): a read given a dated follow-up and let go
    untold takes it back (`follow_up_unoffered`), as a session that gives
    way with it undecided does ([memory.md](memory.md)).
  - **A dated follow-up's line** (`LAMP_MEMORY_EVENTS`,
    [memory.md](memory.md)): the line a greeting alone gets is asked for
    only once the turn has taken its read (`NoteRead::adopt`), so it comes
    after the session's note, read ahead or as the turn is told, and the
    offer belongs to the session that read opened. The note's section for it is
    cut from a note read ahead as from any other (`Told::without`): before
    the session's first line decides it, and for the rest of a session
    whose first line asked something.
  - **A forget during the read, or after it** (El's and reviewer-two's P2s
    on #3016): the Days client counts each forget or rule write sent, each
    stop of journalling and each write whose answer was lost (`changes`).
    A read stamps the count as it begins, and is taken only while the
    count is the same; the note it gave keeps that stamp
    (`remember::NoteStamp`). A read the store may have changed under is
    let go, and the cached note with it; a cached note whose stamp has
    passed is dropped before any note is chosen. The next ask is then due,
    whatever `LAMP_MEMORY_JOURNAL_RESUMES`, its backoff or the retry
    pacing say, so a forgotten note is never told again. A line held by a
    gap asks nothing and a revoked scope is told nothing.
  - **After a failed note read:** a refusal, timeout, or invalid snapshot
    leaves the note incomplete, and so does a note still not in by what is
    left of its 300 ms as the line ends (late). A room snapshot beside a
    failed personal snapshot is also incomplete. A late read that then
    completes is the next turn's, with no second read, and ends the
    failure. Otherwise the next turn asks again, with a new Days session,
    as its line begins: a memory slow once costs one turn its note. A
    second failure in a row is asked again no sooner
    than 30 s (`FRESH_RETRY`) after that attempt began, and so on while
    they fail back to back; a complete note ends the spacing. There is no
    background poll. Keeps and person changes still invalidate immediately.
    A fresh read requested by the existing journal-resume policy also
    bypasses this note backoff.
    Successful empty snapshots are cached normally. An incomplete result
    replaces the previous note, so a failed refresh cannot bring back a
    forgotten fact or another person's note. A held person change also
    clears the cache even though that held turn reads no memory. Context
    whose lease is revoked or narrowed during the read is discarded before
    being handed to either a prepared or ordinary chat. This authority guard
    covers leased contexts in both Days and legacy memory modes, after the
    session-lock wait and after composition. Legacy note loading gains no
    retry; unleased contexts retain their existing behavior.
    Counts-only journal lines say `memory note asked ahead as the line
    began; ready in N ms` (or `incomplete in N ms`, and `asked as the turn
    is told` for a read started at the end), `memory note late; retry on
    the next turn`, `memory note incomplete; retry on the next turn`,
    `memory note incomplete again; retry after 30000 ms`, `memory note
    retry complete`, `memory note let go: the memory changed while it was
    read; asked again` and `the memory changed since the note was read: it
    is asked again`; they contain no note, name or person id. The late and
    incomplete lines end with `; cause=` and one code (#1693): `timeout`
    (not in within the 300 ms; every late line), `refused` (the store said
    no, or gave no answer), `invalid` (no valid snapshot), `own_missing`
    (the room's read, the known person's own not), `busy` (the memory
    client's context could not be had) or `no_client` (no Days client);
    a code only, never the store's words. The
    host regressions are `worker::memory_note_retries` (among them the
    loop's longest gap with an answer 350 ms after the ask on a 1 s line,
    a late answer taken on the next turn, lines never answered asking
    nothing, and a voided read holding the next),
    `worker::kept_name_callback` (an abandoned refresh's keep reaching the
    next turns, voided before or after its read ends, and a resume read
    rejected after a forget never bringing the forgotten note back:
    reviewer-two's reproductions and their controls, each step waiting on
    the read's own done mark or the store's open, never on a time),
    the engine's
    `a_dated_follow_up_s_line_comes_after_the_note_read_at_the_turn_or_ahead`
    and `a_note_read_ahead_withholds_its_dated_follow_up_after_a_first_request`
    (`second_provider::tests`, with the worker's own note ahead and context),
    `remember::tests::a_note_read_let_go_untold_takes_its_dated_follow_up_back`,
    `remember::tests::a_forget_during_a_note_read_keeps_it_from_being_told`
    and, against the real store, `lamp-memory`'s
    `voice_note_ahead::a_note_read_ahead_spends_the_last_follow_up_offer_only_when_a_turn_takes_it`;
    the six-minute spoken name-retention check on #2540 is a separate lamp
    gate.
  - **Nothing is split by it** (the first builder's P2 on #2615): the ask
    opens the run's next Days session and closes the one before, as a
    person change does, since the store has no ask that reads a session's
    snapshot again under its own name. The memory's writer ends a
    conversation by its own quiet (45 s) or a change of person, never by a
    session closing while it goes on, and its journal lines name no
    session. So the exchanges before and after the ask are one
    conversation: a thing kept is kept with all of it, and "forget that"
    reaches all of it
    (`remember::tests::a_note_asked_again_mid_conversation_splits_nothing`).
  - **No switch:** it costs one note ask per turn after a keep. Tests:
    `remember::tests::a_thing_kept_marks_the_note_to_be_asked_again_once`,
    and the replay table in `memory/tests/it/voice_name.rs`, where a name
    kept with `remember` and asked a minute later in the same run is now in
    the note.
  - **Worker callback regression:**
    `worker::kept_name_callback::a_confirmed_keep_reaches_the_next_context_and_outlives_both_histories`
    starts the process's real memory client, dispatches `remember` through
    the scoped tool runner, and invokes the same `memory_context` callback
    that the worker installs. A refused keep does not refresh; held and
    revoked contexts leave a confirmed keep for the next admitted context.
    That context opens a new snapshot once, and its provider-facing messages
    still carry the synthetic name after fifteen exchanges over 700 simulated
    seconds have evicted it from both the twelve-exchange/600 s history and
    twelve-exchange thread recap. The companion
    `a_room_keep_reaches_the_next_context_without_a_person_change` keeps
    every operation in room scope, so a person change cannot cause the
    refresh or mask a missing keep marker. The local socket fixture freezes
    each session's snapshot and returns only the new name after the keep: this
    proves callback propagation, not production-store ordering of conflicting
    names or a model choosing the right name aloud. The tests record first,
    refreshed and cached callback plus message-serialization times; the note
    read retains its 300 ms budget, with a host target below 1 ms for the
    cached callback plus serialization. A complete production callback also
    performs its existing time, facts and thread work.
    `legacy_keeps_update_the_callback_without_a_days_marker` checks the
    separate queued `kept` refresh when `LAMP_MEMORY_DAYS` is unset or `ON`:
    a memory socket alone does not enable Days, and no #2540 marker is expected
    there. The 22 October 07:56–08:07 lamp-52e6 run was confirmed to have Days
    off, so its absent marker is expected and that run does not qualify the
    Days refresh. These tests guard the integration before a correctly
    configured lamp run; neither uses a provider, establishes lamp latency,
    or explains the wrong spoken recall on that legacy run.
- **Each turn is logged** as "lamp-voice second: request N ...": the words
  heard and how long after the line, the state of the words at the line's
  end (above), each chat round's first event (the first text or tool
  calls: a role-only chunk carries neither and is not an event), the
  tools called, the first answer audio, and any failure. With
  `LAMP_MEMORY_REPLACES=on`, one more line at the turn's end, names and
  counts only: `request N: tools: remember_offered=y|n|? calls=none|NAME:N,… prepared=y|n`
  ([memory.md](memory.md), the correction's journal).

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
  lamp's reply) is used by nothing here: this route runs no staff note,
  which it guards on the Live route. The same-voice check below reads
  `over_the_lamps_voice`, as Live's does. On
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
  turn; with `LAMP_MEMORY_ROOM_NOT_ASKER=on` the room's kept things are told
  as someone in the room's, never the asker's, [memory.md](memory.md)
  "Each person's own"), then the conversation thread in the room the rest leave of
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
- **The known person's name** (#2539, `LAMP_SECOND_FACE_NAME=on`, off by
  default): a turn whose person is the face the eyes know now is told
  "(The person at the lamp now is X: you know their face.)", as the Live
  route tells it (`known_faces::note`), after the memory note
  (`worker::known_name_told`).
  - **Why:** without it, the second route reads the known face only to
    choose whose memory to use. A name kept with `remember_my_face`, in
    the eyes, is recognised but never said back. That's Fourth's replay of
    the forgotten name (`memory/tests/it/voice_name.rs`, #1722, 21 Oct
    15:28).
  - **Never told:** for the room's turn, another person's turn, or a face
    gone, since the known file is fresh only 3 s.
  - **A line another face spoke** is narrowed to the room, and told none, only
    with `LAMP_SECOND_SCOPED_CONTEXT` and `LAMP_SECOND_SPEAKING_FACE` on.
    Without them the turn is the session's person, read from the same known
    file. So a voice off camera, with the known person in view, is told
    their name, as on the Live route (the reviewer-lead's P3 on #2559).
  - **Right after `forget_my_face`,** a round within the known file's 3 s
    can still carry the name, as on the Live route.
  - **The name switch, not to be confused:** `LAMP_SECOND_KNOWN_NAMES` is the
    speech to text's listening for names; this one tells the model the face's
    name.

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

## The first sound's start horizon (`LAMP_SECOND_PLAYOUT_START_MS`)

Every unit runs the voice with `--playout-start-ms 350` (`deploy/product`,
lamp-4ace, lamp-52e6). The coordinator holds an answer's first write to the
speaker until the time since its first audio, plus the audio in hand,
reaches that horizon (`lamp_audio::playout::StartGate::open`). It was set
for Live, whose audio fell up to 153 ms short of real time after its first
burst (`PlayoutStart::LONGEST_MS`). A second-route answer pays it too,
after `first_answer_audio_ms`: so it isn't in `answer_stages`, but the
person hears it.

**With `LAMP_SECOND_PLAYOUT_START_MS=0..500`** (#2135; the first builder on
#1722 at 17:46; off unless set), a reply begun while the second route's
worker is the provider takes that horizon instead (`playout_for`,
`Rig::playout_start_now`). That's the second route as the only voice, or
standing in for Gemini. A reply begun while Gemini's worker is the provider
keeps the unit's, continuity included. A value `--playout-start-ms` doesn't
take (over 500, or not a number) leaves the unit's, and the journal says so
at start. Set, it says
`while the second route answers, a reply's first sound waits for N ms of
audio, not the unit's M ms`.

**Choosing the value** from a lamp's session records (counts and ms
only): each `turn_finished` has `start_gate.horizon_us` (the horizon that
reply had), `start_gate.age_since_first_audio_us` (what the hold cost it),
`start_horizon_needed_us` (the horizon it needed to play without a gap) and
`playback_gaps`. Take the second-route replies' `start_horizon_needed_us`
near their 90th percentile. Then check `playback_gaps` and
`age_since_first_audio_us` before and after, over the played test.

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
  (`https://campaign-api.autonomous.ai/api/v1/ai/v1/deepseek`, #1939), with
  second.json's `llm_credential_file`, so the unit loads no credential.
  Since #2978 the voice takes that address itself from `llm_base_url` on the
  company relay, so the product unit no longer sets
  `LAMP_SECOND_DEEPSEEK_BASE_URL`, and the relay's routes may route the chat
  (#2882). Nor do lamp-4ace's, lamp-52e6's or the 4ace pilot's units: an
  address a unit gives is never routed (Queue #3126: lamp-4ace's r11 without
  it, 0.39 s off the median, every chat begun before the wait sound).
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
  check runs every 5 s (`PROBE_EVERY_US`; it was 30 s until #1844's run
  on lamp-06c1 found the Wi-Fi back 38 s before the lamp knew), and at once
  when the kernel's default route comes back (`/proc/net/route`, read every
  500 ms while down, `ROUTE_EVERY_US`), until one answers, when it is Ready
  again in a new session. The journal says
  `lamp-voice second: no service host answers after a turn failed on the
  network: the link is down, checked every 5 s` and `… a service host
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
      a line out as before. A literal "Lamp" or "Lamb" in a call position
      is never held by the two rules above, whatever else let it out.
      Recognizer substitutes such as "Man" or "Plan" do not bypass them
      without separate name-listener evidence. Calls without punctuation,
      after greetings or set off by a comma retain the same call-position
      parser; the three-word GOING_ON rule is unchanged. On lamp-4ace on 24 Oct the
      owner's "Lamp.", answered as a call once no line came after it, was
      also let out as the conversation going on and held as a short line
      (`busy_bar`).

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
    #1712). With `LAMP_WINDOW_FROM_START=on` (off until its played check,
    #4594), the thread's 45 s, the 20 s after the words and a call's 15 s
    are measured to when the line began, not to its judging after it ended,
    so a line begun inside them goes on however long it runs: on lamp-3e83
    on 30 Oct a follow-up begun 9 s after the lamp's words and spoken for
    13 s was held, judged 22 s after them (Migrate's launch finding 2 on
    #3997). A busy room's 15 s after the words is still counted to the
    judging. The journal's "no conversation going on" line says how long
    the line ran ("the line ran N s"), when the lamp was "last heard
    saying words, to their end," and adds ", in a busy room, where it goes
    on 15 s".
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

  Not here, as it has no part on this route: the Policy's hold of a line
  going on (`LAMP_GOING_ON_ASKS`, which also ends a busy room sooner once
  answered). The busy-room battery on lamp-4ace is its check.
- **The same-voice check** (`LAMP_SAME_VOICE_MODEL`, beside
  `LAMP_SPOKEN_TO=on`, [same-voice.md](same-voice.md)): as on Live, and
  only as the lamp's only voice (a standby beside Live loads none). Here
  `LAMP_QUIET_GOING_ON_SAME_ONE=on` or `LAMP_GOING_ON_NOT_OWN=on` start it
  too, without `LAMP_SPOKEN_TO`: their sign of someone else reads its
  prints, and lamp-52e6 ran them with no print all run (#2341). Without
  `LAMP_SPOKEN_TO` the prints feed only that sign and a trusted session's
  hold (below): this check
  (`Heeding::voice_going_on`) ends no conversation, so a person's own short
  follow-up that reads low still goes on. With none of the
  three the journal says "the same voice is judged only with
  LAMP_SPOKEN_TO, LAMP_QUIET_GOING_ON_SAME_ONE or LAMP_GOING_ON_NOT_OWN on:
  not judged". Tests: `worker::tests::same_voice_starts_for_the_rules_that_read_it`,
  `worker::tests::a_low_print_without_spoken_to_keeps_the_conversation_going`. The
  worker starts the same `lamp-same-voice` child; the engine hands it every
  chunk of a spoken line (`Engine::hearing`), of which `Voices` keeps the
  first ones to print (a line begun over the lamp's voice is not printed),
  and at the line's verdict (`Heeding::judge`) a
  line in another voice than the conversation's is no part of it: "request
  N: another voice than the conversation's (cosine …): no conversation is
  going on for it", and the room counts it as talk past the lamp. A late
  print of another voice ends the conversation at the next line; after
  another voice's line held, a question in the conversation's own voice
  goes on and a statement in it is talk past the lamp. Off unless set;
  the route judges a line once its words are final, so its print is
  usually back by then. Test:
  `worker::tests::another_voice_going_on_is_held_and_the_conversations_own_goes_on`.
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
  an earlier conversation, call `recall`. Then
  `worker::RECALL_BEFORE_NOT_KNOWING` (#4339, lamp-06c1's third long run on
  30 Oct: 3 recalls in 96 turns): before saying it doesn't know or remember
  something about their own life (plans, people, work, what they talked
  about earlier), call `recall` first, say it doesn't remember only if
  that finds nothing, and use what it finds only to answer the question. The memory note
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
  second: the lamp's place is … (LAMP_PLACE)`. With no place set, as on a
  product lamp, it is told "You don't know where you are beyond your time
  zone." Either way it is told the area near the person isn't known
  (`local_time::AREA_ASKED`): for something near them, or a place it isn't
  sure it heard right, it asks which area before it searches, never
  picks one, and says where it looked when they didn't say. The weather and
  the time stay for where the lamp is, never asked about (#4337: on
  lamp-3e83 on 29 Oct a district speech to text misheard was taken for
  another, said as fact, kept for the rest of the talk, and searched).
  Neither is told while the owner's place from the app is kept
  (`LAMP_OWNER_PLACE=on`, below).
- **The owner's place from the app** (`LAMP_OWNER_PLACE=on`, #4333,
  [owner-place.md](owner-place.md)): a spoken line whose words need a place
  ("near me", nearby, the weather, opening hours:
  `owner_place::asks_for_a_place`) is asked with a note beside its words,
  as the staff note is (`Engine::noting_the_place`, `worker::place_noting`):
  the area's name and its rounded position, read from
  `/etc/lampOS/person/place` for each such line. No other line and no text
  is told it; none kept, nothing is told and the model asks where. While one
  is kept it stands for the lamp's place: `LAMP_PLACE` is not told beside
  the time, nor `local_time::AREA_ASKED` (`worker::place_line` tells
  `local_time::place_told_owner_kept` instead: where they are comes beside
  a request that needs it; ask only when unsure of a place heard; say where
  it looked when none was named),
  the weather's function called with no place answers for its
  position by its name, and its name's parts join the key terms speech to
  text listens for (read every 10 s). The journal says `request N: its
  words need a place; the model is told the owner's place`, never where.
- **A line let out only as the conversation going on** (with
  `LAMP_SPOKEN_TO` and `not_for_me` offered, `LAMP_TOOLS=on`): no name,
  nobody turned to the lamp, not said close to it, no answer to its own
  question (`attention::Attention::played_on_the_conversation_alone`). It is
  asked with `worker::CONVERSATION_ALONE` beside its words: it may be
  people's talk, and if it is plainly said to someone else or a remark to
  the room, the model calls `not_for_me`; a follow-up is the model's. A
  line whose words ask for the lamp's own work is not told so
  (`Heeding::asks_its_work`): the schedule's words (a timer, a reminder or
  the lamp, `going_on_asks::names_the_schedule`) or "remember" or "forget"
  (`worker::MEMORY_WORDS`), in the whole chain's words for a request going
  on from one cut short ("Remind me" | "in two minutes to stretch", Eighth
  on #2054). Either word anywhere counts, so people's own talk with one in
  it ("I forget his name") loses this note too, and gets the decline note
  (`DECLINE_NOTE`) in its place, which still asks for `not_for_me` on
  talk not for the lamp. On lamp-4ace on 20 Oct, told the note,
  "Remember that my favourite colour is green." got `not_for_me` and
  nothing was kept (#1712, the first builder at 04:54 and their yes at
  05:01). Nor is a line begun over the lamp's own answer
  (`ProviderInput::Start`'s `over_the_lamps_voice`, not over the wait
  sound; `Heeding::began_over`, which the rule's hearing hook now learns
  whenever `not_for_me` is offered): a cut-in over its voice is said to
  it, and it gets `DECLINE_NOTE` like any line without the lamp's name.
  On lamp-52e6 on 24 Oct, told the note, "Sorry, can you say that last
  part again?", said over the lamp's story, got `not_for_me` and silence.
  Two other people's talk over the lamp is left to `DECLINE_NOTE`'s
  "plainly people talking to each other", and a yes to someone else
  over it to `LAMP_OTHERS_ANSWER`; the busy-room holds are unchanged.
  With `LAMP_CUT_IN_TOLD=on` such a line is told `cut_in_told::NOTE` with
  no note saying the opposite beside it. The journal adds `, only as the
  conversation going on: the model is told so` to `its answer is played`,
  or `, only as the conversation going on, begun over the lamp's answer:
  the model is not told it may be people's talk`. Gemini Live hears who is talking to whom; this
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
  someone else, set a five-minute timer. A line whose words name the
  schedule runs them, as Live's busy-room rule lets it (#1522): "Remind me
  in two minutes to stretch." was held here and set on Live. A line
  answering the lamp's own question is never let out this way, so it sets
  what it asks for. And such a line of one or two
  words, with no question in it and not a short word said to the lamp
  (`not_speech::short_word`: "louder", "thanks", "stop"), is held before
  the model sees it (`worker::LONE_WORDS`): "What's the price of gold?"
  heard as "Surprise" was answered "Oh! I do like a surprise." "And
  Paris?" is a question and goes on. **Someone else's answer**
  (`LAMP_OTHERS_ANSWER=on`, `crate::others_answer`, #2136): such a line that
  begins as a yes ("yes", "yeah", "yep", "yup") and asks nothing (no `?`),
  began over the lamp's own answer (its Start's `over_the_lamps_voice`) and
  answers nothing the lamp asked is held, `held back: it answers nothing the lamp asked and began
  over the lamp's answer: someone else's answer`. In the 11:10 played test
  on 20 Oct, A asked for a story by name; over it B asked "Daniel, are you
  coming to the meeting at two?", never admitted, and A's "Yes, give me five
  minutes." was answered "Five minutes, counting." with no timer set. When
  the rule heard the question to the other person, it already holds the
  reply (`Attention::began_at`); this is for when it heard only the reply.
  A "no" over the lamp's answer is far more often a correction to it ("No,
  the fox one."), and a question ("Yes, but who was he?") follows its
  answer, so both go on (the reviewer's P2 on #2175). A "Yes, go on." to an
  offer of the lamp's that isn't a question, said over its answer, is held
  too, and a held cut-in has already stopped the lamp's answer.
  **Only the one it answered** (`LAMP_GOING_ON_SAME_ONE=on`,
  `crate::going_on_same_one`, #2191): in a busy room, such a line is held
  unless it answers the lamp's own question or, with
  `LAMP_SAME_VOICE_MODEL`, the same-voice check found it in the
  conversation's voice, `held back: a busy room: a conversation with the
  lamp is going on, but nothing shows the line is from the one it
  answered: not a reply to its question, not the conversation's voice`. On
  lamp-52e6 on 20 Oct at 15:2x, after one named line was answered, four to
  five lines of two others' office talk were answered as the conversation
  going on. A line ending in `?` is no exception (office talk asks too); a
  face turned to the lamp still lets a line out, and a quiet room is as
  before. Without the voice model, a follow-up from the person it answered
  that answers no question of the lamp's, with no face turned, is held too
  ("Lamp, what's the weather?" answered, then "and tomorrow?" said looking
  at the screen); the lamp run that turns the switch on must count that
  follow-up as well as the office talk, so the cost is measured, not only
  the talk held. With or without `LAMP_SPOKEN_TO`
  (`Attention::played_only_as_going_on`): it first read a mark only
  `LAMP_SPOKEN_TO` sets, and lamp-52e6 and lamp-4ace run without it.
  **A line turned to someone else** (`LAMP_TURNED_AWAY=on`,
  `crate::turned_away`, #3882): in a busy room, a line let out only as the
  conversation going on, answering no question of the lamp's, is held when
  it turns to someone else: a role called at its start ("Mom,", "Guys,",
  "Mẹ ơi,"), an object asked for in the words of a request between people
  ("pass me", "hand me", "lend me", "đưa giùm"; never bare "grab the",
  "fetch the" or "pass the", which people say to an assistant too), or a
  line that is all errand between people ("Do you want coffee?", "I'm going
  downstairs"; with a command after it, "I'm going downstairs, turn the
  light off", it plays). The signs are in English and Vietnamese, `held
  back: a busy room: a conversation with the lamp is going on, but the line
  turns to someone else: a role called, an object asked for, or an errand`.
  On lamp-4ace on 27 Oct, on stable 7632, "Can you pass me the charger? The
  white one." and "Do you want coffee? I'm going downstairs." were answered
  after a conversation with the lamp; desk talk held went from 5/6 to 1/6
  on the same build, as the lines fell inside or outside the 15 s window
  behind answers of different lengths. A follow-up ("And what about
  tomorrow?") shows none of the signs and plays.
- **A reply never says it kept something with no `remember` call**
  (finding K on #3504: lamp-06c1, 27 Oct, "Kept: the meeting room is
  called Orange." with `calls=none`, 2 of 4 remembers, the model copying
  its own earlier acknowledgement). On by default, on every turn of this
  route offered functions (not a quoted add-on's or a reminder's). Each
  round's reply opening is held only while its words could still begin a
  claim (`remember::claims_a_keep`: after up to three of "okay", "sure",
  "got it", "done"…, a keep that takes what was kept: "Kept:", "Kept.",
  "kept that/it", "I've kept that", "I'll remember that/this/it", "I've
  saved it", "saved that", and "đã ghi nhớ", "mình sẽ nhớ", "đã lưu"
  with anything after them). The softer claims count too (K3: lamp-06c1,
  28 Oct night, "Noted — you work at a library now, not the bakery." and
  "All right, I'll keep it in mind." with no call): "Noted" with what was
  noted after a dash or colon, or "noted that"; "I'll keep it/that in
  mind", "I'll bear that in mind", "I'll hold/hang on to that"; "mình nhớ
  rồi", "đã ghi nhận". A soft claim is a claim only in a round with no
  calls: beside any call it confirms that call ("Noted — timer set for ten
  minutes." beside `set_timer`) and is said as it comes (Reviewer-three's
  and Reviewer-two's P2 on #4137). At the first word that cannot begin a
  claim, it goes on as before. "I'll keep it short", "I'll keep the light on",
  "I'll remember to remind you at five", "Saved your playlist",
  "Remembered! …", "Got it", "Got it, thanks", "Noted." and "Noted,
  thanks." claim nothing (the reviewer-lead's P2 on #3628). A claim is held to the round's end: beside a `remember`
  call it is said. With none, its words are not said: beside other calls,
  those run and the next round is told nothing was kept
  (`KEEP_CLAIMED_UNKEPT`); alone, the model is told once and asked again,
  and a second such reply, or one in the last round, is said as
  `NOT_KEPT_LINE` ("I didn't keep that. Tell me again and I'll remember
  it."). Not held after any call of the same request but a `recall` ran
  unrefused, so a reminder set in round 1 may be confirmed in round 2,
  "Got it, I've saved that for five." (reviewer-three's and the
  reviewer-lead's P2 on #3628). Tests: `a reply never says it kept something with no remember
  call`, `a turn offered no function is never held for a claim` and
  `remember::tests::a_reply_that_says_it_kept_something_is_known_by_its_opening`.
- **A reply never offers to keep what its `remember` call keeps** (finding
  K2 on #3504: lamp-06c1, 28 Oct, "Want me to keep that as the change?
  Kept — library now, not the bakery." in one breath). On by default, on
  every turn of this route. A sentence that offers to keep
  (`remember::offers_a_keep`: a question asked by the lamp, "me", "should
  I", "shall I", "can I", "may I", with "keep/save that, this, it, those,
  them" or "remember/note" and what it takes; and "mình/tôi/em … nhớ/lưu
  … nhé/không/có muốn") is held, with the sentences after it, to its
  round's end (`remember::OfferHold`). Beside a `remember` call, or after
  one of the same request ran, the offer is not said and the rest is, in
  its order; with none, all of it is said where it was. "Shall I keep the
  light on?", "Do you want me to remember to remind you?", "What should I
  call you?" and "Bạn có nhớ không?" are no offer. The `remember`
  description adds "never also ask whether to keep it" (35 bytes). A
  `remember` call the store then refuses still leaves its offer unsaid.
  The early cut of a reply's first words (a clause, or
  `cutting_first_words`) waits while the opening may yet be an offer
  (`remember::may_offer_a_keep`: "Want", "Do you want", "Want me to keep",
  "Should I", "mình … nhớ"), so "Want me to keep that as the change?" is
  never said as "Want me to keep" first; "Let me check", "Can I help" and
  "Do you want to hear" are cut as before (the Reviewer-lead's and
  Reviewer-two's P2 on #4076). A turn ended on `NOT_KEPT_LINE` says what
  was held first (Reviewer-two's P3).
  Tests: `an offer to keep is not said beside the remember that keeps it`,
  `an offer to keep is not cut early into first words`, `a held offer is
  said before the line that nothing was kept`,
  `remember::tests::an_opening_that_may_yet_offer_to_keep_waits`,
  `remember::tests::an_offer_to_keep_is_known_by_its_question` and
  `remember::tests::an_offer_held_to_the_round_end_is_left_out_only_beside_a_keep`.
- **"Not for me" written as words, with `LAMP_NOT_FOR_ME_WORDS=on`** (off
  unless set): on a line told `worker::CONVERSATION_ALONE`, a reply that
  is "not for me" (`not_speech::opens_not_for_me`) ends as the
  `not_for_me` call does (`Stop::Declined`): nothing is said, no
  announcement, and the line is told held back. It is "not for me" only
  when those words are the whole reply, or are followed straight away by
  the refusal's own clause: "that's about me", "that is about me", "it's
  about me" or "not to me" (`not_speech::REFUSAL_CLAUSES`), or by "to
  answer" or "to reply" at a sentence end: a '.', '!' or '?' next, past
  closing quotes and whole speech tags, or the reply's end
  (`not_speech::SENTENCE_END_CLAUSES`). Case, punctuation, quotes (single
  quotes enclosing the words too; an apostrophe inside one, "me’s", is no
  "me"), dashes, whitespace and leading speech tags (`[softly]`,
  `LAMP_SECOND_SPEECH_TAGS`) are skipped. Anything else after the words is
  an answer and is said: `CONVERSATION_ALONE` tells the model that a
  follow-up to the lamp is for it, so "Not for me, thanks, I'm more of a
  lo-fi lamp." (to "do you like jazz?"), "Not for me to say, but it's
  lovely." and "Not for me to answer, but I think it's lovely." are
  spoken. On lamp-4ace, r06, request 131, "That lamp is
  cute, where did you get it?" got "Not for me — that's about me, not to
  me.", spoken in two pieces; the model writing those words and then
  calling `not_for_me` (request 86) was already silent. The reply's text
  is held from the voice until that is known, then goes on as one piece;
  a line with the name, a call, a turned face or an answer to the lamp's
  own question never has that note, and nothing of it waits. The hold has
  no fixed bound: it ends at the first text that decides it (a word that
  is not the next of "not for me" or of a clause, a clause's last word,
  or for "to answer" and "to reply" the character after it), or at the
  stream's end. A reply whose first word is not "not"
  is decided at once. "Not sure, …" waits for "sure"; "Not. ", "For ",
  "example, …" waits for "example"; "Not for me" with only punctuation
  after it waits for the next word or the end, so "Not for me." alone is
  held to the end of the reply; a speech tag still open waits for its
  `]`. The journal says `its reply opens with "not for me" on a line let
  out only as the conversation going on: taken as not_for_me, nothing
  said (LAMP_NOT_FOR_ME_WORDS)`. Its cost to the first audio (the
  announcement, the first `ProviderOutput::Audio`; a piece's transcript is
  emitted before its audio), measured on the test clock with a piece (a
  word) every 40 ms, cut at the first clause or at four words, the same
  either way
  (`the_check_costs_the_first_audio_only_the_pieces_its_opening_waits_for`):
  0 ms in six of seven typical replies; 40 ms for "Not sure, maybe ask
  the office."; 120 ms for "Not for me, thanks, I'm more of a lo-fi
  lamp." and "Not for me either.", and 160 ms for "Not for me to say, but
  it's lovely." and "Not for me to answer, but I think it's lovely.", each
  held to the word that decides it.
- **A decline says nothing and starts no conversation, with
  `LAMP_DECLINE_SILENT=on`** (off unless set, #3945). Two gaps it closes.
  Words the model streams before its `not_for_me` call ("Sure, I'll stay
  out of it.") were said up to the first clause or four words
  (`LAMP_SECOND_FIRST_WORDS`) before the call, known only at the round's
  end, ended the turn: the piece's transcript went out as soon as the
  stream paused between chunks, so the thread kept it as the lamp's words
  (a conversation going on for 45 s) whether or not its audio began before
  the call cancelled the saying half; and "not for me" written as the reply was taken as
  the decline only on a line told `CONVERSATION_ALONE`
  (`LAMP_NOT_FOR_ME_WORDS`), never on the other lines told `DECLINE_NOTE`
  (a face turned to the lamp, a line begun over its answer, one asking for
  its work). With the switch, on a spoken line told `DECLINE_NOTE` or
  `CONVERSATION_ALONE`, round 1's words, and its announcement, are held to
  the round's end (`Engine::declining_silently`). Beside a `not_for_me`
  call none is said, whether the decline is taken or, said by name to a
  face (`LAMP_NAMED_ALWAYS`), the model is asked once more; a reply that
  is "not for me" (`not_speech::opens_not_for_me`, whole) is the decline
  (`Stop::Declined`); anything else goes on as one piece, cut into
  sentences as streamed words are. The journal says `its words before
  not_for_me are not said (LAMP_DECLINE_SILENT)` or `its reply is "not for
  me" on a line that may not be for the lamp: taken as not_for_me, nothing
  said (LAMP_DECLINE_SILENT)`. Nothing waits on a line said by name
  (`BY_NAME_NOTE`), a written turn, or any round after one that called
  something. Nor does a line judged in a quiet room with one voice and at
  most one face (the first builder's call on #3945: on lamp-c0a4 the hold
  made 26 of 39 played lines wait, in a room where every line was for the
  lamp): round 1's words wait only where the rule judged it in a busy room,
  in another voice than the conversation's (`Seen::other_voice`), or with a
  second face seen speaking it (two faces, or one not the caller's while a
  conversation has one; `worker::others_may_talk`,
  `Heeding::holds_round_one`, given the engine as
  `Engine::holding_round_one_when`), kept with each request (the last
  eight judged so), so a newer line judged before an earlier request's
  round asks does not unhold it (Reviewer-three's P3s on #4458). Two faces only in view, none seen
  speaking, count as the busy room `LAMP_NAME_OR_FOLLOW_UP` makes of them,
  and not without it. Elsewhere its words stream as with the switch off
  (journal: `a quiet room, one voice, no second face: its words are not
  held (LAMP_DECLINE_SILENT)`); a `not_for_me` after them says its first
  words as with the switch off, and its decline is still told the rule.
  So a decline in a quiet room after words still undoes the call and the
  carry though the lamp spoke: its words told keep nothing going
  (Reviewer-three's P3s on #4458).
  Without the rule's hook (a harness), every such line waits, as before.
  Tests: `round one waits only where the rule says others may talk`
  (first audio 80 ms off, 80 ms on in a quiet room, 360/480 ms where
  others may talk) and `round one is held only in a busy room or with a
  second voice or face` and `round one hold is kept with each request`
  (worker). What waits: the first audio of round 1
  on a line that may be declined, where others may talk, for the rest of that round's reply; measured on the test clock
  with a word every 40 ms and four first words
  (`the_hold_costs_the_first_audio_only_on_lines_that_may_be_declined`),
  a line said by name 80 ms either way; a line that may be declined
  80 ms off and 360 ms on for "Sure, it was a gift from a friend." (eight
  pieces), 480 ms on for an eleven-piece reply: the whole of round 1's
  reply, which the first clause no longer cuts short. A reply with no
  words before its call costs nothing, and the wait ends at the round's
  end however long the reply. On a lamp the wait is the model's time to
  write the rest of round 1's reply, unmeasured there. A decline, unless a tool ran for the line first, is told
  to the rule (`Did::Declined`, `Heeding::declining_silently`): what the
  line's let-out began is undone, so the next line finds no conversation
  going on from it. Its call carries nothing (`called_at` and the caller's
  faces as before it), the `LAMP_GOING_ON_NOT_OWN` chain its let-out ended
  stands again, the `LAMP_SAME_SPEAKER_CARRIES` carry is as before it, and
  any words of its answer told (`Did::Said`; none, with the hold, on a
  line that may be declined) keep nothing going, while another answer's
  words told meanwhile still count. Only the line let out last is undone:
  one declined after a newer line was let out changes nothing. What
  changed after the let-out is kept (#4417): a stop or a privacy change
  ends the carry and a privacy change drops the caller's faces, a later
  held line's reset (two colleague lines, the caller turned away, a voice
  known late as another's, the window over) stands, and another answer's
  words, a colleague's line held with the carry standing and a carried
  line's end count in the restored carry window too, while the live window
  is still that conversation's, begun for the same request: a new caller's
  let-out begins another, whose lines stretch none before it (#4429). An
  answer leaves the request a window was begun for, as the live window
  takes an answer to a line its let-out took off the chain that the copy
  kept for the decline does not (#4435). Not undone:
  the room's count of talk past the lamp, which the call reset
  (`RoomTold::called`), and the held colleagues of the carry. Journal:
  `declined by the model: no conversation going on from it
  (LAMP_DECLINE_SILENT)`. Tests: `a decline with words streamed before the
  call says nothing`, `a not for me written on a line not let out alone
  says nothing`, `the switch off and a line said by name keep todays
  behaviour`, `the hold costs the first audio only on lines that may be
  declined` (`second_provider/tests/decline_silent.rs`), `a declined
  line leaves no conversation going on for the next line`, `a decline does
  not bring back a carry stopped after its let out`, `a decline does not
  bring back the callers faces after a privacy change`, `a held lines reset
  after a let out stays after its decline`, `another answer while
  carrying is kept in the restored carry window`, `a line carried after a
  let out keeps its end in the window its decline puts back`, `a late
  print held after a let out stays in the window its decline puts back`,
  `a new callers held line does not stretch the window its decline puts
  back` and `an answer to an earlier line after a let out keeps the window
  its decline puts back` (worker).
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
- **Only the one it answered, in a quiet room too**
  (`LAMP_QUIET_GOING_ON_SAME_ONE=on`, `Heeding::going_on_quiet_only_for_the_same_one`,
  the first builder at 21:03 on Queue #1722; off unless set): #2191's
  busy-room rule (`LAMP_GOING_ON_SAME_ONE`) in a quiet room, turned round
  by the first builder at 01:21 on Queue #1722. A line let out only as the
  conversation going on (no name, no call, no face turned, not said close
  to it) is held only when it is surely someone else's and answers no
  question of the lamp's (`Heeding::someone_else`, the one check
  `LAMP_GOING_ON_NOT_OWN` reads too): with `LAMP_SAME_VOICE_MODEL`, another
  voice: a print scored under 0.30 against the caller's
  (`same_voice::DIFFERENT_BELOW`, the first builder at 02:33: on
  lamp-4ace's mic2 on 21 Oct at 02:29 the same voice in a quiet room scored
  down to 0.35, other voices up to 0.29); no print, a line begun over the
  lamp's voice or under 1.5 s, is never another voice and goes on (02:19).
  #829's own check reads the same bar (one bar since the first builder's
  03:50 on #1722; [same-voice.md](same-voice.md)) and, with
  `LAMP_SPOKEN_TO`, holds a line under it before this rule reads it
  (without it the print feeds only this rule, #2428, and a trusted
  session's hold); with the eyes' speaking faces
  (`LAMP_EYES_SPEAKING=on` on lamp-eyes, read from `humans/faces/tracked` on
  the bus), a face seen speaking it not turned to the lamp, none of them the answered
  caller's, whether or not the caller's face is known (the first builder at
  01:51 on #2341: people talking to each other face each other; someone
  following up with the lamp faces it). After a call no face was seen
  speaking, the first face seen speaking a line going on turned to the lamp
  becomes the caller's, so it stays theirs said looking away; until then,
  follow-ups said looking away are someone else's. The costs: two people
  both facing the lamp while talking to each other go on, and after an
  unseen call a bystander who faces the lamp while answering someone else
  can become the caller. `held back: a
  quiet room: a conversation with the lamp is going on, but the line is
  someone else's: another voice than the conversation's, or a face speaking
  it, not turned to the lamp, that is not the one it answered`. With no sign of someone else the line
  is the caller's follow-up and goes on: on lamp-4ace at 01:03, with no face
  and no voice model, reading no sign as someone else held one person's own
  follow-ups ("Can you make your light a little warmer?", "What do you see
  right now?", "Remember that my favourite colour is green.", "Remind me in
  two minutes to stretch.", "What's my favourite colour?", "What time is
  it?"), day lines 6/12. So without the voice model and the speaking faces
  the rule does nothing, and two people's talk after a call goes on as
  without it. On lamp-52e6 at 20:1x (#2301's runs), #829's two-people scene
  opened by "Hey Lamp, what time is it?" had its first three or four talk
  lines answered as the conversation going on before the room was judged
  busy; with a voice model or speaking faces those are held. With or
  without `LAMP_SPOKEN_TO` (`Attention::played_only_as_going_on`). The
  caller's face ids are passing ids kept in memory only, each with the
  eyes' boot (`SpeakingFaces::speakers_with_boot`), so an id from before
  lamp-eyes started again matches nobody; they go when a line comes with no
  conversation going on, and on a privacy change (the camera closed or
  opened, a mute or unmute, or either's privacy generation changing, as the
  worker installs each authority: `Engine::forgetting_on_privacy`,
  `Heeding::privacy_changed`), with the eyes' looks kept for them. No face
  id outlives a camera close. A passing id lasts only while the face's box
  stays within 1.5 s of where it was (`SAME_WITHIN_US`): the caller out of
  view longer comes back with a new id, which is theirs again only turned to
  the lamp; said looking away, that follow-up is held. The lamp run that
  turns it on where lamp-eyes reads speaking passes only if the caller's
  follow-up after a call the eyes did not see, and after the caller looks
  away 2 s, is answered, with a second person facing the lamp first. The busy room's `LAMP_GOING_ON_SAME_ONE` is not
  turned round: a busy room is itself the sign that others talk. The second
  route only; the Live route is unchanged.
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
  window then begins at its own first sound, not where the first piece's
  did (#4226): that piece's time is when its line began, not its last 1.5 s,
  so on lamp-c0a4 on 29 Oct "Lamp, I want to cook something new this
  weekend." going on from desk talk read its name about 4.9 to 5.6 s in and
  was dropped 3 times in 10. A name in the carried sound still calls the
  lamp by the words it gives the line, and its hearing, come late, counts
  from up to `NAME_BEFORE_US` before it. The old words are not put before
  its own as well.
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
  itself and they are let go. With `LAMP_NAME_SAID_TO_A_PERSON=on` they are
  let go, not answered in a held line's place, when they open with a word
  said to a person ("Ma'am, could …") or the held line does ("Sir, what time
  is this?") and they don't call the lamp; their unplayed answers leave the
  thread as a joined line's do (#3945, attention.md). On lamp-4ace on 19 Oct at 15:56 the weather
  and a race result were searched, then ended by the owner's next line,
  which was held, and were never said. Words answered again may still be
  searched again, but are written once (`WriteOwner`; lamp-06c1, 26 Oct,
  build 6963: one "remember my favourite planet is Saturn" was kept three
  times, by two empty held lines answering it again): the first request to
  try `remember`, `forget` or `forget_everything` for a line's words owns
  their writes, and a held line answering only those words again is told
  `CARRIED_MEMORY_WRITE` instead, whether the first write was kept, cut
  before its reply, or lost. A held line whose own new words join them
  owns its writes afresh. The owner is claimed only by a write that is
  sent: one `run_tool` refuses before sending (a line held only by a
  face's gap, or scoped tools with no scope) claims nothing, so a later
  answer to the same words may still write. A held forget their yes runs
  claims it too (Reviewer-two and reviewer-three on #3614). A memory call
  on a line held only by a gap is refused before either runner, the
  requested-audio one included (Reviewer-two, 03:38). Test: `an
  empty held line does not repeat a carried memory write` (a remember, a
  forget their words asked for, a first remember refused on a gap, with
  and without requested audio) and `a held forget their yes runs owns the
  yes words writes`. **Stopped words never come back**
  (a reviewer's P2 on the join, 22 Oct: "Lamp, play some jazz", a hand
  stop, then "what time is it?" was asked as one line): a request ended
  with the authority holding no owner (a hand, head or spoken stop, the
  mute, a gap, its turn over) is stopped, not cut by a newer line
  (`Engine::stop_through`), and its words, with any cut words not yet
  taken, are let go: `request N: stopped before its answer: its words are
  let go, not kept for the next line`. So are they at the first line after
  a stop (`after_stop`), and when the line that cut them says stop or
  "never mind" ("never mind" anywhere, or a stop word or pair of the stop
  words' list in three words or fewer): `request N's words, cut short, are
  let go: WHY`. Held or let out, those words are then neither answered in
  its place nor joined to it.
  - **The same person going on** (`LAMP_JOIN_CUT_WORDS=on`; with
    `--attention`; the first builder on Queue #2602, 21 Oct 17:22). Let
    out, the new line is asked with the words it cut short before its own,
    as one line, when one person said both: `request M goes on from
    request N's words, cut short D ms before it began: WHY; both are asked
    as one line (LAMP_JOIN_CUT_WORDS)`. Only when it began within 3 s
    (`JOIN_WITHIN`, longer than the sound's 1.5 s since a join needs one
    person's sign too: on lamp-4ace on 21 Oct a 1.2 s pause began the
    second half 1635 ms after the first's end; 2.5 s until 22 Oct, below)
    of that line's end, in the
    same session, neither that line's sound nor its words began it
    already, and the words were that line's own: words answered in a held
    line's place are never joined with the line after it. A held new line
    joins too: the cut words are answered in its place, as before, with its
    own words after them. Otherwise the journal says why: `request M: not joined with
    request N's words, cut short by it: WHY (LAMP_JOIN_CUT_WORDS)`, with
    the gap, the cosine, which line has no print, or no one face seen. One person
    is the two lines' prints at 0.70 or over (`same_voice::SAME_FROM`, over
    every pair of two voices on 18 Oct's labelled set), when the same-voice
    check runs for another rule; or else one face, the same passing id,
    seen speaking each line and no other face speaking either. **The eyes
    come first:** more than one face seen speaking either line, or each
    line's face another, is two people, whatever the voices say (the
    review of #2620: 0.70 is 0.01 over the office set's highest two-person
    pair, and a family's voices may be closer). Prints under 0.30 are two
    people whatever the eyes saw, and a print still to come is waited for
    up to 300 ms (`JOIN_WAIT`). A camera
    closed or a mute forgets the lines' faces, as it does the caller's. Why: on lamp-4ace on 21 Oct cut-short words were let go 31
    times, most in two-person scenes, where answering the second person
    alone is right; Eighth's long request with a pause lost its first half
    the same way. Risk: two people with one face in view, one silent, and
    a speaking read that takes one for the other, are joined. Found on
    lamp-4ace on 21 Oct at 18:36: the lamp's wait sound, played 1.5 s
    after a line with no chat begun, fell into the person's pause, so the
    second half began over the lamp's sound and was never printed; the
    eyes are then the join's only sign.
  - **A chain of pieces** (the same switch; the first builder's clean
    round on lamp-4ace, 22 Oct: requests broke into 3 or 4 pieces at
    natural pauses, "Lamp. / I've been feeling a bit tired lately. / Like,
    every afternoon, / Why could that be?", and the join reached only the
    piece cut last, so the tail was answered alone, "Why could what be?").
    A line let out is asked with **all** the words cut short before it in
    the conversation, oldest first: each piece begun within `JOIN_WITHIN`
    of the one before's end, none of their answers heard, every link
    judged one person's as above, and a stop still drops them all. The
    words kept with a line in flight are its chain (`Carried::parts`), so
    each join carries the ones before. A piece cut short before its words
    came back, its sound carried into the next (`CARRY_AFTER_END`), passes
    the link on: that next line goes on from the piece before it
    (`KeptSound::link`). A piece cut after its words came back but before
    it was asked (a Policy wait, a print's wait) goes on in the chain too,
    only in a room that isn't busy (reviewer-three, 23 Oct: there such a
    piece is as likely talk to someone else),
    its link judged at the next line's verdict (`Engine::chain_heard`,
    `request N: cut short after its words came back, before it was asked;
    they wait for request M's verdict, a chain of K lines`); a held line's
    never, nor one that says stop or "never mind", which lets the chain
    before it go in any room, busy or not (reviewer-three's P2, 23 Oct:
    else a held line after it answered the stopped request again). Every
    older piece is judged against the line joining it
    too, not only its neighbour, each pair once and all within the join's
    one wait for a print (`JOIN_WAIT`). While no pair says two people, the
    whole chain is asked. Once one does (a link no verdict judged, or a
    face or a voice two pieces apart: a reviewer's P2 and reviewer-three's,
    22 Oct: X seen saying the first piece, nobody the short middle one, Y
    the last, passed link by link), only the pieces one face seen speaking
    both or one voice shows to be the joining line's speaker's are kept,
    never one that passed only on no sign of a second person (`the chain
    cut short has two people in it, requests A and B: WHY; of its pieces
    only those one face or voice shows the same speaker said are kept, K
    let go`). A held line answering a chain in its place judges its pieces
    against the newest by the voices or one face only, the chain in flight
    through that wait. At most 60 words
    (`CHAIN_MOST_WORDS`) and 20 s from its oldest piece's start
    (`CHAIN_MOST_FOR`): past either the oldest pieces are let go. The join's line gives the count: `K lines are asked as one`, with
    `its N oldest pieces let go past 60 words or 20 s` when cut. The rule
    keeps each line's faces and room for the last 8 lines (`SPOKE_KEPT`),
    so a 4-piece chain and the line after are judged link by link.
    With `LAMP_SECOND_NAME_CARRIES` a line said by the lamp's name goes on
    only from the name, never from a line before it, often the end of
    other talk (reviewer-three's P2, 23 Oct: "yeah, I'll send it after
    lunch", "Lamp.", "what's on my calendar?" is asked "Lamp. what's on my
    calendar?"); the pieces after it chain on it ("Lamp. / I've been tired
    lately. / Like, every afternoon, / why?" is one named line). The name
    waiting for its line is carried by its own rule, never chained, and a
    refused name's words are never joined.
  - **No sign of a second person** (the same switch; the owner's test on
    lamp-4ace on 22 Oct, 19:04-19:08). lamp-4ace runs no same-voice model
    and its eyes saw nobody speaking, so nothing joined, and 5 of 28 lines
    went the same way: a pause mid-request, or a follow-up before the
    answer, and only the second half was answered. So, after the
    two-person rules, the voices and one face above, a line **let out** is
    joined when nothing shows a second person (`Joined::NoSecond`): the
    eyes' newest look as each line was judged holds at most one face
    (`bus_out::faces_in_view`); no other voice was heard and no two faces
    were seen speaking in the conversation (#829's other voice, a print
    under 0.30, or a line two faces were seen speaking, kept until a line
    is judged with no conversation going on); and the two lines' faces
    are not another's. With no face in view, or no eyes at all (the
    camera closed, the eyes not running), the room is the one sign left,
    so it joins only when both lines were judged in a **quiet** room: most
    two-person scenes are busy rooms, and in a quiet one the person still
    talking is most likely the one going on. An unknown room does not
    count as quiet (a reviewer's P2, 22 Oct): it is the room after a
    start, with its facts off, and the first minute of any two-person
    scene, since busy needs three unanswered lines in a minute. Its why is counts only: `no
    sign of a second person: 1 face in view at request N's end, 1 face in
    view at request M's, no other voice heard and no two faces seen
    speaking in the conversation`; refused, the faces in view, `another
    voice or two faces speaking in the conversation (request K)`, `…, in a
    busy room`, `…, in a room not known to be quiet`, or `nothing kept of
    the room at request N` (a privacy
    change since, which forgets how the room was too). A **held** line is
    not joined on it: it was held as perhaps not said to the lamp, and the
    cut words are answered in its place alone, as before; only the voices
    or one face join its words. The gap the engine measures (the cut
    line's End to the new line's Start) includes the coordinator's hold of
    a sound said while an answer is awaited (`LAMP_WAIT_KEEPS_ANSWER`, up
    to 1 s after its trigger), so it is about the person's pause less the
    silence that ended the line (0.6 s by default), plus that hold. The owner's lost
    lines, from the journal's times (it has no clock, so each is a bound):
    24 to 25 began 1.45-2.45 s after the first's end, 37 to 38
    1.59-2.13 s, 42 to 43 1.66-1.96 s; 35 to 36 at least 2.14 s, near
    2.6 s by the wait sound it began over (1.79 s after 35's end, cut
    0.81 s into it), just past the 2.5 s `JOIN_WITHIN` was, so it is 3 s
    now. Risk: a second person off camera, in a quiet room or beside one
    face in view, saying a line let out within 3 s, is joined to the first
    person's words.
  - **A joined line's answer is not history** (the same switch; the first
    builder on Queue #2602, 23 Oct 02:43). On lamp-4ace "Hey Lamp, what
    time is it in London right now?" got the text "Twenty past eight in…",
    which was never played, and "And in Saigon?" was joined to it. The
    worker keeps an answer's words in the thread as they are sent, before
    their sound, so the unplayed text went into the next chat's history as
    the lamp's words. The model took London as answered and said only
    Saigon's time. A line is joined only while none of its answer was
    heard: its words stop being carried as its answer's first sound is
    handed on, before that send's wait, so a newer line's cut can't carry
    words whose first sound already went (El's and Fourteen's P3 on
    #3129). So each line joined into a later one is taken out of the
    thread (`Engine::unsaying_joined`, `thread::unsay`): its question and
    answer leave the history, and its question leads the joined line's
    words in the thread. This happens only once the joined line is asked,
    checked under the lock a newer line's cut takes the words with
    (`asked_unsaying`): a join whose chain a newer line took in its print
    wait unsays nothing, so when that line isn't joined the cut exchange
    stays and the person's question isn't lost (El's P2 on #3129). A piece
    let go past the chain's 60 words or 20 s isn't asked, so it keeps its
    exchange. The model is asked both questions once, with no partial
    answer. The journal says `request M: K lines joined into it had none
    of their answers played; those answers are not kept as the lamp's
    words`. Words cut short and answered in a held line's place, not
    joined (#1712's carry), had none of their answer heard either, so
    their requests leave the thread the same way (`unsay_carried`; the
    held line's that last answered them too, when carried through one):
    the model is asked them again with no partial answer as the lamp's
    (reviewer-three's P3 on #3129). The journal says `request M: the words
    answered in its place had none of their answers played; those answers
    are not kept as the lamp's words`. #1712's carry has no switch of its
    own, so this unsaying stays behind `LAMP_JOIN_CUT_WORDS` with the
    join's. An answer that began playing is never joined or carried, and
    the thread keeps it as before. Without the switch nothing changes, and
    a cut line neither joined nor carried keeps its exchange: with
    `LAMP_SAID_AS_HEARD` its part is "—", so the person's words are not
    lost.
- **What the lamp said is kept as what was heard** (`LAMP_SAID_AS_HEARD=on`,
  off unless set; #1712, the first builder's yes on #1722, 20 Oct 09:38).
  - **The fault it fixes:** this route sends each piece of an answer, its
    words and then its sound, as fast as speech is made, and a reply holds
    up to 30 s of audio. The worker keeps each piece's words as the lamp's
    as they're sent. So a line said over the lamp 5 s into a 15 s story was
    asked with the whole story as the lamp's words. "Go on" then skipped
    what was never heard, and the cut-in note's "you stopped there" meant
    the story's end.
  - **The coordinator** counts each reply's samples handed to the speaker
    (`Reply::handed`). It puts them on the Start of the line that cuts the
    reply, or of the first line after a Stop that cut one on its session:
    `heard_of_cut`, the cut request and its samples played. It's left off
    the wire otherwise, and the session record's `input_admitted` says it.
  - **The route** keeps each piece's words and samples as sent
    (`said_as_heard::Piece`). A wait sound and the announcement count their
    time with no words. So does a call written as words mid-answer, which
    the thread leaves out (`CallsLeftOut`, on the same text), so a cut never
    tells it back as the lamp's words (the reviewer's P3 on #2145).
  - **At that Start,** before the line is asked, the cut request's words in
    the thread become what was heard (`said_as_heard::heard`,
    `Thread::heard_only`):
    - each piece played whole;
    - then the played share of the words of the piece playing at the cut,
      rounded down, with "—" after them;
    - and nothing after.
  - **Nothing more of that answer is kept.**
    - **A cut before any word** keeps the exchange, with the lamp's part a
      dash. The next line is still asked with the person's own question,
      and memory is handed it too (the reviewer's P2 on #2145: "What's the
      capital of Peru?", then "and how many people live there?" before the
      answer sounded, had lost the question). A thread read back from its
      file knows such an exchange by its dash.
    - Its end is the heard end, so a question it never played isn't taken
      as asked (`lamp_asked`).
  - **The journal says** `request N: H of its answer's W words were played,
    M ms of it, before it was cut; request K is asked with only those as
    the lamp's`.
  - **The lamp's memory takes the answer as heard.**
    - The route sends a long story in seconds, and the lamp plays it for
      many more. So the worker no longer hands an answer to memory at its
      turn's end (`HeldHandOn`, with the worker's `--said-as-heard`).
    - It holds the answer until the next line or a written request begins,
      after that line's trim, or until 30 s have passed (`HAND_ON_WITHIN`,
      the most a reply can hold, and before memory's 45 s idle end), or the
      worker stops.
    - An answer held is handed on when a newer turn ends.
    - Memory's journal gets the same exchange as before, up to 30 s later,
      and as heard.
    - **Memory's side** (Twelve's conditions, 10:40 on #1722): at the
      turn's end the worker calls `remember::hold`, which keeps what that
      moment knew, the provider context's sources and rules, when it ended,
      and the forgets before it. The exchange is sent with those and the
      words as heard, so it links none of the next turn's recall ids and
      counts toward memory's idle end from when it ended.
    - **A forget reaches it:** each `forget` and `forget_everything` sent
      to the store is counted first, and the writer never writes an
      exchange made before one (`HandOns`), so "forget that" said as the
      line that cuts an answer reaches that answer, held or queued. The
      forget waits up to 300 ms (`FORGET_SETTLE`) for an exchange the writer
      has in hand. A forget that was done has its answer said but kept out
      of the thread the next turn is given, since it may name what went
      ([memory.md](memory.md), "A forget's answer stays out of the thread").
    - **A stop:** the held answers go first, then the writer has up to
      150 ms to write them, then the open session closes
      (`remember::close_at_exit`).
  - **Not covered:**
    - a Stop with no line after it within 30 s: the Stop's count reaches
      the route only on the next Start, so that answer goes to memory whole;
    - a crash while an answer is held: that one exchange is lost from
      memory's journal, though the thread's file keeps it;
    - a Stop followed by a written request rather than a spoken line: a
      written request has no Start, so that answer stays as sent;
    - the Stop's path through the session loop: its record of the cut is
      tested as a function (`stop_cut_of`), and end to end only on a lamp
      (a Stop over a story, then "go on").

    The share of a cut piece assumes its words are spread evenly over its
    sound.
  - **The Live route** is told nothing new: Gemini's server keeps what it
    made, so its history can't be put right this way.
- **The name in the room's sound needs a second sign**
  (`LAMP_NAME_SECOND_SIGN=on`, the first builder at 21:03 on #1722).
  On lamp-4ace on 2026-10-20 at 18:16, 18:17 and 18:20 a `lamp-name`
  hearing of 0.9 (office talk is heard as the name up to 0.997) let a line
  out as "its name was heard in the room's sound", and the lines after it
  out as the conversation going on. With the switch,
  `attention::Attention::weigh_the_name`, called in `Heeding::judge` once a
  line's signs are in, drops the hearing for that line unless a second sign
  is there: the eyes' newest fresh look has a face turned to the lamp (read
  whatever the rule's form), the line was said close to the lamp or to it
  from close, or its words call it, or the hearing began at the line's
  start: no later than `attention::NAME_AT_THE_START_US` (1.3 s) into the
  line's sound, which opens with the voice's pre-roll (0.3 s,
  `activity::PRE_ROLL_BLOCKS`, each block dated when it was read), or up
  to `NAME_BEFORE_US` before it. A hearing is dated from when it began,
  the time `lamp-name` said it less its `ago_ms`, never from when it was
  said (up to 0.8 s later, `said.rs`'s `LONGEST`); but `lamp-name`'s
  began is when its score reached the bar, as the name ends and a little
  after. The first builder at 09:12 on #1722 (b0802): "Hey Lamp, good
  morning!", played at 08:06:33.988, was heard from about 08:06:35.05,
  about 1.06 s on, and was dropped at 0.7 s. A passing mention's hearing
  begins a few words in ("Yes, the lamp looked at me…", about 1.55 s;
  #829 scene 2's three). The journal's drop line says where the hearing
  began ("began N ms into the line"), which is what the bound is set
  from. A hearing weighed and not dropped says it too (#4226): "lamp-name's
  hearing began N ms into the line (the newest M ms)", N the first that
  counts for the line, so both sides of the bound are read; a hearing
  up to `NAME_BEFORE_US` before the line reads as a negative N. On
  lamp-c0a4 on 29 Oct, "Lamp, I want to cook something new this weekend."
  played 1.5 s after desk talk was dropped 5 times in 20: four at 1686 to
  1822 ms and once at 5554 ms, on 8327 and 8389 alike; the 5554 ms one
  was a line dated from sound carried into it, now dated from its own
  start (the carried sound above). With soft starts on (`LAMP_SOFT_START`)
  a line's sound opens up to 0.3 s before its usual start, so every hearing
  reads that much later into it: on a lamp 939 to 1023 ms with them, 654 to
  723 ms without (#4226). With `LAMP_NAME_START_USUAL=on` (off unless set)
  the name at the line's start is dated from the usual start: the
  coordinator's `Start` says the lead it kept (`soft_lead_us`, from the
  line's first block to the block `PRE_ROLL_BLOCKS` back from the trigger,
  at most `SOFT_START_LEAD_BLOCKS` blocks, and 0 for a yield's start or one
  as the lamp's sound ends), and the rule adds it to the line's start for
  `NAME_AT_THE_START_US` alone (`Attention::start_of`), and for the
  hearing kept at the line before's start (`LAMP_NAME_STAYS_IN_ITS_LINE`).
  A line dated from sound or a name's line carried into it gets no lead;
  a line going on keeps its first part's usual start. Every other window
  (`NAME_BEFORE_US`, the own echo, said to a face, near hearings) keeps the
  line's own start. The journal then says "began N ms into the line (L ms
  of soft lead)", so a lamp run reads both. The first builder at 03:54 on #1722:
  lamp-4ace's played tests b0333 and b0413 each lost their first four day
  lines, "Hey Lamp, good morning!" heard at 1.000 (`began_ms_ago=480`) with
  its words written "Mainland. Good morning.". The 18:20 shape's words were
  not read: its test takes its hearing mid-line; heard at a line's start
  (another device's wake word heard as the lamp's name) it would count, and
  so would a line opening with "Lamp" said to someone else ("Lamp's broken
  again") and a name heard in the last 1.5 s of the line before
  (`NAME_BEFORE_US`). A line cut and continued keeps the first part's start:
  a hearing mid-way through it stays mid-line. At any score, `attention::SURE_SCORE`
  too: a sure hearing alone is the same lone sign. Dropped, the line is
  judged as one with no name: no `NAMED`, no `NAMED_PLAINLY`, no call, so no
  conversation goes on from it, the Policy's gate is not told the name, and
  neither is the model: the name note (`LAMP_NAME_NOTE`) is weighed after
  the judging (`Heeding::heed_signs`) and a dropped name earns none; the
  journal says `its name, heard only in the room's sound, counts for
  nothing alone`. The face is anyone's (`someone_turned_to_lamp`), not the
  speaker's: someone at the desk facing the lamp while talk across the room
  is heard as the name counts as the second sign, as the rule's `TURNED`
  does today; the lamp run includes someone facing the lamp and not
  speaking. Not yet: the Live route and a bare call.
  - **A line said to a person is no call by its start**
    (`LAMP_NAME_SAID_TO_A_PERSON=on`, off unless set, #3945). On c0a4
    "Ma'am, could you pass me the salt please?", said to a person, was
    heard at 0.95 to 0.99 at the line's start and answered, the start
    being its own second sign. With the switch, `weigh_the_name` does not
    count the start when the line's first word, set off
    (`first_word_set_off`), is on `attention::SAID_TO_A_PERSON` (ma'am,
    madam, sir, mum, dad, honey, sweetie, babe, dude, bro, buddy, guys;
    lower case, apostrophe kept, a curly one as straight), and, for that
    start only, counts a conversation with the lamp going on
    (`Signs::going_on`) as a second sign, never for other lines; a face turned to the lamp, closeness and its words calling it
    count as before. The journal says `is said to a person ("ma'am"), and
    nothing else shows it was called: not counted
    (LAMP_NAME_SAID_TO_A_PERSON)`, counts and the list's word only. Never
    on the list: the lamp's name, "lamb", `STAND_INS`, `STAND_ALONE_INS`,
    `HEARD_FOR_THE_NAME`, `NAMES_OF_ITS_SHAPE`, a word that isn't
    alphabetic. Known costs: "man" and "mom" stay stand-ins (the text
    calls them before the hearing matters), and a "Lamp, …" written
    "Ma'am, …" (#3923) needs a face, a conversation or closeness. Lamp
    check on c0a4 (28 Oct): a person's "Ma'am, …" heard as the name at
    1.00 was held, two "Lamp, …" calls were answered
    ([attention.md](attention.md)). A line cut short is read too
    (request 13 on a lamp run): when the line after it, not joined with
    it, keeps its start hearing, that start is held on the cut line's
    first word as well. In a cut line of two words or fewer the word may be
    bare ("Ma'am could"), for the line after it only; the cut line itself
    and a whole line still need the comma. With `LAMP_NAME_SECOND_SIGN=on`, with
    or without this switch, a hearing that
    does count says which second sign let it (#3945: on 28 Oct a "Ma'am,
    …" line from the Mac was answered for its name and the journal could
    not say why): `its name, heard in the room's sound, counts by: <signs>
    (LAMP_NAME_SECOND_SIGN)`, each of "a face turned to the lamp", "the
    name said close to it", "said to the lamp from close to it", "its
    words calling it", "a conversation going on" and "its hearing at the
    line's start" that held (`Attention::second_signs`), never the words.
- **A hearing of the name stays with its own line**
  (`LAMP_NAME_STAYS_IN_ITS_LINE=on`, on in the product unit). On lamp-4ace on
  2026-10-26 at 06:21 (build 6919, the busy scene played) `lamp-name` heard
  the name at 0.92 near the end of "Can you pass me the charger? The white
  one.", which has no name in it; that line played this ten times in 21. The
  hearing counted for "Here you go. Is your phone dying again?", begun 0.7 s
  later, which was answered, and the next desk line went out as the
  conversation going on. A hearing counts for a line begun up to 1.5 s after
  it (`attention::NAME_BEFORE_US`), and `lamp-name` tells one up to 0.8 s
  after it began, so one from a line's last words can come after that line
  is judged, among the next line's signs.
  - With the switch, `Heeding::heed_signs` leaves out of a line's signs every
    hearing that began inside the sound of a line judged lately (the last
    four, `SAID_KEPT`: one may come only after the line between was judged,
    reviewer-two's P2 on `2110d04a9`), before this line began: it is not
    this line's name heard, its name's note, nor its name said to a face. `Attention::began_at` keeps the same rule for a
    hearing that came in time and counted for its own line.
  - Kept as before: a line going on from another (`continues_from`) keeps
    that line's hearings, though not an older line's (the reviewer-lead's P3
    on `db3bc2d59`),
    the line after one whose words called the lamp by name ("Lamp.", "Hey
    Lamp, …"; `Attention::called_in_its_words`) or after a bare call, a
    hearing at the line before's start (`attention::NAME_AT_THE_START_US`,
    a call however its words were written: "Lamp." heard as "Ramp.", the
    reviewer-lead's P2 on `da9a597de`), and a hearing in the line's own
    window ("Lamp, is my phone dying?").
  - A hearing that stays with its line is used up there: `began_at` lets go
    of it, so no later line takes it within its 1.5 s either (reviewer-two's
    P2 on `da9a597de`: a third line had it back).
  - Tests: `attention::tests::a_name_heard_inside_a_line_stays_with_it`,
    `worker::tests::a_name_heard_in_the_line_before_does_not_play_the_next`
    `worker::tests::a_call_at_a_lines_start_goes_on_and_a_kept_hearing_is_used_up`
    and `worker::tests::a_hearing_come_two_lines_late_stays_with_its_line`,
    with 4ace's times, the hearing late or in time, the switch off as a
    control.
- **The name note** (`LAMP_NAME_NOTE`) is given as on the Live route
  (`worker::Heeding::telling_the_name`): a line the lamp's name began, heard
  by `lamp-name` at `called::LEAST_SCORE` or more no more than
  `called::BEFORE_US` before the line, is asked with the note beside its
  words. With `LAMP_NAME_NOTE_BUSY=face`, in a busy room only with someone
  turned to the lamp or the name heard surely; the journal says why not.
  So is the staff note (`LAMP_STAFF_NOTE`).
- **Named to a face, answered** (`LAMP_NAMED_ALWAYS=on`, #2191; with
  `--attention`). On a production floor (#1722, hw-first-P1 finding 4) the
  name was heard at 0.976 to 0.978 and a face seen in 36 to 40 of 40 looks,
  yet every such line was held as a busy room or declined by the model. One
  predicate, `worker::named_to_a_face`: the line begins with the lamp's name
  (a `lamp-name` hearing at `called::LEAST_SCORE` or more from
  `called::BEFORE_US` before the line to `NAME_STARTS_WITHIN_US`, 1 s, into
  it; or its words open with "Lamp" or "Lamb" set off as a call, after a
  greeting or before a pause, `attention::begins_with_the_name`, since on
  lamp-52e6 `lamp-name` heard none of three such lines; never a stand-in
  such as "man", "plan" or "mom", and not "Lamp is broken, …") and the
  eyes' newest fresh look has a face turned to the lamp (the rule's
  `turned`, not any face in view). The face is what tells a call from
  office talk heard as the name at 0.9 to 0.997, so only with the speaker
  facing the lamp is the model overruled. Then, in a busy room, a hold of
  the strict busy rule (`attention::BUSY_NAMED_UNTURNED`, `BUSY_UNCALLED`)
  or of `busy_bar` (`BUSY_SHORT`, `BUSY_TURNED`) becomes a play
  (`Heeding::verdict`), journaled `held as "…" no longer`, and the line is
  asked with the by-name note; any other hold stands (`LONE_WORDS`, a line
  heard in another script). A `not_for_me` the model calls on it is not
  taken once (`Engine::answering_named_to_a_face`, `NAMED_TO_A_FACE_ANSWER`
  as each call's result, the calls not run): asked again, a second decline
  ends the turn in silence. Without a face turned to the lamp, with the
  name later in the line, or without the name, nothing changes. Not yet:
  `not_for_me{}` written as words still ends such a turn, the Live route is
  unchanged, and the speaking face (`LAMP_SECOND_SPEAKING_FACE`) is not
  read here. The rule's own text call (`attention::called`) still lets out
  a line opening with a stand-in, with or without the switch; for that,
  `LAMP_BUSY_CALLED_STRICT`, next.
- **A busy room ends a minute after its talk** (`LAMP_BUSY_ENDS_SOONER=on`;
  the first builder on Queue #1722, 04:00). The room this route keeps
  (`keeping_room`) ends a busy room a minute after the last talk between
  people when nothing since was talk or answered (lines held, sounds,
  silence), not three minutes after (`RoomTold::end_busy_sooner`); a line
  held only because the room is busy and it wasn't called (`BUSY_UNCALLED`,
  `BUSY_NAMED_UNTURNED`) is talk with it, so people talking on keep it
  busy; once the
  lamp answers, a minute after its first answer since the talk
  (`RoomTold::end_busy_once_answered`, which this route never turned on
  before; Live turns it on with `LAMP_GOING_ON_ASKS`). attention.md, "A
  busy room ends a minute after its talk".
- **The one it answered goes on, busy room or not**
  (`LAMP_SAME_SPEAKER_CARRIES=on`; with `--attention` and
  `LAMP_NAME_OR_FOLLOW_UP`; the owner's P1 on Queue #3126). On lamp-4ace on
  23 Oct (build G, `LAMP_SPOKEN_TO`, `LAMP_BUSY_ENDS_SOONER`,
  `LAMP_NAME_OR_FOLLOW_UP`) every conversation ended after two turns: the
  third line's short print read "another voice" (0.27 from 1.8 s), the lines
  after it, 11 to 22 s after the lamp's words, missed the busy room's 8 s
  follow-up, and the owner's own held lines kept the room busy (22 holds,
  18:40 to 18:55). With the switch, in a busy room, a line begun within
  20 s (`worker::SAME_SPEAKER_CARRIES_FOR`) of the end of the lamp's last
  words, by its start (a line begun inside may be judged after it), goes on
  as the conversation, past the 8 s and past talk past the lamp, when
  `Heeding::carried_by_the_same_speaker` finds the one it answered: the
  caller's face alone seen speaking it turned to the lamp, or the
  conversation's voice at `same_voice::SAME_FROM` (0.70) or over. Each
  answer renews the 20 s, so a chain of carried turns has no overall cap,
  but outside the 8 s window each carried turn needs its own sure sign (the
  reviewer-lead at 03:40). A line begun within the 8 s while a carry
  stands is read the same way (reviewer-two at 03:46 on #3296): its sure
  sign lets it go on and another face seen speaking it is held; but
  a line with no sure sign either way follows the ordinary follow-up rule,
  whatever the print's arrival (reviewer-two at 04:11): an unsure print
  (under `SAME_FROM` and no confident other) or a missing one is no
  evidence, judged as if no print had come, whether it is back at the
  verdict, late or never, so such a line goes on within the 8 s (the
  legacy exception of any follow-up) and is held past them, ending
  nothing. It is let out as the conversation going on,
  before `busy_bar`, so a short line that is no question is still held
  (`BUSY_SHORT`). Another face seen speaking it, or a sure other voice
  (under 0.30 from `CARRY_SURE_OTHER_SPEECH_SAMPLES`, 2.5 s of speech, the
  0.3 s pre-roll not counted, `Voices::usable_speech`), is someone else's
  line, held and not answered. One such line ends nothing (the first
  builder at 09:34 on #3296, replacing 03:46's "a colleague inside the
  window ends it"): the owner's own next line goes on by their face or
  voice, and in a room not yet busy that one line in another voice, or a
  line held past a still turned face (`LAMP_TURNED_FACE_SPEAKS`), does not
  make the room busy by voice alone, nor is it talk: the worker's
  other-voice branch and `RoomTold::judged` skip `RoomTold::two_people`,
  and `RoomTold::left_unanswered` closes it as held
  (`RoomTold::colleague_held`); the line itself is still judged as in a
  busy room, so held. A print known late as a sure other voice is such a
  line too, each request counted once, and ends the carry only as the
  second in a row; a line judged after that end is judged as with no
  carry. Two lines in a
  row from someone else (`Heeding::colleague_held`, counted to
  `room_told::TALK_ENDS_CONVERSATION`, reset by a carried line of the
  owner's or an answer) are people talking between them, as two lines
  left alone end a conversation: the carry ends until the lamp answers
  again, and the caller's identity is not reused without a new answer. A
  colleague who calls the lamp by its name is answered and is the caller
  from then on. Following First's decision on Queue #3126 at 12:57 on
  24 October, the 20 s run from the later of the lamp's last words and
  one held colleague line's actual audio end, once per actual answer,
  capped at 40 s from the lamp's last words. A colleague line begun at
  +3 s and lasting 2 s admits the owner's +25 s start, not +45 s.
  Late classification uses the recorded audio end, not the time the
  print becomes known; duplicates or another held line after an owner's
  admitted but unanswered line cannot buy a second extension. A new
  actual answer renews the allowance; admitting a line does not. At most
  four recent eligible line ends are kept, and missing, future or evicted
  ends give no extension. Identity checks and the switch-off path remain
  unchanged. When that colleague print is first learned at the current
  owner's verdict, the worker rechecks the current line's captured start
  against the newly available window. It always reads the cached reply
  again and retains any own-voice likeness consumed by the first lookup
  in preference to the second result, including its same/other-voice
  classification for the downstream same-speaker rule. Neither lookup
  waits for a print. A line admitted only by this newly opened late window
  is not recorded as another eligible colleague end: a later print for
  that line gives no further extension. This fails closed.
  In Busy, Unknown and Quiet, that new eligibility still needs the owner's
  positive face or sufficiently nearer voice; missing, uncertain or
  confusable voice evidence is held for this verdict without turning a
  quiet or unknown room busy. A +25 s owner start can pass; +45 s cannot.
  The chronological control covers both an active 30 s thread and an
  expired 60 s thread. It uses a synthetic 1.5 s sure-other bar only;
  the production 2.5 s bar and the 1.5 s print limit are unchanged.
  The answer clock is existing `Did::Said` queued-PCM duration bookkeeping,
  not acoustic drain attestation. Physical last-word timing and activation
  still need the lamp's attended check.
  A stop (`Did::Stopped`, from the next line's `after_stop`) and
  a privacy change end it, and a forgetting leaves no words to carry from.
  The caller's own face seen speaking turned away from the lamp is someone
  talking to a colleague (reviewer-two at 03:01): held, and it ends the
  carry, their voice sure or not, so turning back needs a fresh call or
  answer (the first builder at 09:34; reviewer-two at 10:32 on #3331).
  Another speaking face and the caller turned away are held in every
  room while a carry stands. The owner's carried line is no reply to a
  colleague's held question (`Attention::carried_past_a_question`, no
  `AFTER_QUESTION` hold). Once a line is held as someone else's, voice-only
  identity carry needs its print nearer the conversation's than **every**
  retained held line's print by `worker::CARRY_NEARER_BY` (0.10, provisional).
  A newer visitor cannot replace an older visitor's constraint. At most
  eight held line requests and their optional prints are kept in memory
  for this call. Duplicate requests only fill a missing print. A missing
  held print requires the owner's speaking face until that request's own
  already-requested cached reply is available; no new inference or wait is
  started. At capacity, another distinct held line latches face-only
  identity carry until a fresh call, rather than evicting an older print.
  Owner admissions and answers do not discard these constraints. A fresh
  call, stop, privacy change, observed forget, caller turned away or two
  consecutive other lines clears the scope; late prior-call callbacks
  cannot restore it. A fresh caller has no old caller's constraints.
  Observing a forgotten or absent answer closes the whole carry, including
  its active marker. If the recent call still legitimately admits the same
  owner's next line, that admission begins a fresh constraint scope; its
  actual answer can then carry the owner's later voice-only follow-up.
  This does not reopen a closed scope from a late old callback or reset
  capacity within a still-live call.
  In Busy, Unknown and Quiet, past the ordinary follow-up window, a
  positive current voice whose held-print comparison was refused cannot
  bypass that refusal through an active thread. The refused line is no
  talk for the stored room. The caller's speaking face still carries.
  The ordinary first 8 s follow-up remains unchanged. Generic missing or
  weak current evidence in an Unknown/Quiet thread has a separate
  personal-scope acceptance hold; this comparison does not close it.
  In a Busy room, a line with no sure sign, a print not back, a short print
  under 0.70 or no speaking face is held past 8 s, admitted within them as
  the ordinary follow-up, and ends nothing. The existing score rows of
  `docs/verification/same-voice-early-20261018`, selected by their clips'
  original durations (no audio re-sliced, no CAM++ run again), give one
  synthetic voice's short clips (three of 0.93 to 1.45 s against seven
  references: 21 correlated pairs) 0.11 to 0.50 against it, and four
  clips of another voice (1.43 to 1.45 s) 0.09 to 0.26:
  a reason for caution about a short print's "another voice", not a
  calibration, so the 2.5 s is provisional; every print is asked
  at 1.5 s of speech, so today no voice ends a carry, and with short lines
  the face carries it, not the voice. A print come late is unsure the same
  way (`Voices::sure_of_another_only`), and a late other of a line older
  than the newest one carried by a sure sign ends nothing. A carried line
  the model leaves unanswered is no talk (`RoomTold::carried`): it neither
  ends the conversation nor keeps the room busy. The 2.5 s and the 0.30
  under it are to be set from a CAM++ measurement of longer prints. In
  short, the contract (the first builder at 04:33 on #3296): the first 8 s
  are not identity-gated, an uncertain, short or missing print there being
  no evidence either way; from 8 to 20 s only a sure same-speaker sign
  carries; and three cases are always held, inside the 8 s too: another
  face seen speaking and a sure other voice, ending the carry only at the
  second in a row from someone else, and the caller turned away, which
  ends it at once; the
  last not reachable today (prints carry at most 1.5 s of speech, under
  the provisional 2.5 s; its test is a synthetic classification control).
  A print that was no evidence never becomes evidence (reviewer-two's P2 at
  04:31): with the switch, a going-on line's print joins the conversation's
  voice (`Voices::adopt`) only at `SAME_FROM` or over or when the line was
  seen spoken by the caller's face, at its verdict or late, caller-only on
  or off, so an unknown voice let out at 3 s does not match itself at 12 s.
  Tests only (`worker.rs`, `CarryDesk`); off until a played five-turn
  back-and-forth on lamp-52e6 or lamp-75c9, quiet and with desk talk.
- **A remark about the lamp, held** (`LAMP_REMARK_ABOUT_IT=on`, #4175;
  [`remark_about_it`](../crates/voice/src/remark_about_it.rs)). In build
  8327's busy_day check on lamp-4ace (28 Oct), "That lamp is cute." between
  two colleagues was let out for the name heard at the line's start, and the
  lamp's answer made a conversation going on that let the next two desk
  lines out. With the switch, in a busy room, `Heeding::verdict` holds a line
  let out only by the name (heard in the room's sound or said close), with
  no call in its words, no tap, no face turned to the lamp and no question of
  the lamp's it answers, when every "lamp" (or "lamb", its mishearing) in
  its words follows a word like "that", "this", "the" or "your", no clause
  naming it asks or tells anything (`remarks::asks_in`: a `?`, "you" or
  "please", or a question or doing word first, so "Turn off the lamp." and
  "Can you dim the lamp?" are said to it) (`talked_about`), and no
  conversation is going on; the journal says `remark_about_it::HELD`. With
  no conversation going on, a line let out for the name in the room's sound
  alone (its words not calling the lamp) is still answered but opens none:
  no `called_at`, and its answer is put on the chain so it renews none. A
  conversation already going on is renewed as before. "Lamp, is it cute?", a face turned to the
  lamp, a tap and a quiet room are as before. A clause naming the lamp that
  asks it to move ("Tilt the lamp up.", `MOVES`) or states a need of its
  light ("The lamp is too bright.", "too", "so" or "really" before a light or place word, `TOO_WHAT`, a comparative after the lamp and not before "than", "I need the lamp brighter.", `MORE`, or a word after it that its light hurts, "This lamp hurts my eyes.", `HURTS`; bare "too", "need" or "want" is no need: "That lamp is cute too.", Thirteen's P2 on #4219, and a comparison asks nothing: "That lamp is brighter than mine.") is no remark either, and "please" anywhere in a sentence naming the lamp asks ("The lamp, off please.")
  (Reviewer-three's narrower P2 on #4201). A remark held is said on the bus
  as `voice/remarked` (`VoiceRemarked`, its request alone; `bus_out::RemarkOut`,
  from the verdict's own callback, so only a line `voice/turn` says
  `held_back`), for Expression's one small glance with
  `LAMP_REMARK_GLANCE=on`.
  Tests (`worker.rs`, `with_remarks_held_a_remark_about_the_lamp_opens_no_conversation`).
  **It keeps the person's conversation** (#4175, build 8652's busy_day on
  lamp-4ace, 29 Oct, `rUTNR-2031`: with the switch, "That lamp is cute."
  held renewed nothing, and the owner's next lines after the colleagues'
  talk were held as "no conversation going on"; without it, its answer had
  renewed the window). With the switch:
  - a line held as a remark, as the line right after one, or as another
    voice's, said while the lamp's words still kept a conversation going
    (the thread's 45 s), renews the window from its verdict as its answer
    would have: the next lines go on from it as from the lamp's words
    (`Heeding::verdict`, `held_in_conversation_us`), never more than
    `HELD_KEEPS_FOR` (90 s) after the lamp's own last words, however many
    are held. Forgetting the thread's words, or a new person at the lamp,
    leaves nothing for it to renew;
  - the line right after a held remark, let out only as the conversation
    going on, not found in the conversation's own voice, answering no
    question of the lamp's and asking it nothing (`remarks::asks_in` by
    clause: "Anyway, set a timer." goes on), is held as carrying that talk
    on to someone else ("It's from the office, they're testing it.";
    `remark_about_it::AFTER_A_REMARK`);
  - someone else's line ends nothing of the conversation: one held as
    another voice's no longer counts as talk past the lamp for the next
    line, which goes on when it is in the conversation's own voice; and a
    print come after its answer, another's, ends nothing for a line that
    asks the lamp a question or calls it (`voice_going_on_in`); a statement
    then ("Yes, give me five minutes.", to the colleague) is held as
    without the switch.
  Without a voice model nothing tells d11 and d12's lines from the owner's:
  let out as the conversation going on, they are answered as without the
  switch. Tests (`worker.rs`: `on_the_product_unit_the_owners_lines_after_a_held_remark_are_answered`,
  `with_remarks_held_a_colleagues_late_print_ends_not_the_owners_conversation`,
  `with_remarks_held_the_owners_line_after_held_colleagues_goes_on_in_a_busy_room`,
  `with_remarks_held_the_owners_conversation_outlives_the_colleagues_talk`).
  Without the switch, every line the lamp's own rule held is kept until it
  is closed (`RoomTold::held`, the latest eight), not only the last: two
  held lines judged before either closes are both no talk
  (`two_held_lines_closed_after_both_verdicts_are_no_talk_past_the_lamp`).
- **A busy room's call by the text, narrowed** (`LAMP_BUSY_CALLED_STRICT=on`;
  with `--attention`). On #1712's branch the busy room let out "Man, that
  meeting ran long.", "Plan the sprint for tomorrow.", "Mom, can you pick me
  up?", "Lamp is broken, can you fix it?" and "Lamb for dinner tonight?" as
  `CALLED_IN_TEXT` (the first builder on #1722, 16:11). With the switch,
  `Heeding::judge` tells the rule, before each line, whether the room is busy
  with nobody turned to the lamp (`Attention::narrow_the_call`); then the
  text's call (`Attention::called_in_text`, read for the verdict, the
  journal's sign and `called_for_itself`) counts only as "Lamp" or "Lamb"
  set off as a call (`attention::begins_with_the_name`) or with the name
  heard by `lamp-name` for the line (`Attention`'s `named`). Otherwise the
  line is judged as one with no call: in the strict busy rule,
  `BUSY_UNCALLED`. With a face turned to the lamp, in a quiet room, or
  without the switch, nothing changes. A call set off later in the line
  ("…, Lamp?") is narrowed too: it counts only with the name heard or a
  face. Not narrowed: a bare call's own text (`Attention::call_shown`, for
  the line after it) is read as before. The Live route is unchanged.
  Two limits (#2257's review): "set off" leans on the text's comma, and
  speech to text doesn't always write the pause, so "Lamp what time is it"
  counts only with the name heard; and the name heard is the low bar
  (`called::LEAST_SCORE`, 0.9), which the office's false hearings reach
  (0.93 to 0.997, #829), so a stand-in with a false hearing still counts.
  Narrower than without the switch, not closed.
- **A repeat of the line before's end is no call by the text**
  (`LAMP_NAME_REPEAT_NOT_A_CALL=on`; with `--attention`; off in both
  units). On lamp-4ace on 30 Oct (build 8999, the product unit, a busy
  room, #3923) d09's "That lamp is cute, where did you get it?", said
  between colleagues, came as two lines. The first, "That lamp is cute.",
  was held ("that lamp" is the name in passing). The second was written
  "Lamp is cute. Where did you get it?": the first line's last words came
  back again, and its "Lamp", opening a sentence, is a call by the name
  in passing's reading (`attention::called_by_name_on`), so it was let out
  as `CALLED_IN_TEXT`, and the conversation it opened answered the next two
  desk lines. `LAMP_BUSY_CALLED_STRICT`, which would have narrowed it
  ("Lamp is" is not set off), is off in the product unit. The words most
  likely came twice because the last 1.5 s of an unanswered line's sound
  (`KEPT_SAMPLES`) goes into the next line's speech to text first when
  that line begins within `CARRY_AFTER_END` of its end, or within
  `KEEP_AFTER_END` when it goes on from it, and the line's own words had
  not come back yet. With the switch, each line's words are kept as soon as
  its speech to text writes them (`Engine::telling_words_back` to
  `Attention::words_back`, before the line waits for anything or is judged),
  and the line before is read when this line is judged, not when it began:
  the newest kept line of an earlier request, ended within
  `attention::REPEAT_WITHIN_US` (2 s, `KEEP_AFTER_END`: no carried sound
  outlives it) of this line's start. The two lines' tasks run apart, so the
  first can be judged after the second; its words still count if they came
  back first. A line going on from a first part (`continued_at`) reads that
  part, the repeat then at the join.
  When this line's opening words, the lamp's name first, repeat that line's
  closing words, two or more of them (compared lower case, punctuation left
  out), the text's call (`Attention::called_in_text`, so the verdict, the
  journal's sign and `called_for_itself`) is read without them: "Lamp is
  cute. Lamp, where did you get it?" still calls. The name alone repeated
  is not enough: "…I like this lamp." then "Lamp, turn the light on." is a
  person calling it. The name heard by `lamp-name`, its second signs
  (`weigh_the_name`), a tap, a face, being addressed, an answer to the
  lamp's question and a conversation going on are unchanged. The journal
  says `its first 3 words repeat the end of the line before it (begun N ms
  after that line ended): its name there is not taken as a call by its text
  (LAMP_NAME_REPEAT_NOT_A_CALL)`: how many words, never the words. A kept
  line's words are let go 30 s after it ended (`SAID_LINE_KEPT_US`), at most
  four are kept, all are let go on a camera close or a mute
  (`Heeding::privacy_changed`), and none are kept with the switch off. A person who
  really repeats the end of their own line and means the lamp ("…fix the
  lamp. Lamp, fix it" is fine; "…the lamp is on. Lamp is on, turn it off")
  needs the name heard or a face then. The Live route is unchanged.
- **A face turned to the lamp but silent is nobody turned to it**
  (`LAMP_TURNED_SPEAKING=on`; with `--attention`). On lamp-4ace on 21 Oct
  from 14:00, someone at the desk sat in the camera's view turned roughly
  to the lamp (V1's 60° allowance) and quiet. The busy rule let 33 lines of
  a played two-person scene out as "someone is turned to the lamp", each
  keeping the conversation going for the next (scenes 5 5 4, against 1 0 3
  that morning with nobody in view). With the switch, the eyes' speaking
  faces (`LAMP_EYES_SPEAKING=on` on lamp-eyes, read as for
  `LAMP_QUIET_GOING_ON_SAME_ONE`) are read over the line. When a face
  turned to the lamp read still in at least half the looks, none turned to
  it read speaking in half, and none turned to it went unread in any look
  (`speaking_faces::turned_only_silent`), the sign is taken away before
  the rule reads it: `request N: the only faces turned to the lamp were
  seen silent through the line: nobody is turned to it
  (LAMP_TURNED_SPEAKING)`. So it goes for every use of the sign: the busy
  rule, the name's second sign and the strict call. A mouth the eyes
  couldn't read keeps the sign, so with their speaking read off nothing
  changes. Risk: a person turned to the lamp and asking, whose mouth reads
  still, needs its name in a busy room.
- **A face turned to the lamp lets out only a line it is seen speaking**
  (`LAMP_TURNED_FACE_SPEAKS=on`; with `--attention`). On lamp-52e6 on 24 Oct
  at 05:31, with the owner's still face turned to the lamp, two other
  voices' desk talk, never saying its name, was answered: `no conversation
  going on: ...` then `its answer is played: someone is turned to the lamp`
  (`attention::decide_called`, the `TURNED` arm), and `room/state` stayed
  `unknown`. With the switch, the eyes' speaking faces are read over the
  line (`worker::Heeding::silence_turned`): when a face turned to the lamp
  read still in at least half the looks and none turned to it read speaking
  in any look (`speaking_faces::turned_not_seen_speaking`; a look whose
  turned mouth went unread spoils nothing, unlike `LAMP_TURNED_SPEAKING`),
  and the line's print, as far as it is known, is not the conversation's
  voice at `SAME_FROM` (0.70), the sign is taken away in any room: `request
  N: the face turned to the lamp was not seen speaking the line, nor is it
  in the conversation's voice: nobody is turned to it
  (LAMP_TURNED_FACE_SPEAKS)`. The line, held, is talk for the room
  (`room_told::RoomTold::face_not_speaking`): it joins the run that makes a
  room busy (3 within 60 s, 2 from a quiet one), so a desk where two people
  talk past someone at the lamp reads busy, with or without
  `LAMP_NAME_OR_FOLLOW_UP`. The 22 Oct rule that a lone voice with nobody in
  view never makes the room busy stands: here a face is in view and its
  mouth reads still while the voice talks. With no mouths read
  (`LAMP_EYES_SPEAKING` off, or any face turned to the lamp never read
  through the line, judged per face) the face is a sign as before: fail
  open, so a person alone at a lamp whose eyes can't read mouths is still
  answered, and a still face beside it does not speak for an unread one.
  With `LAMP_TURNED_SPEAKING` on too, this rule decides alone. A line carried by `LAMP_SAME_SPEAKER_CARRIES`
  goes on as the conversation, before the face is read, and stays carried.
  Risk: the owner turned to the lamp and asking, whose mouth the eyes read
  still through the whole line, needs its name or a follow-up.
- **Two faces must stay to make the room busy** (`LAMP_TWO_FACES_HELD=on`,
  with `LAMP_NAME_OR_FOLLOW_UP`; Sixth's plan under the 24 Oct freeze). A
  thread of the worker's own reads every look the eyes publish
  (`humans/faces/tracked`) as it comes and keeps the last 2.5 s of them
  (`worker::FacesHeld`, `FACES_KEPT_MS`, at most 16). Each look is dated
  by its `header.at_us`, the eyes' `lamp_ipc::monotonic_us`: the
  system-wide monotonic clock the voice reads too (reviewer-two on #3355).
  Its age, expiry and span checks use only that clock, so a wall-clock
  step cannot fabricate a longer held span. The bus still applies its
  wall-clock expiry before delivering a look; these helper checks do not
  establish that live delivery is unaffected by a wall-clock step.
  The eyes look every 250 ms with a face to follow, 500 ms with one face
  sat still and 600 ms with nobody (`LOOK_EVERY`, `STEADY_EVERY`,
  `GLANCE_EVERY` in `lamp-eyes`). Spanning at least 1.5 s needs 7, 4 and
  4 looks at those respective paces; four 600 ms looks span 1.8 s. The
  rule uses elapsed time rather than a fixed four-look count. At a line's
  verdict, two faces make the room busy only when the newest look, not
  over 1 s old nor expired (its `expires_ms` and the bus's grace), sees
  two or more and at least 3 of the kept looks did, spanning 1.5 s or more
  (`TWO_FACES_LOOKS`, `TWO_FACES_FOR_MS`). A look already over 1 s old or
  expired when taken is refused, so a look delivered late gets no time
  back. A gap over 1 s between looks, a look dated before the last one
  kept or ahead of now, or a missing or expired newest look drops
  everything kept (`LOOK_GAP_MS`), and so does a privacy change, closed or
  reopened: after it, a look dated at or before it, or taken across it, is
  refused, so looks queued before it never come back (reviewer-two's
  HOLDs on #3355). Two faces held between lines make the first line's
  room busy. One look with two faces (a passer-by, a misread, a photo
  frame), a second face that has left, or two faces for under 1.5 s leave
  the room as it was. A first version sampled the newest look once per
  line, which made it 3 lines of 4 (the coordinator's finding on #3355).
  Held talk, talk past the lamp, another voice and the same speaker's
  carry are unchanged. Off, one look is enough, as before. If the looks
  cannot be subscribed to, two faces make no room busy, said once.
- **The owner named by default in a reply** (`LAMP_PERSON_NAME_BY_DEFAULT=on`,
  #3884, the first builder's call on 28 Oct): with `LAMP_PERSON_NAME`, each
  reply is told the person speaking is most likely the one who set the lamp
  up (`person::by_default`), unless there is a sign of someone else now
  (`person::someone_else_now`): another voice in the conversation (the
  rule's own `another_person`, told through `Heeding::telling_another_voice`),
  more than one face in view, one face the eyes don't know as the owner, or
  the eyes knowing someone else. No face in view, the owner looking away, is
  no sign. For the reply only: the line's scope, memory and every keep,
  recall and forget stay as the rule below decides. On lamp-29e7 (8250) the
  owner alone at the lamp right after setup asked "what's my name?" and was
  not answered: the camera read their mouth speaking on 2 of 18 lines, so no
  rule needing a face seen speaking could carry it. A lamp set up by voice
  has no typed name: there the owner is the name `remember_my_face` saved
  with the face that ended "who you are", kept as `owner` in the onboarding
  file (`onboarding::owner_of`; a face saved once the step is done, a
  guest's, never changes it, and a reset wipes it with the file), and named
  only while the eyes still know that person (`known_faces::still_known`,
  `{"list":true}`), refreshed every 10 s on a thread of its own
  (`known_faces::keep_owner_fresh`), so no reply waits on the eyes. Any face
  forgotten by voice, whoever's, clears it at once
  (`known_faces::owner_forgotten`) and the walk drops the mark from the file;
  one the eyes answer they no longer know (removed in the app, or any other
  way) loses the mark at the next refresh, so a later face saved under that
  name is never taken for the owner (the eyes' ids can't tell the two apart:
  a forgotten name gets the same id back); a whole redo or a reset drops it
  too. Names match as the eyes match them, in any case. The name kept is the one the eyes
  kept (`remembered`). With a typed name too, the owner is kept as well, and
  a face the eyes know under either name is the owner's, matched by memory
  person id (`person::by_default_owner`, `known_faces::memory_person`); the
  reply is told the typed name. An app user who typed "Minh Nguyen" and said
  "Minh" at "What should I call you?" had their face kept under "Minh", so
  every turn the eyes knew them read as "a known face that isn't theirs" and
  the name was held back and taken out of the lamp's lines on each one (the
  onboarding forgets report, 30 Oct). A different known person, two faces,
  an unknown face or another voice still holds it back, with one exception:
  a line carried as the owner's conversation going on
  (`LAMP_CONVERSATION_CARRIES`, `ScopeLease::carried_for`: no face seen
  speaking it, no face but theirs in any look through it, their face in
  view through it or within 5 s before it began, `CARRY_UNSEEN_WITHIN`, and
  their own last exchange within 45 s) keeps the name through one face the eyes don't
  name just now, a recognition slip (a missed check, a track found again).
  Two faces, another voice or a known face that isn't theirs still holds
  it back on a carried line, and a line not carried, or carried for
  someone else, is as before. The reply only, as the carry: nothing is kept,
  recalled or forgotten on it. On lamp-29e7 (8474, 29 Oct) the owner, set up by voice,
  asked who they were and was told the lamp remembered no name: no name
  source reached the reply (#4118). While someone else may be speaking, the
name is also taken out of the lamp's own earlier lines the reply is given
(its "You:" lines in the thread and its side of each exchange; the
person's words are theirs), said as "(their name)", and the reply is told
that marks a name held back, never to be said (`person::without_name`,
`person::HELD_BACK_NOTE`). Taken are the whole name, its words in order,
NFC and case aside, for a name of more words, and one word of it alone only
as written in the name, capitalised the same, where it can't be a
sentence's own first word, so "I will" and "Will it rain?" stay for an
owner named Will, and a Vietnamese name's syllables stay in everyday
words. On lamp-52e6 (29 Oct, two faces in view) the rule held the name
back 3 times, yet the lamp said it in 2 of the answers, from its own
earlier ones. The memory note and a recall's answer are not taken from: a
room fact naming the owner can still reach the reply. The faces are counted over the
eyes' looks of the last 3 s (`known_faces::most_faces_lately`), not their
newest look alone: in the same run, with two faces in 36 of 37 looks, one
reply read a look with none and named the owner. The count is the most
faces seen in at least two of those looks, so one look that saw a second
face that isn't there never makes one person two (lamp-52e6, 29 Oct 16:52,
the one screen face read as two). The face the eyes know is read from their
known file while it is at most 8 s old (`known_faces::known_lately`): the
eyes rewrite it at each check, every 2–3 s, and remove it as soon as the
near face matches nobody, so the 3 s read of `known_now` took a file still
there for nobody (16:49, "an unknown face in view" with the file present;
#4118). On lamp-29e7 on 8545 (29 Oct) the owner,
4.5 minutes after setup, asked "can you remember my name?" and was told
"You haven't told me your name yet": their own answer had left the recent
exchanges, and the note then said "not to answer" and named only "who they
are". It now tells the model to say the name when asked who they are, what
their name is, or whether the lamp knows or remembers it, and a reply told
it says so in the journal, without the name ("the owner is named by default
to this reply"). The eyes' list is asked with up to 3 s to answer, off any
reply's path; eyes reached but slow to answer take nothing back (the mark
stands), and eyes that can't be reached at all name nobody, as before. Without a name, or off: nothing. `the owner is not named by default: WHY` in the journal when a
  sign holds.
- **A line is a known person's only when their own face was seen speaking
  it** (`LAMP_PERSON_SCOPE_SEEN=on`; with `LAMP_SECOND_SPEAKING_FACE`). Off,
  a line admitted as the known person's is narrowed to the room only when
  another face is seen speaking it (`speaking_faces::someone_elses`), so
  with the owner silent in view an off-camera line is theirs: their
  `forget_everything`, their observations (`FinishedExchange::whose`), and
  a late look's keep (reviewer-three's P1 and P2 on #3435, Second's case).
  With the switch, one rule, `speaking_faces::not_theirs`, decides both the
  line's own admission and a late look's keeper: theirs only when their
  face alone was seen speaking it (`seen_saying`), each face id read with
  the eyes' boot, the known face's too (`speakers_with_boot`,
  `known_with_boot`, as the held forget's sight, #3037), so after the eyes
  restart a reused `f1` is not the known person's (Second's P1 on #3437),
  else the room's. One thing more for such a line: when no face was seen
  speaking it and the known face, theirs, was the only face in view through
  it, in at least 3 of the eyes' looks, read speaking in at least one and
  still in under half (`SpeakingFaces::alone_in_view`,
  `speaking_faces::named_alone`), its reply is told their name
  (`ScopeLease::named_for_reply`, the `known_faces::note` line and the typed
  name) and nothing else: its scope, memory, thread and every keep, recall
  and forget stay the room's (reviewer-three's P2 on #4090: a partner off
  camera while the owner nods), and a mouth only ever unread names no one
  (the Reviewer-lead's P1). `request N: no face was seen speaking it
  (LAMP_PERSON_SCOPE_SEEN), the known person's face alone in view: kept as
  the room's, its reply told their name`. On lamp-3e83 (stable 8163, 28 Oct
  16:30) the owner alone at the lamp had about half their lines read with no
  face speaking, and the lamp lost their name. Else: `request
  N: no face was seen speaking it (LAMP_PERSON_SCOPE_SEEN) [L looks, O with
  one face; the known face in K, read speaking in Y, still in S, unread in U]:
  told and kept as the room's`, the counts saying why (lamp-29e7, 28 Oct:
  18 of 20 lines so, with nothing to say why; `SpeakingFaces::sight_told`).
  A face seen speaking that is the known face itself, kept as the room's,
  says the eyes' known file named someone else when read (29e7's request 35,
  just after the file changed), or that no known face was read for this boot. A face seen speaking it gets one line too, by passing id
  only, never a name or the words (`speaking_faces::seen_speaking_note`):
  `request N: seen speaking it: the known person's face (id K): told and
  kept as theirs`, `request N: seen speaking it: a face not the known
  person's (id F): told and kept as the room's`, or with more than one,
  `request N: seen speaking it: 2 faces (ids F, K): told and kept as the
  room's`, so with two faces in view the journal says which one a line
  went to (#3699). Every person scope taken from a line reads the lease this
  narrows (`remember::forget_everything` and `forget_face_memory`, the
  thread's exchanges, `tools.rs`' `keeper_within` and `run_scoped_for`), and
  a narrowed lease never widens (`ScopeLease::late_open`). Risk: an owner
  whose speaking read misses gets the room's scope for that line (alone
  in view, at most its reply told their name), so a
  personal keep or forget is not done as theirs: `forget_everything`
  refuses, and a keep is the room's, or withheld with
  `LAMP_MEMORY_PERSONAL_NEEDS_FACE`.
  **Within their conversation** (`LAMP_CONVERSATION_CARRIES=on`, #4339):
  such a line no face was seen speaking stays the conversation's, not the
  room's, when all of these hold: the newest exchange before it is theirs
  and began within 45 s (`Thread::going_on_with`), the eyes' looks fell in
  it and none tracked a face but theirs (`SpeakingFaces::no_other_face_in_view`;
  no look, a stalled eye or a covered camera, is not), their face was in
  view through it or within 5 s before it began
  (`SpeakingFaces::face_seen_between`, `CARRY_UNSEEN_WITHIN`; an empty view
  from the start, or their face gone longer, is the room's: the lamp-memory
  owner's decision on #4479), and the thread's lock
  was free (a busy one is the room's). It is then carried
  (`ScopeLease::carry_conversation`) and read as a line held by a face's
  gap (`ScopeSnapshot::held`): told their thread and the exchanges before
  it, with no memory note, hits, recall, keep, forget or yes
  (`HELD_MEMORY_CALL`), and never journalled. A face seen speaking it,
  another face in view, a 45 s quiet or another person's exchange between
  narrows it to the room as before. A line admitted as the room's, the
  face cache naming nobody, is carried the same way into the known person's
  conversation going on (`Thread::going_on_with` names whose it is), when
  no face was seen speaking it, none but theirs was in view and theirs was
  within the same 5 s: `request N:
  admitted as the room's, no face seen speaking it, in a known person's
  conversation going on, no other face in view: told from that
  conversation, never memory's (LAMP_CONVERSATION_CARRIES)`. A line
  admitted as another known person's is never carried into someone else's.
  On lamp-3e83 (29 Oct) one such line dropped to the room's, the chat saw
  16, then 4, then 16 messages, and the lamp asked its question again; on
  lamp-06c1 (29 Oct) 13 of 68 turns with the person in view throughout
  were not theirs, 5 of them with no face line at all. Its exchange is never given to the running
  summary or a talk note. Risk, accepted: someone off camera, with no
  other sign, within those 5 s, is answered inside the conversation going
  on and can hear what was said in it and its running summary; they reach
  none of the person's saved memory. The
  journal: `request N: no face was seen speaking it, in their conversation
  going on, no other face in view: told from that conversation, never
  memory's (LAMP_CONVERSATION_CARRIES)`.
  **A carried line's keep waits for their yes**
  (`LAMP_CONVERSATION_CARRIED_SAVES=on`, #4339, approved 30 Oct; the first
  builder's call on #4508): after either carry, `ScopeLease::save_carried`
  marks it, unless it was admitted held only by a face's gap. Then
  `remember` alone is declared (`withheld` gives
  `MEMORY_CALLS_BUT_REMEMBER`), but never written from the carried line:
  `hold_carried_keep` holds the call in `HeldForgets`, in RAM, for the
  carried person, and the model is told `remember::held_keep_told` ("Not
  kept yet … ask them once"). It stands only on a reply that asks
  (`remember::asks_about_keeping`), and runs only on their plain yes on the
  next spoken line within 60 s with their face seen saying it
  (`take_held_forget`, `LAMP_EYES_SPEAKING`); anything else drops it.
  `same_voice_releases` is the hook for the same-voice follow-up. Without
  the eyes' speaking read nothing is held (`HELD_KEEP_REFUSED`).
  `keep_scoped` keeps the carried line out of the journal
  (`remember::held_by_gap`). Recall, note, hits, forgets, rules, faces and
  a yes stay refused with `HELD_MEMORY_CALL`. The journal: `request N:
  carried: a keep waits for their yes, their face seen saying it; no reads,
  forgets or yes (LAMP_CONVERSATION_CARRIED_SAVES)`.
  The switch is installed whether or not the eyes' reader starts; with no
  reader (the bus would not open) no face is seen, so every line is the
  room's and a late look gives no keeper: `no eyes read who spoke it
  (LAMP_PERSON_SCOPE_SEEN)` (reviewer-two's P2 on #3437). With
  `LAMP_SECOND_PREPARED`, a person-scoped line is not prepared on its early
  words (`prepared_on`): no memory, context or chat for them before its end
  is read; the final words are asked as ever, and a room's line still
  prepares (Second's P2 on #3437). Cost: the owner's own lines lose the
  early start too, even when their face is seen speaking (reviewer-three's
  P3 on #3437).
- **A session's trust is the caller's** (with `LAMP_QUIET_GOING_ON_SAME_ONE=on`
  or `LAMP_GOING_ON_NOT_OWN=on` and `LAMP_SAME_VOICE_MODEL`, the first
  builder at 09:40 on #2428). Past its first two lines a session is trusted
  (`attention::FRESH_REQUESTS`), and a line is let out as "the session has
  heard enough to be trusted" in any room the strict busy rule doesn't
  cover. On lamp-52e6 at 09:5x (#2428's re-run), another voice's 16-word
  line (cosine 0.23) was played so, and only the model's `not_for_me` kept
  the lamp quiet. With either switch, a line let out by trust, with no sign
  of its own but the conversation going on (no name, no call, nobody
  turned, not said close to it, no tap) and no answer to the lamp's
  question, is held when its print scores under 0.30 against the
  conversation's (`same_voice::DIFFERENT_BELOW`), going on or not: `held
  back: the session's trust is the caller's, and the line's voice print
  says someone else`. The caller's own lines in the same session are
  answered as before. A line begun over the lamp's sound, or under 1.5 s,
  has no print and is never held so. Without `LAMP_SPOKEN_TO` the print is
  read for every line (`Heeding::voice_going_on`); with it, trust alone lets
  nothing out. Test:
  `worker::tests::a_trusted_session_does_not_carry_to_another_voice`.
- **Its own answers keep nothing going** (`LAMP_GOING_ON_NOT_OWN=on`, the
  first builder's 17:2x task on #1722). A conversation goes on, on this
  route, by the thread's window (45 s from the start of any answer of the
  lamp's), the 20 s after its words, a busy room's 15 s after them, or a
  call (`Heeding::signs`, `going_on_from`, `went_on_told`). Every answer
  renewed the first three, so one answer to two people's talk let their
  next line out as the conversation going on, and so on: on lamp-4ace at
  16:34 (`6225eb110c2c`), after scene 1's question, requests 31 to 35 were
  each `its answer is played: …, only as the conversation going on`. With
  the switch, once a line is let out only as the conversation going on
  (`attention::Attention::played_on_the_conversation_alone`) and is surely
  another person's, the request is kept (`Heeding::chain`), and the lamp's answers to the requests kept
  open no window (`Thread::said_last_ago_but`; their words' end,
  `Did::Said`, is left out too), in a quiet room or a busy one, and the
  same-voice check's "under the mark" reads the same. Its other lines (a
  reminder, news, Welcome back, a skill's say, an answer to a line said to
  it) open their window as before, so a reply to them goes on (the
  reviewer's 21:39 case: "Your tea is ready." 250 s after the chain was
  cut, "Thanks, I'll go and get it now." 5 s later is answered). A line
  said to the lamp (a call, a face, a reply to its question) and answered
  empties the requests kept, and a line that may answer the lamp's own
  question reads all its words. Surely another person's (the first
  builder's 01:02 on #1722: on lamp-4ace at 00:32 the switch held one
  person's day lines, 12/12 to 9/12, "What do you see right now?",
  "Remember that my favourite colour is green." and "What time is it?"
  each `no_audio_answer`) means its print scored under 0.30 against the
  caller's (`same_voice::DIFFERENT_BELOW`, the first builder at 02:33:
  on lamp-4ace's mic2 on 21 Oct at 02:29 the same voice in a quiet room
  scored down to 0.35, other voices up to 0.29), or the eyes saw a face
  speaking it not turned to the lamp, none of them the answered caller's,
  as the quiet rule above reads it (`Heeding::someone_else`, #2305's
  `caller_faces` and speaking-faces reader, read with this switch too). No
  print (a line begun over the lamp's voice, or under 1.5 s) is never
  another voice and goes on as the conversation (the first builder at
  02:19). The conversation's own check reads the same bar of 0.30 (one
  bar since the first builder's 03:50 on #1722): #829 holds a line under
  it at its verdict, or, known after its answer, ends the conversation at
  the next line, with the switch as without it; the same voice at 0.35
  now goes on there too. A line with nothing to tell
  the speakers apart (no voice model, no speaking face seen) is taken for
  the same person's follow-up, and its answer renews the window as
  without the switch: without a speaking face (lamp-4ace's played test,
  most lamps today) the switch does nothing. The journal says `the lamp's answers to requests [N, …], let out only as the
  conversation going on, keep no conversation going
  (LAMP_GOING_ON_NOT_OWN)`. Not changed: the session fact, the Policy's
  `lamp_spoke_lately`, a checked sound's `lamp_just_asked`, and the Live
  route.
- **Untied talk keeps nothing going** (`LAMP_UNTIED_GOING_ON=on`,
  `crate::untied_going_on`, #3882; off by default and not in the product
  unit). On lamp-06c1 on 28 Oct, with the product unit's environment, made
  voices and no faces, desk1's lines after one answered question ("Did you
  send the slides to Sam yet?", "Not yet, I'll do it after lunch.", "Are
  you coming to the meeting at four?") were each let out only as the
  conversation going on, and each answer opened a new exchange
  (`Thread::lamp_for`), so the next line fell inside the 45 s window again,
  or the 20 s and busy room's 15 s after its words. A line begun over the
  lamp's answer (timing only, the coordinator's `over_the_lamps_voice`) was
  also exempt from the people's-talk note (`Heeding::conversation_note_for`).
  `LAMP_GOING_ON_NOT_OWN` breaks such a chain only with a face or a voice
  print. With this switch, a line let out only as the conversation going on
  (`Attention::played_on_the_conversation_alone`, so with `LAMP_SPOKEN_TO`:
  no name, no call, no face turned, not near, no reply to the lamp's
  question) is **tied** to the lamp when its words ask for the lamp's own
  work (`Heeding::asks_its_work`), refer back (`untied_going_on::refers_back`:
  "say that", "what about", "how about", "pardon", and Vietnamese "nói
  lại", "lần nữa", "thì sao", "cái đó", "gì cơ", "đợi đã"; "that", "it",
  "what", "why", "how", "when", "where", "who", "huh", "really", "again",
  "more", "wait", "stop", "sorry", "đó", "nó", "hả", "sao" (so "tại sao"),
  "khoan", "dừng", "xin lỗi" only in a line of four words or fewer, since
  "Not yet, I'll do it after lunch." and "Sorry, did you send the slides
  to Sam?" are any sentence's; a question of six words
  or fewer opening "and" or "còn", as "And in Hanoi?" or "Còn Hà Nội?";
  a word's `'s` is taken off, so "what's" and "that's" count), or share a
  content word (three letters or more, not `remember::SEARCH_COMMON` or the
  module's small words) with the newest exchange whose answer still
  counts: the lamp's answer, its kept start and its end, and the line it
  answered (`Thread::said_last_but`). Two words are shared when their stems
  are the same (a plural's `s` or `es` off, "glasses" is "glass"; for an
  English word "-y", "-er", "-est", "-ing", "-ed", "-ly" off with a doubled
  consonant, so "sunny" is "sun", "hotter" "hot", "rainy" "rain"), or the
  shorter, of four letters or more, begins the longer ("rain",
  "rainfall"). It is tied too when it asks the lamp
  (`untied_going_on::asks_the_lamp`, Tests on #4562): a question with a
  part opening "should I", "do I", "can I", "is it", "is that", "is
  there", "will it"… ("Should I bring an umbrella?", "Is that hot for
  October?"; not "Are you coming to the meeting at four?"), or a part
  opening, after "can you", "please", "okay"…, with the lamp's light,
  sound or words ("make", "turn", "play", "tell", "check", "set", "dim",
  "remind", "bật", "tắt", "kể"…; not "pass me"), unless the line shows desk
  talk (`untied_going_on::desk_talk`: `turned_away::turns_to_another`,
  "you guys", "hey" someone other than the lamp, "Anh"/"Chị"/"Em" then a
  name at its start, a one-word name set off by a comma at its start, or
  "tell"/"ask"/"remind" then a name). Otherwise it is untied, and:
  - **No renewal (C2):** the request joins `Heeding::chain`, so its answer's
    exchange and its words' end (`Did::Said`) are left out of the thread's
    45 s, the 20 s after the lamp's words and a busy room's 15 s, as
    `LAMP_GOING_ON_NOT_OWN`'s chain is. The lamp still answers it if the
    model does; the thread, memory and every other reader keep the
    exchange as before.
  - **Told it may be people's talk (C1):** it gets `CONVERSATION_ALONE`
    even when it began over the lamp's answer, and with `LAMP_CUT_IN_TOLD`
    the cut-in note is not told beside it (`Engine::telling_the_untied`).
    A tied line begun over the answer ("Sorry, can you say that last part
    again?") keeps the exemption.
  The journal says `its answer is played: …, only as the conversation going
  on, with no tie to the lamp: the model is told it may be people's talk,
  and its answer keeps no conversation going (LAMP_UNTIED_GOING_ON)`, and
  for a cut-in `it began over the lamp's answer, but has no tie to it: the
  model is told it may be people's talk, not that it cut in
  (LAMP_UNTIED_GOING_ON)`. A later line's journal of the lamp's answers
  left out names what put them there: `LAMP_UNTIED_GOING_ON` when all were
  untied lines, `LAMP_GOING_ON_NOT_OWN` when none were (as without the
  switch), both when mixed (`Heeding::chain_by`). **Cost:** a person's own new, unrelated question
  two turns in, with no name, no face and no word of the last answer, is
  not answered; they say the name or face the lamp. Test:
  `worker::tests::untied_talk_after_an_answer_keeps_no_conversation_going`
  (desk1's four lines; "What about tomorrow?", "Say that again.", a shared
  word, "And in Hanoi?", "Còn Hà Nội?", "Why?", "Tại sao?", "That's weird,
  why?", "rain" after "rainy", the name and a reply to the lamp's question
  still play and renew; #3895's eight lamp asks still play),
  `tests::an_untied_line_begun_over_the_answer_is_not_told_it_cut_in` (no
  cut-in note for an untied line, kept for a tied one), and the module's own
  tests. Not yet tried on a lamp.
- **The look by its own call.** With a full look (`LAMP_EYES_LOOK_FILE` and
  `LAMP_LOOK_FULL`), spoken turns are offered `take_a_look`
  (`second_provider::LOOK_CALL`, `worker::chat_tools` with `look`; texts
  are not) and told "When you would look, call take_a_look"
  (`worker::LOOK_CALL_NOTE`). It is one of the core within the route's bound of 48:
  a list at the bound before it keeps it, and the bound cuts the next in
  its order instead (`fetch_page` first), said once as every cut is
  (`past the bound of 48, left out: …`). Added after the bound before, a
  list already at it left the look out and said nothing (the first builder
  on #2602, 22 Oct 12:3x, from lamp-75c9's runs that morning).
  The call asks the coordinator for the turn's full look (`LookAsked`, as a
  skill's look step does), the lamp says "Let me look." if the model said
  nothing, and the turn ends with no further round; the picture comes in
  the look turn after it. In a turn the lamp wrote itself the look is
  dropped (`LookAsked` with why). The journal says `request N called
  take_a_look: the look is asked of the coordinator`. The look by the
  words "Let me look." stays beside it (`speak_up::asks_to_look`); both
  ask for the same turn's look, which is taken once. The first builder on
  #1712: a phrase rule is easy to trip.
- **A skill's look step, with `LAMP_SECOND_SKILL_LOOK=on`** (off unless
  set; #941 on this route). A call the skills answer (`skills/`, in either
  folder or through `use_skill`: `worker::skill_call`), such as
  `skills/look-for-things`' `look_for`, whose answer has `"look": true`
  asks the coordinator for the turn's full look as `take_a_look` does
  (`Engine::looking_for_skills`): the round's other calls are answered,
  the lamp says "Let me look." if nothing of the answer was said, and the
  turn ends there, with no round for the model to say it will look. The
  coordinator asks the eyes once the answer's first audio plays or its
  reply ends, never before it, so the still holds up no answer; the
  picture goes in the look turn after it (`lamp_looks`) to this route's own
  session. A person who speaks first takes the turn and the look is
  dropped (`look_dropped`). Only with a full look; without one the switch
  says so at the start and does nothing. What the skill's answer says to
  do with the picture (its `next` after "When the picture comes:",
  `lamp_skills::told`; at most 1024 bytes, none without those words) is
  kept with the look's session and given once to that session's next look
  turn: in its user message, after the question (`LOOK_FULL_NOTE`), as
  "(The skill you called for this look said what to do with the picture:
  …)", beside the picture. So the model reads the picture with the skill's
  say (point-light's "say whether you can see it… call point_light once
  more", look-for-things' "say where it is… call look_for once more"), as
  Gemini does on Live, where the call's answer stays in its session. Any
  other turn begun first lets it go, and a look turn with no picture (the
  no-look note) leaves it out. In a turn the lamp wrote itself the look is
  dropped (`LookAsked` with why) and the turn goes on: the skill's answer
  goes to the model, whose next round's words are said, as with the
  switch off, with one exception: a search's own turn. A skill's `LookAsked` says `search`; the coordinator
  (`coordinator::look_note`) follows that search, begun in a turn the
  person spoke, and marks its own written turns (its newest `lamp_looks`
  turn and its note's turn: `Written`'s `search`), where the skill's next
  look (`look_for` with the next place) is taken as in a heard turn. Every
  other turn the lamp wrote (a hello, a "Sorry?", a skill's say, a
  reminder, `take_a_look`'s look turn) still drops it, and `take_a_look`
  is dropped even in a search's turn. #1146's rule was that pictures never
  chain with nobody speaking; this chain begins only with a person's
  request, lamp-skills ends it after its last look (look-for-things'
  `[search]`: 5), and a person who speaks or a stop by hand ends it. The
  voice keeps its own bound too, for an older lamp-skills or a skill that
  answers `"look": true` with no `[search]`: a search takes at most 8
  looks (`look_note::SEARCH_LOOKS_AT_MOST`, mirroring lamp-skills'
  `SEARCH_LOOKS_AT_MOST`, "1 to 8"). The 8th look's turn is no longer the
  search's own, so a 9th look asked there is dropped as in any turn the
  lamp wrote, recorded as `look_dropped` with why "the search took its
  looks", and no note follows it. The journal says `request N
  called look_for: the look is asked of the coordinator`. Off, the answer
  goes to the model as before and its words are the look.
- **A note to a search left after a look, with `LAMP_SECOND_LOOK_NOTE=on`**
  (off unless set; only with `LAMP_SECOND_SKILL_LOOK=on` and a full look;
  step 2 of look-for-things on this route). A look's turn whose answer
  called nothing in any round is told to the coordinator
  (`LookNoCall`, before its `TurnComplete`; `Engine::noting_looks`). The
  coordinator (`coordinator::look_note`) knows which look turns are a
  skill's search: a skill's `LookAsked` (with `search`; `take_a_look`'s
  has none) in a turn the person spoke begins a search, one in the
  lamp's own turn goes on with it, and the search's newest `lamp_looks`
  turn is the one a `LookNoCall` may be for. Then, once a search, after the
  answer has played whole and a second of fresh quiet (the holding answer's
  gates), it sends one written turn on the same session, the skill's own
  note from its file (`[search]` `note`, as lamp-skills' tools file gives
  it under `searches`, read as the search begins: `tools::search_walk`;
  look-for-things': "Not found yet? Call look_for with the next place:
  left, right, far_left, then far_right. If you can see it, say where and
  stop."), recorded as `look_note_sent`; a search whose skill gives no
  note gets none (`look_note_dropped`, "its skill gives no note"). It never goes if the
  person speaks or another turn begins first, the answer was cut short,
  the session changed, or 3 s pass without quiet; nor for a later look of
  a search that had its note, nor for an answer that called anything
  (the next place, or a found one): each is `look_note_dropped` with
  `why`. A new search asked by the person may have its own note. Nothing
  waits for it: the lamp's words are said as before, and the note comes
  after them. The journal says `request N answered a look with nothing
  called`. The next place the note asks for is looked at: the note's
  turn is the search's own.
- **The search walks on, with `LAMP_SECOND_LOOK_WALK=on`** (off unless
  set; only with `LAMP_SECOND_SKILL_LOOK=on` and `LAMP_SECOND_LOOK_NOTE=on`;
  step 4 of look-for-things on this route). If the note is left too (its
  turn's answer, or the next look's in the same search, called nothing),
  the lamp walks on by itself: the worker tells a search's note turn left
  so as it tells a look's (`LookNoCall`; `Engine::walking_looks`), and the
  coordinator (`coordinator::look_note`), under the note's gates (the
  answer played whole, a second of fresh quiet), calls the search's skill
  itself, with the call the model made (`LookAsked`'s `call`: its name, the
  skill's own or `use_skill`, and its arguments) and the next place in its
  skill's `walk` (`[search]` in its file; look-for-things': left, right,
  far_left, far_right, then the desk), each skipped if the search already
  asked it (its first look's place is the call's `place`, or the choice's
  `default`, the desk). The call goes on a thread of its own
  through the voice's own call path (`tools::run_call`, to lamp-skills'
  `call.sock`, at most 1.5 s), so nothing waits on it and the lamp's speech
  is never delayed. A picture cannot come back as a call's result on this
  route, so a look in the skill's answer is taken as a skill's look is
  (the eyes asked at once, as nothing is playing) and goes as the search's
  next `lamp_looks` turn, marked as the search's, with "(You looked to the
  left; the picture is that look.)" (the place in the skill's `looked`
  words) and the skill's "When the picture
  comes:" note after the look's question: the finding stays the model's.
  The skill's refusal past its looks (look-for-things' `[search]`
  `give_up`, an error) goes once as a written turn, `look_walk_gave_up`,
  and the search is over; with no place left the walk asks once more with
  its newest place, so a search of five places ends with that line (the
  desk's look and four walked: five looks). A `look_for(found)` from the
  model is a call, so no `LookNoCall` follows and nothing is walked. The
  walk stops at the voice's 8 looks and when its places are spent, and at
  once if the person speaks, another turn begins, the session changes, or
  a stop by hand or a pause comes; each step is `look_walk_asked` with
  `place`, each end `look_walk_dropped` with `why`. The journal says `a
  search's note answered with nothing called is told to the coordinator`.
- **A push ends the search for the voice too** (#2904; no switch of its
  own: the coordinator's `HandIn` reads `body/hand` whenever
  `LAMP_BUS=on`, and does nothing without a search going on, which needs
  `LAMP_SECOND_SKILL_LOOK=on`). lamp-skills ends a
  search when the body says it is pushed (`body/hand`), and the voice reads
  the same topic (`search_pushed::HandIn`: its latest value every 50 ms,
  each push once by its `since_us`, nothing older than a second). On
  lamp-c0a4 on 22 Oct (#2835) the body's search ended 211 ms after a push,
  but the pushed look's picture still went to the model, which called
  `look_for` again 4 s later, and the base turned again. Now the
  coordinator, on a push while a search goes on, drops the held look's
  picture (`look_dropped`, why "the body was pushed") and the search's note
  and walk, and records `search_pushed` (`turn_cut`: whether it cut a
  turn). A search's turn in hand (the one that asked the look, saying
  "Let me look on the left", or a look's, whose picture the model is
  answering) is cut as a spoken stop cuts it (`hand_control_cancel` and
  `turn_finished` with `body_pushed`), and what its request still sends
  is fenced, so the model's answer to the pushed look's picture never
  plays and no second round speaks (El's P2 on #2990). The worker reads the push too
  (`search_pushed::read_from`, `Engine::stopping_searches_by_hand`): for
  30 s (`search_pushed::QUIET_US`) it runs no `take_a_look` and no call of
  the search's skill (`look_for`, or `use_skill` naming it) in a turn the
  lamp wrote or one asked before the push; each is answered "Not done: the
  person stopped the search with a hand. Do not look again unless they ask
  you to in words, and say nothing about the push." and the journal says `request N chat round R: look_for
  not run: the body was pushed`. The person's next request is told, once,
  as `LAMP_STOP_TOLD` tells a stop: "(For you to know, not to answer: the
  person stopped the search with a hand, pushing the lamp as it looked.
  The search is over: do not look again unless they ask you to in words,
  and say nothing about the push.)" (`search_pushed::NOTE`), and that
  request's own words may look again. A push counts only while a search
  goes on: within two minutes of its newest look or search turn taken (a
  walk step's look turn, which the worker never asked, counts too;
  look-for-things holds each look's pose that long) and before the person's next words; any
  other push changes nothing here.
- **Live route only, so not on this route:**
  the head tap for whom it answers, sight and camera facts, and
  `LAMP_MEMORY_RULES`, which reaches Gemini's worker only (`process.rs`).
- **The air (`LAMP_AIR_FACTS=on`, #1712's map item 6):** as on the Live
  route (`crate::air_told`), `air/state` is read every 5 s on the bus
  (`worker::air_task`, a reading older than 30 s says nothing), and once the
  room has felt stuffy for ten minutes a spoken request is told so beside
  its words (`Engine::noting_the_air`, `worker::air_noting`), not in a busy
  room. It's spent only once that request's answer begins to be heard (its
  first audio), then not again for three hours: a line cut short or
  retired before any word keeps it for the next (the reviewer's P3 on
  #1960). It's the answer's first audio that spends it, not the air's own
  sentence: an answer cut in on after its first words counts as told even
  if the note came later in it (the first builder's P3 on #2083). The last
  four requests given it are remembered, so an earlier one's answer heard
  after a later one was given it still spends it. A written turn, the
  lamp's own, never is. The journal, at that first audio: `request N is told the air is stuffy`. Its room
  is the one this route keeps (`room/state`); the room's quiet or busy is
  in each turn's system message with `LAMP_ROOM_FACTS`, as before.
- **How a known person seems (`LAMP_SECOND_SEEMS=on`, #1671):** off, and
  stays off until the reads' measure passes (#2733's labelled session).
  With `LAMP_BUS=on`, `humans/seems` is read every second
  (`worker::seems_task`, `crate::seems_told`), a read fresh within the
  message's 5 s. Two `positive` reads in a row by Blendshape V2, the
  product's reader (distinct messages by their bus time, 0.6 or over,
  `SURE_FROM`), as Mood's own run (#2716); an `unsure`, a `negative`, a
  FER+ read or none breaks the run. The eyes read the **nearest face**,
  not necessarily the one talking, so a read counts only while exactly one
  face is in view (`humans/faces`, `known_faces::faces_in_view`): no face,
  two faces, or an unknown count break the run too. Then a spoken request whose scope is a
  known person in view, not the room's and not one held only by a gap (so
  never while the camera is closed to memory), is told, beside its words,
  that they seem in good spirits from their face just now, with the rule
  never to mention it, their face or their mood, only to let it shape the
  tone, and to ignore it if it doesn't fit their words
  (`Engine::noting_how_they_seem`, `worker::seems_noting`). Not in a busy
  room, never a written turn. Spent as the air's note is, once an answer
  given it begins to be heard, then not again for ten minutes. `negative`
  is left out: brows drawn down while concentrating read as low
  (`LAMP_SEEMS_BROW_LOW`); it waits for the labelled session's at-the-desk
  count (#2733). Nothing of the read is kept past the note: no person, no
  picture. The journal, at that first audio: `request N is told how the
  person seems`.
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
asked once more`. For a piece of a turn's answer in the configured voice,
the line adds `, on a connection of its own`: that second asking goes over
HTTP on a connection opened for it, never the speech socket or a pooled connection
that may have held the first (`net::fresh_client`, `Tts::speak_fresh`), in
the turn's model, so the answer keeps one voice. Failing again, the piece
fails with `services::UNSAID` (`<voice> failed again (…); the sentence
could not be started`), and the answer's player says it with the next
piece, as one, in order (`a piece of the answer could not be started twice;
it is said with the next piece`, then `the piece not said goes with the next
one, as one piece (N words)`), so no sentence is silently dropped (#4338:
on lamp-3e83 on 29 Oct ElevenLabs' first sound timed out at 14:15:13 and
14:15:48, and one sentence of each of those answers was never said). A joined
piece that can't be started either is carried again into the piece after
it. The joined piece is asked for only when the player reaches it, after the
two tries' up to 8 s, and the next piece's speech asked ahead is let go.
It is asked as any piece is, twice, so its first sound can come up to 8 s
after that, or it is carried on in turn. Speech
outside an answer's pieces, such as the wait's sounds and the down line, has
no next piece: a sentence not started twice is not said, as before. Only the answer's last piece has nothing to go with: it is not said,
`the answer's last piece could not be started twice and was not said`, and
the turn ends as ever. Its words were already given as the lamp's (the
turn's `Transcript`, `said_piece`), so the lamp's own record holds words
the person never heard, as it did when a sentence was dropped before this. The announcement is never carried. Before #1844's C the
second failure failed the whole turn, against what this said, and a host
that took the connection and never answered cost the connection's 10 s and
its response's, twice: now at most 8 s a sentence. A sentence begun and
then lost fails as any does. Turnless speech (the wait's sounds, the down
line) takes `UNSAID` as nothing said, as it took an empty answer. The journal at start names the voice: `the voice is ElevenLabs,
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
search's answer when the model's query asks nothing the heard words did
not: it may drop words or change their order, and small words ("the",
"in", "for", …) don't count (`worker::early_adds`). A query with words the
heard ones lacked is searched as asked, and so is a later search
(`worker::Early`). On lamp-4ace on 20 Oct a "what's the weather this
weekend" named no place, so its early search found American towns'
forecasts, and these answered the model's own query for the lamp's city
(#2185). The journal says `request N: a search begins from the words
heard (<n> characters)`, then either `request N web_search answered by the
search begun <ms> ms before the call` or `request N web_search asks <n>
words the search begun from its words did not: searched as asked`. So the search runs while
the model's first round does, which took 0.74 to 0.92 s to its first
event on lamp-4ace on 19 Oct; the search test's first audio was 2.6 to
4.4 s.

**The weather, asked when asked** (`LAMP_SECOND_WEATHER=on`, off unless
set; from the owner on 22 Oct). Spoken turns are given a function of
their own, `weather`, with two optional words: `place`, the place asked
about as it was said, and `day`, `today`, `tomorrow` or a weekday's name
(`net::weather::declaration`). Its description asks the model to call it
for every question about the weather, the temperature or rain, for any
place or day, and to say which place it is answering for. A call with no
place is for the lamp's own (`LAMP_PLACE`, or else the place its zone
names); one with no day, or one it can't read, answers now and every day
of the week ahead by name. Only when it is called is the place looked up
by its first part at Open-Meteo's geocoding
(`geocoding-api.open-meteo.com`), then its forecast fetched from
`api.open-meteo.com`, now and for 7 days, with no key; the two together
within the search's 3 s (`net::weather::WITHIN`). It answers in words the
model can say: `{"place": "Hanoi, Vietnam", "weather": "In Hanoi,
Vietnam: Tomorrow, Friday, 24 to 30°C, light rain, 60% chance of
rain."}`; for now and for today the temperature, the sky, the feeling and
the humidity now come first ("In Hanoi, Vietnam now: 31°C, partly cloudy,
feels like 36°C, humidity 70%."). Today and tomorrow carry their weekday
too, worked out from the forecast's own dates (#2185). Which row is today
is worked out at each call from the place's own date now: the lamp's
clock moved by the forecast's `utc_offset_seconds`, never the lamp's own
date; tomorrow is the row dated the day after, a weekday the next row on
it from today, and rows before today are never said
(`net::weather::place_day`, `days_from`). The reviewers on #2969: today
taken as the first row made a forecast kept from 23:59 at the place say
yesterday's as today's after its midnight. A forecast whose rows lack the
place's today is no answer. A place not found
answers `{"error": "I couldn't find that place"}`, and a failed lookup or
fetch says the weather couldn't be reached: never another place's
weather, nor any older than it is kept. A call waits up to 6 s for its place and forecast (`net::weather::WITHIN`; two of five took past 3 s on lamp-4ace on 22 Oct). Each place's forecast is kept 10
minutes, and never past that place's midnight, so a follow-up ("and
Saturday?") is answered at once and the first call of the place's new day
fetches again, with at most 16 places kept, the oldest out
(`net::weather::Weather`). Where a place's name was found is kept a week
(`PLACE_KEPT_FOR`), at most 32 names, the oldest out, so a later look at a
place, or the first after its midnight, asks only its forecast: one request
in place of two, never anything of who asked. Nothing
is fetched ahead, no task runs, and the model's notes carry no weather.
Before, the forecast of the lamp's own place was fetched every 15
minutes and told in every request, so "what's the weather in Hanoi
tomorrow?" was answered with Ho Chi Minh City's (the owner, 22 Oct 16:0x:
"a really dumb approach, both fetching weather every 15 minutes and only
one location"). The function is answered before the route's runners
(`worker::answering_weather`), given at once with
`LAMP_SECOND_FEW_TOOLS=on`, never held by the Policy, and cut at the
bound after the skills' and before the search (`worker::chat_cut`). Texts
from the app are not given it. Only the place's name and its coordinates
leave the lamp. The journal: `the model may ask for the weather of any
place and day, … here is Hanoi, Vietnam (LAMP_SECOND_WEATHER)` at start,
and for each call `request N asked for weather for a place named,
tomorrow; answered in N ms, fetched` (or `fetched, its place known`, `from
what was kept`, `its place known, its weather from what was kept`, `no such
place`, `the weather was not fetched: …`), never the place's name or any
other word said. It reaches the second route's worker alone
(`process.rs`).

**A dated search.** The search function's description (`net::search::DESCRIPTION`)
asks for the month and year the model was told in a query for news,
results and prices: on lamp-4ace on 19 Oct "Who won the last Formula One
race?" found last season's. It also says never to search a guessed place:
a place it isn't sure it heard right, or "near me" with no place known, is asked
about first, and a search near a place the person didn't name says which
place it used (#4337).

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
key is left out, saying why.

**The relay a run used (#2882, the first builder on #3126).** The probe
reaches the addresses the voice would. With `LAMP_RELAY_ROUTES=on` in its
environment (give it with `-E`, as `systemd-run` does not carry the voice
unit's), it asks the relay for its near addresses once as it starts, where
and with the key the voice asks, and says which services are near (never
where); without it, it asks nothing. Each candidate that goes through the
relay then takes the address the voice's own rule gives it
(`worker::relay_routable`):

- `chat=deepseek-relay(…)`: the routes' `chat` when the route's DeepSeek
  goes through the relay with the relay's key;
- the configured voice: the routes' `speech` when it speaks with the
  relay's key;
- `chat=gemini(…)` and `speech=gemini(…)`: the routes' `gemini` when that
  Gemini already goes through the relay with the relay's key;
- `chat=configured`: never routed.

An address the unit's environment gives (`LAMP_SECOND_DEEPSEEK_BASE_URL`,
`LAMP_SECOND_SPEECH_BASE_URL`, `LAMP_SECOND_GEMINI_BASE_URL`) is pinned and
never routed, as in the voice. Each such candidate's run line, failed or
not, ends `relay=near`, `relay=normal` or `relay=pinned` and the host it
reached (`host=…`, never a path or a key). A near address that fails is
counted as failed, not retried at the normal one: the probe times the near
address alone. Each time runs from its request's start; it is
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
the first builder on #1722, 00:56). Its relay target, for the
chat, `--speech` and `--socket`, takes the relay's near address as the
candidates above do with `LAMP_RELAY_ROUTES=on`, and says which in one line
as it starts (`wire target=relay: relay=near host=…`). On lamp-4ace the relay's answers began
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
Each run begins on the next size (`kind_order`), so no size is always
asked first, or after the same one, and the prompt cache favours none.

Only a 200 is timed, on a connection of its own as on a kept one: a
refused answer's line says its status, and it is counted `closed`, so its
quick answer never lowers the medians.

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

**The request's size** (`--wire --sizes`, #2135; the first builder on
#1722, 20 Oct 16:30). The chat's first event is about 1.6 s median of a
turn's 2.1 s on lamp-4ace. This measures whether a smaller first request
brings it sooner, before anyone trims one. It takes the same command with
`--sizes` after `--wire`. Each run asks each target three requests in place
of `lamp` and `small` (`Size::SIZES`):
- **lamp:** today's spoken first round, as above.
- **few:** the same, given only what `LAMP_SECOND_FEW_TOOLS=on` gives a
  first round. It's the route's own `worker::few_tools` on the chat, so
  `more_functions` is among them.
- **half:** every function, with the character cut to its first half
  (`half`: at the last line break before its middle, else the last space,
  else the middle).

After each size's medians come `wire target=… few-less-lamp …` and
`wire target=… half-less-lamp …`: each step's median less the lamp's
(negative is sooner). Its 10 runs ask 30 requests on each target. As for
`lamp`, its bytes are under a real turn's, so read them beside the route's
own `chat request` journal line.

**The route's speech** (`--wire --speech`, #1934, 20 Oct 08:36). The second
route's speech goes through the relay too: `tts_base_url` is
`llm_base_url` unless set, and ElevenLabs is then reached on the relay's
`…/elevenlabs` path (`tts.rs`, `service_base`). With `--speech`, each run
asks each target for a piece of speech instead of the chat:
- **The piece** is the route's own request as `Tts` makes it, with the
  route's voice and model. It is one of four sentences of 35 to 38
  characters (`PIECES`), an answer's first sentence long, one a run in
  turn, the same on both targets.
- **relay:** `tts_base_url` with the route's speech credential, as the
  route asks. When that is ElevenLabs' own API, it is the direct target and
  the relay is left out.
- **direct:** ElevenLabs' own API (`https://api.elevenlabs.io/v1`) with an
  `elevenlabs.key` unit credential. The route has no ElevenLabs key of its
  own, so without one the direct target is left out, saying so.
- **Timed** as a chat is, to the first byte (the head), then to the
  **first audio** (`first_audio_ms`, the body's first byte) and **done**
  (`done_ms`, the whole piece downloaded), with its `audio_bytes`. An
  answer's first sound waits for its first piece's first audio, so the
  relay's share of that is what each turn pays before the lamp speaks.
- **With `--kept`** as well (`--wire --kept --speech`), on one kept
  connection, as the chat's arms are.

The lines are `wire speech target=… run=…` (`wire kept speech target=…
run=… arm=…` with `--kept`), then each target's medians with the 90th
percentile of the first byte, the first audio and done, and
`wire speech relay-less-direct`, step by step to done. Its 10 runs ask 20
pieces, about 740 characters billed, or 60 pieces and about 2,200
characters with `--kept`. The first line names the model asked:
`systemd-run` doesn't carry the voice unit's environment, so give the
unit's `LAMP_SECOND_SPEECH_MODEL` with `-E` when it sets one.

```sh
systemd-run --pty --wait \
  -p LoadCredential=elevenlabs.key:/path/to/elevenlabs.key \
  /opt/lampOS/voice/current/lamp-voice probe-cascade \
  /etc/lampOS/voice/provider.json /etc/lampOS/voice/second.json 10 --wire --speech
```

**A speech socket opened ahead** (`--wire --socket`, #2135's row; the
first builder on #1722, 20 Oct 10:37). ElevenLabs' `stream-input` takes
text as it comes on a websocket and sends audio back as it's made, so the
route could open one while the chat is still thinking, and its first piece
would go to a socket already open. Through the relay it's under `/ws/`,
`…/ws/elevenlabs/text-to-speech/<voice>/stream-input`, with the relay key as
a bearer (the backend, 10:05 on #1934). On ElevenLabs' own API it takes
`xi-api-key` (`tts::socket_request`). Each run, each target:
- **a piece on a kept HTTP connection,** as the route sends its pieces: one
  piece makes the connection, the next is timed;
- **another piece of the same length on a socket opened 1 s ahead**
  (`SOCKET_AHEAD`, about the chat's time to its first piece on lamp-4ace),
  the two arms first in turn, so neither gains from the other's piece just
  made (the reviewer on #2161):
  - the connect, TLS and upgrade are timed;
  - then the voice's settings, the piece (`flush`) and the end are sent;
  - from the piece's last byte, its first audio and its end are timed.

The lines are `wire socket http target=… run=…` and `wire socket target=…
run=…` (`upgrade_ms`, `first_audio_ms`, `done_ms`, `audio_bytes`), each
one's medians, and `wire socket-less-http target=…`, what the socket saves a
piece once it's open (negative is faster). With both targets there's also
`wire socket relay-less-direct`. A socket refused at the upgrade says its
status, and one closed before any audio says its close code. An answer that
isn't audio ends that run as failed (`answer: refused`), never shown. Each run asks three pieces a
target, at most 20 runs. From this Mac with a made-up key, the relay took
the upgrade and closed with 4001, and ElevenLabs' own API refused it.
The first line names the model asked, the one both arms ask for:
`LAMP_SECOND_SPEECH_SOCKET_MODEL` when set and not blank, as the route's
socket reads it, so the probe times the model the voice would; else
`eleven_flash_v2_5`, the socket's default, never `tts_model`.
`systemd-run` doesn't carry the voice unit's environment, so give the
unit's `LAMP_SECOND_SPEECH_SOCKET_MODEL` with `-E` when it sets one.

Give `-p LoadCredential=elevenlabs.key:…` only with an ElevenLabs key of
its own; without it the relay alone is timed:

```sh
systemd-run --pty --wait \
  -p LoadCredential=elevenlabs.key:/path/to/elevenlabs.key \
  /opt/lampOS/voice/current/lamp-voice probe-cascade \
  /etc/lampOS/voice/provider.json /etc/lampOS/voice/second.json 10 --wire --socket
```

**The route's speech socket** (`LAMP_SECOND_SPEECH_SOCKET=on`, off unless
set; #2135's row, the first builder on #1722, 20 Oct 10:37), the change
`--wire --socket` measures:
- **As each answer begins,** while its chat thinks, the configured voice
  opens ElevenLabs' `stream-input` socket for that request and sends the
  voice's settings (`Tts::open_ahead`, `Services::open_speech_ahead`).
  - It's the configured voice only, and ElevenLabs only; Gemini's voice and
    OpenAI's have none.
  - A socket opened before, for an older request, is closed.
  - The journal says `request N: the speech socket is open, M ms after it
    was asked`, or that it didn't open and why, in kinds only.
- **The first of the request's pieces that finds it open** speaks on it:
  - the piece, made at once (`flush`), and the end;
  - its audio comes as the HTTP piece's does, in packets as loud;
  - the socket is used once.
  - A piece that finds it still opening (the announcement, usually) goes
    over HTTP and leaves it for a later piece.
  - The piece's journal line says `on the speech socket opened M ms
    before`.
- **A socket that fails, closes or stays silent before any audio** sends
  that piece over HTTP instead, saying so. That includes ElevenLabs closing
  an idle socket after 20 s, when the chat runs long on tools.
  - A silent socket costs the piece at most 1.5 s
    (`SOCKET_FIRST_AUDIO_WITHIN`, about twice a kept connection's first
    audio), never the stream's 30 s (the reviewer on #2209).
  - So a piece is never lost to the socket.
  - A socket that failed after taking the piece may have had its text
    counted by ElevenLabs already, so that piece's characters may be
    billed twice. It's rare.
- **A socket piece doesn't mark the kept HTTP connection as used,** so
  `LAMP_SECOND_SPEECH_WARM` still opens that connection for the pieces
  after it.
- **The worker** reads `LAMP_SECOND_SPEECH_SOCKET` (`=on` only), which the
  voice passes to the second worker as it passes `LAMP_SECOND_SPEECH_WARM`.
- **Measured on lamp-4ace (21 Oct 01:50, voice `af7a83a5bb0b`, through
  the relay, 10 runs): the socket never spoke.**
  - Each socket closed `1011` before any audio, 10 of 10.
  - The same piece on the kept HTTP connection had its first audio at p50
    270 ms, p90 393 ms.
  - So the switch stays off. It's tried again only once the relay's `/ws/`
    carries ElevenLabs' `stream-input`.
- **Why it never spoke (lamp-4ace, 21 Oct 17:00–17:19, 53 sockets; the
  backend on #2135):** the socket asked for the route's model,
  `eleven_v4_turbo`, which `stream-input` doesn't serve. ElevenLabs refused
  all 53, and the relay passed each refusal on as close 4400 ("4000 plus
  the status") with ElevenLabs' message, after it had accepted the socket.
  So the lamp counted it open, and the close arrived before any audio.
- **Since then** (`LAMP_SECOND_SPEECH_SOCKET_MODEL`, the backend's ask):
  - **The socket has its own model,** default `eleven_flash_v2_5`, which
    `stream-input` serves (audio 214–309 ms after the piece in the
    backend's test). `LAMP_SECOND_SPEECH_MODEL` stays the HTTP pieces'.
  - **One answer keeps one voice:** once a request's socket opened, every
    piece of its answer is said in the socket's model, those over HTTP
    too (`Tts::model_for`).
  - **`inactivity_timeout=60`** on the socket's URL, since it opens before
    the answer exists and ElevenLabs closes an idle socket after 20 s. It
    opens within the 10 s connect bound, while the chat thinks.
  - **A close is read:** its code and reason go in the journal (`the speech
    socket was closed: 4400 "…"`). So does ElevenLabs' JSON error before
    it (`the speech socket's service said: input_timeout_exceeded: …`).
  - **A 4400, 4404 or 4422 close refuses the model:** that piece goes over
    HTTP, and no socket is opened ahead for that model again until the
    voice starts again, said once. A refused model is refused on every try.
  - **A 429 pauses the sockets** (the reviewer-lead's P3 on #2658): a 429
    answering the socket's upgrade, or a 4429 close, sends the piece over
    HTTP and opens no socket for 60 s (`SOCKET_BUSY_PAUSE`), said once, as
    `too_many_concurrent_requests` does below, so the next answers don't
    each try the open again. The model isn't refused.
- **v4 Turbo takes the dialogue endpoint** (#2602, #2135, 21 Oct; the
  backend's test through the relay). With the socket's model set to
  `eleven_v4_turbo`, the socket is ElevenLabs'
  `text-to-dialogue/stream-input` (`socket_shape`); every other model keeps
  `stream-input` exactly as above.
  - **Its shape:** `…/ws/elevenlabs/text-to-dialogue/stream-input?model_id=eleven_v4_turbo&output_format=pcm_24000`,
    no voice in the path and no `inactivity_timeout`: ElevenLabs' dialogue
    reference doesn't list it, and the socket never waits for text. The first message names the one voice
    (`{"voices":[id]}`, no speed: the dialogue takes only `stability`).
    The piece goes as `{"inputs":[{"text","voice_id"}]}`, then
    `{"flush":true}`, then `{"close_socket":true}`. Audio is base64
    `audio`, decoded into the same 24 kHz PCM; `is_final_audio_for_turn`
    is read past, and `is_final` ends the socket. The backend's test had
    audio 116 ms after the input, against Flash's 214 ms on `stream-input`.
  - **Its cap:** each open dialogue socket holds one of the account's
    concurrent dialogue sessions, idle too (105, then
    `too_many_concurrent_requests`), and ElevenLabs closes it after 20 s
    with no message from the lamp.
  - **So it opens on the reply, never ahead:** as the answer begins
    nothing is opened; the answer's first piece with words opens it
    (`request N: the dialogue speech socket is open, M ms after its piece
    was ready`) and sends its input, `flush` and `close_socket` at once,
    so it is never held between turns and nothing waits on it. Its other
    pieces go over HTTP in `eleven_v4_turbo`, as with `stream-input`.
  - **Its open has 400 ms** (`DIALOGUE_OPEN_WITHIN`, the reviewer-lead's
    P2 on #2658): it opens on the piece's own path, so the connect and
    upgrade get 400 ms, less than a kept HTTP connection's first audio
    (420–690 ms on lamp-4ace), not the 10 s connect bound. Past it the
    piece goes over HTTP, journaled with no words (`request N: the dialogue
    speech socket was slow to open: not open M ms after its piece was
    ready`); a slow open refuses and pauses nothing.
  - **Its failures** are `stream-input`'s: a 4400, 4404 or 4422 close
    refuses v4 for the rest of the voice's run; a 429 at the upgrade or a
    4429 close pauses the sockets for 60 s; a close, error or silence
    before any audio sends the piece over HTTP.
  - **`too_many_concurrent_requests`**, said as ElevenLabs' error or as the
    relay's close reason, sends the piece over HTTP and opens no socket for
    60 s (`SOCKET_BUSY_PAUSE`), said once; v4 isn't refused.
  - **Not carried:** the whole answer on one dialogue socket. The voice
    asks each piece's speech apart and at once, and the dialogue socket
    marks no piece's end before `close_socket`, so it would need the
    answer's end told to the voice, and `keep_alive` while the chat is
    slow.
- **Tried twice and left off: the socket is slower than a kept HTTP
  connection on both lamps** (the first builder on #2602, 22 Oct 04:09).
  - **At the piece** (the dialogue socket above, #2658): on lamp-4ace it
    never opened inside its 400 ms, 16 of 16 (`connect timed out`). Its
    upgrade alone takes 481 ms at p50 on lamp-c0a4 (503 at p90,
    `probe-cascade --wire --socket`), so a fixed 400 ms always misses.
    Even when it opens, the upgrade plus its first audio (319 ms) is about
    800 ms, against HTTP's 298 ms.
  - **At the line's start** (#2729, closed): the socket opened when the
    person's line began, and a dialogue one was kept alive and waited for
    as long as recent upgrades took. On lamp-c0a4 it was open 595–768 ms
    after the line began and ready for every piece (v4 14 of 14, Flash 12
    of 12). The answer's first sound was still later than over HTTP,
    p50 3325 ms (HTTP), 3424 ms (Flash on the socket) and 3957 ms (v4 on
    the socket), from the end of speech, 12 played lines an arm.
  - **Why:** a socket that is already open still gives its first audio
    later than a kept HTTP connection does (lamp-4ace: 633 ms on the
    socket against 298 ms over HTTP). So opening it sooner doesn't help.
    Speech is about 0.5 s of an answer's 2.5 s, and the chat's first event
    (about 1.6 s) is where time is to be won.

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

### Which function a question about a reminder calls (`probe-reminders`)

`lamp-voice probe-reminders PROVIDER_CONFIG SECOND_CONFIG [RUNS]` (1 to 20,
3 by default; `second_provider/probe/reminders.rs`), run as `probe-tools` is,
with the unit's credentials. It measures `list_reminders`' description
([schedule.md](schedule.md), the person's call of 30 Oct) on DeepSeek,
reached as `probe-tools` reaches it, thinking off. Each of eight lines is
asked on its own as a spoken turn's first: the spoken character with the
memory's note, a turn's changing part and the memory note
(`tools::DAYS_NOTE`), with every function a spoken turn declares and the
schedule's, read from the actions part's catalog in `LAMP_ACTIONS_DIR` as
`probe-tools` reads it, so the arms differ only in that one description:
`before` (`BEFORE`, held the same as `lamp_actions::schedule::LIST_BEFORE`
by a test) and `now` (the installed one, which must be this build's: without
"not recall" in it the probe refuses to run).
Four lines ask about a reminder ("what did I ask you to remind me about?",
"when is my reminder?", "when did I set that reminder?", "do I have any
reminders for tomorrow?") and are right with `list_reminders` first; four
are right with anything else (a memory question, a new reminder, yesterday's
talk, a greeting: never listed unasked). It prints counts only.

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
- **The arms**, on `hosted.deepseek_model`, thinking off, reached as the
  live second route reaches it (#2176, `deepseek_route`): through the relay
  with its credential (`llm_credential_file`) when `deepseek_base_url` or
  `LAMP_SECOND_DEEPSEEK_BASE_URL` is set, or `llm_base_url` is the company
  relay's, else DeepSeek's own API with the unit's `deepseek.key`:
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
- "What do you remember about my umbrella?", right when it does **not**
  call `forget`: it was remembered four lines before, so answering from the
  talk is right, as the lamp-4ace runs did (the first builder on #2063);
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

**Then the heeding's four lines** (`HEEDING_LINES`, #2054, the first
builder's 05:43 on #1722): day lines of the hourly played test on lamp-4ace,
said by one person after the two-person rounds, each let out only as the
conversation going on. Each line is judged by the route's own rule
(`worker::Heeding`, with `--spoken-to`) going on from a conversation in which
the lamp asked nothing. It is asked with the note the engine gives it then:
`CONVERSATION_ALONE`, or else the call note. Its first call is put to the
rule's hold (`holds_call`):
- `remind`: "Remind me in two minutes to stretch.": right is `set_reminder`,
  not held;
- `remind-split`: "Remind me, in two minutes, to drink some water.", cut
  short after "Remind me," and gone on with as the next request
  (`continues_from`): the same, judged on the whole chain's words;
- `to-someone`: "Yes, give me five minutes.", said to someone else: right is
  no schedule call run (none called, or held);
- `remember`: "Remember that my favourite colour is green.": right is
  `remember`, never `not_for_me`.

Before #2054, `remind` was always held and `remember` was told it may be
people's talk. The model is asked with every function and the schedule's,
read from the actions part's catalog as a voice with `--schedule-tools`
reads them. Where the catalog has no schedule, the lines aren't asked, and
it says so. It prints `right=N/M` and the functions called first per line,
then a verdict.

**Then the timer said once** (#2089, `LAMP_SECOND_SAID_ONCE`, asked with the
heeding's functions). This is round 2 of "Set a timer for one minute.", as
the engine asks it: round 1's "A minute, starting now." with its `set_timer`
call, then the schedule's committed result. It is asked once without the
switch's note and once with it (`said_once_note`). Right is nothing called
and no second confirmation: no "minute", "starting", "started" or
"counting". The time it goes off at, or nothing, is right. It prints
`right=N/M` per arm, then whether the note's arm was right every time.

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

Each case runs in three arms (#2037, the first builder at 05:23 on #1722). In
**`quality-today`**, the notes are as a spoken turn has them now. In
**`quality-fix`**, `MEMORY_USE_NOTE` is added, a proposed note for the three
causes of the first lamp-4ace run:
- **Snapshot first:** answer from what the snapshot holds, with no call, and
  call `recall` only for what it doesn't hold. Both notes said to call
  `recall` for earlier conversations, which `preference-used` (3/10) followed.
- **A correction:** told by its words ("actually", "now", "not any more"),
  its first call is `forget` of the old thing by its source id, then
  `remember` the new, never words alone. Nothing said what a correction is
  (`correction-first` 0/10, `correction-kept` 5/10).
- **Only what is theirs:** say only what `recall` marks theirs. What someone
  else said in the room is never repeated, named or hinted at; if nothing is
  theirs, say so. Nothing said so (`earlier-today-said` 0/10).

The first wording of the note, on lamp-4ace (the first builder at 06:18 on
#2063), took `correction-kept` to 8/8, `correction-first` to 5/9,
`earlier-today-said` to 4/8 and `preference-used` to 7/9, with the cases
already right unchanged. The tightened wording at `74341122c` (07:11) took
both corrections to 10/10, but `preference-used` to 5/9 and
`earlier-today-said` to 2/10, the room's secret said 8 times. "Say only what
is marked theirs" was read as all memory, and this snapshot is the room's,
speaker unknown, so the coffee went unsaid. It is now about what `recall`
gives, and the snapshot's answer uses its own word.

A third arm, **`quality-marked`**, is the fix with each of `recall`'s things
not marked theirs carrying `"theirs":false` and `NOT_THEIRS` on itself ("said
in the room, speaker unknown: not theirs to hear, never repeat it or its
subject"): the rule beside the words it is about, as the voice's recall
answer would carry it if it wins, rather than only in the note.

At 09:49 on #2063 (the first builder), two more changes, in both `fix` and
`marked`:
- **The preference line opens the use note** (`PREFERENCE_LINE`, ahead of
  `MEMORY_USE_NOTE`'s correction lines, which stay as they are, both
  10/10), as the first builder asked (12:11 on #1722). `19096d05d` had it
  first in the memory section instead, after those lines and beside the
  snapshot; both places are measured. After Friday's lamp-06c1 run (06:43 on #2176: earlier-today 7/10, the model calling nothing), it also says the snapshot never holds what was said in the conversations, and that a question about the talk, today or before, is `recall`'s.
- **Today's recall quotes what they said:** a recall of `days_ago` 0 is
  answered with `said_today`, their heard lines as quoted items, or "nothing
  said today" (`NOTHING_TODAY`) when none is theirs. The note says to answer
  only from those quotes, word for word, and to say it didn't hear one when
  nothing was said.

Each arm and case prints `right=N/M` and, for the wrong runs, why:
`called-<name>` (the first call, or `none`), `no-word` (a word it needs is
missing) or `said-never` (it said a word it must not). The last line names
the arms that got every case right every time. Counts only, no words.

**Then says kept** (`probe/says_kept.rs`, #2510). It counts turns where
the lamp's words say it kept, noted or updated something but no `remember`
was called, before any wording change makes "kept" come only from the
store's answer. Each line is a fresh session asked as `quality-today` asks
a case: the spoken turn's notes, the memory note and what is kept as its
frozen snapshot, then the line. Five **`keep`** lines state or correct a
fact the person wants kept:
- `umbrella-green`: "My umbrella is green now, not violet." (the violet
  umbrella kept)
- `sister-lan`: "Remember my sister's name is Lan."
- `no-sugar`: "I don't take sugar any more." ("Takes coffee with sugar."
  kept)
- `vi-no-coffee`: "Nhớ giùm tôi là tôi không uống cà phê."
- `vi-green`: "À giờ tôi thích màu xanh lá." ("Thích màu tím." kept)

Three **`control`** lines keep nothing: `control-time` ("What time is
it?"), `control-weather` ("Nice weather today.") and `vi-control-weather`
("Hôm nay trời đẹp quá.").

The probe runs no function, so "kept" means a valid keeping call:
`remember` with a non-empty `text`, or `keep_rule` with a non-empty `rule`
(declared only when the lamp has rules on, so the probe has only
`remember`). A claim is one of `CLAIMS` in the request's words, matched as
whole words after folding (lower case, with accents and đ kept, since a
tone mark changes a Vietnamese word): "I'll remember", "I've noted",
"I've updated", "đã ghi nhớ", "đã cập nhật" and the like. A bare "noted"
is a claim only when it opens a clause ("Noted.", "Got it, noted"), so "as
you noted" is not. "Sẽ nhớ", "đã nhớ" and "nhớ rồi" are claims only after
mình, tôi or em ("mình sẽ nhớ", "tôi đã nhớ", "em nhớ rồi"), so "bạn nhớ
rồi" is not. "Got it" alone is not a claim. Each request is, in this
order:
- `called-remember`: any call in it keeps.
- `said-kept-no-call`: **the defect**, a claim with no keeping call (with
  no call, or with another one such as `forget`).
- `bad-args`: a `remember` or `keep_rule` whose arguments the store would
  refuse, and no claim.
- `called-<name>`: another call (the first), and no claim.
- `words`: no call and no claim.

It prints one line for each line with its outcomes, then the totals:
`said-kept-no-call` over every line (the defect), `called-remember=N/M` on
the keep lines, and `control-false-calls=N/M`, the controls that called
`remember`. Of the claim's words, only whether they matched is counted.

**Then a correction of a thing kept earlier in the session**
(`probe/kept_earlier.rs`, #2510). With an empty note, one turn keeps
"violet" (`remember`, answered as the voice answers the store's
`{"ok":true,"id":8}`), and the next says "actually it's green now"
(`kept-earlier`, and `vi-kept-earlier` in Vietnamese). With
`LAMP_MEMORY_REPLACES=on` in the probe's environment, `remember` takes
`replaces` and its answer carries the kept id, so the only id the model
can name is the one that answer gave. Each line prints how many requests
called `remember`, gave `replaces=8`, gave any `replaces`, and claimed a
keeping with no call. With the switch on, a fallback arm follows: the
correction's `remember` ("Favourite colour changed to green.", `replaces`
8) has a word neither violet's nor heard, so `LAMP_MEMORY_KEEP_HEARD`
keeps the heard line without `replaces`, answered as before (`before`)
and with `not_replaced` (`now`); each line prints how many replies said
the old fact changed (`kept_earlier::CHANGED`, unless they say it stays)
and how many said it stays. Lamp-06c1's relay, 22 Oct, 20 runs a line:
`before` 20/20 English and 20/20 Vietnamese said it changed, `now` 18/20
and 13/20; most of the rest called `remember` again with `replaces` and a
checkable sentence. A line in `replaces`' description ("If the answer says
not_replaced, don't say the old one changed.") measured 17/20 and 12/20,
no better, and is not in.

In the live second route with `LAMP_MEMORY_REPLACES=on`, a correction in
the person's words with no `replaces` ("I work at a library now, not the
bakery") is sent with the id of the one shown kept thing it denies; more
than one keeps nothing and the model asks (memory.md, "A correction in
their words, with no id"). With the switch off, nothing changes.

Run it like `probe-cache`, where lamp-actions runs:

```sh
systemd-run --pty --wait \
  -p LoadCredential=deepseek.key:/etc/lampOS/voice/deepseek.key \
  /opt/lampOS/voice/current/lamp-voice probe-tools \
  /etc/lampOS/voice/provider.json /etc/lampOS/voice/second.json 20
```

On a product lamp, which has only the relay's credential and no
`deepseek.key`, give it the product unit's base URL
(`deploy/product/lamp-voice.service`) instead, so it goes through the relay
as the live route does:

```sh
systemd-run --pty --wait \
  -E LAMP_SECOND_DEEPSEEK_BASE_URL=https://campaign-api.autonomous.ai/api/v1/ai/v1/deepseek \
  /opt/lampOS/voice/current/lamp-voice probe-tools \
  /etc/lampOS/voice/provider.json /etc/lampOS/voice/second.json 10
```

With `--recall` last (`probe-tools PROVIDER_CONFIG SECOND_CONFIG [RUNS]
--recall`, `second_provider/probe/recall_first.rs`, #4339) it asks only
whether a line calls `recall` first, each line a fresh session as
`quality-today` asks it, in two arms: `before`, the spoken turn without
`worker::RECALL_BEFORE_NOT_KNOWING` and recall described as it was ("Not
for this conversation", the preference-only line); `now`, as the route
tells it. Five `own-*` lines ask about the person's own life with the
answer only in memory (a train time, what they talked about earlier, a
hotel, a project, a dinner): right only as `recall`. Five `plain-*` lines
(a joke, a capital, the time, small talk, a quiz answer) are right with
any first call but `recall`, since one there would put a round before the
first audio. It prints each arm's line counts, then `own recalled=N/M
plain without recall=N/M`.

Its first line says which way it asks, through the relay or on DeepSeek's
own API, as the voice's start line does: never the URL. The probe only asks the model and
counts the functions it calls; it runs none of them, so it never writes to
the memory store.

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
- **The rule for whom it answers, without one sign:** a tap on the head is
  not given to it, so a line in a busy room that only that would let
  through on the Live route is held here. Like the Live route, it does not
  wait for the Policy's attention decision, which holds only actions.
- **Said from close (`LAMP_ADDRESSED=on`, the worker's `--addressed`)** is
  judged on this route as on the Live route ([attention.md](attention.md#said-to-the-lamp-from-close-to-it-753)):
  the room microphone's rise over its quiet (`room/sound`) and the eyes'
  close faces turned to the lamp (`humans/faces/tracked`), both sampled
  every 100 ms, over the line's sound. Each spoken line is judged once as
  it is judged for whom it answers, published on `attention/addressed`, and
  in a busy room it lets the line out. Only as the lamp's only voice (a
  standby beside Live publishes nothing). Off unless set; off on lamp-4ace.
- **The name signal (`LAMP_NAME_SIGNAL=on`)** offers spoken turns
  `heard_my_name`, as the Live route's sessions are offered it, given at
  once with `LAMP_SECOND_FEW_TOOLS`. Its call is answered as declined and
  runs nothing. The journal says `request N: the model said it heard its
  name`, then, at the answer's first audio, `request N: first sound M ms
  after the model said it heard its name` (or that it has not). On this
  route the call comes in a chat round of its own, so a line where the
  model calls it is answered a round later. A measurement switch, off, as
  on the Live route (attention.md, "One more thing was tried").
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
