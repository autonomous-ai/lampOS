# Saved inputs for head-touch replay

Part of #3689. The private `head_touch::capture::codec` module saves the
observations made by the [bounded collector](head-touch-capture.md). It
does not change ordinary projection, the collector's native identity
comparison or the settings writer. No storage, command, service or update
path calls it yet.

`encode(&FrozenInputs)` returns bounded JSON bytes. `decode(bytes,
expected_sha256, expected_size)` checks the exact external size and SHA-256
before parsing and returns a separate immutable `SavedInputs`. A matching
digest is an integrity check, not authentication of the input, root or
intent. The decoder neither reconstructs native `Stat` values nor reads
the filesystem. It retains the complete original bytes, including accepted
future metadata and whitespace; `as_bytes()` returns those exact bytes.
There is no mutation or re-encoding API for a decoded value.

## Closed replay and explicit fresh comparison

`SavedInputs` implements the existing `HeadInputs` interface. Requested
lexical keys must have been captured for that method. An uncaptured key
refuses; it never becomes an invented absence or a live-filesystem lookup.
Text and directory APIs remain separate. The known root is a directory,
not a text fact; a captured root listing must be present and carry its
single bound root-directory witness. A key present in both APIs must
represent the same missing fact and witness chain.

Regular UTF-8 text, empty regular text, actual null masks, missing leaves
and missing-ancestor suffixes retain distinct typed facts. Empty regular
text and a null mask both give the evaluator empty text without becoming
the same observation. Missing directories remain distinct from present
empty directories. All directory names, including ignored names, retain
their original enumeration order. Paths, names, link targets and unresolved
suffixes use exact raw OS bytes through base64; no lossy conversion or
path normalization is used. The existing lexical-key restrictions still
apply, while unresolved link suffixes may retain `.` and `..`.

The portable stamps retain every field selected by the collector's native
identity equality: device, inode, mode/type, link count, uid, gid, rdev,
size and mtime/ctime with nanoseconds. Atime and its nanoseconds are excluded.
Signed device words and timestamps are preserved with checked portable
widths. Unsupported ranges, mode bits and nanoseconds refuse uniformly.
Raw null-device metadata is retained; decoding never opens a device.

`SavedInputs::revalidate(root)` is a separate, explicit fresh read-only
comparison. It requires the same root spelling and identity, then compares
each saved text and directory observation with a new bounded collector.
Directory membership compares as a set, while replay keeps saved order.
This comparison does not run during decode or replay. Trusted ancestry,
root ownership, excluded writers and final-boundary use remain caller
responsibilities; a successful comparison is not an atomic snapshot,
authentication or permission to write.

## Schema and resource limits

Schema 1 uses operation `head_inputs`. Unknown schema, operation or fact
kind is refused. Duplicate known fields, including escaped spellings and
nullable fields, are refused. Additive nonsemantic fields are ignored only
within the same global bounds, and remain present in the original encoded
bytes. Whole-document UTF-8 is checked before any parser or skipped-field
handling. A bounds-only lexical scan precedes streaming semantic decoding;
Serde remains the authoritative JSON parser, and trailing documents refuse.

Schema 1 accepts producer limits no larger than the following caps. A
future producer-default increase does not silently widen this format.

| Resource | Maximum |
| --- | --- |
| One regular text | 1 MiB |
| Aggregate recorded regular text | 4 MiB |
| Text and directory facts combined | 256 |
| Names in one directory | 128 |
| Aggregate raw name bytes | 64 KiB |
| One requested path or link target | 4,096 bytes |
| Requested/expanded components | 64 |
| Link hops per lookup | 16 |
| Producer lookup/resolution work | 16,384 |
| Aggregate saved witnesses | 8,192 |
| Aggregate saved names | 8,192 |
| Aggregate raw path/name/link/suffix bytes | 2 MiB |
| Encoded document | 16 MiB |
| JSON nesting depth | 32 |
| Counted JSON starts | 524,288 |
| One lexical JSON string's contents before unescaping | 2 MiB |
| One lexical JSON primitive | 128 bytes |

The lexical counter counts string, container and primitive starts, including
field-name strings. It excludes separators and closing delimiters; it is
not a count of every JSON grammar token. String limits count encoded
content bytes before unescaping, excluding the surrounding quotation marks.

Missing suffixes also retain the bounded expanded-text allowance of at
most `17 * 4,096` bytes. The directory/member check mirrors the producer's
conservative `parent bytes + 1 + name bytes` cap even for root `/`; it does
not loosen that check based on `Path::join` behavior. Ignored names consume
budgets. Streaming seeds enforce collection and decoded-byte quotas before
growing a collection or allocating a base64 output buffer. Encoding checks
the facts first and writes through an output cap. Conservative refusal is
allowed; successful capture does not promise successful encoding.

These portable observations are data, not a serialized native filesystem
handle, signed plan, durable publication receipt or restoration grant.
They do not authenticate carried units, signed settings, raw before-images
or derived output, and do not prove that a callback evaluated a complete
plan. A future authenticated plan must bind all of those inputs and replay
the unchanged evaluator against this closed provider.

## Validation boundaries

The private source controls exercise real capture followed by removal of
the original filesystem, typed absences and null masks, aliases and raw
bytes, malformed or substituted input, exact byte retention, semantic and
shape refusals, and quotas checked before allocation. Evaluator replay,
fresh revalidation and resource bounds have distinct controls. Their actual
execution heads and outcomes belong in the PR and immutable verification
evidence; this protocol document assigns no test or native qualification.

Portable synthetic raw-name facts do not establish physical raw-name
support on a Mac filesystem. The Linux physical subsection is separate.
No codec result establishes authenticated admission, live restoration,
guard bootstrap, helper installation or attended power-cut acceptance.
