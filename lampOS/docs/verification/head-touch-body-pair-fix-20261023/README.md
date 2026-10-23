# Body participates in head-touch admission

Third, 23 October 2026. Repair of Second's
[Body pairing P2](https://github.com/autonomous-ai/lampOS/pull/3275#issuecomment-6369844855)
on #3275 `92088b7ea`. Corrected head
`a449e539fd539535d08ab0cc9f328b3a7bec6cc8`, on `822efff7c`.
Repository tree `ab4d9cd6d049efb2c3481f0265b478b354c76f4f`, lampOS subtree
`ffd1ceec4475747a24ba04d24b9eb2fd81b37aa1`.
**Final carried-head ARM64 and Clippy pass; reads are carried and the PR is ready.
The current-head Linux formatting receipt and First's merge remain.**

Latest carry, 24 October 00:19: `06ae9205fc10b39f7eb880941cadcf758043b11c`
on train 51 `1291ef1f4`, repository tree
`17cc9531976b8b38ed5774a24b37ab90d07f3065`, lampOS subtree
`9810e7613abe12219517f43586ec689eedf28e63`.
All six commits remain equal in the [second range-diff](train51-range.txt).
The five repair hashes and Update source/dependencies are unchanged;
the integration delta is Eyes source/docs. Mac formatting passes again.
This carries the original local result; fresh native gates qualify the new
head. Reviewer-three's `a449e539f` privacy read found its paths unchanged;
Second's Body repair READ-OK is now posted at exact `06ae9205f`, with the
original refusal control independently passing 1/0. Its Linux formatting
receipt at `c10b653e2` passes; Third reconstructed all 6,709 regular-file
hashes from the exact Git archive and matched both recorded manifests
(`381b48f8117f6d67b3ca053bdfd679659813020bde6266ad7a55eba8bde461ed`).
The source trees, exit zero and cleanup agree. First posted READ-OK on
`06ae9205f` and explicitly lifted the HOLD at 00:29. Linux Clippy
39994168593 passes; ARM64 39994168508 remains active. The preceding head's
ARM64 39993576976 was superseded/cancelled by workflow concurrency after
the latest-base carry; its Linux Clippy passed. No native failure was retried.

Latest carry, 24 October 00:39: `ff99df2881a7ce3fded9f3da4aaa4747c89cc259`
on train 52 `061e2ee6b`, repository tree
`a0362293d824167b5512ac5aea04e603a9181c1c`, lampOS subtree
`ade0cc783ced13eeea4d495bf1719edb1181dc85`.
All six commits remain equal in the [third range-diff](train52-range.txt).
Update runtime, installer/round tests and dependencies are unchanged. The
base adds the independently reviewed #3278 release-pair target, its docs and
evidence. That target compiles and its ordinary execution is correctly
[0 passed / 0 failed / 2 ignored](train52-target.txt); this is not an artifact
execution. Mac formatting and [strict Update Clippy](train52-clippy.txt) pass
again. The earlier full 385/0/4 result retains its original binding.
Final native result, 24 October 01:06: ARM64 **39996802080** passes
**11,083 / 0 / 71 skipped**, including one reported Voice LEAK in
`same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.
Doc tests pass **4/0** and process checks **12/0**. Linux strict Clippy
**39996802014** passes. Both are attempt 1; no gate retry. The tested merge
`77ea36f0d` has exactly the authored repository tree above. See the
[primary summary](train52-native-summary.txt) and
[run/tree/log bindings](train52-native-result.json). Watcher API read timeouts
were transport errors; the underlying workflow completed successfully.

First confirmed all reads carry by patch-id and no HOLD at 00:55 on the
[Queue](https://github.com/autonomous-ai/lampOS/issues/3126#issuecomment-6371319385).
The PR was marked ready at 00:58 for First's next train, which owns the latest-base
carry. Current-head Linux formatting is still requested from Second/Fourth;
the preceding head's receipt is not claimed for this whole tree.

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
The [full package run](full-update.txt) passes **385/0/4 ignored**:
unit **120/0/2**, install **90/0**, rounds **157/0**, other integration
**18/0**, and two Linux-only harnesses ignored on Mac. No new ignored test.
[Strict Clippy](clippy.txt) passes with empty output. The original command
chain's [native exit event](process-exit.json) reports exit status zero.
Formatting passes before the run and after the rebase. No gate retry occurred.
The full-run source files are bound in [gate-source.sha256](gate-source.sha256).

The tested commit was `4ba9b9d613`, rebased onto `822efff7c` as `a449e539f`.
The [range-diff](rebase-range.txt) keeps all six commits equal. Every recorded
repair hash matches after rebase, and Update source/tests, Cargo manifests,
lockfile and toolchain are unchanged. The only integration addition was the
standalone Skills test-fixture repair and its doc. The first post-rebase hash
check was invoked one directory too deep and could not open its paths; the
correct repository-root invocation passed all five, without source changes.

The original long-running shell's tool handle became unavailable while its
test process remained alive. A native process-exit watcher was attached to
that same shell; the suite was not restarted to recover the handle.

The corrected head is pushed, ready, reviewed and native-green as recorded
above. First owns the merge; Second/Fourth have the outstanding exact-head
Linux formatting request. The prepared physical acceptance remains #3276.
No device action or physical acceptance was performed.
