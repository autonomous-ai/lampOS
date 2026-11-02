"""Room-like augmentation toward lamp-4ace's mic2.

The mic2 path measured on the rP validation calls (channel_mag.py) is very
muffled: a high-pass near 120 Hz, a notch near 500 Hz, -13 dB at 1 kHz,
-18 dB at 3 kHz, -26 dB at 4 kHz, -31 dB above 5 kHz; its floor is about
-80 dBFS (no room hiss to speak of), calls peak near -20 dBFS.
Each copy gets: that response (or a generic band-limit) with a random
smooth tilt, a synthetic small-room reverb, a lost onset now and then, a
level, and under it room sound from the live recording's training gaps or the
lamp's own voice.
"""
import numpy as np
from scipy.signal import fftconvolve, butter, sosfilt

W = '/Users/autonomous/lamp-name-round/work/'
SR = 16000

def _minphase(mag_lin, n=512):
    """Minimum-phase FIR of length n from a magnitude on rfft(n_fft) bins."""
    n_fft = 2 * (len(mag_lin) - 1)
    logm = np.log(np.maximum(mag_lin, 1e-5))
    cep = np.fft.irfft(logm, n_fft)
    w = np.zeros(n_fft); w[0] = 1; w[1:n_fft // 2] = 2; w[n_fft // 2] = 1
    h = np.fft.irfft(np.exp(np.fft.rfft(cep * w)), n_fft)
    return h[:n] * np.hanning(2 * n)[n:]

class Room:
    def __init__(self, seed, noise_pool, own_pool):
        self.rng = np.random.default_rng(seed)
        f, db = np.load(W + 'mic2_mag.npy')
        # smooth over ~1/6 octave so the FIR does not carry estimation noise
        sm = np.copy(db)
        for i in range(len(f)):
            lo, hi = f[i] / 1.06, f[i] * 1.06
            m = (f >= lo) & (f <= hi)
            sm[i] = np.mean(db[m]) if m.any() else db[i]
        self.f, self.db = f, sm
        self.noise_pool = noise_pool      # list of float32 arrays (real mic2 room sound)
        self.own_pool = own_pool          # list of float32 arrays (the lamp's own voice, clean)

    def channel(self):
        r = self.rng
        if r.random() < 0.75:
            db = self.db.copy()
            # random smooth tilt / ripple in log frequency, +-4 dB
            lf = np.log2(np.maximum(self.f, 50) / 50)
            k = r.normal(0, 1, 4)
            db += 2.0 * (k[0] * np.sin(lf * 0.8 + k[1]) + k[2] * np.sin(lf * 1.9 + k[3])) / 1.4
            db += r.uniform(-4, 4) * (lf / lf.max())        # overall tilt
            db = np.clip(db, -60, 6)
            h = _minphase(10 ** (db / 20))
        else:
            # generic band-limit: high-pass 80-300 Hz, low-pass 1.5-5 kHz
            hp = butter(2, r.uniform(80, 300), 'highpass', fs=SR, output='sos')
            lp = butter(int(r.integers(2, 6)), r.uniform(1500, 5000), 'lowpass', fs=SR, output='sos')
            imp = np.zeros(512); imp[0] = 1
            h = sosfilt(lp, sosfilt(hp, imp))
        return h

    def reverb(self):
        r = self.rng
        rt60 = r.uniform(0.15, 0.6)
        n = int(SR * min(0.7, rt60 * 1.2))
        t = np.arange(n) / SR
        tail = r.normal(0, 1, n) * np.exp(-6.9 * t / rt60)
        tail[: int(0.002 * SR)] = 0
        drr = r.uniform(-2, 12)   # direct-to-reverberant, dB
        tail *= np.sqrt(1.0 / (np.sum(tail ** 2) * 10 ** (drr / 10)))
        h = tail; h[0] += 1.0
        for _ in range(int(r.integers(2, 6))):            # early reflections
            d = int(r.uniform(0.003, 0.025) * SR); h[d] += r.uniform(-0.5, 0.5)
        return h

    def path(self, x):
        y = fftconvolve(x, self.reverb())[: len(x) + 4000]
        return fftconvolve(y, self.channel())[: len(y)]

    def pick(self, pool, n):
        a = pool[int(self.rng.integers(len(pool)))]
        if len(a) <= n:
            return np.resize(a, n)
        s = int(self.rng.integers(0, len(a) - n)); return a[s: s + n]

def p90_level(x):
    n = len(x) // 1600 * 1600
    if n == 0:
        return -120.0
    r = np.sqrt(np.mean(x[:n].reshape(-1, 1600) ** 2, 1))
    return 20 * np.log10(np.percentile(r, 90) / 32768 + 1e-12)

def augment(room, x, onset_s=None, pre_s=2.2):
    """One room copy of clean speech x (16 kHz, int16 scale). Returns (y, offset_samples):
    y has pre_s seconds of room sound before x's start; offset is where x begins in y."""
    r = room.rng
    x = x.astype(np.float64)
    if onset_s is not None and r.random() < 0.3:          # the microphone loses the onset
        o = int(onset_s * SR); L = int(r.uniform(0.03, 0.15) * SR)
        ramp = np.ones(len(x)); seg = ramp[o: o + L]
        ramp[o: o + L] = np.linspace(0, 1, len(seg))
        x = x * ramp
    y = room.path(x)
    y *= 10 ** ((r.uniform(-28, -13) - p90_level(y)) / 20)
    pre = int(pre_s * SR)
    y = np.concatenate([np.zeros(pre), y, np.zeros(int(0.3 * SR))])
    n = len(y)
    if room.noise_pool and r.random() < 0.6:
        b = room.pick(room.noise_pool, n).astype(np.float64)
        bl = p90_level(b)
        if bl > -100:
            b *= 10 ** ((p90_level(y) + r.uniform(-30, -6) - bl) / 20)
        y += b
    if room.own_pool and r.random() < 0.3:              # the lamp's own voice under it
        o = room.path(room.pick(room.own_pool, n).astype(np.float64))[:n]
        o *= 10 ** ((p90_level(y) + r.uniform(-18, -2) - p90_level(o)) / 20)
        y += o
    y += r.normal(0, 1, n) * 32768 * 10 ** (r.uniform(-88, -72) / 20)
    return np.clip(y, -32768, 32767).astype(np.float32), pre
