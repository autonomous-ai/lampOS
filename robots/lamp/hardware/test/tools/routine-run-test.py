#!/usr/bin/env python3
# Host test for routine-run.py with stand-ins for systemctl, aplay, steps.sh and
# the ring and privacy scripts: no lamp, no sound. Checks the prompt order, that
# lamp-voice is stopped for each prompt and running again for each step that
# needs it, the answers file, and that an unanswered question is not_checked.
#
#   python3 routine-run-test.py
import json, os, subprocess, sys, tempfile, textwrap

HERE = os.path.dirname(os.path.abspath(__file__))
RUNNER = os.path.join(HERE, "routine-run.py")


def stub(path, body):
    with open(path, "w") as f:
        f.write("#!/bin/bash\n" + textwrap.dedent(body))
    os.chmod(path, 0o755)


def setup(tmp):
    log = os.path.join(tmp, "events.log")
    state = os.path.join(tmp, "voice.state")
    open(state, "w").write("active")
    stub(os.path.join(tmp, "systemctl"), f"""
        case "$1" in
          is-active) [ "$(cat {state})" = active ] ;;
          stop) echo inactive > {state}; echo "voice stop" >> {log} ;;
          start) echo active > {state}; echo "voice start" >> {log} ;;
        esac
    """)
    stub(os.path.join(tmp, "aplay"), f"""
        [ "$(cat {state})" = active ] && echo "PROMPT WHILE VOICE ON" >> {log}
        echo "say $(basename "${{@: -1}}" .wav)" >> {log}
    """)
    st = os.path.join(tmp, "selftest"); os.makedirs(st)
    stub(os.path.join(tmp, "steps.sh"), f"""
        echo "steps $*" >> {log}
        [ "$(cat {state})" = active ] || echo "STEP WITHOUT VOICE" >> {log}
        case "$*" in *"--from 4"*) echo '{{"verdict":"pass"}}' > {st}/123.json ;; esac
    """)
    for name, check in (("ring.py", "ring_pixels"), ("privacy.py", "privacy_switch")):
        with open(os.path.join(tmp, name), "w") as f:
            f.write(f"import json,sys,os\n"
                    f"open({log!r},'a').write('script {check}\\n')\n"
                    f"print(json.dumps({{'check':'{check}','verdict':'pass','reason':'stand-in','at':'2026-10-24T00:00:00Z'}}))\n")
    prompts = os.path.join(tmp, "prompts"); os.makedirs(prompts)
    for k in ("start", "inspection", "pads", "selftest", "ring", "privacy", "buzz", "exchange", "done_pass", "done_fail"):
        open(os.path.join(prompts, k + ".wav"), "wb").write(b"RIFF")
    env = dict(os.environ, SYSTEMCTL=os.path.join(tmp, "systemctl"), APLAY=os.path.join(tmp, "aplay"),
               STEPS_SH=os.path.join(tmp, "steps.sh"), RING_PIXELS=os.path.join(tmp, "ring.py"),
               PRIVACY_SWITCH=os.path.join(tmp, "privacy.py"), SELFTEST_DIRS=st,
               ROUTINE_RUN_STDIN_OK="1", ROUTINE_RUN_NOT_ROOT_OK="1")
    return log, prompts, env


def run(answers, extra=()):
    tmp = tempfile.mkdtemp()
    log, prompts, env = setup(tmp)
    out = os.path.join(tmp, "run")
    p = subprocess.run([sys.executable, RUNNER, "--unit-id", "lamp-test", "--prompts", prompts,
                        "--out", out, "--voice-settle", "0", *extra],
                       input=answers, text=True, capture_output=True, env=env, timeout=60)
    doc = json.load(open(os.path.join(out, "answers.json")))
    return p.returncode, open(log).read().splitlines(), doc, out


fails = 0
def check(name, ok):
    global fails
    print(("ok   " if ok else "FAIL ") + name)
    fails += 0 if ok else 1

# 1. Everything answered pass.
# Inputs, in order: inspection p, (Enter before the self-test), buzz p, exchange p.
rc, ev, doc, out = run("p\n\np\np\n")
checks = {s["check"]: s["verdict"] for s in doc["steps"]}
check("all pass: exit 0", rc == 0)
check("all pass: five answers, all pass", checks == {"assembly_inspection": "pass", "ring_pixels": "pass",
      "privacy_switch": "pass", "speaker_buzz": "pass", "spoken_exchange": "pass"})
check("prompts in step order", [e[4:] for e in ev if e.startswith("say ")] ==
      ["start", "inspection", "pads", "selftest", "ring", "privacy", "buzz", "exchange", "done_pass"])
check("never a prompt while lamp-voice holds the speaker", "PROMPT WHILE VOICE ON" not in ev)
check("lamp-voice running for every factory step", "STEP WITHOUT VOICE" not in ev)
check("self-test report kept", os.path.exists(os.path.join(out, "selftest.json")))
check("lamp-voice left running", ev[-1] == "voice start")

# 2. Nothing answered: the asked checks are not_checked, the run is incomplete.
rc, ev, doc, out = run("")
checks = {s["check"]: s["verdict"] for s in doc["steps"]}
check("unanswered: exit 3 (incomplete)", rc == 3)
check("unanswered: inspection not_checked", checks.get("assembly_inspection") == "not_checked")
check("unanswered: never a pass for an asked check",
      all(checks[c] != "pass" for c in ("assembly_inspection", "speaker_buzz", "spoken_exchange")))

# 3. A fail with a reason.
rc, ev, doc, out = run("f\nscrew loose at the base\n\np\np\n")
insp = [s for s in doc["steps"] if s["check"] == "assembly_inspection"][0]
check("fail: exit 1", rc == 1)
check("fail: reason kept", insp["verdict"] == "fail" and "screw loose" in insp["reason"])

# 4. --no-voice: no prompt at all, steps still run.
rc, ev, doc, out = run("p\n\np\np\n", ["--no-voice"])
check("--no-voice: silent", not any(e.startswith("say ") for e in ev))
check("--no-voice: steps ran", any(e.startswith("script ring_pixels") for e in ev))

# 5. --only ring: one step.
rc, ev, doc, out = run("", ["--only", "ring"])
check("--only ring: just ring_pixels", [s["check"] for s in doc["steps"]] == ["ring_pixels"])

print(f"{fails} failed" if fails else "all passed")
sys.exit(1 if fails else 0)
