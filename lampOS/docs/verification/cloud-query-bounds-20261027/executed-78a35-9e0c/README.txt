Cloud exact-head local qualification
Head: 78a35fe5d013f28dd62ab36468d6b3149abafc77
Base: 9e0ccb206ee796ef46929bf463c5176f3a493ed9
Prior target boundary: 553e0d653eb9557a7b804e8fb6c469c102159808

cloud:
  test result: ok. 174 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 6.58s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.57s
  test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

Commands (absolute Cargo is the reviewer installation of cargo +1.96.0):
  '<cargo>' +1.96.0 metadata --locked --offline --format-version 1 --filter-platform aarch64-apple-darwin
  '<cargo>' +1.96.0 fmt --all --check
  '<cargo>' +1.96.0 clippy --locked --offline -p lamp-cloud --all-targets --message-format=short -- -D warnings
  '<cargo>' +1.96.0 test --locked --offline -p lamp-cloud --no-run --message-format=json
  '<cargo>' +1.96.0 test --locked --offline -p lamp-cloud --no-fail-fast -- --test-threads=16

The initial independent carry-preflight expected test.sh; the actual third changed file is stand-in-test.sh. That path-expectation error is preserved and was corrected without source or runtime retry.
Source/head/status/Cargo configuration guards passed. Compiler JSON bound actual test executables; local output and depfile copies were preserved and hashed around runtime. Each scope closed before subsequent compilation. Receipts retain their original log hashes; original logs and artifact payloads remain local. This sanitized packet contains no binary or depfile payload.
The latest 553 / 5982 completed Cloud scope actual paths, preserved outputs, depfiles, executables and receipts were reverified before compilation. Earlier eee / 33d5, 6bed / 0811, 129 and sandbox-failed results remain historical; their overwritten actual paths were not reasserted.
This run is full Cloud only. Old 553/5982 native evidence is separate: root reported CI success with 11,458 PASS and one SameVoice LEAK, retained with Tests #3572. That old case was not rerun or waived by this new-composition host pass. No negative/base probe, other-crate runtime, workspace gate, local ARM or device execution.
