//! Earth ISA layers and deliberately simple planetary atmosphere approximations.
use crate::model::{Planet, Simulation};
#[derive(Debug, Clone, Copy)]
pub struct Atmosphere {
    pub temperature: f64,
    pub pressure: f64,
    pub density: f64,
    pub sound_speed: f64,
    pub viscosity: f64,
    pub gravity: f64,
    pub gamma: f64,
    pub dynamic_pressure: f64,
}
pub fn calculate(s: &Simulation) -> Atmosphere {
    let h = s.altitude;
    let (temperature, pressure, gas_constant, gamma, gravity, viscosity) = match s.planet {
        Planet::Earth => {
            let mut temperature: f64 = 288.15;
            let mut pressure: f64 = 101325.0;
            let gas_constant = 287.05287;
            // Integrate through every base layer so pressure is continuous at boundaries.
            for (base, top, lapse) in [
                (0.0, 11000.0, -0.0065),
                (11000.0, 20000.0, 0.0),
                (20000.0, 32000.0, 0.001),
                (32000.0, 47000.0, 0.0028),
                (47000.0, 51000.0, 0.0),
                (51000.0, 71000.0, -0.0028),
                (71000.0, 84852.0, -0.002),
            ] {
                let dh = (h.min(top) - base).max(0.0);
                if dh == 0.0 {
                    break;
                }
                let next = temperature + lapse * dh;
                pressure *= if lapse == 0.0 {
                    (-9.80665 * dh / (gas_constant * temperature)).exp()
                } else {
                    (next / temperature).powf(-9.80665 / (gas_constant * lapse))
                };
                temperature = next;
            }
            let mu = 1.716e-5 * (temperature / 273.15).powf(1.5) * (273.15 + 110.4)
                / (temperature + 110.4);
            (temperature, pressure, gas_constant, 1.4, 9.80665, mu)
        }
        Planet::Mars => {
            let t = (210.15 - 0.000998 * h).max(130.0);
            (
                t,
                699.0 * (-0.00009 * h).exp(),
                192.1,
                1.29,
                3.721,
                1.48e-5 * (t / 293.15).powf(1.5) * (293.15 + 240.0) / (t + 240.0),
            )
        }
        Planet::Venus => {
            let t = (737.0 - 0.0077 * h).max(220.0);
            (
                t,
                9.2e6 * (-h / 15900.0).exp(),
                188.9,
                1.28,
                8.87,
                2.9e-5 * (t / 737.0).powf(0.7),
            )
        }
        Planet::Titan => (
            (94.0 - 0.0006 * h).max(70.0),
            146700.0 * (-h / 21000.0).exp(),
            296.8,
            1.4,
            1.352,
            7e-6,
        ),
        Planet::Water => (
            293.15,
            101325.0 + 998.2 * 9.80665 * h,
            287.05287,
            1.4,
            9.80665,
            1.002e-3,
        ),
        Planet::Custom => (
            288.15,
            s.custom_density * 287.05287 * 288.15,
            287.05287,
            1.4,
            9.80665,
            1.789e-5,
        ),
    };
    let density = if s.planet == Planet::Water {
        998.2
    } else {
        pressure / (gas_constant * temperature)
    };
    let sound_speed = match s.planet {
        Planet::Water => 1481.0,
        Planet::Custom => s.custom_speed_of_sound,
        _ => (gamma * gas_constant * temperature).sqrt(),
    };
    Atmosphere {
        temperature,
        pressure,
        density,
        sound_speed,
        viscosity,
        gravity,
        gamma,
        dynamic_pressure: 0.5 * density * s.velocity.powi(2),
    }
}
