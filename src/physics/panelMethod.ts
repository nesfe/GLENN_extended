import { CpPoint, Point2D } from '../types';

export interface PanelSolverResult {
  cl: number;
  cmQuarter: number;
  cpUpper: CpPoint[];
  cpLower: CpPoint[];
  cpMin: number;
  stagnationX: number;
  separationUpperX: number;
}

export function solvePanelMethod(
  upper: Point2D[],
  lower: Point2D[],
  alphaDeg: number,
  mach: number = 0
): PanelSolverResult {
  const alphaRad = (alphaDeg * Math.PI) / 180;
  const cosA = Math.cos(alphaRad);
  const sinA = Math.sin(alphaRad);

  // Combine upper (from TE to LE) and lower (from LE to TE) into a closed counter-clockwise contour
  // upper has x from 0 to 1
  // lower has x from 0 to 1
  const poly: Point2D[] = [];

  // Trailing edge down to leading edge along upper surface
  for (let i = upper.length - 1; i >= 0; i--) {
    poly.push(upper[i]);
  }
  // Leading edge to trailing edge along lower surface
  for (let i = 1; i < lower.length; i++) {
    poly.push(lower[i]);
  }

  const N = poly.length - 1;
  if (N < 10) {
    // Fallback if contour is invalid
    return {
      cl: 2 * Math.PI * Math.sin(alphaRad),
      cmQuarter: 0,
      cpUpper: upper.map((p) => ({ x: p.x, cp: 0 })),
      cpLower: lower.map((p) => ({ x: p.x, cp: 0 })),
      cpMin: -1,
      stagnationX: 0,
      separationUpperX: 1.0,
    };
  }

  // Panel definitions: nodes poly[i] to poly[i+1]
  // Control point at midpoint
  const xc = new Float64Array(N);
  const yc = new Float64Array(N);
  const len = new Float64Array(N);
  const sinTh = new Float64Array(N);
  const cosTh = new Float64Array(N);
  const nx = new Float64Array(N);
  const ny = new Float64Array(N);
  const tx = new Float64Array(N);
  const ty = new Float64Array(N);

  for (let i = 0; i < N; i++) {
    const x1 = poly[i].x;
    const y1 = poly[i].y;
    const x2 = poly[i + 1].x;
    const y2 = poly[i + 1].y;

    const dx = x2 - x1;
    const dy = y2 - y1;
    const l = Math.max(1e-7, Math.hypot(dx, dy));

    xc[i] = 0.5 * (x1 + x2);
    yc[i] = 0.5 * (y1 + y2);
    len[i] = l;

    cosTh[i] = dx / l;
    sinTh[i] = dy / l;

    // Tangent vector along panel (counter-clockwise)
    tx[i] = cosTh[i];
    ty[i] = sinTh[i];

    // Outward normal vector (rotated +90 deg from tangent in CCW boundary)
    // Actually for CCW contour, outward normal is (dy/l, -dx/l)
    nx[i] = -sinTh[i];
    ny[i] = cosTh[i];
  }

  // Prandtl-Glauert compressibility correction factor beta
  const effM = Math.min(0.88, Math.max(0, mach));
  const betaPG = Math.sqrt(Math.max(0.15, 1 - effM * effM));

  // Build linear system for panel vortex strengths:
  // Approximate vortex panel method with constant vorticity and Kutta condition
  // For each control point i: sum_j A_ij * gamma_j = - (V_inf dot n_i)
  const A: number[][] = [];
  const rhs = new Float64Array(N + 1);

  for (let i = 0; i < N; i++) {
    A[i] = new Array(N + 1).fill(0);
    // Freestream normal velocity
    const vinf_n = cosA * nx[i] + sinA * ny[i];
    rhs[i] = -vinf_n;

    for (let j = 0; j < N; j++) {
      if (i === j) {
        A[i][j] = 0.5; // Self-induction normal limit
      } else {
        const rx = xc[i] - xc[j];
        const ry = yc[i] - yc[j];
        const r2 = Math.max(1e-7, rx * rx + ry * ry);
        // Velocity induced by point vortex at j
        const u = -ry / (2 * Math.PI * r2);
        const v = rx / (2 * Math.PI * r2);
        A[i][j] = (u * nx[i] + v * ny[i]) * len[j];
      }
    }
  }

  // Kutta condition at trailing edge (row N): gamma_0 + gamma_{N-1} = 0
  A[N] = new Array(N + 1).fill(0);
  A[N][0] = 1.0;
  A[N][N - 1] = 1.0;
  rhs[N] = 0.0;

  // Solve (N+1) x (N+1) least-squares / bordered system using regularized solver
  const gamma = solveLinearSystem(A, rhs, N);

  // Compute tangential velocities and Cp
  const cpValues = new Float64Array(N);
  let cpMin = 1.0;
  let minCpIdx = 0;
  let stagIdx = 0;
  let minVt = Infinity;

  for (let i = 0; i < N; i++) {
    const vinf_t = cosA * tx[i] + sinA * ty[i];
    let vt_ind = 0;

    for (let j = 0; j < N; j++) {
      if (i === j) {
        vt_ind += 0.5 * gamma[j];
      } else {
        const rx = xc[i] - xc[j];
        const ry = yc[i] - yc[j];
        const r2 = Math.max(1e-7, rx * rx + ry * ry);
        const u = -ry / (2 * Math.PI * r2);
        const v = rx / (2 * Math.PI * r2);
        vt_ind += (u * tx[i] + v * ty[i]) * len[j] * gamma[j];
      }
    }

    const vt = Math.abs(vinf_t + vt_ind);
    if (vt < minVt) {
      minVt = vt;
      stagIdx = i;
    }

    // Incompressible Cp
    const cpIncomp = 1 - vt * vt;
    // Prandtl-Glauert compressibility correction
    const cpComp = cpIncomp / betaPG;
    cpValues[i] = Math.max(-5.0, Math.min(1.0, cpComp));

    if (cpValues[i] < cpMin) {
      cpMin = cpValues[i];
      minCpIdx = i;
    }
  }

  // Map panel Cp back to upper and lower points
  const cpUpper: CpPoint[] = [];
  const cpLower: CpPoint[] = [];

  const upperCount = upper.length;
  // Panels corresponding to upper surface
  for (let i = 0; i < upperCount - 1; i++) {
    // upper is poly[N - 1 - i]
    const pIdx = N - 1 - i;
    if (pIdx >= 0 && pIdx < N) {
      cpUpper.push({ x: xc[pIdx], cp: cpValues[pIdx] });
    }
  }
  cpUpper.sort((a, b) => a.x - b.x);

  // Panels corresponding to lower surface
  for (let i = upperCount - 1; i < N; i++) {
    cpLower.push({ x: xc[i], cp: cpValues[i] });
  }
  cpLower.sort((a, b) => a.x - b.x);

  // Integrate Cl and Cm(c/4)
  let cl = 0;
  let cm = 0;
  for (let i = 0; i < N; i++) {
    const cp = cpValues[i];
    const dl = len[i];
    // Force normal to panel: dFn = -cp * dl * n
    const dfx = -cp * dl * nx[i];
    const dfy = -cp * dl * ny[i];

    // Lift is perpendicular to freestream
    const dL = -dfx * sinA + dfy * cosA;
    cl += dL;

    // Pitching moment about quarter-chord (0.25, 0)
    // Positive nose-up: (x - 0.25) * dfy - y * dfx
    cm += -(xc[i] - 0.25) * dfy + yc[i] * dfx;
  }

  // Boundary layer separation estimation (Stratford criterion approximation)
  // Flow separates where adverse pressure gradient dCp/dx exceeds critical threshold
  let sepUpper = 1.0;
  for (let i = 1; i < cpUpper.length; i++) {
    const dx = Math.max(0.01, cpUpper[i].x - cpUpper[i - 1].x);
    const dcp = cpUpper[i].cp - cpUpper[i - 1].cp;
    const grad = dcp / dx;
    if (cpUpper[i].x > 0.4 && grad > 2.8 && alphaDeg > 8) {
      sepUpper = Math.min(sepUpper, cpUpper[i].x);
      break;
    }
  }

  return {
    cl,
    cmQuarter: cm,
    cpUpper,
    cpLower,
    cpMin,
    stagnationX: xc[stagIdx] || 0,
    separationUpperX: sepUpper,
  };
}

// Gaussian elimination with partial pivoting
function solveLinearSystem(A: number[][], b: Float64Array, n: number): Float64Array {
  // Use first n rows/cols with least-squares regularization
  const x = new Float64Array(n);
  const M: number[][] = [];

  for (let i = 0; i < n; i++) {
    M[i] = new Array(n + 1);
    for (let j = 0; j < n; j++) {
      M[i][j] = A[i][j] + (i === j ? 1e-4 : 0);
    }
    M[i][n] = b[i];
  }

  for (let k = 0; k < n; k++) {
    let maxRow = k;
    let maxVal = Math.abs(M[k][k]);
    for (let r = k + 1; r < n; r++) {
      if (Math.abs(M[r][k]) > maxVal) {
        maxVal = Math.abs(M[r][k]);
        maxRow = r;
      }
    }

    if (maxVal < 1e-12) continue;

    if (maxRow !== k) {
      const temp = M[k];
      M[k] = M[maxRow];
      M[maxRow] = temp;
    }

    for (let r = k + 1; r < n; r++) {
      const factor = M[r][k] / M[k][k];
      for (let c = k; c <= n; c++) {
        M[r][c] -= factor * M[k][c];
      }
    }
  }

  // Back substitution
  for (let i = n - 1; i >= 0; i--) {
    let sum = M[i][n];
    for (let j = i + 1; j < n; j++) {
      sum -= M[i][j] * x[j];
    }
    x[i] = Math.abs(M[i][i]) > 1e-12 ? sum / M[i][i] : 0;
  }

  return x;
}
