#!/usr/bin/env bash
# Offline only: this tool reads annotations/predictions and never opens a device.
set -euo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd)
cd "$HERE"
exec cargo run --offline --locked --quiet -- "$@"
