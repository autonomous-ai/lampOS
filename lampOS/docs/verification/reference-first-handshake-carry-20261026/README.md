# Current first-reference handshake carry

Head `250fd8673e53d1ac63aa7165b52a729fa2a00a30`, base `690dff041e` (full base in results). Original twelve-crate 7,427/0/52 execution remains on 7c483e0f9, base 1cad670df. The factory carry ea5b57924 on 1aa409fcb changed only two embedded Update test drivers out of 102 compiler executable bindings: 100 matched the original. All five authored file blobs and the patch stayed identical.

On ea5b57924: fmt, strict twelve-crate Clippy, matched twelve-crate compilation, both factory shell syntax checks passed. Factory completed **94 passed / 0 failed**. Fresh, matching Update package/target compile and runtime selections completed **222 passed / 0 failed / 2 ignored**. The three targeted executable bindings and 7,236 source entries were unchanged afterward. These overlapping counts are not added to 7,427.

The initial factory invocation was interrupted to correct the planned Update compile/runtime target selection before running Update. Its partial output is not counted. The 102-path guard preceded the targeted rebuild; afterward only the three freshly compiled bindings are claimed. The continuation receipt preserves that distinction.

Final carry from ea5b57924 to 250fd8673 changes only docs/build-rows.md. Formatting and strict twelve-crate Clippy passed again; source and the three targeted executable hashes remained unchanged. No test count is relabeled as fresh execution on the final scorecard-only carry. Original PR ARM64 and Linux Clippy remain required. No hardware, setup repeat, provider or acoustic acceptance ran.
