# Music b04: cancelled native attempt

PR #3515 head `b04c78803e6fc8180270d91a6052527bc44d0598` had original ARM64 run **40250300313**, attempt 1, cancelled after a newer head was pushed. The run selected 11,330 tests but has no terminal nextest summary and produced no artifact. It is **not passing qualification**. Raw partial logs and final API metadata remain retained privately.

Original Linux Clippy **40250300310**, attempt 1, succeeded on that head. This result belongs only to b04; it does not qualify the later ee9 carry. [outcome.json](outcome.json) binds both runs and their log hashes to the original preview and authored tree. No workflow was manually cancelled or rerun by this audit.
