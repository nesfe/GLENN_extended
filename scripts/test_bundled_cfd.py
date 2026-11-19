"""Exercise packaged CLI with no shell activation or separately installed CFD.
Move the package first: this must also work outside its build directory.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

# Solver progress is Russian; a Windows console codepage must not fail the test.
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
sys.stderr.reconfigure(encoding='utf-8', errors='replace')
exe = Path(sys.argv[1]).resolve()
fixture = Path(sys.argv[2]).resolve()
env = {k: v for k, v in os.environ.items() if not k.startswith(('FOAM_', 'WM_', 'OPENFOAM'))}
env.pop('LD_LIBRARY_PATH', None)
env.pop('DYLD_LIBRARY_PATH', None)
# Deliberately exclude developer OpenFOAM/MSYS/WSL installations from lookup.
env['PATH'] = str(Path(os.environ.get('SystemRoot', 'C:/Windows')) / 'System32') if os.name == 'nt' else '/usr/bin:/bin'
with tempfile.TemporaryDirectory(prefix='glenn-case-Продувка ') as tmp:
    cases = [(mode, False) for mode in ('laminar', 'rans', 'urans')]
    if '--compressible' in sys.argv[3:]:
        cases.extend([('rans', True), ('urans', True)])
    for mode, gas in cases:
        case = Path(tmp) / (mode + ('-gas' if gas else ''))
        flow = ['--speed', '170', '--air-temperature', '288.15'] if gas else ['--speed', '1', '--nu', '0.01']
        result = subprocess.run([str(exe), str(fixture), '--case', str(case), '--mode', mode,
                                 *flow, '--iterations', '30' if gas else '100', '--time', '0.1'],
                                cwd=exe.parent, env=env, text=True, encoding='utf-8', errors='strict', capture_output=True, timeout=900)
        print(result.stdout, result.stderr, flush=True)
        if result.returncode:
            raise RuntimeError(f'Bundled {mode} solver failed: {result.returncode}')
        inspection = subprocess.run([str(exe), '--inspect', str(case)], env=env,
                                    text=True, encoding='utf-8', errors='strict', capture_output=True, check=True)
        print(inspection.stdout, flush=True)
        assert 'samples=0 ' not in inspection.stdout
        if mode == 'laminar':
            value = inspection.stdout.split('min_Ux=')[1].split()[0]
            assert float(value) < -0.01, 'Missing resolved backflow at Re=100'
        assert json.loads((case / 'glenn-case.json').read_text())['speed'] == (170.0 if gas else 1.0)
        if gas:
            solver = 'rhoCentralFoam' if mode == 'urans' else 'rhoSimpleFoam'
            assert (case / f'log.{solver}').is_file()
            field_time = float(inspection.stdout.split('t=')[1].split()[0])
            latest = max((p for p in case.iterdir() if p.is_dir() and (p / 'T').is_file()), key=lambda p: float(p.name))
            assert float(latest.name) == field_time
            pressure = case / 'postProcessing' / 'wallPressure' / latest.name / 'p_body.raw'
            assert pressure.is_file() and pressure.stat().st_size > 1000
