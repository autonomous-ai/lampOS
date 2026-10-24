# Focused observation after a retained Voice failure

The unpublished reminder carry `aff8b0e95` previously failed Voice
`provider_worker::tests::held_consent::asked_about_reports_after_a_held_stop_it_says_who_and_when`
during its combined three-crate run. The original failure remains under
current-carry-gates-20261024/reminder-failed. This comparison does not
replace that failure or establish that it also fails on the base.

Interleaved focused serial runs, base `d7b85b1dd` then candidate `aff8b0e95`,
passed **1/0** each, three observations per head (six total). The test file
is byte-identical on both heads. Exact commands, timings and raw output
hashes are retained. Each observation ran that one test; the other Voice
library tests were filtered. Compilation time is included in wall times.

The failure did not reproduce in these focused conditions. This does not
establish its cause, prove behavior under combined-suite load, or turn the
original combined gate green. #3312 had already landed through the train;
the unpublished failing carry was not pushed. No runtime or test assertion
was changed. Private home prefixes are redacted with original/published
hashes retained, without removing test output.
