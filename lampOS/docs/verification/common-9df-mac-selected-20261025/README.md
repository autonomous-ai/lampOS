# Common 9df Mac result

All eight stages passed on the pinned 9df/bcb source: formatting, strict workspace/all-target Clippy, workspace build, Eyes 357/0/5, Voice 4153/0/21, Update 396/0/6, Skills 516/0/9 and Memory 324/0/8 (passed/failed/ignored). Five required controls passed once; all 46 local workspace members have bound fresh compilation evidence. Source stability, process cleanup and target release verified.

Commands were cargo fmt --all --check; cargo clippy -q --offline --locked --workspace --all-targets --message-format=json-render-diagnostics -- -D warnings; cargo build -q --offline --locked --workspace --message-format=json-render-diagnostics; and cargo test --offline --locked --no-fail-fast -p <lamp-eyes|lamp-voice|lamp-update|lamp-skills|lamp-memory> -- --format pretty --test-threads=1.

This is a Mac result only. Linux's full Skills 518/1/9 and its first three direct failures remain failed, with Tests owning the observer repair. No historical failure is waived. Later fc31 carries, current native ARM64/Linux Clippy, readiness, privacy and physical acceptance are separate; this does not qualify them. The released Mac slot now runs Name d60's separate gate; its Common parent/order holds remain.

Only selected counts, source hashes and Booleans are public. The immutable private packet manifest SHA-256 is 04c6c740b4b70f88b80ea164de342ee4f892e22b39a9a910d173d301c75b974f; selected summary SHA-256 is 6a4118ce2732c88e3bd8a3ab08516f9da1c2301cbfc7587d005759cfdc608988. Raw logs, source maps, helpers and environment values remain private and have not been independently recounted by readers. No lamp or activation action.
