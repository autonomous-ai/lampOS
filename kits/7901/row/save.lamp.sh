# lamp, root sh (row common first). Before the legs, on the runner's install of #7901: the row's baselines (made index,
# skills list + tar, control's notes files, active units), so unrow removes only what the legs made. No unit start.
# Prints SAVED OK or SAVE STOP <why>. Run once per ROW: a second run would take the row's skills as the base.
set -u
[ -f $R/skills.before ] && { echo "SAVE STOP: $R/skills.before exists (an earlier save not used by unrow): run unrow first, or move it"; exit 3; }
u=$(updbusy); [ -z "$u" ] || { echo "SAVE STOP: lamp-update round under way ($u): wait"; exit 3; }
echo "ENGINE $(engine)"; [ "$(engine)" = openclaw ] || { echo "SAVE STOP: the default agent is '$(engine)', not openclaw"; exit 3; }
v=$(pid lamp-voice); for e in $NEED; do [ "$(envval $v $e)" = on ] || { echo "SAVE STOP: lamp-voice has no $e=on (the product's)"; exit 3; }; done
[ "$(envval $v LAMP_MADE_NOTES)" = on ] || { echo "SAVE STOP: lamp-voice has no LAMP_MADE_NOTES=on (../run.sh notes)"; exit 3; }
echo "BASE build $(build)"
mkdir -p $MS; [ -f $MS/made.json ] && cp -p $MS/made.json $R/made.json.before || echo '[]' > $R/made.json.before
skillnames > $R/skills.before; tar -C $SK -cf $R/skills.before.tar . 2>/dev/null || true
notesfiles > $R/notes.before
systemctl list-units --state=active --plain --no-legend 'lamp-*' | awk '{print $1}' > $R/active.before
echo "BASELINE made: $(madenames | tr '\n' ' ')/ skills $(wc -l < $R/skills.before) / notes files $(wc -l < $R/notes.before)"
cl=$(skillnames | grep -iE '(coffee|price|desk|camera|look)' | tr '\n' ' ')
[ -n "$cl" ] && echo "CLASH names a make may meet: $cl(a make may ask a clash question; a leftover in the baseline is not removed by unrow)" || echo "CLASH none"
n "row baselines saved in $R (made.json, skills list + tar, notes files)"
# LAMP_NAME_NEAR=off for the row (ND_NAMENEAR): 1 lamp-voice start; unrow removes it
mkdir -p ${ND_NAMENEAR%/*}; printf '[Service]\n# #7901 row: Hey Lamp in the Mac voice, gone at reboot\nEnvironment=LAMP_NAME_NEAR=off\n' > $ND_NAMENEAR
systemctl daemon-reload; systemctl restart lamp-voice; n "added $ND_NAMENEAR (LAMP_NAME_NEAR=off); lamp-voice restarted"; sleep 15
nn=$(envval $(pid lamp-voice) LAMP_NAME_NEAR); [ "$nn" = off ] || { echo "SAVE STOP: LAMP_NAME_NEAR is '$nn' after the drop-in"; exit 3; }
echo "NAMENEAR off (row drop-in)"
echo "SAVED OK"
