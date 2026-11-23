#!/bin/bash
# Mac. A row fixture (row/probe/<name>: lemon-probe, slow-probe, fork-probe; legs C, H, P) by the product's push
# install, with no pairing: the #8237 kit's row/stage.sh ported (thirteen/kit-8237 5b5e8e4291). The same package the
# SDK's push sends (lampOS/sdk/python lampos.push.pack, imported from this repo's source: no pip, no pairing), staged
# on the lamp as lamp-control's door keeps a push it took, then root's manage helper asked {"install_pushed": SHA} on
# its socket (probe.lamp.sh). From there it is the product's own path: lamp-skills' install_pushed_at and every check
# it makes, into /var/lib/lampOS/skills-dev. Only the network door (the Mac's pairing) is skipped. remove is the
# helper's {"remove_pushed": {client, name}}; unrow does it for every fixture the row pushed.
#   bash push.sh push <folder>   -> probe.lamp.sh's lines (PUSHED ... + PROBE INSTALLED | PUSH REFUSED ... + PROBE NOT INSTALLED)
#   bash push.sh remove <name>   -> REMOVED <name> | REMOVE REFUSED <why>
#   bash push.sh pack <folder> <out.tar>  -> "<name> <sha256>" (the Mac's packing alone, no lamp)
D=${D:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}; . $D/kit.sh
SDK=${PUSH_SDK:-$(git -C "$D" rev-parse --show-toplevel 2>/dev/null)/lampOS/sdk/python/src}
op=${1:-}; what=${2:-}
pack(){ python3 -I - "$SDK" "$1" "$2" <<'PY'
import sys, os
# lampos's push and errors modules only: the package's __init__ (its client, websockets) is never run.
import importlib.util as u; d = os.path.join(sys.argv[1], "lampos")
pkg = u.module_from_spec(u.spec_from_file_location("lampos", os.path.join(d, "__init__.py"), submodule_search_locations=[d])); sys.modules["lampos"] = pkg
try:
    from lampos.push import pack
    p = pack(sys.argv[2]); open(sys.argv[3], "wb").write(p.data); print(p.name, p.sha256)
except Exception as error:
    print("ERR %s: %s" % (type(error).__name__, error))
PY
}
case "$op" in
  pack) pack "$what" "$3";;
  push)
    S=$D/logs/push-$(basename "$what"); rm -rf $S; mkdir -p $S
    # A copy of the fixture with this run's mark in SKILL.md: each run is another folder SHA-256, which the lamp asks
    # aloud again (skills task.rs ask_refuses: an ask is said once a day per skill folder). No lamp state is touched.
    F=$S/fixture/$(basename "$what"); mkdir -p $S/fixture && cp -R "$what" $F && printf '\n<!-- row run %s -->\n' "${ROW_NONCE:-$(date +%s)}" >> $F/SKILL.md
    m=$(pack $F $S/package.tar)
    case "$m" in ERR*|"") echo "PUSH REFUSED no package packed: ${m#ERR }"; echo "PROBE NOT INSTALLED"; exit 3;; esac
    name=${m% *}; sum=${m#* }
    RENEW || { echo "PUSH REFUSED lock not ours"; echo "PROBE NOT INSTALLED"; exit 3; }
    SSHR -n $H 'rm -rf /tmp/t8180-probe; mkdir -m 0700 /tmp/t8180-probe' && $SCP -q $S/package.tar $H:/tmp/t8180-probe/package.tar ||
      { echo "PUSH REFUSED the package was not copied to the lamp"; echo "PROBE NOT INSTALLED"; exit 3; }
    LAMP "OP=push SUM=$sum NAME=$name" $D/probe.lamp.sh 2>&1 | tee $S/push.txt
    grep -q '^PROBE INSTALLED' $S/push.txt || exit 3;;
  remove)
    RENEW || { echo "REMOVE REFUSED lock not ours"; exit 3; }
    out=$(LAMP "OP=remove NAME=$what" $D/probe.lamp.sh 2>&1); echo "$out"
    echo "$out" | grep -q '^REMOVED ' || exit 3;;
  *) echo "push.sh push <folder> | remove <name> | pack <folder> <out.tar>"; exit 2;;
esac
