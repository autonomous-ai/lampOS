# Body-push fixture cadence

Qualified base: `2747c72a709882a62d625d5bbb3bfbe3fd797581`, native x86_64
Mac with Rust 1.96. No runtime code changes.

The original fixture writes `body/hand` on every 2 ms scenario step although
the topic promises a refresh at least every 100 ms. The counter-only
[negative overlay](negative.patch) keeps that behavior. It reaches the
normal scripted mute and verified child cleanup, then fails the added
2–111 publication bound with **4,582 writes**. The [negative receipt](negative.json)
binds its source, raw log and retained executable; it is not a watchdog repro.

The correction publishes immediately on the first push and then every
100 ms. [Fixed qualification](fixed.json): formatting and strict all-target
Voice Clippy pass; both existing body-push tests and seven Hand reader
controls pass (**9/0**). The two tests contain pushed/unpushed paired runs:
publication counts **97, 0, 92, 0**. The original search cancellation,
authority, scripted mute and every-child Stop/exit assertions still pass.
Host watchdog and cleanup bounds are unchanged.

A separate reminder repair's frozen d21 full Voice run had 3,787 passes,
two watchdog failures and 15 ignored tests. Its retained binary's isolates
were 1/0 and 0/1; an older unmodified-production 3d binary passed the latter.
Those results remain distinct and failed where recorded. Reducing fixture
filesystem work does not prove the cause of every historical watchdog;
the reminder repair still needs its own full qualification.

No binary artifacts or helper scripts are committed. Full raw logs and
executables remain in the builder's private evidence directory. No device,
acoustic latency or physical hand result is claimed.
