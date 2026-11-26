#!/usr/bin/env -S python3 -I
"""How many of the two-person battery's left-alone lines the lamp never heard (the Reviewer-lead's P2 on #8673).

usage: twopeople_unheard.py RAW ... [-- RAW ...]     each side's raw scene files ($PLAYED_OUT/scenarios/*-LABEL-829-*.txt)
twopeople_compare.py counts a left-alone line as left alone when the lamp said nothing, as October's 74.7% was
counted; a line the microphones missed, or a muted speaker, would count the same. This reads what the lamp heard
in each scene's raw file: a line is heard when a HEARD row follows its PLAY row before the next PLAY. A line with
"Hey Lamp" is an ask, as twopeople_compare.py reads it; every other line is a left-alone line. Prints, for each
side, the left-alone lines with nothing heard, so a mic or speaker fault can't lift either side's score."""
import re
import sys

ROW = re.compile(r'^\s*(-?[0-9.]+)s (PLAY |LAMP |HEARD|UNPLD|     ) (.*)$')
LINE = re.compile(r'^([A-Za-z]+): (.*)$')


def side(paths):
    left_alone = unheard = 0
    for path in paths:
        current = None
        for raw in open(path, errors='replace'):
            m = ROW.match(raw)
            if not m:
                continue
            tag, text = m.group(2), m.group(3).strip()
            if tag == 'PLAY ':
                if current is not None and not current:
                    unheard += 1
                line = LINE.match(text)
                spoken = line.group(2) if line else text
                if 'hey lamp' in spoken.lower():
                    current = None
                else:
                    left_alone += 1
                    current = False
            elif tag == 'HEARD' and text and current is not None:
                current = True
        if current is not None and not current:
            unheard += 1
    return left_alone, unheard


def main():
    args = sys.argv[1:]
    if not args:
        sys.exit(__doc__)
    sides = [args] if '--' not in args else [args[:args.index('--')], args[args.index('--') + 1:]]
    for n, paths in enumerate(sides, 1):
        left_alone, unheard = side(paths)
        print(f'side {n}: {len(paths)} scene files, {unheard} of {left_alone} left-alone lines with nothing heard')


main()
