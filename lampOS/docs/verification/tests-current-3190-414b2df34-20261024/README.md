# #3190 carried-head local qualification

Tested head `414b2df34f5398d164affb128780fdad2ba78d63`, parent `9ed44f88c573837c50451c94f4d88f010e8a6721`.

The full x86 Mac run completed **7,249 passed / 3 failed / 34 ignored**. Voice was **3,976/1/15**, including its library **3,696/0/15**; eleven readers were **3,273/2/19**. Every planned target completed. Formatting, strict twelve-package Clippy, workspace build and diff/source guards passed. All 88 authored fixture cases passed.

[Full run](candidate/README.md) retains all three failures and exact diagnostic bytes. [Current failure classification](current-standalone/current-failure-review.json) and [six actual standalone observations](current-standalone/results.json) apply the accepted rule: the light-call identity/predicate failed on the frozen matched base too; both additional Memory failures pass exactly three standalone observations at this carried head. These categories do not prove a historical cause or an improvement in timing.

The [frozen matched comparison](../tests-qualification-pair-4a84fa5ab-20261024/) remains separately bound to its original base/candidate identities. No new full baseline was run for this carry. Exact-source Linux x86 formatting receipts for all three owned carries are under `linux-formatting/`; no compilation or test execution is claimed by those formatting receipts.

Native ARM64 and Linux Clippy are pending a source-branch push. No local ARM build or lamp action ran. The train now owns testing a carried PR against the newest integration base. The source remains a test fixture change with no product default or assertion change.

The standalone controller is retained as exact UTF-8 inside a JSON evidence record; no executable measurement script is added to lampOS. The original pre-execution controller metadata error is retained and executed no test.
