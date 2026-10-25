# Onboarding: exact native build for the pending trial

PR #3472 head `74dafff29ee70cd30f66b91802627c1379d09d29` has successful original
[ARM64 run 40229684238](https://github.com/autonomous-ai/lampOS/actions/runs/40229684238)
and [Linux Clippy run 40229684153](https://github.com/autonomous-ai/lampOS/actions/runs/40229684153).
Both checked out merge preview `b37ad00673163288fa92c34cf51a341fb58faff3`; its tree
`250b88ad5af2d19a6fbf5dee887a26e56679727e` equals the published head's tree.
The local tested tree differs only in the verification README.

ARM64 completed 11,308 unique test passes, zero failures/leaks/timeouts and
106 summary-reported skipped cases. Thirty-five repeated successful display
rows are not additional tests. Included: onboarding 125, same-voice 21,
installation 92 and workflow readers two. Workspace doctests completed four
tests across 46 targets; the separately invoked turn-end-wire harness passed
12 cases and the relay invocation reported success. Linux formatting and
strict workspace/all-target Clippy passed.

## Candidate artifacts

[The original artifact](https://github.com/autonomous-ai/lampOS/actions/runs/40229684238/artifacts/12613750226)
expires on 1 November 2026 at 20:28:05 UTC. It was not expired when read.
Its 116,061,977-byte archive digest matches the API's SHA-256. The manifest's
49 unique entries cover the archive inventory. Voice and Eyes were each
streamed and hashed separately; both match their manifest entry and have
ELF64 little-endian AArch64 headers. Only these two binaries were individually
hashed; no binary was extracted or executed. This is artifact/checksum and
checkout evidence, not a verified signed source attestation.

| Binary | SHA-256 | Bytes |
| --- | --- | ---: |
| lamp-voice | `0b26a477e9b6ff37b1bd4ce494bbb76f777b72d5cd82b0ddab77f6dfe98f6c5c` | 28,632,520 |
| lamp-eyes | `bf1654ecc2ff649c836e185fa859adebaddff9bf8637c115f602840024af9b4f` | 26,407,600 |

Machine-readable selected metadata is in [pins.json](pins.json). Raw logs and
the archive remain private; no credentials, signed download URL or device
reading is included here.

## Remaining trial

First assigns the lamp-owning builder. With two people present, normal light
and the usual seat, the intended person must enroll within two tries before
this activation ships on. Count every attempt, speaking-read miss, refusal
and intended-owner memory keep withheld. Include cancellation, a split
consent/name reply with one natural retry, low light/turned mouth, and a
Voice-on/Eyes-off refusal with a paired successful control. Record counts and
times only. An all-refusal round does not qualify. No lamp acceptance,
acoustic latency or identity/liveness accuracy is established by these runs.

The follow-up test change and any later source carry have no result credit
from this head. The PR remains draft under the physical readiness hold.
