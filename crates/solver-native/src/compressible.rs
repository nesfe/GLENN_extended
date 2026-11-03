//! Conservative 2D compressible Navier–Stokes, ideal calorically perfect gas.
//! MUSCL/MC reconstruction, HLLE flux, SSP-RK2; Newtonian stress and Fourier heat
//! conduction. Units: chord=U_inf=rho_inf=1. This is a coarse section calculation,
//! not a replacement for a grid-converged three-dimensional flight calculation.
use crate::{Lattice, Snapshot};

type State = [f64; 4]; // conserved: rho, rho*u, rho*v, total energy
// Primitive: rho, u, v, p. Temperature below means R*T in velocity-squared units.
const PR: f64 = 0.72;

fn conserved(w: State, gamma: f64) -> State {
    [
        w[0],
        w[0] * w[1],
        w[0] * w[2],
        w[3] / (gamma - 1.0) + 0.5 * w[0] * (w[1] * w[1] + w[2] * w[2]),
    ]
}
fn primitive(q: State, gamma: f64) -> State {
    let (u, v) = (q[1] / q[0], q[2] / q[0]);
    [
        q[0],
        u,
        v,
        (gamma - 1.0) * (q[3] - 0.5 * (q[1] * u + q[2] * v)),
    ]
}
fn valid(w: State) -> bool {
    w.iter().all(|x| x.is_finite()) && w[0] > 1e-10 && w[3] > 1e-10
}
fn euler_flux(w: State, axis: usize, gamma: f64) -> State {
    let q = conserved(w, gamma);
    let un = w[axis + 1];
    let mut f = [q[0] * un, q[1] * un, q[2] * un, (q[3] + w[3]) * un];
    f[axis + 1] += w[3];
    f
}
/// Einfeldt wave-speed bounds include the Roe-averaged acoustic waves.
fn hlle(l: State, r: State, axis: usize, gamma: f64) -> State {
    let (ul, ur) = (l[axis + 1], r[axis + 1]);
    let (al, ar) = ((gamma * l[3] / l[0]).sqrt(), (gamma * r[3] / r[0]).sqrt());
    let (ql, qr) = (conserved(l, gamma), conserved(r, gamma));
    let (rl, rr) = (l[0].sqrt(), r[0].sqrt());
    let u = (rl * l[1] + rr * r[1]) / (rl + rr);
    let v = (rl * l[2] + rr * r[2]) / (rl + rr);
    let h = ((ql[3] + l[3]) / rl + (qr[3] + r[3]) / rr) / (rl + rr);
    let a = ((gamma - 1.0) * (h - 0.5 * (u * u + v * v)))
        .max(0.0)
        .sqrt();
    let un = if axis == 0 { u } else { v };
    let sl = (ul - al).min(ur - ar).min(un - a);
    let sr = (ul + al).max(ur + ar).max(un + a);
    let (fl, fr) = (euler_flux(l, axis, gamma), euler_flux(r, axis, gamma));
    if sl >= 0.0 {
        return fl;
    }
    if sr <= 0.0 {
        return fr;
    }
    std::array::from_fn(|k| (sr * fl[k] - sl * fr[k] + sl * sr * (qr[k] - ql[k])) / (sr - sl))
}
fn mc(a: f64, b: f64) -> f64 {
    if a * b <= 0.0 {
        0.0
    } else {
        a.signum() * (2.0 * a.abs()).min(2.0 * b.abs()).min(0.5 * (a + b).abs())
    }
}

pub struct Compressible {
    width: usize,
    height: usize,
    cells: usize,
    solid: Vec<bool>,
    q: Vec<State>,
    stage: Vec<State>,
    rhs: Vec<State>,
    w: Vec<State>,
    slopes: Vec<[State; 2]>,
    gradients: Vec<[[f64; 3]; 2]>, // central gradients of u,v,R*T
    far: State,
    gamma: f64,
    mu: f64,
    time: f64,
    step: u64,
    force: [f64; 2],
    periodic: bool,
}
impl Compressible {
    pub(crate) fn new(lattice: Lattice) -> Result<Self, String> {
        let c = &lattice.config;
        let far = [1.0, 1.0, 0.0, 1.0 / (c.gamma * c.mach * c.mach)];
        if !valid(far) {
            return Err("Недопустимое состояние сжимаемого газа".into());
        }
        let size = lattice.width * lattice.height;
        Ok(Self {
            width: lattice.width,
            height: lattice.height,
            cells: c.chord_cells,
            solid: lattice.solid,
            q: vec![conserved(far, c.gamma); size],
            stage: vec![[0.0; 4]; size],
            rhs: vec![[0.0; 4]; size],
            w: vec![far; size],
            slopes: vec![[[0.0; 4]; 2]; size],
            gradients: vec![[[0.0; 3]; 2]; size],
            far,
            gamma: c.gamma,
            mu: 1.0 / c.reynolds,
            time: 0.0,
            step: 0,
            force: [0.0; 2],
            periodic: false,
        })
    }
    fn neighbor(&self, i: usize, axis: usize, sign: isize) -> Option<usize> {
        let (mut x, mut y) = ((i % self.width) as isize, (i / self.width) as isize);
        if axis == 0 {
            x += sign;
        } else {
            y += sign;
        }
        if self.periodic {
            x = x.rem_euclid(self.width as isize);
            y = y.rem_euclid(self.height as isize);
        }
        (x >= 0 && y >= 0 && x < self.width as isize && y < self.height as isize)
            .then_some((y.max(0) as usize) * self.width + x.max(0) as usize)
    }
    fn ghost(&self, i: usize, j: Option<usize>) -> State {
        match j {
            Some(j) if !self.solid[j] => self.w[j],
            Some(_) => {
                let mut w = self.w[i];
                w[1] = -w[1];
                w[2] = -w[2];
                w
            }
            None => self.far,
        }
    }
    /// Rebuild primitives and derivatives. No density/pressure clipping: an
    /// inadmissible stage is retried at a smaller dt by the caller.
    fn prepare(&mut self) -> Result<f64, String> {
        let mut rate: f64 = 0.0;
        let n = self.cells as f64;
        for i in 0..self.q.len() {
            if self.solid[i] {
                continue;
            }
            let w = primitive(self.q[i], self.gamma);
            if !valid(w) {
                return Err(format!(
                    "Сжимаемый расчёт: недопустимые плотность или давление на шаге {}",
                    self.step
                ));
            }
            self.w[i] = w;
            let a = (self.gamma * w[3] / w[0]).sqrt();
            rate = rate.max(
                n * (w[1].abs() + w[2].abs() + 2.0 * a)
                    + 4.0 * n * n * self.mu / w[0] * (4.0_f64 / 3.0).max(self.gamma / PR),
            );
        }
        for i in 0..self.q.len() {
            if self.solid[i] {
                continue;
            }
            let w = self.w[i];
            for axis in 0..2 {
                let jm = self.neighbor(i, axis, -1);
                let jp = self.neighbor(i, axis, 1);
                let (m, p) = (self.ghost(i, jm), self.ghost(i, jp));
                let interior =
                    jm.is_some_and(|j| !self.solid[j]) && jp.is_some_and(|j| !self.solid[j]);
                self.slopes[i][axis] = std::array::from_fn(|k| {
                    if interior {
                        mc(w[k] - m[k], p[k] - w[k])
                    } else {
                        0.0
                    }
                });
                self.gradients[i][axis] = [
                    0.5 * n * (p[1] - m[1]),
                    0.5 * n * (p[2] - m[2]),
                    0.5 * n * (p[3] / p[0] - m[3] / m[0]),
                ];
            }
        }
        Ok(0.4 / rate)
    }
    fn face_flux(&self, left: Option<usize>, right: Option<usize>, axis: usize) -> State {
        let wall_l = left.is_some_and(|i| self.solid[i]);
        let wall_r = right.is_some_and(|i| self.solid[i]);
        if wall_l && wall_r {
            return [0.0; 4];
        }
        let mut l = left.map_or(self.far, |i| self.w[i]);
        let mut r = right.map_or(self.far, |i| self.w[i]);
        let wall = wall_l || wall_r;
        if wall_l {
            l = r;
            l[axis + 1] = -r[axis + 1];
        }
        if wall_r {
            r = l;
            r[axis + 1] = -l[axis + 1];
        }
        // Reflect only normal velocity for the inviscid flux. Tangential
        // no-slip is imposed by the viscous gradient below.
        let mut lf = l;
        let mut rf = r;
        if !wall {
            if let Some(i) = left {
                lf = std::array::from_fn(|k| l[k] + 0.5 * self.slopes[i][axis][k]);
            }
            if let Some(i) = right {
                rf = std::array::from_fn(|k| r[k] - 0.5 * self.slopes[i][axis][k]);
            }
        }
        // Limiting density and pressure componentwise preserves positivity of
        // reconstructed states; retain a defensive first-order face fallback.
        if !valid(lf) {
            lf = l;
        }
        if !valid(rf) {
            rf = r;
        }
        let mut flux = hlle(lf, rf, axis, self.gamma);
        let n = self.cells as f64;
        let mut grad = [[0.0; 3]; 2];
        let velocity;
        if wall {
            let i = if wall_l {
                right.unwrap()
            } else {
                left.unwrap()
            };
            let sign = if wall_l { 1.0 } else { -1.0 };
            // Wall lies halfway to the solid-cell center. Stationary,
            // impermeable, no slip and adiabatic: no wall energy flux.
            grad[axis][0] = sign * 2.0 * n * self.w[i][1];
            grad[axis][1] = sign * 2.0 * n * self.w[i][2];
            velocity = [0.0, 0.0];
            flux[0] = 0.0;
            flux[3] = 0.0;
        } else {
            for (d, gradient) in grad.iter_mut().enumerate() {
                for (k, value) in gradient.iter_mut().enumerate() {
                    *value = 0.5
                        * (left.map_or(0.0, |i| self.gradients[i][d][k])
                            + right.map_or(0.0, |i| self.gradients[i][d][k]));
                }
            }
            grad[axis] = [
                n * (r[1] - l[1]),
                n * (r[2] - l[2]),
                n * (r[3] / r[0] - l[3] / l[0]),
            ];
            velocity = [0.5 * (l[1] + r[1]), 0.5 * (l[2] + r[2])];
        }
        let div = grad[0][0] + grad[1][1];
        let shear = self.mu * (grad[0][1] + grad[1][0]);
        let stress = if axis == 0 {
            [self.mu * (2.0 * grad[0][0] - 2.0 / 3.0 * div), shear]
        } else {
            [shear, self.mu * (2.0 * grad[1][1] - 2.0 / 3.0 * div)]
        };
        flux[1] -= stress[0];
        flux[2] -= stress[1];
        flux[3] -= velocity[0] * stress[0]
            + velocity[1] * stress[1]
            + self.mu * self.gamma / ((self.gamma - 1.0) * PR) * grad[axis][2];
        flux
    }
    fn accumulate(
        &mut self,
        left: Option<usize>,
        right: Option<usize>,
        axis: usize,
        force: &mut [f64; 2],
    ) {
        let flux = self.face_flux(left, right, axis);
        let n = self.cells as f64;
        for (index, sign) in [(left, -1.0), (right, 1.0)] {
            if let Some(i) = index {
                if self.solid[i] {
                    force[0] += sign * flux[1] / n;
                    force[1] += sign * flux[2] / n;
                } else {
                    for (k, f) in flux.iter().enumerate() {
                        self.rhs[i][k] += sign * n * f;
                    }
                }
            }
        }
    }
    fn residual(&mut self) -> [f64; 2] {
        self.rhs.fill([0.0; 4]);
        let mut force = [0.0; 2];
        for y in 0..self.height {
            for x in 0..=self.width {
                if self.periodic && x == self.width {
                    continue;
                }
                let l = if x > 0 {
                    Some(y * self.width + x - 1)
                } else if self.periodic {
                    Some(y * self.width + self.width - 1)
                } else {
                    None
                };
                let r = (x < self.width).then_some(y * self.width + x);
                self.accumulate(l, r, 0, &mut force);
            }
        }
        for y in 0..=self.height {
            if self.periodic && y == self.height {
                continue;
            }
            for x in 0..self.width {
                let l = if y > 0 {
                    Some((y - 1) * self.width + x)
                } else if self.periodic {
                    Some((self.height - 1) * self.width + x)
                } else {
                    None
                };
                let r = (y < self.height).then_some(y * self.width + x);
                self.accumulate(l, r, 1, &mut force);
            }
        }
        force
    }
    pub fn advance(&mut self, count: usize) -> Result<(), String> {
        for _ in 0..count {
            let mut dt = self.prepare()?;
            let first_force = self.residual();
            let initial = self.q.clone();
            let first_rhs = self.rhs.clone();
            let mut accepted = false;
            for _ in 0..10 {
                for i in 0..self.q.len() {
                    self.q[i] = std::array::from_fn(|k| initial[i][k] + dt * first_rhs[i][k]);
                }
                if self.prepare().is_ok() {
                    let second_force = self.residual();
                    let mut admissible = true;
                    for (i, initial) in initial.iter().enumerate() {
                        self.stage[i] = std::array::from_fn(|k| {
                            0.5 * (initial[k] + self.q[i][k] + dt * self.rhs[i][k])
                        });
                        if !self.solid[i] && !valid(primitive(self.stage[i], self.gamma)) {
                            admissible = false;
                        }
                    }
                    if admissible {
                        std::mem::swap(&mut self.q, &mut self.stage);
                        self.force =
                            std::array::from_fn(|k| 0.5 * (first_force[k] + second_force[k]));
                        accepted = true;
                        break;
                    }
                }
                dt *= 0.5;
            }
            if !accepted {
                self.q = initial;
                return Err(format!(
                    "Сжимаемый расчёт потерял устойчивость на шаге {}",
                    self.step
                ));
            }
            self.time += dt;
            self.step += 1;
        }
        Ok(())
    }
    pub fn snapshot(&self) -> Snapshot {
        let mut velocity = vec![[0.0; 2]; self.q.len()];
        let mut pressure = vec![0.0; self.q.len()];
        let (mut density, mut count, mut min_ux) = (0.0, 0, 1.0_f32);
        for i in 0..self.q.len() {
            if self.solid[i] {
                continue;
            }
            let w = primitive(self.q[i], self.gamma);
            velocity[i] = [w[1] as f32, w[2] as f32];
            pressure[i] = (2.0 * (w[3] - self.far[3])) as f32;
            density += w[0];
            count += 1;
            min_ux = min_ux.min(w[1] as f32);
        }
        Snapshot {
            width: self.width,
            height: self.height,
            cells_per_chord: self.cells as f32,
            step: self.step,
            time: self.time,
            velocity,
            pressure,
            solid: self.solid.clone(),
            drag: (2.0 * self.force[0]) as f32,
            lift: (2.0 * self.force[1]) as f32,
            mean_density: density / count as f64,
            min_ux,
        }
    }
}
