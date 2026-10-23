# Body participates in head-touch admission

Third, 23 October 2026. Repair of Second's
[Body pairing P2](https://github.com/autonomous-ai/lampOS/pull/3275#issuecomment-6369844855)
on #3275 `92088b7ea`. **Validation in progress; no corrected head qualified yet.**

## Failure retained

Applied the [original additive installer regression](original-regression.patch)
to the unchanged runtime. It failed **0 passed / 1 failed**, exit 101:
install succeeded beside a preserved Body head-off override, with Voice and
Expression enabled and product pins written. [Raw result](before.txt).
The regression's required-refusal assertions are retained in the repair.

## Repair boundaries

When the new Voice/Expression head pair is enabled, both installer and updater
admission now require Body's `LAMP_HEAD_TOUCH=on` and the same effective bus.
Body's sleep reader obtains that bus from `LAMP_BUS_ROOT` (empty or unset
selects the default), independently of `LAMP_BUS`. Legacy head-off operation
does not acquire a Body requirement.

The narrow Body reader preserves its known `%H` and `$BODY_POSES` command
substitutions and known product/generic calibration paths. It checks the
existing calibration bytes: comments, blank lines and literal single-line
double-quoted `BODY_POSES` assignments are accepted. Other assignments,
multiline/escaped forms, unknown environment files, relevant drop-ins and
multiple start commands are refused. The per-lamp calibration paths are
excluded from manifest settings; admission does not rewrite them. A new
unmeasured lamp may still lack its required calibration file, keeping Body
stopped as before.

The source audit checked Body's `SleepIn` construction and bus semantics,
factory-generated poses, blank-install behavior and the generic unit's two
optional calibration files. This is installation/update admission work,
not a physical fold, microphone privacy or servo-limit qualification.

## Checks

Intel Mac, Rust 1.96.0, from the source checkout's `lampOS/`, with
`CARGO_BUILD_JOBS=2` and
`CARGO_TARGET_DIR=/private/tmp/third-head-touch-install-target`:

```sh
cargo test -q --locked --offline -p lamp-update --test install second_review_body_head_off_refuses_before_bundle_mutation -- --exact --nocapture
cargo test -q --locked --offline -p lamp-update --test install -- second_review_body_ install_head_body install_review_head_pair_ head_touch --test-threads=2
cargo fmt --all --check
cargo test -q --locked --offline -p lamp-update -- --test-threads=2
cargo clippy -q --locked --offline -p lamp-update --all-targets -- -D warnings
```

The first command is the retained pre-repair failure. The focused installer
run passes **9/0/0** ([raw output](focused-install.txt)), including the
original failure, refusal before mutation, and preserved factory calibration.
The full package run and strict Clippy are pending. No gate retry has occurred.
The full-run source files are bound in [gate-source.sha256](gate-source.sha256).

The original long-running shell's tool handle became unavailable while its
test process remained alive. A native process-exit watcher was attached to
that same shell; the suite was not restarted to recover the handle.

Fresh Linux formatting, ARM64 and Clippy, Second's re-read, and First's
exact-head disposition remain required after the corrected head is pushed.
No device action or physical acceptance was performed.
