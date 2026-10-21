//! Bounded file import, versioned projects, and text exports without UI dependencies.
use crate::{
    geometry::WingMesh,
    model::{Point, Profile, Simulation},
    physics::aerodynamics::Analysis,
};
use serde::{Deserialize, Serialize};
use std::{fmt::Write as _, path::Path};
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
struct Project {
    format_version: u32,
    simulation: Simulation,
}
pub fn save_project(state: &Simulation) -> Result<String, String> {
    state.validate()?;
    serde_json::to_string_pretty(&Project {
        format_version: 3,
        simulation: state.clone(),
    })
    .map_err(|e| e.to_string())
}
pub fn load_project(text: &str) -> Result<(Simulation, Option<String>), String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let (state, notice) = if value.get("format_version").is_some() {
        let project: Project = serde_json::from_value(value).map_err(|e| e.to_string())?;
        if project.format_version != 3 {
            return Err("Неподдерживаемая версия проекта".into());
        }
        (project.simulation, None)
    } else {
        let mut state: Simulation = serde_json::from_value(value).map_err(|e| e.to_string())?;
        // v2 exposed kg/(kgf h) values while its formula treated them as kg/(N s).
        let mut notes = Vec::new();
        if state.tsfc > 0.01 {
            state.tsfc /= 9.80665 * 3600.0;
            notes.push("TSFC переведён из кг/(кгс·ч) в кг/(Н·с)");
        }
        if state.fuel_mass >= state.aircraft_mass {
            state.fuel_mass = state.aircraft_mass * 0.13;
            notes.push("исправлена масса топлива старого пресета");
        }
        (
            state,
            Some(format!(
                "Импорт v2: {}",
                if notes.is_empty() {
                    "параметры восстановлены".into()
                } else {
                    notes.join("; ")
                }
            )),
        )
    };
    state.validate()?;
    Ok((state, notice))
}
pub fn read_text(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut text = String::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err("Максимальный размер файла — 2 МБ".into());
    }
    Ok(text)
}

/// Parse Selig loops, Lednicer counted blocks, or one ASCII DXF polyline.
/// Surfaces are split by contour order, never by the sign of y (camber can
/// place both surfaces above zero). Each surface is resampled independently.
pub fn import_airfoil(
    text: &str,
    filename: &str,
    state: &Simulation,
) -> Result<Simulation, String> {
    let dxf = filename.to_lowercase().ends_with(".dxf");
    let (mut upper, mut lower) = if dxf {
        split_loop(&dxf_points(text)?)?
    } else {
        dat_surfaces(text)?
    };
    let min_x = upper
        .iter()
        .chain(&lower)
        .map(|p| p.x)
        .fold(f64::INFINITY, f64::min);
    let max_x = upper
        .iter()
        .chain(&lower)
        .map(|p| p.x)
        .fold(f64::NEG_INFINITY, f64::max);
    let chord = max_x - min_x;
    if !chord.is_finite() || chord < 1e-9 {
        return Err("Профиль имеет нулевую хорду".into());
    }
    for surface in [&mut upper, &mut lower] {
        for p in surface.iter_mut() {
            p.x = (p.x - min_x) / chord;
            p.y /= chord;
        }
        surface.sort_by(|a, b| a.x.total_cmp(&b.x));
        surface.dedup_by(|a, b| (a.x - b.x).abs() < 1e-9);
    }
    let mean = |points: &[Point]| points.iter().map(|p| p.y).sum::<f64>() / points.len() as f64;
    if mean(&upper) < mean(&lower) {
        std::mem::swap(&mut upper, &mut lower);
    }
    let mut out = state.clone();
    out.profile_type = Profile::Custom;
    out.profile_name = Path::new(filename)
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("Custom")
        .chars()
        .take(80)
        .collect();
    out.custom_upper = Some(upper);
    out.custom_lower = Some(lower);
    out.validate()?;
    Ok(out)
}
fn numeric_pair(line: &str) -> Option<Point> {
    let mut parts = line.split_whitespace();
    let x = parts.next()?.parse::<f64>().ok()?;
    let y = parts.next()?.parse::<f64>().ok()?;
    Some(Point::new(x, y))
}
fn dat_surfaces(text: &str) -> Result<(Vec<Point>, Vec<Point>), String> {
    let mut points: Vec<Point> = text.lines().filter_map(numeric_pair).collect();
    if points.len() < 12
        || points.len() > 8192
        || points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite())
    {
        return Err("DAT: нужны 12…8192 конечных координат".into());
    }
    let count = points[0];
    if count.x >= 6.0 && count.y >= 6.0 && count.x.fract() == 0.0 && count.y.fract() == 0.0 {
        if count.x + count.y != (points.len() - 1) as f64 {
            return Err("Lednicer: число координат не совпадает с заголовком".into());
        }
        points.remove(0);
        let lower = points.split_off(count.x as usize);
        return Ok((points, lower));
    }
    split_loop(&points)
}
fn split_loop(points: &[Point]) -> Result<(Vec<Point>, Vec<Point>), String> {
    if points.len() < 12 {
        return Err("Недостаточно координат профиля".into());
    }
    let leading = points
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.x.total_cmp(&b.x))
        .unwrap()
        .0;
    // Closed CAD polylines may start anywhere: rotate the contour to the TE first.
    let trailing = points
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.x.total_cmp(&b.x))
        .unwrap()
        .0;
    let mut ordered = points.to_vec();
    if leading == 0 || leading == points.len() - 1 || trailing > 1 && trailing < points.len() - 2 {
        if ordered.first() == ordered.last() {
            ordered.pop();
        }
        let trailing = ordered
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.x.total_cmp(&b.x))
            .unwrap()
            .0;
        ordered.rotate_left(trailing);
        ordered.push(ordered[0]);
    }
    let leading = ordered
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.x.total_cmp(&b.x))
        .unwrap()
        .0;
    if leading < 5 || ordered.len() - leading < 6 {
        return Err("Не удалось выделить две поверхности; используйте Selig или Lednicer".into());
    }
    Ok((ordered[..=leading].to_vec(), ordered[leading..].to_vec()))
}
fn dxf_points(text: &str) -> Result<Vec<Point>, String> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    if !lines.len().is_multiple_of(2) {
        return Err("DXF: нарушены пары код/значение".into());
    }
    let mut points = Vec::new();
    let mut in_entities = false;
    let mut entity = "";
    let mut x = None;
    let mut polylines = 0;
    for pair in lines.chunks_exact(2) {
        let code = pair[0];
        let value = pair[1];
        if code == "2" && value == "ENTITIES" {
            in_entities = true;
            continue;
        }
        if code == "0" && value == "ENDSEC" {
            in_entities = false;
        }
        if !in_entities {
            continue;
        }
        if code == "0" {
            entity = value;
            x = None;
            if matches!(value, "LWPOLYLINE" | "POLYLINE") {
                polylines += 1;
            }
            if matches!(value, "SPLINE" | "ARC" | "LINE" | "CIRCLE") {
                return Err("DXF: экспортируйте профиль одной POLYLINE/LWPOLYLINE без дуг".into());
            }
        }
        if !matches!(entity, "LWPOLYLINE" | "VERTEX") {
            continue;
        }
        if code == "42" && value.parse::<f64>().unwrap_or(1.0).abs() > 1e-12 {
            return Err("DXF: дуги bulge нужно преобразовать в отрезки".into());
        }
        if code == "10" {
            x = Some(
                value
                    .parse::<f64>()
                    .map_err(|_| "DXF: неверная координата X")?,
            );
        }
        if code == "20" {
            let px = x.take().ok_or("DXF: Y без X")?;
            let py = value
                .parse::<f64>()
                .map_err(|_| "DXF: неверная координата Y")?;
            if !px.is_finite() || !py.is_finite() {
                return Err("DXF: неконечные координаты".into());
            }
            points.push(Point::new(px, py));
        }
    }
    if polylines != 1 {
        return Err("DXF должен содержать ровно одну полилинию профиля".into());
    }
    Ok(points)
}
pub fn export_dat(analysis: &Analysis) -> String {
    let mut out = format!(
        "{}\n",
        analysis.state.profile_name.replace(['\n', '\r'], " ")
    );
    for p in analysis
        .foil
        .upper
        .iter()
        .rev()
        .chain(analysis.foil.lower.iter().skip(1))
    {
        writeln!(out, "{:.8} {:.8}", p.x, p.y).unwrap();
    }
    out
}
pub fn export_csv(analysis: &Analysis) -> String {
    let mut out = String::from(
        "alpha_deg,CL,CD,CD_profile,CD_induced,CD_wave,CD_stall,L_D,Cm,Lift_N,Drag_N,Re,Mach\n",
    );
    for (alpha, r) in &analysis.polar {
        writeln!(
            out,
            "{alpha},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6}",
            r.cl,
            r.cd,
            r.cd_profile,
            r.cd_induced,
            r.cd_wave,
            r.cd_stall,
            r.lift_drag_ratio(),
            r.cm,
            r.lift,
            r.drag,
            r.reynolds,
            r.mach
        )
        .unwrap();
    }
    out
}
pub fn export_obj(mesh: &WingMesh) -> String {
    let mut out = String::from("# GLENN Extended, metres\n");
    for [x, y, z] in &mesh.vertices {
        writeln!(out, "v {x:.8} {y:.8} {z:.8}").unwrap();
    }
    for [a, b, c] in &mesh.triangles {
        writeln!(out, "f {} {} {}", a + 1, b + 1, c + 1).unwrap();
    }
    out
}
pub fn export_stl(mesh: &WingMesh) -> String {
    let mut out = String::from("solid GLENN_metres\n");
    for triangle in &mesh.triangles {
        let [a, b, c] = triangle.map(|i| mesh.vertices[i]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-20);
        writeln!(
            out,
            "facet normal {} {} {}\nouter loop",
            n[0] / length,
            n[1] / length,
            n[2] / length
        )
        .unwrap();
        for [x, y, z] in [a, b, c] {
            writeln!(out, "vertex {x:.8} {y:.8} {z:.8}").unwrap();
        }
        out.push_str("endloop\nendfacet\n");
    }
    out.push_str("endsolid GLENN_metres\n");
    out
}
