# Capturing raw setting ownership

`crates/update/src/writer_settings.rs` is an unwired read-only prerequisite
for [#3689](https://github.com/autonomous-ai/lampOS/issues/3689). It gives
the future writer guard a bounded association between each ordinary setting,
its observed old bytes and the existing ownership decision. It is **not** an
authenticated, projected or executable apply plan.

## Existing decisions are reused

The ordinary `apply::plan` still reads and hashes the same paths and emits
the same steps. Its ordered decision table now lives in the internal pure
`observed_act` function. The caller supplies an optional hash; the function
reads no file and grants no path authority. The raw capture adapter uses
that same table with hashes of its descriptor-observed bytes.

This preserves build-owned writes/removals, custom-file retention, a
previously recorded local deletion, writing records, desired-value records,
omitted no-ops and the legacy unrecorded tmpfiles ownership fallback.
Ordinary planning still ignores a same-byte mode difference. Image-shipped
ownership remains supported by the pure classifier; the raw adapter does
not select or capture image operations.

## Bounded captured inputs

`capture` first bounds caller-constructed field lengths, setting count and
aggregate new text before hashing or constructing validation diagnostics.
It then checks the ordinary compiled path, mode, duplicate and content/hash
rules. It accepts at most 128 settings and 160 KiB of new text. Each selected
record value must be a lowercase SHA-256, or the existing set-aside marker
in the recorded map only. These are stricter adapter inputs; the ordinary
planner's existing input behavior is unchanged. Unrelated records are not
scanned, copied or defaulted.

The adapter uses the [file observer](writer-file-observation.md). Old bytes
are limited independently to 1 MiB per file and 4 MiB across the capture.
Each read's cap is the smaller of the per-file limit and remaining budget.
At zero remaining budget, an empty or absent file can still be captured;
a nonempty file refuses the entire capture. No successful result contains
a truncated prefix or a partial collection.

There is one entry per input, in order, including an ordinary no-op. An
entry borrows its setting and selected old records, owns the corresponding
observation, and exposes read-only access. Missing leaf and missing ancestor
both classify as absent but keep their distinct observations. Present
bytes, selected metadata and ancestor identities remain associated with
that exact path. Observed root identities must agree across entries.
Debug output and returned errors omit path values, contents and record
values; no state file or journal is read or written.

## Rechecking before a later effect

`RawOwnership::revalidate` re-observes every retained path. It requires the
same complete bytes, selected file metadata, ancestor identities and absence
kind, including the missing-ancestor index. A new empty file where no leaf
existed, or a newly created ancestor while the leaf is still absent, refuses.
So do same-byte inode, mode, ancestor or root replacements. The check uses
the observer's existing no-follow and nonregular-file refusal rules.

Each read is capped at the original file's byte count, or zero for an absent
entry. A growing file cannot expand the original 4 MiB budget. Failure
returns no partial successful recheck and leaves the original observations
and decisions intact. It does not re-plan, update ownership records, write a
journal or grant permission to mutate anything. An empty capture still
observes no root and establishes no root admission.

The method retains no live descriptors after it returns. The future executor
must establish and retain exclusion through the final mutation boundary;
calling this method early and then allowing another writer is insufficient.
This checks only the captured raw files. It does not revalidate projection
inputs, record files, signatures, service state or an entire apply plan.

## Limits before an executable plan

The caller must establish a trusted stable root and exclude writers. Rust
borrows protect the in-memory inputs, not the disk. Per-file checks and
matching root identities are not an atomic cross-file snapshot. No final
mutation boundary is wired. ACL/xattr capture and restoration remain absent.

This adapter does not verify signatures, update channel/sequence/refusal
admission, confirmed self-update or product-journal state. It does not run
head-touch projection, freeze its unit/drop-in/calibration dependency set,
coordinate Controls/Skills/Reset writers or admit a tmpfiles/reset/image
effect. A raw `Write` is not permission to execute it. Neither a candidate
executor nor a first-apply grant can consume this as a complete plan.

The next boundary must independently authenticate the exact operation,
freeze projection and bookkeeping inputs under durable exclusion, reject
unsupported effects, and retain independent recovery before any helper or
setting write. The separate record/admission foundation is
[#3775](https://github.com/autonomous-ai/lampOS/pull/3775); receipt identity
checking in [#3808](https://github.com/autonomous-ai/lampOS/pull/3808) does
not authenticate a helper or validate its claimed paths/postimages.

## Verification

Private filesystem controls compare captured ordinary decisions with the
normal planner while checking explicit outcomes, retained no-ops, absence
and mode evidence. They cover exact/exceeded old-byte budgets, zero-budget
empty/missing inputs, selected invalid versus unrelated large records,
bounded/redacted new inputs, and tmpfiles/image-shipped precedence. The
observer's separate controls cover descriptor traversal and substitutions.
Recheck controls retain unchanged and exact-budget captures, refuse changed
bytes, growth, mode, deletion, same-byte replacements, changed absences,
ancestors, roots, symlinks and directories, and retain redacted diagnostics.
Local and native results belong to their actual source and scope. No
privileged execution, on-lamp GOOD/BAD, reboot or power cut is established
by these fixtures.

An empty input produces no file/root observation and proves no root admission.
The planner comparison shares the extracted table; explicit action and
precedence assertions provide separate expectations. No old-code runtime
experiment is claimed for this behavior-preserving extraction.
