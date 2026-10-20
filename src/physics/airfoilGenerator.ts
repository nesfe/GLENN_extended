import { AirfoilGeometry, AirfoilType, FlapType, Point2D } from '../types';

export interface AirfoilGenParams {
  type: AirfoilType;
  camber: number; // % (0-10)
  camberPos: number; // % (10-90)
  thickness: number; // % (2-35)
  flapAngle: number; // degrees (-20 to 45)
  flapType: FlapType;
  slatAngle: number; // degrees (0 to 25)
  numPoints?: number;
  customUpper?: Point2D[] | null;
  customLower?: Point2D[] | null;
}

export function generateAirfoilCoordinates(params: AirfoilGenParams): AirfoilGeometry {
  const n = params.numPoints || 65;
  const m = params.camber / 100;
  const p = params.camberPos / 100;
  const t = params.thickness / 100;
  const flapRad = (params.flapAngle * Math.PI) / 180;
  const slatRad = (params.slatAngle * Math.PI) / 180;

  // Handle custom coordinates if provided
  if (params.customUpper && params.customLower && params.customUpper.length > 5) {
    const rawUpper = [...params.customUpper];
    const rawLower = [...params.customLower];
    const flapHinge = 0.75;
    const slatHinge = 0.15;

    const upper: Point2D[] = [];
    const lower: Point2D[] = [];
    const camberLine: Point2D[] = [];
    const thicknessDist: Point2D[] = [];

    for (let i = 0; i < rawUpper.length; i++) {
      let xu = rawUpper[i].x;
      let yu = rawUpper[i].y;
      let xl = rawLower[i].x;
      let yl = rawLower[i].y;

      // Trailing-edge flap
      if (xu > flapHinge && Math.abs(params.flapAngle) > 0.05) {
        const dx = xu - flapHinge;
        yu -= dx * Math.tan(flapRad) * 0.9;
      }
      if (xl > flapHinge && Math.abs(params.flapAngle) > 0.05) {
        const dx = xl - flapHinge;
        yl -= dx * Math.tan(flapRad) * 0.9;
      }

      // Leading-edge slat
      if (xu < slatHinge && params.slatAngle > 0.05) {
        const dx = slatHinge - xu;
        yu -= dx * Math.tan(slatRad) * 0.8;
      }
      if (xl < slatHinge && params.slatAngle > 0.05) {
        const dx = slatHinge - xl;
        yl -= dx * Math.tan(slatRad) * 0.8;
      }

      upper.push({ x: xu, y: yu });
      lower.push({ x: xl, y: yl });
      camberLine.push({ x: xu, y: 0.5 * (yu + yl) });
      thicknessDist.push({ x: xu, y: Math.abs(yu - yl) });
    }

    return {
      upper,
      lower,
      camberLine,
      thicknessDist,
      chord: 1.0,
      maxThickness: t,
      maxThicknessX: 0.3,
      maxCamber: m,
      maxCamberX: p,
      leadingEdgeRadius: 1.1019 * t * t,
      trailingEdgeAngle: 15,
    };
  }

  const upper: Point2D[] = [];
  const lower: Point2D[] = [];
  const camberLine: Point2D[] = [];
  const thicknessDist: Point2D[] = [];

  const flapHinge = 0.75;
  const slatHinge = 0.15;

  for (let i = 0; i <= n; i++) {
    // Cosine clustering towards leading and trailing edges
    const beta = (i / n) * Math.PI;
    const x = 0.5 * (1 - Math.cos(beta));

    let yt = 0;
    let yc = 0;
    let dyc = 0;

    switch (params.type) {
      case 'biconvex': {
        // Parabolic arc biconvex supersonic profile
        yt = 2 * t * x * (1 - x);
        yc = 4 * m * x * (1 - x);
        dyc = 4 * m * (1 - 2 * x);
        break;
      }

      case 'double_wedge': {
        // Diamond supersonic profile
        if (x < 0.5) {
          yt = t * x;
          yc = m * 2 * x;
          dyc = m * 2;
        } else {
          yt = t * (1 - x);
          yc = m * 2 * (1 - x);
          dyc = -m * 2;
        }
        break;
      }

      case 'cylinder': {
        // Circular cylinder cross section
        const r = 0.5;
        const xc = x - 0.5;
        const disc = r * r - xc * xc;
        yt = disc > 0 ? Math.sqrt(disc) : 0;
        yc = 0;
        dyc = 0;
        break;
      }

      case 'flat_plate': {
        // Infinitesimal flat plate with tiny rounded edge
        yt = t > 0.01 ? t * Math.sqrt(Math.max(0, 0.01 - (x - 0.5) * (x - 0.5))) : 0.005;
        yc = 0;
        dyc = 0;
        break;
      }

      case 'supercritical': {
        // NASA SC(2) series representation: flat top, aft loading camber cusp
        const x_sc = Math.max(0, Math.min(1, x));
        // Thickness distribution with flatter crest
        yt = 5 * t * (0.24 * Math.sqrt(x_sc) - 0.08 * x_sc - 0.3 * x_sc * x_sc + 0.22 * Math.pow(x_sc, 3) - 0.08 * Math.pow(x_sc, 4));
        // Aft camber loading
        if (x_sc < 0.6) {
          yc = m * (0.5 * x_sc);
          dyc = 0.5 * m;
        } else {
          const delta = (x_sc - 0.6) / 0.4;
          yc = m * (0.3 + 0.7 * Math.pow(delta, 2.5));
          dyc = (m * 0.7 * 2.5 * Math.pow(delta, 1.5)) / 0.4;
        }
        break;
      }

      case 'naca5': {
        // NACA 23012 standard aviation formula
        yt = 5 * t * (0.2969 * Math.sqrt(x) - 0.126 * x - 0.3516 * x * x + 0.2843 * Math.pow(x, 3) - 0.1015 * Math.pow(x, 4));
        const m_5 = 0.15; // Mean camber location
        const k1 = 15.957;
        if (x < m_5) {
          yc = (k1 / 6) * (Math.pow(x, 3) - 3 * m_5 * x * x + m_5 * m_5 * (3 - m_5) * x);
          dyc = (k1 / 6) * (3 * x * x - 6 * m_5 * x + m_5 * m_5 * (3 - m_5));
        } else {
          yc = ((k1 * Math.pow(m_5, 3)) / 6) * (1 - x);
          dyc = -((k1 * Math.pow(m_5, 3)) / 6);
        }
        break;
      }

      case 'naca4':
      default: {
        // Standard NACA 4-digit thickness
        yt = 5 * t * (0.2969 * Math.sqrt(x) - 0.126 * x - 0.3516 * x * x + 0.2843 * Math.pow(x, 3) - 0.1036 * Math.pow(x, 4));

        // Mean camber line
        if (m > 1e-5 && p > 0.05 && p < 0.95) {
          if (x < p) {
            yc = (m / (p * p)) * (2 * p * x - x * x);
            dyc = ((2 * m) / (p * p)) * (p - x);
          } else {
            yc = (m / Math.pow(1 - p, 2)) * (1 - 2 * p + 2 * p * x - x * x);
            dyc = ((2 * m) / Math.pow(1 - p, 2)) * (p - x);
          }
        } else {
          yc = 0;
          dyc = 0;
        }
        break;
      }
    }

    // Kinematic Flap Deflection
    if (x > flapHinge && Math.abs(params.flapAngle) > 0.05) {
      const dx = x - flapHinge;
      if (params.flapType === 'fowler') {
        // Fowler flap moves backwards and downwards
        yc -= dx * Math.sin(flapRad) * 1.25;
        dyc -= Math.sin(flapRad) * 1.25;
      } else if (params.flapType === 'slotted') {
        yc -= dx * Math.sin(flapRad) * 1.05;
        dyc -= Math.sin(flapRad) * 1.05;
      } else {
        yc -= dx * Math.sin(flapRad) * 0.85;
        dyc -= Math.sin(flapRad) * 0.85;
      }
    }

    // Kinematic Slat Deflection (Leading-edge droop)
    if (x < slatHinge && params.slatAngle > 0.05) {
      const dx = slatHinge - x;
      yc -= dx * Math.sin(slatRad) * 0.8;
      dyc += Math.sin(slatRad) * 0.8;
    }

    const theta = Math.atan(dyc);
    const xu = Math.max(0, Math.min(1, x - yt * Math.sin(theta)));
    const yu = yc + yt * Math.cos(theta);
    const xl = Math.max(0, Math.min(1, x + yt * Math.sin(theta)));
    const yl = yc - yt * Math.cos(theta);

    upper.push({ x: xu, y: yu });
    lower.push({ x: xl, y: yl });
    camberLine.push({ x, y: yc });
    thicknessDist.push({ x, y: 2 * yt });
  }

  return {
    upper,
    lower,
    camberLine,
    thicknessDist,
    chord: 1.0,
    maxThickness: t,
    maxThicknessX: p > 0 ? p : 0.3,
    maxCamber: m,
    maxCamberX: p,
    leadingEdgeRadius: 1.1019 * t * t,
    trailingEdgeAngle: 14.5,
  };
}

export function parseAirfoilFile(
  content: string,
  filename: string
): { name: string; upper: Point2D[]; lower: Point2D[] } | null {
  const ext = filename.split('.').pop()?.toLowerCase() || '';

  // DXF Parsing
  if (ext === 'dxf') {
    const lines = content.split(/\r?\n/).map((l) => l.trim());
    let inEnt = false;
    let cx: number | null = null;
    const pts: Point2D[] = [];

    for (let i = 0; i < lines.length - 1; i++) {
      if (lines[i] === '2' && lines[i + 1] === 'ENTITIES') {
        inEnt = true;
        i += 2;
        continue;
      }
      if (inEnt && lines[i] === '0') {
        if (lines[i + 1] === 'ENDSEC') break;
      }
      if (inEnt) {
        const c = parseInt(lines[i], 10);
        const v = parseFloat(lines[i + 1]);
        if (c === 10 || c === 11) cx = v;
        if ((c === 20 || c === 21) && cx !== null && !isNaN(v)) {
          pts.push({ x: cx, y: v });
          cx = null;
        }
      }
    }

    if (pts.length < 10) return null;

    // Normalize coordinates
    let minX = Infinity,
      maxX = -Infinity,
      minY = Infinity,
      maxY = -Infinity;
    pts.forEach((p) => {
      minX = Math.min(minX, p.x);
      maxX = Math.max(maxX, p.x);
      minY = Math.min(minY, p.y);
      maxY = Math.max(maxY, p.y);
    });

    const chordLen = Math.max(0.001, maxX - minX);
    const normPts = pts.map((p) => ({
      x: (p.x - minX) / chordLen,
      y: (p.y - (minY + maxY) * 0.5) / chordLen,
    }));

    // Split upper and lower
    normPts.sort((a, b) => a.x - b.x);
    const midIdx = Math.floor(normPts.length / 2);
    return {
      name: filename.replace(/\.[^/.]+$/, '').toUpperCase(),
      upper: normPts.filter((p) => p.y >= 0),
      lower: normPts.filter((p) => p.y < 0),
    };
  }

  // Selig / Lednicer .DAT Parsing
  const lines = content
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l.length > 0 && !l.startsWith('#'));

  if (lines.length < 10) return null;

  const name = lines[0].slice(0, 24).trim();
  const pts: Point2D[] = [];

  for (let i = 1; i < lines.length; i++) {
    const parts = lines[i].split(/\s+/);
    if (parts.length >= 2) {
      const x = parseFloat(parts[0]);
      const y = parseFloat(parts[1]);
      if (!isNaN(x) && !isNaN(y)) {
        pts.push({ x, y });
      }
    }
  }

  if (pts.length < 10) return null;

  // Detect whether format is Selig (single loop from TE around LE to TE) or Lednicer (upper then lower)
  let minXIdx = 0;
  let minX = pts[0].x;
  for (let i = 1; i < pts.length; i++) {
    if (pts[i].x < minX) {
      minX = pts[i].x;
      minXIdx = i;
    }
  }

  let upperRaw: Point2D[] = [];
  let lowerRaw: Point2D[] = [];

  if (minXIdx > 0 && minXIdx < pts.length - 1) {
    // Selig loop: TE -> LE -> TE
    const loop1 = pts.slice(0, minXIdx + 1);
    const loop2 = pts.slice(minXIdx);
    const avgY1 = loop1.reduce((s, p) => s + p.y, 0) / loop1.length;
    const avgY2 = loop2.reduce((s, p) => s + p.y, 0) / loop2.length;

    if (avgY1 > avgY2) {
      upperRaw = [...loop1].reverse();
      lowerRaw = loop2;
    } else {
      upperRaw = [...loop2].reverse();
      lowerRaw = loop1;
    }
  } else {
    // Separate blocks
    const mid = Math.floor(pts.length / 2);
    upperRaw = pts.slice(0, mid);
    lowerRaw = pts.slice(mid);
  }

  // Normalize
  const maxXU = Math.max(...upperRaw.map((p) => p.x));
  const maxXL = Math.max(...lowerRaw.map((p) => p.x));
  const maxChord = Math.max(1e-4, Math.max(maxXU, maxXL));

  const upper = upperRaw.map((p) => ({ x: Math.max(0, Math.min(1, p.x / maxChord)), y: p.y / maxChord }));
  const lower = lowerRaw.map((p) => ({ x: Math.max(0, Math.min(1, p.x / maxChord)), y: p.y / maxChord }));

  upper.sort((a, b) => a.x - b.x);
  lower.sort((a, b) => a.x - b.x);

  return {
    name: name || filename.replace(/\.[^/.]+$/, '').toUpperCase(),
    upper,
    lower,
  };
}
