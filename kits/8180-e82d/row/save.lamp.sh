# lamp, root sh (row common first). Before the legs, on the runner's PR install (kit8157's swap.lamp.sh, its save part
# only: no binds, no unit start). Saves the made index, long-make marks and the skills (list + whole tar, so a clash the
# row changes is put back), lists leftovers a make may clash with (days / year / week / calendar / bitcoin / price / mail /
# grant-probe), and clears the test ledgers (backed up first; an absent one is left absent, kit7819b's fix).
# Prints SAVED OK or SAVE STOP <why>. Idempotent only before the legs: a second run would take the row's skills as base.
set -u
[ -f $R/skills.before ] && { echo "SAVE STOP: $R/skills.before exists (an earlier save not used by unrow): run unrow first, or move it"; exit 3; }
u=$(updbusy); [ -z "$u" ] || { echo "SAVE STOP: lamp-update round under way ($u): wait"; exit 3; }
echo "ENGINE $(engine)"; [ -n "$(engine)" ] || { echo "SAVE STOP: no engine-default"; exit 3; }
v=$(pid lamp-voice); for e in $NEED; do [ "$(envval $v $e)" = on ] || { echo "SAVE STOP: lamp-voice has no $e=on (the product's)"; exit 3; }; done
[ -z "$(envval $v LAMP_MAKE_MAY_RUN_ON)" ] || { echo "SAVE STOP: LAMP_MAKE_MAY_RUN_ON already '$(envval $v LAMP_MAKE_MAY_RUN_ON)'"; exit 3; }
echo "BASE build $(build)"
mkdir -p $MS; [ -f $MS/made.json ] && cp -p $MS/made.json $R/made.json.before || echo '[]' > $R/made.json.before
rm -f $R/long-make.json.before $R/long-make.absent.before
[ -f $MS/long-make.json ] && cp -p $MS/long-make.json $R/long-make.json.before || : > $R/long-make.absent.before
skillnames > $R/skills.before; tar -C $SK -cf $R/skills.before.tar . 2>/dev/null || true
systemctl list-units --state=active --plain --no-legend 'lamp-*' | awk '{print $1}' > $R/active.before
echo "BASELINE made: $(madenames | tr '\n' ' ')/ skills $(wc -l < $R/skills.before)"
cl=$(skillnames | grep -iE '^(.*days.*|.*year.*|week|.*calendar.*|.*bitcoin.*|.*price.*|.*mail.*|grant-probe)$' | tr '\n' ' ')
[ -n "$cl" ] && echo "CLASH names a row may meet: $cl(a make may ask a clash question; the tar above puts a changed one back, a leftover not in the baseline is not ours to remove)" || echo "CLASH none"
for f in task-asks.json task-runs.json; do [ -f $SKST/$f ] || { n "no $SKST/$f: left absent"; echo "LEDGER $f absent: left"; continue; }
  cp -p $SKST/$f $SKST/$f.bak-row-8180-$(date +%H%M%S); printf '[]' > $SKST/$f.t && chown --reference=$SKST/$f $SKST/$f.t && chmod --reference=$SKST/$f $SKST/$f.t && mv -f $SKST/$f.t $SKST/$f; n "backed up and cleared $SKST/$f"; echo "LEDGER $f cleared"; done
n "row baselines saved in $R (made.json, long-make, skills tar)"
echo "SAVED OK"
