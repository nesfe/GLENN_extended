import { AtmosphereData, FlightPerformance, SimState } from '../types';

export function calculateFlightPerformance(
  state: SimState,
  atmo: AtmosphereData,
  clMax: number,
  cd0: number,
  oswaldE: number
): FlightPerformance {
  const g = atmo.gravity;
  const rho = atmo.density;
  const m = Math.max(10, state.aircraftMass);
  const W = m * g; // Total weight in Newtons

  const b = state.span;
  const cRoot = state.chord;
  const taper = state.taper;
  const S = 0.5 * b * cRoot * (1 + taper);
  const AR = (b * b) / S;

  // Maximum Lift Coefficient
  const Cl_max_usable = Math.max(0.8, clMax);

  // 1G Stall speed
  const stallSpeed = Math.sqrt((2 * W) / (rho * S * Cl_max_usable));

  // Minimum drag airspeed (Best glide speed)
  const k = 1 / (Math.PI * AR * oswaldE);
  const minDragSpeed = Math.sqrt((2 * W) / (rho * S)) * Math.pow(k / cd0, 0.25);
  const maxGlideRatio = 1 / (2 * Math.sqrt(cd0 * k));

  // Minimum power airspeed (Maximum endurance)
  const minPowerSpeed = minDragSpeed / Math.pow(3, 0.25);
  const minSinkRate = minDragSpeed / maxGlideRatio;

  // Available thrust calculation according to engine type and altitude
  const rho0 = 1.225;
  const densityRatio = Math.max(0.001, rho / rho0);
  const V_curr = Math.max(1.0, state.velocity);
  const Mach_curr = V_curr / atmo.speedOfSound;

  let thrustAvailable = state.maxThrust;

  switch (state.engineType) {
    case 'rocket': {
      // Rocket thrust is nearly constant, slightly higher in vacuum
      thrustAvailable = state.maxThrust * (1 + 0.12 * (1 - densityRatio));
      break;
    }
    case 'turbojet': {
      // High-speed military turbojet
      thrustAvailable =
        state.maxThrust * Math.pow(densityRatio, 0.9) * (1 + 0.3 * Mach_curr * Mach_curr);
      break;
    }
    case 'piston_prop': {
      // Constant power: P = eta * P_eng -> T = P / V
      const propEfficiency = 0.82;
      const ratedPowerWatts = state.maxThrust * 45; // Equivalent power
      const lapse = Math.max(0.05, densityRatio - 0.15);
      thrustAvailable = (ratedPowerWatts * lapse * propEfficiency) / Math.max(10, V_curr);
      break;
    }
    case 'electric_prop': {
      const propEfficiency = 0.85;
      const ratedPowerWatts = state.maxThrust * 40;
      thrustAvailable = (ratedPowerWatts * propEfficiency) / Math.max(10, V_curr);
      break;
    }
    case 'turbofan':
    default: {
      // High-bypass commercial turbofan
      thrustAvailable =
        state.maxThrust * Math.pow(densityRatio, 0.82) * Math.max(0.2, 1 - 0.32 * Mach_curr);
      break;
    }
  }

  // Drag at current airspeed
  const q_curr = 0.5 * rho * V_curr * V_curr;
  const Cl_level = W / (q_curr * S);
  const Cd_level = cd0 + k * Cl_level * Cl_level;
  const dragAtCurrentSpeed = q_curr * S * Cd_level;

  // Excess thrust and climb performance
  const excessThrust = thrustAvailable - dragAtCurrentSpeed;
  const maxClimbRate = Math.max(0, (excessThrust * V_curr) / W);
  const climbAngle = excessThrust > 0 ? (Math.asin(Math.min(0.95, excessThrust / W)) * 180) / Math.PI : 0;

  // Estimate maximum airspeed (where Thrust = Drag)
  let maxSpeed = stallSpeed * 1.2;
  for (let testV = minDragSpeed; testV <= 1200; testV += 5) {
    const q_test = 0.5 * rho * testV * testV;
    const cl_test = W / (q_test * S);
    const cd_test = cd0 + k * cl_test * cl_test;
    const drag_test = q_test * S * cd_test;
    let t_test = state.maxThrust;
    if (state.engineType === 'turbofan') {
      t_test *= Math.pow(densityRatio, 0.82) * Math.max(0.1, 1 - 0.32 * (testV / atmo.speedOfSound));
    }
    if (t_test < drag_test) {
      maxSpeed = testV;
      break;
    }
    maxSpeed = testV;
  }

  // Service ceiling estimation (altitude where RoC drops to 0.5 m/s)
  // Approximate based on thrust lapse with altitude
  const scaleHeight = 8500;
  const serviceCeiling = Math.min(35000, Math.max(0, Math.log(state.maxThrust / (W * 0.12)) * scaleHeight));

  // Breguet Range & Endurance
  const fuelMass = Math.max(1, state.fuelMass);
  const W_empty = Math.max(10, W - fuelMass * g);
  const weightRatio = Math.max(1.01, W / W_empty);
  const tsfc = Math.max(1e-6, state.tsfc); // kg / (N * s) or 1/s

  // Range R = (V / (g * c_t)) * (L/D) * ln(W0 / W1)
  const rangeMeters = (V_curr / (g * tsfc)) * maxGlideRatio * Math.log(weightRatio);
  const rangeKm = rangeMeters / 1000;

  // Endurance E = (1 / (g * c_t)) * (L/D) * ln(W0 / W1)
  const enduranceSeconds = (1 / (g * tsfc)) * maxGlideRatio * Math.log(weightRatio);
  const enduranceHours = enduranceSeconds / 3600;

  return {
    stallSpeed,
    minDragSpeed,
    minPowerSpeed,
    maxSpeed,
    maxGlideRatio,
    minSinkRate,
    maxClimbRate,
    climbAngle,
    serviceCeiling,
    thrustAvailable,
    dragAtCurrentSpeed,
    excessThrust,
    rangeKm,
    enduranceHours,
  };
}
