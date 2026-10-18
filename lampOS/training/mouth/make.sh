#!/bin/sh
# Makes face_mesh_v2.onnx from Google's own MediaPipe release, on a Mac or
# Linux machine with uv, about 2 GB of free disk and a network (#720). See
# models/mouth/SOURCE.md.
#
#     sh make.sh
#
# Downloads go to lampOS/.cache/mouth (git-ignored):
#   face_landmarker.task (float16, version 1) from
#   https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/1/face_landmarker.task
# The model is written to lampOS/.cache/mouth/face_mesh_v2.onnx and checked
# against the SHA-256 in SOURCE.md. It is not kept in git.
set -e
here=$(cd "$(dirname "$0")" && pwd)
cache="$here/../../.cache/mouth"
cd "$here"
export UV_PROJECT_ENVIRONMENT="$cache/venv" UV_CACHE_DIR="$cache/uv-cache"
uv sync --python 3.11
py="$cache/venv/bin/python"

task="$cache/face_landmarker.task"
[ -s "$task" ] || curl -sSL --fail --retry 5 -o "$task" \
  https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/1/face_landmarker.task
echo "64184e229b263107bc2b804c6625db1341ff2bb731874b0bcc2fe6544e0bc9ff  $task" | shasum -a 256 -c -

rm -rf "$cache/task"
unzip -q "$task" -d "$cache/task"
tflite="$cache/task/face_landmarks_detector.tflite"
echo "c7d54204ce0448474c7f3fa9af494787c0965cbdd6f20fc72867e43046bd43d5  $tflite" | shasum -a 256 -c -

"$py" -I -m tf2onnx.convert --tflite "$tflite" --output "$cache/converted.onnx" --opset 13
"$py" -I canon.py "$cache/converted.onnx" "$cache/face_mesh_v2.onnx"
"$py" -I parity.py "$tflite" "$cache/face_mesh_v2.onnx"
want=$(sed -n 's/^| SHA-256 | `\([0-9a-f]*\)` |$/\1/p' ../../models/mouth/SOURCE.md)
echo "$want  $cache/face_mesh_v2.onnx" | shasum -a 256 -c -
