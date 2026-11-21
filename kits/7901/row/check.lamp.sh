# lamp, root sh (row common first, then PR_VOICE_SHA= PR_SKILLS_SHA= PR_CONTROL_SHA= PR_TASKS_SHA=). READ ONLY.
# Before the legs: is the lamp on #7901's build (the runner's round), with the row's product switches, and ready?
# Each part: the running exe's sha256 and the installed file's (/opt/lampOS/<part>/current/<bin>, which
# lamp-skills-manage@ and the agent's make check run). With PR_<PART>_SHA given, both must be exactly it; with none,
# they are printed only. Also OpenClaw's gateway view of lamp-skills (its own mount namespace).
# Prints BIN lines, VERIFY PASS | VERIFY FAIL <why>, then READY | NOT_READY <why>.
cd /
why=""; given=0
for p in $PARTS; do part=${p%%:*}; r=${p#*:}; unit=${r%%:*}; bin=${r#*:}
  case $part in voice) w=${PR_VOICE_SHA:-};; skills) w=${PR_SKILLS_SHA:-};; control) w=${PR_CONTROL_SHA:-};; tasks) w=${PR_TASKS_SHA:-};; esac
  q=$(pid $unit); rs=$( [ "${q:-0}" != 0 ] && sha256sum /proc/$q/exe 2>/dev/null | cut -d' ' -f1 || echo none)
  f=/opt/lampOS/$part/current/$bin; fs=$(sha256sum "$f" 2>/dev/null | cut -d' ' -f1)
  echo "BIN $unit running=$rs file=${fs:-none} ($(readlink -f $f 2>/dev/null)) dropins=$(systemctl show -p DropInPaths --value $unit 2>/dev/null | tr ' ' ',')"
  if [ -n "$w" ]; then given=$((given+1))
    [ "$rs" = "$w" ] && [ "$fs" = "$w" ] && echo "BIN $unit OK = PR_$(echo $part | tr a-z A-Z)_SHA" || why="$why $unit-not-the-given-PR-sha"
  else echo "BIN $unit PR_$(echo $part | tr a-z A-Z)_SHA not set: compare running=$rs with the artifact's SHA256SUMS"; fi
done
gp=$(pid lamp-openclaw-gateway); gs=$( [ "${gp:-0}" != 0 ] && nsenter -t $gp -m sha256sum $SKBIN 2>/dev/null | cut -d' ' -f1)
echo "GATEWAY sees lamp-skills ${gs:-unread}"
[ -n "${PR_SKILLS_SHA:-}" ] && [ -n "$gs" ] && [ "$gs" != "$PR_SKILLS_SHA" ] && why="$why gateway-sees-another-lamp-skills"
echo "BUILD sequence $(build); lamp-update dropins=$(systemctl show -p DropInPaths --value lamp-update 2>/dev/null | tr ' ' ',')"
u=$(updbusy); [ -z "$u" ] || why="$why update-round-under-way($u)"
e=$(engine); echo "ENGINE ${e:-none}"; [ "$e" = openclaw ] || why="$why engine-not-openclaw"
v=$(pid lamp-voice); for x in $NEED; do [ "$(envval $v $x)" = on ] || why="$why lamp-voice-no-$x=on"; done
echo "SWITCHES lamp-voice LAMP_MADE_NOTES='$(envval $v LAMP_MADE_NOTES)' lamp-control LAMP_MADE_NOTES='$(envval $(pid lamp-control) LAMP_MADE_NOTES)' LAMP_MADE_CODE_ON_AT_ONCE='$(envval $v LAMP_MADE_CODE_ON_AT_ONCE)'"
echo "UNITS $(for x in lamp-voice lamp-tasks lamp-skills lamp-control lamp-openclaw-gateway lamp-cloud lamp-update; do printf '%s=%s ' $x $(systemctl is-active $x); done)"
for x in lamp-voice lamp-tasks lamp-skills lamp-control lamp-openclaw-gateway lamp-cloud; do systemctl is-active -q $x || why="$why $x-not-active"; done
if [ -z "$why" ]; then [ $given = 4 ] && echo "VERIFY PASS (all four parts = the given PR shas)" || echo "VERIFY PASS (shas printed, $given of 4 given: compare the BIN lines by eye)"
else echo "VERIFY FAIL:$why"; fi
# Ready: units up, OpenClaw among the targets, the lamp not starting.
why=""
T=$(python3 -I - <<'P'
import socket
s=socket.socket(socket.AF_UNIX); s.settimeout(5); s.connect("/run/lampos-tasks/request.sock"); s.sendall(b'{"op":"targets"}\n'); b=b""
while not b.endswith(b"\n"):
    c=s.recv(65536)
    if not c: break
    b+=c
print(b.decode().strip())
P
)
echo "$T" | grep -q '"lamp": *"starting"' && why="$why targets-starting"
echo "$T" | grep -q 'openclaw' || why="$why targets-without-openclaw"
[ -z "$why" ] && echo "READY" || echo "NOT_READY:$why"
