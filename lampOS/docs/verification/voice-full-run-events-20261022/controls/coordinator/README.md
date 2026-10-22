# Requested-audio coordinator host-stall control

The unchanged e2232b836 fixture publishes a real SpeakerState with the
production three-second wire freshness. In the named cancelled/gapped/expired
news test, hold the test thread for four seconds immediately after Test::new
and before load. The independently scheduled real monitor sees the old state
expire; the original accept_offer assertion fails (exit 101, 4.25 s in
libtest). The full unmutated run at the same base passed this test.

This is a controlled scheduling/clock-domain failure, not proof that Blue's
or the earlier wait_ready failure had this exact cause. A separately scheduled
monitor can also hit its intentional fail-closed cache-contention path; the
runtime's existing contention-cancels tests keep checking that product rule.

The before patch, exact commands/exits/digests, raw failure output, source
hashes and byte-for-byte restoration are beside this file. The raw output's
last END line is a wrapper; its digest records the original bytes.

The text rendering trims libtest's trailing progress-line space; the exact
original bytes and digest remain in before-host-pause.raw.json.

The identical four-second idle pause passes after the stepped fixture: all
five stop/new-owner/privacy/gap/expiry branches finish their original checks,
1 passed, 0 failed, 21.13 s in libtest. The remaining 11 coordinator recording
cases pass (0.28 s), and all 18 requested-audio runtime tests pass (0.22 s),
including original expiry and contention-cancels cases. No test was retried.
The after control's source was restored exactly before final gates.
