# OpenClaw gateway wire fixtures (openclaw@2026.11.2)

Real WebSocket frames recorded from the OpenClaw gateway, used as golden
fixtures for the Rust client and its fake gateway.

- **Gateway version:** `openclaw@2026.11.2` (npm package),
  `server.buildId` `2026.11.2-release-b556fdf457b0-2026-11-07T15-35-25.381Z`, protocol 4.
- **Recorded:** 2026-11-23, on a macOS dev machine (heavily loaded, so absolute
  timings are slower than on an idle host).
- **Client:** small Node script using the `ws@8.22.0` copy bundled in the
  openclaw package. No device identity.

## How it was recorded

The gateway ran fully isolated: a fresh temp `HOME`, `OPENCLAW_STATE_DIR` and
`OPENCLAW_CONFIG_PATH` under `/tmp/ocgw-trace/`, loopback bind, port 28789,
`OPENCLAW_NO_AUTO_UPDATE=1`, `OPENCLAW_DISABLE_BONJOUR=1`,
`update.checkOnStart: false`. Started with
`openclaw gateway run --port 28789 --bind loopback --verbose`.

Config (token redacted):

```json
{
  "gateway": { "mode": "local", "bind": "loopback", "port": 28789,
               "auth": { "mode": "token", "token": "<token>" } },
  "models": { "mode": "replace", "providers": { "stub": {
      "baseUrl": "http://127.0.0.1:28950/v1", "apiKey": "stub-key", "api": "openai-completions",
      "models": [{ "id": "stub-1", "name": "Stub 1", "reasoning": false, "input": ["text"],
                   "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 },
                   "contextWindow": 128000, "maxTokens": 4096 }] } } },
  "agents": { "defaults": { "workspace": "/tmp/ocgw-trace/workspace",
                            "model": { "primary": "stub/stub-1" }, "utilityModel": "" } },
  "tools": { "exec": { "host": "gateway", "mode": "full", "strictInlineEval": false } },
  "update": { "checkOnStart": false }
}
```

The model was a local Python OpenAI-compatible stub (streaming SSE
`/v1/chat/completions`), with no real API keys:

- default: streams `hi there!` in two chunks;
- message contains `tool`: returns one `exec` tool call `{"command":"echo hi"}`,
  then `The command printed hi.` after the tool result;
- message contains `slow`: streams `slow reply done.` over about 5 s;
- message contains `fail`: HTTP 400 with an OpenAI-style error body.

Connect params for every case except (b) and the read/write part of (l):

```json
{"minProtocol":4,"maxProtocol":4,
 "client":{"id":"gateway-client","version":"0.1","platform":"linux","mode":"backend"},
 "role":"operator",
 "scopes":["operator.read","operator.write","operator.admin","operator.approvals"],
 "caps":["tool-events"],"auth":{"token":"<token>"}}
```

## Record format

One JSON object per line:

| Field    | Meaning                                                                |
| -------- | ---------------------------------------------------------------------- |
| `ts`     | ISO wall-clock time at the client                                      |
| `tRelMs` | ms since the script (or, for j2, the gateway spawn) started            |
| `conn`   | connection label within the file (`c1`, `rw`/`admin`, `pre`/`post`, `tryN`) |
| `dir`    | `out` (client to gateway), `in` (gateway to client), `meta` (socket lifecycle notes) |
| `frame`  | the exact JSON frame (for `in`/`out`)                                  |
| `kind`, `code`, `reason`, ... | for `meta`: `ws-open`, `ws-close` (close code and reason), `ws-error`, `handshake-done`, `sending-SIGTERM`, and so on |

## Redactions

The shared token is `<token>`. `hello-ok.auth.recoveryScope` is `<recoveryScope>`.
Plugin surface capability URLs end in `<cap>`. The host name in `presence` /
`snapshot` is `gateway-host.local`, its LAN IP `192.0.2.10`, and the hardware
model `Mac-model`. Nothing else was changed.

## Files

| File | Case |
| ---- | ---- |
| `a-handshake.jsonl` | challenge, connect, full `hello-ok` |
| `b-client-id-test.jsonl` | `client.id:"test"`, no device: hello-ok with `scopes: []`, then `sessions.list` FORBIDDEN |
| `c-wrong-token.jsonl` | `AUTH_TOKEN_MISMATCH` error then close 1008 |
| `d-sessions-list.jsonl` | `sessions.list` on an empty store |
| `e1-chat-send-text.jsonl` | `chat.send` `say hi` (`idempotencyKey: lamp-t1-1`), all `chat` + `agent` events |
| `e2-chat-send-tool.jsonl` | run with one `exec` tool call (`item`, `tool`, `command_output` streams) |
| `e3-chat-send-model-error.jsonl` | provider 400: lifecycle `error`, chat `error` |
| `f-chat-history-inflight.jsonl` | `chat.history` during a slow run (`inFlightRun`) and right after `final` |
| `g1-chat-abort-before-model.jsonl` | `chat.abort` before the model started |
| `g2-chat-abort-mid-stream.jsonl` | `chat.abort` after the first assistant delta, then `chat.history` |
| `h1-chat-steer-before-model.jsonl` | `queueMode:"steer"` sent before run 1 reached the model: becomes a follow-up run |
| `h2-chat-steer-mid-stream.jsonl` | steer sent mid-stream: folded into run 1; run 2 gets an immediate empty `final` |
| `i-exec-approvals.jsonl` | `exec.approvals.get`, `set` with `baseHash`, a stale `baseHash`, a missing `baseHash`, `get` again |
| `j1-startup-first-boot.jsonl` | first boot: listener up but `UNAVAILABLE startup-sidecars`, close 1013 |
| `j2-startup-window-after-restart.jsonl` | connect loop every 250 ms after spawn: ECONNREFUSED, then UNAVAILABLE/1013, then hello-ok |
| `k-term-mid-run-restart.jsonl` | SIGTERM mid-run (`pre`): run drains, `shutdown` event, close 1012; reconnect after restart (`post`) and `chat.history` |
| `l-sessions-reset.jsonl` | `sessions.reset` with read/write scopes (FORBIDDEN) and with admin (ok) |

## Restart recovery (lamp ee17, 2026-11-23, not a recorded file)

From the gateway's journal on a lamp, no wire capture. At 15:12:33 the gateway
closed with 1012 while a lamp task's run was in flight. At 15:15:54 the
restarted gateway logged `[main-session-restart-recovery] started interrupted
main session: agent:main:lamp`. The recovery candidate had `sourceRunId` ==
`recoveryRunId` == the lamp's own run id (`lamp-lamp-33dcf…-1795421437084`),
with reason `dispatch_started`. The gateway ran the task again by itself and
logged `restart recovery terminal … status=ok reason=completed` at 15:17:56.
So a run in flight at a restart is recovered by the gateway on its own.
A second restart on ee17 (17:0x) logged the same recovery 76 s after the
gateway was ready. It ran 4 min 22 s, and its follow-up run had a gateway
uuid (`d280be1c…`), not the lamp's run id. A short task handed over at
17:01:04 ran only at 17:05:01, behind it. The client now fails a run at
the drop (`DROPPED`, the owner's call) and aborts such a stray recovery run
once it shows (`io::abort_stray`, `Session::abort_strays`).
