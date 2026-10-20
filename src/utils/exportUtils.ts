import { AeroResults, AirfoilGeometry, SimState } from '../types';

export function exportOBJ(
  vertices: Float32Array,
  indices: Uint16Array | Uint32Array,
  profileName: string
): void {
  let obj = `# NASA Glenn FoilSim 3D PRO - Wing Mesh\n`;
  obj += `# Profile: ${profileName}\n`;
  obj += `# Vertices: ${vertices.length / 3}\n`;
  obj += `# Triangles: ${indices.length / 3}\n\n`;

  for (let i = 0; i < vertices.length; i += 3) {
    obj += `v ${vertices[i].toFixed(5)} ${vertices[i + 1].toFixed(5)} ${vertices[i + 2].toFixed(5)}\n`;
  }

  obj += `\ns 1\n`;

  for (let i = 0; i < indices.length; i += 3) {
    obj += `f ${indices[i] + 1} ${indices[i + 1] + 1} ${indices[i + 2] + 1}\n`;
  }

  downloadBlob(obj, `Wing3D_${sanitizeFilename(profileName)}.obj`, 'text/plain');
}

export function exportSTL(
  vertices: Float32Array,
  indices: Uint16Array | Uint32Array,
  profileName: string
): void {
  let stl = `solid ${sanitizeFilename(profileName)}\n`;

  for (let i = 0; i < indices.length; i += 3) {
    const i1 = indices[i] * 3;
    const i2 = indices[i + 1] * 3;
    const i3 = indices[i + 2] * 3;

    const v1 = [vertices[i1], vertices[i1 + 1], vertices[i1 + 2]];
    const v2 = [vertices[i2], vertices[i2 + 1], vertices[i2 + 2]];
    const v3 = [vertices[i3], vertices[i3 + 1], vertices[i3 + 2]];

    // Normal calculation
    const ax = v2[0] - v1[0], ay = v2[1] - v1[1], az = v2[2] - v1[2];
    const bx = v3[0] - v1[0], by = v3[1] - v1[1], bz = v3[2] - v1[2];
    let nx = ay * bz - az * by;
    let ny = az * bx - ax * bz;
    let nz = ax * by - ay * bx;
    const len = Math.hypot(nx, ny, nz) || 1;
    nx /= len; ny /= len; nz /= len;

    stl += `  facet normal ${nx.toFixed(5)} ${ny.toFixed(5)} ${nz.toFixed(5)}\n`;
    stl += `    outer loop\n`;
    stl += `      vertex ${v1[0].toFixed(5)} ${v1[1].toFixed(5)} ${v1[2].toFixed(5)}\n`;
    stl += `      vertex ${v2[0].toFixed(5)} ${v2[1].toFixed(5)} ${v2[2].toFixed(5)}\n`;
    stl += `      vertex ${v3[0].toFixed(5)} ${v3[1].toFixed(5)} ${v3[2].toFixed(5)}\n`;
    stl += `    endloop\n`;
    stl += `  endfacet\n`;
  }

  stl += `endsolid ${sanitizeFilename(profileName)}\n`;
  downloadBlob(stl, `Wing3D_${sanitizeFilename(profileName)}.stl`, 'text/plain');
}

export function exportDAT(geom: AirfoilGeometry, profileName: string): void {
  let dat = `${profileName} - Selig DAT Format\n`;

  // Upper surface from TE (1.0) to LE (0.0)
  for (let i = geom.upper.length - 1; i >= 0; i--) {
    dat += `  ${geom.upper[i].x.toFixed(6)}   ${geom.upper[i].y.toFixed(6)}\n`;
  }
  // Lower surface from LE (0.0) to TE (1.0)
  for (let i = 1; i < geom.lower.length; i++) {
    dat += `  ${geom.lower[i].x.toFixed(6)}   ${geom.lower[i].y.toFixed(6)}\n`;
  }

  downloadBlob(dat, `${sanitizeFilename(profileName)}.dat`, 'text/plain');
}

export function exportCSV(
  state: SimState,
  solveFn: (alpha: number) => AeroResults
): void {
  let csv = `# NASA Glenn FoilSim 3D PRO - Aerodynamic Polar Data\n`;
  csv += `# Profile,${state.profileName},Span(m),${state.span},Chord(m),${state.chord},Sweep(deg),${state.sweep},Taper,${state.taper}\n`;
  csv += `# Planet,${state.planet},Altitude(m),${state.altitude},Velocity(m/s),${state.velocity},Flap(deg),${state.flapAngle}\n`;
  csv += `Alpha(deg),CL,CD,CD0,CDi,CDwave,CDstall,L_D,Cm_c4,Xcp(%),Lift(N),Drag(N),Re,Mach\n`;

  for (let alpha = -15; alpha <= 25; alpha += 1) {
    const res = solveFn(alpha);
    csv += `${alpha},${res.CL.toFixed(4)},${res.CD.toFixed(5)},${res.CD0.toFixed(5)},${res.CDi.toFixed(5)},${res.CDwave.toFixed(5)},${res.CDstall.toFixed(5)},${res.LD.toFixed(2)},${res.Cm.toFixed(4)},${(res.xcp * 100).toFixed(1)},${res.Lift.toFixed(1)},${res.Drag.toFixed(1)},${res.Reynolds.toExponential(3)},${res.Mach.toFixed(3)}\n`;
  }

  downloadBlob(csv, `Polar_${sanitizeFilename(state.profileName)}.csv`, 'text/csv');
}

export function exportStateJSON(state: SimState): void {
  const json = JSON.stringify(state, null, 2);
  downloadBlob(json, `GLENN_Simulation_${sanitizeFilename(state.profileName)}.json`, 'application/json');
}

function sanitizeFilename(name: string): string {
  return name.replace(/[^a-zA-Z0-9_\-]/g, '_');
}

function downloadBlob(content: string, filename: string, mimeType: string): void {
  const blob = new Blob([content], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}
