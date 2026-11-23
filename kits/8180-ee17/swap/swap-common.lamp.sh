# Lamp-side defines for swap.lamp.sh and restore.lamp.sh, cat'ed after row/common.lamp.sh. The lamp stays on its
# installed stable build; #8180's four runtime parts (lamp-tasks, lamp-control, lamp-skills, lamp-voice) are each
# swapped by a runtime read-only bind of /opt/lampOS/<part>/test-8180/<bin> over the installed file (gone on reboot),
# so every caller of /opt/lampOS/<part>/current/<bin> gets it. lamp-update is not swapped: #8180 changes its tests only.
TAG=test-8180
# part:unit:binary
PARTS="tasks:lamp-tasks:lamp-tasks control:lamp-control:lamp-control skills:lamp-skills:lamp-skills voice:lamp-voice:lamp-voice"
UNITS="lamp-voice lamp-skills lamp-control lamp-tasks"
inst(){ readlink -f /opt/lampOS/$1/current/$2; }
tdir(){ echo /opt/lampOS/$1/$TAG; }
mounted(){ grep -c " $1 " /proc/self/mountinfo; }
HD=/run/systemd/system/lamp-update.service.d/zz-row-8180-hold.conf
