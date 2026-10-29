#!/usr/bin/env python3
"""The four quality rows of docs/build-rows.md, scored from one repeatable played run on whatever build the lamp runs.

usage: quality_rows.py run LABEL [--stops N]   (night.py, cutin.sh, the stop scene, then score; plays sound, ~70 min)
       quality_rows.py score LABEL             (scores the three outputs already in $PLAYED_OUT)
       quality_rows.py selftest                (the scorer on built-in fixtures: no lamp, nothing played)

`run` plays, in order: night.py LABEL-night (every scene), cutin.sh LABEL-cutin 1 1, and the stop scene below, then
scores. Nothing on the lamp is changed (no switches, no drop-ins). It takes no lamp lock: on a shared lamp run it under
lamp-lock.sh <lamp> run, as the kit's other scripts. The stop scene, N trials (default 10): a quiet lamp (as night.py),
"Hey Lamp, tell me a long story about <topic>." at Mac volume 45, the story's first write (none in 14 s: void), 1.5 s,
then a stop line at volume 90. Stopped: the story's turn_finished as user_interrupted within 2.0 s of the stop clip's
end (the gap is from its start). Not stopped and still speaking: "Lamp, stop." once more, "stopped on the second",
still a miss. Saves $PLAYED_OUT/quality/LABEL-stop.json, and the score as LABEL.txt and LABEL.json."""
import hashlib, json, os, re, shutil, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.environ.get('PLAYED_OUT') or os.path.expanduser('~/lamp-played')
ROWS = [('Cutting in', 0.75), ('Stop words', 0.75), ('Knows when you have finished', 0.85), ('Not spoken to', 0.90)]
TOPICS = ['a lighthouse keeper', 'a lost kite', 'a fox who learned to swim', 'a clockmaker', 'a sailing trip',
          'a baker who could not sleep', 'a snowy mountain village', 'a robot gardener', 'a river otter', 'a night train']
STOPS = ['Lamp, stop.', 'Stop, stop.', 'Stop now.', 'Okay, stop.']
STOP_WINDOW = 2.0   # seconds after the stop clip's end for the story's user_interrupted


def volume(v): subprocess.run(['/usr/bin/osascript', '-e', f'set volume output volume {v}'], check=False)


def clip(text):
    """The line spoken by say (Daniel), silence trimmed both ends and levelled to -20 LUFS, cached by its text's sha."""
    d = f'{OUT}/quality/clips'; os.makedirs(d, exist_ok=True)
    wav = f'{d}/{hashlib.sha256(text.encode()).hexdigest()[:16]}.wav'
    if not os.path.exists(wav):
        aiff = wav[:-4] + '.aiff'
        subprocess.run(['/usr/bin/say', '-v', 'Daniel', '-o', aiff, text], check=True)
        trim = 'silenceremove=start_periods=1:start_threshold=-50dB'
        subprocess.run(['ffmpeg', '-loglevel', 'error', '-y', '-i', aiff, '-af',
                        f'{trim},areverse,{trim},areverse,loudnorm=I=-20:TP=-2', '-ar', '24000', '-ac', '1', wav], check=True)
        os.remove(aiff)
    return wav


def play(path):
    start = time.time(); subprocess.run(['/usr/bin/afplay', path], check=True); return start, time.time()


def stopped_after(ev, story, start, end):
    """The gap from the stop clip's start to the story's user_interrupted, if it came within STOP_WINDOW of the end."""
    deadline = end + STOP_WINDOW
    while True:
        ev.pump()
        for r in ev.rows:
            if (start <= r['_mac'] <= deadline and r.get('kind') == 'turn_finished' and r.get('outcome') == 'user_interrupted'
                    and (story is None or r.get('turn') == story)):
                return round(r['_mac'] - start, 2)
        if time.time() > deadline: return None
        time.sleep(0.05)


def stop_scene(label, n):
    sys.path.insert(0, HERE); import night
    ev = night.Events(); time.sleep(2); trials, played = [], []
    try:
        for i in range(n):
            # night.run_scene's quiet wait: nothing playing, and QUIET_BEFORE s since anything played or was said.
            while True:
                ev.pump()
                last = max([r['_mac'] for r in ev.rows if r.get('kind') in ('speaker_first_write', 'speech_final_sample_retired')]
                           + played + [0])
                if not ev.speaking() and time.time() - last >= night.QUIET_BEFORE: break
                time.sleep(0.5)
            topic, line = TOPICS[i % len(TOPICS)], STOPS[i % len(STOPS)]
            t = {'topic': topic, 'stop_line': line, 'result': 'no story, void', 'gap_s': None}
            volume(45); _, ask_end = play(clip(f'Hey Lamp, tell me a long story about {topic}.')); played.append(ask_end)
            first = ev.wait_for('speaker_first_write', ask_end, 14.0)
            if first is not None:
                w = next(r for r in ev.rows if r['_mac'] == first and r.get('kind') == 'speaker_first_write')
                story = (w.get('owner') or {}).get('turn', w.get('turn'))
                time.sleep(1.5); volume(90); s, e = play(clip(line)); volume(45); played.append(e)
                t['gap_s'] = stopped_after(ev, story, s, e)
                t['result'] = 'stopped' if t['gap_s'] is not None else 'not stopped'
                if t['gap_s'] is None and ev.speaking():
                    volume(90); s, e = play(clip('Lamp, stop.')); volume(45); played.append(e)
                    if stopped_after(ev, story, s, e) is not None: t['result'] = 'stopped on the second'
            trials.append(t); print(f"stop {i + 1}/{n}: {topic}; {line!r}: {t['result']} {t['gap_s'] or ''}", flush=True)
    finally:
        volume(45); ev.close(); night.stop_on_lamp()
    os.makedirs(f'{OUT}/quality', exist_ok=True)
    json.dump({'label': label, 'at': time.strftime('%Y-%m-%d %H:%M'), 'trials': trials}, open(f'{OUT}/quality/{label}-stop.json', 'w'), indent=1)


def read(path, how):
    try:
        with open(path) as f: return how(f)
    except (OSError, ValueError): return None


def row(name, bar, passes, trials, detail):
    if not trials: return {'row': name, 'bar': bar, 'measured': False, 'line': f'{name}  not measured  bar {bar}', 'detail': detail}
    sc = round(passes / trials, 2)
    return {'row': name, 'bar': bar, 'measured': True, 'passes': passes, 'trials': trials, 'score': sc, 'pass': sc >= bar,
            'line': f"{name}  {passes}/{trials} = {sc:.2f}  bar {bar}  {'PASS' if sc >= bar else 'below'}", 'detail': detail}


def score(label, out=OUT):
    night = read(f'{out}/night/{label}-night.json', json.load)
    cutin = read(f'{out}/scenarios/cutin-{label}-cutin.txt', lambda f: f.read().splitlines())
    stop = read(f'{out}/quality/{label}-stop.json', json.load)
    bars = dict(ROWS); rows = []
    # Cutting in: each summary entry's first word is its decision; a cut-in should commit, an aside resume.
    got, detail = [], []
    for l in cutin or []:
        m = re.match(r'^(aside|cutin)\s+(\d+) s: (.*)', l)
        if m:
            for e in m.group(3).split('; '):
                d = e.split()[0] if e.split() else ''
                got.append(d == {'cutin': 'commit', 'aside': 'resume'}[m.group(1)]); detail.append(f'{m.group(1)} {m.group(2)} s: {d}')
        elif l.startswith('idle:'): detail.append(l)
    rows.append(row('Cutting in', bars['Cutting in'], sum(got), len(got), detail if cutin is not None else ['no cutin report']))
    # Stop words: stopped on the first stop line, of the trials where a story began.
    real = [t for t in (stop or {}).get('trials', []) if t['result'] != 'no story, void']
    gaps = [t['gap_s'] for t in real if t['result'] == 'stopped']
    rows.append(row('Stop words', bars['Stop words'], len(gaps), len(real),
                    [f'gaps {gaps}', f"stopped on the second {sum(t['result'] == 'stopped on the second' for t in real)}",
                     f"void {len((stop or {}).get('trials', [])) - len(real)}"] if stop is not None else ['no stop scene']))
    nr = (night or {}).get('rows', [])
    ends = [r for r in nr if r.get('kind') in ('pause', 'long')]
    ok = [r for r in ends if r.get('answered') and len(r.get('pieces') or []) == 1]
    rows.append(row('Knows when you have finished', bars['Knows when you have finished'], len(ok), len(ends),
                    [f"missed {r['scene']} {r['id']} answered={r.get('answered')} pieces={r.get('pieces')}" for r in ends if r not in ok]
                    if night is not None else ['no night round']))
    desk = [r for r in nr if r.get('kind') == 'desk']
    held = [r for r in desk if not r.get('answered')]
    rows.append(row('Not spoken to', bars['Not spoken to'], len(held), len(desk),
                    [f"answered {r['scene']} {r['id']}" for r in desk if r.get('answered')] if night is not None else ['no night round']))
    build = next((l[len('build: '):] for l in cutin or [] if l.startswith('build: ')), 'not read')
    first = ((night or {}).get('summary') or {}).get('first_ms_median')
    txt = [f'quality {label}  build: {build}  time to first sound: {f"{first} ms" if first is not None else "not measured"}']
    for r in rows: txt += [r['line']] + ['    ' + d for d in r['detail']]
    os.makedirs(f'{out}/quality', exist_ok=True)
    open(f'{out}/quality/{label}.txt', 'w').write('\n'.join(txt) + '\n')
    json.dump({'label': label, 'build': build, 'first_ms_median': first, 'rows': rows}, open(f'{out}/quality/{label}.json', 'w'), indent=1)
    print('\n'.join(txt)); return rows


def selftest():
    d = tempfile.mkdtemp(); os.makedirs(f'{d}/night'); os.makedirs(f'{d}/scenarios'); os.makedirs(f'{d}/quality')
    def n(kind, answered, pieces, scene='s'): return {'scene': scene, 'id': 'x', 'kind': kind, 'answered': answered, 'pieces': pieces}
    json.dump({'summary': {'first_ms_median': 1420},
               'rows': [n('pause', True, [1]), n('pause', True, [1, 2]), n('long', True, [3]), n('long', False, []), n('ask', True, [4]),
                        n('desk', False, []), n('desk', True, [5]), n('desk', False, [], 'after1')]}, open(f'{d}/night/t-night.json', 'w'))
    open(f'{d}/scenarios/cutin-t-cutin.txt', 'w').write(
        'cutin t-cutin  2026-10-29T00:00:00Z  lamp x\nbuild: /opt/lampOS/voice/current/lamp-voice sha256 0123456789abcdef\n'
        'idle: 1 of 1 stories; 0 false yields in 1.50 min of reply heard: 0.00 per min\n'
        'aside  3 s: resume (onset->yield 300 ms, ->decision 900 ms, answer accepted - / acoustic - ms)\n'
        'cutin  3 s: commit (onset->yield 280 ms, ->decision 700 ms, answer accepted 1200 / acoustic 1300 ms)\n'
        'aside  8 s: commit (onset->yield 310 ms, ->decision 650 ms, answer accepted - / acoustic - ms)\n'
        'cutin  8 s: none (onset->yield - ms, ->decision - ms, answer accepted - / acoustic - ms)\n')
    trial = lambda res, gap=None: {'topic': 't', 'stop_line': 'Stop now.', 'result': res, 'gap_s': gap}
    json.dump({'trials': [trial('stopped', 0.9), trial('stopped', 1.4), trial('stopped on the second'), trial('not stopped'),
                          trial('no story, void')]}, open(f'{d}/quality/t-stop.json', 'w'))
    r = {x['row']: x for x in score('t', d)}
    assert (r['Cutting in']['passes'], r['Cutting in']['trials']) == (2, 4) and not r['Cutting in']['pass']
    assert (r['Stop words']['passes'], r['Stop words']['trials'], r['Stop words']['score']) == (2, 4, 0.5)
    assert 'stopped on the second 1' in r['Stop words']['detail'] and 'gaps [0.9, 1.4]' in r['Stop words']['detail']
    assert (r['Knows when you have finished']['passes'], r['Knows when you have finished']['trials']) == (2, 4)
    assert (r['Not spoken to']['passes'], r['Not spoken to']['trials']) == (2, 3)
    out = open(f'{d}/quality/t.txt').read()
    assert 'build: /opt/lampOS/voice/current/lamp-voice sha256 0123456789abcdef' in out and 'time to first sound: 1420 ms' in out
    assert 'idle: 1 of 1 stories' in out
    os.remove(f'{d}/quality/t-stop.json')
    r = {x['row']: x for x in score('t', d)}
    assert not r['Stop words']['measured'] and 'Stop words  not measured' in r['Stop words']['line'] and r['Cutting in']['measured']
    shutil.rmtree(d); print('quality_rows.py selftest: ok')


def main():
    a = sys.argv[1:]
    if a[:1] == ['selftest']: return selftest()
    if len(a) < 2 or a[0] not in ('run', 'score'): sys.exit(__doc__)
    label = a[1]
    if a[0] == 'score': return score(label)
    n = int(a[a.index('--stops') + 1]) if '--stops' in a else 10
    if not os.environ.get('LAMP_HOST'):
        print('quality_rows.py: LAMP_HOST is not set (README.md); nothing was played', file=sys.stderr); sys.exit(2)
    t = time.localtime()
    if t.tm_wday < 5 and 12 <= t.tm_hour < 14 and os.environ.get('LAMP_PLAY_AT_LUNCH') != 'yes':
        print('quality_rows.py: not playing between 12:00 and 14:00 local, when the office rests', file=sys.stderr); sys.exit(2)
    env = {**os.environ, 'PLAYED_OUT': OUT}
    # A failed part leaves its row "not measured"; the rest still run.
    for cmd in ([sys.executable, HERE + '/night.py', label + '-night'], [HERE + '/cutin.sh', label + '-cutin', '1', '1']):
        rc = subprocess.run(cmd, env=env).returncode
        if rc: print(f'quality_rows.py: {os.path.basename(cmd[1] if cmd[0] == sys.executable else cmd[0])} exited {rc}', file=sys.stderr)
    stop_scene(label, n)
    score(label)


if __name__ == '__main__':
    main()
