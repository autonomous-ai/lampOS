#!/usr/bin/env python3
"""Find the train pull request's tested ARM64 build for lamp-voice's push (#6556).

A train (`first/train-*`) pull request's run tests every crate with the push
run's own profile. When lamp-voice's push is that pull request's merge and its
tree is the very tree the pull request's run built and tested, the push
publishes that run's artifact instead of building and testing it again.

Writes `reused=true`, `run_id` and `artifact` to GITHUB_OUTPUT when every
check holds, and `reused=false` otherwise, with one line saying why: the push
then builds and tests as before. Never fails: any error is a refusal.

The checks, in order:
  1. the pull request merged as this commit is a train's, from this repository;
  2. its newest ARM64 run on its head commit whose build, release and every
     test runner (1/N to N/N, none missing or skipped) succeeded, latest attempt;
  3. that run names the merge commit it tested (its `lampos-arm64-tested-<sha>`
     artifact), a merge of the pull request's head, whose tree equals this
     push's tree; the tree includes .github/, so the same workflow and tests ran;
  4. the run's binaries artifact, `lampos-arm64-<head sha>`, is there, unexpired;
  5. the cache each of its build jobs restored (its `lampos-arm64-cache-key-<job>-<key>`
     artifact, `none` when cold) is lamp-voice's alone: every cache entry under that
     exact key is lamp-voice's, none the pull request's own scope's, which an
     earlier head of it could have filled (the Reviewer-lead's HOLD on #6563).
Publish checks the tree again, from the TREE file inside that artifact
(lampos-publish.py), so a mismatch the API missed refuses to publish.

Inputs: GITHUB_SHA, GITHUB_REPOSITORY, GH_TOKEN. Tested offline by
.github/scripts/lampos-reuse-test.py.
"""
import json
import os
import re
import sys
import urllib.request

WORKFLOW = ".github/workflows/lampos-arm64.yml"
BRANCH = "lamp-voice"
TRAIN = "first/train-"
ARM64 = "lampOS — build and test on Linux ARM64 ("
BUILD = ARM64 + "build)"
RELEASE = ARM64 + "release)"
SHARD = re.compile(re.escape(ARM64) + r"([1-9][0-9]*)/([1-9][0-9]*)\)")
TESTED = re.compile(r"lampos-arm64-tested-([0-9a-f]{40})")
CACHED = re.compile(r"lampos-arm64-cache-key-([a-z]+)-(none|lampos-arm64-u2204-[0-9a-f]+-[0-9a-f]{40})")
CACHING = ["build", "release"]  # the jobs that restore a cache, each recording the key it restored
API_TIMEOUT = 15  # s per request


class Refused(Exception):
    pass


def require(ok, reason):
    if not ok:
        raise Refused(reason)


def api(path):
    req = urllib.request.Request(
        f"https://api.github.com/repos/{os.environ['GITHUB_REPOSITORY']}/{path}",
        headers={"Authorization": "Bearer " + os.environ["GH_TOKEN"],
                 "Accept": "application/vnd.github+json",
                 "X-GitHub-Api-Version": "2022-11-28"})
    with urllib.request.urlopen(req, timeout=API_TIMEOUT) as r:
        return json.loads(r.read())


def decide(get, sha, repo):
    """(run_id, artifact, reason) for a reuse; raises Refused otherwise."""
    pulls = [p for p in get(f"commits/{sha}/pulls")
             if p.get("merged_at") and p.get("merge_commit_sha") == sha and p["base"]["ref"] == BRANCH]
    require(len(pulls) == 1, f"no single pull request merged as {sha} ({len(pulls)})")
    pr = pulls[0]
    head, ref = pr["head"]["sha"], pr["head"]["ref"]
    require(ref.startswith(TRAIN), f"#{pr['number']}'s head {ref} is not a train's")
    require((pr["head"].get("repo") or {}).get("full_name") == repo, f"#{pr['number']}'s head is not in {repo}")

    runs = get(f"actions/workflows/lampos-arm64.yml/runs?event=pull_request&head_sha={head}&per_page=30")["workflow_runs"]
    run = None
    for r in runs:  # newest first
        if not (r["head_sha"] == head and r["head_branch"] == ref and r["event"] == "pull_request"
                and r["path"] == WORKFLOW and r["repository"]["full_name"] == repo
                and r["status"] == "completed" and r["conclusion"] == "success"):
            continue
        jobs = [j for j in get(f"actions/runs/{r['id']}/attempts/{r['run_attempt']}/jobs?per_page=100")["jobs"]
                if j["name"].startswith(ARM64)]
        # The build, the release and every test runner, 1/N to N/N, each once: a missing or skipped one is no pass.
        shards = sorted(m.groups() for j in jobs if (m := SHARD.fullmatch(j["name"])))
        total = len(shards)
        whole = total > 0 and shards == sorted((str(i), str(total)) for i in range(1, total + 1))
        names = [j["name"] for j in jobs]
        if whole and len(jobs) == total + 2 and BUILD in names and RELEASE in names \
                and all(j["conclusion"] == "success" for j in jobs):
            run = r
            break
    require(run is not None, f"#{pr['number']} has no ARM64 run on {head} whose build, release and every test runner passed")

    artifacts = [a for a in get(f"actions/runs/{run['id']}/artifacts?per_page=100")["artifacts"] if not a["expired"]]
    tested = [m.group(1) for a in artifacts if (m := TESTED.fullmatch(a["name"]))]
    require(len(tested) == 1, f"run {run['id']} names no single tested merge commit")
    commit = get(f"git/commits/{tested[0]}")
    require(head in [p["sha"] for p in commit["parents"]], f"run {run['id']}'s tested commit {tested[0]} is not a merge of {head}")
    mine = get(f"git/commits/{sha}")["tree"]["sha"]
    require(commit["tree"]["sha"] == mine,
            f"run {run['id']} tested tree {commit['tree']['sha']} ({tested[0]}), this push's is {mine}: lamp-voice moved under the train")
    name = f"lampos-arm64-{head}"
    require(any(a["name"] == name for a in artifacts), f"run {run['id']}'s artifact {name} is absent or expired")
    keys = sorted(m.groups() for a in artifacts if (m := CACHED.fullmatch(a["name"])))
    require([job for job, _ in keys] == CACHING, f"run {run['id']} records no single restored cache per job {CACHING}")
    for job, key in keys:
        if key == "none":
            continue
        refs = [c["ref"] for c in get(f"actions/caches?key={key}&per_page=100")["actions_caches"] if c["key"] == key]
        require(refs and all(r == f"refs/heads/{BRANCH}" for r in refs),
                f"run {run['id']}'s {job} restored {key}, which is not lamp-voice's alone ({sorted(set(refs))})")
    return run["id"], name, f"#{pr['number']}'s run {run['id']} tested this tree ({tested[0]}); publishing its {name}"


def main():
    try:
        sha, repo = os.environ["GITHUB_SHA"], os.environ["GITHUB_REPOSITORY"]
        run_id, artifact, reason = decide(api, sha, repo)
        out = f"reused=true\nrun_id={run_id}\nartifact={artifact}\n"
    except Refused as refusal:
        reason, out = f"build here: {refusal}", "reused=false\n"
    except Exception as error:  # an API error or an odd answer: build as before
        reason, out = f"build here: {type(error).__name__}: {error}", "reused=false\n"
    print(reason)
    for name, text in (("GITHUB_OUTPUT", out), ("GITHUB_STEP_SUMMARY", f"ARM64 reuse: {reason}\n")):
        if os.environ.get(name):
            with open(os.environ[name], "a") as f:
                f.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
