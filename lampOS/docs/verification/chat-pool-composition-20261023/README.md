# Chat pool policy and fresh requests: scoped source read

Second read public #3115 `45b8ef5de` alongside #3107 `6f603c138`.
Reviewer-two owns the full #3115 review. This read covers construction of
the retained and fresh clients; it is not a combined build or runtime test.
No additional P1 or P2 was found in that scope.

In #3115, `net::http_client` delegates to `client_kept(None)`. Its TLS,
HTTPS-only policy, redirect refusal, ten-second connection timeout and
thirty-second read timeout remain the same. `chat_client` applies the
twenty-five-second pool idle limit only when `LAMP_SECOND_CHAT_KEPT` is
exactly `on`. `Chat::with` uses that client and selects the twenty-second
warm eligibility threshold instead of sixty seconds.

In #3107, `Chat::send_fresh` constructs a new `http_client` for each attempt
and passes it to `send_on`. It does not borrow the retained chat pool.
Composition must preserve both constructors: `chat_client` for `Chat::with`
and `http_client` for `send_fresh`. The fresh path remains independent of
the retained pool's idle limit. This source read does not measure whether
warming completes before a spoken line ends.

## Pinned dependency defaults

The cached reqwest 0.13.5 crate archive hashes exactly to the checksum in
this head's `Cargo.lock`. The extracted `async_impl/client.rs` also matches
the archive byte for byte. Its builder defaults are a ninety-second pool
idle timeout, fifteen-second TCP keepalive idle time, fifteen-second probe
interval and three retries. `reqwest-provenance.json` records the archive
and source digests and the default field values with their source lines.

Thus #3115 preserves the existing keepalive values; its behavioral changes
are the pool idle limit and warm eligibility threshold. The fresh request's
default client already uses the same keepalive settings. This independently
supports reviewer-two's existing documentation P3, rather than identifying
a second fault. Neither a keepalive default nor a shorter idle limit proves
that a router retains a mapping or explains a historical provider stall.

Fourth's #3064 per-dispatch permission work remains required before #3107
activation. This read does not clear that guard, real-provider performance,
native CI, lamp acceptance or merge. No Cargo command, provider request or
device action ran for this source-only composition read.
