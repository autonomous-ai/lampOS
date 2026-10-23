# Published release 6439: wrong-key refusal controls

Two negative controls for #1812, executed on 23 October 2026 with current
Mac updater source `46f5f384ad94f9c2dcc5f7050991ef4325621384`.
**Both controls passed. Positive verification with the real trusted key has
not run.** No release-signature acceptance follows from these refusals.

The [6439 release binding](../previous-helper-6439-20261023/README.md)
identifies the published envelope, source and successful publisher.
The CLI was built locally with Rust 1.96.0 using
`cargo +1.96.0 build -q --locked --offline -p lamp-update --bin lamp-update`.
The single-crate host build exited zero with no diagnostics. The exact CLI
SHA-256 and source head are in [receipt.json](receipt.json); no binary is
committed and no local ARM64 build or workspace gate was run.

| Input | Expected and observed | Root |
| --- | --- | --- |
| Unchanged published envelope, different public key | Exit 1; refused because envelope key `54213f8d3aa2f241` differs from supplied key `21fe31dfa154a261` | Unchanged |
| Same manifest and signature, envelope key label changed to the supplied key | Exit 1; the signature does not match the manifest | Unchanged |

The second case changes only the unauthenticated envelope's key label, then
serializes that envelope. It reaches cryptographic verification with the
wrong public key and refuses. It does not use or recover the real signing key.
The supplied key is the public half of a published RFC 8032 test vector,
retained as [rfc8032-public-key.pub](rfc8032-public-key.pub); no private key is
used. The original and changed envelopes are retained as text inputs.

Each check used a fresh scratch root with only an existing updater state at
sequence 6439, a local `file://` channel, `--apply off` and self-update off.
The process had a 30-second bound. Both exited normally with their expected
refusal in 0.663 s and 0.011 s respectively. These are host command times,
not lamp response latency. Directory types, permission modes and every file
hash were compared before and after each case; the snapshot is retained.
No asset download, installation, signing, publication or device action occurred
inside either check.

The command shape for each retained envelope is:

```text
lamp-update check --channel-url file:///SCRATCH/negative-channels/CASE/ --channel lamp-voice --key-file SCRATCH/rfc8032-public-key.pub --root SCRATCH/wrong-key-check-root --lamp release-check --apply off
```

Each channel contains that case's envelope as `lampos-manifest.json`.
The temporary state's relative path and exact hash are in the receipt; its
content was `{"installed":{"sequence":6439,"commit":"0000000000000000000000000000000000000000"},"previous":null,"refused":[],"made":[]}`
with ordinary JSON spacing and a final newline. Captured stdout is unchanged;
stderr replaces only the temporary scratch prefix with `SCRATCH`. The receipt
retains both original and published output hashes. Empty output/build files
are intentional.

Migrate's independently sourced public key with fingerprint
`54213f8d3aa2f241` is still needed for the prepared positive and tampered-payload
controls. First and the assigned lamp holder/person retain the real signed
update, quiet switch, rollback and physical-cut acceptance. These negative
controls do not waive any of those requirements.
