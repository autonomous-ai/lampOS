# lamp, root sh (row common first, then KEEP_SKILLS=0|1). The row's put-back, run after the legs and before the runner's own put-back (kit8157's
# restore.lamp.sh without binds, units or gateway): leg E's runon and leg N's named drop-ins gone (lamp-voice restarted
# once, only if one was there), the row's pushed fixtures (grant-, slow-, fork-probe) removed by the manage helper's
# remove_pushed (its ANY_ASKER drop-in gone, task-yes.json as found), the row's new skills removed with the installed
# lamp-skills, a pre-existing skill the row changed put back from save's tar, made.json and long-make.json back to save's copies for
# the row's entries, the row's /tmp files gone. Idempotent; with no save baseline it only does the drop-in and /tmp.
# Never touches volume, lamp-cloud or a reset. Prints UNROW OK or UNROW NOT OK.
set -u; ok=1
gone=
for f in $ND_RUNON $ND_NAMED $ND_NAMENEAR; do [ -f $f ] && { rm -f $f; n "removed $f"; gone="$gone ${f##*/}"; }; done
if [ -n "$gone" ]; then rmdir ${ND_RUNON%/*} 2>/dev/null; systemctl daemon-reload
  systemctl restart lamp-voice; n "restarted lamp-voice (drop-ins gone:$gone)"; sleep 15; echo "DROPINS removed:$gone; lamp-voice restarted"; fi
v=$(pid lamp-voice)
for sw in LAMP_MAKE_MAY_RUN_ON LAMP_NAMED_AGENT_TAKES_LINE; do
  [ -z "$(envval $v $sw)" ] && echo "PASS $sw not set" || { echo "FAIL $sw still '$(envval $v $sw)'"; ok=0; }; done
[ "$(envval $v LAMP_NAME_NEAR)" != off ] && echo "PASS LAMP_NAME_NEAR back to the product's ('$(envval $v LAMP_NAME_NEAR)')" || { echo "FAIL LAMP_NAME_NEAR still off"; ok=0; }
# The row's pushed fixtures (probe.lamp.sh, legs C, H, P): each name in pushed.names taken off by the manage helper's
# {"remove_pushed": {client, name}}, the one-request ANY_ASKER drop-in gone after it, task-yes.json back as found.
for s in $(cat $PUSHED 2>/dev/null); do
  [ -d $DEV/$s ] || { echo "PUSHED $s already gone from $DEV"; continue; }
  r=$(mhelper "{\"remove_pushed\": {\"client\": \"$PCLIENT\", \"name\": \"$s\"}}")
  echo "PUSHED by the row, removed: $s (helper: $(echo "$r" | cut -c1-140))"; n "remove_pushed $s by the manage helper"
done
any_off || { echo "FAIL $AD still there"; ok=0; }
x=; for s in $(cat $PUSHED 2>/dev/null); do [ -d $DEV/$s ] && x="$x $s"; done
if [ -n "$x" ]; then echo "FAIL pushed fixtures still in $DEV:$x"; ok=0
else [ -f $PUSHED ] && { mv -f $PUSHED $PUSHED.used; n "the row's pushed fixtures gone ($PUSHED.used)"; }; echo "PASS pushed fixtures gone from $DEV, no ANY_ASKER drop-in"; fi
if [ -f $R/task-yes.before ]; then
  if [ "$(sha $YESF)" = "$(sha $R/task-yes.before)" ]; then echo "YES-FILE as found $(sha $YESF)"
  else cp -p $R/task-yes.before $YESF; n "put $YESF back as found"; echo "YES-FILE put back $(sha $YESF)"; fi
  [ "$(sha $YESF)" = "$(sha $R/task-yes.before)" ] && rm -f $R/task-yes.before || { echo "FAIL $YESF not as found"; ok=0; }
elif [ -f $R/task-yes.none ]; then
  [ -f $YESF ] && { rm -f $YESF; n "removed $YESF (none before the push)"; }; echo "YES-FILE none, as found"; rm -f $R/task-yes.none
fi
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
if out != d: json.dump(out, open(p + ".t8180", "w"))
P
  [ -f $MS/made.json.t8180 ] && { chown --reference=$MS/made.json $MS/made.json.t8180; chmod --reference=$MS/made.json $MS/made.json.t8180; mv -f $MS/made.json.t8180 $MS/made.json; n "the row's made entries out of made.json"; }
  for s in $(cat $R/changed.names 2>/dev/null); do
    [ -f $R/skills.before.tar ] || break
    rm -rf "$SK/$s"; tar -C $SK -xf $R/skills.before.tar "./$s" && { echo "SKILL put back from the save's copy: $s"; n "put back $SK/$s from $R/skills.before.tar"; } || { echo "FAIL put back $s"; ok=0; }
  done
  if [ -f $R/long-make.json.before ]; then cmp -s $R/long-make.json.before $MS/long-make.json || { cp -p $R/long-make.json.before $MS/long-make.json; n "long-make.json back to the save's copy"; echo "LONGMAKE put back"; }
  elif [ -f $R/long-make.absent.before ] && [ -f $MS/long-make.json ]; then rm -f $MS/long-make.json; n "removed the row's long-make.json"; echo "LONGMAKE removed (absent before)"; fi
  x=$(skillnames | LC_ALL=C comm -13 $R/skills.before - | tr '\n' ' '); [ -z "$x" ] && echo "PASS skills = save's list" || { echo "FAIL skills beyond the save's list: $x"; ok=0; }
elif [ "${KEEP_SKILLS:-0}" = 1 ]; then echo "NOTE KEEP_SKILLS=1: the row's skills left"
else echo "NOTE no save baseline ($R/skills.before): skills untouched"; fi
rm -rf /tmp/t8180-probe /tmp/t8180-mark* /tmp/t8180-stop*
[ -f $R/active.before ] && for u in $(cat $R/active.before); do systemctl is-active -q $u || echo "NOTE $u was active before the legs, now $(systemctl is-active $u)"; done
# The speaker's volume against save's (read only): a change is a leg's own volume call, never changed back by the kit
# (the user decides), so it is UNROW NOT OK and the release waits for a person.
if [ -f $R/volume.before ]; then vs=$(cat $R/volume.before); vn=$(volnow)
  [ "$vn" = "$vs" ] && echo "PASS volume $vn = saved" || { echo "FAIL volume moved $vs -> $vn (a leg's own volume call; not changed back: the user decides)"; ok=0; }
else echo "NOTE no saved volume ($R/volume.before): not compared"; fi
systemctl is-active -q lamp-cloud && echo "PASS lamp-cloud active" || { echo "FAIL lamp-cloud $(systemctl is-active lamp-cloud)"; ok=0; }
[ $ok = 1 ] && { for f in $R/*.before $R/*.before.tar; do [ -f $f ] && mv -f $f $f.used; done; echo "UNROW OK"; } || echo "UNROW NOT OK"
