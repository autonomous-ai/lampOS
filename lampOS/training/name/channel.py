"""The lamp's own channel, measured with test sounds, for a rescue model trained on public speech
(#1546, step 1 of the home lamp's rescue model; the first builder's yes, 19 Oct 10:3x).

The rescue model heard 28 of 32 calls in voices it never heard on lamp-4ace (#1605's live check),
but it was trained on the office's recordings, which are never published. What it learned that the
installed model did not is the lamp's channel: the office speaker through the room into mic2, and
mic2 after the lamp's canceller. That channel can be measured without a voice: an exponential sine
sweep gives the room's impulse response (Farina's deconvolution), pink noise the level each band
arrives at, and the gaps the floor of the room and the lamp. Public speech (LibriTTS, CC BY 4.0,
and the open generated calls) passed through the measured channel is then training data that
carries no person and no office.

    python channel.py make OUT_DIR
        sweep.wav (three 8 s exponential sweeps, 40 Hz to 7.6 kHz, 2 s of silence after each) and
        pink.wav (20 s of pink noise), 16 kHz, mono, 16 bits, at -12 dBFS peak.

    python channel.py play OUT_DIR PLAYED.tsv        (on the Mac, beside the lamp, afplay)
        Plays sweep.wav, then pink.wav, after 5 s of silence, and writes each one's start to
        PLAYED.tsv as `UNIX_MS<TAB>name`. Record the lamp meanwhile:
        `lamp-bus record DIR mic2/audio --seconds 60` on lamp-4ace, the office empty.

    python channel.py analyse --played PLAYED.tsv RECORD.jsonl --out channel.npz
        Finds each sweep in the recording by its own cross-correlation (the two clocks need not
        agree to better than a second), deconvolves each, and keeps their mean impulse response
        (10 ms before the direct sound to 0.5 s after); the gain in each third-octave band from
        the pink noise against the file; and the floor (the recording's quietest second, as a
        spectrum). Prints the response's decay (RT60 from the Schroeder curve), how alike the
        three sweeps were, the band gains and the floor's level. The delay is not kept: the two
        clocks' offset is in it, and the response starts at the direct sound.

    python channel.py apply --channel channel.npz --floor-db -60 IN.wav... --out DIR
        Each WAV through the impulse response, then the floor added at its measured shape and the
        level given, peak-normalised to the input's peak: training data as mic2 would hear it.

Numbers only: the recording is read and dropped, and channel.npz holds an impulse response, gains
and a floor spectrum, nothing anyone said. The office's own recordings stay a private test set
(#1546), never this script's input beyond the test sounds.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import time
import wave
from pathlib import Path

import numpy as np

RATE = 16_000
SWEEP_S, SWEEP_GAP_S, SWEEPS = 8.0, 2.0, 3
SWEEP_F0, SWEEP_F1 = 40.0, 7_600.0
PINK_S = 20.0
PEAK = 10 ** (-12 / 20)
IR_S = 0.5
BANDS = [100 * 2 ** (k / 3) for k in range(0, 19)]  # 100 Hz to 6.4 kHz, third octaves


def write_wav(path: Path, samples: np.ndarray) -> None:
    pcm = np.clip(np.round(samples * 32767), -32768, 32767).astype("<i2")
    with wave.open(str(path), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(RATE)
        out.writeframes(pcm.tobytes())


def read_wav(path: Path) -> np.ndarray:
    with wave.open(str(path), "rb") as source:
        if (source.getframerate(), source.getnchannels(), source.getsampwidth()) != (RATE, 1, 2):
            raise SystemExit(f"{path}: not 16 kHz mono 16-bit")
        return np.frombuffer(source.readframes(source.getnframes()), "<i2").astype(np.float64) / 32768


def sweep() -> np.ndarray:
    """One exponential sweep, faded in and out over 50 ms."""
    t = np.arange(int(SWEEP_S * RATE)) / RATE
    k = np.log(SWEEP_F1 / SWEEP_F0)
    x = np.sin(2 * np.pi * SWEEP_F0 * SWEEP_S / k * (np.exp(t * k / SWEEP_S) - 1))
    fade = int(0.05 * RATE)
    ramp = np.sin(np.linspace(0, np.pi / 2, fade)) ** 2
    x[:fade] *= ramp
    x[-fade:] *= ramp[::-1]
    return x * PEAK


def inverse(x: np.ndarray) -> np.ndarray:
    """Farina's inverse filter: the sweep reversed, falling 6 dB an octave, so x * inverse is an
    impulse."""
    t = np.arange(len(x)) / RATE
    k = np.log(SWEEP_F1 / SWEEP_F0)
    f = x[::-1] * np.exp(-t * k / SWEEP_S)
    peak = np.abs(fftconvolve(x, f)).max()
    return f / peak


def pink(seconds: float, seed: int = 0) -> np.ndarray:
    n = int(seconds * RATE)
    spectrum = np.fft.rfft(np.random.default_rng(seed).standard_normal(n))
    freqs = np.fft.rfftfreq(n, 1 / RATE)
    spectrum[1:] /= np.sqrt(freqs[1:])
    spectrum[0] = 0
    x = np.fft.irfft(spectrum, n)
    return x / np.abs(x).max() * PEAK


def fftconvolve(a: np.ndarray, b: np.ndarray) -> np.ndarray:
    n = len(a) + len(b) - 1
    size = 1 << (n - 1).bit_length()
    return np.fft.irfft(np.fft.rfft(a, size) * np.fft.rfft(b, size), size)[:n]


def band_levels(x: np.ndarray) -> np.ndarray:
    """Mean power in each third-octave band, in dB."""
    spectrum = np.abs(np.fft.rfft(x)) ** 2
    freqs = np.fft.rfftfreq(len(x), 1 / RATE)
    out = []
    for centre in BANDS:
        lo, hi = centre / 2 ** (1 / 6), centre * 2 ** (1 / 6)
        inside = (freqs >= lo) & (freqs < hi)
        out.append(10 * np.log10(spectrum[inside].mean() + 1e-20))
    return np.array(out)


def make(out: Path) -> None:
    out.mkdir(parents=True, exist_ok=True)
    one = np.concatenate([sweep(), np.zeros(int(SWEEP_GAP_S * RATE))])
    write_wav(out / "sweep.wav", np.tile(one, SWEEPS))
    write_wav(out / "pink.wav", pink(PINK_S))
    print(f"wrote {out}/sweep.wav ({SWEEPS} x {SWEEP_S + SWEEP_GAP_S:.0f} s) and pink.wav ({PINK_S:.0f} s)")


def play(out: Path, played: Path) -> None:
    time.sleep(5)
    with open(played, "w", encoding="utf-8") as log:
        for name in ("sweep", "pink"):
            start = time.time()
            log.write(f"{start * 1000:.0f}\t{name}\n")
            log.flush()
            subprocess.run(["afplay", str(out / f"{name}.wav")], check=True)
            time.sleep(2)
    print(f"played; starts in {played}")


def find(signal: np.ndarray, near: int, x: np.ndarray, within: int) -> int:
    """Where `x` begins in `signal`, searched `within` samples either side of `near`."""
    lo = max(0, near - within)
    hi = min(len(signal), near + within + len(x))
    corr = fftconvolve(signal[lo:hi], x[::-1])[len(x) - 1 :]
    return lo + int(np.argmax(np.abs(corr[: hi - lo - len(x) + 1])))


def analyse(played: Path, records: list[str], out: Path) -> None:
    from lamp import recording  # this folder's reader of `lamp-bus record`

    starts = {}
    for row in open(played, encoding="utf-8"):
        at, name = row.rstrip("\n").split("\t")[:2]
        starts[name] = float(at) / 1000
    samples, first = np.zeros(0), None
    for path in records:
        s, t0 = recording(path)
        if first is None:
            samples, first = s.astype(np.float64) / 32768, t0
    if first is None or not len(samples):
        raise SystemExit("no sound in the recording")
    one, inv = sweep(), inverse(sweep())
    period = int((SWEEP_S + SWEEP_GAP_S) * RATE)
    at = find(samples, int((starts["sweep"] - first) * RATE), one, RATE)
    # A sweep's own match finds it only to within its lowest notes' period, so each one is
    # deconvolved from 0.25 s before, and its response taken from the deconvolution's own peak
    # (the direct sound), 10 ms before it to IR_S after.
    irs, lead, pre = [], int(0.25 * RATE), int(0.01 * RATE)
    for k in range(SWEEPS):
        begin = max(0, find(samples, at + k * period, one, int(0.05 * RATE)) - lead)
        part = samples[begin : begin + lead + len(one) + int(IR_S * RATE)]
        full = fftconvolve(part, inv)
        peak = int(np.argmax(np.abs(full[: len(one) - 1 + 2 * lead])))
        irs.append(full[peak - pre : peak + int(IR_S * RATE)])
    ir = np.mean(irs, axis=0)
    # The repeat: how alike the three were.
    alike = min(float(np.corrcoef(irs[0], other)[0, 1]) for other in irs[1:])
    energy = np.cumsum((ir ** 2)[::-1])[::-1]
    decay = 10 * np.log10(energy / energy[0] + 1e-20)
    fit = np.where((decay <= -5) & (decay >= -25))[0]
    rt60 = (-60 / np.polyfit(fit / RATE, decay[fit], 1)[0]) if len(fit) > 10 else float("nan")
    pink_file = pink(PINK_S)
    p_at = find(samples, int((starts["pink"] - first) * RATE), pink_file, RATE)
    heard = samples[p_at : p_at + len(pink_file)]
    gains = band_levels(heard) - band_levels(pink_file)
    seconds = [samples[i : i + RATE] for i in range(0, len(samples) - RATE, RATE)]
    quiet = min(seconds, key=lambda s: float(np.mean(s ** 2)))
    floor = np.abs(np.fft.rfft(quiet)) ** 2
    floor_db = 10 * np.log10(np.mean(quiet ** 2) + 1e-20)
    np.savez(out, ir=ir, bands=np.array(BANDS), gains_db=gains, floor_power=floor, floor_db=floor_db,
             sweeps_alike=alike)
    print(f"channel: RT60 {rt60 * 1000:.0f} ms, "
          f"the three sweeps alike to {alike:.3f}; floor {floor_db:.1f} dBFS")
    print("  band gains (dB): " + " ".join(f"{c:.0f}:{g:+.1f}" for c, g in zip(BANDS, gains)))
    print(f"wrote {out}")


def apply(channel: Path, floor_db: float, wavs: list[str], out: Path) -> None:
    c = np.load(channel)
    ir, floor = c["ir"], c["floor_power"]
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(0)
    for path in wavs:
        x = read_wav(Path(path))
        y = fftconvolve(x, ir)[: len(x)]
        y *= np.abs(x).max() / (np.abs(y).max() + 1e-12)
        # The floor at its measured shape, at the level asked for.
        spectrum = np.fft.rfft(rng.standard_normal(len(y)))
        shape = np.interp(np.fft.rfftfreq(len(y), 1 / RATE), np.fft.rfftfreq(2 * (len(floor) - 1), 1 / RATE),
                          np.sqrt(floor))
        noise = np.fft.irfft(spectrum * shape, len(y))
        noise *= 10 ** (floor_db / 20) / (np.sqrt(np.mean(noise ** 2)) + 1e-20)
        write_wav(out / Path(path).name, np.clip(y + noise, -1, 1))
    print(f"wrote {len(wavs)} file(s) to {out}")


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="command", required=True)
    s = sub.add_parser("make")
    s.add_argument("out")
    s = sub.add_parser("play")
    s.add_argument("out")
    s.add_argument("played")
    s = sub.add_parser("analyse")
    s.add_argument("--played", required=True)
    s.add_argument("records", nargs="+")
    s.add_argument("--out", required=True)
    s = sub.add_parser("apply")
    s.add_argument("--channel", required=True)
    s.add_argument("--floor-db", type=float, default=-60.0)
    s.add_argument("wavs", nargs="+")
    s.add_argument("--out", required=True)
    a = p.parse_args()
    if a.command == "make":
        make(Path(a.out))
    elif a.command == "play":
        play(Path(a.out), Path(a.played))
    elif a.command == "analyse":
        analyse(Path(a.played), a.records, Path(a.out))
    else:
        apply(Path(a.channel), a.floor_db, a.wavs, Path(a.out))


if __name__ == "__main__":
    sys.exit(main())
