# Surround candidate and operation-journal release pair

`release_pair` has three ordinary tests: a current-CLI smoke and complete/dribbling
HTTP framing controls. The smoke exercises the actual built CLI and its private
Unix HTTP listener, not the crate's current router or configuration reader.
Two ignored cases exercise 6408 → 6498 → 6408 and the reverse using actual
published executable copies. Each case starts seven owned process groups: four
normal operator servers, two expected saved-state refusal exits, and one healthy
recovery server. No models, provider requests, devices or hardware are used.

The harness only calls login, status and `save_provider`. The runtime configuration
has enabled true and the CLI receives `--enable`; owner, release, network and bus
are absent. The startup declarations remain disabled and unconfigured. Settings,
operation journal, generation store and listener directory are distinct private
siblings. No apply, reset, network, probe, release or startup operation is sent.
HTTP creates operation IDs internally: the fixture does not invent a client CAS,
retry-ID or replay API. It checks unique generated IDs, admission generation,
retained used-ID history, completed operations and no automatic re-execution on
reopen. No selected configuration is created.

Three distinct synthetic documents/instruction files are saved. Generic JSON
inspection and exact hashes check candidate identity, manifest inventory, provider
payload paths, credential bytes and private file modes. The immediately previous
candidate is byte-for-byte immutable while its replacement is staged. The third
save intentionally collects the first, unselected superseded generation under the
released implementation's existing collection policy; precollection hashes remain
in the transcript. This is not an all-generations-retained claim.

Synthetic HTTP controls reject an unknown operation, an unknown provider field and
an unknown authentication enum before journal admission. After all normal servers
exit, a candidate with an unknown field and a manifest with schema version 2 each
make the released server refuse startup. Bad bytes, remaining generation files and
journal must stay unchanged. The fixture restores only its own original bytes,
then checks healthy reopen without automatic saves. These specific refusals are
not a blanket future-schema or future-field compatibility promise.

## Actual released inputs

Set `LAMP_COMPAT_SURROUND_6408`, `LAMP_COMPAT_SURROUND_6498` and
`LAMP_COMPAT_OUTPUT`. The output parent must already exist, be canonical and short,
and be under `/tmp` or `/private/tmp`. Every case creates a new `sr-<pid>-<counter>`
private directory and retains partial evidence after failure. It never deletes
inherited state. Unix socket paths must remain shorter than 103 bytes.

Both selected release payloads are identical: 13,871,368 bytes, SHA-256
`f75b1a57dd2cfc4edcc521b0d8a254ca72c9ba4402de61a6873b3bcafd0ca765`,
ELF64 little-endian AArch64. Release 6408 asset 668607931 comes from source
`ee53765c8d014eb00e41c1a5a7054d2546c18bdb`; release 6498 asset 670759914 comes
from `846b3d203fc00ec3886d71f7673d8cc29cc4a634`. The result therefore qualifies
these selected published artifacts, not differing Surround implementations.
Original/copy hashes and modes are checked before and after execution; private
copies are executable and read-only. Cargo's current ordinary-test executable
may have multiple hard links on Linux; it is read only and its bytes, mode and
link count are checked before/after. Only its new single-link private copy is
executed. Released downloads and all private saved inputs still require one link.

Ignored cases require native Linux/aarch64, a plain root-owned, non-writable-by-
others `/run/lamp-surround-pair-isolated` marker containing exactly:

```text
lamp-surround-candidate-pair network-none tmpfs-v1
```

They check loopback-only interfaces and no IPv4 default route. The marker records
the official workflow's isolation assertion; it independently proves neither
container creation nor mount isolation. The workflow must use a fresh network-none
container and private writable tmpfs paths, with no lamp or host state mounted.
Released ARM64 executables must not run locally on the Mac.

## Evidence and deadlines

Each case has one original 90-second deadline. Each exchange also has a single
three-second absolute limit across asynchronous connect, all partial writes,
header and body reads. HTTP content lengths, chunk lengths, headers and bodies are
bounded. Dribbling bytes cannot renew the budget. Process exits are bounded,
exactly reaped and followed by absent-group checks; fallible bounded failure
cleanup retains partial receipts. A passing result records the test, passed
result, released-pair flag, reaped children, absent groups and exchange count.

All login codes, credentials and instructions are synthetic. Cookies are transient
and are never recorded. Status, stdout and stderr are checked for the synthetic
credential and operator code. Publish only `case.json`, `artifacts.json`,
`transcript.jsonl`, `result.json`, and checked/redacted stdout/stderr receipts;
**do not publish the executable copies, settings or generation payload tree**.
The transcript records hashes, sizes and modes, not credential contents. The
fixture does not qualify hardware acceptance, provider connectivity, configuration
activation, filesystem power-loss durability, physical privacy or unattended use.
