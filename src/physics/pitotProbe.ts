import { AtmosphereData, PitotProbeData, SimState } from '../types';

export function samplePitotProbe(
  probePos: { x: number; y: number; z: number },
  state: SimState,
  atmo: AtmosphereData,
  CL: number,
  stallFactor: number
): PitotProbeData {
  const V_inf = Math.max(0.1, state.velocity);
  const rho = atmo.density;
  const p_inf = atmo.pressure;
  const a_sound = atmo.speedOfSound;

  const px = probePos.x;
  const py = probePos.y;
  const pz = probePos.z;

  // Wing span boundary
  const halfSpan = (state.span / state.chord) * 0.48;
  const isInsideSpan = Math.abs(pz) <= halfSpan * 1.1;

  let vRatio = 1.0;

  if (isInsideSpan) {
    // Distance from quarter-chord (0, 0)
    const distLE = Math.hypot(px + 0.25, py);
    const distChord = Math.hypot(px, py);

    // Stagnation region around leading edge (-0.25, 0)
    if (distLE < 0.2) {
      vRatio = Math.max(0.05, distLE / 0.2);
    } else if (px >= -0.25 && px <= 0.8) {
      // Flow over upper or lower surface
      const camberLiftBoost = CL * 0.25;
      if (py > 0.02 && py < 0.5) {
        // Upper suction region (flow accelerated)
        const decay = Math.exp(-py * 4.0);
        if (stallFactor > 0.4 && px > 0.1) {
          // Separated wake behind wing: turbulent deficit
          vRatio = Math.max(0.2, 0.45 + 0.2 * Math.sin(px * 10));
        } else {
          vRatio = 1.0 + (0.35 + camberLiftBoost) * decay;
        }
      } else if (py < -0.02 && py > -0.5) {
        // Lower compression region (flow decelerated)
        const decay = Math.exp(-Math.abs(py) * 4.0);
        vRatio = Math.max(0.65, 1.0 - (0.2 + camberLiftBoost * 0.5) * decay);
      }
    } else if (px > 0.8 && Math.abs(py) < 0.3) {
      // Trailing wake deficit
      const wakeWidth = 0.15 + stallFactor * 0.35;
      const wakeDecay = Math.exp(-Math.pow(py / wakeWidth, 2));
      const deficit = (0.25 + stallFactor * 0.55) * Math.exp(-(px - 0.8) * 0.8);
      vRatio = Math.max(0.1, 1.0 - deficit * wakeDecay);
    }
  }

  const localV = V_inf * vRatio;
  const localMach = localV / a_sound;
  const localQ = 0.5 * rho * localV * localV;

  // Local static pressure from Bernoulli: p_s = p_inf + q_inf - q_local
  const q_inf = 0.5 * rho * V_inf * V_inf;
  const staticPressure = Math.max(10, p_inf + (q_inf - localQ));

  // Total pitot pressure: p_total = p_s + q_local (with supersonic Rayleigh pitot correction)
  let totalPressure: number;
  if (localMach > 1.0) {
    // Normal shock Rayleigh formula
    const gamma = 1.4;
    const m2 = localMach * localMach;
    const term1 = ((gamma + 1) * m2) / (2 + (gamma - 1) * m2);
    const term2 = (gamma + 1) / (2 * gamma * m2 - (gamma - 1));
    const p02_over_p1 = Math.pow(term1, gamma / (gamma - 1)) * Math.pow(term2, 1 / (gamma - 1));
    totalPressure = staticPressure * p02_over_p1;
  } else {
    totalPressure = staticPressure + localQ;
  }

  return {
    x: px,
    y: py,
    z: pz,
    localVelocity: localV,
    localMach,
    staticPressure,
    totalPressure,
    dynamicPressure: localQ,
  };
}
