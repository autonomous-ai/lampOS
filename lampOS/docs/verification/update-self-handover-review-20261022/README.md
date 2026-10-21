# Updater handover: existing implementation and remaining recovery check

Read-only review by Second, 2026-10-22 +07, of `lamp-voice`
`4efcf4eb0cda0ef730d30658572f49da7592a0b8`. No updater source was changed,
no build or test was run, and no lamp was touched. The proposed change is
not implemented or qualified by this document.

First's [assignment](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6322546808)
asks for updater-last handover, proof, rollback and interruption fixtures.
The [shape posted before code](https://github.com/autonomous-ai/lampOS/issues/1369#issuecomment-6322682547)
identifies that #1788 and #1801 already provide the handover. #1369 is closed
for image-owned files; #1680 is closed as delivered. First accepted the remaining
slice in [#2700](https://github.com/autonomous-ai/lampOS/issues/2700), claimed
by Second before implementation.

## What already exists

With exact `LAMP_UPDATE_SELF=on`, an ordinary compatible round finishes its
parts and saves their state before `own_switch`. The updater is installed
beside the retained old binary, with size/SHA checks and file/directory
syncs. `self.json` is saved before the atomic `update/current` replacement.
`main.rs` then execs the candidate with the same arguments. A failed exec
invokes rollback while the old process still runs.

The new updater confirms only after a signed manifest dry round and a
retained-state read under the lock. Until then, applying rounds are refused.
The fixed, independently installed `lamp-update-back` restores the old
link, refuses the candidate hash, clears pending state and requests a
restart. The fallback is outside the updater service's writable paths.

The proof deadline is 600 wall-clock seconds. Recovery also depends on the
next timer opportunity: every two minutes with 30-second accuracy, and
possibly later under lock contention. This is not a strict 600-second
restart guarantee. An offline but otherwise good candidate is deliberately
rolled back and refused when it cannot prove the channel path.

`min_updater` is an intentional exception to updater-last: when the running
updater cannot read a release, it takes only the carried updater first.
Parts wait for confirmation. That exception must remain.

Source at the reviewed commit:

- [round.rs:470](https://github.com/autonomous-ai/lampOS/blob/4efcf4eb0cda0ef730d30658572f49da7592a0b8/lampOS/crates/update/src/round.rs#L470): ordinary-round ordering.
- [round.rs:2166](https://github.com/autonomous-ai/lampOS/blob/4efcf4eb0cda0ef730d30658572f49da7592a0b8/lampOS/crates/update/src/round.rs#L2166): updater-first climb and shared switch.
- [own.rs:284](https://github.com/autonomous-ai/lampOS/blob/4efcf4eb0cda0ef730d30658572f49da7592a0b8/lampOS/crates/update/src/own.rs#L284): confirmation; `back` begins at line 384.
- [main.rs:760](https://github.com/autonomous-ai/lampOS/blob/4efcf4eb0cda0ef730d30658572f49da7592a0b8/lampOS/crates/update/src/main.rs#L760): exec and immediate failed-exec rollback.
- [records.rs:175](https://github.com/autonomous-ai/lampOS/blob/4efcf4eb0cda0ef730d30658572f49da7592a0b8/lampOS/crates/update/src/records.rs#L175): durable file and link replacement.

First's [merged-head lamp trial](https://github.com/autonomous-ai/lampOS/pull/1788#issuecomment-6279387052)
already reports successful confirmation and both immediate-failure and
missing-proof rollback. This review did not repeat or extend that trial.

## Remaining source-derived concern

`own_switch` checks the current link and candidate but never checks that
independent recovery is available. The fallback service deliberately skips
when its fixed binary is absent; the [older-install regression](https://github.com/autonomous-ai/lampOS/blob/4efcf4eb0cda0ef730d30658572f49da7592a0b8/lampOS/crates/update/tests/install.rs#L3644)
pins that behavior. Such an older installation reaches today's self-update
code only after a newer updater is deployed without rerunning install;
the old OTA code does not acquire the new mechanism by itself.

There is also a current-install path: [install.rs:805](https://github.com/autonomous-ai/lampOS/blob/4efcf4eb0cda0ef730d30658572f49da7592a0b8/lampOS/crates/update/src/install.rs#L805)
enables units without starting them. Starting the updater with self-update
enabled before starting its fallback timer leaves deadline recovery unarmed.
This follows the [Bookworm systemctl contract](https://manpages.debian.org/bookworm/systemd/systemctl.1.en.html).
These are source-derived paths, not reproduced failures.

| Recovery state | What is missing |
| --- | --- |
| Fixed fallback absent or not executable | An independent program to restore the old updater |
| Timer absent, masked or stopped | Deadline recovery while the candidate remains running |
| Timer enabled but inactive | Recovery is configured for boot but not armed now |
| Timer active but disabled or enabled only at runtime | Recovery activation that persists across reboot |

Systemd distinguishes persistent `enabled` from `enabled-runtime`; the
latter does not survive reboot. A timer's `Unit` names what it activates.
These are available in the [systemd 252 interface](https://www.freedesktop.org/software/systemd/man/252/org.freedesktop.systemd1.html).
The current `world::Systemd::states` queries only ordinary part-health
properties, so it cannot establish this whole condition.

Proposed scope: one readiness check after unchanged/refused-candidate
short-circuits and before candidate download, pending state or link movement.
Check the fixed fallback and effective recovery service/timer, including
current scheduling and persistent enablement. Keep this query bounded;
do not install, enable or start anything as a side effect. Both handover
routes share `own_switch`. A deferred self-switch preserves an ordinary
round's completed parts; a deferred climb preserves the original refusal.
The exact effective-unit checks remain a design decision, not proof that
all possible systemd overrides or later administrative changes are safe.

## Fixture plan

Existing tests in `tests/round.rs` cover confirmation, an immediately
exiting candidate, deadline expiry, a refused manifest, switch-off,
concurrent rounds, forward-compatible pending state, retention of the old
binary and the `min_updater` climb. Corrupt-pending rollback already has
interruption coverage. No ordinary handover cut matrix was found.

The additional matrix should preserve the real durable ordering:

| Boundary left on disk | Required recovery assertion |
| --- | --- |
| Candidate durable, no pending record | Old link remains usable; a later round can retry |
| Pending durable, old link still selected | No second applying round; fallback settles the pending switch |
| New link durable, before exec/confirmation | Boot selects the candidate; signed proof confirms it or fallback restores old |
| Confirmation record removal complete | Later fallback does not undo the confirmed switch |
| Ordinary rollback restored link, pending still present | Repeated rollback keeps old selected, records refusal and completes cleanup |
| Refusal durable, pending cleanup interrupted | Cleanup resumes without selecting the refused candidate |

Use the existing fixture filesystem and real record/link helpers. A failed
temporary-link replacement can expose the pending-before-link boundary;
a failed refusal write exposes ordinary rollback after link restoration.
Reconstructed persistent states must be named as fixtures, not physical
power-loss measurements. No additional ordering defect was found in this
read, and the proposed matrix has not run.

The readiness regression should cover missing/non-executable fallback,
unloaded/masked service, inactive or runtime-only timer, query failure and
the ready case, for both ordinary and updater-first paths. On refusal:
unchanged updater link, no `self.json`, no candidate fetch, and the expected
parts outcome. The off-switch path should perform no recovery query.

The only updater `Systemd` implementations found across `crates/` are the
real `Systemctl` and update's round fixture. Keeper has a separate trait
with the same name. If this slice is taken, the Mac gate is formatting,
strict all-target Clippy and tests for changed crates; the PR's native
Linux ARM64 run supplies the ARM64 gate. No local ARM64 gate is planned.
