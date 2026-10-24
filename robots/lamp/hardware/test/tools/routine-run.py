#!/usr/bin/env python3
# routine-run.py — the routine run on one V2 lamp, in the test cases' order
# (cases/test-cases-v0.toml), each step announced on the lamp's own speaker,
# as the hardware lead's bench test lamp_fulltest.py does.
#
# Run on the lamp, as root, at a console (the steps ask the person), wrapped in
# the lamp's lock from the builder's machine (lampOS/tools/lamp-lock.sh):
#
#   lamp-lock.sh <lamp> run <builder> <session> routine -- \
#     ssh -t orangepi@<lamp> sudo python3 /var/lib/lamp-test/tools/routine-run.py --unit-id <lamp>
#
#   routine-run.py --unit-id lamp-dbda                  the whole routine run
#   routine-run.py --unit-id lamp-dbda --only ring,privacy
#   routine-run.py --unit-id lamp-dbda --no-voice       no spoken prompts
#   routine-run.py --unit-id lamp-dbda --dry-run        say what it would do
#
# Steps, in order (each is a test case; --only/--skip take the short names):
#   inspection  TC-RT-01  assembly_inspection   asked on the console
#   pads        TC-RT-02  (head pads, feeds touch_by_hand)   steps.sh step 2
#   selftest    TC-RT-03  the factory self-test               steps.sh step 4
#   ring        TC-RT-05  ring_pixels                         ring-pixels.py
#   privacy     TC-RT-06  privacy_switch                      privacy-switch.py
#   buzz        TC-RT-04  speaker_buzz           asked, after the lamp speaks
#   exchange    TC-RT-08  spoken_exchange        asked on the console
#
# Spoken prompts: short pre-recorded WAVs, one per step, made once on a Mac by
# make-prompts.sh and copied to --prompts (default /var/lib/lamp-test/prompts).
# A missing prompt is silence, as in the bench test. They are played with
# `aplay -D device_speaker` at the lamp's own level: this never touches the
# volume and never plays a tone. On V2 lamp-voice holds the speaker, so a
# prompt is played with lamp-voice stopped and lamp-voice is started again
# before any step that needs it (all but the inspection); the prompt is waited
# for before a step's microphones or speaker are used.
#
# Output, in --out (default /var/lib/lamp-test/runs/<started>/):
#   answers.json   lamp-station's answers file (crates/station/src/inputs.rs):
#                  one step per check, written whole after every step, so a run
#                  cut short leaves the steps done and nothing claimed for the rest
#   selftest.json  the factory self-test's report, for lamp-station --selftest
#   run.log        everything the steps printed
# An unanswered question, a step that could not run, or one cut short is
# not_checked / skipped / interrupted, never pass.
#
# Exit: 0 every step pass, 1 any fail, 2 could not start, 3 incomplete.
# Python 3.11 standard library only.

import argparse
import datetime
import glob
import json
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
STEPS = ["inspection", "pads", "selftest", "ring", "privacy", "buzz", "exchange"]
NEEDS_VOICE = {"pads", "selftest", "ring", "privacy", "buzz", "exchange"}
SELFTEST_DIRS = os.environ.get("SELFTEST_DIRS", "/var/lib/lampOS/selftest:/var/lib/lamp-selftest").split(":")

# Environment overrides, for a host test with stand-ins (routine-run-test.py).
APLAY = os.environ.get("APLAY", "aplay")
SYSTEMCTL = os.environ.get("SYSTEMCTL", "systemctl")
STEPS_SH = os.environ.get("STEPS_SH", "/var/lib/lamp-test/factory/steps.sh")
RING = os.environ.get("RING_PIXELS", os.path.join(HERE, "ring-pixels.py"))
PRIVACY = os.environ.get("PRIVACY_SWITCH", os.path.join(HERE, "privacy-switch.py"))


def utc_now():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


class Run:
    def __init__(self, args):
        self.args = args
        self.started = utc_now()
        self.out = args.out or os.path.join("/var/lib/lamp-test/runs", self.started.replace(":", ""))
        self.steps = []          # lamp-station answer steps
        self.notes = []          # what this runner saw, for the console and run.log
        self.voice_proc = None
        if not args.dry_run:
            os.makedirs(self.out, exist_ok=True)
        self.log = None if args.dry_run else open(os.path.join(self.out, "run.log"), "a")

    # ── reporting ────────────────────────────────────────────────────────────
    def say(self, text):
        print(text, file=sys.stderr, flush=True)
        if self.log:
            self.log.write(f"{utc_now()} {text}\n"); self.log.flush()

    def record(self, check, verdict, reason="", measurement=None):
        step = {"check": check, "verdict": verdict, "at": utc_now()}
        if verdict != "pass" or reason:
            step["reason"] = (reason or verdict)[:200]
        if measurement:
            step["measurement"] = measurement
        self.steps.append(step)
        self.say(f"  {check}: {verdict}" + (f" ({reason})" if reason else ""))
        self.write()

    def write(self):
        if self.args.dry_run:
            return
        doc = {"started": self.started, "ended": utc_now(), "steps": self.steps, "repairs": []}
        tmp = os.path.join(self.out, "answers.json.tmp")
        with open(tmp, "w") as f:
            json.dump(doc, f, indent=1)
            f.write("\n")
        os.replace(tmp, os.path.join(self.out, "answers.json"))

    # ── voice and prompts ────────────────────────────────────────────────────
    def voice_active(self):
        r = subprocess.run([SYSTEMCTL, "is-active", "--quiet", "lamp-voice"])
        return r.returncode == 0

    def voice(self, on):
        if self.args.dry_run:
            self.say(f"  (would {'start' if on else 'stop'} lamp-voice)")
            return
        subprocess.run([SYSTEMCTL, "start" if on else "stop", "lamp-voice"], check=False)
        if on:
            time.sleep(self.args.voice_settle)

    def announce(self, key):
        """Play prompt <key>.wav on the lamp's speaker and wait for it. lamp-voice
        holds the speaker on V2, so it is stopped for the prompt; the caller starts
        it again when the step needs it."""
        path = os.path.join(self.args.prompts, key + ".wav")
        if self.args.no_voice or not os.path.exists(path):
            return
        if self.args.dry_run:
            self.say(f"  (would say {key}.wav)")
            return
        if self.voice_active():
            self.voice(False)
        subprocess.run([APLAY, "-q", "-D", self.args.speaker, path],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15)

    def ready_for(self, step):
        if step in NEEDS_VOICE and not self.args.dry_run and not self.voice_active():
            self.voice(True)

    # ── asking the person ────────────────────────────────────────────────────
    def ask(self, question):
        """pass / fail / not_checked. Empty, timeout or no console is not_checked."""
        if self.args.dry_run:
            self.say(f"  (would ask: {question})")
            return "not_checked", "dry run"
        if not sys.stdin.isatty() and not os.environ.get("ROUTINE_RUN_STDIN_OK"):
            return "not_checked", "no console to ask"
        print(f"{question} [p]ass / [f]ail (Enter = not checked): ", end="", file=sys.stderr, flush=True)
        try:
            a = sys.stdin.readline().strip().lower()
        except (EOFError, KeyboardInterrupt):
            return "not_checked", "not answered"
        if a.startswith("p"):
            return "pass", ""
        if a.startswith("f"):
            print("  What was wrong (one line)? ", end="", file=sys.stderr, flush=True)
            why = sys.stdin.readline().strip() or "failed, no reason given"
            return "fail", why
        return "not_checked", "not answered"

    # ── running a step's own script ──────────────────────────────────────────
    def run_cmd(self, cmd, capture_json=False):
        """Run a step's script with the console passed through. With capture_json,
        stdout's last JSON line is returned."""
        if self.args.dry_run:
            self.say("  (would run: " + " ".join(cmd) + ")")
            return 0, None
        if not os.path.exists(cmd[1] if cmd[0] in ("bash", "python3") else cmd[0]):
            return 2, None
        p = subprocess.run(cmd, stdout=subprocess.PIPE if capture_json else None, text=True)
        line = None
        if capture_json and p.stdout:
            for raw in p.stdout.splitlines():
                raw = raw.strip()
                if raw.startswith("{"):
                    try:
                        line = json.loads(raw)
                    except json.JSONDecodeError:
                        pass
            if self.log:
                self.log.write(p.stdout)
        return p.returncode, line

    def factory_env(self):
        env = []
        c = self.args.channel
        if c:
            env = [f"MEASURE_BIN={c}/lamp-body-measure", f"MOOD_BIN={c}/lamp-body-mood"]
        return env

    # ── the steps ────────────────────────────────────────────────────────────
    def step_inspection(self):
        v, why = self.ask("Assembly: shell closed, cables seated, nothing loose or rattling when tilted?")
        self.record("assembly_inspection", v, why)

    def step_pads(self):
        cmd = ["env", *self.factory_env(), "bash", STEPS_SH, "--unit-id", self.args.unit_id,
               "--write", "--from", "2", "--to", "2"]
        if self.args.dry_run:
            self.run_cmd(cmd); return
        if not os.path.exists(STEPS_SH):
            self.notes.append("head pads: steps.sh not found"); self.say(f"  head pads: {STEPS_SH} not found"); return
        rc = subprocess.run(cmd).returncode
        # TC-RT-02 feeds touch_by_hand in the self-test; it is not an answer of its own.
        self.notes.append(f"head pads (steps.sh step 2): exit {rc}")
        self.say(f"  head pads: steps.sh step 2 exit {rc} (the self-test's touch_by_hand judges the pads)")

    def step_selftest(self):
        print("Before the self-test, ask the lamp something with a long answer (4 s or more), "
              "so its speaker can be judged. Press Enter when it has spoken, or now to go on.",
              file=sys.stderr, flush=True)
        if not self.args.dry_run and (sys.stdin.isatty() or os.environ.get("ROUTINE_RUN_STDIN_OK")):
            sys.stdin.readline()
        t0 = time.time()
        cmd = ["env", *self.factory_env(), "bash", STEPS_SH, "--unit-id", self.args.unit_id,
               "--write", "--from", "4", "--to", "4"]
        if self.args.dry_run:
            self.run_cmd(cmd); return
        if not os.path.exists(STEPS_SH):
            self.notes.append("self-test: steps.sh not found"); return
        rc = subprocess.run(cmd).returncode
        newest = None
        for d in SELFTEST_DIRS:
            for f in glob.glob(os.path.join(d, "*.json")):
                if os.path.basename(f).startswith("latest"):
                    continue
                if os.path.getmtime(f) >= t0 - 1 and (newest is None or os.path.getmtime(f) > os.path.getmtime(newest)):
                    newest = f
        if newest:
            shutil.copy(newest, os.path.join(self.out, "selftest.json"))
            self.say(f"  self-test report: {newest} → selftest.json (exit {rc})")
        else:
            self.notes.append("self-test: no report written")
            self.say("  self-test: no report written; lamp-station will find it missing")

    def step_script(self, script, check, extra=()):
        rc, line = self.run_cmd(["python3", script, *extra], capture_json=True)
        if self.args.dry_run:
            return
        if line and line.get("check") == check and line.get("verdict"):
            self.steps.append({k: line[k] for k in ("check", "verdict", "reason", "at") if k in line})
            self.say(f"  {check}: {line['verdict']} ({line.get('reason', '')})")
            self.write()
        else:
            self.record(check, "not_checked", f"{os.path.basename(script)} gave no verdict (exit {rc})")

    def step_ring(self):
        self.step_script(RING, "ring_pixels")

    def step_privacy(self):
        self.step_script(PRIVACY, "privacy_switch")

    def step_buzz(self):
        v, why = self.ask("While the lamp spoke: any buzz, rattle or distortion? (pass = clean)")
        self.record("speaker_buzz", v, why)

    def step_exchange(self):
        print('Say to the lamp: "Hey Lamp, what time is it?"', file=sys.stderr, flush=True)
        v, why = self.ask("Did it answer out loud, with the right time?")
        self.record("spoken_exchange", v, why)

    # ── the run ──────────────────────────────────────────────────────────────
    def go(self, steps):
        voice_was_on = self.args.dry_run or self.voice_active()
        self.say(f"routine run on {self.args.unit_id}, started {self.started}, steps: {', '.join(steps)}")
        self.announce("start")
        try:
            for s in steps:
                self.say(f"== {s}")
                self.announce(s)
                self.ready_for(s)
                getattr(self, "step_" + s)()
        except KeyboardInterrupt:
            self.say("stopped by the person: the steps not done are not in the answers")
        verdicts = [st["verdict"] for st in self.steps]
        if "fail" in verdicts:
            code, key = 1, "done_fail"
        elif len(verdicts) and all(v == "pass" for v in verdicts):
            code, key = 0, "done_pass"
        else:
            code, key = 3, "done_fail"
        self.announce(key)
        if voice_was_on and not self.args.dry_run and not self.voice_active():
            self.voice(True)
        self.write()
        self.say(f"result: {['PASS', 'FAIL', '', 'INCOMPLETE'][code]} · answers {self.out}/answers.json")
        for n in self.notes:
            self.say(f"  note: {n}")
        return code


def main():
    ap = argparse.ArgumentParser(description="The routine run on one V2 lamp (see the header).")
    ap.add_argument("--unit-id", required=True, help="this lamp, e.g. lamp-dbda")
    ap.add_argument("--only", default="", help="comma list of: " + ",".join(STEPS))
    ap.add_argument("--skip", default="", help="comma list to leave out")
    ap.add_argument("--prompts", default="/var/lib/lamp-test/prompts", help="folder of <step>.wav prompts")
    ap.add_argument("--speaker", default="device_speaker", help="ALSA device for the prompts")
    ap.add_argument("--no-voice", action="store_true", help="no spoken prompts")
    ap.add_argument("--voice-settle", type=float, default=6.0, help="seconds after starting lamp-voice")
    ap.add_argument("--channel", default="/var/tmp/lampos-channel",
                    help="folder with lamp-body-measure and lamp-body-mood (install does not place them, #2208)")
    ap.add_argument("--out", help="run folder (default /var/lib/lamp-test/runs/<started>)")
    ap.add_argument("--dry-run", action="store_true", help="say what it would do, change nothing")
    args = ap.parse_args()
    only = [s for s in args.only.split(",") if s] or STEPS
    bad = [s for s in only + [s for s in args.skip.split(",") if s] if s not in STEPS]
    if bad:
        print(f"routine-run: unknown step(s): {', '.join(bad)}", file=sys.stderr)
        return 2
    steps = [s for s in STEPS if s in only and s not in args.skip.split(",")]
    if not args.dry_run and os.geteuid() != 0 and not os.environ.get("ROUTINE_RUN_NOT_ROOT_OK"):
        print("routine-run: run as root (it stops and starts lamp-voice and runs the factory steps)", file=sys.stderr)
        return 2
    return Run(args).go(steps)


if __name__ == "__main__":
    sys.exit(main())
