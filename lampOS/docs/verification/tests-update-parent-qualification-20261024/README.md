Qualification of the Update fixture ownership change (private publication draft)

Historical validation ran on parent 22b753df5db6434d21455f40f67926e0ecee9d91
with the exact four-file delta later committed as 4e96727d7b. That commit was
pushed and rebased cleanly onto 4270083a2, producing
fb8d2f71b836266df07bf1b658e40c0aa13804f3; the exact range-diff is unchanged (=).
The four authored paths did not change upstream. These are historical 22b
validation receipts, not validation of the new 427/fb8 composition. New-base local
gates and native Linux/ARM64 CI remain pending; this is not a ready claim.

The matched parent is 22b753df5db6434d21455f40f67926e0ecee9d91, before the four
candidate test/docs files. Exactly one standalone observation for each of the ten
failed full-run cases passed (10/10). As none reproduced its original failure
phase, the original frozen current executables then received exactly three
observations per case; all 30/30 passed. No retries, rebuilds, extra cases, or
full-suite reruns were used for this qualification.

The original default-parallel result remains FAILED: 7,300 passed, 10 failed,
52 ignored (Voice 4,000/4/21; eleven readers 3,300/6/31). Standalone qualification
is separate from that result. It neither identifies a historical cause nor
establishes a full green run. The older 4a mid-search/light-call failures and their
source/phase comparison are retained as context, not inherited causal proof.

Each row below is baseline PASS once and original-current PASS three times:

- `coordinator::tests::second_route::a_push_mid_search_drops_its_looks_picture`
- `coordinator::tests::second_route::a_push_once_the_looks_picture_went_cuts_its_answer`
- `provider_worker::tests::held_consent::a_held_lines_reports_stop_is_done_and_its_answer_stays_held`
- `light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`
- `runtime::supervisor::tests::dropping_reaped_predecessor_does_not_remove_active_successor_sockets`
- `health::the_environment_alone_does_not_enable_reporting_or_create_bus_data`
- `voice::a_refused_follow_up_forget_survives_the_next_turn_note_refresh`
- `voice::forget_that_right_after_a_follow_up_forgets_the_entry_it_was_about`
- `status::tests::a_restart_keeps_the_last_result_each_end_is_said_and_it_is_said_again`
- `physical::mac::audit_grant_tests::fast_startup_cues_before_the_first_live_pong_are_published_in_order_with_original_times`

`independent-count-projection.json` derives the counts from copied primary exact
stdout and original full target-outcome rows. Every stdout/stderr, command record,
and permanent launch-consumption receipt is retained under `run/baseline/` or
`run/current/`. `manifest.json` binds their original and copied byte counts and
SHA-256 hashes. All copied files are UTF-8/JSON; no binary or runnable script is
included. `source-protocol-snapshots.json` contains read-only JSON text snapshots
of the reviewed controllers/support/census, allowing independent interpretation.
The complete file list is in `FILE_LIST.txt`.

Baseline metadata matched the complete frozen graph (451 packages/nodes), with
48 local packages: 46 workspace members plus excluded Sonora/Tough. All 53 tracked
Cargo.toml files and lock/config matched the parent. The new, separate baseline
target used the supported 48-local-ID clean, then exact Voice and eleven-reader
--no-run commands. All 138 first local unit contexts compiled fresh:false; later
reuse retained exact raw package ID, target/profile/features and output proof.
Six complete case/ignored inventories (5,117 cases, 22 ignored) matched current;
all ten selected cases appeared once and were nonignored. Cargo clean ignores
supplied URL/version and selects declared package names; no claim is made about
unresolved historical same-name generated caches or thirdparty warm-cache reuse.

Before current observations, only the two original qualified Memory CLI paths
were atomically changed to captured reader-built 27ae/17d0 binaries. Reviewed
before/after primary bytes/modes checks passed for the observations. Finally,
both workspace programs were independently verified restored to 26bebadb... and
2fb36e77..., with their original 0755 modes. Inodes/mtimes of generated programs
are not claimed preserved. `receipts/current-final-receipt.json` includes all
four swap records, restored identities and original case/executable bindings.
Per-observation source/child guards are assertion-backed by the reviewed completed
controller; no invented per-case hash-sampling timestamps are supplied.

Baseline 10,442 and current 10,444 source bytes/full modes/mtimes, frozen witness
identities, PATH-resolved /bin/cat and /usr/bin/true guards passed. All 62 owned
metadata/clean/compile/inventory/baseline/current command groups closed with direct
children waited. The baseline four-job compile shared the approved resource
budget with NoPause's separate two-job target. No globally isolated hardware,
byte-identical Cargo-injected runtime-loader environment, ARM64 execution, or
parallel-load reproduction is claimed. No source worktree or log branch was
changed while preparing this publication bundle.

Primary full Voice and reader stdout/stderr and their original command records are now included under `original-full-run/`. `original-full-stream-supplement.json` binds their original hashes/lengths. Root independently verified every published failure excerpt against its primary byte offset and stream hash. No run was repeated and the original failed full result is unchanged.
