# Enrollment epoch interleaving test gates

PR #3349 `6213596a3e31f118f20f3d11f9a9687eb2c1ce32`, stacked on #3344
without changing its cleared head. Test/docs only, no runtime/product change.

Focused **26/0**, full serial Voice **3,995/0/21 ignored** (library 3,714/0/15,
integration 270/0), formatting, strict all-target Voice Clippy and workspace
build pass. Source hashes stay unchanged and match the committed test blob.
No actual-release tests or ARM64 execution occurred on the Mac.

The new cases retain a real Engine proof across either forget or camera
privacy close/reopen, with its original lease still current. Both cache and
guarded transport refuse the retained stale proof. No asynchronous filesystem,
physical switch/camera, concurrent race or measured latency is claimed.
Official ARM64/Clippy dispatches run on this branch while the PR is stacked;
final lamp-voice retarget/rebase and gates follow the parent integration.

The earlier sandbox diagnostic has 12 passes/14 failures (socket permission,
then poisoned locks). The source was edited during that preliminary build,
so it has no stable source-snapshot qualification. The final unchanged-source
socket-enabled run above is the qualified result; no runtime cause inferred.

## Landed qualification

Original authored-head native dispatch 40077846199 passes **11,197 PASS plus
one LEAK, 90 skipped**, process 12/0; both epoch controls PASS. Linux Clippy
40077847930 passes. A local watcher network read timeout was resumed on the
same run; it was not a CI failure or a CI rerun.

After parent train 77 landed, local equal-patch rebase 781b607bc on b2c9eb3dd
passes fmt, strict Clippy, focused 26/0, full serial Voice **3,999/0/21** and
workspace build with the same test-source digest. That rebase was not pushed:
the train bot already carried the reviewed original patch in train 79 #3352.
Git safely dropped the duplicate local commit when updating to lamp-voice.
Only the unrelated Memory test/verification files differ between that local
gated tree and the landed tree; every other tracked blob is identical.

Landed **ed0056b45** has equal range-diff and identical test/docs blobs to
6213596a3. Train native 40080079345 passes **7,357 PASS plus one LEAK,
51 skipped**, process 12/0, both new epoch controls PASS; Linux Clippy
40080079358 passes. Tested preview 2ed1740ff has the exact landed tree.
These current integration results are separate from the older direct-head
manual run. Second and reviewer-two READ-OKs are on the PR. Scope remains
ordered helper interleaving; physical/asynchronous-filesystem results stay
unverified. No activation occurred.
