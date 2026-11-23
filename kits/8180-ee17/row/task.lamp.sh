# lamp, root sh (common.lamp.sh first, then SKILL=<name> MARK=<file on the lamp, touched at the ask>). READ ONLY.
# Leg D: the newest lamp-tasks task's prompt (tasks.sqlite3 read-only, kit7819's tq2 way): does it name
# <folder>/SKILL.md first and say never to read the lamp's own files? Then every OpenClaw/Hermes session file
# written since MARK: which lamp paths its tool calls name (paths only; no mail text is printed).
python3 -I - "$SKILL" "$R/prompt.$SKILL.txt" <<'P'
import json, sqlite3, sys
skill = sys.argv[1]; open(sys.argv[2], "w").close(); folder = "/var/lib/lampOS/skills/" + skill
c = sqlite3.connect("file:/var/lib/lampos-tasks/store/tasks.sqlite3?mode=ro", uri=True)
d = json.loads(c.execute("select snapshot from state").fetchone()[0])
ts = [t for t in d.get("tasks", []) if skill in json.dumps(t.get("launch") or {})]
if not ts: print("PROMPT none for %s" % skill); sys.exit(0)
t = ts[-1]; p = str((t.get("launch") or {}).get("prompt") or ""); open(sys.argv[2], "w").write(p)
print("PROMPT task=%s card=%s first=%s never_lamp_files=%s state=%s" % (t.get("id"),
    "yes" if folder + "/SKILL.md" in p else "no",
    "yes" if "SKILL.md" in p and (p.find("SKILL.md") < p.find("python3") or "python3" not in p) else "no",
    "yes" if "never read the lamp's own files" in p else "no", (t.get("dispatch") or {}).get("state")))
print("PROMPTTEXT " + p.replace("\n", " ")[:600])
P
[ -f "$MARK" ] || { echo "SESSIONS no mark $MARK"; exit 0; }
F=$(find /var/lib /home /root -xdev -maxdepth 9 -name '*.jsonl' -path '*session*' -newer "$MARK" 2>/dev/null | head -20)
[ -n "$F" ] || { echo "SESSIONS none written since the ask (look by hand: find / -name '*.jsonl' -newer $MARK)"; exit 0; }
for f in $F; do
  # lamp paths named in the session, less those the task's own prompt names (the SDK path, the skill's folder)
  own=$(grep -oE '/(opt/lampOS|etc/lampOS|var/lib/lampos[a-z-]*|var/lib/lampOS/(voice|update)|run/lampos[a-z-]*|home/'"$LAMP_USER"')[^" \\]*' "$f" | sort -u | while read -r x; do grep -qF -- "$x" $R/prompt.$SKILL.txt 2>/dev/null || echo "$x"; done)
  card=$(grep -c "/var/lib/lampOS/skills/$SKILL/SKILL.md" "$f")
  echo "SESSION $f card_named=$card lamp_paths=$(echo "$own" | grep -c .)"
  echo "$own" | head -15 | sed 's/^/  LAMPPATH /'
done
