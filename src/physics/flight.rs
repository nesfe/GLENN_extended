//! Steady level-flight estimates; thrust is evaluated consistently at every speed.
use super::{
    aerodynamics::Analysis,
    atmosphere::{self, Atmosphere},
};
use crate::model::{Engine, Planet, Simulation};
use std::f64::consts::PI;
#[derive(Debug, Clone)]
pub struct Performance {
    pub stall_speed: f64,
    pub glide_speed: f64,
    pub endurance_speed: f64,
    pub glide_ratio: f64,
    pub sink_rate: f64,
    pub thrust: f64,
    pub required_thrust: f64,
    pub climb_rate: f64,
    pub climb_angle: f64,
    pub maximum_speed: Option<f64>,
    pub ceiling: Option<f64>,
    pub range_km: Option<f64>,
    pub endurance_hours: Option<f64>,
}
pub fn thrust(state: &Simulation, atmosphere: &Atmosphere, speed: f64) -> f64 {
    let density_ratio = atmosphere.density / 1.225;
    let mach = speed / atmosphere.sound_speed;
    match state.engine_type {
        Engine::Rocket => state.max_thrust * (1.0 + 0.12 * (1.0 - density_ratio.min(1.0))),
        Engine::Turbojet => state.max_thrust * density_ratio.powf(0.9) * (1.0 + 0.3 * mach * mach),
        Engine::Turbofan => {
            state.max_thrust * density_ratio.powf(0.82) * (1.0 - 0.32 * mach).max(0.2)
        }
        Engine::Piston => {
            state.max_thrust * 45.0 * (density_ratio - 0.15).max(0.0) * 0.82 / speed.max(10.0)
        }
        Engine::Electric => state.max_thrust * 40.0 * 0.85 / speed.max(10.0),
    }
}
fn level_drag(weight: f64, area: f64, density: f64, speed: f64, cd0: f64, k: f64) -> f64 {
    let q = 0.5 * density * speed.max(0.1).powi(2);
    let cl = weight / (q * area);
    q * area * (cd0 + k * cl * cl)
}
pub fn calculate(analysis: &Analysis) -> Performance {
    let state = &analysis.state;
    let atmosphere = &analysis.atmosphere;
    let aero = &analysis.current;
    let weight = state.aircraft_mass * atmosphere.gravity;
    let area = state.area();
    let k = 1.0 / (PI * state.aspect_ratio() * aero.efficiency);
    let cd0 = aero.cd_profile.max(0.002);
    let cl_max = analysis.maximum_lift();
    let stall_speed = (2.0 * weight / (atmosphere.density * area * cl_max)).sqrt();
    let glide_speed = (2.0 * weight / (atmosphere.density * area)).sqrt() * (k / cd0).powf(0.25);
    let glide_ratio = 1.0 / (2.0 * (cd0 * k).sqrt());
    let endurance_speed = glide_speed / 3.0_f64.powf(0.25);
    let available = thrust(state, atmosphere, state.velocity);
    let required = level_drag(weight, area, atmosphere.density, state.velocity, cd0, k);
    let climb_rate = (available - required) * state.velocity / weight;
    let mut maximum_speed = None;
    for i in 0..=600 {
        let speed = stall_speed + i as f64 * 2.0;
        if speed > 1200.0 {
            break;
        }
        if thrust(state, atmosphere, speed)
            >= level_drag(weight, area, atmosphere.density, speed, cd0, k)
        {
            maximum_speed = Some(speed);
        }
    }
    // Only Earth uses this ceiling estimate. Sample the entire feasible speed range
    // at each altitude instead of substituting an arbitrary logarithmic formula.
    let ceiling = if state.planet == Planet::Earth {
        let mut highest = None;
        for h in (0..=30000).step_by(500) {
            let mut sample = state.clone();
            sample.altitude = h as f64;
            let atmosphere = atmosphere::calculate(&sample);
            let v_stall = (2.0 * weight / (atmosphere.density * area * cl_max)).sqrt();
            let mut best = f64::NEG_INFINITY;
            for i in 0..80 {
                let speed = v_stall + 3.0 * i as f64;
                let drag = level_drag(weight, area, atmosphere.density, speed, cd0, k);
                best = best.max((thrust(state, &atmosphere, speed) - drag) * speed / weight);
            }
            if best >= 0.5 {
                highest = Some(h as f64);
            }
        }
        highest
    } else {
        None
    };
    // The Breguet jet formula is not meaningful for electric or rocket propulsion.
    let supports_breguet = matches!(state.engine_type, Engine::Turbofan | Engine::Turbojet);
    let duration = if supports_breguet {
        Some(
            glide_ratio * (state.aircraft_mass / (state.aircraft_mass - state.fuel_mass)).ln()
                / (atmosphere.gravity * state.tsfc),
        )
    } else {
        None
    };
    Performance {
        stall_speed,
        glide_speed,
        endurance_speed,
        glide_ratio,
        sink_rate: glide_speed / glide_ratio,
        thrust: available,
        required_thrust: required,
        climb_rate,
        climb_angle: ((available - required) / weight)
            .clamp(-1.0, 1.0)
            .asin()
            .to_degrees(),
        maximum_speed,
        ceiling,
        range_km: duration.map(|t| t * state.velocity / 1000.0),
        endurance_hours: duration.map(|t| t / 3600.0),
    }
}
