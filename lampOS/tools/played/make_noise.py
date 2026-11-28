#!/usr/bin/env python3
"""Background beds for voiceset.py's noisy-room scenes. Writes 16 kHz mono 16-bit WAVs under
$PLAYED_OUT/prompts/noise/ (PLAYED_OUT defaults to ~/lamp-played), 100 s each:
  babble.wav  six voices talking at once (macOS `say`), a cafe or an open office
  tv.wav      one newsreader with short pauses, a TV in the room
  fan.wav     brown noise, a desk fan or air conditioning
  music.wav   soft chords and a melody synthesised here (no recording, so nothing anyone owns)
  typing.wav  keyboard clicks at a typist's pace
Deterministic (a fixed seed), so every run plays the same beds. Rerun with --force to remake them."""
import os, subprocess, sys, wave
import numpy as np

RATE = 16_000
SECONDS = 100
OUT = (os.environ.get('PLAYED_OUT') or os.path.expanduser('~/lamp-played')) + '/prompts/noise'
rng = np.random.default_rng(20261125)

TEXTS = [
    'So I told him the meeting moved to Thursday, and he said that was fine, but then nobody sent the invite, '
    'and on Thursday morning half the team was still waiting in the other room wondering where everyone was.',
    'The new coffee place downstairs is actually really good. They do this oat milk thing that I did not expect '
    'to like, and the pastries are fresh every morning, though the line gets long after nine.',
    'Did you see the numbers from last quarter? Sales were up in the north but the south was flat, and the '
    'marketing budget is going to be the first thing they look at in the review next week.',
    'My daughter has a recital on Saturday, so I will be leaving early on Friday to help set up. She has been '
    'practising the same piece every evening for a month and honestly I can play it myself by now.',
    'We should probably book the venue before the end of the month, because last year we waited and the only '
    'place left was that hall with the terrible heating and the broken projector.',
    'I think the bug is in the cache. When the request comes back late it overwrites the newer value, and then '
    'the page shows the old price until someone refreshes it twice.',
]
VOICES = ['Samantha', 'Daniel', 'Karen', 'Moira', 'Rishi', 'Tessa']
NEWS = ('Good evening. Our top story tonight: heavy rain is expected across the north of the country through the '
        'weekend, with flood warnings in place for several river towns. In other news, the city council has approved '
        'the new bus routes, which will start running in the spring. Sports now: the home team won by three goals '
        'last night in front of a full stadium, their fourth win in a row. And finally, a local bakery has broken '
        'a record for the longest loaf of bread, measuring more than twenty metres. Stay with us after the break.')


def say(voice, text, path):
    subprocess.run(['/usr/bin/say', '-v', voice, '-r', '180', '-o', path, '--data-format=LEI16@16000', text], check=True)
    with wave.open(path) as w:
        return np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32)


def fit(x, n):
    """`x` repeated or cut to `n` samples."""
    if len(x) >= n:
        return x[:n]
    return np.tile(x, n // len(x) + 1)[:n]


def write(name, x, peak=0.5):
    x = x / (np.max(np.abs(x)) + 1e-9) * peak * 32767
    path = f'{OUT}/{name}.wav'
    with wave.open(path, 'w') as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(RATE)
        w.writeframes(x.astype(np.int16).tobytes())
    print(f'{path}  {len(x) / RATE:.0f} s')


def main():
    os.makedirs(OUT, exist_ok=True)
    force = '--force' in sys.argv
    n = RATE * SECONDS
    tmp = f'{OUT}/.tmp.wav'
    if force or not os.path.exists(f'{OUT}/babble.wav'):
        mix = np.zeros(n, dtype=np.float32)
        for voice, text in zip(VOICES, TEXTS):
            speech = say(voice, text + ' ' + text, tmp)
            start = int(rng.integers(0, RATE * 3))
            part = fit(np.concatenate([np.zeros(start, dtype=np.float32), speech]), n)
            mix += part * float(rng.uniform(0.6, 1.0))
        write('babble', mix)
    if force or not os.path.exists(f'{OUT}/tv.wav'):
        speech = say('Fred', NEWS, tmp)
        write('tv', fit(np.concatenate([speech, np.zeros(RATE * 2, dtype=np.float32)]), n))
    if force or not os.path.exists(f'{OUT}/fan.wav'):
        brown = np.cumsum(rng.normal(0, 1, n)).astype(np.float32)
        brown -= np.convolve(brown, np.ones(RATE // 4) / (RATE // 4), mode='same')  # no slow drift
        write('fan', brown, peak=0.4)
    if force or not os.path.exists(f'{OUT}/music.wav'):
        t = np.arange(n) / RATE
        chords = [(220.0, 277.2, 329.6), (196.0, 246.9, 293.7), (174.6, 220.0, 261.6), (196.0, 246.9, 329.6)]
        melody = [440.0, 493.9, 523.3, 587.3, 523.3, 493.9, 440.0, 392.0]
        x = np.zeros(n, dtype=np.float32)
        for i in range(n // (RATE * 2)):
            a, b = i * RATE * 2, (i + 1) * RATE * 2
            seg = t[a:b] - t[a]
            env = np.minimum(1, seg * 8) * np.exp(-seg * 0.6)
            for f in chords[i % 4]:
                x[a:b] += (0.25 * np.sin(2 * np.pi * f * seg) * env).astype(np.float32)
            for j in range(4):
                c, d = a + j * RATE // 2, a + (j + 1) * RATE // 2
                s2 = t[c:d] - t[c]
                x[c:d] += (0.35 * np.sin(2 * np.pi * melody[(i * 4 + j) % 8] * s2) * np.exp(-s2 * 4)).astype(np.float32)
        write('music', x, peak=0.45)
    if force or not os.path.exists(f'{OUT}/typing.wav'):
        x = np.zeros(n, dtype=np.float32)
        at = 0
        while at < n - RATE // 10:
            click = rng.normal(0, 1, RATE // 100).astype(np.float32) * np.exp(-np.arange(RATE // 100) / 25.0)
            x[at:at + len(click)] += click * float(rng.uniform(0.5, 1.0))
            at += int(rng.uniform(0.08, 0.25) * RATE)
            if rng.random() < 0.08:
                at += int(rng.uniform(0.5, 1.5) * RATE)  # a pause to think
        write('typing', x, peak=0.5)
    if os.path.exists(tmp):
        os.remove(tmp)


main()
