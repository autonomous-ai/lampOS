# lamp, root sh (common.lamp.sh first, then NAME=<fixture>, default grant-probe). Legs C, H, P: installs the fixture
# (copied to /tmp/t8180-probe/<NAME>) with the installed (PR build) lamp-skills, as root. It is new against
# save.lamp.sh's baseline, so unrow removes it.
set -u
NAME=${NAME:-grant-probe}
[ -e $SK/$NAME ] && { echo "PROBE already installed ($NAME)"; echo "PROBE INSTALLED"; exit 0; }
out=$($SKBIN install --dir $SK --state-dir $SKST /tmp/t8180-probe/$NAME 2>&1); rc=$?
echo "PROBE install $NAME exit $rc: $(echo "$out" | tail -2 | tr '\n' ' ' | cut -c1-240)"; n "lamp-skills install $NAME (exit $rc)"
rm -rf /tmp/t8180-probe/$NAME; sleep 3
[ -d $SK/$NAME ] && echo "PROBE INSTALLED" || echo "PROBE NOT INSTALLED"
