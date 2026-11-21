# lamp, root sh (row common first, then KEEP_SKILLS=0|1). The row's put-back, run after the legs and BEFORE the runner's
# round back to stable: the notes drop-ins gone (lamp-voice and lamp-control restarted once, only if one was there),
# the row's new skills removed by the installed lamp-skills, made.json back to save's entries (a changed one put back
# from save's tar), the legs' new notes files removed, the row's /tmp files gone. Idempotent; with no save baseline it
# only does the drop-ins and /tmp. Never touches volume, lamp-cloud or a reset. Prints UNROW OK or UNROW NOT OK.
set -u; ok=1
ch=
for x in voice:$ND_VOICE control:$ND_CONTROL; do f=${x#*:}; [ -f $f ] && { rm -f $f; rmdir ${f%/*} 2>/dev/null; n "removed $f"; ch="$ch lamp-${x%%:*}"; }; done
[ -n "$ch" ] && { systemctl daemon-reload; for u in $ch; do systemctl restart $u; n "restarted $u (notes drop-in gone)"; done; sleep 15; echo "DROPINS removed; restarted:$ch"; }
for f in $ND_VOICE $ND_CONTROL; do [ -f $f ] && { echo "FAIL $f still there"; ok=0; }; done
if [ "${KEEP_SKILLS:-0}" != 1 ] && [ -f $R/skills.before ]; then
  for s in $(skillnames | LC_ALL=C comm -13 $R/skills.before -); do
    out=$($SKBIN remove --dir $SK --state-dir $SKST "$s" 2>&1); rc=$?
    echo "SKILL made by the row, removed: $s (exit $rc: $(echo "$out" | tail -1 | cut -c1-140))"; n "lamp-skills remove $s (exit $rc)"
    [ -e "$SK/$s" ] && { echo "FAIL $SK/$s still there"; ok=0; }
  done
  python3 -I - $MS/made.json $R/made.json.before $R/changed.names <<'P'
import json, os, sys
p, before, changed = sys.argv[1:4]
open(changed, "w").close()
try: d = json.load(open(p))
except FileNotFoundError: sys.exit(0)
b = json.load(open(before)); bm = {e.get("name"): e for e in b}
out, names = [], []
for e in d:
    n = e.get("name")
    if n not in bm:
        print("MADE by the row, dropped: %s (package %s)" % (n, e.get("package")))
        pk = os.path.join(os.path.dirname(p), os.path.basename(e.get("package") or ""))
        if e.get("package") and os.path.isfile(pk): os.remove(pk)
        continue
    if json.dumps(e, sort_keys=True) != json.dumps(bm[n], sort_keys=True):
        print("MADE changed by the row, put back: %s" % n); names.append(n); out.append(bm[n]); continue
    out.append(e)
open(changed, "w").write("\n".join(names))
if out != d: json.dump(out, open(p + ".t7901", "w"))
P
  [ -f $MS/made.json.t7901 ] && { chown --reference=$MS/made.json $MS/made.json.t7901; chmod --reference=$MS/made.json $MS/made.json.t7901; mv -f $MS/made.json.t7901 $MS/made.json; n "the row's made entries out of made.json"; }
  for s in $(cat $R/changed.names 2>/dev/null); do
    [ -f $R/skills.before.tar ] || break
    rm -rf "$SK/$s"; tar -C $SK -xf $R/skills.before.tar "./$s" && { echo "SKILL put back from the save's copy: $s"; n "put back $SK/$s from $R/skills.before.tar"; } || { echo "FAIL put back $s"; ok=0; }
  done
  [ -f $R/notes.before ] && for x in $(notesfiles | LC_ALL=C comm -13 $R/notes.before - | grep '^n_'); do rm -f $NOTES/$x; n "removed $NOTES/$x (a row skill's notes)"; echo "NOTES removed $x"; done
  x=$(skillnames | LC_ALL=C comm -13 $R/skills.before - | tr '\n' ' '); [ -z "$x" ] && echo "PASS skills = save's list" || { echo "FAIL skills beyond the save's list: $x"; ok=0; }
elif [ "${KEEP_SKILLS:-0}" = 1 ]; then echo "NOTE KEEP_SKILLS=1: the row's skills left"
else echo "NOTE no save baseline ($R/skills.before): skills untouched"; fi
rm -f /tmp/t7901-stop.* $R/egress.ls.*
[ -f $R/active.before ] && for u in $(cat $R/active.before); do systemctl is-active -q $u || echo "NOTE $u was active before the legs, now $(systemctl is-active $u)"; done
systemctl is-active -q lamp-cloud && echo "PASS lamp-cloud active" || { echo "FAIL lamp-cloud $(systemctl is-active lamp-cloud)"; ok=0; }
[ $ok = 1 ] && { for f in $R/*.before $R/*.before.tar; do [ -f $f ] && mv -f $f $f.used; done; echo "UNROW OK"; } || echo "UNROW NOT OK"
