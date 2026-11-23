# Mac helpers for the split #8156 + #8213 ROW kit, copied from the #8156 round kit's mac.zsh (first run on lamp-c0a4).
# Every lamp call goes through LAMP_SSH, the runner's own stand-in: `$LAMP_SSH "<command>"` runs <command> on the lamp
# as its login user, passes this kit's stdin through, and `sudo -n` works in it. Auth is the stand-in's: the kit holds
# no password, key or host name. LL is the runner's lamp lock script (`$LL <lamp> try|renew|release|status <who>
# <session> [for]`), called with LAMP_SSH unset (lamp-lock.sh reads LAMP_SSH as its own ssh command).
zmodload zsh/datetime
D=${D:-${0:a:h}}
LAMP=${LAMP:-lamp-ee17}
: ${LAMP_SSH:?LAMP_SSH=<the runner ssh stand-in for the lamp> (README)} ${LL:?LL=<the runner lamp lock script> (README)}
typeset +x LAMP_SSH; W=$LAMP_SSH
LOCK_LAMP=$LAMP; export LOCK_WHO=${LOCK_WHO:-row-kit} LOCK_SESSION=${LOCK_SESSION:-split-row}; LOCK_FOR=${LOCK_FOR:-"#8156+#8213 split ROW"}
LOGROOT=${LOGROOT:-$D/logs}; mkdir -p $LOGROOT; ST=$LOGROOT/starts.log; touch $ST
# Root sh on the lamp: common.lamp.sh, the prepended values, the body, on the stand-in's stdin.
LAMP(){ local pre=$1; shift; { cat $D/common.lamp.sh; [ -n "$pre" ] && print -r -- "$pre"; cat "$@"; } | $W "sudo -n sh -s"; }
RENEW(){ $LL $LOCK_LAMP renew $LOCK_WHO $LOCK_SESSION >/dev/null 2>&1 || { echo "LOCK: not ours: stop"; return 1; }; }
# <=10 service starts in any 10 min: the kit's own starts plus the journal's (EARLIER adds those).
GATE(){ local lbl=$1 n=$2 now c i
  while :; do now=$(date +%s); c=$(awk -v t=$((now - 600)) '$1 > t' $ST | wc -l | tr -d ' ')
    [ $((c + n)) -le 10 ] && break
    echo "GATE $lbl: $c starts in the last 10 min + $n > 10: waiting 60 s"; RENEW || return 1; sleep 60; done
  for i in $(seq 1 $n); do echo "$now $(date '+%T') $lbl" >> $ST; done; }
EARLIER(){ $W "journalctl -u lamp-tasks -u lamp-update -u lamp-update-harness -u lamp-openclaw-gateway -u lamp-voice -u lamp-skills -u lamp-control -u lamp-cloud --since -10min -o short-unix --no-pager" < /dev/null | grep -E "systemd\[1\]: Started " | awk '{printf "%d earlier-start\n", $1}' >> $ST; sort -u -o $ST $ST; }
STARTS(){ echo "service starts by this kit: $(grep -vc earlier-start $ST); in the last 10 min (any): $(awk -v t=$(( $(date +%s) - 600 )) '$1 > t' $ST | wc -l | tr -d ' ')"; }
