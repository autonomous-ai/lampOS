# PR 3913 original production artifact addendum

This adds archive proof to the separately sealed original source/runtime packet. Head `ee4ddf6f6d5b5856e9fc398860a6ab2984619bf9`, base `44d1ec5abe70723bc7e232552f05eb14f029c0d8`, actual preview `6f4ebe606e3d6e92a423990906da33a5fdc38de0`, tree `694dc2a574b2d00e4af20f6b8a100ba6590be11e`; original attempt-1 run 40562988182. The original 300-second read timeout and nonzero audit exit remain preserved. Only a bounded read-only archive download was recovered; no CI, compiler or test was replayed.

Original artifact 12763100852 has 118477035 bytes, SHA-256 `ca4541d72cb07bb03a1ed084a6ce4a43aad50f5a55375f35e3c6bf367568b31f`. Its original API and upload-log digests match. The complete manifest accounts for all 48 production binaries and 2 selected examples, each SHA-256 and ELF64 little-endian AArch64 header verified. `lamp-update` has 4903240 bytes and SHA-256 `2cdb43d716d780e5b929180e3bb0489039a064908571b92ed5942826f4cc166f`.

These are production artifact bytes from the same original job's current-workspace release build. Individual Nextest test executable bytes/hashes were not uploaded and are not inferred from production binaries. This adds no runtime passes and transfers no qualification to a later head. No writer activation, helper copy, key/signing/promotion, release/deployment handoff, device or power-cut acceptance is claimed.
