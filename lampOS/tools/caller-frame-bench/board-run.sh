#!/bin/sh
# Voluntary local benchmark. It reads only the synthetic WAVs shipped in this kit.
set -eu
umask 077
kit=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
case "$(uname -s)/$(uname -m)" in
    Linux/aarch64|Linux/arm64) ;;
    *) echo 'This kit requires Linux ARM64.' >&2; exit 1 ;;
esac
output=${1:-"$PWD/sensor6-board-timing.tsv"}
if [ "$#" -gt 1 ]; then
    echo 'usage: board-run.sh [NEW_OUTPUT.tsv]' >&2
    exit 2
fi
(cd "$kit" && sha256sum -c SHA256SUMS >/dev/null)
"$kit/bin/board-timer" "$kit" "$output"
printf '%s\n' "$output"
