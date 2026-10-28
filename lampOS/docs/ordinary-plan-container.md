# Saved ordinary plan bytes

This private, unwired container saves a successfully prepared ordinary-settings
plan as bounded bytes and verifies those bytes again on reopening. It performs
no filesystem or State read, publication, fsync, journal admission, execution,
restoration or acknowledgement. A returned container is finite checked data;
it does not make an apply operation safe to start.

## Exact inputs and independently protected expectations

Saving retains the original signed envelope, ordinary preimage encoding and
Head-input encoding without reserializing them. Additive metadata and lexical
spelling in those encodings remain byte-for-byte intact. The envelope already
contains the exact signed document; a second mutable manifest is unnecessary.

A known-field context snapshot records the apply transaction, envelope identity,
channel, sequence, commit, full trusted-key SHA-256, known refresh identities,
product-before identity and selected sequence/installed/refusal values, every
ordered selected recorded/writing value, preimage and Head identities, raw
root/unit path bytes, ordinary policy and derivation version. Unknown refresh
metadata is not copied. Artifact equality means SHA-256 plus size.

Reopening requires the expected plan identity, original protected context,
trusted public key and offered writer bytes from outside the container. Its
own context snapshot or hash cannot authenticate itself. In particular, the
original selected product records must remain independently available across
restart. Reconstructing them from mutable post-apply State changes the facts
being verified and is unsupported. Authentication and extraction of that
protected recovery context remain future parent work.

## Complete decisions and exact projection order

Every signed row is saved in signed order with its index, explicit initial
omitted/Write/Remove/Keep/Record decision and optional complete effective
Setting/action. Keep reasons and actual effective text are retained. Original
signed PINS and projected PINS remain separate. A separate order vector retains
an appended PINS row after a later signed row; sorting by signed index would
change this plan and is refused.

Reopening runs the existing pure preparation against the exact frozen inputs.
It compares a bounded deterministic encoding of the protected context and all
recomputed decisions/order with the corresponding saved sections. No context
or row table is accepted into an unbounded object parser. A changed action,
reason, mode, text, hash, index, omission or order refuses even if someone
recomputes the outer artifact identity. Content-change status is derived from
verified decisions, rather than trusted from the wire.

A future executor must consume the exact checked plan/order; it must not
substitute a fresh plan from live files. The checked type exposes no execution
or admission method. Policy changes that alter the deterministic result refuse
under this derivation until an explicit compatibility protocol exists.

## Bounded framing and forward compatibility

The framing has a fixed magic, semantic version and derivation, followed by six
mandatory ordered sections: context, envelope, preimages, Head inputs, rows and
projection order. Lengths use checked conversion, sums and slice boundaries.
The whole framing is checked before nested decoding or payload copies.
The header is `LAMPPLAN`, followed by little-endian u32 version, derivation
and section count. Each section is a u32 tag and u64 byte length followed
by that payload; tags 1 through 6 name the mandatory order above. Only tags
with bit 31 set may follow as optional metadata, at most sixteen unique tags.
A mandatory payload cannot be empty; an optional payload may be empty.
Context and decision scalar tags/lengths use one fixed deterministic layout;
they are verified by comparison, never independently interpreted as authority.

Conservative bounds are 40 MiB total; 128 KiB context; 512 KiB envelope;
16 MiB each for preimages and Head; 2 MiB rows; 1 KiB order; and 64 KiB aggregate
optional metadata. At most 128 signed rows and order indices are represented.
Each encoder append checks its remaining budget before reserving. Refusal
returns no partial container. These are format admission bounds, not a measured
allocator RSS or a claim that every future producer output will fit.

Unknown semantic versions, derivations, actions or mandatory section tags,
duplicate tags, reordered mandatory sections and trailing malformed framing
refuse. A designated optional nonsemantic tag range may carry bounded opaque
future metadata; accepted bytes are retained in the complete original encoding.
Such metadata cannot change a known context, decision or authority boundary.

## Authority and recovery still required

The plan identity is calculated only after encoding: prepare, encode, obtain
artifact identity, then a future caller may publish durably and bind it in
FirstApply. The plan contains neither its own hash nor a grant, avoiding a hash
cycle. Encoding/reopening supply no durability receipt.

A matching signature and plan hash do not confine a privileged executable.
Stable root ancestry, excluded writers, protected-context provenance, fresh
journal admission, trustworthy exact-plan consumption, independent restoration,
afterimage ownership, retained compatible writers, root bootstrap and attended
power cuts remain separate #3689 work. Ordinary update callers, allowlists,
feature level, switches and defaults remain unchanged.
