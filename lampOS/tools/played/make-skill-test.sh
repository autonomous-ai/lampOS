#!/usr/bin/env bash
# Tests for make-skill.sh with no lamp and no speaker: a stand-in lamp_ssh answers from what the stand-in speaker
# has played (the stand-in Harness's calls, the lamp's answers and words, the journal's lines, `lamp-skills list`),
# and stand-ins for say, afplay and the quiet gate. Run: lampOS/tools/played/make-skill-test.sh [all|focused]
# `all` (the default) runs every group, about 13 minutes. `focused` (Third on #3936) skips the groups that play an
# arm and runs only the save, put-back and guard controls, none of which plays one: the failed list, the put-back
# that differs, digest failures, links, the empty folder, the failed stop, the save cut short, the copy left over
# and the lost Mac with the guard. They take a few seconds.
set -uo pipefail
case ${1:-all} in all | focused) GROUPS_RUN=${1:-all} ;; *) echo "usage: make-skill-test.sh [all|focused]" >&2; exit 2 ;; esac

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
            # The voice keeps it in its made skills: reset before the next arm, put back at the end.
            made = os.path.join(d, "voice", "made-skills", "stand-up-nudge")
            os.makedirs(made, exist_ok=True)
            open(os.path.join(made, "skill.toml"), "w").write("made")
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
# FAKE_GONE: the Mac loses the lamp at the stand-in's setup; nothing answers after.
[[ -n ${FAKE_GONE:-} && -e $FAKE_DIR/gone ]] && { cat >/dev/null; exit 255; }
case "$*" in
  *"lamp-cloud >/dev/null"*) cat >/dev/null ;;  # the run's lamp-cloud, put beside the stand-in
  *"lamp-cloud make-skill make"*)  # the app's skills.make, by lamp-cloud's own call
    echo "APP make me a skill" >>"$FAKE_DIR/played"; echo making ;;
  *"lamp-cloud make-skill made"*)  # the app's skills.made: on once installed, else asked (FAKE_APP_ to differ)
    if [[ -e $FAKE_DIR/installed ]]; then on=on; else on=asked; fi
    echo "${FAKE_APP_STATE:-$on} ${FAKE_APP_SHA:-$FAKE_SHA}" ;;
  *"rm -rf /usr/local/lib/played-stand-in"*) true ;;
  *"sudo -n true"*) true ;;
  *"systemctl show"*) printf 'ActiveState=active\nEnvironment=%s\n' "$FAKE_ENV" ;;
  *"is-active -q lamp-skills-manage.socket"*) true ;;
  *"harness-fixture"*) cat >"$FAKE_DIR/fixture" ;;
  *"plan.json"*) cat >"$FAKE_DIR/plan.json"; [[ -z ${FAKE_PLAN_FAILS:-} ]] ;;
  *"sh -s -- check"*) sh -s -- check ;;  # the save's check of the voice's paths, run here on this test's folders
  *"sh -s -- in"*) cat >/dev/null
    # FAKE_LINK_AT_SETUP: once saved, tasks-told turns into a dangling link, as no run makes one.
    [[ -z ${FAKE_LINK_AT_SETUP:-} ]] || { rm -f "$FAKE_DIR/voice/tasks-told"; ln -s "$FAKE_DIR/nowhere" "$FAKE_DIR/voice/tasks-told"; }
    [[ -z ${FAKE_GONE:-} ]] || { echo "$*" >"$FAKE_DIR/setup"; touch "$FAKE_DIR/gone"; exit 255; }
    [[ -z ${FAKE_SETUP_FAILS:-} ]] || { echo "stand-in setup: no stand-in" >&2; exit 1; }
    touch "$FAKE_DIR/stand-in"; echo "$*" >"$FAKE_DIR/setup"; echo "stand-in in: lamp-tasks as lampos" ;;
  *"sh -s -- out"*) cat >/dev/null; rm -f "$FAKE_DIR/stand-in"; echo out >>"$FAKE_DIR/put-back"; echo "put back" ;;
  *"$FAKE_DIR/saved"*)  # the voice's saved state, run here as on the lamp, on this test's folders; after the setup's
    # case, whose PLAYED_RESTORE names it. FAKE_RESTORE_BREAKS: the put-back's copy is not as saved;
    # FAKE_DIGEST_FAILS: sha256sum and shasum exit 17 with no digest.
    # The put-back runs in one root shell with lamp-voice's stop and start (El's P3s on #3889 and #3936): its step is
    # logged where it runs, between them, and this test's systemctl logs each of those.
    case "$*" in *"restore save"*) w=save ;; *"restore reset") w=reset ;; *"/restore; rc="*) w=restore ;; *) w="" ;; esac
    [[ -z $w || $w == restore ]] || echo "$w" >>"$FAKE_DIR/voice-state"
    [[ $w == save && -n ${FAKE_SAVE_FAILS:-} ]] && { cat >/dev/null; exit 1; }
    c=$*; c=${c//sudo -n /}
    r="sh $FAKE_DIR/saved/restore;"; c=${c//"$r"/"echo restore >>$FAKE_DIR/voice-state; $r"}
    # FAKE_SAVE_CUT: the save is killed after its first copy, and the Mac with it (no trap runs), as a Mac that drops.
    [[ $w == save && -n ${FAKE_SAVE_CUT:-} ]] && { PATH=$FAKE_CUTCP:$PATH sh -c "$c"; kill -9 $PPID; exit 255; }
    p=$FAKE_SYSBIN:$PATH; [[ $w == restore && -n ${FAKE_RESTORE_BREAKS:-} ]] && p=$FAKE_BADCP:$p
    [[ $w == restore && -n ${FAKE_DIGEST_FAILS:-} ]] && p=$FAKE_BADDIGEST:$p
    PATH=$p sh -c "$c" ;;
  *"date +%s"*) date +%s ;;
  *calls.jsonl*) [[ $* == *'"new"'* ]] && state new || state show ;;
  *turn_finished*) state answered ;;
  *'"kind\":\"transcript'*|*transcript*) state said ;;
  *journalctl*) state journal ;;
  *" list"*)
    [[ -z ${FAKE_LIST_FAILS:-} ]] || exit 17
    [[ -n ${FAKE_LIST_PART:-} ]] || echo "NAME            KIND         VERSION  MAY USE"
    echo "dance           core         1.0.0    body"
    [[ ! -e $FAKE_DIR/installed && -z ${FAKE_NUDGE_ON:-} ]] || echo "stand-up-nudge  installable  1.0.0    light" ;;
  *" remove stand-up-nudge"*) [[ -e $FAKE_DIR/installed ]] || exit 1; rm -f "$FAKE_DIR/installed"; echo stand-up-nudge >>"$FAKE_DIR/removed" ;;
  *) echo "fake lamp: not known: $*" >&2; exit 1 ;;
esac
EOF
# A cp whose copy is not the saved one: one file more in the made skills, (CP_CORRUPT=leaf, as Third's probe on
# 1c4d27e02) the same files with one's bytes changed, or (CP_CORRUPT=emptydir, Third's P3 on #3889) the same files
# with an empty folder dropped.
mkdir -p "$T/badcp"
cat >"$T/badcp/cp" <<'EOF'
#!/bin/sh
/bin/cp "$@" || exit
for d; do :; done
[ -d "$d/made-skills" ] || exit 0
case ${CP_CORRUPT:-} in
  leaf) echo corrupted >"$d/made-skills/own/skill.toml" ;;
  emptydir) rm -rf "$d/made-skills/kept" ;;
  *) echo extra >"$d/made-skills/extra" ;;
esac
EOF
# A cp that copies, then kills the save that ran it.
mkdir -p "$T/cutcp"
cat >"$T/cutcp/cp" <<'EOF'
#!/bin/sh
/bin/cp "$@" || exit
kill -9 $PPID
EOF
# The lamp's systemctl, inside the put-back's root shell: each call to lamp-voice logged, in order.
mkdir -p "$T/sysbin"
# FAKE_STOP_FAILS: its stop exits 17; FAKE_STILL_ACTIVE: its stop exits 0 and the voice is still active;
# FAKE_QUERY=unknown: is-active prints unknown and exits 4; FAKE_QUERY=none: it prints nothing and exits 1.
cat >"$T/sysbin/systemctl" <<'EOF'
#!/bin/sh
case "$1 $2 ${3:-}" in
  "stop lamp-voice "*) echo stop >>"$FAKE_DIR/voice-state"; [ -z "${FAKE_STOP_FAILS:-}" ] || exit 17
    [ -n "${FAKE_STILL_ACTIVE:-}" ] || touch "$FAKE_DIR/voice-stopped" ;;
  "start lamp-voice "*) echo start >>"$FAKE_DIR/voice-state"; rm -f "$FAKE_DIR/voice-stopped" ;;
  "is-active lamp-voice "* | "is-active -q lamp-voice")  # -q: the same status, nothing printed
    case ${FAKE_QUERY:-} in unknown) st=unknown; rc=4 ;; none) st=""; rc=1 ;; *)
      if [ -e "$FAKE_DIR/voice-stopped" ]; then st=inactive; rc=3; else st=active; rc=0; fi ;; esac
    [ "$2" = -q ] || [ -z "$st" ] || echo "$st"; exit $rc ;;
esac
EOF
chmod +x "$T/sysbin/systemctl"
# Digest tools that fail: exit 17, no digest (Third's HOLD on 1c4d27e02).
mkdir -p "$T/baddigest"
printf '#!/bin/sh\necho "digest failed" >&2\nexit 17\n' >"$T/baddigest/sha256sum"
cp "$T/baddigest/sha256sum" "$T/baddigest/shasum"
chmod +x "$T"/say "$T"/afplay "$T"/quiet "$T"/state "$T"/lamp_ssh "$T/badcp/cp" "$T/cutcp/cp" "$T/baddigest/sha256sum" "$T/baddigest/shasum"
# The voice's state as the lamp has it before a run, and its print: names and bytes.
seed() {
  mkdir -p "$T/d/voice/made-skills/own"
  printf 'own skill\n' >"$T/d/voice/made-skills/own/skill.toml"
  mkdir -p "$T/d/voice/made-skills/kept/empty"   # an empty subtree: no file in it
  printf 'told 381\n' >"$T/d/voice/tasks-told"
}
print_of() { (cd "$1" && find . | sort && find . -type f | sort | xargs cat) | cksum; }
SEED=$(rm -rf "$T/d"; seed; print_of "$T/d/voice")
SEED_MADE=$(print_of "$T/d/voice/made-skills")

fail=0
run() { # NAME ARMS EXPECTED_EXIT GREP...: the script with this test's FAKE_ settings
  local name=$1 arms=$2 want=$3 got out
  shift 3
  rm -rf "$T/d"; mkdir -p "$T/d"; seed
  case ${FAKE_LEFTOVER:-} in
    '') ;;
    part) mkdir -p "$T/d/saved.part"; printf 'made-skills dir\n' >"$T/d/saved.part/kinds" ;;  # a save cut short
    *) mkdir -p "$T/d/saved"; touch "$T/d/saved/done" ;;
  esac
  # FAKE_TOLD_LINK: tasks-told a link, dangling (to nothing) or to a file (Third's P2 on 93f0584c2).
  case ${FAKE_TOLD_LINK:-} in
    dangling) rm -f "$T/d/voice/tasks-told"; ln -s "$T/d/nowhere" "$T/d/voice/tasks-told" ;;
    file) printf 'told 381\n' >"$T/d/real-told"; rm -f "$T/d/voice/tasks-told"; ln -s "$T/d/real-told" "$T/d/voice/tasks-told" ;;
  esac
  out=$(FAKE_DIR=$T/d FAKE_STATE=$T/state FAKE_SHA=$SHA FAKE_BADCP=$T/badcp FAKE_BADDIGEST=$T/baddigest FAKE_CUTCP=$T/cutcp FAKE_SYSBIN=$T/sysbin MAKE_SAVED=$T/d/saved MAKE_VOICE_DIR=$T/d/voice \
    LAMP_HOST=fake LAMP_SSH=$T/lamp_ssh QUIET_GATE=$T/quiet \
    SAY=$T/say AFPLAY=$T/afplay PLAYED_OUT=$T/out MAKE_SETTLE=0 NOT_NOW_WAIT=0 LAMP_PLAY_AT_LUNCH=yes \
    "$SCRIPT" "$name" "$T/fixture" "$arms" 2>&1)
  got=$?
  [[ $got == "$want" ]] || { echo "FAIL $name: exit $got, wanted $want"; echo "$out"; fail=1; }
  for g in "$@"; do grep -qF -- "$g" <<<"$out" || { echo "FAIL $name: no '$g' in:"; echo "$out"; fail=1; }; done
  [[ ! -e $T/d/stand-in ]] || { echo "FAIL $name: the stand-in was left in"; fail=1; }
  [[ ! -e $T/d/installed ]] || { echo "FAIL $name: a skill was left installed"; fail=1; }
  # The voice's state as before the run, and no saved copy left, unless the test keeps it (KEEP_SAVED).
  if [[ -z ${KEEP_SAVED:-} ]]; then
    [[ ! -e $T/d/saved && ! -e $T/d/saved.part ]] || { echo "FAIL $name: the saved copy was left"; fail=1; }
    [[ $(print_of "$T/d/voice") == "$SEED" ]] || { echo "FAIL $name: the voice's state is not as before"; fail=1; }
  fi
}

export FAKE_ENV="LAMP_VOICE_ROUTE=second LAMP_TASKS_DIR=/run/lampos-tasks"

if [[ $GROUPS_RUN == all ]]; then
# Both arms as they should go, offered at once: handed over to the stand-in, made, asked, put on only at the yes,
# enabled, loaded, removed; and nothing on a no.
run both both 0 "PASS asked to confirm: not needed" "PASS offered" "PASS handed over" "PASS the answer read" \
  "PASS made" "PASS asked to put it on" "PASS nothing on before the yes" \
  "PASS installed after the yes, the kept SHA-256" "PASS enabled" "PASS loaded by lamp-skills" "PASS removed" \
  "PASS nothing installed" "18 checks, 0 failed" "put back"
grep -qx stand-up-nudge "$T/d/removed" || { echo "FAIL both: the skill was not removed by the arm"; fail=1; }
grep -q 'PLAYED_VOICE_ENV=LAMP_MAKE_SKILL=on' "$T/d/setup" || { echo "FAIL both: the switch was not given to the voice"; fail=1; }
grep -q 'PLAYED_TARGET=auto' "$T/d/setup" || { echo "FAIL both: no targets file for the run"; fail=1; }
grep -q 'PLAYED_CLOUD_ENV' "$T/d/setup" && { echo "FAIL both: lamp-cloud was touched for a spoken ask"; fail=1; }
# The put-on's yes is a plain one (the voice's plain_agreement), never "Yes, put it on.", which it refuses.
grep -qx 'Yes, go ahead.' "$T/d/played" || { echo "FAIL both: the put-on's yes was not a plain one"; fail=1; }
# The voice's made skills saved, back as saved before each arm, and both put back at the end.
[[ $(tr '\n' ' ' <"$T/d/voice-state") == "save reset reset stop restore start " ]] || { echo "FAIL both: voice state: $(tr '\n' ' ' <"$T/d/voice-state")"; fail=1; }
grep -q "PLAYED_RESTORE=$T/d/saved/restore" "$T/d/setup" || { echo "FAIL both: the guard was not given the put-back"; fail=1; }
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
# The running lamp-cloud gets it too, so the owner's app shows "Make a skill" (fourteen-P11 on #2814).
grep -q "PLAYED_CLOUD_ENV=LAMP_APP_MAKE_SKILL=on" "$T/d/setup" || { echo "FAIL app: the app's switch was not given to lamp-cloud"; fail=1; }
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

fi

# Third's controls on #3889, none of which plays an arm.
# The skill list fails (exit 17, nothing printed) or is not whole: the run stops before any change.
unchanged() { [[ ! -e $T/d/fixture && ! -e $T/d/voice-state ]] && ! grep -q " remove " "$T/d/calls" || { echo "FAIL $1: something was changed or removed"; fail=1; }; }
FAKE_LIST_FAILS=1 run list-fails yes 3 "could not be listed (exit 17); nothing changed"; unchanged list-fails
FAKE_LIST_PART=1 run list-part yes 3 "is not whole; nothing changed"; unchanged list-part
# A put-back that is not as saved (after a setup that failed): exit 8, the copy kept whole and named, never removed.
FAKE_SETUP_FAILS=1 FAKE_RESTORE_BREAKS=1 KEEP_SAVED=1 run restore-fails yes 8 \
  "restore: $T/d/voice is not as saved in $T/d/saved" "the saved copy is kept at $T/d/saved: put it back with: sudo sh $T/d/saved/restore" "put back"
[[ -e $T/d/saved/done && -e $T/d/saved/restore && $(print_of "$T/d/saved/made-skills") == "$SEED_MADE" ]] \
  || { echo "FAIL restore-fails: the saved copy is not kept whole"; fail=1; }
grep -q "rm -rf $T/d/saved" "$T/d/calls" && { echo "FAIL restore-fails: the saved copy was removed"; fail=1; }
# A put-back whose check fails still starts lamp-voice again, after its stop and the put-back (El's P3 on #3936).
[[ $(tr '\n' ' ' <"$T/d/voice-state") == "save stop restore start " ]] || { echo "FAIL restore-fails: lamp-voice not started again after a failed put-back: $(tr '\n' ' ' <"$T/d/voice-state")"; fail=1; }
# lamp-voice can't be stopped (its stop exits 17), is still active after it (Third's HOLD on a8962558b), or its state
# is not known (unknown, exit 4; nothing, exit 1: Third's HOLD on b2d2dfb0b): nothing is put back, the copy is kept
# whole, the voice started again, and the run exits 8 with the stand-in taken out.
for c in FAKE_STOP_FAILS=1 FAKE_STILL_ACTIVE=1 FAKE_QUERY=unknown FAKE_QUERY=none; do
  eval "$c FAKE_SETUP_FAILS=1 KEEP_SAVED=1 run stop-fails-${c#*=} yes 8 'lamp-voice could not be stopped, so nothing was put back' 'the saved copy is kept at $T/d/saved' 'put back'"
  [[ $(tr '\n' ' ' <"$T/d/voice-state") == "save stop start " && -e $T/d/saved/done && -e $T/d/saved/restore \
     && $(print_of "$T/d/saved/made-skills") == "$SEED_MADE" && ! -e $T/d/voice-stopped ]] \
    || { echo "FAIL stop-fails-${c#*=}: something was put back, the copy not kept whole, or the voice left stopped: $(tr '\n' ' ' <"$T/d/voice-state")"; fail=1; }
done
# The digest tool fails (exit 17) and the copy differs, or the copy is right and only the digest fails: nothing is
# proved, so exit 8 and the copy kept whole.
for c in "FAKE_RESTORE_BREAKS=1 CP_CORRUPT=leaf digest-fails-copy-differs" "FAKE_RESTORE_BREAKS= digest-fails"; do
  eval "${c% *} FAKE_SETUP_FAILS=1 FAKE_DIGEST_FAILS=1 KEEP_SAVED=1 run ${c##* } yes 8 'could not be read whole' 'the saved copy is kept at $T/d/saved'"
  [[ -e $T/d/saved/done && $(print_of "$T/d/saved/made-skills") == "$SEED_MADE" ]] || { echo "FAIL ${c##* }: the saved copy is not kept whole"; fail=1; }
done
# The put-back's copy drops an empty folder, every file's bytes the same (Third's P3 on #3889): folders are in the
# manifest, so it differs, exits 8, and the copy is kept whole, the empty folder in it.
FAKE_SETUP_FAILS=1 FAKE_RESTORE_BREAKS=1 CP_CORRUPT=emptydir KEEP_SAVED=1 run restore-drops-empty-dir yes 8 \
  "restore: $T/d/voice is not as saved in $T/d/saved" "the saved copy is kept at $T/d/saved"
[[ -d $T/d/saved/made-skills/kept/empty && ! -e $T/d/voice/made-skills/kept && $(print_of "$T/d/saved/made-skills") == "$SEED_MADE" ]] \
  || { echo "FAIL restore-drops-empty-dir: the copy is not kept whole, or the folder was not dropped"; fail=1; }
# tasks-told a link, dangling or to a file: the run stops before any change, the link and what it names as they were.
for k in dangling file; do
  FAKE_TOLD_LINK=$k KEEP_SAVED=1 run told-link-$k yes 3 "tasks-told is a symbolic link" "not a plain file or folder (above); nothing changed"
  [[ -L $T/d/voice/tasks-told && $(readlink "$T/d/voice/tasks-told") == "$T/d/"* && ! -e $T/d/saved && ! -e $T/d/fixture && ! -e $T/d/voice-state ]] \
    || { echo "FAIL told-link-$k: something was changed"; fail=1; }
  [[ $k != file || $(cat "$T/d/real-told") == "told 381" ]] || { echo "FAIL told-link-file: the linked file was changed"; fail=1; }
done
# A link where the save found a file: the put-back never removes it, exits 8, and keeps the copy whole.
FAKE_LINK_AT_SETUP=1 FAKE_SETUP_FAILS=1 KEEP_SAVED=1 run link-at-restore yes 8 "tasks-told is not a plain file or folder, so not this run's to replace" \
  "the saved copy is kept at $T/d/saved"
[[ -L $T/d/voice/tasks-told && -e $T/d/saved/done && $(cat "$T/d/saved/tasks-told") == "told 381" && $(print_of "$T/d/saved/made-skills") == "$SEED_MADE" ]] \
  || { echo "FAIL link-at-restore: the link was removed or the copy not kept whole"; fail=1; }
# A copy an earlier run left: the next run stops before any change, and names how to put it back.
FAKE_LEFTOVER=1 KEEP_SAVED=1 run leftover-copy yes 3 "an earlier run's saved copy of the voice's state is at $T/d/saved" "sudo sh $T/d/saved/restore"
[[ -e $T/d/saved/done && ! -e $T/d/fixture ]] || { echo "FAIL leftover-copy: the copy was touched or the stand-in sent"; fail=1; }
# A save cut short, the Mac gone with it (El's P3 on #3889): only saved.part is left, never a copy named as whole, and
# the voice's state is as it was.
FAKE_SAVE_CUT=1 KEEP_SAVED=1 run save-cut yes 137
[[ ! -e $T/d/saved && -d $T/d/saved.part && ! -e $T/d/saved.part/done && $(print_of "$T/d/voice") == "$SEED" ]] \
  || { echo "FAIL save-cut: a save cut short was left as the copy, or the voice was changed"; fail=1; }
# The next run after it: stopped before any change, told the voice is as it was and how to clear the cut-short save.
FAKE_LEFTOVER=part KEEP_SAVED=1 run part-left yes 3 "save was cut short at $T/d/saved.part, before anything was changed" \
  "remove it with: sudo rm -rf $T/d/saved.part"
[[ -e $T/d/saved.part/kinds && ! -e $T/d/saved && ! -e $T/d/fixture && ! -e $T/d/voice-state ]] \
  || { echo "FAIL part-left: the cut-short save was touched or the stand-in sent"; fail=1; }
# The Mac loses the lamp at the setup: its put-back can't reach the lamp (exit 8), the copy is kept, and the lamp's
# guard, armed by the setup with PLAYED_RESTORE, puts the voice's state back first; a guard whose put-back is not as
# saved keeps the copy and still takes the stand-in out.
FAKE_GONE=1 KEEP_SAVED=1 run gone yes 8 "were not put back as saved; the saved copy is kept at $T/d/saved"
restore=$(grep -o 'PLAYED_RESTORE=[^ ]*' "$T/d/setup" | cut -d= -f2)
mkdir -p "$T/d/voice/made-skills/stand-up-nudge"; printf 'made' >"$T/d/voice/made-skills/stand-up-nudge/skill.toml"
mkdir -p "$T/g/bin" "$T/g/remote"
cat >"$T/g/bin/systemctl" <<EOF
#!/bin/sh
case "\$1" in
cat) printf '[Service]\nUser=%s\nExecStart=$T/g/tasks serve --dir $T/g/store --harness /usr/local/bin/harness\n' "\$(id -un)" ;;
show) printf 'User=%s\nGroup=%s\n' "\$(id -un)" "\$(id -gn)" ;;
stop|start) echo "\$1 \$2" >>"$T/g/stops"; [ "\$1 \$2" != "stop lamp-voice" ] || [ -z "\${G_STOP_FAILS:-}" ] || exit 17 ;;
is-active) case \${G_QUERY:-} in unknown) st=unknown; rc=4 ;; none) st=""; rc=1 ;; *) st=inactive; rc=3 ;; esac
  [ "\$2" = -q ] || [ -z "\$st" ] || echo "\$st"; exit \$rc ;;
esac
EOF
printf '#!/bin/sh\nprintf "%%s\\n" "$@" >"%s"\n' "$T/g/guard" >"$T/g/bin/systemd-run"
printf '#!/bin/sh\n# --sign-in-by-code\n' >"$T/g/tasks"; printf '#!/bin/sh\n' >"$T/g/remote/harness-fixture"
chmod +x "$T/g/bin/systemctl" "$T/g/bin/systemd-run" "$T/g/tasks" "$T/g/remote/harness-fixture"
PATH=$T/g/bin:$PATH PLAYED_REMOTE=$T/g/remote PLAYED_UNITS=$T/g/units PLAYED_TASKS=$T/g/tasks PLAYED_RESTORE=$restore \
  sh "$(dirname "$SCRIPT")/stand-in.sh" in >"$T/g/out" 2>&1 || { echo "FAIL gone: the stand-in's setup failed:"; cat "$T/g/out"; fail=1; }
guard=$(tail -n 1 "$T/g/guard" 2>/dev/null)
[[ $restore == "$T/d/saved/restore" && $guard == "[ ! -e $restore ] || { systemctl stop lamp-voice && systemctl is-active lamp-voice | grep -qx -e inactive -e failed && /bin/sh $restore; systemctl start lamp-voice; }; rm -f "*"; rm -rf $T/g/remote; [ ! -e $restore ]" ]] \
  || { echo "FAIL gone: the guard does not put back first: $guard"; fail=1; }
# A guard whose stop of lamp-voice fails (Third's HOLD on a8962558b), or whose is-active says unknown (exit 4) or
# nothing (exit 1; Third's HOLD on b2d2dfb0b), puts nothing back, keeps the copy, starts the voice, still takes the
# stand-in out, and exits nonzero; so does one whose put-back is not as saved.
before=$(print_of "$T/d/voice")
for g in G_STOP_FAILS=1 G_QUERY=unknown G_QUERY=none; do
  mkdir -p "$T/g/remote" "$T/g/units/lamp-voice.service.d"; touch "$T/g/units/lamp-voice.service.d/zz-played-stand-in.conf"
  env "$g" PATH="$T/g/bin:$PATH" /bin/sh -c "$guard" 2>"$T/g/err"; got=$?
  [[ $got != 0 && -e $T/d/saved/done && $(print_of "$T/d/voice") == "$before" && ! -e $T/g/remote && ! -e $T/g/units/lamp-voice.service.d/zz-played-stand-in.conf ]] \
    || { echo "FAIL gone ($g): the guard exited $got, put back, lost the copy or left the stand-in in"; fail=1; }
done
PATH=$T/badcp:$T/g/bin:$PATH /bin/sh -c "$guard" 2>"$T/g/err"; got=$?
[[ $got != 0 && -e $T/d/saved/done && ! -e $T/g/remote && ! -e $T/g/units/lamp-voice.service.d/zz-played-stand-in.conf ]] \
  || { echo "FAIL gone: a guard whose put-back failed exited $got, lost the copy or left the stand-in in"; fail=1; }
PATH=$T/g/bin:$PATH /bin/sh -c "$guard"; got=$?
[[ $got == 0 && ! -e $T/d/saved && $(print_of "$T/d/voice") == "$SEED" ]] || { echo "FAIL gone: the guard exited $got or did not put the voice's state back"; fail=1; }
# Each guard run stops lamp-voice before its put-back and starts it after, its stop or put-back failed or not.
[[ $(grep ' lamp-voice$' "$T/g/stops" | tr '\n' ' ') == "$(printf 'stop lamp-voice start lamp-voice %.0s' 1 2 3 4 5)" ]] \
  || { echo "FAIL gone: the guard did not stop and then start lamp-voice around each put-back: $(tr '\n' ' ' <"$T/g/stops")"; fail=1; }

[[ $fail == 0 ]] && echo "make-skill-test: all passed"
exit $fail
