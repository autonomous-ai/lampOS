# Photo first-pass input: source-owner contract proposal

Read-only source audit on Main `6b7f929300ab0134b3a588c3b588aa893f64732b` and Common `e9d00f935029a6d5054ddf35b0203fb8546f7995`. This is a proposed interface, not implemented provenance, runtime qualification or physical evidence. It takes no ownership of First's active soft-start/name-continuation fixes.

The current path loses information: `MicrophoneFrame` has capture epoch/generation/sequence; `ObservedAudio` drops its capture association, `queue_input` renumbers, retained route commands are cloneable/rebindable, Engine treats incoming Audio as fresh, and Live resend uses a fresh Instant. A valid transport boot/sequence, face or lease cannot establish first-pass origin. Normal capture DSP/VAD buffering must also remain distinct from logical replay.

Source hooks: [capture/coordinator](https://github.com/autonomous-ai/lampOS/blob/6b7f929300ab0134b3a588c3b588aa893f64732b/lampOS/crates/voice/src/coordinator.rs#L8080), [route retention/rebind](https://github.com/autonomous-ai/lampOS/blob/6b7f929300ab0134b3a588c3b588aa893f64732b/lampOS/crates/voice/src/coordinator/second.rs), [provider replay](https://github.com/autonomous-ai/lampOS/blob/6b7f929300ab0134b3a588c3b588aa893f64732b/lampOS/crates/voice/src/provider_worker.rs), and [Common one-shot recorder ownership](https://github.com/autonomous-ai/lampOS/blob/e9d00f935029a6d5054ddf35b0203fb8546f7995/lampOS/crates/voice/src/memory_scope.rs#L613). The existing recorder is a reusable ownership pattern, not existing original-input evidence.

Recommended split:

| Owner | Proposed responsibility |
| --- | --- |
| Capture/coordinator | Non-cloneable candidate before capture metadata is discarded; bind source span, worker/session/request and generations. Hold/replay/join/rebind/reissue permanently downgrade; stop/forget/privacy/restart revoke. |
| Existing optional private role channel | Begin/Seal/Reject notices, bounded one active slot. Keep strict existing public DTOs unchanged. Missing/late/wrong-key/overflowed evidence denies Photo only and never delays normal readiness. |
| Accepted-audio owner, on each route | Match successfully accepted nonempty input to the producer spans, then consume a one-shot seal alongside Common's recorder. A cloned read lease cannot mint or refresh origin. |
| Photo consumer | Read opaque evidence and current revocation only; never upgrade it from timestamps, face matching or a lease. Retain all independent consent/eye/final-write fences. |

Proposed, unmeasured budgets: at most three notices per line, each encoded at most256bytes; one fixed receiver slot, no PCM retention or historical ledger; nonblocking work targeting under5microseconds per capture block and under50microseconds per worker tick. Validity ends at the existing scope deadline or original audio end plus30seconds, whichever comes first. Reconnect and resend never extend it. Serialization/resource and real-target latency tests must validate these targets before acceptance.

New/new can qualify only after matching/sealing. New/old, old/new, absent optional channel and old/old retain ordinary voice with no Photo origin. Test partial upgrades explicitly; the current binaries do not implement this behavior. This is trusted-process evidence, not protection against a compromised same-UID worker or a dishonest producer, and not proof of acoustic identity/liveness. No hash/nonce ledger or new service is indicated by the audited path.

Required regressions: original positive versus cloned/held/rebound input; early-end/name/cut joins; Live resend and Second network reissue; direct/manufactured input; missing/reordered/repeated/overflowed notices; retirement, generation and privacy changes; cloned leases; old/new matrix and slow dependency/resource bounds. Assert ordinary input still works while Photo proof stays absent. These are proposed cases, not executed results.

First retains producer/admission hooks and coordination with #3392/#3396. Fourth's consumer can require/read opaque proof without inventing producer truth. Eyes retains camera and final mutation fences. Original-input provenance remains separate from Photo's still-unimplemented strict whole-interval coverage with at least three distinct valid looks, including initially Room/unnamed input. PhotoSpeaker stays OFF; origin/interval/privacy/physical and voice-freeze holds remain. No code, device, source/cache/index/ref change or runtime test was performed by this audit. Detailed source evidence remains privately checkpointed.
