Reset exact-head local qualification
Head: bd40bc557766f1d7bafdf41d7dffdabfb7a0fffe
Base: 33d5627cadad7184f9ea2dcdc5b858f3aa3de58d
Prior target boundary: ff03ad11f3884e4ed09bb5538a6ba2a495891124

setup-cloud:
  test result: ok. 166 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 34.90s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.87s
  test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.44s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 163 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 13.85s
  test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.89s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

Commands (absolute Cargo is the reviewer installation of cargo +1.96.0):
  '<cargo>' +1.96.0 metadata --locked --offline --format-version 1 --filter-platform aarch64-apple-darwin
  '<cargo>' +1.96.0 fmt --all --check
  '<cargo>' +1.96.0 clippy --locked --offline -p lamp-setup -p lamp-cloud --all-targets --message-format=short -- -D warnings
  '<cargo>' +1.96.0 test --locked --offline -p lamp-setup -p lamp-cloud --no-run --message-format=json
  '<cargo>' +1.96.0 test --locked --offline -p lamp-setup -p lamp-cloud --no-fail-fast -- --test-threads=1

Source/head/status/Cargo configuration guards passed. Compiler JSON bound actual test executables; local output and depfile copies were preserved and hashed around runtime. Each scope closed before subsequent compilation. Receipts retain their original log hashes; original logs and artifact payloads remain local. This sanitized packet contains no binary or depfile payload.
The prior ff03 / 737 Setup+Cloud boundary FAILED with 388 passed, 1 failed, 5 ignored (runtime exit 101); its source and actual/preserved artifact closure passed. That failure and its receipts remain preserved. The failing wake test measured 163.063834 ms against the unchanged 150 ms assertion.
This is the first full qualification of the new bd40 / 33d5 composition, with one runtime test thread. A serial pass neither proves the earlier failure cause nor waives that failure. No threshold, assertion or source change; no negative/base probe, automatic retry, workspace gate, local ARM or device execution.
