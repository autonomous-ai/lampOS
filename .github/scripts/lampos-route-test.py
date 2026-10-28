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
    base = {"LAMPOS_STUDIO": "on", "LAMPOS_STUDIO_BEAT": f"v1 {NOW - 20} 0 0 1",
            "GITHUB_EVENT_NAME": "push", "GITHUB_REF": "refs/heads/lamp-voice",
            "GITHUB_REPOSITORY": REPO, "FULL": "false", "PROMOTE": ""}
    base.update(over)
    return base


def busy(n, status="in_progress", age=60):
    return lambda: [(i, status, age) for i in range(n)]


class Decide(unittest.TestCase):
    def studio(self, e, jobs=busy(0)):
        return R.decide(e, NOW, jobs)

    def test_a_lamp_voice_push_goes_to_the_studio(self):
        self.assertEqual(self.studio(env())[0], True)

    def test_switch_off_unknown_or_absent_is_hosted(self):
        for value in ("off", "", "On", "true", None):
            e = env(LAMPOS_STUDIO=value) if value is not None else {k: v for k, v in env().items() if k != "LAMPOS_STUDIO"}
            self.assertEqual(self.studio(e), (False, "switch off"), value)

    def test_pull_requests_other_branches_full_and_promotions_never_reach_the_studio(self):
        for e in (env(GITHUB_EVENT_NAME="pull_request", GITHUB_REF="refs/pull/42/merge"),
                  env(GITHUB_EVENT_NAME="pull_request"), env(GITHUB_EVENT_NAME="pull_request_target"),
                  env(GITHUB_REF="refs/heads/main"), env(GITHUB_REF="refs/heads/devops007/x"),
                  env(GITHUB_EVENT_NAME="workflow_dispatch", GITHUB_REF="refs/heads/devops007/x"),
                  env(GITHUB_EVENT_NAME="schedule"), env(GITHUB_EVENT_NAME="workflow_dispatch", FULL="true"),
                  env(GITHUB_EVENT_NAME="workflow_dispatch", PROMOTE="7"), {**env(), "GITHUB_REF": None}):
            self.assertEqual(self.studio(e), (False, "event not eligible"), e)

    def test_a_manual_run_on_lamp_voice_is_eligible(self):
        self.assertTrue(self.studio(env(GITHUB_EVENT_NAME="workflow_dispatch"))[0])

    def test_a_re_run_is_hosted(self):
        # The sweep re-runs a stuck run: the Studio must not take it again.
        self.assertTrue(self.studio(env(GITHUB_RUN_ATTEMPT="1"))[0])
        for attempt in ("2", "3", ""):
            self.assertEqual(self.studio(env(GITHUB_RUN_ATTEMPT=attempt)), (False, "a re-run"), attempt)

    def test_canary_takes_manual_runs_only(self):
        self.assertTrue(self.studio(env(LAMPOS_STUDIO="canary", GITHUB_EVENT_NAME="workflow_dispatch"))[0])
        self.assertEqual(self.studio(env(LAMPOS_STUDIO="canary")), (False, "event not eligible"))

    def test_stale_future_malformed_or_unhealthy_beat_is_hosted(self):
        for beat in (f"v1 {NOW - 121} 0 0 1", f"v1 {NOW + 300} 0 0 1", "", "v1 x 0 0 1",
                     f"v2 {NOW} 0 0 1", f"v1 {NOW} 0 0", f"v1 {NOW} 2 2 0"):
            studio, reason = self.studio(env(LAMPOS_STUDIO_BEAT=beat))
            self.assertFalse(studio, beat)
            self.assertRegex(reason, "heartbeat|healthy")

    def test_the_pull_request_pool_is_not_needed(self):
        self.assertTrue(self.studio(env(LAMPOS_STUDIO_BEAT=f"v1 {NOW} 0 0 1"))[0])

    def test_a_push_waits_behind_one_job_not_two(self):
        # 9-minute Studio jobs, 25 hosted: one ahead (18 min) pays, two (27) don't.
        self.assertTrue(self.studio(env(), busy(1))[0])
        self.assertRegex(self.studio(env(), busy(2))[1], "Studio full")

    def test_the_queue_follows_the_timings(self):
        saved = R.STUDIO_JOB, R.HOSTED_JOB
        R.STUDIO_JOB, R.HOSTED_JOB = 5 * 60, 12 * 60
        try:
            # 3 slots, 5-minute jobs, hosted 12: one round of waiting (5 min) pays, two don't.
            e = env(LAMPOS_STUDIO_BEAT=f"v1 {NOW} 0 0 3")
            self.assertTrue(self.studio(e, busy(5))[0])
            self.assertRegex(self.studio(e, busy(6))[1], "Studio full")
        finally:
            R.STUDIO_JOB, R.HOSTED_JOB = saved

    def test_a_stuck_job_sends_everything_hosted(self):
        self.assertRegex(self.studio(env(), busy(1, "queued", R.STUCK_QUEUED + 1))[1], "stuck")
        self.assertRegex(self.studio(env(), busy(1, "in_progress", R.STUCK_RUNNING + 1))[1], "stuck")
        self.assertTrue(self.studio(env(), busy(1, "queued", R.STUCK_QUEUED - 1))[0])

    def test_a_job_queued_behind_one_is_not_stuck(self):
        self.assertGreater(R.STUCK_QUEUED, R.STUDIO_JOB)
        self.assertLess(R.STUCK_RUNNING, R.HOSTED_JOB)

    def test_runs_still_choosing_count_as_busy_but_never_stuck(self):
        jobs = lambda: [(i, "routing", 0) for i in range(2)]
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
            return json.loads(lines["runner"]), lines["studio"], lines["reason"], summary.read_text()

    def test_studio_output_is_the_trusted_group_and_labels(self):
        e = env(LAMPOS_STUDIO_BEAT=f"v1 {int(R.time.time())} 0 0 1")
        runner, studio, _, summary = self.outputs(e, lambda repo, run: [])
        self.assertEqual(runner, {"group": "lampos-studio", "labels": ["self-hosted", "linux", "arm64", "studio-trusted"]})
        self.assertEqual(studio, "true")
        self.assertIn("ARM64 runner: Studio", summary)

    def test_shadow_decides_but_stays_hosted(self):
        e = env(LAMPOS_STUDIO="shadow", LAMPOS_STUDIO_BEAT=f"v1 {int(R.time.time())} 0 0 1")
        runner, studio, reason, _ = self.outputs(e, lambda repo, run: [])
        self.assertEqual((runner, studio), ("ubuntu-22.04-arm", "false"))
        self.assertTrue(reason.startswith("shadow, would use the Studio"))

    def test_any_exception_routes_hosted(self):
        def boom(repo, run):
            raise RuntimeError("unexpected")
        e = env(LAMPOS_STUDIO_BEAT=f"v1 {int(R.time.time())} 0 0 1")
        runner, studio, reason, _ = self.outputs(e, boom)
        self.assertEqual((runner, studio), ("ubuntu-22.04-arm", "false"))
        self.assertIn("unexpected", reason)


if __name__ == "__main__":
    unittest.main(verbosity=2)
