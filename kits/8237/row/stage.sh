#!/bin/bash
# Mac. The desk-hello fixture's install route with no pairing (PUSHCMD="bash row/stage.sh"): the same package the SDK's
# push sends (lampOS/sdk/python lampos.push.pack, imported from this repo's source: no pip, no pairing), staged on the
# lamp as lamp-control's door keeps a push it took (/var/lib/lampos-control/push: push.json + package.tar), then root's
# manage helper asked {"install_pushed": SHA} on its socket. From there it is the product's own path: lamp-skills'
# install_pushed_at and every check it makes, into /var/lib/lampOS/skills-dev. Only the network door is skipped: say so
# on the ROW line. remove is the helper's {"remove_pushed": {client, name}} (which also takes the owner's yes back).
#   bash stage.sh push <folder> [ignored]   -> PUSHED <the helper's answer, JSON> | PUSH REFUSED <why>  (exit 0 | 3)
#   bash stage.sh remove <name> [ignored]   -> REMOVED <name> | REMOVE REFUSED <why>
D=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd); . $D/kit.sh
SDK=${STAGE_SDK:-$(cd $D/../../.. && pwd)/lampOS/sdk/python/src}
CLIENT=${STAGE_CLIENT:-c_row8237}                      # the stand-in computer's id (the helper's c_... shape)
op=${1:-}; what=${2:-}
case "$op" in
  push)
    S=$D/logs/stage; rm -rf $S; mkdir -p $S
    m=$(python3 -I - "$SDK" "$what" "$S/package.tar" <<'PY'
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
)
    case "$m" in ERR*|"") echo "PUSH REFUSED no package packed: ${m#ERR }"; exit 3;; esac
    name=${m% *}; sum=${m#* }
    RENEW || { echo "PUSH REFUSED lock not ours"; exit 3; }
    $SSH $H 'rm -rf /tmp/t8237s; mkdir -m 0700 /tmp/t8237s' && $SCP -q $S/package.tar $H:/tmp/t8237s/package.tar ||
      { echo "PUSH REFUSED the package was not copied to the lamp"; exit 3; }
    out=$(LAMP "OP=push; NAME=$name; SUM=$sum; CLIENT=$CLIENT" $D/stage.lamp.sh 2>&1); echo "$out" > $S/stage.txt
    echo "$out" | grep -E '^(PUSHED|PUSH REFUSED) ' | tail -1 | grep . || echo "PUSH REFUSED no answer from the lamp body (row/logs/stage/stage.txt)"
    echo "$out" | grep -q '^PUSHED ' || exit 3;;
  remove)
    RENEW || { echo "REMOVE REFUSED lock not ours"; exit 3; }
    out=$(LAMP "OP=remove; NAME=$what; CLIENT=$CLIENT" $D/stage.lamp.sh 2>&1); mkdir -p $D/logs/stage; echo "$out" > $D/logs/stage/unstage.txt
    # The YES-FILE line first: run.sh keeps the last line (REMOVED or REMOVE REFUSED).
    echo "$out" | grep '^YES-FILE '; echo "$out" | grep -E '^(REMOVED|REMOVE REFUSED) ' | tail -1 | grep . || echo "REMOVE REFUSED no answer from the lamp body"
    echo "$out" | grep -q '^REMOVED ' || exit 3;;
  *) echo "stage.sh push <folder> | remove <name>"; exit 2;;
esac
