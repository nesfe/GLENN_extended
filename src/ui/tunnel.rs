//! Native interactive wing projection. All primitives are clipped to the viewport.
//! The projected mesh is the same closed mesh exported to OBJ/STL.
use super::plots::{AMBER, CYAN, MUTED};
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
use glenn_extended::{model::Point, physics::aerodynamics::Analysis};

#[derive(Clone)]
pub struct View {
    pub yaw: f64,
    pub pitch: f64,
    pub zoom: f64,
    pub animate: bool,
    pub streamlines: bool,
    pub vortices: bool,
    pub forces: bool,
    pub pressure: bool,
    pub wireframe: bool,
    pub probe: bool,
    pub shock: bool,
    pub density: usize,
    pub palette: usize,
    pub probe_position: [f64; 3],
}
impl Default for View {
    fn default() -> Self {
        Self {
            yaw: -0.65,
            pitch: 0.48,
            zoom: 1.0,
            animate: true,
            streamlines: true,
            vortices: true,
            forces: true,
            pressure: true,
            wireframe: false,
            probe: false,
            shock: true,
            density: 3,
            palette: 0,
            probe_position: [-0.4, 0.35, 0.5],
        }
    }
}
fn color(cp: f64, palette: usize) -> Color32 {
    let t = ((cp + 2.0) / 3.0).clamp(0.0, 1.0) as f32;
    let (a, b) = match palette {
        1 => ([94.0, 48.0, 171.0], [248.0, 210.0, 86.0]),
        2 => ([45.0, 62.0, 122.0], [247.0, 107.0, 67.0]),
        _ => ([37.0, 160.0, 215.0], [244.0, 179.0, 75.0]),
    };
    Color32::from_rgb(
        (a[0] + t * (b[0] - a[0])) as u8,
        (a[1] + t * (b[1] - a[1])) as u8,
        (a[2] + t * (b[2] - a[2])) as u8,
    )
}
/// Orthographic camera fits the current wing bounds before applying user zoom.
struct Camera {
    center: Pos2,
    scale: f64,
    yaw: f64,
    pitch: f64,
    origin: [f64; 3],
    alpha: f64,
}
impl Camera {
    fn rotate(&self, p: [f64; 3]) -> [f64; 3] {
        let x = p[0] - self.origin[0];
        let y = p[1] - self.origin[1];
        let z = p[2] - self.origin[2];
        let (sa, ca) = self.alpha.sin_cos();
        let (x, y) = (x * ca + y * sa, -x * sa + y * ca);
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let horizontal = x * cy - z * sy;
        let depth = x * sy + z * cy;
        [horizontal, y * cp - depth * sp, y * sp + depth * cp]
    }
    fn project(&self, p: [f64; 3]) -> (Pos2, f64) {
        let [x, y, z] = self.rotate(p);
        (
            self.center + Vec2::new((x * self.scale) as f32, (-y * self.scale) as f32),
            z,
        )
    }
}
pub fn show(ui: &mut egui::Ui, analysis: &Analysis, view: &mut View, time: f64) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new("Аэродинамическая труба")
                .strong()
                .size(20.0),
        );
        for (label, yaw, pitch) in [
            ("Изометрия", -0.65, 0.48),
            ("Профиль", 0.0, 0.0),
            ("План", 0.0, 1.57),
            ("Торец", 1.57, 0.0),
        ] {
            if ui.button(label).clicked() {
                view.yaw = yaw;
                view.pitch = pitch;
                view.zoom = 1.0;
            }
        }
        if ui.button("Вписать").clicked() {
            view.zoom = 1.0;
        }
    });
    ui.label(
        egui::RichText::new(
            "Перетаскивание — поворот · колесо — масштаб · «Вписать» — восстановить вид",
        )
        .small()
        .color(MUTED),
    );
    let height = (ui.available_height() - 110.0).clamp(280.0, 620.0);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), egui::Sense::drag());
    if response.dragged() {
        let delta = ui.input(|i| i.pointer.delta());
        view.yaw += delta.x as f64 * 0.008;
        view.pitch = (view.pitch + delta.y as f64 * 0.008).clamp(-1.55, 1.55);
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            view.zoom = (view.zoom * (scroll as f64 * 0.002).exp()).clamp(0.35, 3.0);
        }
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 10, Color32::from_rgb(9, 17, 27));
    let mesh = &analysis.mesh;
    let state = &analysis.state;
    let aero = &analysis.current;
    let mut bounds_min = [f64::INFINITY; 3];
    let mut bounds_max = [f64::NEG_INFINITY; 3];
    for p in &mesh.vertices {
        for axis in 0..3 {
            bounds_min[axis] = bounds_min[axis].min(p[axis]);
            bounds_max[axis] = bounds_max[axis].max(p[axis]);
        }
    }
    let origin = std::array::from_fn(|i| (bounds_min[i] + bounds_max[i]) * 0.5);
    let mut camera = Camera {
        center: rect.center(),
        scale: 1.0,
        yaw: view.yaw,
        pitch: view.pitch,
        origin,
        alpha: state.alpha.to_radians(),
    };
    let rotated: Vec<_> = mesh.vertices.iter().map(|p| camera.rotate(*p)).collect();
    let width = rotated
        .iter()
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max)
        - rotated.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let height = rotated
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max)
        - rotated.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    camera.scale = (rect.width() as f64 * 0.72 / width.max(state.chord))
        .min(rect.height() as f64 * 0.6 / height.max(state.chord * 0.5))
        * view.zoom;
    for i in -6..=6 {
        let z = i as f64 * state.span / 10.0;
        let a = camera
            .project([-2.0 * state.chord, -0.4 * state.chord, z])
            .0;
        let b = camera.project([3.0 * state.chord, -0.4 * state.chord, z]).0;
        painter.line_segment([a, b], Stroke::new(1.0, Color32::from_rgb(27, 43, 57)));
    }
    // Streamlines and vortices are illustrative, not velocities from a 3D CFD solve.
    if view.streamlines {
        for lane in 0..(view.density * 4 + 5) {
            let fraction = lane as f64 / (view.density * 4 + 4) as f64;
            let z = (fraction - 0.5) * state.span * 1.4;
            let y = 0.22 * state.chord;
            let points: Vec<Pos2> = (0..64)
                .map(|i| {
                    let x = -2.0 + 5.0 * i as f64 / 63.0;
                    let bend = 0.22 * (-((x - 0.2) / 0.65).powi(2)).exp();
                    camera
                        .project([x * state.chord, y + bend * state.chord, z])
                        .0
                })
                .collect();
            painter.add(egui::Shape::line(
                points,
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(66, 155, 180, 90)),
            ));
            for particle in 0..3 {
                let progress = (time * 0.25 * (state.velocity / 65.0).min(4.0)
                    + fraction
                    + particle as f64 / 3.0)
                    .fract();
                let x = -2.0 + 5.0 * progress;
                let point = camera
                    .project([
                        x * state.chord,
                        y + 0.22 * state.chord * (-((x - 0.2) / 0.65).powi(2)).exp(),
                        z,
                    ])
                    .0;
                if rect.contains(point) {
                    painter.circle_filled(point, 1.6, CYAN);
                }
            }
        }
    }
    if view.vortices {
        for sign in [-1.0, 1.0] {
            let points = (0..100)
                .map(|i| {
                    let u = i as f64 / 99.0;
                    let phase = 18.0 * u - time * 2.0;
                    camera
                        .project([
                            (1.0 + 3.0 * u) * state.chord,
                            0.12 * state.chord * phase.sin(),
                            sign * state.span * 0.5 + 0.12 * state.chord * phase.cos(),
                        ])
                        .0
                })
                .collect();
            painter.add(egui::Shape::line(
                points,
                Stroke::new(1.3, Color32::from_rgba_unmultiplied(168, 133, 230, 140)),
            ));
        }
    }
    let projected: Vec<_> = mesh.vertices.iter().map(|p| camera.project(*p)).collect();
    let mut triangles: Vec<_> = mesh.triangles.iter().collect();
    triangles.sort_by(|a, b| {
        let za = a.iter().map(|i| projected[*i].1).sum::<f64>();
        let zb = b.iter().map(|i| projected[*i].1).sum::<f64>();
        za.total_cmp(&zb)
    });
    let contour = analysis.foil.contour();
    let n = contour.len();
    let mut painted_mesh = egui::Mesh::default();
    let mut wire_edges = Vec::new();
    for triangle in triangles {
        let points = triangle.map(|i| projected[i].0);
        let index = triangle[0] % n;
        let local = contour[index];
        let pressure = if index < analysis.foil.upper.len() - 1 {
            &aero.pressure.upper
        } else {
            &aero.pressure.lower
        };
        let cp = glenn_extended::geometry::interpolate(pressure, local.x);
        let base = if view.pressure {
            color(cp, view.palette)
        } else {
            Color32::from_rgb(90, 159, 174)
        };
        // Submit triangles directly. Polygon anti-aliasing creates unbounded
        // mitres for nearly edge-on panels; a mesh keeps even thin panels stable.
        let first = painted_mesh.vertices.len() as u32;
        for position in points {
            painted_mesh.vertices.push(egui::epaint::Vertex {
                pos: position,
                uv: egui::epaint::WHITE_UV,
                color: base,
            });
        }
        painted_mesh.indices.extend([first, first + 1, first + 2]);
        if view.wireframe {
            wire_edges.extend([
                [points[0], points[1]],
                [points[1], points[2]],
                [points[2], points[0]],
            ]);
        }
    }
    painter.add(egui::Shape::mesh(painted_mesh));
    for edge in wire_edges {
        painter.line_segment(edge, Stroke::new(0.5, Color32::from_gray(40)));
    }
    if view.forces {
        let origin = camera.project([0.0, 0.0, 0.0]).0;
        for (label, delta, color) in [
            ("L", Vec2::new(0.0, -70.0 * aero.cl.signum() as f32), CYAN),
            ("D", Vec2::new(65.0, 0.0), AMBER),
        ] {
            painter.arrow(origin, delta, Stroke::new(2.5, color));
            painter.text(
                origin + delta,
                egui::Align2::LEFT_BOTTOM,
                label,
                egui::FontId::proportional(14.0),
                color,
            );
        }
    }
    if view.probe {
        let p = camera
            .project(view.probe_position.map(|v| v * state.chord))
            .0;
        painter.circle_stroke(p, 6.0, Stroke::new(2.0, AMBER));
        painter.line_segment([p, p + Vec2::new(25.0, 0.0)], Stroke::new(2.0, AMBER));
    }
    if view.shock && aero.mach > 1.0 {
        let mu = (1.0 / aero.mach).asin();
        let origin = camera.project([0.0, 0.0, 0.0]).0;
        for sign in [-1.0, 1.0] {
            let tip = camera
                .project([2.0 * state.chord, 0.0, sign * mu.tan() * 2.0 * state.chord])
                .0;
            painter.line_segment([origin, tip], Stroke::new(1.5, AMBER));
        }
    }
    if view.pressure {
        let legend_start = rect.right_top() + Vec2::new(-158.0, 18.0);
        for step in 0..100 {
            let start = legend_start + Vec2::new(step as f32 * 1.3, 0.0);
            painter.rect_filled(
                egui::Rect::from_min_size(start, Vec2::new(1.5, 7.0)),
                0,
                color(-2.0 + step as f64 * 0.03, view.palette),
            );
        }
        painter.text(
            legend_start + Vec2::new(0.0, 13.0),
            egui::Align2::LEFT_TOP,
            "Cp  −2                  +1",
            egui::FontId::proportional(10.0),
            MUTED,
        );
    }
    painter.text(
        rect.left_top() + Vec2::splat(16.0),
        egui::Align2::LEFT_TOP,
        format!(
            "{}  /  {:.1}°  /  {:.0} м/с",
            state.profile_name, state.alpha, state.velocity
        ),
        egui::FontId::proportional(13.0),
        MUTED,
    );
    painter.text(
        rect.left_bottom() + Vec2::new(16.0, -14.0),
        egui::Align2::LEFT_BOTTOM,
        "Геометрия в метрах · линии тока иллюстративные",
        egui::FontId::proportional(11.0),
        MUTED,
    );
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut view.animate, "Анимация");
        ui.checkbox(&mut view.streamlines, "Линии тока");
        ui.checkbox(&mut view.vortices, "Вихри");
        ui.checkbox(&mut view.forces, "Силы");
        ui.checkbox(&mut view.pressure, "Давление");
        ui.checkbox(&mut view.wireframe, "Сетка");
        ui.checkbox(&mut view.probe, "Зонд");
        ui.checkbox(&mut view.shock, "Конус Маха");
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Палитра");
        for (i, label) in ["Океан", "Плазма", "Тепловая"].iter().enumerate() {
            ui.selectable_value(&mut view.palette, i, *label);
        }
        ui.add(egui::Slider::new(&mut view.density, 1..=5).text("Дым"));
    });
}

pub fn section(ui: &mut egui::Ui, analysis: &Analysis) {
    ui.heading("Сечение профиля");
    ui.label("Контур, средняя линия и хорда; угол атаки задаёт направление набегающего потока.");
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 260.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8, Color32::from_rgb(9, 17, 27));
    let scale = (rect.width() * 0.76).min(700.0);
    let center = rect.center();
    let transform = |p: Point| center + Vec2::new((p.x as f32 - 0.5) * scale, -p.y as f32 * scale);
    painter.line_segment(
        [
            transform(Point::new(0.0, 0.0)),
            transform(Point::new(1.0, 0.0)),
        ],
        Stroke::new(1.0, MUTED),
    );
    for (points, color) in [(&analysis.foil.upper, CYAN), (&analysis.foil.lower, AMBER)] {
        painter.add(egui::Shape::line(
            points.iter().map(|p| transform(*p)).collect(),
            Stroke::new(2.2, color),
        ));
    }
    // Display normal pressure forces on a bounded visual scale. The underlying
    // Cp values remain unchanged and are shown quantitatively in the chart.
    for (surface, pressure) in [
        (&analysis.foil.upper, &analysis.current.pressure.upper),
        (&analysis.foil.lower, &analysis.current.pressure.lower),
    ] {
        for pair in surface.windows(2).step_by(4) {
            let midpoint = Point::new((pair[0].x + pair[1].x) * 0.5, (pair[0].y + pair[1].y) * 0.5);
            let cp = glenn_extended::geometry::interpolate(pressure, midpoint.x);
            let direction = if std::ptr::eq(surface, &analysis.foil.upper) {
                -1.0
            } else {
                1.0
            };
            let delta = Vec2::new(0.0, (direction * cp * 16.0).clamp(-35.0, 35.0) as f32);
            painter.arrow(transform(midpoint), delta, Stroke::new(1.0, MUTED));
        }
    }
    let mean: Vec<_> = analysis
        .foil
        .upper
        .iter()
        .zip(&analysis.foil.lower)
        .map(|(u, l)| transform(Point::new((u.x + l.x) * 0.5, (u.y + l.y) * 0.5)))
        .collect();
    painter.add(egui::Shape::line(
        mean,
        Stroke::new(1.0, Color32::from_rgb(175, 140, 226)),
    ));
    let cp = analysis.current.center_of_pressure;
    if (0.0..=1.0).contains(&cp) {
        painter.circle_filled(transform(Point::new(cp, 0.0)), 4.0, Color32::WHITE);
    }
    painter.text(
        rect.left_top() + Vec2::splat(12.0),
        egui::Align2::LEFT_TOP,
        &analysis.state.profile_name,
        egui::FontId::proportional(14.0),
        MUTED,
    );
    super::plots::plot(
        ui,
        "Распределение давления",
        "x/c",
        "Cp",
        &[
            (
                "Верхняя поверхность",
                &analysis.current.pressure.upper,
                CYAN,
            ),
            (
                "Нижняя поверхность",
                &analysis.current.pressure.lower,
                AMBER,
            ),
        ],
        true,
    );
}
