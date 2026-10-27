//! Probe thermodynamics driven by the same local velocity field as tracers.
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
/// Coordinates are in root-chord units, in the fixed tunnel frame. A probe
/// inside the solid has no fluid reading rather than a fabricated velocity.
pub fn sample(field: &super::flow::FlowField<'_>, position: [f64; 3]) -> Option<Probe> {
    let analysis = field.analysis;
    let state = &analysis.state;
    let atmosphere = &analysis.atmosphere;
    let world = glam::DVec3::from_array(position) * state.chord;
    if field.contains_solid(world) {
        return None;
    }
    let speed = state.velocity * field.velocity(world).length();
    let mach = speed / atmosphere.sound_speed;
    let dynamic_pressure = 0.5 * atmosphere.density * speed * speed;
    let static_pressure =
        (atmosphere.pressure + atmosphere.dynamic_pressure - dynamic_pressure).max(1.0);
    let total_pressure = if state.planet == Planet::Water {
        static_pressure + dynamic_pressure
    } else {
        static_pressure * total_pressure_ratio(mach, atmosphere.gamma)
    };
    Some(Probe {
        speed,
        mach,
        static_pressure,
        total_pressure,
        dynamic_pressure,
    })
}
