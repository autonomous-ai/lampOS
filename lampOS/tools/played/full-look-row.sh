#!/bin/zsh
# The product's full look on a lamp (#2998, #2791's product half): the eyes' cost of `--sight-from-look` off and on, then
# ten "what do you see?" asks as installed. Needs a lamp on the product units with #2998 (lamp-eyes passes --full-look and
# --sight-from-look on; the voice LAMP_LOOK_FULL=on). PLAYS SOUND IN THE ROOM, and each look sends a still of the desk to
# the model: no ask is played while the eyes see a face (their presence file, /run/lampos-eyes/present, a face on a
# screen included), and the run stops there. About 35 minutes. argv: LABEL.
#  Before each change to the eyes and before each ask it waits for the lamp to have heard and said nothing for 45 s
#  (quiet_gate.sh; unreadable counts as not quiet), up to QUIET_MAX seconds (default 900), else it stops (exit 5).
#  1. The pair, as lamp-75c9's (22 Oct 06:30, Ten on #2602): a /run drop-in runs the unit's own ExecStart with
#     `--sight-from-look off`, then `on`, both with `--look-on-change on` (the 10 s line's stage times need it); 30 s to
#     settle, then 4 minutes each. Prints the medians of the 10 s line's "sight file" and "decode" ms a look and of its
#     "ms a look". The drop-in is removed on exit and the eyes restarted, as installed.
#  2. The asks: "Hey Lamp, what do you see?", each in a scene of its own. Per ask, from the voice's session: the first
#     and second times the lamp starts speaking after the end of speech was decided (for a look, normally its "Let me
#     look." and the look's answer), and what the lamp said.
# Its stand-in test is full-look-row-test.sh.
cd "${0:A:h}" || exit 9
. ./settings.sh
L=${1:?usage: full-look-row.sh LABEL}
D=/run/systemd/system/lamp-eyes.service.d
lamp() { ./lamp_ssh "$@" </dev/null }
lamp "ls $D 2>/dev/null" | grep -q . && { echo "full-look-row: a /run drop-in is on lamp-eyes already; not starting" >&2; exit 3 }
cmd=$(lamp 'tr "\0" " " < /proc/$(systemctl show -p MainPID --value lamp-eyes)/cmdline')
[[ $cmd == *"--full-look "* && $cmd == *"--sight-from-look on"* ]] || { echo "full-look-row: lamp-eyes runs without --full-look and --sight-from-look on (a build before #2998?): $cmd" >&2; exit 3 }
# Each step checked, and the drop-in's absence after: a failed step fails the whole put-back.
restore() { lamp "sudo -n rm -f $D/zz-look-row.conf && sudo -n systemctl daemon-reload && sudo -n systemctl restart lamp-eyes && ! test -e $D/zz-look-row.conf" }
# Tried up to three times; says so when the lamp is still left changed.
put_back() {
  local i
  for i in 1 2 3; do restore && return 0; sleep 5; done
  echo "full-look-row: could not take $D/zz-look-row.conf off lamp-eyes; take it off and restart lamp-eyes by hand" >&2
  return 1
}
# Armed only once the first drop-in goes on: a run refused before that touches nothing on the lamp.
armed=no
trap 'exit 130' INT TERM
QUIET_MAX=${QUIET_MAX:-900}
quiet() { # until the lamp has heard and said nothing for 45 s
  local waited=0
  until ./quiet_gate.sh 45 >/dev/null 2>&1; do
    ((waited >= QUIET_MAX)) && { echo "full-look-row: the lamp was not quiet for 45 s within ${QUIET_MAX} s; stopped" >&2; exit 5 }
    sleep 10; waited=$((waited + 10))
  done
}
nobody() { # no face in the eyes' view: each look's still goes to the model
  [[ $(lamp 'test -e /run/lampos-eyes/present && echo face || echo none') == none ]] ||
    { echo "full-look-row: the eyes see a face (or couldn't be asked); no still is sent with someone in view; stopped" >&2; exit 8 }
}
line=$(lamp 'grep "^ExecStart=" "$(systemctl show -p FragmentPath --value lamp-eyes)" | cut -d= -f2-')
for mode in off on; do
  quiet
  [[ $armed == yes ]] || { trap 'put_back' EXIT; armed=yes }
  print -r -- "[Service]
ExecStart=
ExecStart=${line/--sight-from-look on/--sight-from-look $mode} --look-on-change on" | ./lamp_ssh "cat > /tmp/zz-look-row.conf && sudo -n mkdir -p $D && sudo -n install -m 0644 /tmp/zz-look-row.conf $D/zz-look-row.conf && rm /tmp/zz-look-row.conf && sudo -n systemctl daemon-reload && sudo -n systemctl restart lamp-eyes" || { echo "full-look-row: the drop-in could not be put on (sudo -n)" >&2; exit 4 }
  sleep 30
  got=$(lamp 'tr "\0" " " < /proc/$(systemctl show -p MainPID --value lamp-eyes)/cmdline')
  [[ $got == *"--sight-from-look $mode"*"--look-on-change on"* ]] || { echo "full-look-row: lamp-eyes isn't running the $mode arm: $got" >&2; exit 6 }
  s=$(lamp 'date +%s'); sleep 240
  lamp "sudo -n journalctl -u lamp-eyes --since @$s --no-pager" | python3 -c '
import re, statistics, sys
look, sight, decode = [], [], []
for l in sys.stdin:
    m = re.search(r"unreadable, (\d+) ms a look", l)
    if m: look.append(int(m.group(1)))
    m = re.search(r"ms a look: sight file ([\d.]+), decode ([\d.]+)", l)
    if m: sight.append(float(m.group(1))); decode.append(float(m.group(2)))
med = lambda v: "%.1f" % statistics.median(v) if v else "none"
print("sight-from-look %s: %d lines, ms a look %s, sight file %s, decode %s, the two %s" % (sys.argv[1], len(look), med(look), med(sight), med(decode),
      med([a + b for a, b in zip(sight, decode)])))' $mode
done
# The guard stays armed until the eyes are known to run as installed again.
quiet
put_back || exit 6
# The asks go to the eyes as installed: wait until they run their own line again, with the full look.
for i in {1..30}; do
  got=$(lamp 'systemctl is-active lamp-eyes; tr "\0" " " < /proc/$(systemctl show -p MainPID --value lamp-eyes)/cmdline')
  [[ $got == active*"--full-look "*"--sight-from-look on"* && $got != *"--look-on-change on"* ]] && break
  sleep 2
done
[[ $got == active*"--full-look "*"--sight-from-look on"* && $got != *"--look-on-change on"* ]] || { echo "full-look-row: lamp-eyes isn't back as installed with the full look: $got" >&2; exit 6 }
trap - EXIT
sleep 10
files=()
for i in {1..10}; do
  quiet; nobody
  mark=$(mktemp)
  ./scenario.py $L-look-$i "Daniel|Hey Lamp, what do you see?|40" > /dev/null 2>&1 || { echo "full-look-row: scenario.py failed on ask $i (exit $?); stopped" >&2; rm -f $mark; exit 7 }
  # This run's file only: one with the same LABEL from an earlier run is never read as this one's.
  F=$(find "$PLAYED_OUT"/scenarios -name "*-$L-look-$i.txt" -newer $mark | head -1); rm -f $mark
  [[ -n $F ]] || { echo "full-look-row: scenario.py left no file for ask $i" >&2; exit 7 }
  files+=("$F")
done
python3 - "${files[@]}" <<'EOF'
import re, sys
asks, cur = [], None
for l in (l for path in sys.argv[1:] for l in open(path)):
    if ' PLAY ' in l: cur = {'starts': [], 'said': []}; asks.append(cur); continue
    if cur is None: continue
    m = re.search(r'\[lamp starts speaking ([\d.]+) s after that decision\]', l)
    if m: cur['starts'].append(float(m.group(1)))
    m = re.match(r'\s*[\d.]+s LAMP  (.+)', l)
    if m: cur['said'].append(m.group(1).strip())
for i, a in enumerate(asks, 1):
    s = a['starts'] + [None, None]
    f = lambda v: '%.2f s' % v if v is not None else 'none'
    print('ask %d: first sound %s, second %s; said: %s' % (i, f(s[0]), f(s[1]), ' | '.join(a['said'])[:220]))
EOF
echo "full-look-row $L: scenes ${files[1]:h}/*-$L-look-{1..10}.txt"
