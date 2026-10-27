# A read boundary for head-touch projection

The updater must pair Controls pins with the units that will read them.
`crates/update/src/head_touch.rs` keeps that existing policy and introduces
two internal read methods: text and lazy directory entries. Its ordinary
entry points still use the existing filesystem reader. This is a prerequisite
for [#3689](https://github.com/autonomous-ai/lampOS/issues/3689), not a frozen
settings plan or an executable writer guard.

## Existing behavior stays in one evaluator

Ordinary projection, verification, active-pins discovery and install
preflight use `OrdinaryInputs`. Text reads retain the current handling of
regular symlinks, missing/dangling paths and actual null-device masks.
Directory enumeration retains filesystem order and stays lazy: rejecting
an early override does not first consume a later entry error. Each returned
iterator owns its directory handle independently of the input provider and
closes it when dropped. No extra filesystem lookup or process is added.

The policy still scans a reader's drop-ins before selecting its carried or
installed base unit. Voice, Expression and Controls are checked in the same
order. Body and its recognized calibration files are read only when the
Voice head mode requires them. A signed exact-copy no-op can still become
a projected write; custom active pins remain read dependencies rather than
new write targets. Existing wrappers and callers keep their signatures.

## What a future closed reader must supply

`HeadInputs` is internal to Update. An implementation can return a recorded
text value or explicit absence, and an explicit absent or present directory
whose entries are yielded separately. A closed reader must refuse an unseen
key; it cannot silently interpret an uncaptured path as absent or fall back
to the live filesystem. Ignored entry names remain visible at the enumeration
boundary so a collector can bound all listing work before policy filtering.

Depending on Body, the existing policy visits 25 or 30 unique drop-in
directory keys across five bases; repeated lookups are preserved. Both
absent and present-empty directories matter to later revalidation. Carried
base text does not remove local drop-in dependencies. Actual active pins,
including a custom path, and the conditional Body calibration inputs also
belong to the future dependency ledger.

The interface carries evaluator values, not complete filesystem evidence.
A collector still needs bounded bytes/names/link resolution, typed regular
file/link/null-mask facts, ancestor and directory membership identities,
the null-device witness, exclusion and final-boundary revalidation. The
strict ordinary-setting observer cannot replace this reader directly:
existing compatible symlinks and merged-/usr directory aliases remain valid.

No collector, persistent snapshot, authority check, signature verification,
freshness proof, new allowlist, service, grant or restoration is implemented
here. Ordinary text reads keep their existing behavior and limits; this
refactor does not make them bounded snapshot reads. Frozen error handling
must redact supplied paths/contents at its boundary. This filesystem view
does not claim to reproduce all effective systemd manager configuration.

## Verification scope

Private fixtures compare the ordinary wrappers with a recording reader and
then replay the same evaluation after removing the fixture filesystem.
Eight combinations cover old/new head modes, omitted no-op/explicit-write
steps and carried/installed bases. They assert projected compatibility,
conditional Body dependencies, unique directory counts, missing versus
empty membership and identical lookup order. Separate controls check
uncaptured versus captured absence, lazy early-error ordering and existing
regular-link/null-mask/directory-alias behavior.

The replay backend exists only in tests. Those fixtures establish neither
snapshot freshness nor OS directory-failure injection or measured FD counts.
The ordinary path adds trait dispatch and a box for a present directory
iterator; no latency improvement is claimed. Native and device evidence
remain separately source-qualified.
