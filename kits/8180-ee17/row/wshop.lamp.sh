# lamp, root sh (row common first, then MODE=hash|config). READ ONLY. Leg W (#8130, folded into #8180's OpenClaw leg):
#   hash:   WSHOP <sha256 of every file's path+sha under workshop-skills/> files=<n>   (missing folder: files=0)
#   config: CHECK (W1) tools.deny holds skill_workshop, CHECK (W2) the gateway accepts the file (active, no exit 78
#           since its start), with the deny list and the gateway's start time.
set -u
OCJ=/var/lib/lampos-openclaw/state/openclaw.json
WS=/var/lib/lampos-openclaw/state/agents/main/agent/workshop-skills
case "${MODE:-hash}" in
hash) if [ -d $WS ]; then n=$(find $WS -type f | wc -l | tr -d ' '); h=$(cd $WS && find . -type f | LC_ALL=C sort | xargs -r sha256sum | sha256sum | cut -c1-16)
        echo "WSHOP $h files=$n"; find $WS -maxdepth 2 | LC_ALL=C sort | head -20 | sed 's/^/WSHOPLS /'
      else echo "WSHOP none files=0 (no $WS)"; fi;;
config)
  python3 -I - $OCJ <<'P'
import json, sys
try: d = json.load(open(sys.argv[1]))
except Exception as e: print("CHECK (W1) tools.deny holds skill_workshop: FAIL (openclaw.json unreadable: %s)" % type(e).__name__); sys.exit()
deny = ((d.get("tools") or {}).get("deny")) or []
print("DENY %s" % json.dumps(deny))
print("CHECK (W1) tools.deny holds skill_workshop: %s" % ("PASS" if "skill_workshop" in deny else "FAIL"))
P
  since=$(systemctl show -p ActiveEnterTimestamp --value lamp-openclaw-gateway)
  st=$(systemctl is-active lamp-openclaw-gateway)
  x=$(journalctl -u lamp-openclaw-gateway --since "$since" --no-pager -o cat 2>/dev/null | grep -cE 'status=78|exit code 78|code=exited, status=78')
  r=$(systemctl show -p NRestarts --value lamp-openclaw-gateway)
  x90=$(journalctl -u lamp-openclaw-gateway --since -90min --no-pager -o cat 2>/dev/null | grep -cE 'status=78|exit code 78')
  [ "$x90" = 0 ] || { echo "NOTE exit-78 lines in the last 90 min (before this start too): $x90"; }
  echo "GATEWAY $st since '$since' restarts=$r exit78_lines=$x"
  [ "$st" = active ] && [ "$x" = 0 ] && echo "CHECK (W2) OpenClaw accepts the file (gateway active, no exit 78 since start): PASS" || echo "CHECK (W2) OpenClaw accepts the file: FAIL (gateway $st, exit-78 lines $x)";;
esac
