#!/usr/bin/env python3
"""The desk kit's scene builder and scorer (desk-kit.sh, README.md "The desk kit").

usage: desk_kit.py scene TRIAL PLAN_OUT     print one trial's scenario.py steps, one a line; write its plan
       desk_kit.py score [--arm NAME] [--tsv PATH] [--marks LCC..] SCENARIO [SCENARIO ...]
       desk_kit.py selftest

scene: each ask of hesitant.tsv (its variant TRIAL, round robin), its un-named follow-up if it has one, then side talk
between colleagues: a clip from $DESK_KIT_AMI/clips.tsv (default ~/lamp-played/ami; columns file meeting start_s end_s
speakers overlap text, file relative to that folder) or, without one, a `say`-voiced exchange. Every line is one wav
made here before the scene starts, an ask's pauses exact silences inside it: between two scenario.py steps there is
a `say` and an ssh round trip, so a 0.6 s pause step came out as 3 to 10 s.
The plan has one row per PLAY line: mark (L to the lamp, C side talk), ask, keys, first part's word, seconds, text.

score: a scenario file's PLAY lines are matched in order to the plan beside it (SCENARIO.plan); without one, asks are
inferred from the PLAY lines (a line not ending in . ? ! goes on into the next; L when it names the lamp, unless
--marks gives the asks' marks in order). Empty HEARD and LAMP lines are ignored.
"""
import hashlib, os, re, statistics, subprocess, sys, tempfile, time, wave

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.environ.get('PLAYED_OUT') or os.path.expanduser('~/lamp-played')
RATE = os.environ.get('LAMP_SAY_RATE', '185')
ASKER = os.environ.get('DESK_KIT_ASKER', 'Daniel')
COLLEAGUE = os.environ.get('DESK_KIT_COLLEAGUE', 'Samantha')
AMI = os.environ.get('DESK_KIT_AMI') or os.path.expanduser('~/lamp-played/ami')
FILLERS = {'uh', 'um', 'hey', 'lamp', 'a', 'an', 'the', 'and', 'or', 'of', 'to', 'for', 'in', 'is', 'me', 'you', 'i',
           'it', 'do', 'does', 'what', 'whats', 'how', 'can', 'tell', 'like', 'some', 'my', 'your', 'today'}
# Side talk when there are no AMI clips: two colleagues at the desk, not to the lamp.
SIDE = [("Did you see the email from the client?", "Yes, they want the draft by Friday."),
        ("Can we push it to Monday?", "I'll ask them after lunch."),
        ("Do you want a coffee?", "Yes please, with milk."),
        ("The meeting moved to three.", "Okay, I'll update the calendar."),
        ("What do you think of the new logo?", "I like the blue one better."),
        ("Is the printer working again?", "No, it jammed this morning."),
        ("Who is presenting on Thursday?", "I think it's the design team."),
        ("Did the parcel arrive?", "It's at the front desk."),
        ("Are you going to the lunch talk?", "Only if there's pizza."),
        ("My laptop is so slow today.", "Try restarting it after the call."),
        ("The heating is on too high.", "I'll turn it down a bit."),
        ("Have you booked the flights yet?", "Not yet, prices went up.")]


def words(text):
    return [w.replace("'", '') for w in re.findall(r"[a-z']+", text.lower())]


def first_word(part):
    """The first part's word to look for in what was heard: its longest word that isn't a filler, else the name."""
    cands = [w for w in words(part) if w not in FILLERS]
    return max(cands, key=len) if cands else 'lamp'


def make_wav(voice, text):
    """text with {0.4} pauses -> one 16 kHz wav, made once and reused; returns (path, seconds)."""
    key = hashlib.sha256(f'desk|{voice}|{RATE}|{text}'.encode()).hexdigest()[:16]
    path = f'{OUT}/prompts/generated/{key}.wav'
    if not os.path.exists(path):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        frames = b''
        with tempfile.TemporaryDirectory() as tmp:
            for i, piece in enumerate(re.split(r'\{([\d.]+)\}', text)):
                if i % 2:
                    frames += b'\0\0' * int(16000 * float(piece)); continue
                if not piece.strip(): continue
                seg = f'{tmp}/{i}.wav'
                subprocess.run(['/usr/bin/say', '-v', voice, '-r', RATE, '-o', seg, '--data-format=LEI16@16000', piece.strip()], check=True)
                with wave.open(seg) as w: frames += w.readframes(w.getnframes())
        with wave.open(path, 'wb') as w:
            w.setnchannels(1); w.setsampwidth(2); w.setframerate(16000); w.writeframes(frames)
        with open(f'{OUT}/prompts/generated/index.tsv', 'a') as f: f.write(f'{key}\t{voice}\t{RATE}\t{text}\n')
    with wave.open(path) as w: return path, w.getnframes() / w.getframerate()


def read_tsv(path):
    with open(path) as f:
        return [l.rstrip('\n').split('\t') for l in f if l.strip() and not l.startswith('#')]


def scene(trial, plan_out):
    asks = {}
    for mark, ask, variant, keys, wait, text in read_tsv(os.path.join(HERE, 'hesitant.tsv')):
        a = asks.setdefault(ask, {'variants': [], 'then': None})
        row = (mark, keys, wait, text)
        if variant == 'then': a['then'] = row
        else: a['variants'].append(row)
    clips = []
    if os.path.exists(f'{AMI}/clips.tsv'):
        for r in read_tsv(f'{AMI}/clips.tsv'):
            if len(r) >= 4 and re.fullmatch(r'[\d.]+', r[2]):
                clips.append((r[0] if r[0].startswith('/') else f'{AMI}/{r[0]}', r[1], r[2], r[3]))
    steps, plan = [], []
    for j, (name, a) in enumerate(asks.items()):
        lines = [a['variants'][(trial - 1) % len(a['variants'])]] + ([a['then']] if a['then'] else [])
        # The follow-up is an ask of its own, so its request is never counted as the first's split.
        for (mark, keys, wait, text), ask_name in zip(lines, (name, f'{name}-then')):
            path, dur = make_wav(ASKER, text)
            label = re.sub(r'\s*\{[\d.]+\}\s*', ' ', text)
            first = first_word(text.split('{')[0]) if '{' in text else ''
            steps.append(f'wav|{path}|{wait}|{label}')
            plan.append((mark, ask_name, keys, first, '%.2f' % dur, f'wav: {label}'))
        k = (trial - 1) * len(asks) + j
        if clips:
            path, meeting, start, end = clips[k % len(clips)]
            label = f'ami {meeting} {start}-{end}'
            steps.append(f'wav|{path}|4|{label}')
            plan.append(('C', f'side{j}', '', '', '%.2f' % (float(end) - float(start)), f'wav: {label}'))
        else:
            for voice, said in zip((COLLEAGUE, ASKER), SIDE[k % len(SIDE)]):
                path, dur = make_wav(voice, said)
                steps.append(f'wav|{path}|4|{voice}: {said}')
                plan.append(('C', f'side{j}', '', '', '%.2f' % dur, f'wav: {voice}: {said}'))
    with open(plan_out, 'w') as f:
        f.write('#mark\task\tkeys\tfirst\tseconds\tplay\n')
        for r in plan: f.write('\t'.join(r) + '\n')
    print('\n'.join(steps))


LINE = re.compile(r'^\s*(-?\d+\.\d+)s (.{5}) ?(.*)$')


def parse(text):
    """-> [(t, kind, text)] for PLAY, HEARD and LAMP lines with words, and SOUND: scenario.py's speaker_first_write
    line ("[lamp starts speaking ...]"), when the lamp's answer first reached its speaker."""
    events = []
    for line in text.splitlines():
        m = LINE.match(line)
        if not m: continue
        kind, said = m.group(2).strip(), m.group(3).strip()
        if not kind and said.startswith('[lamp starts speaking'): kind = 'SOUND'
        if kind in ('PLAY', 'SOUND') or (kind in ('HEARD', 'LAMP') and said):
            events.append((float(m.group(1)), kind, said))
    return events


def infer_plan(plays, marks=None):
    rows, ask, n = [], None, 0
    for i, p in enumerate(plays):
        said = p.split(': ', 1)[-1]
        if ask is None:
            ask = f'ask{n}'; n += 1; texts = []
        texts.append(said)
        rows.append([None, ask, None, None, '%.2f' % (len(said.split()) * 60 / float(RATE)), p])
        if re.search(r'[.?!]$', said) or i == len(plays) - 1:
            whole = ' '.join(texts)
            content = sorted({w for w in words(whole) if w not in FILLERS}, key=lambda w: (-len(w), w))
            mark = marks[n - 1] if marks and n - 1 < len(marks) else ('L' if 'lamp' in words(whole) else 'C')
            first = first_word(texts[0]) if len(texts) > 1 else ''
            for r in rows:
                if r[1] == ask: r[0], r[2], r[3] = mark, ' '.join(content[:2]), first
            ask = None
    return [tuple(r) for r in rows]


def has_keys(heard, keys):
    hw = set(words(heard))
    return all(any(alt in hw for alt in k.split('/')) for k in keys.split())


def score_file(text, plan):
    """-> list of per-ask dicts."""
    events = parse(text)
    plays = [i for i, e in enumerate(events) if e[1] == 'PLAY']
    if len(plays) != len(plan):
        raise SystemExit(f'desk_kit.py: {len(plays)} PLAY lines but {len(plan)} plan rows')
    asks = []
    for k, (row, i) in enumerate(zip(plan, plays)):
        mark, ask, keys, first, dur, _ = row
        if asks and asks[-1]['ask'] == ask:
            a = asks[-1]
        else:
            a = {'ask': ask, 'mark': mark, 'keys': keys, 'first': first, 'start': i, 'text': []}
            asks.append(a)
        a['last'] = i; a['end_t'] = events[i][0] + float(dur or 0); a['text'].append(events[i][2].split(': ', 1)[-1])
    for n, a in enumerate(asks):
        stop = asks[n + 1]['start'] if n + 1 < len(asks) else len(events)
        seg = events[a['start'] + 1:stop]
        heard = [e[2] for e in seg if e[1] == 'HEARD']
        a['heard'] = heard
        a['spoke'] = any(e[1] == 'LAMP' for e in seg)
        a['split'] = len(heard) >= 2
        a['lost'] = bool(a['first'] and heard and a['first'] not in words(heard[-1]))
        # Answered whole: a LAMP line whose nearest HEARD before it (inside the ask's stretch) holds every key.
        a['whole'], last_heard = False, None
        for e in seg:
            if e[1] == 'HEARD': last_heard = e[2]
            elif e[1] == 'LAMP' and last_heard is not None and has_keys(last_heard, a['keys']): a['whole'] = True
        # The answer time ends at the answer's first sound on the lamp's speaker, not at its transcript, which comes
        # later and by a varying amount; no recorded first sound, no answer time.
        after = [e[0] for e in events[a['last'] + 1:stop] if e[1] == 'SOUND']
        a['answer_ms'] = round((after[0] - a['end_t']) * 1000) if after else None
    return asks


def plan_for(path, marks):
    if os.path.exists(path + '.plan'):
        return [tuple((r + [''] * 6)[:6]) for r in read_tsv(path + '.plan')]
    with open(path) as f: plays = [e[2] for e in parse(f.read()) if e[1] == 'PLAY']
    return infer_plan(plays, marks)


def summary(asks, arm, nfiles):
    L = [a for a in asks if a['mark'] == 'L']; C = [a for a in asks if a['mark'] == 'C']
    firsts = [a for a in L if a['first'] and a['heard']]
    ms = [a['answer_ms'] for a in L if a['answer_ms'] is not None]
    return {'arm': arm, 'trials': nfiles, 'asks': len(L), 'whole': sum(a['whole'] for a in L),
            'split': sum(a['split'] for a in L), 'first_lost': sum(a['lost'] for a in firsts), 'first_of': len(firsts),
            'side': len(C), 'side_let_out': sum(a['spoke'] for a in C), 'answers': len(ms),
            'median_ms': round(statistics.median(ms)) if ms else '', 'max_ms': max(ms) if ms else ''}


def score(argv):
    arm, tsv, marks, files = 'arm', None, None, []
    while argv:
        a = argv.pop(0)
        if a == '--arm': arm = argv.pop(0)
        elif a == '--tsv': tsv = argv.pop(0)
        elif a == '--marks': marks = argv.pop(0)
        else: files.append(a)
    every = []
    for path in files:
        with open(path) as f: asks = score_file(f.read(), plan_for(path, marks))
        every += asks
        print(f'== {os.path.basename(path)}')
        for a in asks:
            if a['mark'] == 'L':
                flags = [w for w, on in (('whole', a['whole']), ('SPLIT', a['split']), ('FIRST-LOST', a['lost']),
                                         ('unanswered', not a['spoke'])) if on]
            else:
                flags = ['LET-OUT' if a['spoke'] else 'left']
            ms = '' if a['answer_ms'] is None or a['mark'] != 'L' else f" {a['answer_ms']} ms"
            print(f"{a['mark']} {' / '.join(a['text'])[:60]:<60} heard {len(a['heard'])}{ms} {' '.join(flags)}")
    s = summary(every, arm, len(files))
    print(f"arm {s['arm']} ({s['trials']} trials): to the lamp answered whole {s['whole']} of {s['asks']}; split {s['split']} of "
          f"{s['asks']}; first words lost {s['first_lost']} of {s['first_of']}; side talk let out {s['side_let_out']} of "
          f"{s['side']}; answer time median {s['median_ms']} ms, max {s['max_ms']} ms ({s['answers']} answers)")
    if tsv:
        new = not os.path.exists(tsv)
        with open(tsv, 'a') as f:
            if new: f.write('time\t' + '\t'.join(s) + '\n')
            f.write(time.strftime('%Y-%m-%dT%H:%M:%S') + '\t' + '\t'.join(str(v) for v in s.values()) + '\n')
    return s


FIXTURE = '''scenario fixture  2026-10-22T09:00:00Z  say rate 185
   5.00s PLAY  wav: Lamp, what's the capital of Australia?
   8.10s HEARD Lamp, what's the capital of Australia?
   8.20s       [end of speech decided after 200 ms of quiet (silence)]
   9.50s       [lamp starts speaking 1.30 s after that decision]
  12.00s LAMP  Canberra.
   9.60s LAMP
  20.00s PLAY  wav: Hey Lamp, um what's a good name for a small grey cat?
  21.00s HEARD Hey, Lamp.
  21.40s HEARD
  24.00s HEARD for a small grey cat?
  26.00s       [lamp starts speaking]
  27.50s LAMP  Maybe Smoky.
  35.00s PLAY  Samantha: Do you want a coffee?
  37.00s HEARD Do you want a coffee?
  38.40s       [lamp starts speaking 1.40 s after that decision]
  38.50s LAMP  I can't drink it.
  45.00s PLAY  Daniel: Yes please, with milk.
  47.00s HEARD Yes, please, with milk.
'''
FIXTURE_PLAN = [('L', 'capital', 'capital australia', 'capital', '3.00', 'wav: x'),
                ('L', 'cat', 'name cat', 'name', '3.50', 'wav: x'),
                ('C', 'side0', '', '', '', 'x'), ('C', 'side0', '', '', '', 'x')]


def selftest():
    asks = score_file(FIXTURE, FIXTURE_PLAN)
    cap, cat, side = asks
    assert cap['whole'] and not cap['split'] and not cap['lost'] and cap['answer_ms'] == 1500, cap
    assert cat['split'] and cat['lost'] and not cat['whole'] and cat['heard'] == ['Hey, Lamp.', 'for a small grey cat?'], cat
    assert cat['answer_ms'] == 2500 and side['spoke'] and side['mark'] == 'C', side
    # The transcript alone (no recorded first sound) gives no answer time.
    assert score_file('  1.00s PLAY  wav: Lamp, hi.\n  3.00s LAMP  Hello.\n', [('L', 'hi', 'hi', '', '1.00', 'x')])[0]['answer_ms'] is None
    s = summary(asks, 'fixture', 1)
    assert (s['whole'], s['split'], s['first_lost'], s['side_let_out'], s['median_ms'], s['max_ms']) == (1, 1, 1, 1, 2000, 2500), s
    inferred = infer_plan(['Daniel: Hey Lamp, tell me a short', 'Daniel: fact about elephants.', 'Samantha: Do you want a coffee?'])
    assert [r[0] for r in inferred] == ['L', 'L', 'C'] and inferred[0][1] == inferred[1][1] and inferred[0][3] == 'short', inferred
    assert first_word("Hey Lamp, um ") == 'lamp' and has_keys('Elephants are big', 'fact/elephants')
    print('desk_kit.py selftest: ok')


if __name__ == '__main__':
    cmd = sys.argv[1] if len(sys.argv) > 1 else ''
    if cmd == 'scene' and len(sys.argv) == 4: scene(int(sys.argv[2]), sys.argv[3])
    elif cmd == 'score' and len(sys.argv) > 2: score(sys.argv[2:])
    elif cmd == 'selftest': selftest()
    else: sys.exit(__doc__)
