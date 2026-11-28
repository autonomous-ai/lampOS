#!/usr/bin/env -S python3 -I
"""October's two-person scenes (#829, battery.sh / twopeople829.sh) and tonight's, scored by one rule.

usage: twopeople_compare.py FILE ... [-- FILE ...]     the files before "--" are one side, after it the other
Each scene's lines are "Voice: text | what the lamp said" (empty: nothing). The rule, from each scene's expectation:
a line with "Hey Lamp" is answered (A); every other line is left alone (Q): in a played test no face is in view, so
"B's follow-up answered only when facing or named" is left alone too (and that is the product owner's rule of 25 Nov:
a follow-up counts unless clearly someone else). Prints each side's A and Q counts, per scene and in all."""
import re, sys

SCENE = re.compile(r'^== round \d+, (\d): ')
LINE = re.compile(r'^(Daniel|Samantha): (.*?)\s*\|\s?(.*)$')


def tally(paths):
    per = {}  # (scene, kind) -> [ok, n]
    for path in paths:
        scene = None
        for raw in open(path, errors='replace'):
            m = SCENE.match(raw)
            if m:
                scene = m.group(1); continue
            if raw.startswith('-- '):
                scene = None; continue
            m = LINE.match(raw.rstrip('\n'))
            if not (m and scene):
                continue
            text, said = m.group(2), m.group(3).strip()
            kind = 'A' if re.search(r'\bhey lamp\b', text, re.I) else 'Q'
            ok = bool(said) if kind == 'A' else not said
            t = per.setdefault((scene, kind), [0, 0]); t[1] += 1; t[0] += ok
    return per


def show(name, per):
    a = [sum(v[0] for (s, k), v in per.items() if k == 'A'), sum(v[1] for (s, k), v in per.items() if k == 'A')]
    q = [sum(v[0] for (s, k), v in per.items() if k == 'Q'), sum(v[1] for (s, k), v in per.items() if k == 'Q')]
    pct = lambda x: f'{100 * x[0] / x[1]:.1f}%' if x[1] else '-'
    print(f'{name}: asks answered {a[0]} of {a[1]} ({pct(a)}), talk left alone {q[0]} of {q[1]} ({pct(q)}), '
          f'all {a[0] + q[0]} of {a[1] + q[1]} ({pct([a[0] + q[0], a[1] + q[1]])})')
    for (s, k) in sorted(per):
        ok, n = per[(s, k)]
        print(f'   scene {s} {k}: {ok} of {n}')


args = sys.argv[1:]
if '--' in args:
    i = args.index('--')
    show('before', tally(args[:i])); show('after', tally(args[i + 1:]))
else:
    show('files', tally(args))
