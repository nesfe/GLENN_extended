//! Convert shared geometry and solver samples to triangles, without modifying
//! physical positions. Thin tubes remain visible from any camera direction.
use super::{LineInstance, Vertex};
use crate::ui::tunnel::{View, color};
use glam::{DVec3, Vec3};
use glenn_extended::{
    geometry::{body_to_world, interpolate},
    model::Point,
    physics::{aerodynamics::Analysis, flow::Streamline},
};

pub struct Scene {
    pub vertices: Vec<Vertex>,
    pub solid_count: u32,
    pub lines: Vec<LineInstance>,
}

/// Does the flow just off the skin run back towards the nose? Measured along
/// the local surface tangent, not along the chord: air rounding the leading
/// edge moves backwards in chord coordinates while still firmly attached.
fn separated(
    field: &dyn glenn_extended::physics::flow::SectionField,
    foil: &glenn_extended::geometry::Airfoil,
    local: Point,
    upper: bool,
) -> bool {
    let line = if upper { &foil.upper } else { &foil.lower };
    let (ahead, behind) = ((local.x - 0.02).max(0.0), (local.x + 0.02).min(1.0));
    let tangent = Point::new(
        behind - ahead,
        interpolate(line, behind) - interpolate(line, ahead),
    );
    let length = tangent.x.hypot(tangent.y);
    if length < 1e-9 {
        return false;
    }
    let tangent = Point::new(tangent.x / length, tangent.y / length);
    // Outward normal, flipped for the lower surface.
    let (nx, ny) = if upper {
        (-tangent.y, tangent.x)
    } else {
        (tangent.y, -tangent.x)
    };
    // Real separation has thickness. A single near-wall sample also goes
    // negative in the one-cell layer the lattice cannot resolve at this
    // Reynolds number, which would paint an attached section orange.
    [0.04, 0.08].iter().all(|offset| {
        let probe = Point::new(local.x + nx * offset, local.y + ny * offset);
        field
            .sample(probe)
            .is_some_and(|u| u.x * tangent.x + u.y * tangent.y < -0.02)
    })
}
impl Scene {
    /// Root-section viscous field in the actual wing frame. No spanwise
    /// extension of 2D velocities and no potential-pressure colouring.
    pub fn native(
        analysis: &Analysis,
        field: &glenn_solver_native::Snapshot,
        quantity: usize,
    ) -> Self {
        let mut scene = Self {
            vertices: Vec::new(),
            solid_count: 0,
            lines: Vec::new(),
        };
        let state = &analysis.state;
        for triangle in &analysis.mesh.triangles {
            let p = triangle.map(|i| {
                body_to_world(state, DVec3::from_array(analysis.mesh.vertices[i])).as_vec3()
            });
            let normal = (p[1] - p[0])
                .cross(p[2] - p[0])
                .normalize_or_zero()
                .to_array();
            for v in p {
                scene.vertices.push(Vertex {
                    position: v.to_array(),
                    normal,
                    color: [0.65, 0.75, 0.8, 1.0],
                    data: [0.0; 4],
                });
            }
        }
        scene.solid_count = scene.vertices.len() as u32;
        let chord = state.chord as f32;
        let h = 2.0 / field.cells_per_chord;
        for y in (0..field.height - 2).step_by(2) {
            for x in (0..field.width - 2).step_by(2) {
                let i = y * field.width + x;
                if field.solid[i] {
                    continue;
                }
                let point = field.point(x, y);
                let c =
                    crate::ui::native::field_color(field, i, quantity).to_normalized_gamma_f32();
                for [dx, dy] in [[0.0, 0.0], [h, 0.0], [h, h], [0.0, 0.0], [h, h], [0.0, h]] {
                    scene.vertices.push(Vertex {
                        position: [(point[0] + dx - 0.25) * chord, (point[1] + dy) * chord, 0.0],
                        normal: [0.0, 0.0, 1.0],
                        color: [c[0], c[1], c[2], 0.85],
                        data: [2.0; 4],
                    });
                }
            }
        }
        scene
    }
    pub fn new(
        analysis: &Analysis,
        paths: &[Streamline],
        view: &View,
        viscous: Option<&dyn glenn_extended::physics::flow::SectionField>,
    ) -> Self {
        let mut scene = Self {
            vertices: Vec::new(),
            solid_count: 0,
            lines: Vec::new(),
        };
        let state = &analysis.state;
        let positions: Vec<Vec3> = analysis
            .mesh
            .vertices
            .iter()
            .map(|p| body_to_world(state, DVec3::from_array(*p)).as_vec3())
            .collect();
        let mut normals = vec![Vec3::ZERO; positions.len()];
        for triangle in &analysis.mesh.triangles {
            let [a, b, c] = triangle.map(|i| positions[i]);
            let normal = (b - a).cross(c - a);
            for &i in triangle {
                normals[i] += normal;
            }
        }
        let contour = analysis.foil.contour();
        let section_size = contour.len();
        // Each section uses its own incidence, so twist also changes surface Cp.
        let mut pressures = Vec::new();
        for section in positions.chunks(section_size) {
            let eta = (section[0].z as f64 / (state.span * 0.5)).abs().min(1.0);
            pressures.push(
                if state.profile_type == glenn_extended::model::Profile::Cylinder {
                    let spin = (state.cylinder_rpm * std::f64::consts::TAU / 60.0
                        * state.mean_chord()
                        * 0.5
                        / state.velocity.max(0.1))
                    .clamp(-4.0, 4.0);
                    analysis
                        .panel
                        .cylinder_pressure(state.alpha + state.twist * eta, spin)
                } else {
                    analysis.panel.evaluate(
                        state.alpha + state.twist * eta,
                        analysis.current.mach * state.sweep.to_radians().cos(),
                    )
                },
            );
        }
        for triangle in &analysis.mesh.triangles {
            for &i in triangle {
                let local = contour[i % section_size];
                let pressure = &pressures[(i / section_size).min(pressures.len() - 1)];
                let upper = i % section_size < analysis.foil.upper.len() - 1;
                let surface = if upper {
                    &pressure.upper
                } else {
                    &pressure.lower
                };
                let cp = interpolate(surface, local.x);
                // Separated skin is marked on the wing itself: without it the
                // detachment is only a few hundredths of a chord thick and
                // disappears at any view that shows the whole wing.
                let detached =
                    viscous.is_some_and(|field| separated(field, &analysis.foil, local, upper));
                let rgb = if i >= positions.len() - 2 {
                    [0.36, 0.46, 0.55, 1.0]
                } else if detached {
                    // Crimson: no Cp palette reaches it, so separation never
                    // reads as just another pressure level.
                    [0.70, 0.08, 0.20, 1.0]
                } else if view.pressure {
                    color(cp, view.palette).to_normalized_gamma_f32()
                } else {
                    [0.62, 0.75, 0.82, 1.0]
                };
                scene.vertices.push(Vertex {
                    position: positions[i].to_array(),
                    normal: normals[i].normalize_or_zero().to_array(),
                    color: rgb,
                    data: [0.0; 4],
                });
            }
        }
        scene.solid_count = scene.vertices.len() as u32;
        let chord = state.chord as f32;
        let floor = positions.iter().map(|p| p.y).fold(f32::INFINITY, f32::min) - chord * 0.85;
        // Directional shadow is projected from the same solid triangles onto
        // the reference plane. It adds depth cues without altering flow geometry.
        let light = Vec3::new(-0.5, 1.0, 0.4);
        for triangle in &analysis.mesh.triangles {
            let [a, b, c] = triangle.map(|i| positions[i]);
            if (b - a).cross(c - a).dot(light) <= 0.0 {
                continue;
            }
            for point in [a, b, c] {
                let projected = point - light * ((point.y - floor) / light.y);
                scene.vertices.push(Vertex {
                    position: projected.to_array(),
                    normal: Vec3::Y.to_array(),
                    color: [0.008, 0.014, 0.025, 0.35],
                    data: [2.0; 4],
                });
            }
        }
        let far_x = positions.iter().map(|p| p.x).fold(chord, f32::max) + 3.8 * chord;
        for i in -7..=7 {
            let z = i as f32 * state.span as f32 / 10.0;
            scene.tube(
                Vec3::new(-2.5 * chord, floor, z),
                Vec3::new(far_x, floor, z),
                chord * 0.002,
                [0.21, 0.32, 0.40, 0.45],
                [2.0; 4],
                [2.0; 4],
            );
        }
        for i in -3..=(far_x / chord).ceil() as i32 {
            let x = i as f32 * chord;
            scene.tube(
                Vec3::new(x, floor, -state.span as f32 * 0.7),
                Vec3::new(x, floor, state.span as f32 * 0.7),
                chord * 0.002,
                [0.21, 0.32, 0.40, 0.45],
                [2.0; 4],
                [2.0; 4],
            );
        }
        if view.wireframe {
            for triangle in &analysis.mesh.triangles {
                let [a, b, c] = triangle
                    .map(|i| positions[i] + normals[i].normalize_or_zero() * chord * 0.0005);
                for (p, q) in [(a, b), (b, c), (c, a)] {
                    scene.tube(
                        p,
                        q,
                        chord * 0.001,
                        [0.06, 0.12, 0.16, 0.65],
                        [2.0; 4],
                        [2.0; 4],
                    );
                }
            }
        }
        if state.velocity > 0.0 {
            for path in paths {
                if (path.vortex && !view.vortices) || (!path.vortex && !view.streamlines) {
                    continue;
                }
                for pair in path.samples.windows(2) {
                    let rgb = if path.vortex {
                        [0.69, 0.57, 0.97, 0.85]
                    } else {
                        let mut color = crate::ui::tunnel::speed_color(pair[0].speed_ratio)
                            .to_normalized_gamma_f32();
                        color[3] = 0.65;
                        color
                    };
                    let data = |i: usize| [1.0, pair[i].travel_time as f32, 0.0, 0.0];
                    scene.tube(
                        pair[0].position.as_vec3(),
                        pair[1].position.as_vec3(),
                        chord * if path.vortex { 0.0045 } else { 0.003 },
                        rgb,
                        data(0),
                        data(1),
                    );
                }
            }
        }
        scene
    }
    fn tube(&mut self, a: Vec3, b: Vec3, radius: f32, color: [f32; 4], da: [f32; 4], db: [f32; 4]) {
        if a.distance_squared(b) < 1e-20 {
            return;
        }
        // One instance per physical segment instead of 24 expanded vertices.
        // Camera-facing thickness is generated by the GPU, not on the CPU.
        self.lines.push(LineInstance {
            start: [a.x, a.y, a.z, da[1]],
            end: [b.x, b.y, b.z, db[1]],
            color,
            data: [radius, da[0], 0.0, 0.0],
        });
    }
}

impl Scene {
    /// CFD surfaces retain neutral lighting; volume colors/lines are read from
    /// OpenFOAM. Never paint a panel Cp onto an unrelated imported solid.
    pub fn volume(
        mesh: &glenn_extended::mesh::SolidMesh,
        paths: &[Vec<DVec3>],
        field: Option<&glenn_extended::cfd::VolumeField>,
        speed: f64,
    ) -> Self {
        let mut scene = Self {
            vertices: Vec::new(),
            solid_count: 0,
            lines: Vec::new(),
        };
        for t in &mesh.triangles {
            let [a, b, c] = t.map(|i| mesh.vertices[i].as_vec3());
            let n = (b - a).cross(c - a).normalize_or_zero();
            for p in [a, b, c] {
                scene.vertices.push(Vertex {
                    position: p.to_array(),
                    normal: n.to_array(),
                    color: [0.62, 0.75, 0.82, 1.0],
                    data: [0.0; 4],
                });
            }
        }
        scene.solid_count = scene.vertices.len() as u32;
        if paths.is_empty() {
            return scene;
        }
        let (lo, hi) = mesh.bounds();
        let radius = ((hi - lo).max_element() * 0.002) as f32;
        let surface = glenn_extended::mesh::SurfaceIndex::new(mesh);
        for line in paths {
            let mut travel = 0.0_f32;
            for pair in line.windows(2) {
                if surface.crosses(pair[0], pair[1]) {
                    break;
                }
                let v = field
                    .and_then(|f| f.sample_velocity(pair[0]))
                    .map_or(0.0, |v| v.length());
                let mut rgb =
                    crate::ui::tunnel::speed_color(v / speed.max(1e-8)).to_normalized_gamma_f32();
                rgb[3] = 0.75;
                let next = travel + (pair[0].distance(pair[1]) / v.max(1e-8)) as f32;
                scene.tube(
                    pair[0].as_vec3(),
                    pair[1].as_vec3(),
                    radius,
                    rgb,
                    [1.0, travel, 0.0, 0.0],
                    [1.0, next, 0.0, 0.0],
                );
                travel = next;
            }
        }
        scene
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn line_instances_preserve_physical_endpoints_and_convective_time() {
        let mut scene = Scene {
            vertices: vec![],
            lines: vec![],
            solid_count: 0,
        };
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(-4.0, 5.0, 6.0);
        scene.tube(
            a,
            b,
            0.03,
            [1.0; 4],
            [1.0, 2.5, 0.0, 0.0],
            [1.0, 4.5, 0.0, 0.0],
        );
        assert_eq!(scene.lines.len(), 1);
        assert_eq!(scene.lines[0].start, [1.0, 2.0, 3.0, 2.5]);
        assert_eq!(scene.lines[0].end, [-4.0, 5.0, 6.0, 4.5]);
        assert!(scene.vertices.is_empty());
    }
}

#[cfg(test)]
mod separation_tests {
    use super::*;
    use glenn_extended::{model::Simulation, viscous::Section};
    use glenn_solver_native::{Config, Solver};
    use std::sync::Arc;

    /// The separated skin must actually reach the wing surface the viewer sees.
    /// A field that reverses but paints nothing leaves a stalled wing looking
    /// exactly like an attached one at any view of the whole aircraft.
    #[test]
    fn a_stalled_section_marks_the_skin_and_an_attached_one_does_not() {
        let marked = |alpha: f64| {
            let state = Simulation {
                alpha,
                ..Simulation::default()
            };
            let analysis = Analysis::new(state).unwrap();
            let mut solver =
                Solver::new(Config::airfoil(&analysis.foil, alpha, 1.0e6, 40)).unwrap();
            solver.advance(1500).unwrap();
            let section = Section::new(Arc::new(solver.snapshot()), alpha);
            let scene = Scene::new(
                &analysis,
                &[],
                &View::default(),
                Some(&section as &dyn glenn_extended::physics::flow::SectionField),
            );
            scene.vertices[..scene.solid_count as usize]
                .iter()
                .filter(|v| v.color == [0.70, 0.08, 0.20, 1.0])
                .count()
        };
        let stalled = marked(24.0);
        let attached = marked(2.0);
        assert!(stalled > 50, "a stalled wing must show it: {stalled}");
        assert_eq!(attached, 0, "an attached wing must not: {attached}");
    }
}
