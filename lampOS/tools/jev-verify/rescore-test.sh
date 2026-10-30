#!/usr/bin/env bash
# rescore-test.sh: rescore.py on two made-up decisions: a desk-talk line with the name listener on and
# the name not heard (WITH_NAME weights, quiet room), and the same with no name listener, busy room, in
# conversation (plain weights). Each probability must be the hand-computed sum; with quiet 0 the first
# drops to stay_quiet; a hello decision is copied as it is.
set -u
cd "$(dirname "$0")" || exit 1
T=$(mktemp -d) || exit 1; trap 'rm -rf "$T"' EXIT
mkdir "$T/run"
A='"answers":{"calls_lamp":0.05,"names_someone_else":0.04,"others_business":0.9,"lamp_could_answer":0.3,"follows_lamp":0.1,"listening_noise":0.04,"asks_to_stop":0.02}'
{ echo "{\"about\":\"attention\",$A,\"context\":{\"heard\":\"SECRET\",\"room\":\"quiet\",\"name_heard\":0.0},\"in_conversation\":false,\"judgement\":{\"probability\":0,\"choice\":\"answer\",\"from_rule\":false}}"
  echo "{\"about\":\"attention\",$A,\"context\":{\"heard\":\"SECRET\",\"room\":\"busy\"},\"in_conversation\":true,\"judgement\":{\"probability\":0,\"choice\":\"answer\",\"from_rule\":false}}"
  echo '{"about":"hello","judgement":{"probability":0.2,"choice":"wait"}}'; } > "$T/run/decisions.jsonl"
python3 rescore.py "$T/run" "$T/a" 2>/dev/null; echo '{"with_name":{"quiet":0}}' > "$T/w.json"; python3 rescore.py "$T/run" "$T/b" "$T/w.json" 2>/dev/null
python3 - "$T" <<'PY'
import json, math, sys
t = sys.argv[1]
a = [json.loads(l) for l in open(t + "/a/decisions.jsonl")]
b = [json.loads(l) for l in open(t + "/b/decisions.jsonl")]
x = [0.05, 0.04, 0.9, 0.3, 0.1, 0.04, 0.02]
wn = [0.540, -0.634, -1.704, 0.439, 0.104, -0.588, 0.038]; wp = [0.722, -0.249, -2.042, 1.132, -0.136, -0.645, 0.031]
p1 = 1 / (1 + math.exp(-(-0.328 + sum(i * j for i, j in zip(wn, x)) + 1.683)))
p2 = 1 / (1 + math.exp(-(0.263 + sum(i * j for i, j in zip(wp, x)) - 0.206)))
ok = abs(a[0]["judgement"]["probability"] - p1) < 1e-9 and abs(a[1]["judgement"]["probability"] - p2) < 1e-9
ok &= a[0]["judgement"]["choice"] == ("look_up" if p1 >= 0.4 else "stay_quiet")
ok &= b[0]["judgement"]["choice"] == "stay_quiet" and a[2] == b[2] and a[2]["about"] == "hello"
print("PASS" if ok else f"FAIL {a} {b}")
PY
