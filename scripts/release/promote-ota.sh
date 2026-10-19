#!/usr/bin/env bash
# Raise an OTA component's min_version (default: its current version) so bootstrap auto-rolls it out.
# Usage: ./scripts/release/promote-ota.sh <component|device:<type>> [min_version]
# FOR=<device_type> raises min_version_by_device[<device_type>] instead (other types keep min_version).
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/ota-config.sh"
source "${RELEASE_DIR}/ota-metadata.sh"

component="${1:-}"
override_min="${2:-}"
if [[ "$component" == "device" ]]; then
  dtype="${2:-}"
  if [[ -z "$dtype" ]]; then
    echo "Usage: $0 device <type> [min_version]   (e.g. device lamp)" >&2
    exit 1
  fi
  component="device:${dtype}"
  override_min="${3:-}"
fi
if [[ -z "$component" ]]; then
  echo "Usage: $0 <component> [min_version]   (flat key | device <type> | device:<type>)" >&2
  exit 1
fi

# Activation is explicit; signing follows the configured feed trust mode.
if [[ -n "${ACTIVATE:-}" ]]; then
  ota_metadata_signing_preflight
  [[ "$ACTIVATE" = 0 || "$ACTIVATE" = 1 ]] || { echo 'ACTIVATE must be 0 or 1' >&2; exit 1; }
  [[ -n "${FOR:-}" ]] || { echo 'Activation promotion requires FOR=<device_type>' >&2; exit 1; }
fi

METADATA_PATH="${BUCKET_PREFIX}/ota/metadata.json"
METADATA_GCS="gs://${GCS_BUCKET}/${METADATA_PATH}"
METADATA_TMP=$(mktemp)
PAYLOAD_TMP=$(mktemp)
trap 'rm -f "$METADATA_TMP" "$PAYLOAD_TMP"' EXIT

echo "========== Fetch metadata from ${METADATA_GCS} =========="
ota_metadata_fetch "$METADATA_GCS" "$METADATA_TMP"
if [[ -n "${ACTIVATE:-}" ]]; then
  ota_metadata_preserve_trust "$METADATA_TMP"
fi
ota_metadata_unpack "$METADATA_TMP" "$PAYLOAD_TMP"
# Ordinary service updates use the same trust policy as service activation.
# Legacy component promotion keeps its existing behavior.
if jq -e --arg component "$component" '.[$component] | type == "object" and .service == $component' "$PAYLOAD_TMP" >/dev/null; then
  ota_metadata_signing_preflight
  ota_metadata_preserve_trust "$METADATA_TMP"
fi

python3 - "$PAYLOAD_TMP" "$component" "$override_min" "${FOR:-}" "${ACTIVATE:-}" <<'PY'
import json, re, sys

path, comp, override, only_for = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
activation = sys.argv[5]
data = json.load(open(path))

if comp.startswith("device:"):
    dtype = comp.split(":", 1)[1]
    entry = data.get("devices", {}).get(dtype)
    where = "devices.%s" % dtype
else:
    entry = data.get(comp)
    where = comp

if not isinstance(entry, dict):
    sys.exit("no metadata entry for %s" % where)

target = override or entry.get("version")
if not target:
    sys.exit("%s has no version to promote to" % where)

if activation:
    reserved = {'hal', 'os-server', 'hermes', 'openclaw', 'codex', 'claudecode', 'picoclaw', 'opencode',
                'bootstrap', 'bootstrap-server', 'device', 'web', 'nginx', 'claude-desktop-buddy',
                'ssh', 'sshd', 'dbus', 'systemd', 'networking', 'network-manager', 'system', 'agent',
                'devices', 'signed', 'recover', 'rollback'}
    if (activation not in ('0', '1') or not re.fullmatch(r'[a-z][a-z0-9-]{0,47}', comp)
            or comp in reserved or entry.get('service') != comp or not only_for):
        sys.exit('Activation requires a service component and FOR=<device_type>')
    def version(value):
        if not isinstance(value, str) or not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', value):
            sys.exit('Activation requires numeric major.minor.patch versions')
        return tuple(map(int, value.split('.')))
    if activation == '1' and (version(target) == (0, 0, 0) or version(target) != version(entry.get('version'))):
        sys.exit('Activation must select the exact current published version')
    if activation == '1' and not re.fullmatch(r'[0-9a-f]{64}', entry.get('sha256', '')):
        sys.exit('Activation requires the published artifact SHA-256')
    activations = entry.setdefault('activate_by_device', {})
    if not isinstance(activations, dict) or any(not isinstance(k, str) or not k or type(v) is not bool for k, v in activations.items()):
        sys.exit('Invalid activate_by_device map')
    releases = entry.setdefault('activation_release_by_device', {})
    if not isinstance(releases, dict):
        sys.exit('Invalid activation_release_by_device map')
    activations[only_for] = activation == '1'
    if activation == '1':
        releases[only_for] = {'version': entry['version'], 'sha256': entry['sha256']}
    else:
        releases.pop(only_for, None)
    if activation == '0':
        json.dump(data, open(path, 'w'), indent=2)
        print('%s: automatic activation disabled for %s; existing installations unchanged' % (where, only_for))
        sys.exit(0)

if only_for:
    floors = entry.setdefault("min_version_by_device", {})
    old = floors.get(only_for, "(unset -> min_version)")
    floors[only_for] = target
    field = "min_version_by_device.%s" % only_for
else:
    old = entry.get("min_version", "(unset -> version)")
    entry["min_version"] = target
    field = "min_version"
json.dump(data, open(path, "w"), indent=2)
print("%s: %s %s -> %s" % (where, field, old, target))
PY

echo "========== Upload metadata to ${METADATA_GCS} =========="
ota_metadata_sign "$PAYLOAD_TMP" "$METADATA_TMP"
ota_metadata_publish "$METADATA_TMP" "$METADATA_GCS"

echo "Promoted. Devices with current version below min_version will now auto-update."
