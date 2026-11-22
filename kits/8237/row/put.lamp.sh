# BODY put
# lamp, root sh (common first, then PR_SKILLS_SHA= PR_TASKS_SHA=). #8237's lamp-skills and lamp-tasks on by a runtime
# read-only bind of /opt/lampOS/<part>/test-8237/<bin> over the installed file (gone at reboot; the installed
# stable file is never written), as the #7901 kit's optional swap. lamp-update's apply held (--apply off) while the
# binds are on. LAMP_NAME_NEAR=off for lamp-voice by a /run drop-in. 1 start each of lamp-tasks, lamp-skills,
# lamp-voice. Every change -> changes.txt. Ends PUT OK | PUT NOT OK.
TAG=test-8237; tdir(){ echo /opt/lampOS/$1/$TAG; }; mounted(){ grep -c " $1 " /proc/self/mountinfo; }
set -e
u=$(updbusy); [ -z "$u" ] || { echo "PUT VOID: lamp-update round under way ($u): wait"; exit 3; }
[ "$(engine)" = openclaw ] || { echo "PUT VOID: engine is '$(engine)', not openclaw"; exit 3; }
for p in $PUTP; do part=${p%%:*}; r=${p#*:}; bin=${r#*:}; f=$(inst $part $bin)
  [ "$(mounted "$f")" = 0 ] || { echo "PUT VOID: already a mount over $f"; exit 3; }
  case $bin in lamp-skills) w=$PR_SKILLS_SHA;; lamp-tasks) w=$PR_TASKS_SHA;; esac
  [ "$(sha /tmp/t8237/$bin)" = "$w" ] || { echo "PUT VOID: /tmp/t8237/$bin is not $w"; exit 3; }
  [ -f $R/$bin.stable ] || sha "$f" > $R/$bin.stable
  echo "STABLE $bin $(cat $R/$bin.stable) ($f)"
done
X=$(systemctl show -p ExecStart --value lamp-update | sed -n 's/.*argv\[\]=\([^;]*\);.*/\1/p' | sed 's/ *$//')
if echo "$X" | grep -q -- '--apply on'; then mkdir -p ${HD%/*}
  printf '[Service]\nExecStart=\nExecStart=%s\n' "$(echo "$X" | sed 's/--apply on/--apply off/')" > $HD; n "added $HD (lamp-update --apply off)"; fi
mkdir -p ${ND%/*}; printf '[Service]\nEnvironment=LAMP_NAME_NEAR=off\n' > $ND; n "added $ND"
for p in $PUTP; do part=${p%%:*}; bin=${p##*:}; install -d $(tdir $part); install -m 0755 -o root -g root /tmp/t8237/$bin $(tdir $part)/$bin; n "placed $(tdir $part)/$bin"; done
rm -rf /tmp/t8237; systemctl daemon-reload
[ -f $HD ] && { systemctl restart lamp-update; n "lamp-update restarted, apply off"; }
systemctl stop lamp-skills lamp-tasks; n "stopped lamp-skills lamp-tasks for the binds"
for u in lamp-skills lamp-tasks; do [ "$(pid $u)" = 0 ] || { echo "PUT NOT OK: $u still has a pid: nothing bound"; exit 4; }; done
for p in $PUTP; do part=${p%%:*}; bin=${p##*:}; mount --bind -o ro $(tdir $part)/$bin "$(inst $part $bin)"; n "bind $(tdir $part)/$bin over $(inst $part $bin)"; done
systemctl start lamp-tasks lamp-skills; systemctl restart lamp-voice; n "started lamp-tasks lamp-skills on #8237, lamp-voice with LAMP_NAME_NEAR=off"
set +e; upwait 60 lamp-tasks lamp-skills lamp-voice; sleep 10; ok=1
for p in $PUTP; do part=${p%%:*}; r=${p#*:}; unit=${r%%:*}; bin=${r#*:}; pp=$(pid $unit); s=$(sha /proc/$pp/exe); t=$(sha $(tdir $part)/$bin)
  [ -n "$s" ] && [ "$s" = "$t" ] && echo "PASS $unit runs #8237 $bin $s" || { echo "FAIL $unit not on #8237 (pid $pp, ${s:-none})"; ok=0; }; done
[ "$(envval "$(pid lamp-voice)" LAMP_NAME_NEAR)" = off ] && echo "PASS lamp-voice LAMP_NAME_NEAR=off" || { echo "FAIL lamp-voice LAMP_NAME_NEAR is '$(envval "$(pid lamp-voice)" LAMP_NAME_NEAR)'"; ok=0; }
[ $ok = 1 ] && echo "PUT OK" || echo "PUT NOT OK"
