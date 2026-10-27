//! Constant-source Hess–Smith panels with one global vortex strength and Kutta row.
//!
//! Geometry preparation is O(N³). Two unit-freestream solutions are cached;
//! every subsequent incidence evaluation is O(N), including a full polar sweep.
use crate::{geometry::Airfoil, model::Point};
use std::f64::consts::PI;
#[derive(Debug, Clone)]
struct Panel {
    start: Point,
    center: Point,
    tangent: Point,
    normal: Point,
    length: f64,
}
#[derive(Debug, Clone)]
pub struct PanelModel {
    panels: Vec<Panel>,
    upper_count: usize,
    tangent_x: Vec<f64>,
    tangent_y: Vec<f64>,
    strengths: Vec<[f64; 2]>,
}
#[derive(Debug, Clone, Default)]
pub struct Pressure {
    pub upper: Vec<Point>,
    pub lower: Vec<Point>,
    pub lift: f64,
    pub moment: f64,
}
fn dot(a: Point, b: Point) -> f64 {
    a.x * b.x + a.y * b.y
}
fn source_velocity(panel: &Panel, point: Point) -> Point {
    let delta = Point::new(point.x - panel.start.x, point.y - panel.start.y);
    let x = dot(delta, panel.tangent);
    let y = -delta.x * panel.tangent.y + delta.y * panel.tangent.x;
    let r1 = (x * x + y * y).max(1e-24);
    let r2 = ((x - panel.length).powi(2) + y * y).max(1e-24);
    let u = (r1 / r2).ln() / (4.0 * PI);
    let v = (y * panel.length).atan2(x * (x - panel.length) + y * y) / (2.0 * PI);
    Point::new(
        u * panel.tangent.x - v * panel.tangent.y,
        u * panel.tangent.y + v * panel.tangent.x,
    )
}

/// Partial-pivot Gaussian elimination for multiple right-hand sides.
/// Singular geometry is an error, never silently regularized into plausible output.
fn solve(mut matrix: Vec<Vec<f64>>, mut rhs: Vec<[f64; 2]>) -> Result<Vec<[f64; 2]>, String> {
    let n = rhs.len();
    for k in 0..n {
        let pivot = (k..n)
            .max_by(|&a, &b| matrix[a][k].abs().total_cmp(&matrix[b][k].abs()))
            .unwrap();
        if matrix[pivot][k].abs() < 1e-11 {
            return Err(
                "Вырожденная геометрия: панельная система не имеет устойчивого решения".into(),
            );
        }
        matrix.swap(k, pivot);
        rhs.swap(k, pivot);
        for i in k + 1..n {
            let factor = matrix[i][k] / matrix[k][k];
            let (pivot_rows, following_rows) = matrix.split_at_mut(i);
            for (value, pivot_value) in following_rows[0][k..].iter_mut().zip(&pivot_rows[k][k..]) {
                *value -= factor * pivot_value;
            }
            let pivot_rhs = rhs[k];
            for (value, pivot_value) in rhs[i].iter_mut().zip(pivot_rhs) {
                *value -= factor * pivot_value;
            }
        }
    }
    let mut solution = vec![[0.0; 2]; n];
    for i in (0..n).rev() {
        for axis in 0..2 {
            let sum: f64 = (i + 1..n).map(|j| matrix[i][j] * solution[j][axis]).sum();
            solution[i][axis] = (rhs[i][axis] - sum) / matrix[i][i];
        }
    }
    Ok(solution)
}
impl PanelModel {
    pub fn prepare(foil: &Airfoil) -> Result<Self, String> {
        let points = foil.contour();
        let n = points.len();
        let mut panels = Vec::with_capacity(n);
        for i in 0..n {
            let (a, b) = (points[i], points[(i + 1) % n]);
            let length = (b.x - a.x).hypot(b.y - a.y);
            if length < 1e-9 {
                return Err("Совпадающие узлы профиля".into());
            }
            let tangent = Point::new((b.x - a.x) / length, (b.y - a.y) / length);
            panels.push(Panel {
                start: a,
                center: Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5),
                tangent,
                normal: Point::new(tangent.y, -tangent.x),
                length,
            });
        }
        let mut normal = vec![vec![0.0; n + 1]; n + 1];
        let mut tangent = vec![vec![0.0; n + 1]; n];
        let mut rhs = vec![[0.0; 2]; n + 1];
        for i in 0..n {
            rhs[i] = [-panels[i].normal.x, -panels[i].normal.y];
            for j in 0..n {
                let velocity = if i == j {
                    Point::new(0.5 * panels[i].normal.x, 0.5 * panels[i].normal.y)
                } else {
                    source_velocity(&panels[j], panels[i].center)
                };
                let vortex = Point::new(-velocity.y, velocity.x);
                normal[i][j] = dot(velocity, panels[i].normal);
                tangent[i][j] = dot(velocity, panels[i].tangent);
                normal[i][n] += dot(vortex, panels[i].normal);
                tangent[i][n] += dot(vortex, panels[i].tangent);
            }
        }
        // Oppositely directed TE panels must have equal-speed outgoing flow.
        for j in 0..=n {
            normal[n][j] = tangent[0][j] + tangent[n - 1][j];
        }
        rhs[n] = [
            -panels[0].tangent.x - panels[n - 1].tangent.x,
            -panels[0].tangent.y - panels[n - 1].tangent.y,
        ];
        let strengths = solve(normal, rhs)?;
        let response = |axis: usize| -> Vec<f64> {
            (0..n)
                .map(|i| {
                    let free = if axis == 0 {
                        panels[i].tangent.x
                    } else {
                        panels[i].tangent.y
                    };
                    free + (0..=n)
                        .map(|j| tangent[i][j] * strengths[j][axis])
                        .sum::<f64>()
                })
                .collect()
        };
        let tangent_x = response(0);
        let tangent_y = response(1);
        Ok(Self {
            panels,
            upper_count: foil.upper.len() - 1,
            tangent_x,
            tangent_y,
            strengths,
        })
    }
    /// Velocity response to unit free streams along the section x and y axes.
    /// Uses the same source/vortex strengths as Cp, including the Kutta condition.
    /// Coordinates and velocities are dimensionless (chord and free-stream speed).
    pub fn velocity_basis(&self, point: Point) -> [Point; 2] {
        let mut velocity = [Point::new(1.0, 0.0), Point::new(0.0, 1.0)];
        let circulation = self.strengths[self.panels.len()];
        for (index, panel) in self.panels.iter().enumerate() {
            let source = source_velocity(panel, point);
            for axis in 0..2 {
                velocity[axis].x +=
                    source.x * self.strengths[index][axis] - source.y * circulation[axis];
                velocity[axis].y +=
                    source.y * self.strengths[index][axis] + source.x * circulation[axis];
            }
        }
        velocity
    }

    pub fn velocity_at(&self, point: Point, alpha_degrees: f64) -> Point {
        let [x_response, y_response] = self.velocity_basis(point);
        let (sin, cos) = alpha_degrees.to_radians().sin_cos();
        Point::new(
            x_response.x * cos + y_response.x * sin,
            x_response.y * cos + y_response.y * sin,
        )
    }

    pub fn evaluate(&self, alpha_degrees: f64, mach: f64) -> Pressure {
        let (sin, cos) = alpha_degrees.to_radians().sin_cos();
        let speeds: Vec<_> = self
            .tangent_x
            .iter()
            .zip(&self.tangent_y)
            .map(|(x, y)| x * cos + y * sin)
            .collect();
        self.pressure_from_speeds(alpha_degrees, mach, &speeds)
    }

    /// Analytic circular-cylinder solution; rotation changes both Cp and flow.
    /// Positive spin corresponds to clockwise circulation and positive lift.
    pub fn cylinder_pressure(&self, alpha_degrees: f64, spin_ratio: f64) -> Pressure {
        let speeds: Vec<_> = self
            .panels
            .iter()
            .map(|panel| {
                let theta = panel.center.y.atan2(panel.center.x - 0.5);
                -2.0 * (theta - alpha_degrees.to_radians()).sin() - spin_ratio
            })
            .collect();
        self.pressure_from_speeds(alpha_degrees, 0.0, &speeds)
    }
    fn pressure_from_speeds(&self, alpha_degrees: f64, mach: f64, speeds: &[f64]) -> Pressure {
        let (sin, cos) = alpha_degrees.to_radians().sin_cos();
        // Bounded Prandtl–Glauert correction, not a transonic solution.
        let beta = (1.0 - mach.min(0.8).powi(2)).sqrt();
        let mut out = Pressure::default();
        for (i, p) in self.panels.iter().enumerate() {
            let velocity = speeds[i];
            let cp = (1.0 - velocity * velocity) / beta;
            let (fx, fy) = (-cp * p.length * p.normal.x, -cp * p.length * p.normal.y);
            out.lift += -fx * sin + fy * cos;
            out.moment += -(p.center.x - 0.25) * fy + p.center.y * fx;
            let target = if i < self.upper_count {
                &mut out.upper
            } else {
                &mut out.lower
            };
            target.push(Point::new(p.center.x, cp));
        }
        out.upper.sort_by(|a, b| a.x.total_cmp(&b.x));
        out.lower.sort_by(|a, b| a.x.total_cmp(&b.x));
        out
    }
}

/// Unit-speed potential flow around a circular cylinder of radius 0.5 chord.
/// The doublet enforces zero radial velocity and circulation supplies Magnus lift.
pub fn cylinder_velocity(point: Point, incidence: f64, spin_ratio: f64) -> Point {
    let x = point.x - 0.5;
    let y = point.y;
    let radius = x.hypot(y).max(1e-8);
    let theta = y.atan2(x);
    let radial = (theta - incidence).cos() * (1.0 - 0.25 / (radius * radius));
    let tangential =
        -(theta - incidence).sin() * (1.0 + 0.25 / (radius * radius)) - spin_ratio * 0.5 / radius;
    Point::new(
        radial * theta.cos() - tangential * theta.sin(),
        radial * theta.sin() + tangential * theta.cos(),
    )
}
