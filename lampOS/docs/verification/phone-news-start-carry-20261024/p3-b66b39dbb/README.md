# Phone-fixture cancellation and cleanup findings

Local checkpoint b66b39dbb adds a drop witness proving cancellation of the
30-second phone future, plus an owned worker/root guard for early failures.
Normal completion aborts and awaits the worker. All five original assertions
are byte-exact; two added assertions cover cancellation and normal task exit.
The original30s fake sleep, strict under2s response and full2s unwanted-news
window remain. Runtime code is unchanged.

Mac fmt, strictVoiceClippy and full serialVoice pass4004/0/21ignored; changed
case once, source unchanged.26 selected package names were invalidated;
records observe25 path-package identities, including vendor. All observed
workspace artifacts are fresh; the unchanged vendor Sonora cache was retained
(41/42Clippy and39/40test artifacts fresh=false). No claim that26 members
compiled. Executed test binaries were hashed before target invalidation.

Two isolated freshly compiled fault controls each fail once at the intended
assertion, expected101: suppress cancellation evidence, and panic after
startup. Both leave no owned fixture directory; temporary roots removed and
isolated worktree restored clean. No normal case was retried. These expected
negative results are separate from the successful full gate.

b66 was a local checkpoint, not pushed; its repair diff against publicc185
is included. Equal rebase onto22b753df5 maps the feature to6cd08d29c with
identical messages.rs. Current-base qualification is separate and pending.
The historical Update2/3 HOLD under6370483473 remains. No full local workspace
gate, localARM execution, lamp action or physical/latency claim.
