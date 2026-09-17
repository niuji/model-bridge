"""Validate generated units with systemd's parser without installing or starting services."""
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest


@unittest.skipUnless(shutil.which('systemd-analyze'), 'systemd-analyze is unavailable')
class InstallerUnitTests(unittest.TestCase):
    def test_generated_units_are_accepted_by_systemd(self):
        script = (Path(__file__).parent / 'install-user.sh').read_text()
        with tempfile.TemporaryDirectory(prefix='mb-unit-test-') as directory:
            units = []
            for variable, name in [('UNIT_FILE', 'model-bridge-test.service'),
                                   ('UPDATE_UNIT_FILE', 'model-bridge-update-test.service')]:
                unit = re.search(r'cat > "\$' + variable + r'" <<\'EOF\'\n(.*?)\nEOF', script, re.S).group(1)
                # Only substitute executables: the fixture is not installed on this machine.
                unit = unit.replace('"%h/.local/bin/model-bridge"', '"/usr/bin/true"')
                unit = unit.replace('"%h/.local/share/model-bridge/update/worker"', '"/usr/bin/true"')
                path = Path(directory) / name
                path.write_text(unit + '\n')
                units.append(str(path))
            result = subprocess.run(['systemd-analyze', 'verify', *units], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
