#!/bin/zsh
# Mac (split ROW kit: logs under $LOGROOT, lock on $LAMP). lock.zsh take | release | status. take: lamp-lock try (exit 1 at once if held), then a background
# renewer every 4 min (pid in logs/renew.pid) that stops itself if the lock is no longer ours.
set -u; D=${0:a:h}; source $D/mac.zsh   # needs LAMP_SSH and LL in env
case ${1:-} in
take) $LL $LOCK_LAMP try $LOCK_WHO $LOCK_SESSION "$LOCK_FOR" || { echo "LOCK HELD by another (or the lamp not reachable): stop"; exit 1; }
      ( while sleep 240; do $LL $LOCK_LAMP renew $LOCK_WHO $LOCK_SESSION >/dev/null 2>&1 && echo "$(date +%T) renewed" >> $LOGROOT/renew.log || { echo "$(date +%T) RENEW FAILED: stop the row" >> $LOGROOT/renew.log; break; }; done ) >/dev/null 2>&1 &!
      echo $! > $LOGROOT/renew.pid; echo "LOCK TAKEN; renewer pid $(cat $LOGROOT/renew.pid)";;
release) [ -f $LOGROOT/renew.pid ] && { kill $(cat $LOGROOT/renew.pid) 2>/dev/null; rm -f $LOGROOT/renew.pid; }
      $LL $LOCK_LAMP release $LOCK_WHO $LOCK_SESSION && echo "LOCK RELEASED";;
status) $LL $LOCK_LAMP status;;
*) echo "lock.zsh take|release|status"; exit 2;;
esac
