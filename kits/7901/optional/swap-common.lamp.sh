# OPTIONAL PATH ONLY (not the ROW's way in: the ROW's build gets on by the runner's real round). Lamp-side defines for
# swap.lamp.sh and restore.lamp.sh, cat'ed after row/common.lamp.sh. Four parts swapped by a runtime read-only bind
# of /opt/lampOS/<part>/test-7901/<bin> over the installed file (gone on reboot), so every caller of
# /opt/lampOS/<part>/current/<bin> gets it: the units, lamp-skills-manage@, the agent's make check and the fence's
# net relay.
TAG=test-7901
UNITS="lamp-voice lamp-skills lamp-control lamp-tasks"
inst(){ readlink -f /opt/lampOS/$1/current/$2; }
tdir(){ echo /opt/lampOS/$1/$TAG; }
mounted(){ grep -c " $1 " /proc/self/mountinfo; }
HD=/run/systemd/system/lamp-update.service.d/zz-row-7901-hold.conf
