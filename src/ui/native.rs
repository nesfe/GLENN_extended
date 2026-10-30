//! Live native tunnel. Every colour, arrow and probe comes from one viscous
//! snapshot. Panel forces remain on the explicitly separate engineering pages.
use super::{
    plots::{AMBER, CYAN, MUTED},
    render,
};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use glenn_extended::physics::aerodynamics::Analysis;
use glenn_solver_native::{Config, Snapshot, worker::Worker};
use std::sync::Arc;

pub struct Native {
    worker: Worker,
    config: Option<Config>,
    generation: u64,
    pub snapshot: Option<Arc<Snapshot>>,
    error: Option<String>,
    pub paused: bool,
    pub export_requested: bool,
    cells: usize,
    quantity: usize,
    pub(super) three_d: bool,
    yaw: f32,
    pitch: f32,
    texture: Option<egui::TextureHandle>,
    dirty: bool,
    paths: Vec<Vec<[f32; 2]>>,
    pub renderer: Option<render::Renderer>,
    started: std::time::Instant,
}
impl Default for Native {
    fn default() -> Self {
        Self {
            worker: Worker::default(),
            config: None,
            generation: 0,
            snapshot: None,
            error: None,
            paused: false,
            export_requested: false,
            cells: 40,
            quantity: 0,
            three_d: false,
            yaw: 0.6,
            pitch: 0.45,
            texture: None,
            dirty: true,
            paths: Vec::new(),
            renderer: None,
            started: std::time::Instant::now(),
        }
    }
}
pub(super) fn field_color(f: &Snapshot, i: usize, quantity: usize) -> Color32 {
    if f.solid[i] {
        return Color32::from_rgb(225, 234, 240);
    }
    let u = f.velocity[i];
    if quantity == 0 {
        if u[0] < 0.0 {
            let t = (-u[0]).min(1.0);
            return Color32::from_rgb((150.0 + 105.0 * t) as u8, (65.0 + 65.0 * t) as u8, 34);
        }
        let t = (u[0] / 1.6).clamp(0.0, 1.0);
        return Color32::from_rgb(
            (15.0 + 28.0 * t) as u8,
            (32.0 + 145.0 * t) as u8,
            (56.0 + 160.0 * t) as u8,
        );
    }
    let value = if quantity == 2 {
        f.pressure[i] * 0.45
    } else {
        let x = i % f.width;
        let y = i / f.width;
        if x == 0 || y == 0 || x == f.width - 1 || y == f.height - 1 {
            0.0
        } else {
            ((f.velocity[i + 1][1] - f.velocity[i - 1][1])
                - (f.velocity[i + f.width][0] - f.velocity[i - f.width][0]))
                * f.cells_per_chord
                * 0.08
        }
    }
    .clamp(-1.0, 1.0);
    let t = value.abs();
    let base = [18.0, 29.0, 43.0];
    let end = if value < 0.0 {
        [40.0, 173.0, 242.0]
    } else {
        [249.0, 116.0, 52.0]
    };
    Color32::from_rgb(
        (base[0] + (end[0] - base[0]) * t) as u8,
        (base[1] + (end[1] - base[1]) * t) as u8,
        (base[2] + (end[2] - base[2]) * t) as u8,
    )
}
impl Native {
    pub fn unavailable_reason(analysis: &Analysis) -> Option<&'static str> {
        let state = &analysis.state;
        if state.velocity <= 0.0 {
            Some("Скорость потока равна нулю.")
        } else if analysis.current.mach > 0.3 {
            Some("Вязкий решатель работает при M ≤ 0,3.")
        } else if state.profile_type == glenn_extended::model::Profile::Cylinder
            && state.cylinder_rpm != 0.0
        {
            Some("Вязкий решатель поддерживает только неподвижный цилиндр.")
        } else {
            None
        }
    }

    pub fn snapshot_for(&self, analysis: &Analysis) -> Option<Arc<Snapshot>> {
        if Self::unavailable_reason(analysis).is_some() || self.error.is_some() {
            return None;
        }
        let state = &analysis.state;
        let re = state.velocity * state.chord * analysis.atmosphere.density
            / analysis.atmosphere.viscosity;
        (self.config.as_ref()
            == Some(&Config::airfoil(
                &analysis.foil,
                state.alpha,
                re,
                self.cells,
            )))
        .then(|| self.snapshot.clone())
        .flatten()
    }

    pub fn export_csv(&self, analysis: &Analysis) -> Result<String, String> {
        use std::fmt::Write;
        let field = self.snapshot.as_ref().ok_or("Нет рассчитанного поля")?;
        let config = self.config.as_ref().ok_or("Нет текущего расчёта")?;
        let state = &analysis.state;
        let re = state.velocity * state.chord * analysis.atmosphere.density
            / analysis.atmosphere.viscosity;
        if config != &Config::airfoil(&analysis.foil, state.alpha, re, self.cells) {
            return Err("Дождитесь поля для новых параметров".into());
        }
        let mut csv = format!(
            "# Native D2Q9 root section; step={}; tU/c={}; Re={}; alpha={}; cl={}; cd={}\nx_m,y_m,Ux_m_s,Uy_m_s,p_minus_pinf_Pa,solid\n",
            field.step, field.time, re, state.alpha, field.lift, field.drag
        );
        for y in 0..field.height {
            for x in 0..field.width {
                let i = y * field.width + x;
                let p = field.point(x, y);
                writeln!(
                    csv,
                    "{},{},{},{},{},{}",
                    (p[0] as f64 - 0.25) * state.chord,
                    p[1] as f64 * state.chord,
                    field.velocity[i][0] as f64 * state.velocity,
                    field.velocity[i][1] as f64 * state.velocity,
                    field.pressure[i] as f64 * analysis.atmosphere.dynamic_pressure,
                    u8::from(field.solid[i])
                )
                .unwrap();
            }
        }
        Ok(csv)
    }
    pub fn suspend(&self) {
        self.worker.pause(true);
    }
    pub fn reverse_strength(&self) -> f64 {
        self.snapshot
            .as_ref()
            .map_or(0.0, |s| (-s.min_ux as f64).clamp(0.0, 1.0))
    }
    pub fn ready(&self, steps: u64) -> bool {
        self.error.is_some() || self.snapshot.as_ref().is_some_and(|s| s.step >= steps)
    }
    pub fn show(&mut self, ui: &mut egui::Ui, analysis: &Analysis) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Аэродинамическая труба");
            if ui.selectable_label(!self.three_d, "Сечение").clicked() {
                self.three_d = false;
            }
            if ui.selectable_label(self.three_d, "Крыло в 3D").clicked() {
                self.three_d = true;
                self.dirty = true;
            }
            if ui
                .button(if self.paused {
                    "Продолжить"
                } else {
                    "Пауза"
                })
                .clicked()
            {
                self.paused = !self.paused;
            }
            if ui
                .add_enabled(self.snapshot.is_some(), egui::Button::new("CSV поля"))
                .clicked()
            {
                self.export_requested = true;
            }
            if ui.button("Начать заново").clicked() {
                self.config = None;
                self.paused = false;
            }
        });
        let quantities = ["Скорость вдоль трубы", "Завихрённость", "Давление"];
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("native_quantity")
                .width(190.0)
                .selected_text(quantities[self.quantity])
                .show_ui(ui, |ui| {
                    for (i, name) in quantities.iter().enumerate() {
                        if ui.selectable_value(&mut self.quantity, i, *name).changed() {
                            self.dirty = true;
                        }
                    }
                });
            egui::ComboBox::from_id_salt("native_resolution")
                .width(165.0)
                .selected_text(format!("{} ячеек / хорду", self.cells))
                .show_ui(ui, |ui| {
                    for n in [32, 40, 64, 96] {
                        ui.selectable_value(&mut self.cells, n, format!("{n} ячеек / хорду"));
                    }
                });
        });
        if let Err(message) = self.advance(analysis) {
            ui.colored_label(AMBER, message);
            return;
        }
        if let Some(e) = &self.error {
            ui.colored_label(AMBER, e);
        }
        let Some(f) = self.snapshot.as_ref() else {
            ui.spinner();
            ui.label("Запуск потока…");
            ui.ctx().request_repaint();
            return;
        };
        let q = analysis.atmosphere.dynamic_pressure;
        let state = &analysis.state;
        self.draw(ui, f.clone(), state, q, analysis);
    }

    /// Compact controls for the three-dimensional tunnel, where the section
    /// view with its own toolbar is not on screen.
    pub fn controls(&mut self, ui: &mut egui::Ui) {
        if ui
            .button(if self.paused {
                "Продолжить"
            } else {
                "Пауза"
            })
            .clicked()
        {
            self.paused = !self.paused;
        }
        egui::ComboBox::from_id_salt("native_tunnel_cells")
            .width(165.0)
            .selected_text(format!("{} ячеек / хорду", self.cells))
            .show_ui(ui, |ui| {
                for n in [32, 40, 64, 96] {
                    ui.selectable_value(&mut self.cells, n, format!("{n} ячеек / хорду"));
                }
            });
    }

    pub fn summary(&self, ui: &mut egui::Ui) {
        let text = self.snapshot.as_ref().map_or_else(|| "Подготовка поля сечения…".to_string(), |f| format!(
            "Сечение: cℓ = {:.2} · cd = {:.2} · Re расчёта {:.0} · tU/c = {:.2} · min Ux/U∞ = {:.2}",
            f.lift, f.drag, self.config.as_ref().map_or(0.0, |c| c.reynolds), f.time, f.min_ux
        ));
        ui.add(egui::Label::new(egui::RichText::new(&text).small()).truncate())
            .on_hover_text(text);
    }

    /// Keep the solver fed and collect its latest field. Separate from drawing
    /// so the three-dimensional tunnel runs the same computation without the
    /// section view being open.
    pub fn advance(&mut self, analysis: &Analysis) -> Result<(), String> {
        let state = &analysis.state;
        let re = state.velocity * state.chord * analysis.atmosphere.density
            / analysis.atmosphere.viscosity;
        if let Some(message) = Self::unavailable_reason(analysis) {
            self.suspend();
            self.config = None;
            self.snapshot = None;
            self.error = Some(message.into());
            return Err(message.into());
        }
        let config = Config::airfoil(&analysis.foil, state.alpha, re, self.cells);
        if self.config.as_ref() != Some(&config) {
            self.generation = self.worker.submit(config.clone());
            self.config = Some(config);
            self.snapshot = None;
            self.error = None;
            self.started = std::time::Instant::now();
            self.paused = false;
        }
        self.worker.pause(self.paused || self.error.is_some());
        if let Some(result) = self.worker.poll(self.generation) {
            match result {
                Ok(s) => {
                    self.snapshot = Some(s);
                    self.dirty = true;
                }
                Err(e) => self.error = Some(e),
            }
        }
        self.error
            .as_ref()
            .map_or(Ok(()), |error| Err(error.clone()))
    }

    fn draw(
        &mut self,
        ui: &mut egui::Ui,
        f: Arc<Snapshot>,
        state: &glenn_extended::model::Simulation,
        q: f64,
        analysis: &Analysis,
    ) {
        let f = &f;
        let re = state.velocity * state.chord * analysis.atmosphere.density
            / analysis.atmosphere.viscosity;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(format!("Сечение: cℓ = {:.3} · cd = {:.3}", f.lift, f.drag))
                    .color(CYAN),
            );
            ui.label(format!(
                "Re расчёта = {:.0} · Re полёта = {re:.2e} · tU/c = {:.2} · {:.2} с",
                glenn_solver_native::resolved_reynolds(self.cells).min(re),
                f.time,
                f.time * state.chord / state.velocity
            ));
            ui.label(format!("min Ux/U∞ = {:.2}", f.min_ux));
        });
        ui.label(
            egui::RichText::new(
                "Встроенный вязкий расчёт сечения · оранжевый на карте скорости — обратное течение",
            )
            .color(MUTED),
        );
        if self.dirty {
            self.paths = (-7..=7)
                .map(|row| f.trace([-0.65, row as f32 * 0.13]))
                .collect();
            // Additional seeds inside computed reverse-flow cells reveal
            // closed recirculation which upstream streamlines cannot enter.
            for y in (1..f.height - 1).step_by(8) {
                for x in (1..f.width - 1).step_by(8) {
                    let i = y * f.width + x;
                    if !f.solid[i] && f.velocity[i][0] < -0.05 {
                        self.paths.push(f.trace(f.point(x, y)));
                    }
                }
            }
            let mut pixels = Vec::with_capacity(f.width * f.height);
            for y in (0..f.height).rev() {
                for x in 0..f.width {
                    pixels.push(field_color(f, y * f.width + x, self.quantity));
                }
            }
            let image = egui::ColorImage::new([f.width, f.height], pixels);
            if let Some(texture) = &mut self.texture {
                texture.set(image, egui::TextureOptions::LINEAR);
            } else {
                self.texture = Some(ui.ctx().load_texture(
                    "native-viscous-flow",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
            if self.three_d
                && let Some(renderer) = &mut self.renderer
            {
                renderer.upload(&render::Scene::native(analysis, f, self.quantity));
            }
            self.dirty = false;
        }
        let height = (ui.available_height() - 60.0).clamp(240.0, 650.0);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), height), egui::Sense::drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 8, Color32::from_rgb(9, 17, 27));
        if self.three_d {
            if response.dragged() {
                let d = response.drag_delta();
                self.yaw += d.x * 0.006;
                self.pitch = (self.pitch + d.y * 0.006).clamp(-1.4, 1.4);
            }
            if let Some(renderer) = &mut self.renderer {
                let radius = state.span.max(state.chord * 5.0) as f32;
                let target = glam::Vec3::new(state.chord as f32, 0.0, 0.0);
                let eye = target
                    + glam::Vec3::new(
                        self.yaw.sin() * self.pitch.cos(),
                        self.pitch.sin(),
                        self.yaw.cos() * self.pitch.cos(),
                    ) * radius
                        * 2.0;
                let extent = radius * 0.65;
                let h = extent * rect.height() / rect.width();
                let matrix =
                    glam::Mat4::orthographic_rh(-extent, extent, -h, h, 0.01, radius * 6.0)
                        * glam::Mat4::look_at_rh(eye, target, glam::Vec3::Y);
                let scale = ui.ctx().pixels_per_point();
                let tex = renderer.render(
                    [
                        (rect.width() * scale) as u32,
                        (rect.height() * scale) as u32,
                    ],
                    matrix,
                    0.0,
                );
                painter.image(
                    tex,
                    rect,
                    Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        } else {
            // Crop to the body and near wake; preserve physical aspect ratio.
            let scale = (rect.width() / 3.8).min(rect.height() / 2.4);
            let view = Rect::from_center_size(rect.center(), Vec2::new(3.8 * scale, 2.4 * scale));
            let uv = Rect::from_min_max(
                egui::pos2(0.8 / 7.0, 0.8 / 4.0),
                egui::pos2(4.6 / 7.0, 3.2 / 4.0),
            );
            painter.image(
                self.texture.as_ref().unwrap().id(),
                view,
                uv,
                Color32::WHITE,
            );
            let project = |x: f32, y: f32| {
                egui::pos2(view.left() + (x + 0.7) * scale, view.center().y - y * scale)
            };
            // The lattice mask is cell-stepped, so the body reads as a staircase.
            // Draw the contour the mask was built from over it: the same rotation
            // about the quarter chord, so outline and solid cells coincide.
            if let Some(config) = &self.config {
                let (s, c) = (-config.alpha.to_radians()).sin_cos();
                let body: Vec<_> = config
                    .contour
                    .iter()
                    .map(|p| {
                        project(
                            (0.25 + (p.x - 0.25) * c - p.y * s) as f32,
                            ((p.x - 0.25) * s + p.y * c) as f32,
                        )
                    })
                    .collect();
                painter
                    .with_clip_rect(view)
                    .add(egui::Shape::Path(egui::epaint::PathShape {
                        points: body,
                        closed: true,
                        fill: Color32::from_rgb(225, 234, 240),
                        // Wide enough to cover the staircase edge of the mask beneath it.
                        stroke: egui::Stroke::new(2.5, Color32::from_rgb(225, 234, 240)).into(),
                    }));
            }
            for path in &self.paths {
                let points: Vec<_> = path.iter().map(|p| project(p[0], p[1])).collect();
                painter.with_clip_rect(view).add(egui::Shape::line(
                    points,
                    egui::Stroke::new(0.7, Color32::from_white_alpha(80)),
                ));
            }
            // Arrows sample the same computed field, including near-wall reversal.
            for row in -5..=5 {
                for col in -2..=14 {
                    let (x, y) = (col as f32 * 0.2, row as f32 * 0.2);
                    if let Some(u) = f.sample(x, y) {
                        let len = u[0].hypot(u[1]);
                        if len > 0.015 {
                            let v = Vec2::new(u[0], -u[1]) * scale * 0.07 / len.max(1.0);
                            painter.arrow(
                                project(x, y),
                                v,
                                egui::Stroke::new(1.0, Color32::from_white_alpha(145)),
                            );
                        }
                    }
                }
            }
            if let Some(p) = response.hover_pos()
                && view.contains(p)
            {
                let x = (p.x - view.left()) / scale - 0.7;
                let y = (view.center().y - p.y) / scale;
                if let Some(u) = f.sample(x, y) {
                    let gx = ((x + 1.5) * f.cells_per_chord) as usize;
                    let gy = ((y + 2.0) * f.cells_per_chord) as usize;
                    response.on_hover_text(format!(
                        "x/c={x:.3}, y/c={y:.3}\nUx={:.2} м/с, Uy={:.2} м/с\np−p∞={:.2} Па",
                        u[0] as f64 * state.velocity,
                        u[1] as f64 * state.velocity,
                        f.pressure[gy * f.width + gx] as f64 * q
                    ));
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.small(format!("{} × {} · шаг {} · {:.0} шаг/с",f.width,f.height,f.step,f.step as f64/self.started.elapsed().as_secs_f64().max(0.001)));
            ui.small(if self.three_d {"Перетаскивание — поворот. Цветной срез — расчёт корневого сечения, а не всего размаха."} else {"Наведите курсор для измерения скорости и давления."});
        });
        if !self.paused {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(33));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_old_field_cannot_be_used_after_leaving_the_solver_range() {
        let mut analysis = Analysis::new(glenn_extended::model::Simulation::default()).unwrap();
        let re = analysis.state.velocity * analysis.state.chord * analysis.atmosphere.density
            / analysis.atmosphere.viscosity;
        let config = Config::airfoil(&analysis.foil, analysis.state.alpha, re, 40);
        let solver = glenn_solver_native::Solver::new(config.clone()).unwrap();
        let view = Native {
            config: Some(config),
            snapshot: Some(Arc::new(solver.snapshot())),
            ..Native::default()
        };
        assert!(view.snapshot_for(&analysis).is_some());
        let initial = analysis.state.clone();
        let mut state = initial.clone();
        state.velocity = 250.0;
        analysis.update(&state).unwrap();
        assert!(view.snapshot_for(&analysis).is_none());
        analysis.update(&initial).unwrap();
        assert!(view.snapshot_for(&analysis).is_some());
        state = initial;
        state.alpha += 5.0;
        analysis.update(&state).unwrap();
        assert!(view.snapshot_for(&analysis).is_none());
    }

    #[test]
    fn exported_viscous_field_uses_si_units_and_rejects_changed_geometry() {
        let mut analysis = Analysis::new(glenn_extended::model::Simulation::default()).unwrap();
        let re = analysis.state.velocity * analysis.state.chord * analysis.atmosphere.density
            / analysis.atmosphere.viscosity;
        let config = Config::airfoil(&analysis.foil, analysis.state.alpha, re, 40);
        let solver = glenn_solver_native::Solver::new(config.clone()).unwrap();
        let view = Native {
            config: Some(config),
            snapshot: Some(Arc::new(solver.snapshot())),
            ..Native::default()
        };
        let csv = view.export_csv(&analysis).unwrap();
        let row: Vec<f64> = csv
            .lines()
            .nth(2)
            .unwrap()
            .split(',')
            .map(|s| s.parse().unwrap())
            .collect();
        assert!((row[2] - analysis.state.velocity).abs() < 0.001);
        assert!(row[3].abs() < 0.001);
        assert!(row[4].abs() < 0.1);
        analysis.state.alpha += 1.0;
        assert!(view.export_csv(&analysis).is_err());
    }
}
