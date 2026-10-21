#!/usr/bin/env python3
"""Build an offline, deterministic lampOS payload; never activate a device."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import stat
import struct
import zipfile

CORE_BINARIES = ('lamp-keeper', 'lamp-voice', 'lamp-turn-end', 'lamp-body',
                 'lamp-body-worker', 'lamp-eyes', 'lamp-name', 'lamp-memory',
                 'lamp-actions', 'lamp-controls', 'lamp-expression', 'lamp-health', 'lamp-policy')
# Optional executables are carried when built; carrying one never adds its unit.
# lamp-cloud (company broker, opt-in, must not share V1 os-server's MQTT client ID)
# and lamp-skills (needs the product skills and settings) stay out of DEFAULT_UNITS.
# lamp-peek (read-only page, off unless LAMP_PEEK=on, never on a product lamp) is carried
# without a unit, as lamp-cloud and lamp-skills are.
OPTIONAL_BINARIES = ('lamp-surround', 'lamp-update', 'lamp-ears', 'lamp-schedule',
                     'lamp-tasks', 'lamp-air', 'lamp-records', 'lamp-mood', 'lamp-bus',
                     'lamp-selftest', 'lamp-settings', 'lamp-setup', 'lamp-cloud', 'lamp-skills',
                     'lamp-peek')
DEVELOPMENT_BINARIES = {'lamp-audio', 'lamp-body-inspect', 'lamp-ipc-probe',
    'lamp-memory-eval', 'lamp-observer', 'lamp-policy-fit', 'lamp-policy-judges',
    'lamp-sound-fixtures', 'lamp-tasks-harness-fixture', 'lamp-voice-eval',
    'lamp-memory-fixture', 'lamp-records-fixture', 'lamp-same-voice', 'lamp-policy-replay',
    # The production station's bench tool (a station run's record and verdict), not on a lamp.
    'lamp-station'}
# Factory/provisioning tools are not product runtime. The generic package carries
# them under tools/ for the V1 migration script, which measures each lamp's own
# BODY_POSES; poses are never shipped or copied between lamps. From a source with
# lamp-body-measure.service (lampOS #2163), the generic package also runs it: a lamp
# without poses is read seated at rest before its first body start, so the tool
# also sits beside lamp-body, as lamp-body-worker does.
PROVISIONING_BINARIES = {'lamp-body-measure', 'lamp-body-mood'}
SEATED_MEASURE_UNIT = 'lamp-body-measure.service'
PROVISION_SETTINGS = ('voice-provider.json', 'controls.json', 'controls-pins.json')
# The adapter's prepare runs `lamp-keeper prepare` (lamp-voice #1094); its usage
# line is the build's own statement that the command exists.
KEEPER_PREPARE = b'\nlamp-keeper prepare\n'
# Upstream verifies these same immutable bytes before inference.
ASSETS = {
    'smart-turn-v3.2-gpu.onnx': 'ab8dc64b88713f90b571c15b714bd1330e6c883cad8763dacf65c9376dc539be',
    'libonnxruntime.so.1.30.0': '64e903a43a041240fd6bcffe0ac6d4fea47ef87bf24b9d097801bd00a9612a4b',
    'melspectrogram.onnx': 'ba2b0e0f8b7b875369a2c89cb13360ff53bac436f2895cced9f479fa65eb176f',
    'embedding_model.onnx': '70d164290c1d095d1d4ee149bc5e00543250a7316b59f31d056cff7bd3075c1f',
    'face_detection_yunet_2023mar.onnx': '8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4',
}
OPTIONAL_ASSETS = {
    'face_recognition_sface_2021dec.onnx': '0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79',
}
DEFAULT_UNITS = tuple('lamp-' + part + '.service' for part in ('voice', 'body', 'eyes', 'name', 'memory', 'actions', 'controls', 'expression', 'health', 'policy')) + ('lampos.target', 'lampos-voice.slice', 'lampos-parts.slice')
PROFILE_TOKENS = {'UNIT_ID', 'LAMP_USER', 'MEMORY_USER', 'SERVO_DEVICE', 'CAMERA_DEVICE',
    'CAMERA_USB', 'MOTOR_LOCK_DIR', 'CAMERA_LOCK_DIR', 'VOICE_OPTIONS', 'BODY_POSES',
    'FAST_CPUS', 'MEMORY_DIR', 'MEMORY_SOCKET', 'ACTIONS_DIR', 'MIXER_CARD',
    'MIXER_CONTROL', 'HEALTH_EXPECT', 'JUDGE_ZONES'}
TOKEN = re.compile(r'@[A-Z][A-Z0-9_]*@')
SECRET = re.compile(r'-----BEGIN .*PRIVATE KEY-----|AIza[0-9A-Za-z_-]{30,}|gh[pousr]_[A-Za-z0-9]{20,}')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def read_file(root, relative):
    path = PurePosixPath(relative)
    if path.is_absolute() or '..' in path.parts or not path.parts:
        raise ValueError(f'unsafe path: {relative}')
    root = Path(root).resolve()
    candidate = root.joinpath(*path.parts)
    if candidate.is_symlink() or not candidate.is_file() or not candidate.resolve().is_relative_to(root):
        raise ValueError(f'missing or unsafe file: {relative}')
    return candidate.read_bytes()


def check_elf(data, name):
    if (len(data) < 64 or data[:6] != b'\x7fELF\x02\x01'
            or struct.unpack_from('<H', data, 18)[0] != 183
            or struct.unpack_from('<H', data, 16)[0] not in (2, 3)):
        raise ValueError(f'{name}: expected Linux ARM64 ELF executable')


def generic_policy(text, product):
    """Align the template's policy with the product unit's identity, state and relay.

    The template's DynamicUser is outside the parts' bus group (root:LAMP_USER,
    2770), its state directory defaults to 0755, which lamp-policy refuses, and
    without --endpoint it would call its built-in default instead of the
    product's relay. A changed template fails here rather than being guessed at.
    """
    endpoint = re.search(r'--endpoint\s+(https://\S+)', product)
    if not endpoint:
        raise ValueError('deploy/product/lamp-policy.service names no --endpoint')
    if ('DynamicUser=yes\n' not in text or 'StateDirectory=lampos-policy\n' not in text
            or not re.search(r'^ExecStart=\S*/lamp-policy ', text, re.M)):
        raise ValueError('lamp-policy.service template changed; review the generic policy rewrite')
    text = text.replace('DynamicUser=yes\n', 'User=@LAMP_USER@\nGroup=@LAMP_USER@\n', 1)
    if 'StateDirectoryMode=' not in text:
        text = text.replace('StateDirectory=lampos-policy\n', 'StateDirectory=lampos-policy\nStateDirectoryMode=0700\n', 1)
    if '--endpoint' not in text:
        text = re.sub(r'^(ExecStart=\S*/lamp-policy) ', lambda m: f'{m[1]} --endpoint {endpoint[1]} ', text, count=1, flags=re.M)
    return text


BUS_ENVIRONMENT = ('LAMP_BUS', 'LAMP_BUS_AUDIO')
BUS_ROOT_OPTION = '--bus-root /run/lampos-bus'


# lampOS keeps the classification beside the binaries it builds (lamp-voice
# #2481): a source with this file decides which built binary is optional,
# provisioning or development, so a new one needs no change here. The lists
# above remain for sources from before it. Core parts still need this packager's
# own handling (their folders, units and placement), so a core binary it does
# not know is refused.
BINARY_CLASSES = 'deploy/package-binaries.json'


def binary_classes(source):
    """(optional, provisioning, development) for this source."""
    path = Path(source) / BINARY_CLASSES
    if not path.exists():
        return OPTIONAL_BINARIES, set(PROVISIONING_BINARIES), set(DEVELOPMENT_BINARIES)
    try:
        classes = json.loads(path.read_text())
    except (OSError, ValueError) as error:
        raise ValueError(f'{BINARY_CLASSES}: {error}') from None
    lists = {}
    for key in ('core', 'optional', 'provisioning', 'development'):
        names = classes.get(key, [])
        if not isinstance(names, list) or not all(
                isinstance(name, str) and re.fullmatch(r'lamp-[a-z0-9-]+', name) for name in names):
            raise ValueError(f'{BINARY_CLASSES}: {key} must be a list of lamp-* names')
        lists[key] = names
    seen = [name for names in lists.values() for name in names]
    twice = sorted({name for name in seen if seen.count(name) > 1})
    if twice:
        raise ValueError(f'{BINARY_CLASSES}: in more than one list: {twice}')
    unknown_core = sorted(set(lists['core']) - set(CORE_BINARIES))
    if unknown_core:
        raise ValueError(f'{BINARY_CLASSES}: core binaries this packager cannot place yet: {unknown_core}')
    return tuple(lists['optional']), set(lists['provisioning']), set(lists['development'])


def generic_bus(unit, text, product):
    """Join the parts' bus wherever the product unit of that part does.

    The templates leave the voice, body, eyes and memory off the bus, so name
    never hears the microphone and health never hears those parts.
    """
    lines = ''.join(f'Environment={name}=on\n' for name in BUS_ENVIRONMENT
                    if f'Environment={name}=on\n' in product and f'Environment={name}=' not in text)
    if lines:
        anchor = 'Environment=LD_LIBRARY_PATH=@RELEASE_ROOT@/lib\n'
        if anchor not in text:
            raise ValueError(f'{unit}: no Service section to join the bus in')
        text = text.replace(anchor, anchor + lines, 1)
    if BUS_ROOT_OPTION in product and '--bus-root' not in text:
        execs = re.findall(r'^ExecStart=.*$', text, re.M)
        if len(execs) != 1:
            raise ValueError(f'{unit}: expected one ExecStart to give the bus root')
        text = text.replace(execs[0], f'{execs[0]} {BUS_ROOT_OPTION}', 1)
    return text


def package(source, binaries, assets, profile, version, output, adapter, libraries, source_commit):
    if not re.fullmatch(r'[0-9a-f]{40}', source_commit):
        raise ValueError('source_commit must be a full lowercase Git SHA')
    if not re.fullmatch(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)', version):
        raise ValueError('version must be major.minor.patch')
    generic = profile is None
    if generic:
        seated = [SEATED_MEASURE_UNIT] if (Path(source) / 'deploy/units' / SEATED_MEASURE_UNIT).exists() else []
        profile = {'substitutions': {}, 'units': list(DEFAULT_UNITS) + seated,
                   'required_paths': ['/etc/lampOS/voice/provider.json',
                       '/etc/lampOS/controls/controls.json', '/etc/lampOS/controls/controls-pins.json']}
    if set(profile) - {'substitutions', 'units', 'required_paths'}:
        raise ValueError('unknown profile fields')
    substitutions = profile.get('substitutions', {})
    units = profile.get('units', [])
    if not isinstance(substitutions, dict) or not isinstance(units, list) or not units:
        raise ValueError('profile needs substitutions and explicit units')
    for key, value in substitutions.items():
        if (not re.fullmatch(r'[A-Z][A-Z0-9_]*', key) or key.endswith('_SHA256')
                or key == 'RELEASE_ROOT' or not isinstance(value, str)
                or any(c in value for c in '\n\r\x00') or SECRET.search(value)):
            raise ValueError(f'invalid or sensitive substitution: {key}')
    if not generic and not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9.-]*', substitutions.get('UNIT_ID', '')):
        raise ValueError('explicit device UNIT_ID is required')
    if len(set(units)) != len(units):
        raise ValueError('duplicate unit')
    if {'lamp-update.service', 'lamp-surround.service'} & set(units):
        raise ValueError('bootstrap owns updates; independent updater activation is forbidden')
    mandatory = {'lampos.target', 'lampos-voice.slice', 'lampos-parts.slice', 'lamp-voice.service'}
    if not mandatory <= set(units):
        raise ValueError('profile must include voice, target and both slices')
    entries = {}

    def add(name, data, executable=False):
        if name in entries:
            raise ValueError(f'duplicate archive path: {name}')
        entries[name] = (data, 0o755 if executable else 0o644)

    adapter_data = Path(adapter).read_bytes()
    check_elf(adapter_data, 'service adapter')
    add('bin/service', adapter_data, True)
    required_paths = profile.get('required_paths', [])
    if (not isinstance(required_paths, list) or any(not isinstance(p, str)
            or not p.startswith('/') or '..' in PurePosixPath(p).parts
            or any(c in p for c in '\n\r\x00') for p in required_paths)):
        raise ValueError('required_paths must contain absolute external paths')
    library_root = Path(libraries)
    if not library_root.is_dir() or not (library_root / 'libasound.so.2').is_file():
        raise ValueError('libraries must include libasound.so.2')
    for library in sorted(library_root.iterdir()):
        if not re.fullmatch(r'lib[A-Za-z0-9_+.-]+\.so(?:\.[0-9]+)*', library.name):
            raise ValueError(f'invalid library filename: {library.name}')
        data = read_file(library_root, library.name)
        check_elf(data, library.name)
        add(f'lib/{library.name}', data)
    optional_binaries, provisioning_binaries, development_binaries = binary_classes(source)
    unexpected = ({p.name for p in Path(binaries).glob('lamp-*') if p.is_file()}
                  - set(CORE_BINARIES + optional_binaries) - development_binaries - provisioning_binaries)
    if unexpected:
        raise ValueError(f'unclassified binaries: {sorted(unexpected)}; add each to lampOS/{BINARY_CLASSES}')
    binary_hashes = {}
    for name in CORE_BINARIES + optional_binaries:
        if name not in CORE_BINARIES and not (Path(binaries) / name).exists():
            if (Path(source) / 'crates' / name.removeprefix('lamp-') / 'Cargo.toml').exists():
                raise ValueError(f'missing runtime binary declared in source: {name}')
            continue
        data = read_file(binaries, name)
        check_elf(data, name)
        binary_hashes[name] = digest(data)
        part = name.removeprefix('lamp-')
        if name == 'lamp-keeper':
            if KEEPER_PREPARE not in data:
                raise ValueError('lamp-keeper lacks the prepare command (source predates lamp-voice #1094); '
                                 'an initial migration package cannot be built from it')
            add('bin/lamp-keeper', data, True)
        elif name in ('lamp-voice', 'lamp-turn-end'):
            # The turn worker and model must live beside the voice executable.
            continue
        elif name == 'lamp-body-worker':
            continue
        else:
            add(f'runtime/{part}/{digest(data)}/{name}', data, True)
    voice_dir = f'runtime/releases/{binary_hashes["lamp-voice"]}'
    for name in ('lamp-voice', 'lamp-turn-end'):
        add(f'{voice_dir}/{name}', read_file(binaries, name), True)
    add(f'runtime/body/{binary_hashes["lamp-body"]}/lamp-body-worker', read_file(binaries, 'lamp-body-worker'), True)
    if generic:
        # What scripts/provision/lamp-v2-migrate reads from this package on a V1
        # lamp: the bench tool for its poses and V2's own setting templates.
        for name in sorted(provisioning_binaries):
            data = read_file(binaries, name)
            check_elf(data, name)
            add(f'tools/{name}', data, True)
            if name == 'lamp-body-measure' and SEATED_MEASURE_UNIT in units:
                add(f'runtime/body/{binary_hashes["lamp-body"]}/{name}', data, True)
        for name in PROVISION_SETTINGS:
            data = read_file(source, f'deploy/product/settings/{name}')
            if not isinstance(json.loads(data), dict) or SECRET.search(data.decode()):
                raise ValueError(f'invalid nonsecret setting template: {name}')
            add(f'provision/{name}', data)
    for name, expected in (ASSETS | OPTIONAL_ASSETS).items():
        if name in OPTIONAL_ASSETS and not (Path(assets) / name).exists():
            continue
        data = read_file(assets, name)
        if digest(data) != expected:
            raise ValueError(f'asset checksum mismatch: {name}')
        if name.startswith(('smart-turn', 'libonnxruntime')):
            directory = voice_dir
        elif name.startswith('face_'):
            directory = 'runtime/eyes'
        else:
            directory = 'runtime/name/models'
        add(f'{directory}/{name}', data)
    for relative, target in (
        ('models/name/name.onnx', 'runtime/name/models/name.onnx'),
        ('models/stop/stop.onnx', 'runtime/stop/models/stop.onnx'),
        ('models/policy/local-weights.json', 'runtime/policy/local-weights.json'),
        ('deploy/character/lamp.txt', 'runtime/character/lamp.txt'),
    ):
        data = read_file(source, relative)
        if data.startswith(b'version https://git-lfs.github.com/spec/v1'):
            raise ValueError(f'Git LFS object not materialized: {relative}')
        add(target, data)
    if 'lamp-controls.service' in units and not generic:
        for name in ('controls.json', 'controls-pins.json'):
            data = read_file(assets, f'config/{name}')
            parsed = json.loads(data)
            if not isinstance(parsed, dict) or SECRET.search(data.decode()):
                raise ValueError(f'invalid nonsecret controls config: {name}')
            add(f'runtime/controls/{binary_hashes["lamp-controls"]}/{name}', data)
    mapping = dict(substitutions)
    mapping.update({name.removeprefix('lamp-').upper().replace('-', '_') + '_SHA256': sha
                    for name, sha in binary_hashes.items()})
    declared_placeholders = set()
    for unit in sorted(units):
        if not isinstance(unit, str) or not re.fullmatch(r'[a-z][a-z0-9-]*\.(service|target|slice)', unit):
            raise ValueError(f'invalid unit name: {unit}')
        text = read_file(source, f'deploy/units/{unit}').decode()
        # Template documentation uses @NAME@ as a metasyntactic example.
        text = '\n'.join(line for line in text.splitlines() if not line.lstrip().startswith('#')) + '\n'
        if unit.endswith('.service'):
            if text.count('[Service]') != 1:
                raise ValueError(f'{unit}: expected exactly one Service section')
            text = text.replace('[Service]', '[Service]\nEnvironment=LD_LIBRARY_PATH=@RELEASE_ROOT@/lib', 1)
        if generic and unit == 'lamp-policy.service':
            text = generic_policy(text, read_file(source, 'deploy/product/lamp-policy.service').decode())
        # The seated read is a oneshot with no product unit and no bus.
        if generic and unit.startswith('lamp-') and unit.endswith('.service') and unit != SEATED_MEASURE_UNIT:
            text = generic_bus(unit, text, read_file(source, f'deploy/product/{unit}').decode())
        for key, value in mapping.items():
            text = text.replace(f'@{key}@', value)
        if generic and unit == 'lamp-controls.service':
            for name in ('controls.json', 'controls-pins.json'):
                text = text.replace(f'/opt/lampOS/controls/{binary_hashes["lamp-controls"]}/{name}',
                                    f'/etc/lampOS/controls/{name}')
        text = text.replace('/opt/lampOS/', '@RELEASE_ROOT@/runtime/')
        unresolved = set(TOKEN.findall(text)) - {'@RELEASE_ROOT@'}
        if generic:
            declared_placeholders.update(token.strip('@') for token in unresolved)
            unresolved -= {f'@{token}@' for token in PROFILE_TOKENS}
        if unresolved or SECRET.search(text):
            raise ValueError(f'{unit}: unresolved or sensitive unit: {sorted(unresolved)}')
        for path in re.findall(r'@RELEASE_ROOT@/([^\s";]+)', text):
            if path not in entries and not any(name.startswith(path.rstrip('/') + '/') for name in entries):
                raise ValueError(f'{unit}: missing packaged path: {path}')
        for line in text.splitlines():
            if line.startswith('LoadCredential='):
                credential = line.split('=', 1)[1].split(':', 1)
                if len(credential) != 2 or not credential[1].startswith('/'):
                    raise ValueError(f'{unit}: unsupported credential source')
                required_paths.append(credential[1])
            if line.startswith('EnvironmentFile=/'):
                required_paths.append(line.split('=', 1)[1])
        add(f'units/{unit}', text.encode())
    add('SERVICE_CLEANUP', b'1\n')
    # The updater runs `bin/service prepare` before stopping V1; initial migration requires it.
    add('SERVICE_PREFLIGHT', b'1\n')
    add('package-manifest.json', (json.dumps({'format': 1, **({'profile_path': '/etc/lampOS/package-profile.json',
            'placeholders': sorted(declared_placeholders)} if generic else {'unit_id': substitutions['UNIT_ID']}),
        'required_paths': sorted(set(required_paths)), 'units': sorted(units)}, sort_keys=True) + '\n').encode())
    add('VERSION', (version + '\n').encode())
    manifest = {'schema': 1, 'version': version, 'source_commit': source_commit, 'device': substitutions.get('UNIT_ID'),
                'units': sorted(units), 'binaries': binary_hashes,
                'files': {name: {'sha256': digest(data), 'mode': oct(mode)}
                          for name, (data, mode) in sorted(entries.items())}}
    add('package.json', (json.dumps(manifest, indent=2, sort_keys=True) + '\n').encode())
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(output.name + '.partial')
    try:
        with zipfile.ZipFile(temporary, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
            for name, (data, mode) in sorted(entries.items()):
                info = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
                info.create_system = 3
                info.external_attr = (stat.S_IFREG | mode) << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                archive.writestr(info, data)
        temporary.replace(output)
    finally:
        temporary.unlink(missing_ok=True)
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile')
    for name in ('source', 'binaries', 'assets', 'version', 'output', 'adapter', 'libraries', 'source-commit'):
        parser.add_argument('--' + name, required=True)
    args = parser.parse_args()
    try:
        package(args.source, args.binaries, args.assets,
                json.loads(Path(args.profile).read_text()) if args.profile else None, args.version, args.output, args.adapter, args.libraries, args.source_commit)
    except (OSError, ValueError, KeyError) as error:
        parser.exit(1, f'lampOS package: {error}\n')


if __name__ == '__main__':
    main()
