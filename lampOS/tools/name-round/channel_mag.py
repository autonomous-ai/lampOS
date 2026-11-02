"""Average magnitude response of the iMac -> room -> mic2 path (lamp-4ace),
from the four rP training runs' calls (not the validation runs) against their clean kit clips: the ratio of
long-term spectra over each call's first piece, robust to alignment and
phase. Writes work/mic2_mag.npy: (freqs, dB) on a 513-bin grid."""
import sys; sys.path.insert(0, '/Users/autonomous/lamp-name-round')
import numpy as np, wave
from scipy.signal import welch, resample_poly
from real_data import rp_sessions

def readwav(p):
    w = wave.open(p); sr = w.getframerate()
    x = np.frombuffer(w.readframes(w.getnframes()), '<i2').astype(np.float64)
    return resample_poly(x, 16000, sr) if sr != 16000 else x

Px = 0; Py = 0; Pn = 0
for s in [r for r in rp_sessions() if r['name'] in ('rP-0017', 'rP-0141', 'rP-0355', 'rP-2221')]:
    a = s['audio'].astype(np.float64)
    for p in s['plays']:
        x = readwav(f"/Users/autonomous/lamp-voice-data/prompts/eleven/wav/{p['id']}.wav")[int(0.2*16000):int(0.95*16000)]
        s0 = int(p['s0']) + int(0.515 * 16000)
        y = a[s0 + int(0.2*16000): s0 + int(0.95*16000)]
        n = a[int(p['s0']) - 16000: int(p['s0']) - 4000]
        f, A = welch(x, 16000, nperseg=1024); _, B = welch(y, 16000, nperseg=1024); _, C = welch(n, 16000, nperseg=1024)
        Px = Px + A; Py = Py + B; Pn = Pn + C
mag = 10*np.log10((Py - Pn).clip(1e-12) / Px)
mag -= mag[(f > 150) & (f < 600)].max()
np.save('/Users/autonomous/lamp-name-round/work/mic2_mag.npy', np.stack([f, mag]))
for fr in [60, 100, 150, 200, 300, 400, 500, 700, 1000, 1500, 2000, 2500, 3000, 3500, 4000, 5000, 6000, 7000, 7900]:
    i = np.argmin(np.abs(f - fr)); print('%5d Hz %6.1f dB   (noise before the call %6.1f dB)' % (fr, mag[i], 10*np.log10(Pn[i]+1e-12)))
