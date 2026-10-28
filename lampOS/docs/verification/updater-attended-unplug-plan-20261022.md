# Attended updater unplug preparation — NOT EXECUTED

Preparation for [#2810](https://github.com/autonomous-ai/lampOS/issues/2810),
separate from the [process-cut proof](updater-process-cuts-20261022/README.md).
Second has no lamp. No device access, power interruption or device result is
recorded here. [#1812](https://github.com/autonomous-ai/lampOS/issues/1812) remains
deferred until #2810 is accepted.

First must complete and approve these blanks before device work. Keep private
identities/access details outside public evidence; publish role labels and
redacted results.

| Required input | Assignment / evidence |
| --- | --- |
| Target and attending operator | ______ |
| Reserved window and explicit SSH approval | ______ |
| Physical support, clearance, boot-motion precautions and recovery access | ______ |
| Installed updater/fallback identity and qualified candidate artifact/digest | ______ |
| Exact trial argv, environment, service override, local channel and public key | ______ |
| Reviewed observer implementation, deadline/abort behavior and rollback procedure | ______ |

## Intended boundary and missing observer

Choose pending saved with the old updater still selected.
[own_switch:2311–2313](../../crates/update/src/round.rs#L2311) writes pending before
replacing the link; [write_whole:176–202](../../crates/update/src/records.rs#L176)
syncs the file, renames it, then syncs its directory. Stop at successful directory
`fsync` exit: valid `self.json` names the expected old/new builds, the verified
candidate exists, and `current` still names old. The real updater holds its lock.

**The proposed `cut-watch` observer is not delivered, tested or executable
guidance.** Its implementation and review are separate from the host-test PR.
There is no command here to copy. The design is to attach to the actual systemd
MainPID before exposing the candidate manifest, validate PID start identity,
executable, argv and service, then observe that syscall/prefix read-only. A
wrapper must not impersonate the updater MainPID. Thread handling, namespace/path
resolution, ptrace permission, unexpected exits and tracer death need explicit
handling. Proposed bounds are 20 minutes to reach the cut and 30 seconds held;
timeout must safely resume/detach. Only a verified READY receipt authorizes the
planned human unplug. No receipt or missed deadline means abort, not unplug.

## Preflight checkpoints

- Person present; arm supported against lost torque and boot movement. A stopped
  service alone does not establish physical safety; retain the
  [installation precautions](../install.md). Physical readiness: ______.
- Capture boot/storage identity, free space, current link, updater/fallback hashes,
  records, units/drop-ins, switches and public-key fingerprint. Require no pending
  self-update. Save and sync a recoverable backup. Evidence: ______.
- Verify executable fixed fallback, loaded/unmasked service and persistently
  enabled, active timer with the correct target, including the actual persistent
  unit files/overrides. Product readiness checks do not certify arbitrary overrides
  ([recovery contract](../updates.md#lamp-update-switching-itself-lamp_update_self-on-for-the-product)).
  Evidence: ______.
- Use real, distinct, qualified ARM64 updater binaries and a signed local trial
  channel. Review the installed-sequence manifest so ordinary parts/skills remain
  unchanged. Keep the test private key off-device; select an alternate public-key
  file without replacing product trust. Review explicit self-update opt-in and
  exact dry-check output; preserve quiet/safety guards and other switches.
  Manifest/provenance/backup verification: ______.

## One unplug and recovery observations

1. Start the approved real service against an unchanged trial manifest; attach
   the reviewed observer before exposing the candidate. Record the synced prefix
   and READY off-device. Receipt/time: ______.
2. The attending person unplugs once within the hold deadline, then restores power
   under the agreed physical precautions. Times/observations: ______.
3. Observe boot, selected executable, pending/refusal records and updater/fallback
   journals without reconstructing state or changing clocks. Allow **at least
   15 minutes after stable boot**, plus an approved investigation margin: old
   returns [NotOwn](../../crates/update/src/own.rs#L318); recovery may wait
   [600 seconds from pending's timestamp](../../crates/update/src/own.rs#L418),
   then a [two-minute timer pass with 30-second accuracy](../../deploy/product/lamp-update-back.timer#L8).
   This is an observation window, not a guaranteed deadline. Require automatic
   refusal/cleanup, old selection and a successful next round. Results: ______.
4. Preserve failures before any approved manual recovery. Record intervention
   separately. Restore trial configuration while quiescent; verify original
   trust, fallback, switches, service/timer state and clean pending state.
   Restoration evidence: ______.

One outcome covers this device/storage and boundary only. It does not prove every
cut, new-binary exec at this boundary, or general flash durability. All result
fields remain blank until attended execution.
