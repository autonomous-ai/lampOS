#!/usr/bin/env bash
# Publish one complete V2 service package; activation remains a separate promotion.
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/ota-config.sh"

source "${RELEASE_DIR}/ota-metadata.sh"

for tool in git docker curl python3 tar; do
  command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }
done
build_only="${LAMP_OS_BUILD_ONLY:-0}"
[[ "$build_only" = 0 || "$build_only" = 1 ]] || { echo 'LAMP_OS_BUILD_ONLY must be 0 or 1' >&2; exit 1; }
if [ "$build_only" = 0 ]; then
  for tool in gsutil jq openssl; do
    command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }
  done
  ota_metadata_signing_preflight
fi
python3 -c 'import sys; sys.exit("Python 3.10+ is required" if sys.version_info < (3, 10) else 0)'
docker info >/dev/null
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
remote=$(git -C "$ROOT_DIR" remote get-url origin)
git init --bare -q "$work/source.git"
git -C "$work/source.git" fetch --depth=1 "$remote" refs/heads/lamp-voice
commit=$(git -C "$work/source.git" rev-parse --verify 'FETCH_HEAD^{commit}')
mkdir -p "$work/source" "$work/build" "$work/assets"
git -C "$work/source.git" archive "$commit" lampOS | tar -xf - -C "$work/source"
source_dir="$work/source/lampOS"
[ -f "$source_dir/Cargo.lock" ] || { echo 'lampOS/Cargo.lock is missing' >&2; exit 1; }
rust_version=$(python3 - "$source_dir/rust-toolchain.toml" <<'PY'
from pathlib import Path
import re, sys
match = re.search(r'^channel\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"', Path(sys.argv[1]).read_text(), re.M)
if not match:
    sys.exit('lampOS must pin a numeric Rust toolchain')
print(match[1])
PY
)
version_file="${ROOT_DIR}/system/${VERSION_FILE:-VERSION_LAMP_OS}"
version=$(python3 - "$version_file" <<'PY'
from pathlib import Path
import re, sys
previous = Path(sys.argv[1]).read_text().strip()
if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', previous):
    sys.exit('VERSION_LAMP_OS must be major.minor.patch')
major, minor, patch = map(int, previous.split('.'))
print(f'{major}.{minor}.{patch + 1}')
PY
)
image="lamp-os-release-builder:${rust_version}"
echo "Building complete lamp-os $version from lamp-voice @ $commit"
docker build --platform linux/arm64 --build-arg "RUST_VERSION=$rust_version" \
  -t "$image" -f "$RELEASE_DIR/lamp-os-builder.Dockerfile" "$RELEASE_DIR"
python3 "$RELEASE_DIR/fetch_lamp_os_assets.py" "$work/assets"
docker run --rm --platform linux/arm64 \
  -v "$source_dir:/source:ro" -v "$RELEASE_DIR/lamp-os-adapter:/adapter:ro" \
  -v "$work/assets:/assets:ro" -e CARGO_BUILD_JOBS=4 \
  -v "$RELEASE_DIR/lamp-os-build-container.sh:/build.sh:ro" \
  -v "$work/build:/out" "$image" bash /build.sh
output="${LAMP_OS_OUTPUT:-${ROOT_DIR}/lamp-os-${version}.zip}"
python3 "$RELEASE_DIR/lamp_os_package.py" --source "$source_dir" \
  --source-commit "$commit" --binaries "$work/build/binaries" --libraries "$work/build/lib" \
  --assets "$work/assets" --adapter "$work/build/service" --version "$version" --output "$output"
if [ "$build_only" = 1 ]; then
  echo "Built complete package: $output (version file unchanged; no publication)"
  exit 0
fi
# Retain the bumped version even if publication fails, like other release targets.
printf '%s\n' "$version" > "$version_file"
COMPONENT=lamp-os V="$version" PACKAGE="$output" REPLACES='hal os-server' \
  bash "$RELEASE_DIR/upload-service.sh"
echo "Complete lamp-os $version published; activation was not promoted."
