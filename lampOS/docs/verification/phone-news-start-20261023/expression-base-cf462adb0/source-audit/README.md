# Expression first-step failure source audit

The phone candidate `4e1e00e5298b31f797fd90f58d2b439ff2b4e32e` changes only Voice `messages.rs` test code and its documentation relative to `cf462adb04f64d699c226dd3b1781c0ad927068f`. Expression, bus, message declarations and Cargo inputs checked here are byte-identical. This read-only audit does not classify either failure as inherited or establish a cause.

Both panics occur at the first synchronous output expectation after input publication and service construction: `runtime.rs:127` expects a body before any intended lease, expiry or restart assertion; `runtime.rs:1048` expects a light request before the listening/offline-face comparison. The latter panic does not identify its closure arm. Exact raw fragments, offsets, source hashes and captured artifact metadata are in [audit.json](audit.json).

The fixed service-step clock and captured wall time do not replace bus admission time. `Subscriber::admit` reads real wall time; VoiceTiming has a 500 ms lifetime plus 50 ms expiry grace. Setup/admission delay could therefore remove a retained voice input before the service parses it. The output contains no observed age or admission counters, so this remains a hypothesis. It is not evidence of host-load causation.

The captured runtime executable was compiled in the eleven-reader gate, with SHA256 `3a7c9e37eef9dff82b667a4641229e9428c4dbc702efad308caecab1c3205286` (6,631,448 bytes). A later baseline artifact must be compared using captured metadata; a changed worktree path or Cargo feature graph can change a binary hash independently of test behavior. No current shared-target binary was read for this audit.

No Cargo command, new test run, source change or deadline change was performed. The two failures remain failed observations; a separately authorized whole Expression runtime-target clean-base comparison is pending.
