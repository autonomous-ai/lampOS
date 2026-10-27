Reset exact-head local qualification
Head: ebd641e1fb4ccc0fbb250287c4428013389c61db
Base: 5982e4b7fa465177f55697ddb362d78b964809b7
Prior target boundary: bd40bc557766f1d7bafdf41d7dffdabfb7a0fffe

setup-cloud:
  test result: ok. 166 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 35.33s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.84s
  test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.46s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 163 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 13.81s
  test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.91s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

Commands (absolute Cargo is the reviewer installation of cargo +1.96.0):
  '<cargo>' +1.96.0 metadata --locked --offline --format-version 1 --filter-platform aarch64-apple-darwin
  '<cargo>' +1.96.0 fmt --all --check
  '<cargo>' +1.96.0 clippy --locked --offline -p lamp-setup -p lamp-cloud --all-targets --message-format=short -- -D warnings
  '<cargo>' +1.96.0 test --locked --offline -p lamp-setup -p lamp-cloud --no-run --message-format=json
  '<cargo>' +1.96.0 test --locked --offline -p lamp-setup -p lamp-cloud --no-fail-fast -- --test-threads=1

Source/head/status/Cargo configuration guards passed. Compiler JSON bound actual test executables; local output and depfile copies were preserved and hashed around runtime. Each scope closed before subsequent compilation. Receipts retain their original log hashes; original logs and artifact payloads remain local. This sanitized packet contains no binary or depfile payload.
The latest prior actual/preserved target boundary is successful bd40 / 33d5 serial Setup+Cloud: 389 passed, 0 failed, 5 ignored. Its actual and preserved outputs, depfiles, executables and receipts were reverified before any compiler. Old ff03 / 737 is historical only: 388 passed, 1 failed, 5 ignored (runtime exit 101), wake observation 163.063834 ms against unchanged 150 ms assertion. Historical preserved copies/receipts were rechecked; its overwritten actual paths were not reasserted.
This is the first full qualification of the new ebd641e1 / 5982 composition, with one runtime test thread. A serial pass neither proves the earlier failure cause nor waives that failure. No threshold, assertion or source change; no negative/base probe, automatic retry, workspace gate, local ARM or device execution.
