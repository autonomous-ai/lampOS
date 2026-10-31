#!/usr/bin/env bash
# Builds the canned lamp events of row-cutin-words.sh's DRY matrix (plan-cutin-words.md): one dir per scenario, one
# events file per arm and row (PC, PO, WC, WO), on the kit's DRY clock (the ask's #PLAY_US 100000000000; the first answer
# audio 6 s on; row C's line at +10 s, row O's end line at +96 s). No lamp. make.sh OUTDIR (matrix.sh: $M/cases).
set -euo pipefail
mkdir -p "${1:?usage: make.sh OUTDIR}"; cd "$1"
python3 -I - <<'PY'
import json, os
ASK = 100_000_000_000; S = 1_000_000
OWN = {"boot": [1], "generation": 1, "turn": 1}
def first(): return [{"kind": "speaker_first_write", "at_us": ASK + 6 * S, "owner": OWN}]
def yld(at, end, how, held=0, words=None, finish=None):
    """A yield at `at` ended at `end` by `how` (commit|resume), with stop_words `words` (None: no record)."""
    r = [{"kind": "duplex_yield", "at_us": at, "owner": OWN, "frame_sequence": 1}]
    if words is not None: r.append({"kind": "stop_words", "at_us": at + 200_000, "stop": False, "words": words, "backchannel": False})
    r.append({"kind": "duplex_" + how, "at_us": end, "owner": OWN, "held_ms": held, "backchannel": None})
    r.append({"kind": "duplex_yield_end", "at_us": end, "started_at_us": at, "owner": OWN,
              "outcome": "commit_decision" if how == "commit" else "resumed"})
    if how == "commit" or finish: r.append({"kind": "turn_finished", "at_us": end + 10_000, "turn": 1, "outcome": finish or "user_interrupted"})
    return r
LINE = ASK + 10 * S
def cut(onset_ms, how="commit", held=0, words=None): return first() + yld(LINE + onset_ms * 1000, LINE + onset_ms * 1000 + 600_000 + held * 1000, how, held, words)
own_commit = first() + yld(ASK + 40 * S, ASK + 40 * S + 600_000, "commit")             # the 4ace case: a wordless commit
own_held = first() + yld(ASK + 40 * S, ASK + 40 * S + 900_000, "resume", 500, words=0)  # held for words, 0 came, resumed
quiet = first()
P = {"PC": cut(400), "PO": own_commit}
W = {"WC": cut(420, held=300, words=1), "WO": own_held}
cases = {
    "pass": {**P, **W},
    "norepro": {**P, "PO": quiet, **W},
    "wordless": {**P, **W, "WO": own_commit},
    "missed": {**P, **W, "WC": cut(420, "resume", 500, words=0)},
    "slowyield": {**P, **W, "WC": cut(700, held=300, words=1)},
    "void": {**P, **W, "WC": []},
}
for name, files in cases.items():
    os.makedirs(name, exist_ok=True)
    for f, rows in files.items():
        with open("%s/%s.jsonl" % (name, f), "w") as o:
            o.write("".join(json.dumps(r, separators=(",", ":"), sort_keys=True) + "\n" for r in rows))
# story_done's held stream in pass: an old turn and the story's own end
with open("pass/feed.jsonl", "w") as o:
    o.write('{"at_us":%d,"kind":"turn_finished","outcome":"old_turn"}\n{"at_us":%d,"kind":"turn_finished","outcome":"user_interrupted"}\n' % (ASK - S, ASK + 11 * S))
PY
