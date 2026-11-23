# lamp, root sh (common first). engine-default.json put back to its bytes as found at piece A's start (FOUND_B64, its
# base64, or `absent`, prepended): #8156's lamp.sh `engine` verb writes its own bytes, and its .orig copy goes with
# /tmp/sd8156. Run only once the kept engine is the found one, so no restart is needed. Prints ENGINEFILE ...
cd /
EF=/var/lib/lampos-tasks/store/engine-default.json
cur(){ [ -f $EF ] && base64 -w0 < $EF || echo absent; }
[ "$(cur)" = "$FOUND_B64" ] && { echo "ENGINEFILE AS FOUND (nothing changed)"; exit 0; }
if [ "$FOUND_B64" = absent ]; then rm -f $EF; n "engine-default.json removed (absent at piece A's start)"
else tmp=$EF.next-$$; ( umask 077 && printf %s "$FOUND_B64" | base64 -d > $tmp ) && chown lampos:lampos $tmp && chmod 600 $tmp && sync && mv -f $tmp $EF
  n "engine-default.json written back to its bytes at piece A's start"; fi
[ "$(cur)" = "$FOUND_B64" ] && echo "ENGINEFILE RESTORED to the bytes found" || echo "ENGINEFILE FAIL: $EF differs from the bytes found"
