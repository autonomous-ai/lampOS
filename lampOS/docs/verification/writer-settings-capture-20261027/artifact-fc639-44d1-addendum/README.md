# PR 3858 original production artifact addendum

This adds archive proof to the separately sealed original source/runtime packet. Head `fc6398c51b2e90a40841ec14c5f0288851d8d6fe`, base `44d1ec5abe70723bc7e232552f05eb14f029c0d8`, actual preview `322ec4a3e7e6112e8d91c437a77b23622bba8fd4`, tree `30000dcd531cbd81b06660f4c2bb118fd40ab672`; original attempt-1 run 40562766574. The original 300-second read timeout and nonzero audit exit remain preserved. Only a bounded read-only archive download was recovered; no CI, compiler or test was replayed.

Original artifact 12761263846 has 118476481 bytes, SHA-256 `e5d45abf7a2208511148fddaea7f0fe373807af9726b4bb9fb6c4913e85ee476`. Its original API and upload-log digests match. The complete manifest accounts for all 48 production binaries and 2 selected examples, each SHA-256 and ELF64 little-endian AArch64 header verified. `lamp-update` has 4916000 bytes and SHA-256 `69acbf4e7ab2870e13f2e2496953c20869a2c08f5b621775193393981b395142`.

These are production artifact bytes from the same original job's current-workspace release build. Individual Nextest test executable bytes/hashes were not uploaded and are not inferred from production binaries. This adds no runtime passes and transfers no qualification to a later head. No writer activation, helper copy, key/signing/promotion, release/deployment handoff, device or power-cut acceptance is claimed.
