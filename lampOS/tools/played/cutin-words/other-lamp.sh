# other-lamp.sh: sourced by row-cutin-words.sh for its other-lamp gate (from Seventh's kit lib, the otherplaying gate).
# SELF_LAMP=lamp-xxxx is the lamp this kit plays on; its own PLAYING lines are not "another lamp".
# otherplaying: true while another lamp plays, from Queue #3336 (comments of the last 6 h): its latest
# "PLAYING lamp-XXXX" (a comment that starts so, XXXX not SELF_LAMP) has no later "QUIET lamp-XXXX" and its window has
# not ended. The window's end: "HH:MM–HH:MM" -> the second time; "HH:MM–about +N min" -> start + N; else 30 min
# after the start (or the comment). Also true when #3336 can't be read (wait rather than play over a lamp).
# Reads every page (one JSON object per line): one page of 100 missed the latest PLAYING, 23 Oct 12:04.
# Prints the reason when it changes. Called in the runners' wait loops, before the lock.
read -r -d '' PYPLAY <<'PYEOF'
import json, os, re, sys, time, datetime as dt
ign = set(filter(None, os.environ.get("IGNORE_LAMPS", "").split(",")))  # lamps ruled not audible here
self_id = os.environ["SELF_LAMP"].replace("lamp-", "")
cs = [json.loads(l) for l in sys.stdin if l.strip()]; now = time.time(); last = {}; quiet = {}
for c in cs:
    t = dt.datetime.strptime(c["created_at"], "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=dt.timezone.utc).timestamp(); b = c.get("body") or ""
    q = re.match(r"\s*\**QUIET lamp-([0-9a-f]{4})\b", b)  # only a comment that starts so; a mention ("I'll post QUIET lamp-x") is not one
    if q: quiet[q.group(1)] = max(quiet.get(q.group(1), 0), t)
    m = re.match(r"\s*\**PLAYING lamp-([0-9a-f]{4})\b\**\s*(.*)", b)
    if m and m.group(1) != self_id and m.group(1) not in ign: last[m.group(1)] = (t, m.group(2))
for x, (t, rest) in sorted(last.items()):
    if quiet.get(x, 0) >= t: continue
    loc = dt.datetime.fromtimestamp(t); end = t + 1800
    m = re.match(r"(\d{1,2}):(\d{2})\s*[–-]\s*(?:(\d{1,2}):(\d{2})|about \+(\d+) ?min)?", rest)
    if m:
        st = loc.replace(hour=int(m.group(1)), minute=int(m.group(2)), second=0).timestamp()
        if st < t - 12 * 3600: st += 86400
        if m.group(3): e = loc.replace(hour=int(m.group(3)), minute=int(m.group(4)), second=0).timestamp(); end = e + 86400 if e < st else e
        else: end = st + 60 * int(m.group(5) or 30)
    if now < end: print("lamp-%s PLAYING until %s, no QUIET after it" % (x, time.strftime("%H:%M", time.localtime(end)))); sys.exit(0)
sys.exit(1)
PYEOF
OPLAST=
otherplaying() { local j r=0 msg
  j=$(gh api --paginate "repos/autonomous-ai/lampOS/issues/3336/comments?per_page=100&since=$(date -u -v-6H +%Y-%m-%dT%H:%M:%SZ)" -q '.[] | {created_at, body}' 2>/dev/null) \
    && msg=$(printf '%s' "$j" | SELF_LAMP=${SELF_LAMP:?} python3 -I -c "$PYPLAY") || r=$?
  [ -z "$j" ] && { msg="Queue #3336 not readable"; r=0; }
  [ $r = 0 ] || msg="no other lamp PLAYING"
  [ "$msg" != "$OPLAST" ] && echo "$(date +%H:%M) #3336: $msg" >&2; OPLAST=$msg
  return $r; }
