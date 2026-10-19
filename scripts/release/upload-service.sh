#!/usr/bin/env bash
# Publish a prebuilt service package; activation is controlled by explicit per-device promotion.
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/ota-config.sh"
source "${RELEASE_DIR}/ota-metadata.sh"
: "${COMPONENT:?COMPONENT is required}"
: "${V:?V is required}"
: "${PACKAGE:?PACKAGE is required}"
ota_metadata_signing_preflight
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cp "$PACKAGE" "$tmp/package.zip"
metadata="gs://${GCS_BUCKET}/${BUCKET_PREFIX}/ota/metadata.json"
# Never replace an existing feed with an empty document on a fetch failure.
ota_metadata_fetch "$metadata" "$tmp/envelope.json"
ota_metadata_preserve_trust "$tmp/envelope.json"
ota_metadata_unpack "$tmp/envelope.json" "$tmp/payload.json"
sha=$(ota_artifact_sha256 "$tmp/package.zip")
url="https://storage.googleapis.com/${GCS_BUCKET}/${BUCKET_PREFIX}/services/${COMPONENT}/${sha}.zip"
python3 - "$tmp/payload.json" "$COMPONENT" "$V" "$tmp/package.zip" "$url" "$sha" "${REPLACES:-}" <<'PY'
import json, re, stat, sys, zipfile
from pathlib import PurePosixPath
path, key, version, archive, url, digest, replacements = sys.argv[1:]
legacy = {'hal', 'os-server'}
reserved = legacy | {'hermes', 'openclaw', 'codex', 'claudecode', 'picoclaw', 'opencode', 'bootstrap', 'bootstrap-server', 'device', 'web', 'nginx', 'claude-desktop-buddy',
                     'ssh', 'sshd', 'dbus', 'systemd', 'networking', 'network-manager', 'system', 'agent', 'devices', 'signed', 'recover', 'rollback', 'rollback-to-legacy'}
def require(ok, message):
    if not ok:
        sys.exit(message)
require(re.fullmatch(r'[a-z][a-z0-9-]{0,47}', key) and key not in reserved, 'Invalid service component')
require(re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', version), 'V must be a numeric major.minor.patch version')
data = json.load(open(path))
require(isinstance(data, dict), 'Metadata must be an object')
replaces = replacements.split()
require(len(set(replaces)) == len(replaces), 'Duplicate replacements')
for old in replaces:
    entry = data.get(old)
    require(old != key and (old in legacy or (isinstance(entry, dict) and entry.get('service') == old
            and re.fullmatch(r'[a-z][a-z0-9-]{0,47}', old) and old not in reserved)), 'Unknown replacement: ' + old)
entry = data.get(key, {})
require(isinstance(entry, dict), 'Invalid existing component')
if key in data:
    require(entry.get('service') == key and sorted(entry.get('replaces', [])) == sorted(replaces),
            'Topology is immutable; publish and activate a new component')
    previous = entry.get('version', '')
    require(isinstance(previous, str) and re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', previous), 'Invalid published version')
    require(tuple(map(int, version.split('.'))) >= tuple(map(int, previous.split('.'))), 'Cannot publish a version older than the current release')
    require(previous != version or entry.get('sha256') == digest, 'Version already published with different content')
with zipfile.ZipFile(archive) as package:
    names, total = set(), 0
    for item in package.infolist():
        p, mode = PurePosixPath(item.filename), item.external_attr >> 16
        require(not p.is_absolute() and '..' not in p.parts and '\\' not in item.filename and p.parts
                and str(p) not in names and stat.S_IFMT(mode) in (0, stat.S_IFREG, stat.S_IFDIR), 'Unsafe archive entry')
        names.add(str(p))
        total += item.file_size
        require(total <= 1024 ** 3 and len(names) <= 10000, 'Archive exceeds extraction limit')
    require('VERSION' in names and 'bin/service' in names, 'Missing VERSION or bin/service')
    require(package.read('VERSION').decode().strip() == version, 'VERSION mismatch')
    require(package.getinfo('bin/service').external_attr >> 16 & 0o111, 'bin/service must be executable')
if entry.get('version') != version or entry.get('sha256') != digest:
    # Older bootstrap versions ignore release bindings: clear their boolean gate too.
    if 'activate_by_device' in entry:
        entry['activate_by_device'] = {device: False for device in entry['activate_by_device']}
    entry.pop('activation_release_by_device', None)
entry.update(version=version, url=url, sha256=digest, service=key, replaces=replaces)
data[key] = entry
with open(path, 'w') as output:
    json.dump(data, output, indent=2)
PY
# Sign before publishing anything, so an invalid signing key cannot publish a release.
ota_metadata_sign "$tmp/payload.json" "$tmp/envelope.json"
gsutil cp "$tmp/package.zip" "gs://${GCS_BUCKET}/${BUCKET_PREFIX}/services/${COMPONENT}/${sha}.zip"
ota_metadata_publish "$tmp/envelope.json" "$metadata"
printf 'Published %s %s. Promote automatic migration with make promote-service COMPONENT=%s FOR=<device_type> V=%s ACTIVATE=1.\n' "$COMPONENT" "$V" "$COMPONENT" "$V"
