#!/bin/bash
# Check rendered build arguments and the actual profile block on owned files.
# MAKE=: prevents GNU make's recursive-command exception from executing a build
# during -n. No Docker, mounts, image content or devices are used.
set -euo pipefail
SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
MAKEFILE="$SCRIPT_DIR/../Makefile"
BUILDER="$SCRIPT_DIR/../build-orangepi-v2.sh"
ROOT=$(mktemp -d "${TMPDIR:-/tmp}/v2-image-inputs.XXXXXX")
ROOT=$(cd "$ROOT" && pwd -P)
trap 'rm -rf "$ROOT"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
MAKE_COMMAND=${MAKE:-make}
mkdir "$ROOT/bin"
printf '#!/bin/sh\nexit 1\n' > "$ROOT/bin/git"
chmod +x "$ROOT/bin/git"
fail() { echo "FAIL: $*" >&2; exit 1; }
passed=0
pass() { passed=$((passed + 1)); printf 'PASS: %s\n' "$1"; }

render() {
  local output=$1
  shift
  (
    cd "$ROOT"
    unset MAKEFLAGS MFLAGS MAKEOVERRIDES MAKEFILES CASE_COLOR OUT_IMG_SIZE
    export PATH="$ROOT/bin:$PATH"
    "$MAKE_COMMAND" --no-print-directory -f "$MAKEFILE" -n build-v2 \
      MAKE=: VERSION=fixture LAMPOS_UPDATE_KEY=/fixture/public-key "$@"
  ) > "$output"
  grep -F 'pi-builder /work/build-orangepi-v2.sh' "$output" >/dev/null || fail 'build command missing'
  [ ! -e "$ROOT/input" ] && [ ! -e "$ROOT/output" ] || fail 'dry run executed staging or output work'
}
render "$ROOT/explicit.txt" OUT_IMG_SIZE=16G
grep -F -- '-e OUT_IMG_SIZE=16G ' "$ROOT/explicit.txt" >/dev/null || fail 'explicit size was not forwarded'
pass explicit_size_is_forwarded
render "$ROOT/default.txt"
grep -F -- '-e OUT_IMG_SIZE= ' "$ROOT/default.txt" >/dev/null || fail 'default size was not forwarded'
grep -F 'OUT_IMG_SIZE="${OUT_IMG_SIZE:-8G}"' "$BUILDER" >/dev/null || fail 'default size changed'
pass empty_size_keeps_8G_default

# This exact block is the only variant-marker writer. Exercise the production
# block, including its actual removal command, with no unrelated image phases.
sed -n '/^if \[ "$VARIANT" != standard \]; then$/,/^fi$/p' "$BUILDER" > "$ROOT/profile.sh"
[ "$(grep -c '^if ' "$ROOT/profile.sh")" -eq 1 ] || fail 'profile block is missing or ambiguous'
for scenario in stale-standard fresh-standard selected-pro; do
  MNT="$ROOT/$scenario"
  mkdir -p "$MNT/etc/autonomous"
  printf 'keep\n' > "$MNT/etc/autonomous/sentinel"
  VARIANT=standard
  if [ "$scenario" != fresh-standard ]; then
    printf 'pro-respeaker-lite\n' > "$MNT/etc/autonomous/hardware-profile"
  fi
  [ "$scenario" != selected-pro ] || VARIANT=pro
  MNT="$MNT" VARIANT="$VARIANT" bash -eu "$ROOT/profile.sh"
  if [ "$VARIANT" = standard ]; then
    [ ! -e "$MNT/etc/autonomous/hardware-profile" ] || fail "$scenario retained a profile"
  else
    printf 'pro\n' > "$ROOT/expected"
    cmp "$ROOT/expected" "$MNT/etc/autonomous/hardware-profile" || fail 'wrong profile written'
  fi
  [ "$(cat "$MNT/etc/autonomous/sentinel")" = keep ] || fail 'unrelated file changed'
  pass "$scenario"
done
printf '%s passed; 0 failed\n' "$passed"
