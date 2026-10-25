# Sustained two-face evidence on the current release base

Runtime merge `a497afbd1` preserves original authored commit `031990a89`
and both parents: the previous offered `d9fa4c79c` and current release base
`e1c69457a`. Its tree exactly equals the frozen tested index tree. The net
added/removed lines in all three original runtime/test paths remain equal.
The two docs intentionally differ: the requested cadence correction now
states 7/4/4 looks at 250/500/600 ms, and four 600 ms looks span 1.8 s.
The wall-clock claim is also bounded to the helper's monotonic span checks;
the bus still applies wall-clock expiry before delivering a look.

Fresh native Mac formatting, strict all-target Body/Memory/Voice/Eval
clippy, workspace build and full Body/Memory/Voice/Eval passed
**5613/0/29 ignored**, 38 summaries.
All three own controls passed once, and all 7079 tracked
source hashes stayed fixed. Exact commands, UTC times, exits, literal summaries,
source identity, source-map hash and raw-log hashes accompany this report.

The prior author full 4,675/0/21 and original ARM 40206954233/Linux
40206954264 remain on d9/bf. The reader's same-source full **4,674/1/21**
is still failed: light_call's sorted index 10 was 25.66326 ms against strict
less than 25 ms, despite all twenty operations returning Done. Its same-binary
isolated 1/0 measured 3.5021 ms. Reviewer-two applies reviewer.md item1 for
the already tracked #3315 intermittent family; that review disposition does
not erase the failure, prove a load cause, fix runtime latency or establish
hardware acceptance. Earlier author failures remain recorded separately.

Self-read: the bounded sixteen-look history, monotonic span/freshness/order
checks, privacy epoch/cutoff and exact-on/default-off paths remain intact.
The changed signal makes a room busy; it assigns no caller identity and
does not widen personal memory. The landed late-verdict retained-likeness
and owner checks remain present. Stops still end carrying and intentionally
retain current room observations. No new combined late-verdict/two-face
scenario or hardware cadence is established by this source review. Direct
helper clock-step controls do not demonstrate live bus clock-step immunity.

Rust 1.96.0, native x86_64 Mac, one compiler job, serial tests, existing owned
cache, incremental off and zero development/test debug info. This is source
identity, not all-fresh binaries or pre-execution binary attestation. Current
original Linux gates remain separate. No product activation, lamp operation,
local ARM64 link or acoustic acceptance is claimed.
