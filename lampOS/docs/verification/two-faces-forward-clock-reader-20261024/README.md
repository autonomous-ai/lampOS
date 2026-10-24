# #3355 corrected-head reader: forward-clock persistence finding

Exact authored head `51925472d61d712db1d2169edd9d33892c9ac9d8`.
**HOLD, P2** at `worker.rs:4125–4129`: expiration uses monotonic birth,
but the minimum 1.5-second persistence still subtracts wall timestamps.

The complete unmodified `FacesHeld` helper/constants were extracted into
`controls.rs`, with only a `lamp_bus::EXPIRY_GRACE_MS = 50` shim matching
production `crates/bus/src/wire.rs:30`. Source/control hashes and exact head
are in `source-binding.json`. Rust 1.96 compiled this private source-control
executable; no repository source or host clock was changed.

Six controls completed: **5 passed, 1 failed**. Ordinary persistence,
short-span refusal, delayed delivery preserving original age, queued-before-
privacy refusal and no-stream rollback expiry pass. The negative forward-
wall-step assertion fails; see [primary output](result.txt).

Immediate-delivery `(wall ms, monotonic ms)` observations at ordinary
250 ms cadence are `(100000,100000)`, `(100750,100250)`,
`(101000,100500)`, `(101250,100750)`, `(101500,101000)`. There is one
500 ms forward wall step between the first two. Each look sees two faces;
none is stale, expired or separated by a wall gap greater than one second.
At monotonic 101000, only **1000 ms** of real observation has elapsed;
the code says Busy because its wall span is **1500 ms**. This can hold a
person's line after a second face has stayed less than the promised limit.

The owner should require an appropriate monotonic persistence duration and
conservatively handle publication/receipt clock discontinuities, retaining
this trace alongside all prior privacy, age and rollback controls. This
report prescribes no untested change to production code.

The shell wrapper tried to assign zsh's reserved `status` after the Rust
test completed and failed separately. Therefore the original Rust process
exit code was not retained; no wrapper success or captured Rust exit 101 is
claimed. Its completed 5-pass/1-fail test summary is retained unchanged.
The generated executable is excluded from this public text proof.

Full broad Mac gates were deferred once this concrete source P2 reproduced;
no old-head broad gate or native pass clears it. Metadata preparation alone
is not a successful gate. Initial full offline Cargo metadata wanted an
unrelated missing dependency; `--no-deps` workspace metadata succeeded, and
no shared generated outputs were removed or source timestamps changed.
No physical, thermal, played-hearing or device acceptance is inferred.
