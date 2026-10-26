//! Geometry-coupled tunnel flow: section potential flow, finite-span trailing
//! vortices and an empirical separated wake. This is a hybrid visualization
//! model, not a time-resolved Navier–Stokes solver.
use super::{aerodynamics::Analysis, panel::PanelModel};
use crate::{
    geometry::{Airfoil, SectionFrame, interpolate},
    model::Point,
};
use glam::DVec3;
use std::f64::consts::PI;

// The cached domain covers the region the tracer actually visits: the rake is
// injected about three chords upstream and lines are followed roughly four
// chords past the trailing edge. A point outside still gets the exact panel
// sum, but that costs ~15x a cached lookup, so leaving the grid too small made
// a third of all samples take the slow path. Cell size is ~0.05 x 0.045 chord.
const GRID_X: usize = 221;
const GRID_Y: usize = 89;
const X_MIN: f64 = -3.6;
const X_MAX: f64 = 7.4;
const Y_MIN: f64 = -2.0;
const Y_MAX: f64 = 2.0;

/// Split an index range across the available cores and concatenate the chunk
/// results in order. Every chunk is independent, so the output is identical to
/// a serial run; only the wall-clock cost changes.
fn parallel_chunks<T: Send>(
    total: usize,
    body: impl Fn(std::ops::Range<usize>) -> Vec<T> + Sync,
) -> Vec<T> {
    let threads = std::thread::available_parallelism()
        .map_or(1, |count| count.get())
        .min(total);
    if threads < 2 {
        return body(0..total);
    }
    let span = total.div_ceil(threads);
    let body = &body;
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|i| {
                let range = (i * span).min(total)..((i + 1) * span).min(total);
                scope.spawn(move || body(range))
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("flow worker panicked"))
            .collect()
    })
}

/// Two velocity bases are sampled once per airfoil. Changing incidence combines
/// these bases instead of solving or sampling thousands of panels again.
pub struct SectionGrid {
    pub foil: Airfoil,
    bases: Vec<[Point; 2]>,
}
impl SectionGrid {
    pub fn new(foil: &Airfoil, panel: &PanelModel) -> Self {
        // Row-major, matching the bilinear lookup below.
        let bases = parallel_chunks(GRID_Y, |rows| {
            let mut out = Vec::with_capacity(rows.len() * GRID_X);
            for row in rows {
                for column in 0..GRID_X {
                    let point = Point::new(
                        X_MIN + (X_MAX - X_MIN) * column as f64 / (GRID_X - 1) as f64,
                        Y_MIN + (Y_MAX - Y_MIN) * row as f64 / (GRID_Y - 1) as f64,
                    );
                    out.push(panel.velocity_basis(point));
                }
            }
            out
        });
        Self {
            foil: foil.clone(),
            bases,
        }
    }

    pub fn velocity(&self, panel: &PanelModel, point: Point, incidence: f64) -> Point {
        let (sin, cos) = incidence.sin_cos();
        if !(X_MIN..X_MAX).contains(&point.x) || !(Y_MIN..Y_MAX).contains(&point.y) {
            return panel.velocity_at(point, incidence.to_degrees());
        }
        // Do not interpolate across the solid boundary. Exact panels retain the
        // no-penetration condition where a coarse grid would mix both surfaces.
        if (-0.03..1.03).contains(&point.x) {
            let upper = interpolate(&self.foil.upper, point.x);
            let lower = interpolate(&self.foil.lower, point.x);
            if (point.y - upper).abs().min((point.y - lower).abs()) < 0.07 {
                return panel.velocity_at(point, incidence.to_degrees());
            }
        }
        let x = (point.x - X_MIN) / (X_MAX - X_MIN) * (GRID_X - 1) as f64;
        let y = (point.y - Y_MIN) / (Y_MAX - Y_MIN) * (GRID_Y - 1) as f64;
        let column = (x as usize).min(GRID_X - 2);
        let row = (y as usize).min(GRID_Y - 2);
        let mut result = Point::default();
        for (dx, wx) in [(0, 1.0 - x.fract()), (1, x.fract())] {
            for (dy, wy) in [(0, 1.0 - y.fract()), (1, y.fract())] {
                let basis = self.bases[(row + dy) * GRID_X + column + dx];
                result.x += wx * wy * (basis[0].x * cos + basis[1].x * sin);
                result.y += wx * wy * (basis[0].y * cos + basis[1].y * sin);
            }
        }
        result
    }
}

/// Every input the traced field reads, in one comparable value. Equal
/// fingerprints give identical streamlines, so a caller can keep the previous
/// trace when an unrelated parameter moved: engine, mass, fuel and display
/// settings never enter the flow solution. Derived aerodynamic outputs are
/// included because the wake and the trailing filaments read them, which is
/// how an environment change reaches the field through Mach and lift.
pub fn field_fingerprint(analysis: &Analysis) -> [f64; 13] {
    let state = &analysis.state;
    [
        state.chord,
        state.span,
        state.alpha,
        state.twist,
        state.taper,
        state.sweep,
        state.dihedral,
        f64::from(state.has_winglets),
        state.velocity,
        state.cylinder_rpm,
        f64::from(state.profile_type == crate::model::Profile::Cylinder),
        analysis.current.cl,
        analysis.current.stall_fraction,
    ]
}

#[derive(Debug, Clone, Copy)]
pub struct FlowSample {
    pub position: DVec3,
    pub speed_ratio: f64,
    /// Convective time in root-chord / free-stream-speed units.
    pub travel_time: f64,
}
#[derive(Debug, Clone)]
pub struct Streamline {
    pub samples: Vec<FlowSample>,
    pub vortex: bool,
}

pub struct FlowField<'a> {
    pub analysis: &'a Analysis,
    pub grid: &'a SectionGrid,
}
impl FlowField<'_> {
    pub fn spin_ratio(&self) -> f64 {
        let state = &self.analysis.state;
        (state.cylinder_rpm * 2.0 * PI / 60.0 * state.mean_chord() * 0.5 / state.velocity.max(0.1))
            .clamp(-4.0, 4.0)
    }
    pub fn contains_solid(&self, world: DVec3) -> bool {
        let state = &self.analysis.state;
        if world.z.abs() > state.span * 0.5 {
            return false;
        }
        let local = SectionFrame::at(state, world.z).to_section(world);
        if !(0.0..=1.0).contains(&local.x) {
            return false;
        }
        let upper = interpolate(&self.analysis.foil.upper, local.x);
        let lower = interpolate(&self.analysis.foil.lower, local.x);
        local.y < upper && local.y > lower
    }

    pub fn tip_origin(&self, side: f64) -> DVec3 {
        let frame = SectionFrame::at(&self.analysis.state, side * self.analysis.state.span * 0.5);
        frame.to_world(*self.analysis.foil.upper.last().unwrap())
    }

    /// Signed rolled-up circulation in m²/s. It vanishes at zero lift or speed.
    pub fn tip_circulation(&self) -> f64 {
        let state = &self.analysis.state;
        2.0 * state.velocity * state.area() * self.analysis.current.cl / (PI * state.span)
    }

    pub fn wake_weight(&self, position: DVec3) -> f64 {
        let state = &self.analysis.state;
        let frame = SectionFrame::at(state, position.z.clamp(-state.span * 0.5, state.span * 0.5));
        let trailing = frame.to_world(*self.analysis.foil.upper.last().unwrap());
        let downstream = (position.x - trailing.x) / state.chord;
        if downstream <= 0.0 {
            return 0.0;
        }
        let wake_height = state.chord * (0.16 + 0.45 * self.analysis.current.stall_fraction);
        let center_y = trailing.y - downstream * state.chord * self.analysis.current.cl * 0.045;
        let span_falloff = (1.0 - (position.z.abs() / (state.span * 0.58)).powi(8)).clamp(0.0, 1.0);
        (1.0 - (-downstream * 2.0).exp())
            * (-(position.y - center_y).powi(2) / wake_height.powi(2)).exp()
            * span_falloff
    }

    /// Velocity divided by free-stream speed, in the fixed tunnel frame.
    pub fn velocity(&self, world: DVec3) -> DVec3 {
        let state = &self.analysis.state;
        let span_position = world.z.clamp(-state.span * 0.5, state.span * 0.5);
        let frame = SectionFrame::at(state, span_position);
        let local = frame.to_section(world);
        let section_velocity = if state.profile_type == crate::model::Profile::Cylinder {
            super::panel::cylinder_velocity(local, frame.incidence, self.spin_ratio())
        } else {
            self.grid
                .velocity(&self.analysis.panel, local, frame.incidence)
        };
        let mut velocity = frame.velocity_to_world(section_velocity);
        // Fade the 2D section's influence beyond the physical wing tips.
        let outside = (world.z.abs() - state.span * 0.5).max(0.0) / state.chord;
        velocity = DVec3::X + (velocity - DVec3::X) * (-outside * outside * 6.0).exp();
        let chord = state.chord;
        let circulation = self.tip_circulation() / state.velocity.max(0.1);
        for side in [-1.0, 1.0] {
            let tip = self.tip_origin(side);
            let downstream = (world.x - tip.x) / chord;
            if downstream <= 0.0 {
                continue;
            }
            let dy = world.y - tip.y;
            let dz = world.z - tip.z;
            let core = chord * (0.045 + 0.025 * downstream.sqrt());
            // Counter-rotating trailing filaments; handedness follows lift sign.
            let strength = -side * circulation * (1.0 - (-downstream * 4.0).exp());
            let factor = strength / (2.0 * PI * (dy * dy + dz * dz + core * core));
            velocity.y -= factor * dz;
            velocity.z += factor * dy;
        }
        let wake = self.wake_weight(world);
        let stall = self.analysis.current.stall_fraction;
        velocity.x *= 1.0 - 0.6 * stall * wake;
        velocity.y -= wake * self.analysis.current.cl * 0.035;
        // The present model predicts a mean deficit, not vortex shedding.
        // Do not manufacture time-dependent eddies absent from the equations.
        velocity
    }

    pub fn trace(&self, seed: DVec3, vortex: bool) -> Streamline {
        let state = &self.analysis.state;
        let chord = state.chord;
        let mut position = seed;
        let mut travel_time = 0.0;
        let mut samples = Vec::with_capacity(300);
        let seed_frame = SectionFrame::at(state, seed.z.clamp(-state.span * 0.5, state.span * 0.5));
        let max_x = seed_frame
            .to_world(*self.analysis.foil.upper.last().unwrap())
            .x
            + 4.2 * chord;
        let steps = (((max_x - seed.x).abs() / chord / 0.035) as usize + 200).min(1600);
        for _ in 0..steps {
            if self.contains_solid(position)
                || position.x > max_x
                || position.y.abs() > 4.0 * chord + state.span * 0.3
            {
                break;
            }
            let velocity = self.velocity(position);
            let speed = velocity.length();
            if speed < 0.015 {
                break;
            }
            samples.push(FlowSample {
                position,
                speed_ratio: speed,
                travel_time,
            });
            let frame =
                SectionFrame::at(state, position.z.clamp(-state.span * 0.5, state.span * 0.5));
            let local = frame.to_section(position);
            // Long swept wings need larger steps in undisturbed upstream air.
            // Return to fine section-sized steps before reaching the surface.
            let distance = (-local.x - 1.0).max(0.0) * frame.chord;
            let spacing = (frame.chord * 0.035 + distance * 0.08).min(chord * 4.0);
            let mut step = spacing / speed.max(0.3);
            let mut next = position;
            let mut accepted = false;
            for _ in 0..7 {
                // RK4 in convective time. Subdivide steps that touch the body;
                // never draw a segment through a surface to hide integration error.
                let k1 = velocity;
                let k2 = self.velocity(position + k1 * (step * 0.5));
                let k3 = self.velocity(position + k2 * (step * 0.5));
                let k4 = self.velocity(position + k3 * step);
                next = position + (k1 + 2.0 * k2 + 2.0 * k3 + k4) * (step / 6.0);
                if [0.25, 0.5, 0.75, 1.0]
                    .iter()
                    .all(|t| !self.contains_solid(position.lerp(next, *t)))
                {
                    accepted = true;
                    break;
                }
                step *= 0.5;
            }
            if !accepted || position.distance(next) < chord * 1e-6 {
                break;
            }
            travel_time += step / chord;
            position = next;
        }
        Streamline { samples, vortex }
    }

    pub fn streamlines(&self, density: usize) -> Vec<Streamline> {
        let seeds = self.seeds(density);
        // Each trace reads the shared field and writes nothing, so integrating
        // them on several cores gives the same lines in the same order.
        parallel_chunks(seeds.len(), |range| {
            seeds[range]
                .iter()
                .map(|&(seed, vortex)| self.trace(seed, vortex))
                .collect()
        })
        .into_iter()
        // A line that stops within a few steps of its seed carries no
        // information; tip strands are kept so the vortex core stays visible.
        .filter(|line| line.vortex || line.samples.len() > 12)
        .collect()
    }

    /// Injection points for the tracer rake and the tip strands, in the order
    /// they are drawn.
    fn seeds(&self, density: usize) -> Vec<(DVec3, bool)> {
        let state = &self.analysis.state;
        let mut paths = Vec::new();
        let span_lanes = 5 + density * 2;
        // Forward sweep can put a tip upstream of the root. Place the entire
        // injection rake ahead of the actual geometry, not at a root-only bound.
        let inlet_x = self
            .analysis
            .mesh
            .vertices
            .iter()
            .map(|p| crate::geometry::body_to_world(state, DVec3::from_array(*p)).x)
            .fold(f64::INFINITY, f64::min)
            - 2.0 * state.chord;
        let layers = [-0.72, -0.38, -0.16, 0.08, 0.30, 0.58, 0.9];
        for lane in 0..span_lanes {
            let z = state.span * 0.55 * (2.0 * lane as f64 / (span_lanes - 1) as f64 - 1.0);
            for layer in layers {
                // Injection rake is fixed in tunnel coordinates; it does not
                // rotate with incidence. Dihedral only sets its vertical coverage.
                let seed = DVec3::new(
                    inlet_x,
                    z.abs() * state.dihedral.to_radians().tan() + layer * state.chord,
                    z,
                );
                paths.push((seed, false));
            }
        }
        if self.tip_circulation().abs() > state.velocity * state.chord * 0.01 {
            for side in [-1.0, 1.0] {
                let tip = self.tip_origin(side);
                for strand in 0..6 {
                    let phase = 2.0 * PI * strand as f64 / 6.0;
                    let seed = tip
                        + DVec3::new(
                            state.chord * 0.08,
                            phase.sin() * state.chord * 0.11,
                            phase.cos() * state.chord * 0.11,
                        );
                    paths.push((seed, true));
                }
            }
        }
        paths
    }
}
