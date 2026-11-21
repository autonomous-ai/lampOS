# lamp, root sh (row common first, then STAGE=<label>). READ ONLY. What is new since save's baselines:
#   STAGE <label>
#   NEW <name> on=<True|False|?> grants=<a,b,...> reads_notes=<yes|no> sets_notes=<yes|no> notes=<file|none> keys=<n> sha=<16>
#   NOTESJSON <name> <the notes file, at most 800 chars>   (coffee prices, nothing private)
echo "STAGE ${STAGE:-?}"
python3 -I - "$MS" "$SK" "$R/made.json.before" "$R/skills.before" "$NOTES" <<'P'
import glob, hashlib, json, os, re, sys, tarfile
ms, sk, mb, sb, notes = sys.argv[1:6]
try: made = json.load(open(os.path.join(ms, "made.json")))
except Exception: made = []
try: base = {e.get("name") for e in json.load(open(mb))}
except Exception: base = set()
try: sbase = set(open(sb).read().split())
except Exception: sbase = set()
names = [e.get("name") for e in made if e.get("name") not in base]
names += [d for d in sorted(os.listdir(sk)) if d not in sbase and d not in names] if os.path.isdir(sk) else []
G = re.compile(r'grants\s*=\s*\[([^\]]*)\]')
for name in names:
    e = next((x for x in made if x.get("name") == name), {})
    texts = []
    for f in glob.glob(os.path.join(sk, name, "**", "*"), recursive=True):
        if os.path.isfile(f) and f.endswith((".toml", ".py", ".md")):
            texts.append((f, open(f, errors="replace").read()))
    pk = os.path.join(ms, os.path.basename(e.get("package") or "")) if e.get("package") else ""
    if not texts and pk and os.path.isfile(pk):
        try:
            with tarfile.open(pk) as t:
                for m in t.getmembers():
                    if m.isfile() and m.name.endswith((".toml", ".py", ".md")):
                        texts.append((m.name, t.extractfile(m).read().decode("utf-8", "replace")))
        except Exception as x: print("NOTE %s package unreadable: %s" % (name, x))
    grants = []
    for f, t in texts:
        if f.endswith(".toml"):
            for g in G.findall(t):
                for w in re.findall(r'"([^"]+)"', g):
                    if w not in grants: grants.append(w)
    py = "\n".join(t for f, t in texts if f.endswith(".py"))
    reads = "yes" if re.search(r'notes\.get\s*\(', py) else "no"
    sets = "yes" if re.search(r'notes\.set\s*\(', py) else "no"
    nf = os.path.join(notes, "n_" + hashlib.sha256(("skill-notes:" + name).encode()).hexdigest()[:16] + ".json")
    if os.path.isfile(nf):
        raw = open(nf, "rb").read(); sha = hashlib.sha256(raw).hexdigest()[:16]
        try: d = json.loads(raw); keys = len(d.get("notes", d) if isinstance(d, dict) else d)
        except Exception: keys = "?"
        nfs = nf
    else: raw, sha, keys, nfs = b"", "-", 0, "none"
    print("NEW %s on=%s grants=%s reads_notes=%s sets_notes=%s notes=%s keys=%s sha=%s" % (name, e.get("on", "?"), ",".join(grants) or "-", reads, sets, nfs, keys, sha))
    if raw: print("NOTESJSON %s %s" % (name, raw.decode("utf-8", "replace").replace("\n", " ")[:800]))
P
