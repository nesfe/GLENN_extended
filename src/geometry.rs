//! Airfoil generation and a shared, closed wing mesh for rendering and CAD export.
use crate::model::{Flap, Point, Profile, Simulation};
use std::f64::consts::PI;

pub const SECTION_PANELS: usize = 48;
#[derive(Debug, Clone, PartialEq)]
pub struct Airfoil {
    pub upper: Vec<Point>,
    pub lower: Vec<Point>,
}

pub fn interpolate(points: &[Point], x: f64) -> f64 {
    let i = points
        .partition_point(|p| p.x < x)
        .clamp(1, points.len() - 1);
    let (a, b) = (points[i - 1], points[i]);
    let fraction = ((x - a.x) / (b.x - a.x).max(1e-12)).clamp(0.0, 1.0);
    a.y + fraction * (b.y - a.y)
}

impl Airfoil {
    pub fn generate(state: &Simulation) -> Self {
        let (m, p, t) = (
            state.camber / 100.0,
            state.camber_pos / 100.0,
            state.thickness / 100.0,
        );
        let mut upper = Vec::with_capacity(SECTION_PANELS + 1);
        let mut lower = Vec::with_capacity(SECTION_PANELS + 1);
        for i in 0..=SECTION_PANELS {
            let x = (1.0 - (PI * i as f64 / SECTION_PANELS as f64).cos()) * 0.5;
            let naca_thickness = 5.0
                * t
                * (0.2969 * x.sqrt() - 0.126 * x - 0.3516 * x * x + 0.2843 * x.powi(3)
                    - 0.1036 * x.powi(4));
            let (yt, yc, slope) = match state.profile_type {
                Profile::Biconvex => (
                    2.0 * t * x * (1.0 - x),
                    4.0 * m * x * (1.0 - x),
                    4.0 * m * (1.0 - 2.0 * x),
                ),
                Profile::DoubleWedge => (
                    t * x.min(1.0 - x),
                    2.0 * m * x.min(1.0 - x),
                    if x < 0.5 { 2.0 * m } else { -2.0 * m },
                ),
                Profile::Cylinder => ((0.25 - (x - 0.5).powi(2)).max(0.0).sqrt(), 0.0, 0.0),
                Profile::FlatPlate => (t * 0.5 * (PI * x).sin(), 0.0, 0.0),
                // A shape family, not a claim to reproduce a NASA SC(2) coordinate table.
                Profile::Supercritical => (
                    naca_thickness * (1.0 - 0.25 * x),
                    4.0 * m * x * (1.0 - x) * (0.6 + x),
                    4.0 * m * (0.6 + 0.8 * x - 3.0 * x * x),
                ),
                Profile::Naca5 => {
                    // Standard non-reflex NACA 230 mean line: m=0.2025, k1=15.957.
                    let a: f64 = 0.2025;
                    let k = 15.957 / 6.0;
                    if x < a {
                        (
                            naca_thickness,
                            k * (x.powi(3) - 3.0 * a * x * x + a * a * (3.0 - a) * x),
                            k * (3.0 * x * x - 6.0 * a * x + a * a * (3.0 - a)),
                        )
                    } else {
                        (naca_thickness, k * a.powi(3) * (1.0 - x), -k * a.powi(3))
                    }
                }
                _ => {
                    if x < p {
                        (
                            naca_thickness,
                            m / (p * p) * (2.0 * p * x - x * x),
                            2.0 * m / (p * p) * (p - x),
                        )
                    } else {
                        (
                            naca_thickness,
                            m / (1.0 - p).powi(2) * (1.0 - 2.0 * p + 2.0 * p * x - x * x),
                            2.0 * m / (1.0 - p).powi(2) * (p - x),
                        )
                    }
                }
            };
            let theta = slope.atan();
            let (mut u, mut l) = if state.profile_type == Profile::Custom {
                (
                    Point::new(
                        x,
                        interpolate(
                            state
                                .custom_upper
                                .as_ref()
                                .expect("validated custom profile"),
                            x,
                        ),
                    ),
                    Point::new(
                        x,
                        interpolate(
                            state
                                .custom_lower
                                .as_ref()
                                .expect("validated custom profile"),
                            x,
                        ),
                    ),
                )
            } else {
                (
                    Point::new(
                        (x - yt * theta.sin()).clamp(0.0, 1.0),
                        yc + yt * theta.cos(),
                    ),
                    Point::new(
                        (x + yt * theta.sin()).clamp(0.0, 1.0),
                        yc - yt * theta.cos(),
                    ),
                )
            };
            // Continuous hinge deformation; slot gaps are not resolved by this model.
            let flap_gain = match state.flap_type {
                Flap::Fowler => 1.25,
                Flap::Slotted => 1.05,
                _ => 0.85,
            };
            for point in [&mut u, &mut l] {
                point.y -=
                    (point.x - 0.75).max(0.0) * state.flap_angle.to_radians().sin() * flap_gain;
                point.y -= (0.15 - point.x).max(0.0) * state.slat_angle.to_radians().sin() * 0.8;
            }
            upper.push(u);
            lower.push(l);
        }
        // One common trailing-edge node is necessary for a closed Kutta contour.
        let trailing_y = (upper[SECTION_PANELS].y + lower[SECTION_PANELS].y) * 0.5;
        upper[SECTION_PANELS] = Point::new(1.0, trailing_y);
        lower[SECTION_PANELS] = Point::new(1.0, trailing_y);
        lower[0] = upper[0];
        Self { upper, lower }
    }

    /// Counter-clockwise contour without duplicate end nodes: TE → upper → LE → lower.
    pub fn contour(&self) -> Vec<Point> {
        self.upper
            .iter()
            .rev()
            .chain(self.lower.iter().skip(1).take(self.lower.len() - 2))
            .copied()
            .collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct WingMesh {
    pub vertices: Vec<[f64; 3]>,
    pub triangles: Vec<[usize; 3]>,
}
impl WingMesh {
    pub fn generate(state: &Simulation, foil: &Airfoil) -> Self {
        let contour = foil.contour();
        let n = contour.len();
        let stations = 24;
        let mut mesh = Self::default();
        for station in 0..=stations {
            let eta = 2.0 * station as f64 / stations as f64 - 1.0;
            let local_chord = state.chord * (1.0 - eta.abs() * (1.0 - state.taper));
            let twist = (state.twist * eta.abs()).to_radians();
            let span_position = eta * state.span * 0.5;
            let winglet_height = if state.has_winglets {
                (eta.abs() - 0.9).max(0.0) * state.span * 0.65
            } else {
                0.0
            };
            for p in &contour {
                let x = (p.x - 0.25) * local_chord;
                let y = p.y * local_chord;
                mesh.vertices.push([
                    x * twist.cos()
                        + y * twist.sin()
                        + span_position.abs() * state.sweep.to_radians().tan(),
                    -x * twist.sin()
                        + y * twist.cos()
                        + span_position.abs() * state.dihedral.to_radians().tan()
                        + winglet_height,
                    span_position,
                ]);
            }
        }
        for station in 0..stations {
            for j in 0..n {
                let a = station * n + j;
                let b = station * n + (j + 1) % n;
                let c = (station + 1) * n + j;
                let d = (station + 1) * n + (j + 1) % n;
                mesh.triangles.extend([[a, b, c], [b, d, c]]);
            }
        }
        // End caps make STL/OBJ watertight, including a shared seam at the trailing edge.
        for (station, reverse) in [(0, true), (stations, false)] {
            let center = mesh.vertices.len();
            let mut centroid = [0.0; 3];
            for p in &mesh.vertices[station * n..(station + 1) * n] {
                for axis in 0..3 {
                    centroid[axis] += p[axis] / n as f64;
                }
            }
            mesh.vertices.push(centroid);
            for j in 0..n {
                let a = station * n + j;
                let b = station * n + (j + 1) % n;
                mesh.triangles.push(if reverse {
                    [center, b, a]
                } else {
                    [center, a, b]
                });
            }
        }
        mesh
    }
}
