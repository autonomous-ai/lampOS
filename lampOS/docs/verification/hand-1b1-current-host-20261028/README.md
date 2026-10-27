# Parent #3544: current-source host verification, 28 October 2026

Commit `1b1da9ae3fc6fbf1101cd1b88f10bcaa37fb55ad` (tree `5c1f55fda333a87b71de05b1e3faa859a2e10642`), based on `82a62867cc1bcbbc4b0848519c6757f623d112ce`,
passed the four normal compiler stages and **4,177 unique host cases**:
**four exact hand-watchdog controls** followed by a **disjoint 4,173-case
nextest remainder**. All **22 host-required controls** passed once.

Scope is the complete Voice **library** (3,887 cases), Update library (180),
Update install (108), and its two CI-path readers. Child #3577 / whole
Voice-it is separate. Earlier 4,163-case results supply no credit here.
Workspace docs are separate: **4 PASS / 0 FAIL / 0 ignored**.

Each direct control had a 30-second watchdog and two-second TERM grace.
Its start/summary references and recorded whole-session times are in
[host22-ledger.json](host22-ledger.json); these are host test boundaries,
not measured lamp touch-to-feedback latency. The remainder recorded no
failure, LEAK, timeout, retry, or repeated selected case. One required Linux
transport control is unrun on this Mac and receives no host credit.

The original authored patch compares `=`; all three owned blobs are exact
`4084e4e1dde24ea17d6c88d944256f78904579c0`. [source-hashes.json](source-hashes.json)
contains the three source hashes and the saved normal Voice/Update and four
test-driver bindings. They were copied from the recorded Parent input
manifest; the current target was neither read nor rehashed for this proof.
Inputs were guarded before and after the direct and nextest stages; later
Cargo docs and builds have their own artifact boundary.

Native PR checks and review remain separate gates. See
[receipt.json](receipt.json) for the source-scoped counts and boundaries.
