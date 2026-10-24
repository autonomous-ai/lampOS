# Face-speaker guard: independent Linux formatting

Second, 24 October 2026, 09:06 +07. Requested on Queue #3126 by the author
of PR #3318. Exact source head `46c072c98c509f8a8357d8e27887dd9cda28cc2f`,
based on `4a84fa5ab`.

`cargo fmt --all --check` passed with exit 0 in an offline container using
the cached pinned image recorded in the receipt. All **6,967 regular source
files** had identical hashes before and after; the source was unwritable.
The container ran as UID/GID 65534 with no host mounts, network or devices,
a read-only root and private tmpfs. Its owner label, removal and absence were
verified. The isolated source worktree remained clean.

This is source-only Linux formatting, not compilation, Clippy, tests, native
ARM64 qualification, a privacy review or lamp evidence. Raw output and command,
source, image and cleanup bindings are retained beside this note.
