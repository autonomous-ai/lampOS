#!/usr/bin/env bash
set -euo pipefail
HERE=$(cd -- "$(dirname -- "$0")" && pwd)
exec "$HERE/run.sh" score speaking_face "$@"
