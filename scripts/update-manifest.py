#!/usr/bin/env python3
"""Generate release checksums and Linux updater metadata from packaged artifacts."""
import hashlib
import json
from pathlib import Path
import re
import sys


def main():
    tag, directory = sys.argv[1:]
    version = tag.removeprefix('v')
    number = r'(?:0|[1-9][0-9]*)'
    identifier = r'(?:0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)'
    if not re.fullmatch(rf'{number}\.{number}\.{number}(?:-{identifier}(?:\.{identifier})*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?', version):
        raise ValueError('release version must be SemVer')
    root = Path(directory)
    archives = sorted([*root.rglob('*.tar.gz'), *root.rglob('*.zip')])
    if len({path.name for path in archives}) != len(archives):
        raise ValueError('duplicate release asset names')
    linux = next(path for path in archives if path.name == 'model-bridge-linux-amd64.tar.gz')
    digests = {}
    for path in archives:
        with path.open('rb') as archive:
            digests[path.name] = hashlib.file_digest(archive, 'sha256').hexdigest()
    size = linux.stat().st_size
    if not 0 < size <= 256 * 1024 * 1024:
        raise ValueError('Linux archive size is outside updater limits')
    manifest = dict(version=version, target='x86_64-unknown-linux-musl', asset=linux.name,
                    sha256=digests[linux.name], size=size, update_protocol=1)
    (root / 'SHA256SUMS').write_text(''.join(f'{digests[path.name]}  {path.name}\n' for path in archives))
    (root / 'update-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')


if __name__ == '__main__':
    main()
