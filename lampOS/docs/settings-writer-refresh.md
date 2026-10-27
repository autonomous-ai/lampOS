# Refreshing the fixed settings writer over the air

Design and record/admission foundation for
[#3689](https://github.com/autonomous-ai/lampOS/issues/3689), 27 October 2026.
**The foundation is unwired; the privileged refresh remains a plan.**
The settings writer comes first; adding parts and carrying
templates, sockets and timers follows separately. Store key delivery is
Seventh's #3692. Harness installation is #3693.

The goal is a lamp shipped on build 7071 taking a later authenticated
settings writer without a person running `install`, then accepting a
newly allowlisted setting. A failed writer must restore both the previous
writer and any partially written settings automatically. No round may lose
its independent self-update fallback while doing this.

## What exists and what we keep

Source inspected: build 7071, `aa9ec9e27a788ee19d7b52a9c91296382208e490`,
and merged Main `a9d3a827439de1605e62b90cde9d9dcb8f571280`, which includes
#3466 through train 213. API PR base metadata is not ancestry evidence.
The relevant files are under `crates/update/src/` and `deploy/product/`.

| Existing behavior | Keep or change |
|---|---|
| `own.rs`: the updater switches its own `current`, then confirms only after an authenticated nonapplying round. The fixed `lamp-update-back` and its timer recover an unconfirmed updater. | Keep this barrier. Refresh starts only after confirmation, with no `self.json` or unfinished product switch. The updater's own unit is unchanged. |
| 7071 `lamp-update.service` runs as root and can write `/etc/systemd/system`, but its sandbox cannot write `/usr/local/lib/lampos`. | Use that existing authority to install a narrowly scoped root one-shot. The old settings writer is not the bootstrap mechanism. |
| `apply.rs` verifies the staged envelope independently and accepts only its compiled setting/image paths and guarded postcommands. | Keep authentication and ownership rules. A new path needs an explicit allowlist change and consumer tests. A signed filename alone is insufficient. |
| Fixed `lamp-update-back` serves both `back` and `apply`. `install` alone currently copies it. | Refresh needs an independent recovery executable established before replacing this file. A `.prev` file without a working restore route is insufficient. |
| `apply::back` validates paths with that executable's compiled allowlist. | A 7071 `.prev` cannot undo a partially written setting in a new folder. The refresh guard must finish file restoration before restoring the old writer. |
| `Records` holds the round lock while `ask_apply` runs. `own::back` can report busy without exercising recovery. | Do not acquire the parent's lock in an apply child or count a busy back probe as success. Bootstrap/probes run between rounds; the guard has its own lock. |
| `records::write_whole` uses a new sibling, file fsync, rename and directory fsync. | Reuse the mechanism and owned-child bounded execution. Retain recovery material until terminal state is durable. |

V1's actual `scripts/provision/software-update` records absent rootfs
paths in `snapshot_device_rootfs`, restores those paths in
`restore_device_rootfs`, and keeps `pending-update.json` until restoration
and health checking succeed. `system/bootstrap/recovery.go` invokes recovery
before version detection, including the legacy missing-live/present-backup
case. Keep explicit absence, recovery-before-version-detection and a durable
pending record. Do not copy V1's broad rootfs authority or claim its shell
and process layout is equivalent to this sandboxed Rust updater.

Use existing Rust, signature, filesystem and process helpers. Systemd is
the installed supervisor; no new runtime dependency is needed. Its Bookworm
manual distinguishes ordering from dependency admission: `Before=` does not
pull in a recovery unit. Failure handling also needs a boot path for power
loss, not just `OnFailure=`. Verify the resulting units against the target
systemd version, not only a fake manager:
[unit](https://manpages.debian.org/bookworm/systemd/systemd.unit.5.en.html),
[service](https://manpages.debian.org/bookworm/systemd/systemd.service.5.en.html),
[sandbox](https://manpages.debian.org/bookworm/systemd/systemd.exec.5.en.html).

## Capability and authority

Reserve updater level 7 for the writer protocol if still unallocated at
implementation. #3466's level 6 remains ordinary installed-service carrying.
Require a signed minimum, a signed carried updater and successful existing
updater climb/confirmation before refresh. No helper, setting or product
unit write occurs during the dry confirmation round. A read-only check or
explicitly disabled apply cannot refresh the helper.

The request binds the signed envelope, sequence, candidate executable hash
and size, exact running confirmed updater identity, guard protocol and
operation. The root entry point independently checks the lamp's key and
the actual executable bytes; it does not trust a caller's success receipt,
`current` symlink or an arbitrary command/path in JSON. Channel, sequence
and refusal rules still apply. Reject unsupported operations and protocol
versions before mutation; additive-field tolerance cannot change recovery
meaning. Never write the update trust key.

Use only fixed compiled unit names and argv. Refuse local replacements,
masks, aliases, drop-ins, nonregular files, unsafe ancestors or unknown
manager identities at these new names. Preserve existing recovery units
and their timer. Existing custom helper bytes are not adopted by matching
a filename. Verify actual file ownership, permissions, identity and digest
again at the mutation boundary.

## A guard independent of the candidate writer

The first confirmed level-7 updater bootstraps a **separate frozen,
root-owned guard copy**, outside the updater service's writable paths.
Provisional executable: `/usr/local/lib/lampos/lamp-update-writer-guard`.
It can be a restricted mode of the signed Rust updater binary, copied to
that fixed location. It is not resolved through `update/current` and is
not replaced in the same transaction as the writer it supervises.

Operational separation is not an independent implementation proof. The
guard's restore path must not delegate to the candidate's `apply::back`,
and must have distinct fault tests. An initial binary that cannot execute
its guard mode fails before changing the existing writer. Future writers
whose semantics exceed the installed guard protocol are refused until a
separately compatible guard upgrade is designed and tested.

The bootstrap one-shot grants only the filesystem access required to
install/check this guard and its private records. It must durably establish
and verify the guard, failure recovery and boot admission **before** the
first writer rename. A partial bootstrap can be repeated or refused with
the old writer intact. Do not make the existing updater sandbox writable
over `/usr/local/lib/lampos`.

The guard is enabled independently at boot and ordered before updater,
back and apply. Those consumers must also require successful guard
admission while a refresh is pending; ordering alone is insufficient.
No new self-update or product round may outrun an unfinished guard journal.
The failure route starts the same recovery mode. Preserve recovery units
and `.prev` across repeated interrupted restores; never consume the only
old copy with the first rename.

Admission has one explicit exception to avoid a circular wait: after the
back probe, the guard may grant **one first-apply product transaction**.
Before any product mutation, that grant binds a fresh authenticated
envelope, complete supported apply plan, writer hash, product-state
before-image and unique transaction identity. It permits only that
transaction to enter the normal round lock and ask for supervised apply;
it does not permit another updater handover or a different build. Planning
and authentication can run without mutation to obtain the grant. The
guard lock serializes grants and recovery, while the parent keeps the
existing product-record lock through its apply child. Both sides recheck
the grant identity; neither may reacquire a lock held by its caller.

## Transaction and first use

The guard owns a separate, bounded, root-protected versioned journal and
before-images. They are not mutable requests/preimages owned by the
candidate and are not interpreted by 7071's generic journal reader. Unknown
version, missing preimage or malformed journal blocks admission and remains
available for recovery; do not rename it aside and start a new transaction.

| Phase | Durable facts and permitted action |
|---|---|
| Prepared | No product switch or self-update is pending. Authenticated candidate, guard identity, old writer bytes/mode/owner and refusal identity are known. Guard boot/failure admission is verified. Stage and self-check the candidate without replacing `back`. |
| Armed | Preserve the exact old writer as `lamp-update-back.prev`, with fsync and parent sync. Persist complete recovery metadata before the first live rename. An already existing `.prev` or journal must reconcile with this transaction, not be overwritten blindly. |
| Published | Replace the writer using a synced sibling and atomic rename, then sync its directory. Guard remains independent and journal remains pending. No success claim from rename alone. |
| Probed | Run a bounded real `back` probe between rounds when no self transition exists. Verify the expected no-work outcome and positive executable/protocol identity. Busy, timeout, wrong output or descendant leakage is failure. |
| First apply armed | Before the first real apply, independently validate its entire finite authenticated operation set and persist all relevant preimages: existing and new settings, removals, image/reset-plan effects if selected, modes, absent paths/directories and apply bookkeeping. A variant whose effects the guard cannot restore is refused before any write. |
| First apply | The guard supervises the candidate and validates the request-bound completion receipt. Keep the parent's round lock convention; do not re-lock it from a child. All executed postcommands count toward success or failure. Missing/partial completion is failure. |
| Committed | Persist a receipt binding guard, writer and successful first back/apply. Release ordinary refresh admission only when product recovery is also closed. The normal product round still owns its own installed-state/health commit. |
| Restoring | Inhibit further use; close and reap owned candidate work. Restore partial apply effects and bookkeeping first, then the old writer, then prove old serviceability. Persist refusal/completion before clearing the guard journal. Retain it on incomplete recovery. |

If no real apply is requested, publication/probe does not silently count as
first-apply acceptance. The guard retains that obligation and may grant the
first actual apply later. On reboot an uncommitted refresh is restored
before normal admission, rather than waiting for a product round that is
itself blocked. A later refresh can prepare a fresh grant. A successful
no-op `apply` with no request is not new-settings acceptance.

Before-images distinguish absent from empty. Restore exact content and
mode and remove only positively owned newly created files whose current
identity matches the transaction. Remove new directories only if empty.
Keep unrelated files, personal data and later local edits; an unexpected
third value means explicit incomplete recovery, not destructive guessing.
Journal all candidate-owned possible output identities before allowing
their write so a cut after an effect but before its receipt is recoverable.

The guard accepts a finite compiled set of supported operation/path
semantics. Do not broaden this into arbitrary `/etc/lampOS/**` writes.
Trust files, credentials, person/name data, setup answers, reset authority
and per-device calibration keep their existing independent policies.
The first new settings folder must have a real consumer, explicit
ownership/reset semantics and consumer tests in its implementation PR.
Restoration also replays the explicitly supported post-restore effects,
including required tmpfiles and image reload operations, as current
`apply::back` does. Unknown or nonrecoverable effects refuse the entire
plan before any writer or setting write; bytes alone are insufficient.

There are two commit records: writer qualification and the product round.
They must agree after every prefix. If the writer qualifies but product
health later fails, ordinary product fallback must still restore the
settings with a compatible executor. If recovery is incomplete, retain
the executor/preimages and block admission; never leave old units paired
with a newly advanced installed sequence or an old writer asked to parse
new restore semantics.

Concretely, retain the immutable qualified writer, frozen guard, complete
guard apply preimages and normal apply before-images until the product
journal closes durably. A qualification receipt does not authorize their
pruning. On boot, settle any unqualified writer/partial first apply first,
then recover the product journal with its retained compatible executor;
only after both close may ordinary rounds or another refresh proceed.
If qualification is already committed but the product journal remains,
recover that product transition first without reverting the writer to a
version unable to read its preimages. Test both commit windows separately.

## Verification before enabling it

`crates/update/src/writer_refresh.rs` now provides the bounded versioned
record/admission foundation only. It retains exact refresh and first-apply
identities, permits one live first apply, and requires recovery after
reopening any unfinished transaction. Qualification retains the writer and
preimages until the separate product closure. A persistence error poisons
the live session because rename may precede a failed sync acknowledgment;
only a fresh disk-state recovery can proceed. Restoration provenance keeps
the original grant/preimage identities after partial apply. Unknown
nonsemantic metadata survives; unknown operations/phases/versions and
malformed or oversized records block admission without deletion.

The module does not authenticate signatures or verify represented artifact
bytes, install a guard, copy a helper, execute restoration or activate a
service. Its external-effect acknowledgments require verified caller
results. An existing private stable store directory is a caller
precondition; privileged ancestor/ownership confinement belongs to the
later bootstrap. No CLI or round currently calls this module. Eighteen existing
focused tests cover admission, records and recovery inputs; broader gates and
original native qualification are recorded separately on the PR.

The local recovery-input follow-up adds `Session::recovery_inputs`, a
borrowed view available only after `Store::recover` loads a valid journal.
It distinguishes writer-only restoration, first-apply restoration with
its retained plan/preimages and original stage, qualified product recovery
with the compatible writer, and terminal ordinary admission. Poisoned or
live sessions cannot obtain that view; it neither authenticates represented
bytes nor executes recovery. The source-bound local follow-up passed fmt,
strict all-target Update Clippy and the complete library: 163 passed,
0 failed and 2 ignored, including all eighteen writer-refresh controls.
That execution used the earlier foundation plus these owned changes;
current-base and native qualification remain separate.

Implement in reviewable steps: protocol/guard records and restore engine;
root bootstrap plus failure/boot admission; supervised first use and round
integration; then the new setting's consumer and installed rehearsal.
No unit creation for general parts or activation types is included here.

Required controls:

1. Actual 7071 reader fixture and old/new executable identities: old helper
   refuses a new path; the refreshed helper accepts it. Older updater sees
   the higher signed floor before product writes and climbs only itself.
2. Invalid signature, modified executable, wrong size/identity, stale
   request, unsupported protocol/op, read-only/off and unmanaged/custom
   installation cause no refresh writes.
3. Unstartable/hanging self-check, failed first back, busy back, failed
   first apply before and **after** one new-path write, failed postcommand,
   missing/false completion receipt and inherited child output. Every child
   has a deadline, output cap and verified owned-group cleanup.
4. Restore all old bytes/modes/records and absent paths, not only the
   writer filename. Old writer cannot perform new-path undo: a fixture
   must prove recovery uses the independent guard instead.
5. Local file/symlink/mask/drop-in collision, concurrent updater, mutation
   after snapshot, existing `.prev`, malformed/unknown guard journal and
   unavailable independent recovery fail without overwriting authority.
6. Enumerate cuts before/after every write, file fsync, rename, directory
   fsync, unit/enablement publication, reload, start, completion/state save
   and journal clear. Include cuts during rollback and a second boot.
   Inject EIO/ENOSPC and lost replies after successful manager actions.
7. Preserve self-update cuts, level-6 product-unit carrier, settings,
   image/reset-plan, old manifests and release-pair regression coverage.
   Grep every changed protocol/unit name across all readers and CI filters.
8. Prove generated units and boot dependency/failure behavior in isolated
   real Bookworm systemd, then original PR Linux ARM64/Clippy. A fake
   manager or a green unit exit is not this evidence.

Use scoped Mac fmt, strict affected-crate all-target Clippy and complete
changed/actual-reader tests, not a full local workspace or ARM run. Record
first failures, exact source/executables and PASS/FAIL/LEAK/skip/unrun counts.
Do not shorten quiet/watch assertions to gain speed. Helper maintenance is
off the audio path; preserve the existing quiet boundary for the later
product switch. Set child deadlines explicitly, and measure refresh,
first-use and fallback durations under normal and slow-manager conditions.

First owns the separately authorized 7071-imaged lamp rehearsal: GOOD
refresh plus a genuinely new allowlisted setting, then BAD writer/partial
apply with automatic exact fallback. SIGKILL tests require witnessed
boundaries and are not physical unplugs. A true power cut requires a
person and an agreed slot. No device, signing, feed or promotion action
is authorized by this design.

## What has not been shown

Only the unwired record/admission foundation has local implementation and
test evidence. No privileged refresh, process-cut trial, root systemd run,
new-path setting acceptance or lamp rehearsal has run for this extension.
The cited source audit establishes the bootstrap route and the old-helper
rollback gap; #3466's earlier ordinary-unit GOOD/BAD does not close them.


### Owner-scoped guard lock release

The unwired `Store` acquires a private `OwnedLockGuard` only after its
nonblocking exclusive lock succeeds. `inspect`, `prepare`, `recover` and a
live `Session` record the acquiring process ID and release that owner's
lock explicitly before closing the file when dropped in that same process.
An inherited guard dropped in another process only closes its descriptor;
it must not explicitly unlock the parent's shared lock. A duplicate descriptor may otherwise keep the same open-file-description
lock alive after the controller is dropped. The journal, recovery admission,
timeouts and grant policy are unchanged. Unlock errors cannot be returned
from `Drop`; closing the file remains the fallback.

This is an owner-scoped lease, not ownership transferable across `fork`.
The parent controls the lease; a child must not use an inherited live
`Session` as an independent journal or grant owner. The PID check protects
automatic child-side guard drop, not child-side journal operations or general
fork safety. Explicit unlocking affects every
duplicate reference to the original lock, including a child's, as the
[Linux flock manual](https://man7.org/linux/man-pages/man2/flock.2.html) and
[Apple flock manual](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/flock.2.html)
describe. A later session acquires an independent open description; closing
a stale duplicate must not release that new session's lock.

A deterministic control retains a duplicate descriptor across session drop
and checks immediate recovery and the independent new lease. A second
control supplies a private non-owner PID to model inherited-descriptor drop
and verifies that the parent's lease stays held. These model descriptor
retention and ownership; they do not run a real fork or spawn or establish
the cause of any reported concurrent-test failure. The old production code
plus this new control was preserved before the candidate change. The protected old-production control ran once: 0 PASS / 1 expected FAIL,
with OS code 35 (`WouldBlock`) at the immediate inspection after session
drop. Its original orchestration exit 1 is retained: a parser expected a
plural header and seven SKIP dashes, while a one-case run used a singular
header and three dashes. Separately hashed offline analysis accounts for
that one failure and all 182 skipped pairs without a compiler or runtime
replay. It does not establish the reported flakes' cause. Earlier baseline-20
and instrumented-candidate-38 cohorts remain INCONCLUSIVE and unwaived.
At this preparation freeze the PID-guard candidate and its two models are
UNRUN; later qualification must retain its exact source and separate result.


The eighteen existing record/admission controls plus the two lock-lifetime
models make twenty focused module controls. On the frozen preparation at
Main `f903659bb`, the first full candidate execution passed 512 cases with
0 bad or selected unrun and 6 policy ignores; all 33 required controls
(twenty module, eight bound-receipt and five Head-input/mask cases) passed
once. Fresh fmt, strict all-target Update Clippy, compilation and discovery
passed, and all 7,500 source/configuration/runner guards and ten preserved
executable hashes closed. Doctests had no cases; the two Mac custom-driver
contracts were already policy-ignored and added no passes. This paragraph
is a later documentation-only qualification note; its Rust and test bytes
are exact to that frozen execution. Later current-base/native evidence is
separate, and no old reported failure is waived or explained by that pass.
