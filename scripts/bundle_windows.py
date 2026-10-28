"""Build a relocatable native CFD runtime; run on a Windows build worker.
The upstream installer is only an archive here: no WSL, registry or elevation.
"""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import urllib.request

URL = 'https://dl.openfoam.com/source/v2412/OpenFOAM-v2412-windows-mingw.exe'
SHA256 = '7c31c0526786307465cd34b4ac820530fbd52c31225d9b03713903a8f84a7028'
TOOLS = ('blockMesh', 'snappyHexMesh', 'checkMesh', 'simpleFoam', 'pimpleFoam', 'postProcess')


def bundle(destination):
    destination = Path(destination).resolve()
    if destination.exists():
        raise RuntimeError(f'Refusing to replace existing runtime: {destination}')
    seven = shutil.which('7z') or r'C:\Program Files\7-Zip\7z.exe'
    with tempfile.TemporaryDirectory(prefix='glenn-foam-') as tmp:
        tmp = Path(tmp)
        archive = tmp / 'upstream.exe'
        urllib.request.urlretrieve(URL, archive)
        with archive.open('rb') as f:
            digest = hashlib.file_digest(f, 'sha256').hexdigest()
        if digest != SHA256:
            raise RuntimeError('OpenFOAM installer checksum does not match the pinned release')
        subprocess.run([seven, 'x', '-y', f'-o{tmp / "outer"}', str(archive), 'msys64.7z', 'licenses/*', 'README.txt'], check=True, stdout=subprocess.DEVNULL)
        subprocess.run([seven, 'x', '-y', f'-o{tmp / "inner"}', str(tmp / 'outer/msys64.7z'), 'msys64/home/ofuser/OpenFOAM/OpenFOAM-v2412/*'], check=True, stdout=subprocess.DEVNULL)
        source = tmp / 'inner/msys64/home/ofuser/OpenFOAM/OpenFOAM-v2412'
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(source), str(destination))
        shutil.copytree(tmp / 'outer/licenses', destination / 'upstream-licenses')
    binary = destination / 'platforms/win64MingwDPInt32Opt/bin'
    # Serial OpenFOAM's own Pstream implementation avoids the system MSMPI
    # installation. GLENN does not launch mpiexec or promise MPI parallelism.
    shutil.copy2(binary / 'libPstream.dll-dummy', binary / 'libPstream.dll')
    for tool in TOOLS:
        if not (binary / f'{tool}.exe').is_file():
            raise RuntimeError(f'Upstream package is missing {tool}')
    manifest = {
        'schema': 1, 'version': '2412', 'platform': 'windows-x86_64',
        'bin': binary.relative_to(destination).as_posix(),
        'libraries': [binary.relative_to(destination).as_posix()],
        'environment': {'WM_PROJECT_DIR': '.', 'FOAM_ETC': 'etc',
                        'FOAM_APPBIN': binary.relative_to(destination).as_posix(),
                        'FOAM_LIBBIN': binary.relative_to(destination).as_posix()},
        'upstream_url': URL, 'upstream_sha256': SHA256,
    }
    (destination / 'runtime.json').write_text(json.dumps(manifest, indent=2), encoding='utf-8')


if __name__ == '__main__':
    bundle(sys.argv[1])
