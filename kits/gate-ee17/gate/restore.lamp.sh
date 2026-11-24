# lamp, root sh (common first, then MODE=read|clean|rewind [TO=<seq>]). Used by restore and putback.
#  read    READ ONLY: "STATE installed=.. previous=.. refused=.. busy='..'", "UPD <sha256>", one "BIND <mountpoint>
#          <source>" per mount over /opt/lampOS (an earlier kit's binary swap), one "DROPIN <path>" per leftover row or
#          gate /run drop-in (zz-row-*, zz-gate-*), one "OTHER-DROPIN <path>" per other /run drop-in on a lamp unit.
#  clean   removes every DROPIN (each into ~/changes.txt), daemon-reload, restarts each unit that had one (lamp-update
#          too; a template unit is socket-started and only reloaded). A lamp-control restart is followed by the gateway
#          (PartOf on stable), waited for. Ends CLEAN OK | CLEAN NOT OK. The removed build is never added to a refused list.
#  rewind  only on the runner's REWIND_YES (run.sh): lamp-update stopped, state.json kept as found under ~/tgate, its
#          "installed" sequence set to TO (stable - 1; its commit, previous and refused untouched), lamp-update started.
#          Then an ordinary round to stable's release can take the lamp down to stable. Ends REWOUND OK | REWIND NOT OK.
cd /
dropins(){ ls -1 /run/systemd/system/*.d/zz-row-*.conf /run/systemd/system/*.d/zz-gate-*.conf 2>/dev/null; }
case $MODE in
read)
  echo "STATE $(stateval) busy='$(updbusy)'"
  echo "UPD $(updsha)"
  awk '$5 ~ "^/opt/lampOS/" {print "BIND " $5 " " $4}' /proc/self/mountinfo
  for f in $(dropins); do echo "DROPIN $f"; done
  for f in $(ls -1 /run/systemd/system/lamp*.d/*.conf 2>/dev/null); do dropins | grep -qxF "$f" || echo "OTHER-DROPIN $f"; done
  ;;
clean)
  ok=1; us=
  for f in $(dropins); do u=$(basename "$(dirname "$f")" .d); rm -f "$f"; rmdir "$(dirname "$f")" 2>/dev/null; n "removed leftover drop-in $f"; echo "REMOVED $f"
    case " $us " in *" $u "*) ;; *) us="$us $u";; esac; done
  systemctl daemon-reload
  for u in $us; do case $u in *@.service) echo "RELOADED $u (socket-started: the next start reads it)";;
    *) systemctl restart $u && { n "restarted $u without the drop-in"; echo "RESTARTED $u"; } || { echo "RESTART FAILED $u"; ok=0; };; esac; done
  case " $us " in *" lamp-control.service "*) sleep 10; gwlisten 120 || ok=0;; esac
  [ -z "$(dropins)" ] || { echo "LEFT $(dropins | tr '\n' ' ')"; ok=0; }
  [ $ok = 1 ] && echo "CLEAN OK" || echo "CLEAN NOT OK"
  ;;
rewind)
  [ -n "${TO:-}" ] || { echo "REWIND NOT OK: no TO"; exit 3; }
  u=$(updbusy); [ -z "$u" ] || { echo "REWIND NOT OK: a round is under way ($u)"; exit 3; }
  systemctl stop lamp-update; n "stopped lamp-update for the rewind"
  k=$R/state.json.before-rewind.$(date +%s); cp -p $UST $k; echo "KEPT $k"
  python3 -I - "$UST" "$TO" <<'P'
import json, os, sys
p, to = sys.argv[1], int(sys.argv[2]); d = json.load(open(p))
was = (d.get("installed") or {}).get("sequence")
d["installed"]["sequence"] = to
t = p + ".gate"; fd = os.open(t, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600); os.write(fd, json.dumps(d).encode()); os.fsync(fd); os.close(fd); os.replace(t, p)
print("REWOUND installed %s -> %s (previous and refused untouched)" % (was, to))
P
  n "state.json installed set to $TO (kept as found in $k)"
  systemctl start lamp-update; n "started lamp-update after the rewind"
  echo "STATE $(stateval)"
  stateval | grep -q "^installed=$TO " && echo "REWOUND OK" || echo "REWIND NOT OK"
  ;;
*) echo "no MODE '$MODE'"; exit 2;;
esac
