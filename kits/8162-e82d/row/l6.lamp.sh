# lamp, root sh (common first; then LEG=). Leg L6, READ ONLY. The app's `engines.install` reaches lamp-cloud only over
# the backend's socket (cloud answer.rs "engines.install", sealed to the owner's account), so the Mac cannot send it.
# Read here: tasks/agent (lamp-cloud adds it to update.status as `agent`, update.rs add_agent) and lamp-cloud's journal.
cd /
agent_now
journalctl -u lamp-cloud --since "-30min" --no-pager -o short-unix 2>/dev/null | grep -iE "engines\.install|installed and running|install" | tail -6 | cut -c1-220 | sed 's/^/J /'
echo "L6 READ done"
