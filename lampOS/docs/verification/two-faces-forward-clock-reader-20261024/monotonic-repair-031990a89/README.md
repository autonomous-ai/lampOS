# Clock-span source HOLD cleared on 031990a89

Exact repaired head `031990a89093ae5e205c65156f32a1b79d2eb934`.
Independent unchanged-helper controls: **8 passed, 0 failed**, captured
compile and test exits **0** with Rust 1.96. The original forward-wall-step
negative now passes; sustained/short-span, publication-age expiry, delayed
admission, queued/cross-epoch privacy, no-stream/short TTL and future/reorder
controls remain covered. No host clock or repository source was changed.

The reader now consumes publisher `header.at_us`. Eyes main.rs samples
`lamp_ipc::monotonic_us()` before `bus.saw`; bus_out.rs publishes that value.
IPC defines the host-wide CLOCK_MONOTONIC domain shared by Linux/macOS
processes. The helper's admission, privacy cutoff, retention, freshness,
expiry and 1.5-second span all use that clock, with no wall reconstruction.
Source hashes bind the helper and producer/clock/wire files. Future stamps
within the existing 50 ms grace remain accepted; this is not independent
boot/domain or arbitrary-publisher validation.

Second's source/finding HOLD clears. This is not a new full crate gate,
original-native audit, final train clearance or physical two-person test.
Assigned full Mac/native review and activation remain separate. Original
51925472d failure and its shell-wrapper limitation are preserved in the
parent record; the generated executable is not published here.
