# Quiet-room fixture: controlled low-uptime counter

Second, 24 October 2026. Follow-up to PR #3312's independent ordinary ARM64
failure, run 40051251716. This is a scratch API counter, not full worker or
native qualification. It links an existing Mac Voice library without
rebuilding it. The receipt retains that library's stable hash and separately
binds the relevant source at the failed head and both bases.

With `rustc +1.96.0 --test -C overflow-checks=off`, `counter.rs` gives
**2 passed / 1 intended failure**, exit 101. The failing control reproduces
Unknown at injected uptime 300 seconds. The interval table demonstrates the
90–400 second failure boundary; the Quiet precondition via existing
`Kept`/`take_back` passes at all five controlled epochs.

The original runner did not record uptime. This proves a reachable mechanism
matching its failure, not the historical runner's measured cause. A later
ordinary-suite pass does not fix this dependency. The recommended fixture
correction is to establish the required Quiet state at the real epoch while
keeping live bus timestamps real. Clamping a shared clock can instead make
Eyes freshness wrong. The test-fix owner has the report on Queue #3126.

To reproduce against a compiled Voice library, supply its actual paths:

```sh
rustc +1.96.0 --edition=2024 --test -C overflow-checks=off counter.rs \
  --extern lamp_voice="$VOICE_RLIB" -L dependency="$VOICE_DEPS" -o counter
./counter --nocapture --test-threads=1
```

No lamp, source worktree or shared Cargo target was changed by the counter.
