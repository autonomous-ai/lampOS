# Read-only setting-file observations

`crates/update/src/writer_files.rs` is an unwired prerequisite for
[#3689](https://github.com/autonomous-ai/lampOS/issues/3689). It observes
one file for a future independent writer guard. No CLI, round or service
calls it; this change installs no helper and writes or restores no file.

## What exists and what we reuse

V1's `scripts/provision/apply-overrides.py` refuses symlinked or nonregular
top-level device JSON inputs and prepares its outputs before writing.
Keep that preflight intent. It does not provide this descriptor-rooted
observation contract, so this is not a V1 parity or restoration claim.

Reuse the existing rustix dependency and the `openat`/`DIRECTORY`/
`NOFOLLOW` pattern in `crates/setup/src/reset.rs`. Reuse Update's
same-descriptor metadata check from `head_touch.rs`, with a distinct
policy: the existing head-pair reader still accepts regular symlinks and
actual null-device masks; this new observer accepts neither. Existing
readers and reset behavior remain unchanged.

## Contract

`observe(root, relative, cap)` requires a trusted, stable, caller-owned
root and exclusive coordination with writers. It opens the root's final
component with `DIRECTORY | NOFOLLOW`, then opens each validated relative
ancestor through the preceding directory descriptor. It does not verify
the root path's ancestry, ownership or the caller's authority.

- Relative paths contain at most 4,096 bytes and 64 components. Absolute,
  empty, dot, dot-dot and NUL-containing components refuse. At most 64
  directory descriptors plus the leaf remain open during observation.
- A no-follow descriptor-relative stat refuses known nonregular,
  hard-linked or oversized leaves before opening them. The leaf open uses
  `RDONLY | NOFOLLOW | NONBLOCK | CLOEXEC`. Its descriptor metadata must
  match the pre-open stat before reading; no known device is opened just
  to discover its type.
- The caller supplies a content cap of at most 1 MiB; zero permits only
  an empty file. The read buffer holds at most one byte beyond the
  validated initial length, which is already within that cap. Success
  returns complete bytes, never a truncated prefix.
- Descriptor identity, link count, length, mode, uid/gid and mtime/ctime
  must agree before and after reading. Access time is excluded because
  the read may advance it. Existing ancestor descriptor metadata is
  checked again before returning an observation.
- Present, missing leaf and missing ancestor are distinct results. A
  missing ancestor's zero-based component index identifies the unresolved
  component and its entire suffix. Returned ancestor identities are root
  first, then the existing relative prefix; their count is that index
  plus one. Later components were not inspected. The caller must bind
  this result to the validated requested path in its own plan.
- Errors use fixed messages and error categories. Debug output omits
  filenames, path values and file contents.

The observation confers no permission to change a path. It captures no
ACLs, xattrs or other restoration policy, and cannot prove an exact
restore. Metadata comparisons detect observed changes; they do not make a
race-free snapshot or eliminate replacement/device-open races when the
exclusion precondition fails. Regular-file kernel and storage operations
have no wall-clock deadline here. All descriptors close on return.

Ancestor comparisons intentionally use the full selected identity, including
directory length, link count, mtime and ctime. An unrelated entry change in
`/`, `/etc` or another observed parent can therefore refuse the capture or
recheck even when the selected leaf is unchanged. This conservative refusal
does not establish that a live lamp can keep every parent stable. The future
guard must qualify its writer coordination and refusal rate before wiring
this API; private fixtures do not justify weakening those identity checks.

## Verification and limits

Ten authored controls cover regular/binary/empty contents, missing suffix
identities, invalid path/resource bounds, symlink and hard-link refusal,
directory/FIFO refusal before opening, deterministic pre-open replacement,
post-read metadata changes and redacted diagnostics. The FIFO fixture has
a panic-safe local unblock/join guard. These are private filesystem
fixtures, not tests against a lamp's settings.

Local gate results belong to their exact source and are recorded in the
change's evidence. Native Linux ARM64, real privileged guard bootstrap,
before-image persistence, restoration, root ancestry/ownership admission
and an attended 7071 GOOD/BAD rehearsal remain separate work. This module
does not close any of those acceptance rows.
