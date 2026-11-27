#!/bin/sh
# The public sound the "Hey Lamp" retrain (#8907, heylamp.py) needs beside
# Backend's rendered set: only what training negatives and measuring false
# wakes an hour take, from October's own sources (training/stop/make.sh,
# training/name/make.sh), each file checked against October's SHA-256
# (training/stop/downloads.sha256) or, for the Piper voices, which October did
# not record, against heylamp-downloads.sha256 (recorded on 27 Nov 2026).
#
#     sh heylamp-data.sh [front piper libri sc ami]
#
#   front  openWakeWord v0.5.1 melspectrogram.onnx, embedding_model.onnx (2.4 MB)
#   piper  en_US-libritts-high (the dry run's train voices; 128 MB), en_GB-cori-medium
#          and en_US-ljspeech-medium (the dry run's held-out voices; 2 x 63 MB)
#   libri  LibriSpeech dev-clean, dev-other (talk to train against, 10.5 h) and
#          test-clean, test-other (held-out talk, 10.7 h): 1.3 GB, as the parquet files
#          of Hugging Face's openslr/librispeech_asr (not gated; the same recordings, the
#          copy October took train-clean-100 from): openslr.org's own tarballs came at
#          about 100 KB/s on 27 Nov and its mirrors did not answer
#   sc     Speech Commands v0.02, read as it downloads (2.3 GB through, nothing of
#          it kept but its six noise recordings, its two split lists and a few
#          clips of each word: about 0.3 GB), SHA-256 checked on the stream
#   ami    AMI, the 34 meetings October measured on (Mix-Headset, 24.1 h): 2.8 GB,
#          held-out office talk, never trained on. Not fetched unless named: without
#          them on disk, heylamp.py evaluate reads each meeting from the AMI site into
#          memory (about 2.7 MB/s here), checks its SHA-256 and keeps nothing
#
# Not fetched (October's, not needed for a "Hey Lamp" only model or too large
# for this disk): LibriSpeech train-clean-100 (6.3 GB), MSWC shards (its real
# "lamp" and near words; 26 shards and an aligner pass), FLEURS Vietnamese,
# EdAcc, the AMI transcripts. Every step runs at the lowest priority, and none
# starts with less than 10 GiB free after it.
set -e
here=$(cd "$(dirname "$0")" && pwd)
stop="$here/../../.cache/stop"
name="$here/../../.cache/name"
d="$stop/downloads"
mkdir -p "$d/ami" "$stop/oww" "$stop/piper" "$name/piper"
what=${*:-front piper libri sc}

# Refuses to go on unless the disk keeps 10 GiB free after $1 more MiB.
room() {
  free=$(df -k "$stop" | awk 'NR == 2 { print int($4 / 1024) }')
  if [ "$free" -lt $((10240 + $1)) ]; then
    echo "heylamp-data: $free MiB free, $1 MiB more would leave under 10 GiB; stopped" >&2
    exit 3
  fi
}
get() { [ -s "$1" ] || nice -n 19 curl -sSL --fail --retry 5 -C - -o "$1" "$2"; }
# Checks the files of one list that are named in $2 (a grep pattern).
check() { (cd "$3" && grep -E "$2" "$1" | shasum -a 256 -c -); }

for step in $what; do
  case "$step" in
  front)
    for f in melspectrogram.onnx embedding_model.onnx; do
      get "$stop/oww/$f" "https://github.com/dscripka/openWakeWord/releases/download/v0.5.1/$f"
    done
    check "$here/heylamp-downloads.sha256" ' oww/' "$stop" ;;
  piper)
    room 300
    for f in en_US-libritts-high.onnx en_US-libritts-high.onnx.json MODEL_CARD; do
      get "$stop/piper/$f" "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/libritts/high/$f"
    done
    v=https://huggingface.co/rhasspy/piper-voices/resolve/main/en
    for x in en_GB/cori/medium/en_GB-cori-medium en_US/ljspeech/medium/en_US-ljspeech-medium; do
      get "$name/piper/$(basename "$x").onnx" "$v/$x.onnx"
      get "$name/piper/$(basename "$x").onnx.json" "$v/$x.onnx.json"
    done
    check "$here/heylamp-downloads.sha256" ' piper/(en_US-libritts|MODEL_CARD)' "$stop"
    check "$here/heylamp-downloads.sha256" ' piper/en_(GB-cori|US-ljspeech)' "$name" ;;
  libri)
    room 1400
    mkdir -p "$d/librispeech-parquet"
    for s in validation.clean validation.other test.clean test.other; do
      get "$d/librispeech-parquet/$s.parquet" \
        "https://huggingface.co/datasets/openslr/librispeech_asr/resolve/main/all/$s/0000.parquet"
    done
    check "$here/heylamp-downloads.sha256" ' librispeech-parquet/' "$d" ;;
  sc)
    room 2500
    sh "$here/run.sh" heylamp.py fetch-sc ;;
  ami)
    room 2900
    for meeting in ES2004a ES2004b ES2004c ES2004d IS1009a IS1009b IS1009c TS3003a TS3003b TS3003c \
      EN2001d EN2005a EN2006b EN2009d ES2005b ES2005c ES2005d ES2007c ES2008d ES2009c ES2012c ES2014d \
      ES2016a ES2016b IS1007c TS3004c TS3012c ES2010b IN1016 IN1014 IS1003d TS3008b TS3010d EN2001a; do
      get "$d/ami/$meeting.Mix-Headset.wav" "https://groups.inf.ed.ac.uk/ami/AMICorpusMirror/amicorpus/$meeting/audio/$meeting.Mix-Headset.wav"
    done
    check "$here/../stop/downloads.sha256" ' ami/.*\.wav$' "$d" ;;
  *) echo "heylamp-data: no step $step" >&2; exit 2 ;;
  esac
done
