# Independent current Cloud acknowledgment fixture read

Exact 72a5a0f67 on bcb540a08. READ-OK for software, no P1/P2 demonstrated.
Mac fmt, strict Cloud all-target Clippy, fresh compilation and full Cloud
196/0/4 pass over five target summaries. All source hashes remain unchanged.
Only the cfg(test) settling wait changes. Release clears wifi_busy after
ack resolution and any relink request; Acquire observes completion before
the final equality assertion. Both acknowledged/unacknowledged branches,
initial no-immediate-relink assertion, final equality, helper join and
socket removal remain. A fixed absolute 5.5-second watchdog bounds polling,
without changing or renewing the product's five-second ack deadline.

Existing original current native ARM64 40130844249 passes all 1,216 selected
cases, eight skips and no leaks/failed/unrun: all 196 Cloud cases, 397 Update
and 623 Voice-eval. Target census includes both library and integration
qualifiers, not library rows alone. The changed case binds literal PASS.
Actual Linux 40130844271 pinned fmt/strict all-target Clippy pass.
Actual preview 9fa61c699 and authored whole trees equal e2ea465c2.
Primary hashes refer to gh rendered output, not API/Actions ZIP streams.

No timing-pair repeat, general performance/isolated-host claim, full workspace
local gate, real Wi-Fi behavior, product deadline change, native re-execution,
activation or device action. Producer's old timing/failed observations retain
their original scopes; latest-base readiness is separately the producer's.
