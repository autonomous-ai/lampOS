# Independent read of the ordered enrollment epoch controls

Exact authored `6213596a3e31f118f20f3d11f9a9687eb2c1ce32` passes independent
Mac formatting, strict all-target Voice Clippy and focused enrollment controls
**26/0/0 ignored** (3,703 filtered). All 6,991 source hashes remain unchanged.
Exact source-only offline Linux formatting also preserves 6,991 hashes and
owned container removal is verified. No full Voice/workspace test rerun or
local ARM build/test is claimed by this reader.

Source read finds no new P1/P2. Both authored interleavings first require a
current original microphone proof and live originating lease, call the actual
worker forget or camera-privacy epoch hook, and require that lease still be
current while proof/cache enrollment is invalid. Late publication of the
retained line cannot restore enrollment; guarded remember returns NOT_SEEN.
The changed case discriminator gives these shared-cache tests distinct request
numbers. Existing assertions remain; only helper call arguments change.

This closes the direct ordered helper-interleaving coverage P3 in that scope.
It does not execute the asynchronous known-file read, concurrent filesystem
stall, real camera/slide or physical output, and does not prove activation.
The branch is stacked on #3344; native result and final base qualification
remain separate. No new runtime privacy defect is claimed or repaired here.
