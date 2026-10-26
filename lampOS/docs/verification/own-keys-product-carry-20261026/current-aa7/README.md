# Own-keys imager-only carry

Head `f378095a90aa5b4183bb90e499c6cc7853fd546e`, base `aa7cf9029ef051ee5770837ccaed4ffc646cd74f`, carries all seven patches from tested `ff17b8c4e57dccf6fa3fe16e8f83ab2b68e66474` / `690dff041e331b5c89160901b4f38f3a9915e60b` with equal range-diff entries. All 38 authored files retain identical blobs and modes.

The complete 10,690-entry repository has the same path set. Its only changed blob is `scripts/imager/build-orangepi-v2.sh`, outside `lampOS/`: inherited Phase 2 image setup now normalizes the `/etc` owner and mode. A separate `bash -n scripts/imager/build-orangepi-v2.sh` passes; this is syntax evidence, not an image build or real filesystem permission change.

All **7,258 lampOS source entries**, their modes and inventory membership remain identical to the completed ff17/690 gate. All **66 actual executable and preserved-copy hashes** were independently rechecked unchanged after the carry. The earlier **6,175/0/48** outer-target test result, separate Memory child pass and **94/0 factory stand-ins** retain their original ff17/690 execution identity. No runtime test was repeated or relabeled for this imager-only carry. The complete six-crate packet is retained separately; its binary manifest is referenced by hash instead of duplicated.

Current native qualification remains separate. No device, provider/key, image build, installation, release, activation or physical latency claim is made.
