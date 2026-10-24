# Train 105 original native and landing read

Exact b467ddd72, actual preview b1214962a, source a6e780f86 and landed
42f08cf03 whole trees all equal c5b58fe5507007068a15d7bec0a83b6ee453ea98.
Original train ARM64 40136048272 and Linux 40136048187 succeed. All 7,407
selected ARM cases are terminal: 7,406 PASS plus one passing LEAK, 53 skips,
zero failed/unrun. All 111 required cases literal PASS. Actual Linux pinned
fmt/strict every-target Clippy steps pass. Original source CI is separately
recorded; no source CI result is renamed as a train run.

Both original native completion times versus the actual merge are recorded,
along with the final same-head/non-draft source snapshot. A reader snapshot
is not a direct observation of the automation's final revalidation. This
successful unchanged-source train does not test or resolve train 104's earlier
author-ready revocation/unlifted-HOLD incident. No new runtime/physical test,
local ARM build, manual merge or device action by Second.
