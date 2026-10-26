//! Convert shared geometry and solver samples to triangles, without modifying
//! physical positions. Thin tubes remain visible from any camera direction.
use super::Vertex;
use crate::ui::tunnel::{View, color};
use glam::{DVec3, Vec3};
use glenn_extended::{
    geometry::{body_to_world, interpolate},
    physics::{aerodynamics::Analysis, flow::Streamline},
};

pub struct Scene {
    pub vertices: Vec<Vertex>,
    pub solid_count: u32,
}
impl Scene {
    pub fn new(analysis: &Analysis, paths: &[Streamline], view: &View) -> Self {
        let mut scene = Self {
            vertices: Vec::new(),
            solid_count: 0,
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
                let surface = if i % section_size < analysis.foil.upper.len() - 1 {
                    &pressure.upper
                } else {
                    &pressure.lower
                };
                let cp = interpolate(surface, local.x);
                let rgb = if i >= positions.len() - 2 {
                    [0.36, 0.46, 0.55, 1.0]
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
        let tangent = (b - a).normalize_or_zero();
        if tangent == Vec3::ZERO {
            return;
        }
        let reference = if tangent.y.abs() < 0.9 {
            Vec3::Y
        } else {
            Vec3::Z
        };
        let u = tangent.cross(reference).normalize() * radius;
        let v = tangent.cross(u);
        let offsets = [u, v, -u, -v];
        for j in 0..4 {
            let k = (j + 1) % 4;
            for (p, offset, data) in [
                (a, offsets[j], da),
                (b, offsets[j], db),
                (b, offsets[k], db),
                (a, offsets[j], da),
                (b, offsets[k], db),
                (a, offsets[k], da),
            ] {
                self.vertices.push(Vertex {
                    position: (p + offset).to_array(),
                    normal: offset.normalize().to_array(),
                    color,
                    data,
                });
            }
        }
    }
}
