"""Score one try of row-cutin-words.sh (#3997 step 2) from the voice session's trace, or print the verdict.

  score-cutin-words.py C ARM I ASK_US SCENE EVENTS TALLY WORDS_US   a real cut-in ("Stop." / "Wait, hold on.")
  score-cutin-words.py O ARM I ASK_US SCENE EVENTS TALLY            the own tail: a long answer, nobody talking
  score-cutin-words.py verdict TALLY C_TRIALS O_TRIALS TOL_MS

SCENE is the phrase's scenario file (one #PLAY_US: the cut-in, or row O's end line). The yield, its own end and its
decision are read as tools/played/cutin.sh reads them: a yield's decision is the commit or resume its own
duplex_yield_end names (started_at_us and owner the yield's). Onset is #PLAY_US, the lamp's clock just before afplay
(0.1 to 0.3 s early, the same on both arms). A wordless commit is any duplex_commit while no person sound played:
row O's whole answer up to its end line, and a cut-in row's answer up to its line.

Tally (TSV): C: arm C i valid decision onset_us decision_us held_ms words wordless_before
             O: arm O i valid commits yields resumes held_resumes answer_ms
"""
import json, sys


def load(events, ask):
    rows = []
    for line in open(events, errors='replace'):
        try:
            r = json.loads(line)
        except ValueError:
            continue
        if isinstance(r, dict) and r.get('at_us', 0) >= ask:
            rows.append(r)
    rows.sort(key=lambda r: (r.get('at_us', 0), r.get('service_sequence', 0)))
    return rows


def plays(scene):
    return [int(l.split()[1]) for l in open(scene, errors='replace') if l.startswith('#PLAY_US ')] if scene else []


def turn_of(r):
    return (r.get('owner') or {}).get('turn', r.get('turn'))


def score(mode, arm, i, ask, scene, events, tally, words_us=0):
    rows = load(events, ask)

    def kinds(a, b, *ks):
        return [r for r in rows if r.get('kind') in ks and a <= r.get('at_us', 0) < b]

    def out(text, cells):
        print('%s%s %s %s' % (arm, mode, i, text))
        with open(tally, 'a') as f:
            f.write('\t'.join([arm, mode, str(i)] + [str(c) for c in cells]) + '\n')

    def ended(y):  # cutin.sh's ended(): the yield's own end, and its own decision record
        e = next((r for r in rows if r.get('kind') == 'duplex_yield_end' and r.get('started_at_us') == y.get('at_us')
                  and r.get('owner') == y.get('owner')), None)
        if e is None:
            return None, None, 'unended'
        kind = {'commit_decision': 'duplex_commit', 'resumed': 'duplex_resume'}.get(e.get('outcome'))
        d = next((r for r in rows if kind and r.get('kind') == kind and r.get('at_us') == e.get('at_us')
                  and r.get('owner') == y.get('owner')), None)
        return e, d, kind[7:] if d else str(e.get('outcome'))

    p = plays(scene)
    if len(p) != 1:
        out('phrase not played whole; not a try', ['no'] + [''] * (6 if mode == 'C' else 5))
        return
    line_at = p[0]
    first = next(iter(kinds(ask, line_at, 'speaker_first_write')), None)
    if first is None:
        out('the lamp had not begun the answer; not a try', ['no'] + [''] * (6 if mode == 'C' else 5))
        return
    story = turn_of(first)
    end = next((r for r in kinds(first['at_us'], 1 << 62, 'turn_finished') if r.get('turn') == story), None)
    if mode == 'O':
        stop = min(line_at, end['at_us']) if end else line_at
        commits, ys = kinds(first['at_us'], stop + 1, 'duplex_commit'), kinds(first['at_us'], stop, 'duplex_yield')
        rs = kinds(first['at_us'], stop + 1, 'duplex_resume')
        held = [r for r in rs if (r.get('held_ms') or 0) > 0 and r.get('backchannel') is None]
        words = [r.get('words') for r in kinds(first['at_us'], stop + 1, 'stop_words')]
        answer_ms = (stop - first['at_us']) // 1000
        valid = 'yes' if commits or answer_ms >= 30_000 else 'no'
        out('answer %.1f s, nobody talking: %d yields, %d wordless commits%s, %d resumes (%d held for words that came empty); '
            'stop_words words %s; answer %s%s' % (
                answer_ms / 1000, len(ys), len(commits),
                ' (held_ms %s)' % ','.join(str(c.get('held_ms')) for c in commits) if commits else '',
                len(rs), len(held), words or 'none', end.get('outcome') if end and end['at_us'] <= line_at else 'ran to the end line',
                '' if valid == 'yes' else '; under 30 s and no commit: not a try'),
            [valid, len(commits), len(ys), len(rs), len(held), answer_ms])
        return
    before = len(kinds(first['at_us'], line_at, 'duplex_commit'))
    if end and end['at_us'] < line_at:
        out('the answer ended (%s) before the line, %d wordless commits; not a try' % (end.get('outcome'), before),
            ['no', '', '', '', '', '', before])
        return
    y = next(iter(kinds(line_at, line_at + words_us + 1_500_000, 'duplex_yield')), None)
    if y is None:
        out('no yield within the line + 1.5 s: the cut-in missed (%d wordless commits before it)' % before,
            ['yes', 'none', '', '', '', '', before])
        return
    e, d, decision = ended(y)
    held = d.get('held_ms') if d else ''
    sw = [r for r in kinds(y['at_us'], (e or {}).get('at_us', y['at_us'] + 2_000_000) + 1, 'stop_words')]
    words = sw[0].get('words') if sw else ''
    words = 'null' if words is None else words
    onset = y['at_us'] - line_at
    out('onset->yield %d ms, yield->%s %s ms, held_ms %s, stop_words words %s; answer %s' % (
        onset // 1000, decision if d else '%s, no decision' % decision, (d['at_us'] - y['at_us']) // 1000 if d else '-',
        held if held != '' else '-', words if sw else 'none', end.get('outcome') if end else 'went on'),
        ['yes', decision if d else decision, onset, d['at_us'] - y['at_us'] if d else '', held, words, before])


def verdict(tally, c_need, o_need, tol_ms):
    rows = [l.rstrip('\n').split('\t') for l in open(tally) if l.strip()] if tally else []
    lines = []
    med = {}
    wordless = {}
    for arm in ('P', 'W'):
        c = [r for r in rows if r[0] == arm and r[1] == 'C' and r[3] == 'yes']
        o = [r for r in rows if r[0] == arm and r[1] == 'O' and r[3] == 'yes']
        cuts = sum(r[4] == 'commit' for r in c)
        on = sorted(int(r[5]) for r in c if r[5])
        med[arm] = on[len(on) // 2] if on else None
        wl_o = sum(int(r[4] or 0) for r in rows if r[0] == arm and r[1] == 'O')
        wl_c = sum(int(r[9] or 0) for r in rows if r[0] == arm and r[1] == 'C' and len(r) > 9)
        wordless[arm] = wl_o + wl_c
        held = [r[7] for r in c if r[4] == 'commit' and r[7]]
        lines.append('%s: cut-ins %d/%d (%s); onset->yield ms %s, median %s; own tail: %d valid of %d, %.1f min heard, '
                     '%d yields, wordless commits %d (row O %d, before a cut-in %d), held-and-resumed %d; commit held_ms %s' % (
                         arm, cuts, len(c), ','.join(r[4] for r in c) or '-', ','.join(str(x // 1000) for x in on) or '-',
                         med[arm] // 1000 if med[arm] is not None else '-', len(o),
                         sum(r[0] == arm and r[1] == 'O' for r in rows), sum(int(r[8] or 0) for r in o) / 60000,
                         sum(int(r[5] or 0) for r in o), wordless[arm], wl_o, wl_c, sum(int(r[7] or 0) for r in o),
                         ','.join(held) or '-'))
        lines[-1] += '' if (len(c) >= c_need and len(o) >= o_need) else ' [short: C %d of %d, O %d of %d]' % (len(c), c_need, len(o), o_need)
    c = {a: [r for r in rows if r[0] == a and r[1] == 'C' and r[3] == 'yes'] for a in 'PW'}
    o = {a: [r for r in rows if r[0] == a and r[1] == 'O' and r[3] == 'yes'] for a in 'PW'}
    if any(len(c[a]) < c_need or len(o[a]) < o_need for a in 'PW'):
        v = 'VOID (valid tries: P C %d, P O %d, W C %d, W O %d; needed C %d, O %d)' % (
            len(c['P']), len(o['P']), len(c['W']), len(o['W']), c_need, o_need)
    elif sum(r[4] == 'commit' for r in c['W']) < len(c['W']):
        v = 'FAIL (a real cut-in on W did not commit: %s)' % ','.join(r[4] for r in c['W'])
    elif wordless['W']:
        v = 'FAIL (W made %d wordless commits)' % wordless['W']
    elif med['P'] is None or med['W'] is None:
        v = 'VOID (no onset->yield to compare)'
    elif abs(med['W'] - med['P']) > tol_ms * 1000:
        v = 'FAIL (time to yield changed: median W %d ms against P %d ms, over %d ms)' % (med['W'] // 1000, med['P'] // 1000, tol_ms)
    elif wordless['P'] == 0:
        v = ('NO REPRO (W kept every cut-in and the yield time, but P made no wordless commit either: '
             'W\'s 0 is not evidence for the hold)')
    else:
        v = 'PASS (W: cut-ins %d/%d, 0 wordless commits against P\'s %d, median onset->yield %d ms against %d)' % (
            len(c['W']), len(c['W']), wordless['P'], med['W'] // 1000, med['P'] // 1000)
    for l in lines:
        print(l)
    print('VERDICT: ' + v)


if __name__ == '__main__':
    a = sys.argv[1:]
    if a[0] == 'verdict':
        verdict(a[1], int(a[2]), int(a[3]), int(a[4]))
    elif a[0] == 'C':
        score('C', a[1], a[2], int(a[3]), a[4], a[5], a[6], int(a[7]))
    else:
        score('O', a[1], a[2], int(a[3]), a[4], a[5], a[6])
