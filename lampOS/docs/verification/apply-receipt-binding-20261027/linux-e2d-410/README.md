# Receipt PR: original e2d Linux preview qualification

Original attempt-1 run 40492815101, job 122150725700 passed. The complete job log confirms actual checkout `3b24fb0e2fd5f8c51ef7da7fc8fe1389d48d0740` with ordered parents [`184532c6cbf929ba12a7d3fb3b61c94091d5bed4`, `e2d393ad3ec634e089ac32ce58982a5252a66421`]. Rust 1.96.0 x86 Linux, pinned workspace formatting and strict workspace all-target Clippy actually succeeded; Update was checked. Required stages passed. Cache and source-time reuse are recorded, not a clean rebuild.

The actual preview complete tree differs from the offered e2d/410 tree because Main advanced during publication. The four receipt-owned blobs/modes are identical, while installer and other consumed upstream context changed. This is qualification of the tested preview composition, not a transfer to the older offered complete tree or newest Main. The subsequent Main movement remains separate. No runtime, device or writer activation is claimed by this lint audit.
