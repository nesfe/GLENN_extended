"""Relocate an installed OpenCFD 2412 runtime and its non-glibc dependencies.
Run in the oldest supported Linux build image, after installing the pinned solver.
"""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

source, target = (Path(p).resolve() for p in sys.argv[1:3])
assert 'api=2412' in (source / 'META-INFO/api-info').read_text()
target.mkdir(parents=True, exist_ok=False)
shutil.copytree(source / 'etc', target / 'etc', symlinks=False)
platform = source / 'platforms/linux64GccDPInt32Opt'
(target / 'bin').mkdir()
(target / 'lib').mkdir()
for tool in ('blockMesh', 'snappyHexMesh', 'checkMesh', 'simpleFoam', 'pimpleFoam', 'postProcess'):
    shutil.copy2(platform / 'bin' / tool, target / 'bin' / tool)
for lib in (platform / 'lib').glob('*.so*'):
    if lib.is_file():
        shutil.copy2(lib, target / 'lib' / lib.name)
for lib in (platform / 'lib/dummy').glob('*.so'):
    shutil.copy2(lib, target / 'lib' / lib.name)
env = dict(os.environ)
env['LD_LIBRARY_PATH'] = str(target / 'lib') + ':' + env.get('LD_LIBRARY_PATH', '')
# libc and the ELF loader belong to the supported OS; other libraries travel
# with the solver, including transitive dependencies and dynamically loaded libs.
glibc = re.compile(r'^(?:ld-linux|lib(?:c|m|pthread|dl|rt|resolv|util)\.so)')
for file in list((target / 'bin').iterdir()) + list((target / 'lib').iterdir()):
    result = subprocess.run(['ldd', str(file)], env=env, capture_output=True, text=True)
    for line in result.stdout.splitlines():
        match = re.search(r'\s+(\S+) => (/\S+) \(', line)
        if match and not glibc.match(match[1]):
            name, path = match.groups()
            output = target / 'lib' / name
            if not output.exists():
                shutil.copy2(path, output)
        elif '=> not found' in line:
            raise RuntimeError(f'Missing runtime dependency in {file.name}: {line}')
manifest = {'schema': 1, 'version': '2412', 'platform': 'linux-x86_64',
            'bin': 'bin', 'libraries': ['lib'],
            'environment': {'WM_PROJECT_DIR': '.', 'FOAM_ETC': 'etc', 'FOAM_APPBIN': 'bin', 'FOAM_LIBBIN': 'lib'}}
(target / 'runtime.json').write_text(json.dumps(manifest, indent=2))
