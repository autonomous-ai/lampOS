# Current-base phone-fixture carry

Head `6cd08d29cbe2ffecb4d05f0e8bbcee4ff1c98586` is an equal two-commit carry
onto `22b753df5`, with original authorship retained. All five original test
assertions remain byte-exact, plus cancellation/task-exit assertions. The
whole messages.rs file is identical to the isolated fault-control source.

Mac fmt, strict Voice all-targets Clippy and full serial Voice pass:
**4,004 passed / 0 failed / 21 ignored**. The changed phone case passes once,
with all tracked inputs unchanged. The 26 invalidated names are distinguished
from the 25 observed path-package identities; all observed workspace artifacts
are fresh, and unchanged vendor caching is recorded separately. Exact-head
source-only Linux fmt passes with 7,023 unchanged regular files and owned
container removal. No full local workspace gate or local ARM execution.

Prior P3 controls retain both expected failures and cleanup evidence; no new
fault-control rerun is claimed. Historical Update 2/3 qualification under
6370483473 remains separate and held. Official current-head native checks and
new-head reads remain required; no failed observation is waived. No device,
release, signature-trust, acoustic or physical acceptance claim.
