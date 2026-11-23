# lamp, root sh (common first, then PR_VOICE_SHA=<sha of #6449's head's lamp-voice>). READ ONLY. The build the row
# rests on: the installed stable build, lamp-voice running #6449's head's binary (bound over by `swap`), no update
# round under way, and the product's LAMP_ACTION_CLAIM_CHECKED=on. Prints VERIFY PASS|FAIL, then READY|NOT READY.
ok=1
echo "BUILD $(build)"; u=$(updbusy); [ -z "$u" ] || { echo "FAIL a lamp-update round is under way ($u)"; ok=0; }
p=$(pid lamp-voice); s=$(sha256sum /proc/$p/exe 2>/dev/null | cut -d' ' -f1)
if [ -n "${PR_VOICE_SHA:-}" ]; then
  [ "$s" = "$PR_VOICE_SHA" ] && echo "PASS lamp-voice runs #6449's head's binary $s" || { echo "FAIL lamp-voice runs ${s:-none}, not $PR_VOICE_SHA"; ok=0; }
else echo "NOTE lamp-voice runs ${s:-none} (no PR_VOICE_SHA given: not checked)"; ok=0; fi
c=$(envval $p LAMP_ACTION_CLAIM_CHECKED); [ "$c" = on ] && echo "PASS LAMP_ACTION_CLAIM_CHECKED=on (the product's)" || { echo "FAIL LAMP_ACTION_CLAIM_CHECKED='$c'"; ok=0; }
w=$(envval $p LAMP_ACTION_CLAIM_WORDED); echo "NOW LAMP_ACTION_CLAIM_WORDED='${w}' (the row's drop-in turns it on at \`on\`)"
[ $ok = 1 ] && echo "VERIFY PASS" || echo "VERIFY FAIL"
a=$(systemctl is-active lamp-voice); [ "$a" = active ] && [ "$p" != 0 ] && echo "READY lamp-voice active pid $p" || echo "NOT READY lamp-voice $a"
