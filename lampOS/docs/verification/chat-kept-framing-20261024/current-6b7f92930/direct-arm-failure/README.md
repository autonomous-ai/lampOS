# Separate direct ARM failure after train landing

Direct run40111245085 on feature95a fails: **1,139 passed / 1 failed / 53 skipped / 6,235 selected unrun**. The shared SleepingService step fails its counts.refused==0 precondition, observing2 at Expression runtime.rs:764. This is not an observation of two look-ups or a demonstrated privacy-output fault. No cause, waiver or rerun. No framing-control pass is credited from this fail-fast run.

Expression fixture/source is unchanged from base. Tests already owns the SleepingService fixture work; exact observation was handed there. Train96 independently passed the identical complete tree, but that result does not replace this failed observation.
