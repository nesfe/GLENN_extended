//! Integral boundary layer along one surface of the section.
//!
//! The panel solution is inviscid, so on its own it never leaves the wall: that
//! is why stall used to be a curve fit bolted onto the forces. Here the viscous
//! layer is marched along the surface and separation is located, which is what
//! actually ends the attached flow.
//!
//! Laminar part: Thwaites' correlation, separating at λ = −0.09.
//! Transition: Michel's criterion on Re_θ against Re_s.
//! Turbulent part: Head's entrainment method with the Ludwieg–Tillmann skin
//! friction, separating at H = 2.4.
//!
//! Lengths are in chords and speeds in free-stream units, so the Reynolds
//! number carries the viscosity: ν = 1/Re.

/// Edge conditions sampled along one surface, from the stagnation point.
pub struct Surface {
    /// Arc length from the stagnation point, in chords. Strictly increasing.
    pub s: Vec<f64>,
    /// Chordwise station of each sample, in chords.
    pub x: Vec<f64>,
    /// Edge speed over free-stream speed. Non-negative.
    pub ue: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    Laminar,
    Turbulent,
}

#[derive(Debug, Clone, Copy)]
pub struct Layer {
    /// Chordwise station where the layer separates, if it does before the edge.
    pub separation: Option<f64>,
    /// Chordwise station where the layer becomes turbulent, if it does.
    pub transition: Option<f64>,
    /// Momentum thickness at the last marched station, in chords.
    pub momentum_thickness: f64,
    pub state: State,
}

/// Head's shape-factor relation, inverted for the entrainment variable.
fn entrainment_shape(h: f64) -> f64 {
    if h <= 1.6 {
        3.3 + 0.8234 * (h - 1.1).max(1e-3).powf(-1.287)
    } else {
        3.3 + 1.5501 * (h - 0.6778).powf(-3.064)
    }
}

/// Inverse of `entrainment_shape`, needed because Head marches H1, not H.
fn shape_from_entrainment(h1: f64) -> f64 {
    if h1 <= 3.3 {
        return 3.0;
    }
    if h1 < 5.3 {
        0.6778 + 1.1536 * (h1 - 3.3).powf(-0.326)
    } else {
        1.1 + 0.86 * (h1 - 3.3).powf(-0.777)
    }
}

/// March the layer and report where it separates.
///
/// `reynolds` is based on the chord and the free-stream speed.
pub fn march(surface: &Surface, reynolds: f64) -> Layer {
    let n = surface.s.len();
    let reynolds = reynolds.max(1.0e3);
    let mut layer = Layer {
        separation: None,
        transition: None,
        momentum_thickness: 0.0,
        state: State::Laminar,
    };
    if n < 3 {
        return layer;
    }
    // Thwaites' integral needs the running ∫ue⁵ds; the leading edge starts it.
    let mut integral = 0.0;
    let mut theta = 0.0;
    let mut shape = 2.3;
    let mut entrainment = entrainment_shape(shape);
    for i in 1..n {
        let ds = surface.s[i] - surface.s[i - 1];
        if ds <= 0.0 {
            continue;
        }
        let ue = surface.ue[i].max(1e-6);
        let previous = surface.ue[i - 1].max(1e-6);
        let gradient = (ue - previous) / ds;
        match layer.state {
            State::Laminar => {
                integral += 0.5 * (ue.powi(5) + previous.powi(5)) * ds;
                theta = (0.45 * integral / (reynolds * ue.powi(6))).sqrt();
                let lambda = reynolds * theta * theta * gradient;
                if lambda <= -0.09 {
                    layer.separation = Some(surface.x[i]);
                    layer.momentum_thickness = theta;
                    return layer;
                }
                // Thwaites' correlation for the shape factor, used at transition.
                shape = if lambda < 0.0 {
                    2.088 + 0.0731 / (lambda + 0.14)
                } else {
                    2.61 - 3.75 * lambda + 5.24 * lambda * lambda
                }
                .clamp(1.35, 3.5);
                let momentum_reynolds = reynolds * ue * theta;
                let length_reynolds = (reynolds * ue * surface.s[i]).max(1.0);
                if momentum_reynolds
                    >= 1.174 * (1.0 + 22400.0 / length_reynolds) * length_reynolds.powf(0.46)
                {
                    layer.state = State::Turbulent;
                    layer.transition = Some(surface.x[i]);
                    shape = shape.min(1.6);
                    entrainment = entrainment_shape(shape);
                }
            }
            State::Turbulent => {
                let momentum_reynolds = (reynolds * ue * theta).max(1.0);
                let friction = 0.246 * 10f64.powf(-0.678 * shape) * momentum_reynolds.powf(-0.268);
                let d_theta = friction / 2.0 - (shape + 2.0) * theta * gradient / ue;
                // Head's entrainment equation, marched as ue·θ·H1.
                let flux = ue * theta * entrainment;
                let d_flux = ue * 0.0306 * (entrainment - 3.0).max(1e-3).powf(-0.6169);
                theta = (theta + d_theta * ds).max(1e-7);
                entrainment = ((flux + d_flux * ds) / (ue * theta)).max(3.0 + 1e-3);
                shape = shape_from_entrainment(entrainment);
                if shape >= 2.4 {
                    layer.separation = Some(surface.x[i]);
                    layer.momentum_thickness = theta;
                    return layer;
                }
            }
        }
    }
    layer.momentum_thickness = theta;
    layer
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flat plate at zero incidence: no adverse gradient, so no separation,
    /// and the laminar thickness must follow Blasius within the correlation's
    /// own accuracy (Thwaites gives 0.671/√Re_x against Blasius' 0.664).
    fn flat_plate(samples: usize, length: f64) -> Surface {
        let s: Vec<f64> = (0..samples)
            .map(|i| length * i as f64 / (samples - 1) as f64)
            .collect();
        Surface {
            x: s.clone(),
            ue: vec![1.0; samples],
            s,
        }
    }

    #[test]
    fn flat_plate_matches_blasius_and_never_separates() {
        let surface = flat_plate(400, 1.0);
        let reynolds = 1.0e5;
        let layer = march(&surface, reynolds);
        assert!(layer.separation.is_none(), "zero gradient cannot separate");
        let blasius = 0.664 / reynolds.sqrt();
        let error = (layer.momentum_thickness - blasius).abs() / blasius;
        assert!(
            error < 0.02,
            "θ={} vs Blasius {blasius}",
            layer.momentum_thickness
        );
    }

    fn retarded_flow(length: f64, slope: f64, samples: usize) -> Surface {
        let s: Vec<f64> = (0..samples)
            .map(|i| length * i as f64 / (samples - 1) as f64)
            .collect();
        let ue: Vec<f64> = s.iter().map(|s| (1.0 - slope * s).max(1e-6)).collect();
        Surface {
            x: s.clone(),
            ue,
            s,
        }
    }

    /// Howarth's linearly retarded flow ue = 1 − s/8. Thwaites puts separation
    /// at λ = −0.09, which for this family reduces to λ = −0.075(ue⁻⁶ − 1) and
    /// so to s/8 = 0.123; the exact solution gives 0.120.
    ///
    /// The Reynolds number is low enough that Michel's criterion does not call
    /// transition first - this checks the laminar branch on its own.
    #[test]
    fn howarth_flow_separates_where_theory_says() {
        let surface = retarded_flow(8.0, 1.0 / 8.0, 2000);
        let layer = march(&surface, 1.0e4);
        assert_eq!(layer.state, State::Laminar);
        let separation = layer.separation.expect("retarded flow must separate");
        assert!(
            (separation / 8.0 - 0.123).abs() < 0.01,
            "separation at s/8={}",
            separation / 8.0
        );
    }

    /// A turbulent layer survives an adverse gradient that detaches a laminar
    /// one: the physical reason tripped wings and dimpled balls hold on longer.
    /// Raising the Reynolds number moves transition ahead of laminar separation.
    #[test]
    fn turbulent_layer_outlasts_the_laminar_one() {
        let surface = retarded_flow(1.0, 0.55, 1500);
        let laminar = march(&surface, 1.0e4);
        let turbulent = march(&surface, 1.0e7);
        assert_eq!(laminar.state, State::Laminar);
        assert_eq!(turbulent.state, State::Turbulent);
        assert!(turbulent.transition.is_some());
        let early = laminar.separation.expect("laminar layer separates here");
        let late = turbulent.separation.unwrap_or(1.0);
        assert!(
            late > early,
            "turbulent {late} should outlast laminar {early}"
        );
    }
}
