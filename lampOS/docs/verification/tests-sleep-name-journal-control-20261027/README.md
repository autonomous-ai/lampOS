# Sleeping-name journal fixture: paired control

On pinned `bdc277e266`, the old fixture hit its unchanged 15-second host watchdog **3/3** times. The proposed fixture passed the original status and exact single-journal-line checks **3/3**, including all 25 post-checks. [audit.json](audit.json) binds source, private overlays, artifacts and raw receipts.

| Pair | Order | Old | New |
| --- | --- | --- | --- |
| 1 | old → new | Watchdog, 15.097 s | Pass, 5.535 s |
| 2 | new → old | Watchdog, 15.108 s | Pass, 5.483 s |
| 3 | old → new | Watchdog, 15.111 s | Pass, 5.454 s |

The table records supervisor command wall time, including startup and process-group closure; the inner deadline still starts after child spawn.

Both versions received the same private 75 ms prepublication delay and 20 ms Room-to-Voice delay. Measured publication spans were 21–102 ms. Trace writes occurred after the publication boundary and did not enter the journal. These synthetic delays and observers are absent from the plain proposal.

The new setup first admits fresh absence evidence, crosses the 60-second threshold on its injected clock with fresh inputs, then ends on observed `asleep`. In these observations sleep was first reported at step 22; each new run completed 48 steps. This preserves the settling event instead of assuming sleep immediately after the jump. The ordinary 200 ms step, real bus lifetimes, freshness, cache periods and original watchdog start/bound remain unchanged.

Both variants compiled before the fixed six-run sequence, using one isolated source-copy path and target. Each frozen runtime re-executed its own child. Profiles/features/source paths matched, and the normal helper had the same SHA-256 across variants. All three successful journals had exactly the same 85 bytes. Source and artifact guards passed; the private plain candidate was restored and the lease released.

The first controller stopped at metadata with a process-observation permission error: **zero compilation and runtime observations**. Its original INCONCLUSIVE receipt remains unchanged. A separate read-only absence audit and lease disposition permitted the fresh v2 continuation; no direct wait status was reconstructed and no runtime was retried.

This is a controlled fixture-budget result, not proof of the historical cause of earlier flakes. It does not waive the failed Ready full Mac 338/2/0 command, qualify product behavior, or replace full Expression, latest-base, Linux/ARM64 or device gates. The later plain-candidate gate on `2bf5122dc` is separate and receives no credit from this control.
