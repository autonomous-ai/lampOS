# Head-touch settings transition review, 23 October 2026

Exact head: `f69bdea6466191f44ad5ec0aeecf50abe38c4a69`.
Old units and pins: `efc8db9cb`, byte-identical to its product files.
Intel macOS; no device action or production edits.

The real update round and `apply::run` execute with the existing round
fixture's channel, systemd and binary mocks. Settings-off preserves the old
head reader (pass). Settings-on reaches `Switched(2)`, records the new pins
hash and restarts Controls, but removes `legacy_head` while keeping the
old linked product units without Voice/Expression head-touch switches
(fail). This conditional P2 is present in the original patch too, not
introduced by its rebase. The product's settings switch is unset by default.

## Reproduce

In a disposable checkout of the exact head:

1. Copy [regression.rs.txt](regression.rs.txt) to
   `lampOS/crates/update/tests/round/head_touch_carry_review.rs`.
2. Append `mod head_touch_carry_review;` to
   `lampOS/crates/update/tests/round.rs`.
3. Create `lampOS/crates/update/tests/fixtures/pr2948-before/`. For each
   of `lamp-voice.service`, `lamp-expression.service`,
   `lamp-controls.service` and `lamp-body.service`, place the output of
   `git show efc8db9cb:lampOS/deploy/product/<file>` in that folder.
   Add `controls-pins.json` from
   `git show efc8db9cb:lampOS/deploy/product/settings/controls-pins.json`.
4. From `lampOS`, run the command below.

```sh
CARGO_BUILD_JOBS=2 cargo test -q --offline -p lamp-update --test round pr2948_settings_ -- --nocapture
```

[Result](result.txt): 1 passed, 1 failed, exit 101. The availability
assertion is the only failure. No ARM64 linking or hardware test was run.
Migrate settings and reader units coherently; retain both transition
controls. The [transition audit](transition-read.md) refines the predicates:
either new reader excludes legacy, and Expression head sleep requires
Voice head plus durable mute. Partial readers are not an old-reader state.


## Recovery controls added at the same head

[Extended fixture](recovery-regression.rs.txt) preserves the original two
tests and adds Controls-crash rollback and interrupted-reload recovery.
Both require byte-exact old pins and their recorded settings hashes; the
interrupted round recovers without the channel.

The [first run](recovery-stale-build.txt) failed both cases before recovery:
the shared build target's compiled product pins equaled the old fixture,
despite the checked-out product template differing. This is an invalid
source/build match, not a recovery failure. Cwd and workspace were the
correct detached checkout; dependency fingerprints named relative source
and include paths, with timestamps newer than those checkout inputs.

Invalidating only the Update library source timestamp forced recompilation;
the source bytes and fixtures were unchanged. The [corrected run](recovery-rebuilt.txt)
records source/input hashes and its exact command. It reports **3 passed,
1 failed**: both new recovery cases and settings-off passed; the unchanged
settings-on case still failed at the missing-reader assertion. The expected
interrupted-reload panic is caught. Both invocations exited 101 and are
retained; this is not a green gate or a claim against Second's upcoming fix.

A corrected implementation may keep the pins bytes unchanged for the old
readers. To exercise its transformed-byte rollback path, a future fixture
must explicitly request a valid unrelated pins change as well; the current
cut control deliberately requires a write before the cut and must not be
weakened into a no-op recovery test. Use a checkout-specific target directory
for the incoming fix's gate to prevent another cross-checkout cache mix.

This remains a focused host review fixture with mocked systemd/binaries,
not an ARM64 or lamp gate.
