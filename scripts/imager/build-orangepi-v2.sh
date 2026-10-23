#!/bin/bash
# V2 golden image for a lamp (OrangePi 4 Pro v2, A733): a new lamp boots straight into lampOS's setup (#2538).
# The base and board steps are build-orangepi.sh's; the payload is lampOS's signed build, put on by its own
# `lamp-update install --setup on`, with no V1 part (no os-server, nginx site, HAL, OpenClaw or bootstrap).
# Phases: source, expand, base packages and radio, the image's own files, lampOS, resize-once, finalize.
# Run via the imager Makefile (`make build-v2`, Docker, --privileged for losetup/mount).
#
# Inputs, staged under /input/lamp-v2/ by the Makefile:
#   key.pub           the update key's public half (/etc/lampOS/update/key.pub); required
#   channel/          a signed channel folder (lampos-manifest.json and the build's files); optional:
#                     without it the build is read from LAMPOS_CHANNEL_URL
#   robots-lamp/      robots/lamp's rootfs/ and overrides/
#   shipped.json      lampOS/deploy/image/shipped.json: which of those files are the image's for V2
#   models.json       lampOS/deploy/product/models.json: the models install places, pinned
#   models/           those models' release files; optional, used only for what install cannot fetch

set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib/check_ext4.sh"

DEVICE_TYPE="lamp"
OUT_IMG_SIZE="${OUT_IMG_SIZE:-8G}"
AP_BAND="${AP_BAND:-2.4}"
AP_CHANNEL="${AP_CHANNEL:-}"
COUNTRY_CODE="${COUNTRY_CODE:-US}"
VARIANT="${VARIANT:-}"
VARIANT="${VARIANT:-standard}"
if [[ ! "$VARIANT" =~ ^[a-z][a-z0-9_-]{0,63}$ ]]; then
  echo "Invalid VARIANT: expected a lowercase name (1-64 characters)" >&2
  exit 1
fi
# The product's channel (deploy/product/lamp-update.service); a local channel folder replaces it at build.
LAMPOS_CHANNEL_URL="${LAMPOS_CHANNEL_URL:-https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice/}"
LAMPOS_CHANNEL="${LAMPOS_CHANNEL:-lamp-voice}"

V2_INPUT="/input/lamp-v2"
DEVICE_BASE_IMG="${DEVICE_BASE_IMG:-/input/lamp/golden-opi-dev.img.xz}"
MNT="/mnt/opi"
SRC_IMG="/work/base-lamp-v2.img"
OUT_DIR="/output/lamp-v2"
FINAL_IMG="${OUT_DIR}/lamp-os-v2.img"
OUT_IMG="${FINAL_IMG}.building"
# Where the channel folder sits inside the image while install reads it; removed after.
CHROOT_CHANNEL="/var/tmp/lampos-channel"

LOOP_DEV=""
PART_LOOP=""

cleanup() {
  set +e
  mountpoint -q "${MNT}/dev"  && umount -lf "${MNT}/dev"
  mountpoint -q "${MNT}/sys"  && umount -lf "${MNT}/sys"
  mountpoint -q "${MNT}/proc" && umount -lf "${MNT}/proc"
  mountpoint -q "${MNT}"      && umount -lf "${MNT}"
  [ -n "${PART_LOOP}" ] && losetup -d "${PART_LOOP}" 2>/dev/null
  [ -n "${LOOP_DEV}" ]  && losetup -d "${LOOP_DEV}"  2>/dev/null
}
trap cleanup EXIT

log() { echo "==> $*"; }
err() { echo "ERROR: $*" >&2; exit 1; }

for bin in losetup parted resize2fs e2fsck qemu-aarch64-static xz growpart jq sha256sum base64; do
  command -v "$bin" >/dev/null || err "missing tool: $bin (check Dockerfile)"
done
[ -s "${V2_INPUT}/key.pub" ] || err "no update key at ${V2_INPUT}/key.pub — build via 'make build-v2 LAMPOS_UPDATE_KEY=...'"
[ -s "${V2_INPUT}/shipped.json" ] || err "no ${V2_INPUT}/shipped.json — build via 'make build-v2'"
[ -d "${V2_INPUT}/robots-lamp/rootfs" ] || err "no ${V2_INPUT}/robots-lamp/rootfs — build via 'make build-v2'"
jq -e --arg v "$VARIANT" '.variants[$v]' "${V2_INPUT}/shipped.json" >/dev/null \
  || err "VARIANT=${VARIANT} is not in shipped.json (one of: $(jq -r '.variants|keys|join(" ")' "${V2_INPUT}/shipped.json"))"
if [ -d "${V2_INPUT}/channel" ]; then
  [ -s "${V2_INPUT}/channel/lampos-manifest.json" ] || err "${V2_INPUT}/channel has no lampos-manifest.json"
fi
mkdir -p /output "${OUT_DIR}" /work "${MNT}"

# Phase 0: source base image (the hardware team's lamp base, as build-orangepi.sh).
log "Base image: ${DEVICE_BASE_IMG}"
[ -f "${DEVICE_BASE_IMG}" ] || err "Base image not found: ${DEVICE_BASE_IMG} — place it at imager/input/lamp/"
xz -dkc --threads=0 "${DEVICE_BASE_IMG}" > "${SRC_IMG}"

# Phase 1: expand, resize2fs, mount.
log "Copying source → ${OUT_IMG} and expanding to ${OUT_IMG_SIZE}…"
TARGET_BYTES=$(numfmt --from=iec "${OUT_IMG_SIZE}")
[ "$(stat -c %s "${SRC_IMG}")" -le "${TARGET_BYTES}" ] \
  || err "OUT_IMG_SIZE would truncate the source image"
mv -f "${SRC_IMG}" "${OUT_IMG}"
truncate -s "${OUT_IMG_SIZE}" "${OUT_IMG}"

LOOP_DEV=$(losetup --find --show "${OUT_IMG}")
sleep 1
growpart "${LOOP_DEV}" 1 || parted -s "${LOOP_DEV}" resizepart 1 100%

# No udev in Docker Desktop, so /dev/loopXp1 never appears; attach the partition by offset.
PART_START=$(parted -s "${LOOP_DEV}" unit B print | awk '/^ 1/{gsub(/B/,""); print $2}')
PART_SIZE=$( parted -s "${LOOP_DEV}" unit B print | awk '/^ 1/{gsub(/B/,""); print $4}')
log "Partition 1: start=${PART_START} size=${PART_SIZE}"
PART_LOOP=$(losetup --find --show --offset "${PART_START}" --sizelimit "${PART_SIZE}" "${OUT_IMG}")
PART="${PART_LOOP}"
[ -b "${PART}" ] || err "partition loop device ${PART} did not appear"

prepare_ext4_for_resize "${PART}"
resize2fs "${PART}"
mount "${PART}" "${MNT}"

python3 "$(dirname "${BASH_SOURCE[0]}")/lib/defer_orangepi_update_count.py" --root "${MNT}"

log "Setting up chroot…"
cp /usr/bin/qemu-aarch64-static "${MNT}/usr/bin/qemu-aarch64-static"
mount --bind /proc "${MNT}/proc"
mount --bind /sys  "${MNT}/sys"
mount --bind /dev  "${MNT}/dev"
cp -f "${MNT}/etc/resolv.conf" "${MNT}/etc/resolv.conf.bak" 2>/dev/null || true
cp -f /etc/resolv.conf "${MNT}/etc/resolv.conf"

chroot "${MNT}" debconf-set-selections <<'DBCONF' || true
debconf debconf/frontend select Noninteractive
keyboard-configuration keyboard-configuration/layoutcode string us
DBCONF
cat > "${MNT}/etc/apt/apt.conf.d/99-lamp-silent" <<'APT'
Dpkg::Use-Pty "false";
APT

# Phase 2: base packages and the radio setup needs (docs/install.md, "What the image must hold").
log "Phase 2 — base packages and radio"
# Unquoted heredoc so outer ${VAR} expand inside the chroot.
chroot "${MNT}" /bin/bash <<CHROOT_STAGES
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

echo "[stage] apt update + install"
apt-get update -qq
# lampOS's binaries need libasound2 beyond libc; setup needs the radio (hostapd, dnsmasq,
# wpa_supplicant, dhcpcd, iw, rfkill, killall) and nginx installed, not enabled, so its stop succeeds.
apt-get install -y \\
  hostapd dnsmasq nginx-light \\
  wpasupplicant dhcpcd5 openresolv \\
  iproute2 iw rfkill wireless-tools psmisc \\
  curl jq ca-certificates tzdata fake-hwclock systemd-timesyncd \\
  alsa-utils libasound2 \\
  cloud-guest-utils \\
  avahi-daemon libnss-mdns \\
  openssh-server
apt-get purge -y --auto-remove network-manager network-manager-gnome 2>/dev/null || true
apt-get clean

# Disable IPv6, as V1's image does (STA drops).
mkdir -p /etc/sysctl.d
cat > /etc/sysctl.d/99-lamp-wifi.conf <<'SYSCTL'
net.ipv6.conf.all.disable_ipv6 = 1
net.ipv6.conf.default.disable_ipv6 = 1
net.ipv6.conf.lo.disable_ipv6 = 1
SYSCTL

# resolvconf fallback DNS, so /etc/resolv.conf is never empty in AP mode.
if [ -f /etc/resolvconf.conf ]; then
  grep -q '^name_servers=' /etc/resolvconf.conf || echo 'name_servers="1.1.1.1 8.8.8.8"' >> /etc/resolvconf.conf
else
  echo 'name_servers="1.1.1.1 8.8.8.8"' > /etc/resolvconf.conf
fi

# nginx: installed for setup's free_port, never enabled, and with no site, so a restart by
# device-ap-mode holds no port.
rm -f /etc/nginx/sites-enabled/*
systemctl disable nginx 2>/dev/null || true

# wpa_supplicant@wlan0 override → use per-interface config, not global.
mkdir -p /etc/systemd/system/wpa_supplicant@wlan0.service.d
cat > /etc/systemd/system/wpa_supplicant@wlan0.service.d/override.conf <<'OVR'
[Service]
ExecStart=
ExecStart=/sbin/wpa_supplicant -c /etc/wpa_supplicant/wpa_supplicant-wlan0.conf -i wlan0 -D nl80211,wext
Restart=on-failure
RestartSec=5
OVR

# ── radio scripts: build-orangepi.sh's, which lamp-setup runs (crates/setup/src/wifi.rs) ──
echo "[stage] radio scripts"

cat > /usr/local/bin/device-ap-mode <<'EOFSCRIPT'
#!/bin/bash
set -e
echo "Switching to AP mode..."
for cmd in ip iw systemctl hostapd dnsmasq rfkill; do
  command -v "\$cmd" >/dev/null 2>&1 || { echo "Missing required command: \$cmd"; exit 1; }
done
rfkill unblock wlan 2>/dev/null || true
rfkill unblock wlan0 2>/dev/null || true
systemctl stop wpa_supplicant@wlan0 2>/dev/null || true
systemctl disable wpa_supplicant@wlan0 2>/dev/null || true
systemctl mask wpa_supplicant@wlan0 2>/dev/null || true
killall wpa_supplicant 2>/dev/null || true
# dhcpcd keeps the wired link; only wlan0 is denied it (prepended: options after an
# "interface X" line belong to that interface).
touch /etc/dhcpcd.conf
if ! grep -q '^denyinterfaces wlan0\$' /etc/dhcpcd.conf; then
  if [ -s /etc/dhcpcd.conf ]; then
    sed -i '1i denyinterfaces wlan0' /etc/dhcpcd.conf
  else
    echo 'denyinterfaces wlan0' > /etc/dhcpcd.conf
  fi
fi
systemctl enable dhcpcd 2>/dev/null || true
systemctl restart dhcpcd 2>/dev/null || true
systemctl stop NetworkManager systemd-networkd 2>/dev/null || true
rm -f /var/lib/dhcpcd5/dhcpcd-wlan0 2>/dev/null || true
rm -f /var/lib/dhcpcd/dhcpcd-wlan0 2>/dev/null || true

# SSID suffix from hardware serial or eth MAC.
SERIAL=\$(tr -d '\0' </proc/device-tree/serial-number 2>/dev/null || true)
if [ -z "\$SERIAL" ]; then
  SERIAL=\$(awk '/^Serial/ {print \$3}' /proc/cpuinfo 2>/dev/null || true)
fi
if [ -z "\$SERIAL" ]; then
  for iface in eth0 end0; do
    mac=\$(cat "/sys/class/net/\$iface/address" 2>/dev/null | tr -d ':' || true)
    if [ -n "\$mac" ] && [ "\$mac" != "000000000000" ]; then
      SERIAL=\$mac
      break
    fi
  done
fi
SUFFIX=\${SERIAL: -4}
SUFFIX_LC=\$(echo "\$SUFFIX" | tr '[:upper:]' '[:lower:]')
AP_SSID="lamp-\${SUFFIX_LC}"
[ -f /etc/hostapd/hostapd.conf ] && sed -i "s/^ssid=.*/ssid=\${AP_SSID}/" /etc/hostapd/hostapd.conf

# The host is lamp-xxxx, which setup reads after this script (crates/setup/src/serve.rs, start).
DEVICE_HOSTNAME="lamp-\${SUFFIX_LC}"
hostnamectl set-hostname "\$DEVICE_HOSTNAME" 2>/dev/null || hostname "\$DEVICE_HOSTNAME" || true
if grep -q '^127\.0\.1\.1' /etc/hosts; then
  sed -i "s/^127\.0\.1\.1.*/127.0.1.1 \$DEVICE_HOSTNAME/" /etc/hosts
else
  echo "127.0.1.1 \$DEVICE_HOSTNAME" >> /etc/hosts
fi
systemctl enable avahi-daemon 2>/dev/null || true
systemctl restart avahi-daemon 2>/dev/null || true

REG=\$(grep "^country_code=" /etc/hostapd/hostapd.conf 2>/dev/null | cut -d= -f2)
[ -z "\$REG" ] && REG=US
iw reg set "\$REG" 2>/dev/null || true

ip link set wlan0 down; sleep 1
iw dev wlan0 set type __ap
sleep 1
ip link set wlan0 up; sleep 1
ip addr flush dev wlan0
ip addr add 192.168.100.1/24 dev wlan0

command -v resolvconf >/dev/null 2>&1 && resolvconf -d wlan0.dhcp 2>/dev/null || true

grep -q '^address=/#/' /etc/dnsmasq.d/99-lamp.conf 2>/dev/null || echo 'address=/#/192.168.100.1' >> /etc/dnsmasq.d/99-lamp.conf

systemctl unmask hostapd dnsmasq 2>/dev/null || true
systemctl enable hostapd dnsmasq

systemctl restart nginx 2>/dev/null || true
systemctl restart dnsmasq

systemctl restart hostapd; sleep 2
if ! systemctl is-active --quiet hostapd; then
  echo "hostapd failed. Retrying..."
  systemctl restart hostapd; sleep 2
fi
if ! systemctl is-active --quiet hostapd; then
  echo "ERROR: hostapd still not running"
  journalctl -u hostapd -n 50 --no-pager || true
  exit 1
fi
echo "AP MODE ENABLED  SSID=\$AP_SSID  IP=192.168.100.1"
EOFSCRIPT
chmod +x /usr/local/bin/device-ap-mode

cat > /usr/local/bin/device-sta-mode <<'EOFSCRIPT'
#!/bin/bash
set -e
echo "Switching to STA mode..."
for cmd in ip iw systemctl rfkill; do
  command -v "\$cmd" >/dev/null 2>&1 || { echo "Missing required command: \$cmd"; exit 1; }
done
rfkill unblock wlan 2>/dev/null || true
rfkill unblock wlan0 2>/dev/null || true
systemctl stop hostapd dnsmasq 2>/dev/null || true
systemctl disable hostapd dnsmasq 2>/dev/null || true
killall hostapd 2>/dev/null || true
killall dnsmasq 2>/dev/null || true
ip link set wlan0 down 2>/dev/null || true; sleep 1
iw dev wlan0 set type managed
ip link set wlan0 up; sleep 1
ip addr flush dev wlan0
# Hand wlan0 back to dhcpcd.
sed -i '/static ip_address=192.168.100.1\\/24/d;/nohook wpa_supplicant/d;/^denyinterfaces wlan0\$/d' /etc/dhcpcd.conf 2>/dev/null || true
sed -i '/^address=\\/#\\//d' /etc/dnsmasq.d/99-lamp.conf 2>/dev/null || true
systemctl unmask wpa_supplicant@wlan0 2>/dev/null || true
systemctl enable wpa_supplicant@wlan0
systemctl restart wpa_supplicant@wlan0
systemctl enable dhcpcd
systemctl restart dhcpcd
echo "Waiting for IP..."; sleep 5
if ip addr show wlan0 | grep -q "inet "; then
  IP=\$(ip -4 addr show wlan0 | grep inet | awk '{print \$2}')
  echo "Connected. IP address: \$IP"
else
  echo "WARNING: wlan0 did not receive an IP address"
fi
systemctl restart avahi-daemon 2>/dev/null || true
echo "STA MODE ENABLED"
EOFSCRIPT
chmod +x /usr/local/bin/device-sta-mode

# ── network configs (hostapd, dnsmasq, wpa, dhcpcd) ──────────────────────────
echo "[stage] network configs"

# wpa_supplicant: country only; setup writes the home's network.
mkdir -p /etc/wpa_supplicant
cat > /etc/wpa_supplicant/wpa_supplicant-wlan0.conf <<EOF
country=${COUNTRY_CODE}
ctrl_interface=DIR=/run/wpa_supplicant
update_config=1
EOF
chmod 600 /etc/wpa_supplicant/wpa_supplicant-wlan0.conf

# hostapd: SSID placeholder, device-ap-mode replaces it at runtime.
if [ "${AP_BAND}" = "5" ]; then
  cat > /etc/hostapd/hostapd.conf <<EOF
interface=wlan0
driver=nl80211
ssid=lamp-xxxx
hw_mode=a
channel=${AP_CHANNEL:-36}
country_code=${COUNTRY_CODE}
ieee80211n=1
ieee80211ac=1
wmm_enabled=1
auth_algs=1
ignore_broadcast_ssid=0
EOF
else
  cat > /etc/hostapd/hostapd.conf <<EOF
interface=wlan0
driver=nl80211
ssid=lamp-xxxx
hw_mode=g
channel=${AP_CHANNEL:-6}
country_code=${COUNTRY_CODE}
ieee80211n=1
wmm_enabled=1
auth_algs=1
ignore_broadcast_ssid=0
EOF
fi
echo 'DAEMON_CONF="/etc/hostapd/hostapd.conf"' > /etc/default/hostapd
systemctl disable hostapd dnsmasq 2>/dev/null || true

mkdir -p /etc/dnsmasq.d
cat > /etc/dnsmasq.d/99-lamp.conf <<'EOF'
interface=wlan0
bind-interfaces
dhcp-range=wlan0,192.168.100.50,192.168.100.150,255.255.255.0,24h
address=/#/192.168.100.1
domain-needed
bogus-priv
no-resolv
EOF
[ -f /etc/dnsmasq.conf ] && sed -i 's/^interface=wlan0/#&/' /etc/dnsmasq.conf || true
# dnsmasq must not become the lamp's own resolver (its wildcard would point every name at itself).
if [ -f /etc/default/dnsmasq ]; then
  sed -i '/^[[:space:]]*#*[[:space:]]*DNSMASQ_EXCEPT=/d' /etc/default/dnsmasq
fi
echo 'DNSMASQ_EXCEPT="lo"' >> /etc/default/dnsmasq

systemctl mask wpa_supplicant.service 2>/dev/null || true
systemctl mask orangepi-firstrun-config.service 2>/dev/null || true

for unit in avahi-daemon ssh dhcpcd systemd-timesyncd fake-hwclock; do
  systemctl enable "\$unit" 2>/dev/null || true
done

# DT overlays (A733): SPI3 for the ring, I2C0 for the air sensor.
if grep -q "^overlays=" /boot/orangepiEnv.txt 2>/dev/null; then
  grep -q 'spi3-cs0-cs1-spidev' /boot/orangepiEnv.txt || sed -i "s/^overlays=.*/& spi3-cs0-cs1-spidev/" /boot/orangepiEnv.txt
  grep -q '\bi2c0\b' /boot/orangepiEnv.txt || sed -i "s/^overlays=.*/& i2c0/" /boot/orangepiEnv.txt
else
  echo "overlays=spi3-cs0-cs1-spidev i2c0" >> /boot/orangepiEnv.txt
fi

echo "[stage] chroot Phase 2 complete"
CHROOT_STAGES

# Phase 3: the image's own files, exactly the paths shipped.json names for this variant (#1369):
# robots/lamp/rootfs, then the variant's overrides/<variant>/rootfs over it. V1's parts in that
# tree (HAL's env, led-boot, led-shutdown, hal.service.d) are not the image's for V2 and stay out.
log "Phase 3 — the image's own files (variant ${VARIANT})"
mapfile -t IMAGE_FILES < <(jq -r --arg v "$VARIANT" '.variants[$v] | keys[]' "${V2_INPUT}/shipped.json")
for path in "${IMAGE_FILES[@]}"; do
  src=""
  for root in "${V2_INPUT}/robots-lamp/overrides/${VARIANT}/rootfs" "${V2_INPUT}/robots-lamp/rootfs"; do
    if [ -f "${root}${path}" ]; then src="${root}${path}"; break; fi
  done
  [ -n "$src" ] || err "shipped.json names ${path} for ${VARIANT}, but robots/lamp has no such file"
  sum=$(sha256sum "$src" | cut -c1-64)
  jq -e --arg v "$VARIANT" --arg p "$path" --arg s "$sum" '.variants[$v][$p] | index($s)' \
    "${V2_INPUT}/shipped.json" >/dev/null \
    || err "${path} (${sum}) is not a SHA-256 shipped.json knows: add it there first (#1369)"
  install -D -m "$(stat -c %a "$src")" "$src" "${MNT}${path}"
  echo "    ${path}"
done
if [ "$VARIANT" != standard ]; then
  mkdir -p "${MNT}/etc/autonomous"
  printf '%s\n' "$VARIANT" > "${MNT}/etc/autonomous/hardware-profile"
fi

# Phase 4: lampOS, from one signed build, by its own installer (docs/install.md, "The install command").
log "Phase 4 — lampOS (lamp-update install --setup on)"
install -D -m 0644 "${V2_INPUT}/key.pub" "${MNT}/etc/lampOS/update/key.pub"
rm -rf "${MNT}${CHROOT_CHANNEL}"
mkdir -p "${MNT}${CHROOT_CHANNEL}"
if [ -d "${V2_INPUT}/channel" ]; then
  cp -a "${V2_INPUT}/channel/." "${MNT}${CHROOT_CHANNEL}/"
  INSTALL_URL="file://${CHROOT_CHANNEL}/"
  log "Channel: the folder given (${INSTALL_URL})"
else
  INSTALL_URL="${LAMPOS_CHANNEL_URL}"
  log "Channel: ${INSTALL_URL}"
  curl -fsSL -o "${MNT}${CHROOT_CHANNEL}/lampos-manifest.json" "${INSTALL_URL%/}/lampos-manifest.json"
fi
# The manifest's own files_url and lamp-update's SHA-256; install checks the signature itself.
MANIFEST_JSON=$(jq -r '.manifest' "${MNT}${CHROOT_CHANNEL}/lampos-manifest.json" | base64 -d)
UPDATE_SHA=$(jq -r '.files[] | select(.name == "lamp-update") | .sha256' <<<"$MANIFEST_JSON")
[ -n "$UPDATE_SHA" ] || err "the build carries no lamp-update"
if [ ! -f "${MNT}${CHROOT_CHANNEL}/lamp-update" ]; then
  FILES_URL=$(jq -r '.files_url' <<<"$MANIFEST_JSON")
  curl -fsSL -o "${MNT}${CHROOT_CHANNEL}/lamp-update" "${FILES_URL%/}/lamp-update"
fi
echo "${UPDATE_SHA}  ${MNT}${CHROOT_CHANNEL}/lamp-update" | sha256sum -c - >/dev/null \
  || err "lamp-update is not the build's (SHA-256 differs from the manifest)"
chmod 0755 "${MNT}${CHROOT_CHANNEL}/lamp-update"

run_install() {
  chroot "${MNT}" "${CHROOT_CHANNEL}/lamp-update" install \
    --channel-url "${INSTALL_URL}" --channel "${LAMPOS_CHANNEL}" \
    --key-file /etc/lampOS/update/key.pub --setup on 2>&1 | tee "$1"
}
run_install /work/install-1.log

# The models install fetches where missing (deploy/product/models.json). A model it could not
# fetch (the models release is a draft, #811) is placed here from models/, by the same checks
# install makes: the file's size and SHA-256, or its archive's and then the member's. A second
# install then finds each one pinned and records it as its own.
if grep -q 'could not be had' /work/install-1.log; then
  [ -d "${V2_INPUT}/models" ] || err "install could not fetch every model: give LAMPOS_MODELS_DIR (gh release download models-2026-10-16 -R autonomous-ai/lampOS -D <dir>)"
  log "Placing the models install could not fetch, from models/"
  while IFS=$'\t' read -r path file bytes sha a_bytes a_sha member; do
    dest="${MNT}${path}"
    if [ -f "$dest" ] && [ "$(sha256sum "$dest" | cut -c1-64)" = "$sha" ]; then continue; fi
    src="${V2_INPUT}/models/${file}"
    [ -f "$src" ] || err "models/ has no ${file} (for ${path})"
    tmp="/work/model.placing"
    if [ -n "$member" ]; then
      [ "$(stat -c %s "$src")" = "$a_bytes" ] && [ "$(sha256sum "$src" | cut -c1-64)" = "$a_sha" ] \
        || err "${file} is not the pinned archive"
      tar -xzOf "$src" "$member" > "$tmp"
    else
      cp "$src" "$tmp"
    fi
    [ "$(stat -c %s "$tmp")" = "$bytes" ] && [ "$(sha256sum "$tmp" | cut -c1-64)" = "$sha" ] \
      || err "${path}: ${file} is not the pinned model"
    install -d -m 0755 "$(dirname "$dest")"
    install -m 0644 "$tmp" "$dest"
    rm -f "$tmp"
    echo "    ${path}"
  done < <(jq -r '.models[] | [.path, (.url | split("/") | last), .bytes, .sha256,
                              (.archive.bytes // ""), (.archive.sha256 // ""), (.archive.member // "")] | @tsv' \
             "${V2_INPUT}/models.json")
  run_install /work/install-2.log
  ! grep -q 'could not be had' /work/install-2.log || err "a model is still missing after placing models/"
fi
rm -rf "${MNT}${CHROOT_CHANNEL}"

# What install left, read back: the build it recorded, setup's settings, no marker, no V1 part.
STATE="${MNT}/var/lib/lampos-update/state.json"
[ -s "$STATE" ] || err "install wrote no state.json"
INSTALLED=$(jq -r '.installed | select(. != null) | "build \(.sequence) (\(.commit))"' "$STATE")
[ -n "$INSTALLED" ] || err "state.json records no installed build"
[ "$(jq -r '.systemd_owed // false' "$STATE")" = false ] || err "install still owes systemd its enable"
log "lampOS installed: ${INSTALLED}"
for f in /etc/lampOS/setup/serve.json /etc/lampOS/setup/reset-plan.json /etc/lampOS/update/key.pub \
         /opt/lampOS/setup/current/lamp-setup /opt/lampOS/update/current/lamp-update; do
  [ -e "${MNT}${f}" ] || err "missing after install: ${f}"
done
[ ! -e "${MNT}/var/lib/lampos-setup/done" ] || err "setup's marker is there: the lamp would skip setup"
[ ! -e "${MNT}/etc/lampOS/body/lamp.env" ] || err "a lamp.env is there: the body's poses are each lamp's own"
for unit in lamp-setup.service lampos-setup-done.path lamp-update.service; do
  [ -L "${MNT}/etc/systemd/system/multi-user.target.wants/${unit}" ] || err "${unit} is not enabled"
done
for unit in hal os-server bootstrap openclaw hermes-gateway led-boot led-shutdown; do
  for dir in etc/systemd/system lib/systemd/system usr/lib/systemd/system; do
    [ ! -e "${MNT}/${dir}/${unit}.service" ] || err "V1's ${unit}.service is on the image (${dir})"
  done
done
for path in /opt/hal /usr/local/bin/os-server /usr/local/bin/bootstrap-server /usr/local/bin/software-update; do
  [ ! -e "${MNT}${path}" ] || err "V1's ${path} is on the image"
done

# Phase 5: resize-once.service (as build-orangepi.sh).
log "Phase 5 — resize-once (first-boot expand)"
cat > "${MNT}/usr/local/bin/resize-once" <<'RESIZE_EOF'
#!/bin/bash
# Runs ONCE at first boot: expands the root partition and ext4 to fill the card, then removes itself.
set -uo pipefail
log() { echo "==> $*"; }
ROOT_PART=$(findmnt -n -o SOURCE /)
[ -z "${ROOT_PART}" ] && { echo "ERROR: cannot determine root partition"; exit 1; }
DEV_NAME=$(basename "${ROOT_PART}")
case "${DEV_NAME}" in
  mmcblk*p*|nvme*p*) PARENT="${DEV_NAME%p*}"; PART_NUM="${DEV_NAME##*p}" ;;
  sd?[0-9]*)         PARENT="${DEV_NAME%%[0-9]*}"; PART_NUM="${DEV_NAME##*[a-z]}" ;;
  *) echo "ERROR: unrecognised root device naming ${DEV_NAME}"; exit 1 ;;
esac
DISK="/dev/${PARENT}"
log "parent disk=${DISK} partition=${PART_NUM}"
growpart "${DISK}" "${PART_NUM}" || { log "growpart already at max — nothing to do"; }
resize2fs "${ROOT_PART}" || { log "WARN resize2fs failed"; }
log "resize complete"
systemctl disable resize-once.service 2>/dev/null || true
rm -f /etc/systemd/system/resize-once.service
rm -f /etc/systemd/system/multi-user.target.wants/resize-once.service
rm -f /usr/local/bin/resize-once
RESIZE_EOF
chmod +x "${MNT}/usr/local/bin/resize-once"

cat > "${MNT}/etc/systemd/system/resize-once.service" <<'UNIT'
[Unit]
Description=Expand root filesystem to fill SD card on first boot (self-destructing)
ConditionPathExists=/usr/local/bin/resize-once
DefaultDependencies=no
After=local-fs.target systemd-remount-fs.service
Before=basic.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/local/bin/resize-once

[Install]
WantedBy=multi-user.target
UNIT
mkdir -p "${MNT}/etc/systemd/system/multi-user.target.wants"
ln -sf /etc/systemd/system/resize-once.service \
  "${MNT}/etc/systemd/system/multi-user.target.wants/resize-once.service"

# Phase 6: restore resolv.conf, unmount, compress.
log "Phase 6 — finalize"
mv -f "${MNT}/etc/resolv.conf.bak" "${MNT}/etc/resolv.conf" 2>/dev/null || true

# No battery RTC: seed fake-hwclock with the build time so TLS works before NTP syncs.
date -u '+%Y-%m-%d %H:%M:%S' > "${MNT}/etc/fake-hwclock.data"

for pid in $(lsof -t +D "${MNT}" 2>/dev/null || true); do
  kill -9 "$pid" 2>/dev/null || true
done
fuser -k -M "${MNT}" 2>/dev/null || true
rm -f "${MNT}/run/sshd.pid" "${MNT}/run/dbus/pid" 2>/dev/null || true
rm -rf "${MNT}/run/lock"/* 2>/dev/null || true
rm -f "${MNT}/usr/bin/qemu-aarch64-static"

umount "${MNT}/dev"
umount "${MNT}/sys"
umount "${MNT}/proc"

sync
umount "${MNT}"
log "Validating final ext4 filesystem before publishing image…"
check_ext4 "${PART}"
losetup -d "${PART_LOOP}"; PART_LOOP=""
losetup -d "${LOOP_DEV}"; LOOP_DEV=""
mv -f "${OUT_IMG}" "${FINAL_IMG}"
OUT_IMG="${FINAL_IMG}"

if [ "${COMPRESS:-1}" = "0" ]; then
  log "DONE: ${OUT_IMG} (COMPRESS=0, skipped .xz)"
  log "Flash:  make sd-card-flash-raw DEVICE_TYPE=lamp-v2 DISK=N"
  exit 0
fi
log "Compressing ${OUT_IMG} → ${OUT_IMG}.xz (this takes a few minutes)…"
xz -9 -c --threads=0 "${OUT_IMG}" > "${OUT_IMG}.xz.building"
mv -f "${OUT_IMG}.xz.building" "${OUT_IMG}.xz"
log "DONE: ${OUT_IMG}.xz ($(du -h "${OUT_IMG}.xz" | cut -f1))"
