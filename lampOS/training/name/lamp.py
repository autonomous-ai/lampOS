"""The name, trained on the lamp's own microphone (docs/bare-name.md, "On the lamp's own recordings").

Generated speech heard the bare "Lamp" in 86 % of English calls off the lamp, and on lamp-4ace's
mic2, through its speaker and canceller, in one voice of three (#705, 18 Oct: Daniel's two bare
calls at 0.82 and 0.57, Samantha's and Karen's four under 0.30). So the next model learns from
the lamp's own recordings of played voices, and is judged only on voices it never heard.

Three commands, run on a laptop or the office Mac, never on the lamp:

    python lamp.py lines VOICES.txt > plays.txt
        The play list for the scenario player, `Voice|text|seconds`, from `say -v '?'`'s output:
        English voices only, calls and negatives spread over them, about 15 minutes in all.

    python lamp.py train --front DIR --plays PLAYED.tsv RECORD.jsonl... [--negatives RECORD.jsonl...]
                         [--compare NAME.onnx...] [--folds 3] [--out name.onnx]
        DIR holds the two fixed models (the installed melspectrogram.onnx and embedding_model.onnx).
        PLAYED.tsv: one line a played line, `UNIX_MS<TAB>VOICE<TAB>KIND<TAB>TEXT`, KIND one of
        bare, hey, oi (calls) or near, mid, other (not calls); UNIX_MS when it began to play.
        RECORD.jsonl: `lamp-bus record DIR mic2/audio` while they played (its messages carry
        their own times, so no anchor is needed). --negatives: recordings with no call at all
        (an empty office, the lamp telling stories), every step a negative.
        It trains `--folds` models, each on the voices of the other folds, and scores each call
        only by the model that never heard its voice. Then one model on every voice, `--out`.
        --compare scores the same steps with other name models (the installed one, the bare-name
        one) for a table side by side.

    python lamp.py dry-run --front DIR WAV...
        The same pipeline on speech files and made-up plays, to show it runs end to end; its
        numbers mean nothing.

The model is `train.py`'s "flat" shape (Flatten, Gemm, Relu, Gemm, Relu, Gemm, Sigmoid) on
`[1, 16, 96]`, the scaling folded into the first layer, so `hear.rs` runs it as `name.onnx`.
It is fitted with scikit-learn's MLP (no torch). Nothing of a person: played voices only.
"""

from __future__ import annotations

import argparse
import base64
import json
import random
import sys
from pathlib import Path

import numpy as np

import shared  # noqa: F401  (training/stop on the path)
from features import FrontEnd, score_time, stacks
from hearing import hearings

RATE = 16_000
CALLS = {"bare", "hey", "oi"}
KINDS = CALLS | {"near", "mid", "other"}
# A call's steps that are its own: a score at window end from 0.35 s to 1.6 s after the line began
# holds the name (a bare "Lamp" ends about 0.4 s in, "Hey Lamp" about 0.8 s). The rest of the call,
# to 4 s, is neither: its own follow-on words, and the lamp's answer.
CALL_FROM, CALL_TO, CALL_AFTER = 0.35, 1.6, 4.0
LEASTS = (0.5, 0.9, 0.98)

FOLLOW_ONS = [
    "what time is it?", "are you there?", "make the light a little warmer.", "tell me a joke.",
    "how are you?", "what's the weather like?", "turn it down a bit.", "set a timer for five minutes.",
    "can you hear me?", "good morning.",
]
NEGATIVES = {
    "mid": ["Turn on the lamp in the hall.", "That lamp is new.", "My lamp needs a bulb.",
            "Leave the lamp on tonight.", "The lamp by the window is broken."],
    "near": ["The lamb was lovely.", "Meet me at the lab.", "We went to camp last summer.",
             "Pass me the clamp.", "Lan, are you coming?", "Sam, what time is it?"],
}
VIETNAMESE = ["Lamp ơi, mấy giờ rồi?", "Lamp ơi, bật đèn lên.", "Lamp ơi, kể chuyện cười đi."]


def lines(voices_file: str) -> None:
    """The play list: per English voice, 5 bare calls and 3 "Hey Lamp" (7 s each, room for the
    lamp's answer) and 3 negatives (5 s); a Vietnamese voice says "Lamp ơi" if there is one."""
    english, vietnamese = [], []
    for row in open(voices_file, encoding="utf-8"):
        parts = row.split()
        if len(parts) < 2:
            continue
        # `say -v '?'`: NAME  LOCALE  # sample; a name may hold spaces, the locale is xx_YY.
        locale = next((p for p in parts if len(p) == 5 and p[2] == "_"), "")
        name = row.split(locale)[0].strip() if locale else ""
        if locale.startswith("en_") and name:
            english.append(name)
        elif locale.startswith("vi_") and name:
            vietnamese.append(name)
    rng = random.Random(0)
    chosen = english[:12]
    out = []
    for n, voice in enumerate(chosen):
        follow = FOLLOW_ONS[n % len(FOLLOW_ONS):] + FOLLOW_ONS[: n % len(FOLLOW_ONS)]
        out += [(voice, f"Lamp, {f}", 7, "bare") for f in follow[:5]]
        out += [(voice, f"Hey Lamp, {f}", 7, "hey") for f in follow[5:8]]
        out += [(voice, rng.choice(NEGATIVES["mid"]), 5, "mid"), (voice, rng.choice(NEGATIVES["near"]), 5, "near"),
                (voice, rng.choice(NEGATIVES["near"]), 5, "near")]
    for voice in vietnamese[:1]:
        out += [(voice, line, 7, "oi") for line in VIETNAMESE]
    rng.shuffle(out)
    total = sum(seconds for _, _, seconds, _ in out)
    print(f"# {len(chosen)} English voices, {len(vietnamese[:1])} Vietnamese; {len(out)} lines, {total / 60:.1f} min")
    print("# Voice|text|seconds; the kind of each line is in the comment after it, for PLAYED.tsv")
    for voice, text, seconds, kind in out:
        print(f"{voice}|{text}|{seconds}  # {kind}")


def recording(path: str) -> tuple[np.ndarray, float]:
    """A `lamp-bus record` of mic2/audio: its samples (a lost block as silence) and the unix time of
    the first sample."""
    blocks, first_ms, last_seq = [], None, None
    for row in open(path, encoding="utf-8"):
        row = json.loads(row)
        if "payload_b64" not in row:
            continue
        payload = base64.b64decode(row["payload_b64"])
        head, _, raw = payload.partition(b"\n")
        header = json.loads(head)
        block = np.frombuffer(raw, "<i2")
        if first_ms is None:
            first_ms = float(row["header"].get("at_unix_ms", 0))
            first_ms -= 1000.0 * len(block) / RATE
        if last_seq is not None and header["sequence"] > last_seq + 1:
            blocks.append(np.zeros((header["sequence"] - last_seq - 1) * len(block), np.int16))
        last_seq = header["sequence"]
        blocks.append(block)
    return (np.concatenate(blocks) if blocks else np.zeros(0, np.int16)), (first_ms or 0.0) / 1000.0


def played(path: str) -> list[tuple[float, str, str]]:
    """(unix seconds, voice, kind) of each played line."""
    out = []
    for row in open(path, encoding="utf-8"):
        if not row.strip() or row.startswith("#"):
            continue
        at, voice, kind = row.rstrip("\n").split("\t")[:3]
        if kind not in KINDS:
            raise SystemExit(f"{path}: kind {kind!r} is none of {sorted(KINDS)}")
        out.append((float(at) / 1000.0, voice, kind))
    return out


def label(times: np.ndarray, plays: list[tuple[float, str, str]]):
    """For each step (its score's unix time): 1 a call's own step, 0 a negative, -1 neither; and the
    index of the call or line it belongs to (-1 none)."""
    y = np.zeros(len(times), np.int8)
    which = np.full(len(times), -1)
    for i, (at, _, kind) in enumerate(plays):
        after = times - at
        inside = (after >= -0.3) & (after < CALL_AFTER)
        which[inside] = i
        if kind in CALLS:
            y[inside] = -1
            y[(after >= CALL_FROM) & (after <= CALL_TO)] = 1
    return y, which


def steps_of(front: FrontEnd, samples: np.ndarray, start: float) -> tuple[np.ndarray, np.ndarray]:
    """Every model input of a recording and its score's unix time."""
    x = stacks(front.of(samples.astype(np.float32)))
    return x, start + np.array([score_time(j) for j in range(len(x))])


class Flat:
    """train.py's "flat" listener, fitted with scikit-learn's MLP."""

    def __init__(self, seed: int = 0):
        from sklearn.neural_network import MLPClassifier

        self.mlp = MLPClassifier(hidden_layer_sizes=(64, 64), alpha=1e-3, max_iter=60,
                                 early_stopping=False, random_state=seed)

    def fit(self, x: np.ndarray, y: np.ndarray, against: float = 3.0) -> "Flat":
        flat = x.reshape(len(x), -1)
        self.mean = flat.mean(axis=0)
        self.scale = flat.std(axis=0) + 1e-3
        pos, neg = np.flatnonzero(y == 1), np.flatnonzero(y == 0)
        # Balance: each call's steps as often as the negatives, weighed `against` to 1.
        rng = np.random.default_rng(0)
        reps = max(1, int(len(neg) / max(1, len(pos)) / against))
        idx = np.concatenate([np.repeat(pos, reps), neg])
        rng.shuffle(idx)
        self.mlp.fit((flat[idx] - self.mean) / self.scale, y[idx])
        return self

    def scores(self, x: np.ndarray) -> np.ndarray:
        return self.mlp.predict_proba((x.reshape(len(x), -1) - self.mean) / self.scale)[:, 1]

    def export(self, path: Path) -> None:
        import onnx
        from onnx import TensorProto, helper, numpy_helper

        (w1, w2, w3), (b1, b2, b3) = self.mlp.coefs_, self.mlp.intercepts_
        w1 = w1 / self.scale[:, None]
        b1 = b1 - self.mean @ w1
        weights = {"w1": w1.T, "b1": b1, "w2": w2.T, "b2": b2, "w3": w3.T, "b3": b3}
        inits = [numpy_helper.from_array(np.asarray(v, np.float32), k) for k, v in weights.items()]
        nodes = [
            helper.make_node("Flatten", ["input"], ["flat"], axis=1),
            helper.make_node("Gemm", ["flat", "w1", "b1"], ["h1"], transB=1),
            helper.make_node("Relu", ["h1"], ["r1"]),
            helper.make_node("Gemm", ["r1", "w2", "b2"], ["h2"], transB=1),
            helper.make_node("Relu", ["h2"], ["r2"]),
            helper.make_node("Gemm", ["r2", "w3", "b3"], ["h3"], transB=1),
            helper.make_node("Sigmoid", ["h3"], ["output"]),
        ]
        graph = helper.make_graph(
            nodes, "name",
            [helper.make_tensor_value_info("input", TensorProto.FLOAT, [1, 16, 96])],
            [helper.make_tensor_value_info("output", TensorProto.FLOAT, [1, 1])],
            inits,
        )
        model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 13)], producer_name="lampOS training/name lamp.py")
        model.ir_version = 8
        onnx.checker.check_model(model)
        path.parent.mkdir(parents=True, exist_ok=True)
        onnx.save(model, str(path))


def onnx_scores(path: str, x: np.ndarray) -> np.ndarray:
    import onnxruntime as ort

    session = ort.InferenceSession(path, providers=["CPUExecutionProvider"])
    name = session.get_inputs()[0].name
    return np.array([session.run(None, {name: x[i : i + 1].astype(np.float32)})[0].item() for i in range(len(x))])


def report(title: str, scores: np.ndarray, y: np.ndarray, which: np.ndarray, plays, negative_hours: float, neg_scores) -> None:
    """Per voice: the share of calls whose highest own-step score reaches each bar; then hearings an
    hour on the negatives (the played negatives and --negatives), by `said.rs`'s rule."""
    print(f"\n{title}")
    voices = sorted({voice for _, voice, kind in plays if kind in CALLS})
    print(f"  {'voice':18} {'calls':>5}  " + "  ".join(f"heard@{least}" for least in LEASTS) + "   best scores")
    total = {least: [0, 0] for least in LEASTS}
    for voice in voices:
        best = []
        for i, (_, v, kind) in enumerate(plays):
            if v == voice and kind in CALLS:
                own = (which == i) & (y == 1)
                best.append(float(scores[own].max()) if own.any() else float("nan"))
        best = np.array(best)
        shares = []
        for least in LEASTS:
            heard = int(np.nansum(best >= least))
            total[least][0] += heard
            total[least][1] += len(best)
            shares.append(f"{heard:>2}/{len(best):<5}")
        print(f"  {voice[:18]:18} {len(best):>5}  " + "  ".join(f"{s:>9}" for s in shares) + "   " + " ".join(f"{b:.2f}" for b in best))
    print("  all" + " " * 21 + "  ".join(f"{t[0]:>2}/{t[1]:<5}" for t in total.values()))
    for least in LEASTS:
        print(f"  false hearings at {least}: {len(hearings(neg_scores, least)) / max(negative_hours, 1e-9):.1f} an hour of negatives ({negative_hours * 60:.1f} min)")


def train(args) -> None:
    front = FrontEnd(Path(args.front))
    plays = played(args.plays)
    xs, ys, whichs, offset = [], [], [], 0
    for path in args.records:
        samples, start = recording(path)
        x, times = steps_of(front, samples, start)
        y, which = label(times, plays)
        xs.append(x), ys.append(y), whichs.append(np.where(which >= 0, which, -1))
    x, y, which = np.concatenate(xs), np.concatenate(ys), np.concatenate(whichs)
    neg_x = [x[(y == 0)]]
    for path in args.negatives:
        samples, start = recording(path)
        neg_x.append(steps_of(front, samples, start)[0])
    extra = np.concatenate(neg_x[1:]) if len(neg_x) > 1 else np.zeros((0, 16, 96), np.float32)
    voice_of = np.array([plays[i][1] if i >= 0 else "" for i in which])
    voices = sorted({voice for _, voice, kind in plays if kind in CALLS})
    folds = [voices[k :: args.folds] for k in range(args.folds)]
    print(f"{len(x)} steps from {len(args.records)} recording(s): {int((y == 1).sum())} call steps of "
          f"{sum(kind in CALLS for _, _, kind in plays)} calls, {int((y == 0).sum())} negative; "
          f"{len(extra)} more negative steps; {len(voices)} voices in {args.folds} folds: {folds}")
    held = np.zeros(len(x))
    held_extra = np.zeros(len(extra))
    for fold in folds:
        out = np.isin(voice_of, fold)
        model = Flat(args.seed).fit(np.concatenate([x[~out & (y >= 0)], extra]),
                                    np.concatenate([y[~out & (y >= 0)], np.zeros(len(extra), np.int8)]))
        held[out] = model.scores(x[out])
        # A negative of no voice (a gap, or --negatives) is scored by each fold's model in turn;
        # its worst score counts.
        free = voice_of == ""
        held[free] = np.maximum(held[free], model.scores(x[free]))
        if len(extra):
            held_extra = np.maximum(held_extra, model.scores(extra))
    neg_scores = np.concatenate([held[y == 0], held_extra])
    hours = len(neg_scores) * 0.08 / 3600
    report("Trained on the lamp's recordings, each voice scored by a model that never heard it:", held, y, which, plays, hours, neg_scores)
    for path in args.compare:
        s = onnx_scores(path, x)
        se = onnx_scores(path, extra) if len(extra) else np.zeros(0)
        report(f"{path}, on the same steps:", s, y, which, plays, hours, np.concatenate([s[y == 0], se]))
    final = Flat(args.seed).fit(np.concatenate([x[y >= 0], extra]), np.concatenate([y[y >= 0], np.zeros(len(extra), np.int8)]))
    final.export(Path(args.out))
    check = onnx_scores(args.out, x[:200])
    print(f"\nwrote {args.out}: one model on every voice; ONNX against the fit, largest difference "
          f"{np.abs(check - final.scores(x[:200])).max():.2e}")


def dry_run(args) -> None:
    """Speech files stand in for a recording, and made-up plays for its calls: the pipeline end to
    end, numbers meaningless."""
    import tempfile
    import wave

    folder = Path(tempfile.mkdtemp(prefix="lamp-name-dry-"))
    rng = np.random.default_rng(0)
    record, played_tsv = folder / "mic2_audio.jsonl", folder / "played.tsv"
    at_ms, sequence = 1_700_000_000_000.0, 0
    with open(record, "w", encoding="utf-8") as out, open(played_tsv, "w", encoding="utf-8") as plays:
        for n, path in enumerate(args.wavs * 4):
            with wave.open(path, "rb") as source:
                speech = np.frombuffer(source.readframes(source.getnframes()), "<i2")
            quiet = rng.normal(0, 20, RATE * 2).astype(np.int16)
            kind = ["bare", "hey", "near", "bare", "mid", "hey"][n % 6]
            plays.write(f"{at_ms + 2000:.0f}\tvoice{n % 6}\t{kind}\tdry run\n")
            samples = np.concatenate([quiet, speech])
            for i in range(0, len(samples) - 319, 320):
                head = json.dumps({"sample_rate": RATE, "channels": 1, "sample_format": "S16LE", "sequence": sequence}).encode()
                at_ms += 20.0
                block = base64.b64encode(head + b"\n" + samples[i : i + 320].astype("<i2").tobytes()).decode()
                out.write(json.dumps({"header": {"at_unix_ms": at_ms}, "payload_b64": block}) + "\n")
                sequence += 1
    train(argparse.Namespace(front=args.front, plays=str(played_tsv), records=[str(record)], negatives=[],
                             compare=args.compare, folds=3, out=str(folder / "name.onnx"), seed=0))
    print(f"(dry run in {folder})")


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="command", required=True)
    s = sub.add_parser("lines")
    s.add_argument("voices")
    s = sub.add_parser("train")
    s.add_argument("--front", required=True)
    s.add_argument("--plays", required=True)
    s.add_argument("records", nargs="+")
    s.add_argument("--negatives", nargs="*", default=[])
    s.add_argument("--compare", nargs="*", default=[])
    s.add_argument("--folds", type=int, default=3)
    s.add_argument("--out", default="name-lamp.onnx")
    s.add_argument("--seed", type=int, default=0)
    s = sub.add_parser("dry-run")
    s.add_argument("--front", required=True)
    s.add_argument("--compare", nargs="*", default=[])
    s.add_argument("wavs", nargs="+")
    args = p.parse_args()
    {"lines": lambda: lines(args.voices), "train": lambda: train(args), "dry-run": lambda: dry_run(args)}[args.command]()


if __name__ == "__main__":
    sys.exit(main())
