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

TOOLS = ('blockMesh', 'snappyHexMesh', 'checkMesh', 'simpleFoam', 'pimpleFoam', 'postProcess')
# libc and the ELF loader belong to the supported OS; everything else travels
# with the solver, including transitive and dynamically loaded libraries.
GLIBC = re.compile(r'^(?:ld-linux|lib(?:c|m|pthread|dl|rt|resolv|util)\.so)')

source, target = (Path(p).resolve() for p in sys.argv[1:3])
assert 'api=2412' in (source / 'META-INFO/api-info').read_text()
target.mkdir(parents=True, exist_ok=False)
shutil.copytree(source / 'etc', target / 'etc', symlinks=False)
shutil.copytree(source / 'META-INFO', target / 'META-INFO')
platform = source / 'platforms/linux64GccDPInt32Opt'
(target / 'bin').mkdir()
(target / 'lib').mkdir()
for tool in TOOLS:
    shutil.copy2(platform / 'bin' / tool, target / 'bin' / tool)
for lib in (platform / 'lib').glob('*.so*'):
    if lib.is_file():
        shutil.copy2(lib, target / 'lib' / lib.name)
# Serial Pstream keeps the runtime free of a system MPI installation.
for lib in (platform / 'lib/dummy').glob('*.so'):
    shutil.copy2(lib, target / 'lib' / lib.name)

env = dict(os.environ)
env['LD_LIBRARY_PATH'] = os.pathsep.join(
    p for p in (str(target / 'lib'), env.get('LD_LIBRARY_PATH', '')) if p)
origins = {str(platform)}
pending = list((target / 'bin').iterdir()) + list((target / 'lib').iterdir())
resolved = set()
# A copied library brings its own dependencies, so keep walking what we add.
while pending:
    file = pending.pop()
    if file.name in resolved:
        continue
    resolved.add(file.name)
    for line in subprocess.run(['ldd', str(file)], env=env,
                               capture_output=True, text=True).stdout.splitlines():
        if '=> not found' in line:
            raise RuntimeError(f'Missing runtime dependency in {file.name}: {line.strip()}')
        match = re.search(r'\s+(\S+) => (/\S+) \(', line)
        if not match or GLIBC.match(match[1]):
            continue
        name, path = match.groups()
        origins.add(os.path.realpath(path))
        copied = target / 'lib' / name
        if not copied.exists():
            shutil.copy2(path, copied)
            pending.append(copied)

# GPL and the dependencies' own licences must travel with the binaries.
documentation = Path(sys.argv[3]) if len(sys.argv) > 3 else Path('/usr/share/doc')
licences = target / 'upstream-licences'
licences.mkdir()
packages = {source.name}
for path in origins:
    query = subprocess.run(['dpkg-query', '-S', path], capture_output=True, text=True)
    if query.returncode == 0:
        packages.add(query.stdout.split(':', 1)[0])
package_info = []
for package in sorted(packages):
    info = subprocess.run(['dpkg-query', '-W', '-f=${binary:Package} ${Version} ${source:Package} ${source:Version}\n', package],
                          capture_output=True, text=True)
    if info.returncode == 0:
        package_info.append(info.stdout)
(target / 'dependency-packages.txt').write_text(''.join(package_info), encoding='utf-8')
for package in packages:
    copyright_file = documentation / package / 'copyright'
    if copyright_file.is_file():
        shutil.copy2(copyright_file, licences / f'{package}.txt')
if not (licences / f'{source.name}.txt').is_file():
    raise RuntimeError(f'Missing solver licence: {documentation / source.name / "copyright"}')
for licence in ('GPL-3', 'LGPL-2.1', 'LGPL-3', 'GPL-2'):
    shutil.copy2(Path('/usr/share/common-licenses') / licence, licences / licence)

manifest = {'schema': 1, 'version': '2412', 'platform': 'linux-x86_64',
            'bin': 'bin', 'libraries': ['lib'],
            'environment': {'WM_PROJECT_DIR': '.', 'FOAM_ETC': 'etc',
                            'FOAM_APPBIN': 'bin', 'FOAM_LIBBIN': 'lib'}}
(target / 'runtime.json').write_text(json.dumps(manifest, indent=2), encoding='utf-8')
