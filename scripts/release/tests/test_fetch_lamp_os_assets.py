"""Asset downloads must match pinned content; no network used by these tests."""
import hashlib
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

RELEASE = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(RELEASE))
spec = importlib.util.spec_from_file_location('asset_fetch', RELEASE / 'fetch_lamp_os_assets.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class AssetTests(unittest.TestCase):
    def test_verified_cache_skips_network(self):
        data = b'verified model'
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'model.onnx'; path.write_bytes(data)
            with patch.object(module, 'ASSETS', {'model.onnx': hashlib.sha256(data).hexdigest()}), \
                    patch.object(module, 'OPTIONAL_ASSETS', {}), patch.object(module.subprocess, 'run') as run:
                module.fetch_assets(directory)
                run.assert_not_called()

    def test_corrupt_download_never_replaces_cached_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'model.onnx'; path.write_bytes(b'old')
            def download(args, **kwargs):
                self.assertNotIn('--insecure', args)
                Path(args[args.index('--output') + 1]).write_bytes(b'corrupt')
            with patch.object(module, 'ASSETS', {'model.onnx': 'a' * 64}), \
                    patch.object(module, 'OPTIONAL_ASSETS', {}), \
                    patch.object(module, 'URLS', {'model.onnx': 'https://example.invalid/model'}), \
                    patch.object(module.subprocess, 'run', side_effect=download):
                with self.assertRaisesRegex(ValueError, 'SHA-256 mismatch'):
                    module.fetch_assets(directory)
            self.assertEqual(path.read_bytes(), b'old')
