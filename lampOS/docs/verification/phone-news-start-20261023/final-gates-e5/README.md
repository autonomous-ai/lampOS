# Phone-news event fixture: terminal Mac gate failure

**Ready: NO.** The single planned full gate finished with runner exit **1**.
Voice and readers each exited **101**; fmt, strict all-target Clippy, workspace
build and diff check exited **0**. No command timed out and no target is unrun
or incomplete. All six failures remain unwaived.

The tested source is the uncommitted two-file candidate on
`e5cd73aca116e6bb52fbc41fdd0094b8bf10f5bc`: one `cfg(test)` notification/setup
change in `voice/src/messages.rs` and its `docs/messages.md` paragraph.
The original five assertions, response/quiet windows and product behavior are
unchanged. No candidate commit or later head is substituted for this source.

After the gate, parent committed the same bytes as
`47adb38fd0f61dfbfc1172e9a668d54c43983794`. The separate
[post-gate binding](post-gate-commit-binding.json) and
[validation](post-gate-commit-validation.json) bind its tree to the frozen
candidate. They do not rewrite the original null candidate commit, timestamps
or failed results.

| Gate | Exit | Wall seconds, including compilation |
| --- | ---: | ---: |
| fmt | 0 | 12.347101 |
| clippy | 0 | 91.388655 |
| voice-default-parallel | 101 | 358.749842 |
| readers-default-parallel | 101 | 1718.563969 |
| workspace-build | 0 | 74.824749 |
| diff-check | 0 | 2.607885 |

Full Voice: **3868 passed / 2 failed / 15 ignored**, 6 targets, 3885 identities.
Its library is **3588 / 1 / 15** and integration **269 / 1 / 0**. The changed
`messages::tests::a_text_never_waits_on_the_news_for_the_phone` passed.
Eleven readers: **3200 passed / 4 failed / 17 ignored**, 68 targets, 3221
identities. Combined selected gates: **7068 / 6 / 32**, 74 targets and 7106
identities. These are 12 selected packages, not a full workspace test run.
Workspace build is recorded separately.

Both test commands use default parallel libtest and `--no-fail-fast`; no thread
override, exact-case retry or timing sample was added. Clippy covers all 12
packages with `--all-targets -- -D warnings`. Exact commands/timestamps and
individual target durations are in [commands.json](commands.json),
[results.json](results.json) and [targets.json](targets.json).

## All six failures

| Case | Actual failed boundary |
| --- | --- |
| Voice `scoped_audio_rejects_revocation_and_mixed_request_keys_before_dispatch` | `tools.rs:2270`: directory creation returned OS 17 / AlreadyExists before that iteration's dispatch assertions. |
| Voice `twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them` | `light_call.rs:90`: upper-middle 75.413405 ms versus strict 25 ms; all 20 Done assertions preceded it. Maximum 127.632138 ms is descriptive. |
| Skills `thinking_after_a_moment_of_speaking_is_still_the_line` | `skills.rs:4686`: first observed gesture 1.163475702 s after stopped versus strict 300 ms. |
| Skills `unplug_tool_returns_before_folding_and_says_ready_only_after_render_receipt` | `skills.rs:11692`: no say connection before the shared 10 s startup/readiness watchdog. Admission was 625.825 microseconds. |
| Skills `refused_cap_reload_keeps_audio_generation_but_asset_scan_still_revokes_it` | `requested_audio.rs:1639`: generation changed from 1 to 2; later asset mutation/revocation checks were not reached. |
| VoiceEval `a_closed_runner_transport_stops_the_runtime_and_its_workers` | `relay_hardening.rs:126`: Empty PID parse before the final worker-absence check. |

[Failures](failures.json) retains every complete panic block; complete human
stdout/stderr is retained under `raw/`, with offsets and hashes in
[raw-payloads.json](raw-payloads.json). The separate
[directory-collision audit](eexist-audit/README.md) retains a corrected display-only
source projection and lossless original projection bytes. It does
not establish PID reuse, a stale owner or a same-process collision: the panic
contains neither directory path nor process ID.

[Reader source audit](reader-source-audit.json) and
[line-numbered excerpts](reader-source-excerpts.json) distinguish observation
from possible mechanisms. Skills and VoiceEval sources are unchanged. The
Thinking bound includes real service/IPC/stand-in observation. Unplug uses one
watchdog across setup and completion, while product readiness requires three
fresh matching receipts. The cap fixture writes folders incrementally into a
live watched directory; an accepted partial reload before the later refusal is
possible but unproven. The relay closer waits on PID-file existence, which can
precede complete PID writing; no captured file/trace proves this interleaving.
No exact-e5 reader baseline was executed. Unchanged source, prior occurrences
or a successful independent build do not classify these failures as inherited
or waive them.

## Reused exact-e5 Voice comparison

The [already published baseline](https://github.com/autonomous-ai/lampOS/blob/699591f0cb8ec5839f6e9c1e953ebb1ae89d4733/lampOS/docs/verification/voice-boot-clock-20261023/baseline-e5/README.md) at `699591f0cb`
remains **3860 / 10 / 15**, with its original execution timestamps and failed
qualification. It was not rerun. [Focused delta](baseline-delta.json) binds
both complete sets of 3885 case identities and the same 15 ignored identities:
nine prior failures passed in this phone run, the tools-directory case newly
failed, light-call failed both, and the changed messages case passed both.
This is an observation pair, not evidence that the phone change fixed unrelated
failures. Library executable hashes differ; matching filenames are not compiled
identity. [Dependency pins](published-dependencies.json) verify the published
baseline and separate audit manifests.

## Source and receipt verification

[Source proof](source-proof.json) reconstructs all **9992 tracked paths**
(**6649 lampOS paths**, one literal symlink) from e5 Git blobs plus the two
frozen candidate snapshots, including repository-root fixtures. Candidate
aggregate `5dd15400b0c4a66eed69f9f02de03e574cbacf429e48c738d60a78d93349aa97`
is equal before/after freshness, after every command and at completion. No
transient mutation controls ran in this full gate. Packaging read no live
worktree or MAIN artifact and ran no tests.

[Environment](environment.json) retains the prescribed settings and captured
Rust 1.96.0 `x86_64-apple-darwin` identity. All 62 listed test artifacts have
captured Mach-O x86_64 output. Hardware/translation and matched host load are
not established. Local PATH and host-node/build metadata are omitted; the original result
seal and explicit PATH hash preserve their binding. Diagnostic bytes remain lossless and unredacted.

All **193 commands / 386 stdout-stderr streams** are verified and packaged.
Only recognized complete Cargo machine-record lines are omitted from portable
stdout; exact original byte ranges, lengths, SHA256 values and external paths
remain. No test/progress/failure human bytes are omitted. Compiler diagnostics
and build-finished metadata are retained separately in
[compilation.json](compilation.json). No binaries, Cargo machine streams or
measurement scripts are packaged.

[Census](census.json) binds full raw list/ignored-list outputs to
[artifact identities](artifacts.json). Every actual execution row is independently
matched to that census; should-panic display annotations map only to unique
compiled identities. Listing is never counted as successful execution.
[Completed run seal](completed-run-seal.json) pins every original run file;
[external metadata](external-files.json) preserves bulk provenance digests.
[Validation](validation.json) and [SHA256SUMS](SHA256SUMS) verify the portable
package. Publication remains a separate parent-owned action.
