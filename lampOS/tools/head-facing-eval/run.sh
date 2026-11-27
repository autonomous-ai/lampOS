#!/usr/bin/env bash
# Offline public-image replay only. This never opens a camera or changes a unit.
set -euo pipefail
umask 077

if [[ $# -lt 6 || $# -gt 7 ]]; then
  printf '%s\n' 'usage: run.sh BINARY DETECTOR MESH PUBLIC_IMAGE_ROOT IMAGE_LIST.json NEW_OUTPUT_DIRECTORY [working|full]' >&2
  exit 2
fi
binary=$1
detector=$2
mesh=$3
image_root=$4
image_list=$5
output=$6
crop=${7:-working}
case "$crop" in working|full) ;; *) printf '%s\n' 'invalid crop mode' >&2; exit 2 ;; esac
[[ -x "$binary" && -f "$detector" && -f "$mesh" && -d "$image_root" && -f "$image_list" ]] || {
  printf '%s\n' 'missing replay input' >&2
  exit 2
}
# A bare filename must execute the file just hashed, not a namesake on PATH.
binary="$(cd -- "$(dirname -- "$binary")" && pwd -P)/$(basename -- "$binary")"
output="$(cd -- "$(dirname -- "$output")" && pwd -P)/$(basename -- "$output")"
command -v timeout >/dev/null || { printf '%s\n' 'GNU timeout is required' >&2; exit 2; }
if command -v sha256sum >/dev/null; then
  digest() { sha256sum -- "$1" | cut -d ' ' -f 1; }
elif command -v shasum >/dev/null; then
  digest() { shasum -a 256 -- "$1" | cut -d ' ' -f 1; }
else
  printf '%s\n' 'SHA-256 tool is required' >&2
  exit 2
fi
binary_hash=$(digest "$binary")
detector_hash=$(digest "$detector")
mesh_hash=$(digest "$mesh")
list_hash=$(digest "$image_list")
[[ "$detector_hash" == 8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4 &&
   "$mesh_hash" == 09d24504cdc51240b6ccd8c457040c7cb61ab3485e0e08696dd44ce52ed8131a ]] || {
  printf '%s\n' 'model hash mismatch' >&2
  exit 2
}
# mkdir refuses an existing result directory; no prior evidence is overwritten.
mkdir -- "$output"
chmod 700 "$output"
architecture=$(uname -m)
case "$architecture" in aarch64|arm64|x86_64) ;; *) architecture=unknown ;; esac
started=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
replay_argv=(nice -n 15 "$binary" "$detector" "$mesh" "$image_root" "$image_list")
# The previous reviewed producer has no --crop option; its default is working.
if [[ "$crop" == full ]]; then
  replay_argv+=(--crop full)
fi
if timeout --signal=TERM --kill-after=5s 900s "${replay_argv[@]}" \
  >"$output/results.jsonl" 2>"$output/stderr.txt"; then
  result=0
else
  result=$?
fi
finished=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
results_hash=$(digest "$output/results.jsonl")
stderr_hash=$(digest "$output/stderr.txt")
printf '{"schema":"lamp-head-replay-run/1","architecture":"%s","crop":"%s","started_at":"%s","finished_at":"%s","clock_trust":"unknown","exit_code":%s,"nice":15,"timeout_seconds":900,"binary_sha256":"%s","detector_sha256":"%s","mesh_sha256":"%s","list_sha256":"%s","results_sha256":"%s","stderr_sha256":"%s","timing_boundary":"file_read_start_to_result","source_capture_age_us":null}\n' \
  "$architecture" "$crop" "$started" "$finished" "$result" \
  "$binary_hash" "$detector_hash" "$mesh_hash" "$list_hash" "$results_hash" "$stderr_hash" \
  >"$output/run.json"
printf 'replay exit_code=%s; all output retained\n' "$result"
exit "$result"
