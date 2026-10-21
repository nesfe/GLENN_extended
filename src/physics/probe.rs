//! Illustrative near-wing probe; not atmosphere resolved 3D flow-field solver.
use super::aerodynamics::Analysis;
use crate::model::Planet;
#[derive(Debug, Clone, Copy)]
pub struct Probe {
    pub speed: f64,
    pub mach: f64,
    pub static_pressure: f64,
    pub total_pressure: f64,
    pub dynamic_pressure: f64,
}
/// Isentropic subsonic pressure and Rayleigh pitot pressure behind atmosphere normal shock.
pub fn total_pressure_ratio(mach: f64, gamma: f64) -> f64 {
    if mach <= 1.0 {
        (1.0 + 0.5 * (gamma - 1.0) * mach * mach).powf(gamma / (gamma - 1.0))
    } else {
        (((gamma + 1.0).powi(2) * mach * mach) / (4.0 * gamma * mach * mach - 2.0 * (gamma - 1.0)))
            .powf(gamma / (gamma - 1.0))
            * (1.0 - gamma + 2.0 * gamma * mach * mach)
            / (gamma + 1.0)
    }
}
pub fn sample(analysis: &Analysis, position: [f64; 3]) -> Probe {
    let state = &analysis.state;
    let atmosphere = &analysis.atmosphere;
    let aero = &analysis.current;
    let [x, y, z] = position;
    let mut ratio = 1.0;
    if z.abs() < state.span / state.chord * 0.5 {
        if (-0.25..=0.75).contains(&x) {
            ratio += (0.3 + 0.2 * aero.cl) * (-4.0 * y.abs()).exp() * y.signum();
        } else if x > 0.75 {
            ratio -= (0.2 + 0.5 * aero.stall_fraction)
                * (-(x - 0.75)).exp()
                * (-(y / 0.3).powi(2)).exp();
        }
    }
    let speed = state.velocity * ratio.clamp(0.1, 2.5);
    let mach = speed / atmosphere.sound_speed;
    let dynamic_pressure = 0.5 * atmosphere.density * speed * speed;
    let static_pressure =
        (atmosphere.pressure + atmosphere.dynamic_pressure - dynamic_pressure).max(1.0);
    let total_pressure = if state.planet == Planet::Water {
        static_pressure + dynamic_pressure
    } else {
        static_pressure * total_pressure_ratio(mach, atmosphere.gamma)
    };
    Probe {
        speed,
        mach,
        static_pressure,
        total_pressure,
        dynamic_pressure,
    }
}
