# Boot-clock carry onto the current base

Ready for lamp-voice: no. PR #3190, part of #1569.

Current base: `70cbefceee62c0bc4cad4ec0b7054335dd8f0a5d`.
Carried head: `172389869129a7466623cc9b45d1c536f42f7429`.
The original frozen execution remains `f477d6c49` → `da595851f`.

The seven source files keep the original correction. The insertion conflict
at the start of the second-worker test module was resolved by keeping both
new upstream tests and inserting the clock helper before them. The source
range-diff changes only surrounding context. The added fixture documentation
explains the clock and its limits. All 1,827 assertion macros in the seven
current-base files are byte-identical; this lexical source comparison is
not a current-base test execution.

The original full Mac run's five additional candidate library failures remain
unwaived. Current-base ARM64 run 39891902416 and x86 Linux Clippy run
39891902443 were in progress when this receipt was written. Neither an ARM
build on the Mac nor a lamp run was performed. Full run results and current-
base local qualification will be reported separately.

`source.patch.txt` and `range-diff.txt` are exact Git outputs, not rewritten
patches. `audit.json` gives the per-file assertion and content digests.
