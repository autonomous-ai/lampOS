#!/usr/bin/env python3
"""Offline tests of the ARM64 runner choice: the jobs API is simulated."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("route", Path(__file__).with_name("lampos-route.py"))
R = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(R)
NOW = 1_800_000_000
REPO = "autonomous-ai/lampOS"


def env(**over):
    base = {"LAMPOS_STUDIO": "on", "LAMPOS_STUDIO_BEAT": f"v1 {NOW - 20} 3 3 1",
            "GITHUB_EVENT_NAME": "pull_request", "GITHUB_REPOSITORY": REPO,
            "HEAD_REPO": REPO, "PR_NUMBER": "42", "FULL": "false", "PROMOTE": ""}
    base.update(over)
    return base


def busy(n, status="in_progress", age=60):
    return lambda: [(i, status, age) for i in range(n)]


class Decide(unittest.TestCase):
    def studio(self, e, jobs=busy(0)):
        return R.decide(e, NOW, jobs)

    def test_free_slot_goes_to_the_studio(self):
        self.assertEqual(self.studio(env())[0], True)

    def test_switch_off_unknown_or_absent_is_hosted(self):
        for value in ("off", "", "On", "true", None):
            e = env(LAMPOS_STUDIO=value) if value is not None else {k: v for k, v in env().items() if k != "LAMPOS_STUDIO"}
            self.assertEqual(self.studio(e), (False, "switch off"), value)

    def test_forks_pushes_full_and_promotions_never_reach_the_studio(self):
        for e in (env(HEAD_REPO="someone/lampOS"), env(GITHUB_EVENT_NAME="push"),
                  env(GITHUB_EVENT_NAME="schedule"), env(GITHUB_EVENT_NAME="workflow_dispatch", FULL="true"),
                  env(GITHUB_EVENT_NAME="workflow_dispatch", PROMOTE="7"), env(GITHUB_EVENT_NAME="pull_request_target")):
            self.assertEqual(self.studio(e), (False, "event not eligible"))

    def test_manual_run_is_eligible(self):
        self.assertTrue(self.studio(env(GITHUB_EVENT_NAME="workflow_dispatch", PR_NUMBER=""))[0])

    def test_canary_takes_even_pull_requests_only(self):
        self.assertTrue(self.studio(env(LAMPOS_STUDIO="canary", PR_NUMBER="42"))[0])
        self.assertEqual(self.studio(env(LAMPOS_STUDIO="canary", PR_NUMBER="43")), (False, "canary: odd pull request"))

    def test_stale_future_malformed_or_unhealthy_beat_is_hosted(self):
        for beat in (f"v1 {NOW - 121} 3 3 1", f"v1 {NOW + 300} 3 3 1", "", "v1 x 3 3 1",
                     f"v2 {NOW} 3 3 1", f"v1 {NOW} 3 3", f"v1 {NOW} 3 0 1"):
            studio, reason = self.studio(env(LAMPOS_STUDIO_BEAT=beat))
            self.assertFalse(studio, beat)
            self.assertRegex(reason, "heartbeat|healthy")

    def test_a_free_slot_or_hosted(self):
        # 9-minute Studio jobs, 13.5 hosted: waiting one job out (18 min) never pays.
        e = env(LAMPOS_STUDIO_BEAT=f"v1 {NOW} 2 2 1")
        self.assertTrue(self.studio(e, busy(1))[0])
        self.assertRegex(self.studio(e, busy(2))[1], "Studio full")

    def test_the_queue_follows_the_timings(self):
        saved = R.STUDIO_JOB, R.HOSTED_JOB
        R.STUDIO_JOB, R.HOSTED_JOB = 5 * 60, 12 * 60
        try:
            # 3 slots, 5-minute jobs, hosted 12: one round of waiting (5 min) pays, two don't.
            self.assertTrue(self.studio(env(), busy(5))[0])
            self.assertRegex(self.studio(env(), busy(6))[1], "Studio full")
        finally:
            R.STUDIO_JOB, R.HOSTED_JOB = saved

    def test_a_stuck_job_sends_everything_hosted(self):
        self.assertRegex(self.studio(env(), busy(1, "queued", R.STUCK_QUEUED + 1))[1], "stuck")
        self.assertRegex(self.studio(env(), busy(1, "in_progress", R.STUCK_RUNNING + 1))[1], "stuck")
        self.assertTrue(self.studio(env(), busy(1, "queued", R.STUCK_QUEUED - 1))[0])

    def test_runs_still_choosing_count_as_busy_but_never_stuck(self):
        jobs = lambda: [(i, "routing", 0) for i in range(6)]
        self.assertRegex(self.studio(env(), jobs)[1], "Studio full")
        self.assertEqual(R.stuck([(1, "routing", 10_000)]), [])

    def test_api_error_is_hosted(self):
        def broken():
            raise OSError("timed out")
        self.assertEqual(self.studio(env(), broken), (False, "jobs API: timed out"))


class Route(unittest.TestCase):
    def outputs(self, e, monkey):
        with tempfile.TemporaryDirectory() as d:
            out, summary = Path(d, "out"), Path(d, "summary")
            e = dict(e, GITHUB_OUTPUT=str(out), GITHUB_STEP_SUMMARY=str(summary))
            saved = R.studio_jobs
            R.studio_jobs = monkey
            try:
                R.route(e)
            finally:
                R.studio_jobs = saved
            lines = dict(line.split("=", 1) for line in out.read_text().splitlines())
            return json.loads(lines["runner"]), lines["reason"], summary.read_text()

    def test_studio_output_is_the_group_and_labels(self):
        e = env(LAMPOS_STUDIO_BEAT=f"v1 {int(R.time.time())} 3 3 1")
        runner, _, summary = self.outputs(e, lambda repo, run: [])
        self.assertEqual(runner, {"group": "lampos-studio", "labels": ["self-hosted", "linux", "arm64", "studio"]})
        self.assertIn("ARM64 runner: Studio", summary)

    def test_shadow_decides_but_stays_hosted(self):
        e = env(LAMPOS_STUDIO="shadow", LAMPOS_STUDIO_BEAT=f"v1 {int(R.time.time())} 3 3 1")
        runner, reason, _ = self.outputs(e, lambda repo, run: [])
        self.assertEqual(runner, "ubuntu-22.04-arm")
        self.assertTrue(reason.startswith("shadow, would use the Studio"))

    def test_any_exception_routes_hosted(self):
        def boom(repo, run):
            raise RuntimeError("unexpected")
        e = env(LAMPOS_STUDIO_BEAT=f"v1 {int(R.time.time())} 3 3 1")
        runner, reason, _ = self.outputs(e, boom)
        self.assertEqual(runner, "ubuntu-22.04-arm")
        self.assertIn("unexpected", reason)


if __name__ == "__main__":
    unittest.main(verbosity=2)
