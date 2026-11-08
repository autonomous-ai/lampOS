#!/usr/bin/env python3
"""One loudness for every played clip: each WAV gets one linear gain to TARGET LUFS, under a -1 dBTP ceiling.

usage: level.py DIR OUT [--target N]    (every *.wav in DIR into OUT, listed in OUT/level.json; N defaults to -20)

Loudness is BS.1770-4's integrated loudness: K-weighting at the file's own rate, 400 ms blocks every 100 ms, a -70 LUFS
absolute gate and a -10 LU relative gate. True peak is read 4x oversampled. A clip that would pass the ceiling at the
target gets the gain that puts its true peak at the ceiling instead, and is marked capped: it is never limited, so its
onsets keep their shape. One gain moves every sample, so the length and a lead of zeros stay as they were; the ceiling
is checked on the written 16-bit samples. Python's standard library only (README.md "Night rounds", #5540)."""
import argparse, hashlib, json, math, os, struct, sys, tempfile, wave

TARGET = -20.0    # LUFS integrated
CEILING = -1.0    # dBTP
# The meter's version, kept in level.json: a change to how clips are metered or levelled raises it, and the copies made
# by an older meter are levelled again.
METER_VERSION = 1


def sha256(path):
    with open(path, 'rb') as f: return hashlib.sha256(f.read()).hexdigest()


def read(path):
    """(channels as lists of floats in -1..1, rate, the file's wave params); 16-bit PCM only, as the kits are."""
    with wave.open(path) as w:
        p = w.getparams()
        if p.sampwidth != 2: raise ValueError(f'{path}: {8 * p.sampwidth}-bit samples; level.py reads 16-bit PCM only')
        raw = w.readframes(p.nframes)
    s = struct.unpack(f'<{len(raw) // 2}h', raw)
    return [[v / 32768.0 for v in s[c::p.nchannels]] for c in range(p.nchannels)], p.framerate, p


def biquad(x, b0, b1, b2, a1, a2):
    y = []; x1 = x2 = y1 = y2 = 0.0; out = y.append
    for v in x:
        o = b0 * v + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        x2, x1, y2, y1 = x1, v, y1, o; out(o)
    return y


def k_weight(x, rate):
    """BS.1770-4's two stages, its high shelf and its high pass, worked out for this rate (as libebur128 does)."""
    f0, gain, q = 1681.974450955533, 3.999843853973347, 0.7071752369554196
    k = math.tan(math.pi * f0 / rate); vh = 10 ** (gain / 20); vb = vh ** 0.4996667741545416
    a0 = 1 + k / q + k * k
    x = biquad(x, (vh + vb * k / q + k * k) / a0, 2 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0,
               2 * (k * k - 1) / a0, (1 - k / q + k * k) / a0)
    f0, q = 38.13547087602444, 0.5003270373238773
    k = math.tan(math.pi * f0 / rate); a0 = 1 + k / q + k * k
    return biquad(x, 1.0, -2.0, 1.0, 2 * (k * k - 1) / a0, (1 - k / q + k * k) / a0)


def loudness(chans, rate):
    """Integrated loudness in LUFS, or None for a clip with no block above the absolute gate. Every channel weighs 1
    (mono and stereo)."""
    step = rate // 10   # 100 ms: a 400 ms block is four steps, so blocks overlap by 75 %
    n = len(chans[0]); segs = [0.0] * (n // step)
    for c in chans:
        y = k_weight(c, rate)
        for i in range(len(segs)): segs[i] += sum(v * v for v in y[i * step:(i + 1) * step])
    if len(segs) < 4:   # shorter than one block: the whole clip is the block
        blocks = [sum(sum(v * v for v in k_weight(c, rate)) for c in chans) / n] if n else []
    else:
        blocks = [sum(segs[i:i + 4]) / (4 * step) for i in range(len(segs) - 3)]
    lk = lambda z: -0.691 + 10 * math.log10(z) if z > 0 else -math.inf
    kept = [z for z in blocks if lk(z) > -70.0]
    if not kept: return None
    rel = lk(sum(kept) / len(kept)) - 10.0
    kept = [z for z in kept if lk(z) > rel]
    return lk(sum(kept) / len(kept))


def _phases(taps=12, over=4):
    """A 4x interpolating low pass, cut at the input's Nyquist: a Kaiser-windowed sinc, split into its four phases."""
    n = taps * over; beta = 8.0; mid = (n - 1) / 2
    i0 = lambda v: sum(((v / 2) ** k / math.factorial(k)) ** 2 for k in range(30))
    h = []
    for i in range(n):
        t = (i - mid) / over
        sinc = 1.0 if t == 0 else math.sin(math.pi * t) / (math.pi * t)
        h.append(sinc * i0(beta * math.sqrt(1 - (2 * (i - mid) / (n - 1)) ** 2)) / i0(beta))
    phases = [h[p::over] for p in range(over)]
    return [[v / sum(ph) for v in ph] for ph in phases]   # each phase passes DC at 1


PHASES = _phases()


def true_peak(chans):
    """True peak in dBTP: the largest magnitude of the samples and of the signal 4x oversampled."""
    peak = 0.0
    for x in chans:
        peak = max(peak, max(map(abs, x), default=0.0))
        for h in PHASES:
            t = len(h); padded = [0.0] * (t - 1) + x + [0.0] * (t - 1)
            acc = [0.0] * (len(x) + t - 1)
            for k, c in enumerate(h):
                acc = [a + c * v for a, v in zip(acc, padded[t - 1 - k:])]
            peak = max(peak, max(map(abs, acc), default=0.0))
    return 20 * math.log10(peak) if peak > 0 else -math.inf


def measure(path):
    chans, rate, _ = read(path)
    return {'lufs': loudness(chans, rate), 'tp': true_peak(chans)}


def _scaled(chans, gain):
    """The 16-bit samples at one gain, interleaved, and the same read back as floats."""
    out = [[max(-32768, min(32767, round(v * gain * 32768.0))) for v in c] for c in chans]
    return out, [[v / 32768.0 for v in c] for c in out]


def _tmp(dst):
    """A temp file of its own beside dst, so two runs into one folder never write the same file."""
    d = os.path.dirname(os.path.abspath(dst)); os.makedirs(d, exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix='.' + os.path.basename(dst) + '.', suffix='.tmp', dir=d); os.close(fd)
    return tmp


def level(src, dst, target=TARGET, ceiling=CEILING):
    """Writes src to dst with one linear gain to `target` LUFS, or to the true-peak `ceiling` when that is lower
    (capped). The gain is taken down again if the written 16-bit samples read above the ceiling. A clip with nothing
    above the -70 LUFS gate is copied at 0 dB and marked unlevelled."""
    chans, rate, params = read(src)
    lufs_in = loudness(chans, rate); tp_in = true_peak(chans)
    capped = False
    if lufs_in is None:   # silence, or nothing above -70 LUFS: nothing to level by, so it is copied
        gain_db = 0.0
    else:
        gain_db = target - lufs_in
        if tp_in + gain_db > ceiling: gain_db = ceiling - tp_in; capped = True
    for _ in range(20):
        ints, floats = _scaled(chans, 10 ** (gain_db / 20))
        tp_out = true_peak(floats)
        if tp_out <= ceiling or lufs_in is None: break
        gain_db -= tp_out - ceiling + 0.005; capped = True
    else:
        raise RuntimeError(f'{src}: could not hold {ceiling} dBTP')
    frames = [v for frame in zip(*ints) for v in frame]
    tmp = _tmp(dst)
    try:
        with wave.open(tmp, 'wb') as w:
            w.setnchannels(params.nchannels); w.setsampwidth(2); w.setframerate(rate)
            w.writeframes(struct.pack(f'<{len(frames)}h', *frames))
        os.replace(tmp, dst)
    finally:
        if os.path.exists(tmp): os.remove(tmp)
    r2 = lambda v: None if v is None or v == -math.inf else round(v, 2)
    return {'source_sha256': sha256(src), 'played_sha256': sha256(dst), 'lufs_in': r2(lufs_in), 'gain_db': round(gain_db, 2),
            'lufs_out': r2(loudness(floats, rate)), 'tp_out': r2(tp_out), 'capped': capped,
            'unlevelled': lufs_in is None}


def level_dir(src_dir, out_dir, names=None, target=TARGET, ceiling=CEILING):
    """Levels the WAVs `names` (default: every *.wav) of src_dir into out_dir, listed in out_dir/level.json. A clip
    already there is kept while its source's sha, its own sha, the target, the ceiling and METER_VERSION are unchanged."""
    names = sorted(names if names is not None else (f for f in os.listdir(src_dir) if f.endswith('.wav')))
    at = os.path.join(out_dir, 'level.json')
    try:
        old = json.load(open(at))
        if (old.get('target'), old.get('ceiling'), old.get('meter')) != (target, ceiling, METER_VERSION): old = {}
    except (OSError, ValueError):
        old = {}
    clips = old.get('clips', {})
    for name in names:
        src, dst = os.path.join(src_dir, name), os.path.join(out_dir, name)
        had = clips.get(name)
        if had and os.path.exists(dst) and had['source_sha256'] == sha256(src) and had['played_sha256'] == sha256(dst): continue
        clips[name] = level(src, dst, target, ceiling)
    os.makedirs(out_dir, exist_ok=True)
    tmp = _tmp(at)
    try:
        with open(tmp, 'w') as f:
            json.dump({'target': target, 'ceiling': ceiling, 'meter': METER_VERSION, 'clips': clips}, f, indent=1, sort_keys=True)
        os.replace(tmp, at)
    finally:
        if os.path.exists(tmp): os.remove(tmp)
    return {n: clips[n] for n in names}


def main():
    ap = argparse.ArgumentParser(description='Level every WAV in DIR into OUT, one gain a clip, listed in OUT/level.json.')
    ap.add_argument('dir'); ap.add_argument('out'); ap.add_argument('--target', type=float, default=TARGET)
    a = ap.parse_args()
    if not os.path.isdir(a.dir): sys.exit(f'level.py: no folder {a.dir}')
    got = level_dir(a.dir, a.out, target=a.target)
    for name, r in got.items():
        print(f"{name:16} {r['lufs_in']} LUFS {r['gain_db']:+.2f} dB -> {r['lufs_out']} LUFS {r['tp_out']} dBTP{' capped' if r['capped'] else ''}{' unlevelled' if r['unlevelled'] else ''}")
    print(f'{a.out}/level.json: {len(got)} clips, {sum(r["capped"] for r in got.values())} capped')


if __name__ == '__main__':
    main()
