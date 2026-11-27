#!/bin/sh
# Makes lampOS/.cache/stop/venv, the environment run.sh runs every script in
# (training/stop's: its pyproject.toml and uv.lock), and checks that it loads.
#
#     sh env.sh
#
# On an Apple Silicon Mac or Linux this is training/stop's own `uv sync`. On an
# Intel Mac that lock cannot be installed (its torch 2.14.1 and onnxruntime
# 1.31.0 have no macOS x86_64 wheels), so the same packages are installed from
# intel-mac.txt instead: torch and torchaudio 2.2.2, NumPy 1.26.4, onnxruntime
# 1.19.2, every other package at the lock's version, each wheel's SHA-256
# checked. intel-mac.txt is compiled from intel-mac-overrides.txt:
#
#     uv pip compile ../stop/pyproject.toml --python-version 3.12 \
#       --python-platform x86_64-apple-darwin --override intel-mac-overrides.txt \
#       --generate-hashes --no-header -o intel-mac.txt
#
# The interpreter is uv's own Python 3.12, kept beside the environment
# (lampOS/.cache/stop/python), so nothing outside lampOS/.cache can break it.
set -e
here=$(cd "$(dirname "$0")" && pwd)
cache="$here/../../.cache/stop"
mkdir -p "$cache"
export UV_CACHE_DIR="$cache/uv-cache" UV_PYTHON_INSTALL_DIR="$cache/python"
if [ "$(uname -s)-$(uname -m)" = "Darwin-x86_64" ]; then
  [ -x "$cache/venv/bin/python" ] || uv venv --python 3.12 --python-preference only-managed "$cache/venv"
  VIRTUAL_ENV="$cache/venv" uv pip sync --require-hashes "$here/intel-mac.txt"
else
  (cd "$here/../stop" && UV_PROJECT_ENVIRONMENT="$cache/venv" uv sync --python 3.12 --python-preference only-managed)
fi
# The front end's two models, as training/stop's make.sh fetches them (each
# SHA-256 is checked again by features.py when it loads them).
mkdir -p "$cache/oww"
for f in melspectrogram.onnx embedding_model.onnx; do
  [ -s "$cache/oww/$f" ] || curl -sSL --fail --retry 5 -o "$cache/oww/$f" \
    "https://github.com/dscripka/openWakeWord/releases/download/v0.5.1/$f"
done
sh "$here/run.sh" heylamp.py check-env
