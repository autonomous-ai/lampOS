# Common carry onto train 50 and its docs update

Common #3064 is one commit `4d634474e`, parent `be16c3327`, authored tree
`137baf4f76e4caf0af5bb436c4e102943b4b0721`. The exact-lease push succeeded.
The [range-diff](range-diff.txt) is equal to prior Common `00c790722`.
All 35 Common file contents and modes are unchanged.

The host gate runs on frozen train-50 tree `624aff2b9`, base `09f8e7412`.
The [source parity receipt](source.json) proves that the final published
composition differs only by the merged six-line `docs/build-rows.md` table
edit. All 10,065 final tracked file contents/modes match the final index.
No reference to that table was found in crates, tools or workflows. Diff
check and upstream-reviewed table inspection pass. This proof is separate
from final-head native CI.

At publication, Linux formatting, strict workspace/all-target Clippy and
workspace build pass. Full Update passes 374/0/2, with all 41 required named
controls passing once; privacy passes 9/0/0. Source/index/mode/mtime guards
and container cleanup pass. Mac formatting, strict Clippy and build pass;
the original full Update run remains active and is not counted as passed.
A slow case subsequently advanced; a bounded owned-process sample observed
real durable installation writes, not a fixed lock or nested build. No
cancellation, retry, fixture change or failure waiver was applied. All 28
Mac head-touch cases have now passed within that continuing package run.

Fresh ARM64 and Clippy are required on the published head. Current reviewer
source/settings dispositions, compatible pins and First's physical acceptance
remain holds. New dependent Photo/Name publication is parked under First's
no-stacked-PR direction until Common lands or routing is decided. No device
or score claim is made.

## Completed host result

Both gates are now terminal 0. Mac Update is 372/0/4 and Linux 374/0/2;
privacy is 9/0/0 on each host. All 41 selected integration identities passed
once within each full Update run. Every source/index/mode/mtime guard and
process/container cleanup check passed, and both targets are released.
The [host receipt](host-receipt.json) records commands, per-target counts and
identities; [readable receipt](host-receipt.md) states the source boundary.
Mac's two additional ignores are Linux-only standalone process controls.

Fresh Clippy 39978630861 passed on merge `efd5080ba`, exactly equal to
published tree `137baf4f7`. ARM64 39978630852 is still running at this entry.
