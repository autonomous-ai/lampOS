#!/usr/bin/env bash
# lamp-tasks built static (#7133): lamp-update's byte copy of it is the
# root-owned skill-start launcher an OpenClaw agent may exec
# (/usr/local/lib/lampos/skill-start, #7135). With no ELF interpreter, ld.so
# never runs, so LD_PRELOAD, LD_LIBRARY_PATH and LD_AUDIT can't put code in it
# before main. lamp-tasks does no name lookups (no DNS, no user or group
# database), so glibc's static link needs none of its NSS modules.
#
#   deploy/build-lamp-tasks-static.sh OUT_DIR
#
# Builds lamp-tasks alone with +crt-static for the host's own target, or
# LAMP_TASKS_TARGET when cross-compiling (its own target dir, so the workspace
# build's flags are untouched), checks it has no INTERP program header, and
# installs it as OUT_DIR/lamp-tasks. Run from lampOS/. Exit 1 when it isn't
# static, or when readelf can't say (fails closed). When the target is the
# host's own, the static copy is also run once as the launcher with no run
# id, which must refuse with exit 2 (`usage: skill-start <run-id>`).
set -euo pipefail
[ "$#" -eq 1 ] || { echo "usage: $0 OUT_DIR" >&2; exit 2; }
out=$1
triple=${LAMP_TASKS_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static" \
  cargo build --locked --release -p lamp-tasks --bin lamp-tasks \
  --target "$triple" --target-dir target/lamp-tasks-static
bin=target/lamp-tasks-static/$triple/release/lamp-tasks
headers=$(readelf -l "$bin") || { echo "readelf could not read $bin" >&2; exit 1; }
grep -qi 'program headers' <<<"$headers" || { echo "readelf printed no program headers for $bin" >&2; exit 1; }
if grep -q 'INTERP' <<<"$headers"; then
  echo "$bin has an ELF interpreter: not static" >&2
  exit 1
fi
install -m 0755 "$bin" "$out/lamp-tasks"
if [ "$triple" = "$(rustc -vV | sed -n 's/^host: //p')" ]; then
  # The launcher with no run id refuses before it connects anywhere.
  code=0; "$out/lamp-tasks" skill-start >/dev/null 2>&1 || code=$?
  [ "$code" -eq 2 ] || { echo "the static lamp-tasks as skill-start with no run id exited $code, not 2" >&2; exit 1; }
  echo "lamp-tasks static: runs as skill-start (no run id: exit 2)"
fi
echo "lamp-tasks static: $out/lamp-tasks"
