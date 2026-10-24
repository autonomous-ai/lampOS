import hashlib
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import time

WORKTREE = Path('/private/tmp/second-reminder-task-release-pair-20261024')
EVIDENCE = Path('/private/tmp/second-state-pair-artifacts-20261024/current-59c4/linux-format')
HEAD = 'cec3372b7f26a728d0b96e0e681d257d191ff7fb'
IMAGE = 'sha256:7dc3bb4b3b88bb00e16e37eeb5c98a74f9d926f956584eb8c009c9ca86023cd2'
DOCKER = ['docker', '--context', 'colima-lampos']
NAME = 'second-state-59c4-fmt-' + secrets.token_hex(6)
RECEIPT = EVIDENCE / 'linux-fmt-current.json'
CLEANUP = EVIDENCE / 'linux-fmt-current-cleanup.json'
ARCHIVE = EVIDENCE / 'linux-fmt-current-source.tar'
LOG = EVIDENCE / 'linux-fmt-current.txt'
record = {'name': NAME, 'image': IMAGE, 'context': 'colima-lampos', 'head': HEAD,
          'command': 'cargo fmt --all --check', 'gate_executed': False,
          'commands': [], 'removed': False}
cleanup = {'name': NAME, 'owner_label': NAME, 'removed': False}
created = False

SCRIPT = r'''set -euo pipefail
test "$(id -u)" = 65534
test "$(id -g)" = 65534
mkdir /work/source
tar --no-same-owner -xpf - -C /work/source
cd /work/source
chmod -R a-w lampOS
test -z "$(find lampOS -type f -perm /222 -print -quit)"
find lampOS -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum > /tmp/before.sha256
printf 'files_before=%s\n' "$(wc -l < /tmp/before.sha256)"
sha256sum /tmp/before.sha256
cd lampOS
printf 'gate_started=cargo fmt --all --check\n'
set +e
cargo fmt --all --check
fmt_status=$?
set -e
cd ..
find lampOS -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum > /tmp/after.sha256
printf 'files_after=%s\n' "$(wc -l < /tmp/after.sha256)"
sha256sum /tmp/after.sha256
set +e
cmp /tmp/before.sha256 /tmp/after.sha256
source_status=$?
set -e
printf 'fmt_exit=%s\nsource_comparison_exit=%s\n' "$fmt_status" "$source_status"
test "$source_status" = 0
exit "$fmt_status"
'''

def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def save():
    RECEIPT.write_text(json.dumps(record, indent=2) + '\n')
    CLEANUP.write_text(json.dumps(cleanup, indent=2) + '\n')

def run(args, tag, timeout=40, stdin=None, combined=False, check=True):
    output = LOG if combined else EVIDENCE / f'linux-fmt-current-{tag}.stdout'
    error = EVIDENCE / f'linux-fmt-current-{tag}.stderr'
    entry = {'tag': tag, 'argv': args, 'timeout_seconds': timeout,
             'stdout': output.name, 'stderr': output.name if combined else error.name}
    record['commands'].append(entry)
    save()
    with output.open('xb') as out, error.open('xb') as err:
        try:
            result = subprocess.run(args, stdin=stdin, stdout=out,
                                    stderr=subprocess.STDOUT if combined else err,
                                    timeout=timeout, check=False)
            entry['exit'] = result.returncode
        except subprocess.TimeoutExpired:
            entry['timeout'] = True
            save()
            raise
    save()
    if check and result.returncode != 0:
        raise RuntimeError(f'{tag} exited {result.returncode}; outputs retained')
    return result.returncode, output.read_text()

def git(*args):
    return subprocess.check_output(['git', *args], cwd=WORKTREE,
                                   env={**os.environ, 'GIT_NO_LAZY_FETCH': '1'},
                                   timeout=30).decode().strip()

try:
    for path in (RECEIPT, CLEANUP, ARCHIVE, LOG):
        if path.exists():
            raise RuntimeError(f'will not overwrite existing evidence: {path.name}')
    assert git('rev-parse', 'HEAD') == HEAD
    assert not git('status', '--porcelain')
    record['repository_tree'] = git('rev-parse', HEAD + '^{tree}')
    record['lampOS_tree'] = git('rev-parse', HEAD + ':lampOS')
    with ARCHIVE.open('xb') as archive:
        subprocess.run(['git', 'archive', '--format=tar', HEAD, 'lampOS'],
                       cwd=WORKTREE, stdout=archive, check=True, timeout=60,
                       env={**os.environ, 'GIT_NO_LAZY_FETCH': '1'})
    record['source_tar_sha256'] = digest(ARCHIVE)
    record['source_tar_bytes'] = ARCHIVE.stat().st_size
    record['environment'] = {'user': '65534:65534', 'network': 'none',
        'root_filesystem': 'read-only', 'host_mounts': False,
        'source': 'git archive of the exact head streamed on stdin into private tmpfs; chmod a-w; all regular-file SHA-256s compared before/after',
        'build_or_test': False}
    _, image = run(DOCKER + ['image', 'inspect', '--format', '{{.Id}} {{.Os}}/{{.Architecture}}', IMAGE], 'image')
    assert image.strip() == IMAGE + ' linux/arm64'
    created = True
    run(DOCKER + ['container', 'create', '--pull=never', '--name', NAME, '--platform', 'linux/arm64',
        '--network', 'none', '--read-only', '--user', '65534:65534', '--cap-drop', 'ALL',
        '--security-opt', 'no-new-privileges', '--pids-limit', '128', '--memory', '1536m',
        '--tmpfs', '/work:rw,nosuid,nodev,noexec,size=768m,mode=0700,uid=65534,gid=65534',
        '--tmpfs', '/tmp:rw,nosuid,nodev,noexec,size=128m,mode=0700,uid=65534,gid=65534',
        '--label', 'lamp.compat.owner=' + NAME, '--env', 'CARGO_NET_OFFLINE=true',
        '--interactive', '--entrypoint', '/bin/bash', IMAGE, '--noprofile', '--norc', '-c', SCRIPT], 'create')
    _, configured = run(DOCKER + ['container', 'inspect', '--format', '{{json .}}', NAME], 'configuration')
    configuration = json.loads(configured)
    assert configuration['Config']['User'] == '65534:65534'
    assert configuration['HostConfig']['ReadonlyRootfs'] is True
    assert configuration['HostConfig']['NetworkMode'] == 'none'
    assert configuration['HostConfig'].get('Binds') in (None, [])
    assert all(mount['Type'] == 'tmpfs' for mount in configuration['Mounts'])
    assert set(configuration['HostConfig']['Tmpfs']) == {'/tmp', '/work'}
    with ARCHIVE.open('rb') as archive:
        code, output = run(DOCKER + ['container', 'start', '--attach', '--interactive', NAME],
                           'start', timeout=180, stdin=archive, combined=True, check=False)
    record['docker_start_exit'] = code
    record['gate_executed'] = 'gate_started=cargo fmt --all --check' in output
    _, state = run(DOCKER + ['container', 'inspect', '--format', '{{json .State}}', NAME], 'state')
    record['state'] = json.loads(state)
    record['log_sha256'] = digest(LOG)
    record['source_manifests_match'] = 'source_comparison_exit=0\n' in output
    for line in output.splitlines():
        if line.startswith(('files_before=', 'files_after=', 'fmt_exit=')):
            key, value = line.split('=', 1)
            record[key] = int(value)
        elif line.endswith('/tmp/before.sha256'):
            record['regular_files_before_sha256'] = line.split()[0]
        elif line.endswith('/tmp/after.sha256'):
            record['regular_files_after_sha256'] = line.split()[0]
    assert record['state']['Status'] == 'exited'
    assert record['state']['Running'] is False and record['state']['OOMKilled'] is False
    assert record['state']['Error'] == ''
    assert record['state']['ExitCode'] == 0 and code == 0
    assert record['gate_executed'] and record['fmt_exit'] == 0 and record['source_manifests_match']
    assert record['files_before'] == record['files_after']
    assert record['regular_files_before_sha256'] == record['regular_files_after_sha256']
    assert git('rev-parse', 'HEAD') == HEAD and not git('status', '--porcelain')
    record['worktree_unchanged'] = True
    record['result'] = 'passed'
except BaseException as error:
    record['result'] = 'failed'
    record['failure'] = str(error)
finally:
    if created:
        try:
            _, owner = run(DOCKER + ['container', 'inspect', '--format', '{{index .Config.Labels "lamp.compat.owner"}}', NAME], 'cleanup-owner')
            assert owner.strip() == NAME, 'container owner label mismatch'
            cleanup['owner_verified'] = True
            code, removed = run(DOCKER + ['container', 'rm', '--force', NAME], 'cleanup-remove')
            cleanup['remove_exit'] = code
            cleanup['removed_name'] = removed.strip()
            _, remaining = run(DOCKER + ['container', 'ls', '--all', '--filter', 'name=^/' + NAME + '$',
                                        '--filter', 'label=lamp.compat.owner=' + NAME, '--format', '{{.ID}}'], 'cleanup-remaining')
            assert not remaining.strip(), 'owned container remains'
            cleanup['remaining_empty'] = True
            cleanup['removed'] = True
            record['removed'] = True
        except BaseException as error:
            cleanup['failure'] = str(error)
            record['result'] = 'failed'
    save()
    print(json.dumps({key: record.get(key) for key in ('name', 'head', 'result', 'failure', 'gate_executed',
          'docker_start_exit', 'fmt_exit', 'files_before', 'files_after', 'source_manifests_match', 'removed')}))
    print('receipt=' + RECEIPT.name + ' log=' + LOG.name + ' cleanup=' + CLEANUP.name)
sys.exit(0 if record.get('result') == 'passed' and record['removed'] else 1)
