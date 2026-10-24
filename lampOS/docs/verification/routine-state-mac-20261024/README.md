# Routine state: current-base Mac gates and Linux formatting

Second, 24 October 2026. PR #3323 head
`51cc5dfb9bbe9ebeae0a9452d1188895ab652289`, based on `59c4ac795`.

Sequential Mac commands all passed: `cargo fmt --all --check`,
`cargo clippy -q --locked -p lamp-skills --all-targets --message-format=short -- -D warnings`,
and `cargo test -q --locked -p lamp-skills`:
**514 passed / 0 failed / 5 ignored**. Four ignored tests are the new
published-reader cases and have not been counted as executed here.

Exact-head Linux source-only formatting passed with **6,968 regular files
unchanged** before/after. The unprivileged offline container used the pinned
image, no host mounts or devices and a read-only root. Its ownership, removal
and absence were checked. This was not a local ARM64 build/test gate.

Initial dedicated native run 40053585243, on superseded `1ee2df940`, failed
while fetching the crates.io serde_core dependency with an HTTP/2 framing
error, before compilation or any test. Its raw output remains beside this
note: zero executed cases, not four failed tests. Current-head native
qualification remains separate. No live output, lamp, power-cut, rested-night,
full-loader or independently trusted release-signature result is claimed.
