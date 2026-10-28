# Pure ordinary-settings preparation

This private, unwired API prepares an in-memory result for a future settings
writer. It verifies a signed input against independently protected expectations
and derives ordinary setting decisions from saved observations. It does not
read State or the filesystem, fetch a build, execute a binary, grant an apply,
publish a plan, write a setting or acknowledge recovery.

## What the caller must supply

`ExpectedOrdinary` borrows the apply transaction, exact envelope identity,
channel, sequence, commit, full trusted-key SHA-256, refresh binding, product
selection, preimage and Head-input identities, exact logical root and installed
unit directory, ordinary-enabled policy and compiled derivation version.
Constructing this value does not authenticate its provenance. A future trusted
parent must supply it independently of staged requests and artifacts.

The product selection contains the product-before artifact identity, selected
sequence, installed sequence/refusal decision and ordered recorded/writing
values for every signed setting. Those values must be independently extracted
from protected product-before context. A matching snapshot hash alone does not
prove that a staged bundle's selected records came from that snapshot. This
API deliberately supplies no State parser or default for unrelated records.
The separate [original-product projection](ordinary-product-context.md) can
derive these selected facts from independently retained present raw-State
bytes; it is not a live loader, full State validator or source authenticator.

Known caller fields are checked before copying: lowercase digest/transaction
and commit widths, ordinary channel syntax, at most 128 selected records,
compiled setting paths and selected digest/removed-here values. Root and unit
paths retain canonical raw OS-byte spelling, at most 4,096 bytes and 64
components; the installed-unit path must be a strict descendant of the root.
Artifact widths/sizes use existing validators. Unknown refresh metadata is
neither cloned nor serialized; identity matching compares SHA-256 and size,
not derived equality that includes future metadata.

## Verification and supported input

The function requires exact envelope bytes, the complete supplied public-key
identity, matching preimage transaction/original bytes and matching original
Head bytes. It then calls the existing full `manifest::open_as` with the
compiled `UPDATER` (currently 6), retaining the exact decoded signed bytes.
Channel, sequence and commit must match the protected expectation. No partial
apply-manifest reader, fingerprint-only trust or caller-selected capability
level substitutes for signature verification.

The apply transaction must differ from the refresh transaction. Its sequence
cannot precede the refresh; at equal sequence its envelope identity must match.
A later sequence and different envelope may use the same writer. The protected
product selection must name the apply sequence, not be refused, and advance
the selected installed sequence where present. Freshness/uniqueness admission
by an authenticated journal remains separate from these consistency checks.

The initial supported manifest is deliberately narrow:

- Exactly one `lamp-update` file whose SHA-256 and size match the protected
  writer identity; the borrowed offered bytes must match as well. This verifies
  this buffer, not a root copy, running executable or writer capability.
- Nonempty ordinary settings in the current compiled allowlist. Existing modes
  and Write/Remove/Record/Keep/omitted ownership behavior are preserved.
- No units, other binaries, models, image entries, reset plan, tmpfiles trigger
  or skills-folder settings, even when one would appear to be a no-op. Their
  effects and undo protocols are not represented by this first API.

No allowlist, schema, wire enum, unit, switch, feature level or ordinary reader
is widened. Unsupported signed inputs are refused before deriving steps.

## Complete rows and deterministic projection

Signed settings, bundle entries and independently protected selected records
must have identical counts and ordered paths. Both selected record values must
match. Pairwise ordinary-prefix checks reject contradictory represented shared
directory identities or regular/absent shapes between bundle rows. The existing
[Head/preimage check](head-preimage-coherence.md) then compares cross-artifact
root, prefixes, overlapping bytes, typed absences and directory membership.
Neither check proves a simultaneous snapshot, exclusion or global alias facts.

The existing `apply::observed_act` receives the complete saved old-byte hash or
typed absence and only the selected recorded/writing maps. It does not reread
files or apply image-shipped ownership. Every signed row remains represented,
including omitted no-ops and Keep/Record rows.

Only the owned [bounded Head projector](head-touch-plan-output.md), followed by
the unchanged verifier, runs against the same closed `SavedInputs`. Uncaptured
keys refuse; no generic callback, live fallback or implicit revalidation exists.
The original signed Setting and signed bytes remain distinct from derived PINS
text/SHA. Derived PINS output is capped at 1 MiB including the newline; this is
not a new bound on all preexisting inputs or allocator RSS.

Reconciliation rejects foreign/duplicate destinations, dropped initial actions
and non-PINS changes. PINS may change only through the projector's sanctioned
text/SHA/action path, with path, mode and removal semantics retained. Rows stay
in signed order while `projected_order` retains the projector's actual order:
an initially omitted PINS row can be appended after a later signed row.
Content-change status is computed after projection. All-Keep/no-op/Record-only
results remain complete and do not acquire apply or qualification authority.

## Result and remaining work

`PreparedOrdinary` owns the exact envelope, signed bytes, parsed manifest,
preimages, saved Head facts, complete rows and projected order, while borrowing
the immutable protected context. The separate [saved-plan container](ordinary-plan-container.md)
can explicitly encode and recheck it; neither type is independently durable.
No partial prepared result escapes a failure. Errors have one fixed value-free
Display/Debug string; prepared Debug shows counts and content-change status.
Neither representation exposes paths, text, key material or nested errors.

Authenticated parent/context extraction, stable root ancestry and excluded
writers, durable publication and protected external plan-hash binding,
fresh journal grants, afterimage ownership, independent restoration, old-BACK
compatibility, root bootstrap and physical power cuts remain separate #3689
work. A future plan artifact is obtained after encoding/publication; it must
not contain its own hash or require a FirstApply grant to create those bytes.
