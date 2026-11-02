"""Real room recordings already on this Mac, cut into labelled stretches.

- V1 (13-18 Oct): lampOS V1 capture of the lamp's microphone (pre_aec) while
  macOS `say` lines were played from the iMac (lamp-voice-data/names/*).
- rP (24-25 Oct): lamp-4ace mic2/audio off the bus while ten kit "Lamp, {pause}
  question" lines were played (lamp-tools/onset/rP-*, night/rP-*.json). Used as
  the VALIDATION set, never trained on.
- live (31 Oct): the test (lamp-tools/namerec), never trained on except its
  even non-call gaps (see split_live_gaps).
"""
import base64, glob, json, os, re
import numpy as np

NAME_RE = re.compile(r"^\s*(\[\[[^\]]*\]\]\s*)?((hey|hi|ok|okay|oh|um|so|and)[ ,]+)?lamp\b", re.I)

def is_call(text):
    return bool(NAME_RE.match(text))

def mentions(text):
    return 'lamp' in text.lower()

def decode_bus(path):
    blocks = []; meta = []; n = 0
    for line in open(path):
        d = json.loads(line)
        if 'payload_b64' not in d:
            continue
        raw = base64.b64decode(d['payload_b64']); k = raw.index(b'\n')
        pcm = np.frombuffer(raw[k+1:], dtype='<i2')
        meta.append((n + len(pcm), d['header']['at_unix_ms'])); blocks.append(pcm); n += len(pcm)
    a = np.concatenate(blocks); m = np.array(meta, float)
    k, c = np.polyfit(m[:, 0], m[:, 1], 1)
    return a, (k, c)          # unix_ms = k * sample_index + c

def v1_sessions():
    out = []
    for d in sorted(glob.glob('/Users/autonomous/lamp-voice-data/names/*/')):
        caps = glob.glob(d + '*/capture-audio/pre_aec.pcm16le')
        if not caps or not os.path.exists(d + 'plays.tsv'):
            continue
        cap = caps[0]; ev = os.path.dirname(cap) + '/events.jsonl'
        a = np.fromfile(cap, dtype='<i2')
        pts = []
        for l in open(ev):
            e = json.loads(l)
            if e.get('kind') == 'capture' and e.get('frames'):
                pts.append((e['pre_aec_byte_offset'] / 2, e['meta']['read_completed_at_us']))
        pts = np.array(pts, float)
        k, c = np.polyfit(pts[:, 0], pts[:, 1], 1)   # at_us = k * sample + c
        plays = []
        for l in open(d + 'plays.tsv'):
            f = l.rstrip('\n').split('\t')
            t_us, dur_us, voice, kind, text = int(f[0]), int(f[1]), f[2], f[3], f[4]
            s0 = (t_us - c) / k; s1 = (t_us + dur_us - c) / k
            plays.append(dict(s0=s0, s1=s1, voice=voice, kind=kind, text=text, call=is_call(text), mention=mentions(text)))
        out.append(dict(name=os.path.basename(d.rstrip('/')), audio=a, plays=plays))
    return out

def rp_sessions():
    out = []
    for d in sorted(glob.glob('/Users/autonomous/lamp-tools/onset/rP-*/')):
        r = os.path.basename(d.rstrip('/'))
        bus = d + 'rec/mic2_audio.jsonl'; js = f'/Users/autonomous/lamp-voice-data/night/{r}.json'
        if not (os.path.exists(bus) and os.path.exists(js)):
            continue
        a, (k, c) = decode_bus(bus)
        plays = []
        for row in json.load(open(js))['rows']:
            s0 = (row['start'] * 1000 - c) / k; s1 = (row['end'] * 1000 - c) / k
            plays.append(dict(s0=s0, s1=s1, voice=row['voice'], id=row['id'], text=row['text'], call=is_call(row['text']),
                              mention=mentions(row['text']), lamp_scores=row.get('name_scores')))
        out.append(dict(name=r, audio=a, plays=plays))
    return out

if __name__ == '__main__':
    tot = 0; calls = 0
    for s in v1_sessions():
        n = len(s['audio']) / 16000; tot += n; c = sum(p['call'] for p in s['plays']); calls += c
        inside = all(0 <= p['s0'] < len(s['audio']) for p in s['plays'])
        print('V1', s['name'], '%.0fs' % n, 'plays', len(s['plays']), 'calls', c, 'in range', inside,
              'rms dBFS %.1f' % (20*np.log10(np.sqrt(np.mean(s['audio'].astype(float)**2))/32768)))
    print('V1 total s', tot, 'calls', calls)
    for s in rp_sessions():
        n = len(s['audio']) / 16000
        inside = all(0 <= p['s0'] < len(s['audio']) for p in s['plays'])
        print('rP', s['name'], '%.0fs' % n, 'plays', len(s['plays']), 'calls', sum(p['call'] for p in s['plays']), 'in range', inside,
              [p['id'] for p in s['plays']])
