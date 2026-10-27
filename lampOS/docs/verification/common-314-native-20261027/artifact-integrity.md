# Common 314 artifact integrity supplement

Ready for lamp-voice: no

The [initial audit](receipt.json) used a 32 MiB download cap and did not
download the binary archive. A later bounded follow-up used
160 MiB compressed and 1 GiB aggregate uncompressed caps. That separate
follow-up downloaded artifact `12736055317` from ARM64 run `40504365418`:
118,176,415 bytes. Its complete ZIP SHA-256 matches both API and upload evidence.
The initial receipt and its historical limit remain unchanged.

The [supplemental receipt](artifact-integrity.json) records successful download
and verification exits. All 51 archive members have safe paths and types;
318,191,011 uncompressed bytes fit the cap. The
[validated manifest](validated-export-manifest.json) contains exactly 48
workspace binaries and two examples. Every payload was streamed, checked
against `SHA256SUMS` and its ZIP CRC, and identified as ELF64, little-endian,
AArch64. No binary was extracted or executed.

These verified digests come from the same artifact:

| Payload | SHA-256 |
| --- | --- |
| `lamp-voice` | `5bf07f4c10a005092cb52d944bd046fb1a995ec1bbc61239bd70e4124ce72223` |
| `lamp-eyes` | `2e08b98629d903e00ecaa6aa159b77d7a32e6b8f5e3160e6fdae450c41da5e9a` |

Their source is actual CI checkout `433935815339d6d773d44e2107635db7d73b7a0c`,
tree `fd5a3439849af70dd65fc06927163cc6cbb23d78`, merging base
`241f51500f258ea06594f9edf3928a702d6a2038` with published head
`314a0f6bcb4061e360b85b8e0c9f0a9048cba8cd`. They remain evidence for that CI source. Installed-pin and mixed-build
compatibility checks, deployment and physical acceptance remain with their
holders; privacy, reader and latest-base holds stay open.
