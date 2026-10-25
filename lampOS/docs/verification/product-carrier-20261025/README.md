# Product carrier acceptance — pending owner execution

Required by PR #3466 comment 6397116959. No device round is claimed here.
Tests owns no lamp. A product-lamp owner or authorized borrower must claim
an attended slot, obtain the lamp's required access approval and hold its
lease. Lamp-4ace is excluded.

## Inputs and scope

- Confirm a product install made by `lamp-update install`, an updater at
  level 5, independent executable recovery with its timer armed, and Voice
  already active and listening. Reject a pending update or unknown install.
- Record the installed sequence, original channel, updater/Voice current
  targets, their hashes and modes, and effective units/drop-ins. Voice's
  base unit must have one unambiguous current-linked `ExecStart`; verify
  that its bytes are safe to publish and preserve every existing argument.
- Use a passing native Linux ARM64 artifact bound to the current PR head.
  Verify its recorded SHA-256. A Mac binary or stale artifact is excluded.
- The owner supplies the exact current Voice binary and sidecars. Only a
  byte-for-byte copy of that binary is declared: this is a unit-only trial.
- Use a dedicated non-production HTTPS test channel and two immutable asset
  prefixes. Never change the production release/head or use the production
  publication script. Choose GOOD above the original installed sequence,
  then BAD above GOOD. Record the concrete values before execution.

## Backup and isolation

With updater/helper activity stopped and no pending transition, take a
coherent, verified private backup under the existing updater lock. Release
that lock before starting the trial. Preserve units/drop-ins, enablement
and timer states, all original current links and referenced build bytes
outside any prunable build folder, complete updater records (including
sequences/refusals/absence), original trust/channel configuration and modes.
Do not expose credentials, conversations or full private records.

Generate a disposable Ed25519 key in the private publisher workspace; the
existing CLI accepts base64 PKCS#8 DER. `lamp-update public-key --key-file
PRIVATE` emits its public key. For this settings-free trial, prefer an
owner-reviewed runtime-only updater override using `once`, the test channel
and a temporary public-key path through the existing `--key-file` option.
Keep the product base unit and sandbox unchanged, `LAMP_UPDATE_SELF=on`,
normal quiet/watch limits and the fallback timer armed during the climb.
Preserve the original production key. Review the effective command before
starting; abort if the owner cannot establish isolated test trust/options.
The successful `once` service becoming inactive is expected.

## Signed builds

Each manifest has schema 1, min_updater 6, exactly two binary entries
(candidate ARM64 updater and the unchanged installed Voice), and one unit:
`product/lamp-voice.service`, asset `product.lamp-voice.service`. No settings,
reset plan, models, image or skills changes are declared. Existing Voice
sidecars and its current target remain unchanged.

Use the existing native publisher CLI with explicit `--settings off`:

```sh
"$PUBLISHER" manifest --binaries "$STAGE" --deploy "$DEPLOY" --skills "$SOURCE/lampOS/skills" --channel "$TEST_CHANNEL" --sequence "$SEQ" --commit "$PR_HEAD" --files-url "$FILES_URL" --min-updater 6 --settings off
"$PUBLISHER" sign --key-file "$PRIVATE_KEY" --manifest "$STAGE/manifest.json" --out "$STAGE/lampos-manifest.json"
```

Between those commands, project the unsigned manifest to the exact scope
above: the generator adds `lamp-product-skills`, which must be removed
before signing and not uploaded. Verify every declared hash/size and all
identity/sequence/URL fields. Upload only declared assets, their checksums
and the envelope. Re-download and verify before the lamp follows it.
Never modify signed bytes or overwrite an immutable GOOD/BAD prefix.

## GOOD and BAD acceptance

GOOD appends only a harmless comment to the verified current Voice base
unit. Capture a journal cursor, then start the controlled updater. Require
journal evidence of 5→6 climb and confirmation, unit installation, automatic
Voice restart and successful normal health watch. Require the exact new
unit bytes, unchanged Voice current target, and active/listening Voice.
Do not manually restart Voice to make the check pass.

BAD starts from GOOD and appends `--product-carrier-test-invalid` to its
single `ExecStart`. Require an actual unknown-option startup failure, then
automatic health fallback restoring the exact GOOD unit and Voice link,
with Voice active/listening again. Capture the failed sequence refusal.
Manual restoration is recovery, not acceptance. Failed automatic rollback
means HOLD; preserve the first evidence and stop rather than retry to green.

## Teardown and evidence

Stop trial writers/recovery activity and save redacted test evidence before
restoring the coherent original snapshot. Restore original units, current
links/builds, complete production records, sequence/refusal state, trust,
channel and original service/timer states. Remove only the identified trial
override/key files. Verify the original hashes and healthy/listening Voice.
The updater must again use `lamp-voice` with the original URL ending in
`/lampos-lamp-voice/`. Test sequences must not remain in production records.
Retaining updater 6 is a separate production migration, not this teardown.

Remove disposable signing/trust material after verified restoration. Retain
redacted GOOD/BAD journal lines, hashes and teardown attestations; exclude
keys, private unit values, complete records and personal/device identities.
This check covers product units and rollback. It does not establish default
settings/reset-plan application or actual old fixed-helper settings apply.
