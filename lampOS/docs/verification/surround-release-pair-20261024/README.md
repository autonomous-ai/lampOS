# Surround candidate/journal qualification, 24 October 2026

Authored head `806e4006c0af067ef8d66854ab4d45d86d74f984`, draft #3361,
base `9872cb692`. This is candidate-only software qualification for #2809.
No runtime/default/consumer changes. Actual selected 6408/6498 Surround
artifacts and source trees are identical; the fixture pins both downloads.

Host gate: fmt and strict Surround Clippy pass; scoped crate tests
**268 passed, 0 failed, 2 ignored**. All 7,016 captured lampOS source hashes
are unchanged and bound to committed head. Offline source-only Linux fmt
passes on the same 7,016 files; owned container removed. No released ARM
executable, local ARM build or local ARM test was run.

`host/current-cli` is the ordinary synthetic CLI smoke, not released-artifact
proof: seven reaps/group-absent receipts, three candidate writes, existing
superseded generation collection, three request refusals, two saved-state
startup refusals and explicit fixture restoration/recovery. No selected or
applied configuration and no provider/model request. HTTP creates operation
IDs internally; no client compare-and-set/retry-ID API is claimed.

Published host logs replace private home prefixes with `<private-home>`;
`host/export.json` records original and published hashes and whether that
redaction occurred. Those redacted logs are not byte-identical originals.
Executable/configuration/credential payloads and archives are excluded;
all exported files must decode as UTF-8 without NUL bytes.

Official isolated ARM64 pair run [40083244916](https://github.com/autonomous-ai/lampOS/actions/runs/40083244916)
passed **2/0/0 ignored/3 filtered**. [Raw safe receipts and audit](native-40083244916/)
bind authored head to tested merge `9d655727e`, base `7e82733e0`, all
10,375 tracked path hashes unchanged (one inherited symlink follows its
matching Git target), two pinned published payloads, 14 matching reaps and
absent groups, 50 completed HTTP exchanges, six generation writes and strict
refusal/recovery controls. Fresh container isolation and owned removal pass.
API ZIP artifact 12556405804 is 921,631 bytes with verified SHA-256
`0d8b7507441bb40467be2077c29f3adc819a34c701810b5d48987389b2a27747`;
its archive is byte-identical to the gh download. No archive is published here.
Strict Linux Clippy run 40083244738 passes; ordinary ARM64 and independent
reader remain pending. Passing software controls will not
establish configuration activation, provider connectivity, physical privacy,
filesystem power-loss durability, updater handoff or lamp acceptance.
