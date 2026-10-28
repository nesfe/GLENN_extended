"""Exercise packaged CLI with no shell activation or separately installed CFD.
Move the package first: this must also work outside its build directory.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

exe = Path(sys.argv[1]).resolve()
fixture = Path(sys.argv[2]).resolve()
env = {k: v for k, v in os.environ.items() if not k.startswith(('FOAM_', 'WM_', 'OPENFOAM'))}
env.pop('LD_LIBRARY_PATH', None)
env.pop('DYLD_LIBRARY_PATH', None)
# Deliberately exclude developer OpenFOAM/MSYS/WSL installations from lookup.
env['PATH'] = str(Path(os.environ.get('SystemRoot', 'C:/Windows')) / 'System32') if os.name == 'nt' else '/usr/bin:/bin'
with tempfile.TemporaryDirectory(prefix='glenn-case-') as tmp:
    for mode in ('laminar', 'rans', 'urans'):
        case = Path(tmp) / mode
        result = subprocess.run([str(exe), str(fixture), '--case', str(case), '--mode', mode,
                                 '--speed', '1', '--nu', '0.01', '--iterations', '100', '--time', '0.1'],
                                env=env, text=True, capture_output=True, timeout=600)
        print(result.stdout, result.stderr, flush=True)
        if result.returncode:
            raise RuntimeError(f'Bundled {mode} solver failed: {result.returncode}')
        inspection = subprocess.run([str(exe), '--inspect', str(case)], env=env,
                                    text=True, capture_output=True, check=True)
        print(inspection.stdout, flush=True)
        assert 'samples=0 ' not in inspection.stdout
        if mode == 'laminar':
            value = inspection.stdout.split('min_Ux=')[1].split()[0]
            assert float(value) < -0.01, 'Missing resolved backflow at Re=100'
        assert json.loads((case / 'glenn-case.json').read_text())['speed'] == 1.0
