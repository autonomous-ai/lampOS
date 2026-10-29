#!/usr/bin/env python3
"""Night rounds: desk-kit-2's made people's lines played to your own lamp, scored for hearing and answering.

usage: night.py ROUND [SCENE ...]      (no SCENE: every scene in SCENES, in order)
       night.py --check [SCENE ...]    (the kit's lines and WAVs only: no lamp, nothing played)

Plays desk-kit-2's lines (50 WAVs from eight made voices, rendered once through a lamp's own speech route; README.md
"Night rounds") from this computer's speakers to the lamp at LAMP_HOST (lamp_ssh), one scene at a time. A follow-up waits for the lamp's real answer: `a+S` plays S s after
the lamp's last sample of its answer, `d+S` S s after its first sample (cutting in), `g+S` S s after the previous clip
ended. Then it scores each played line from the session's events and the voice's journal: words heard against the
script, lines split, cut-short words let go, answered or held as its kind says (desk talk must be held, every other
line answered), and the first answer sound. Words are read only from played made people's lines, in an empty office.
Saves $PLAYED_OUT/night/<ROUND>.txt (the table) and <ROUND>.json; nothing on the lamp is changed."""
import difflib, hashlib, json, os, re, shutil, subprocess, sys, time, statistics

HERE = os.path.dirname(os.path.abspath(__file__))
SSH = [HERE + '/lamp_ssh']   # LAMP_HOST, LAMP_SSH_KEY, LAMP_KNOWN_HOSTS (README.md)
OUT = os.environ.get('PLAYED_OUT') or os.path.expanduser('~/lamp-played')
LINES = HERE + '/desk-kit-2/lines.tsv'
# The kit's audio is one versioned tar, fetched once into $PLAYED_OUT and checked against NIGHT_KIT_SHA256 before use.
KIT = os.environ.get('NIGHT_KIT_TAR', 'desk-kit-2')
KIT_SHA256 = os.environ.get('NIGHT_KIT_SHA256', '60128bcda66193ba118a2b442ff42c24c9b6e9e39f7818ce219e200190711729')
WAV = os.environ.get('NIGHT_WAV') or f'{OUT}/{KIT}/wav'
# This run's name on the lamp: its bus taps subscribe under it and its events tail runs as it, so stop_on_lamp() ends
# exactly this run's (a tap on a quiet topic would otherwise live until its next write fails, hours later).
# The lamp-bus CLI as a command (LAMP_BUS_BIN, e.g. "sudo -n /tmp/x_lamp-bus" on a build that installs none).
BUS = os.environ.get('LAMP_BUS_BIN') or '/usr/local/bin/lamp-bus'
TAG = f'night{os.getpid()}'   # the bus's rule for a name: [a-z][a-z0-9_]{0,31}

# Each scene: (line id, when). The first step of a scene waits for a quiet lamp. Timers and reminders are left out: they
# would sound later in the night.
SCENES = {
    'weather':   [('a02', 'start'), ('f01', 'a+2.0'), ('f02', 'a+2.5')],
    'run':       [('p01', 'start'), ('f09', 'a+2.0')],
    'dinner':    [('a07', 'start'), ('f07', 'a+1.5')],
    'egg':       [('p02', 'start'), ('f10', 'a+2.0')],
    'london':    [('a03', 'start'), ('f11', 'd+1.2')],
    'desk1':     [('d01', 'start'), ('d02', 'g+0.6'), ('d03', 'g+1.2'), ('d04', 'g+0.5')],
    'gift':      [('p04', 'start'), ('f07', 'a+2.0')],
    'moon':      [('a06', 'start'), ('f06', 'a+3.0')],
    'dalat':     [('l01', 'start')],
    'laptop':    [('l02', 'start'), ('f03', 'a+2.0')],
    'desk2':     [('d05', 'start'), ('d06', 'g+0.7'), ('d07', 'g+1.5'), ('d08', 'g+0.6')],
    'danang':    [('p03', 'start'), ('f01', 'a+1.8')],
    'thanks':    [('p12', 'start'), ('f12', 'a+2.0')],
    'worldcup':  [('a08', 'start'), ('f08', 'a+2.0')],
    'slides':    [('p06', 'start')],
    'word':      [('p07', 'start')],
    'rainbow':   [('p08', 'start'), ('f06', 'd+2.0')],
    'tired':     [('p09', 'start'), ('f06', 'a+2.5')],
    'watch':     [('p10', 'start')],
    'veggie':    [('p11', 'start'), ('f03', 'a+2.0')],
    'books':     [('l03', 'start')],
    'pho':       [('l04', 'start'), ('f06', 'a+2.0')],
    'desk3':     [('d09', 'start'), ('d10', 'g+0.6'), ('d11', 'g+1.4'), ('d12', 'g+0.5')],
    'time':      [('a01', 'start'), ('a10', 'a+2.0')],
    # Desk talk 4 s after the lamp's answer, inside the conversation's 8 s (Ten's desk kit on 75c9, 23 Oct 04:4x).
    'after1':    [('a02', 'start'), ('d03', 'a+4.0'), ('d04', 'g+0.5')],
    'after2':    [('a01', 'start'), ('d07', 'a+4.0'), ('d08', 'g+0.6')],
}
QUIET_BEFORE = 22.0     # seconds of lamp quiet before a scene: past the conversation's windows
ANSWER_WAIT = 14.0      # longest wait for an answer to start before a follow-up goes anyway
ANSWER_LONGEST = 40.0   # longest wait for an answer to end


def lines():
    out = {}
    for row in open(LINES):
        if row.startswith('#') or not row.strip(): continue
        id_, voice, kind, text = row.rstrip('\n').split('\t')
        out[id_] = (voice, kind, re.sub(r'\s*\{[\d.]+\}\s*', ' ', text).strip())
    return out


def lamp(command, timeout=40):
    return subprocess.run(SSH + [command], capture_output=True, text=True, timeout=timeout, stdin=subprocess.DEVNULL).stdout


def wav_seconds(path):
    import wave
    with wave.open(path) as w: return w.getnframes() / w.getframerate()


class Events:
    """The newest session's events, followed live over one ssh."""
    def __init__(self):
        cmd = ('S=$(sudo -n ls -t /var/lib/lampOS/voice/sessions | head -1); '
               f'exec sudo -n bash -c "exec -a {TAG}_events tail -n 0 -q -F /var/lib/lampOS/voice/sessions/$S/metadata/events-0.jsonl '
               '/var/lib/lampOS/voice/sessions/$S/metadata/events-1.jsonl /var/lib/lampOS/voice/sessions/$S/metadata/events-2.jsonl '
               '/var/lib/lampOS/voice/sessions/$S/metadata/events-3.jsonl" 2>/dev/null')
        self.p = subprocess.Popen(SSH + [cmd], stdout=subprocess.PIPE, stdin=subprocess.DEVNULL, text=True, bufsize=1)
        os.set_blocking(self.p.stdout.fileno(), False)
        self.rows, self.buf = [], ''

    def pump(self):
        try: chunk = self.p.stdout.read()
        except (BlockingIOError, TypeError): chunk = None
        if chunk:
            self.buf += chunk
            *done, self.buf = self.buf.split('\n')
            for line in done:
                try: r = json.loads(line)
                except ValueError: continue
                r['_mac'] = time.time(); self.rows.append(r)

    def answer_end(self, first, longest):
        # The answer's end: its speech_final_sample_retired or turn_finished (an answer cut or ended without its final
        # sample has only the latter: 20 answers began and 15 retired a final sample in r12's first half), or else the
        # speaker quiet for 1 s after the answer began.
        # (r12, 23 Oct: gift's end event never matched, so its follow-up waited the whole 40 s and came 37 s late).
        end = time.time() + longest; quiet_since = None
        while time.time() < end:
            self.pump()
            for r in self.rows:
                if r['_mac'] >= first and r.get('kind') in ('speech_final_sample_retired', 'turn_finished'): return r['_mac']
            if time.time() - first > 1.0 and not self.speaking():
                quiet_since = quiet_since or time.time()
                if time.time() - quiet_since >= 1.0: return quiet_since
            else:
                quiet_since = None
            time.sleep(0.05)
        return None

    def wait_for(self, kind, since, longest):
        end = time.time() + longest
        while time.time() < end:
            self.pump()
            for r in self.rows:
                if r['_mac'] >= since and r.get('kind') == kind: return r['_mac']
            time.sleep(0.05)
        return None

    def speaking(self):
        """Whether the lamp's answer is playing: a first write since the last retired sample."""
        self.pump(); state = False
        for r in self.rows:
            if r.get('kind') == 'speaker_first_write': state = True
            elif r.get('kind') in ('speech_final_sample_retired', 'turn_finished'): state = False
        return state

    def close(self): self.p.kill()


class Names:
    """lamp-name's hearings off the bus (name/heard, name/rising, name/near when it exists), each with the Mac time it came."""
    def __init__(self):
        cmd = ('for t in name/heard name/rising name/near; do ' + BUS + ' tap $t --name ' + TAG + '_names --seconds 36000 '
               '| sed -u "s|^|$t |" & done; wait')
        self.p = subprocess.Popen(SSH + [cmd], stdout=subprocess.PIPE, stdin=subprocess.DEVNULL, text=True, bufsize=1)
        os.set_blocking(self.p.stdout.fileno(), False)
        self.rows, self.buf = [], ''

    def pump(self):
        try: chunk = self.p.stdout.read()
        except (BlockingIOError, TypeError): chunk = None
        if chunk:
            self.buf += chunk
            *done, self.buf = self.buf.split('\n')
            for line in done:
                topic, _, rest = line.partition(' ')
                rest = rest.strip()
                if not rest.startswith('{'): continue
                try:
                    dec = json.JSONDecoder(); hdr, at = dec.raw_decode(rest); r, _ = dec.raw_decode(rest[at:].lstrip())
                except ValueError: continue
                if isinstance(r, dict):
                    score = next((r[k] for k in ('score', 'probability', 'p') if isinstance(r.get(k), (int, float))), None)
                    # When it was heard, not when this was read: the bus header's wall time less how long ago it began.
                    sent = hdr.get('at_unix_ms') / 1000 if isinstance(hdr, dict) and hdr.get('at_unix_ms') else time.time()
                    ago = (r.get('began_ms_ago') or r.get('ago_ms') or 0) / 1000
                    self.rows.append({'topic': topic, 'score': score, '_mac': sent - ago})

    def close(self): self.p.kill()


class Journal:
    """lamp-voice's journal followed live over one ssh, read by a thread: (lamp time, text). A fetch at the end loses
    what journald has already dropped: 52e6 kept only ~15 min of lamp-voice (30 Oct, every row before 00:52 lost)."""
    def __init__(self):
        import threading
        self.rows = []
        self.p = subprocess.Popen(SSH + ['exec journalctl -u lamp-voice -f -n 0 -o short-unix'], stdout=subprocess.PIPE,
                                  stdin=subprocess.DEVNULL, text=True, bufsize=1)
        def read():
            for l in self.p.stdout:
                m = re.match(r'([\d.]+) \S+ \S+: (.*)', l.rstrip('\n'))
                if m: self.rows.append((float(m.group(1)), m.group(2)))
        threading.Thread(target=read, daemon=True).start()

    def close(self): self.p.kill()


class Heard:
    """What the voice heard, off the bus (voice/heard: the model's transcript with its request), followed live."""
    def __init__(self):
        self.p = subprocess.Popen(SSH + [f'exec {BUS} tap voice/heard --name {TAG}_heard --seconds 36000'],
                                  stdout=subprocess.PIPE, stdin=subprocess.DEVNULL, text=True, bufsize=1)
        os.set_blocking(self.p.stdout.fileno(), False)
        self.by, self.buf = {}, ''

    def pump(self):
        try: chunk = self.p.stdout.read()
        except (BlockingIOError, TypeError): chunk = None
        if chunk:
            self.buf += chunk
            *done, self.buf = self.buf.split('\n')
            for line in done:
                line = line.strip()
                if not line.startswith('{'): continue
                # `lamp-bus tap` prints the header and the payload as two JSON objects on one line.
                try:
                    dec = json.JSONDecoder(); _, at = dec.raw_decode(line); r, _ = dec.raw_decode(line[at:].lstrip())
                except ValueError: continue
                if isinstance(r, dict) and r.get('request') is not None and isinstance(r.get('text'), str):
                    self.by[int(r['request'])] = r['text']   # the bus never marks a piece last: keep the newest

    def close(self): self.p.kill()


def stop_on_lamp():
    """Ends this run's taps and events tail on the lamp; the bracket keeps pkill from matching its own command line."""
    lamp(f"pkill -f '[{TAG[0]}]{TAG[1:]}_names'; pkill -f '[{TAG[0]}]{TAG[1:]}_heard'; sudo -n pkill -f '[{TAG[0]}]{TAG[1:]}_events'; true")


def play(id_):
    path = f'{WAV}/{id_}.wav'
    start = time.time(); subprocess.run(['/usr/bin/afplay', path], check=True); return start, time.time()


def run_scene(name, steps, ev, played):
    # A quiet lamp first: nothing playing, and QUIET_BEFORE s since anything played.
    while True:
        ev.pump()
        last = max([r['_mac'] for r in ev.rows if r.get('kind') in ('speaker_first_write', 'speech_final_sample_retired')] + [0])
        if not ev.speaking() and time.time() - last >= QUIET_BEFORE and (not played or time.time() - played[-1]['end'] >= QUIET_BEFORE): break
        time.sleep(0.5)
    prev_end = None
    for id_, when in steps:
        if when.startswith('a+'):
            first = ev.wait_for('speaker_first_write', prev_end, ANSWER_WAIT)
            done = ev.answer_end(first, ANSWER_LONGEST) if first else None
            time.sleep(max(0.0, (done or time.time()) + float(when[2:]) - time.time()))
        elif when.startswith('d+'):
            first = ev.wait_for('speaker_first_write', prev_end, ANSWER_WAIT)
            time.sleep(max(0.0, (first or time.time()) + float(when[2:]) - time.time()))
        elif when.startswith('g+'):
            time.sleep(float(when[2:]))
        start, end = play(id_)
        played.append({'scene': name, 'id': id_, 'when': when, 'start': start, 'end': end}); prev_end = end
    ev.wait_for('speech_final_sample_retired', prev_end, ANSWER_WAIT + ANSWER_LONGEST)


def journal(since, until):
    out = lamp(f"journalctl -u lamp-voice --since @{int(since) - 2} --until @{int(until) + 2} --no-pager -o short-unix", timeout=60)
    rows = []
    for l in out.splitlines():
        m = re.match(r'([\d.]+) \S+ \S+: (.*)', l)
        if m: rows.append((float(m.group(1)), m.group(2)))
    return rows


def wer(said, heard):
    a = re.findall(r"[a-z0-9']+", said.lower()); b = re.findall(r"[a-z0-9']+", heard.lower())
    if not a: return 0.0
    sm = difflib.SequenceMatcher(None, a, b); same = sum(t.size for t in sm.get_matching_blocks())
    return 1 - same / len(a)


def score(played, ev, J, L, heard_by):
    # The journal and this computer read the same wall clock to within the ssh time.
    out = []
    for i, p in enumerate(played):
        voice, kind, text = L[p['id']]
        until = played[i + 1]['start'] if i + 1 < len(played) else p['end'] + 45
        begun = {int(m.group(1)): t for t, l in J for m in [re.search(r'request (\d+): speech to text (?:kept ready|opened)', l)] if m}
        reqs = sorted(q for q, t in begun.items() if p['start'] - 0.5 <= t < until - 0.3)
        # The clip's own pieces: lines begun while it played or just after (the rest of the window is the lamp's answer).
        pieces = [q for q in reqs if begun[q] <= p['end'] + 1.0]
        heard = ' '.join(heard_by.get(q, '') for q in reqs).strip()
        answer = ''.join((r.get('text') or '') for r in ev.rows if r.get('kind') == 'transcript' and r.get('turn') in reqs).strip()
        verdicts, firsts, let_go, joined, not_joined = [], [], 0, 0, 0
        for t, l in J:
            for q in reqs:
                if re.search(rf'request {q}: its answer is (played|held back)', l): verdicts.append(re.search(r'its answer is (played|held back): (.*)', l).group(0)[14:][:90])
                m = re.search(rf'request {q} first answer audio (\d+) ms', l)
                if m: firsts.append(int(m.group(1)))
                if re.search(rf"request {q}'s words, cut short, are let go", l): let_go += 1
                # The voice logs no line for a join that went through: "request N: K lines joined into it ..." is written
                # only when none of those lines' answers played (LAMP_JOIN_CUT_WORDS), so `joined` is that subset. "request
                # N: not joined with request M's words" is a refusal, counted apart.
                m = re.search(rf'request {q}: (\d+) lines joined into it', l)
                if m: joined += int(m.group(1))
                if re.search(rf'request {q}: not joined with request', l): not_joined += 1
        # What was spoken is the journal's own line for the request ("first answer audio"): a transcript can hold words
        # the voice cancelled (a not_for_me after its text), and a sound's time is only when the Mac read it.
        answered = bool(firsts)
        want = kind != 'desk'
        out.append({**p, 'voice': voice, 'kind': kind, 'text': text, 'requests': reqs, 'pieces': pieces, 'heard': heard, 'wer': round(wer(text, heard), 2),
                    'answered': answered, 'right': answered == want, 'verdicts': verdicts, 'first_ms': firsts[:1], 'let_go': let_go,
                    'joined': joined, 'not_joined': not_joined, 'answer': answer})
    return out


def fetch_kit():
    """The kit's WAVs into $PLAYED_OUT once, from the update bucket, refused unless the tar's sha256 is the kit's."""
    if os.environ.get('NIGHT_WAV') or os.path.exists(f'{OUT}/{KIT}/README.md'): return
    os.makedirs(OUT, exist_ok=True); tar_at = f'{OUT}/{KIT}.tar'
    got = (subprocess.run(['curl', '-fsSL', '-o', tar_at, f'https://cdn.autonomous.ai/os/test-kits/{KIT}.tar']).returncode == 0
           or (shutil.which('gsutil') is not None and
               subprocess.run(['gsutil', '-q', 'cp', f'gs://s3-autonomous-upgrade-3/os/test-kits/{KIT}.tar', tar_at]).returncode == 0))
    if not got: sys.exit(f'night.py: could not fetch {KIT}.tar')
    with open(tar_at, 'rb') as f: sha = hashlib.sha256(f.read()).hexdigest()
    if sha != KIT_SHA256:
        os.remove(tar_at); sys.exit(f'night.py: {KIT}.tar is not the kit\'s (sha256 differs); not using it')
    subprocess.run(['tar', '-xf', tar_at, '-C', OUT], check=True); os.remove(tar_at)


def main():
    check = sys.argv[1:2] == ['--check']
    if len(sys.argv) < 2 or (not check and sys.argv[1].startswith('-')): sys.exit(__doc__)
    rnd = sys.argv[1]; names = sys.argv[2:] or list(SCENES)
    unknown = [n for n in names if n not in SCENES]
    if unknown: sys.exit('night.py: no scene ' + ', '.join(unknown) + '; scenes: ' + ' '.join(SCENES))
    L = lines()
    fetch_kit()
    ids = sorted({i for n in names for i, _ in SCENES[n]})
    missing = [i for i in ids if i not in L or not os.path.exists(f'{WAV}/{i}.wav')]
    if missing: sys.exit('night.py: no line or WAV for ' + ', '.join(missing))
    if check:
        print(f'night.py: {len(names)} scenes, {len(ids)} lines, every one in {LINES} with its WAV in {WAV}'); return
    hour, minute = time.localtime().tm_hour, time.localtime().tm_min
    if time.localtime().tm_wday < 5 and (12, 0) <= (hour, minute) < (14, 0) and os.environ.get('LAMP_PLAY_AT_LUNCH') != 'yes':
        sys.exit('night.py: not playing between 12:00 and 14:00 local, when the office rests')
    if subprocess.run(SSH + ['true'], stdin=subprocess.DEVNULL, timeout=30).returncode != 0:
        sys.exit('night.py: the lamp did not answer over lamp_ssh (LAMP_HOST, README.md)')
    os.makedirs(OUT + '/night', exist_ok=True)
    # Each stream is a new ssh (5-6 s to connect to 52e6): let all of them attach before the first scene.
    ev = Events(); hb = Heard(); nm = Names(); jl = Journal(); time.sleep(10); played = []; began = time.time()
    try:
        for n in names:
            run_scene(n, SCENES[n], ev, played); hb.pump(); nm.pump()
        time.sleep(3); ev.pump(); hb.pump(); nm.pump()
    finally:
        time.sleep(2); ev.close(); hb.close(); nm.close(); jl.close(); stop_on_lamp()
    J = [(t, l) for t, l in jl.rows if t >= began - 2]
    rows = score(played, ev, J, L, hb.by)
    # lamp-name's best score per topic within 3.5 s of the clip's start (its hearings land 1.45–1.90 s in).
    for r in rows:
        r['name_scores'] = {}
        for x in nm.rows:
            if r['start'] - 0.3 <= x['_mac'] <= r['start'] + 3.5 and x['score'] is not None:
                r['name_scores'][x['topic']] = max(r['name_scores'].get(x['topic'], 0), x['score'])
    held_up = sum(' held up ' in l for t, l in J); stalls = [int(m.group(1)) for t, l in J for m in [re.search(r' held up (\d+) ms', l)] if m]
    talk = [r for r in rows if r['kind'] != 'desk']; desk = [r for r in rows if r['kind'] == 'desk' and not r['scene'].startswith('after')]
    # Desk talk a few seconds after the lamp's answer (after1/after2), apart so held_of_desk stays comparable with r08/r09.
    after = [r for r in rows if r['kind'] == 'desk' and r['scene'].startswith('after')]
    firsts = sorted(r['first_ms'][0] for r in rows if r['first_ms'])
    summary = {
        'round': rnd, 'at': time.strftime('%Y-%m-%d %H:%M'), 'lines': len(rows),
        'answered_of_talk': f"{sum(r['answered'] for r in talk)}/{len(talk)}", 'held_of_desk': f"{sum(not r['answered'] for r in desk)}/{len(desk)}", 'held_of_desk_after_answer': f"{sum(not r['answered'] for r in after)}/{len(after)}",
        'split': sum(len(r['pieces']) > 1 for r in rows), 'pieces_total': sum(len(r['pieces']) for r in rows), 'let_go': sum(r['let_go'] for r in rows), 'joined': sum(r['joined'] for r in rows), 'not_joined': sum(r['not_joined'] for r in rows),
        'not_heard': sum(not r['requests'] for r in rows),
        'wer_median': statistics.median([r['wer'] for r in rows if r['requests']] or [0]), 'wer_over_0.2': sum(r['wer'] > 0.2 for r in rows if r['requests']),
        'first_ms_median': firsts[len(firsts) // 2] if firsts else None, 'first_ms_max': firsts[-1] if firsts else None,
        'stalls': held_up, 'stall_ms_max': max(stalls) if stalls else 0,
        'journal': {k: sum(k in l for t, l in J) for k in ('memory note incomplete', 'memory note late', 'past the bound', 'cut short, are let go', 'lines joined into it', 'not joined with request', "the line is someone else's", 'another voice than the conversation', 'not for me', 'in a busy room: its words are not chained', 'says stop or never mind', 'judged_without')},
    }
    txt = [json.dumps(summary)]
    for r in rows:
        txt.append(f"{r['scene']:9} {r['id']} {r['when']:6} {r['kind']:6} pieces={r['pieces']} req={r['requests']} wer={r['wer']:.2f} "
                   f"{'ANSWERED' if r['answered'] else 'held'}{'' if r['right'] else ' <-- WRONG'} first={r['first_ms']} letgo={r['let_go']} "
                   f"joined={r['joined']} not_joined={r['not_joined']} names={r.get('name_scores')}\n    said:   {r['text']}\n    heard:  {r['heard']}\n    lamp:   {r['answer'][:300]}\n    why:    {r['verdicts']}")
    open(f'{OUT}/night/{rnd}.txt', 'w').write('\n'.join(txt) + '\n')
    json.dump({'summary': summary, 'rows': rows}, open(f'{OUT}/night/{rnd}.json', 'w'), indent=1, default=str)
    print('\n'.join(txt[:1])); print(f'{OUT}/night/{rnd}.txt')


if __name__ == '__main__':
    main()
