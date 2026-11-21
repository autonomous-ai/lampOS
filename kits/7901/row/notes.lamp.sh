# lamp, root sh (row common first, then MODE=on|off). LAMP_MADE_NOTES=on in lamp-voice and lamp-control by /run drop-ins
# (gone at reboot), or off again (drop-ins removed). Restarts only a unit whose drop-in changed (1 start each).
# Prints NOTES ON OK | NOTES OFF OK | NOTES FAIL <why>.
cd /
ch=
for x in voice:$ND_VOICE control:$ND_CONTROL; do u=lamp-${x%%:*}; f=${x#*:}
  if [ "$MODE" = on ]; then
    [ "$(envval $(pid $u) LAMP_MADE_NOTES)" = on ] && [ ! -f $f ] && { echo "NOTES $u already on (the product's): no drop-in"; continue; }
    [ -f $f ] && continue
    mkdir -p ${f%/*}; printf '# #7901 row: /run, removed by notes off and unrow\n[Service]\nEnvironment=LAMP_MADE_NOTES=on\n' > $f; n "added $f"; ch="$ch $u"
  else [ -f $f ] && { rm -f $f; rmdir ${f%/*} 2>/dev/null; n "removed $f"; ch="$ch $u"; }; fi
done
[ -n "$ch" ] && { systemctl daemon-reload; for u in $ch; do systemctl restart $u; n "restarted $u (notes $MODE)"; done; sleep 20; }
ok=1; for u in lamp-voice lamp-control; do x=$(envval $(pid $u) LAMP_MADE_NOTES); echo "NOTES $u LAMP_MADE_NOTES='$x' active=$(systemctl is-active $u)"
  systemctl is-active -q $u || ok=0
  if [ "$MODE" = on ]; then [ "$x" = on ] || ok=0; else [ -f $ND_VOICE ] || [ -f $ND_CONTROL ] && ok=0; fi; done
M=$(echo $MODE | tr a-z A-Z); [ $ok = 1 ] && echo "NOTES $M OK (restarted:${ch:- none})" || echo "NOTES FAIL ($MODE)"
