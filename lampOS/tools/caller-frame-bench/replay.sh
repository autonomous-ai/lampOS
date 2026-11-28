#!/usr/bin/env bash
# Local comparison. Only pinned public weights are downloaded; all audio is read locally.
set -euo pipefail
umask 077
if [[ $# != 3 && $# != 5 ]]; then
    echo 'usage: bash replay.sh KIND PRIVATE_CORPUS NEW_BUNDLE [CAMPLUS.onnx FRAME_MODEL_DIRECTORY]' >&2
    exit 2
fi
kind=$1
case "$kind" in
    synthetic|public-speech|synthetic-heldout|lamp-replay) ;;
    *) echo 'unknown evidence kind' >&2; exit 2 ;;
esac
archive=$(cd "$2" && pwd -P)
if [[ ! -f "$archive/pairs.tsv" || ! -f "$archive/audio-SHA256SUMS" ]]; then
    echo 'the corpus must include pairs.tsv and audio-SHA256SUMS' >&2
    exit 2
fi
tool=$(cd "$(dirname "$0")" && pwd -P)
workspace=$(cd "$tool/../.." && pwd -P)
mkdir "$3"
bundle=$(cd "$3" && pwd -P)
if [[ $# == 5 ]]; then
    cam=$4
    frame=$5
else
    frame="$bundle/models"
    mkdir "$frame"
    source_url='https://huggingface.co/wq2012/personal-vad-v1-st-ce/resolve/56f67ae31897518a994dec0cb3fd283fd9db027e'
    for asset in model.safetensors speaker_subspace.npz; do
        curl -fLsS --retry 2 --output "$frame/$asset" "$source_url/$asset"
    done
    cam="$frame/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx"
    curl -fLsS --retry 2 --output "$cam" "https://github.com/autonomous-ai/lampOS/releases/download/models-2026-11-04/${cam##*/}"
    ruby "$tool/prepare-model.rb" "$frame"
fi
git -C "$workspace" rev-parse HEAD > "$bundle/source-revision.txt"
git -C "$workspace" diff --binary HEAD -- crates/same-voice tools/caller-frame-bench > "$bundle/source.patch"
cargo run --quiet --release --locked --manifest-path "$workspace/Cargo.toml" \
    -p lamp-same-voice --example caller_frame_bench -- "$archive/pairs.tsv" "$cam" "$frame" "$bundle/run"
ruby "$tool/score.rb" compare "$kind" "$archive/pairs.tsv" \
    "$bundle/run/whole.tsv" "$bundle/run/frame.tsv" "$bundle/paired"
ruby "$tool/score.rb" compare "$kind" "$archive/pairs.tsv" \
    "$bundle/run/unguarded-cam.tsv" "$bundle/run/frame.tsv" "$bundle/unguarded"
echo 'Local paired replay. The source corpus has its own audio-SHA256SUMS. run/ contains raw predictions and timings. paired/ compares the frozen classifiers. unguarded/ is the CAM++ duration diagnostic.' > "$bundle/README.md"
(cd "$bundle" && find . -type f ! -name SHA256SUMS -print0 | xargs -0 shasum -a 256 > SHA256SUMS)
