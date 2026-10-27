//! Desktop composition. Panels own their space; no floating tool windows can
//! become unreachable. Narrow windows put controls on a dedicated page.
mod controls;
mod jobs;
mod pages;
mod plots;
mod render;
mod tunnel;
mod volume;
use crate::audio::TunnelAudio;
use eframe::egui::{self, Color32, RichText, Vec2};
use glenn_extended::{
    interchange,
    model::{self, Point, Simulation},
    physics::{
        aerodynamics::Analysis,
        flight::{self, Performance},
        probe,
    },
};
use plots::{AMBER, CYAN, MUTED};
use std::{path::PathBuf, time::Duration};

#[derive(Clone)]
pub struct Capture {
    pub path: PathBuf,
    pub size: [f32; 2],
    pub page: usize,
    pub scale: f32,
    pub control_tab: usize,
    pub preset: usize,
    pub scroll_end: bool,
    pub alpha: Option<f64>,
    pub camera: usize,
    pub input: Option<PathBuf>,
    pub cfd_case: Option<PathBuf>,
}
impl Capture {
    pub fn from_args(args: &[String]) -> Option<Self> {
        let value = |flag: &str| {
            args.iter()
                .position(|arg| arg == flag)
                .and_then(|i| args.get(i + 1))
        };
        let path = PathBuf::from(value("--screenshot")?);
        let size = value("--size")
            .and_then(|s| s.split_once('x'))
            .and_then(|(w, h)| Some([w.parse().ok()?, h.parse().ok()?]))
            .unwrap_or([1440.0, 900.0]);
        Some(Self {
            input: value("--input").map(PathBuf::from),
            cfd_case: value("--cfd-case").map(PathBuf::from),
            scroll_end: args.iter().any(|arg| arg == "--scroll-end"),
            alpha: value("--alpha").and_then(|s| s.parse().ok()),
            camera: value("--camera").and_then(|s| s.parse().ok()).unwrap_or(0),
            control_tab: value("--control-tab")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            preset: value("--preset").and_then(|s| s.parse().ok()).unwrap_or(0),
            path,
            size,
            page: value("--page").and_then(|s| s.parse().ok()).unwrap_or(0),
            scale: value("--scale").and_then(|s| s.parse().ok()).unwrap_or(1.0),
        })
    }
}
pub struct Workbench {
    state: Simulation,
    analysis: Analysis,
    performance: Performance,
    worker: glenn_extended::engine::Worker<jobs::Request, jobs::Snapshot>,
    requested: Option<jobs::Request>,
    import_message: Option<Result<String, String>>,
    importer: glenn_extended::engine::Worker<(PathBuf, Simulation), jobs::Imported>,
    importing: bool,
    view: tunnel::View,
    tunnel: tunnel::Tunnel,
    volume_flow: volume::Volume,
    page: usize,
    control_tab: usize,
    preset: usize,
    presets: Vec<model::Preset>,
    status: String,
    audio: Option<TunnelAudio>,
    volume: f32,
    capture: Option<Capture>,
    frames: usize,
    /// Rectangles recorded for headless layout regression checks.
    regions: Vec<(&'static str, egui::Rect)>,
}
fn style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "DejaVu".into(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/DejaVuSans.ttf")).into(),
    );
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "DejaVu".into());
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = Color32::from_rgb(14, 22, 33);
    style.visuals.window_fill = Color32::from_rgb(19, 29, 42);
    style.visuals.extreme_bg_color = Color32::from_rgb(9, 17, 27);
    style.visuals.selection.bg_fill = Color32::from_rgb(28, 79, 91);
    style.visuals.selection.stroke = egui::Stroke::new(1.0, CYAN);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(34, 104, 117);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(38, 57, 75);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(25, 38, 53);
    style.spacing.item_spacing = Vec2::new(10.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 7.0);
    style.spacing.interact_size.y = 30.0;
    style.spacing.slider_width = 150.0;
    style.spacing.scroll = egui::style::ScrollStyle::solid();
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
    ctx.set_style(style);
}
impl Workbench {
    pub fn new(creation: &eframe::CreationContext<'_>, capture: Option<Capture>) -> Self {
        style(&creation.egui_ctx);
        let saved = if capture.is_none() {
            creation
                .storage
                .and_then(|storage| storage.get_string("simulation-v3"))
                .and_then(|text| interchange::load_project(&text).ok())
                .map(|(state, _)| state)
        } else {
            None
        };
        let mut starting_state = capture
            .as_ref()
            .and_then(|capture| {
                model::presets()
                    .get(capture.preset)
                    .map(|preset| preset.state.clone())
            })
            .or(saved)
            .unwrap_or_default();
        if let Some(alpha) = capture.as_ref().and_then(|c| c.alpha) {
            starting_state.alpha = alpha;
        }
        let mut app = Self::with_state(starting_state);
        if let Some(capture) = &capture {
            app.page = capture.page.min(7);
            app.control_tab = capture.control_tab.min(3);
            app.preset = capture.preset.min(app.presets.len() - 1);
            app.view.animate = false;
            let cameras = [
                (-0.65, 0.48),
                (0.0, 0.0),
                (0.0, std::f64::consts::FRAC_PI_2),
                (std::f64::consts::FRAC_PI_2, 0.0),
            ];
            (app.view.yaw, app.view.pitch) = cameras[capture.camera.min(3)];
            creation.egui_ctx.set_zoom_factor(capture.scale);
        }
        app.tunnel.renderer = creation
            .wgpu_render_state
            .clone()
            .map(render::Renderer::new);
        app.volume_flow.renderer = creation
            .wgpu_render_state
            .clone()
            .map(render::Renderer::new);
        if let Some(path) = capture.as_ref().and_then(|c| c.input.clone()) {
            app.load(path);
        }
        if let Some(path) = capture.as_ref().and_then(|c| c.cfd_case.clone()) {
            app.volume_flow.open_case(path);
            app.page = 7;
        }
        app.capture = capture;
        app
    }
    fn with_state(state: Simulation) -> Self {
        let (analysis, status) = match Analysis::new(state) {
            Ok(analysis) => (
                analysis,
                "Готово. Перетащите DAT, DXF, STL или JSON в окно.".into(),
            ),
            Err(error) => (
                Analysis::new(Simulation::default()).expect("tested default simulation"),
                format!(
                    "Сохранённая геометрия недоступна: {error}. Восстановлены начальные параметры."
                ),
            ),
        };
        let state = analysis.state.clone();
        let performance = flight::calculate(&analysis);
        let worker = jobs::worker(analysis.clone());
        Self {
            worker,
            requested: None,
            import_message: None,
            importer: jobs::importer(),
            importing: false,
            state,
            analysis,
            performance,
            view: tunnel::View::default(),
            tunnel: tunnel::Tunnel::default(),
            volume_flow: volume::Volume::default(),
            page: 0,
            control_tab: 0,
            preset: 0,
            presets: model::presets(),
            status,
            audio: None,
            volume: 0.35,
            capture: None,
            frames: 0,
            regions: Vec::new(),
        }
    }
    fn load(&mut self, path: PathBuf) {
        self.status = format!("Чтение {}…", path.display());
        self.import_message = None;
        self.importing = true;
        self.importer.submit((path, self.state.clone()));
    }
    fn poll_import(&mut self) {
        if let Some((_, result)) = self.importer.poll() {
            self.importing = false;
            let message = result.map(|imported| match imported {
                jobs::Imported::Profile(state, message) => {
                    self.state = state;
                    self.page = 1;
                    message
                }
                jobs::Imported::Project(state, message) => {
                    self.state = state;
                    message
                }
                jobs::Imported::Solid(mesh, name) => {
                    self.volume_flow.set_mesh(mesh, name.clone());
                    self.page = 7;
                    format!("STL «{name}» загружен. Выберите единицы координат и запустите CFD.")
                }
            });
            self.status = match &message {
                Ok(s) => s.clone(),
                Err(e) => format!("Ошибка импорта: {e}"),
            };
            self.import_message = Some(message);
        }
    }
    fn import_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Профиль или проект", &["dat", "dxf", "stl", "json"])
            .pick_file()
        {
            self.load(path);
        }
    }
    fn export(&mut self, extension: &str) {
        if extension != "json" && (self.tunnel.busy || self.analysis.state != self.state) {
            self.import_message = Some(Err(
                "Экспорт результата доступен после завершения текущего расчёта.".into(),
            ));
            return;
        }
        let data = match extension {
            "json" => interchange::save_project(&self.state),
            "dat" => Ok(interchange::export_dat(&self.analysis)),
            "csv" => Ok(interchange::export_csv(&self.analysis)),
            "obj" => Ok(interchange::export_obj(&self.analysis.mesh)),
            "stl" => Ok(interchange::export_stl(&self.analysis.mesh)),
            _ => return,
        };
        match data {
            Err(error) => self.status = error,
            Ok(text) => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter(extension, &[extension])
                    .set_file_name(format!("GLENN.{}", extension))
                    .save_file()
                {
                    self.status = match std::fs::write(&path, text) {
                        Ok(()) => format!("Сохранено: {}", path.display()),
                        Err(error) => format!("Ошибка сохранения: {error}"),
                    };
                }
            }
        }
    }
    fn recalculate(&mut self) {
        let request = jobs::Request {
            state: self.state.clone(),
            view: self.view.clone(),
            scene: self.tunnel.renderer.is_some(),
        };
        if self
            .requested
            .as_ref()
            .is_none_or(|old| !old.same(&request))
        {
            self.worker.submit(request.clone());
            self.requested = Some(request);
            self.tunnel.busy = true;
        }
        if let Some((_, result)) = self.worker.poll() {
            self.tunnel.busy = false;
            match result {
                Ok(snapshot) => {
                    self.analysis = snapshot.analysis;
                    self.performance = snapshot.performance;
                    self.tunnel.grid = snapshot.grid;
                    self.tunnel.pending_scene = snapshot.scene;
                }
                Err(error) => {
                    self.status = format!("Расчёт остановлен: {error}");
                    self.import_message = Some(Err(self.status.clone()));
                    self.state = self.analysis.state.clone();
                }
            }
        }
    }
    fn draw(&mut self, ctx: &egui::Context) {
        self.regions.clear();
        let compact = ctx.content_rect().width() < 1000.0;
        let header = egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(16, 26, 38))
                    .inner_margin(14),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("GLENN").size(25.0).strong().color(CYAN));
                    // From the manifest, so a release never shows a stale number.
                    ui.label(
                        RichText::new(concat!("EXTENDED  /  ", env!("CARGO_PKG_VERSION")))
                            .size(12.0)
                            .color(MUTED),
                    );
                    ui.separator();
                    ui.label("Аэродинамическая лаборатория");
                    if ui.button("Открыть…").clicked() {
                        self.import_dialog();
                    }
                    if ui
                        .add_enabled(self.page != 7, egui::Button::new("Сохранить проект"))
                        .clicked()
                    {
                        self.export("json");
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("presets")
                        .width(ui.available_width().clamp(100.0, 380.0))
                        .truncate()
                        .selected_text(self.presets[self.preset].name.clone())
                        .show_ui(ui, |ui| {
                            for (i, preset) in self.presets.iter().enumerate() {
                                if ui
                                    .selectable_value(&mut self.preset, i, &preset.name)
                                    .clicked()
                                {
                                    self.state = preset.state.clone();
                                }
                            }
                        });
                    if ui.button("Сбросить").clicked() {
                        self.state = Simulation::default();
                        self.view = tunnel::View::default();
                        self.preset = 0;
                    }
                    let mut audio_enabled = self.audio.is_some();
                    if ui.checkbox(&mut audio_enabled, "Звук").changed() {
                        if audio_enabled {
                            match TunnelAudio::start() {
                                Ok(audio) => self.audio = Some(audio),
                                Err(error) => self.status = error,
                            }
                        } else {
                            self.audio = None;
                        }
                    }
                    if audio_enabled {
                        ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.0).text("Громкость"));
                    }
                });
                ui.add_space(3.0);
                ui.horizontal_wrapped(|ui| {
                    for (i, label) in [
                        "3D-труба",
                        "Профиль",
                        "Графики",
                        "Полёт",
                        "Файлы",
                        "О модели",
                        "Параметры",
                        "Объёмная CFD",
                    ]
                    .iter()
                    .enumerate()
                    {
                        if i != 6 || compact {
                            ui.selectable_value(&mut self.page, i, *label);
                        }
                    }
                });
            });
        self.regions.push(("header", header.response.rect));
        let footer = egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            if self.importing {
                ui.spinner();
            }
            ui.label(RichText::new(&self.status).small().color(MUTED));
        });
        self.regions.push(("footer", footer.response.rect));
        if !compact && self.page != 7 {
            let sidebar = egui::SidePanel::left("parameters")
                .resizable(false)
                .exact_width(294.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("control-scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add_space(8.0);
                            controls::show(ui, &mut self.state, &mut self.control_tab);
                        });
                });
            self.regions.push(("sidebar", sidebar.response.rect));
            if self.page == 6 {
                self.page = 0;
            }
        }
        self.recalculate();
        let central = egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(11, 18, 28))
                    .inner_margin(16),
            )
            .show(ctx, |ui| {
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt(("page-scroll", self.page))
                    .auto_shrink([false, false]);
                if self
                    .capture
                    .as_ref()
                    .is_some_and(|capture| capture.scroll_end)
                {
                    scroll = scroll.stick_to_bottom(true);
                }
                scroll.show(ui, |ui| self.active_page(ui));
            });
        self.regions.push(("central", central.response.rect));
        self.recalculate();
    }
}
impl eframe::App for Workbench {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames += 1;
        if self.frames == 1 {
            ctx.set_theme(egui::Theme::Dark);
            style(ctx);
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            if let Some(path) = file.path {
                self.load(path);
            }
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O)) {
            self.import_dialog();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
            && self.page != 7
        {
            self.export("json");
        }
        self.poll_import();
        self.volume_flow.poll(ctx);
        self.draw(ctx);
        if self.tunnel.busy || self.importing {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        if let Some(message) = &self.import_message {
            let mut dismiss = false;
            egui::Window::new(if message.is_ok() {
                "Импорт завершён"
            } else {
                "Ошибка импорта / расчёта"
            })
            .collapsible(false)
            .resizable(true)
            .default_width(500.0)
            .show(ctx, |ui| {
                match message {
                    Ok(text) => {
                        ui.label(text);
                    }
                    Err(text) => {
                        ui.colored_label(AMBER, text);
                    }
                }
                dismiss = ui.button("Понятно").clicked();
            });
            if dismiss {
                self.import_message = None;
            }
        }
        if let Some(audio) = &self.audio {
            audio.update(
                self.state.velocity,
                self.analysis.current.stall_fraction,
                self.volume,
            );
        }
        if self.view.animate && self.page == 0 {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
        if let Some(capture) = &self.capture {
            ctx.request_repaint_after(Duration::from_millis(20));
            if self.frames >= 8
                && !self.tunnel.busy
                && !self.importing
                && !self.volume_flow.pending()
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
            let screenshot = ctx.input(|i| {
                i.events.iter().find_map(|event| {
                    if let egui::Event::Screenshot { image, .. } = event {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(screenshot) = screenshot {
                let pixels: Vec<u8> = screenshot
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_array())
                    .collect();
                if let Some(parent) = capture.path.parent() {
                    std::fs::create_dir_all(parent).expect("create capture directory");
                }
                image::save_buffer(
                    &capture.path,
                    &pixels,
                    screenshot.width() as u32,
                    screenshot.height() as u32,
                    image::ColorType::Rgba8,
                )
                .expect("save screenshot");
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
    fn persist_egui_memory(&self) -> bool {
        self.capture.is_none()
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.capture.is_none()
            && let Ok(text) = interchange::save_project(&self.state)
        {
            storage.set_string("simulation-v3", text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_import_reports_success_and_failure_without_losing_the_model() {
        let dir = std::env::temp_dir().join(format!(
            "glenn-ui-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = Workbench::with_state(Simulation::default());
        let foil = glenn_extended::geometry::Airfoil::generate(&app.state);
        let mut dxf = "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n70\n1\n".to_string();
        for p in foil.contour() {
            dxf.push_str(&format!("10\n{}\n20\n{}\n", p.x, p.y));
        }
        dxf.push_str("0\nENDSEC\n0\nEOF\n");
        let valid = dir.join("профиль с пробелами.DXF");
        std::fs::write(&valid, dxf).unwrap();
        let wait = |app: &mut Workbench| {
            let until = std::time::Instant::now() + Duration::from_secs(10);
            while app.importing {
                app.poll_import();
                assert!(std::time::Instant::now() < until, "Импорт завис");
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        app.load(valid);
        wait(&mut app);
        assert_eq!(app.page, 1);
        assert!(app.state.custom_upper.is_some());
        assert!(app.import_message.as_ref().unwrap().is_ok());
        let saved = app.state.clone();
        let broken = dir.join("сломанный.dxf");
        std::fs::write(&broken, "not a DXF file").unwrap();
        app.load(broken);
        wait(&mut app);
        assert!(app.import_message.as_ref().unwrap().is_err());
        assert!(app.status.contains("Ошибка импорта"));
        assert_eq!(app.state, saved);
        app.load(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cube.stl"));
        wait(&mut app);
        assert_eq!(app.page, 7);
        assert!(app.import_message.as_ref().unwrap().is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn panels_fit_without_intersection_at_supported_sizes() {
        for (width, height) in [
            (640.0, 480.0),
            (1280.0 / 1.5, 720.0 / 1.5),
            (800.0, 600.0),
            (1280.0, 720.0),
            (1440.0, 900.0),
            (1920.0, 1080.0),
        ] {
            let ctx = egui::Context::default();
            style(&ctx);
            let mut app = Workbench::with_state(Simulation::default());
            for scenario in 0..12 {
                let page = if scenario == 11 {
                    7
                } else if scenario == 10 {
                    0
                } else {
                    scenario.min(6)
                };
                app.view.probe = scenario == 10;
                app.page = page;
                app.control_tab = scenario.saturating_sub(6).min(3);
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(width, height),
                    )),
                    ..Default::default()
                };
                let _ = ctx.run(input, |ctx| app.draw(ctx));
                // Pixel rounding at fractional DPI can move an edge by less
                // than one logical pixel; larger overflow is a layout failure.
                const EDGE_TOLERANCE: f32 = 1.0;
                for (name, rect) in &app.regions {
                    assert!(
                        rect.width() > 0.0 && rect.height() > 0.0,
                        "{name}: {width}x{height}"
                    );
                    assert!(
                        rect.min.x >= -EDGE_TOLERANCE
                            && rect.min.y >= -EDGE_TOLERANCE
                            && rect.max.x <= width + EDGE_TOLERANCE
                            && rect.max.y <= height + EDGE_TOLERANCE,
                        "{name} {rect:?}, page {page}, {width}x{height}"
                    );
                }
                for i in 0..app.regions.len() {
                    for j in i + 1..app.regions.len() {
                        let intersection = app.regions[i].1.intersect(app.regions[j].1);
                        assert!(
                            intersection.width() <= 0.1 || intersection.height() <= 0.1,
                            "{} overlaps {}",
                            app.regions[i].0,
                            app.regions[j].0
                        );
                    }
                }
            }
        }
    }
}
