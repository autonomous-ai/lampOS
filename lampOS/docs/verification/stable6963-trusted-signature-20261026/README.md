# Trusted signature check of stable release 6963

For #1812, checked on 26 October 2026 on a Mac. **Five checks passed, zero failed.** This completes Second's previously blocked host signature check; it is not installation, quiet-switch, rollback, attended Body or power-loss acceptance.

The trusted PUBLIC key was read from the repository's `lampos-update-publish` environment variable `LAMPOS_UPDATE_PUBLIC_KEY`, whose completion is recorded on [the shipping checklist #3530](https://github.com/autonomous-ai/lampOS/issues/3530). Its decoded 32 bytes independently hash to fingerprint `54213f8d3aa2f241`. [Key provenance](key-provenance.json) retains the public value, configuration update time and retrieval time. The trust source is the repository's configured verification key, not the key label in the downloaded envelope. No private key, signer, secret or configuration mutation was used.

The captured `lampos-stable` envelope is byte-identical to immutable `lampos-lamp-voice-6963`. Both downloads match their API asset size and SHA-256 digest; the decoded manifest names sequence 6963 and source `41233c1b6954abc812186d7014a7adb19422e29e`. Captures and asset bindings are retained. Only manifest text was downloaded, not executable assets.

| Check | Expected and observed | Scratch root |
| --- | --- | --- |
| Original envelope, trusted public key | Exit 0, already at build 6963 | Unchanged |
| Original envelope, RFC 8032 public test key | Exit 1, key fingerprint mismatch | Unchanged |
| Envelope key label changed to test key | Exit 1, signature does not match manifest | Unchanged |
| One trailing space added to signed manifest bytes | Exit 1, signature does not match manifest | Unchanged |
| One bit flipped in the decoded signature | Exit 1, signature does not match manifest | Unchanged |

The payload control preserves valid JSON and semantic manifest content, so its refusal tests exact signed-byte integrity. No private half of either key was used. No negative was retried.

The checker is the retained native Mac `lamp-update` compiler artifact from head `46b5c220d` on base `88335bfbe`, SHA-256 `799a71ccd12590795d2b340662c6027fee26a69ac70888844c0ca34d980aff3b`. [Its binding](checker-binding.json) links the earlier public compiler receipt. Both original actual/preserved files matched that hash before copying, and the executed copy remained identical after every case. This is not execution of the downloaded ARM64 release, nor a fresh build on current Main. Its later docs-only carry does not relabel the compiled source.

Each command was `lamp-update check`, with `--apply off`, `LAMP_UPDATE_SELF=off`, a local captured `file://` channel, explicit scratch root/key/lamp/bus paths, and a 30-second process bound. The scratch state was already at sequence 6963. Source `round::Run::look` verifies the envelope and channel before checking that sequence; a successful up-to-date result therefore passes signature verification. `check` writes nothing and does not subscribe to the bus. Every file hash/type/mode in each root was compared before and after. Commands, outputs and snapshots are in [results.json](results.json) and `cases/`; only the temporary root prefix in published commands/stderr is replaced by `SCRATCH`, with original and published output hashes retained.

Historical wrong-key controls and earlier download-only receipts stay unchanged. No lamp, provider, systemd install, release publication, promotion, signing or rollback ran. #1812's remaining acceptance belongs to the assigned lamp holder/person and release operators.
