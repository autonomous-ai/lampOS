#!/usr/bin/env python3
"""Offline checks of lampos-reuse.py's decision (#6556): a fake GitHub API."""
import copy
import importlib.util
from pathlib import Path

spec = importlib.util.spec_from_file_location("reuse", Path(__file__).with_name("lampos-reuse.py"))
reuse = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reuse)

REPO = "autonomous-ai/lampOS"
PUSH, HEAD, TESTED, BASE = "a" * 40, "b" * 40, "c" * 40, "d" * 40
TREE = "e" * 40


def world():
    run = {"id": 7, "run_attempt": 1, "head_sha": HEAD, "head_branch": "first/train-900", "event": "pull_request",
           "path": reuse.WORKFLOW, "repository": {"full_name": REPO}, "status": "completed", "conclusion": "success"}
    job = lambda name: {"name": name, "conclusion": "success"}  # noqa: E731
    return {
        f"commits/{PUSH}/pulls": [{"number": 900, "merged_at": "2026-11-12T00:00:00Z", "merge_commit_sha": PUSH,
                                   "base": {"ref": "lamp-voice"},
                                   "head": {"sha": HEAD, "ref": "first/train-900", "repo": {"full_name": REPO}}}],
        f"actions/workflows/lampos-arm64.yml/runs?event=pull_request&head_sha={HEAD}&per_page=30": {"workflow_runs": [run]},
        "actions/runs/7/attempts/1/jobs?per_page=100": {"jobs": [job(reuse.BUILD)] + [job(f"{reuse.ARM64}{i}/3)") for i in (1, 2, 3)]},
        "actions/runs/7/artifacts?per_page=100": {"artifacts": [
            {"name": f"lampos-arm64-{HEAD}", "expired": False},
            {"name": f"lampos-arm64-tested-{TESTED}", "expired": False},
            {"name": "lampos-arm64-tests", "expired": False}]},
        f"git/commits/{TESTED}": {"tree": {"sha": TREE}, "parents": [{"sha": BASE}, {"sha": HEAD}]},
        f"git/commits/{PUSH}": {"tree": {"sha": TREE}, "parents": [{"sha": BASE}, {"sha": HEAD}]},
    }


def decide(w):
    return reuse.decide(lambda path: copy.deepcopy(w[path]), PUSH, REPO)


def refused(w, words):
    try:
        decide(w)
    except reuse.Refused as refusal:
        assert words in str(refusal), (words, str(refusal))
        return
    raise AssertionError(f"reused, expected a refusal: {words}")


# Same tree: reuse the run's binaries artifact.
run_id, artifact, _ = decide(world())
assert (run_id, artifact) == (7, f"lampos-arm64-{HEAD}")

# lamp-voice moved under the train: a different tree builds here.
w = world()
w[f"git/commits/{PUSH}"]["tree"]["sha"] = "f" * 40
refused(w, "lamp-voice moved under the train")

# A builder's pull request, not a train's.
w = world()
w[f"commits/{PUSH}/pulls"][0]["head"]["ref"] = "seventh/x"
refused(w, "is not a train's")

# A push that isn't any pull request's merge.
w = world()
w[f"commits/{PUSH}/pulls"] = []
refused(w, "no single pull request")

# A test runner failed, or the run failed or isn't done: not green.
for change in ("job", "run", "shards", "missing", "skipped", "build"):
    w = world()
    jobs = w["actions/runs/7/attempts/1/jobs?per_page=100"]["jobs"]
    if change == "job":
        jobs[2]["conclusion"] = "failure"
    elif change == "run":
        w[f"actions/workflows/lampos-arm64.yml/runs?event=pull_request&head_sha={HEAD}&per_page=30"]["workflow_runs"][0]["conclusion"] = "failure"
    elif change == "shards":
        del jobs[1:]
    elif change == "missing":
        del jobs[2]
    elif change == "skipped":
        jobs[3]["conclusion"] = "skipped"
    else:
        del jobs[0]
    refused(w, "no ARM64 run")

# The binaries artifact expired (kept 7 days), or the tested commit isn't named.
w = world()
w["actions/runs/7/artifacts?per_page=100"]["artifacts"][0]["expired"] = True
refused(w, "absent or expired")
w = world()
del w["actions/runs/7/artifacts?per_page=100"]["artifacts"][1]
refused(w, "no single tested merge commit")

# The named tested commit is not a merge of the pull request's head.
w = world()
w[f"git/commits/{TESTED}"]["parents"] = [{"sha": BASE}]
refused(w, "is not a merge of")

print("lampos-reuse: every decision as expected")
