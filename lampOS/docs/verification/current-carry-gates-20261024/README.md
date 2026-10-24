# Mac gates after integration carries

Actions `ab5662a4a`: fmt, strict scoped Clippy and **120/0/2 ignored** pass.
Rested `fe04835b0` and package `618644e2a`: fmt, strict scoped Skills Clippy
and **514/0/7 ignored** pass each. Full tracked lampOS source manifests
match before/after, with respective counts 6,994, 6,974 and 6,974.

The reminder carry `aff8b0e95` is retained as **failed**: 4,136 passes, one
failure, 26 ignored before Cargo stopped. The failed case is Voice
`provider_worker::tests::held_consent::asked_about_reports_after_a_held_stop_it_says_who_and_when`,
which returned the reports-not-running error instead of the expected field.
Its source is outside this test-only PR. This receipt does not establish a
base comparison or a passing full suite. #3312 subsequently landed through
the train; this unpublished carry was not pushed over the landed branch.

Cloud `2ad786f91`: fmt, strict Cloud Clippy and **196/0/4 ignored** pass.
Its tracked source manifest covers the original 6,967 files; the new fixture
and workflow were untracked during the host gate, so their six-file frozen
hash record was checked separately after Clippy and tests. The manifest
alone must not be described as covering those new files.

Commands and timing are in each result.json. No ignored actual-release case
ran on the Mac. Full logs are preserved except private home prefixes, whose
original/published hashes and replacement counts are in evidence-map.json.
Official native execution, integration tests and physical acceptance are
separate requirements. No failed attempt has been converted to a pass.
