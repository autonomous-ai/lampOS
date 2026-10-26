# Reset read-only query bounds

Final commit `8a050d1bfd098769a8361849171271964a5082f7` on base `521dcb006848a5bc99b11513828233ef65a4883c`. Four files only: Setup reset delegation, private query helper/tests, setup and reuse documentation. No push or device/provider action. Queries retain at most 64 KiB stdout and use a five-second deadline. System::run, stop/writer ordering, custom-stop policy and journal/reboot semantics are unchanged.

The runner reuses nonblocking owned-child mechanics from the preserved Rust runner 258b2a9f10. It retains an unreaped leader while stdout remains open, so error cleanup owns its process group. Successful EOF and leader exit end ownership; detached descendants or descendants closing stdout are not claimed as cleaned up. Null stdin and discarded stderr preserve the previous externally visible query policy without unbounded stderr storage. Stop commands retain their own allowances. Kernel spawn/reap/filesystem time is not claimed as hard-wall bounded.

## Qualification and source identities

The initial frozen draft passed fmt, strict Setup/Cloud all-target Clippy and matching full `cargo test -p lamp-setup -p lamp-cloud` compilation/runtime at 16 threads: **Setup 191/0/1; Cloud 196/0/4; total 387 passed, 0 failed, 5 ignored**, plus two zero doc-test summaries. Runtime command 20.948s. Six query controls passed once. These counts remain bound to the draft source inventory, not relabeled as a full final-source run.

After complete draft source/artifact closure, only tests and documentation changed. The final supplement added actual System::read cap wiring and a positive stdout-closed phase witness; production query bytes are identical. Final fmt and strict Setup/Cloud all-target Clippy passed. Matching `--lib` compilation and exact runtime filters ran each changed/new control once: **1/0/0 each**. Both source inventories and the exact test-only diff are retained. No failed runtime or automatic retry occurred. Final committed source matches the frozen final inventory.

Metadata used a separate scratch target. The exclusive compilation target was atomically created absent/empty immediately before Clippy, with wrapper/output-redirection configuration checked. All local inputs are inside the exhaustively inventoried lampOS tree. Draft closure retained 10 executable, 63 local-output and 445 depfile bindings. Each later scope closed its actual/preserved guards before the next compile; older actual paths may be rebuilt, so historical preserved copies and closure receipts carry their claims. Only the final scope's actual outputs are rechecked at packet creation. Raw command/log hashes, literal case summaries and artifact freshness are included. No binaries or scripts are in this packet.

## Review boundaries

The wrong-person boundary is fail-closed: a query error cannot authorize wiping while writers may remain; a real failed command control proves data/journal retention and reboot. No authority, configuration, protocol, switch or stored format changed. Query error cleanup is limited to its owned CLI group and does not cancel systemd jobs. Existing full Setup reset/custom-stop/helper controls passed in the draft. Cloud is the direct Setup test consumer; no actual setup.md/reuse-decisions.md embedded reader was found. Native Linux and installed reset acceptance remain separate requirements.
