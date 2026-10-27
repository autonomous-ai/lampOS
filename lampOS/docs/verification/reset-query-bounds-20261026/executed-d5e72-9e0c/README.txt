Reset exact-head local qualification
Head: d5e72ea9bf30d9d50adf38b1a4fd986f0ae6191a
Base: 9e0ccb206ee796ef46929bf463c5176f3a493ed9
Prior target boundary: ebd641e1fb4ccc0fbb250287c4428013389c61db

setup-cloud:
  test result: ok. 166 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 35.08s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.83s
  test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.45s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 163 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 13.80s
  test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.85s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

Commands (absolute Cargo is the reviewer installation of cargo +1.96.0):
  '<cargo>' +1.96.0 metadata --locked --offline --format-version 1 --filter-platform aarch64-apple-darwin
  '<cargo>' +1.96.0 fmt --all --check
  '<cargo>' +1.96.0 clippy --locked --offline -p lamp-setup -p lamp-cloud --all-targets --message-format=short -- -D warnings
  '<cargo>' +1.96.0 test --locked --offline -p lamp-setup -p lamp-cloud --no-run --message-format=json
  '<cargo>' +1.96.0 test --locked --offline -p lamp-setup -p lamp-cloud --no-fail-fast -- --test-threads=1

The initial independent carry-preflight expected test.sh; the actual third changed file is stand-in-test.sh. That path-expectation error is preserved and was corrected without source or runtime retry.
Source/head/status/Cargo configuration guards passed. Compiler JSON bound actual test executables; local output and depfile copies were preserved and hashed around runtime. Each scope closed before subsequent compilation. Receipts retain their original log hashes; original logs and artifact payloads remain local. This sanitized packet contains no binary or depfile payload.
The latest prior actual/preserved target boundary is successful ebd / 5982 serial Setup+Cloud: 389 passed, 0 failed, 5 ignored. Its actual and preserved outputs, depfiles, executables and receipts were reverified before any compiler. Old ff03 / 737 is historical only: 388 passed, 1 failed, 5 ignored (runtime exit 101), wake observation 163.063834 ms against unchanged 150 ms assertion. Historical preserved copies/receipts were rechecked; its overwritten actual paths were not reasserted.
This is the first full qualification of the new d5e72ea9 / 9e0c composition, with one runtime test thread. A serial pass neither proves the earlier failure cause nor waives that failure. Earlier ebd/5982 native 1,452 PASS remains a separate source identity. No threshold, assertion or source change; no negative/base probe, automatic retry, workspace gate, local ARM or device execution.
