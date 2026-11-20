"""Mechanical gates for the routing-asks spec: schema, v2 compatibility,
twins, traceability, blind export, three-way diff and reviewer agreement. Standard library only; run with python3 -I."""
import argparse
import json
import random
import re
import sys
import unicodedata

# rule -> (expect, doors or None, acts required). Mirrors policy.md.
RULES = {
    "P1": ("answer", None, False),
    "P2": ("answer", None, True),
    "P3": ("hand_over", frozenset({"ask"}), False),
    "P4": ("hand_over", frozenset({"task"}), False),
    "P4b": ("hand_over", frozenset({"ask", "task"}), False),
    "P5": ("hand_over", frozenset({"ask", "task"}), False),
    "P7a": ("answer", None, False),
    "P7b": ("hand_over", frozenset({"answer", "task"}), False),
    "P8": ("answer", None, False),
}
ACTS = frozenset({"light", "timer", "reminder", "volume", "memory",
                  "time", "weather", "search", "skill"})
DOORS = {"answer", "ask", "task"}
POLICY_HEADINGS = ("Order of application", "Scoring contract", "Known weaknesses")
TRACE = ("Source:", "New ruling, owner-confirmed", "New ruling, provisional")


def load(path):
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    return data["asks"] if isinstance(data, dict) else data


def save(path, cases, about=None):
    data = cases if about is None else {"about": about, "asks": cases}
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False, indent=2)
        f.write("\n")


def door_set(case):
    if case["expect"] == "hand_over":
        return frozenset(case["doors"])
    return frozenset({"answer"})


def _type_errors(c):
    errs = []
    if not isinstance(c.get("id"), int) or isinstance(c.get("id"), bool):
        errs.append("id is not an int")
    for k in ("ask", "kind", "why"):
        if k in c and not isinstance(c[k], str):
            errs.append(f"{k} is not a string")
    if c.get("expect") not in ("answer", "hand_over"):
        errs.append("expect is not answer or hand_over")
    if "lang" in c and c["lang"] != "vi":
        errs.append('lang must be "vi" or absent')
    if "acts" in c and (not isinstance(c["acts"], str) or c["acts"] not in ACTS):
        errs.append(f"acts {c['acts']!r} is not one of the acts classes")
    if "doors" in c:
        d = c["doors"]
        if c.get("expect") == "answer":
            errs.append("doors on an answer case")
        elif not (isinstance(d, list) and d and all(x in DOORS for x in d)):
            errs.append("doors is not a non-empty list of answer/ask/task")
    elif c.get("expect") == "hand_over":
        errs.append("hand_over without doors")
    return errs


def _rule_errors(c):
    rule = c.get("rule")
    if rule not in RULES:
        return [f"rule {rule!r} is not in the rule table"]
    expect, doors, needs_acts = RULES[rule]
    errs = []
    if c["expect"] != expect:
        errs.append(f"{rule} expects {expect}, case says {c['expect']}")
    elif doors is not None and door_set(c) != doors:
        errs.append(f"{rule} doors are {sorted(doors)}, case says {sorted(door_set(c))}")
    if needs_acts and "acts" not in c:
        errs.append(f"{rule} needs acts")
    if not needs_acts and "acts" in c:
        errs.append(f"{rule} takes no acts")
    if rule == "P8" and c["id"] <= 100:
        errs.append("P8 is for ids 101 and up only")
    return errs


def _policy_errors(text):
    sections = {}
    for m in re.finditer(r"^## (.+?)\n(.*?)(?=^## |\Z)", text, re.M | re.S):
        sections[m.group(1).strip()] = m.group(2)
    errs = []
    for rule in list(RULES) + ["P6"]:
        body = next((b for h, b in sections.items() if h.split(" ")[0] == rule), None)
        if body is None:
            errs.append(f"no '## {rule} ' heading")
        elif not any(line.startswith(TRACE) for line in body.splitlines()):
            errs.append(f"{rule} has no Source, owner-confirmed or provisional line")
    for h in POLICY_HEADINGS:
        if h not in sections:
            errs.append(f"no '## {h}' heading")
    return errs


def check(cases, v2_cases, policy_text, twins):
    """Return every error as '<id>: <code>: <detail>'; an empty list passes."""
    errors = []

    def err(i, code, detail):
        errors.append(f"{i}: {code}: {detail}")

    ids = [c.get("id") for c in cases]
    ints = [i for i in ids if isinstance(i, int)]
    if len(set(ints)) != len(ids) or ints != sorted(ints) or not set(range(1, 101)) <= set(ints):
        err(0, "ids", "ids must be unique, ascending and include 1-100")
    v2_by_id = {c["id"]: c for c in v2_cases}
    by_id = {c["id"]: c for c in cases if isinstance(c.get("id"), int)}

    for c in cases:
        i = c.get("id")
        base = v2_by_id.get(i)
        if base is not None:
            for k in base:
                # A relabelled case may drop doors or acts; the rule check owns those.
                if k not in c and k not in ("doors", "acts"):
                    err(i, "v2-field", f"{k} is missing")
            if c.get("ask") != base["ask"]:
                err(i, "ask-changed", "ask differs from v2")
            for k in ("kind", "lang"):
                if k in c and c.get(k) != base.get(k):
                    err(i, "v2-field", f"{k} differs from v2")
        types = _type_errors(c)
        for detail in types:
            err(i, "type", detail)
        if not types:
            for detail in _rule_errors(c):
                err(i, "rule", detail)
        if isinstance(i, int):
            if (i >= 101) != (c.get("new") is True) or (i <= 100 and "new" in c):
                err(i, "new", "new: true belongs on ids 101 and up, and only there")
            if i >= 101 and isinstance(c.get("ask"), str) and \
                    unicodedata.normalize("NFC", c["ask"]) != c["ask"]:
                err(i, "nfc", "ask is not NFC")
        for entry in c.get("pre", []):
            m = re.fullmatch(r"after:(\d+)", str(entry))
            if entry != "face" and not (m and isinstance(i, int)
                                        and int(m.group(1)) < i and int(m.group(1)) in by_id):
                err(i, "pre", f"bad precondition {entry!r}")

    for group in twins:
        members = [by_id.get(i) for i in group]
        if any(m is None for m in members):
            err(group[0], "twins", f"group {group} has an absent member")
            continue
        try:
            labels = {(m["expect"], door_set(m), m.get("rule")) for m in members}
        except (KeyError, TypeError):
            continue
        if len(labels) > 1:
            err(group[0], "twins", f"group {group} differs in expect, doors or rule")

    for detail in _policy_errors(policy_text):
        err(0, "policy", detail)
    return errors


def blind(cases, seed, n=None):
    """Export rows with only a blind id, ask and lang, plus the key to real ids."""
    rng = random.Random(seed)
    if n is not None:
        if n > len(cases):
            raise ValueError(f"sample {n} exceeds {len(cases)} cases")
        picked = rng.sample(cases, n)
    else:
        picked = list(cases)
        rng.shuffle(picked)
    rows, key = [], {}
    for blind_id, c in enumerate(picked, 1):
        row = {"id": blind_id, "ask": c["ask"]}
        if "lang" in c:
            row["lang"] = c["lang"]
        rows.append(row)
        key[str(blind_id)] = c["id"]
    return rows, key


def unblind(labels, key):
    """Map reviewer rows from blind ids back to real ids, sorted by id."""
    out, seen = [], set()
    for row in labels:
        k = str(row["id"])
        if k not in key:
            raise ValueError(f"blind id {k} is not in the key")
        if k in seen:
            raise ValueError(f"blind id {k} appears twice")
        seen.add(k)
        out.append(dict(row, id=key[k]))
    return sorted(out, key=lambda r: r["id"])


def _label(row):
    return {"expect": row["expect"], "doors": sorted(door_set(row)),
            "acts": row.get("acts"), "rule": row.get("rule")}


def diff3(v2, a, b):
    """One row per v2 case with the verdict keep, change or decide."""
    a_by, b_by = {r["id"]: r for r in a}, {r["id"]: r for r in b}
    rows = []
    for c in v2:
        i = c["id"]
        if i not in a_by or i not in b_by:
            raise ValueError(f"a reviewer has no label for id {i}")
        base, la, lb = _label(c), _label(a_by[i]), _label(b_by[i])
        route = lambda x: (x["expect"], x["doors"])
        if route(la) == route(lb) and la["acts"] == lb["acts"]:
            verdict = "keep" if route(la) == route(base) else "change"
        else:
            verdict = "decide"
        rows.append({"id": i, "ask": c["ask"], "v2": base, "a": la, "b": lb,
                     "verdict": verdict})
    return rows


def agree(v3, reviewer):
    """Score a reviewer against v3 over the cases the reviewer labelled."""
    if not reviewer:
        raise ValueError("the reviewer labelled nothing")
    spec = {c["id"]: c for c in v3}
    miss_expect, miss_door = [], []
    for row in reviewer:
        i = row["id"]
        if i not in spec:
            raise ValueError(f"reviewer id {i} is not in v3")
        if row.get("expect") == "hand_over" and not row.get("doors"):
            raise ValueError(f"reviewer id {i} is hand_over without doors")
        if row["expect"] != spec[i]["expect"]:
            miss_expect.append(i)
        if not door_set(row) <= door_set(spec[i]):
            miss_door.append(i)
    n = len(reviewer)
    return {"n": n, "expect_rate": (n - len(miss_expect)) / n,
            "door_rate": (n - len(miss_door)) / n,
            "misses": {"expect": sorted(miss_expect), "door": sorted(miss_door)}}


def _load_key(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def _cmd_diff3(a):
    key = _load_key(a.key)
    rows = diff3(load(a.v2), unblind(load(a.a), key), unblind(load(a.b), key))
    save(a.out, rows)
    for verdict in ("keep", "change", "decide"):
        print(verdict, sum(r["verdict"] == verdict for r in rows))
    return 0


def _cmd_agree(a):
    v3, key = load(a.v3), _load_key(a.key)
    failed, miss_sets = False, []
    for path in a.reviews:
        got = agree(v3, unblind(load(path), key))
        print(path, json.dumps(got))
        miss_sets.append(set(got["misses"]["expect"]) | set(got["misses"]["door"]))
        if got["expect_rate"] < 0.95 or got["door_rate"] < 0.90:
            failed = True
        if not a.partial and got["n"] < len(v3):
            print(f"{path}: labelled {got['n']} of {len(v3)} cases")
            failed = True
    both = sorted(set.intersection(*miss_sets))
    print("both_miss", both)
    return 1 if failed or both else 0


def _cmd_check(a):
    with open(a.policy, encoding="utf-8") as f:
        policy = f.read()
    with open(a.twins, encoding="utf-8") as f:
        twins = json.load(f)
    cases = load(a.v3)
    errors = check(cases, load(a.v2), policy, twins)
    for e in errors:
        print(e)
    if errors:
        return 1
    print(f"OK {len(cases)} cases")
    return 0


def _cmd_blind(a):
    rows, key = blind(load(a.input), a.seed, a.sample)
    save(a.out, rows)
    with open(a.key, "w", encoding="utf-8") as f:
        json.dump(key, f, indent=2)
        f.write("\n")
    print(f"{len(rows)} rows -> {a.out}, key -> {a.key}")
    return 0


def main(argv=None):
    p = argparse.ArgumentParser(prog="rasks.py")
    sub = p.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("check")
    c.add_argument("v3")
    c.add_argument("--v2", required=True)
    c.add_argument("--policy", required=True)
    c.add_argument("--twins", required=True)
    c.set_defaults(fn=_cmd_check)
    b = sub.add_parser("blind")
    b.add_argument("input")
    b.add_argument("--seed", type=int, required=True)
    b.add_argument("--sample", type=int)
    b.add_argument("--out", required=True)
    b.add_argument("--key", required=True)
    b.set_defaults(fn=_cmd_blind)
    d = sub.add_parser("diff3")
    d.add_argument("v2")
    d.add_argument("a")
    d.add_argument("b")
    d.add_argument("--key", required=True)
    d.add_argument("--out", required=True)
    d.set_defaults(fn=_cmd_diff3)
    g = sub.add_parser("agree")
    g.add_argument("v3")
    g.add_argument("reviews", nargs="+")
    g.add_argument("--key", required=True)
    g.add_argument("--partial", action="store_true")
    g.set_defaults(fn=_cmd_agree)
    a = p.parse_args(argv)
    return a.fn(a)


if __name__ == "__main__":
    sys.exit(main())
