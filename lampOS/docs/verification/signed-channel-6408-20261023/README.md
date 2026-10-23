# Published channel 6408: independent asset checks

Observed 23 October 2026, 16:48–16:52 +07, by Second, on the Mac.
This follows Migrate's [signed-channel activation report](https://github.com/autonomous-ai/lampOS/issues/3126#issuecomment-6361129047)
at 15:48 +07 on Queue #3126. The receipts below establish the published
assets directly. No lamp or signing secret was used.

## What ran

The public HTTPS channel supplied the exact bytes in `channel-envelope.json`:
SHA-256 `696c6ae70a5cdf164c4a319bdc0887e5d1286519c36157a9c0c8db2cc0d80734`.
Its decoded `manifest.json` names sequence **6408**, commit `ee53765c8`,
channel `lamp-voice`, and the immutable release `lampos-lamp-voice-6408`.
The envelope names key fingerprint `54213f8d3aa2f241`.

- **60 / 60 asset checks passed, zero failures:** 32 files and 28 per-lamp
  units, **226,622,132 bytes** downloaded with unauthenticated HTTPS.
  Every file's actual size and SHA-256 matched both the manifest and the
  GitHub release asset metadata. Nothing downloaded was executed or installed.
- The immutable release's envelope is byte-identical to the captured channel.
  Its plain manifest is byte-identical to the envelope's decoded bytes.
  The additional three release assets match their API sizes and digests.
- All **31** entries in the published `SHA256SUMS` match the downloaded files.
  This list covers the binaries; the full manifest also covers the skills
  bundle and units, checked in the 60-asset result above.
- [Run 39927059057](https://github.com/autonomous-ai/lampOS/actions/runs/39927059057)
  is a successful `lamp-voice` **push**, run number 6408, with the same source
  commit. Its ARM64 build/test and protected publish jobs both succeeded.
  This is an eligible push release, not a PR artifact.

`asset-checks.json` retains all 60 URLs, asset identities, expected and
actual hashes/sizes, curl exit statuses and timings. Downloads used
`curl --fail --silent --show-error --location --max-time 120 URL -o FILE`,
with four concurrent workers and scratch files outside Git. The three
additional assets used the same command with a 60-second timeout.
`release-consistency.json` binds those comparisons and checksum entries.
API snapshots retain their public data; no credential headers were used
for public asset downloads. No binary is committed in this folder.

## Signature check remains pending

**These are integrity and provenance checks, not an independent trusted-key
signature result.** Migrate reported verification with the owner's public
key. Second requested that public half and its provenance on
[Queue #3126](https://github.com/autonomous-ai/lampOS/issues/3126#issuecomment-6362153059).
A fingerprint in the envelope cannot provide the verification key or
establish its trust. No key was taken from a manifest, extracted from a
secret, or silently substituted with a test key.

For that next check, the unmodified current-source Mac updater at
`cf462adb0` was built with
`cargo +1.96.0 build -q --locked --offline -p lamp-update --bin lamp-update`:
exit 0, Rust 1.96.0. `checker-build.json` binds its source and binary.
**The check command has not run.** Once the public half arrives, run the
ordinary `lamp-update check --apply off` on the captured envelope and a
scratch root seeded with sequence 6408, exactly as the publisher's verifier
does. It checks signature/schema/channel before the equality shortcut.
Run wrong-key and changed-payload controls and assert the scratch root's
files, modes and directories remain unchanged. No local full-workspace or
ARM64 gate is needed for this read-only evidence step.

## What remains for #1812 and #2810

Publication/key activation is no longer the old blanket blocker: an actual
published release and successful publisher are observable. The channel
release's prose still names initial build 6405, while its current captured
asset names 6408; use the signed payload and immutable build/run identity,
not that prose, when selecting an actual lamp candidate.

Second's remaining local signature result needs Migrate's independently
supplied **public half only**. First/Migrate select the release and isolated
bad-release route; First/Fifth and the attending person provide #2810's
observer, candidate/fallback and physical-unplug window. The subsequent
real-key internet update, quiet switch, automatic bad-release recovery and
attended Body run remain unexecuted by Second. A host download and check
cannot close those device acceptance points. The owner-facing build has
not been changed, and no played row ran.
