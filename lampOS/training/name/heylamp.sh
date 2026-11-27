#!/bin/sh
# The "Hey Lamp" retrain (#8907) end to end: Backend's tar in; a name model trained with only
# "Hey Lamp" (a greeting, then "Lamp") as the call, and the tables that put it beside today's
# models on the same held-out sound, out. heylamp.py says what each step does.
#
#     sh heylamp.sh TAR|URL [SHA256] [SET]
#
# TAR: Backend's tar, or its CDN URL (then it is fetched into ~/lamp-voice-data/heylamp-8907,
# kept there for good, if the disk keeps 10 GiB free after it). SHA256: as Backend posted it
# (else TAR.sha256 beside it). SET: a name for this set's caches (default heylamp-1).
# Before it, once: sh env.sh (the environment) and sh heylamp-data.sh (the public sound).
#
# Every step runs at the lowest priority with at most six threads: three processes of two, or
# one of three to train. Knobs, from the environment:
#   HEYLAMP_EVERY=N          evaluate every Nth held-out item (1: all)
#   HEYLAMP_AMI=N            the first N of October's 34 AMI meetings (34)
#   HEYLAMP_AMI_MINUTES=M    only the first M minutes of each (0: all)
#   HEYLAMP_LIBRI_EVERY=N    every Nth of the 73 LibriSpeech test readers (1: all, 10.7 h)
#   HEYLAMP_STEPS=N          training steps (12000, as October)
#   HEYLAMP_TALK_SCALE=F     long talk to train against: 10.5 h times F (1)
#   HEYLAMP_CALLS=N          examples made of each training call (2)
#   HEYLAMP_HARD=N           examples made of each not-call other than plain talk (2)
#   HEYLAMP_CHANNEL=FILE     a measured channel.npz (channel.py analyse) to put voices through
#   HEYLAMP_NOTCALL_SHARE=F  the share of not-calls aligned (1); the rest are placed anywhere
#   HEYLAMP_TRUTH=FILE       known name ends (the dry run's), to check the alignment against
#   HEYLAMP_TRAIN_ARGS=...   more options for training/stop/train.py (e.g. "--hard 1")
#   HEYLAMP_FROM=STEP        start at a step (public, unpack, align, inputs, train, evaluate, report)
set -e
here=$(cd "$(dirname "$0")" && pwd)
cd "$here"
src=$1
sha=${2:-}
set=${3:-heylamp-1}
[ -n "$src" ] || { echo "usage: sh heylamp.sh TAR|URL [SHA256] [SET]" >&2; exit 2; }
data="$HOME/lamp-voice-data/heylamp-8907"
sets="$here/../../.cache/heylamp/sets/$set"
mkdir -p "$sets/runs" "$data"
log() { echo "$(date '+%Y-%m-%d %H:%M:%S') $*"; }
free_mib() { df -k "$1" | awk 'NR == 2 { print int($4 / 1024) }'; }

# Runs "$@" as parts 0/3, 1/3 and 2/3, two threads each; stops if any part fails.
three() {
  base=$1
  shift
  pids=""
  for p in 0 1 2; do
    OMP_NUM_THREADS=2 sh run.sh "$@" --part "$p/3" > "$base-$p.log" 2>&1 &
    pids="$pids $!"
  done
  failed=0
  for pid in $pids; do wait "$pid" || failed=1; done
  tail -n 2 "$base"-[012].log
  if [ "$failed" != 0 ]; then
    echo "heylamp: a part of '$*' failed; see $base-*.log" >&2
    exit 1
  fi
}

order="public unpack align inputs train evaluate report"
from=${HEYLAMP_FROM:-public}
case " $order " in *" $from "*) ;; *) echo "HEYLAMP_FROM: one of $order" >&2; exit 2 ;; esac
started=""
want() { [ -n "$started" ] || [ "$1" = "$from" ] && started=1; [ -n "$started" ]; }

case "$src" in
http://* | https://*)
  tar="$data/$(basename "$src")"
  if [ ! -s "$tar" ]; then
    size=$(curl -sSIL "$src" | awk 'tolower($1) == "content-length:" { n = $2 } END { print int(n / 1048576) }')
    if [ "$(free_mib "$data")" -lt $((10240 + size)) ]; then
      echo "heylamp: $(free_mib "$data") MiB free; the tar ($size MiB) would leave under 10 GiB" >&2
      exit 3
    fi
    log "fetching $src ($size MiB) into $data"
    nice -n 19 curl -sSL --fail --retry 5 -C - -o "$tar.part" "$src"
    mv "$tar.part" "$tar"
  fi
  [ -n "$sha" ] && echo "$sha  $(basename "$tar")" > "$tar.sha256"
  ;;
*) tar=$src ;;
esac
installed="$HOME/lamp-voice-data/name-models/name.onnx"

if want public; then
  log "1/7 the public sets (Speech Commands' noise and words; LibriSpeech is read from its parquet files)"
  sh run.sh heylamp.py public
fi
if want unpack; then
  log "2/7 unpack $tar"
  sh run.sh heylamp.py unpack "$tar" ${sha:+--sha256 "$sha"} --set "$set"
fi
if want align; then
  log "3/7 align: where the name ends in each call (and the word a not-call could be taken for)"
  three "$sets/align" heylamp.py align "$set" --notcall-share "${HEYLAMP_NOTCALL_SHARE:-1}"
  sh run.sh heylamp.py align "$set" --merge ${HEYLAMP_TRUTH:+--truth "$HEYLAMP_TRUTH"}
fi
if want inputs; then
  for what in words talk check check-talk; do
    log "4/7 inputs: $what"
    three "$sets/inputs-$what" heylamp.py inputs "$set" "$what" --talk-scale "${HEYLAMP_TALK_SCALE:-1}" \
      --calls "${HEYLAMP_CALLS:-2}" --hard "${HEYLAMP_HARD:-2}" ${HEYLAMP_CHANNEL:+--channel "$HEYLAMP_CHANNEL"}
  done
  du -sh "$sets/inputs"
fi
if want train; then
  log "5/7 train (training/stop/train.py, October's recipe; every call input kept)"
  # shellcheck disable=SC2086
  OMP_NUM_THREADS=3 sh run.sh ../stop/train.py --inputs "$sets/inputs" --shape time --hidden 64 \
    --steps "${HEYLAMP_STEPS:-12000}" --heard -99 --lowest -99 --against 3 --hard 0 $HEYLAMP_TRAIN_ARGS \
    --out "$sets/runs/heylamp.onnx" > "$sets/train.log" 2>&1
  tail -n 4 "$sets/train.log"
fi
if want evaluate; then
  log "6/7 evaluate the new model and today's on the same held-out sound"
  models="$sets/runs/heylamp.onnx ../../models/name/name.onnx"
  names="new,repo-f376e964"
  if [ -s "$installed" ]; then
    models="$models $installed"
    names="$names,installed-1d238ca7"
  fi
  # shellcheck disable=SC2086
  three "$sets/eval" heylamp.py evaluate "$set" $models --names "$names" --every "${HEYLAMP_EVERY:-1}" \
    --ami-meetings "${HEYLAMP_AMI:-34}" --ami-minutes "${HEYLAMP_AMI_MINUTES:-0}" \
    --libri-every "${HEYLAMP_LIBRI_EVERY:-1}" ${HEYLAMP_CHANNEL:+--channel "$HEYLAMP_CHANNEL"}
fi
if want report; then
  log "7/7 report"
  sh run.sh heylamp.py report "$set" --rule 80 > "$sets/runs/report-80.log"
  sh run.sh heylamp.py report "$set" --rule 10w
  # Kept for good beside the set: the model and every score (a dry run's in dry-run/).
  case "$set" in dry*) keep="$data/dry-run/$set" ;; *) keep="$data" ;; esac
  mkdir -p "$keep/models" "$keep/scores/$set"
  cp "$sets/runs/heylamp.onnx" "$keep/models/$set-heylamp.onnx"
  cp "$sets/runs/"report-*.md "$sets/runs/"eval-part-*.json "$sets/unpack.json" "$sets/train.log" "$keep/scores/$set/"
  [ -f "$sets/align.json" ] && cp "$sets/align.json" "$keep/scores/$set/"
  shasum -a 256 "$keep/models/$set-heylamp.onnx"
  log "done: $keep/models/$set-heylamp.onnx, $keep/scores/$set/report-10w.md"
fi
