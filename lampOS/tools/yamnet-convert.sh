#!/usr/bin/env bash
# Our own conversion of Google's YAMNet to the yamnet.onnx and yamnet.data
# lamp-ears loads (#6232; sound-events.md, "Where the weights come from"), so
# the files we release are reproducible from the licensed source. Every input
# is pinned here; run on a Mac or Linux host with Python 3.11, git and curl:
#
#   lampOS/tools/yamnet-convert.sh WORKDIR
#
# WORKDIR must not exist. It gets dl/yamnet.h5 (checked against H5_SHA256),
# the two source trees at their pinned commits, a venv with the pinned
# packages, and out/ with yamnet.onnx, yamnet.data, parity.txt (our ONNX
# against Google's Keras model on synthesized clips, tools/yamnet-convert.py),
# pip-freeze.txt and SHA256SUMS. It exits non-zero on any mismatch.
set -euo pipefail

H5_URL=https://storage.googleapis.com/audioset/yamnet.h5
# Downloaded twice, at 14:00 and 16:25 on 11 Nov 2026 (+07), into separate
# folders: the same 15,296,092 bytes both times (#6232).
H5_SHA256=13c3308955bbfaef262f175ac9c40e47b134573a93984f009220dd7cc12a1744
# tensorflow/models, the last commit touching research/audioset/yamnet: Google's
# Keras model and frontend, the reference the conversion is checked against.
TFM_URL=https://github.com/tensorflow/models.git
TFM_COMMIT=c14bf9ad91962cf189f9f58db2132c06247fcd53
# w-hc/torch_audioset (MIT): its YAMNet in PyTorch and tf_2_torch/convert_yamnet.py's
# weight mapping, the conversion #6119 names.
TA_URL=https://github.com/w-hc/torch_audioset.git
TA_COMMIT=daed697ddcd1d2f7304cc3c2b1b4295ac7e304e4
PYTHON=${PYTHON:-python3.11}
PACKAGES=(tensorflow==2.18.0 tf_keras==2.18.0 torch==2.5.1 onnx==1.17.0
  onnxruntime==1.20.1 h5py==3.12.1 numpy==1.26.4 pyyaml==6.0.2)

[ $# -eq 1 ] || { echo "usage: $0 WORKDIR" >&2; exit 2; }
here=$(cd "$(dirname "$0")" && pwd)
work=$1
mkdir "$work"
work=$(cd "$work" && pwd)
set -x

mkdir "$work/dl"
curl -fsSL --proto '=https' -o "$work/dl/yamnet.h5" "$H5_URL"
echo "$H5_SHA256  $work/dl/yamnet.h5" | shasum -a 256 -c -

git clone -q --filter=blob:none --sparse "$TFM_URL" "$work/tfm"
git -C "$work/tfm" sparse-checkout set research/audioset/yamnet
git -C "$work/tfm" -c advice.detachedHead=false checkout -q "$TFM_COMMIT"
git clone -q "$TA_URL" "$work/ta"
git -C "$work/ta" -c advice.detachedHead=false checkout -q "$TA_COMMIT"

"$PYTHON" -I -m venv "$work/venv"
"$work/venv/bin/python" -I -m pip install -q --disable-pip-version-check "${PACKAGES[@]}"

"$work/venv/bin/python" -I "$here/yamnet-convert.py" \
  --h5 "$work/dl/yamnet.h5" \
  --yamnet-dir "$work/tfm/research/audioset/yamnet" \
  --torch-audioset "$work/ta" \
  --out "$work/out"

"$work/venv/bin/python" -I -m pip freeze >"$work/out/pip-freeze.txt"
"$work/venv/bin/python" -I -VV >>"$work/out/pip-freeze.txt"
(cd "$work/out" && shasum -a 256 yamnet.onnx yamnet.data && wc -c yamnet.onnx yamnet.data) \
  | tee "$work/out/SHA256SUMS"
