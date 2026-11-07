#!/usr/bin/env bash
# Tests for level.py and night.py's play path with no lamp and no sound: made-up WAVs, python3 only. The meter is
# pinned to BS.1770-4's own figure (a full-scale 997 Hz sine reads -3.01 LUFS); ffmpeg's ebur128 checks it too when
# ffmpeg is on the PATH, and is skipped otherwise. night.py is imported with its afplay call stood in for.
# Run: lampOS/tools/played/level-test.sh
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
failures=0
check() {
  if eval "$2"; then
    echo "ok   $1"
  else
    echo "FAIL $1"
    failures=$((failures + 1))
  fi
}

# make NAME RATE: a mono s16 WAV at RATE from the Python expression on stdin, `x(n)` a sample in -1..1 and `N` the
# length in samples.
make() {
  python3 -I -c "
import math, struct, sys, wave
rate = int(sys.argv[2]); ns = {'math': math, 'rate': rate}; exec(sys.stdin.read(), ns)
w = wave.open(sys.argv[1], 'wb'); w.setnchannels(1); w.setsampwidth(2); w.setframerate(rate)
w.writeframes(b''.join(struct.pack('<h', max(-32768, min(32767, round(32767 * ns['x'](n))))) for n in range(ns['N'])))
" "$1" "$2"
}
# py CODE ARGS...: CODE with level.py importable.
py() { local code=$1; shift; PYTHONPATH="$HERE" python3 -c "$code" "$@"; }

mkdir -p "$T/src" "$T/sine"
make "$T/sine/sine.wav" 48000 <<'PY'
N = 5 * rate
def x(n): return math.sin(2 * math.pi * 997 * n / rate)
PY
# Two 3 s 1 kHz tones, 20 LU apart, each after 50 ms of zeros (24 kHz, the kits' rate).
for db in 34 14; do
  make "$T/src/tone$db.wav" 24000 <<PY
lead = rate // 20; N = lead + 3 * rate; a = 10 ** (-$db / 20)
def x(n): return 0.0 if n < lead else a * math.sin(2 * math.pi * 1000 * (n - lead) / rate)
PY
done
# A click train: one half-scale sample every 100 ms, so its peak is far above its loudness (16 kHz).
make "$T/src/clicks.wav" 16000 <<'PY'
N = 3 * rate
def x(n): return 0.5 if n % (rate // 10) == 0 else 0.0
PY

lufs() { py 'import sys, level; print(level.measure(sys.argv[1])["lufs"])' "$1"; }
near() { python3 -c 'import sys; sys.exit(0 if abs(float(sys.argv[1]) - float(sys.argv[2])) <= float(sys.argv[3]) else 1)' "$@"; }

got=$(lufs "$T/sine/sine.wav" 2>/dev/null)
check "a full-scale 997 Hz sine reads -3.01 LUFS +-0.1 (BS.1770-4) [got ${got:-nothing}]" 'near "${got:-x}" -3.01 0.1 2>/dev/null'

python3 "$HERE/level.py" "$T/src" "$T/out" >"$T/cli.out" 2>&1
check "level.py DIR OUT levels every WAV and writes OUT/level.json" '[ -s "$T/out/level.json" ] && [ -s "$T/out/tone34.wav" ] && [ -s "$T/out/tone14.wav" ] && [ -s "$T/out/clicks.wav" ]'
field() { py 'import json, sys; print(json.load(open(sys.argv[1]))["clips"][sys.argv[2]][sys.argv[3]])' "$T/out/level.json" "$1" "$2" 2>/dev/null; }
for db in 34 14; do
  out=$(lufs "$T/out/tone$db.wav" 2>/dev/null)
  check "the -$db dBFS tone comes out at -20 +-0.5 LUFS [got ${out:-nothing}]" 'near "${out:-x}" -20 0.5 2>/dev/null'
  check "the -$db dBFS tone's manifest says lufs_out -20 +-0.5 and not capped" 'near "$(field tone$db.wav lufs_out)" -20 0.5 2>/dev/null && [ "$(field tone$db.wav capped)" = False ]'
  check "the -$db dBFS tone keeps its length and its 50 ms of zeros" 'py "
import sys, wave
a, b = wave.open(sys.argv[1]), wave.open(sys.argv[2])
assert a.getparams() == b.getparams(), (a.getparams(), b.getparams())
lead = b.readframes(1200); assert lead == bytes(2400), \"lead not zeros\"
assert b.readframes(2)[2:] != bytes(2), \"the tone does not begin after the lead\"
" "$T/src/tone$db.wav" "$T/out/tone$db.wav" 2>/dev/null'
done
check "one gain a clip: the gains are -3 and +17 dB, 20 dB apart" \
  'near "$(field tone34.wav gain_db)" 17.0 0.2 2>/dev/null && near "$(field tone14.wav gain_db)" -3.0 0.2 2>/dev/null'
tp=$(py 'import sys, level; print(level.measure(sys.argv[1])["tp"])' "$T/out/clicks.wav" 2>/dev/null)
check "the click train is held at the ceiling: TP <= -1.0 dBTP after rounding [got ${tp:-nothing}]" 'python3 -c "import sys; sys.exit(0 if float(sys.argv[1]) <= -1.0 else 1)" "${tp:-x}" 2>/dev/null'
check "the click train is capped, below -20, and its manifest's tp_out is its own" \
  '[ "$(field clicks.wav capped)" = True ] && python3 -c "import sys; sys.exit(0 if float(sys.argv[1]) < -20.5 else 1)" "$(field clicks.wav lufs_out)" && near "$(field clicks.wav tp_out)" "${tp:-x}" 0.01'
check "source_sha256 is shasum -a 256 of the source" '[ -n "$(field tone34.wav source_sha256)" ] && [ "$(field tone34.wav source_sha256)" = "$(shasum -a 256 "$T/src/tone34.wav" | cut -d" " -f1)" ]'
check "played_sha256 is shasum -a 256 of the levelled file" '[ -s "$T/out/tone34.wav" ] && [ "$(field tone34.wav played_sha256)" = "$(shasum -a 256 "$T/out/tone34.wav" | cut -d" " -f1)" ]'
python3 "$HERE/level.py" "$T/src" "$T/again" >/dev/null 2>&1
check "the same source levels to the same played_sha256 twice" \
  '[ -n "$(field tone34.wav played_sha256)" ] && [ "$(shasum -a 256 "$T/again/tone34.wav" | cut -d" " -f1)" = "$(field tone34.wav played_sha256)" ] && cmp -s "$T/out/clicks.wav" "$T/again/clicks.wav"'
python3 "$HERE/level.py" "$T/src" "$T/m23" --target -23 >/dev/null 2>&1
out=$(lufs "$T/m23/tone34.wav" 2>/dev/null)
check "--target -23 levels to -23 [got ${out:-nothing}]" 'near "${out:-x}" -23 0.5 2>/dev/null'

# ffmpeg's ebur128 on the same files, when there is one.
if command -v ffmpeg >/dev/null 2>&1; then
  for f in "$T/sine/sine.wav" "$T/out/tone34.wav" "$T/out/tone14.wav"; do
    ff=$(ffmpeg -hide_banner -nostats -i "$f" -filter_complex ebur128 -f null - 2>&1 | awk '/Integrated loudness:/ {s=1} s && /I:/ {print $2; exit}')
    mine=$(lufs "$f" 2>/dev/null)
    check "ffmpeg ebur128 agrees within 0.2 LU on $(basename "$f") [ffmpeg ${ff:-nothing}, level.py ${mine:-nothing}]" 'near "${ff:-x}" "${mine:-y}" 0.2 2>/dev/null'
  done
else
  echo "skip ffmpeg ebur128 agreement: no ffmpeg on the PATH"
fi

# night.py's play path, its afplay stood in for: the levelled copy by default, the raw WAV with NIGHT_LEVEL=off.
mkdir -p "$T/kit"
cp "$T/src/tone34.wav" "$T/kit/a02.wav"
played() { # played LEVEL: one line, the path night.py hands afplay for a02, its row's three fields and the summary's level
  env PLAYED_OUT="$T/night" NIGHT_WAV="$T/kit" NIGHT_LEVEL="$1" PYTHONPATH="$HERE" python3 -c '
import night
said = []
night.subprocess.run = lambda argv, **kw: said.append(argv)
night.prepare(["a02"]); night.play("a02")
c = night.CLIPS["a02"]
print(said[0][-1], c["source_sha256"], c["played_sha256"], c["gain_db"], night.level_summary())
' 2>/dev/null
}
raw_sha=$(shasum -a 256 "$T/kit/a02.wav" | cut -d" " -f1)
cached="$T/night/desk-kit-2/level-m20/a02.wav"
# (checks are eval'd inside check(), where $1 is check's own: so the fields go into names)
read -r path src_sha played_sha gain lvl _ <<<"$(played -20) - - - - -"
check "night.py plays the levelled copy under the kit's out dir [$path]" '[ "$path" = "$cached" ]'
check "night.py's row: the source's sha, the played copy's sha, +17 dB; the summary's level -20" \
  '[ "$src_sha" = "$raw_sha" ] && [ "$played_sha" = "$(shasum -a 256 "$cached" | cut -d" " -f1)" ] && near "$gain" 17 0.2 2>/dev/null && [ "$lvl" = -20 ]'
check "the cache lists it in level.json" '[ -s "$T/night/desk-kit-2/level-m20/level.json" ]'
read -r path src_sha played_sha gain lvl _ <<<"$(played off) - - - - -"
check "NIGHT_LEVEL=off plays the raw WAV [$path]" '[ "$path" = "$T/kit/a02.wav" ]'
check "NIGHT_LEVEL=off's row: the raw sha twice, 0 dB; the summary's level off" \
  '[ "$src_sha" = "$raw_sha" ] && [ "$played_sha" = "$raw_sha" ] && [ "$gain" = 0.0 ] && [ "$lvl" = off ]'

echo "level-test: $failures failed"
[ "$failures" -eq 0 ]
