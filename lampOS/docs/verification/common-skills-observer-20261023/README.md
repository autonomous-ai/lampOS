# Common Linux Skills observer failure

This preserves the failed run, not a retry or a cause finding. `failure.json`
binds the exact source, command, counts, missing completion and complete
synthetic child-journal segment printed by the failed assertion.

The observer received three initial Done events; the child journal printed
four. The assertion failed before any judge-drop decisions or fifth proposal.
The fixture drains proposals before completions over a bus whose sends may
be dropped. Relevant audited source matches the base, but this run recorded
no drop count, queue statistics or sequence evidence proving saturation.
A journal line is written before the publish attempt, so it is not proof
of delivery. The original eight-second budget and judge assertions have
not changed. No diagnostic or rerun was performed for this receipt.

The complete local gate also passed the changed privacy controls, Eyes,
formatting, strict Clippy and workspace build. Those passes do not relabel
Skills as green. Native ARM64 remains the full-suite publication gate.
