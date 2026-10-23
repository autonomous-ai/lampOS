# One attended updater exec cut — preparation only

Second, 22 October 2026; routing updated 23 October. **Not executed; no lamp result.** This worksheet
refines [#2810](https://github.com/autonomous-ai/lampOS/issues/2810)'s earlier
pending-before-link plan. It selects a successful exec of the candidate,
before its new main runs. The lead chooses the person-test window on lamp-52e6;
Fifth is that lamp's recorded holder and coordinates the actual device work
and access/install approval. First supplies the qualified build and reviewed
trial configuration. Second has performed no SSH, installation or physical cut.

The [ARM64 CLI rehearsal](updater-cli-exec-20261022/README.md) establishes the
boundary on synthetic binaries in Podman. It does not qualify a device
observer, distinct release versions, systemd, flash or a cold boot. This is
not a command sheet: First must supply and review the actual persistent
configuration, observer and restoration procedure before use.

## Person-test routing, 23 October

The lead's [08:21 routing](https://github.com/autonomous-ai/lampOS/issues/1711#issuecomment-6355091313)
places person tests on lamp-52e6. It supersedes the earlier table's 0c89
entry for #2810. This is a scheduling/ownership handoff, not permission for
Second to access a lamp; Second's role still names none. Fifth's recorded
ownership is confirmed by `deploy/lamp-52e6/LAMP.md` on `7bfcb31b8`.
No window is booked by this document, and the ongoing voice trial must not
be interrupted for an unplug. The holder needs the completed inputs below
and a qualified actual-device observer before the person is asked to act.

The same-sequence, self-only scope below leaves ordinary part updates out of
this trial. The separate dependency-order change in #3173 is simulated
restart evidence; it does not qualify this physical boundary. A power cut
still removes torque, so arm support remains necessary even when Body is
not being updated. The real signed internet/Body trial remains #1812 and
follows acceptance of this one.

## Inputs to complete before a window is booked

| Input | Actual value / evidence |
| --- | --- |
| Assigned lamp, attending role, explicit access/install approval | Lead's route: lamp-52e6; holder Fifth. Window, attending person and access/install approval pending |
| Reserved window; no conflicting played round on Queue | Pending lead and Fifth |
| Arm support, clearance, power/boot precautions and recovery access | Pending First |
| Installed old and candidate release, exact ARM64 hashes and green source heads | Pending First |
| Fixed fallback hash; actual unit/drop-in identities and recovery timer state | Pending First |
| Installed sequence N, clean recovery journal/pending, baseline refusal identities | Pending First |
| Reviewed same-N trial channel, manifest hash, public-key fingerprint | Pending First |
| Persistent service argv/environment/override and post-boot channel availability | Pending First |
| Reviewed observer binary/hash, PID/start-time binding, attach/detach/abort behavior | Pending First |
| Read-only off-device evidence route, bounded hold and abort deadlines | Pending First |
| Original configuration backup and reviewed uncut restoration method | Pending First |

Do not publish access details, a person's identity, credentials or private
keys. Keep the private signing key off the lamp. Use a separate reviewed
trial public key; do not replace production trust. Preserve ordinary
quiet/safety controls. Any played round needs its own Queue `PLAYING` slot.

## Source contract being tested

The original reading was bound to PR head `97ca7b322`. A [23 October source
comparison](updater-attended-source-20261023.json) finds all seven relevant
updater runtime and service/timer files byte-identical at integration head
`7bfcb31b8`. This updates the source binding only; it does not qualify a new
binary, an observer or a device. The actual trial artifact remains to select.
In `crates/update/src/round.rs`, `own_switch` stages and syncs the candidate,
saves `made`, writes pending, then replaces and syncs `current`.
`records::write_whole` syncs its temporary file and containing directory;
link replacement syncs its parent. Only after `round` returns does
`main.rs` exec `current/lamp-update`, retaining argv. A successful candidate
exec event therefore follows those barriers. A syscall entry, a changed
link or the initial service exec is insufficient.

`own::begin` samples pending's timestamp before link replacement. Its
600-second deadline includes persistence, observer hold and powered-off
time. Product fallback has a two-minute boot/activation timer and
30-second accuracy, plus service-failure triggering. Reserve at least
15 minutes after stable boot, with an investigation margin; this is not a
hard scheduling guarantee. Do not change clocks or pending timestamps.

Actual `lamp-update back` accepts no options, including `--root`. It uses
root `/`, `/var/lib/lampos-update/`, and real service state. The fallback
unit must use `/usr/local/lib/lampos/lamp-update-back back`. A relocated
root or mocked service cannot stand in for these device paths.

## Preinstall and dry preparation

1. Capture boot/storage identity, current and fixed-fallback executable
   hashes/modes, installed state bytes/sequence N, baseline refusals,
   units/drop-ins, switches and public-key fingerprints. Require no pending
   self-update or ordinary recovery journal. Preserve recoverable backups.
2. Preinstall the approved observer, trial channel/key and persistent service
   configuration. Verify that they survive reboot and that fixed fallback
   remains executable, its service loaded/unmasked, and its timer enabled,
   active and aimed at the right service. Inspect overrides as well as units.
3. Use a signed, self-only manifest at the **already installed sequence N**.
   Check its signature/schema/channel and exact dry output. Same-N equality
   skips ordinary parts/Body/skills planning after validation, while the
   self-update path remains available. An existing recovery journal would
   run first, which is why its absence is a precondition. Preserve all
   existing refusals and keep N throughout the trial and restoration.
4. Finish and record a **global sync before exposing the trial candidate**.
   This flushes deliberate preinstallation work. Rehearse the observer's
   no-cut detach/abort on the actual service under the approved procedure.
   A shell wrapper or the Podman helper alone does not establish its
   behavior on the multithreaded persistent device service. If that rehearsal
   completes a candidate handover, restore and recapture the old/current
   baseline before the cut attempt; an already-current candidate is skipped.

## One cut, with evidence outside the lamp

1. Attach to the verified actual service MainPID before exposing the
   candidate. Bind PID start time, executable identity, argv and service.
   Observe the **successful candidate exec event**, before candidate main.
   Require the same PID/start identity, expected candidate executable hash,
   reviewed unchanged argv, current=new, pending old/new/timestamp, complete
   candidate and unchanged installed state N.
2. Write the READY receipt off-device, including timestamps and remaining
   hold allowance. The observer must have a reviewed bounded abort that
   resumes/detaches safely. A mismatch, missing receipt or expired hold
   means abort with **no unplug**. Do not add a global sync, `syncfs`, file
   sync, state rewrite or journal-flush command at this cut: that would add
   durability beyond the updater's own persistence barriers.
3. The attending person disconnects power **once**, then restores it under
   the agreed physical precautions. Record power-off/on and the new boot
   identity outside the lamp. Do not fabricate the pending prefix after boot.
4. Observe actual executing binaries, records and journals. The candidate
   confirms only if its running folder/current/pending identities agree and
   its signed-channel dry round/state-lock checks pass. An active service or
   new link alone is not confirmation. Allow the recovery observation window
   above before distinguishing a stalled system from a delayed timer pass.

| Outcome | Required actual observations |
| --- | --- |
| Candidate succeeds | New boot; actual candidate executable/digest; confirmation; pending absent; successful next round; N and ordinary parts unchanged |
| Automatic fallback | Actual old executable/digest; candidate refused; pending absent; successful next round; N and ordinary parts unchanged |
| Neither | Preserve identity, times and failure evidence before approved intervention; report failure/inconclusive honestly |

Fallback is recovery, not successful candidate confirmation. Journal writes
are best effort, so combine them with executable and on-disk identity.
This one boundary on one storage device cannot establish all-cut flash
durability. The Podman seven-cut matrix remains a separate process-death test.

## Preserve evidence and restore without a second cut

Save the before/held/boot/after identities and result before any correction.
Record all manual intervention separately. Restore trial channel, key
selection, environment and overrides through the reviewed normal path;
recheck production trust, fixed fallback, service/timer, switches and clean
pending/journal state. Do not restore an old whole state file or lower N.
Preserve baseline refusals. Any removal of a trial-added refusal needs
explicit reviewed cleanup after evidence preservation.

After successful confirmation, `back` cannot restore old: pending is gone.
If binary restoration is required, plan a signed same-N handover to the
approved old binary only if it is not refused, or another reviewed recovery
method. Complete any signed same-N binary restoration before returning to
the ordinary production channel. Restoration is an uncut step, not another
power-loss trial.

Result and restoration fields remain **unexecuted**. The assigned device
operator must post the actual steps/outcome with First on #2810; [#1812](https://github.com/autonomous-ai/lampOS/issues/1812)'s
real signed internet release follows acceptance of that work.
