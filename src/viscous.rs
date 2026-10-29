//! The native viscous section, presented to the three-dimensional tunnel field
//! the same way the panel section always was: a solved plane applied along the
//! span. Only the source changes, so separation in the streamlines is computed
//! rather than drawn.
use crate::model::Point;
use crate::physics::flow::SectionField;
use glenn_solver_native::Snapshot;
use std::sync::Arc;

/// The lattice holds the body rotated by −alpha about the quarter chord with the
/// stream along +x; the section frame holds the body level and inclines the
/// stream instead. One rotation about the quarter chord maps between them.
pub struct Section {
    field: Arc<Snapshot>,
    sin: f64,
    cos: f64,
    /// Upper bounds of the solved box in lattice coordinates; the lower ones are
    /// fixed at the solver's own origin offsets.
    far: [f64; 2],
}

/// Distance over which the surrounding potential field takes over, in chords.
const FADE: f64 = 0.5;

impl Section {
    pub fn new(field: Arc<Snapshot>, alpha_degrees: f64) -> Self {
        let (sin, cos) = alpha_degrees.to_radians().sin_cos();
        let cells = f64::from(field.cells_per_chord);
        Self {
            far: [
                field.width as f64 / cells - 1.5,
                field.height as f64 / cells - 2.0,
            ],
            field,
            sin,
            cos,
        }
    }

    fn to_lattice(&self, p: Point) -> (f64, f64) {
        (
            0.25 + (p.x - 0.25) * self.cos + p.y * self.sin,
            -(p.x - 0.25) * self.sin + p.y * self.cos,
        )
    }
}

impl SectionField for Section {
    fn sample(&self, point: Point) -> Option<Point> {
        let (x, y) = self.to_lattice(point);
        let u = self.field.sample(x as f32, y as f32)?;
        let (ux, uy) = (f64::from(u[0]), f64::from(u[1]));
        Some(Point::new(
            ux * self.cos - uy * self.sin,
            ux * self.sin + uy * self.cos,
        ))
    }

    fn weight(&self, point: Point) -> f64 {
        let (x, y) = self.to_lattice(point);
        let margin = (x + 1.5)
            .min(self.far[0] - x)
            .min(y + 2.0)
            .min(self.far[1] - y);
        (margin / FADE).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Airfoil;
    use crate::model::Simulation;
    use glenn_solver_native::{Config, Solver};

    /// Undisturbed air far upstream must arrive in the section frame as the
    /// free stream at incidence, which is what the panel bases give there. A
    /// sign slipped in either rotation shows up here as a tilt the wrong way.
    #[test]
    fn far_upstream_air_returns_the_free_stream_at_incidence() {
        for alpha in [-12.0, 0.0, 18.0] {
            let state = Simulation {
                alpha,
                ..Simulation::default()
            };
            let foil = Airfoil::generate(&state);
            let solver = Solver::new(Config::airfoil(&foil, alpha, 1.0e6, 24)).unwrap();
            let section = Section::new(Arc::new(solver.snapshot()), alpha);
            let upstream = section.sample(Point::new(-1.0, 0.0)).unwrap();
            let (sin, cos) = alpha.to_radians().sin_cos();
            assert!(
                (upstream.x - cos).abs() < 0.02 && (upstream.y - sin).abs() < 0.02,
                "{alpha}°: {upstream:?} is not the free stream"
            );
            assert!(section.weight(Point::new(-1.0, 0.0)) > 0.99);
            // Outside the solved box the potential field must take over fully.
            assert_eq!(section.weight(Point::new(-3.0, 0.0)), 0.0);
        }
    }

    /// The rotation must put the solved body where the section says the body
    /// is: a point just inside the surface has no fluid velocity to report.
    #[test]
    fn the_solved_body_coincides_with_the_section_profile() {
        let state = Simulation {
            alpha: 15.0,
            ..Simulation::default()
        };
        let foil = Airfoil::generate(&state);
        let solver = Solver::new(Config::airfoil(&foil, 15.0, 1.0e6, 40)).unwrap();
        let section = Section::new(Arc::new(solver.snapshot()), 15.0);
        let thickness = crate::geometry::interpolate(&foil.upper, 0.3);
        assert!(section.sample(Point::new(0.3, thickness * 0.5)).is_none());
        assert!(section.sample(Point::new(0.3, thickness + 0.4)).is_some());
    }
}
