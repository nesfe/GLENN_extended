//! Volume workbench. Owns presentation only; cases run in glenn-solver-cfd.
use super::{
    plots::{AMBER, CYAN},
    render,
};
use eframe::egui::{self, Color32, Vec2};
use glenn_extended::{
    cfd::{self, Backend, Config, FlowModel, Resolution, RunResult},
    engine::Worker,
    mesh::SolidMesh,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
struct Request {
    mesh: Option<Arc<SolidMesh>>,
    cfg: Config,
    backend: Backend,
    path: PathBuf,
    scale: f64,
}
struct Completed {
    result: RunResult,
    mesh: SolidMesh,
    scene: render::Scene,
    settings: String,
    loaded: bool,
    config: Config,
    bounds: (glam::DVec3, glam::DVec3),
}
pub struct Volume {
    mesh: Option<Arc<SolidMesh>>,
    name: String,
    pub cfg: Config,
    backend: Backend,
    scale: f64,
    folder: String,
    worker: Worker<Request, Completed>,
    preview_worker: Worker<Arc<SolidMesh>, render::Scene>,
    preview: Option<render::Scene>,
    preview_busy: bool,
    bounds: (glam::DVec3, glam::DVec3),
    progress: Arc<Mutex<String>>,
    result: Option<Completed>,
    pub renderer: Option<render::Renderer>,
    dirty: bool,
    pub busy: bool,
    error: Option<String>,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    slice: usize,
    quantity: usize,
    tab: usize,
    slice_map: Option<SliceMap>,
    show_lines: bool,
    reference_area: f64,
    comparison: super::volume_compare::Comparison,
    animate: bool,
    clock: f64,
    last_frame: Option<f64>,
}
impl Default for Volume {
    fn default() -> Self {
        let progress = Arc::new(Mutex::new(String::new()));
        let state = progress.clone();
        let worker = Worker::new(move |request: Request, token| {
            let loaded = request.mesh.is_none();
            let (mesh, config, result) = if let Some(mesh) = request.mesh {
                let mesh = mesh.scaled(request.scale)?;
                let result = cfd::run(
                    &mesh,
                    &request.cfg,
                    &request.backend,
                    &request.path,
                    token,
                    &state,
                )?;
                (mesh, request.cfg, result)
            } else {
                let config: Config = serde_json::from_str(
                    &std::fs::read_to_string(request.path.join("glenn-case.json"))
                        .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                let mesh = glenn_extended::interchange::stl::read(
                    &request.path.join("constant/triSurface/body.stl"),
                )?;
                let result = cfd::load_result(&request.path)?;
                (mesh, config, result)
            };
            token.check()?;
            let scene = render::Scene::volume(
                &mesh,
                &result.streamlines,
                Some(&result.field),
                config.speed,
            );
            let settings = serde_json::to_string(&config).unwrap() + &request.scale.to_string();
            Ok(Completed {
                bounds: mesh.bounds(),
                result,
                mesh,
                scene,
                settings,
                config,
                loaded,
            })
        });
        Self {
            preview_worker: Worker::new(|mesh: Arc<SolidMesh>, token| {
                token.check()?;
                let scene = render::Scene::volume(&mesh, &[], None, 1.0);
                token.check()?;
                Ok(scene)
            }),
            preview: None,
            preview_busy: false,
            bounds: (glam::DVec3::ZERO, glam::DVec3::ONE),
            mesh: None,
            name: String::new(),
            cfg: Config::default(),
            backend: Backend::default(),
            scale: 1.0,
            folder: std::env::current_dir()
                .unwrap_or_default()
                .join("glenn-cases")
                .display()
                .to_string(),
            worker,
            progress,
            result: None,
            renderer: None,
            dirty: false,
            busy: false,
            error: None,
            yaw: -0.65,
            pitch: 0.48,
            zoom: 1.0,
            slice: 12,
            quantity: 0,
            tab: 0,
            slice_map: None,
            reference_area: 0.0,
            comparison: Default::default(),
            show_lines: true,
            animate: true,
            clock: 0.0,
            last_frame: None,
        }
    }
}
impl Volume {
    pub fn pending(&self) -> bool {
        self.busy || self.preview_busy
    }
    pub fn open_case(&mut self, path: PathBuf) {
        self.worker.submit(Request {
            mesh: None,
            cfg: self.cfg.clone(),
            backend: self.backend.clone(),
            path,
            scale: 1.0,
        });
        self.busy = true;
        self.error = None;
    }
    pub fn set_mesh(&mut self, mesh: SolidMesh, name: String) {
        self.worker.cancel();
        self.busy = false;
        self.comparison.reset();
        self.result = None;
        self.tab = 0;
        self.slice_map = None;
        self.error = None;
        self.bounds = mesh.bounds();
        let mesh = Arc::new(mesh);
        self.preview_worker.submit(mesh.clone());
        self.preview = None;
        self.preview_busy = true;
        self.mesh = Some(mesh);
        self.name = name;
        self.reference_area = 0.0;
        self.scale = 1.0;
        self.zoom = 1.0;
        self.yaw = -0.65;
        self.pitch = 0.48;
        self.slice = 0;
        self.dirty = true;
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some((_, result)) = self.preview_worker.poll() {
            self.preview_busy = false;
            match result {
                Ok(scene) => {
                    self.preview = Some(scene);
                    self.dirty = true;
                }
                Err(e) => self.error = Some(e),
            }
        }
        if let Some((_, result)) = self.worker.poll() {
            self.busy = false;
            match result {
                Ok(result) => {
                    if result.loaded {
                        self.reference_area = 0.0;
                        self.cfg = result.config.clone();
                        self.scale = 1.0;
                        self.mesh = Some(Arc::new(result.mesh.clone()));
                        self.bounds = result.bounds;
                        self.name = "Сохранённый CFD-расчёт".into();
                    }
                    // Default to the plane through the model, not a slice far
                    // outside it (the old fixed index showed only free stream).
                    let mid_z = (result.bounds.0.z + result.bounds.1.z) * 0.5;
                    let field = &result.result.field;
                    self.slice = (((mid_z - field.min.z) / (field.max.z - field.min.z))
                        * (field.dims[2] - 1) as f64)
                        .round()
                        .clamp(0.0, (field.dims[2] - 1) as f64)
                        as usize;
                    self.slice_map = None;
                    self.comparison.reset();
                    self.result = Some(result);
                    self.dirty = true;
                }
                Err(e) => self.error = Some(e),
            }
        }
        if self.busy || self.preview_busy {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.heading("Объёмная CFD · STL / крыло");
        ui.label(format!("OpenFOAM 2412 · {}", self.cfg.solver()));
        if let Some(error) = &self.error {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.colored_label(AMBER, error);
            });
        }
        if ui.button("Открыть сохранённый CFD-расчёт…").clicked()
            && let Some(path) = rfd::FileDialog::new().pick_folder()
        {
            self.open_case(path);
        }
        if self.busy && self.mesh.is_none() {
            ui.spinner();
        }
        let Some(mesh) = self.mesh.clone() else {
            ui.label("Откройте STL через «Открыть…» или передайте текущее крыло кнопкой выше.");
            return;
        };
        ui.label(format!(
            "{} · {} треугольников",
            self.name,
            mesh.triangles.len()
        ));
        let (lo, hi) = self.bounds;
        ui.horizontal_wrapped(|ui| {
            ui.label("Единицы координат STL:");
            for (value, label) in [(1.0, "м"), (0.001, "мм"), (0.01, "см")] {
                if ui.selectable_value(&mut self.scale, value, label).changed() {
                    self.dirty = true;
                }
            }
            let size = (hi - lo) * self.scale;
            ui.label(format!(
                "Размеры: {:.4} × {:.4} × {:.4} м",
                size.x, size.y, size.z
            ));
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Рассчитать продувку"))
                .clicked()
            {
                let stamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                let path = PathBuf::from(&self.folder).join(format!("case-{stamp}"));
                self.worker.submit(Request {
                    mesh: Some(mesh.clone()),
                    cfg: self.cfg.clone(),
                    backend: self.backend.clone(),
                    path,
                    scale: self.scale,
                });
                self.busy = true;
                self.error = None;
            }
            if self.busy {
                ui.spinner();
                if ui.button("Отменить").clicked() {
                    self.worker.cancel();
                    self.busy = false;
                    *self.progress.lock().unwrap() =
                        "Отмена запрошена; процесс завершает работу".into();
                }
            }
        });
        if self.busy {
            ui.label(self.progress.lock().unwrap().clone());
        }
        ui.horizontal_wrapped(|ui| {
            for (index, label) in ["3D-поток", "Срез поля", "Графики", "Настройки"]
                .into_iter()
                .enumerate()
            {
                ui.add_enabled_ui(index == 0 || index == 3 || self.result.is_some(), |ui| {
                    ui.selectable_value(&mut self.tab, index, label);
                });
            }
        });
        if self.tab == 0 {
            // Show the imported model immediately, before settings that can scroll
            // it below the window. Camera fitting uses its projected silhouette.
            ui.horizontal_wrapped(|ui| {
                for (label, yaw, pitch) in [
                    ("Изометрия", -0.65, 0.48),
                    ("Сверху", 0.0, 1.57),
                    ("Сбоку", 0.0, 0.0),
                ] {
                    if ui.button(label).clicked() {
                        self.yaw = yaw;
                        self.pitch = pitch;
                        self.zoom = 1.0;
                    }
                }
                if ui.button("Вписать модель").clicked() {
                    self.zoom = 1.0;
                }
                ui.checkbox(&mut self.show_lines, "Линии тока");
                ui.checkbox(&mut self.animate, "Анимация");
            });
            self.viewport(ui);
        }
        if self.tab == 3 {
            let mut air = self.cfg.air_temperature.is_some();
            if ui
                .checkbox(&mut air, "Сжимаемый воздух · уравнение энергии")
                .changed()
            {
                self.cfg.air_temperature = air.then_some(288.15);
            }
            if let Some(t) = &mut self.cfg.air_temperature {
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::DragValue::new(t)
                            .range(150.0..=500.0)
                            .prefix("Температура, К "),
                    );
                    ui.label(format!(
                        "M = {:.3}",
                        self.cfg.speed / Config::sound_speed(*t)
                    ));
                });
            }
            ui.horizontal_wrapped(|ui| {
                for (value, label) in [
                    (FlowModel::Laminar, "Ламинарный"),
                    (FlowModel::SteadyRans, "Стационарный RANS SST"),
                    (FlowModel::UnsteadyRans, "Нестационарный URANS SST"),
                ] {
                    ui.selectable_value(&mut self.cfg.model, value, label);
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Сетка:");
                for (value, label) in [
                    (Resolution::Preview, "Быстрый просмотр"),
                    (Resolution::Standard, "Средняя"),
                    (Resolution::Fine, "Подробная"),
                ] {
                    ui.selectable_value(&mut self.cfg.resolution, value, label);
                }
            });
            if self.cfg.resolution == Resolution::Preview {
                ui.colored_label(AMBER,"Грубая сетка без призматических слоёв: только предварительное исследование потока, не проверенная точность сил/отрыва.");
            }
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::DragValue::new(&mut self.cfg.speed)
                        .range(
                            0.001
                                ..=self
                                    .cfg
                                    .air_temperature
                                    .map_or(100.0, |t| 0.95 * Config::sound_speed(t)),
                        )
                        .speed(0.2)
                        .prefix("V, м/с: "),
                );
                ui.add(
                    egui::DragValue::new(&mut self.cfg.alpha_degrees)
                        .range(-90.0..=90.0)
                        .speed(0.2)
                        .prefix("α, °: "),
                );
                ui.add(
                    egui::DragValue::new(&mut self.cfg.density)
                        .range(0.001..=20000.0)
                        .speed(0.01)
                        .prefix("ρ, кг/м³: "),
                );
                ui.add(
                    egui::DragValue::new(&mut self.cfg.kinematic_viscosity)
                        .range(1e-9..=1.0)
                        .speed(1e-6)
                        .max_decimals(9)
                        .prefix("ν, м²/с: "),
                );
            });
            ui.horizontal_wrapped(|ui| {
                if self.cfg.model != FlowModel::UnsteadyRans {
                    ui.add(
                        egui::DragValue::new(&mut self.cfg.iterations)
                            .range(10..=100000)
                            .prefix("Итерации: "),
                    );
                }
                if self.cfg.model == FlowModel::UnsteadyRans {
                    ui.add(
                        egui::DragValue::new(&mut self.cfg.transit_times)
                            .range(0.1..=1000.0)
                            .prefix("Время, L/V: "),
                    );
                }
                ui.label(format!(
                    "Re по максимальному габариту: {:.3e}",
                    self.cfg.speed * (hi - lo).max_element() * self.scale
                        / self.cfg.kinematic_viscosity
                ));
            });
            ui.collapsing("Каталог расчётов и дополнительные настройки",|ui| {
            ui.checkbox(&mut self.backend.wsl,"Запуск через WSL (Windows)");
            ui.label("Внешний OpenFOAM etc/bashrc (для разработчиков); пусто — комплектный решатель:");ui.text_edit_singleline(&mut self.backend.bashrc);
            ui.small("Обычный запуск использует решатель из полного архива GLENN. Эти настройки нужны только для замены расчётного модуля.");
            ui.label("Каталог результатов (каждый запуск — отдельная папка):");ui.text_edit_singleline(&mut self.folder);
        });
        }
        if let Some(completed) = &self.result {
            if completed.settings
                != serde_json::to_string(&self.cfg).unwrap() + &self.scale.to_string()
            {
                ui.colored_label(
                    AMBER,
                    "Настройки изменены. Отображается предыдущий расчёт; запустите новый.",
                );
            }
            let r = &completed.result;
            ui.label(format!(
                "t = {:.6} · Fx {:.4} Н · Fy {:.4} Н · Fz {:.4} Н",
                r.field.time, r.force.x, r.force.y, r.force.z
            ));
            if let Some([min, max, avg]) = r.wall_y_plus {
                ui.label(format!(
                    "Стенка модели · y⁺ мин. {min:.1} · сред. {avg:.1} · макс. {max:.1}"
                ));
            }
            if completed.config.model == FlowModel::UnsteadyRans {
                ui.colored_label(AMBER, "Мгновенное поле URANS. Для средних сил требуется установившаяся статистика во времени.");
            } else if !r.converged {
                ui.colored_label(AMBER,"Стационарная сходимость не подтверждена. Это не автоматически достоверный инженерный результат.");
            }
            ui.small(format!(
                "Журналы, поля и result.foam для ParaView: {}",
                r.case_dir.display()
            ));
        }
        if self.tab == 2
            && let Some(result) = &self.result
        {
            charts(ui, result);
            if !result.result.wall_pressure.is_empty() {
                let f = &result.result.field;
                ui.add(
                    egui::Slider::new(&mut self.slice, 0..=f.dims[2] - 1)
                        .text("Сечение давления Z"),
                );
                let dz = (f.max.z - f.min.z) / (f.dims[2] - 1) as f64;
                let z = f.min.z + dz * self.slice as f64;
                let q = 0.5 * result.config.density * result.config.speed.powi(2);
                let cp: Vec<_> = result
                    .result
                    .wall_pressure
                    .iter()
                    .filter(|(p, _)| (p.z - z).abs() <= dz * 0.5)
                    .map(|(p, v)| glenn_extended::model::Point::new(p.x, v / q))
                    .collect();
                ui.label(format!(
                    "Z = {z:.3} ± {:.3} м · {} граней",
                    dz * 0.5,
                    cp.len()
                ));
                super::plots::scatter(ui, "Давление на стенке", "X, м", "Cp", &cp);
            }
            ui.horizontal_wrapped(|ui| {
                ui.label("Опорная площадь крыла, м²");
                ui.add(
                    egui::DragValue::new(&mut self.reference_area)
                        .range(0.0..=1e8)
                        .speed(0.01),
                );
                if let Some((cl, cd)) = result
                    .config
                    .coefficients(result.result.force, self.reference_area)
                {
                    ui.label(format!("CL {cl:.5} · CD {cd:.5}"));
                }
            });
            self.comparison
                .show(ui, &result.result, &result.config, self.reference_area);
            if ui.button("Сохранить историю сил CSV…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_file_name("forces.csv")
                    .save_file()
            {
                let mut csv = String::from(if result.config.model == FlowModel::UnsteadyRans {
                    "time_s"
                } else {
                    "iteration"
                });
                csv.push_str(",Fx_N,Fy_N,Fz_N,CL,CD\n");
                for &(t, f) in &result.result.force_history {
                    use std::fmt::Write;
                    let coeff = result
                        .config
                        .coefficients(f, self.reference_area)
                        .map_or_else(|| ",".to_owned(), |(cl, cd)| format!("{cl},{cd}"));
                    writeln!(&mut csv, "{t},{},{},{},{coeff}", f.x, f.y, f.z).unwrap();
                }
                if let Err(e) = std::fs::write(path, csv) {
                    self.error = Some(e.to_string());
                }
            }
        }
        if self.tab == 1
            && let Some(result) = &self.result
        {
            show_slice(
                ui,
                &result.result.field,
                &mut self.slice,
                &mut self.quantity,
                &mut self.slice_map,
            );
        }
        ui.small("Отрыв исследуйте по обратной скорости в следе и завихренности; для выводов нужны сходимость, независимость от сетки, y+ и сравнение с экспериментом. URANS не разрешает все турбулентные вихри.");
    }
    fn viewport(&mut self, ui: &mut egui::Ui) {
        if self.result.is_none() && self.preview.is_none() {
            ui.label("Подготовка геометрии для отображения…");
            return;
        }
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(
                ui.available_width(),
                (ui.clip_rect().bottom() - ui.cursor().top() - 12.0).clamp(240.0, 1400.0),
            ),
            egui::Sense::drag(),
        );
        if response.dragged() {
            let d = ui.input(|i| i.pointer.delta());
            self.yaw += d.x * 0.008;
            self.pitch = (self.pitch + d.y * 0.008).clamp(-1.5, 1.5);
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            self.zoom = (self.zoom * (scroll * 0.002).exp()).clamp(0.25, 4.0);
        }
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        renderer.show_lines = self.show_lines;
        if self.dirty {
            if let Some(result) = &self.result {
                renderer.upload(&result.scene);
            } else if let Some(preview) = &self.preview {
                renderer.upload(preview);
            }
            self.dirty = false;
        }
        let (lo, hi) = if let Some(result) = &self.result {
            result.bounds
        } else {
            // Preview stays in source units; camera normalization makes its
            // appearance independent of the selected physical unit conversion.
            self.bounds
        };
        let center = ((lo + hi) * 0.5).as_vec3();
        let length = (hi - lo).max_element() as f32;
        let aspect = rect.width() / rect.height();
        let rotation =
            glam::Mat4::from_rotation_x(self.pitch) * glam::Mat4::from_rotation_y(self.yaw);
        let mut projected = glam::Vec3::ZERO;
        for x in [lo.x, hi.x] {
            for y in [lo.y, hi.y] {
                for z in [lo.z, hi.z] {
                    projected = projected.max(
                        rotation
                            .transform_vector3(
                                glam::Vec3::new(x as f32, y as f32, z as f32) - center,
                            )
                            .abs(),
                    );
                }
            }
        }
        let half = (projected.y.max(projected.x / aspect) * 1.3).max(length * 0.04) / self.zoom;
        let projection = glam::Mat4::orthographic_rh(
            -half * aspect,
            half * aspect,
            -half,
            half,
            -length * 20.0,
            length * 20.0,
        );
        let camera = projection * rotation * glam::Mat4::from_translation(-center);
        let now = ui.input(|i| i.time);
        if self.show_lines && self.animate && self.result.is_some() {
            self.clock += self
                .last_frame
                .map_or(0.0, |last| (now - last).clamp(0.0, 0.1));
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        }
        self.last_frame = Some(now);
        let px = ui.ctx().pixels_per_point();
        let texture = renderer.render(
            [(rect.width() * px) as u32, (rect.height() * px) as u32],
            camera,
            self.clock as f32,
        );
        ui.painter_at(rect).image(
            texture,
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}
struct SliceMap {
    slice: usize,
    quantity: usize,
    texture: egui::TextureHandle,
    range: (f64, f64),
}
fn charts(ui: &mut egui::Ui, completed: &Completed) {
    use super::plots::plot;
    use glenn_extended::model::Point;
    let r = &completed.result;
    let x = if completed.config.model == FlowModel::UnsteadyRans {
        "Время, с"
    } else {
        "Итерация SIMPLE"
    };
    let colors = [CYAN, AMBER, Color32::LIGHT_GREEN, Color32::LIGHT_RED];
    let forces: [Vec<Point>; 3] = std::array::from_fn(|i| {
        r.force_history
            .iter()
            .map(|(t, f)| Point::new(*t, f[i]))
            .collect()
    });
    plot(
        ui,
        "Силы на модели",
        x,
        "Н",
        &[
            ("Fx", &forces[0], colors[0]),
            ("Fy", &forces[1], colors[1]),
            ("Fz", &forces[2], colors[2]),
        ],
        false,
    );
    let residuals: [Vec<Point>; 4] = std::array::from_fn(|i| {
        r.residuals[i]
            .iter()
            .filter(|(_, v)| *v > 0.0)
            .map(|(t, v)| Point::new(*t, v.log10()))
            .collect()
    });
    if residuals.iter().any(|s| !s.is_empty()) {
        plot(
            ui,
            "Начальные невязки уравнений",
            x,
            "log₁₀ r",
            &[
                ("Ux", &residuals[0], colors[0]),
                ("Uy", &residuals[1], colors[1]),
                ("Uz", &residuals[2], colors[2]),
                ("p", &residuals[3], colors[3]),
            ],
            false,
        );
    }
}
fn slice_color(t: f64) -> Color32 {
    let colors = [
        [34., 44., 110.],
        [39., 119., 175.],
        [70., 187., 162.],
        [225., 216., 91.],
        [231., 95., 48.],
    ];
    let position = t.clamp(0.0, 1.0) * 4.;
    let i = (position as usize).min(3);
    let channel =
        |k: usize| (colors[i][k] + (position - i as f64) * (colors[i + 1][k] - colors[i][k])) as u8;
    Color32::from_rgb(channel(0), channel(1), channel(2))
}
fn show_slice(
    ui: &mut egui::Ui,
    field: &cfd::VolumeField,
    slice: &mut usize,
    quantity: &mut usize,
    cache: &mut Option<SliceMap>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.add(egui::Slider::new(slice, 0..=field.dims[2] - 1).text("Сечение Z"));
        for (i, label) in ["Скорость X, м/с", "Давление, Па", "|ω|, 1/с"]
            .iter()
            .enumerate()
        {
            ui.selectable_value(quantity, i, *label);
        }
    });
    let val = |i: usize| match *quantity {
        0 => field.velocity[i].map(|v| v.x),
        1 => field.pressure[i],
        _ => field.vorticity[i].map(|v| v.length()),
    };
    let offset = *slice * field.dims[0] * field.dims[1];
    let count = field.dims[0] * field.dims[1];
    let [nx, ny, _] = field.dims;
    if cache
        .as_ref()
        .is_none_or(|c| (c.slice, c.quantity) != (*slice, *quantity))
    {
        let (lo, hi) = (offset..offset + count)
            .filter_map(val)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(v), hi.max(v))
            });
        let mut pixels = Vec::with_capacity(count);
        for y in (0..ny).rev() {
            for x in 0..nx {
                pixels.push(
                    val(offset + y * nx + x).map_or(Color32::from_rgb(9, 17, 27), |v| {
                        slice_color((v - lo) / (hi - lo).max(1e-12))
                    }),
                );
            }
        }
        let texture = ui.ctx().load_texture(
            "cfd-slice",
            egui::ColorImage::new([nx, ny], pixels),
            egui::TextureOptions::NEAREST,
        );
        *cache = Some(SliceMap {
            slice: *slice,
            quantity: *quantity,
            texture,
            range: (lo, hi),
        });
    }
    let c = cache.as_ref().unwrap();
    let (lo, hi) = c.range;
    if !lo.is_finite() {
        ui.label("Нет данных в этом сечении");
        return;
    }
    let unit = ["м/с", "Па", "1/с"][*quantity];
    ui.label(format!(
        "Z = {:.3} м · тёмные ячейки вне поля данных",
        field.position(offset).z
    ));
    let w = ui.available_width();
    let ratio = ((field.max.x - field.min.x) / (field.max.y - field.min.y)) as f32;
    let h = (w / ratio).min(ui.ctx().content_rect().height() * 0.60);
    let response = ui.add(
        egui::Image::new((c.texture.id(), Vec2::new(h * ratio, h))).sense(egui::Sense::hover()),
    );
    if let Some(p) = response.hover_pos() {
        let rect = response.rect;
        let x = (((p.x - rect.left()) / rect.width() * nx as f32) as usize).min(nx - 1);
        let y = (((rect.bottom() - p.y) / rect.height() * ny as f32) as usize).min(ny - 1);
        let i = offset + y * nx + x;
        let pos = field.position(i);
        response.on_hover_text(format!(
            "X {:.3} м · Y {:.3} м · {}",
            pos.x,
            pos.y,
            val(i).map_or_else(|| "нет данных".to_string(), |v| format!("{v:.4} {unit}"))
        ));
    }
    ui.horizontal(|ui| {
        ui.label(format!("{lo:.3}"));
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width().min(180.0), 12.0),
            egui::Sense::hover(),
        );
        for i in 0..64 {
            let x = rect.left() + i as f32 * rect.width() / 64.0;
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(x, rect.top()),
                    Vec2::new(rect.width() / 64.0 + 0.5, rect.height()),
                ),
                0,
                slice_color(i as f64 / 63.0),
            );
        }
        ui.label(format!("{hi:.3} {unit}"));
    });
}
