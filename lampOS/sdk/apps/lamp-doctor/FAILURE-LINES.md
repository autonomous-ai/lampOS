# Journal lines written when an external call fails or times out

Read from `lampOS/crates/` on `origin/lamp-voice` at `a7f39e23d9` (26 Nov). Paths are under `lampOS/crates/`; `{x}` is a
value the line prints; a quoted part is the exact text. Only production code (not tests). The journal tag is
`lamp-voice` (voice), `[lamp-tasks]` (tasks) or `lamp-control`. `lamp_doctor/cases.py` parses exactly these, and
only these: its tests are built from the formats below.

**What `{error}` is** (`voice/src/second_provider/net.rs:65-92`, `net::Error`'s Display, a fixed vocabulary, never the
service's own text): `unusable service URL`, `{step} failed` (transport), `{step} timed out`,
`service answered HTTP {code}`, `service answered HTTP 429 (usage limit)`, `service answered HTTP 400 (the key invalid)`,
`unexpected {what}`. The `{step}` words: speech-to-text connect / send / stream / final transcript / backlog; speech
socket / connect / first audio / closed before its audio / answer; speech stream error / audio format / audio part;
chat stream cut / data / line length; tool call arguments / index; search answer; request, connection (weather).
Its category (`second_provider.rs:10183`, `fault()`): local, transport, connect_timeout, response_timeout,
authentication (401, 403, key invalid), quota_exceeded (402), rate_limited (429), server_unavailable (5xx),
server_rejected (other), protocol. The category goes on the bus (`voice/decision`), not in a journal line.

## LLM chat (second route)
| file:line | exact format | outcome | fields |
|---|---|---|---|
| voice/src/second_provider.rs:20914 | `lamp-voice second: request {request} chat round {round} had not begun in {ms} ms; the turn ends unanswered` | timeout | request, round, ms |
| :20902 | `... chat round {round} had not begun in {ms} ms; the onboarding line's own words are said` | timeout (handled) | request, round, ms |
| :20894 | `... request {request} look round had not begun in {ms} ms; the lamp says it could not look` | timeout | request, ms |
| :20971 | `... request {request} chat round {round} failed after {ms} ms: {error}` | error or timeout (by {error}) | request, round, ms, error |
| :21024 | `... request {request} chat round {round} failed; the onboarding line's own words are said` | error | request, round |
| :20956 | `... request {request} the chat refused the look's picture ({status}); asked again without it` | refused | request, HTTP status |
| :20985 | `... request {request} the chat refused the history's {n} kept call message(s) ({status}); asked again without them (LAMP_HISTORY_KEEPS_CALLS)` | refused | request, status |
| :15757 | `... request {request} held line's round failed: {error}` | error | request, error |
| :9995 | `... request {request} said it has hit its usage limit (429s for {s} s{...})` | refused (429) | request, seconds |
| :18650 | `... request {request}: an app tool's call is unanswered at {ms} ms with nothing said; said "{line}" (...)` | slow, not failed | request, ms (the line is the lamp's own words: ignored) |
| voice/src/second_provider/worker.rs:1035 | `lamp-voice second: DeepSeek: the person's own key was refused (HTTP {status}); this request goes through the relay (LAMP_OWN_KEYS)` | refused | status |
| worker.rs:1039 | `... the person's own model gave no answer; this request goes to the lamp's own (LAMP_OWN_KEYS)` | timeout | none |
| worker.rs:1043 | `... the person's own model couldn't answer (HTTP {status}); this request goes to the lamp's own (LAMP_OWN_KEYS)` | error | status |

## Speech to text
| file:line | exact format | outcome | fields |
|---|---|---|---|
| voice/src/second_provider.rs:10663 | `lamp-voice second: request {request} speech to text failed: {error}` | error or timeout | request, error |
| :10626 | `... request {request}: its speech to text failed after the next line began, and nothing of this line is said (LAMP_SECOND_NAME_CARRIES)` (the `{}` is one of three phrases) | error | request |

## Text to speech
| file:line | exact format | outcome | fields |
|---|---|---|---|
| voice/src/second_provider/net/tts.rs:233 | `lamp-voice second: ElevenLabs refused the person's own key or voice (HTTP {status}); the lamp's own voice from now on (LAMP_OWN_KEYS)` | refused | status |
| tts.rs:246 | `... the person's own ElevenLabs voice failed a sentence twice ({why}); the lamp's own voice from now on (LAMP_OWN_KEYS)` | error | why |
| tts.rs:801 | `... request {request}: the speech socket refused {model} ({error}): no socket is opened ahead for it again until the voice starts again` | refused | request, error |
| tts.rs:894 | `... request {request}: the speech socket didn't open ({error}); its pieces go over HTTP` | error | request, error |
| tts.rs:1173 | `... request {request}: the speech socket failed before its first audio ({error}); the piece goes over HTTP` | error | request, error |
| voice/src/second_provider/services.rs:1139 | `... {provider} failed to start a sentence ({error}); asked once more{how}` | error | provider, error |
| services.rs:1154 | `... {provider} failed again ({error}); the lamp's own voice is asked for it (LAMP_OWN_KEYS)` | error | error |
| services.rs:1162 | `... the lamp's own voice failed it too ({error}); the sentence could not be started` | error | error |
| services.rs:1170 | `... {provider} failed again ({error}); the sentence could not be started` | error | error |

## Service answers (any provider)
| file:line | exact format | outcome | fields |
|---|---|---|---|
| voice/src/second_provider/net.rs:282 | `lamp-voice second: a service answered HTTP {status}, naming no code` | error | status |
| net.rs:285 | `... a service answered HTTP {status} ({code})` | error | status, code |
| net.rs:295 | `... the service's usage limit (HTTP {status}, usage_limit) resets at {when} (in {s} s)` | refused | status |
| net.rs:299 | `... the service's usage limit (HTTP {status}, usage_limit), no reset said` | refused | status |
| voice/src/coordinator.rs:3349 | `lamp-voice: the provider refuses for now (quota, rate limit or key); staying {s} s before leaving, ...` | refused | seconds |
| coordinator.rs:3372 | `lamp-voice: the provider refused this lamp's key; leaving now to ask once more, and waiting {s} s if it refuses again` | refused | seconds |
| voice/src/coordinator/key_refusals.rs:38 | `lamp-voice: request {request}: a voice service refused this lamp's key ({n} in a row)` | refused | request, count |
| voice/src/coordinator/model_refusing.rs:125 | `lamp-voice: request {request}: the model service refused the turn ({kind}); turns are held ...` | refused | request, kind |
| voice/src/second_provider/worker.rs:16520 | `lamp-voice second: no service host answers after a turn failed on the network: the link is down, checked every {s} s` | error (network) | seconds |
| voice/src/second_provider/net/public.rs:539 | `lamp-voice second: a public service's name looked up through another resolver failed after {ms} ms: {error}` | error | ms, error |
| voice/src/second_provider/net/history.rs:164 | `lamp-voice history: a forget was refused for good ({status}, {why}); let go` | refused | status |

## Live route (Gemini Live)
| file:line | exact format | outcome | fields |
|---|---|---|---|
| voice/src/provider_worker.rs:5137 | `lamp-voice provider: request {request} got no answer in {ms} ms; closed as unanswered, connection kept` | timeout | request, ms |
| :5130 | `... request {request}, said over the lamp's reply, got no answer in {ms} ms; closed, connection kept, and not counted toward a busy room` | timeout | request, ms |
| :5143 | `... a sound (request {request}) got no answer in {ms} ms, as a sound should; closed, connection kept` | timeout (expected) | request, ms |
| :5149 | `... reminder request {request} got no answer in {ms} ms; closed, connection kept` | timeout | request, ms |
| :3724 | `lamp-voice provider: connection lost ({error}); output already received is still delivered` | error | error |
| :3743 | `... request {request} was lost at {stage} ({error}); its line is kept to send again once on the next session` | error | request, stage, error |
| :3730 | `... request {request} settled by {error} after generation completed` | error | request, error |
| :3648 | `... recovered after {error}; session={n} context={c} attempts={n} outage_ms={ms}` | recovered | error, ms |
| :5167 | `... refused a server message ({error}); fields: {shape}` | refused | error |
| :2490 | `... {n} requests in a row went unanswered; renewing the session when it is idle` | error | count |

## Harness and agent tasks
| file:line | exact format | outcome | fields |
|---|---|---|---|
| voice/src/agent_tasks.rs:4901 | `lamp-voice provider: request {request}: no answer to the status before preparing, {ms} ms in` | timeout | request, ms |
| agent_tasks.rs:5118 | `... request {request}: no answer to the preparation, {ms} ms in` | timeout | request, ms |
| agent_tasks.rs:6348 | `lamp-voice: tasks: {NOW} was refused: {code}` | refused | code |
| agent_tasks.rs:6411 | `lamp-voice: tasks: {STATUS} was refused: {why}` | refused | why |
| voice/src/second_provider.rs:18707 | `... request {request}: the make has no answer {s} s after its waiting line: it runs on, its one line is said, and the turn ends` | timeout | request, seconds |
| second_provider.rs:18726 | `... request {request}: the make has no answer {s} s after its waiting line: its one long-wait line is said` | timeout | request, seconds |
| tasks/src/skill_task.rs:4127 | `[lamp-tasks] skill task {run}: Harness didn't make the fixed write's run (new_failed) and didn't answer within {s} s, so it isn't asked again` | timeout | run, seconds |
| skill_task.rs:4113 | `... Harness didn't make the fixed write's run (new_failed); it answers now with nothing in the run's folder, so it is asked once more` | error | run |
| skill_task.rs:4122 | `... Harness didn't make the fixed write's run (new_failed), but an agent is in the run's folder, so it isn't asked again: it may have begun` | error | run |
| skill_task.rs:3172 | `[lamp-tasks] skill task {run}: a run without Harness is unavailable; Harness is signed out, so trying Harness's own lamp target` | refused | run |

## Memory and app calls
| file:line | exact format | outcome | fields |
|---|---|---|---|
| voice/src/remember.rs:7121 | `lamp-voice provider: no Days memory worker; memory unavailable` | error | none |
| remember.rs:2900 | `lamp-voice provider: the memory refused a room line's keeper; kept without it` | refused | none |
| remember.rs:2852 | `lamp-voice provider: an older memory refused a selective forget; sent as before` | refused | none |
| control/src/app_calls.rs:173 | `lamp-control: app {name}: a call's end not told to the voice: {error}` | error | app, error |
| voice/src/app_tools.rs:715, :721 | `... app tool {function} refused: ...` | refused (policy, not a failed call) | request, function |

## Not journalled (no line to parse)
- A tool call's own 4.5 s timeout: the journal shows it as the chat round that "had not begun in {ms} ms" (above).
- web_search and weather failures: the journal has only `web_search answered in {ms} ms` (search.rs:207); a failure
  reaches the chat as the tool's result, not as a journal line.
- STT, chat and TTS fault **categories**: on the bus (`voice/decision`), not in the journal.
