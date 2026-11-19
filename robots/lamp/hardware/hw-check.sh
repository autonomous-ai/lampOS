#!/bin/bash
# hw-check.sh — read-only check of every lamp hardware part. Run on the lamp as root:
#   sshpass -p orangepi ssh orangepi@IP 'echo orangepi | sudo -S -p "" bash -s' < hw-check.sh
#   ... bash -s -- --restart-body   (start lamp-body again afterwards; only with hands off)
# It plays no sound, moves nothing, writes nothing. The servo read stops lamp-body (the bus
# has one owner); it is left stopped unless --restart-body is given.
RESTART=; [ "$1" = --restart-body ] && RESTART=1
ok(){ printf "  %-14s OK    %s\n" "$1" "$2"; }
bad(){ printf "  %-14s FAIL  %s\n" "$1" "$2"; FAILS="$FAILS $1"; }
FAILS=
echo "== $(hostname)  build $(jq -r .installed.sequence /var/lib/lampos-update/state.json 2>/dev/null)  up $(cut -d. -f1 /proc/uptime)s"

echo "-- USB"
for p in "camera 01da:5875" "speaker 0d8c:0014" "servo-board 1a86:55d3" "jieli-mic 4c4a:4155"; do
  set -- $p; l=$(lsusb | grep "$2"); [ -n "$l" ] && ok "$1" "$2" || bad "$1" "$2 missing"
done
e=$(dmesg | grep -E "error -32|not accepting address|unable to enumerate" | grep -oE "usb [0-9.-]+" | sort -u | tr '\n' ' ')
[ -n "$e" ] && echo "  enumeration errors on: $e"

echo "-- Camera"
v=$(for d in /sys/class/video4linux/video*; do grep -l "USB_Camera\|ZVCAI" $d/name 2>/dev/null; done | head -1)
if [ -n "$v" ]; then dev=/dev/$(basename $(dirname $v))
  if command -v ffmpeg >/dev/null; then
    timeout 10 ffmpeg -loglevel error -f v4l2 -i $dev -frames:v 1 -y /tmp/hwcheck.jpg 2>/tmp/hwcheck.err \
      && ok frame "$dev $(stat -c %s /tmp/hwcheck.jpg) bytes" \
      || { grep -q busy /tmp/hwcheck.err && ok frame "$dev in use by lampOS (not grabbed)" || bad frame "$(head -c 100 /tmp/hwcheck.err)"; }
  else ok node "$dev (no ffmpeg, frame not grabbed)"; fi
else bad node "no video node for the camera"; fi

echo "-- Audio"
aplay -l 2>/dev/null | grep -qi cmedia && ok speaker-card "$(amixer -c device_cmedia sget Speaker 2>&1 | grep -o '\[[0-9]*%\]' | head -1)" || bad speaker-card "C-Media not in aplay -l"
mc=$(arecord -l 2>/dev/null | grep -i -m1 -E "jieli|usb composite" | grep -oE "card [0-9]+" | cut -d' ' -f2)
if [ -n "$mc" ]; then
  peak=$(timeout 4 arecord -q -D plughw:$mc -f S16_LE -r 16000 -c 1 -d 2 -t raw 2>/dev/null | python3 -c "import sys,array;a=array.array('h',sys.stdin.buffer.read());print(max(map(abs,a)) if a else -1)")
  [ "${peak:--1}" -gt 0 ] 2>/dev/null && ok mic "card $mc, 2 s peak $peak" || ok mic "card $mc (busy or silent: peak ${peak:-n/a})"
else bad mic "Jieli capture card not found"; fi

echo "-- Inputs (at rest)"
G=$(cat /sys/kernel/debug/gpio 2>/dev/null)
for p in "button_primary gpio-99 " "reset_button gpio-100 "; do set -- $p
  s=$(echo "$G" | grep -E "$2" | grep -oE " (hi|lo) *$" | tr -d ' '); [ "$s" = hi ] && ok $1 "at rest (hi)" || bad $1 "reads '${s:-unclaimed}' (stuck pressed or not wired)"
done
echo "  head pads: $(grep -E '^HEAD_PAD_LINES=' /var/lib/lamp-test/factory.env 2>/dev/null | cut -d= -f2 || echo 'not recorded')"

echo "-- SD card"
c=/sys/block/mmcblk1/device; [ -d $c ] || c=/sys/block/mmcblk0/device
echo "  $(cat $c/name 2>/dev/null) manfid $(cat $c/manfid 2>/dev/null) oemid $(cat $c/oemid 2>/dev/null) date $(cat $c/date 2>/dev/null)  $(df -h / | awk 'NR==2{print $3"/"$2" used"}')"
dmesg | grep -qiE "mmc.*(error|timeout)" && bad sd "mmc errors in dmesg" || ok sd "no mmc errors"

echo "-- Board"
t=$(( $(cat /sys/class/thermal/thermal_zone0/temp)/1000 )); [ $t -lt 75 ] && ok cpu-temp "${t}C" || bad cpu-temp "${t}C"
w=$(iw dev wlan0 link 2>/dev/null | grep -E "SSID|signal" | tr -s ' \n\t' ' '); ok wifi "${w:-not associated (hotspot mode?)}"

echo "-- Servos"
if [ -e /dev/ttyACM0 ]; then
  was=$(systemctl is-active lamp-body); systemctl stop lamp-body 2>/dev/null
  SV=$(python3 - <<'P'
import os, select, termios, time
fd=os.open("/dev/ttyACM0",os.O_RDWR|os.O_NOCTTY); t=termios.tcgetattr(fd)
t[0]=0;t[1]=0;t[2]=termios.CS8|termios.CREAD|termios.CLOCAL;t[3]=0;t[4]=t[5]=termios.B1000000
termios.tcsetattr(fd,termios.TCSANOW,t)
def rd(i,a,n):
    p=[0xFF,0xFF,i,4,2,a,n]; p.append((~sum(p[2:]))&0xFF)
    termios.tcflush(fd,termios.TCIOFLUSH); os.write(fd,bytes(p)); b=b""; e=time.time()+0.1
    while len(b)<6+n and time.time()<e:
        if select.select([fd],[],[],0.02)[0]: b+=os.read(fd,64)
    k=b.find(b"\xff\xff")
    if k<0 or len(b)<k+6+n: return None
    return b[k+5:k+5+n]
def u16(b): return None if b is None else b[0]|(b[1]<<8)
names={1:"base_yaw",2:"shoulder",3:"elbow",4:"wrist_roll",5:"head_pitch"}
for i in range(1,6):
    pos=u16(rd(i,56,2))
    if pos is None: print(f"  servo {i} {names[i]:<10} FAIL  no answer"); continue
    o=u16(rd(i,31,2)); o=-(o&0x7ff) if o&0x800 else o
    ld=u16(rd(i,60,2)); ld=(ld&0x3ff)*(-1 if ld&0x400 else 1)
    v=rd(i,62,1)[0]/10; tc=rd(i,63,1)[0]; st=rd(i,65,1)[0]; tq=rd(i,40,1)[0]
    acc=rd(i,41,1)[0]; mx=rd(i,85,1); mx=mx[0] if mx else None
    lo,hi=u16(rd(i,9,2)),u16(rd(i,11,2))
    flag=[]
    if st: flag.append(f"status {st:#04x}")
    if tc>=55: flag.append(f"hot {tc}C")
    if not 6.0<=v<=8.5: flag.append(f"voltage {v}V")
    print(f"  servo {i} {names[i]:<10} {'FAIL ' if flag else 'OK   '} pos {pos} off {o} lim {lo}-{hi} load {ld} {v}V {tc}C torque {tq} acc {acc} reg85 {mx} {' '.join(flag)}")
P
)
  echo "$SV"; echo "$SV" | grep -q " FAIL " && FAILS="$FAILS servos"
  if [ -n "$RESTART" ] && [ "$was" = active ]; then systemctl start lamp-body; echo "  lamp-body started again"
  else echo "  lamp-body left stopped (was $was); start it with hands off: systemctl start lamp-body"; fi
else bad servo-bus "no /dev/ttyACM0"; fi

echo "-- Factory record"
echo "  status: $(cat /var/lib/lamp-test/factory-run.status 2>/dev/null || echo none)"
grep -E "^(RESULT_UNSET|INPUTS|CAMERA_4B|SPEAKER_LEVEL|BURN_IN|BURN_IN_PEAK_C|SERVO_ACCEL_CAP)=" /var/lib/lamp-test/factory.env 2>/dev/null | cut -c1-110 | sed 's/^/  /'
echo "== VERDICT: $([ -z "$FAILS" ] && echo 'all parts OK' || echo "check:$FAILS")"
