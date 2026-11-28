"""Mac only: python3 -I mac/fake_logs.py lamp/logcount.py
Made-up /var/log and /var/log.hdd (RFC 3339 and traditional stamps, a .gz rotated copy, a line with no readable
stamp, a link that must not be followed), then logcount.py's counts checked. No journalctl on the Mac, so the
journal count is checked as files=0 here; on the lamp it reads each journal file through journalctl --file."""
import gzip, os, subprocess, sys, tempfile, time

LC = sys.argv[1]
T = 1795000000  # the "reset"
iso = lambda t: time.strftime('%Y-%m-%dT%H:%M:%S', time.gmtime(t)) + '.123456+00:00'
trad = lambda t: time.strftime('%b %e %H:%M:%S', time.localtime(t))
with tempfile.TemporaryDirectory() as r:
    ram, hdd = os.path.join(r, 'var/log'), os.path.join(r, 'var/log.hdd')
    os.makedirs(ram); os.makedirs(hdd)
    with open(os.path.join(hdd, 'syslog'), 'w') as f:
        f.write('%s lamp lamp-voice[9]: a line before\n' % iso(T - 100))     # before
        f.write('%s lamp lamp-voice[9]: another before\n' % iso(T - 1))      # before
        f.write('%s lamp lamp-setup[3]: not the voice\n' % iso(T - 50))      # not counted as voice
        f.write('%s lamp lamp-voice[9]: after\n' % iso(T + 10))              # after
        f.write('garbage lamp-voice no stamp\n')                             # unparsed: counted as before
    with gzip.open(os.path.join(hdd, 'syslog.2.gz'), 'wt') as f:
        f.write('%s lamp lamp-voice[9]: old rotated\n' % iso(T - 86400))     # before
    with open(os.path.join(hdd, 'user.log'), 'w') as f:
        f.write('%s lamp lamp-voice[9]: traditional before\n' % trad(T - 3600))  # before
        f.write('%s lamp lamp-voice[9]: traditional after\n' % trad(T + 3600))   # after
    os.utime(os.path.join(hdd, 'user.log'), (T + 3600, T + 3600))
    with open(os.path.join(hdd, 'daemon.log'), 'w') as f:                    # not a TEXT_LOG: never read
        f.write('%s lamp lamp-voice[9]: elsewhere\n' % iso(T - 5))
    with open(os.path.join(r, 'outside'), 'w') as f:
        f.write('%s lamp lamp-voice[9]: behind a link\n' % iso(T - 5))
    os.symlink(os.path.join(r, 'outside'), os.path.join(hdd, 'kern.log'))    # a link: passed over
    with open(os.path.join(ram, 'syslog'), 'w') as f:
        f.write('%s lamp lamp-voice[9]: ram after\n' % iso(T + 5))
    out = subprocess.run([sys.executable, '-I', LC, str(T), '--root', r], stdout=subprocess.PIPE, text=True).stdout
    want = ['TEXT dir=/var/log files=1 lines=1 voice=1 voice_before=0 voice_after=1 voice_unparsed=0',
            'TEXT dir=/var/log.hdd files=3 lines=8 voice=7 voice_before=5 voice_after=2 voice_unparsed=1',
            'JOURNAL dir=/var/log.hdd/journal files=0 voice=0 voice_before=0 voice_after=0 unread=0']
    got = out.strip().splitlines()
    ok = got == want and 'behind a link' not in out and 'before' not in ' '.join(l for l in got if not l.startswith(('TEXT', 'JOURNAL')))
    print('\n'.join('  ' + l for l in got))
    print('logcount.py on made-up logs: %s' % ('as expected (counts only, no line printed)' if ok else 'WRONG, wanted:\n  ' + '\n  '.join(want)))
    sys.exit(0 if ok else 1)
