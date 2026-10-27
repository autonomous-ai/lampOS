# Agreement between saved Head inputs and ordinary preimages

This private finite-plan prerequisite compares already decoded saved artifacts.
It performs no filesystem read, serialization, write, authentication, freshness
check or admission. Ordinary update callers and both wire formats are unchanged.

`SavedInputs::check_preimages(root, bundle)` returns unit or one fixed,
value-free refusal. It requires the supplied root's exact OS-byte spelling to
match the saved Head header. An empty preimage bundle is refused because it
contains no ordinary root observation. For every preimage, including a path
with no Head leaf request, its root identity must match the Head root's common
selected fields. Bundle does not retain historical root spelling: a future
protected caller must bind both artifacts to the intended logical root.

Both formats already bound and validate canonical requested paths. The check
compares every represented common lexical prefix as a directory, regular file
or absence. Directory and regular identities compare device, inode, mode,
link count, owner, group, length and modified/changed seconds and nanoseconds.
Device and length conversions are checked; a negative Head device word is
refused rather than bitcast to an unsigned identity. Access time is omitted
by both formats. Raw retains no `rdev`, so it is not invented or compared.
Overlapping regular files require identical bytes and the ordinary single-link
identity. Bytes are never decoded lossily or treated as an ownership grant.

A missing leaf remains distinct from a missing ancestor. The latter must have
the exact canonical unresolved suffix and matching existing prefix. An existing
Head directory cannot describe an ordinary regular or absent target. A complete
captured directory listing also checks whether the next represented ordinary
component exists or is the first missing component. A name alone never proves
its kind. This does not require a Head key for an unrelated ordinary leaf.

The first version compares no-link witness chains only when they have the
exact canonical prefix count. Before a represented link, its common directory
prefix can still agree. The first link must name an actual lexical component,
not an extra witness beyond the leaf. Overlap reaching that link is refused: this check does
not resolve links, discover physical aliases or map a null/device mask onto an
ordinary file. Valid unrelated link and null requests retain their existing
closed replay semantics. Unsupported mapping is conservative plan ineligibility,
not a new restriction on ordinary Head reads or saved-input decoding.

The check retains both complete original encodings on success and refusal.
Its work is bounded by the existing finite entry, key and component limits;
it does not accumulate file text or introduce an unbounded resolver. Agreement
between represented lexical observations is not a simultaneous snapshot or
proof of writer exclusion. Comparisons cross the two artifacts; this does not
validate every pair of rows within Bundle or discover global physical aliases. Protected expected hashes, signature/context checks,
complete plan rows, durable publication, live admission, independent restoration
and root bootstrap remain separate work under #3689. No runtime caller, grant,
capability, helper, unit, path authority or effect is enabled here.
