# Independent held product-unit/evidence read

Exact authored 414305564 on fc31; latest-base 42f08 ancestry is absent.
Software unit consistency has no new P1/P2 demonstrated: both units have
SOFT_START on, FIRST_WORDS kept and duplex. CALL_TAKES_NEXT_LINE stays off.
The 83 archived assignments equal the 89 product assignments minus exactly
the six documented exceptions; no duplicate LAMP keys. This checks static
artifacts, not live environment or deployed binary state.

R2 P2 timing-label HOLD 6390200148 is independently confirmed. The timer
starts in answer processing; first provider audio is sent through Shared's
output channel before the journal entry. night.py takes the first associated
journal value per row and calls bool(firsts) answered. It does not attest
physical playout or grade answer correctness. The supplied 22 rows yield
upper-middle 1,864 ms, midpoint median 1,848 ms and maximum 4,733 ms. These are
recomputed from recorded values, not new audio or clock measurements.

Causal precision also matters: both switches changed together in the A/B.
Assigning first-word figures to one and bare-call gains to the other is
mechanistic interpretation, not separately isolated experimental effects.
Keep the repo reference-token scoring, question-word versus wake-token and
route/endpoint confounding limitations stated by the existing reader.
No raw recordings/live state or new single-switch effect was regenerated.

No READ-OK while the timing claim and latest-base requirement are open.
No archived script, Cargo test, runtime experiment, SSH or playback ran.
A corrected current carry can preserve these static checks by explicit
input comparison, with its actual gates and activation disposition separate.
