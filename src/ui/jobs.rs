//! Adapter between the desktop and the solver runtime. Neither engine nor
//! numerical crates depend on egui/wgpu. CPU scene assembly also stays off UI.
use super::{render::Scene, tunnel::View};
use glenn_extended::{
    engine::Worker,
    model::Simulation,
    physics::{
        aerodynamics::Analysis,
        flight::{self, Performance},
        flow::{self, FlowField, SectionGrid, Streamline},
    },
};
use std::sync::Arc;
#[derive(Clone)]
pub struct Request {
    pub state: Simulation,
    pub view: View,
    pub scene: bool,
}
impl Request {
    pub fn same(&self, other: &Self) -> bool {
        self.state == other.state
            && self.scene == other.scene
            && style(&self.view) == style(&other.view)
    }
}
fn style(v: &View) -> (bool, bool, bool, bool, usize, usize) {
    (
        v.streamlines,
        v.vortices,
        v.pressure,
        v.wireframe,
        v.palette,
        v.density,
    )
}
pub struct Snapshot {
    pub analysis: Analysis,
    pub performance: Performance,
    pub grid: Option<Arc<SectionGrid>>,
    pub scene: Option<Scene>,
}
pub fn worker(initial: Analysis) -> Worker<Request, Snapshot> {
    let mut analysis = initial;
    let mut grid: Option<Arc<SectionGrid>> = None;
    let mut paths: Vec<Streamline> = Vec::new();
    let mut key = None;
    Worker::new(move |request: Request, cancel| {
        cancel.check()?;
        analysis.update(&request.state)?;
        let performance = flight::calculate(&analysis);
        cancel.check()?;
        let scene = if request.scene {
            if grid.as_ref().is_none_or(|g| g.foil != analysis.foil) {
                grid = Some(Arc::new(
                    SectionGrid::new_cancellable(&analysis.foil, &analysis.panel, &|| {
                        cancel.is_cancelled()
                    })
                    .ok_or("Расчёт отменён")?,
                ));
                key = None;
            }
            cancel.check()?;
            let flow_key = (flow::field_fingerprint(&analysis), request.view.density);
            if key != Some(flow_key) {
                paths = FlowField {
                    analysis: &analysis,
                    grid: grid.as_ref().unwrap(),
                }
                .streamlines_cancellable(request.view.density, &|| cancel.is_cancelled())
                .ok_or("Расчёт отменён")?;
                key = Some(flow_key);
            }
            cancel.check()?;
            Some(Scene::new(&analysis, &paths, &request.view))
        } else {
            None
        };
        cancel.check()?;
        Ok(Snapshot {
            analysis: analysis.clone(),
            performance,
            grid: grid.clone(),
            scene,
        })
    })
}

pub enum Imported {
    Profile(Simulation, String),
    Project(Simulation, String),
    Solid(glenn_extended::mesh::SolidMesh, String),
}
pub fn importer() -> Worker<(std::path::PathBuf, Simulation), Imported> {
    Worker::new(|(path, state): (std::path::PathBuf, Simulation), cancel| {
        let name = path
            .file_name()
            .and_then(|p| p.to_str())
            .unwrap_or("file")
            .to_string();
        cancel.check()?;
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("stl"))
        {
            let mesh = glenn_extended::interchange::stl::read(&path)?;
            cancel.check()?;
            return Ok(Imported::Solid(mesh, name));
        }
        let text = glenn_extended::interchange::read_text(&path)?;
        cancel.check()?;
        if name.to_lowercase().ends_with(".json") {
            let (state, notice) = glenn_extended::interchange::load_project(&text)?;
            Analysis::new(state.clone())?;
            Ok(Imported::Project(
                state,
                notice.unwrap_or("Проект загружен".into()),
            ))
        } else {
            let state = glenn_extended::interchange::import_airfoil(&text, &name, &state)?;
            Analysis::new(state.clone())?;
            let count = state.custom_upper.as_ref().map_or(0, Vec::len)
                + state.custom_lower.as_ref().map_or(0, Vec::len);
            Ok(Imported::Profile(
                state,
                format!("Файл «{name}» загружен: {count} точек. Открыта вкладка профиля."),
            ))
        }
    })
}
