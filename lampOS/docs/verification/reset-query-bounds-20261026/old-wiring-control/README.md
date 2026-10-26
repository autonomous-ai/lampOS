# Reset-query wiring negative control

Original commit 8a050d1bfd098769a8361849171271964a5082f7 remained clean and source-identical before and after. A local git archive of that exact commit was verified against the original source inventory, then only scratch System::read was restored to its former Command::output body. The query helper and every test were unchanged. No deadlines were disabled.

The isolated target was absent and atomically empty immediately before compilation. Matching Setup --lib exact test selection compiled with exit 0. One runtime execution under a 120-second deadline and at most 15-second cleanup failed as expected (Cargo exit 101):

`test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 22 filtered out; finished in 0.01s`

The sole case was reset::query::tests::system_read_enforces_the_query_output_bound. Its overflow unwrap_err received Ok, demonstrating that reverting the production wiring is caught even with the bounded helper still present. Runtime command duration 1.186s. The 65KiB panic payload is deliberately omitted; its raw log hash and sanitized failure description are retained. This is an expected mutant failure, not a failure or rerun of the original implementation.

Closure checks covered 1 actual driver, 15 actual/preserved local compiler outputs and 139 depfiles, plus both mutant and original source identities. No original worktree edit, commit, push, device or provider action occurred. No additional cases or automatic retries ran. This control qualifies the cap wiring; it does not independently prove the default five-second deadline or installed acceptance.
