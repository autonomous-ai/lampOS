# Skills tool-call observations, 26 October 2026

PR3591, head `7182777ea` on `c121bc16eb`, changes only the existing tool-call test’s observations and failure context. It retains ten calls, all waits/timeouts and the 25 ms median assertion. The measured call interval ends before diagnostic output.

Fresh 48-local-package qualification on the 830 source passed fmt, strict Skills Clippy, workspace build and test compilation. The final focused test passed once with complete ten-row output. A necessary preceding observation-only version passed but interleaved stderr; its private receipt remains separate. The current Body/docs base carry passed compiler gates and retained all six saved executable bytes. Only integration driver and Skills child execute in the focused test; its runtime inputs are private synthetic skills, not repository Body files.

`current-c121.json` binds the current commit to the actual runtime source/file hash. `focused-host.txt` records catalog observation 787.720 ms after spawn, first call 256.614 microseconds and median 256.614 microseconds. These host observations do not establish physical latency or the cause of the original timeout.

The unpublished reminder candidate’s 538/4/9 run stays failed. No source failure, retry or waiver is hidden by this diagnostic PR; it changes no product behavior, threshold, protocol, dependency or switch.

## Deferred-output correction

Current `12f677377` on `22a2ddd79` buffers observations during calls and flushes them only on scope exit/unwind. This resolves the author finding that 718’s synchronous diagnostics could alter arrival timing. The original 718 results above remain historical and held. `native-718-held.json` preserves their original native qualification without clearing that finding.

`deferred-12f-host.json` binds fresh compiler checks and focused 1/0 to the committed source, with six stable executable hashes and an unchanged product child. `deferred-focused-host.txt` records all ten complete deferred rows: catalog observed 906.311 ms, first call 605.106 microseconds, median 423.096 microseconds. No diagnostic I/O occurs in the call loop. These observations do not clear the original reminder failures or establish their cause. New-head native/read gates remain separate.
