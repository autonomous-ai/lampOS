# Common 218c local failure, 27 October 2026

Ready for lamp-voice: no

This is a completed, failed local gate on source tree `218c1d5f2`, with
HEAD `ed9aff39b` and pinned Main `3d5ea2dd7`. The [receipt](receipt.json)
binds counts and actual native results to the retained private evidence by
SHA-256. It publishes no raw output, private words or executable artifacts.

Normal formatting, full metadata, strict workspace/all-target Clippy and
workspace build each exit 0. Ordinary Cargo reuse is retained; this does not
claim every local package was freshly rebuilt. Host test discovery, under
DEBUG0, lists 5,564 identities and selects 5,521 nonignored cases. Discovery
and the saved-binary parser check execute no test bodies and give no runtime
credit.

The one nextest union runs the full Eyes, Voice, Memory and Skills test
packages, plus IPC's camera-authority binary, using saved compiler metadata,
profile `pr`, two slots, zero retries and no fail-fast. It gives **5,435 clean
passes and 86 failures**, with a terminal for every selected case and zero
leaks or timeouts. All 110 required controls terminate: 79 pass and 31 fail.
The 43 natural ignores are 27 JSON ignore terminals and 16 start-only entries
with human SKIP; none is a test execution. The separately mapped 525
contracts and their 25 overlaps add no cases to these counts.

Actual controller exit **125** and nextest exit **100** remain failed. The
run was **not cancelled**: a planned cancellation precheck found it already
terminal, and no manual signal was sent. Documentation and custom harness
stages remain unrun. This is not the full host workspace or all transitive
dependencies, and gives no native or physical acceptance.

The two failed drivers are retained independently. Actual dsymutil and an
independent Mach-O N_OSO reader inspect both and observe zero backing inputs;
zero is not assumed from DEBUG0. The separate preservation helper exits 0,
verifies source stability and owned-process closure, and releases the target.
That preservation success does not change the original failed run.

Three archived source worktrees are removed only after explicit owner
approval and a fresh check that every distinct source/index state is already
backed up and restore-verified. Their three branches remain; backups, build
targets and failure evidence are untouched. The proof covers 815,503,724
logical tracked bytes. The shared volume's observed free-space change cannot
be attributed exclusively to this removal.

Later named-CONTROL EOF, socket-fixture and diagnostic candidates require
fresh qualification. No cause is assigned to all 86 failures by this receipt,
and no earlier failure or later Main carry inherits a pass.
