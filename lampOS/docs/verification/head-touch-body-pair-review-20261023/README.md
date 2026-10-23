# Body is missing from the installed head-touch bundle check

Second, 23 October 2026, 23:36 +07. **HOLD / P2** on #3275
`92088b7eaf1127cba12037399e7a8f7aef451846`, base
`be16c3327524ccef8a1908a3b8ebf40c16efba8a`.

The product unit enables Body's `LAMP_HEAD_TOUCH=on`, but
`head_touch::legacy` checks only Voice, Expression and Controls. Consequently
`preflight_install` never inspects a Body-specific local override. A retained
`/etc/systemd/system/lamp-body.service.d/30-local.conf` containing
`[Service]` and `Environment=LAMP_HEAD_TOUCH=off` is admitted alongside the
new Voice, Expression and product pins.

The additive control in `body-override.patch` fails **0 passed / 1 failed**,
exit 101, in 3.29 seconds. It observes successful install, the unchanged
Body-off drop-in, enabled Voice and Expression, and written product pins.
Its required refusal assertion fails. This is a temporary filesystem and fake
host result; no systemd or physical lamp was run.

The runtime consequence follows from `crates/body/src/bin/lamp-body.rs`
at the `SleepIn` construction: with this switch off, Body never creates the
reader of `voice/mute`. The head hold therefore lacks Body's immediate fold
and inhibition of new motion even while Voice mutes and Expression sleeps.
This does not establish a microphone privacy violation or a servo-limit bypass.

The original six installer head-pair controls pass **6 / 0 / 0**, exit 0,
in 3.22 seconds. No original test assertion or production file was changed.
The separate update-round check then passed **28 / 0 / 0**, exit 0, in
128.64 seconds (`round.txt`). Runtime and round-fixture source were unchanged;
the additive installer fixture is not compiled by that target. The Body P2
remains; these original cases do not test a Body-off override.

Commands, from the isolated exact-head `lampOS/` checkout, with Rust 1.96.0,
two build jobs and the existing Mac cache:

```sh
cargo +1.96.0 test -q --locked --offline -p lamp-update --test install -- install_review_head_pair_ head_touch --test-threads=2
cargo +1.96.0 test -q --locked --offline -p lamp-update --test install second_review_body_head_off_refuses_before_bundle_mutation -- --exact --nocapture
```

The round command was `cargo +1.96.0 test -q --locked --offline -p lamp-update --test round head_touch:: -- --test-threads=2`.

The first installer command ran before the additive patch. The second ran once after it.
`receipt.json` hashes both raw logs and the patch. The exact runtime and
existing fixture source remains the PR head.

Repair belongs in Third's carry: include Body's effective sleep-reader
configuration in admission, preserve the original failing control, and retain
a positive case for ordinary per-lamp Body calibration. The existing generic
reader parser cannot simply be applied to Body unchanged: the product Body
unit intentionally uses `EnvironmentFile`, `%H` and `$BODY_POSES`. Both install
and update admission need to preserve the complete bundle without guessing
unknown overrides. First retains final scope and merge disposition.

Other source-read results: the Voice/Expression/Controls override scan includes
`/usr/local/lib/systemd/system`; install checks precede state migration,
users, links, units and pins (but follow records-lock creation); candidate
unit/settings writes are journaled and verified before reader restart, with
rollback restoring files and restarting unchanged Controls when needed.
The shared cue constant is explicitly −20 dBFS, with its default assertion
and documented effects on tap, sleep and other hum paths. Existing ear evidence
is for taps, not all those paths. These observations do not clear the Body P2.
