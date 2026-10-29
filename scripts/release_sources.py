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

def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def download(target, url, checksum):
    partial = target.with_name(target.name + '.part')
    if not target.is_file():
        # curl --retry restarts a timed-out download from the beginning. Keep
        # the partial archive and issue a fresh Range request on each attempt.
        for attempt in range(3):
            print(f'{target.name}: download attempt {attempt + 1}/3', flush=True)
            result = subprocess.run([
                'curl', '-fsSL', '--connect-timeout', '30', '--max-time', '600',
                '--continue-at', '-', url, '-o', str(partial)])
            if result.returncode == 0:
                break
        else:
            result.check_returncode()
        if sha256(partial) != checksum:
            partial.unlink()
            raise RuntimeError(f'Upstream source checksum mismatch: {target.name}')
        partial.replace(target)
    elif sha256(target) != checksum:
        raise RuntimeError(f'Upstream source checksum mismatch: {target.name}')
    print(f'{target.name}: SHA-256 verified', flush=True)


if __name__ == '__main__':
    destination = Path(sys.argv[1])
    destination.mkdir(parents=True, exist_ok=True)
    for name, (url, checksum) in SOURCES.items():
        download(destination / name, url, checksum)
