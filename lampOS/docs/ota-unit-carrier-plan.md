# Adding a part and carrying its activation units

Later piece of [#3689](https://github.com/autonomous-ai/lampOS/issues/3689).
Design only, 27 October 2026; the settings-writer refresh comes first.
No unit installation, tests or lamp trial are claimed here.

## Existing behavior and scope

Source: Main `f7252d81dc2e72494afc6ea842bd8b0445f1991c`, containing
#3466 through train 213. Its manifest, round, publisher and records files
are byte-identical to #3466's offered `dd5a88c61`. Thus the reviewed
ordinary-service carrier is the starting point, not a proposed replacement.

`manifest::is_unit_name` allows ordinary `lamp-*.service` names only.
`publish` requires a binary-backed unit. `round::carried_units` starts from
an installed unit and skips an absent part; a product unit needs a managed
current link. `Switching` records previous unit text, activity and links,
not absent-before units or complete enablement state. Existing `Watch`
assumes a continuously running service. Widening the filename test cannot
safely add this feature.

Fresh install already carries socket/template and timer/service pairs:
`install::PRODUCT` and `enabled()` install templates without enabling them,
enable their sockets, and do not independently enable timer payload
services. Reuse that finite inventory and those relationships, not its
blanket fresh-install enable operation as a rollback protocol. Some
product sockets use `Accept=yes` and `MaxConnections=1`; records' timer is
`Persistent=true`. Both need explicit lifecycle treatment.

V1's `scripts/provision/software-update` records absent rootfs paths and
old service activity, restores rootfs before daemon reload, and keeps its
pending journal through restoration/health. Keep explicit absence and
recovery-before-normal-work (`system/bootstrap/recovery.go`). Do not copy
V1's broad rootfs writes, enabled-implies-start policy or shared runtime.
No new dependency is needed for the proposed Rust/systemd extension.

## Chosen initial contract

Use a **compiled finite product registry** in the authenticated updater,
with signed unit/binary bytes in the existing manifest. Each registry row
names part/folder/binary, exact member units, activation relationships,
prerequisites, bounded instance policy, desired initial activation and
health contract. Do not install every shipped binary or every `lamp-*`
name. A new part arrives with an updater that understands its registry.
If a future extension needs a data-driven activation plan, specify a new
signed typed protocol rather than inferring authority from unit text.

Reserve a capability above the settings writer's level 7, provisionally 8
only if still free at implementation. Keep the level-6 behavior intact.
An older updater authenticates the higher floor and climbs/confirms itself
before any new unit, link, enablement or setting write. A signed floor does
not make an old recovery reader understand a new journal; the independently
retained recovery executor must support this transaction first.

Initial additions are managed product-linked parts with existing supported
users/groups, directories and dependencies. Required versus opt-in is
explicit: new optional parts remain disabled. Existing locally disabled
or inactive members preserve their choices. Missing prerequisites,
unprovisioned identities and unsupported activation forms refuse the whole
plan before mutation. The updater's own service and independent recovery,
apply and writer-guard contracts are excluded from this generic carrier.

Preserve #3466's exact-lamp precedence, pinned handling and local drop-ins.
A missing `/etc` fragment does not prove a unit name is absent: inspect
canonical Id/Names, fragment/drop-ins, masks, aliases, generated/transient
state, persistent/runtime links and loaded state. Refuse ambiguous local
authority and unknown manager state, not force-enable or unmask it.
Revalidate expected identities at each write boundary and reject unsafe
ancestors and links.

Compute an exact bounded enablement-link delta. Reject `Alias=`, `Also=`
and implicit `DefaultInstance=` expansion in the initial registry. No
global presets or unconstrained `enable`/`reenable`. Templates are not
started or enabled directly. Explicit instances are finite registry
entries; dynamically socket-created instances require a separately bounded
family policy with verified canonical template/manager identity. A name
prefix alone never establishes ownership.

## One recoverable switch

1. Authenticate and validate the entire candidate graph, all artifacts,
   binary references, settings and activation policies. Stage verified
   bytes without activating anything. Snapshot the complete old graph and
   all affected readers, including shared binary folders.
2. Persist true absent/present before-images for unit files, content,
   modes, current links, enablement links and relevant manager state.
   Save old and new order plus intended effects before mutation. Never
   encode absence as empty old unit text.
3. Reuse the existing fresh quiet boundary. Inhibit socket/timer admission
   first, then settle already owned work within its bounded family policy.
   If a request cannot safely finish/cancel, defer before moving its files.
   Preserve one writer for hardware and all privacy/consent checks.
4. Install service/template/binary members, then activators, with synced
   atomic file/link changes. Reload only a coherent candidate set. Start
   declared service prerequisites, prove readiness, and admit socket/timer
   activation last under a fresh quiet check.
5. Retain the complete journal through typed readiness and the configured
   health watch. Save a consistent installed-state/terminal receipt before
   durable journal closure. A cut between state save and journal clear must
   never leave old units paired with a new installed sequence.
6. On failure inhibit new activations first, settle only owned instances,
   restore exact old presence/bytes/modes/links/enablement, reload and resume
   only the previously active authorized set in saved old order. Reestablish
   old serviceability before completion; retain the journal on uncertainty.

Boot admission is part of step 2, not a later cleanup: establish a durable
recovery gate for **every** affected activation root before publishing any
candidate file or persistent enablement. Candidate sockets/timers must not
start through `sockets.target`, `timers.target` or another dependency while
a transaction is pending, even if the updater has not started. Runtime
masks vanish at reboot, and `Before=` alone does not hold a consumer after
failed recovery. The implementation must prove a persistent requirement
and successful-admission barrier with the compatible fixed recovery
executor; if that executor cannot enforce the new semantics, refuse the
transaction before writes. Retain the gate through recovery and product
journal closure. A real-manager boot test must cut after persistent
enablement/file publication and prove zero candidate admission, queued
activation or timer catch-up before recovery completes. Do not infer this
from journal-before-write or an in-process fake.

A new continuously running part needs positive fresh application readiness
even without an old health entry. A socket needs the correct listening
endpoint and service relation; a template file is dormant, not a process.
A timer needs its correct target/waiting state; a successful one-shot need
not remain running. Do not trigger a real maintenance job just to produce
a health receipt. Real fixture roundtrips must reject an old or wrong
instance's response. These roundtrips use controlled fixtures. A real
helper connection can itself do work (the camera helper reauthorizes a
device), so production readiness must use a reviewed passive or explicitly
safe family probe, not a blind health connection.

Stopping/restarting a socket can lose queued clients; a timer can become
due or perform persistent catch-up. Each supported family must define its
admission/drain behavior and safe retry semantics. Preserve actual job
completion history; do not rewind it or claim to undo external effects.
Refuse/defer an unsafe family rather than silently triggering work during
staging or fallback. In particular, existing persistent timers require
their own missed-deadline/duplicate-execution tests before admission.

Rollback removes only positively owned newly created product artifacts
whose current identities still match the transaction. Preserve later local
edits and all personal/new-part data; remove a created directory only if
empty. A foreign value yields explicit incomplete recovery. No recursive
data deletion or Reset is part of fallback.

## Required proof

Start B0 with a positively absent part and an existing OLD socket/template
pair. Snapshot bytes, modes, links, persistent/runtime enablement, masks,
activity, identities and health. Preserve unrelated custom and disabled
members as witnesses.

- GOOD adds absent part A and makes a real socket/template protocol change
  G, then positively proves A and G readiness through the full watch.
- BAD in the same continuous trial adds a **second absent part B** and
  changes that socket/template again, then deliberately fails the watch.
  Automatic fallback must remove B, restore exact G configuration and
  enablement, retain A and restore G serviceability. Reusing A alone would
  not prove absent-before rollback. Alternatively run BAD from a separately
  prepared coherent B0 baseline and label the manual preparation separately.
- Exercise a timer's idle, due, persistent catch-up and one-shot failure
  states; bounded Accept=yes activity and rollback; local edit/mask/alias
  refusal; disabled/active combinations; stale/unknown readiness; quiet
  changes at each final activation boundary; old/new protocol compatibility
  in both directions; off/check/no-op behavior and protected-unit equality.
- Enumerate before/after every durable write/fsync/rename/unlink, enablement
  change, reload/start/arm, readiness, state commit and journal clear.
  Include interrupted rollback, lost manager replies and EIO/ENOSPC.
  A malformed or unsupported new journal blocks destructive interpretation;
  test the actual preceding recovery executable, not a new reader pretending
  to be old.

Run scoped fmt/strict Clippy and full changed/actual-reader tests, retain
first failures and exact counts, then original PR ARM64/Linux and isolated
real Bookworm systemd. Existing twelve product-carrier controls and the
older updater self-cut harness are regression prerequisites; they are not
new-part/timer acceptance. SIGKILL requires a witnessed operation boundary
and is distinct from physical power loss.

First owns a separately approved lamp/person slot and the installed
GOOD/BAD/power rehearsal. Record quiet-to-activation, start-to-readiness,
watch, fault-to-inhibition and fallback-to-old-readiness durations under
normal and slow-manager behavior. Keep current quiet/watch bounds; do not
trade correctness for a shorter update. Restore the agreed original lamp,
feed/trust and unit state afterward; exclude personal values from reports.

## What is not shown

This document is source design only. No new selection, activation, journal,
manager, timer, cut or device test has executed. No general carrier code or
capability allocation is changed. The settings-writer recovery protocol,
exact registry rows and each family's quiescence/readiness contract must
land and qualify before this later piece is enabled.
