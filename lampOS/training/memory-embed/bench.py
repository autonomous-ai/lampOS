"""Paraphrase recall@3 and embed timings for lamp-memory's kept list (#2510).

Measures only: nothing in crates/ or the image depends on it. It runs on a
laptop, reads local files and makes no network call. The set is
crates/memory/tests/fixtures/paraphrase-recall.json; never send it to any
API.

Without --model it scores the two baselines the lamp has:
  word  - #2291's kept-list match: the kept things sharing the most content
          words with what was heard (lower-cased, three letters or more, not
          a small word, a plural's s off), then the newest.
  fts5  - FTS5 BM25 with unicode61 remove_diacritics 2 over search.rs's
          text() form, asked with query_with(skip_common) (#2282's COMMON),
          ranked by bm25 and then the newest.
With --model (an ONNX export of a sentence encoder) and --tokenizer (its
tokenizer.json) it also embeds every kept thing and question, ranks by
cosine, and times one query embed and one write (passage) embed each,
tokenizing included, one at a time as the lamp would.

The models and their revisions are in downloads.sha256. Check the downloads
against it from the folder they were saved in, then run the bench isolated
(-I: no user site-packages, no current-folder imports):

    shasum -a 256 -c /path/to/downloads.sha256

The bench checks the --model and --tokenizer files against the same manifest
again before it loads them and refuses a file that is not listed or does not
match; --no-verify skips that, for a local experiment only. One model per
run, so the peak RSS is that model's:

    python3 -I bench.py --fixture ../../crates/memory/tests/fixtures/paraphrase-recall.json \
        --model e5-fp32/onnx/model.onnx --tokenizer e5-fp32/onnx/tokenizer.json \
        --query-prefix "query: " --passage-prefix "passage: " --threads 4
"""

import argparse
import hashlib
import json
import os
import resource
import sqlite3
import statistics
import sys
import time
import unicodedata

HITS = 3
QUERY_WORDS = 16

# #2291's SMALL_WORDS (crates/voice/src/remember.rs).
SMALL_WORDS = set("""a about all also am an and any are around at back be bit but can could
did do does down for from get give go going got had has have he hello her here hers hey hi
him his how i i'm in into is it it's its just know lamp let like little make me mine more my
need no not now of off ok okay on one or our out please really remember right said say see
she so some tell than thank thanks that the their them then there these they think this those
to today too up us very want wants was way we well were what what's whats when where which who
why will with would yes you your yours""".split())

# #2282's COMMON (crates/memory/src/search.rs).
COMMON = """the and or but of to in on at for with from by about as is are was were be been am
it its my me mine you your yours we our us he him his she her they them their what which who
when where why how that this these those do does did have has had can could would should will
any some not no so if there tell told remember please của là tôi và có không này đó thì mà
những các một cho với được gì nào""".split()


def content_words(text):
    """#2291's content_words."""
    lowered = text.lower().replace("’", "'")
    words = []
    for word in "".join(c if (c.isalnum() or c == "'") else " " for c in lowered).split():
        word = word.strip("'")
        if word.endswith("'s"):
            word = word[:-2]
        if len(word) < 3 or word in SMALL_WORDS:
            continue
        if word.endswith("s") and len(word[:-1]) >= 3:
            word = word[:-1]
        words.append(word)
    return sorted(set(words))


def word_rank(kept, question):
    wanted = set(content_words(question))
    scored = []
    for entry in kept:
        shared = len(wanted.intersection(content_words(entry["text"])))
        if shared:
            scored.append((-shared, -entry["id"], entry["id"]))
    return [entry_id for _, _, entry_id in sorted(scored)[:HITS]]


def fold(value):
    """search.rs's text(): NFC, lower-cased, d-with-stroke folded."""
    value = unicodedata.normalize("NFC", value).lower().replace("đ", "d")
    return unicodedata.normalize("NFC", value)


COMMON_FOLDED = {fold(word) for word in COMMON}


def fts_query(value):
    """search.rs's query_with(value, skip_common = true)."""
    words = []
    for word in "".join(c if c.isalnum() else " " for c in fold(value)).split():
        if len(word) >= 2 and word not in words and word not in COMMON_FOLDED:
            words.append(word)
            if len(words) == QUERY_WORDS:
                break
    return " OR ".join(f'"{word}"' for word in words) or None


def fts_index(kept):
    connection = sqlite3.connect(":memory:")
    connection.execute(
        "CREATE VIRTUAL TABLE search USING fts5(body, tokenize='unicode61 remove_diacritics 2')"
    )
    for entry in kept:
        connection.execute(
            "INSERT INTO search(rowid, body) VALUES (?, ?)", (entry["id"], fold(entry["text"]))
        )
    return connection


def fts_rank(connection, question):
    query = fts_query(question)
    if query is None:
        return []
    rows = connection.execute(
        "SELECT rowid FROM search WHERE search MATCH ? ORDER BY rank, rowid DESC LIMIT ?",
        (query, HITS),
    ).fetchall()
    return [row[0] for row in rows]


MANIFEST = os.path.join(os.path.dirname(os.path.abspath(__file__)), "downloads.sha256")


def read_manifest(path):
    """downloads.sha256 as {relative name: sha256}, as shasum -c reads it."""
    listed = {}
    with open(path, encoding="utf-8") as file:
        for line in file:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            digest, name = line.split(None, 1)
            listed[name.lstrip("*")] = digest.lower()
    return listed


def verify(path, listed):
    """Refuse a file that downloads.sha256 does not list or that differs."""
    given = os.path.normpath(path).replace(os.sep, "/")
    names = [name for name in listed if given == name or given.endswith("/" + name)]
    if not names:
        sys.exit(f"refusing {path}: not in downloads.sha256 (--no-verify for a local experiment)")
    name = max(names, key=len)
    digest = hashlib.sha256()
    with open(path, "rb") as file:
        for block in iter(lambda: file.read(1 << 20), b""):
            digest.update(block)
    if digest.hexdigest() != listed[name]:
        sys.exit(f"refusing {path}: its sha256 does not match downloads.sha256's {name}")


def percentile(values, share):
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, round(share * (len(ordered) - 1)))]


class Encoder:
    def __init__(self, model, tokenizer, threads, max_tokens, dims, output):
        import numpy
        import onnxruntime
        from tokenizers import Tokenizer

        self.numpy = numpy
        options = onnxruntime.SessionOptions()
        options.intra_op_num_threads = threads
        options.inter_op_num_threads = 1
        self.session = onnxruntime.InferenceSession(
            model, options, providers=["CPUExecutionProvider"]
        )
        self.inputs = {item.name for item in self.session.get_inputs()}
        self.tokenizer = Tokenizer.from_file(tokenizer)
        self.tokenizer.enable_truncation(max_tokens)
        self.tokenizer.no_padding()
        self.dims = dims
        self.output = output

    def __call__(self, text):
        numpy = self.numpy
        encoding = self.tokenizer.encode(text)
        ids = numpy.array([encoding.ids], dtype=numpy.int64)
        mask = numpy.array([encoding.attention_mask], dtype=numpy.int64)
        feed = {"input_ids": ids, "attention_mask": mask}
        if "token_type_ids" in self.inputs:
            feed["token_type_ids"] = numpy.zeros_like(ids)
        if self.output == "pooled":
            vector = self.session.run(["sentence_embedding"], feed)[0][0]
        else:
            hidden = self.session.run(None, feed)[0][0]
            weights = mask[0][:, None].astype(hidden.dtype)
            vector = (hidden * weights).sum(axis=0) / weights.sum()
        if self.dims:
            vector = vector[: self.dims]
        return vector / numpy.linalg.norm(vector)


def recall(questions, ranked):
    found = {"en": [0, 0], "vi": [0, 0]}
    for question, ids in zip(questions, ranked):
        found[question["lang"]][0] += question["kept"] in ids
        found[question["lang"]][1] += 1
    return {lang: round(100 * hit / total, 1) for lang, (hit, total) in found.items()}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--fixture", required=True)
    parser.add_argument("--model")
    parser.add_argument("--tokenizer")
    parser.add_argument("--name", default="model")
    parser.add_argument("--query-prefix", default="")
    parser.add_argument("--passage-prefix", default="")
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--max-tokens", type=int, default=512)
    parser.add_argument("--dims", type=int, default=0, help="keep the first N (Matryoshka)")
    parser.add_argument("--output", choices=["mean", "pooled"], default="mean")
    parser.add_argument("--rounds", type=int, default=10, help="timed passes over the set")
    parser.add_argument("--misses", action="store_true", help="print each miss")
    parser.add_argument("--manifest", default=MANIFEST, help="the sha256 list to check against")
    parser.add_argument(
        "--no-verify", action="store_true", help="skip the sha256 check (local experiments only)"
    )
    args = parser.parse_args()
    if args.model and not args.tokenizer:
        parser.error("--model needs --tokenizer")
    if args.model and not args.no_verify:
        listed = read_manifest(args.manifest)
        for path in (args.model, args.tokenizer):
            verify(path, listed)

    with open(args.fixture, encoding="utf-8") as file:
        fixture = json.load(file)
    kept, questions = fixture["kept"], fixture["questions"]
    pools = {lang: [entry for entry in kept if entry["lang"] == lang] for lang in ("en", "vi")}
    results = []

    ranked = [word_rank(pools[q["lang"]], q["text"]) for q in questions]
    results.append({"name": "word match (#2291)", "recall@3": recall(questions, ranked)})
    indexes = {lang: fts_index(pool) for lang, pool in pools.items()}
    ranked_fts = [fts_rank(indexes[q["lang"]], q["text"]) for q in questions]
    results.append({"name": "FTS5 BM25 + COMMON", "recall@3": recall(questions, ranked_fts)})
    if args.misses:
        for label, rows in (("word", ranked), ("fts5", ranked_fts)):
            for question, ids in zip(questions, rows):
                if question["kept"] not in ids:
                    print(f"miss {label}: {question['text']} -> {ids}", file=sys.stderr)

    if args.model:
        before = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        started = time.perf_counter()
        encoder = Encoder(
            args.model, args.tokenizer, args.threads, args.max_tokens, args.dims, args.output
        )
        load_ms = (time.perf_counter() - started) * 1000
        passages = {e["id"]: encoder(args.passage_prefix + e["text"]) for e in kept}
        asked = [encoder(args.query_prefix + q["text"]) for q in questions]
        ranked_vectors = []
        for question, vector in zip(questions, asked):
            pool = pools[question["lang"]]
            scored = sorted(pool, key=lambda e: -float(passages[e["id"]] @ vector))
            ranked_vectors.append([entry["id"] for entry in scored[:HITS]])
            if args.misses and question["kept"] not in ranked_vectors[-1]:
                print(f"miss {args.name}: {question['text']} -> {ranked_vectors[-1]}", file=sys.stderr)
        query_ms, write_ms = [], []
        for _ in range(args.rounds):
            for question in questions:
                started = time.perf_counter()
                encoder(args.query_prefix + question["text"])
                query_ms.append((time.perf_counter() - started) * 1000)
            for entry in kept:
                started = time.perf_counter()
                encoder(args.passage_prefix + entry["text"])
                write_ms.append((time.perf_counter() - started) * 1000)
        peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        # ru_maxrss is bytes on macOS and kilobytes on Linux.
        scale = 1 if sys.platform == "darwin" else 1024
        results.append(
            {
                "name": args.name,
                "dims": len(asked[0]),
                "threads": args.threads,
                "load_ms": round(load_ms),
                "query_ms": [round(percentile(query_ms, 0.5), 1), round(percentile(query_ms, 0.95), 1)],
                "write_ms": [round(percentile(write_ms, 0.5), 1), round(percentile(write_ms, 0.95), 1)],
                "mean_query_ms": round(statistics.mean(query_ms), 1),
                "rss_peak_mb": round(peak * scale / 2**20),
                "rss_added_mb": round((peak - before) * scale / 2**20),
                "recall@3": recall(questions, ranked_vectors),
            }
        )
    for result in results:
        print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
