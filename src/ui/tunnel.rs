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
    pub playback: f64,
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
            playback: 0.05,
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
/// Shared pressure transfer function for surface colors and the legend.
/// Neutral pressure is light; suction and overpressure remain distinguishable.
pub(super) fn color(cp: f64, palette: usize) -> Color32 {
    let stops = match palette {
        1 => [
            [-2.0, 70.0, 28.0, 130.0],
            [-0.5, 184.0, 63.0, 140.0],
            [0.0, 245.0, 169.0, 92.0],
            [1.0, 252.0, 237.0, 151.0],
        ],
        2 => [
            [-2.0, 39.0, 63.0, 139.0],
            [-0.5, 92.0, 163.0, 206.0],
            [0.0, 232.0, 232.0, 221.0],
            [1.0, 224.0, 74.0, 52.0],
        ],
        _ => [
            [-2.0, 33.0, 87.0, 178.0],
            [-0.5, 46.0, 184.0, 206.0],
            [0.0, 211.0, 226.0, 222.0],
            [1.0, 245.0, 146.0, 52.0],
        ],
    };
    let cp = cp.clamp(-2.0, 1.0);
    let i = stops.partition_point(|p| p[0] < cp).clamp(1, 3);
    let (a, b) = (stops[i - 1], stops[i]);
    let t = (cp - a[0]) / (b[0] - a[0]);
    Color32::from_rgb(
        (a[1] + t * (b[1] - a[1])) as u8,
        (a[2] + t * (b[2] - a[2])) as u8,
        (a[3] + t * (b[3] - a[3])) as u8,
    )
}
pub(super) fn speed_color(ratio: f64) -> Color32 {
    let t = ((ratio - 0.4) / 1.6).clamp(0.0, 1.0) as f32;
    Color32::from_rgb(
        ((0.18 + 0.5 * t) * 255.0) as u8,
        ((0.65 + 0.25 * t) * 255.0) as u8,
        ((0.91 - 0.3 * t) * 255.0) as u8,
    )
}
/// Orthographic camera fits the current wing bounds before applying user zoom.
struct Camera {
    center: Pos2,
    scale: f64,
    yaw: f64,
    pitch: f64,
    origin: [f64; 3],
}
impl Camera {
    fn rotate(&self, p: [f64; 3]) -> [f64; 3] {
        let x = p[0] - self.origin[0];
        let y = p[1] - self.origin[1];
        let z = p[2] - self.origin[2];
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let horizontal = x * cy - z * sy;
        let depth = x * sy + z * cy;
        [horizontal, y * cp - depth * sp, y * sp + depth * cp]
    }
    fn matrix(&self, size: Vec2) -> glam::Mat4 {
        let (sy, cy) = (self.yaw as f32).sin_cos();
        let (sp, cp) = (self.pitch as f32).sin_cos();
        let rotation = glam::Mat4::from_cols(
            glam::Vec4::new(cy, -sy * sp, sy * cp, 0.0),
            glam::Vec4::new(0.0, cp, sp, 0.0),
            glam::Vec4::new(-sy, -cy * sp, cy * cp, 0.0),
            glam::Vec4::W,
        );
        let depth = size.x.max(size.y) / self.scale as f32 * 20.0;
        let projection = glam::Mat4::from_cols(
            glam::Vec4::new(2.0 * self.scale as f32 / size.x, 0.0, 0.0, 0.0),
            glam::Vec4::new(0.0, 2.0 * self.scale as f32 / size.y, 0.0, 0.0),
            glam::Vec4::new(0.0, 0.0, -1.0 / depth, 0.0),
            glam::Vec4::new(0.0, 0.0, 0.5, 1.0),
        );
        projection
            * rotation
            * glam::Mat4::from_translation(-glam::DVec3::from_array(self.origin).as_vec3())
    }
    fn project(&self, p: [f64; 3]) -> (Pos2, f64) {
        let [x, y, z] = self.rotate(p);
        (
            self.center + Vec2::new((x * self.scale) as f32, (-y * self.scale) as f32),
            z,
        )
    }
}
/// Numerical results arrive atomically from the engine; this module only draws.
#[derive(Default)]
pub struct Tunnel {
    pub renderer: Option<super::render::Renderer>,
    pub grid: Option<std::sync::Arc<glenn_extended::physics::flow::SectionGrid>>,
    pub pending_scene: Option<super::render::Scene>,
    pub busy: bool,
    clock: f64,
    last_time: Option<f64>,
}
pub fn show(ui: &mut egui::Ui, analysis: &Analysis, view: &mut View, tunnel: &mut Tunnel) {
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
    if tunnel.busy {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Пересчёт… Показан последний завершённый результат.");
        });
    }
    if analysis.current.mach > 0.75 {
        ui.colored_label(AMBER, "За пределами применимости: эти линии не показывают ударные волны и вязкий отрыв. Объёмный несжимаемый CFD также не подходит для этого числа Маха.");
    } else if analysis.current.stall_fraction > 0.12 {
        ui.colored_label(AMBER, "Панельный режим: отрыв не решается. Для вязкого отрыва используйте вкладку «Объёмная CFD».");
    }
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
    let now = ui.input(|input| input.time);
    let elapsed = tunnel
        .last_time
        .map_or(0.0, |last| (now - last).clamp(0.0, 0.1));
    tunnel.last_time = Some(now);
    if view.animate {
        tunnel.clock += elapsed * analysis.state.velocity / analysis.state.chord * view.playback;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 10, Color32::from_rgb(9, 17, 27));
    let state = &analysis.state;
    let aero = &analysis.current;
    let mesh = &analysis.mesh;
    let world: Vec<_> = mesh
        .vertices
        .iter()
        .map(|p| {
            glenn_extended::geometry::body_to_world(state, glam::DVec3::from_array(*p)).to_array()
        })
        .collect();
    let mut minimum = [f64::INFINITY; 3];
    let mut maximum = [f64::NEG_INFINITY; 3];
    for p in &world {
        for axis in 0..3 {
            minimum[axis] = minimum[axis].min(p[axis]);
            maximum[axis] = maximum[axis].max(p[axis]);
        }
    }
    let origin = std::array::from_fn(|i| (minimum[i] + maximum[i]) * 0.5);
    let mut camera = Camera {
        center: rect.center(),
        scale: 1.0,
        yaw: view.yaw,
        pitch: view.pitch,
        origin,
    };
    let rotated: Vec<_> = world.iter().map(|p| camera.rotate(*p)).collect();
    let extent = |axis: usize| {
        rotated
            .iter()
            .map(|p| p[axis])
            .fold(f64::NEG_INFINITY, f64::max)
            - rotated
                .iter()
                .map(|p| p[axis])
                .fold(f64::INFINITY, f64::min)
    };
    camera.scale = (rect.width() as f64 * 0.64 / extent(0).max(state.chord * 2.0))
        .min(rect.height() as f64 * 0.50 / extent(1).max(state.chord * 0.85))
        * view.zoom;
    if let Some(renderer) = &mut tunnel.renderer {
        if let Some(scene) = tunnel.pending_scene.take() {
            renderer.upload(&scene);
        }
        let pixels = ui.ctx().pixels_per_point();
        let texture = renderer.render(
            [
                (rect.width() * pixels) as u32,
                (rect.height() * pixels) as u32,
            ],
            camera.matrix(rect.size()),
            tunnel.clock as f32,
        );
        painter.image(
            texture,
            rect,
            egui::Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    if view.forces {
        let cp = aero.center_of_pressure.clamp(0.0, 1.0);
        let origin_world =
            glenn_extended::geometry::SectionFrame::at(state, 0.0).to_world(Point::new(cp, 0.0));
        let origin = camera.project(origin_world.to_array()).0;
        let force_scale = 110.0 / camera.scale / aero.lift.abs().max(aero.drag).max(1.0);
        for (label, force, color) in [
            (
                format!("L {:.2} кН", aero.lift / 1000.0),
                glam::DVec3::Y * aero.lift,
                CYAN,
            ),
            (
                format!("D {:.2} кН", aero.drag / 1000.0),
                glam::DVec3::X * aero.drag,
                AMBER,
            ),
        ] {
            let tip = camera
                .project((origin_world + force * force_scale).to_array())
                .0;
            if tip.distance(origin) > 0.1 {
                painter.arrow(origin, tip - origin, Stroke::new(2.0, color));
                painter.text(
                    tip,
                    egui::Align2::LEFT_BOTTOM,
                    label,
                    egui::FontId::proportional(12.0),
                    color,
                );
            }
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
    // Opaque backplates keep quantitative legends legible over dense flow.
    if view.pressure || view.streamlines {
        painter.rect_filled(
            egui::Rect::from_min_size(
                rect.right_top() + Vec2::new(-174.0, 8.0),
                Vec2::new(164.0, 91.0),
            ),
            5,
            Color32::from_rgb(9, 17, 27),
        );
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
    if view.streamlines {
        let start = rect.right_top() + Vec2::new(-158.0, 65.0);
        for step in 0..100 {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    start + Vec2::new(step as f32 * 1.3, 0.0),
                    Vec2::new(1.5, 7.0),
                ),
                0,
                speed_color(0.4 + step as f64 * 1.6 / 99.0),
            );
        }
        painter.text(
            start + Vec2::new(0.0, 13.0),
            egui::Align2::LEFT_TOP,
            "V/V∞  0.4               2.0",
            egui::FontId::proportional(10.0),
            MUTED,
        );
    }
    painter.rect_filled(
        egui::Rect::from_min_size(rect.left_top() + Vec2::splat(8.0), Vec2::new(300.0, 49.0)),
        5,
        Color32::from_rgb(9, 17, 27),
    );
    painter.rect_filled(
        egui::Rect::from_min_size(
            rect.left_bottom() + Vec2::new(8.0, -35.0),
            Vec2::new(350.0, 29.0),
        ),
        5,
        Color32::from_rgb(9, 17, 27),
    );
    if let Some(grid) = &tunnel.grid {
        let field = glenn_extended::physics::flow::FlowField { analysis, grid };
        painter.text(
            rect.left_top() + Vec2::new(16.0, 39.0),
            egui::Align2::LEFT_TOP,
            format!("Γ₀ {:.2} м²/с", field.tip_circulation()),
            egui::FontId::proportional(11.0),
            MUTED,
        );
    }
    painter.text(
        rect.left_top() + Vec2::splat(16.0),
        egui::Align2::LEFT_TOP,
        format!(
            "{}  /  {:.1}°  /  {:.0} м/с",
            state.profile_name.chars().take(24).collect::<String>(),
            state.alpha,
            state.velocity
        ),
        egui::FontId::proportional(13.0),
        MUTED,
    );
    painter.text(
        rect.left_bottom() + Vec2::new(16.0, -14.0),
        egui::Align2::LEFT_BOTTOM,
        "Панельное поле + приближённый след · единицы: м",
        egui::FontId::proportional(11.0),
        MUTED,
    );
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut view.animate, "Анимация");
        ui.checkbox(&mut view.streamlines, "Линии тока");
        ui.checkbox(&mut view.vortices, "Модель концевого следа");
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
    });
    ui.horizontal_wrapped(|ui| {
        for control in 0..2 {
            ui.allocate_ui_with_layout(
                Vec2::new(250.0, 30.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().slider_width = 110.0;
                    if control == 0 {
                        ui.add(egui::Slider::new(&mut view.density, 1..=5).text("Линии"));
                    } else {
                        ui.add(
                            egui::Slider::new(&mut view.playback, 0.01..=1.0)
                                .logarithmic(true)
                                .text("Время ×"),
                        );
                    }
                },
            );
        }
    });
}

pub fn section(ui: &mut egui::Ui, analysis: &Analysis) {
    ui.heading("Сечение профиля");
    ui.label("Контур, средняя линия, хорда и нормальные силы; поток направлен слева направо.");
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 260.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8, Color32::from_rgb(9, 17, 27));
    let alpha = analysis.state.alpha.to_radians();
    let rotated: Vec<_> = analysis
        .foil
        .contour()
        .iter()
        .map(|p| {
            glenn_extended::geometry::rotate_incidence(glam::DVec3::new(p.x - 0.5, p.y, 0.0), alpha)
        })
        .collect();
    let vertical = rotated
        .iter()
        .map(|p| p.y)
        .fold(f64::NEG_INFINITY, f64::max)
        - rotated.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    let scale = (rect.width() * 0.76)
        .min(700.0)
        .min(180.0 / vertical.max(0.15) as f32);
    let center = rect.center();
    let transform = |p: Point| {
        let p = glenn_extended::geometry::rotate_incidence(
            glam::DVec3::new(p.x - 0.5, p.y, 0.0),
            alpha,
        );
        center + Vec2::new(p.x as f32 * scale, -p.y as f32 * scale)
    };
    painter.arrow(
        rect.left_top() + Vec2::new(18.0, 52.0),
        Vec2::new(65.0, 0.0),
        Stroke::new(1.5, CYAN),
    );
    painter.text(
        rect.left_top() + Vec2::new(90.0, 52.0),
        egui::Align2::LEFT_CENTER,
        format!(
            "V∞ {:.0} м/с · α {:.1}°",
            analysis.state.velocity, analysis.state.alpha
        ),
        egui::FontId::proportional(11.0),
        MUTED,
    );
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
            let outward_sign = if std::ptr::eq(surface, &analysis.foil.upper) {
                1.0
            } else {
                -1.0
            };
            let tangent_x = pair[1].x - pair[0].x;
            let tangent_y = pair[1].y - pair[0].y;
            let length = tangent_x.hypot(tangent_y).max(1e-12);
            let normal_x = -outward_sign * tangent_y / length;
            let normal_y = outward_sign * tangent_x / length;
            let strength = (-cp * 16.0).clamp(-35.0, 35.0);
            let normal = glenn_extended::geometry::rotate_incidence(
                glam::DVec3::new(normal_x, normal_y, 0.0),
                alpha,
            );
            let delta = Vec2::new((normal.x * strength) as f32, (-normal.y * strength) as f32);
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
