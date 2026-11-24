# lamp, root sh (common first, then MODE=on|measure|off|check [LABEL=idle|task SECS=60]). #8417's read of lamp-peek
# in monitor mode (docs/peek.md, "Monitor mode"), from the lamp itself (127.0.0.1:80, a loopback peer).
#  on       records once whether /etc/systemd/system/lamp-peek.service.d/monitor.conf was there (its bytes kept) and
#           whether lamp-peek was active, writes the drop-in ([Service] Environment=LAMP_PEEK=monitor), restarts
#           lamp-peek (1 start), asks /run/lampos-peek/admin.sock for a one-time code (each connection gets one code
#           and a newline), POSTs /in code=<code> and keeps the peek_session cookie in ~/tgate (0600). The code and
#           the cookie are never printed. Ends PEEK ON OK | PEEK ON NOT OK.
#  measure  one minute: `timeout SECS curl -sN -b peek_session=... http://127.0.0.1/events | wc -c`, and lamp-peek's CPU
#           over the same SECS (pidstat -p PID 1 SECS when there, and /proc/PID/stat utime+stime ticks always).
#           Prints MEASURE <label> bytes= ticks0= ticks1= clk= secs= pid= pid_after= pidstat=.
#  off      puts the drop-in back exactly as recorded (removed, or its old bytes) and lamp-peek's state (restarted if
#           it was active, else stopped). Ends PEEK BACK OK | PEEK BACK NOT OK.
#  check    READ ONLY: PEEK BACK OK when nothing laid by the gate is left.
cd /
PD=/etc/systemd/system/lamp-peek.service.d; PF=$PD/monitor.conf; ASOCK=/run/lampos-peek/admin.sock
B=$R/peek.before; BA=$R/peek.active.before; BC=$R/peek.monitor.conf.before; CK=$R/peek.cookie
back_ok(){ b=$(cat $B 2>/dev/null) || return 0
  if [ "$b" = present ]; then cmp -s $BC $PF || return 1; else [ ! -e $PF ] || return 1; fi
  a=$(systemctl is-active lamp-peek); if [ "$(cat $BA)" = active ]; then [ "$a" = active ]; else [ "$a" != active ]; fi; }
case $MODE in
on)
  if [ ! -f $B ]; then
    if [ -f $PF ]; then cp -p $PF $BC; echo present > $B; else echo absent > $B; fi
    systemctl is-active lamp-peek > $BA 2>/dev/null; n "lamp-peek before the gate's read: drop-in $(cat $B), $(cat $BA)"
  fi
  mkdir -p $PD; printf '[Service]\nEnvironment=LAMP_PEEK=monitor\n' > $PF; n "wrote $PF (LAMP_PEEK=monitor) for #8417's read"
  systemctl daemon-reload; systemctl restart lamp-peek; n "restarted lamp-peek in monitor mode"
  i=0; while [ $i -lt 20 ] && [ ! -S $ASOCK ]; do sleep 1; i=$((i+1)); done
  [ -S $ASOCK ] || { echo "PEEK ON NOT OK: no $ASOCK after 20 s ($(systemctl is-active lamp-peek))"; exit 3; }
  cd=$(python3 -I -c 'import socket,sys
s=socket.socket(socket.AF_UNIX); s.settimeout(5); s.connect(sys.argv[1]); b=b""
while not b.endswith(b"\n"):
    c=s.recv(256)
    if not c: break
    b+=c
print(b.decode().strip())' $ASOCK 2>/dev/null)
  echo "$cd" | grep -qE '^[0-9a-f]{64}$' || { echo "PEEK ON NOT OK: admin.sock gave no 64-hex code (${#cd} chars)"; exit 3; }
  rm -f $R/peek.jar; hc=$(curl -s -o /dev/null -w '%{http_code}' -c $R/peek.jar --data "code=$cd" http://127.0.0.1/in); cd=
  c=$(awk '$6 == "peek_session" {print $7}' $R/peek.jar 2>/dev/null | tail -1); rm -f $R/peek.jar
  [ -n "$c" ] || { echo "PEEK ON NOT OK: POST /in http=$hc, no peek_session cookie"; exit 3; }
  ( umask 077; echo "$c" > $CK ); c=
  echo "PEEK ON OK monitor (drop-in was $(cat $B), lamp-peek was $(cat $BA)); POST /in http=$hc, cookie kept (not printed)"
  ;;
measure)
  [ -s $CK ] || { echo "MEASURE $LABEL NOT OK: no session (MODE=on first)"; exit 3; }
  p=$(pid lamp-peek); [ "${p:-0}" != 0 ] || { echo "MEASURE $LABEL NOT OK: lamp-peek not running"; exit 3; }
  ps_out=$R/pidstat.$LABEL; rm -f $ps_out
  command -v pidstat >/dev/null && { pidstat -p $p 1 $SECS > $ps_out 2>&1 & }
  t0=$(awk '{print $14+$15}' /proc/$p/stat)
  b=$(timeout $SECS curl -sN -b "peek_session=$(cat $CK)" http://127.0.0.1/events | wc -c)
  t1=$(awk '{print $14+$15}' /proc/$p/stat 2>/dev/null); wait
  pc=$(awk '/%CPU/ && !h {for (i=1;i<=NF;i++) if ($i=="%CPU") h=i} /^Average/ && h {print $h}' $ps_out 2>/dev/null | tail -1)
  echo "MEASURE $LABEL bytes=$b ticks0=$t0 ticks1=${t1:-none} clk=$(getconf CLK_TCK) secs=$SECS pid=$p pid_after=$(pid lamp-peek) pidstat=${pc:-none}"
  ;;
off)
  [ -f $B ] || { echo "PEEK BACK OK (nothing laid by the gate)"; exit 0; }
  if [ "$(cat $B)" = present ]; then cp -p $BC $PF; n "put $PF back as it was"; else rm -f $PF; rmdir $PD 2>/dev/null; n "removed $PF (it was not there)"; fi
  systemctl daemon-reload
  if [ "$(cat $BA)" = active ]; then systemctl restart lamp-peek; n "restarted lamp-peek as it was"; else systemctl stop lamp-peek; systemctl reset-failed lamp-peek 2>/dev/null; n "stopped lamp-peek ($(cat $BA) before)"; fi
  rm -f $CK
  if back_ok; then echo "PEEK BACK OK (drop-in $(cat $B), lamp-peek $(systemctl is-active lamp-peek), was $(cat $BA))"; rm -f $B $BA $BC
  else echo "PEEK BACK NOT OK (drop-in $( [ -e $PF ] && echo present || echo absent ), lamp-peek $(systemctl is-active lamp-peek); before: $(cat $B), $(cat $BA))"; fi
  ;;
check)
  if [ ! -f $B ]; then echo "PEEK BACK OK (nothing laid by the gate)"; else echo "PEEK BACK NOT OK: the gate's monitor drop-in is still recorded as laid ($(cat $B) before)"; fi
  ;;
*) echo "no MODE '$MODE'"; exit 2;;
esac
