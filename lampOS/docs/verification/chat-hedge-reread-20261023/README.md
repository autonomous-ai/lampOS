# Delayed chat request: corrected-head re-read

Second re-read public #3107 at
`1d03e57b531843a8abe39091d7d711437dab8b33`, whose parent is `001a3d714`.
The two independently reproduced P2 contracts now pass. No new P1 or P2
was found in the scoped correction read; one diagnostic wording P3 remains.

## Source and counterexample parity

- The delayed closure in `second_provider.rs:6903` checks the captured
  lease immediately before `services.chat_again`. A revoked lease returns
  `None`; `asked_again:6304` continues waiting for the original stream under
  the existing outer deadline, without sending a second request.
- `services.rs:911-918` restores own-key refusal handling: a refused own
  key invokes the configured callback and sends through the configured
  relay using a fresh connection. Function selection is preserved.
- The original camera-revocation counter was adopted with an identical
  function body except for its name. The refusal counter differs only in
  its name and formatting; both bodies are identical after Rust 1.96.0
  formatting. `adopted-parity.json` retains the direct and normalized hashes.

The [original two deliberate failures](../chat-hedge-counters-20261023/README.md)
remain separate evidence. The corrected replay ran the adopted tests on
the author's clean source without applying or weakening another patch:

```sh
cargo +1.96.0 test -q -p lamp-voice --lib --offline an_ask_again_ -- --nocapture
```

Native Mac, `Darwin arm64`, Rust 1.96.0 (`aarch64-apple-darwin`):
**2 passed, 0 failed, 0 ignored, 3,502 filtered out**, exit 0. Test time was
0.02 s; the complete command took 51.363 s. The full output is `replay.txt`;
`replay.json` binds the command, source hashes and output digest. All 6,509
tracked regular files under `lampOS/` and `.github/workflows` retained their
hashes during the command; the review checkout remained clean.

The camera case keeps the microphone turn alive, verifies its original
personal-context request, then closes the camera at two seconds and observes
zero delayed requests. Its intact-scope control still observes one. The
refusal case verifies the ordinary request first, then the fresh request;
both use the configured relay and report the own-key refusal.

## Remaining limits and P3

The new outgoing diagnostic logs before `post(...).await`
(`net/chat.rs:569`, before line 579). It proves a local send attempt, not
that bytes left the process or reached the relay. DNS, connection or TLS
failure, and an HTTP refusal, can also produce that line without the later
successful-response diagnostic. The definite causal wording at
`net/chat.rs:565-568` and `docs/second-voice.md:390-392` should say what was
attempted rather than imply a lost request or a failed reused connection.
The diagnostic itself contains fixed mode wording and elapsed time, not
the prompt, credential or endpoint.

Fourth's separate #3064 per-dispatch permission guard remains an integration
gate. The two passing tests do not establish authority across later awaited
near/own/refusal fallbacks. This re-read is not a full Voice/VoiceEval gate,
native Linux qualification, real provider measurement or lamp result. The
author's failed Live-fixture command and its historical cause remain
separate from these two passing contracts.
