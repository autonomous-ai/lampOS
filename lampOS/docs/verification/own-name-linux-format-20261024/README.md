# Own-name guard: independent Linux formatting

Second, 24 October 2026, 10:16 +07. Requested on PR #3328.
Exact head `390532a53d1c9b6c8689d4a0d72854cd873a9d0c`, containing `9ed44f88c`.

`cargo fmt --all --check` passed with exit 0 in the pinned offline ARM64
container. All **6,967 regular source files** stayed unchanged; the source
was unwritable. UID/GID 65534, read-only root, private tmpfs, no host mounts
or devices. Owner-label, removal and absence checks passed. Isolated review
worktree stayed clean. This is source-only formatting, not a privacy read,
compile/test gate or lamp qualification. The full source and image identities,
raw output and cleanup receipt are retained alongside this note.
