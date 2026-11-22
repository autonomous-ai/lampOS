# fixcheck.py <fixtures dir>: each row fixture's skill.toml and SKILL.md against the product loader's rules, mirrored
# here since lamp-skills has no check-only command (#7901's head, lampOS/crates/skills/src/lib.rs: load_with :4130,
# its file :236 deny_unknown_fields, :4354 a trigger or a tool and a [[do]], :4371 the tool's name, :3594 asks,
# :4987-5047 do.task, :4997 grants only beside it, :5003 secrets only beside it; task.rs :196-250 the run path, :383
# a do.task only from the Store or a push; voice skill_task_yes.rs :103 GRANTS and :158 NET). Ported from the #8180
# e82d kit's (kits/8180-e82d/mock/row/fixcheck.py). One line per fixture, "FIX <name> OK" or "FIX <name> BAD <why>";
# exit 0 only when every fixture is OK.
import os, re, sys, tomllib
# skill_task_yes.rs:103 GRANTS, and NET (on with LAMP_TASK_NET, on in the product)
GRANTS = {"body", "light", "speak", "look", "hear", "sound", "air", "device", "camera", "microphone", "notes", "speech", "net"}
KEYS = {"asks", "kind", "version", "tool", "grants", "secrets", "permissions", "do", "trigger", "choice", "flag", "description"}
SECRET = re.compile(r"[A-Z][A-Z0-9_]{2,63}")
def check(folder):
    name = os.path.basename(folder.rstrip("/"))
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,63}", name) or "_" in name: return "folder name %r: 1-64 lowercase letters, digits and - (lib.rs:4134)" % name
    if os.path.exists(os.path.join(folder, "run.py")): return "a run.py: it won't load (lib.rs:4147)"
    card = open(os.path.join(folder, "SKILL.md")).read()
    if not card.startswith("---\n"): return "SKILL.md: no front matter"
    front = dict(l.split(":", 1) for l in card[4:].split("\n---", 1)[0].splitlines() if ":" in l)
    if front.get("name", "").strip() != name: return "SKILL.md name %r is not the folder's %r (lib.rs:4134)" % (front.get("name"), name)
    if not front.get("description", "").strip(): return "SKILL.md: no description"
    if "made: here" in [l.strip() for l in card.splitlines()]: return "made: here: not a Store or pushed skill"
    t = tomllib.load(open(os.path.join(folder, "skill.toml"), "rb"))
    bad = set(t) - KEYS
    if bad: return "unknown keys %s (lib.rs:236 deny_unknown_fields)" % sorted(bad)
    steps, tool, asks = t.get("do", []), t.get("tool"), t.get("asks", [])
    if t.get("kind", "installable") != "installable": return "kind: a pushed skill is installable"
    if not re.fullmatch(r"\d+\.\d+\.\d+", t.get("version", "")): return "version: x.y.z"
    if (not t.get("trigger") and tool is None) or not steps: return "a trigger or a tool, and at least one [[do]] (lib.rs:4354)"
    if tool is not None and not re.fullmatch(r"[a-z][a-z0-9_]{0,47}", tool): return "tool %r (lib.rs:4371)" % tool
    if len(front.get("description", "").encode()) > 1024: return "description over 1024 bytes with a tool"
    if asks and (tool is None or "choice" in t or "flag" in t): return "asks need a plain tool (lib.rs:3594)"
    for a in asks:
        if not 2 <= len(a.split(" ")) <= 8: return "ask %r: 2 to 8 words (lib.rs:3594)" % a
    tasks = [s for s in steps if "task" in s]
    if len(tasks) > 1: return "one do.task a skill (lib.rs:4987)"
    for s in tasks:
        if len(s) != 1: return "do.task is alone in its [[do]] table (lib.rs:4987)"
        task = s["task"]
        if set(task) != {"task", "run"}: return "do.task = { task, run } (lib.rs:4987)"
        w = task["task"]
        if not (1 <= len(w.encode()) <= 120) or "\n" in w: return "do.task's words: one plain sentence of at most 120 characters (Skill::made_refuses, #7874)"
        r = task["run"]; parts = r.split("/")
        if parts[0] != "scripts" or not 2 <= len(parts) <= 4 or ".." in parts or any(p.startswith(".") for p in parts) \
           or not os.path.isfile(os.path.join(folder, r)) or os.path.islink(os.path.join(folder, r)):
            return "do.task run %r: a plain file in its own scripts/ (task.rs:196)" % r
    if "grants" in t:
        if not tasks: return "grants only beside a do.task (lib.rs:4997)"
        g = t["grants"]
        if len(set(g)) != len(g): return "a grant twice"
        if set(g) - GRANTS: return "not a grant: %s (skill_task_yes.rs:103)" % sorted(set(g) - GRANTS)
    if "secrets" in t:
        if not tasks: return "secrets only beside a do.task (lib.rs:5003)"
        sc = t["secrets"]
        if not (1 <= len(sc) <= 16) or len(set(sc)) != len(sc) or not all(SECRET.fullmatch(x) and not x.startswith(("LAMP_", "HERMES_", "PYTHON")) for x in sc):
            return "secrets: at most 16 names (A-Z, 0-9 and _, 3 to 64, not the run's own), once each (lib.rs:5003)"
    if not tasks: return "no do.task: this kit's fixtures each carry one"
    return None
bad = 0
for n in sorted(os.listdir(sys.argv[1])):
    why = check(os.path.join(sys.argv[1], n))
    print("FIX %s %s" % (n, "OK" if why is None else "BAD " + why)); bad += why is not None
sys.exit(1 if bad else 0)
