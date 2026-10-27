# Fixed updater Round target

For #3796 and draft #3843, the first complete fixed-source Round invocation passed **212 / 0 / 0 once**, including all fourteen new regressions. All original six control cases are unchanged. The before-control remains 2 passes and four exact refusals; these fixed observations are separate.

[Raw output](round.stdout.txt) and [audit](audit.json) bind every case, the command, source identity, native exit zero and final guards. All tracked source stayed unchanged; all 511 sealed witnesses and 102 current executable bindings passed, and owned groups closed. The candidate was compiled before commit; all 10,802 tracked file bytes and modes subsequently matched offered `ee6f88d2a` exactly. This is not a claim that those local binaries were built after that commit.

The original twelve-package host command remains failed/incomplete on a separately reproduced Main Body socket deadlock. This target had never started in that command. No completed case was replayed to obtain this result. The later Round result does not waive the original failure or qualify unstarted drivers/docs. Native Linux/ARM64 and product-update acceptance are separate, pending checks. No ARM build or device action ran on the Mac.
