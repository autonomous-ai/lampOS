# Original #3544 native audit at 4084e4e1d

Original ARM64 and Linux Clippy passed. Readiness and current-base/physical decisions remain separate; this audit changed no draft state.

Published head: `4084e4e1dde24ea17d6c88d944256f78904579c0`, authored on frozen `7400e4b95`. Both original runs checked out merge `312cca2005cd73966b74383793223375ff6f0b4a`, tree `fb089fd4e8bd8c6adeebd24f2c76a5c9baacc598`, with parents `28e7a63f238ed1f7078d15eb0eed705ea22baad4` and the published head. The merge tree is not the authored tree. All three owned file blobs and modes match the published head. All 23 required hand/transport source bodies and all 16 privacy cohort bodies match their respective prior ledgers. Of 16 source files inspected, only upstream `remember.rs` differs; no whole-tree or whole-binary equivalence was assumed.

## Original terminal results

- ARM `40549421469`, attempt 1: native watch 15163 exit 0, chunk 717145, completed 17:35:29 UTC. **7,791 unique outer passes, zero bad terminals, duplicates, retries or selected unrun.** The summary reports 53 skips without a per-case skip catalog.
- Linux Clippy `40549421398`, attempt 1: native watch 85962 exit 0, chunk 4de188, completed 17:27:33 UTC.
- All **23 hand-required controls** passed exactly once, including the Linux-only transport control that was not credited on Mac.
- Privacy: all **nine Voice controls passed once**. The logged PR selection chose 12 of 47 crates and excluded Bus and Msgs; their seven named controls were not selected, not ignored or credited. The corresponding source bodies match, but source equality is not execution.
- Separate from nextest: **47 doctest targets, four passes; relay one aggregate success; process harness 13 passed/zero failed.**

The original run selected 7,791 tests across 70 binaries; 120 binaries were outside selection. `native-selection.json` retains the actual logged package filter. The parser uses the numerator for partial N/M summaries and verifies it against unique terminal rows.

The N13 journal-conversation fixture passed once in 0.012 s. Its body is exact to the earlier c0cf failed fixture and is not #3891's repair. This pass neither explains nor erases the prior failure. The source-based ordering diagnosis remains historical; no rerun was used to obtain this result.

## Artifact limits

The export step reports 48 workspace binaries and two examples with complete SHA256SUMS. Artifact `12753979998` is 118,416,117 compressed bytes. API and upload log agree on ZIP SHA256 `5c5344be6d7aa4173dc3561ff266033d9b7a58a61ea081f280368a6f844d77d1`.

No archive was downloaded in this task. Payload hashes, CRCs and ELF headers were not independently checked; there is no new Voice/Eyes pin pair, installed-build claim, physical acceptance or stable-feed publication disposition. Original logs are privately retained within the 32 MiB compressed/64 MiB expanded log caps.

The local 4,163-pass result, four doctests and 22 host-required controls remain separately bound to the tested tree `29fb356e324483a4d2c08ab2e68a53e2da865d12` on frozen 7400 and public proof 51d718a8d1. None was added to the native count. Prior failures and the zero-body controller 125 were not relabelled.

## New coordination requiring action

Final read covered all 18 Queue bodies since the prior inclusive boundary 6448740526 through 6449073151 at 17:35:12 UTC, plus all new/edited comments and review channels on Fourth's seven PRs and #3898 responses. Current Main remained `3aa752f73914d0f29fc90edb010a3a7fa1cf526b`; #3891 remained OPEN/unmerged, still next in the lead's queue. AGENTS SHA256 stayed `0954b38728a27cbe4413e5a2b8898ad8a5b1fad83f594ffabcaa81c18a7f1dad`.

- **#3472 activation P2, 6448849961 / Queue 6448851347:** Reviewer-two holds c0cf because initial enrollment has no caller-bound consent admission; own name/proof/lease alone is insufficient. Required negatives are name-only first answer before an invitation, model-initiated save without yes, earlier/other speaker's yes and canceled consent, with a same-speaker consent-and-name positive. The separate #3893 device report is not c0cf device evidence. The previously prepared documentation-only carry does not fix this. Reviewer-three raised the policy conflict at 6448789263; Eighth offers a policy-dependent follow-up at 6448853319. No new assignment or waiver was inferred.
- **#3544 source read, 6448935739:** Reviewer-two reports no new P1/P2, exact authored carry and all eleven privacy source files matching accepted #3315. The narrow kept-resync fixture prerequisite is explicitly cleared for this source under 6444805122; old failed cohorts and other privacy/Common/device conditions remain. Fresh native and current composition remain separate.
- **#3898, 6449002588 / Queue 6449003131:** Fourth retired the standalone polled-counter HOLD on 4f9d23ccb and keeps immutable job lifetime/unknown-frame refusal in Common's composition. Fourteen's 6449072216 closes their old HOLD and gives READ-OK. No Common qualification is inferred.
- Friday 6448993084 asks for #3891 at the front of the train, citing repeated N13 failures; this is not clearance of a failed gate. Nine 6448986303 reports another Main N13 failure with no artifact and waits for a usable build. The earlier publication rejection is not resolved by these messages.
- Other read updates retain Second's #3892 original mutation failure and corrected-source gate, Third's #3646 current native audit and #3889 repair read, Fourteen's #3902 combining-mark HOLD, and SDK/body #3904 timing/read limits. No Fourth action beyond the direct items above.

The separate c0cf parser-correction receipt is `/private/tmp/fourth-c0cf-offline-count-correction-20261028/receipt.json`, SHA256 `e07283b797c639c249c41b720f65edc3fa6bd08567bf1e8f15c7eed2bc3b0d9c`. It preserves old files and corrects only the earlier JSON's executed/unrun denominator error to 9,068/2,692; old prose and failed outcome were already correct. Root was notified that the public JSON also needs this correction.

No Cargo, source checkout, target use, branch/ref/index mutation, workflow dispatch/rerun, public post, archive execution, lamp action or readiness transition occurred. Every native tool handle used by this audit has closed.
