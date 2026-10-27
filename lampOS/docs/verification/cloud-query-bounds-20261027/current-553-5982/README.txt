Cloud exact-head local qualification
Head: 553e0d653eb9557a7b804e8fb6c469c102159808
Base: 5982e4b7fa465177f55697ddb362d78b964809b7
Prior target boundary: eee85fa51cff7a7d910158bba8c795b03cae8c76

cloud:
  test result: ok. 174 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 6.52s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.31s
  test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

Commands (absolute Cargo is the reviewer installation of cargo +1.96.0):
  '<cargo>' +1.96.0 metadata --locked --offline --format-version 1 --filter-platform aarch64-apple-darwin
  '<cargo>' +1.96.0 fmt --all --check
  '<cargo>' +1.96.0 clippy --locked --offline -p lamp-cloud --all-targets --message-format=short -- -D warnings
  '<cargo>' +1.96.0 test --locked --offline -p lamp-cloud --no-run --message-format=json
  '<cargo>' +1.96.0 test --locked --offline -p lamp-cloud --no-fail-fast -- --test-threads=16

Source/head/status/Cargo configuration guards passed. Compiler JSON bound actual test executables; local output and depfile copies were preserved and hashed around runtime. Each scope closed before subsequent compilation. Receipts retain their original log hashes; original logs and artifact payloads remain local. This sanitized packet contains no binary or depfile payload.
The latest eee / 33d5 completed Cloud scope actual paths, preserved outputs, depfiles, executables and receipts were reverified before compilation. Earlier 6bed / 0811, 129 and sandbox-failed results remain historical; their overwritten actual paths were not reasserted.
This run is full Cloud only. No negative/base probe, other-crate runtime, workspace gate, local ARM or device execution.
