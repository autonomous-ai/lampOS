# el-8400 kit, in the rig container (sourced). Paths and the build pair from STAGE.env.
K=/var/tmp/el-8400
OUT=$K/out
UNITS=/etc/systemd/system
REC=/var/lib/lampos-update
PROD_FP=54213f8d3aa2f241
[ -f "$K/STAGE.env" ] && . "$K/STAGE.env"

say() { echo "el-8400: $*"; }
die() { echo "el-8400: STOP: $*"; exit 1; }

# The first 16 hex of the SHA-256 of a base64 key line: lamp-update's fingerprint.
fp() { python3 -I -c "import base64,hashlib,sys;print(hashlib.sha256(base64.b64decode(open(sys.argv[1]).read().strip())).hexdigest()[:16])" "$1"; }

# lamp-update's own unit's Environment= lines (unit, then its drop-ins), as `env` words:
# a round here runs with what lamp-update.service would give it (LAMP_UPDATE_SELF=on among them).
unit_env() {
  for f in "$UNITS/lamp-update.service" "$UNITS"/lamp-update.service.d/*.conf; do
    [ -f "$f" ] && sed -n 's/^Environment=//p' "$f"
  done | tr '\n' ' '
}

mark() { echo "$(date +%s.%N) ===== $*" >> "$K/calls.log"; }

# Every step but rig-init runs only on the rig: the logged systemctl stand-in is there and no
# systemd runs. Never on a lamp (snap.sh restore replaces whole folders, fine for a throwaway
# container only).
rig_only() {
  grep -q 'el-8400 rig' /usr/local/bin/systemctl 2>/dev/null || die "not the el-8400 rig (no stand-in systemctl): never run this on a lamp"
  [ -e /run/systemd/system ] && die "systemd runs here: this is not the rig"
  [ "$(uname -m)" = aarch64 ] || die "not aarch64"
  return 0
}
