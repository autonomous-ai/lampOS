# Second reader: build rows at 06:02

Reviewed exact head `207f301b44c3c293afcae7ea9f291bc4b3a2bd88`, parent `da57072b2e6420ae25cf6779f1ee1756178ce724`. Only `docs/build-rows.md` changes; diff whitespace check passes.

Independent decimal arithmetic: 70 shipping rows sum to 65.02, averaging 92.885714...% (92.9%); two excluded mood rows sum to 1.17, averaging 58.5%; 16 quality rows now sum to 10.00, averaging 62.5%. The sole quality increase of 0.10 changes its sum from 9.90.

The changed soft-start row agrees with the landed product switch and 30 additional 10 ms blocks in `activity::SOFT_START_LEAD_BLOCKS`. This is the conditional software lead budget, bounded by retained PCM and lamp-silent evidence; it does not establish 300 ms of physical speech recovered on every line or an isolated causal quality improvement. The score is the team's assessment; this read verifies arithmetic and consistency, not an independent quality measurement.

No P1/P2 finding. No compilation, runtime test, playback or device action was needed for this Markdown-only path.
