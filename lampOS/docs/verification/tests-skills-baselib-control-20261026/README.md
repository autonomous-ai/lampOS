# Skills baseline library control

Candidate `91ca92d058adae8ce8992065afadd5e5753c55c8` on parent `1aa409fcbdd1f1c32ae3bb9ad6698f83453dde68` completed one full Skills run: **521 passed, 1 failed, 9 ignored**, with all 531 cases accounted for. The delayed-sample routine test failed at its 50 ms Request receive; the changed addon case passed once. That original failure is preserved.

One baseline control checked out exact Main `1aa409fcb`, compiled only the Skills library, and ran its complete 157-case inventory once: **157 passed, 0 failed, 0 ignored**. The previously failing case passed. No retry or test-bound change occurred.

All 15 direct library inputs, root compiler profile and features match. The 209 matching dependency-artifact keys show no feature differences. However, the executable bytes differ, including the `__TEXT:__text` section. Candidate and baseline binary hashes and sizes are recorded in `control.json`; this is **not byte-equivalent execution**, and the baseline pass establishes neither a cause nor a fix for the candidate failure. The full-package and library-only scopes are distinct; no latency comparison is claimed.

The baseline source, all 169 preexisting protected witnesses, and actual/new frozen executable guards passed. This control grants no qualification clearance, ready status or push approval. Raw receipts remain private; their hashes are published here without local paths or environment values.
