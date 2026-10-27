# Restart: current host qualification across two controllers

Head `a8a55c00fa359b754b2514a58b5c92c3dda00863`, based on
`9e0ccb206ee796ef46929bf463c5176f3a493ed9`, has a completed Mac host
**COMPOSITE_PASS**. Five Cargo stages completed in the earlier controller:
fmt, metadata, strict all-targets for the actual 12-package reverse closure,
workspace build and Voice-library test compilation. The separate compile-free
continuation then completed four inventories and the first full SameVoice module
observation: **21 passed, 0 failed, 0 ignored**, serial, unchanged 120s watchdog.
This is evidence across two controllers, not one whole-gate PASS or a full Voice
runtime result. No test body was retried.

The complete Voice-library inventory contains 3,832 unique names, 15 declared
ignores; the selected 21 cases contain no ignores and 3,811 names were filtered out.
The fresh complete all-list finished in 0.141159s under a diagnostic 300s watchdog;
module runtime finished in 5.028682s. These are process-observation durations,
not performance comparisons. All 21 case identities/outcomes are in `audit.json`.

Both earlier controller failures remain unchanged. The first stopped before
launching any Cargo command or test body during admission. The next completed
all five Cargo stages but its first all-list emitted 3,238 complete names and
exceeded 60s; its process was terminated with -15 and the owned group closed. Its
native/controller exit remains 1/FAIL, with zero runtime attempts. The new full
list has the same 3,238-name prefix. Why the earlier listing stalled is unknown;
the administrative retry does not turn that controller into PASS.

The compiled Voice test module was observed `fresh:false`, SHA
`7d07b7bcf060a9d754025d33b26c176a7ae571a9c55b58d71a7e4c5d666d7697`,
116,069,816 bytes. Its actual profiles/features and receipt bindings are recorded.
All 10,679 tracked source files and the official owner remained unchanged, as did
244 sealed witnesses and the actual module. Full source/owner/sealed hashes were
checked initially/finally; intermediate metadata and adjacent consumed-executable
checks are stated precisely in the audit. All owned groups closed and disk reserve
held. The continuation's actual native/controller exit is 0/COMPOSITE_PASS.
There is no detached-descendant, filesystem write-protection or idle-host claim.

A separate original Linux ARM64 witness is retained: PR #3644, run 40439732017,
job 121976926287, actual checkout `ac5bd8f6fe70eb181152a8c65639b23420f754b1`
(merge of `553e0d653eb9557a7b804e8fb6c469c102159808` into
`5982e4b7fa465177f55697ddb362d78b964809b7`). Its 11,459 unique observations
include **11,458 PASS and 1 LEAK**, 106 summary skips and zero selected-unrun cases.
The LEAK is `same_voice::tests::a_child_that_hangs_after_it_is_ready_is_killed_and_started_again`,
ordinal 9,202/11,459. That same case identity passed in this Mac module. The original
ARM LEAK is not waived; its source/platform differ, and its cause remains unknown.
This packet makes no current-head ARM64, full Voice, device or unrelated-patch
qualification claim. Raw logs stay private; immutable receipt/log hashes are in
the audit. No ARM binary was downloaded or run for this proof.
