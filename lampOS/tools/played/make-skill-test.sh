#!/usr/bin/env bash
# Tests for make-skill.sh with no lamp and no speaker: a stand-in lamp_ssh answers from what the stand-in speaker
# has played (the stand-in Harness's calls, the lamp's answers and words, the journal's lines, `lamp-skills list`),
# and stand-ins for say, afplay and the quiet gate. Run: lampOS/tools/played/make-skill-test.sh
set -uo pipefail

SCRIPT=$(cd "$(dirname "$0")" && pwd)/make-skill.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
SHA=$(printf 'a%.0s' $(seq 64))
printf 'fixture' >"$T/fixture"

cat >"$T/say" <<'EOF'
#!/usr/bin/env bash
# say -v VOICE -r RATE -o FILE TEXT: the "sound" is the text
printf '%s\n' "$7" >"$6"
EOF
cat >"$T/afplay" <<'EOF'
#!/usr/bin/env bash
cat "$1" >>"$FAKE_DIR/played"
EOF
cat >"$T/quiet" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
# The lamp, from what was played. Each played line is answered once. In an arm (from its request on): the lamp offers
# make_skill at once, or (FAKE_CONFIRM) after a first "Yes, please."; the next "Yes, please." hands it over (the
# stand-in's new, then show); the voice keeps the skill (FAKE_KEPT) and asks to put it on; "Yes, go ahead." (or,
# with FAKE_ON_NO, "No, not now.") puts it on, once an arm.
cat >"$T/state" <<'EOF'
#!/usr/bin/env python3
import os, sys
d = os.environ["FAKE_DIR"]
lines = open(os.path.join(d, "played")).read().splitlines() if os.path.exists(os.path.join(d, "played")) else []
arms, arm = [], None
for line in lines:
    if "make me a skill" in line:
        arm = []
        arms.append(arm)
    if arm is not None:
        arm.append(line)
confirm = bool(os.environ.get("FAKE_CONFIRM"))
kept = os.environ.get("FAKE_KEPT", "yes") == "yes"
out = {"answered": len(lines), "new": 0, "show": 0, "said": [], "journal": []}
for n, arm in enumerate(arms):
    # The script reads the journal since its arm began: this arm's lines only.
    out["journal"] = []
    yeses = sum(1 for line in arm if line == "Yes, please.")
    # Typed in the app (MAKE_FROM=app): handed over at once, with no yes.
    app = arm[0].startswith("APP ")
    offered = app or (yeses >= 1 if confirm else True)
    handed = app or yeses >= (2 if confirm else 1)
    if offered:
        out["journal"].append("lamp-voice provider: request 7 asked for make_skill")
        out["said"].append("I'll ask the computer to make a skill that nudges you to stand up. Shall I start it?")
    if handed:
        out["new"] += 1
        out["show"] += 1
        if kept:
            out["journal"].append("lamp-voice: a skill made by voice is kept, version 1.0.0, sha256 " + os.environ["FAKE_SHA"])
            # The news gate (El on #2889): typed in the app, the ask comes only once someone was heard (the greeting)
            # or is seen (FAKE_FACE).
            if not app or os.environ.get("FAKE_FACE") or "Hey Lamp, I'm back at my desk." in arm:
                out["said"].append("It breathes the light once an hour. Shall I put it on the lamp?")
            # FAKE_EARLY: put on before anyone answered, and the yes then refused as already on.
            if os.environ.get("FAKE_EARLY"):
                open(os.path.join(d, "installed"), "w").close()
                out["journal"].append("lamp-voice: a made skill is on the lamp: stand-up-nudge, sha256 " + os.environ["FAKE_SHA"])
        else:
            out["journal"].append("lamp-voice: a skill made by voice is not kept: it doesn't pass the lamp's check")
    if "Yes, go ahead." in arm and os.environ.get("FAKE_EARLY"):
        out["journal"].append("lamp-voice: a made skill was not put on: stand-up-nudge: it is on the lamp already")
    elif "Yes, go ahead." in arm or ("No, not now." in arm and os.environ.get("FAKE_ON_NO")):
        mark = os.path.join(d, "on-%d" % n)
        if not os.path.exists(mark):
            open(mark, "w").close()
            open(os.path.join(d, "installed"), "w").close()
        out["journal"].append("lamp-voice: a made skill is on the lamp: stand-up-nudge, sha256 " + os.environ.get("FAKE_ON_SHA", os.environ["FAKE_SHA"]))
        out["journal"].append('lamp-skills: loaded ["dance", "stand-up-nudge"]')
    # FAKE_SILENT_ON: on at a no with no line of the voice's, as an earlier arm's ask answered (lamp-06c1, 27 Oct).
    if "No, not now." in arm and os.environ.get("FAKE_SILENT_ON"):
        open(os.path.join(d, "installed"), "w").close()
what = sys.argv[1]
if what in ("new", "show"):
    print(out[what])
elif what == "answered":
    print(out["answered"])
elif what == "said":
    for text in out["said"]:
        print('{"kind":"transcript","turn":3,"text":"%s"}' % text)
else:
    print("\n".join(out["journal"]))
EOF
cat >"$T/lamp_ssh" <<'EOF'
#!/usr/bin/env bash
state() { "$FAKE_STATE" "$1"; }
echo "$*" >>"$FAKE_DIR/calls"
case "$*" in
  *"lamp-cloud >/dev/null"*) cat >/dev/null ;;  # the run's lamp-cloud, put beside the stand-in
  *"lamp-cloud make-skill make"*)  # the app's skills.make, by lamp-cloud's own call
    echo "APP make me a skill" >>"$FAKE_DIR/played"; echo making ;;
  *"lamp-cloud make-skill made"*)  # the app's skills.made: on once installed, else asked (FAKE_APP_ to differ)
    if [[ -e $FAKE_DIR/installed ]]; then on=on; else on=asked; fi
    echo "${FAKE_APP_STATE:-$on} ${FAKE_APP_SHA:-$FAKE_SHA}" ;;
  *"played-stand-in/saved"*)  # the voice's made skills and told news: saved, reset before an arm, put back
    case "$*" in *"touch"*) w=save ;; *"tasks-told && "*) w=restore ;; *) w=reset ;; esac
    echo "$w" >>"$FAKE_DIR/voice-state"
    [[ $w != save || -z ${FAKE_SAVE_FAILS:-} ]] ;;
  *"rm -rf /usr/local/lib/played-stand-in"*) true ;;
  *"sudo -n true"*) true ;;
  *"systemctl show"*) printf 'ActiveState=active\nEnvironment=%s\n' "$FAKE_ENV" ;;
  *"is-active -q lamp-skills-manage.socket"*) true ;;
  *"harness-fixture"*) cat >"$FAKE_DIR/fixture" ;;
  *"plan.json"*) cat >"$FAKE_DIR/plan.json"; [[ -z ${FAKE_PLAN_FAILS:-} ]] ;;
  *"sh -s -- in"*) cat >/dev/null; [[ -z ${FAKE_SETUP_FAILS:-} ]] || { echo "stand-in setup: no stand-in" >&2; exit 1; }
    touch "$FAKE_DIR/stand-in"; echo "$*" >"$FAKE_DIR/setup"; echo "stand-in in: lamp-tasks as lampos" ;;
  *"sh -s -- out"*) cat >/dev/null; rm -f "$FAKE_DIR/stand-in"; echo out >>"$FAKE_DIR/put-back"; echo "put back" ;;
  *"date +%s"*) date +%s ;;
  *calls.jsonl*) [[ $* == *'"new"'* ]] && state new || state show ;;
  *turn_finished*) state answered ;;
  *'"kind\":\"transcript'*|*transcript*) state said ;;
  *journalctl*) state journal ;;
  *" list"*)
    echo "NAME            KIND         VERSION  MAY USE"
    echo "dance           core         1.0.0    body"
    [[ -e $FAKE_DIR/installed || -n ${FAKE_NUDGE_ON:-} ]] && echo "stand-up-nudge  installable  1.0.0    light" ;;
  *" remove stand-up-nudge"*) [[ -e $FAKE_DIR/installed ]] || exit 1; rm -f "$FAKE_DIR/installed"; echo stand-up-nudge >>"$FAKE_DIR/removed" ;;
  *) echo "fake lamp: not known: $*" >&2; exit 1 ;;
esac
EOF
chmod +x "$T"/say "$T"/afplay "$T"/quiet "$T"/state "$T"/lamp_ssh

fail=0
run() { # NAME ARMS EXPECTED_EXIT GREP...: the script with this test's FAKE_ settings
  local name=$1 arms=$2 want=$3 got out
  shift 3
  rm -rf "$T/d"; mkdir -p "$T/d"
  out=$(FAKE_DIR=$T/d FAKE_STATE=$T/state FAKE_SHA=$SHA LAMP_HOST=fake LAMP_SSH=$T/lamp_ssh QUIET_GATE=$T/quiet \
    SAY=$T/say AFPLAY=$T/afplay PLAYED_OUT=$T/out MAKE_SETTLE=0 NOT_NOW_WAIT=0 LAMP_PLAY_AT_LUNCH=yes \
    "$SCRIPT" "$name" "$T/fixture" "$arms" 2>&1)
  got=$?
  [[ $got == "$want" ]] || { echo "FAIL $name: exit $got, wanted $want"; echo "$out"; fail=1; }
  for g in "$@"; do grep -qF -- "$g" <<<"$out" || { echo "FAIL $name: no '$g' in:"; echo "$out"; fail=1; }; done
  [[ ! -e $T/d/stand-in ]] || { echo "FAIL $name: the stand-in was left in"; fail=1; }
  [[ ! -e $T/d/installed ]] || { echo "FAIL $name: a skill was left installed"; fail=1; }
}

export FAKE_ENV="LAMP_VOICE_ROUTE=second LAMP_TASKS_DIR=/run/lampos-tasks"

# Both arms as they should go, offered at once: handed over to the stand-in, made, asked, put on only at the yes,
# enabled, loaded, removed; and nothing on a no.
run both both 0 "PASS asked to confirm: not needed" "PASS offered" "PASS handed over" "PASS the answer read" \
  "PASS made" "PASS asked to put it on" "PASS nothing on before the yes" \
  "PASS installed after the yes, the kept SHA-256" "PASS enabled" "PASS loaded by lamp-skills" "PASS removed" \
  "PASS nothing installed" "18 checks, 0 failed" "put back"
grep -qx stand-up-nudge "$T/d/removed" || { echo "FAIL both: the skill was not removed by the arm"; fail=1; }
grep -q 'PLAYED_VOICE_ENV=LAMP_MAKE_SKILL=on' "$T/d/setup" || { echo "FAIL both: the switch was not given to the voice"; fail=1; }
grep -q 'PLAYED_TARGET=auto' "$T/d/setup" || { echo "FAIL both: no targets file for the run"; fail=1; }
# The put-on's yes is a plain one (the voice's plain_agreement), never "Yes, put it on.", which it refuses.
grep -qx 'Yes, go ahead.' "$T/d/played" || { echo "FAIL both: the put-on's yes was not a plain one"; fail=1; }
# The voice's made skills saved, back as saved before each arm, and both put back at the end.
[[ $(tr '\n' ' ' <"$T/d/voice-state") == "save reset reset restore " ]] || { echo "FAIL both: voice state: $(tr '\n' ' ' <"$T/d/voice-state")"; fail=1; }
# The stand-in's answer is the two files a made skill's answer has, in its `show`.
python3 - "$T/d/plan.json" <<'PY' || { echo "FAIL both: the plan's answer is not a made skill's"; fail=1; }
import json, sys
plan = json.load(open(sys.argv[1]))
show = json.loads(plan["show"]["stdout"])
answer = show["answer"]
assert show["state"] == "idle" and plan["new"] == {"default": True}, show
# The agent the fixture's default `new` makes: `show`, `wait` and `ps` all name it.
agent = "11111111-1111-4111-8111-111111111111"
assert show["id"] == agent, show["id"]
assert [row["id"] for row in json.loads(plan["wait"]["stdout"])["harnesses"]] == [agent], plan["wait"]
assert [row["id"] for row in json.loads(plan["ps"]["stdout"])["harnesses"]] == [agent], plan["ps"]
assert "```toml skill.toml\n" in answer and "```markdown SKILL.md\n" in answer, answer
assert 'kind = "installable"' in answer and "name: stand-up-nudge" in answer and 'policy = "nudge"' in answer, answer
assert "speak" not in answer and "tool" not in answer, answer
PY

# Typed in the app (#2813): the words to the voice's app-make socket, no spoken ask and no yes to the hand-over; the
# app's view kept with the same SHA-256, then on (yes) or still off (no); only the spoken yes puts it on.
CLOUD=$T/fixture MAKE_FROM=app run app both 0 "PASS asked by the app" "PASS handed over" "PASS the app told it kept, the same SHA-256" \
  "PASS installed after the yes, the kept SHA-256" "PASS the app told it on" "PASS removed" "PASS nothing installed" \
  "PASS the app told it still off" "0 failed" "put back"
grep -q "LAMP_APP_MAKE_SKILL=on" "$T/d/setup" || { echo "FAIL app: the app's switch was not given to the voice"; fail=1; }
grep -q "^Hey Lamp, make me a skill" "$T/d/played" && { echo "FAIL app: the request was spoken"; fail=1; }
grep -q "python3" "$T/d/calls" && { echo "FAIL app: Python was run on the lamp"; fail=1; }
grep -q "sudo -n -u lampos .*lamp-cloud make-skill make" "$T/d/calls" || { echo "FAIL app: not asked as lampos"; fail=1; }
grep -qx "Hey Lamp, I'm back at my desk." "$T/d/played" || { echo "FAIL app: nothing said for the news to be told"; fail=1; }
# reviewer-two's controls on #2887: the app told another SHA-256, or on in the no arm, fails its check and the run.
FAKE_APP_SHA=$(printf 'b%.0s' $(seq 64)) CLOUD=$T/fixture MAKE_FROM=app run app-other-sha both 1 \
  "FAIL the app told it kept, the same SHA-256" "FAIL the app told it on" "FAIL the app told it still off"
FAKE_APP_STATE=on CLOUD=$T/fixture MAKE_FROM=app run app-on-at-no no 1 "FAIL the app told it still off"
# A face in view: the ask comes without the greeting, and no greeting is played over it (El on #2889).
FAKE_FACE=1 CLOUD=$T/fixture MAKE_FROM=app run app-face yes 0 "PASS asked to put it on" "PASS the app told it on" "0 failed"
grep -qx "Hey Lamp, I'm back at my desk." "$T/d/played" && { echo "FAIL app-face: the greeting was played over the ask"; fail=1; }
# No lamp-cloud given: nothing changed, nothing played.
MAKE_FROM=app run app-no-cloud yes 2 "needs CLOUD=PATH"
[[ ! -e $T/d/played ]] || { echo "FAIL app-no-cloud: something was played"; fail=1; }
FAKE_ENV="LAMP_VOICE_ROUTE=second LAMP_TASKS_DIR=/run/lampos-tasks LAMP_APP_MAKE_SKILL=on" CLOUD=$T/fixture MAKE_FROM=app run app-set yes 3 "already set"

# The lamp's own confirm before the hand-over: answered, then the hand-over's yes.
FAKE_CONFIRM=1 run confirm yes 0 "PASS asked to confirm: the lamp's answer played" "PASS handed over" "0 failed"
[[ $(grep -c '^Yes, please.$' "$T/d/played") == 2 ]] || { echo "FAIL confirm: not two yeses before the ask"; fail=1; }

# Another skill's bytes put on: not the one kept and asked about. It is still removed on exit.
FAKE_ON_SHA=$(printf 'b%.0s' $(seq 64)) run other-sha yes 1 "FAIL installed after the yes, the kept SHA-256"

# A skill put on at a "No, not now.": caught, and removed on exit.
FAKE_ON_NO=1 run on-no no 1 "FAIL nothing installed" "removed on exit: stand-up-nudge"

# A skill put on before the yes (the Reviewer-lead on #2745): failed, and removed on exit though the yes's own
# put-on found it on already.
FAKE_EARLY=1 run early yes 1 "FAIL nothing on before the yes" "FAIL installed after the yes" "removed on exit: stand-up-nudge"

# The yes arm's ask answered by the no arm's hand-over yes (lamp-06c1, 27 Oct): nothing logs it, and stand-up-nudge is
# still removed on exit.
FAKE_SILENT_ON=1 run leftover no 1 "FAIL nothing installed" "removed on exit: stand-up-nudge"

# stand-up-nudge on the lamp already: nothing changed, nothing played.
FAKE_NUDGE_ON=1 run nudge-on yes 3 "already on the lamp; nothing changed"
[[ ! -e $T/d/fixture ]] || { echo "FAIL nudge-on: the stand-in was sent"; fail=1; }

# The voice's state can't be saved: nothing played, and the stand-in's folder is removed.
FAKE_SAVE_FAILS=1 run save-fails yes 9 "could not be saved; nothing played"
[[ ! -e $T/d/played ]] || { echo "FAIL save-fails: something was played"; fail=1; }
grep -q 'rm -rf /usr/local/lib/played-stand-in$' "$T/d/calls" || { echo "FAIL save-fails: the stand-in's folder was left"; fail=1; }

# A skill the voice did not keep: the arm stops there.
FAKE_KEPT=no run not-kept yes 1 "FAIL made" "is not kept" "1 failed"

# The plan's upload fails: nothing played, and the stand-in's folder is removed.
FAKE_PLAN_FAILS=1 run plan-fails yes 9
grep -q 'rm -rf /usr/local/lib/played-stand-in' "$T/d/calls" || { echo "FAIL plan-fails: the stand-in's folder was left"; fail=1; }

# The stand-in's setup fails: nothing played, and it is put back.
FAKE_SETUP_FAILS=1 run setup-fails yes 9 "the stand-in's setup failed; nothing was played" "put back"
[[ ! -e $T/d/played ]] || { echo "FAIL setup-fails: something was played"; fail=1; }

# The switch already set, or no tasks part: nothing changed, nothing played.
FAKE_ENV="LAMP_TASKS_DIR=/run/lampos-tasks LAMP_MAKE_SKILL=on" run already yes 3 "already set"
[[ ! -e $T/d/fixture ]] || { echo "FAIL already: the stand-in was sent"; fail=1; }
FAKE_ENV="LAMP_VOICE_ROUTE=second" run no-tasks yes 3 "no tasks part"

[[ $fail == 0 ]] && echo "make-skill-test: all passed"
exit $fail
