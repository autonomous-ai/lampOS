# Independent ready-speaking fixture review

READ-OK `5713bece435526832373cc9de38db99a39fc706f` on `cced42943`, no P1/P2. The production prefix and every target assertion are unchanged. Existing callers retain their initial Speaking observation. Only this case opts out, declares its publisher before Ready setup, then publishes the identical Speaking payload once immediately before one step. No polling, retry, expiry extension or refresh loop is introduced. The 500 ms transport lifespan and expiry grace remain unchanged; a stall after publication can still expire the input.

Independent Mac formatting, strict all-target Expression Clippy, compilation and full Expression tests pass **340/0/0**. All 7,220 source entries and eight actual/preserved executable hashes pass final guards. This read did not reproduce the author's private 1,100 ms intervention or run reverse-dependency/runtime/native/device gates. Counts remain on this exact head, not the pending newer-base carry.
