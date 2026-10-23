# PR #3204: exact-head Linux formatting check

`cargo fmt --all --check` passed once (exit 0) on Linux x86-64 for head
`418f618e8572392d1df09df07399d0ac2231b1c1`, parent
`498aca8a2c37e02dcdcde9e15d297cedc52040fc`. This fulfills the
[explicit formatting request](https://github.com/autonomous-ai/lampOS/issues/3126#issuecomment-6356959647)
for [PR #3204](https://github.com/autonomous-ai/lampOS/pull/3204).

The check ran from `/tmp/source/lampOS` in the existing rootless Linux
`amd64` image, with Rust/Cargo 1.96.0 and rustfmt 1.9.0-stable. The image
was pinned by ID, without a pull, network access, host source mounts or shared
build targets. Its root filesystem was read-only; the extracted source used a
private tmpfs. Full command arguments, timestamps and terminal exits are in
`commands.json`; the complete stdout and stderr streams are represented
losslessly as UTF-8 strings in `command-streams.json`.

The container command ran from 2026-10-23 04:20:07.834775 UTC to
04:20:24.573181 UTC, taking 16.738 seconds. That duration includes container
startup, toolchain identity checks, extraction and source hashing as well as
formatting. It is not an isolated rustfmt timing.

The input was the full tracked `lampOS` subtree from the immutable Git commit:
6,655 entries and 204,427,860 blob bytes, including docs, vendor files, Cargo
configuration and the toolchain file. No root Rust formatting configuration
was present. The root tree was
`e67ee12f4ab179e38f4b136d6f51afee62545db6`; the `lampOS` tree was
`ad6362c98db49f4da424a20e8b863fc1d5a6f1ac`.

The 210,288,640-byte Git archive had SHA-256
`c5114b2524daf70fd9355791857ba4dc53d976c5cf5b2d0a3f703173539aaebf`.
Its embedded Git commit ID matched the requested head, and its bytes and Git
source identity remained unchanged after the run. In-container manifests of
all regular-file bytes and symlink targets were byte-identical before and after,
with SHA-256
`0638248479bf4c70a0324c3ef8efa9a558050f287a676b78427b481bce5c2d4e`.
The command constructing those manifests is retained; their complete per-file
contents were transient. This proof does not cover inode metadata.

`source.json` also records the five authored file identities. The PR describes
its earlier carry as patch-identical, but that historical claim was not
independently recomputed: the earlier commit object was absent locally and the
preparatory `git range-diff` returned 128. No fetch, branch change or source edit
was needed to verify the exact current head. The separate documentation follow-up
#3207 is outside this receipt.

This was formatting verification only. No compilation, tests, ARM build, device
access or workflow rerun was performed. It does not establish behavior, test
coverage, merge readiness or correctness of another head.

The original image-inspect response remains external. `image-provenance.json`
contains only its image/platform/UID provenance projection and the original
response's byte count and SHA-256; author, label, history and account fields are
not published. Other published command streams are complete and unmodified.
The input archive and external Python runner are not bundled. `manifest.json`
indexes the publication files with byte counts and SHA-256 values.
