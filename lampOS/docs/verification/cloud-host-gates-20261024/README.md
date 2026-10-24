# Cloud provenance fixture host gates

Mac Rust 1.96 formatting, strict all-target Cloud Clippy and full Cloud tests
pass: **196/0/4 ignored**. Complete raw command logs and exits are retained.
New names occur only in this test module; no additional crate test is needed.
The ordinary tracked-source manifest omitted new untracked fixture files;
the six-file fixture/workflow binding was frozen during the gate and checked
after the tests, then matches committed source (verification README was
updated after the gate). It is not a complete contemporaneous source map.

Exact committed head `2ad786f91` then passed source-only Linux formatting
with 6,973 unchanged files and verified owned container removal. YAML parse,
nine extracted `bash -n` blocks and actionlint pass; shellcheck unavailable.
No local ARM64 build/test ran. Native fixture execution and ordinary ARM64
remain separate receipts. Private home prefixes are redacted with original
and published hashes recorded; no result is removed.
