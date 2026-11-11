#!/usr/bin/env python3
"""Replay kit 5194's door counter over a real lamp capture (V2's replay rule, #5194).

    python3 -I replay.py <capture dir> [<capture dir> ...]

Each capture dir holds `said.txt` (a `lamp-bus tap voice/said` capture) and,
if taken, `heard.txt` (`lamp-bus tap voice/heard`) and a one-line `room.txt`
note ("quiet" or "busy"). Each spoken request is one row: what was heard just
before it, the full reply, and the kit's verdict from analyze.py's own
`door()`, which is imported, not copied, so the replay judges exactly as the
kit does. The `person` column is left blank for a person to fill line by line
(`y` = an opening, `n` = none); `agree` is filled when the person column is.

Writes <capture dir>/replay.tsv and prints a markdown table plus the counts.
With LAMP_OPENS_FURTHER off, a door the kit counts is either the model's own
habit (a question the person also reads as an opening) or a false count.
"""
import importlib.util, json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))


def judge():
    """analyze.py's door(), loaded without running its report."""
    src = open(os.path.join(HERE, "analyze.py")).read()
    head = src.split("\nlines = {}", 1)[0]
    mod = {}
    exec(compile(head, "analyze.py", "exec"), mod)
    return mod["door"]


def tap(path):
    out = []
    if not os.path.exists(path):
        return out
    for line in open(path, errors="replace"):
        i = line.find("} {")
        if i < 0:
            continue
        try:
            head, body = json.loads(line[: i + 1]), json.loads(line[i + 2:])
        except ValueError:
            continue
        out.append((head.get("at_unix_ms", 0) / 1000.0, body))
    return out


def requests(items):
    """{request: (first_at, text)}. A finished piece with text is the whole
    line (voice/heard); else the pieces are joined in order (voice/said
    streams its reply in pieces and ends with an empty finished one)."""
    reqs = {}
    for at, p in items:
        text = p.get("text") or ""
        r = p.get("request", f"t{int(at)}")
        first, pieces, whole = reqs.get(r, (at, [], None))
        if p.get("finished") and text.strip():
            whole = text.strip()
        elif text.strip():
            pieces.append(text)
        reqs[r] = (min(first, at), pieces, whole)
    out = {}
    for r, (first, pieces, whole) in reqs.items():
        joined = whole if whole is not None else " ".join("".join(pieces).split())
        if joined:
            out[r] = (first, joined)
    return out


def main(dirs):
    door = judge()
    total = {"rows": 0, "kit": 0, "person": 0, "agree": 0, "judged": 0}
    for d in dirs:
        said = requests(tap(os.path.join(d, "said.txt")))
        heard = sorted(requests(tap(os.path.join(d, "heard.txt"))).values())
        room = open(os.path.join(d, "room.txt")).read().strip() if os.path.exists(os.path.join(d, "room.txt")) else "?"
        old = {}
        rp = os.path.join(d, "replay.tsv")
        if os.path.exists(rp):
            for raw in list(open(rp))[1:]:
                f = raw.rstrip("\n").split("\t")
                if len(f) >= 7:
                    old[f[0]] = f[6]
        rows = []
        for r, (at, reply) in sorted(said.items(), key=lambda kv: kv[1][0]):
            before = [t for a, t in heard if a <= at]
            yn, kind = door(reply)
            person = old.get(str(r), "")
            agree = "" if person not in ("y", "n") else ("yes" if person == yn else "NO")
            rows.append([str(r), room, before[-1] if before else "", reply, yn, kind, person, agree])
        with open(rp, "w") as f:
            f.write("request\troom\theard\treply\tkit\tkit_kind\tperson\tagree\n")
            for row in rows:
                f.write("\t".join(c.replace("\t", " ").replace("\n", " ") for c in row) + "\n")
        print(f"## {os.path.basename(os.path.normpath(d))} ({room}, {len(rows)} replies)\n")
        print("| request | heard | reply | kit | person | agree |\n|---|---|---|---|---|---|")
        cell = lambda s: s.replace("|", "/")
        for row in rows:
            print(f"| {row[0]} | {cell(row[2])} | {cell(row[3])} | {row[4]} {row[5]} | {row[6]} | {row[7]} |")
            total["rows"] += 1
            total["kit"] += row[4] == "y"
            if row[6] in ("y", "n"):
                total["judged"] += 1
                total["person"] += row[6] == "y"
                total["agree"] += row[7] == "yes"
        print()
    print(f"Replies: {total['rows']}. Kit doors: {total['kit']}. Person-judged: {total['judged']}, "
          f"person doors: {total['person']}, agreeing lines: {total['agree']}/{total['judged']}.")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    main(sys.argv[1:])
