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
