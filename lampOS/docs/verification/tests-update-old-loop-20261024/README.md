# Old-loop controlled comparison, 24 October

Ready for lamp-voice: no. Full candidate gates are pending.

The private old-loop comparison ran exactly once after its own fresh compile and actual round inventory. The original two-second helper panicked at 2.000353195 seconds while its child remained pending. The held observation was at 2.009615865 seconds, before the scheduled 2.2-second release; no output write occurred, the nonblocking sample returned WouldBlock and the original flags were restored. The gate was cancelled and joined, the child reaped, and the outer group absent.

The Rust test passed because it checked that expected old timeout and the required evidence. This is not a newly observed failure of the original historical test and makes no historical cause claim. The candidate passed the same scheduled startup intervention in its ten directed controls, recorded separately.

All four candidate source files were independently backed up and restored to their exact bytes and modes. Only the private control suffix was applied and removed. Its actual new modification time was recorded; no timestamp was fabricated. Restored source requires fresh compilation before its full gates. No directed control or original diagnostic was rerun by restoration.

The earlier single original diagnostic remains a pass with its probe unused. Its historical two-of-three HOLD is not waived by this comparison. No product behavior, live device, local ARM build or new original-case observation is claimed.
