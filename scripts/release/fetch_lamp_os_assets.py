#!/usr/bin/env python3
"""Fetch public model/runtime assets and verify immutable content before packaging."""
import argparse
import hashlib
import io
from pathlib import Path
import tarfile
import subprocess
import tempfile
from lamp_os_package import ASSETS, OPTIONAL_ASSETS

URLS = {
    'smart-turn-v3.2-gpu.onnx': 'https://huggingface.co/pipecat-ai/smart-turn-v3/resolve/f766f81d3cfdf7737ac64aad813d91bbfd56bf93/smart-turn-v3.2-gpu.onnx',
    'libonnxruntime.so.1.30.0': 'https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-linux-aarch64-1.30.0.tgz',
    'melspectrogram.onnx': 'https://github.com/dscripka/openWakeWord/releases/download/v0.5.1/melspectrogram.onnx',
    'embedding_model.onnx': 'https://github.com/dscripka/openWakeWord/releases/download/v0.5.1/embedding_model.onnx',
    'face_detection_yunet_2023mar.onnx': 'https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models/face_detection_yunet/face_detection_yunet_2023mar.onnx',
    'face_recognition_sface_2021dec.onnx': 'https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models/face_recognition_sface/face_recognition_sface_2021dec.onnx',
}


def fetch_assets(destination):
    destination = Path(destination)
    destination.mkdir(parents=True, exist_ok=True)
    for name, expected in {**ASSETS, **OPTIONAL_ASSETS}.items():
        path = destination / name
        if path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest() == expected:
            continue
        print(f'Fetching {name}', flush=True)
        with tempfile.TemporaryDirectory() as temporary:
            downloaded = Path(temporary) / 'download'
            subprocess.run(['curl', '--fail', '--location', '--retry', '2',
                            '--connect-timeout', '30', '--max-time', '600',
                            '--max-filesize', str(256 * 1024 * 1024),
                            '--output', str(downloaded), URLS[name]], check=True)
            data = downloaded.read_bytes()
        if len(data) > 256 * 1024 * 1024:
            raise ValueError(f'{name}: download too large')
        if name.startswith('libonnxruntime'):
            with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as archive:
                member = archive.getmember('onnxruntime-linux-aarch64-1.30.0/lib/' + name)
                if not member.isfile() or member.size > 64 * 1024 * 1024:
                    raise ValueError('Invalid ONNX runtime archive member')
                data = archive.extractfile(member).read()
        if hashlib.sha256(data).hexdigest() != expected:
            raise ValueError(f'{name}: SHA-256 mismatch')
        temporary = path.with_suffix(path.suffix + '.partial')
        temporary.write_bytes(data)
        temporary.replace(path)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination')
    fetch_assets(parser.parse_args().destination)
