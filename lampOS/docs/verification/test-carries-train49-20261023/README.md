# Latest-base carries for #3190 and #3218

Both owned PR branches were carried onto `810e05109bced3beca88a5b602930cfed5fba4e5` without conflicts. Both full authored patches and every authored file are byte-identical to their prior heads, and both `git range-diff` results are `=`. The exact old-head push leases succeeded. Worktrees are clean; both PRs remain open drafts with their execution HOLDs unchanged.

| PR | Preserved old head | New head | New parent |
| --- | --- | --- | --- |
| #3190 | `4b92b677d379b6ac1dadbd9e306320a04af48672` | `f121d6391e53bf709ffbf07807e8657f7e86d3ce` | `810e05109bced3beca88a5b602930cfed5fba4e5` |
| #3218 | `238403b1c5959fe8834aba09cfbbf24f03b6d893` | `328f7da0f5c1fd729bd5d22a974f9c0a67cf07dd` | `810e05109bced3beca88a5b602930cfed5fba4e5` |

The old heads remain reachable on both local and remote owned branches `tests/archive-3190-4b92b677d-20261023` and `tests/archive-3218-238403b1c-20261023`. Their remote creation used empty exact leases and an atomic push; no existing archival branch was overwritten. No local Cargo, test, Linux/ARM build or device action ran for these carries. Prior local successes and failures remain attached to their recorded old source and artifacts; none is relabelled as new-head qualification.

[Carry identities](carries.json) include full heads/trees, authored paths and hashes, exact range-diffs, remote draft checks and archive refs. [Commands](commands.json) and [raw carry output](raw-carry.json) preserve the Git actions and complete raw output. Each authored patch appears once because before and after are byte-identical. Decode a raw object's `utf8` string and encode as UTF-8 without adding a newline to recover its exact bytes and hash.

[Upstream source drift](source-drift.json) binds all 27 changed paths between the old and new bases, including substantial Voice and Records code. The full upstream Git diff remains external with its hash and an availability limitation. There is no overlap with either authored patch. [Dependency proof](dependency-proof.json) records all manifest, lock and toolchain hashes: these bytes and the conservative twelve-package static reader closure are unchanged. That does not establish equivalent compiled artifacts or qualify the changed upstream runtime.

[Initial CI metadata](initial-ci.json) identifies newly triggered ARM64 runs #6441/39957082492 and #6442/39957090143, and required Linux Clippy runs #2443/39957082564 and #2444/39957090152. Terminal job outcomes, actual test counts and checked merge identities belong to a separate CI receipt. No workflow was dispatched or retried by this carry. PR bodies, review disposition and log publication remain separate.

Current `lampOS/AGENTS.md`, `docs/joining.md` and `builders/tests.md` were read/compared with the previously read base and are byte-identical. Current Queue and own-PR comments were captured; the reviewer’s latest-base and execution HOLD requirements remain in force.

[Validation](validation.json) and [manifest](SHA256SUMS) bind these public files. Python orchestration, complete upstream source exports and API person metadata are not included.
