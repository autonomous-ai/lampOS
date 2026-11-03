# gen-build-rows.py BUILD_PY OUT_MD LABELS_JSON PRIVATE_NAMES: lampOS/docs/build-rows.md from the review page's tables.
import re, sys, json, datetime
src = open(sys.argv[1]).read(); ns = {}
exec(src[src.index("PROGRESS = ["):src.index("BRAIN_LOG = [")], ns)
labels = json.load(open(sys.argv[3]))
# 3 Nov 15:4x (the Reviewer-lead's P3 on #5068): a `builder:*` label can read like a person's name, and build-rows-pr.sh's
# grep of the whole file can miss it inside a label. A label whose name any private-names.txt rule matches (one
# extended regex a line, case-insensitive, # lines ignored) is left out of the Owners column. The file is required.
try:
    PRIVATE = [re.compile(l.strip(), re.I) for l in open(sys.argv[4]) if l.strip() and not l.lstrip().startswith("#")]
except (IndexError, OSError, re.error) as e:
    sys.exit(f"gen-build-rows: private names (argv[4]) unreadable: {e}")
if not PRIVATE: sys.exit("gen-build-rows: no private names: stop")
def private(label): return any(r.search(label) for r in PRIVATE)
B, P = ns["BUILD"], ns["PROGRESS"]
def first(note, n=220):
    note = re.sub(r"\s+", " ", note).strip()
    if not note: return ""
    m = re.match(r"(.+?[.;])(\s|$)", note)
    s = m.group(1) if m else note
    return (s[:n] + "…") if len(s) > n else s
# Builders who are down: their labels no longer name a live owner (the lead, 21 Oct 07:4x on #1711).
DOWN = set()
# Each row under 1.0 has a live owner (the lead, 21 Oct 07:4x): set by the first builder where the issues' labels name
# nobody live, by the start of the row's name. These come first, then the labels' live builders.
OWNERS = {
    "A Skills Store": ["fourteen"],
    "Updates over the internet": ["migrate", "second"],
    "Updates cover the whole lamp": ["migrate", "tests"],
    "An owner makes their own skill": ["sixth"],
    "Remembers what matters without being asked": ["friday"],
    "Change the Wi-Fi, name or time zone": ["fourteen"],
    "A light for booting": ["third"],
    "Reads your mood from your face or voice": ["ten", "fourth"],
    "Email, calendar and other connectors": ["eighth", "el"],
    "The factory’s per-lamp steps": ["third"],
    "Guard mode": ["jarvis"],
    "Music to relax": ["second", "sixth"],
    "Faces from a photo": ["mobile", "ten"],
    "“I’m unplugging you”": ["fifth"],
}
# The mood rows stay in V2 but out of the build figure, scored on their own line for later; no day slot or new work goes
# into them until the other rows reach 100% (the lead, 23 Oct 12:1x on #1711). On 22 Oct the first builder had kept
# them in the figure as rows that aren't ship gates.
NOT_A_GATE = ("One inner state (mood)", "Reads your mood from your face or voice", "Music matched to your mood")
def owners(refs, name=""):
    out = [o for key, given in OWNERS.items() if name.startswith(key) for o in given]
    for n in re.findall(r"#(\d+)", refs):
        for l in labels.get(n, []):
            if l.startswith("builder:") and l[8:] not in out and l[8:] not in DOWN and not private(l[8:]): out.append(l[8:])
    return ", ".join(o for o in out if not private(o)) or "—"
# The page is written to the owner; the repo is public and names roles only.
PUBLIC = [
    (r"^You, ", "The owner, "),
    (r"after your report that the lamp forgot your name: you said it while your face wasn’t recognised, so it was kept in the room’s memory, not yours",
     "after the owner’s report that the lamp forgot their name: they said it while their face wasn’t recognised, so it was kept in the room’s memory, not theirs"),
    (r"\byour report\b", "the owner’s report"),
]
def public(s):
    for pattern, to in PUBLIC: s = re.sub(pattern, to, s)
    return s
def cell(s): return public(s).replace("|", "\\|")
MVP = [b for b in B if not b[0].startswith(NOT_A_GATE)]; LATER = [b for b in B if b[0].startswith(NOT_A_GATE)]
build = 100 * sum(b[2] for b in MVP) / len(MVP); later = 100 * sum(b[2] for b in LATER) / len(LATER); quality = 100 * sum(p[1] for p in P) / len(P)
now = datetime.datetime.now().strftime("%Y-%m-%d %H:%M")
L = [
 "# Where V2 is: the build rows and the quality rows", "",
 f"Written by the first builder from the team's review page at {now} (+07). The lead asked for it on #1711 (20 Oct, 16:43), so anyone can read where V2 is without waiting for the hourly post. It's a copy, updated a few times a day. The hourly figures on #1722 are the live ones.", "",
 f"**Build {build:.1f}% over {len(MVP)} rows. Quality {quality:.1f}% over {len(P)} rows.** Mood, for later: {later:.1f}% over {len(LATER)} rows, not in the build figure.", "",
 "**How a build row is counted** (the lead's rule): 1.0 when its work is merged and tried on a lamp; less while parts are missing or untried. Build is the mean of the rows. Quality rows score how well a part works today, against a target; quality is the mean of the scores.", "",
 "**Mood, for later:** the mood rows stay in V2 but out of the build figure, which counts the rows that ship. No day slot or new work goes into them until those reach 100% (the lead, 23 Oct on #1711). On 22 Oct the first builder had kept them in the figure as rows that aren't ship gates.", "",
 "## Build rows", "",
 "| Row | Score | Issues | Owners | Where it is |", "|---|---|---|---|---|",
]
for name, refs, v, note in B:
    gate = " *(mood, for later: not in the build figure)*" if name.startswith(NOT_A_GATE) else ""
    L.append(f"| {cell(name)}{gate} | {v:.2f} | {refs or '—'} | {owners(refs, name)} | {cell(first(note))} |")
L += ["", "## Quality rows", "", "| Row | Now | Target | Where it is |", "|---|---|---|---|"]
for name, v, t, note in P:
    L.append(f"| {cell(name)} | {v:.2f} | {t:.2f} | {cell(first(note))} |")
L.append("")
open(sys.argv[2], "w").write("\n".join(L))
print(f"build {build:.1f} quality {quality:.1f} rows {len(MVP)} {len(P)} mood {later:.1f} over {len(LATER)}")
