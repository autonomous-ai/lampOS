# Package-reader fixture host gates

PR #3330, head `22e5f48736fe9b0e796b3783edee76522b0185c2`, based on
#3323 `6c8c52e37`, including integration base `9ed44f88c`.

Mac Rust 1.96.0 commands, run in order with an isolated task working tree:

- `cargo +1.96.0 fmt --all --check`: exit 0.
- `cargo +1.96.0 clippy --locked --offline -p lamp-skills --all-targets -- -D warnings`: exit 0.
- `cargo +1.96.0 test --locked --offline -p lamp-skills`: **514 passed / 0 failed / 7 ignored / 0 filtered** (157 + 20 + 306 + 31 passes).

The six actual released-reader cases are ignored on Mac, along with one existing
test. These are host compilation and ordinary-unit/integration results, not a
native package-pair pass. The fixture source was frozen throughout the gate;
only its verification README's observed host counts were added afterward.

The exact committed source then passed Linux `cargo fmt --all --check` in an
existing pinned, offline, unprivileged container. All **6,974 regular-file hashes**
match before and after; the container was removed and its absence verified.
This is source-only formatting, not a local ARM build/test run.

Raw local logs are retained privately. `evidence-map.json` records original
and published hashes, `.log` to `.txt` byte-preserving renames, and the only
redaction: private home-directory prefixes in command/output records become
`<private-home>`. No result, failure, test name or count is changed. Source
archives, local Python orchestration and ongoing CI watch logs are excluded.
Native qualification and later carries require their own separate receipts.
