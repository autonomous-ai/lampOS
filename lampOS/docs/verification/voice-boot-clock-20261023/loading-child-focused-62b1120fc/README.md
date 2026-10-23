# Loading-child fixture: focused native verification

The corrected fixture observes a real child held before `Ready` through all three original line checks, then checks its release and supervisor reaping. The missing-start control passed with the old fixture and failed at the startup check with the corrected fixture. The corrected normal case passed once. Formatting and strict all-target Voice Clippy passed. **Full Voice, dependent-package and ARM64 qualification for this P3 remain pending.** This receipt does not supersede earlier failed full gates or qualify the separate 86-case clock change again.

The source is the two-file uncommitted P3 overlay on `8ae8651871ed6b6a888157836575b4d994d6722c`, carried onto `62b1120fc1af5462529f0c76089a322500e30929`. The overlay changes only `crates/voice/src/provider_worker/tests/spoken_to.rs` and the corresponding paragraph in `docs/voice-test-fixtures.md`. The complete [candidate patch](patches/candidate.patch.json) and [source proof](source-proof.json) identify the exact tested bytes. The selected case is outside the 86 clock cases.

## Fixed observations

Each row ran exactly once in its own compiled libtest process. Compilation is excluded from the wall times below. Both missing-start controls replace only the `options.voices` assignment with `None`; they do not alter product code or the original line assertions.

| Observation | Actual result | Wall time |
| --- | --- | ---: |
| Old fixture, missing start | 1 passed; the missing child was not detected | 0.386608 s |
| Corrected fixture, missing start | 1 failed at `child.held().unwrap()`: `loading child did not publish its complete PID` | 5.054915 s |
| Corrected fixture, normal child | 1 passed, including held probes and explicit release/reaping checks | 0.933891 s |

The exact case is `provider_worker::tests::spoken_to::a_child_still_loading_its_model_takes_nothing_from_the_worker`. The corrected negative is an intended test failure, not a passing libtest result. It reached no original line assertion. No child was launched in either missing-start control; the corrected guard removed its unused private directory on unwind. No observation timed out at the external 30-second process-group watchdog, and none was repeated.

The static audit compares all 79 original assertion macro bodies byte-for-byte, including the selected test's one assertion executed for its three unchanged voice/text inputs. All Rust bytes outside `LoadingChild`, its small Printer delegate, and the selected case remain identical. The original 300-block line inputs, logical-clock behavior and production deadlines remain unchanged.

## Startup and cleanup boundary

The helper uses the existing shared test guard before starting the real asynchronous `Prints`. A private shell writes a complete newline-terminated positive PID, opens a FIFO and acknowledges numbered, PID-tagged probes without emitting `Ready`. Each check requires the same owned child to be live, using non-consuming `waitid` observation, and a complete matching probe. Its five-second host-clock watchdog does not advance paused Tokio time. The fixed `sleep 10` hold is removed.

Cleanup closes the real request sender before releasing the FIFO, observes the release acknowledgement and waits for the supervisor to reap the child without consuming its status. It sends no PID-based signal. The normal raw output retains the production diagnostic saying the child ended before loading and is scheduled for restart in 5000 ms. That diagnostic precedes the unchanged supervisor's channel check; the already-closed request sender makes that check return without another child. The detached supervisor has no join handle: this verifies child reaping, not a thread join. A startup/reaping cleanup watchdog failure remains a failure and retains the abort-marked directory; it is not reported as successful cleanup. Only the no-start unwind and normal cleanup paths were executed here.

## Gates and retained failures

The first formatter launch failed before Cargo started because the execution PATH lacked Cargo. The next actual `cargo fmt --all --check` failed on five helper-only style differences. Formatting the owned Rust file corrected those differences; the full formatter then passed. The first new-control compilation subsequently failed on three unqualified `Arc` references. That failure produced no test execution. The approved three-name qualification is preserved as its own [patch](patches/arc-qualification.patch.json). The continuation did not repeat the completed old control.

After that correction, `cargo fmt --all --check` passed in 12.035213 s and `cargo clippy -p lamp-voice --all-targets --offline --locked --message-format=json-render-diagnostics -- -D warnings` passed in 86.291849 s. Both runner terminations verified exact whole-source restoration and `git diff --check` exit 0. The earlier setup, formatter and compiler failures remain in [results](results.json), [commands](commands.json) and the raw receipts.

The three binaries have equal complete 3,621-name inventories. [Artifact bindings](artifacts.json) retain their distinct SHA256s, actual Cargo artifact records, selected-case occurrence, ignored inventory and `fresh=false` compilation evidence. Rust 1.96.0 reports `x86_64-apple-darwin`; captured `file` output identifies Mach-O x86_64 executables. Physical hardware and translation mode were not established. The declared Cargo environment used four compiler jobs, debug information disabled, incremental compilation disabled, offline dependencies and the exclusive MAIN target. Full PATH digests differ between runner invocations; the captured compiler identity and declared variables are retained. This is not a matched-host-load or product-latency study.

## Receipt contents and limits

[Raw index](raw-index.json) binds every stream to its original path, size, SHA256 and exact retained byte offsets. [Initial output](raw/initial.txt.json) retains all early failure diagnostics and the old observation. [Corrected output](raw/corrected.txt) retains the new negative, normal case and final gates. [Inventory output](raw/inventory.txt) retains the full inventories once when byte-identical; references explicitly identify deduplicated streams. Wrappers place closing markers outside the original bytes. The initial-output wrapper and three patches are stored as JSON string envelopes so legitimate context-only spaces and raw diagnostic whitespace are preserved without failing the default Git whitespace check.

Only complete recognized Cargo machine JSON records are omitted from the public raw bundles. Their original stream and omitted-range hashes remain recorded; all human stdout/stderr bytes are retained. The complete Cargo streams, 10,002-entry source maps and runner scripts remain at the external paths recorded in this receipt and are not portable public files. The selected source hashes, aggregate restoration proof and portable patches remain here. No executable, Python script, Cargo machine stream, credentials or personal attribution is included. The initial pre-Cargo Python traceback was returned by the execution tool, not saved as a child stream; its structured setup-failure record identifies that limitation rather than inventing raw output.

[Validation](validation.json) records the byte-range, hash, source, assertion and inventory checks. [SHA256SUMS](SHA256SUMS) seals the final public files. Full qualification is still required after the dependent relay fixture correction; this focused result makes no readiness or inherited-failure claim.

## Decoding lossless envelopes

For a `*.patch.json` or `initial.txt.json` file, parse JSON, read its `data` string and encode that string as UTF-8 with no added newline. Check the resulting byte length against `decoded_bytes` and its SHA256 against `decoded_sha256`. Those bytes are exactly the original patch or raw wrapper. Apply a decoded patch normally. In `raw-index.json`, entries with `decoding` use offsets into these decoded bytes, not offsets into the JSON document. All source-stream paths, original stream hashes, omitted Cargo ranges and span hashes remain unchanged. [Packaging revision](packaging-revision.json) identifies the untouched sealed original and each conversion.
