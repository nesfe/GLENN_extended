//! Desktop composition. Panels own their space; no floating tool windows can
//! become unreachable. Narrow windows put controls on a dedicated page.
mod controls;
mod pages;
mod plots;
mod tunnel;
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
    view: tunnel::View,
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
        let starting_state = capture
            .as_ref()
            .and_then(|capture| {
                model::presets()
                    .get(capture.preset)
                    .map(|preset| preset.state.clone())
            })
            .or(saved)
            .unwrap_or_default();
        let mut app = Self::with_state(starting_state);
        if let Some(capture) = &capture {
            app.page = capture.page.min(6);
            app.control_tab = capture.control_tab.min(3);
            app.preset = capture.preset.min(app.presets.len() - 1);
            app.view.animate = false;
            creation.egui_ctx.set_zoom_factor(capture.scale);
        }
        app.capture = capture;
        app
    }
    fn with_state(state: Simulation) -> Self {
        let (analysis, status) = match Analysis::new(state) {
            Ok(analysis) => (
                analysis,
                "Готово. Перетащите DAT, DXF или JSON в окно.".into(),
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
        Self {
            state,
            analysis,
            performance,
            view: tunnel::View::default(),
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
        let result: Result<String, String> = (|| {
            let text = interchange::read_text(&path)?;
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("profile.dat");
            if name.to_lowercase().ends_with(".json") {
                let (state, notice) = interchange::load_project(&text)?;
                self.state = state;
                Ok(notice.unwrap_or_else(|| "Проект загружен".into()))
            } else {
                self.state = interchange::import_airfoil(&text, name, &self.state)?;
                Ok("Профиль загружен".into())
            }
        })();
        self.status = match result {
            Ok(message) => message,
            Err(error) => format!("Ошибка импорта: {error}"),
        };
    }
    fn import_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Профиль или проект", &["dat", "dxf", "json"])
            .pick_file()
        {
            self.load(path);
        }
    }
    fn export(&mut self, extension: &str) {
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
        if self.analysis.state == self.state {
            return;
        }
        match self.analysis.update(&self.state) {
            Ok(()) => self.performance = flight::calculate(&self.analysis),
            Err(error) => {
                self.status = format!("Расчёт остановлен: {error}");
                self.state = self.analysis.state.clone();
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
                    ui.label(RichText::new("EXTENDED  /  3.0").size(12.0).color(MUTED));
                    ui.separator();
                    ui.label("Аэродинамическая лаборатория");
                    if ui.button("Открыть…").clicked() {
                        self.import_dialog();
                    }
                    if ui.button("Сохранить проект").clicked() {
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
                    ]
                    .iter()
                    .enumerate()
                    {
                        if i < 6 || compact {
                            ui.selectable_value(&mut self.page, i, *label);
                        }
                    }
                });
            });
        self.regions.push(("header", header.response.rect));
        let footer = egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(RichText::new(&self.status).small().color(MUTED));
        });
        self.regions.push(("footer", footer.response.rect));
        if !compact {
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
                egui::ScrollArea::vertical()
                    .id_salt(("page-scroll", self.page))
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.active_page(ui));
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
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.export("json");
        }
        self.draw(ctx);
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
            if self.frames == 8 {
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
            for scenario in 0..10 {
                let page = scenario.min(6);
                app.page = page;
                app.control_tab = scenario.saturating_sub(6);
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(width, height),
                    )),
                    ..Default::default()
                };
                let _ = ctx.run(input, |ctx| app.draw(ctx));
                for (name, rect) in &app.regions {
                    assert!(
                        rect.width() > 0.0 && rect.height() > 0.0,
                        "{name}: {width}x{height}"
                    );
                    assert!(
                        rect.min.x >= -0.1
                            && rect.min.y >= -0.1
                            && rect.max.x <= width + 0.1
                            && rect.max.y <= height + 0.1,
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
