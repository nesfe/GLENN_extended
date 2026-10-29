"""Mirror the upstream sources beside the binary release, pinned by SHA-256."""
import hashlib
from pathlib import Path
import subprocess
import sys

SOURCES = {
    'OpenFOAM-2412-b8cf4d35.tar.gz': (
        'https://gitlab.com/openfoam/core/openfoam/-/archive/b8cf4d35/openfoam-b8cf4d35.tar.gz',
        '68066246a30993d6e99183d5418c7cd18afba8c319ce87e09b3539eff06802a8'),
    'OpenFOAM-v2412.tgz': (
        'https://dl.openfoam.com/source/v2412/OpenFOAM-v2412.tgz',
        'c353930105c39b75dac7fa7cfbfc346390caa633a868130fd8c9816ef5f732cd'),
    'ThirdParty-v2412.tgz': (
        'https://dl.openfoam.com/source/v2412/ThirdParty-v2412.tgz',
        'a55f7f1401e227bf59fbfa318924e73db4069d61b937639d6031f23f58de7575'),
}

destination = Path(sys.argv[1])
destination.mkdir(parents=True, exist_ok=True)
for name, (url, checksum) in SOURCES.items():
    target = destination / name
    if not target.is_file():
        subprocess.run(['curl', '-fsSL', '--retry', '3', '--max-time', '600',
                        url, '-o', str(target)], check=True)
    digest = hashlib.sha256()
    with target.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    if digest.hexdigest() != checksum:
        raise RuntimeError(f'Upstream source checksum mismatch: {name}')
    print(f'{name}: SHA-256 verified', flush=True)
