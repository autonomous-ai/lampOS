# Common current-head native receipt

This records #3064 at `00c790722` on `810e05109`. Both native jobs ran
merge `e87f358f7`, whose tree equals the authored tree exactly.

[ARM64 run](https://github.com/autonomous-ai/lampOS/actions/runs/39957390695):
11,177 passed (including one leaky pass), zero failed, 69 skipped, zero
selected tests unrun. Four doctests and twelve process checks passed
separately. All 80 listed core/integration controls explicitly passed.
[Clippy](https://github.com/autonomous-ai/lampOS/actions/runs/39957390652)
passed for the workspace and all targets with warnings denied.

`receipt.json` records the actual source identities, terminal counts, each
required control's source hash/result, and all 49 verified AArch64 binary
checksums. Artifact 12504626314 was downloaded with a bounded reader;
the ZIP digest matched GitHub metadata, each checksum matched its bytes,
and each binary had the expected 64-bit little-endian AArch64 ELF header.
The binary archive is not committed. The verification runner and full logs
remain in `/private/tmp/fourth-common-native-00c790722-20261023/`.

This is not installation, physical consent, liveness or latency acceptance.
Reviewer holds and First's compatible pins/lamp qualification remain
separate. Earlier native/host failures remain recorded, including the
[saved Skills observer failure](../common-skills-observer-20261023/README.md).
