#!/usr/bin/env bash
# lamp-tasks built static (#7133): lamp-update's byte copy of it is the
# root-owned skill-start launcher an OpenClaw agent may exec
# (/usr/local/lib/lampos/skill-start, #7135). With no ELF interpreter, ld.so
# never runs, so LD_PRELOAD, LD_LIBRARY_PATH and LD_AUDIT can't put code in it
# before main. lamp-tasks does no name lookups (no DNS, no user or group
# database), so glibc's static link needs none of its NSS modules.
#
#   tools/build-lamp-tasks-static.sh OUT_DIR
#
# Builds lamp-tasks alone with +crt-static for the host's own target, or
# LAMP_TASKS_TARGET when cross-compiling (its own target dir, so the workspace
# build's flags are untouched), checks it has no INTERP program header, and
# installs it as OUT_DIR/lamp-tasks. Run from lampOS/. Exit 1 when it isn't
# static.
set -euo pipefail
[ "$#" -eq 1 ] || { echo "usage: $0 OUT_DIR" >&2; exit 2; }
out=$1
triple=${LAMP_TASKS_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static" \
  cargo build --locked --release -p lamp-tasks --bin lamp-tasks \
  --target "$triple" --target-dir target/lamp-tasks-static
bin=target/lamp-tasks-static/$triple/release/lamp-tasks
if readelf -l "$bin" | grep -q 'INTERP'; then
  echo "$bin has an ELF interpreter: not static" >&2
  exit 1
fi
install -m 0755 "$bin" "$out/lamp-tasks"
echo "lamp-tasks static: $out/lamp-tasks"
