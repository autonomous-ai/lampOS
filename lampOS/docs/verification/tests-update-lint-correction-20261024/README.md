# Update helper lint correction and retained gate failures

Ready for lamp-voice: no. Corrected-source full gates are running.

The first full-gate allocation failed its local AF_UNIX socket preflight under the default sandbox. It executed zero Rust commands or tests; all five gates remained unrun. That allocation remains failed. The next allocation used the authorized local-socket permission context: formatting passed, then strict Clippy failed on the helper's nested `if` (`clippy::collapsible_if`). No full test stage or workspace build ran. Both failures are retained separately here.

The only source correction folds the ordered deadline/EOF/status condition into Clippy's suggested let-chain. Error, return, deadline and cleanup semantics stay unchanged. One pinned package formatter passed; all other 10,443 source entries retained bytes, full modes and modification times. The actual changed helper modification time was frozen, not fabricated.

A new source epoch was cleaned and compiled once: 428 compiler messages, 422 unique contexts and 120 fresh local artifact messages. Root independently parsed the actual compiler stream and checked current local source paths. The corrected round executable remains byte-identical to the earlier candidate, SHA-256 `7ea1600bba97d23828caf48825fb649d900c2f1a1312a3789b029b34e49fc2d0`. The new full-gate plan binds this source and fresh output proof. It runs no extra standalone diagnostic, old witness or directed controls; the ten-control full-suite execution remains planned.

Previous directed controls and the controlled old-loop witness remain recorded under their actual source epochs. They do not turn the failed gate allocations green or waive the historical two-of-three HOLD. Native Linux checks and a ready PR remain pending. No product, ARM64-on-Mac or device action.
