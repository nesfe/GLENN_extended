//! Compare completed runs for one physical model and free-stream condition.
use super::plots::{AMBER, CYAN, plot};
use eframe::egui;
use glenn_extended::{
    cfd::{self, Config, FlowModel, Resolution, RunResult},
    engine::Worker,
    model::Point,
};
use std::path::PathBuf;

struct Request {
    reference: PathBuf,
    other: PathBuf,
    config: Config,
    time: f64,
}
struct Row {
    path: PathBuf,
    config: Config,
    force: glam::DVec3,
    converged: bool,
    wall_faces: usize,
}
pub struct Comparison {
    worker: Worker<Request, Row>,
    rows: Vec<Row>,
    busy: bool,
    error: Option<String>,
}
impl Default for Comparison {
    fn default() -> Self {
        Self {
            worker: Worker::new(|r: Request, cancel| {
                let geometry = "constant/triSurface/body.stl";
                let read = |p: PathBuf| std::fs::read(p).map_err(|e| e.to_string());
                if read(r.reference.join(geometry))? != read(r.other.join(geometry))? {
                    return Err("Геометрия или масштаб тела отличаются".into());
                }
                let cfg: Config = serde_json::from_slice(&read(r.other.join("glenn-case.json"))?)
                    .map_err(|e| e.to_string())?;
                if cfg.speed != r.config.speed
                    || cfg.density != r.config.density
                    || cfg.kinematic_viscosity != r.config.kinematic_viscosity
                    || cfg.model != r.config.model
                    || cfg.air_temperature != r.config.air_temperature
                {
                    return Err(
                        "Для сравнения нужны одинаковые среда, скорость и модель течения".into(),
                    );
                }
                cancel.check()?;
                let result = cfd::load_result(&r.other)?;
                if cfg.model == FlowModel::UnsteadyRans
                    && (result.field.time - r.time).abs() > 1e-8 * r.time.abs().max(1e-12)
                {
                    return Err("Моменты нестационарных полей различаются".into());
                }
                Ok(Row {
                    path: r.other,
                    config: cfg,
                    force: result.force,
                    converged: result.converged,
                    wall_faces: result.wall_faces,
                })
            }),
            rows: Vec::new(),
            busy: false,
            error: None,
        }
    }
}
impl Comparison {
    pub fn reset(&mut self) {
        self.worker.cancel();
        self.rows.clear();
        self.busy = false;
        self.error = None;
    }
    pub fn show(&mut self, ui: &mut egui::Ui, r: &RunResult, cfg: &Config, area: f64) {
        ui.separator();
        if let Some((_, result)) = self.worker.poll() {
            self.busy = false;
            match result {
                Ok(row) => self.rows.push(row),
                Err(e) => self.error = Some(e),
            }
        }
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new("Добавить расчёт для сравнения…"),
            )
            .clicked()
            && let Some(path) = rfd::FileDialog::new().pick_folder()
            && path != r.case_dir
            && !self.rows.iter().any(|row| row.path == path)
        {
            self.error = None;
            self.worker.submit(Request {
                reference: r.case_dir.clone(),
                other: path,
                config: cfg.clone(),
                time: r.field.time,
            });
            self.busy = true;
        }
        if self.busy {
            ui.spinner();
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(e) = &self.error {
            ui.colored_label(AMBER, e);
        }
        if self.rows.is_empty() {
            return;
        }
        let mut polar = Vec::new();
        let mut grid = Vec::new();
        egui::Grid::new("cfd-comparison")
            .striped(true)
            .show(ui, |ui| {
                for title in ["α, °", "Сетка", "Грани стенки", "CL", "CD", "Сходимость"]
                {
                    ui.label(title);
                }
                ui.end_row();
                let current = (cfg, r.force, r.converged, r.wall_faces);
                for (c, f, converged, faces) in std::iter::once(current).chain(
                    self.rows
                        .iter()
                        .map(|row| (&row.config, row.force, row.converged, row.wall_faces)),
                ) {
                    ui.label(format!("{:.2}", c.alpha_degrees));
                    ui.label(match c.resolution {
                        Resolution::Preview => "Preview",
                        Resolution::Standard => "Standard",
                        Resolution::Fine => "Fine",
                    });
                    ui.label(faces.to_string());
                    if let Some((cl, cd)) = c.coefficients(f, area) {
                        ui.label(format!("{cl:.5}"));
                        ui.label(format!("{cd:.5}"));
                        if c.resolution == cfg.resolution {
                            polar.push((c.alpha_degrees, Point::new(cd, cl)));
                        }
                        if c.alpha_degrees == cfg.alpha_degrees {
                            grid.push((faces as f64, cl, cd));
                        }
                    } else {
                        ui.label("—");
                        ui.label("—");
                    }
                    ui.label(if c.model == FlowModel::UnsteadyRans {
                        "Мгновенные"
                    } else if converged {
                        "Да"
                    } else {
                        "Не достигнута"
                    });
                    ui.end_row();
                }
            });
        if polar.len() > 1 {
            polar.sort_by(|a, b| a.0.total_cmp(&b.0));
            if polar.first().unwrap().0 != polar.last().unwrap().0 {
                let points: Vec<_> = polar.into_iter().map(|(_, p)| p).collect();
                plot(
                    ui,
                    "Поляра на выбранной сетке",
                    "CD",
                    "CL",
                    &[("CL(CD)", &points, CYAN)],
                    false,
                );
            }
        }
        if grid.len() > 1 {
            grid.sort_by(|a, b| a.0.total_cmp(&b.0));
            let cl: Vec<_> = grid.iter().map(|&(x, y, _)| Point::new(x, y)).collect();
            let cd: Vec<_> = grid.iter().map(|&(x, _, y)| Point::new(x, y)).collect();
            plot(
                ui,
                "Чувствительность сил к сетке при выбранном угле",
                "Грани стенки",
                "CL, CD",
                &[("CL", &cl, CYAN), ("CD", &cd, AMBER)],
                false,
            );
        }
    }
}
