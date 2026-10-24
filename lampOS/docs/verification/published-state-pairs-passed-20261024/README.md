# Published state pairs: successful prior heads

These receipts qualify PR #3312 head `cec3372b7f26a728d0b96e0e681d257d191ff7fb` and PR #3323 head `561c6a21b1ea8e8d717561183c1f8d0714eba068` only. Both were tested as merges with `59c4ac7959bfe4709e85c2be4799f58bbefe2736`. They do **not** qualify the subsequent squash/rebase onto `090847e59` or any later carried head.

| Recorded head | Dedicated actual-binary tests | Ordinary native ARM64 |
|---|---|---|
| #3312 `cec3372b7` | [40054409458](https://github.com/autonomous-ai/lampOS/actions/runs/40054409458): Schedule 4, Tasks 2, Voice 6 passed; zero failures or ignored tests | [40054409431](https://github.com/autonomous-ai/lampOS/actions/runs/40054409431): 11,128 PASS + 1 LEAK, zero failed, 83 skipped; 201.988 seconds |
| #3323 `561c6a21b` | [40054631773](https://github.com/autonomous-ai/lampOS/actions/runs/40054631773): Skills 4 passed; zero failures or ignored tests | [40054631836](https://github.com/autonomous-ai/lampOS/actions/runs/40054631836): 11,128 PASS + 1 LEAK, zero failed, 75 skipped; 201.472 seconds |

Each ordinary ARM64 total includes **one LEAK-marked pass**, `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`; it is not an additional clean PASS. Both also passed four doctests, the separate 12-case process harness, the relay check, and publisher/artifact/real-CLI Python suites of 35/12/7. Captured nested test output after the nextest summary is not counted again. Both publication jobs were skipped. [#3323 x86 Clippy 40054631774](https://github.com/autonomous-ai/lampOS/actions/runs/40054631774) succeeded; this receipt does not add a #3312 x86 qualification.

The tested merges are `084a5c57d30f8d2018e9167b049ff03fd77595c1` (#3312) and `bf2b03555b68eb94c7b30ca966e617fa8a7471c5` (#3323). Checkout logs, merge API parents, artifact provenance and tree identities agree. The dedicated source manifests contain 6,973 and 6,968 tracked files respectively, unchanged before/after; retained fixture hashes also match their recorded PR heads. Native warnings are retained in `ci.json`: certificate setup, Node action deprecations, and the existing release example filename collisions. There were no new Rust Clippy diagnostics.

The dedicated binaries are the pinned published ARM64 releases 6408 (`ee53765c8d014eb00e41c1a5a7054d2546c18bdb`) and 6498 (`846b3d203fc00ec3886d71f7673d8cc29cc4a634`). Their per-part hashes and sizes, captured before/after checks, and test commands are retained. Schedule's two payloads are byte-identical, so its result is not evidence of a Schedule code delta.

The case records establish:

- Schedule: eight scenarios inside four tests, covering both directions, same/changed synthetic boot identities and ack/snooze receipts; 32 successful process exits.
- Tasks: six schema/direction cases inside two tests. Each records SIGKILL and reaping for Tasks and its blocked fake backend, preserved store/companions, zero replayed launches, and opposite writes reopened by the original release. Completion remains `unverified`; Harness and Voice are fixtures.
- Voice: six delivery-ledger cases (unfinished, completed, ack intent in both directions), with 18 matching spawn/SIGKILL/reap records. Every case commits one acknowledgement and retains an empty ledger on opposite and return reopen. Schedule API and completion proofs are authored; no physical speech occurs.
- Skills: daily and on/off cases in both directions, with 22 matching spawn/reap records (10 SIGKILL exits and 12 exit-zero commands), persisted refusal reasons and verified state/manifests. Policy never approves. Rested state and physical output are outside this row.

All regular UTF-8 case evidence is copied byte-for-byte, including state JSON and manifests. The four dedicated test logs are renamed from `.log` to `.txt` without changing bytes. `evidence-map.json` binds every copy; `excluded-case-files.json` lists every omitted binary case file and its digest. No database, executable, archive, Python helper or identity-bearing API object is included. Snapshot hashes were checked against captured bytes, and process identities were matched within each case. The explicit Tasks cut records do not independently enumerate every transient backend subprocess.

The downloaded dedicated ZIPs matched GitHub's recorded size and SHA-256:

- #3312 artifact `12545355488`: 704,678 bytes, `7ae4c87e5a8a1d5c7e2939d81f8fd676ce2087c595e63628c1558c34072a4fbb`.
- #3323 artifact `12544703173`: 620,309 bytes, `2493b41514591da006aa21c8891740af33e70e3094f464bb72b7172d0d4e8b47`.

Ordinary ARM64 build artifacts were checked as metadata only; their executable payloads were not downloaded in this audit. There was no device, physical reboot/power cut, real provider, updater installation or attended output acceptance.

Earlier failures remain evidence: [Reminder/Task failed attempts](https://github.com/autonomous-ai/lampOS/tree/993fc3530/lampOS/docs/verification/reminder-task-release-pair-failed-20261024) and [routine-state failed attempt](https://github.com/autonomous-ai/lampOS/tree/a2e456672/lampOS/docs/verification/routine-state-failed-20261024). These successful runs do not erase them.
