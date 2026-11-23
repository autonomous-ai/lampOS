# Lamp-side defines for swap.lamp.sh and restore.lamp.sh, cat'ed after row/common.lamp.sh. lamp-voice alone is
# swapped by a runtime read-only bind of /opt/lampOS/voice/test-6449/lamp-voice over the installed file (gone on
# reboot), so every caller of /opt/lampOS/voice/current/lamp-voice gets it.
TAG=test-6449
UNITS="lamp-voice"
inst(){ readlink -f /opt/lampOS/$1/current/$2; }
tdir(){ echo /opt/lampOS/$1/$TAG; }
mounted(){ grep -c " $1 " /proc/self/mountinfo; }
HD=/run/systemd/system/lamp-update.service.d/zz-row-6449-hold.conf
