Update exact-head local qualification
Head: 7dbc4b21eba41af69949fd95f98abd4fa4b774b3
Base: 33d5627cadad7184f9ea2dcdc5b858f3aa3de58d
Prior target boundary: 14981c5ffdeb2dd0b37ef13935564915b6d9679d

update-lib:
  test result: ok. 131 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 4.90s
update-round-reader:
  test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 182 filtered out; finished in 0.35s

Commands (absolute Cargo is the reviewer installation of cargo +1.96.0):
  '<cargo>' +1.96.0 metadata --locked --offline --format-version 1 --filter-platform aarch64-apple-darwin
  '<cargo>' +1.96.0 fmt --all --check
  '<cargo>' +1.96.0 clippy --locked --offline -p lamp-update --all-targets --message-format=short -- -D warnings
  '<cargo>' +1.96.0 test --locked --offline -p lamp-update --lib --no-run --message-format=json
  '<cargo>' +1.96.0 test --locked --offline -p lamp-update --lib --no-fail-fast -- --test-threads=16
  '<cargo>' +1.96.0 test --locked --offline -p lamp-update --test round a_command_is_read_within_its_bound_and_killed_after --no-run --message-format=json
  '<cargo>' +1.96.0 test --locked --offline -p lamp-update --test round a_command_is_read_within_its_bound_and_killed_after --no-fail-fast -- --exact --test-threads=16

Source/head/status/Cargo configuration guards passed. Compiler JSON bound actual test executables; local output and depfile copies were preserved and hashed around runtime. Each scope closed before subsequent compilation. Receipts retain their original log hashes; original logs and artifact payloads remain local. This sanitized packet contains no binary or depfile payload.
The latest 14981 / 0811 final round scope actual paths, preserved outputs, depfiles, executables and receipts were reverified before compilation. Its earlier library scope was checked by preserved copies and its prior closure only; its overwritten actual paths were not reasserted. Historical full Update results retain their own source identity.
This run is full Update library plus the one exact round output reader test. No full round suite, negative/base probe, other-crate runtime, workspace gate, local ARM or device execution.
