#!/usr/bin/env python3
"""A stand-in lamp for work-records-row.py's dry run on the Mac: no lamp, no key, nothing sent anywhere.

  work-records-stand-in.py init ROOT ENGINE     a fake lamp under ROOT (units, binaries, PATH shims)
  work-records-stand-in.py ssh CMD              WORK_SSH: CMD run here with ROOT's shims first on PATH
  work-records-stand-in.py systemctl ARGS...    the shim's systemctl (show, restart, is-active, daemon-reload)
  work-records-stand-in.py journalctl ARGS...   the shim's journalctl: ROOT/journal.log from --since @N
  work-records-stand-in.py serve                a lamp-tasks stand-in on ROOT/run/lampos-tasks: status, targets,
                                                prepare, confirm, work and work_list
  work-records-stand-in.py play FILE            WORK_PLAY: a clip "played"; -yes and -how add the voice's
                                                journal and session lines a real turn would

ROOT comes from WORK_ROOT (init takes it as its argument). Records move on a short clock: a task is queued
for 1.5 s, works for 6 s with a tool every 1.5 s, then is done. Only tasks (kind task) keep a record.
"""
import json
import os
import signal
import socketserver
import subprocess
import sys
import threading
import time

HERE = os.path.abspath(__file__)


def root():
    return os.environ['WORK_ROOT']


def unit_environment(unit):
    env = open(f'{root()}/env/{unit}').read().split()
    # systemd reads drop-ins from /etc and /run alike.
    for place in ('etc', 'run'):
        drop = f'{root()}/{place}/systemd/system/{unit}.service.d'
        if os.path.isdir(drop):
            for name in sorted(os.listdir(drop)):
                for line in open(os.path.join(drop, name)):
                    if line.startswith('Environment='):
                        env.append(line.strip().split('=', 1)[1])
    return env


def init(path, engine):
    os.makedirs(f'{path}/env', exist_ok=True)
    os.makedirs(f'{path}/bin', exist_ok=True)
    for d in ('run/lampos-tasks', 'home/orangepi', 'var/tmp', 'etc/systemd/system', 'run/systemd/system', 'tmp',
              'var/lib/lampOS/voice/sessions/s1/metadata', 'opt/lampOS/tasks/r1', 'opt/lampOS/voice/r1'):
        os.makedirs(f'{path}/{d}', exist_ok=True)
    base = 'LAMP_VOICE_ROUTE=second LAMP_SECOND_CHAT=deepseek' + (f' LAMP_LAMP_ENGINE={engine}' if engine else '')
    open(f'{path}/env/lamp-tasks', 'w').write(base + '\n')
    open(f'{path}/env/lamp-voice', 'w').write('LAMP_VOICE_ROUTE=second\n')
    for unit, part in (('lamp-tasks', 'tasks'), ('lamp-voice', 'voice')):
        binary = f'{path}/opt/lampOS/{part}/r1/{unit}'
        open(binary, 'w').write(f'#!/bin/sh\necho "{unit} installed 0.1.0"\n')
        os.chmod(binary, 0o755)
        if not os.path.exists(f'{path}/opt/lampOS/{part}/current'):
            os.symlink(f'{path}/opt/lampOS/{part}/r1', f'{path}/opt/lampOS/{part}/current')
    open(f'{path}/journal.log', 'w').close()
    open(f'{path}/var/lib/lampOS/voice/sessions/s1/metadata/events-1.jsonl', 'w').close()
    shims = {
        'sudo': '#!/bin/sh\nwhile [ $# -gt 0 ]; do case "$1" in -u|-p) shift 2;; -*) shift;; *) break;; esac; done\nexec "$@"\n',
        'sha256sum': '#!/bin/sh\nexec shasum -a 256 "$@"\n',
        'systemctl': f'#!/bin/sh\nexec python3 {HERE} systemctl "$@"\n',
        'journalctl': f'#!/bin/sh\nexec python3 {HERE} journalctl "$@"\n',
    }
    for name, text in shims.items():
        open(f'{path}/bin/{name}', 'w').write(text)
        os.chmod(f'{path}/bin/{name}', 0o755)


def ssh(command):
    env = dict(os.environ, PATH=f'{root()}/bin:' + os.environ['PATH'])
    return subprocess.run(['sh', '-c', command], env=env, stdin=sys.stdin).returncode


def systemctl(args):
    if args[:1] == ['show']:
        unit, prop = args[1], args[3]
        if prop == 'Environment':
            print(' '.join(unit_environment(unit)))
        elif prop == 'MainPID':
            print(4242 if unit == 'lamp-voice' else 4241)
        elif prop == 'ExecStart':
            part = 'tasks' if unit == 'lamp-tasks' else 'voice'
            print(f'{{ path={root()}/opt/lampOS/{part}/current/{unit} ; argv[]={unit} serve ; }}')
        return 0
    if args[:1] == ['restart']:
        # Each restarted unit's /proc/PID/exe: the file it was started from, as the kernel's link names it.
        for unit, part, pid in (('lamp-tasks', 'tasks', 4241), ('lamp-voice', 'voice', 4242)):
            if unit in args[1:]:
                os.makedirs(f'{root()}/proc/{pid}', exist_ok=True)
                exe = f'{root()}/proc/{pid}/exe'
                if os.path.lexists(exe):
                    os.unlink(exe)
                os.symlink(os.path.realpath(f'{root()}/opt/lampOS/{part}/current/{unit}'), exe)
        if 'lamp-tasks' in args[1:]:
            pid_file = f'{root()}/run/serve.pid'
            if os.path.exists(pid_file):
                try:
                    os.kill(int(open(pid_file).read()), signal.SIGTERM)
                except (ProcessLookupError, ValueError):
                    pass
                time.sleep(0.3)
            log = open(f'{root()}/serve.log', 'a')
            child = subprocess.Popen(['python3', HERE, 'serve'], stdout=log, stderr=log, stdin=subprocess.DEVNULL,
                                     start_new_session=True)
            open(pid_file, 'w').write(str(child.pid))
        return 0
    if args[:1] == ['is-active']:
        print('active')
        return 0
    return 0


def journalctl(args):
    """lamp-voice's journal is ROOT/journal.log, another unit's ROOT/journal-UNIT.log; `-o cat` drops the time."""
    since = 0.0
    if '--since' in args:
        since = float(args[args.index('--since') + 1].lstrip('@'))
    unit = args[args.index('-u') + 1] if '-u' in args else 'lamp-voice'
    path = f'{root()}/journal.log' if unit == 'lamp-voice' else f'{root()}/journal-{unit}.log'
    cat = '-o' in args and args[args.index('-o') + 1] == 'cat'
    if not os.path.exists(path):
        return 0
    for line in open(path):
        at, _, rest = line.partition(' ')
        if float(at) >= since:
            sys.stdout.write(rest.split(': ', 1)[-1] if cat else line)
    return 0


def journal(unit, text):
    with open(f'{root()}/journal-{unit}.log', 'a') as f:
        f.write(f'{time.time():.6f} lamp {unit}[1]: {text}\n')


class Lamp:
    def __init__(self):
        self.lock = threading.Lock()
        self.tasks = {}
        self.next_id = 1
        self.revision = 1
        engine = 'hermes'
        for part in unit_environment('lamp-tasks'):
            if part.startswith('LAMP_LAMP_ENGINE='):
                engine = part.split('=', 1)[1]
        self.engine = engine
        # As lamp-tasks: a record's step only with LAMP_TASKS_STEPS=on (off in the product, c0a4-16).
        self.steps = 'LAMP_TASKS_STEPS=on' in unit_environment('lamp-tasks')

    def record(self, work_id, kind, words, began, queued, works):
        t = time.time() - began
        state = 'queued' if t < queued else 'working' if t < queued + works else 'done'
        tools = 0 if t < queued else min(int((t - queued) / 1.5) + 1, int(works / 1.5))
        done = state == 'done'
        step = {'n': tools * 2, 'tool': ('web_search', 'web_fetch')[tools % 2]} if tools and self.steps else None
        return {'id': work_id, 'kind': kind, 'who': 'OpenClaw', 'words': words[:300], 'state': state,
                'started_ms': int(began * 1000), 'updated_ms': int(time.time() * 1000), 'elapsed_s': int(t),
                'last_active_ms': None, 'tools': tools, 'step': step, 'done': done,
                'answer': 'Both sites say about 24 degrees and light rain.' if done else None,
                'session': {'machine': '', 'agent': 'agent:main:harness-1ac58eb4'} if state != 'queued' else None}

    def all(self):
        rows = [(b, self.record(i, 'task', w, b, 1.5, 6)) for i, (w, b) in self.tasks.items()]
        return [r for _, r in sorted(rows, key=lambda x: -x[0])]

    def answer(self, request, route):
        op = request.get('op')
        # ROOT/task-new, from a played -yes clip: the voice's confirm started a task.
        task_new = f'{root()}/task-new'
        if os.path.exists(task_new):
            words, began = open(task_new).read(), os.path.getmtime(task_new)
            os.unlink(task_new)
            with self.lock:
                self.tasks[str(self.next_id)] = (words, began)
                self.next_id += 1
                self.revision += 1
        status = {'ok': True, 'store_id': 'stand-in', 'revision': self.revision,
                  'summary': {'session_id': 'stand-in-session'}}
        with self.lock:
            if op == 'status':
                return status
            if op == 'targets':
                return {'ok': True, 'targets': [{'alias': 'lamp', 'engine': self.engine}]}
            if op == 'prepare' and route == 'request':
                # ROOT/prepare-conflict-at holding N: the Nth prepare gets one `conflict` (c0a4-6).
                self.prepares = getattr(self, 'prepares', 0) + 1
                at = f'{root()}/prepare-conflict-at'
                if os.path.exists(at) and open(at).read().strip() == str(self.prepares):
                    os.unlink(at)
                    self.revision += 1
                    return {'ok': False, 'error': 'conflict', 'message': 'Tasks changed; inspect current state'}
                if request['guard']['revision'] != self.revision or request.get('target') != 'lamp':
                    return {'ok': False, 'error': 'stale'}
                task_id = self.next_id
                self.next_id += 1
                self.revision += 1
                self.pending = (task_id, request['text'])
                return {'ok': True, 'id': task_id, 'prepared_digest': f'd{task_id}', 'revision': self.revision}
            if op == 'confirm':
                # ROOT/conflict-at holding N: the Nth confirm since the start gets one `conflict`, as when
                # another task moved the store's revision (c0a4-5).
                self.confirms = getattr(self, 'confirms', 0) + 1
                at = f'{root()}/conflict-at'
                if os.path.exists(at) and open(at).read().strip() == str(self.confirms):
                    os.unlink(at)
                    self.revision += 1
                    return {'ok': False, 'error': 'conflict', 'message': 'Tasks changed; inspect current state'}
                if route != 'voice' or request.get('prepared_digest') != f'd{request.get("id")}':
                    return {'ok': False, 'error': 'not_trusted' if route != 'voice' else 'digest'}
                self.tasks[str(request['id'])] = (self.pending[1], time.time())
                return {'ok': True}
            if op == 'work':
                found = [r for r in self.all() if r['id'] == request['id']]
                if not found:
                    return {'ok': False, 'error': 'no_such_work', 'message': 'No work with that id.'}
                return {'ok': True, 'work': found[0], 'store_id': 'stand-in', 'revision': self.revision}
            if op == 'work_list':
                return {'ok': True, 'works': self.all()[:int(request.get('limit', 5))]}
        return {'ok': False, 'error': 'bad_request'}


def serve():
    lamp = Lamp()
    journal('lamp-tasks', 'lamp-tasks: kept session agent:main:harness-1ac58eb4 idle')
    run = f'{root()}/run/lampos-tasks'

    def handler(route):
        class Handler(socketserver.StreamRequestHandler):
            def handle(self):
                request = json.loads(self.rfile.readline())
                self.wfile.write((json.dumps(lamp.answer(request, route)) + '\n').encode())
        return Handler

    servers = []
    for name, route in (('request.sock', 'request'), ('voice.sock', 'voice')):
        path = f'{run}/{name}'
        if os.path.exists(path):
            os.unlink(path)
        server = socketserver.ThreadingUnixStreamServer(path, handler(route))
        server.daemon_threads = True
        threading.Thread(target=server.serve_forever, daemon=True).start()
        servers.append(server)
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    while True:
        time.sleep(1)


def play(path):
    name = os.path.basename(path)
    now = time.time()
    journal = open(f'{root()}/journal.log', 'a')
    events = open(f'{root()}/var/lib/lampOS/voice/sessions/s1/metadata/events-1.jsonl', 'a')
    if '-yes' in name:
        open(f'{root()}/task-new', 'w').write('research the best houseplants for a dark bedroom')
        journal.write(f'{now:.6f} lamp lamp-voice[1]: lamp-voice second: request 7: tools: remember_offered=n '
                      'calls=confirm_agent_task:1 prepared=n\n')
        journal.write(f'{now:.6f} lamp lamp-voice[1]: lamp-voice: tasks: request 7: task 9 started on a yes taken in code\n')
    if '-how' in name:
        time.sleep(0.2)
        journal.write(f'{time.time():.6f} lamp lamp-voice[1]: lamp-voice second: request 8: tools: remember_offered=n '
                      'calls=check_agent_task:1 prepared=n\n')
        journal.write(f'{time.time():.6f} lamp lamp-voice[1]: lamp-voice: tasks: request 8: check_agent_task told work 1\n')
        text = 'Việc vẫn đang làm, tới bước 3 rồi.' if name.startswith('vi') else "It's still working, on step 3."
        events.write(json.dumps({'kind': 'transcript', 'turn': 8, 'role': 'output', 'text': text,
                                 'at_us': time.monotonic_ns() // 1000}) + '\n')
    return 0


def round_now(path):
    """A lamp-update round as tonight's on c0a4: a new release folder, the `current` links moved to it,
    its journal line, and lamp-tasks restarted on it."""
    for unit, part in (('lamp-tasks', 'tasks'), ('lamp-voice', 'voice')):
        os.makedirs(f'{path}/opt/lampOS/{part}/r2', exist_ok=True)
        binary = f'{path}/opt/lampOS/{part}/r2/{unit}'
        open(binary, 'w').write(f'#!/bin/sh\necho "{unit} round 14732"\n')
        os.chmod(binary, 0o755)
        os.unlink(f'{path}/opt/lampOS/{part}/current')
        os.symlink(f'{path}/opt/lampOS/{part}/r2', f'{path}/opt/lampOS/{part}/current')
    os.environ['WORK_ROOT'] = path
    journal('lamp-update', 'lamp-update: installed 14732, switched to build 14732')
    systemctl(['restart', 'lamp-tasks'])


def main(argv):
    command, rest = argv[0], argv[1:]
    if command == 'round':
        round_now(rest[0])
        return 0
    if command == 'init':
        init(rest[0], rest[1] if len(rest) > 1 else '')
        return 0
    return {'ssh': lambda: ssh(rest[0]), 'systemctl': lambda: systemctl(rest), 'journalctl': lambda: journalctl(rest),
            'serve': serve, 'play': lambda: play(rest[0])}[command]()


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
