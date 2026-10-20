import { AtmosphereData, PlanetType } from '../types';

export function calculateAtmosphere(
  planet: PlanetType,
  altitudeMeters: number,
  velocityMps: number,
  customDensity?: number,
  customSpeedOfSound?: number
): AtmosphereData {
  const h = Math.max(0, altitudeMeters);

  if (planet === 'water') {
    const rho = 998.2; // kg/m^3
    const T = 293.15; // 20 C
    const p = 101325 + rho * 9.80665 * h; // Hydrostatic pressure with depth
    const a = 1481; // m/s speed of sound in water
    const mu = 1.002e-3; // Pa*s
    const q = 0.5 * rho * velocityMps * velocityMps;

    return {
      name: 'Водная среда (Гидродинамика)',
      altitude: h,
      temperature: T,
      pressure: p,
      density: rho,
      speedOfSound: a,
      dynamicViscosity: mu,
      dynamicPressure: q,
      gravity: 9.80665,
    };
  }

  if (planet === 'custom') {
    const rho = customDensity && customDensity > 0 ? customDensity : 1.225;
    const a = customSpeedOfSound && customSpeedOfSound > 0 ? customSpeedOfSound : 340.29;
    const T = 288.15;
    const p = rho * 287.058 * T;
    const mu = 1.789e-5;
    const q = 0.5 * rho * velocityMps * velocityMps;

    return {
      name: 'Пользовательская среда',
      altitude: h,
      temperature: T,
      pressure: p,
      density: rho,
      speedOfSound: a,
      dynamicViscosity: mu,
      dynamicPressure: q,
      gravity: 9.80665,
    };
  }

  if (planet === 'mars') {
    // Mars atmospheric model based on NASA Glenn Mars Atmosphere equations
    const T = Math.max(130, 210.15 - 0.000998 * h);
    const p = 699 * Math.exp(-0.00009 * h);
    const R_mars = 192.1; // J/(kg*K) for CO2
    const gamma_mars = 1.29;
    const rho = Math.max(0.0001, p / (R_mars * T));
    const a = Math.sqrt(gamma_mars * R_mars * T);
    // Sutherland approximation for CO2
    const mu = 1.48e-5 * Math.pow(T / 293.15, 1.5) * ((293.15 + 240) / (T + 240));
    const q = 0.5 * rho * velocityMps * velocityMps;

    return {
      name: 'Марс (Разреженная CO₂)',
      altitude: h,
      temperature: T,
      pressure: p,
      density: rho,
      speedOfSound: a,
      dynamicViscosity: mu,
      dynamicPressure: q,
      gravity: 3.721,
    };
  }

  if (planet === 'venus') {
    // Venus atmospheric model: extreme surface density and pressure
    const lapse = 0.0077;
    const T = Math.max(220, 737 - lapse * h);
    const p = 9.2e6 * Math.pow(Math.max(0.01, T / 737), 5.4);
    const R_venus = 188.9;
    const gamma_venus = 1.28;
    const rho = Math.max(0.01, p / (R_venus * T));
    const a = Math.sqrt(gamma_venus * R_venus * T);
    const mu = 2.9e-5 * Math.pow(T / 737, 0.7);
    const q = 0.5 * rho * velocityMps * velocityMps;

    return {
      name: 'Венера (Сверхкритическая плотная)',
      altitude: h,
      temperature: T,
      pressure: p,
      density: rho,
      speedOfSound: a,
      dynamicViscosity: mu,
      dynamicPressure: q,
      gravity: 8.87,
    };
  }

  if (planet === 'titan') {
    // Titan cryogenic nitrogen atmosphere
    const T = Math.max(70, 94 - 0.0006 * h);
    const p = 146700 * Math.exp(-h / 21000);
    const R_titan = 296.8;
    const gamma_titan = 1.4;
    const rho = Math.max(0.05, p / (R_titan * T));
    const a = Math.sqrt(gamma_titan * R_titan * T);
    const mu = 7.0e-6; // Cold dense N2
    const q = 0.5 * rho * velocityMps * velocityMps;

    return {
      name: 'Титан (Криогенная плотная N₂)',
      altitude: h,
      temperature: T,
      pressure: p,
      density: rho,
      speedOfSound: a,
      dynamicViscosity: mu,
      dynamicPressure: q,
      gravity: 1.352,
    };
  }

  // Standard Earth ISA 1976
  let T: number;
  let p: number;
  const T0 = 288.15;
  const p0 = 101325;
  const g = 9.80665;
  const R = 287.058;
  const gamma = 1.4;

  if (h <= 11000) {
    // Troposphere
    const L = 0.0065;
    T = T0 - L * h;
    p = p0 * Math.pow(T / T0, g / (R * L));
  } else if (h <= 20000) {
    // Tropopause (isothermal)
    const T11 = 216.65;
    const p11 = 22632.1;
    T = T11;
    p = p11 * Math.exp((-g * (h - 11000)) / (R * T11));
  } else {
    // Stratosphere
    const T20 = 216.65;
    const p20 = 5474.89;
    const L_strat = -0.001; // Temperature rises with altitude
    T = T20 - L_strat * (h - 20000);
    p = p20 * Math.pow(T / T20, -g / (R * L_strat));
  }

  const rho = p / (R * T);
  const a = Math.sqrt(gamma * R * T);

  // Sutherland's law for dynamic viscosity
  const mu0 = 1.716e-5;
  const S_const = 110.4;
  const mu = (mu0 * Math.pow(T / 273.15, 1.5) * (273.15 + S_const)) / (T + S_const);
  const q = 0.5 * rho * velocityMps * velocityMps;

  return {
    name: 'Земля (Стандартная атмосфера ISA-1976)',
    altitude: h,
    temperature: T,
    pressure: p,
    density: rho,
    speedOfSound: a,
    dynamicViscosity: mu,
    dynamicPressure: q,
    gravity: g,
  };
}
