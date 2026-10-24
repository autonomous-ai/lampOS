# Existing native updater self-switch controls

Selected from original ARM64 run 40130968164 at authored fa82c279e/bcb,
whose complete tested/authored tree binding is in the mock Eyes native proof.
Actual CLI exec confirmation passes 0.146s; persisted self-switch SIGKILL batch
passes 3.893s. The source declares seven fsync-return cut points and runs all
seven in that named batch. This is batch evidence, not seven separately
visible successful stdout records. No new execution or experiment occurred.

It proves the existing process recovery fixture and CLI confirmation within
their documented scope. It does not establish real power-cut/flash durability,
a real recovery timer, published real-key trust or attended lamp acceptance.
The a039/3d Voice carry gate remains a separate ongoing execution.
