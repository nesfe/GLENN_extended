//! Progressive two-dimensional viscous flow. D2Q9 with regularized collision,
//! local Smagorinsky stress and stationary halfway bounce-back walls.
//! No prescribed separation point, circulation, wake, or lift curve.
pub mod worker;

use glenn_core::{geometry::Airfoil, model::Point};

const C: [[i32; 2]; 9] = [
    [0, 0],
    [1, 0],
    [0, 1],
    [-1, 0],
    [0, -1],
    [1, 1],
    [-1, 1],
    [-1, -1],
    [1, -1],
];
const OPP: [usize; 9] = [0, 3, 4, 1, 2, 7, 8, 5, 6];
const W: [f32; 9] = [
    4.0 / 9.0,
    1.0 / 9.0,
    1.0 / 9.0,
    1.0 / 9.0,
    1.0 / 9.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
    1.0 / 36.0,
];
pub const INFLOW: f32 = 0.07;
pub const CS: f32 = 0.16;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub chord_cells: usize,
    pub reynolds: f64,
    pub alpha: f64,
    pub contour: Vec<Point>,
}
impl Config {
    pub fn airfoil(foil: &Airfoil, alpha: f64, reynolds: f64, chord_cells: usize) -> Self {
        Self {
            contour: foil.contour(),
            alpha,
            reynolds,
            chord_cells,
        }
    }
}
#[derive(Clone)]
pub struct Snapshot {
    pub width: usize,
    pub height: usize,
    pub cells_per_chord: f32,
    pub step: u64,
    /// Convective time t U_inf / chord.
    pub time: f64,
    pub velocity: Vec<[f32; 2]>,
    pub pressure: Vec<f32>,
    pub solid: Vec<bool>,
    /// Instantaneous 2D coefficients from momentum exchange, not wing forces.
    pub drag: f32,
    pub lift: f32,
    pub mean_density: f64,
    pub min_ux: f32,
}
impl Snapshot {
    pub fn point(&self, x: usize, y: usize) -> [f32; 2] {
        [
            (x as f32 + 0.5) / self.cells_per_chord - 1.5,
            (y as f32 + 0.5) / self.cells_per_chord - 2.0,
        ]
    }
    /// Instantaneous streamline integrated with midpoint RK2 from the measured
    /// velocity. Fixed spatial increments resolve the grid and do not encode
    /// a prescribed wake or separation curve.
    pub fn trace(&self, seed: [f32; 2]) -> Vec<[f32; 2]> {
        let mut path = vec![seed];
        let mut p = seed;
        let step = 0.5 / self.cells_per_chord;
        for _ in 0..700 {
            let Some(u) = self.sample(p[0], p[1]) else {
                break;
            };
            let speed = u[0].hypot(u[1]);
            if speed < 0.01 {
                break;
            }
            let dt = step / speed;
            let middle = [p[0] + 0.5 * dt * u[0], p[1] + 0.5 * dt * u[1]];
            let Some(v) = self.sample(middle[0], middle[1]) else {
                break;
            };
            let next = [p[0] + dt * v[0], p[1] + dt * v[1]];
            if self.sample(next[0], next[1]).is_none() {
                break;
            }
            path.push(next);
            p = next;
            if path.len() > 30 && (p[0] - seed[0]).hypot(p[1] - seed[1]) < step {
                break;
            }
        }
        path
    }
    pub fn sample(&self, x: f32, y: f32) -> Option<[f32; 2]> {
        let gx = (x + 1.5) * self.cells_per_chord - 0.5;
        let gy = (y + 2.0) * self.cells_per_chord - 0.5;
        if gx < 0.0 || gy < 0.0 || gx >= (self.width - 1) as f32 || gy >= (self.height - 1) as f32 {
            return None;
        }
        let (ix, iy) = (gx as usize, gy as usize);
        let mut u = [0.0; 2];
        for (dx, wx) in [(0, 1.0 - gx.fract()), (1, gx.fract())] {
            for (dy, wy) in [(0, 1.0 - gy.fract()), (1, gy.fract())] {
                let i = (iy + dy) * self.width + ix + dx;
                if self.solid[i] {
                    return None;
                }
                u[0] += wx * wy * self.velocity[i][0];
                u[1] += wx * wy * self.velocity[i][1];
            }
        }
        Some(u)
    }
}

pub struct Solver {
    config: Config,
    width: usize,
    height: usize,
    populations: Vec<[f32; 9]>,
    next: Vec<[f32; 9]>,
    solid: Vec<bool>,
    /// Index of the upstream cell for each streaming direction; MAX means wall.
    source: Vec<[usize; 9]>,
    step: u64,
    force: [f32; 2],
    periodic: bool,
    smagorinsky: f32,
}
fn equilibrium(rho: f32, u: [f32; 2]) -> [f32; 9] {
    let u2 = u[0] * u[0] + u[1] * u[1];
    std::array::from_fn(|q| {
        let cu = C[q][0] as f32 * u[0] + C[q][1] as f32 * u[1];
        W[q] * rho * (1.0 + 3.0 * cu + 4.5 * cu * cu - 1.5 * u2)
    })
}
fn inside(p: [f64; 2], contour: &[[f64; 2]]) -> bool {
    if contour.is_empty() {
        return false;
    }
    let mut result = false;
    let mut a = contour[contour.len() - 1];
    for &b in contour {
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            result = !result;
        }
        a = b;
    }
    result
}
impl Solver {
    pub fn new(config: Config) -> Result<Self, String> {
        if !(24..=96).contains(&config.chord_cells)
            || !config.reynolds.is_finite()
            || config.reynolds <= 0.0
            || !config.alpha.is_finite()
            || config
                .contour
                .iter()
                .any(|p| !p.x.is_finite() || !p.y.is_finite())
        {
            return Err("Недопустимые параметры встроенного решателя".into());
        }
        let n = config.chord_cells;
        let (width, height) = (7 * n, 4 * n);
        let (s, c) = (-config.alpha.to_radians()).sin_cos();
        let contour: Vec<_> = config
            .contour
            .iter()
            .map(|p| {
                [
                    0.25 + (p.x - 0.25) * c - p.y * s,
                    (p.x - 0.25) * s + p.y * c,
                ]
            })
            .collect();
        let solid: Vec<_> = (0..width * height)
            .map(|i| {
                inside(
                    [
                        (i % width) as f64 / n as f64 + 0.5 / n as f64 - 1.5,
                        (i / width) as f64 / n as f64 + 0.5 / n as f64 - 2.0,
                    ],
                    &contour,
                )
            })
            .collect();
        if !config.contour.is_empty() && !solid.iter().any(|&cell| cell) {
            return Err("Профиль тоньше ячейки. Увеличьте разрешение сетки.".into());
        }
        let source = (0..width * height)
            .map(|i| {
                std::array::from_fn(|q| {
                    let x = (i % width) as i32 - C[q][0];
                    let y = (i / width) as i32 - C[q][1];
                    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                        return i;
                    }
                    let j = y as usize * width + x as usize;
                    if solid[j] { usize::MAX } else { j }
                })
            })
            .collect();
        let initial = equilibrium(1.0, [INFLOW, 0.0]);
        Ok(Self {
            config,
            width,
            height,
            populations: vec![initial; width * height],
            next: vec![initial; width * height],
            solid,
            source,
            step: 0,
            force: [0.0; 2],
            periodic: false,
            smagorinsky: CS,
        })
    }
    pub fn advance(&mut self, count: usize) -> Result<(), String> {
        let tau0 =
            0.5 + 3.0 * INFLOW * self.config.chord_cells as f32 / self.config.reynolds as f32;
        let inlet = equilibrium(1.0, [INFLOW, 0.0]);
        for _ in 0..count {
            let mut force = [0.0; 2];
            for y in 0..self.height {
                for x in 0..self.width {
                    let i = y * self.width + x;
                    if self.solid[i] {
                        continue;
                    }
                    // Open tunnel: prescribed free stream on inflow and farfield;
                    // zero normal gradient at downstream boundary.
                    if !self.periodic && (x == 0 || y == 0 || y == self.height - 1) {
                        self.next[i] = inlet;
                        continue;
                    }
                    if !self.periodic && x == self.width - 1 {
                        self.next[i] = self.populations[i - 1];
                        continue;
                    }
                    let mut f = [0.0; 9];
                    for q in 0..9 {
                        let j = self.source[i][q];
                        if j == usize::MAX {
                            f[q] = self.populations[i][OPP[q]];
                            force[0] -= 2.0 * f[q] * C[q][0] as f32;
                            force[1] -= 2.0 * f[q] * C[q][1] as f32;
                        } else {
                            f[q] = self.populations[j][q];
                        }
                    }
                    let rho: f32 = f.iter().sum();
                    let u = [
                        (f[1] - f[3] + f[5] - f[6] - f[7] + f[8]) / rho,
                        (f[2] - f[4] + f[5] + f[6] - f[7] - f[8]) / rho,
                    ];
                    if !rho.is_finite()
                        || !(0.5..1.5).contains(&rho)
                        || u[0] * u[0] + u[1] * u[1] > 0.25
                    {
                        return Err(format!(
                            "Расчёт потерял устойчивость на шаге {}. Увеличьте разрешение или уменьшите скорость.",
                            self.step
                        ));
                    }
                    let eq = equilibrium(rho, u);
                    let (mut pxx, mut pxy, mut pyy) = (0.0, 0.0, 0.0);
                    for q in 1..9 {
                        let neq = f[q] - eq[q];
                        let cx = C[q][0] as f32;
                        let cy = C[q][1] as f32;
                        pxx += neq * cx * cx;
                        pxy += neq * cx * cy;
                        pyy += neq * cy * cy;
                    }
                    let norm = (pxx * pxx + 2.0 * pxy * pxy + pyy * pyy).sqrt();
                    let tau = 0.5
                        * (tau0
                            + (tau0 * tau0
                                + 18.0
                                    * std::f32::consts::SQRT_2
                                    * self.smagorinsky
                                    * self.smagorinsky
                                    * norm
                                    / rho)
                                .sqrt());
                    let relax = 1.0 - 1.0 / tau;
                    // Second-order Hermite projection removes nonhydrodynamic
                    // moments while preserving density, momentum and stress.
                    for q in 0..9 {
                        let cx = C[q][0] as f32;
                        let cy = C[q][1] as f32;
                        let reg = 4.5
                            * W[q]
                            * ((cx * cx - 1.0 / 3.0) * pxx
                                + 2.0 * cx * cy * pxy
                                + (cy * cy - 1.0 / 3.0) * pyy);
                        self.next[i][q] = eq[q] + relax * reg;
                    }
                }
            }
            std::mem::swap(&mut self.populations, &mut self.next);
            self.force = force;
            self.step += 1;
        }
        Ok(())
    }
    pub fn snapshot(&self) -> Snapshot {
        let mut velocity = vec![[0.0; 2]; self.solid.len()];
        let mut pressure = vec![0.0; self.solid.len()];
        let mut mean_density = 0.0;
        let mut count = 0;
        let mut min_ux = 1.0_f32;
        for (i, f) in self.populations.iter().enumerate() {
            if self.solid[i] {
                continue;
            }
            let rho: f32 = f.iter().sum();
            mean_density += rho as f64;
            count += 1;
            velocity[i] = [
                (f[1] - f[3] + f[5] - f[6] - f[7] + f[8]) / (rho * INFLOW),
                (f[2] - f[4] + f[5] + f[6] - f[7] - f[8]) / (rho * INFLOW),
            ];
            pressure[i] = (rho - 1.0) / (1.5 * INFLOW * INFLOW);
            min_ux = min_ux.min(velocity[i][0]);
        }
        let scale = 0.5 * INFLOW * INFLOW * self.config.chord_cells as f32;
        Snapshot {
            width: self.width,
            height: self.height,
            cells_per_chord: self.config.chord_cells as f32,
            step: self.step,
            time: self.step as f64 * INFLOW as f64 / self.config.chord_cells as f64,
            velocity,
            pressure,
            solid: self.solid.clone(),
            drag: self.force[0] / scale,
            lift: self.force[1] / scale,
            mean_density: mean_density / count as f64,
            min_ux,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glenn_core::model::Simulation;
    fn foil(alpha: f64) -> Solver {
        let state = Simulation {
            camber: 0.0,
            ..Simulation::default()
        };
        Solver::new(Config::airfoil(&Airfoil::generate(&state), alpha, 1e6, 24)).unwrap()
    }
    #[test]
    fn uniform_flow_preserves_mass_and_velocity() {
        let mut s = Solver::new(Config {
            chord_cells: 24,
            reynolds: 1000.0,
            alpha: 0.0,
            contour: vec![],
        })
        .unwrap();
        s.advance(400).unwrap();
        let field = s.snapshot();
        assert!((field.mean_density - 1.0).abs() < 1e-5);
        assert!(
            field
                .velocity
                .iter()
                .all(|u| (u[0] - 1.0).abs() < 1e-4 && u[1].abs() < 1e-4)
        );
    }
    #[test]
    fn shear_wave_decays_at_the_prescribed_viscosity() {
        let mut s = Solver::new(Config {
            chord_cells: 24,
            reynolds: 30.0,
            alpha: 0.0,
            contour: vec![],
        })
        .unwrap();
        s.periodic = true;
        s.smagorinsky = 0.0;
        let k = std::f32::consts::TAU / s.height as f32;
        for i in 0..s.populations.len() {
            s.populations[i] = equilibrium(1.0, [0.01 * (k * (i / s.width) as f32).sin(), 0.0]);
            s.source[i] = std::array::from_fn(|q| {
                let x = ((i % s.width) as i32 - C[q][0]).rem_euclid(s.width as i32);
                let y = ((i / s.width) as i32 - C[q][1]).rem_euclid(s.height as i32);
                y as usize * s.width + x as usize
            });
        }
        s.advance(500).unwrap();
        let f = s.snapshot();
        let amplitude: f32 = f
            .velocity
            .iter()
            .enumerate()
            .map(|(i, u)| 2.0 * u[0] * INFLOW * (k * (i / s.width) as f32).sin())
            .sum::<f32>()
            / f.velocity.len() as f32;
        let nu = INFLOW * 24.0 / 30.0;
        let expected = 0.01 * (-nu * k * k * 500.0).exp();
        assert!(
            (amplitude / expected - 1.0).abs() < 0.02,
            "{amplitude} vs {expected}"
        );
        assert!((f.mean_density - 1.0).abs() < 1e-4);
    }
    #[test]
    fn symmetric_body_has_zero_lift_and_reflected_flow() {
        let mut s = foil(0.0);
        s.advance(700).unwrap();
        let f = s.snapshot();
        assert!(f.lift.abs() < 0.002, "lift {}", f.lift);
        for y in 1..f.height / 2 {
            for x in 1..f.width - 1 {
                let a = y * f.width + x;
                let b = (f.height - y - 1) * f.width + x;
                assert_eq!(f.solid[a], f.solid[b]);
                assert!((f.velocity[a][0] - f.velocity[b][0]).abs() < 0.002);
                assert!((f.velocity[a][1] + f.velocity[b][1]).abs() < 0.002);
            }
        }
    }
    #[test]
    fn separation_is_computed_and_reverses_with_incidence() {
        let mut a = foil(24.0);
        let mut b = foil(-24.0);
        a.advance(1000).unwrap();
        b.advance(1000).unwrap();
        let a = a.snapshot();
        let b = b.snapshot();
        assert!(a.min_ux < -0.1 && b.min_ux < -0.1);
        assert!(a.lift > 0.0 && b.lift < 0.0);
        assert!((a.lift + b.lift).abs() < 0.01);
        assert!((a.drag - b.drag).abs() < 0.01);
        assert!((a.mean_density - 1.0).abs() < 0.01);
        let seed = (0..a.velocity.len())
            .find_map(|i| {
                let p = a.point(i % a.width, i / a.width);
                a.sample(p[0], p[1]).filter(|u| u[0] < -0.1).map(|_| p)
            })
            .expect("a resolved reverse-flow seed");
        let path = a.trace(seed);
        assert!(path.len() > 1 && path[1][0] < path[0][0]);
        assert!(path.iter().all(|p| a.sample(p[0], p[1]).is_some()));

        // Away from the body the inlet stays downstream; the entire field
        // must not be reversed by an angle-dependent visual effect.
        assert!(a.sample(-1.0, 1.0).unwrap()[0] > 0.9);
    }
}
