"""Clean speech already on this Mac: the ElevenLabs kit and the macOS `say` prompts."""
import re, wave, glob
import numpy as np
from scipy.signal import resample_poly
from real_data import is_call, mentions

ELEVEN = '/Users/autonomous/lamp-voice-data/prompts/eleven/'
GEN = '/Users/autonomous/lamp-voice-data/prompts/generated/'

def readwav(p):
    w = wave.open(p); sr = w.getframerate(); ch = w.getnchannels()
    x = np.frombuffer(w.readframes(w.getnframes()), '<i2').astype(np.float32)
    if ch > 1: x = x.reshape(-1, ch).mean(1)
    return resample_poly(x, 16000, sr).astype(np.float32) if sr != 16000 else x

def onset(x, below=35):
    n = len(x) // 160 * 160
    e = np.sqrt(np.mean(x[:n].reshape(-1, 160) ** 2, 1)); db = 20 * np.log10(e + 1e-3)
    return float(np.argmax(db > db.max() - below) * 0.01)

def hey(text):
    return bool(re.match(r'^\s*(\[\[[^\]]*\]\]\s*)?(hey|hi|ok|okay|oh)\b', text, re.I))

def eleven():
    out = []
    for l in open(ELEVEN + 'lines.tsv'):
        if l.startswith('#'): continue
        f = l.rstrip('\n').split('\t')
        cid, voice, kind, text = f[0], f[1], f[2], f[3]
        p = ELEVEN + 'wav/' + cid + '.wav'
        try:
            x = readwav(p)
        except FileNotFoundError:
            continue
        text = re.sub(r'\{[\d.]+\}\s*', '', text)
        out.append(dict(src='eleven', id=cid, voice=voice, text=text, audio=x, call=is_call(text),
                        mention=mentions(text) and not is_call(text), onset=onset(x), hey=hey(text)))
    return out

def generated():
    out = []; seen = set()
    for l in open(GEN + 'index.tsv'):
        f = l.rstrip('\n').split('\t')
        if len(f) < 4: continue
        h, voice, rate, text = f[0], f[1], f[2], f[3]
        key = (voice, rate, text)
        if key in seen: continue
        seen.add(key)
        try:
            x = readwav(GEN + h + '.wav')
        except (FileNotFoundError, wave.Error, EOFError):
            continue
        if len(x) < 1600: continue
        out.append(dict(src='say', id=h, voice=voice, text=text, audio=x, call=is_call(text),
                        mention=mentions(text) and not is_call(text), onset=onset(x), hey=hey(text)))
    return out

if __name__ == '__main__':
    from collections import Counter
    e = eleven(); g = generated()
    print('eleven', len(e), Counter((d['call'], d['mention']) for d in e))
    print('say', len(g), Counter((d['call'], d['mention']) for d in g), Counter(d['voice'] for d in g if d['call']))
    print('say total seconds', sum(len(d['audio']) for d in g) / 16000)
    print([d['text'] for d in g if d['call']][:12])
