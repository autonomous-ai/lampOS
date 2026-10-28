# Original product ownership facts for a saved ordinary plan

This private, unwired projection reads independently retained **present raw
State bytes**, verifies their exact external SHA-256 and size, and extracts
only the facts ordinary preparation needs. It never reads a live file, calls
Records::state or peek_state, substitutes defaults after an error, publishes
bytes, issues an apply grant or changes recovery disposition.

## Original bytes and the caller's trust

`OriginalProductBefore::from_present` retains one immutable complete encoding
and its computed identity after matching the externally protected identity.
This first function establishes only bounded byte retention; JSON validity and
selected facts are checked by `select`, and no usable selection escapes a
failed parse. Hash agreement does not establish the source's provenance.

The caller must independently protect the raw-present profile and identity,
which must match the refresh binding's product-before identity. Empty bytes,
missing input and a corrupt present file cannot become an inferred empty State.
There is no missing-file variant or new framing added to the existing binding.
A future typed absence protocol requires its own explicit contract.

The full original encoding is retained, including whitespace, unselected paths
and unknown metadata. A later signed first-apply release using the same writer
may name paths the refresh envelope did not contain; projection still uses
those original bytes. Mutable post-apply State and the saved plan's own context
cannot supply or replace that trust. Capture under the product lock, writer
exclusion, immutable storage, restart authentication and fresh admission remain
parent responsibilities. A refresh guard lock alone is not the product lock.

## Selected facts without a second State loader

`select` first validates a nonzero requested target sequence and one through
128 distinct compiled setting paths in their requested order. Every path is
at most 511 bytes and 64 components. The complete request is checked before
copying any path. These are bounded request parameters: the existing signed
preparation must still authenticate the actual target/envelope and require
exact signed order and independent preimage agreement.

The projection then streams authoritative serde JSON parsing to completion,
including end-of-input. It extracts installed sequence, whether any original
refusal names the target sequence, and each selected recorded/writing value.
Missing selected entries remain absent; recorded SHA-256, the exact existing
set-aside marker, and writing-only SHA-256 remain distinct. It copies no whole
State, unrelated ownership map, refusal list or unknown Value tree.

| Current State field | Projection contract |
|---|---|
| installed | Missing/null is None. Otherwise require a Build object with u64 sequence and string commit. Retain sequence only, including Some(0). |
| refused | Required array of at most 32 complete Refusal-shaped entries. Require sequence/commit/why/at_unix_ms with their current primitive types, retain only target-sequence membership. |
| made | Required array of at most 4,096 strings, each at most 4,096 bytes. Validate shape, retain no folder authority. |
| settings | Missing is empty; null refuses. Require string map values. Selected values must be lowercase SHA-256 or the exact set-aside marker. |
| settings_writing | Missing is empty; null refuses. Selected values must be lowercase SHA-256. |
| previous, systemd_owed, models, models_before | Detect duplicate known fields, but skip their bounded JSON. Their field-specific validity and authority are outside this projection. |
| Other additive fields | Skip bounded values and retain the original complete encoding. They supply no projected authority. |

Known top-level fields, Build fields and Refusal fields cannot repeat, including
escaped equivalent names. Duplicate selected keys in either ownership map
refuse before last-wins behavior can change a fact. Unselected map keys and
unknown metadata need no unbounded duplicate-name set; repeated unselected
names remain outside the projected authority. Every visited entry still counts
toward its bound. State has no semantic schema/op: a future property with one
of those names is additive metadata here, not an invented supported operation.
No tagged or plain enum is widened.

This is deliberately **not full State validation** and not a replacement for
its ordinary loader. Additional conservative bounds can refuse a valid larger
State. Future effects needing ignored fields must validate those original
facts independently. Projection can retain a refused target or installed
sequence for inspection; existing preparation separately rejects an ineligible
target. The projection itself grants nothing.

## Bounded parsing and ownership

Whole source is at most 16 MiB. An allocation-free lexical pass bounds depth
32, 131,072 JSON starts, encoded string content 64 KiB and primitives 128 bytes.
It neither accepts JSON nor decodes escapes; serde remains authoritative for
syntax, UTF-8, primitive types and trailing data. Whole UTF-8 validation includes
opaque future metadata; this validates the original bytes, not every decoded
Unicode scalar. The pinned IgnoredAny path can retain a lone escaped UTF-16
surrogate in an ignored value, while typed strings/current State would refuse
it. Known keys and selected values use typed string readers. Opaque retention
is not full Value or State validation. Parser escape scratch stays within the
lexical bound.

Known/map keys are at most 4,096 decoded bytes. Top-level/Build/Refusal maps and
each ownership map admit at most 4,096 entries; made has at most 4,096 strings,
refused at most 32 entries. Counting seeds refuse an extra actual entry before
its key or payload is decoded. Fixed bitsets track known fields and at most
128 selected keys. Selected values are checked before fallible copies; output
vectors reserve fallibly after their caps. At most 128 selected path/value
triples are owned, in addition to one complete original input. These are
logical work/content bounds, not allocator RSS or real-time guarantees.

`OwnedProductSelection` borrows its immutable source and owns only selected
strings and sequence/refusal facts. A separate bounded vector of borrowed
SelectedRecords avoids a self-referential object. Constructing ProductSelection
requires the supplied vector to match every owned path/value in exact order.
The caller keeps source, owned projection, borrowed records and expected context
in separate locals through preparation/reopening. Errors keep the existing
fixed value-free refusal; Debug reports byte/record counts only.

## Recovery and remaining work

The source identity and all original inputs must remain protected across a
restart; source capture/publication and durability are not supplied here.
Reopening a saved plan checks data and does not reconstruct a live Permit.
Granted/Applying still require restoration; Qualified retains the qualified
writer for product recovery; terminal journals do not grant another apply.
The older BACK allowlist, compiled capability and ordinary update behavior are
unchanged. Privileged exact-plan consumption, restoration, root bootstrap,
7071 GOOD/BAD and attended power cuts remain separate #3689 work.
