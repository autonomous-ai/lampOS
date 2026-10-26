# Sentinel 57 original native qualification

PR #3543 head `57ecd800a8353066e6e874808ac0455280797943` on pinned
`2bf5122dc76999f970556bb7c78840ab6e107ce0` passed original attempt-1
[ARM64 run 40396415507](https://github.com/autonomous-ai/lampOS/actions/runs/40396415507):
**7,537 unique PASS, 0 FAIL, 0 LEAK, 0 TIMEOUT; 53 aggregate skips**.
The actual `pr` selection was twelve packages, one partition (1/1), with
70 binaries reported at Starting and 62 binary labels in executed PASS identities.
Another 114 binaries were skipped. This is the selected ordinary runtime scope;
workspace compilation is broader.

The actual checkout was preview `2ce6a525372efa13d016810a9b1214123960a64f`,
with ordered parents `[2bf5122dc, 57ecd800a]`. Its tree
`cfe3c82e1a97c596f0fc6e8c04991fcba93924de` equals the offered head's tree.
Original checkout logs, immutable commit metadata and workflow/config/Reader
source blobs agree. This establishes tracked source, without binary equivalence
or check carry to a different source.

The Starting-to-Summary execution window has exactly 7,537 distinct identities
and ordinals. All **fourteen ordinary Linux Reader cases**, including the changed
`session_reader::tests::linux::replaced_record_never_changes_an_outside_sentinel`,
pass once in that window. The **34** later PASS records occur after Summary,
before the doctest step: 22 match realtime and 12 match privacy-worker
`success-output = "final"` overrides. Their original identity, status, duration
and ordinal match; they are preserved as final-output reprints. No retry/flaky
record occurs. This accounting uses execution boundaries, not blind deduplication.

The privileged dropped-identity DAC test remains source-declared ignored/unrun;
it requires isolated Linux root and `setpriv`. The non-Linux fallback is outside
this Linux build. Logs give aggregate skipped counts without individual skipped
identities, so no skipped-name inventory is invented. Separate promoter **25/0/0**,
workspace doctests **4/0/0**, relay-signals `ok` and process harness **13/0**
are not added to nextest totals.

Original attempt-1 [Linux Clippy 40396415483](https://github.com/autonomous-ai/lampOS/actions/runs/40396415483)
passes pinned workspace formatting and strict whole-workspace/all-targets Clippy,
plus promotion formatting/Clippy, on the same preview. Exact-head check records
show two successful checks and three conditional publication/report checks skipped.
No separate general-CI run or classic status context is present in this collection;
branch-protection requirements are not asserted from those records.

Artifact **12687645905** is **116,064,290 bytes**, with API/upload-log ZIP digest
`sha256:315acabb21c542bdebd142b719bfa8bce35d1d57015bc32ecfb562f3da7d8cd8`.
The exporter reports 47 current workspace binaries, two selected examples and
complete SHA256SUMS. Metadata was collected; no ARM64 archive or binary was
downloaded, rehashed or executed locally.

Compared with the [older original 7,535-case run](../tests-original-native-three-prs-20261027/),
all executed identities remain and two upstream Skills ACK/unwind tests are added;
[audit.json](audit.json) names them. Reader identities are unchanged. Both logs
report 53 aggregate skips, but skipped-set equality cannot be established from
the emitted data. The [current Mac 4/0/0](../tests-sentinel-2bf-20261027/) keeps its
separate platform scope. Earlier failures remain retained, without a cause finding
or waiver; privileged DAC, device acceptance and independent read remain separate.
All raw metadata, jobs, logs, execution/reprint records and receipt hashes are
preserved privately. No CI rerun, dispatch, write, target access or source mutation
was performed for this audit.
