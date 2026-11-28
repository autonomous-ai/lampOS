"""The cutter for the lamp-channel recordings of 28-29 Nov 2026 (Backend's "Hey Lamp" clips played
from Mac 2's speaker through the room into lamp-dbda's mic2, the bus topic lamp-name hears).

    python -I lampcut.py RECORDING.jsonl [MORE.jsonl ...] --session SESSION_DIR --out OUT_DIR
                         [--blocks-dir blocks] [--index index.tsv] [--plays plays.tsv] [--no-track-wav]

SESSION_DIR holds the blocks played (BLOCKS_DIR/block-NNN.wav), INDEX (each clip's block and offset)
and PLAYS (per block, the lamp's CLOCK_MONOTONIC ns read just before afplay); session 2 is
`--blocks-dir blocks-2 --index index-2.tsv --plays plays-2.tsv`. A recording still being made (a
snapshot) is fine: a block or clip it does not hold yet is listed as outside the recording. Writes
into OUT_DIR:

  track.wav            the recording as one continuous 16 kHz mono S16 track (unless --no-track-wav);
                       a gap (a step in the frames' at_us against the samples, checked against the
                       payload's sequence) is filled with zeros so that a time in the track is a time
                       on the lamp
  decode.json          frames, lines skipped, every gap, the at_us-against-samples drift
  timeline.npz         each frame's first sample in the track, its at_us and at_unix_ms
  blocks.tsv           per block: where its chirp was looked for (plays) and found (matched filter,
                       +-3 s), the clips' drift line and the alignment residual
  lamp-clips/<file>    each clip as the lamp heard it: 0.3 s before the clip's first sample to 0.5 s
                       after its last, at the recorded level (nothing rescaled)
  lamp-manifest.tsv    one line a clip: the index's columns, then where it was cut, its correlation
                       with the original, an SNR estimate and the flags (weak, gap inside, clipped)
  floors/block-NNN.wav the lamp's own floor in that block: 0.25 to 0.55 s after each clip, laid end
                       to end (what lampeval.py pads a clip with)

Nothing is dropped: a weak cut is written and flagged.
"""

from __future__ import annotations

import argparse
import base64
import csv
import json
import wave
from pathlib import Path

import numpy as np
from scipy.signal import butter, fftconvolve, sosfiltfilt

RATE = 16_000
FRAME = 320  # 20 ms frames on mic2/audio
LEAD_S, TAIL_S = 0.3, 0.5  # the cut around each clip
CHIRP_S = 0.3
SEARCH_S = 3.0  # the chirp is looked for this far either side of the plays' time
CLIP_SEARCH_S = 0.25  # each clip's own lag is looked for this far either side of its place
WEAK_CORR = 0.15  # a cut whose waveform correlation with its original (200 Hz-5 kHz) is under this is flagged
WEAK_ENV = 0.6  # likewise one whose 10 ms log-power envelope correlates with the original's under this


# ---------------------------------------------------------------------------------------------
# The recording


def read_frames(paths: list[str]) -> tuple[dict, np.ndarray, dict]:
    """Every audio line's at_us, at_unix_ms, bus seq, payload sequence and samples (as offsets into
    one buffer), in the bus's order and each message once (files may be given in any order); lines
    that do not parse (a final line cut by the recorder's stop) are counted and skipped."""
    at, unix, seq, sequence, off, size, boots = [], [], [], [], [], [], {}
    buf = bytearray()
    skipped, other = [], 0
    for path in paths:
        with open(path, "rb") as f:
            for n, line in enumerate(f):
                try:
                    row = json.loads(line)
                    if "payload_b64" not in row:
                        other += 1
                        continue
                    payload = base64.b64decode(row["payload_b64"], validate=True)
                    head, sep, pcm = payload.partition(b"\n")
                    meta = json.loads(head)
                    if not sep or meta["sample_rate"] != RATE or meta["channels"] != 1 or meta["sample_format"] != "S16LE":
                        raise ValueError(f"not 16 kHz mono S16LE: {meta}")
                    if len(pcm) % 2:
                        raise ValueError("odd PCM length")
                    h = row["header"]
                    values = (int(h["at_us"]), int(h["at_unix_ms"]), int(h["seq"]), int(meta["sequence"]))
                except (ValueError, KeyError, TypeError) as e:  # json.JSONDecodeError and binascii.Error are ValueErrors
                    skipped.append({"file": str(path), "line": n + 1, "bytes": len(line), "why": str(e)[:120]})
                    continue
                boots[h.get("boot")] = boots.get(h.get("boot"), 0) + 1
                at.append(values[0])
                unix.append(values[1])
                seq.append(values[2])
                sequence.append(values[3])
                off.append(len(buf) // 2)
                size.append(len(pcm) // 2)
                buf += pcm
    fr = {k: np.array(v, np.int64) for k, v in
          (("at", at), ("unix", unix), ("seq", seq), ("sequence", sequence), ("off", off), ("size", size))}
    if len(boots) > 1:
        print(f"warning: {len(boots)} boots in the recording ({boots}); times are kept in file order", flush=True)
        order = np.arange(len(at))
    else:
        order = np.lexsort((fr["at"], fr["seq"]))
    keep = np.ones(len(order), bool)
    keep[1:] = fr["seq"][order][1:] != fr["seq"][order][:-1]
    order = order[keep]
    fr = {k: v[order] for k, v in fr.items()}
    pcm = np.frombuffer(bytes(buf), "<i2")
    return fr, pcm, {"lines_skipped": skipped, "lines_not_audio": other, "duplicate_messages": int((~keep).sum()),
                     "boots": {str(k): v for k, v in boots.items()}}


def rolling_median(x: np.ndarray, w: int, ahead: bool) -> np.ndarray:
    """The median of the w values before each (ahead False: i-w .. i-1) or from it on (i .. i+w-1),
    edges padded with the end values; worked out in chunks so a day's frames fit in memory."""
    n = len(x)
    pad = np.concatenate([np.full(w, x[0]), x, np.full(w, x[-1])])
    out = np.empty(n)
    for a in range(0, n, 200_000):
        b = min(n, a + 200_000)
        s = a + w if ahead else a
        win = np.lib.stride_tricks.sliding_window_view(pad[s : s + (b - a) + w - 1], w)
        out[a:b] = np.median(win, axis=1)
    return out


def build_track(fr: dict, pcm: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray, list, dict]:
    """One continuous track. A gap is a persistent step in at_us against the samples so far: the
    median residual over the 50 frames after a frame against the 50 before it rises by 15 ms or more
    (a lone late frame, or a stall and a burst after it, is not a step). Its length is the step in
    whole frames; the payload's sequence is checked against it and reported. Returns the track, each
    frame's first sample in it and its at_us, the gaps as (first sample, samples) and the report."""
    at, seq, sizes = fr["at"], fr["sequence"], fr["size"]
    n = len(at)
    if np.any(np.diff(at) <= 0):
        bad = np.flatnonzero(np.diff(at) <= 0)
        print(f"warning: at_us not increasing at {len(bad)} frames (first {bad[:5].tolist()})", flush=True)
    before = np.concatenate([[0], np.cumsum(sizes)[:-1]])
    # Residual of at_us against the sample count, in ms (a slow drift between the lamp's clock and
    # the microphone's is left in; a gap is a step).
    r = (at - at[0]) / 1000.0 - before / (RATE / 1000.0)
    w = 50
    step = rolling_median(r, w, ahead=True) - rolling_median(r, w, ahead=False)
    cand = step >= 15.0
    gaps = []
    i = 0
    while i < n:
        if cand[i]:
            j = i
            while j + 1 < n and cand[j + 1]:
                j += 1
            # The frame at which at_us jumps, within the run: the largest single diff.
            lo, hi = max(1, i - w), min(n, j + w)
            k = lo + int(np.argmax(np.diff(at[lo - 1 : hi]))) if hi > lo else i
            missing = int(round(step[i : j + 1].max() / (FRAME / (RATE / 1000.0))))
            if missing > 0:
                gaps.append({"frame": int(k), "missing_frames": missing, "at_us": int(at[k]),
                             "at_us_diff_ms": round(float(at[k] - at[k - 1]) / 1000, 1),
                             "sequence_step": int(seq[k] - seq[k - 1])})
            i = j + 1
        else:
            i += 1
    seq_steps = np.diff(seq)
    jumps = np.flatnonzero(seq_steps != 1)
    seq_jumps = [{"frame": int(k + 1), "sequence_step": int(seq_steps[k]), "at_us_diff_ms": round(float(at[k + 1] - at[k]) / 1000, 1)}
                 for k in jumps[:200]]
    insert = np.zeros(n, np.int64)
    for g in gaps:
        insert[g["frame"]] += g["missing_frames"] * FRAME
    starts = before + np.cumsum(insert)
    total = int(starts[-1] + sizes[-1])
    track = np.zeros(total, np.int16)
    # Copied in runs: frames whose samples follow on in both the buffer and the track.
    src = fr["off"]
    brk = np.flatnonzero((np.diff(src) != sizes[:-1]) | (np.diff(starts) != sizes[:-1])) + 1
    bounds = np.concatenate([[0], brk, [n]])
    for a, b in zip(bounds[:-1], bounds[1:]):
        length = int(starts[b - 1] + sizes[b - 1] - starts[a])
        track[starts[a] : starts[a] + length] = pcm[src[a] : src[a] + length]
    r2 = (at - at[0]) / 1000.0 - starts / (RATE / 1000.0)
    fit = np.polyfit(starts / RATE, r2, 1)
    left = r2 - np.polyval(fit, starts / RATE)
    gap_spans = [(int(starts[g["frame"]]) - g["missing_frames"] * FRAME, g["missing_frames"] * FRAME) for g in gaps]
    report = {
        "frames": int(n), "samples": total, "seconds": round(total / RATE, 3),
        "first_at_us": int(at[0]), "last_at_us": int(at[-1]),
        "at_us_minus_unix_s": round(float(np.median(at / 1e6 - fr["unix"] / 1e3)), 1),
        "frame_sizes": {str(k): int(v) for k, v in zip(*np.unique(sizes, return_counts=True))},
        "at_us_diff_ms": {q: round(float(v) / 1000, 2) for q, v in zip(["min", "p1", "median", "p99", "max"],
                                                                        np.percentile(np.diff(at), [0, 1, 50, 99, 100]))},
        "gaps_by_at_us": gaps,
        "zeros_filled_s": round(sum(g["missing_frames"] for g in gaps) * FRAME / RATE, 3),
        "sequence_jumps": seq_jumps, "sequence_jumps_count": int(len(jumps)),
        "drift_at_us_vs_samples_ppm": round(float(fit[0]) * 1000, 1),
        "residual_after_fill_ms": {"std": round(float(left.std()), 2), "min": round(float(left.min()), 2), "max": round(float(left.max()), 2)},
    }
    return track, starts, at, gap_spans, report


def sample_at(t_us: float, starts: np.ndarray, at: np.ndarray) -> float:
    """The track sample heard at a lamp CLOCK_MONOTONIC time (by the frames' at_us), extrapolated
    at 16 kHz outside the recording."""
    if t_us <= at[0]:
        return float(starts[0] - (at[0] - t_us) * RATE / 1e6)
    if t_us >= at[-1]:
        return float(starts[-1] + (t_us - at[-1]) * RATE / 1e6)
    return float(np.interp(t_us, at, starts))


# ---------------------------------------------------------------------------------------------
# Alignment


def read_wav(path: Path) -> np.ndarray:
    with wave.open(str(path), "rb") as w:
        assert (w.getframerate(), w.getnchannels(), w.getsampwidth()) == (RATE, 1, 2), path
        return np.frombuffer(w.readframes(w.getnframes()), "<i2").copy()


def write_wav(path: Path, x: np.ndarray) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(np.asarray(x, "<i2").tobytes())


BAND = butter(4, [200, 5000], "bandpass", fs=RATE, output="sos")


def band(x: np.ndarray) -> np.ndarray:
    return sosfiltfilt(BAND, np.asarray(x, np.float64))


def ncc(y: np.ndarray, t: np.ndarray) -> np.ndarray:
    """Normalised cross-correlation of template t at every lag in y: <t, y[L:L+n]> / (|t| |y[L:L+n]|)."""
    n = len(t)
    if len(y) < n:
        return np.zeros(0)
    num = fftconvolve(y, t[::-1], mode="valid")
    e = np.concatenate([[0.0], np.cumsum(y * y)])
    den = np.sqrt(np.maximum(e[n:] - e[:-n], 1e-9)) * (np.linalg.norm(t) + 1e-12)
    return num / den


def find_chirp(region_b: np.ndarray, near: int, chirp_b: np.ndarray) -> dict:
    """The chirp's best match within SEARCH_S of `near` (indices into the region)."""
    lo = max(0, near - int(SEARCH_S * RATE))
    hi = min(len(region_b), near + int(SEARCH_S * RATE) + len(chirp_b))
    c = ncc(region_b[lo:hi], chirp_b)
    if not len(c):
        return {"found": None}
    k = int(np.argmax(c))
    side = c.copy()
    side[max(0, k - 160) : k + 160] = -np.inf  # 10 ms either side of the peak
    second = float(side.max()) if np.isfinite(side).any() else 0.0
    return {"found": lo + k, "peak": float(c[k]), "second": second, "ratio": float(c[k] / max(second, 1e-6))}


def envelope(x: np.ndarray, hop: int = 160) -> np.ndarray:
    n = len(x) // hop
    p = np.mean(np.asarray(x[: n * hop], np.float64).reshape(n, hop) ** 2, axis=1)
    return 10 * np.log10(p + 1.0)


def active_level(x: np.ndarray) -> float:
    """mix.py's active level (dBFS on the 16-bit scale): frames within 30 dB of the loudest 5 %."""
    x = np.asarray(x, np.float64)
    hop = RATE // 100
    n = len(x) // hop
    if n == 0:
        return -120.0
    power = np.mean(x[: n * hop].reshape(n, hop) ** 2, axis=1) / 32768.0**2
    loud = np.percentile(power, 95)
    if loud <= 1e-12:
        return -120.0
    return float(10 * np.log10(np.mean(power[power >= loud * 10 ** (-30 / 10)]) + 1e-20))


def power_db(x: np.ndarray) -> float:
    return float(10 * np.log10(np.mean(np.asarray(x, np.float64) ** 2) / 32768.0**2 + 1e-20))


def robust_line(x: np.ndarray, y: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """A line y = a x + b (lag in ms against the clip's offset in s) fitted, then refitted without
    the points more than 10 ms or three robust deviations off it, whichever is more (three times).
    Clip lags scatter by a few ms about the line even where two measures (this correlation and
    GCC-PHAT) agree, so the line, not each clip's own lag, places the cuts."""
    keep = np.ones(len(x), bool)
    fit = np.array([0.0, float(np.median(y))])
    for _ in range(3):
        if keep.sum() >= 3:
            fit = np.polyfit(x[keep], y[keep], 1)
        elif keep.sum() >= 1:
            fit = np.array([0.0, float(np.median(y[keep]))])
        res = y - np.polyval(fit, x)
        mad = float(np.median(np.abs(res[keep] - np.median(res[keep])))) * 1.4826 if keep.any() else 0.0
        keep = np.abs(res) <= max(10.0, 3 * mad)
    return fit, keep


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("recording", nargs="+")
    p.add_argument("--session", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--blocks-dir", default="blocks")
    p.add_argument("--index", default="index.tsv")
    p.add_argument("--plays", default="plays.tsv")
    p.add_argument("--blocks", default="", help="only these blocks (comma-separated numbers)")
    p.add_argument("--no-track-wav", action="store_true", help="do not write track.wav (a day of it is 3 GB)")
    a = p.parse_args()
    session, out = Path(a.session), Path(a.out)
    out.mkdir(parents=True, exist_ok=True)

    fr, pcm, info = read_frames(a.recording)
    track, starts, at, gap_spans, report = build_track(fr, pcm)
    del pcm
    report.update(info)
    report["recording"] = a.recording
    if not a.no_track_wav:
        write_wav(out / "track.wav", track)
    # Each frame's first sample in the track beside its stamps: to place anything timed on the lamp
    # (its journal, say) on the track.
    np.savez(out / "timeline.npz", track_sample=starts, at_us=at, at_unix_ms=fr["unix"], seq=fr["seq"], sequence=fr["sequence"])
    print(f"track: {report['seconds']} s from {report['frames']} frames; {len(report['gaps_by_at_us'])} gaps by at_us "
          f"({report['zeros_filled_s']} s of zeros), {report['sequence_jumps_count']} sequence jumps, "
          f"{len(info['lines_skipped'])} lines skipped; at_us drifts {report['drift_at_us_vs_samples_ppm']} ppm against the samples",
          flush=True)
    gap_first = np.array([g[0] for g in gap_spans], np.int64)
    gap_end = np.array([g[0] + g[1] for g in gap_spans], np.int64)

    def has_gap(a0: int, a1: int) -> bool:
        return bool(np.any((gap_first < a1) & (gap_end > a0))) if len(gap_first) else False

    plays = {int(r["block"]): r for r in csv.DictReader(open(session / a.plays), delimiter="\t")}
    index = list(csv.DictReader(open(session / a.index, encoding="utf-8"), delimiter="\t"))
    by_block: dict[int, list[dict]] = {}
    for r in index:
        by_block.setdefault(int(r["block"]), []).append(r)
    wanted = sorted(set(by_block) | set(plays)) if not a.blocks else [int(b) for b in a.blocks.split(",")]

    block_rows, clip_rows = [], []
    pad = int((SEARCH_S + 1.0) * RATE)
    for b in wanted:
        clips = sorted(by_block.get(b, []), key=lambda r: int(r["clip_in_block"]))

        def outside(why: str) -> None:
            for r in clips:
                clip_rows.append(dict(r, original=r["file"], lamp_file="", note=why))

        if b not in plays:
            outside("not played yet (not in the plays)")
            continue
        played = read_wav(session / a.blocks_dir / f"block-{b:03d}.wav").astype(np.float64)
        chirp_b = band(played[: int(CHIRP_S * RATE)])
        t_us = int(plays[b]["lamp_mono_ns_before"]) / 1000.0
        near = int(round(sample_at(t_us, starts, at)))
        row = {"block": b, "level": plays[b]["level"], "predicted_s": round(near / RATE, 3)}
        # The block's own stretch of the track, filtered once.
        r0 = max(0, near - pad)
        r1 = min(len(track), near + len(played) + pad)
        if r1 - r0 < int(CHIRP_S * RATE) + 2 * int(SEARCH_S * RATE) or near - int(SEARCH_S * RATE) >= len(track):
            print(f"block {b}: predicted at {near / RATE:.1f} s, outside the recording", flush=True)
            row.update(found_s="", after_predicted_ms="", chirp_peak="", chirp_ratio="")
            block_rows.append(row)
            outside("outside the recording")
            continue
        region = track[r0:r1].astype(np.float64)
        region_b = band(region)
        ch = find_chirp(region_b, near - r0, chirp_b)
        c0 = r0 + ch["found"]  # the chirp's first sample in the track
        row.update(found_s=round(c0 / RATE, 4), after_predicted_ms=round((c0 - near) / RATE * 1000, 1),
                   chirp_peak=round(ch["peak"], 3), chirp_ratio=round(ch["ratio"], 2))
        lags, corrs, offs = [], [], []
        for r in clips:
            off = float(r["offset_s"])
            k0 = int(round(off * RATE))
            ln = int(round(float(r["duration_s"]) * RATE))
            orig = band(played[k0 : k0 + ln])
            pred = c0 + k0 - r0
            lo = pred - int(CLIP_SEARCH_S * RATE)
            hi = pred + int(CLIP_SEARCH_S * RATE) + ln
            offs.append(off)
            if lo < 0 or hi > len(region_b) or c0 + k0 + ln > len(track):
                lags.append(np.nan)
                corrs.append(np.nan)
                continue
            c = ncc(region_b[lo:hi], orig)
            k = int(np.argmax(c))
            lags.append((lo + k - pred) / RATE * 1000)
            corrs.append(float(c[k]))
        lags, corrs, offs = np.array(lags), np.array(corrs), np.array(offs)
        ok = np.isfinite(lags) & (corrs >= WEAK_CORR)
        if ok.sum() >= 3:
            fit, keep = robust_line(offs[ok], lags[ok])
            used = np.flatnonzero(ok)[keep]
            res = lags[used] - np.polyval(fit, offs[used])
            row.update(fit_clips=int(len(used)), lag_at_chirp_ms=round(float(fit[1]), 2),
                       drift_ppm=round(float(fit[0]) * 1000, 1),
                       residual_rms_ms=round(float(np.sqrt(np.mean(res ** 2))), 2),
                       residual_max_ms=round(float(np.abs(res).max()), 2))
        else:
            fit = np.array([0.0, 0.0])
            row.update(fit_clips=int(ok.sum()), lag_at_chirp_ms="", drift_ppm="", residual_rms_ms="", residual_max_ms="")
        row["clips_in_recording"] = int(np.isfinite(lags).sum())
        row["clips_weak"] = int((np.isfinite(lags) & ~ok).sum())
        print(f"block {b} (level {plays[b]['level']}): chirp {row['after_predicted_ms']} ms after the plays' time, peak "
              f"{row['chirp_peak']} (x{row['chirp_ratio']} the next); clips' line {row.get('lag_at_chirp_ms')} ms + "
              f"{row.get('drift_ppm')} ppm, residual rms {row.get('residual_rms_ms')} ms, max {row.get('residual_max_ms')} ms "
              f"over {row['fit_clips']}; {row['clips_weak']} weak of {row['clips_in_recording']}", flush=True)
        floor_parts = []
        for r, off, lag, corr in zip(clips, offs, lags, corrs):
            k0 = int(round(off * RATE))
            ln = int(round(float(r["duration_s"]) * RATE))
            start = c0 + k0 + int(round(float(np.polyval(fit, off)) / 1000 * RATE))
            a0, a1 = start - int(LEAD_S * RATE), start + ln + int(TAIL_S * RATE)
            out_row = dict(r)
            out_row["original"] = r["file"]
            if a0 < r0 or a1 > r1:
                out_row.update(lamp_file="", note="outside the recording")
                clip_rows.append(out_row)
                continue
            # The lamp's own floor: 0.25 to 0.55 s after the clip (the next starts 0.6 s after it).
            f0, f1 = start + ln + int(0.25 * RATE), start + ln + int(0.55 * RATE)
            if f1 <= r1 and not has_gap(f0, f1):
                floor_parts.append(track[f0:f1])
            rel = Path("lamp-clips") / r["file"]
            write_wav(out / rel, track[a0:a1])
            span = region[start - r0 : start - r0 + ln]
            pre = region[start - r0 - int(0.3 * RATE) : start - r0 - int(0.05 * RATE)]
            # The envelope (10 ms log power) of the original against the cut at the cut's place.
            e1, e2 = envelope(played[k0 : k0 + ln]), envelope(span)
            m = min(len(e1), len(e2))
            env = float(np.corrcoef(e1[:m], e2[:m])[0, 1]) if m > 3 and e1[:m].std() > 0 and e2[:m].std() > 0 else float("nan")
            lvl, floor = active_level(span), power_db(pre)
            fit_off = (lag - float(np.polyval(fit, off))) if np.isfinite(lag) else float("nan")
            # The correlation at the cut's own place (its best within 15 ms of the line).
            j = int(0.015 * RATE)
            near_c = ncc(region_b[start - r0 - j : start - r0 + ln + j], band(played[k0 : k0 + ln]))
            corr_cut = float(near_c.max()) if len(near_c) else float("nan")
            # Weak: little likeness at the cut's place, or the envelope unlike the original's.
            weak = (not np.isfinite(corr_cut)) or corr_cut < WEAK_CORR or not (env >= WEAK_ENV)
            out_row.update(
                lamp_file=str(rel), cut_start_s=round(a0 / RATE, 5), lag_ms=round(float(lag), 2) if np.isfinite(lag) else "",
                fit_off_ms=round(float(fit_off), 2) if np.isfinite(fit_off) else "", corr_best=round(float(corr), 3),
                corr=round(corr_cut, 3), env_corr=round(env, 3), level_dbfs=round(lvl, 1), floor_dbfs=round(floor, 1),
                snr_db=round(lvl - floor, 1), clipped_samples=int(np.sum(np.abs(track[start : start + ln].astype(np.int32)) >= 32767)),
                gap_inside=int(has_gap(a0, a1)), weak=int(weak), note="")
            clip_rows.append(out_row)
        if floor_parts:
            # Kept as pieces of 0.3 s laid end to end (lampeval.py takes them apart again).
            write_wav(out / "floors" / f"block-{b:03d}.wav", np.concatenate(floor_parts))
            row["floor_pieces"] = len(floor_parts)
            row["floor_dbfs"] = round(power_db(np.concatenate(floor_parts).astype(np.float64)), 1)
        block_rows.append(row)

    (out / "decode.json").write_text(json.dumps(report, indent=1))
    cols = ["block", "level", "predicted_s", "found_s", "after_predicted_ms", "chirp_peak", "chirp_ratio", "fit_clips",
            "lag_at_chirp_ms", "drift_ppm", "residual_rms_ms", "residual_max_ms", "clips_in_recording", "clips_weak",
            "floor_pieces", "floor_dbfs"]
    with open(out / "blocks.tsv", "w") as f:
        w = csv.DictWriter(f, cols, delimiter="\t", lineterminator="\n", extrasaction="ignore")
        w.writeheader()
        w.writerows(block_rows)
    first = ["lamp_file", "original"] + [c for c in index[0].keys() if c != "file"]
    extra = ["cut_start_s", "lag_ms", "fit_off_ms", "corr_best", "corr", "env_corr", "level_dbfs", "floor_dbfs", "snr_db",
             "clipped_samples", "gap_inside", "weak", "note"]
    with open(out / "lamp-manifest.tsv", "w", encoding="utf-8") as f:
        w = csv.DictWriter(f, first + extra, delimiter="\t", lineterminator="\n", extrasaction="ignore", restval="")
        w.writeheader()
        w.writerows(clip_rows)
    got = [r for r in clip_rows if r.get("lamp_file")]
    print(f"{len(got)} clips cut ({sum(int(r['weak']) for r in got)} flagged weak, "
          f"{sum(int(r['gap_inside']) for r in got)} with a gap inside, {sum(int(r['clipped_samples']) > 0 for r in got)} clipped), "
          f"{len(clip_rows) - len(got)} not cut (outside the recording or not played yet); {out}")


if __name__ == "__main__":
    main()
