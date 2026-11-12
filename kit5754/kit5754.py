#!/usr/bin/env python3
"""Kit 5754 verdicts, on the Mac, from a copy of the lamp's /root/kit5754:

    python3 -I kit5754.py COPY_DIR            # both rows that ran
    python3 -I kit5754.py COPY_DIR --pieces   # also every voice/said piece of row 5

Reads rec/*.jsonl (`lamp-bus record`: one line per message, the payload in
base64), logs/journal.txt (journalctl -o short-unix) and row3.window /
row5.window. Prints the ROW3 and ROW5 lines meant for the post.

Why the words were empty in #5555: `voice/said` carries the lamp's words in
pieces, `finished: false`, and the second route (ElevenLabs) ends each line
with one last piece that is EMPTY and `finished: true` ("a last piece, empty
and finished", crates/voice/src/second_provider.rs). A reader that keeps the
finished row as "the whole of what was said" (as msgs' Speech doc reads) gets
"". Here every piece of the line's request is joined in order.
"""
import base64
import json
import re
import sys
import time
from pathlib import Path

NEAR = 0.14  # the skills' `near`: a face at least this share of the picture's height
SKILLS = ("comfort-break", "meal-check")
# What comfort-break's line must never name (its say note; rule 8 and the skill's test).
NEVER = re.compile(
    r"\b(bathroom|toilet|restroom|washroom|loo|wc|pee|lavatory|health|healthy|bladder)\b", re.I
)


def records(path):
    """(header, payload) for each message of a `lamp-bus record` file."""
    out = []
    if not path.exists():
        return out
    for line in path.read_text(errors="replace").splitlines():
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if "header" not in row:
            continue
        try:
            payload = json.loads(base64.b64decode(row.get("payload_b64", "")))
        except (ValueError, TypeError):
            payload = None
        out.append((row["header"], payload))
    return out


def window(path):
    w = {}
    if path.exists():
        for line in path.read_text().splitlines():
            if "=" in line:
                k, v = line.split("=", 1)
                w[k.strip()] = v.strip()
    return w


def journal(path):
    """(unix seconds, text) for each journal line."""
    out = []
    if path.exists():
        for line in path.read_text(errors="replace").splitlines():
            head = line.split(" ", 1)
            try:
                out.append((float(head[0]), head[1] if len(head) > 1 else ""))
            except ValueError:
                continue
    return out


def hms(ms):
    return time.strftime("%H:%M:%S", time.localtime(ms / 1000))


def within(items, t0, t1):
    return [(h, p) for h, p in items if t0 <= h.get("at_unix_ms", 0) <= t1]


def faces_stats(faces):
    looks = near_looks = many = 0
    max_h = 0.0
    for _, p in faces:
        if not isinstance(p, dict):
            continue
        looks += 1
        hs = [f.get("ymax", 0) - f.get("ymin", 0) for f in p.get("faces", [])]
        if any(h >= NEAR for h in hs):
            near_looks += 1
        if len(hs) > 1:
            many += 1
        max_h = max([max_h] + hs)
    return looks, near_looks, many, max_h


def events_of(events, skill):
    return [(h, p) for h, p in events if isinstance(p, dict) and p.get("skill") == skill]


def said_lines(said):
    """{request: [(at_ms, text, finished)]} in arrival order."""
    lines = {}
    for h, p in said:
        if not isinstance(p, dict):
            continue
        lines.setdefault(p.get("request"), []).append(
            (h.get("at_unix_ms", 0), p.get("text", ""), bool(p.get("finished")))
        )
    return lines


def joined(pieces):
    """The whole line: every piece's text in order; a finished piece that
    repeats the whole (a route that says it whole at the end) is not added twice."""
    text = ""
    for _, piece, finished in pieces:
        if finished and piece.strip() and piece.strip() == text.strip():
            continue
        text += piece
    return text.strip()


def row3(d, rec, jl, out):
    w = window(d / "row3.window")
    if not w:
        return
    if not w.get("t1_ms"):
        out.append(f"ROW3 VOID: the row did not finish ({w.get('result', 'no end')})")
        return
    t0, t1 = int(w["t0_ms"]), int(w["t1_ms"])
    mode = w.get("mode", "?")
    looks, near_looks, _, max_h = faces_stats(within(rec["faces"], t0, t1))
    mutes = [p for _, p in within(rec["mute"], t0, t1) if isinstance(p, dict) and p.get("muted")]
    vstates = within(rec["vstate"], t0, t1)
    voids = []
    if looks == 0:
        voids.append("the eyes said nothing on humans/faces/tracked in the row")
    if near_looks:
        voids.append(f"a face at the desk in {near_looks} of {looks} looks (largest {max_h:.2f})")
    if mutes:
        voids.append("the voice was muted in the row")
    if not vstates:
        voids.append("no voice/state in the row: the voice was not there to speak")
    span = (t1 - t0) / 60000
    if mode == "fast":
        skipped = int(w.get("skipped_s", "0") or 0)
        how = f"fast: {skipped // 60} min skipped on lamp-skills' own clock in 60 s steps, {w.get('t0_local')}-{w.get('t1_local')} real"
    else:
        skipped = 0
        how = f"real: {w.get('t0_local')}-{w.get('t1_local')}, {span:.0f} min"
    need = {"comfort-break": 120, "meal-check": 105}  # for + wait, minutes
    said = said_lines(within(rec["said"], t0, t1))
    for skill in SKILLS:
        loaded = w.get("comfort_loaded" if skill == "comfort-break" else "meal_loaded") == "yes"
        if not loaded:
            tag = "OWED" if skill == "meal-check" else "VOID"
            out.append(f"ROW3 {skill} {tag}: not loaded on the lamp in the row" +
                       (" (installed through the app's Store Add first)" if skill == "meal-check" else ""))
            continue
        why = list(voids)
        covered = skipped / 60 if mode == "fast" else span
        if covered < need[skill]:
            why.append(f"only {covered:.0f} min of its clock passed, under its {need[skill]} min (for + wait)")
        if skill == "meal-check" and w.get("meal_window", "none") == "none":
            why.append("not inside a meal window (11:30-13:30, 18:30-20:30)")
        evs = events_of(within(rec["events"], t0, t1), skill)
        notrun = [t for s, t in jl if t0 / 1000 <= s <= t1 / 1000 + 1
                  and re.search(rf"lamp-skills: {re.escape(skill)} (not run|Triggered|Asked|Answered|Done)", t)]
        if evs or notrun:
            first = evs[0] if evs else None
            what = (f"{first[1].get('stage')} at {hms(first[0]['at_unix_ms'])}: {first[1].get('why', '')}"
                    if first else notrun[0].split("lamp-skills: ", 1)[-1])
            out.append(f"ROW3 {skill} FAIL ({how}): with nobody at the desk it still did something: {what}")
        elif why:
            out.append(f"ROW3 {skill} VOID ({how}): " + "; ".join(why))
        else:
            win = f", inside the {w['meal_window']} window" if skill == "meal-check" else ""
            out.append(
                f"ROW3 {skill} PASS ({how}{win}): nobody at the desk (0 faces at the desk in {looks} looks), "
                f"no Triggered, Asked or Done on skills/events, no not-run line, so no say; "
                f"the voice listening and not muted throughout"
            )
    for request, pieces in said.items():
        text = joined(pieces)
        if text:
            out.append(f"ROW3 NOTE something else was said in the row (request {request}, {hms(pieces[0][0])}): \"{text}\"")
    if not any(joined(p) for p in said.values()):
        out.append("ROW3 NOTE voice/said: nothing at all was said in the row")


def row5(d, rec, jl, out, show_pieces):
    w = window(d / "row5.window")
    if not w:
        return
    if not w.get("t1_ms"):
        out.append(f"ROW5 VOID: the row did not finish ({w.get('result', 'no end')})")
        return
    t0, t1 = int(w["t0_ms"]), int(w["t1_ms"])
    looks, near_looks, many, max_h = faces_stats(within(rec["faces"], t0, t1))
    seen = f"a face at the desk in {near_looks} of {looks} looks, 2+ faces in {many}"
    evs = events_of(within(rec["events"], t0, t1), "comfort-break")
    trig = next((e for e in evs if e[1].get("stage") == "triggered"), None)
    done = next((e for e in evs if e[1].get("stage") == "done" and (not trig or e[0]["at_unix_ms"] >= trig[0]["at_unix_ms"])), None)
    if not trig:
        mins = (t1 - t0) / 60000
        if looks == 0 or near_looks < 0.9 * looks:
            out.append(f"ROW5 comfort-break VOID: no Triggered in {mins:.0f} min, and the face was not held at the desk ({seen})")
        else:
            out.append(f"ROW5 comfort-break FAIL: no Triggered in {mins:.0f} min with a face at the desk ({seen})")
        return
    t_ms = trig[0]["at_unix_ms"]
    if not done:
        out.append(f"ROW5 comfort-break FAIL: Triggered {hms(t_ms)} ({trig[1].get('why')}) but no Done in the row")
        return
    d_ms = done[0]["at_unix_ms"]
    dwhy = done[1].get("why", "")
    said = said_lines(within(rec["said"], t_ms - 2000, d_ms + 60000))
    # The line's request: the first one whose first piece came after the trigger.
    cands = sorted((p[0][0], r) for r, p in said.items() if p and p[0][0] >= t_ms - 2000 and r is not None)
    if "spoken" not in dwhy:
        out.append(f"ROW5 comfort-break FAIL: Triggered {hms(t_ms)}, Done {hms(d_ms)} says \"{dwhy}\", not spoken")
        return
    if not cands:
        out.append(f"ROW5 comfort-break WORDS FAIL: Done {hms(d_ms)} says spoken, but voice/said carried no piece after {hms(t_ms)}")
        return
    request = cands[0][1]
    pieces = said[request]
    text = joined(pieces)
    words = [x for x in pieces if x[1].strip()]
    if not text:
        out.append(f"ROW5 comfort-break WORDS FAIL: request {request} had {len(pieces)} pieces, all empty")
        return
    out.append(
        f"ROW5 comfort-break PASS: Triggered {hms(t_ms)} ({trig[1].get('why')}), Done {hms(d_ms)} ({dwhy}); "
        f"request {request}, {len(text)} chars in {len(words)} pieces (+{len(pieces) - len(words)} empty end piece): \"{text}\""
    )
    bad = NEVER.findall(text)
    sentences = len([s for s in re.split(r"[.!?]+", text) if s.strip()])
    if bad:
        out.append(f"ROW5 RULE8 CHECK: the line names {sorted(set(b.lower() for b in bad))}: read it before posting")
    else:
        out.append(f"ROW5 RULE8 ok: {sentences} sentence(s); names no bathroom, toilet or health word (list in kit5754.py)")
    out.append(f"ROW5 NOTE faces in the row: {seen}; largest {max_h:.2f}")
    if show_pieces:
        for at, piece, fin in pieces:
            out.append(f"  piece {hms(at)} finished={str(fin).lower()} {json.dumps(piece)}")


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    d = Path(sys.argv[1])
    show = "--pieces" in sys.argv[2:]
    r = d / "rec"
    rec = {
        "events": records(r / "skills_events.jsonl"),
        "said": records(r / "voice_said.jsonl"),
        "vstate": records(r / "voice_state.jsonl"),
        "mute": records(r / "voice_mute.jsonl"),
        "faces": records(r / "humans_faces_tracked.jsonl"),
    }
    jl = journal(d / "logs" / "journal.txt")
    out = []
    row3(d, rec, jl, out)
    row5(d, rec, jl, out, show)
    pb = d / "logs" / "putback.out"
    if pb.exists():
        last = [x for x in pb.read_text().splitlines() if x.startswith("PUTBACK-")]
        if last:
            out.append(last[-1])
    print("\n".join(out) if out else "no row ran")


if __name__ == "__main__":
    main()
