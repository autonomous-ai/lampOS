# Private name and hits through the admitted request and HTTP

Test-only follow-up to #3064, based on its authored Common head `e9d00f935029a6d5054ddf35b0203fb8546f7995`. Two existing Voice test files add one top-level regression containing two internal cases. It uses actual Engine Start/Audio/End admission and current-piece speaker sealing, real known-name and memory-hit callbacks, the existing Unix Memory fixture and real Net guarded HTTP serialization. STT is deterministic; ordinary note/history callbacks are deliberately omitted so the kept canary has only the hit path.

The silent-visible-person case retains inherited personal scope but has no positive speaker: it must perform zero kept RPCs, complete a neutral HTTP request and send neither private canary. The proved-person case uses three distinct positive frames, one matching-person kept RPC and both canaries in the serialized messages. No manual speaker seal or permission Boolean substitutes for admission. A Done-only response avoids TTS/tools. All HTTP and Engine waits share the existing 10 second case bound, with bounded headers/body, retained-task abort cleanup and owned config removal. Existing thresholds/timeouts remain unchanged.

## Full local qualification

Frozen tested tree `be860815986f43c8a1b37a8ac50a210216977039` passes all four required stages on both hosts: whole-workspace formatting, strict workspace/all-target Clippy, workspace build and full Voice serial pretty tests.

| Host | Voice passed | Failed | Ignored |
| --- | --- | --- | --- |
| Mac | 4,126 | 0 | 21 |
| Linux AMD64 | 4,187 | 0 | 24 |

All five required Voice controls pass once per library target: four retained private-proof/frame controls plus this new combined fixture. Each host validates and invalidates exactly 46 local workspace packages and proves post-clean fresh compilation for all 46 across Clippy/build. Source/index stability, process/container cleanup and target release are confirmed. Raw receipts: `/private/tmp/fourth-combined-private-callback-gates-20261024/{mac,linux}/attempt1/`.

The final commit adds only this result README after those terminals; both tested Rust files and all other blobs/modes remain unchanged. Old-policy function mutants are separate controlled experiments still in progress at publication; no expected failure or causal coverage is claimed here. The full parent Common gates and current native 11,371-pass result belong to e9d, not to this new regression.

This covers real callback/request/proof composition into loopback HTTP under explicit flags. It does not exercise complete worker::serve/product-unit parsing, acoustic STT/Eyes, model decisions, identity/liveness, physical latency or activation. Fixture timing instrumentation is not a reported acoustic or performance result. Native ARM64/Clippy and reader acceptance remain required for this new head. The parent Common dependency and voice-freeze/privacy/pin/physical holds remain; no lamp was touched.
