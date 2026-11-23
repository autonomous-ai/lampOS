# lamp, root sh (common first; then STABLE_SEQ= STABLE_UPD_SHA= HV_STABLE=). READ ONLY. The start state: stable's bytes
# (build STABLE_SEQ, or above it with stable's own lamp-update after an earlier put-back at a higher sequence), on its
# product line, nothing staged, Harness at stable's pin, OpenClaw the lamp's agent, lamp-cloud active.
# Prints SEQ installed=<n> max=<the highest seen>, FAILED0 <units already failed>, EXESHA (the running exes, as found),
# SPEAKER <control> <n>% (read only, never set: at most SPK_MAX), T HAUTH (Harness sign-in, a fact only), PRIVACY (the mic slide), then BASE OK | BASE FAIL <why>.
cd /
why=""
b=$(build); m=$(seqmax); echo "SEQ installed=$b max=$m busy='$(updbusy)'"
u=$(sha256sum $UPD/current/lamp-update 2>/dev/null | cut -d' ' -f1); r=$(exesha lamp-update)
echo "UPDATER current=$u running=$r stable=$STABLE_UPD_SHA"
[ "$u" = "$STABLE_UPD_SHA" ] && [ "$r" = "$STABLE_UPD_SHA" ] || why="$why updater-not-stable's"
if [ "$b" != "$STABLE_SEQ" ]; then [ "${b:-0}" -gt "$STABLE_SEQ" ] 2>/dev/null && [ "$u" = "$STABLE_UPD_SHA" ] && echo "NOTE on $b, above stable $STABLE_SEQ, with stable's lamp-update" || why="$why not-on-stable-$STABLE_SEQ($b)"; fi
[ -z "$(updbusy)" ] || why="$why a-round-under-way"
echo "LINE $(line)"; product_line || why="$why lamp-update-line-not-product"
D=$(dropins); [ -n "$D" ] && { echo "$D"; why="$why drop-ins-present"; }
[ -e $CH ] && why="$why $CH-exists"
v=$(hver); echo "HARNESS $v"; hrec
echo "$v" | grep -qF "$HV_STABLE" || why="$why harness-not-$HV_STABLE"
e=$(engine); echo "ENGINE ${e:-none}"; [ "$e" = openclaw ] || why="$why engine-not-openclaw"
echo "UNITS $(for x in lamp-voice lamp-tasks lamp-skills lamp-openclaw-gateway lamp-cloud lamp-update lamp-control; do printf '%s=%s ' $x $(systemctl is-active $x); done)"
for x in lamp-voice lamp-tasks lamp-openclaw-gateway lamp-cloud lamp-update; do systemctl is-active -q $x || why="$why $x-not-active"; done
agent_now
echo "FAILED0 $(failed)"
exeshas
# Harness's sign-in: a fact only, never judged (the lamp's own OpenClaw runs while Harness is signed out, #6407;
# lamp-tasks' own tasks/agent error/signed_out is verify's check). T: it may move.
echo "T $(hauth)"
# The physical mic slide (voice/privacy physical_muted): closed fails; unread is printed, not judged.
pv=$(mute_live | grep '^PRIVACY'); echo "${pv:-PRIVACY unread}"; echo "$pv" | grep -q 'physical_muted=True' && why="$why mic-slide-closed"
s=$(speaker); echo "$s"; p=$(echo "$s" | sed -n 's/^SPEAKER [^ ]* \([0-9]*\)%$/\1/p'); [ -n "$p" ] && [ "$p" -le "${SPK_MAX:-40}" ] || why="$why speaker-volume-over-${SPK_MAX:-40}-or-unread"
echo "KEYPUB $(sha256sum /etc/lampOS/update/key.pub 2>/dev/null | cut -c1-16)"
[ -z "$why" ] && echo "BASE OK" || echo "BASE FAIL$why"
