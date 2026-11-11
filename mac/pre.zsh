#!/bin/zsh
# Kit 5194 preflight, on the Mac, read-only (changes nothing on the lamp, plays nothing). Reads the
# lamp's installed build and checks with git that it carries #5235 (29cc36da91, train 678); reads
# voice/mute, expression/state and whether lamp-voice already has LAMP_OPENS_FURTHER or /run drop-ins.
#   zsh $K/mac/pre.zsh [LAMPOS_CHECKOUT]    (default: the lamp-jarvis worktree)
set -u
K=${0:A:h:h}; PASS=orangepi; HOST=blue-lamp
G=${1:-/Users/macbookpro/orca/workspaces/autonomous-os/lamp-jarvis}
MERGE=29cc36da91
R() { sshpass -p $PASS ssh -o ConnectTimeout=15 -o LogLevel=ERROR $HOST "echo $PASS | sudo -S -p '' sh -c \"$1\""; }
ok=1
ST=$(R "head -c 300 /var/lib/lampos-update/state.json" | tr -d ' \n')
SEQ=$(print -r -- $ST | grep -o '"installed":{"sequence":[0-9]*' | grep -o '[0-9]*$')
C=$(print -r -- $ST | grep -o '"installed":{"sequence":[0-9]*,"commit":"[0-9a-f]*"' | grep -o '[0-9a-f]\{40\}')
print "installed: ${SEQ:-?} ${C:-?}"
git -C $G fetch -q origin lamp-voice 2>/dev/null
if [[ -n $C ]] && git -C $G cat-file -e "${C}^{commit}" 2>/dev/null; then
  git -C $G merge-base --is-ancestor $MERGE $C && print "OK: $C carries #5235 ($MERGE)" || { print "NO: $C does not carry #5235 ($MERGE): do not run"; ok=0; }
else print "?: $C is not in $G (fetch it); setup.sh's binary check still refuses a build without the switch"; fi
BUS='$(command -v lamp-bus || echo /opt/lampOS/bus/current/lamp-bus) --root /run/lampos-bus'
print "voice/mute: $(R "$BUS latest voice/mute" 2>&1 | grep -o '"muted":[a-z]*' | head -1)"
print "expression: $(R "$BUS latest expression/state" 2>&1 | grep -o '"asleep":[a-z]*' | head -1) (setup refuses asleep:true)"
print "lamp-voice: $(R 'systemctl is-active lamp-voice') drop-ins: $(R 'systemctl show -p DropInPaths --value lamp-voice')"
E=$(R 'grep -a -c LAMP_OPENS_FURTHER= /proc/$(systemctl show -p MainPID --value lamp-voice)/environ')
[[ ${E:-0} == 0 ]] && print "OK: lamp-voice runs without LAMP_OPENS_FURTHER (the off pass is stock)" || { print "NO: lamp-voice already runs with LAMP_OPENS_FURTHER"; ok=0; }
print "updater: self.json $(R '[ -e /var/lib/lampos-update/self.json ] && echo present || echo none'), lamp-update $(R 'systemctl is-active lamp-update')"
print "Mac volume now: $(osascript -e 'output volume of (get volume settings)') (the driver sets 45)"
(( ok )) && print "PREFLIGHT OK" || print "PREFLIGHT NO"
