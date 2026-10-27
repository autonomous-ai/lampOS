# Actual-applier receipt recovery controls

Fresh Mac qualification on base 1fb93206e4a6c26d35c801b3472a5af179231881: committed receipt head 1863f6d58c1d75f128469516c81c9e20d3c41284 plus two new tests and their test helpers. Production code is unchanged. Documentation records these outcomes only after the gate closes.

- fmt and strict Update all-target Clippy: pass.
- Full Update library: 155 passed, 0 failed, 2 ignored.
- The previous 65 exact round selectors plus both new controls: 67 passed, 0 failed, 0 ignored, 134 filtered. Both new functions pass once as subsets of this total.

Each control wraps the existing fixture applier, which calls actual Rust apply/back and writes real private-fixture files and back.json. Witnesses include overwritten settings with modes 0600/0640, a removed setting and an absent-before nested setting.

The first corrupts the actual ApplyDone sequence after real writes; the parent rejects it and actual Back restores bytes, modes and setting records. The second uses a genuinely unwell candidate to fail the watch, then corrupts the actual BackDone operation after file restoration. It requires an error outcome with the journal retained, the old installed sequence and no premature restored-record acknowledgment/refusal. The next actual Back settles records idempotently, clears the journal and refuses the build; another round performs no Apply.

Actual compiler JSON binds both executed and preserved drivers. Eight source/manifest/toolchain inputs match before/after execution. Original e362 expected-failure drivers and prior fixed drivers still match their retained hashes; no old experiment was rerun. Only tests/round.rs and the added actual-count documentation differ from committed 1863. This gate is not a full workspace/full round, Linux/ARM64, CI, root-one-shot, reboot/power or lamp trial, and does not implement helper authentication or the frozen guard. No commit or push was performed for these additions.
