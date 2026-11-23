# lamp, root sh (common first). Legs L7/L8's put-back, only what the install did not: the wrapper and openclaw.json come
# back from the kit's copies only while they still carry the kit's change (an install that ran rewrote the wrapper;
# lamp-tasks rewrote the keys at its start). No service start. Prints L7BACK OK | L7BACK FAIL <why>.
cd /
why=""
if grep -q t8337 $OCW 2>/dev/null; then cp -p $R/openclaw.wrapper.before $OCW && echo "WRAPPER put back" && n "put-back: $OCW from $R" || why="$why wrapper"; else echo "WRAPPER clean (the install wrote it, or never changed)"; fi
if [ -f $R/openclaw.json.before ] && ! grep -q '"lampos"' $OCJ; then
  install -o lampos -g lampos -m 600 $R/openclaw.json.before $OCJ.t8337 && mv $OCJ.t8337 $OCJ && echo "OPENCLAW_JSON put back" && n "put-back: openclaw.json from $R" || why="$why openclaw.json"
else echo "OPENCLAW_JSON holds lampos (as lamp-tasks writes it)"; fi
grep -q t8337 $OCW 2>/dev/null && why="$why wrapper-still-the-kit's"
[ -z "$why" ] && echo "L7BACK OK" || echo "L7BACK FAIL:$why"
