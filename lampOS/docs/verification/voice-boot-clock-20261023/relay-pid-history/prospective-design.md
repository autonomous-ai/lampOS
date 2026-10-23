# Prospective fixture-only repair and control — not implemented or claimed

Potential scope is only
`lampOS/crates/voice-eval/tests/it/relay_hardening.rs`, plus one explanatory
sentence in the existing `lampOS/docs/voice-eval.md` relay-testing paragraph.
No product seam or shared clock change is needed. The parent decides whether
to claim this separately after reporting the failed required gate.

In `a_closed_runner_transport_stops_the_runtime_and_its_workers`, keep
`worker.pid` and its final `read_pid()` unchanged. Add a distinct fixture
`worker.ready` path. The fake shell writes the PID first, then creates the
ready marker only after that command succeeds (`echo $! > PID && : > READY`).
Pass the ready marker to the existing `relay(..., Some(...))` helper. The
helper then closes the transport after completed PID publication. It need not
accept partially numeric file contents or make `read_pid()` retry after the
process is already dead.

All six existing case assertions remain byte-for-byte: elapsed under 4 seconds,
code 3, killed flag, transport-closed error, two trace events, and actual worker
absence. Keep the original 5-second closer watchdog and 5 ms polling, 5-second
session/allowance values, and 3-second/20 ms worker-absence boundary. No padding,
longer deadline, serialization or ignored case is introduced. The original
trace is still written before the worker/ready marker.

If this separate piece is claimed, freeze one adversarial old/new control
before running it. A private test-only channel pauses the fake shell after it
creates the empty PID file but before writing the PID. A closer-side handshake
confirms that exact state before its first readiness probe. The controlled
writer is released only after the closer observes its chosen readiness file
absent and enters the existing wait path:

1. Old readiness path is `worker.pid`: it already exists, so the closer does
   not enter the wait path or release the writer. It drops the stream; the
   normal relay kills/reaps the blocked shell and worker. The unchanged final
   `read_pid()` must fail with the empty-value parse error.
2. New readiness path is `worker.ready`: it is absent, so the acknowledged
   wait releases the writer. The shell writes the PID, publishes ready, and
   the normal transport close/cleanup follows. Every original assertion must
   pass, including the elapsed and actual worker-absence checks.

Use a private bounded FIFO/channel handshake, not a guessed delay. Any control
setup timeout or missing preparation acknowledgement invalidates the control;
it is not an expected product failure. Keep the existing real deadlines and
an external process-group watchdog. Capture the opened/ready/release ordering
and real worker PID outside the tested assertions, and restore transient
control bytes exactly. This control would establish the proposed fixture
ordering mechanism, not retroactively prove either historical interleaving.

After claiming the piece, appropriate validation would include the unchanged exact case,
this single fixed old/new control pair, all original relay-hardening cases,
full Voice-eval at default concurrency, formatting/strict all-target Clippy,
the required reader closure and workspace build. Decide and freeze the exact
scope/commands before execution; retain every result without retry-until-green.
No verification command was run for this audit.
