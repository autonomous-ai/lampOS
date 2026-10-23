#!/usr/bin/env python3
"""Two or more night.py rounds side by side: night_compare.py ROUND ROUND [...]

One column a round: lines for the lamp answered, desk lines held, lines heard in more than one piece, cut-short words
let go, joined, words wrong (median and lines over 0.2), first sound (median and slowest), stalls; then each line whose
outcome differs between the rounds."""
import json, os, sys

V = (os.environ.get('PLAYED_OUT') or os.path.expanduser('~/lamp-played')) + '/night'
if len(sys.argv) < 2: sys.exit(__doc__)
rounds = [(r, json.load(open(f'{V}/{r}.json'))) for r in sys.argv[1:]]
keys = ['answered_of_talk', 'held_of_desk', 'held_of_desk_after_answer', 'split', 'pieces_total', 'let_go', 'joined', 'not_heard', 'wer_median',
        'wer_over_0.2', 'first_ms_median', 'first_ms_max', 'stalls']
w = max(len(r) for r, _ in rounds) + 2
print(' ' * 18 + ''.join(f'{r:>{w}}' for r, _ in rounds))
for k in keys:
    print(f'{k:18}' + ''.join(f"{str(d['summary'].get(k, '-')):>{w}}" for _, d in rounds))
for k in sorted({k for _, d in rounds for k in d['summary'].get('journal', {})}):
    print(f'{k[:18]:18}' + ''.join(f"{str(d['summary'].get('journal', {}).get(k, '-')):>{w}}" for _, d in rounds))
print()
by = [{(x['scene'], x['id']): x for x in d['rows']} for _, d in rounds]
for key in by[0]:
    outs = [b.get(key) for b in by]
    if any(o is None for o in outs): continue
    marks = ['ok' if o['right'] else ('ANSWERED' if o['answered'] else 'held') for o in outs]
    if len(set(marks)) > 1:
        print(f'{key[0]:9} {key[1]} {outs[0]["kind"]:6} ' + '  '.join(f'{m:>8}' for m in marks) + f'   {outs[0]["text"][:60]}')
