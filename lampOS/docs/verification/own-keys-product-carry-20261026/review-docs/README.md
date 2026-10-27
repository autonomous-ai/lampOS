# Own-keys review documentation correction

The product units already enable own keys in this branch. Acceptance is a prerequisite to merging the activation PR, not a runtime gate. The PR remains draft/HOLD for #2094 and fresh current-base lamp acceptance. A retained Voice-only off override leaves Setup and Cloud enabled: they may store keys that Voice ignores, including after its restart. All three units must be off to disable the feature throughout the lamp.

Only own-keys.md and switches.md change after tested parent d71a423ca. The switch census now counts the actual product setting as on; its merge decision remains held. The current Voice product unit replaces the stale drop-in loading reference.

Formatting and strict all-target Voice-eval Clippy pass. The compiled switch-census reader passes **2/0**, with 236 other cases filtered. Commands and exact source/log hashes are in results.json. No runtime source or unit changes, full-suite rerun, device/key action or activation acceptance is claimed. Original native evidence remains on d71a423ca; this amended head requires its own checks.
