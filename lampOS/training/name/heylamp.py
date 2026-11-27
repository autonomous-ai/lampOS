"""The "Hey Lamp" retrain (#8907): Backend's rendered set in, a name model and its measurements out.

The only calls are a greeting and then "Lamp": "Hey Lamp", "Hey, Lamp", "Hi Lamp", "Hello Lamp",
"Um, hey Lamp", "Okay, hey Lamp", alone or with an ask after. Not calls: a bare "Lamp", "OK Lamp"
and "Okay Lamp" (alone or with an ask), look-alike greetings ("Hey man", "Hey Lynn", "Hey Siri"...),
"lamp" inside talk ("turn off the lamp", "clamp it"...), and everyday talk. The rule is `judge`
below; a manifest's own label is checked against it.

    python heylamp.py check-env
        Loads torch, torchaudio, onnxruntime, the pinned front end and both baseline name models.
    python heylamp.py fetch-sc
        Speech Commands v0.02 read as it downloads, its SHA-256 checked on the stream; keeps only
        its six noise recordings, its split lists and a few clips of each word.
    python heylamp.py public
        Speech Commands' noise and words as clip stores; LibriSpeech dev (to train against) and
        test (held out) are read from their parquet files when wanted.
    python heylamp.py unpack TAR --sha256 HEX [--manifest TSV] [--set NAME] [--extra-providers A,B]
        Backend's tar: its SHA-256 checked, the manifest read (`file provider voice style text
        label`, and `split` or a voices.tsv), every WAV made 16 kHz mono, split by the voice's
        mark: `test` voices held out; of the `train` voices about a tenth kept for validation.
        Clips of the --extra-providers (OpenAI's, the product owner 27 Nov 21:5x: no more of
        them, and those already made are kept) are an extra held-out slice, whatever their
        mark: never trained on, and reported apart from the test set.
    python heylamp.py align NAME [--part I/K | --merge] [--truth TRUTH.tsv]
        Where the name ends in each call, and where the word a not-call could be taken for it ends
        ("Lamp" in "OK Lamp", "man" in "Hey man", "lamp" in "turn off the lamp"): forced alignment
        (training/stop/align.py, wav2vec 2.0), the end moved on to where the sound fades
        (`word_end`). Only what is placed: calls with an ask, and training not-calls.
    python heylamp.py inputs NAME words|talk|check|check-talk [--part I/K] [--channel CH.npz]
                     [--partial P]
        The training inputs, as `lamp-name` gives them to the model, in training/stop/train.py's
        format (compressed): volume (-30 to 0 dB from -12 dBFS), the lamp's channel
        (microphone.py's lost onset and loudspeaker, a room, and a measured channel.npz when
        given) and noise or talk around.
    python heylamp.py evaluate NAME MODEL... [--names A,B,...] [--part I/K] [--every N] [--only ...]
        Every model on the same held-out sound, as `lamp-name` runs it: the product's rule
        (`--least 0.9 --step-ms 10 --wake 0.03`) and the plain 80 ms rule, at each bar; the
        test set and the extra slice apart.
    python heylamp.py report NAME [EVAL.json...]
        The tables: calls caught, not-calls woken by kind, false wakes an hour, the trade-off.
    python heylamp.py synth OUT_DIR [--count N]
        A stand-in for Backend's tar from October's Piper voices (LibriTTS speakers to train on,
        others and the Cori and LJ Speech voices held out), in the same format, for a dry run.

heylamp.sh runs them in order. Every step runs at the lowest priority (run.sh), with the threads
OMP_NUM_THREADS gives; one that writes 512 MiB or more refuses to start if it would leave under
10 GiB free, a smaller one under 2 GiB (`room_for`).
"""

from __future__ import annotations

import argparse
import csv
import glob
import hashlib
import io
import json
import os
import re
import shutil
import sys
import tarfile
import time
import unicodedata
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np

import shared  # noqa: F401  (puts training/stop on the path, after this folder)
from features import STEP, WINDOW, FrontEnd, stacks
from hearing import hearings
from make_data import ALONE, INPUTS, LENGTH, SEEN_UNTIL, Voices, labels, scaled
from microphone import clip_onsets, loudspeaker, onsets
from mix import FULL, RATE, active_level, gain_for, mixture, place, room
from store import Store, exists, save

THREADS = int(os.environ.get("OMP_NUM_THREADS", "2"))
HEY = shared.LAMPOS / ".cache" / "heylamp"
CLIPS = HEY / "clips"
SETS = HEY / "sets"
VOICE_DATA = Path.home() / "lamp-voice-data"
DATA_HOME = Path(os.environ.get("HEYLAMP_DATA", VOICE_DATA / "heylamp-8907"))
LINES = DATA_HOME / "lines-8907.tsv"
REPO_MODEL = shared.LAMPOS / "models" / "name" / "name.onnx"
# The file the product installs (deploy/product/models.json, release models-2026-10-16): the one
# the lamps run, not the repository's models/name/name.onnx.
INSTALLED_MODEL = VOICE_DATA / "name-models" / "name.onnx"
INSTALLED_SHA256 = "1d238ca7c468bc3b22d6de94155abaec422f7433693e0696302f20157e9af50f"
DESK_KIT = VOICE_DATA / "desk-kit-2"
FREE_GIB = 10
SC_SHA256 = "af14739ee7dc311471de98f5f9d2c9191b18aedfe957f4a6ff791c709868ff58"
SC_URL = "http://download.tensorflow.org/data/speech_commands_v0.02.tar.gz"

# Volume: the person's level is REF_DB plus a draw from VOLUME_DB (-42 to -12 dBFS at mic2; a
# person at the lamp is -12 to -27 dBFS, docs/duplex-listening.md, and quieter further away).
REF_DB = -12.0
VOLUME_DB = (-30.0, 0.0)
# The product's rule (deploy/product/lamp-name.service) and the bars measured.
PRODUCT_BAR = 0.9
WAKE = 0.03
LEASTS = [0.5, 0.7, 0.8, 0.9, 0.95, 0.98, 0.99, 0.995, 0.999]
RULES = ["80", "10w"]  # one grid of 80 ms; the product's 10 ms grids, woken at 0.03
FIRST_SEEN = 31_712 / RATE
VOICE_DB = -20.0
CALL_CONDITIONS = ["clean", "mixed", "far"]
NOTCALL_CONDITIONS = ["clean", "mixed"]
# The name the report gives the extra held-out slice (unpack --extra-providers).
EXTRA_SLICE = "extra slice (OpenAI voices)"

# ---------------------------------------------------------------------------------------------
# What is a call.

CALL_STARTS = [("hey", "lamp"), ("hi", "lamp"), ("hello", "lamp"), ("um", "hey", "lamp"), ("okay", "hey", "lamp")]
GREETING_WORDS = {"hey", "hi", "hello"}
LAMPISH = {
    "lamp", "lamps", "lampshade", "lamplight", "lamb", "lambs", "clamp", "camp", "damp", "ramp", "stamp",
    "lab", "plan", "land", "lap", "lump", "limp", "ma'am",
}


def words_of(text: str) -> list[str]:
    """Lower-case words of a line, accents taken off, "OK" read as "okay": the aligner's words too."""
    t = unicodedata.normalize("NFKD", text)
    t = "".join(c for c in t if not unicodedata.combining(c)).lower().replace("’", "'")
    t = re.sub(r"[^a-z']+", " ", t)
    out = []
    for w in t.split():
        w = w.strip("'")
        if w:
            out.append("okay" if w == "ok" else w)
    return out


def known_kinds() -> dict[str, tuple[str, str]]:
    """The line list's own (label, kind) by text, if it is here."""
    out = {}
    if LINES.exists():
        with open(LINES, encoding="utf-8") as f:
            for row in csv.DictReader(f, delimiter="\t"):
                out[row["text"].strip()] = (row["label"], row["kind"])
    return out


def judge(text: str, known: dict | None = None) -> dict:
    """The label by the rule above, the kind, and which word is the one to place (the name in a
    call; in a not-call the word that could be taken for it), as an index into `words_of`."""
    w = words_of(text)
    for start in CALL_STARTS:
        if tuple(w[: len(start)]) == start:
            m = re.match(r"^\s*(.*?\blamp\b)", text, re.I)
            opening = re.sub(r"\s+", " ", m.group(1)) if m else " ".join(start)
            opening = opening[0].upper() + opening[1:-4].lower() + "Lamp"
            return {"label": "call", "kind": "alone" if len(w) == len(start) else "with-ask",
                    "anchor": len(start) - 1, "opening": opening, "words": w}
    kind = (known or {}).get(text.strip(), (None, None))[1]
    if kind is None:
        if w[:1] == ["lamp"] or w[:2] == ["okay", "lamp"]:
            kind = "bare" if len(w) == (1 if w[0] == "lamp" else 2) else "bare-with-ask"
        elif (w[:1] and w[0] in GREETING_WORDS and len(w) >= 2) or w[:1] == ["alexa"]:
            head = words_of(text.split(",")[0])
            kind = "lookalike" if len(head) == len(w) else "lookalike-with-ask"
        elif any(x in LAMPISH for x in w):
            kind = "lamp-in-talk"
        else:
            kind = "talk"
    anchor, opening = None, ""
    if kind.startswith("bare") and "lamp" in w:
        anchor = w.index("lamp")
        opening = "OK Lamp" if w[0] == "okay" else "Lamp"
    elif kind.startswith("lookalike"):
        head = words_of(text.split(",")[0])
        anchor = max(0, min(len(w), len(head)) - 1)
        opening = " ".join(head)
    elif kind == "lamp-in-talk":
        anchor = next((i for i, x in enumerate(w) if x in LAMPISH), None)
        opening = w[anchor] if anchor is not None else ""
    return {"label": "not-call", "kind": kind, "anchor": anchor, "opening": opening, "words": w}


# ---------------------------------------------------------------------------------------------
# Small helpers.


def part_of(text: str) -> tuple[int, int]:
    a, b = (int(v) for v in text.split("/"))
    return a, b


def room_for(mib: float, what: str) -> None:
    """Refuses a step that writes 512 MiB or more unless the disk keeps FREE_GIB free after it; a
    smaller one only if it would leave under 2 GiB (the disk is shared, and others fill it too)."""
    HEY.mkdir(parents=True, exist_ok=True)
    free = shutil.disk_usage(HEY).free / 2**20
    floor = FREE_GIB * 1024 if mib >= 512 else 2048
    if free - mib < floor:
        raise SystemExit(f"{what}: {free:.0f} MiB free, {mib:.0f} MiB more would leave under {floor / 1024:.0f} GiB; stopped")
    if free - mib < FREE_GIB * 1024:
        print(f"{what}: {free:.0f} MiB free (under {FREE_GIB} GiB, not by this step); {mib:.0f} MiB more", flush=True)


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 22), b""):
            h.update(block)
    return h.hexdigest()


def hash_share(key: str) -> float:
    """A fixed number in [0, 1) for a key: who is held back is chosen by it, never at random."""
    return hashlib.sha1(key.encode()).digest()[0] / 256


def to_int16(x: np.ndarray) -> np.ndarray:
    return np.clip(np.round(np.asarray(x, np.float64) * 32768.0), -32768, 32767).astype(np.int16)


def decode(data: bytes) -> np.ndarray:
    """A sound file's samples as 16 kHz mono int16."""
    import soundfile as sf
    from scipy.signal import resample_poly

    x, rate = sf.read(io.BytesIO(data), dtype="float32", always_2d=True)
    x = x.mean(axis=1)
    if rate != RATE:
        g = np.gcd(int(rate), RATE)
        x = resample_poly(x, RATE // g, int(rate) // g).astype(np.float32)
    return to_int16(x)


def sounding_end(clip: np.ndarray) -> float:
    """Seconds into a clip at which its last 10 ms louder than 35 dB under its loudest ends."""
    hop = RATE // 100
    n = len(clip) // hop
    if n == 0:
        return len(clip) / RATE
    power = np.mean(np.asarray(clip[: n * hop], np.float64).reshape(n, hop) ** 2, axis=1) + 1e-9
    loud = np.flatnonzero(power >= power.max() * 10 ** (-35 / 10))
    return float((loud[-1] + 1) * hop / RATE) if len(loud) else len(clip) / RATE


def set_dir(name: str) -> Path:
    return SETS / name


class TarClips:
    """One split of Backend's set, read straight from its tar (uncompressed) at the offsets unpack
    noted, so the 3 GB are on the disk once: as `Store`, `meta`, `len` and `audio(i)`."""

    def __init__(self, name: str, split: str):
        d = json.loads((set_dir(name) / "clips.json").read_text())
        self.tar = Path(d["tar"])
        self.meta = d["splits"][split]
        self.file = open(self.tar, "rb")

    def __len__(self) -> int:
        return len(self.meta)

    def audio(self, i: int) -> np.ndarray:
        m = self.meta[i]
        self.file.seek(m["offset"])
        return decode(self.file.read(m["size"])).astype(np.float32)


class ParquetClips:
    """LibriSpeech from Hugging Face's parquet files, held in memory as FLAC and read when asked."""

    def __init__(self, parts: list[str]):
        import pyarrow.parquet as pq

        self.data, self.meta = [], []
        for part in parts:
            path = shared.STOP_CACHE / "downloads" / "librispeech-parquet" / f"{part}.parquet"
            table = pq.read_table(path, columns=["audio", "text", "speaker_id", "id"])
            for audio, text, speaker, uid in zip(*(table.column(c).to_pylist() for c in ["audio", "text", "speaker_id", "id"])):
                self.data.append(audio["bytes"])
                self.meta.append({"id": f"{part}/{uid}", "speaker": f"libri-{speaker}", "text": text.lower()})

    def __len__(self) -> int:
        return len(self.meta)

    def audio(self, i: int) -> np.ndarray:
        import soundfile as sf

        x, rate = sf.read(io.BytesIO(self.data[i]), dtype="int16")
        assert rate == RATE
        return x.astype(np.float32)


_LIBRI = {}


def libri(which: str) -> ParquetClips:
    """"dev": LibriSpeech dev-clean and dev-other (talk to train against, readers ending in 3 kept
    for validation, as training/stop); "test": test-clean and test-other (held out)."""
    if which not in _LIBRI:
        parts = ["validation.clean", "validation.other"] if which == "dev" else ["test.clean", "test.other"]
        _LIBRI[which] = ParquetClips(parts)
    return _LIBRI[which]


def stores_of(name: str) -> dict:
    if (set_dir(name) / "clips.json").exists():
        d = json.loads((set_dir(name) / "clips.json").read_text())
        return {split: TarClips(name, split) for split in d["splits"]}
    return {split: Store(f"{name}-{split}", CLIPS) for split in ("train", "check", "test") if exists(f"{name}-{split}", CLIPS)}


def alignment(name: str) -> dict:
    path = set_dir(name) / "align.json"
    return json.loads(path.read_text()) if path.exists() else {}


def anchor_of(meta: dict, found: dict | None) -> tuple[float | None, str]:
    """Where to place a clip (seconds into it) and how that was known."""
    if found and "end" in found:
        return found["end"], "aligned"
    return None, (found or {}).get("fail", "not aligned")


# ---------------------------------------------------------------------------------------------
# check-env


def check_env(_a) -> None:
    import onnx
    import onnxruntime as ort
    import scipy
    import torch
    import torchaudio

    from features import check_against_streaming

    print(f"python {sys.version.split()[0]}, numpy {np.__version__}, scipy {scipy.__version__}, torch {torch.__version__}, "
          f"torchaudio {torchaudio.__version__}, onnxruntime {ort.__version__}, onnx {onnx.__version__}")
    print(f"torch: {torch.from_numpy(np.ones(4, np.float32)).sum().item():.0f} from NumPy, {torch.get_num_threads()} threads")
    front = FrontEnd(shared.STOP_CACHE / "oww", threads=THREADS)
    rng = np.random.default_rng(0)
    sound = (rng.standard_normal(5 * RATE) * 300).astype(np.float32)
    print(f"front end: pinned models loaded; batch against step-by-step, largest difference {check_against_streaming(front, sound):.2e}")
    paths = [REPO_MODEL] + ([INSTALLED_MODEL] if INSTALLED_MODEL.exists() else [])
    models = Models(paths, ["repo", "installed"][: len(paths)])
    x = stacks(front.of(np.zeros(4 * RATE, np.float32)))
    for name, s in zip(models.names, models.scores(x)):
        print(f"  {name} ({models.sha[name][:12]}): highest score on silence {s.max():.4f}")
    for p in [shared.STOP_CACHE / "piper" / "en_US-libritts-high.onnx", shared.CACHE / "piper" / "en_GB-cori-medium.onnx"]:
        print(f"  piper voice {p.name}: {'here' if p.exists() else 'missing'}")
    print("environment ok")


# ---------------------------------------------------------------------------------------------
# fetch-sc, public


def fetch_sc(_a) -> None:
    """Speech Commands, read as it downloads: noise, lists and about one clip in thirteen of each word."""
    import urllib.request

    out = shared.STOP_CACHE / "data" / "speech_commands"
    words_here = (out / "yes").exists() or exists("sc-words-train", CLIPS)
    if (out / "testing_list.txt").exists() and (out / "_background_noise_").exists() and words_here:
        print(f"{out}: already here")
        return
    if out.exists():
        shutil.rmtree(out)
    room_for(400, "fetch-sc")
    out.mkdir(parents=True, exist_ok=True)
    h = hashlib.sha256()

    class Hashed(io.RawIOBase):
        def __init__(self, raw):
            self.raw = raw

        def readable(self):
            return True

        def readinto(self, b):
            data = self.raw.read(len(b))
            h.update(data)
            b[: len(data)] = data
            return len(data)

    kept = Counter()
    with urllib.request.urlopen(SC_URL, timeout=120) as response:
        stream = io.BufferedReader(Hashed(response), buffer_size=1 << 20)
        with tarfile.open(fileobj=stream, mode="r|gz") as archive:
            for member in archive:
                name = member.name.lstrip("./")
                if not member.isfile() or ".." in name.split("/"):
                    continue
                top = name.split("/")[0]
                if name in ("testing_list.txt", "validation_list.txt", "LICENSE", "README.md") or top == "_background_noise_":
                    keep = True
                elif name.endswith(".wav") and "/" in name:
                    keep = hash_share(name) < 1 / 13
                else:
                    keep = False
                if keep:
                    target = (out / name).resolve()
                    if not str(target).startswith(str(out.resolve())):
                        continue
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(archive.extractfile(member).read())
                    kept[top if "/" in name else "lists"] += 1
            # Read to the end, so that the hash is of the whole file.
            while stream.read(1 << 20):
                pass
    found = h.hexdigest()
    print(f"speech commands: {sum(kept.values())} files kept ({len(kept)} folders); SHA-256 {found}")
    if found != SC_SHA256:
        shutil.rmtree(out)
        raise SystemExit(f"speech commands: SHA-256 {found}, not October's {SC_SHA256}; removed what was kept")


def public(_a) -> None:
    """Speech Commands' noise and words as clip stores in .cache/heylamp/clips (LibriSpeech is read
    from its parquet files when wanted, `libri`), and a count of what is here."""
    import soundfile as sf

    for which in ("dev", "test"):
        c = libri(which)
        readers = {m["speaker"] for m in c.meta}
        print(f"LibriSpeech {which}: {len(c)} utterances by {len(readers)} readers")
    root = shared.STOP_CACHE / "data" / "speech_commands"
    if not exists("sc-noise", CLIPS):
        clips, metas = [], []
        for path in sorted((root / "_background_noise_").glob("*.wav")):
            x, _ = sf.read(path, dtype="int16")
            clips.append(x)
            metas.append({"id": path.name})
        save("sc-noise", clips, metas, root=CLIPS)
    if not exists("sc-words-train", CLIPS):
        test = set((root / "testing_list.txt").read_text().split())
        val = set((root / "validation_list.txt").read_text().split())
        sets = defaultdict(lambda: ([], []))
        for folder in sorted(p for p in root.iterdir() if p.is_dir() and not p.name.startswith("_")):
            for path in sorted(folder.glob("*.wav")):
                rel = f"{folder.name}/{path.name}"
                split = "test" if rel in test or rel in val else "train"
                x, _ = sf.read(path, dtype="int16")
                clips, metas = sets[split]
                clips.append(x)
                metas.append({"id": rel, "speaker": "sc-" + path.name.split("_nohash_")[0], "text": folder.name})
        for split, (clips, metas) in sets.items():
            save(f"sc-words-{split}", clips, metas, root=CLIPS)
        # The stores hold every sample of them: the WAVs are not kept twice (fetch-sc reads them
        # again if the stores go).
        for folder in sorted(p for p in root.iterdir() if p.is_dir() and not p.name.startswith("_")):
            shutil.rmtree(folder)


# ---------------------------------------------------------------------------------------------
# unpack


def read_tsv(data: bytes) -> list[dict]:
    text = data.decode("utf-8-sig")
    return list(csv.DictReader(io.StringIO(text), delimiter="\t"))


def unpack(a) -> None:
    tar_path = Path(a.tar).resolve()
    want = a.sha256
    if not want:
        side = Path(str(tar_path) + ".sha256")
        if not side.exists():
            raise SystemExit(f"{tar_path}: give --sha256 (or put {side.name} beside it)")
        want = side.read_text().split()[0]
    t0 = time.time()
    found = sha256_file(tar_path)
    if found != want.strip().lower():
        raise SystemExit(f"{tar_path}: SHA-256 {found}, not the {want} given; nothing unpacked")
    print(f"{tar_path.name}: SHA-256 {found} as given ({time.time() - t0:.0f} s)", flush=True)
    try:
        tarfile.open(tar_path, "r:").close()
        plain = True  # read in place later, at the offsets noted here
    except tarfile.ReadError:
        plain = False  # compressed: the clips are copied out into stores
        room_for(tar_path.stat().st_size * 1.5 / 2**20, "unpack")
    known = known_kinds()
    name = a.set
    with tarfile.open(tar_path, "r:*") as archive:
        members = {}
        for m in archive.getmembers():
            if m.isfile():
                members[m.name.lstrip("./")] = m
        by_base = defaultdict(list)
        for k in members:
            by_base[k.split("/")[-1]].append(k)

        def member_bytes(key: str) -> bytes:
            return archive.extractfile(members[key]).read()

        def find(path: str) -> str | None:
            path = path.strip().lstrip("./")
            if path in members:
                return path
            hits = by_base.get(path.split("/")[-1], [])
            return hits[0] if len(hits) == 1 else None

        if a.manifest:
            rows = read_tsv(Path(a.manifest).read_bytes())
        else:
            tsvs = [k for k in members if k.split("/")[-1].lower().startswith("manifest") and k.endswith(".tsv")]
            if len(tsvs) != 1:
                raise SystemExit(f"{tar_path.name}: {len(tsvs)} manifest TSVs inside ({tsvs[:3]}); give --manifest")
            rows = read_tsv(member_bytes(tsvs[0]))
        split_of = {}
        voices_tsv = [k for k in members if k.split("/")[-1].lower() == "voices.tsv"]
        if voices_tsv:
            for row in read_tsv(member_bytes(voices_tsv[0])):
                low = {k.lower(): (v or "").strip() for k, v in row.items() if k}
                if low.get("voice") and low.get("split"):
                    split_of[(low.get("provider", ""), low["voice"])] = low["split"]
                    split_of[("", low["voice"])] = low["split"]
        need = {"file", "provider", "voice", "style", "text", "label"}
        if not rows or need - {k.lower() for k in rows[0]}:
            raise SystemExit(f"manifest: needs the columns {sorted(need)}; has {list(rows[0]) if rows else 'no rows'}")
        sets = defaultdict(lambda: ([], []))
        why = Counter()
        disagree = []
        keyed = []
        for row in rows:
            r = {k.lower(): (v or "").strip() for k, v in row.items() if k}
            key = find(r["file"])
            if key is None:
                why["no such file in the tar"] += 1
                continue
            keyed.append((members[key].offset_data, key, r))
        # In the tar's own order: a compressed tar is read forward only.
        keyed.sort(key=lambda t: t[0])
        for done, (_, key, r) in enumerate(keyed):
            if done % 5000 == 0:
                print(f"unpack: {done}/{len(keyed)}", flush=True)
            split = r.get("split") or split_of.get((r["provider"], r["voice"])) or split_of.get(("", r["voice"]))
            extra = any(p and p in r["provider"].lower() for p in a.extra_providers.lower().split(","))
            if split not in ("train", "test") and not extra:
                why[f"no train/test mark ({split})"] += 1
                continue
            j = judge(r["text"], known)
            if r["label"] not in ("call", "not-call"):
                why[f"label {r['label']!r}"] += 1
                continue
            if r["label"] != j["label"]:
                disagree.append((r["label"], j["label"], r["text"]))
            data = member_bytes(key)
            x = decode(data)
            if len(x) < RATE // 5:
                why["shorter than 0.2 s"] += 1
                continue
            voice_key = f"{r['provider']}/{r['voice']}"
            if extra:
                split = "extra"
            elif split == "train" and hash_share(voice_key) < a.check_share:
                split = "check"
            style = r["style"]
            # Backend, 27 Nov 20:12: "whispered" whispers only on Gemini; on ElevenLabs and OpenAI
            # it came out as ordinary voiced speech. Kept, and told apart in the tables.
            if style == "whispered" and "gemini" not in r["provider"].lower():
                style = "whispered (voiced)"
            meta = {
                "file": r["file"], "provider": r["provider"], "voice": r["voice"], "style": style,
                "text": r["text"], "label": j["label"], "manifest_label": r["label"], "kind": j["kind"],
                "anchor": j["anchor"], "opening": j["opening"], "words": j["words"], "split": split,
                "speaker": voice_key, "manifest_style": r["style"], "seconds": round(len(x) / RATE, 3),
                "level_db": round(active_level(x.astype(np.float32)), 1),
            }
            if plain:
                meta.update(offset=members[key].offset_data, size=members[key].size)
            meta["manifest_split"] = r.get("split", "")
            for column in r:
                if column not in meta:
                    meta[column] = r[column]
            clips, metas = sets[split]
            if not plain:
                clips.append(x)
            metas.append(meta)
            why[f"{split} {j['label']}"] += 1
    if len(disagree) > max(5, 0.01 * len(rows)):
        for d in disagree[:10]:
            print("  manifest", d[0], "but the rule says", d[1], ":", d[2])
        raise SystemExit(f"manifest: {len(disagree)} labels disagree with the rule; nothing saved")
    set_dir(name).mkdir(parents=True, exist_ok=True)
    if plain:
        (set_dir(name) / "clips.json").write_text(json.dumps(
            {"tar": str(tar_path), "sha256": found, "splits": {s: metas for s, (_, metas) in sorted(sets.items())}}))
    else:
        for split, (clips, metas) in sorted(sets.items()):
            save(f"{name}-{split}", clips, metas, root=CLIPS)
    summary = {
        "tar": str(tar_path), "sha256": found, "read in place": plain, "rows": len(rows), "counts": dict(why),
        "label disagreements (the rule's label kept)": disagree[:50],
        "voices": {s: sorted({m["speaker"] for m in metas}) for s, (_, metas) in sets.items()},
        "by kind": {s: dict(Counter(f"{m['label']}/{m['kind']}" for m in metas)) for s, (_, metas) in sets.items()},
        "by provider": {s: dict(Counter(m["provider"] for m in metas)) for s, (_, metas) in sets.items()},
        "by style": {s: dict(Counter(m["style"] for m in metas)) for s, (_, metas) in sets.items()},
        "hours": {s: sum(m["seconds"] for m in metas) / 3600 for s, (_, metas) in sets.items()},
    }
    (set_dir(name) / "unpack.json").write_text(json.dumps(summary, indent=1))
    print(json.dumps({k: summary[k] for k in ("counts", "by provider", "hours")}, indent=1))
    print(f"voices: " + ", ".join(f"{s} {len(v)}" for s, v in summary["voices"].items()))
    if disagree:
        print(f"{len(disagree)} manifest labels disagree with the rule (the rule's kept): {disagree[:3]}")


# ---------------------------------------------------------------------------------------------
# align


def align(a) -> None:
    out = set_dir(a.set)
    if a.merge:
        merged = {}
        for path in sorted(out.glob("align-part-*.json")):
            merged.update(json.loads(path.read_text()))
        (out / "align.json").write_text(json.dumps(merged))
        why = Counter(("aligned" if "end" in v else v.get("fail", "?")) + " " + v.get("label", "") for v in merged.values())
        print(f"{len(merged)} clips: {dict(sorted(why.items()))}")
        if a.truth:
            truth_report(a.set, merged, Path(a.truth))
        return
    from align import Aligner

    part, parts = part_of(a.part)
    aligner = Aligner(THREADS)
    found = {}
    n = 0
    t0 = time.time()
    for split, store in stores_of(a.set).items():
        for i, m in enumerate(store.meta):
            # Only where the word's end is not the line's own: a line that is the word alone
            # ("Hey Lamp.", "Lamp.", "Hey man.") ends where its sound does (`sounding_end`), and
            # a held-out not-call is never placed, only heard whole.
            if m.get("anchor") is None or m["kind"] in ("alone", "bare", "lookalike"):
                continue
            if m["label"] == "not-call" and split != "train":
                continue
            n += 1
            if n % parts != part:
                continue
            key = f"{split}/{i}"
            if m["label"] == "not-call" and hash_share(m["file"]) >= a.notcall_share:
                found[key] = {"fail": "not taken", "label": m["label"]}
                continue
            x = store.audio(i) / 32768.0
            text = " ".join(m["words"])
            words = aligner.words(x, text)
            k = m["anchor"]
            if words is None or k >= len(words):
                found[key] = {"fail": "not aligned", "label": m["label"]}
            else:
                w = words[k]
                if w.score < 0.2 or not (0.08 <= w.end - w.start <= 1.2):
                    found[key] = {"fail": "poor alignment", "label": m["label"], "score": round(w.score, 3)}
                else:
                    found[key] = {"start": round(w.start, 3), "end": round(word_end(x, words, k), 3),
                                  "aligner_end": round(w.end, 3), "score": round(w.score, 3), "label": m["label"]}
            if len(found) % 500 == 0:
                print(f"align part {part}: {len(found)} in {time.time() - t0:.0f} s", flush=True)
    out.mkdir(parents=True, exist_ok=True)
    (out / f"align-part-{part}.json").write_text(json.dumps(found))
    print(f"align part {part}/{parts}: {len(found)} clips in {time.time() - t0:.0f} s")


def word_end(x: np.ndarray, words: list, k: int, under: float = 40.0, reach: float = 0.25) -> float:
    """Where word k's sound ends: from the aligner's end (its last letter's frame) on to where the
    sound is `under` dB below the word's loudest 10 ms, within `reach`, and never past the next
    word's start. On the dry run's 399 lines made in two pieces (so their name's end is known), the
    aligner's own end was a median 104 ms early, training/stop's `faded` (25 dB, 150 ms) 55 ms early
    (half of them by over 50 ms), and this (40 dB, 250 ms) 2 ms late, 47 ms off at the 90th
    percentile, none late by over 50 ms. An end put early would label as calls inputs that have
    not yet heard all of "Lamp"; one put a little late only delays the first."""
    w = words[k]
    hop = RATE // 100
    a, b = int(w.start * RATE), int(w.end * RATE)
    c = min(len(x), int((w.end + reach) * RATE))
    if k + 1 < len(words):
        c = min(c, max(b, int(words[k + 1].start * RATE)))
    if b <= a:
        return w.end
    energy = lambda s, e: 10 * np.log10(np.mean(np.asarray(x[s:e], np.float64) ** 2) + 1e-12)  # noqa: E731
    loudest = max(energy(i, i + hop) for i in range(a, max(a + 1, b - hop + 1), hop))
    at = b
    while at + hop <= c and energy(at, at + hop) > loudest - under:
        at += hop
    return at / RATE


def truth_report(name: str, merged: dict, truth: Path) -> None:
    """For a made set whose name ends are known: how far alignment put them."""
    ends = {}
    for row in csv.DictReader(open(truth), delimiter="\t"):
        ends[row["file"]] = float(row["anchor_end"])
    errors = []
    for split, store in stores_of(name).items():
        for i, m in enumerate(store.meta):
            v = merged.get(f"{split}/{i}")
            if v and "end" in v and m["file"] in ends:
                errors.append(v["end"] - ends[m["file"]])
    if errors:
        e = np.abs(np.array(errors))
        print(f"alignment against the known ends of {len(e)} clips: median {np.median(e) * 1000:.0f} ms, "
              f"90th {np.percentile(e, 90) * 1000:.0f} ms, over 100 ms {np.mean(e > 0.1) * 100:.1f} %, "
              f"mean signed {np.mean(errors) * 1000:+.0f} ms")


# ---------------------------------------------------------------------------------------------
# The lamp's channel, noise and talk.


class Channel:
    """A measured channel (training/name/channel.py analyse): its impulse response and floor."""

    def __init__(self, path: Path):
        c = np.load(path)
        self.ir = np.asarray(c["ir"], np.float64)
        self.floor = np.asarray(c["floor_power"], np.float64)
        self.floor_db = float(c["floor_db"])

    def apply(self, x: np.ndarray, rng: np.random.Generator) -> np.ndarray:
        from scipy.signal import fftconvolve

        y = fftconvolve(x, self.ir)[: len(x)]
        y *= (np.abs(x).max() + 1e-9) / (np.abs(y).max() + 1e-9)
        spectrum = np.fft.rfft(rng.standard_normal(len(y)))
        shape = np.interp(np.fft.rfftfreq(len(y), 1 / RATE), np.fft.rfftfreq(2 * (len(self.floor) - 1), 1 / RATE),
                          np.sqrt(self.floor))
        noise = np.fft.irfft(spectrum * shape, len(y))
        noise *= FULL * 10 ** (self.floor_db / 20) / (np.sqrt(np.mean(noise ** 2)) + 1e-20)
        return (y + noise).astype(np.float32)


def person_chain(x: np.ndarray, rng: np.random.Generator, channel: Channel | None) -> np.ndarray:
    """What the room, the lamp's microphone and its channel may do to the person's voice."""
    if rng.random() < 0.3:
        x = room(x, rng)
    if channel is not None and rng.random() < 0.5:
        x = channel.apply(x, rng)
    elif rng.random() < 0.4:
        x = loudspeaker(x, rng)
    if rng.random() < 0.5:
        x = clip_onsets(x, rng)
    return x


class Noise:
    """Stretches of noise: Speech Commands' six recordings (their first 70 % to train, the rest held
    out) and made noise (pink, brown, a hum with its harmonics)."""

    def __init__(self, held_out: bool):
        store = Store("sc-noise", CLIPS)
        self.parts = []
        for i in range(len(store)):
            x = store.audio(i)
            cut = int(len(x) * 0.7)
            self.parts.append(x[cut:] if held_out else x[:cut])

    def stretch(self, rng: np.random.Generator, length: int) -> np.ndarray:
        if rng.random() < 0.7:
            x = self.parts[rng.integers(len(self.parts))]
            if len(x) < length:
                x = np.tile(x, length // len(x) + 1)
            at = int(rng.integers(0, len(x) - length + 1))
            return np.array(x[at : at + length], np.float32)
        kind = rng.integers(3)
        n = length
        if kind == 2:
            t = np.arange(n) / RATE
            f0 = rng.choice([50.0, 60.0, 100.0, 120.0])
            y = sum(np.sin(2 * np.pi * f0 * k * t + rng.uniform(0, 6.3)) / k for k in range(1, 6))
            y = y + 0.3 * rng.standard_normal(n)
        else:
            spectrum = np.fft.rfft(rng.standard_normal(n))
            f = np.fft.rfftfreq(n, 1 / RATE)
            f[0] = f[1]
            spectrum /= np.sqrt(f) if kind == 0 else f
            y = np.fft.irfft(spectrum, n)
        return (y / (np.abs(y).max() + 1e-9) * 8000).astype(np.float32)


def around(rng, level: float, talk, noise: Noise, length: int):
    """Sound around the person, as October's training: the microphone's floor alone (two in five),
    other talk 10 to 30 dB under (one in three), 0 to 10 dB under (one in ten), noise 10 to 30 dB
    under (the rest). The other sound and its level against the person (ALONE when none)."""
    choice = rng.random()
    if choice < 0.4:
        return None, ALONE
    if choice < 0.83:
        relative = -rng.uniform(10, 30) if choice < 0.73 else -rng.uniform(0, 10)
        o = talk.stretch(rng, length)
        return scaled(o, active_level(o), level + relative), relative
    n = noise.stretch(rng, length)
    relative = -rng.uniform(10, 30)
    return scaled(n, active_level(n), level + relative), relative


def held_out_reader(meta: dict) -> bool:
    return meta.get("speaker", "").startswith("libri-") and meta["speaker"].endswith("3")


# ---------------------------------------------------------------------------------------------
# inputs


def place_at(clip: np.ndarray, anchor: float | None, rng, unfinished: bool = False) -> tuple[int, float | None]:
    """Where a clip starts in an example so that its word ends 0.02 to 0.10 s before the end of
    what the first input sees (October's placement), never cutting the clip's start; and where
    that word then ends. Anywhere, with no word to place. `unfinished`: the word ends up to 0.35 s
    after the first input's end instead, so the first inputs hear it unfinished ("Hey La..." of
    "Hey Lan"). A call's inputs that hear its name unfinished are left out (labels), so without
    these no input that hears a word unfinished is ever labelled, and a model may wake on "Hey
    La..." of anything (the dry run's first model woke on every "Hey Lan", "Hey lamb", "Hey Lâm")."""
    if anchor is None:
        return int(rng.integers(-len(clip) // 2, LENGTH - len(clip) // 2)), None
    if unfinished:
        end_at = SEEN_UNTIL[0] + rng.uniform(0.0, 0.35)
    else:
        end_at = SEEN_UNTIL[0] - rng.uniform(0.02, 0.10)
    end_at = max(end_at, anchor + 0.05)
    return int(round((end_at - anchor) * RATE)), end_at


def make_example(clip, anchor, is_call, follow, talk, noise, channel, rng, partial: float = 0.0):
    unfinished = not is_call and partial > 0 and rng.random() < partial
    at, end_at = place_at(clip, anchor, rng, unfinished=unfinished)
    person = place(clip, at, LENGTH)
    if follow is not None:
        start = at + len(clip) + int(rng.uniform(0.05, 0.35) * RATE)
        person = person + place(scaled(follow, active_level(follow), active_level(clip)), start, LENGTH)
    person = person_chain(person, rng, channel)
    level = REF_DB + rng.uniform(*VOLUME_DB)
    person = scaled(person, active_level(person), level)
    other, relative = around(rng, level, talk, noise, LENGTH)
    sound = mixture(person, other, rng)
    if is_call and end_at is not None:
        y = labels([(end_at - 0.3, end_at)])
    else:
        y = np.zeros(INPUTS, np.int8)
    return sound, y, relative


def inputs(a) -> None:
    part, parts = part_of(a.part)
    folder = set_dir(a.set) / "inputs"
    folder.mkdir(parents=True, exist_ok=True)
    front = FrontEnd(shared.STOP_CACHE / "oww", threads=THREADS)
    channel = Channel(Path(a.channel)) if a.channel else None
    stores = stores_of(a.set)
    # What this part will write: 3 KB an input before compression, about half after.
    if a.what == "words":
        n_examples = sum(a.calls if m["label"] == "call" else (a.hard if m["kind"] != "talk" else 1) for m in stores["train"].meta) + 7000
    elif a.what == "check":
        n_examples = len(stores["check"].meta)
    else:
        n_examples = (60 if a.what == "check-talk" else 630 * a.talk_scale) * 750 / INPUTS
    room_for(n_examples * INPUTS * 3072 * 0.5 / 2**20 / parts, f"inputs {a.what} part {part}")
    found = alignment(a.set)
    dev = libri("dev")
    noise = Noise(held_out=False)
    tts_not = Voices([(stores["train"], lambda m: m["label"] == "not-call")])
    talk = Voices([(dev, lambda m: not held_out_reader(m)), (stores["train"], lambda m: m["label"] == "not-call")])
    after = Voices([(dev, lambda m: not held_out_reader(m)), (stores["train"], lambda m: m["kind"] == "talk")])
    t0 = time.time()
    if a.what in ("words", "check"):
        split = "train" if a.what == "words" else "check"
        store = stores[split]
        jobs = []
        why = Counter()
        for i, m in enumerate(store.meta):
            anchor, how = anchor_of(m, found.get(f"{split}/{i}"))
            if m["label"] == "call":
                if anchor is None and m["kind"] == "alone":
                    anchor, how = sounding_end(store.audio(i)), "sound end"
                if anchor is None:
                    why[f"call left out: {how}"] += 1
                    continue
                jobs += [("call", i, r, anchor) for r in range(a.calls if split == "train" else 1)]
                why[f"call, {how}"] += 1
            elif split == "train":
                if anchor is None and m["kind"] in ("bare", "lookalike"):
                    anchor, how = sounding_end(store.audio(i)), "sound end"
                # Half the short talk lines end where a name would: where a line ends says nothing.
                short = m.get("seconds", 9.0) < 3.0
                if anchor is None and m["kind"] == "talk" and short and hash_share(m["file"] + "a") < 0.5:
                    anchor, how = sounding_end(store.audio(i)), "sound end"
                repeat = a.hard if m["kind"] != "talk" else 1
                jobs += [("not", i, r, anchor) for r in range(repeat)]
                why[f"not-call {m['kind']}, {how if anchor is not None else 'anywhere'}"] += 1
        if split == "train":
            words = Store("sc-words-train", CLIPS)
            for i in range(len(words)):
                jobs.append(("sc", i, 0, None))
            why["Speech Commands words"] = len(words)
        if part == 0:
            print(f"{a.what}: {len(jobs)} examples; " + "; ".join(f"{k} {v}" for k, v in sorted(why.items())), flush=True)
        names = ["call", "not", "sc"]
        xs, ys, srcs, levels, ways = [], [], [], [], []
        sc = Store("sc-words-train", CLIPS) if split == "train" else None
        for n, (what, i, r, anchor) in enumerate(jobs):
            if n % parts != part:
                continue
            rng = np.random.default_rng([n, r, 11 if a.what == "check" else 13])
            if what == "sc":
                clip = sc.audio(i)
                anchor = sounding_end(clip)
                kind = "word"
            else:
                clip = store.audio(i)
                kind = store.meta[i]["kind"]
            follow = None
            if kind in ("alone", "bare", "lookalike", "word") and rng.random() < 0.5:
                follow = after.stretch(rng, int(rng.uniform(0.6, 1.5) * RATE))
            sound, y, relative = make_example(clip, anchor, what == "call", follow, talk, noise, channel, rng, a.partial if what == "not" else 0.0)
            x = stacks(front.of(sound))
            assert len(x) == INPUTS, len(x)
            keep = y >= 0
            k = int(keep.sum())
            xs.append(x[keep].astype(np.float16))
            ys.append(y[keep])
            srcs.append(np.full(k, names.index(what), np.int16))
            levels.append(np.full(k, relative, np.float16))
            ways.append(np.full(k, 1 if what == "call" else 0, np.int8))
            if n % 4000 < parts:
                print(f"{a.what} part {part}: {n}/{len(jobs)} ({time.time() - t0:.0f} s)", flush=True)
        y = np.concatenate(ys)
        np.savez_compressed(folder / f"{a.what}-{part}.npz", x=np.concatenate(xs), y=y, source=np.concatenate(srcs),
                 level=np.concatenate(levels), suppressed=np.zeros(len(y), np.int8), names=np.array(names),
                 way=np.concatenate(ways))
        print(f"{a.what} part {part}: {len(y)} inputs, {int((y == 1).sum())} calls, {time.time() - t0:.0f} s")
        return
    # Long stretches, every input 0.
    minute = 60 * RATE
    if a.what == "check-talk":
        plan = [("libri", 1)] * 60
        voices = {"libri": Voices([(dev, held_out_reader)])}
    else:
        scale = a.talk_scale
        plan = ([("libri", 1)] * int(420 * scale) + [("libri", 2)] * int(90 * scale)
                + [("tts", 1)] * int(90 * scale) + [("noise", 1)] * int(30 * scale))
        voices = {"libri": Voices([(dev, lambda m: not held_out_reader(m))]), "tts": tts_not}
    xs = []
    for n, (what, count) in enumerate(plan):
        if n % parts != part:
            continue
        rng = np.random.default_rng([n, 17 if a.what == "talk" else 4])
        if what == "noise":
            sound = noise.stretch(rng, minute)
            sound = scaled(sound, active_level(sound), REF_DB + rng.uniform(-45, -5))
            sound = mixture(None, sound, rng)
        else:
            s = voices[what].stretch(rng, minute)
            if what == "tts":
                s = person_chain(s, rng, channel)
            s = scaled(s, active_level(s), REF_DB + rng.uniform(*VOLUME_DB))
            if rng.random() < 0.3:
                s = clip_onsets(s, rng)
            b = None
            if count == 2:
                b = (tts_not if rng.random() < 0.5 else voices["libri"]).stretch(rng, minute)
                b = scaled(b, active_level(b), active_level(s) + rng.uniform(-20, 0))
            sound = mixture(s, b, rng) if b is not None else mixture(None, s, rng)
        xs.append(stacks(front.of(sound)).astype(np.float16))
        if n % 60 < parts:
            print(f"{a.what} part {part}: {n}/{len(plan)} ({time.time() - t0:.0f} s)", flush=True)
    x = np.concatenate(xs) if xs else np.zeros((0, 16, 96), np.float16)
    np.savez_compressed(folder / f"{a.what}-{part}.npz", x=x, y=np.zeros(len(x), np.int8), source=np.full(len(x), -1, np.int16),
             names=np.array([]))
    print(f"{a.what} part {part}: {len(x)} inputs ({len(x) * 0.08 / 3600:.2f} h), {time.time() - t0:.0f} s")


# ---------------------------------------------------------------------------------------------
# evaluate


class Models:
    """Name models with their batch made free, so a whole stream is scored at once."""

    def __init__(self, paths: list[Path], names: list[str]):
        import onnx
        import onnxruntime as ort

        self.names, self.sessions, self.inputs, self.sha = list(names), [], [], {}
        for path, name in zip(paths, names):
            raw = Path(path).read_bytes()
            self.sha[name] = hashlib.sha256(raw).hexdigest()
            m = onnx.load_from_string(raw)
            m.graph.input[0].type.tensor_type.shape.dim[0].dim_param = "batch"
            m.graph.output[0].type.tensor_type.shape.dim[0].dim_param = "batch"
            options = ort.SessionOptions()
            options.intra_op_num_threads = THREADS
            options.inter_op_num_threads = 1
            s = ort.InferenceSession(m.SerializeToString(), options, providers=["CPUExecutionProvider"])
            self.sessions.append(s)
            self.inputs.append(s.get_inputs()[0].name)

    def scores(self, x: np.ndarray) -> np.ndarray:
        out = np.zeros((len(self.sessions), len(x)), np.float32)
        if len(x) == 0:
            return out
        x = np.asarray(x, np.float32)
        for k, (s, name) in enumerate(zip(self.sessions, self.inputs)):
            for i in range(0, len(x), 8192):
                out[k, i : i + 8192] = s.run(None, {name: x[i : i + 8192]})[0].reshape(-1)
        return out


def runs_of(mask: np.ndarray) -> list[tuple[int, int]]:
    """Inclusive (first, last) of each run of True."""
    out, i, n = [], 0, len(mask)
    while i < n:
        if mask[i]:
            j = i
            while j + 1 < n and mask[j + 1]:
                j += 1
            out.append((i, j))
            i = j + 1
        else:
            i += 1
    return out


def stream_scores(front: FrontEnd, models: Models, sound: np.ndarray, within: tuple[float, float] | None = None):
    """Each model's score at each 80 ms step: on one grid (`--step-ms 80`), and as the product's
    `--step-ms 10 --wake 0.03` gives it (hear::Grids): the highest of the first grid's score and
    each of the seven later grids' newest (of the step before, 10 to 70 ms later), where the later
    grids are awake: from five steps before a step at which the first grid's score reached the wake
    (hear::BACK) to eight after (hear::AWAKE_STEPS). `within` (seconds): the later grids are worked
    out only for steps whose score is given then (a line's own sound, not the quiet around it, in
    which no model here scores near a bar: the repository's name.onnx stays at 0.03 to 0.11 in the
    -70 dBFS floor, so it would wake them through every quiet second)."""
    s0 = models.scores(stacks(front.of(sound)))
    s10 = s0.copy()
    n = s0.shape[1]
    if n < 2:
        return s0, s10
    used = np.zeros_like(s0, dtype=bool)
    for k in range(len(s0)):
        woke = np.flatnonzero(s0[k] >= WAKE)
        if len(woke):
            cover = np.zeros(n + 16, np.int32)
            np.add.at(cover, np.maximum(0, woke - 5), 1)
            np.add.at(cover, np.minimum(n + 15, woke + 9), -1)
            used[k] = np.cumsum(cover)[:n] > 0
    if within is not None:
        t = seen_until(np.arange(n) + 1)  # a later grid's step m is given at the first grid's m + 1
        used &= ((t >= within[0]) & (t <= within[1]))[None, :]
        # A model whose first grid already reached the top bar in the window is over every bar
        # there whatever the later grids say; they are worked out only for the others. (Its
        # best score is then the first grid's, and its first hearing may be up to a step late.)
        t0 = seen_until(np.arange(n))
        inside = (t0 >= within[0]) & (t0 <= within[1])
        for k in range(len(s0)):
            if inside.any() and s0[k][inside].max() >= LEASTS[-1]:
                used[k] = False
    need = used.any(axis=0)
    need[-1] = False  # a later grid's step n-1 would go into step n, which is not there
    for first, last in runs_of(need):
        for g in range(1, 8):
            a = STEP * first + 160 * g
            seg = sound[a : STEP * last + 160 * g + WINDOW]
            if len(seg) < WINDOW:
                continue
            sg = models.scores(stacks(front.of(seg)))
            steps = np.arange(first, first + sg.shape[1])
            for k in range(len(s0)):
                ok = used[k, steps] & (steps + 1 < n)
                s10[k, steps[ok] + 1] = np.maximum(s10[k, steps[ok] + 1], sg[k, ok])
    return s0, s10


def seen_until(j) -> np.ndarray:
    return FIRST_SEEN + 0.08 * np.asarray(j)


def fixed_loudspeaker(x: np.ndarray) -> np.ndarray:
    class Fixed:
        values = iter([425.0, 4500.0, 1650.0, 5.5, 1.75])

        def uniform(self, a, b):
            return next(self.values)

    return loudspeaker(x, Fixed())


def fixed_onset_loss(x: np.ndarray) -> np.ndarray:
    y = np.array(x, np.float32)
    for at in onsets(y):
        end = min(len(y), at + int(0.1 * RATE))
        y[at:end] *= 10 ** (-30 / 20)
    return y


# The 34 AMI meetings October measured on (training/stop/make.sh), in its order: ten first, then
# those with "hold on", "wait" and "stop" in them. Never trained on.
AMI_MEETINGS = """ES2004a ES2004b ES2004c ES2004d IS1009a IS1009b IS1009c TS3003a TS3003b TS3003c
EN2001d EN2005a EN2006b EN2009d ES2005b ES2005c ES2005d ES2007c ES2008d ES2009c ES2012c ES2014d
ES2016a ES2016b IS1007c TS3004c TS3012c ES2010b IN1016 IN1014 IS1003d TS3008b TS3010d EN2001a""".split()
AMI_URL = "https://groups.inf.ed.ac.uk/ami/AMICorpusMirror/amicorpus/{m}/audio/{m}.Mix-Headset.wav"


def ami_meeting(meeting: str) -> np.ndarray:
    """One meeting's Mix-Headset recording, -1 to 1: from lampOS/.cache/stop/downloads/ami if it is
    there, else read from the AMI site into memory (nothing written), its SHA-256 checked against
    training/stop/downloads.sha256 either way."""
    import urllib.request

    import soundfile as sf

    want = {}
    for line in (shared.STOP / "downloads.sha256").read_text().splitlines():
        digest, _, path = line.partition("  ")
        want[path.strip()] = digest
    key = f"ami/{meeting}.Mix-Headset.wav"
    local = shared.STOP_CACHE / "downloads" / key
    if local.exists():
        data = local.read_bytes()
    else:
        for attempt in range(4):
            try:
                with urllib.request.urlopen(AMI_URL.format(m=meeting), timeout=120) as r:
                    data = r.read()
                break
            except OSError:
                if attempt == 3:
                    raise
                time.sleep(10)
    found = hashlib.sha256(data).hexdigest()
    if found != want.get(key):
        raise SystemExit(f"{key}: SHA-256 {found}, not October's {want.get(key)}")
    x, rate = sf.read(io.BytesIO(data), dtype="float32")
    assert rate == RATE
    return x


class Talk:
    """Held-out talk to put under a line: LibriSpeech test readers."""

    def __init__(self):
        self.store = libri("test")

    def stretch(self, rng, length):
        parts, have = [], 0
        while have < length + RATE:
            c = self.store.audio(int(rng.integers(len(self.store))))
            gap = np.zeros(int(rng.uniform(0.05, 0.4) * RATE), np.float32)
            parts += [c, gap]
            have += len(c) + len(gap)
        whole = np.concatenate(parts)
        at = int(rng.integers(0, len(whole) - length))
        return whole[at : at + length]


def line_sound(clip: np.ndarray, how: str, rng, talk: Talk, noise: Noise, channel: Channel | None) -> np.ndarray:
    """A held-out line after 2 s of quiet and before 1.5 s more, heard in one condition:
    "clean": at -20 dBFS (October's "as recorded");
    "mixed": the training's draw of volume, room, microphone, channel and sound around, from a seed
    of the clip's own, with held-out talk and noise;
    "far": at -34 dBFS, the first 100 ms after each silence 30 dB down, the loudspeaker of
    microphone.py at the middle of its ranges, and a LibriSpeech test reader 15 dB under."""
    lead, tail = int(2.0 * RATE), int(1.5 * RATE)
    length = lead + len(clip) + tail
    person = place(clip, lead, length)
    if how == "clean":
        return mixture(scaled(person, active_level(clip), VOICE_DB), None)
    if how == "far":
        x = fixed_loudspeaker(person)
        x = fixed_onset_loss(x)
        x = scaled(x, active_level(x), -34.0)
        o = talk.stretch(rng, length)
        return mixture(x, scaled(o, active_level(o), -34.0 - 15.0))
    x = person_chain(person, rng, channel)
    level = REF_DB + rng.uniform(*VOLUME_DB)
    x = scaled(x, active_level(x), level)
    other, _ = around(rng, level, talk, noise, length)
    return mixture(x, other, rng)


def evaluate(a) -> None:
    part, parts = part_of(a.part)
    paths = [Path(p) for p in a.models]
    names = a.names.split(",") if a.names else [p.stem for p in paths]
    assert len(names) == len(paths), "one name a model"
    models = Models(paths, names)
    front = FrontEnd(shared.STOP_CACHE / "oww", threads=THREADS)
    channel = Channel(Path(a.channel)) if a.channel else None
    only = a.only.split(",")
    stores = stores_of(a.set)
    found = alignment(a.set)
    talk, noise = Talk(), Noise(held_out=True)
    result = {"set": a.set, "part": a.part, "every": a.every, "notcall_every": a.notcall_every or a.every,
              "models": [{"name": n, "path": str(p), "sha256": models.sha[n]} for n, p in zip(names, paths)],
              "calls": [], "notcalls": [], "streams": {}, "desk": []}
    counts = Counter()
    t0 = time.time()

    def mine(kind: str = "item", every: int | None = None) -> bool:
        """Whether this part takes the next item of a kind: every `every`th of them (the same
        items for every model), dealt in turn to the parts."""
        every = every or a.every
        counts[kind] += 1
        return (counts[kind] - 1) % (parts * every) == part * every

    def best(s, sel) -> list:
        return [round(float(s[k][sel].max()), 4) if sel.any() else 0.0 for k in range(len(s))]

    def first_at(s, sel, since: float) -> list:
        out = []
        for k in range(len(s)):
            hit = np.flatnonzero(sel & (s[k] >= PRODUCT_BAR))
            out.append(round(float(seen_until(hit[0]) - since), 3) if len(hit) else None)
        return out

    test = stores.get("test")
    # The held-out test voices, and the extra slice (unpack --extra-providers: OpenAI's), each line
    # tagged with its slice so the report keeps the two apart.
    held = [(s, stores[s]) for s in ("test", "extra") if s in stores]
    for slice_name, store in held:
        if "calls" not in only and "notcalls" not in only:
            break
        for i, m in enumerate(store.meta):
            is_call = m["label"] == "call"
            if (is_call and "calls" not in only) or (not is_call and "notcalls" not in only):
                continue
            if not (mine("call") if is_call else mine("not-call", a.notcall_every)):
                continue
            clip = store.audio(i)
            anchor, how = anchor_of(m, found.get(f"{slice_name}/{i}"))
            if anchor is None:
                anchor, how = sounding_end(clip), "line end"
            row = {k: m.get(k) for k in ("file", "provider", "voice", "style", "kind", "opening", "text")}
            row.update(slice=slice_name, name_end=round(anchor, 3), end_from=how, max={}, first={})
            # A call is caught by a hearing that begins from the line's start to 1 s after the name
            # ends; a not-call wakes the lamp with any hearing from its start to 1.5 s after it.
            window = (2.0, 2.0 + anchor + 1.0) if is_call else (2.0, 2.0 + len(clip) / RATE + 1.5)
            for cond in (CALL_CONDITIONS if is_call else NOTCALL_CONDITIONS):
                rng = np.random.default_rng([int(hashlib.sha1(m["file"].encode()).hexdigest()[:8], 16), len(cond)])
                sound = line_sound(clip, cond, rng, talk, noise, channel)
                s80, s10 = stream_scores(front, models, sound, window)
                t = seen_until(np.arange(s80.shape[1]))
                sel = (t >= window[0]) & (t <= window[1])
                row["max"][cond] = {"80": best(s80, sel), "10w": best(s10, sel)}
                if is_call:
                    row["first"][cond] = {"80": first_at(s80, sel, 2.0 + anchor), "10w": first_at(s10, sel, 2.0 + anchor)}
            result["calls" if is_call else "notcalls"].append(row)
            if len(result["calls"]) + len(result["notcalls"]) in (1, 50) or (len(result["calls"]) + len(result["notcalls"])) % 500 == 0:
                print(f"evaluate part {part}: {len(result['calls'])} calls, {len(result['notcalls'])} not-calls "
                      f"({time.time() - t0:.0f} s)", flush=True)

    def add_stream(group: str, sound: np.ndarray, times: bool = False) -> None:
        g = result["streams"].setdefault(group, {"hours": 0.0, "streams": 0, "hearings": {}})
        s80, s10 = stream_scores(front, models, sound)
        g["hours"] += len(sound) / RATE / 3600
        g["streams"] += 1
        for k, name in enumerate(names):
            h = g["hearings"].setdefault(name, {r: {str(x): 0 for x in LEASTS} for r in RULES})
            for rule, s in (("80", s80), ("10w", s10)):
                for least in LEASTS:
                    h[rule][str(least)] += len(hearings(s[k], least))
            if times:
                # Where each hearing at the product's bar began, to listen to (the sound stays here).
                at = [round(float(seen_until(j)), 2) for j in hearings(s10[k], PRODUCT_BAR)]
                g.setdefault("times", {})[name] = at

    def voiced(clips: list[np.ndarray]) -> np.ndarray:
        parts_ = []
        for c in clips:
            parts_ += [c * gain_for(active_level(c), VOICE_DB), np.zeros(int(0.3 * RATE), np.float32)]
        return mixture(np.concatenate(parts_), None)

    for slice_name, store in held:
        if "tts" not in only:
            break
        who = "held-out voices" if slice_name == "test" else EXTRA_SLICE
        by_voice = defaultdict(list)
        for i, m in enumerate(store.meta):
            if m["label"] == "not-call":
                by_voice[m["speaker"]].append(i)
        for voice in sorted(by_voice):
            if not mine("stream", 1):
                continue
            idx = by_voice[voice]
            add_stream(f"{who}, every not-call line", voiced([store.audio(i) for i in idx]))
            talk_idx = [i for i in idx if store.meta[i]["kind"] == "talk"]
            if talk_idx:
                add_stream(f"{who}, everyday talk lines", voiced([store.audio(i) for i in talk_idx]))
        print(f"evaluate part {part}: {who}' streams ({time.time() - t0:.0f} s)", flush=True)
    if "libri" in only:
        readers = libri("test")
        by_reader = defaultdict(list)
        for i, m in enumerate(readers.meta):
            by_reader[m["speaker"]].append(i)
        for reader in sorted(by_reader)[:: a.libri_every]:
            if mine("stream", 1):
                add_stream("LibriSpeech test readers", voiced([readers.audio(i) for i in by_reader[reader]]))
        print(f"evaluate part {part}: LibriSpeech ({time.time() - t0:.0f} s)", flush=True)
    if "ami" in only:
        for meeting in AMI_MEETINGS[: a.ami_meetings]:
            if not mine("stream", 1):
                continue
            x = ami_meeting(meeting)
            if a.ami_minutes:
                x = x[: int(a.ami_minutes * 60 * RATE)]
            add_stream("AMI meetings, as recorded", x * FULL)
            print(f"evaluate part {part}: AMI {meeting} ({time.time() - t0:.0f} s)", flush=True)
    if "office" in only:
        import soundfile as sf

        # The lamp's own mic2 in the office (lamp-mic/, recorded by the first builder): real
        # office hours. Not labelled, so any real call in it counts as a hearing too.
        for path in sorted((DATA_HOME / "lamp-mic").glob("*/*.wav")):
            if not mine("stream", 1):
                continue
            x, rate = sf.read(path, dtype="float32", always_2d=True)
            if rate != RATE:
                continue
            add_stream(f"lamp mic2, office recording {path.parent.name} (unlabelled)", x.mean(axis=1) * FULL, times=True)
            print(f"evaluate part {part}: {path.name} ({time.time() - t0:.0f} s)", flush=True)
    if "desk" in only and (DESK_KIT / "lines.tsv").exists():
        import soundfile as sf

        known = known_kinds()
        for line in (DESK_KIT / "lines.tsv").read_text().splitlines():
            if line.startswith("#") or not line.strip():
                continue
            lid, voice, kind, text = line.split("\t")[:4]
            path = DESK_KIT / "wav" / f"{lid}.wav"
            if not path.exists() or not mine("desk", 1):
                continue
            plain = re.sub(r"\{[0-9.]+\}", " ", text)
            j = judge(plain, known)
            clip = decode(path.read_bytes()).astype(np.float32)
            sound = line_sound(clip, "clean", None, talk, noise, channel)
            window = (2.0, 2.0 + len(clip) / RATE + 1.5)
            s80, s10 = stream_scores(front, models, sound, window)
            t = seen_until(np.arange(s80.shape[1]))
            sel = (t >= window[0]) & (t <= window[1])
            result["desk"].append({"id": lid, "voice": voice, "text": plain.strip(), "label": j["label"],
                                   "kind": j["kind"], "max": {"clean": {"80": best(s80, sel), "10w": best(s10, sel)}}})
    out = set_dir(a.set) / "runs"
    out.mkdir(parents=True, exist_ok=True)
    tag = a.tag or "eval"
    path = out / f"{tag}-part-{part}-of-{parts}.json"
    path.write_text(json.dumps(result))
    print(f"wrote {path} ({time.time() - t0:.0f} s)")


# ---------------------------------------------------------------------------------------------
# report


def report(a) -> None:
    paths = a.evals or sorted(glob.glob(str(set_dir(a.set) / "runs" / f"{a.tag or 'eval'}-part-*.json")))
    if not paths:
        raise SystemExit("report: no evaluation files")
    merged = None
    for p in paths:
        part = json.load(open(p))
        if merged is None:
            merged = part
            continue
        merged["calls"] += part["calls"]
        merged["notcalls"] += part["notcalls"]
        merged["desk"] += part["desk"]
        for g, v in part["streams"].items():
            m = merged["streams"].setdefault(g, {"hours": 0.0, "streams": 0, "hearings": {}})
            m["hours"] += v["hours"]
            m["streams"] += v["streams"]
            for name, at in v.get("times", {}).items():
                m.setdefault("times", {}).setdefault(name, []).extend(at)
            for name, rules in v["hearings"].items():
                mh = m["hearings"].setdefault(name, {r: {k: 0 for k in rules[r]} for r in rules})
                for r in rules:
                    for k, c in rules[r].items():
                        mh[r][k] += c
    names = [m["name"] for m in merged["models"]]
    rule = a.rule
    bar = a.bar
    lines = []
    say = lines.append
    say(f"# \"Hey Lamp\" retrain: set `{merged['set']}`\n")
    say("Models: " + "; ".join(f"`{m['name']}` ({m['sha256'][:12]}…, {m['path']})" for m in merged["models"]))
    say(f"\nRule: `{rule}` = " + ("the product's `--step-ms 10 --wake 0.03`" if rule == "10w" else "one 80 ms grid")
        + f"; bar {bar} for one step (said.rs). Held-out voices only (the manifest's `test`).\n")
    # The test set is the held-out voices of the providers trained on; the extra slice (OpenAI's
    # voices) is reported on its own, at the end, and is in no other table.
    calls = [r for r in merged["calls"] if r.get("slice", "test") == "test"]
    nots = [r for r in merged["notcalls"] if r.get("slice", "test") == "test"]
    extra_calls = [r for r in merged["calls"] if r.get("slice") == "extra"]
    extra_nots = [r for r in merged["notcalls"] if r.get("slice") == "extra"]
    extra_streams = {g: v for g, v in merged["streams"].items() if g.startswith(EXTRA_SLICE)}
    streams = {g: v for g, v in merged["streams"].items() if not g.startswith(EXTRA_SLICE)}

    def caught(rows, cond, k, least, r=rule):
        sel = [row for row in rows if cond in row["max"]]
        return sum(row["max"][cond][r][k] >= least for row in sel), len(sel)

    def pct(got, n):
        return f"{got}/{n} ({100 * got / n:.1f} %)" if n else "–"

    head = "| | " + " | ".join(f"`{n}`" for n in names) + " |"
    rule_line = "|---|" + "---:|" * len(names)
    say(f"## At the product bar ({bar}, rule {rule})\n")
    say(head)
    say(rule_line)
    for cond in CALL_CONDITIONS:
        say(f"| Calls caught, {cond} | " + " | ".join(pct(*caught(calls, cond, k, bar)) for k in range(len(names))) + " |")
    for kind in ["bare", "bare-with-ask", "lookalike", "lookalike-with-ask", "lamp-in-talk", "talk"]:
        sel = [r for r in nots if r["kind"] == kind]
        if sel:
            say(f"| Woken by {kind}, clean | " + " | ".join(pct(*caught(sel, 'clean', k, bar)) for k in range(len(names))) + " |")
    sel = [r for r in nots if r["opening"] == "OK Lamp"]
    if sel:
        say(f"| Woken by OK/Okay Lamp, clean | " + " | ".join(pct(*caught(sel, 'clean', k, bar)) for k in range(len(names))) + " |")

    def stream_lines(groups: dict) -> None:
        for g, v in sorted(groups.items()):
            cells = []
            for n in names:
                c = v["hearings"][n][rule][str(bar)]
                cells.append(f"{c / max(v['hours'], 1e-9):.2f} ({c} in {v['hours']:.1f} h)")
            say(f"| False wakes an hour: {g} | " + " | ".join(cells) + " |")

    stream_lines(streams)
    say("")
    say(f"## Calls caught, by opening, provider and style (mixed condition, {bar}, {rule})\n")
    say(head)
    say(rule_line)
    for field in ["opening", "provider", "style", "kind"]:
        for value in sorted({r[field] for r in calls}):
            sel = [r for r in calls if r[field] == value]
            say(f"| {field}: {value} | " + " | ".join(pct(*caught(sel, 'mixed', k, bar)) for k in range(len(names))) + " |")
    say("")
    say(f"## Not-calls woken, by opening (clean and mixed, {bar}, {rule})\n")
    say("| Opening | Clips | " + " | ".join(f"`{n}` clean | `{n}` mixed" for n in names) + " |")
    say("|---|---:|" + "---:|---:|" * len(names))
    for opening in sorted({r["opening"] or r["kind"] for r in nots}):
        sel = [r for r in nots if (r["opening"] or r["kind"]) == opening]
        cells = []
        for k in range(len(names)):
            for cond in NOTCALL_CONDITIONS:
                got, n = caught(sel, cond, k, bar)
                cells.append(f"{100 * got / n:.0f} %" if n else "–")
        say(f"| {opening} | {len(sel)} | " + " | ".join(cells) + " |")
    say("")
    say(f"## The trade-off (rule {rule}): calls caught, look-alikes and bare \"Lamp\" woken (clean), false wakes an hour\n")
    groups = sorted(streams)
    say("| Model | Bar | " + " | ".join(f"Calls, {c}" for c in CALL_CONDITIONS)
        + " | Look-alikes woken | Bare/OK Lamp woken | Lamp in talk woken | " + " | ".join(groups) + " |")
    say("|---|---:|" + "---:|" * (6 + len(groups)))
    look = [r for r in nots if r["kind"].startswith("lookalike")]
    bare = [r for r in nots if r["kind"].startswith("bare")]
    lit = [r for r in nots if r["kind"] == "lamp-in-talk"]
    for k, n in enumerate(names):
        for least in LEASTS:
            cells = [pct(*caught(calls, c, k, least)) for c in CALL_CONDITIONS]
            cells += [pct(*caught(s, "clean", k, least)) for s in (look, bare, lit)]
            for g in groups:
                v = streams[g]
                c = v["hearings"][n][rule][str(least)]
                cells.append(f"{c / max(v['hours'], 1e-9):.2f}")
            say(f"| `{n}` | {least} | " + " | ".join(cells) + " |")
    say("")
    say(f"## The bar each model needs for 98 % of calls (mixed, rule {rule})\n")
    for k, n in enumerate(names):
        mixed = np.array([r["max"]["mixed"][rule][k] for r in calls if "mixed" in r["max"]])
        if len(mixed):
            b = float(np.quantile(mixed, 0.02))
            say(f"- `{n}`: 98 % of {len(mixed)} calls score at least {b:.4f}; at that bar look-alikes woken "
                f"{pct(*caught(look, 'clean', k, b))}, bare \"Lamp\" {pct(*caught(bare, 'clean', k, b))}.")
    say("")
    lat = []
    for k, n in enumerate(names):
        t = [r["first"]["clean"][rule][k] for r in calls if r.get("first", {}).get("clean") and r["first"]["clean"][rule][k] is not None]
        if t:
            lat.append(f"`{n}` median {np.median(t) * 1000:.0f} ms, 90th {np.percentile(t, 90) * 1000:.0f} ms ({len(t)} calls)")
    if lat:
        say(f"Hearing begins after the name's end (clean, {bar}, {rule}): " + "; ".join(lat) + ".\n")
    if merged["desk"]:
        say(f"## desk-kit-2 (50 ElevenLabs lines rendered through the lamp's route; its voices may be among the train voices)\n")
        say("| | " + " | ".join(f"`{n}`" for n in names) + " |")
        say(rule_line)
        for label in ["call", "not-call"]:
            sel = [r for r in merged["desk"] if r["label"] == label]
            if sel:
                verb = "caught" if label == "call" else "woken"
                say(f"| {label}s {verb} ({len(sel)}) | " + " | ".join(pct(*caught(sel, 'clean', k, bar)) for k in range(len(names))) + " |")
        say("")
    if extra_calls or extra_nots or extra_streams:
        say(f"## The {EXTRA_SLICE}: never trained on, not in the tables above ({bar}, {rule})\n")
        say(head)
        say(rule_line)
        for cond in CALL_CONDITIONS:
            say(f"| Calls caught, {cond} | " + " | ".join(pct(*caught(extra_calls, cond, k, bar)) for k in range(len(names))) + " |")
        for kind in ["bare", "bare-with-ask", "lookalike", "lookalike-with-ask", "lamp-in-talk", "talk"]:
            sel = [r for r in extra_nots if r["kind"] == kind]
            if sel:
                say(f"| Woken by {kind}, clean | " + " | ".join(pct(*caught(sel, 'clean', k, bar)) for k in range(len(names))) + " |")
        stream_lines(extra_streams)
        say("")
    ends = Counter(r["end_from"] for r in calls)
    say(f"Test set: {len(calls)} calls (name end from {dict(ends)}), {len(nots)} not-calls. "
        f"Extra slice: {len(extra_calls)} calls, {len(extra_nots)} not-calls.")
    text = "\n".join(lines)
    print(text)
    out = set_dir(merged["set"]) / "runs" / f"report-{rule}.md"
    out.write_text(text + "\n")
    print(f"\nwrote {out}")


# ---------------------------------------------------------------------------------------------
# synth: a stand-in for Backend's tar


STYLES = {
    "normal": dict(length=1.0, noise=0.667, gain=0.0),
    "fast": dict(length=0.8, noise=0.6, gain=0.0),
    "slow": dict(length=1.3, noise=0.6, gain=0.0),
    "soft": dict(length=1.05, noise=0.5, gain=-10.0),
    "loud": dict(length=0.95, noise=0.8, gain=4.0),
    "whispered": dict(length=1.1, noise=0.4, gain=-14.0),
    "excited": dict(length=0.85, noise=0.85, gain=2.0),
    "tired": dict(length=1.2, noise=0.4, gain=-4.0),
    "hesitant": dict(length=1.15, noise=0.7, gain=-2.0),
}


def synth(a) -> None:
    import random
    import wave

    from speak import OTHER_VOICES, VOICE, Speaker, trimmed

    out = Path(a.out).resolve()
    room_for(a.count * 0.08, "synth")
    (out / "clips").mkdir(parents=True, exist_ok=True)
    lines = list(csv.DictReader(open(LINES, encoding="utf-8"), delimiter="\t"))
    calls = [r for r in lines if r["label"] == "call"]
    nots = [r for r in lines if r["label"] == "not-call"]
    rng0 = random.Random(a.seed)
    train_ids = [s for s in range(904) if s % 8 != 7]
    test_ids = [s for s in range(904) if s % 8 == 7]
    voices = ([("piper-libritts", f"p{s}", "train", s) for s in rng0.sample(train_ids, a.train_voices)]
              + [("piper-libritts", f"p{s}", "test", s) for s in rng0.sample(test_ids, a.test_voices)]
              + [("piper-cori", "cori", "test", None), ("piper-ljspeech", "ljspeech", "test", None)])
    part, parts = part_of(a.part)
    speakers = {}
    rows, truth = [], []
    for n in range(a.count):
        rng = random.Random(f"heylamp-synth-{a.seed}-{n}")
        provider, voice, split, sid = voices[n % len(voices)]
        line = rng.choice(calls) if rng.random() < 0.45 else rng.choice(nots)
        style = rng.choice(list(STYLES))
        if n % parts != part:
            continue
        path = shared.STOP_CACHE / "piper" / "en_US-libritts-high.onnx" if provider == "piper-libritts" else OTHER_VOICES[voice]
        if str(path) not in speakers:
            speakers[str(path)] = Speaker(THREADS, path)
        sp = speakers[str(path)]
        st = STYLES[style]
        j = judge(line["text"])
        text = line["text"]
        tokens = text.split()
        two = j["anchor"] is not None and j["anchor"] + 1 < len(tokens) and rng.random() < a.two_piece

        def say(t):
            from piper import SynthesisConfig

            config = SynthesisConfig(speaker_id=sid, length_scale=st["length"] * rng.uniform(0.92, 1.08),
                                     noise_scale=st["noise"], noise_w_scale=rng.uniform(0.5, 0.9))
            chunks = list(sp.voice.synthesize(t, syn_config=config))
            if not chunks:
                return np.zeros(0, np.float32)
            from scipy.signal import resample_poly

            audio = np.concatenate([c.audio_int16_array for c in chunks]).astype(np.float32) / 32768.0
            rate = sp.voice.config.sample_rate
            g = np.gcd(rate, RATE)
            return resample_poly(audio, RATE // g, rate // g).astype(np.float32)

        if two:
            first = trimmed(say(" ".join(tokens[: j["anchor"] + 1])))
            second = trimmed(say(" ".join(tokens[j["anchor"] + 1 :])))
            pause = np.zeros(int(rng.uniform(0.05, 0.3) * RATE), np.float32)
            audio = np.concatenate([first, pause, second])
            truth.append({"file": f"clips/{n:06d}.wav", "anchor_end": f"{len(first) / RATE + 0.1:.3f}"})
        else:
            audio = trimmed(say(text))
        if len(audio) == 0:
            continue
        audio = np.concatenate([np.zeros(int(0.1 * RATE), np.float32), audio, np.zeros(int(0.15 * RATE), np.float32)])
        audio = audio * 10 ** (st["gain"] / 20)
        pcm = np.clip(np.round(audio * 32767), -32768, 32767).astype("<i2")
        with wave.open(str(out / "clips" / f"{n:06d}.wav"), "wb") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(RATE)
            w.writeframes(pcm.tobytes())
        rows.append({"file": f"clips/{n:06d}.wav", "provider": provider, "voice": voice, "style": style,
                     "text": text, "label": line["label"], "split": split})
        if len(rows) % 100 == 0:
            print(f"synth part {part}: {len(rows)} clips", flush=True)
    with open(out / f"manifest-part-{part}.tsv", "w", encoding="utf-8", newline="") as f:
        w = csv.DictWriter(f, ["file", "provider", "voice", "style", "text", "label", "split"], delimiter="\t", lineterminator="\n")
        w.writeheader()
        w.writerows(rows)
    with open(out / f"truth-part-{part}.tsv", "w", encoding="utf-8", newline="") as f:
        w = csv.DictWriter(f, ["file", "anchor_end"], delimiter="\t", lineterminator="\n")
        w.writeheader()
        w.writerows(truth)
    print(f"synth part {part}/{parts}: {len(rows)} clips, {len(truth)} with a known end")


def synth_pack(a) -> None:
    """The parts' manifests made one, with voices.tsv, into OUT_DIR.tar and its SHA-256."""
    out = Path(a.out).resolve()
    rows, truth = [], []
    for p in sorted(out.glob("manifest-part-*.tsv")):
        rows += list(csv.DictReader(open(p, encoding="utf-8"), delimiter="\t"))
    for p in sorted(out.glob("truth-part-*.tsv")):
        truth += list(csv.DictReader(open(p, encoding="utf-8"), delimiter="\t"))
    rows.sort(key=lambda r: r["file"])
    with open(out / "manifest.tsv", "w", encoding="utf-8", newline="") as f:
        w = csv.DictWriter(f, ["file", "provider", "voice", "style", "text", "label", "split"], delimiter="\t", lineterminator="\n")
        w.writeheader()
        w.writerows(rows)
    voices = sorted({(r["provider"], r["voice"], r["split"]) for r in rows})
    with open(out / "voices.tsv", "w", encoding="utf-8") as f:
        f.write("provider\tvoice\tsplit\n" + "".join(f"{p}\t{v}\t{s}\n" for p, v, s in voices))
    with open(out.parent / f"{out.name}-truth.tsv", "w", encoding="utf-8", newline="") as f:
        w = csv.DictWriter(f, ["file", "anchor_end"], delimiter="\t", lineterminator="\n")
        w.writeheader()
        w.writerows(truth)
    tar_path = out.parent / f"{out.name}.tar"
    with tarfile.open(tar_path, "w") as tar:
        tar.add(out / "manifest.tsv", arcname="manifest.tsv")
        tar.add(out / "voices.tsv", arcname="voices.tsv")
        for r in rows:
            tar.add(out / r["file"], arcname=r["file"])
    digest = sha256_file(tar_path)
    Path(str(tar_path) + ".sha256").write_text(f"{digest}  {tar_path.name}\n")
    print(f"{tar_path}: {len(rows)} clips ({Counter(r['label'] for r in rows)}), {len(voices)} voices, SHA-256 {digest}")


# ---------------------------------------------------------------------------------------------


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="command", required=True)
    sub.add_parser("check-env")
    sub.add_parser("fetch-sc")
    sub.add_parser("public")
    s = sub.add_parser("unpack")
    s.add_argument("tar")
    s.add_argument("--sha256")
    s.add_argument("--manifest")
    s.add_argument("--set", default="heylamp-1")
    s.add_argument("--check-share", type=float, default=0.1, help="share of the train voices kept for validation")
    s.add_argument("--extra-providers", default="openai,chatgpt",
                   help="providers whose clips are only an extra held-out slice, reported apart (never trained on)")
    s = sub.add_parser("align")
    s.add_argument("set")
    s.add_argument("--part", default="0/1")
    s.add_argument("--merge", action="store_true")
    s.add_argument("--truth")
    s.add_argument("--notcall-share", type=float, default=1.0, help="share of the not-calls with a word to place that are aligned")
    s = sub.add_parser("inputs")
    s.add_argument("set")
    s.add_argument("what", choices=["words", "talk", "check", "check-talk"])
    s.add_argument("--part", default="0/1")
    s.add_argument("--channel")
    s.add_argument("--calls", type=int, default=2, help="examples made of each training call")
    s.add_argument("--hard", type=int, default=2, help="examples made of each not-call that is not plain talk")
    s.add_argument("--talk-scale", type=float, default=1.0, help="10.5 h of long talk times this")
    s.add_argument("--partial", type=float, default=0.0,
                   help="share of the placed not-calls whose first inputs hear their word unfinished (place_at)")
    s = sub.add_parser("evaluate")
    s.add_argument("set")
    s.add_argument("models", nargs="+")
    s.add_argument("--names")
    s.add_argument("--part", default="0/1")
    s.add_argument("--every", type=int, default=1, help="every Nth held-out call (and stream, desk line)")
    s.add_argument("--notcall-every", type=int, default=0, help="every Nth held-out not-call (0: as --every)")
    s.add_argument("--only", default="calls,notcalls,tts,libri,ami,office,desk")
    s.add_argument("--channel")
    s.add_argument("--ami-minutes", type=float, default=0.0, help="only the first minutes of each meeting (0: all)")
    s.add_argument("--ami-meetings", type=int, default=34, help="the first N of October's 34 meetings")
    s.add_argument("--libri-every", type=int, default=1, help="every Nth LibriSpeech test reader (1: all 73)")
    s.add_argument("--tag", default="")
    s = sub.add_parser("report")
    s.add_argument("set")
    s.add_argument("evals", nargs="*")
    s.add_argument("--rule", choices=RULES, default="10w")
    s.add_argument("--bar", type=float, default=PRODUCT_BAR)
    s.add_argument("--tag", default="")
    s = sub.add_parser("synth")
    s.add_argument("out")
    s.add_argument("--count", type=int, default=1200)
    s.add_argument("--part", default="0/1")
    s.add_argument("--seed", type=int, default=0)
    s.add_argument("--train-voices", type=int, default=40)
    s.add_argument("--test-voices", type=int, default=10)
    s.add_argument("--two-piece", type=float, default=0.3, help="share of lines made in two pieces, so their name's end is known")
    s = sub.add_parser("synth-pack")
    s.add_argument("out")
    a = p.parse_args()
    {
        "check-env": check_env, "fetch-sc": fetch_sc, "public": public, "unpack": unpack, "align": align,
        "inputs": inputs, "evaluate": evaluate, "report": report, "synth": synth, "synth-pack": synth_pack,
    }[a.command](a)


if __name__ == "__main__":
    main()
