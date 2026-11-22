# fixcheck.py <probe dir>: each row fixture's skill.toml against the product loader's rules (#8180's head,
# lampOS/crates/skills/src/lib.rs, `load`), mirrored here since lamp-skills has no check-only command. One line per
# fixture, "FIX <name> OK" or "FIX <name> BAD <why>"; exit 0 only when every fixture is OK.
import os, re, sys, tomllib
# voice/src/skill_task_yes.rs:103 GRANTS: the only names a skill's grants may list (task.rs:351 grants_of)
GRANTS = {"body", "light", "speak", "look", "hear", "sound", "air", "device", "camera", "microphone", "notes", "speech"}
def check(folder):
    t = tomllib.load(open(os.path.join(folder, "skill.toml"), "rb"))
    card = open(os.path.join(folder, "SKILL.md")).read()
    steps, tool, asks = t.get("do", []), t.get("tool"), t.get("asks", [])
    trig = t.get("trigger")
    # lib.rs:3800 made_on_the_lamp: only a card marked "made: here" is made here; lib.rs:3723 started_by_words: asks
    # start a skill with no tool only when it is made here (and LAMP_MADE_SKILL_ASKS is on). An installed fixture is not.
    front = card[4:].split("\n---", 1)[0] if card.startswith("---\n") else ""
    made = "made: here" in [l.strip() for l in front.splitlines()]
    by_words = bool(asks) and tool is None and made
    # lib.rs:4521: a trigger or a tool (or the made-here asks), and at least one [[do]]
    if (not trig and tool is None and not by_words) or not steps:
        return "skill.toml needs a trigger or a tool, and at least one [[do]] (lib.rs:4521)"
    # lib.rs:4541: the tool's name
    if tool is not None and not re.fullmatch(r"[a-z][a-z0-9_]{0,47}", tool):
        return "tool %r: a lowercase letter, then at most 47 lowercase letters, digits and _ (lib.rs:4541)" % tool
    # lib.rs:3678 checked_asks: asks call a plain tool (no choice, no flag), 2 to 8 words
    if asks and not ((tool is not None and "choice" not in t and "flag" not in t) or by_words):
        return "asks need a tool with no choice or flag (lib.rs:3682)"
    for a in asks:
        if not 2 <= len(a.split(" ")) <= 8: return "ask %r: 2 to 8 words (lib.rs:3693)" % a
    runs = [s for s in steps if "run" in s or "task" in s]
    # lib.rs:5199: grants only beside a run or task step; each one a known grant
    if "grants" in t and not runs: return "grants only beside a do.task or do.run (lib.rs:5201)"
    bad = [g for g in t.get("grants", []) if g not in GRANTS]
    if bad: return "not a grant: %s (task.rs:363)" % bad
    for s in steps:
        if "run" in s:
            if len(s) != 1: return "do.run is alone in its [[do]] table"
            if not re.fullmatch(r"scripts/[^/]+\.py", s["run"]) or not os.path.isfile(os.path.join(folder, s["run"])):
                return "do.run %r: a Python file in its own scripts/" % s["run"]
    return None
bad = 0
for n in sorted(os.listdir(sys.argv[1])):
    why = check(os.path.join(sys.argv[1], n))
    print("FIX %s %s" % (n, "OK" if why is None else "BAD " + why)); bad += why is not None
sys.exit(1 if bad else 0)
