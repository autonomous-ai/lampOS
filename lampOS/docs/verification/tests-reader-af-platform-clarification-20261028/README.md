# Completed AF Reader platform clarification

This additive correction applies only to the completed macOS Reader module gate on `37076c5df20e47ac7de31df4c6b7b3d6ca0bf216`, base `af546143a9d30f3b3062bb7a6efbc215e3bfcef3`. Its four Reader cases passed once. The accompanying six Trace cases are a separate completed module scope; this packet adds no executions.

The old 16-name Linux field was the entire source catalog. Three cases are portable and already passed on macOS. Twelve ordinary Linux-exclusive cases and one ignored privileged DAC case were unrun on macOS. One ACL fallback case is macOS-only and passed. A catalog entry supplies no runtime credit. The copied original rows, source reference and terminal remain byte-for-byte unchanged.

The original additive audit records the proposed controller's hash at capture time. That proposal was subsequently superseded by the approved freshness-observation adjustment in a new preparation; it is not the controller used for this historical AF run. No original receipt or outcome was changed, and no compiler or test ran for this correction. The later authored branches and their CI remain separate.
