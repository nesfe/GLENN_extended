//! Finite-wing engineering estimates built on the 2D pressure solution.
use super::{
    atmosphere::{self, Atmosphere},
    panel::{PanelModel, Pressure},
};
use crate::{
    geometry::{Airfoil, WingMesh},
    model::{Point, Profile, Simulation},
};
use std::f64::consts::PI;

#[derive(Debug, Clone)]
pub struct AeroResult {
    pub cl: f64,
    pub cd: f64,
    pub cd_profile: f64,
    pub cd_induced: f64,
    pub cd_wave: f64,
    pub cd_stall: f64,
    pub lift: f64,
    pub drag: f64,
    pub moment: f64,
    pub cm: f64,
    pub center_of_pressure: f64,
    pub mach: f64,
    pub critical_mach: f64,
    pub reynolds: f64,
    pub efficiency: f64,
    pub stall_fraction: f64,
    pub pressure: Pressure,
    pub span_loading: Vec<Point>,
    pub circulation: Vec<Point>,
}
impl AeroResult {
    pub fn lift_drag_ratio(&self) -> f64 {
        self.cl / self.cd
    }
}

/// A single cache shared by the UI, charts, performance model and file exports.
/// Presentation settings never enter this type, so changing atmosphere camera or color
/// does not cause numerical work. Geometry changes alone refactor the panel system.
pub struct Analysis {
    pub state: Simulation,
    pub foil: Airfoil,
    pub mesh: WingMesh,
    pub atmosphere: Atmosphere,
    pub current: AeroResult,
    pub polar: Vec<(f64, AeroResult)>,
    pub panel: PanelModel,
    pub geometry_rebuilds: usize,
}
impl Analysis {
    pub fn new(state: Simulation) -> Result<Self, String> {
        state.validate()?;
        let foil = Airfoil::generate(&state);
        let panel = PanelModel::prepare(&foil)?;
        let mesh = WingMesh::generate(&state, &foil);
        let atmosphere = atmosphere::calculate(&state);
        let current = solve(&state, &atmosphere, &panel);
        let polar = polar(&state, &atmosphere, &panel);
        Ok(Self {
            state,
            foil,
            mesh,
            atmosphere,
            current,
            polar,
            panel,
            geometry_rebuilds: 1,
        })
    }
    pub fn update(&mut self, state: &Simulation) -> Result<(), String> {
        if self.state == *state {
            return Ok(());
        }
        state.validate()?;
        let foil = Airfoil::generate(state);
        if foil != self.foil {
            self.panel = PanelModel::prepare(&foil)?;
            self.geometry_rebuilds += 1;
        }
        let planform_changed = self.state.chord != state.chord
            || self.state.span != state.span
            || self.state.sweep != state.sweep
            || self.state.dihedral != state.dihedral
            || self.state.twist != state.twist
            || self.state.taper != state.taper
            || self.state.has_winglets != state.has_winglets;
        if foil != self.foil || planform_changed {
            self.mesh = WingMesh::generate(state, &foil);
        }
        self.foil = foil;
        self.atmosphere = atmosphere::calculate(state);
        self.current = solve(state, &self.atmosphere, &self.panel);
        let mut previous = self.state.clone();
        previous.alpha = state.alpha;
        if previous != *state {
            self.polar = polar(state, &self.atmosphere, &self.panel);
        }
        self.state = state.clone();
        Ok(())
    }
    pub fn maximum_lift(&self) -> f64 {
        self.polar.iter().map(|(_, r)| r.cl).fold(0.2, f64::max)
    }
}
fn polar(
    state: &Simulation,
    atmosphere: &Atmosphere,
    panel: &PanelModel,
) -> Vec<(f64, AeroResult)> {
    (-30..=45)
        .map(|alpha| {
            let mut sample = state.clone();
            sample.alpha = alpha as f64;
            (sample.alpha, solve(&sample, atmosphere, panel))
        })
        .collect()
}
fn solve(state: &Simulation, atmosphere: &Atmosphere, panel: &PanelModel) -> AeroResult {
    let alpha = state.alpha.to_radians();
    let sweep = state.sweep.to_radians();
    let aspect_ratio = state.aspect_ratio();
    let mach = state.velocity / atmosphere.sound_speed;
    let reynolds = atmosphere.density * state.velocity * state.mean_chord() / atmosphere.viscosity;
    let thickness = state.thickness / 100.0;
    let efficiency = ((1.78 * (1.0 - 0.045 * aspect_ratio.powf(0.68)) - 0.64)
        * (sweep * 0.65).cos()
        * if state.has_winglets { 1.12 } else { 1.0 })
    .clamp(0.5, 0.98);
    let stall_angle =
        14.0 + 22.0 * thickness - 0.1 * state.flap_angle.max(0.0) + 0.35 * state.slat_angle;
    let stall_fraction = if state.profile_type == Profile::Cylinder {
        0.0
    } else {
        1.0 / (1.0 + (-(state.alpha.abs() - stall_angle) * 1.1).exp())
    };
    let mut pressure = panel.evaluate(state.alpha, mach * sweep.cos());
    let effective_pressure = panel.evaluate(state.alpha + 0.4 * state.twist, mach * sweep.cos());
    let section_slope = 2.0 * PI / (1.0 - (mach * sweep.cos()).min(0.8).powi(2)).sqrt();
    let finite_wing_factor =
        sweep.cos() / (1.0 + section_slope * sweep.cos() / (PI * aspect_ratio * efficiency));
    let spin_ratio = (state.cylinder_rpm * 2.0 * PI / 60.0 * state.mean_chord() * 0.5
        / state.velocity.max(0.1))
    .clamp(-4.0, 4.0);
    if state.profile_type == Profile::Cylinder {
        pressure = panel.cylinder_pressure(state.alpha, spin_ratio);
    }
    let subsonic_cl = (1.0 - stall_fraction) * effective_pressure.lift * finite_wing_factor
        + stall_fraction * 1.95 * alpha.sin() * alpha.cos();
    let supersonic_cl = 4.0 * alpha / (mach * mach - 1.0).max(0.1).sqrt();
    // Smoothly join approximations across the unvalidated transonic region.
    let blend = ((mach - 0.9) / 0.4).clamp(0.0, 1.0);
    let blend = blend * blend * (3.0 - 2.0 * blend);
    let cl = if state.profile_type == Profile::Cylinder {
        2.0 * PI * spin_ratio
    } else {
        subsonic_cl * (1.0 - blend) + supersonic_cl * blend
    };
    let cf = 0.074 / reynolds.max(1000.0).powf(0.2);
    let cd_profile = if state.profile_type == Profile::Cylinder {
        1.1 + 0.3 * spin_ratio.abs()
    } else {
        2.0 * cf * (1.0 + 2.0 * thickness + 60.0 * thickness.powi(4))
            + 0.00016 * state.flap_angle.powi(2)
            + 0.00012 * state.slat_angle.powi(2)
    };
    let cd_induced = cl * cl / (PI * aspect_ratio * efficiency);
    let critical_mach = (0.87 - 0.1 * cl.abs() - thickness).clamp(0.4, 0.88);
    let transonic_drag = 18.0 * (mach.min(1.05) - critical_mach).max(0.0).powf(3.5);
    let supersonic_drag = (4.0 * alpha * alpha + 16.0 / 3.0 * thickness * thickness)
        / (mach * mach - 1.0).max(0.1).sqrt();
    let cd_wave = transonic_drag * (1.0 - blend) + supersonic_drag * blend;
    let cd_stall = stall_fraction * 1.75 * alpha.sin().powi(2);
    let cd = (cd_profile + cd_induced + cd_wave + cd_stall).max(0.002);
    let lift = atmosphere.dynamic_pressure * state.area() * cl;
    let drag = atmosphere.dynamic_pressure * state.area() * cd;
    let cm = if state.profile_type == Profile::Cylinder {
        0.0
    } else {
        pressure.moment
    };
    let center_of_pressure = if cl.abs() > 0.01 {
        0.25 - cm / cl
    } else {
        0.25
    };
    // Normalize numerical span loading to total lift. Signed lift is preserved.
    let stations = 65;
    let spacing = state.span / (stations - 1) as f64;
    let weights: Vec<f64> = (0..stations)
        .map(|i| {
            let eta = 2.0 * i as f64 / (stations - 1) as f64 - 1.0;
            (1.0 - eta * eta).max(0.0).sqrt() * (1.0 - eta.abs() * (1.0 - state.taper))
        })
        .collect();
    let integral = weights.iter().sum::<f64>() * spacing;
    let span_loading: Vec<Point> = weights
        .iter()
        .enumerate()
        .map(|(i, w)| {
            Point::new(
                -state.span * 0.5 + i as f64 * spacing,
                lift * w / integral.max(1e-12),
            )
        })
        .collect();
    let circulation = span_loading
        .iter()
        .map(|p| {
            Point::new(
                p.x,
                if state.velocity > 0.0 {
                    p.y / (atmosphere.density * state.velocity)
                } else {
                    0.0
                },
            )
        })
        .collect();
    AeroResult {
        cl,
        cd,
        cd_profile,
        cd_induced,
        cd_wave,
        cd_stall,
        lift,
        drag,
        cm,
        center_of_pressure,
        moment: atmosphere.dynamic_pressure * state.area() * state.mean_chord() * cm,
        mach,
        critical_mach,
        reynolds,
        efficiency,
        stall_fraction,
        pressure,
        span_loading,
        circulation,
    }
}
