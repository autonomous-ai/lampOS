# Published release 6592: independent download consistency

Observed 24 October 2026 at 10:28:41 UTC (17:28 +07), for #1812/#2809.
**63 assets passed, 0 failed**, 226,984,741 bytes, including all **60
manifest payloads** (32 files and 28 units). Each public HTTPS download
matches GitHub's asset size/SHA-256; every manifest payload also matches
its declared size/hash. All **31 SHA256SUMS entries** match downloaded
bytes. Transfer/hash run took 22.074 seconds with four bounded workers.
[Complete per-asset receipts](asset-checks.json) record identities/digests.

The immutable downloaded envelope and manifest still equal the earlier
[observation](observation.json). Decoded sequence is 6592, source
`c6dfe980979e0088cd94f1dab1fe4a70ddbcbe46`, claimed key label
`54213f8d3aa2f241`; the embedded manifest equals manifest.json byte for byte.
This is not positive signature verification: a key label and API asset
integrity are not an independent trust anchor. Migrate/holder's trusted
public half remains required. No signing secret was accessed.

No downloaded ARM executable was run. Executable/archive payloads and raw
transfer logs remain private scratch and are not committed. The five public
files here are text without NUL bytes. No installation, service mutation,
physical cut, lamp test or automatic rollback was performed by Second.

Release 6592 contains unit sets for 4ace and 52e6 and no model/settings/image
sections. Existing files beside old binaries may be carried by the updater's
normal `Kept` path; the holder must verify actual runtime model/library
hashes after B and put-back. Such preservation does not qualify a newly
versioned model/settings/image payload. Select a qualified B strictly newer
than actual installed A; 6592 is an observed candidate, not authorization or
a promise it is the newest publication or suitable for a particular lamp.
The [attended worksheet](../update-lamp-acceptance-20261024/README.md) and
[static updater contract](../update-product-contract-20261024/README.md)
remain separate from this download check.
