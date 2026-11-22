# lamp, root sh (row common first, then SECS=<at most how long> STOPF=<a file whose existence ends it>). READ ONLY.
# Leg S, started in the background before its run is asked: the voice's one task ask said unasked, as the voice keeps
# it for its own turn (/run/lampos-voice/task-asked.json, voice skill_task_yes.rs ASKED_FILE, 0600, taken when its turn
# starts): the skill, its grants and say_back, the words said whole before the yes (#7901's head writes them). Once
# per change:   ASKED <the file, one line>
F=/run/lampos-voice/task-asked.json; last=; end=$(( $(date +%s) + ${SECS:-600} ))
while [ $(date +%s) -lt $end ]; do
  [ -n "${STOPF:-}" ] && [ -e "$STOPF" ] && { rm -f "$STOPF"; break; }
  if [ -f $F ]; then c=$(tr '\n' ' ' < $F 2>/dev/null | cut -c1-2000); [ -n "$c" ] && [ "$c" != "$last" ] && { echo "ASKED $c"; last=$c; }; fi
  sleep 0.2
done
echo "ASK-WATCH END $(date +%T)"
