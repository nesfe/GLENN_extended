import { AeroResults, AirfoilGeometry, AtmosphereData, CpPoint, SimState } from '../types';
import { solvePanelMethod } from './panelMethod';

export function solveAerodynamics(
  geom: AirfoilGeometry,
  state: SimState,
  atmo: AtmosphereData
): AeroResults {
  const V = Math.max(0.1, state.velocity);
  const rho = atmo.density;
  const aSound = atmo.speedOfSound;
  const mu = atmo.dynamicViscosity;
  const q = 0.5 * rho * V * V;

  // Wing geometry parameters
  const b = Math.max(0.2, state.span);
  const cRoot = Math.max(0.1, state.chord);
  const taper = Math.max(0.1, Math.min(1.0, state.taper));
  const cTip = cRoot * taper;
  const cMean = (2 / 3) * cRoot * ((1 + taper + taper * taper) / (1 + taper));
  const S = 0.5 * b * cRoot * (1 + taper);
  const AR = (b * b) / S;

  const sweepRad = (state.sweep * Math.PI) / 180;
  const twistRad = (state.twist * Math.PI) / 180;
  const alphaRad = (state.alpha * Math.PI) / 180;

  // Flow non-dimensional numbers
  const Mach = V / aSound;
  const effMach = Mach * Math.cos(sweepRad);
  const Re = Math.max(1000, (rho * V * cMean) / mu);

  // High-lift devices contributions
  const flapRad = (state.flapAngle * Math.PI) / 180;
  const slatRad = (state.slatAngle * Math.PI) / 180;
  const flapLiftCoeff = state.flapType === 'fowler' ? 1.4 : state.flapType === 'slotted' ? 1.15 : 0.85;
  const dCL_flap = Math.sin(flapRad) * flapLiftCoeff;
  const dCL_slat = Math.sin(slatRad) * 0.45;

  // Magnus effect for rotating cylinder
  let cylinderLift = 0;
  let cylinderDrag = 0;
  if (state.profileType === 'cylinder' && Math.abs(state.cylinderRpm) > 1) {
    // Magnus lift: Cl = 2 * pi * (v_surface / V_inf)
    const radius = 0.5 * cMean;
    const omega = (state.cylinderRpm * 2 * Math.PI) / 60;
    const vSurf = omega * radius;
    const spinRatio = Math.max(-4, Math.min(4, vSurf / V));
    cylinderLift = 2 * Math.PI * spinRatio;
    cylinderDrag = 0.4 + 0.3 * Math.abs(spinRatio);
  }

  // 2D Section Panel Method Analysis
  const panelRes = solvePanelMethod(geom.upper, geom.lower, state.alpha, effMach);

  // Prandtl-Glauert / Subsonic compressibility
  const betaPG = Math.sqrt(Math.max(0.12, 1 - effMach * effMach));

  // Helmbold-Diederich 3D lift curve slope formula for swept wings:
  // a3D = (a0 * cos(sweep)) / sqrt( 1 + [ (a0 * cos(sweep)) / (pi * AR) ]^2 ) + (a0 * cos(sweep)) / (pi * AR)
  const t = geom.maxThickness;
  const m = geom.maxCamber;
  const a0_2d = (2 * Math.PI * (1 + 0.77 * t)) / betaPG;
  const sweepCos = Math.cos(sweepRad);
  const term = (a0_2d * sweepCos) / (Math.PI * AR);
  const a3D = (a0_2d * sweepCos) / (Math.sqrt(1 + term * term) + term);

  // Zero-lift angle of attack
  const alpha0_base = -1.15 * Math.PI * m;
  const dAlpha0_flap = -0.55 * flapRad * flapLiftCoeff;
  const dAlpha0_twist = -0.4 * twistRad;
  const alpha0 = alpha0_base + dAlpha0_flap + dAlpha0_twist;

  // Effective angle of attack accounting for washout twist
  const effAlpha = alphaRad - alpha0;

  // Stall prediction
  // Slats delay stall, flaps increase Cl_max but may reduce stall angle
  const stallPosDeg = 14.0 + 22.0 * t + 12.0 * Math.max(0, m) - 0.1 * Math.max(0, state.flapAngle) + 0.35 * state.slatAngle - 0.25 * state.twist;
  const stallNegDeg = -13.0 - 20.0 * t;

  let stallFactor = 0;
  if (state.alpha > stallPosDeg - 2.5) {
    stallFactor = 1 / (1 + Math.exp(-(state.alpha - stallPosDeg) * 1.1));
  } else if (state.alpha < stallNegDeg + 2.5) {
    stallFactor = 1 / (1 + Math.exp((state.alpha - stallNegDeg) * 1.1));
  }

  const isStalled = stallFactor > 0.4;
  const stallState: 'attached' | 'incipient' | 'stalled' =
    stallFactor > 0.4 ? 'stalled' : stallFactor > 0.12 ? 'incipient' : 'attached';

  // 3D Lift Coefficient
  let CL: number;
  if (state.profileType === 'cylinder') {
    CL = cylinderLift;
  } else if (Mach > 1.2) {
    // Supersonic Ackeret rule
    const betaSup = Math.sqrt(Mach * Mach - 1);
    CL = (4 * alphaRad) / betaSup;
  } else {
    const clLinear = a3D * Math.sin(effAlpha) + dCL_flap * 0.75 + dCL_slat * 0.6;
    // Post-stall Kirchhoff cross-flow separation model
    const clStalled = 1.95 * Math.sin(alphaRad) * Math.cos(alphaRad);
    CL = (1 - stallFactor) * clLinear + stallFactor * clStalled;
  }

  // Oswald efficiency factor e with winglet boost
  const wingletBoost = state.hasWinglets ? 0.18 : 0.0;
  const sweepEffect = Math.cos(sweepRad * 0.65);
  const oswaldE = Math.max(
    0.55,
    Math.min(0.98, (1.78 * (1 - 0.045 * Math.pow(AR, 0.68)) - 0.64) * sweepEffect * (1 + wingletBoost))
  );

  // Induced drag coefficient
  const CDi = (CL * CL) / (Math.PI * AR * oswaldE);

  // Parasite zero-lift drag (skin friction + form factor)
  const cf = 0.074 / Math.pow(Re, 0.2); // Turbulent flat plate
  const formFactor = 1 + 2 * t + 60 * Math.pow(t, 4);
  const flapDragIncrement = 0.00016 * Math.pow(state.flapAngle, 2) * (state.flapType === 'fowler' ? 1.3 : 1.0);
  const slatDragIncrement = 0.00012 * Math.pow(state.slatAngle, 2);
  const CD0 = state.profileType === 'cylinder'
    ? cylinderDrag
    : 2 * cf * formFactor + flapDragIncrement + slatDragIncrement;

  // Compressibility wave drag
  // Critical Mach number estimation from Korn's equation: Mcr + 0.1*Cl + t/c approx 0.87
  const MachCrit = Math.max(0.4, Math.min(0.88, 0.87 - 0.1 * Math.abs(CL) - t));
  let CDwave = 0;
  if (Mach > MachCrit && Mach <= 1.05) {
    CDwave = 18.0 * Math.pow(Mach - MachCrit, 3.5);
  } else if (Mach > 1.05) {
    // Supersonic wave drag
    const betaSup = Math.sqrt(Mach * Mach - 1);
    CDwave = (4 * alphaRad * alphaRad + (16 / 3) * t * t) / betaSup;
  }

  // Stall drag increment
  const CDstall = stallFactor * 1.75 * Math.pow(Math.sin(alphaRad), 2);

  // Total Drag
  const CD = Math.max(0.002, CD0 + CDi + CDwave + CDstall);
  const LD = CD > 1e-6 ? CL / CD : 0;

  // Dimensional Forces
  const Lift = q * S * CL;
  const Drag = q * S * CD;

  // Pitching moment about quarter chord
  const cm0 = -0.45 * Math.PI * m - 0.004 * state.flapAngle;
  const cmAlpha = -0.05 * (state.alpha * Math.PI) / 180; // Stable restoring pitch moment
  const cmStall = -0.08 * stallFactor * Math.sign(state.alpha);
  const Cm = cm0 + cmAlpha + cmStall;
  const PitchingMoment = q * S * cMean * Cm;

  // Center of pressure (x/c)
  let xcp = 0.25;
  if (Math.abs(CL) > 0.05) {
    xcp = 0.25 - Cm / CL;
    xcp = Math.max(0.08, Math.min(0.85, xcp));
  }

  // Spanwise lift and circulation distribution calculation (Prandtl Lifting-Line)
  const numSpanStations = 33;
  const spanwiseY: number[] = [];
  const spanwiseLift: number[] = [];
  const spanwiseCirculation: number[] = [];
  const spanwiseDownwash: number[] = [];

  for (let s = 0; s < numSpanStations; s++) {
    // eta from -1 to 1 across wingspan
    const eta = (s / (numSpanStations - 1)) * 2 - 1;
    const y = (eta * b) / 2;
    spanwiseY.push(y);

    const absEta = Math.abs(eta);
    // Local chord
    const cLocal = cRoot * (1 - absEta * (1 - taper));
    // Elliptic baseline circulation profile
    const ellipFrac = Math.sqrt(Math.max(0, 1 - absEta * absEta));
    // Planform circulation with washout twist
    const localWashout = absEta * twistRad;
    const localCirc = Math.max(0, (ellipFrac * (CL + 0.15 * Math.sin(localWashout))) * (cLocal / cMean));
    spanwiseCirculation.push(localCirc);

    // Spanwise lift loading (N/m)
    const localLift = rho * V * localCirc * (b / 2);
    spanwiseLift.push(localLift);

    // Induced downwash w_i(y)
    const downwash = (localCirc * V) / (Math.PI * AR * Math.max(0.2, ellipFrac));
    spanwiseDownwash.push(downwash);
  }

  // Enhance panel Cp with flap suction peak and 3D tip relief
  const cpUpper: CpPoint[] = panelRes.cpUpper.map((p) => {
    let cp = p.cp;
    // Flap hinge suction peak
    if (p.x > 0.7 && Math.abs(state.flapAngle) > 2) {
      const peak = -0.03 * state.flapAngle * Math.exp(-Math.pow((p.x - 0.75) * 12, 2));
      cp += peak;
    }
    // Deep stall pressure flattening
    if (stallFactor > 0.1) {
      cp = (1 - stallFactor) * cp + stallFactor * -0.45;
    }
    return { x: p.x, cp: Math.max(-5.5, Math.min(1.0, cp)) };
  });

  const cpLower: CpPoint[] = panelRes.cpLower.map((p) => {
    let cp = p.cp;
    if (p.x > 0.7 && Math.abs(state.flapAngle) > 2) {
      const peak = 0.02 * state.flapAngle * Math.exp(-Math.pow((p.x - 0.75) * 12, 2));
      cp += peak;
    }
    return { x: p.x, cp: Math.max(-3.0, Math.min(1.0, cp)) };
  });

  return {
    CL,
    CD,
    CD0,
    CDi,
    CDwave,
    CDstall,
    LD,
    Cm,
    xcp,
    Lift,
    Drag,
    PitchingMoment,
    velocity: V,
    Mach,
    MachCrit,
    Reynolds: Re,
    dynamicPressure: q,
    wingArea: S,
    aspectRatio: AR,
    oswaldEfficiency: oswaldE,
    stallFactor,
    isStalled,
    stallState,
    boundaryLayerTransX: Math.max(0.05, Math.min(0.9, 0.45 - 0.015 * state.alpha)),
    stagnationPointX: panelRes.stagnationX,
    cpUpper,
    cpLower,
    cpMin: panelRes.cpMin,
    spanwiseY,
    spanwiseLift,
    spanwiseCirculation,
    spanwiseDownwash,
  };
}
