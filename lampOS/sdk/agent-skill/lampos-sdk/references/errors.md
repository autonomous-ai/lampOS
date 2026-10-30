# Errors, and what to do

Every refusal carries `code` and `why`, the lamp's own words. Read `why`
first: it says exactly what was wrong.

| code | what to do |
|---|---|
| `unpaired` | Pair again with a new code from the owner's app (`lamp pair CODE --pin PIN`): this computer isn't paired, was revoked, or its key no longer matches. |
| `no_hello` | Say hello first: `Lamp.connect()` does; with `request()` alone, send `hello` before anything else. |
| `no_grant` | Ask the owner to tick this grant for this computer in the app, then connect again: grants are read at hello. |
| `no_lease` | Hold a `body` lease first (Python `lamp.lease("body")`, JS `lamp.withLease(['body'], ...)`); `joints` names it. |
| `lease_lost` | The lamp took the body back; `why` says why (a conversation, a hand, private, a stop). Let it be, and take a new lease later. |
| `busy` | A motion still plays (a gesture, move or feeling's move, whoever asked): wait `retry_ms` and send it again. Without `retry_ms`, someone else holds the part (the owner's app, another computer) or your own earlier lease does: try later, or let go of yours first. |
| `refused` | The part's own check said no, in its words in `why` (out of reach, under a pattern's least level, not routed on this lamp yet). Change the request: the same one is refused again. |
| `quiet_hours` | It's the lamp's quiet hours (`limits.quiet_hours`, by default 22:00 to 07:00): `say` and `sound` wait until they end. A light is still fine. |
| `private` | The person set the lamp private: it neither sees nor hears for computers. Wait for `lamp.changed` with `private` false. |
| `paused` | The person paused the lamp. Wait for `lamp.changed` with `paused` false. |
| `asleep` | The lamp is asleep. Wait for `lamp.changed` with `asleep` false. |
| `rate` | Too many of one kind too fast: wait `retry_ms` and send it again. With the rates off (the default), only refused actions and phrase changes are bounded. |
| `bad_request` | The request doesn't fit the protocol; `why` names the field. Check it against the schema (control.schema.json). |
| `off` | Developer access is off on this lamp: the owner turns it on in the app. |

Not from the lamp, but from the SDK: `not_paired` (no pairing here: run `lamp pair`),
`wrong_lamp` (the certificate isn't the pinned one: pair again), `closed`
(the connection ended: connect again; a lamp restarts on every update).
