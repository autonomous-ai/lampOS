# Exact Common source reconstruction from public objects

The command vectors reproduce both frozen trees without either local candidate commit as a build input. Public source inputs are feature `e9d00f935029a6d5054ddf35b0203fb8546f7995`, its authored parent `2e2eee360c47f352dfe94f3d2bfe3d69db7048bd`, and the selected public base:

| Public base | Frozen comparison label | Exact reconstructed tree |
|---|---|---|
| `0ee5ebd2a78f3825847a69e197d3f6b4433e4962` | fabd | `e22f5070ae0fabfd031dddc72cf3e565e6bd5e9d` |
| `da57072b2e6420ae25cf6779f1ee1756178ce724` | 618 | `21d7db4e321b08ac94f370d749fb5c49c4fa1911` |

Each result has exactly 36 changed paths and 10,435 outside paths exact its base (10,471 total files). Every resulting mode/blob matches the local comparison tree. All 35 non-verification-README authored added/removed line multisets match the public feature delta. The two-sided JSON/CSV maps give public authored-parent→feature AND public base→exact candidate mode/blob pairs for every path; null means absent, not omitted.

Use `command-vectors.json` as ordered argv/environment/file-redirection data. It includes every command, expected exit, intermediate hash and sole expected conflict. Start from a public repository containing the listed objects, no inherited GIT_* overrides, a new bare scratch repository and separate temporary indexes. Set only the listed Git environment; all new objects/indexes are scratch-owned. No checkout, commit, source helper, shared ref or shared index is needed. The recorded Git version pins the merge behavior used.

The three-way merge is runtime-clean. Its ONLY conflict is concurrent appended prose in `lampOS/docs/reuse-decisions.md`. Resolve by retaining public-base music prose first, then one blank line, then public-feature request-speaker prose. The vectors derive both sides with `git show`, use `git merge-file --union`, then apply `reuse-decisions-spacing.patch` (one blank line). This yields public-derived blob `1100206a49eda669346d3a266ab7838a28b74a53`, with no local runtime content supplied.

`verification-status-header.patch` is the ONLY additional content input: the identical count/hash/status-prose update for both cases, README blob `423f418ab8fef6b5bf6908f08917fef43339b8c2`→`1729b1371bfdc6c538d2b15795c3d78415a9e3cc`. It preserves failed/historical/unqualified boundaries. It contains no runtime changes, raw logs, environment values, helper bodies, binary data or personal author metadata. Apply it only after the public merge/prose resolution, then `git write-tree` must equal the exact tree above.

The local fabd/618 commit IDs are comparison labels only and are NOT required in a public clone. No hidden runtime input is missing. This verifies source reconstruction, not public-object availability over the network, source clearance, compilation, tests, latency, privacy acceptance, readiness or merge permission. All existing holds remain.

Root independently replayed every command vector for both cases and obtained both exact trees. The four input commits also resolved through GitHub’s commit endpoint in the public repository on 25 October. These checks add source reproducibility only; current gates and all existing holds remain.
