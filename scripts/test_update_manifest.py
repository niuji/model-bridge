import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('update-manifest.py')

class ManifestTests(unittest.TestCase):
    def test_manifest_and_aggregate_checksums(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            linux = root / 'linux' / 'model-bridge-linux-amd64.tar.gz'
            linux.parent.mkdir()
            linux.write_bytes(b'linux archive')
            windows = root / 'windows' / 'model-bridge-windows-amd64.zip'
            windows.parent.mkdir()
            windows.write_bytes(b'windows archive')
            subprocess.run(['python3', str(SCRIPT), 'v1.2.3', str(root)], check=True)
            manifest = json.loads((root / 'update-manifest.json').read_text())
            self.assertEqual(manifest, dict(version='1.2.3', target='x86_64-unknown-linux-musl', asset=linux.name, sha256=hashlib.sha256(linux.read_bytes()).hexdigest(), size=13, update_protocol=1))
            self.assertIn(windows.name, (root / 'SHA256SUMS').read_text())
            self.assertIn(manifest['sha256'] + '  ' + linux.name, (root / 'SHA256SUMS').read_text())

    def test_invalid_version_and_missing_archive_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            for version in ['v1.2.3/evil', 'v01.2.3', 'v1.2.3']:
                result = subprocess.run(['python3', str(SCRIPT), version, directory], capture_output=True)
                self.assertNotEqual(result.returncode, 0)

if __name__ == '__main__':
    unittest.main()
