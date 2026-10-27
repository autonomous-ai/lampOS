# Restart a209 host qualification

PR [#3572](https://github.com/autonomous-ai/lampOS/pull/3572), head `a209424d3c5f07ca3423447bc2bb8bcb6d0a18e0`, authored base `99b5161531791de7ed57ce57f6cacf988d509f25`. The completed Mac gate passed the entire `same_voice::tests::` module **21 passed, 0 failed, 0 ignored, once serially**. Whole-workspace formatting, strict Clippy for the actual 12-package reverse closure, workspace build and Voice-library no-run compilation passed before runtime. No clean or forced freshness flag was used.

The actual library inventory contained 3832 cases; 3811 outside the 21-case module were filtered, not executed. Every module identity and outcome is in [audit.json](audit.json), together with the ten command exits/raw-log digests, source/profile/freshness observations and executable binding. Controller, wrapper and native exit were all 0; every owned process group closed.

The recorded 10677-file source and original official-owner snapshots matched before/after. Full source and all frozen SHA guards ran at initial/final; other boundaries checked every tracked file/stat6, owner identity and every frozen bound metadata field plus actual-module SHA. Ten before/after consumption checks hashed only the consumed new witness and executable. All 242 external witnesses and the actual module retained their bindings and modes; this is no claim of universal filesystem write protection. The run overlapped work on separate targets and makes no idle-host or performance claim.

This is historical **a209/99b Mac coverage**. The later local `1a8a859a…/7373315…` source carry does not rewrite the recorded owner snapshot or establish a current737 Mac gate. No full Voice, native ARM64 or device runtime is claimed here.
