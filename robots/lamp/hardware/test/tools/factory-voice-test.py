#!/usr/bin/env python3
# Host test for factory-voice.py with stand-ins for aplay, systemctl and
# steps.sh: no lamp,
# no sound. The stand-in steps.sh prints steps.sh's own lines and asks its own
# questions at a terminal, as the real one does. Checks that each prompt is
# said in order (the head pads' timeline after its question and the yes),
# that a no starts nothing, that a refused rest read says the
# head is off its stop, that the self-test's touch window asks for the strip
# only with --strip, that --auto answers the stand and the pads itself after
# "Starting in five seconds" (the reset only with --auto-reset, after
# --before-reset), that a speaker that
# refuses does not stop the steps, that
# a run with step 4 on a set-up lamp stops lamp-voice, says the self-test up
# front and starts lamp-voice again before steps.sh runs (and that nothing is
# played while lamp-voice holds the speaker), and that steps.sh's exit status
# passes through.
#
#   python3 factory-voice-test.py
import os, subprocess, sys, tempfile, textwrap

HERE = os.path.dirname(os.path.abspath(__file__))
RUNNER = os.path.join(HERE, "factory-voice.py")
KEYS = ["factory_say_ready", "factory_stand_ready", "factory_stand_go", "factory_stand_30",
        "factory_stand_100", "factory_stand_done", "factory_head_off", "factory_pads_ready",
        "factory_pads_away", "factory_pads_1", "factory_pads_2", "factory_pads_more",
        "factory_pads_wait", "factory_pads_done", "factory_key_done", "factory_selftest_ready",
        "factory_selftest_wait", "factory_selftest_stand", "factory_touch_now",
        "factory_touch_now_strip", "factory_selftest_pass", "factory_reset_ready",
        "factory_resetting", "factory_selftest_fail", "factory_failed", "factory_all_done",
        "factory_selftest_brief", "factory_selftest_brief_strip", "factory_auto_start"]


def stub(path, body):
    with open(path, "w") as f:
        f.write("#!/bin/bash\n" + textwrap.dedent(body))
    os.chmod(path, 0o755)


def setup(tmp, aplay_fails=False, voice_on=False):
    log = os.path.join(tmp, "said.log")
    state = os.path.join(tmp, "voice.state")
    open(state, "w").write("active" if voice_on else "inactive")
    prompts = os.path.join(tmp, "prompts"); os.makedirs(prompts)
    for k in KEYS:
        open(os.path.join(prompts, k + ".wav"), "w").write("RIFF")
    stub(os.path.join(tmp, "systemctl"), f"""
        case "$1" in
          is-active) [ "$(cat {state})" = active ] ;;
          stop) echo inactive > {state}; echo "voice-stop" >> {log} ;;
          start) echo active > {state}; echo "voice-start" >> {log} ;;
        esac
    """)
    # As the lamp's: lamp-voice holds the speaker while it runs.
    busy = 'echo "aplay: main:850: audio open error: Device or resource busy" >&2; exit 1'
    stub(os.path.join(tmp, "aplay"), f"""
        key=$(basename "${{@: -1}}" .wav)
        if [ "$(cat {state})" = active ]; then echo "refused-$key" >> {log}; {busy}; fi
        echo "$key" >> {log}
        {busy if aplay_fails else ''}
    """)
    # steps.sh's own lines, its ask() (refused without a terminal), each step
    # in turn.
    stub(os.path.join(tmp, "steps.sh"), f"""
        ask() {{ [[ -t 0 ]] || {{ echo "factory: FAILED: $1 needs a person at the lamp and a console to answer" >&2; return 1; }}
                read -r -p "factory: $1 Ready? [y/N] " a; [[ $a == y || $a == Y ]] || {{ echo "factory: FAILED: not ready for: $1" >&2; return 1; }}; }}
        case "$*" in
          *head-off*)
            echo "lamp-body-measure: home pitch 2505 is 485 over the body's home range 1760..2020, more than 400: a head off its stop, not a rest reading."
            echo "factory: FAILED: lamp-body-measure refused or could not read the body (its reason is above)" >&2; exit 1 ;;
          *--to\\ 3*)
            echo "factory: step 1 of 4, the body's poses"
            ask "the arm's stand: it rises to 30%, then to its full stand, and folds after each; keep a hand resting on the arm and say stop at any time." || exit 1
            echo "factory: standing at 30%"; echo "factory: standing at 100%"
            echo "factory: this arm stands at shoulder 2623, elbow 2560"; echo "factory: step 1 done"
            echo "factory: step 2 of 4, the touch pads"
            ask "the head pads: for 40 s, hands away from the lamp for the first 11 s (count them), then hold head pad 1 about 2 s and let go." || exit 1
            sleep 2.5; echo "factory: step 2 done"
            echo "factory: step 3 of 4, the update key"; echo "factory: step 3 done"
            echo "factory: steps 1 to 3 done (write)" ;;
          *--from\\ 4*)
            ask "the self-test: the lamp stands up for the head's small move (keep a hand by the arm as it rises, not on it, and say stop at any time), then it moves the head a little." || exit 1
            echo "factory: standing the lamp up for the head's small move"
            echo "lamp-selftest: swipe the base strip, touch the head and press the button now (20 s)"
            echo "selftest   not checked in 25 s"
            ask "the reset, now: the lamp forgets the bench's Wi-Fi, the factory account and any face or name it was given." || exit 1
            echo "factory: resetting: the lamp reboots into setup now."
            echo "factory: steps 4 to 4 done (write)" ;;
        esac
    """)
    env = dict(os.environ, APLAY=os.path.join(tmp, "aplay"), SYSTEMCTL=os.path.join(tmp, "systemctl"),
               FACTORY_VOICE_TIME_SCALE="0.05")
    return log, prompts, env


def run(tmp, args, answers, aplay_fails=False, extra=(), voice_on=False):
    log, prompts, env = setup(tmp, aplay_fails, voice_on)
    cmd = [sys.executable, RUNNER, "--prompts", prompts, *extra, "--", os.path.join(tmp, "steps.sh"), *args]
    # Answers are fed slowly, each after its question has been printed.
    feeder = "; ".join([f"sleep 1.5; printf '{a}\\n'" for a in answers] + ["sleep 3"])
    p = subprocess.run(["bash", "-c", f"({feeder}) | {' '.join(map(repr, cmd))}"],
                       env=env, capture_output=True, text=True, timeout=60)
    said = open(log).read().split() if os.path.exists(log) else []
    return p, said


def check(name, ok, detail=""):
    print(("ok   " if ok else "FAIL ") + name + (f"  ({detail})" if detail and not ok else ""))
    return ok


def main():
    results = []
    with tempfile.TemporaryDirectory() as tmp:
        p, said = run(tmp, ["--unit-id", "x", "--write", "--to", "3"], ["y", "y"])
        results.append(check("steps 1-3 exit 0", p.returncode == 0, p.stderr[-300:]))
        want = ["factory_stand_ready", "factory_say_ready", "factory_stand_go", "factory_stand_30",
                "factory_stand_100", "factory_stand_done", "factory_pads_ready", "factory_say_ready",
                "factory_pads_away", "factory_pads_1", "factory_pads_2", "factory_pads_more",
                "factory_pads_wait", "factory_pads_done", "factory_key_done", "factory_all_done"]
        results.append(check("steps 1-3 prompts in order", said == want, " ".join(said)))
    with tempfile.TemporaryDirectory() as tmp:
        p, said = run(tmp, ["--unit-id", "x", "--write", "--to", "3"], ["n"])
        results.append(check("a no: steps.sh's exit 1 passes through", p.returncode == 1))
        results.append(check("a no starts nothing", "factory_stand_go" not in said and "factory_failed" in said,
                             " ".join(said)))
    with tempfile.TemporaryDirectory() as tmp:
        p, said = run(tmp, ["--unit-id", "x", "--write", "--to", "3", "head-off"], [])
        results.append(check("refused rest read: head off its stop, then failed",
                             said == ["factory_head_off", "factory_failed"], " ".join(said)))
    with tempfile.TemporaryDirectory() as tmp:
        p, said = run(tmp, ["--unit-id", "x", "--write", "--from", "4"], ["y", "y"])
        results.append(check("step 4 exit 0", p.returncode == 0, p.stderr[-300:]))
        results.append(check("step 4 asks head and button only (strip not read)",
                             "factory_touch_now" in said and "factory_touch_now_strip" not in said, " ".join(said)))
        results.append(check("step 4 prompts in order", said == [
            "factory_selftest_ready", "factory_say_ready", "factory_selftest_wait", "factory_selftest_stand",
            "factory_touch_now", "factory_selftest_pass", "factory_reset_ready", "factory_resetting",
            "factory_all_done"], " ".join(said)))
    with tempfile.TemporaryDirectory() as tmp:
        p, said = run(tmp, ["--unit-id", "x", "--write", "--from", "4"], ["y", "y"], voice_on=True)
        results.append(check("set-up lamp: step 4 exit 0", p.returncode == 0, p.stderr[-300:]))
        results.append(check("set-up lamp: voice stopped, brief, voice started, before step 4",
                             said[:3] == ["voice-stop", "factory_selftest_brief", "voice-start"], " ".join(said)))
        results.append(check("set-up lamp: nothing played while lamp-voice holds the speaker",
                             all(s.startswith("refused-") for s in said[3:]) and len(said) > 3, " ".join(said)))
        results.append(check("set-up lamp: the refusal is said once", p.stderr.count("the speaker refused") == 1,
                             p.stderr[-300:]))
    with tempfile.TemporaryDirectory() as tmp:
        p, said = run(tmp, ["--unit-id", "x", "--write", "--to", "3"], ["y", "y"], voice_on=True)
        results.append(check("steps 1-3 never stop lamp-voice", "voice-stop" not in said, " ".join(said)))
    with tempfile.TemporaryDirectory() as tmp:
        p, said = run(tmp, ["--unit-id", "x", "--write", "--from", "4"], ["y", "y"], extra=["--strip"], voice_on=True)
        results.append(check("--strip brief asks for the swipe", "factory_selftest_brief_strip" in said, " ".join(said)))
    with tempfile.TemporaryDirectory() as tmp:
        p, said = run(tmp, ["--unit-id", "x", "--write", "--from", "4"], ["y", "y"], extra=["--strip"])
        results.append(check("--strip asks for the swipe", "factory_touch_now_strip" in said, " ".join(said)))
    with tempfile.TemporaryDirectory() as tmp:
        p, said = run(tmp, ["--unit-id", "x", "--write", "--from", "4"], ["y", "y"], aplay_fails=True)
        results.append(check("a refusing speaker does not stop the steps", p.returncode == 0, p.stderr[-300:]))
        results.append(check("a refusing speaker is said once", p.stderr.count("the speaker refused") == 1,
                             p.stderr[-300:]))
    with tempfile.TemporaryDirectory() as tmp:
        p, said = run(tmp, ["--unit-id", "x", "--write", "--to", "3"], [], extra=["--auto"])
        results.append(check("--auto: steps 1-3 in one go, nothing typed", p.returncode == 0, p.stderr[-300:]))
        results.append(check("--auto: 'Starting in five seconds' in place of 'Say ready', in order", said == [
            "factory_stand_ready", "factory_auto_start", "factory_stand_go", "factory_stand_30",
            "factory_stand_100", "factory_stand_done", "factory_pads_ready", "factory_auto_start",
            "factory_pads_away", "factory_pads_1", "factory_pads_2", "factory_pads_more",
            "factory_pads_wait", "factory_pads_done", "factory_key_done", "factory_all_done"], " ".join(said)))
    with tempfile.TemporaryDirectory() as tmp:
        mark = os.path.join(tmp, "said.log")
        p, said = run(tmp, ["--unit-id", "x", "--write", "--from", "4"], [],
                      extra=["--auto", "--auto-reset", "--before-reset", f"echo before-reset >> {mark}"])
        results.append(check("--auto-reset: step 4 and its reset in one go", p.returncode == 0, p.stderr[-300:]))
        results.append(check("--auto-reset: the record command runs before the reset is answered",
                             "before-reset" in said and said.index("before-reset") < said.index("factory_resetting"),
                             " ".join(said)))
    with tempfile.TemporaryDirectory() as tmp:
        p, said = run(tmp, ["--unit-id", "x", "--write", "--from", "4"], [], extra=["--auto", "--decline-reset"])
        results.append(check("--decline-reset: the reset is answered no", p.returncode == 1
                             and "factory_resetting" not in said and "factory_reset_ready" in said, " ".join(said)))
    sys.path.insert(0, HERE)
    import importlib.util
    spec = importlib.util.spec_from_file_location("factory_voice", RUNNER)
    fv = importlib.util.module_from_spec(spec); spec.loader.exec_module(fv)
    results.append(check("--auto never answers the reset", bool(fv.AUTO) and "reset" not in fv.AUTO, str(fv.AUTO)))
    pads = dict((k, t) for t, k in fv.YES["pads"])
    results.append(check("10 s for each pad after 11 s hands away",
                         pads["factory_pads_1"] - pads["factory_pads_away"] >= 11
                         and pads["factory_pads_2"] - pads["factory_pads_1"] >= 10
                         and pads["factory_pads_more"] - pads["factory_pads_2"] >= 10
                         and pads["factory_pads_wait"] <= 40, str(fv.YES["pads"])))
    print(f"{sum(results)}/{len(results)} passed")
    return 0 if all(results) else 1


if __name__ == "__main__":
    sys.exit(main())
