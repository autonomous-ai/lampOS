# Whole-lamp saved-state compatibility inventory, 23 October 2026

This is Second's source audit for #2809, requested on Queue #3126 at 16:56 +07.
The tables identify each persisted store family, its version field, named tests
and what an update/reopen keeps, migrates or rebuilds. They include shared
settings, skill folders, optional stores and a clearly marked runtime/bench
appendix. No new passing test counts or physical acceptance are claimed.

- [Schedule, delivery ledger, Tasks, Records, Memory, Skills and Setup](core.md).
- [Voice, Controls, Eyes, Body, Expression, Cloud, Actions and remaining stores](rest.md).
- [Updater records, fixed helper, settings, models and installed skill replacement](update.md).

The component audit is bound to `3e75cc3cb`; the updater audit is at
`0f6f728b2`, whose added changes do not alter these updater storage formats.
The separately reviewed head-touch repair #3269 is not counted as landed.
Sources and exact test names are in each row or its immediately following
named-evidence list. The historical census in `docs/updates.md` is explicitly
older; this report does not silently reinterpret it as current qualification.

## What the existing tests prove

There is useful coverage in both data directions: current readers accept dated
older fixtures, and current readers receive synthetic later fields/versions.
Those are distinct from executing a frozen **previous release reader** on
current writer output. No such previous-release-reader execution was found in
the audited compatibility suites. The release boundary must be named before a
whole-lamp down/up guarantee can be made.

Archiving a newer store and starting empty preserves bytes, not the lamp's
active reminders, tasks, memories or recognition. Several readers deliberately
refuse unknown enums or safety/consent fields. A “read” of unknown fields can
also lose them on the next save. These outcomes are specified per store rather
than summarized as universal compatibility.

## Small closing pieces and proposed owners

These are recommended allocations, not new ownership grants. Each runtime
change starts with a failing test; deliberate privacy/mechanical refusals are
not blanket candidates for lenient deserialization.

| Piece | Smallest next evidence/change | Owner recommendation |
|---|---|---|
| Name the supported pair | Choose the prior shipped public build and current candidate, with immutable binary hashes and rollback floor. A fixture date alone does not choose the pair. | First |
| Fixed updater helper | Current writer → actual previous installed helper → current reopen for request/back/done/self/journal, including cut recovery. Keep #2810's attended unplug separate. | Second, after #3269 |
| Reminder/task continuity | Run previous readers on current pending/snoozed/completed/ack-intent and task-target states, then reopen current; assert which work remains active versus archived. | Third |
| Memory rollback floor | Qualify previous SQLite reader through current write/forget/reopen. Explicitly exclude rollback to JSONL-only reader after one-way import deletes legacy files. | Fourth |
| Records/session layout | Previous cleanup checker consumes current Voice layout/report/config and interrupted quarantine; no unsafe deletion and no mistaken “retention works” claim when a layout is refused. | Fourth, with Voice owner |
| Routine and skill folders | Dated daily/on/rested/provenance fixtures; actual previous Skills loader on the current core/installable folders. Preserve strict permission contracts. | First assigns Skills owner; Second can take a named routine-state slice |
| Privacy checkpoint/mute | Add explicit future-field/version and dated old records for onboarding checkpoint and durable mute, proving no false readiness/permission and bounded recovery. | Voice privacy owner + Third for Controls |
| First-stand record | Test a future Outcome with existing and absent lamp.env; preserve ask limits or explicitly refuse unsupported record instead of silently resetting it. Fresh consent/mechanical guards remain separate. | Fifth / Body owner |
| Generated-skill indices | Test malformed/future-shaped made/app/app-ids records before any overwrite or dedup reset; preserve originals and state a recovery policy. | Voice generated/app-skill owner |
| Output caps | Add dated JSON/SQLite fixture and previous-reader tests for restriction/status/tombstone outcomes. Keep the documented product ceiling when optional caps cannot run. | Actions owner + Third for shared Schedule container |
| Setup/account handoff | Previous consumers read current serve/reset/backend/consent output; interrupted reset follows the supported plan/journal pair. | First assigns Setup owner, with Cloud reader owner |
| Inventory drift | Tasks is schema 1–3, delivery nested storage fields are tolerant, Setup rereads its reports choice; add daily/on/rested/provenance, caps, first-stand and newer Voice indices/checkpoint rows. | Second's inventory here; component owners maintain their contracts |

Optional Settings and Surround still have explicitly expected-to-fail future-read
tests and no product service. Body pose bounds intentionally refuse new options.
Photo-unconfirmed data has a deliberate deletion/retention rule. These are not
unreported production regressions, and changing them is not part of this audit.

## Validation and limits

Read code, product unit paths, filesystem/deserializer calls, versions, fixtures,
named tests and update ownership/restoration rules. Spot-checked cross-component
claims: Voice provider/second configuration is excluded from ordinary manifest
settings; second-route provisioning is a separate install-owned operation.
No private data, signing key, camera/voice recording, or device state was read.
No historical binaries, device reboot/unplug or new runtime tests were executed
for this source audit. #3269 has its own host and native gate evidence.
