# Private ordinary before-images

`writer_preimages` is an unwired prerequisite for #3689. It turns an existing
raw ownership capture into immutable owned bytes for future independent
recovery. It authenticates nothing, grants no apply admission and performs
no live-setting write, restore, helper copy, unit change or bootstrap.

`Bundle::from_capture(transaction, capture)` keeps every selected ordinary
path, including Keep and omitted no-op entries. Each keeps its full old
observation: binary bytes or explicit missing leaf/missing ancestor index,
all ancestor identities and selected Unix metadata, and the selected recorded
and in-progress writing values. Desired text, action authorization, image,
projection, reset and service effects are not an executable plan in this
format. The transaction must be 64 lowercase hexadecimal characters; its
freshness and provenance are the future authenticated journal caller's duty.

The version-one JSON operation is `ordinary_preimages`. The three entry kinds
are `present`, `missing_leaf` and `missing_ancestor`. Unknown semantic versions,
operations or kinds are refused. Known duplicate fields and duplicate paths
are refused. Added nonsemantic fields can be ignored by typed validation,
but the bundle retains the **entire original encoded byte sequence**, including
whitespace and unknown values. Read-only accessors expose the selected input;
there is no mutation or reserialization API. Selected record values are a
fixed two-element array: recorded first, writing second; explicit null means
no selected record. Only recorded may contain the existing set-aside marker.

Bounds apply to writing and reading: 128 entries, compiled ordinary paths of
at most 511 bytes and 64 components, at most 64 ancestor stamps, 1 MiB old
contents per file, 4 MiB aggregate old contents, and 16 MiB encoded artifact.
Streaming serde sequence/text/tag visitors skip future fields without internally tagged
Content buffering, and reject overlong typed collections before
base64 decoding. All claimed old-byte budgets, stamp types/timestamps, root
agreement and missing-index/ancestor shapes are checked before allocating any
decode buffer. Base64 encoded length must match the claimed old length; the
library's conservative temporary decode buffer adds at most two bytes per
file. JSON escape scratch space and ignored future metadata are bounded by
the whole artifact limit and serde's nesting limit. No unbounded allocation
or semantic extension is inferred from a future field.

`Bundle::decode` checks the expected SHA-256 and **exact size before parsing**.
These use `writer_refresh::Artifact` as a content identity, not a signature
proof. The expected identity and transaction must come from independently
authenticated durable intent; computing a hash of supplied bytes does not
provide that authority. Loaded data is read-only; Debug reports counts and
errors use fixed text without JSON, paths, record values or contents.

`Store::open` opens the existing directory leaf without following a link,
requires mode 0700 and the effective caller's ownership, and retains its
descriptor. The caller must establish trusted ancestry, a stable directory
and exclusive coordination; these leaf checks do not bootstrap root trust
or prevent malicious races when those preconditions fail.

Publication has only two fixed names inside that descriptor:
`writer-preimages.json` and `.writer-preimages.stage`. It refuses an existing
final or stage of **any type**, never replaces, deletes, retries, sets aside
or defaults either one. A newly created mode-0600 stage is written whole and
file-synced and its directory synced, atomically renamed with no-replace semantics, then the parent
directory is synced before success. Linux and Apple use Rustix's existing
`renameat_with(NOREPLACE)` wrapper. The live setting paths inside the payload
are data only and are never opened by this store.

A failed publication acknowledgement grants nothing. Before rename it leaves
the stage (possibly partial); neither publish nor load cleans it up. After
rename, even if directory sync or the final acknowledgement fails, it leaves
the final bytes. A later expected-identity load can validate those bytes but
is not apply admission, a new durability acknowledgement, or proof that an earlier
acknowledgement succeeded. The future caller must reconcile and sync/journal
before any grant.
Unknown staging bytes require a separate future reconciliation policy. Load
refuses a stage collision and known nonregular, linked, wrong-owner/mode or
wrong-size final files before opening; descriptor metadata must remain stable
through a bounded read. No damaged file is removed or automatically replaced.

The tests are private filesystem fixtures, not Linux/systemd or lamp acceptance.
They cover actual raw binary/empty/missing/no-op captures, selected records,
exact byte budgets, encoding/semantic/stamp/refusal cases, unchanged files,
private collisions, outside symlink witnesses and durable publication cuts.
No ACL/xattr preservation, original inode/timestamp reconstruction, candidate
independence, root guard activation or exact-restoration claim is made. Publishing
in the private bundle directory may change its metadata: this does not solve
revalidation after the guard's own later live-path staging or rename.
