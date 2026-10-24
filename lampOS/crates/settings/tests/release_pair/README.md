# Released Settings store fixture

`current_settings_cli_store_smoke` executes the current test-built CLI. It uses
no Settings domain reader in the test: all reads and writes go through actual
server processes and test-owned bounded JSON framing on a private Unix socket.
Two ignored cases repeat the same semantics using pinned, distinct released
6408 and 6498 Linux AArch64 executables in both orders.

The native cases require Linux AArch64, read-only `LAMP_COMPAT_SETTINGS_6408`
and `LAMP_COMPAT_SETTINGS_6498` inputs, and an existing absolute short
`LAMP_COMPAT_OUTPUT` directory (for example `/tmp/settings-proof`). Payload size,
SHA-256, ELF architecture, original and copied modes/hashes are checked. Private
executable copies are mode 0555. The private case directory is retained on success
and failure. Do not publish executable copies with the text receipts.

Each case runs two real catalog CLIs and checks their complete declarations.
Three explicitly enabled, private servers then execute A writes, B recovers and
updates, and A recovers again. Assertions cover the exact original store ID,
complete catalog, every default/changed value, revisions 0 through 3, persisted
retry receipts, and retries of both earlier mutations after the final reopen.
Each server stage refuses stale revision, wrong store, a reused request ID with
different values, unknown operation, unknown value kind, and out-of-range value;
every refusal preserves the store bytes. Two extra real server starts refuse a
synthetically added stored field and snapshot version 2 without rewriting bytes.
These are strict refusal controls, **not** forward-field tolerance claims.

Successful cases have seven exact child reaps: two catalog commands, three
SIGKILLed servers, and two refused server starts. Each owned process starts a new
process group; every normal completion checks that the group no longer exists.
Failure cleanup kills only the owned group, waits at most five seconds for the
exact child, checks group absence, and retains its receipt. No bus root,
hardware path, consumer, or ambient child environment is provided. Desired-state
save success does not mean consumer adoption or physical qualification.

The case has one absolute 90-second deadline. Each step is capped at ten seconds;
each socket exchange at two seconds. Nonblocking connect/write/read share that
original absolute exchange deadline; incoming fragments never renew it. Reply
allocation is bounded at 128 KiB. Two ordinary test-only framing controls check a
complete reply and refusal of a continuously dribbled unterminated reply.

`transcript.jsonl`, private state snapshots, catalog, stdout/stderr, exact exit
receipts, and before/after payload hash receipts remain beside `result.json`.
The result names the test, actual released-pair status, exact child and RPC counts,
and verified group absence. These tests prove host process-restart compatibility,
not filesystem power-loss durability, updater switching, or behavior on a lamp.
