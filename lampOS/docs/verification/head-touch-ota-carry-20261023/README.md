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
Retain legacy delivery until the paired readers are enabled, or migrate
settings and reader units together; retain both transition controls.
