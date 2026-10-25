# Revised source candidate retains the enabled Adapter path

The initial unrun proposal at 75c7041ef removed the only enabled Adapter construction/sample/worker/drop test. Self-read correction 6391271191 and independent P2 6391279247 identify that gap. That earlier proposal is not a qualified coverage-preserving gate.

This revision retains its controlled Shared-tick/real-bus phase scenario, and adds `unplug::tests::the_enabled_adapter_publishes_its_latch_and_joins_after_stop`. The added case starts the actual enabled Adapter, waits for its real Folding publication/latch, uses the actual safe and unsafe sample method and checks encoded monotonic time/low bit/invalid count/reason, observes its stopped Failed publication, keeps the latch at both publication endpoints, joins its actual worker, and removes the bus root. Publication waits and the post-join check are bounded at two host seconds; a stuck blocking join still needs the outer process watchdog. No uninterrupted 500 ms host-scheduled sample train is required for this separate lifecycle case.

P3 is corrected: the controlled scenario asserts the latch after every observed step, and docs say exactly that. Its two 500 ms proofs, 20 ms sample schedule, actual PERIOD, 2 s logical phases plus host watchdog, early Failed rejection and moved-joint reason remain. Runtime and all other existing test bytes are unchanged. No new clock API or thread scheduling/physical latency claim is added.

## Fixed future observations, none run

After exact current-base/source freeze and a coordinated allocation: fmt, strict all-target Body Clippy, complete no-run compilation, one direct controlled-gap case and one complete Body run. The full run must contain the named enabled-Adapter test's literal result. Dedicated targets and pre-exec witnesses prevent the known old cross-root library binding gap.

One observation each of five distinct negative variants: PERIOD to three seconds; Proof threshold to literal 100 ms while the 500 ms oracle constant remains unchanged; gap treated as permanent failure; Adapter::sample made a no-op; spawned worker loop made to omit its tick. The first three run the gap case, the last two the new enabled-Adapter case. Every failure must bind its intended assertion or bound; all source is restored and owned processes/fixture roots cleaned. No original baseline or failed unchanged observation is repeated, and no test retry-until-green is allowed.

This is source preparation only. Clippy/tests/negatives are UNRUN, with no compiler slot allocated. First queued work remains the Supervisor fresh-target qualification. Original three 0/1 gap observations and their unresolved Ready phase remain unchanged; no historical cause follows.
