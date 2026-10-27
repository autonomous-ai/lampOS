# The lamp's programming door: `lamp-control`

The design is on #3504 (Nine, 27 Oct, from my person's four decisions: developers first,
local only, open within the body's limits, sharing from day one). This page is
the contract as built. It grows with each piece, and says what isn't built
yet.

**Built so far (piece 1, `crates/control`):** the protocol's messages and
checks (`protocol.rs`), the lease rules (`lease.rs`) and the rates
(`rate.rs`), with their tests. **Nothing listens yet**, and there's no unit:
the server, pairing, discovery and the routing to the parts are the next
pieces, off unless `LAMP_CONTROL=on`.

## Who may call

Only a computer the owner paired, with the grants the owner ticked. An
unpaired caller gets `unpaired` and the connection closes. Grants:

| grant | allows |
|---|---|
| `body` | `gesture`, `move`, `play`, `look`, `joints`; with `light`, `express` |
| `light` | `light`; with `body`, `express` |
| `speak` | `say`, `sound` |
| `look` | presence events and state: arrivals, leavings, facing, standing up. Positions, never who |
| `hear` | `phrase.heard` for the client's own phrases (never the words around them), `name.heard` |
| `sound` | `sound.heard`, by class |

Camera frames, audio, transcripts, face prints and names never go through
this door.

## The wire protocol (v1)

One JSON object per WebSocket text frame on `wss://<lamp>.local:7731/v1`
(to come; mDNS `_lampos._tcp`, a per-lamp certificate pinned at pairing).

- **A request** is `{"id": N, "op": "...", ...}`, where `id` is a whole
  number of 0 or more.
- **A reply** is `{"re": N, "ok": true, ...}` or
  `{"re": N, "ok": false, "error": {"code": "...", "why": "...", "retry_ms": M}}`.
  `re` is null when the request's id couldn't be read.
- **An event** is `{"ev": "...", ...}`.
- **A field the op doesn't know refuses it** (`bad_request`), so a typo is
  never a step silently not taken.

| op | fields | needs | lease |
|---|---|---|---|
| `pair` | `name` (1–64 chars), `key` (Ed25519 public, base64), `proof` (HMAC-SHA256 of the owner's code, base64) | — | — |
| `hello` | `client` (`c_` + hex), `sig` (Ed25519 over the lamp's nonce, base64) | — | — |
| `subscribe` | `events` (at least one), `phrases` (≤ 16, ≤ 64 chars, only with `phrase.heard`, which needs them) | each event's grant | — |
| `unsubscribe` | `events` | — | — |
| `get` | `state`: `presence` (`look`), `light`, `mood`, `lamp` | — | — |
| `lease.take` | `parts` (`body`, `light`, `voice`), `for_ms` 1–30,000, `why` ≤ 200 chars | `body`, `light` or `speak` for each part | — |
| `lease.renew` | `lease`, `for_ms` 1–30,000 | — | — |
| `lease.release` | `lease` | — | — |
| `express` | `feeling` (a lowercase name, the `feelings` skill's), `strength` 0–1 (0.5) | `body` + `light` | — |
| `gesture` | `name` (a built-in) | `body` | — |
| `look` | `at`: `"face"` (+ `look`), `"name"`, `"ahead"`, or `{yaw, pitch}` in degrees (±180, ±90) | `body` | body |
| `move` | `step_ms` 20–1,000, `frames` 2–500 of five joints (body counts from home), first and last all 0 | `body` | body |
| `play` | `name` (a recorded move of the client's package) | `body` | body |
| `joints` | `lease`, `targets` (`pitch`, `roll`, `yaw`, `shoulder`, `elbow`; degrees, at least one), `within_ms` ≤ 5,000 | `body` | body |
| `light` | `colour` (one of the ten: yellow, red, green, blue, cyan, purple, orange, pink, white, warm), `level` 1–5, `pattern` (`breathe`, `candle`, `pulse`), `for_ms` 1–30,000 | `light` | — |
| `say` | `text`: words, ≤ 300 chars | `speak` | — |
| `sound` | `name` (a clip of the client's package, ≤ 3 s) | `speak` | — |
| `stop` | — | — | — |

Every op but `pair` and `hello` needs `hello` first on the connection.
Every motion still goes through `lamp-body`'s own limits; `move`'s shape is
checked here, and the body's move check (`crates/body/src/moves.rs`) judges
the rest.

**Errors:**
- `unpaired`, `no_hello`, `no_grant`;
- `no_lease`, `lease_lost`;
- `busy` (a higher priority holds the part);
- `refused` (the part's own check, with its why);
- `quiet_hours`, `private`, `paused`, `asleep`;
- `rate` (with `retry_ms`);
- `bad_request`;
- `off` (developer access off in the app).

## Leases

- **Taking:** a client takes `parts` for at most 30 s, and renews before
  that lapses. One take may be renewed for at most **10 minutes**; then it
  takes again, so no client keeps the lamp by renewing.
- **One holder:** one client holds a part at a time, and a second take of a
  held part is `busy`. Ids are `l_` and a number.
- **Events:** `{"ev": "lease", "lease": "l_81", "state":
  "held|suspended|lost", "why": "..."}`.
- **Suspended,** held again when the talk ends if it hasn't lapsed:
  `conversation`. While suspended, the lease is still the holder's, so
  nobody else may take the part, and its clock runs.
- **Lost:** `hand`, `stop_heard`, `private`, `paused`, `asleep`,
  `expired`, `taken_by_owner_app`, `revoked`, or `released`. An act on a
  lost lease is refused with how it ended.

## Rates, per client

| class | ops | most |
|---|---|---|
| joints | `joints` | 20 a second |
| motion | `gesture`, `move`, `play`, `look` | 1 each 2 s |
| light | `light` | 5 a second |
| say | `say`, `sound` | 6 a minute and 60 an hour |
| express | `express` | 1 a minute |

A refusal says `retry_ms`, the time until the oldest counted action leaves
its window. A refused action isn't counted.

## To come

These pieces follow one PR each, each off by default:
- the server: WSS, mDNS, the per-lamp certificate, pairing and `hello`
  with Ed25519;
- each action routed to the part that does it, under the lamp's own user,
  with the priority stamped by `lamp-control`, never the caller's word;
- the events from the bus;
- the journal line per command;
- the app's switches;
- the SDKs and `lamp-mcp`.
