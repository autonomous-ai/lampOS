#!/usr/bin/env python3
"""Choose the runner for lampOS's ARM64 job: the self-hosted Mac Studio pool
(org runner group lampos-studio) or GitHub-hosted ubuntu-22.04-arm, and
rescue a job the Studio left stuck.

GitHub-hosted is the default and every doubt falls back to it: an unknown
switch, a stale or malformed heartbeat, an API error, a Studio stuck or too
busy to finish sooner than GitHub-hosted would. The Studio is only a faster
lane for the team's own pull requests and manual runs; forks, lamp-voice's
pushes (signed publication builds), the nightly full run and promotions never
reach this script (the job's `if`) and are refused here again.

  route  writes `runner` (JSON for runs-on) and `reason` to GITHUB_OUTPUT and
         a line to the run's summary. Never fails: any error routes hosted.
  sweep  cancels and re-runs (whole run, so `route` decides again) every run
         whose Studio job has been queued longer than STUCK_QUEUED or running
         longer than STUCK_RUNNING. Run by the route job and on a schedule.

Inputs come from the environment, so the decision is testable offline
(.github/scripts/lampos-route-test.py):
  LAMPOS_STUDIO       off | shadow | canary | on (vars.LAMPOS_STUDIO; else off)
  LAMPOS_STUDIO_BEAT  "v1 <epoch> <pr registered> <pr healthy> <trusted healthy>",
                      written every 30 s by the Studio host
  GITHUB_EVENT_NAME, GITHUB_REPOSITORY, GITHUB_RUN_ID, HEAD_REPO, PR_NUMBER,
  FULL, PROMOTE, GH_TOKEN
"""
import json
import math
import os
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime

HOSTED = "ubuntu-22.04-arm"
STUDIO = {"group": "lampos-studio", "labels": ["self-hosted", "linux", "arm64", "studio"]}
WORKFLOW = "lampos-arm64.yml"
ROUTE_JOB = "lampOS — choose the ARM64 runner"

BEAT_STALE = 120  # s: the host writes every 30 s
# A pull request's job on one 12-vCPU Studio slot, cold: about 8.5 to 9 min
# (estimated from 26 Oct's runs at 20 vCPU and the 27 Oct slot benchmark), and
# 13.5 min at the median on GitHub-hosted. So a job waits for a slot only
# while waiting still beats GitHub-hosted: with these, never.
STUDIO_JOB = 9 * 60  # s
HOSTED_JOB = int(13.5 * 60)  # s
STUCK_QUEUED = 8 * 60  # s
STUCK_RUNNING = 25 * 60  # s: the Studio's jobs take 5 to 7 minutes
API_TIMEOUT = 10  # s per request


def api(path, method="GET"):
    req = urllib.request.Request(
        "https://api.github.com" + path, method=method,
        headers={"Authorization": "Bearer " + os.environ["GH_TOKEN"],
                 "Accept": "application/vnd.github+json",
                 "X-GitHub-Api-Version": "2022-11-28"})
    with urllib.request.urlopen(req, timeout=API_TIMEOUT) as r:
        body = r.read()
    return json.loads(body) if body else None


def parse_beat(text, now):
    """(registered, healthy) PR slots from a fresh heartbeat, or a reason it
    can't be trusted."""
    parts = (text or "").split()
    if len(parts) != 5 or parts[0] != "v1" or not all(p.isdigit() for p in parts[1:]):
        return None, "heartbeat missing or malformed"
    at, registered, healthy = int(parts[1]), int(parts[2]), int(parts[3])
    age = now - at
    if age > BEAT_STALE or age < -BEAT_STALE:
        return None, f"heartbeat {age} s old"
    if healthy < 1:
        return None, "no healthy pull request slot"
    return (registered, healthy), None


def studio_jobs(repo, own_run):
    """Every queued or running job of this workflow that asked for the Studio,
    and every other run still choosing, as (run id, status, seconds since
    queued or started)."""
    now = time.time()
    jobs = []
    for status in ("queued", "in_progress"):
        runs = api(f"/repos/{repo}/actions/workflows/{WORKFLOW}/runs?status={status}&per_page=100")
        for run in runs["workflow_runs"]:
            if str(run["id"]) == str(own_run):
                continue
            for job in api(f"/repos/{repo}/actions/runs/{run['id']}/jobs?filter=latest&per_page=100")["jobs"]:
                # Another run still choosing may be about to ask for the Studio:
                # counted as busy, so runs started together don't all see it free
                # (three did on 26 Oct, studio-par1..3).
                if job["name"] == ROUTE_JOB and job["status"] == "in_progress":
                    jobs.append((run["id"], "routing", 0))
                    continue
                # A queued job has no runner group yet: its labels say where it
                # waits (`studio`; the cache warming pool's `studio-trusted` is another).
                if "studio" not in job.get("labels", []) or job["status"] not in ("queued", "in_progress"):
                    continue
                if job["status"] == "in_progress" and job.get("runner_group_name") != "lampos-studio":
                    continue
                since = job["started_at"] if job["status"] == "in_progress" else job["created_at"]
                age = now - datetime.fromisoformat(since.replace("Z", "+00:00")).timestamp()
                jobs.append((run["id"], job["status"], age))
    return jobs


def stuck(jobs):
    return [j for j in jobs
            if (j[1] == "queued" and j[2] > STUCK_QUEUED) or (j[1] == "in_progress" and j[2] > STUCK_RUNNING)]


def eligible(env):
    event = env.get("GITHUB_EVENT_NAME")
    if env.get("FULL") == "true" or env.get("PROMOTE"):
        return False
    if event == "pull_request":
        return env.get("HEAD_REPO") == env.get("GITHUB_REPOSITORY")
    return event == "workflow_dispatch"


def decide(env, now, jobs_of):
    """(use the Studio?, reason). jobs_of() is studio_jobs, injected for tests."""
    switch = env.get("LAMPOS_STUDIO", "")
    if switch not in ("shadow", "canary", "on"):
        return False, "switch off"
    if not eligible(env):
        return False, "event not eligible"
    if switch == "canary" and env.get("GITHUB_EVENT_NAME") == "pull_request" \
            and int(env.get("PR_NUMBER") or 1) % 2:
        return False, "canary: odd pull request"
    slots, why = parse_beat(env.get("LAMPOS_STUDIO_BEAT"), now)
    if why:
        return False, why
    _, healthy = slots
    try:
        jobs = jobs_of()
    except (OSError, ValueError, KeyError, urllib.error.URLError) as e:
        return False, f"jobs API: {e}"
    if stuck(jobs):
        return False, f"{len(stuck(jobs))} Studio job(s) stuck"
    busy = len(jobs)
    ahead = max(0, busy - healthy + 1)
    wait = math.ceil(ahead / healthy) * STUDIO_JOB
    if wait + STUDIO_JOB >= HOSTED_JOB:
        return False, f"Studio full: {busy} busy on {healthy} slots, ~{wait // 60} min wait"
    return True, f"Studio: {busy} busy on {healthy} slots, ~{wait // 60} min wait"


def route(env):
    try:
        studio, reason = decide(env, time.time(),
                                lambda: studio_jobs(env["GITHUB_REPOSITORY"], env.get("GITHUB_RUN_ID")))
    except Exception as e:  # never fail the job: hosted is today's path
        studio, reason = False, f"error: {e!r}"
    if studio and env.get("LAMPOS_STUDIO") == "shadow":
        studio, reason = False, "shadow, would use the " + reason
    runner = STUDIO if studio else HOSTED
    line = f"ARM64 runner: {'Studio' if studio else HOSTED} ({reason})"
    print(line)
    for name, text in (("GITHUB_OUTPUT", f"runner={json.dumps(runner)}\nreason={reason}\n"),
                       ("GITHUB_STEP_SUMMARY", line + "\n")):
        if env.get(name):
            with open(env[name], "a") as f:
                f.write(text)


def completed(repo, run_id, within):
    for _ in range(within // 5):
        if api(f"/repos/{repo}/actions/runs/{run_id}")["status"] == "completed":
            return True
        time.sleep(5)
    return False


def sweep(env):
    repo = env["GITHUB_REPOSITORY"]
    rescued = 0
    for run_id in sorted({j[0] for j in stuck(studio_jobs(repo, None))}):
        print(f"run {run_id}: Studio job stuck, cancelling and running it again")
        try:
            # A runner too slow or gone may never acknowledge a cancel (seen on
            # 40363864391): then force it, which needs no runner.
            for how in ("cancel", "force-cancel"):
                api(f"/repos/{repo}/actions/runs/{run_id}/{how}", "POST")
                if completed(repo, run_id, 60):
                    break
            else:
                print(f"run {run_id}: still not cancelled, left for the next sweep")
                continue
            api(f"/repos/{repo}/actions/runs/{run_id}/rerun", "POST")
            rescued += 1
        except (OSError, ValueError, KeyError) as e:
            print(f"run {run_id}: {e}")
    print(f"rescued {rescued} run(s)")
    if rescued and env.get("GITHUB_STEP_SUMMARY"):
        with open(env["GITHUB_STEP_SUMMARY"], "a") as f:
            f.write(f"Studio watchdog: rescued {rescued} stuck run(s)\n")


if __name__ == "__main__":
    {"route": route, "sweep": sweep}[sys.argv[1]](os.environ)
