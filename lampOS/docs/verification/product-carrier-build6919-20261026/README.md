# Build 6919 product-carrier rehearsal preparation

The first product lamp installed from build 6919 is the rehearsal target for
#1711 before the remaining nine lamps proceed. The lamp alias, its owner or
authorized borrower, the attended slot and required access approval must be
recorded before the round. A build number does not establish a lamp lease.
Lamp-4ace remains excluded under the recorded acceptance scope.

Record the signed install receipt for 6919 and independently verify the
installed updater level is 5. Do not install the candidate updater first:
GOOD must exercise the actual 5-to-6 self-update and confirmation path.
The Voice payload and sidecars are copied from that actual installed lamp,
with their original hashes and target retained, rather than taken from the
candidate's build. Capture the real effective units and updater environment
before constructing either trial asset.

Bind the candidate updater to the green native ARM64 run for the final
current PR head and verify its artifact SHA-256. Any earlier prepared trial
assets remain historical and must not be relabelled as this current carry.
Choose both isolated test sequences only after reading the installed record:
GOOD is greater than the original installed sequence, and BAD greater than
GOOD. The public build number alone does not supply those record values.

The existing settings-free GOOD/BAD scope, backup, failure acceptance and
complete teardown remain mandatory. The first ten-lamp release remains held
until the owner supplies actual GOOD confirmation, automatic BAD fallback,
and verified restoration evidence; local and native CI are preparation only.

The three unit files here are **provisional public-source templates**. They
have not been signed, uploaded or installed. GOOD adds only a comment to
build 6919's public Voice unit; BAD adds one invalid option to its single
ExecStart. Their hashes and the missing concrete inputs are in
`preparation.json`. The owner must compare the real installed unit before
these templates become trial assets. No production record sequence, key,
installed binary identity or completed acceptance is inferred from them.

The complete [acceptance run sheet](https://github.com/autonomous-ai/lampOS/blob/2842f5d7c/lampOS/docs/verification/product-carrier-20261025/README.md)
retains the backup, isolated trust, exact two-binary/one-unit scope, automatic
recovery requirements and full restoration. Current carry 6b727c3f5 on
3d9722f38 has identical reviewed runtime/test/unit blobs. Its sole rebase
conflict preserved both inherited reuse-decision sections and the authored
carrier section. Current local gates are active; current native artifacts
remain pending. No device action has run.
