# #3906 original-run audit (read only)

Frozen code head: `f903659bba298cdee290ee4285038a8f46af930b`.
Repository queried: `autonomous-ai/lampOS` (the publisher's REPO), rather than the workspace remote's Physical-AI-Operating-System name.

## Assignment and restrictions

Queue #3504 comment 6449222010 and issue #3906 comment 6448979020 state publication blocks promotion and ask for minimal redacted diagnostics. No commits, posts, pushes, releases/tags, reruns, dispatches, cancellations, devices, SSH, signing key or token value reads, artifact downloads or artifact execution were performed. Original logs were streamed through an allowlist; artifact-download URLs were removed before inspection. Saved logs contain no env sections or key/token values.

## Exact original runs

| Sequence | Run ID | Attempt | Publish job ID | Head and actual checkout | Publish job UTC | Original refusal |
|---|---|---|---|---|---|---|
| 7621 | 40509311440 | 1 | 122219626257 | 02677931d67d8bd171ef9ea2e5f58f448eea029e | 13:34:43–13:35:23 | gh failed (exit 1) |
| 7713 | 40535064476 | 1 | 122313425544 | 0c0ad0e398a3a1c6c74c924ac51eabdff90f3f70 | 16:17:25–16:18:12 | artifact lacks SHA256SUMS or lamp-update |
| 7719 | 40536872661 | 1 | 122319184321 | 197f21533736cc99a1e1a819721a5a96ff8f3033 | 16:27:17–16:28:04 | gh failed (exit 1) |
| 7733 | 40540447778 | 1 | 122331661308 | 7400e4b951a3ad23552cbb73b4c271796a5876ec | 16:49:02–16:49:43 | gh failed (exit 1) |
| 7750 | 40544637339 | 1 | 122346116124 | d8ca71f4dc6e65b00564afb1e67c1a33311aeab8 | 17:14:34–17:15:13 | gh failed (exit 1) |
| 7758 | 40546923773 | 1 | 122353931361 | 1a1e778e065b3279faaf800762ac71e124d82366 | 17:28:15–17:29:06 | gh failed (exit 1) |
| 7758 | 40546923773 | 2 | 122354671504 | 1a1e778e065b3279faaf800762ac71e124d82366 | 17:29:36–17:30:15 | gh failed (exit 1) |
| 7734 (control) | 40540674263 | 1 | 122331974392 | 0d1d1f2b944581169c9f96adba9f40ab88adba92 | 16:49:49–16:52:06 | succeeded |

All timestamps are 2026-10-27 UTC. Each run was a push to lamp-voice. Build/test succeeded; checkout/download steps reported success; the fourth publish step was the failing step. Actual checkout evidence is original checkout action `git log -1 --format=%H`, not merely the run's API head.

Control metadata also resolves 7632 to 40512963174 (bf6f7bbdf5e936c2ef82621e1bb7592d511c1453), and 7547 to 40493028311 (11a1c867ea709c77d6421b8870144e0e5bba8ecc), both successful. Their logs were not needed.

## What source and logs establish

`Commands.run`, frozen publisher lines 93–104, captures `stderr` but reports only executable name and exit code. Original gh errors cannot reveal which gh operation failed, HTTP status, or error reason. They establish a completed gh process returning 1 (rather than the separate timeout/incomplete message), and that initial context, artifact inventory, classification and preparation completed.

Publisher `run()` lines 418–435 calls these gh operations before or at immutable creation:

1. `gh api repos/autonomous-ai/lampOS` (line 425).
2. `releases()` (line 428): paginated GET releases.
3. `tag_absent(tag)` (line 429): GET matching tag refs.
4. `head()` (line 430): another releases inventory; if channel exists, paginated channel assets and canonical envelope download by gh API; if absent, channel tag-ref lookup instead.
5. `create_release(tag, ...)` (line 435), whose gh invocation is line 345: `gh release create` with target SHA and assets.

No cleanup in this publisher deletes an immutable release/tag. First's report that no 7758 tag/release remained is consistent with failure before or during create, but does not select a call or prove why it failed. Read-only current API checks independently returned no 7758 matching tag ref (`[]`) and release HTTP 404. The generic historical stderr was discarded and cannot be recovered from these logs. Network, authorization, HTTP/API and create failure remain possibilities, not findings. Do not infer a race or a signing defect.

## Serialization

Frozen workflow lines 297–299 already have publish-job `concurrency.group: lampos-publish-lamp-voice` and `cancel-in-progress: false`. The same publication code/concurrency existed at every audited failure head: publisher SHA-256 is `40040134e3d6754af39c1f98b74fd93e25c054834c535a7f1255faa4d8f68b3b`; only workflow difference for 7621 is the later added docs/control.md PR path. Build workflows may overlap independently of publish jobs.

The specifically cited 7733/7734 pair did not overlap publication: 7733 finished 16:49:43; 7734 began 16:49:49. 7758 attempts also did not overlap one another. This refutes those examples as evidence of two concurrent publish jobs, but is not proof that no external release writer/race exists. No external-writer inventory was performed.

## Distinct artifact refusal

7713's original log contradicts the issue body's classification of it as a gh failure. Its refusal at 16:18:10.1476222 comes from `artifact_inventory()` line 155, before `prepare()` or any gh call in run(). At least SHA256SUMS or lamp-update was absent from the downloaded artifact directory; the message cannot identify which. Source uses exact artifact name and current run ID. Metadata lists artifact 12749961422, name `lampos-arm64-0c0ad0e398a3a1c6c74c924ac51eabdff90f3f70`, size 118321699, created 16:17:04, not expired. Download log starts downloading at 16:17:37.1267719 and has no ordinary `Total of 1 artifact(s) downloaded` / `Download artifact has finished successfully` completion lines before the next step at 16:18:09.9621092. Jobs API nevertheless marks download success. This supports auditing the download/extraction boundary separately; it does not prove whether the exported archive was incomplete or extraction failed. No archive was downloaded.

## Next safe step

Land the bounded, redacted gh diagnostic. A later authorized normal publication can identify the exact failed operation and first-line reason without replaying or changing original runs. Preserve fail-closed artifact validation and existing job concurrency. Any repair for 7713 needs separate export/download evidence and must not be attributed to gh stderr handling.

## Retained private primary evidence (not copied here)

- `issue-3906.json`, `issue-3906-comments.json`, `first-assignment-comment.json`: GitHub issue/comment API.
- `runs-2026-10-27.jsonl`: workflow runs API filtered to original day.
- `jobs-<sequence>-<attempt>.json`: `GET /repos/autonomous-ai/lampOS/actions/runs/<id>/attempts/<attempt>/jobs`.
- `publish-log-<sequence>-<attempt>.txt`: allowlisted original `GET /repos/autonomous-ai/lampOS/actions/jobs/<job-id>/logs`.
- `download-7713-1-sanitized.txt`, `artifacts-7713.json`: original download step and artifact metadata; URLs redacted, no archive accessed.
- `lampos-publish-f903659.py`, `lampos-arm64-f903659.yml`: git-show snapshots at frozen SHA.
- `tag-7758.json`, `release-7758-status.txt`: current read-only API absence checks.
