# Product carrier acceptance

Required by PR #3466 comment 6397116959. The owner-approved installed round
on lamp-52e6 passed: meaningful GOOD 900011, actual BAD 900012 rejection,
automatic exact-GOOD fallback and refusal, then verified restoration of
original build 6963, real key and `lampos-stable`. The
[installed evidence](https://github.com/autonomous-ai/lampOS/tree/976c891a9/lampOS/docs/verification/product-carrier-lamp-52e6-20261026/)
records the artifact identities, operational events, observation limits and
cleanup. One GOOD and one BAD ran without retries or a manual Voice restart
for acceptance. Normal quiet/watch and recovery bounds were unchanged.

Tests owns no lamp. This round used the owner's explicit password exception,
the verified host key and a held lease; no SSH key or sudoers entry was added.
The lamp was handed back, lease released and SSH closed. A later round still
requires its own attended slot, access approval and lease. Lamp-4ace is
excluded. The procedure below remains the acceptance contract; the earlier
comment-only round stays a separate historical HOLD.

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

GOOD inserts exactly `Environment=LAMP_CARRIER_TEST=1` inside the existing
`[Service]` section of the verified current Voice base unit. First verify
that this test variable is unused by the deployed program and absent from
the original unit and its effective environment. Preserve the complete
`ExecStart` line byte-for-byte, every existing argument and Environment line,
and all other unit bytes. Do not append the assignment after an unrelated
section. The owner reports 17 additional Environment lines compared with the old
provisional unit, not a total of 17. Preserve every actual installed line and
its exact private value, without reconstructing them from this sheet.

Before signing, assert `unit::same(before, GOOD) == false` using the updater's
actual comparator, require exactly the one intended Environment-line delta,
and compare the unchanged Voice binary, current target and sidecars by full
SHA-256. Stop before publication if any assertion fails. A comment-only unit
is a separate intended no-op control: `unit::same(before, comment_only)` must
be true; its normal applying round may record the sequence as up to date
without replacing unit bytes, restarting or watching Voice. It is not GOOD.

Capture the original installed sequence and a journal cursor, then start the
controlled updater. Keep the installed sequence unchanged through climb and
dry confirmation; require it to advance to GOOD only after real application.
Require
journal evidence of 5→6 climb and confirmation, unit installation, automatic
Voice restart and successful normal health watch. Require the exact new
unit bytes, unchanged Voice current target, and active/listening Voice.
Do not manually restart Voice to make the check pass.

BAD starts from GOOD and appends `--product-carrier-test-invalid` to its
single `ExecStart`. Require an actual unknown-option startup failure, then
automatic health fallback restoring the exact GOOD unit and Voice link,
with Voice active/listening again. Require installed.sequence to remain GOOD,
and capture the BAD sequence refusal.
Manual restoration is recovery, not acceptance. Failed automatic rollback
means HOLD; preserve the first evidence and stop rather than retry to green.

## Prior installed round and quiet-state HOLD

The owner-reported 6963 r2 round remains HOLD, not a successful carrier trial.
The restored installed record was 6963 / `41233c1b6`; updater level 5 was
`6d22838a…`, Voice was `7e17711d…`, and the original unit was `f646f086…`.
That round used the b635671c2 candidate updater `49286ba0…bc31` from ARM64
run 40273208241. Comment-only GOOD 900001 was `920fb514…`; BAD 900002 was
`0ac27055…`. These are the owner's abbreviated trial hashes, not sufficient
preflight checksums: obtain and verify the actual full SHA-256 values from
private source artifacts before any new trial. Do not reuse deleted trial
assets or relabel that old round as the corrected GOOD experiment.

The old round climbed and confirmed, then recorded comment-only GOOD as
up to date without replacing the unit or restarting Voice. That matches the
semantic no-op path and does not establish premature sequence recording in
climb. BAD did not reach application: missing `voice/state` prevented quiet
from being judged, so automatic fallback was not exercised. Restoration was
reported with the real key fingerprint `54213f8d3aa2f241`, the original
`lampos-stable` feed and `lamp-voice` channel, original service/timer states,
no test sudoers drop-in, and deleted trial prereleases and disposable key.
Private preimages and journals have not been independently verified here.

Keep quiet fail-closed. Before the next trial, capture same-cycle startup,
active/privacy/software-mute state, BusOut initialization errors, topic
declaration/publication timestamps and the updater's fresh receipt through
the complete quiet interval. Topic presence later, or one spoken turn, is
not proof of fresh receipt and does not explain the old absence. A missing
state or unresolved publisher failure means HOLD; do not bypass the gate or
change production behavior to force the trial. A fresh second-only service
on an isolated real bus, without an utterance, still needs to demonstrate
fresh Resting through that interval. The file-backed regression does not
establish that service or device result.

## Teardown and evidence

Stop trial writers/recovery activity and save redacted test evidence before
restoring the coherent original snapshot. Restore original units, current
links/builds, complete production records, sequence/refusal state, trust,
channel and original service/timer states. Remove only the identified trial
override/key files. Verify the original hashes and healthy/listening Voice.
The updater must again use the exact signed channel and complete channel
URL recorded before the trial, verified from its effective command and
original unit/drop-ins. The signed channel can remain `lamp-voice` while
the feed URL is `lampos-stable`; do not derive one from the other or change
the installed feed during teardown. Test sequences must not remain in
production records.
Retaining updater 6 is a separate production migration, not this teardown.

Remove disposable signing/trust material after verified restoration. Retain
redacted GOOD/BAD journal lines, hashes and teardown attestations; exclude
keys, private unit values, complete records and personal/device identities.
This check covers product units and rollback. It does not establish default
settings/reset-plan application or actual old fixed-helper settings apply.
