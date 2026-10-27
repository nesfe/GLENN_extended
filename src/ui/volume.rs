//! Volume workbench. Owns presentation only; cases run in glenn-solver-cfd.
use super::{
    plots::{AMBER, CYAN, MUTED},
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
            backend: Backend {
                wsl: cfg!(target_os = "windows"),
                ..Backend::default()
            },
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
        self.result = None;
        self.error = None;
        self.bounds = mesh.bounds();
        let mesh = Arc::new(mesh);
        self.preview_worker.submit(mesh.clone());
        self.preview = None;
        self.preview_busy = true;
        self.mesh = Some(mesh);
        self.name = name;
        self.scale = 1.0;
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
                        self.cfg = result.config.clone();
                        self.scale = 1.0;
                        self.mesh = Some(Arc::new(result.mesh.clone()));
                        self.bounds = result.bounds;
                        self.name = "Сохранённый CFD-расчёт".into();
                    }
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
        ui.label("OpenFOAM: вязкое несжимаемое обтекание. Линии, скорость, завихренность и силы — из одного расчёта.");
        ui.small("Панельные силы и эмпирический срыв в этом режиме не используются. Для высоких чисел Маха нужен другой расчётный модуль.");
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
                    .range(0.001..=100.0)
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
        ui.collapsing("Подключение решателя и каталог расчётов",|ui| {
            ui.checkbox(&mut self.backend.wsl,"Запуск через WSL (Windows)");
            ui.label("Путь к OpenFOAM etc/bashrc; пусто, если решатель уже в PATH:");ui.text_edit_singleline(&mut self.backend.bashrc);
            ui.small("Пример для WSL: /usr/lib/openfoam/openfoam2412/etc/bashrc. OpenFOAM устанавливается отдельно.");
            ui.label("Каталог результатов (каждый запуск — отдельная папка):");ui.text_edit_singleline(&mut self.folder);
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
        ui.label(self.progress.lock().unwrap().clone());
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
        self.viewport(ui);
        if let Some(result) = &self.result {
            show_slice(
                ui,
                &result.result.field,
                &mut self.slice,
                &mut self.quantity,
            );
        }
        ui.small("Отрыв исследуйте по обратной скорости в следе и завихренности; для выводов нужны сходимость, независимость от сетки, y+ и сравнение с экспериментом. URANS не разрешает все турбулентные вихри.");
    }
    fn viewport(&mut self, ui: &mut egui::Ui) {
        if self.result.is_none() && self.preview.is_none() {
            ui.label("Подготовка геометрии для отображения…");
            return;
        }
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 350.0), egui::Sense::drag());
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
        let half = length / self.zoom;
        let projection = glam::Mat4::orthographic_rh(
            -half * aspect,
            half * aspect,
            -half,
            half,
            -length * 20.0,
            length * 20.0,
        );
        let camera = projection
            * glam::Mat4::from_rotation_x(self.pitch)
            * glam::Mat4::from_rotation_y(self.yaw)
            * glam::Mat4::from_translation(-center);
        let px = ui.ctx().pixels_per_point();
        let texture = renderer.render(
            [(rect.width() * px) as u32, (rect.height() * px) as u32],
            camera,
            0.0,
        );
        ui.painter_at(rect).image(
            texture,
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}
fn show_slice(
    ui: &mut egui::Ui,
    field: &cfd::VolumeField,
    slice: &mut usize,
    quantity: &mut usize,
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
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for i in offset..offset + count {
        if let Some(v) = val(i) {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    if !lo.is_finite() {
        ui.label("Нет поля в этом сечении");
        return;
    }
    ui.label(format!(
        "Z = {:.4} м · диапазон [{lo:.4}; {hi:.4}] · тёмные области: нет данных / тело",
        field.position(offset).z
    ));
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 260.0), egui::Sense::hover());
    let ratio = ((field.max.x - field.min.x) / (field.max.y - field.min.y)) as f32;
    let width = rect.width().min(rect.height() * ratio);
    let height = width / ratio;
    let rect = egui::Rect::from_center_size(rect.center(), Vec2::new(width, height));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, Color32::from_rgb(9, 17, 27));
    let w = rect.width() / field.dims[0] as f32;
    let h = rect.height() / field.dims[1] as f32;
    for y in 0..field.dims[1] {
        for x in 0..field.dims[0] {
            let i = offset + y * field.dims[0] + x;
            let point = rect.left_bottom() + Vec2::new(x as f32 * w, -((y + 1) as f32) * h);
            if let Some(v) = val(i) {
                let t = ((v - lo) / (hi - lo).max(1e-12)) as f32;
                let color = Color32::from_rgb(
                    (230.0 * t) as u8,
                    (90.0 + 110.0 * (1.0 - (2.0 * t - 1.0).abs())) as u8,
                    (230.0 * (1.0 - t)) as u8,
                );
                painter.rect_filled(
                    egui::Rect::from_min_size(point, Vec2::new(w + 0.2, h + 0.2)),
                    0,
                    color,
                );
                if x % 3 == 0
                    && y % 3 == 0
                    && let Some(v) = field.velocity[i]
                {
                    let d = Vec2::new(v.x as f32, -v.y as f32).normalized() * w * 1.8;
                    painter.arrow(
                        point + Vec2::new(w, h) * 0.5,
                        d,
                        egui::Stroke::new(1.0, Color32::WHITE),
                    );
                }
            }
        }
    }
    ui.colored_label(CYAN,"Стрелки — направление скорости в XY. Отрицательная скорость X означает возвратное течение.");
    ui.colored_label(MUTED,"Сечение интерполировано из CFD; мгновенные линии тока не являются траекториями частиц во времени.");
}
