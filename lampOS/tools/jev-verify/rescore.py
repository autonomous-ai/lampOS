#!/usr/bin/env python3
"""rescore.py RUN_DIR NEW_DIR [WEIGHTS.json]: re-judges a replay.sh run's attention decisions offline.

Every decision in RUN_DIR/decisions.jsonl keeps the model's seven answers and its context. This
recomputes each probability and choice with other weights and bands, the way lamp-policy's combine
does (crates/policy/src/attention.rs), and writes a copy of the run to NEW_DIR. Then run
`score.sh NEW_DIR` for the table. With no WEIGHTS.json it uses the shipped values, so the copy must
score exactly like the run (a check that this follows lamp-policy).

WEIGHTS.json overrides any of the keys of SHIPPED below. A changed question is not something this can
score: its answers are not in the log.
"""
import json
import math
import shutil
import sys

ORDER = ["calls_lamp", "names_someone_else", "others_business", "lamp_could_answer",
         "follows_lamp", "listening_noise", "asks_to_stop"]
SHIPPED = {
    # ASKED, BIAS, QUIET, IN_CONVERSATION (no name listener on the lamp)
    "plain": {"weights": [0.722, -0.249, -2.042, 1.132, -0.136, -0.645, 0.031],
              "bias": 0.263, "quiet": 1.771, "in_conversation": -0.206},
    # WITH_NAME (a name listener runs: context.name_heard is there)
    "with_name": {"weights": [0.540, -0.634, -1.704, 0.439, 0.104, -0.588, 0.038],
                  "bias": -0.328, "quiet": 1.683, "in_conversation": 0.081, "name": 2.346},
    "name_heard_from": 0.9, "answer_from": 0.8, "look_up_from": 0.4,
}


def judge(d, w):
    a, c = d["answers"], d["context"]
    named = "name_heard" in c
    s = w["with_name"] if named else w["plain"]
    total = s["bias"] + sum(k * a[q] for k, q in zip(s["weights"], ORDER))
    total += s["quiet"] if c["room"] == "quiet" else 0.0
    total += s["in_conversation"] if d.get("in_conversation") else 0.0
    if named and (c["name_heard"] or 0.0) >= w["name_heard_from"]:
        total += s["name"]
    p = 1.0 / (1.0 + math.exp(-total))
    choice = "answer" if p >= w["answer_from"] else "look_up" if p >= w["look_up_from"] else "stay_quiet"
    return p, choice


def main(argv):
    if len(argv) not in (3, 4):
        sys.exit(__doc__.split("\n\n")[0])
    run, new = argv[1], argv[2]
    w = json.loads(json.dumps(SHIPPED))
    if len(argv) == 4:
        with open(argv[3]) as f:
            for k, v in json.load(f).items():
                if isinstance(v, dict):
                    w[k].update(v)
                else:
                    w[k] = v
    shutil.copytree(run, new)
    n = changed = 0
    with open(run + "/decisions.jsonl") as f, open(new + "/decisions.jsonl", "w") as out:
        for line in f:
            d = json.loads(line)
            if (d.get("about") == "attention" and "answers" in d and d.get("failed") is None
                    and not d.get("judgement", {}).get("from_rule")):
                p, choice = judge(d, w)
                old = d["judgement"]["choice"]
                d["judgement"] = {"probability": p, "choice": choice, "from_rule": False}
                n += 1
                changed += choice != old
            out.write(json.dumps(d) + "\n")
    print(f"rescored {n} attention decisions, {changed} changed choice", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv)
