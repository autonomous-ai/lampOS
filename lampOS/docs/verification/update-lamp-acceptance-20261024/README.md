# Real-lamp update acceptance: prepared, not executed

24 October 2026, 17:01 +07. Second's ship-critical #2809 assignment follows
First's Queue comment 6382712180. Second owns no lamp and has made no SSH,
installation, signing, publishing, service or device change. This worksheet
supersedes no holder's device rules and does not mark a build row complete.

## What is available now

Release `lampos-lamp-voice-6592`, published 09:26:46 UTC, points to
`c6dfe980979e0088cd94f1dab1fe4a70ddbcbe46`. The downloaded signed envelope
contains exactly the downloaded manifest bytes; both match GitHub's published
asset size/digest. [Observation](observation.json) records exact hashes.
This is integrity/consistency evidence, **not positive signature verification**.
The claimed key label `54213f8d3aa2f241` is not a trust anchor. Obtain the
trusted public half through Migrate/the holder's provisioned trust path;
never replace it with a key from the feed or a repository fixture.

The decoded manifest has 32 files and 28 units: 16 for lamp-4ace and 12 for
lamp-52e6. It has **no models, settings or image sections**. A successful
run of this release can show its actual part/unit/skills update and state
readback, but cannot qualify #2809's missing payload categories. A lamp
whose installed sequence is already 6592 or newer needs a genuinely newer
qualified release; do not edit sequence/state or force replay.

## Inputs the holder records before a window

| Required input | Disposition |
| --- | --- |
| Lamp, current holder, attending person and arm support | First/Fifth to select/book; no window is assumed |
| Actual installed A sequence/commit and executable/unit hashes | Holder reads from device; do not infer from old LAMP.md |
| Qualified good B, greater sequence and exact signed envelope | 6592 is a candidate only if newer than A and suitable for that lamp |
| Trusted installed public key and independently confirmed fingerprint | Migrate/holder; still unavailable to Second |
| Complete selected-lamp unit diff against actual product configuration | Holder and First read before any switch; no silent drift overwrite |
| Bad-release route C and exact fault, with monotonic restoration D | First/Migrate select an isolated route; no bad build on the shared channel |
| Restore original service/drop-ins, fixed fallback and recovery access | Holder records exact existing files before changing any |
| Separate #2810 attended exec-cut acceptance | Existing worksheet remains unexecuted; person performs any unplug |

## Read-only preflight on the selected lamp

The holder runs these only under its access rules, retaining private full
configuration and publishing only scrubbed hashes/status. These commands
do not substitute for the eventual installed-service/sandbox run.

```sh
systemctl show lamp-update.service -p ActiveState -p SubState -p ExecStart -p FragmentPath -p DropInPaths
readlink -f /opt/lampOS/update/current/lamp-update
sha256sum /opt/lampOS/update/current/lamp-update /usr/local/lib/lampos/lamp-update-back
sudo /opt/lampOS/update/current/lamp-update check   --channel-url https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice-6592/   --channel lamp-voice --key-file /etc/lampOS/update/key.pub
```

`check` is read-only even if apply is given; the CLI appends
`lampos-manifest.json` to the folder URL. Use the actual selected newer
release URL if 6592 is ineligible. Read installed sequence, pending/journal,
refusals, free space and per-part current links privately. Require a clean
ordinary journal and no unresolved self-switch before starting. Do not
publish raw memories, reminder text, credentials, unit secret values or
camera/audio. Use designated synthetic records with IDs and byte hashes
for cross-version readback; preserve and verify unrelated user data privately.

## Attended execution and required result rows

1. **Baseline A.** Book the holder/person slot before any sound or movement;
   document clearance/arm support, active units, health, updater argv and
   sandbox, selected source/binary hashes, trust fingerprint, data/store
   IDs, refusal identities and recoverable original configuration.
2. **#2810 physical boundary.** Use the separately reviewed
   [attended exec-cut worksheet](../updater-attended-exec-cut-20261022.md)
   with selected candidate/fallback and qualified observer. A person does
   the unplug. Process SIGKILL controls are not flash/power-loss evidence.
3. **Good published B through installed service.** Retain the ordinary
   `lamp-update.service` sandbox and safety settings. If pinning an immutable
   signed folder for reproducibility, the holder records the exact temporary
   drop-in and restores it afterward. Do not use a manually invoked `once`,
   relocated root or mock service as acceptance of the installed service.
   Observe verification, waiting through a real busy interval, at least
   45 seconds of quiet, switch and the normal 60-second health watch.
   Record end-to-end boundaries; do not shorten quiet/watch to improve time.
4. **Read all selected parts.** For every changed part retain active binary
   hash, unit identity, readiness/health and designated old-store readback.
   Unchanged payloads are explicitly unchanged, not counted as new writes.
   Hash/read back the carried runtime files beside Voice, including its
   Smart Turn model and ONNX library; a missing dependency is FAIL, never
   repaired by hand before scoring. Preserving old model bytes is separate
   from updating a newly versioned model payload.
   Check pending/journal completion, current/previous and preserved refusal
   state. Include Body only with the person at the arm. Report PASS/FAIL per
   part/category; absent models/settings/image remain NOT RUN.
5. **Controlled failed candidate C and automatic put-back.** First/Migrate
   provide the exact isolated signed candidate and fault before this row.
   The documented old local test replaces Voice with Bus then re-signs;
   it is usable only in a separate reviewed trial channel/key route, never
   by modifying a real signed release or global channel. A test-key result
   qualifies that rollback mechanism, not real-key release trust. Keep
   production keys and the normal trust anchor intact. Observe health
   refusal, rollback without a manual rescue, A/B restored as planned,
   failed candidate in refusals and the failed part starting again. A
   manual restore is recovery evidence and a failed automatic-rollback row.
6. **Restoration D.** Preserve sequence monotonicity and exact refusal
   history; never delete state, lower a sequence or alter a signed envelope.
   Remove only the holder's recorded trial override, restore original
   service/key references, verify every selected part and data ID, sync
   retained device evidence, and copy scrubbed receipts off the device.

Post the completed run sheet on #2396 before execution, then exact PASS,
FAIL or NOT RUN steps/results on #2809 and the appropriate #2810/#1812
issue. First retains actual product/Body acceptance. Today's published
release does not settle missing payload coverage, trust handoff or a lamp
slot. Second continues source/CI work while those inputs wait.
