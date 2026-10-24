# Failed published rested-state attempt

[Native run 40056359883](https://github.com/autonomous-ai/lampOS/actions/runs/40056359883) exercised PR #3327 head `b76fe12ae98c1ac2bd284dae347ab42bb3816776`.
The run finished **4 passed, 2 failed**. Both rested directions failed at the fixture's strict directory creation with `AlreadyExists`, after each A process had completed its negative admission controls, admitted a night, renewed it, and saved the night and daily records.

The fixture created its bus at `<case>/b`, then tried to create the opposite reader's private process directory at the same `<case>/b` path. The selected source-archive member names in [bus-directory-inventory.txt](bus-directory-inventory.txt) show that bus directory was populated already. Both transcripts contain only an A spawn; no B process started.

Each [A exit receipt](kn-6iFfa5/a.exit.json) ([other direction](kn-cJ2RNS/a.exit.json)) records SIGKILL and reap of its owned child: PIDs 10068 and 10104. Those normal stop receipts precede the failed B directory creation; there is no failure-cleanup receipt because no child remained owned at that boundary. This is evidence about the two owned children, not a global process inventory.

[skills-tests.txt](skills-tests.txt) preserves the complete native test log. Each case retains its raw A stdout/stderr, full event transcript, synthetic manifest, and after-A state snapshot. [hashmap.json](hashmap.json) maps every copied archive member to its retained filename, byte count and SHA-256. Log-to-text renames preserve every byte; no redaction or filtering was performed. The bus directory inventory is derived from archive member names and is identified separately. No executable, bus binary payload, Python file, API response, account metadata or credentials is included.

The correction gives process sockets separate `child-<stage>` directories and uses that exact private path for all five socket arguments. Strict directory creation and all continuity assertions remain in place. This failed attempt does not qualify B or returning-A continuity, physical motion, actual Expression behavior, clock synchronization, installation, or power-loss durability. Fresh corrected native execution is required; this record makes no passing claim.
