#!/usr/bin/env python3
"""Kit 5194 analysis: the played before/after of LAMP_OPENS_FURTHER (lampOS #5194).

    python3 -I analyze.py <kit dir>      (holds plays.tsv, mac/lines.tsv, logs/logs/*.txt)

Writes <kit>/transcript.tsv and <kit>/report.md, and prints the report.

The reply to a row is read from the voice/said tap (logs/said.txt: lamp_msgs/Speech, with the request
number), in the window from the row's clip start (for a stop row, the stop's start) to the next row's
start. Of the requests said in that window, the ones whose voice/heard text matches the played line
are the row's; if none matches, all of them, marked `unmatched` (busy-room talk can wake the lamp
too). With no voice/said in the window, the voice session's transcript events (events-PASS.jsonl)
are used, marked `events`. The gate column is the voice's own journal line for that request:
`told` ("the model is told its answer may end with one short opening") or `no: <why>`; with the
switch off there is none.

has_door is a simple heuristic on the reply's last sentence, for counting only; the PM's read
(the blank pm_read column) decides whether each opening fits and reads natural:
  question  the last sentence ends with "?"
  offer     it opens as an offer ("want me", "shall I", "I can", "I'm here", ... as opens_further.rs)
  noticing  it is a small noticing or wish ("that sounds", "sounds like", "hope", "good luck", ...)
"""
import csv, json, os, re, sys

K = sys.argv[1] if len(sys.argv) > 1 else os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LOGS = os.path.join(K, "logs", "logs")
if not os.path.isdir(LOGS):
    LOGS = os.path.join(K, "logs")
WINDOW_MAX = 150.0

OFFER = [s.split() for s in ("want me", "shall i", "should i", "would you like me", "do you want me", "i can",
                             "i could", "how about i", "let me know", "i'm here", "i am here", "if you want",
                             "if you'd like", "happy to")]
NOTICING = re.compile(r"\b(that sounds|sounds like|sounds (hard|rough|tough|lovely|exciting)|hope (you|it|she|he|they|the)|"
                      r"good luck|you've got this|you got this|take care|go easy|be gentle|be kind to yourself|"
                      r"you deserve|well done|congratulations|congrats|proud of you|enjoy|rest well|"
                      r"that's a lot|what a (day|week)|fingers crossed|give (her|him|them) a)\b", re.I)


def tap(name):
    out, path = [], os.path.join(LOGS, name)
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


def words(s):
    return re.findall(r"[a-z0-9']+", (s or "").lower().replace("’", "'"))


def overlap(line, heard):
    a = set(words(line)) - {"lamp", "lamb", "the", "a", "i", "to", "is", "it"}
    b = set(words(heard))
    return len(a & b) / len(a) if a else 0.0


def by_request(items, t0, t1):
    """{request: (first_at, text)} from Speech items in [t0, t1): the finished text, else the longest."""
    reqs = {}
    for at, p in items:
        if not (t0 <= at < t1):
            continue
        text = (p.get("text") or "").strip()
        if not text:
            continue
        r = p.get("request", f"t{int(at)}")
        first, best, fin = reqs.get(r, (at, "", False))
        if p.get("finished"):
            best, fin = text, True
        elif not fin and len(text) > len(best):
            best = text
        reqs[r] = (min(first, at), best, fin)
    return {r: (v[0], v[1]) for r, v in reqs.items()}


def door(reply):
    reply = (reply or "").strip()
    if not reply:
        return "n", ""
    sentences = [s for s in re.split(r"(?<=[.!?])\s+", reply) if s.strip()]
    last = sentences[-1].strip()
    w = words(last)
    if last.endswith("?"):
        return "y", "question"
    if any(w[: len(o)] == o for o in OFFER):
        return "y", "offer"
    if len(sentences) > 1 and NOTICING.search(last):
        return "y", "noticing"
    return "n", ""


lines = {}
for raw in open(os.path.join(K, "mac", "lines.tsv")):
    if raw.startswith("#") or not raw.strip():
        continue
    row, kind, how, clip, text = raw.rstrip("\n").split("\t")
    lines[row] = (kind, how, text)

plays = []
pp = os.path.join(K, "plays.tsv")
if os.path.exists(pp):
    for r in csv.DictReader(open(pp), delimiter="\t"):
        try:
            r["t"], r["mono"] = float(r["lamp_wall"]), int(r["lamp_mono_us"])
        except (ValueError, TypeError):
            continue
        plays.append(r)
said, heard, room = tap("said.txt"), tap("heard.txt"), tap("room.txt")
gates = {}
jp = os.path.join(LOGS, "journal.txt")
if os.path.exists(jp):
    for line in open(jp, errors="replace"):
        m = re.search(r"request (\d+): (the model is told its answer may end with one short opening|no opening further: (.*?)) \(LAMP_OPENS_FURTHER\)", line)
        if m:
            gates[int(m.group(1))] = "told" if m.group(3) is None else "no: " + m.group(3)
events = {}
for p in ("off", "on"):
    ep = os.path.join(LOGS, f"events-{p}.jsonl")
    ev = []
    if os.path.exists(ep):
        for line in open(ep, errors="replace"):
            try:
                e = json.loads(line)
            except ValueError:
                continue
            if e.get("kind") == "transcript":
                ev.append(e)
    events[p] = ev

out = []
for i, r in enumerate(plays):
    nxt = next((q for q in plays[i + 1:] if q["pass"] == r["pass"]), None)
    t0, t1 = r["t"], min(nxt["t"] if nxt else r["t"] + WINDOW_MAX, r["t"] + WINDOW_MAX)
    m0, m1 = r["mono"], (nxt["mono"] if nxt else r["mono"] + int(WINDOW_MAX * 1e6))
    kind, how, text = lines.get(r["row"], ("?", "?", "?"))
    h = by_request(heard, t0 - 1, t1)
    s = by_request(said, t0, t1)
    mine = [q for q, (_, ht) in h.items() if overlap(text, ht) >= 0.4]
    heard_text = " / ".join(h[q][1] for q in mine) or " / ".join(v[1] for v in sorted(h.values()))
    how_read = "said"
    if mine and any(q in s for q in mine):
        reqs = [q for q in sorted(s, key=lambda q: s[q][0]) if q in mine]
    else:
        reqs = sorted(s, key=lambda q: s[q][0])
        how_read = "said,unmatched" if reqs else "none"
    reply = " ".join(s[q][1] for q in reqs)
    if not reply:
        ev = [e for e in events.get(r["pass"], []) if m0 <= e.get("at_us", 0) < m1
              and e.get("role", "output") != "input" and e.get("turn") is not None]
        if ev:
            reply, how_read = " ".join((e.get("text") or "").strip() for e in ev), "events"
    d, dk = door(reply)
    g = "; ".join(gates[q] for q in reqs if isinstance(q, int) and q in gates) or "-"
    rm = [p for at, p in room if at <= t0 + 10]
    rs = re.search(r'"(?:state|room)":"([a-z_]+)"', json.dumps(rm[-1])) if rm else None
    out.append(dict(row=r["row"], switch=r["pass"], kind=kind, line=text, heard=heard_text, reply=reply,
                    has_door=d, door_kind=dk, gate=g, room=rs.group(1) if rs else "?", read=how_read,
                    requests=",".join(str(q) for q in reqs), pm_read=""))

cols = ["row", "switch", "kind", "line", "heard", "reply", "has_door", "door_kind", "gate", "room", "read", "requests", "pm_read"]
with open(os.path.join(K, "transcript.tsv"), "w") as f:
    f.write("\t".join(cols) + "\n")
    for o in out:
        f.write("\t".join(str(o[c]).replace("\t", " ").replace("\n", " ") for c in cols) + "\n")


def count(sw, kinds):
    rows = [o for o in out if o["switch"] == sw and o["kind"] in kinds]
    return sum(o["has_door"] == "y" for o in rows), len(rows), sum(not o["reply"] for o in rows)


rep = ["# Kit 5194 report: LAMP_OPENS_FURTHER played before/after (lampOS #5194)", ""]
bt = os.path.join(K, "logs", "state", "build.txt")
if os.path.exists(bt):
    rep.append(f"Build: `{open(bt).read().strip()}`. Rows played: {len(out)} ({len(plays)} in plays.tsv).")
rep += ["", "| switch | doors on the 20 everyday lines | of them on feeling lines | on plain lines (a nag) | zero-lines with a door (commands, stops, busy room) | no reply |",
        "|---|---|---|---|---|---|"]
for sw in ("off", "on"):
    e, ne, ze = count(sw, ("feel", "plain"))
    fe, nf, _ = count(sw, ("feel",))
    pe, npl, _ = count(sw, ("plain",))
    z, nz, zz = count(sw, ("cmd", "stop", "busy"))
    rep.append(f"| {sw} | {e}/{ne} | {fe}/{nf} | {pe}/{npl} | {z}/{nz} | {ze + zz} |")
e, ne, _ = count("on", ("feel", "plain"))
z, nz, _ = count("on", ("cmd", "stop", "busy"))
rep += ["", "**The bar** (#5194's done-when): with the switch on, at least 8 of 20 answers carry a fitting opening, and 0 of the command, stop and busy-room lines do; the openings read natural to the PM.", ""]
if ne < 20 or nz < 8:
    rep.append(f"- NOT ALL ROWS PLAYED with the switch on ({ne}/20 everyday, {nz}/8 zero-lines): no verdict.")
else:
    a = "meets" if e >= 8 else "misses"
    b = "meets" if z == 0 else "misses"
    rep.append(f"- doors on: {e}/20, by the heuristic: {a} the 8/20 count.")
    rep.append(f"- zero-lines on: {z}/{nz}: {b} the 0 bar.")
    rep.append("- natural: the PM's read of `transcript.tsv` (the pm_read column). The heuristic only counts; it does not judge fit.")
told = sum(o["gate"].startswith("told") for o in out if o["switch"] == "on")
rep += ["", f"The voice's gate (journal, switch on): told on {told} rows; `no:` on "
        f"{sum(o['gate'].startswith('no') for o in out if o['switch'] == 'on')}. Off rows with any gate line: "
        f"{sum(o['gate'] != '-' for o in out if o['switch'] == 'off')} (must be 0).",
        f"Rows whose reply could not be tied to the played line (`unmatched`): {sum('unmatched' in o['read'] for o in out)}; read from session events: {sum(o['read'] == 'events' for o in out)}.",
        "", "## Rows", "", "| row | switch | kind | line | reply | door | gate | room |", "|---|---|---|---|---|---|---|---|"]
for o in sorted(out, key=lambda o: (o["row"], o["switch"])):
    cell = lambda s: (s or "").replace("|", "/")
    rep.append(f"| {o['row']} | {o['switch']} | {o['kind']} | {cell(o['line'])} | {cell(o['reply']) or '(no reply)'} | "
               f"{o['has_door']} {o['door_kind']} | {cell(o['gate'])} | {o['room']} |")
open(os.path.join(K, "report.md"), "w").write("\n".join(rep) + "\n")
print("\n".join(rep))
