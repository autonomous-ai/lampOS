# lamp, root sh (common.lamp.sh first). READ ONLY. Every made skill new or changed since save.lamp.sh's baseline:
#   MADE <name> new|changed on=<..> kind=run|task|both|none grants=<a,b> uses=0|1 result=0|1 answer=0|1
#        stub=<n> prints=<n> fm_with=<..> idx_with=<..> engine=<..> made_by=<..> src=folder|package
#   CARD <name> <the SKILL.md front matter, one line>
python3 -I - "$MS" "$SK" "$R/made.json.before" "$(engine)" <<'P'
import io, json, os, re, sys, tarfile
ms, sk, mb, eng = sys.argv[1:5]
try: made = json.load(open(os.path.join(ms, "made.json")))
except Exception: made = []
try: base = {e.get("name"): json.dumps(e, sort_keys=True) for e in json.load(open(mb))}
except Exception: base = {}
STUB = "The lamp's agent does this task"
for e in made:
    name = e.get("name") or ""
    if name in base and base[name] == json.dumps(e, sort_keys=True): continue
    files, src = {}, "folder"
    d = os.path.join(sk, name)
    if os.path.isdir(d):
        for root, _, fs in os.walk(d):
            for f in fs:
                p = os.path.join(root, f)
                if f.endswith((".md", ".toml", ".py")): files[os.path.relpath(p, d)] = open(p, errors="replace").read()
    pk = os.path.join(ms, os.path.basename(e.get("package") or "")) if e.get("package") else ""
    if not files and pk and os.path.isfile(pk):
        src = "package"
        with tarfile.open(pk) as t:
            for m in t.getmembers():
                if m.isfile() and m.name.endswith((".md", ".toml", ".py")):
                    files[re.sub(r"^(\./)?[^/]+/", "", m.name) if "/" in m.name.lstrip("./") else m.name] = t.extractfile(m).read().decode("utf-8", "replace")
    card = next((v for k, v in files.items() if k.endswith("SKILL.md")), "")
    toml = next((v for k, v in files.items() if k.endswith("skill.toml")), "")
    main = next((v for k, v in files.items() if k.endswith("scripts/main.py")), "")
    fm = card.split("---")[1] if card.startswith("---") and card.count("---") >= 2 else ""
    fmd = dict(l.split(":", 1) for l in fm.strip().splitlines() if ":" in l)
    has = lambda h: "1" if re.search(r"^## %s\s*$" % h, card, re.M) else "0"
    run = bool(re.search(r'^\s*run\s*=', toml, re.M)); task = bool(re.search(r'^\s*task\s*=', toml, re.M))
    kind = "both" if run and task else "run" if run else "task" if task else "none"
    g = re.search(r'grants\s*=\s*\[([^\]]*)\]', toml); grants = ",".join(re.findall(r'"([^"]+)"', g.group(1))) if g else "-"
    stub = sum(1 for v in files.values() if STUB in v)
    prints = len(re.findall(r'^\s*print\(', main, re.M))
    print("MADE %s %s on=%s kind=%s grants=%s uses=%s result=%s answer=%s stub=%d prints=%d fm_with=%s idx_with=%s engine=%s made_by=%s src=%s" % (
        name, "changed" if name in base else "new", e.get("on", "?"), kind, grants, has("Uses"), has("Result"), has("Answer"),
        stub, prints, fmd.get("made_with", "-").strip() or "-", e.get("made_with", "-"), eng or "-", fmd.get("made_by", "-").strip() or "-", src))
    print("CARD %s %s" % (name, " | ".join(l.strip() for l in fm.strip().splitlines())[:300]))
P
