# Parent hand controls: current host proof

Host verification is source-scoped. The companion [original native proof](../hand-5b-original-native-20261028/README.md) records the completed new-head CI gate.

Head `5b018ca5d0278f5c3655548c00c41a45ab58e9db`, base `fc9c781afaed38dc976f650484c30f65bf5db57a`, tree `0ed8528f4684ab660029cd1805685a19f6714634`. The completed host compiler passed formatting, metadata, strict workspace/all-target Clippy, and workspace build. The completed runtime passed **4,224 unique tests**: one exact trace control, four exact hand controls, and 4,219 disjoint remaining tests. All 22 host-required controls passed once; the Linux-only control remains unrun on Mac. Four documentation tests passed separately.

The disjoint group counts are Voice library 3,887, Update library 227, installation 108, and workflow readers 2. Hand, privacy, and transport counts are subsets. The direct controls used 30-second watchdogs and 2-second termination grace; their recorded operation times are in [direct-controls.json](direct-controls.json). These times do not measure device or acoustic latency.

[receipt.json](receipt.json) records actual tool completion, source and preservation guards, counts, and evidence hashes. [required-controls.json](required-controls.json) records the 22 observed host identities and the separate Linux-only identity. [saved-binary-bindings.json](saved-binary-bindings.json) binds normal Voice/Update companions and test drivers using records saved during this gate. This audit read no current target files. [source-ledger.json](source-ledger.json) records relevant frozen source identities.

The original parent ARM failure remains failed: 6,466 passed, three failed, and 1,336 selected cases unrun. Its published failure evidence remains at log commit `a0a4067249`. The older child ARM failure and all earlier failures remain historical failures. This fresh host pass establishes neither their cause nor a waiver. New original ARM64 and Linux Clippy runs must be assessed on their actual checkout. No device acceptance, deployment, artifact pins, or readiness transition is claimed.
