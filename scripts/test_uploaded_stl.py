"""Real packaged CFD regression for the user's thin TsAGI wing, not a cube.
The old package completed successfully but deleted the body and returned U=20
and zero forces. Keep the case for a UI capture and diagnosis on CI workers.
"""
import os
from pathlib import Path
import re
import subprocess
import sys

sys.stdout.reconfigure(encoding='utf-8', errors='replace')
sys.stderr.reconfigure(encoding='utf-8', errors='replace')
exe, model, case = (Path(arg).resolve() for arg in sys.argv[1:4])
env = {k: v for k, v in os.environ.items() if not k.startswith(('FOAM_', 'WM_', 'OPENFOAM'))}
env.pop('LD_LIBRARY_PATH', None)
env['PATH'] = str(Path(os.environ.get('SystemRoot', 'C:/Windows')) / 'System32') if os.name == 'nt' else '/usr/bin:/bin'
run = subprocess.run([str(exe), str(model), '--case', str(case), '--quality', 'preview',
                      '--mode', 'rans', '--speed', '20', '--iterations', '30'],
                     cwd=exe.parent, env=env, capture_output=True, text=True,
                     encoding='utf-8', errors='strict', timeout=1200)
print(run.stdout, run.stderr, flush=True)
run.check_returncode()
boundary = (case / 'constant/polyMesh/boundary').read_text()
body = re.search(r'\bbody\s*\{[^}]*\bnFaces\s+(\d+)', boundary)
assert body and int(body[1]) > 1000, 'The wing vanished from the volume mesh'
assert 'Mesh OK' in (case / 'log.checkMesh').read_text()
result = subprocess.run([str(exe), '--inspect', str(case)], env=env, capture_output=True,
                        text=True, encoding='utf-8', errors='strict', timeout=120, check=True)
print(result.stdout, flush=True)
forces = [float(v) for v in re.search(r'force_N=\[([^]]+)\]', result.stdout)[1].split(',')]
assert sum(v*v for v in forces) > 1.0, 'The body must exert a measured force on the flow'
assert float(result.stdout.split('min_Ux=')[1].split()[0]) < 19.0, 'No velocity deficit near the body'
print('Uploaded STL: wall retained, mesh passed, forces and disturbed flow present.', flush=True)
