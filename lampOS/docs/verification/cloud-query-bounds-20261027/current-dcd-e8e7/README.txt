Cloud exact-head local qualification
Head: dcd4b9b2dfc917714d0718e48f71b2af96499eef
Base: e8e7ac36a74062f567b4afb3d64beef6002d1f5c
Prior target boundary: 78a35fe5d013f28dd62ab36468d6b3149abafc77

cloud:
  test result: ok. 174 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 6.58s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.19s
  test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

Commands (absolute Cargo is the reviewer installation of cargo +1.96.0):
  '<cargo>' +1.96.0 metadata --locked --offline --format-version 1 --filter-platform aarch64-apple-darwin
  '<cargo>' +1.96.0 fmt --all --check
  '<cargo>' +1.96.0 clippy --locked --offline -p lamp-cloud --all-targets --message-format=short -- -D warnings
  '<cargo>' +1.96.0 test --locked --offline -p lamp-cloud --no-run --message-format=json
  '<cargo>' +1.96.0 test --locked --offline -p lamp-cloud --no-fail-fast -- --test-threads=16

The earlier 9e0 carry-preflight filename expectation error remains separate historical evidence. This e8e7 preflight passed; no current source or runtime retry occurred. The prior 9e0 executed heads were unpublished and remain historical source identities.
Source/head/status/Cargo configuration guards passed. Compiler JSON bound actual test executables; local output and depfile copies were preserved and hashed around runtime. Each scope closed before subsequent compilation. Receipts retain their original log hashes; original logs and artifact payloads remain local. This sanitized packet contains no binary or depfile payload.
The latest 78a35 / 9e0c completed Cloud scope actual paths, preserved outputs, depfiles, executables and receipts were reverified before compilation. Earlier 553 / 5982, eee / 33d5, 6bed / 0811, 129 and sandbox-failed results remain historical; their overwritten actual paths were not reasserted.
This run is full Cloud only. Old 553/5982 native evidence is separate: root reported CI success with 11,458 PASS and one SameVoice LEAK, retained with Tests #3572. That old case was not rerun or waived by this new-composition host pass. No negative/base probe, other-crate runtime, workspace gate, local ARM or device execution.
