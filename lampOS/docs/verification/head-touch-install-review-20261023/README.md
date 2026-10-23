# Head activation with an existing install drop-in

Independent review of #2948 at `f69bdea6466191f44ad5ec0aeecf50abe38c4a69`.
Intel Mac, Rust 1.96.0, isolated detached checkout and fresh Cargo target.
No production file was changed; the original install test file is an
unchanged 160,725-byte prefix. [Hashes](hashes.json) bind the exact source,
appended fixture and raw output.

## Reproduced P2: install admits conflicting effective head readers

`install` intentionally preserves local unit drop-ins, but this activation
removes `legacy_head` from custom pins based only on the embedded defaults.
It writes the product base units before that migration without checking
whether a preserved drop-in changes their effective head-reader modes.

The [two controls](regression.rs.txt) use the existing real installer with
its fake channel and host. Both begin with custom pins containing
`legacy_head:true` and an unrelated local marker. One preserves a benign
Voice environment variable; the other preserves:

```ini
[Service]
Environment=LAMP_HEAD_TOUCH=off
```

The benign control passes. The disabling control fails at the intended
admission assertion: install returns success, removes `legacy_head`, writes
the head-enabled Voice and Expression base units, and retains that exact
Voice override. Voice's effective head mode remains off beside the enabled
Expression reader. This violates the paired activation condition; the test
does not execute systemd or prove a particular runtime microphone state.

```sh
# Run from the isolated checkout's lampOS directory.
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/third-2948-install-196-target cargo test -q --locked --offline -p lamp-update --test install install_review_head_pair_ -- --test-threads=1 --nocapture
```

[Result](result.txt): **1 passed, 1 failed**, exit 101, 13.44 seconds of test
runtime; 81 unrelated install tests filtered out. No full gate or device
acceptance is claimed.

The first invocation used `--manifest-path lampOS/Cargo.toml` from the parent
repository root. That selected Rust 1.97.1 and missed lampOS's Cargo config;
Camera's build script refused its missing UAPI input before any test ran.
That [preparation failure](preparation-failure.txt) is retained separately.
The unchanged fixture was then run from `lampOS/` with the pinned
[toolchain](toolchain.txt) and a different fresh target as shown above.

## Scope and repair recommendation

On this carry, `install.rs:1306–1327` writes the product base units;
`:1373` invokes the migration, and `:1655–1661` removes the legacy bit.
The existing drop-in preservation test is at `tests/install.rs:535`; the
product pairing census at `:901` reads embedded units, not effective ones.
Base-unit replacement itself is intentional and is not the finding.

Check the resulting effective reader configuration before changing the head
units or pins. Refuse unknown or conflicting mode overrides without partial
head activation; leave local drop-ins untouched. The update-round guard in
#3269 provides the related reader checks but is not called by `install`.
The two new controls should remain alongside its round controls. A repair
may instead preserve a demonstrably coherent old configuration, but must
not silently admit the mixed pair reproduced here.

This is a separate P2 on #2948. Third's READ-OK on #3269 `46f5f384a` remains
for that prerequisite's corrected update-round scope. First owns the carry
and routes its repair; there is no competing product patch. No lamp access,
ARM64 linking, acoustic measurement or observed privacy failure occurred.
