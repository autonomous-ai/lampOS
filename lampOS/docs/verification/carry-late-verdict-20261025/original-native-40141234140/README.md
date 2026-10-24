# Original native result

{
  "run": 40141234140,
  "authored": "a0b3711669a6ec0dd4eea54f148edf76b9c78cad",
  "tested": "c7659278e29409e6816f127fac492a7d8d3abb63",
  "parents": [
    "da57072b2e6420ae25cf6779f1ee1756178ce724",
    "a0b3711669a6ec0dd4eea54f148edf76b9c78cad"
  ],
  "authored_tree": "849843eeda1d11565933b8e3d8c762451b2c3b29",
  "tested_tree": "fb491b826dd0f2ee78ec89b6f86366fff88e5cc7",
  "tree_changes": [
    "lampOS/deploy/lamp-4ace/lamp-voice.service",
    "lampOS/deploy/product/lamp-voice.service",
    "lampOS/docs/duplex-listening.md",
    "lampOS/docs/switches.md",
    "lampOS/docs/verification/soft-start-product-20261025/README.md",
    "lampOS/docs/verification/soft-start-product-20261025/ab3-arm0-onset.txt",
    "lampOS/docs/verification/soft-start-product-20261025/ab3-arm0-owner-rH0-0402.txt",
    "lampOS/docs/verification/soft-start-product-20261025/ab3-arm0-pause-rP-0355.txt",
    "lampOS/docs/verification/soft-start-product-20261025/ab3-arm1-onset.txt",
    "lampOS/docs/verification/soft-start-product-20261025/ab3-arm1-owner-rH1-0419.txt",
    "lampOS/docs/verification/soft-start-product-20261025/ab3-arm1-pause-rP-0412.txt",
    "lampOS/docs/verification/soft-start-product-20261025/ab3-flags-readback.txt",
    "lampOS/docs/verification/soft-start-product-20261025/onset2.py",
    "lampOS/docs/verification/soft-start-product-20261025/unit-dropin.py",
    "lampOS/docs/verification/soft-start-product-20261025/unit-round-cmdline.txt",
    "lampOS/docs/verification/soft-start-product-20261025/unit-round-dropin.conf",
    "lampOS/docs/verification/soft-start-product-20261025/unit-round-envcheck.txt",
    "lampOS/docs/verification/soft-start-product-20261025/unit-round-rUG-0454.txt"
  ],
  "terminal": 11265,
  "status_counts": {
    "PASS": 11264,
    "LEAK": 1
  },
  "skipped": 106,
  "failed_unrun": 0,
  "process_12_0": true,
  "raw_sha256": "bb0d054996829cb24db8ad67e71ca42657197c173e77fe3e993a4c3e7b6e4502",
  "scope": "Actual original run only; authored/tested identity stated explicitly; older failures and reader failures not waived; latest efbf carry requires own gates.",
  "complete_trees_equal": false
}

The full terminal case census is in native-audit.json. This is the original run on its recorded preview, not a renamed latest-base execution. Original 05 failure and separate reader failed runs remain failed. No cause, device, acoustic latency or readiness waiver follows. The latest efbf composition is separately qualifying.
