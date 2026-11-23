#!/bin/sh
# In a fresh rig container only (debian:bookworm, linux/arm64 under qemu): the libraries lamp
# binaries need, and the logged stand-ins for systemctl, journalctl, systemd-tmpfiles and
# systemd-run. Refuses on anything that already has systemd or a lampOS install.
set -u
. /var/tmp/el-8400/lamp/common.sh
[ "$(uname -m)" = aarch64 ] || die "not aarch64 ($(uname -m)): the container must be --platform linux/arm64"
[ -e /opt/lampOS ] && die "/opt/lampOS is there: this is not a fresh rig container"
[ -e /run/systemd/system ] && die "systemd runs here: this kit is for the qemu rig, not a lamp"
for b in systemctl journalctl systemd-tmpfiles systemd-run; do
  p=$(command -v "$b" 2>/dev/null) && [ "$p" != "/usr/local/bin/$b" ] && die "a real $b is at $p"
done
export DEBIAN_FRONTEND=noninteractive
apt-get update -q >/dev/null && apt-get install -y -q --no-install-recommends \
  libasound2 python3 ca-certificates procps >/dev/null || die "apt-get install failed"
for b in systemctl journalctl systemd-tmpfiles systemd-run; do
  install -m 0755 "$K/lamp/stub/$b" "/usr/local/bin/$b"
done
mkdir -p /var/lib/el-8400-stub/active "$OUT"
: > "$K/calls.log"
# An idle, set-up lamp's two active units (first builder's recipe, #8353 10:59).
echo "active active 0 yes success" > /var/lib/el-8400-stub/active/lampos.target
echo "active waiting 0 yes success" > /var/lib/el-8400-stub/active/lamp-update-back.timer
say "rig ready: $(uname -m), $(python3 --version), stand-ins in /usr/local/bin"
