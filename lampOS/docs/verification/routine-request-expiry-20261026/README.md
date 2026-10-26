# Controlled Routine transport expiry

Initial compiler base: e672ee214. Actual controlled runtime: 1adcbe08c on 7d0017f08. This is an isolated test-only follow-up to the unpublished probe claimed on #3545, and to the retained #3525 538-pass/4-fail/9-ignored host qualification. It does not replace #3545's addon commits.

The paired case uses fresh Manager fixtures, one subscriber each, and the same 700 ms monotonic host stall. The publisher keeps its default 500 ms Event transport lifespan in the first arm; only the second arm overrides future transport headers to 5 s. Synthetic Manager time, request payload deadlines, production declaration, and the ordinary 50 ms receive bound remain fixed. A queued datagram is observed without consuming it, so the same subscriber later supplies expiry counters and delivery. No additional receiver races the primary observation.

The expected distinction is an expired default event versus a delivered identical positive-control payload. Publisher publication/delivery/drop counters and subscriber expiry/malformed/type counters identify the mechanism. Wall-clock age is checked because transport expiry uses Unix time plus 50 ms grace; clock changes or positive-control overrun must not be reported as a passing mechanism result.

Existing three clock-policy fixtures gain scenario/iteration/stage/expected-operation diagnostics only. Their transport limits and logical assertions are preserved in this initial probe. Diagnostics are evaluated on failure or emitted after both paired observations, outside the controlled path.

Fresh dedicated target: third-routine-expiry-target, bound permanently to physical third-routine-expiry worktree. Compiler messages, source hashes, saved actual executables, bounded process terminal status, and results will be retained. No ARM64 linking on the Mac and no device operation.

Controlled expiry can establish a mechanism for recv(None). It cannot retrospectively identify the phase or cause of the historical failures, whose original logs lacked those observations. Historical outcomes remain retained separately.

Observed result: the exact pair passed 1/0 at 1adcbe08c/7d0017f08. Both publications were delivered; the 500 ms event was discarded with expired 0→1, while the 5 s event delivered its exact queued payload with expired unchanged. Both Manager times and deadlines were preserved. Five unexecuted saved drivers get no runtime credit. This is host evidence, not an old-failure causal clearance or device result.

Provenance correction: the first packet used the older scratch folder’s 2d18 label as the base. Direct Git parent inspection proves the actual runtime parent is 7d0017f08; the complete lampOS subtrees at both bases are identical. Outcome, source head and artifact hashes are unchanged.
