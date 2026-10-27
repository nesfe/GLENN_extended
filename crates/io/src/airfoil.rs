//! Bounded file import, versioned projects, and text exports without UI dependencies.
use crate::{
    geometry::WingMesh,
    model::{Point, Profile, Simulation},
    physics::aerodynamics::Analysis,
};
use serde::{Deserialize, Serialize};
use std::{fmt::Write as _, path::Path};
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
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
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("Максимальный размер файла — 16 МБ".into());
    }
    // Legacy CAD text strings may use ANSI code pages. Group codes and geometry
    // are ASCII; lossy decoding affects only names/comments, never numbers.
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dxf"))
    {
        Ok(String::from_utf8_lossy(&bytes)
            .trim_start_matches('\u{feff}')
            .to_string())
    } else {
        String::from_utf8(bytes).map_err(|e| e.to_string())
    }
}

/// Largest surface point count kept after import. A CAD export can hold tens
/// of thousands of vertices; the solver resamples to its own cosine stations,
/// so thinning a surface uniformly changes no result and keeps a dense file
/// inside the validated coordinate budget instead of rejecting it.
const MAX_SURFACE_POINTS: usize = 1024;

/// Parse Selig loops, Lednicer counted blocks, or an ASCII DXF drawing.
/// Surfaces are split by contour order, never by the sign of y (camber can
/// place both surfaces above zero), then normalised to unit chord.
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
    // The leading edge is the origin of the chord line, so both axes are
    // measured from it. Scaling y without moving it left a drawing placed away
    // from y = 0 hanging that far off its own chord, and a sheet far enough
    // from the origin was rejected outright as out-of-range coordinates.
    let leading_edge = upper
        .iter()
        .chain(&lower)
        .copied()
        .reduce(|a, b| if b.x < a.x { b } else { a })
        .ok_or("Профиль без координат")?;
    let max_x = upper
        .iter()
        .chain(&lower)
        .map(|p| p.x)
        .fold(f64::NEG_INFINITY, f64::max);
    let chord = max_x - leading_edge.x;
    if !chord.is_finite() || chord < 1e-9 {
        return Err("Профиль имеет нулевую хорду".into());
    }
    for surface in [&mut upper, &mut lower] {
        for p in surface.iter_mut() {
            p.x = (p.x - leading_edge.x) / chord;
            p.y = (p.y - leading_edge.y) / chord;
        }
        surface.sort_by(|a, b| a.x.total_cmp(&b.x));
        surface.dedup_by(|a, b| (a.x - b.x).abs() < 1e-9);
        if surface.len() > MAX_SURFACE_POINTS {
            let last = surface.len() - 1;
            *surface = (0..MAX_SURFACE_POINTS)
                .map(|i| surface[i * last / (MAX_SURFACE_POINTS - 1)])
                .collect();
        }
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
    // A Lednicer file opens with its two surface point counts, then runs each
    // surface from the leading edge. Read the first line as that header only
    // when it agrees with the rest of the file: a Selig loop may legitimately
    // start at two whole numbers — a millimetre drawing routinely does — and
    // must fall through to the contour split instead of being rejected.
    let count = points[0];
    if count.x >= 6.0
        && count.y >= 6.0
        && count.x.fract() == 0.0
        && count.y.fract() == 0.0
        && count.x + count.y == (points.len() - 1) as f64
        && points[1..].iter().all(|p| p.x >= points[1].x - 1e-9)
    {
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
    // A contour may start anywhere and run either way. Normalise it to begin
    // at the trailing edge and to carry that point at both ends before
    // splitting: otherwise whichever surface happened to hold the closing
    // point spanned the whole chord while the other stopped one point short,
    // leaving its chord-end values to a clamped extrapolation.
    let mut ordered = points.to_vec();
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
/// Group code / value pairs. A DXF file is a flat stream of them. Blank lines
/// appear around sections and at end of file in real exports: a group code is
/// never empty, while a text value legitimately can be, so padding is skipped
/// only where a code is expected.
fn dxf_pairs(text: &str) -> Result<Vec<(i32, &str)>, String> {
    if text.trim_start().starts_with("AutoCAD Binary DXF") {
        return Err("DXF в двоичном формате: сохраните как ASCII DXF".into());
    }
    let mut pairs = Vec::new();
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    while let Some(code) = lines.next() {
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        let code: i32 = code.parse().map_err(|_| {
            format!(
                "DXF: вместо группового кода встречено «{}»",
                code.chars().take(24).collect::<String>()
            )
        })?;
        let value = lines.next().ok_or("DXF: групповой код без значения")?;
        pairs.push((code, value.trim()));
    }
    Ok(pairs)
}

/// One contour-bearing entity while its group codes are being read.
#[derive(Default)]
struct Outline {
    kind: String,
    /// Group 10/20: polyline vertices, or spline control points.
    points: Vec<Point>,
    /// Group 11/21: points a spline passes through.
    fit: Vec<Point>,
    /// Bulge of the segment leaving the given vertex index.
    bulges: Vec<(usize, f64)>,
    knots: Vec<f64>,
    weights: Vec<f64>,
    degree: usize,
    closed: bool,
    /// A POLYLINE carries an unused 10/20 of its own before its VERTEX list.
    vertices_started: bool,
}

/// Interior points of a DXF bulge arc. The bulge is the tangent of a quarter
/// of the included angle, negative when the arc runs clockwise from `a` to `b`.
fn arc_points(a: Point, b: Point, bulge: f64) -> Vec<Point> {
    let chord = (b.x - a.x).hypot(b.y - a.y);
    if bulge.abs() < 1e-12 || chord < 1e-12 {
        return Vec::new();
    }
    let theta = 4.0 * bulge.atan();
    let radius = chord / (2.0 * (theta * 0.5).sin());
    let (ux, uy) = ((b.x - a.x) / chord, (b.y - a.y) / chord);
    let apothem = radius * (theta * 0.5).cos();
    let center = Point::new(
        (a.x + b.x) * 0.5 - uy * apothem,
        (a.y + b.y) * 0.5 + ux * apothem,
    );
    let start = (a.y - center.y).atan2(a.x - center.x);
    let steps = (theta.abs() / 0.12).ceil().max(2.0) as usize;
    (1..steps)
        .map(|i| {
            let angle = start + theta * i as f64 / steps as f64;
            Point::new(
                center.x + radius.abs() * angle.cos(),
                center.y + radius.abs() * angle.sin(),
            )
        })
        .collect()
}

/// B-spline position at parameter `t` by de Boor's recurrence.
fn de_boor(degree: usize, knots: &[f64], control: &[Point], t: f64) -> Point {
    let span = (degree..control.len())
        .rev()
        .find(|&i| t >= knots[i])
        .unwrap_or(degree);
    let mut row: Vec<Point> = (0..=degree).map(|j| control[j + span - degree]).collect();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = j + span - degree;
            let width = knots[i + degree + 1 - r] - knots[i];
            let a = if width.abs() < 1e-12 {
                0.0
            } else {
                (t - knots[i]) / width
            };
            row[j] = Point::new(
                row[j - 1].x * (1.0 - a) + row[j].x * a,
                row[j - 1].y * (1.0 - a) + row[j].y * a,
            );
        }
    }
    row[degree]
}

impl Outline {
    /// Expand bulge arcs; straight segments keep only their own end points.
    fn tessellated(&self) -> Vec<Point> {
        let mut out = Vec::with_capacity(self.points.len());
        for (i, &start) in self.points.iter().enumerate() {
            out.push(start);
            let next = if i + 1 < self.points.len() {
                self.points[i + 1]
            } else if self.closed {
                self.points[0]
            } else {
                continue;
            };
            let bulge = self
                .bulges
                .iter()
                .find_map(|&(index, bulge)| (index == i).then_some(bulge))
                .unwrap_or(0.0);
            out.extend(arc_points(start, next, bulge));
        }
        out
    }

    /// Evaluate the curve defined by its control points, knots and weights.
    fn sampled_spline(&self) -> Result<Vec<Point>, String> {
        // Control points + knots define the curve. Fit points are not a
        // substitute for it (nor can unspecified endpoint tangents be guessed).
        if self.points.len() <= self.degree {
            return Err("DXF: SPLINE без полного набора контрольных точек и узлов. Сохраните сплайн с контрольными точками либо полилинией.".into());
        }
        let degree = self.degree;
        if !(1..=7).contains(&degree) {
            return Err("DXF: степень SPLINE должна быть 1…7".into());
        }
        let expected = self.points.len() + degree + 1;
        if self.knots.len() != expected || self.knots.windows(2).any(|k| k[0] > k[1]) {
            return Err("DXF: неверное число или порядок узлов SPLINE".into());
        }
        if !self.weights.is_empty()
            && (self.weights.len() != self.points.len() || self.weights.iter().any(|w| *w <= 0.0))
        {
            return Err("DXF: некорректные веса рационального SPLINE".into());
        }
        let (first, last) = (self.knots[degree], self.knots[self.points.len()]);
        if last <= first {
            return Err("DXF: нулевой интервал SPLINE".into());
        }
        let samples = (self.points.len() * 8).clamp(400, 8192);
        Ok((0..=samples)
            .map(|i| {
                let t = first + (last - first) * i as f64 / samples as f64;
                if self.weights.is_empty() {
                    de_boor(degree, &self.knots, &self.points, t)
                } else {
                    rational_de_boor(degree, &self.knots, &self.points, &self.weights, t)
                }
            })
            .collect())
    }

    /// The contour this entity contributes, or nothing if it carries none.
    fn contour(&self) -> Result<Option<Vec<Point>>, String> {
        let points = match self.kind.as_str() {
            "LWPOLYLINE" | "POLYLINE" => self.tessellated(),
            "SPLINE" => self.sampled_spline()?,
            "LINE" if !self.points.is_empty() && !self.fit.is_empty() => {
                vec![self.points[0], self.fit[0]]
            }
            _ => return Ok(None),
        };
        Ok((points.len() >= 2).then_some(points))
    }
}

/// Read every contour-bearing entity of the ENTITIES section in file order.
fn dxf_outlines(pairs: &[(i32, &str)]) -> Result<Vec<Outline>, String> {
    let mut out = Vec::new();
    let mut current = Outline::default();
    let mut section = "";
    let mut awaiting_section = false;
    let mut pending = None;
    let mut pending_fit = None;
    for &(code, value) in pairs {
        if code == 0 {
            // A VERTEX continues the POLYLINE that owns it.
            if value == "VERTEX" && current.kind == "POLYLINE" {
                current.vertices_started = true;
            } else {
                out.push(std::mem::take(&mut current));
                current.kind = value.to_string();
            }
            pending = None;
            pending_fit = None;
            awaiting_section = value == "SECTION";
            if value == "ENDSEC" {
                section = "";
            }
            continue;
        }
        if code == 2 && awaiting_section {
            section = value;
            awaiting_section = false;
            continue;
        }
        if section != "ENTITIES" {
            continue;
        }
        if matches!(
            current.kind.as_str(),
            "LINE" | "LWPOLYLINE" | "POLYLINE" | "SPLINE"
        ) && matches!(code, 10 | 20 | 11 | 21 | 40 | 41 | 42)
            && value.parse::<f64>().ok().is_none_or(|v| !v.is_finite())
        {
            return Err(format!(
                "DXF: неверное число в {} (код {code}): {value}",
                current.kind
            ));
        }
        let number = || value.parse::<f64>().ok().filter(|v| v.is_finite());
        match code {
            10 => pending = number(),
            20 => {
                if let (Some(x), Some(y)) = (pending.take(), number())
                    && (current.kind != "POLYLINE" || current.vertices_started)
                {
                    current.points.push(Point::new(x, y));
                }
            }
            11 => pending_fit = number(),
            21 => {
                if let (Some(x), Some(y)) = (pending_fit.take(), number()) {
                    current.fit.push(Point::new(x, y));
                }
            }
            42 => {
                if let Some(bulge) = number() {
                    current
                        .bulges
                        .push((current.points.len().saturating_sub(1), bulge));
                }
            }
            40 => {
                if let Some(knot) = number() {
                    current.knots.push(knot);
                }
            }
            41 => {
                if let Some(weight) = number() {
                    current.weights.push(weight);
                }
            }
            70 if !current.vertices_started => {
                current.closed = value.parse::<i32>().is_ok_and(|flags| flags & 1 != 0)
            }
            71 if current.kind == "SPLINE" => {
                current.degree = value
                    .parse::<usize>()
                    .map_err(|_| "DXF: неверная степень SPLINE")?
            }
            _ => {}
        }
    }
    out.push(current);
    Ok(out)
}

/// The profile contour of an ASCII DXF drawing.
///
/// A real CAD export is rarely a lone polyline: it also carries a chord line,
/// dimensions, text and a border. Those entities are simply not contours, so
/// they never compete. Among actual contours the profile is the widest, which
/// separates it from a small flap box or a leader.
fn dxf_points(text: &str) -> Result<Vec<Point>, String> {
    let pairs = dxf_pairs(text)?;
    let mut paths = Vec::new();
    let mut rejection = None;
    for outline in dxf_outlines(&pairs)? {
        match outline.contour() {
            Ok(Some(points)) => paths.push(points),
            Ok(None) => {}
            Err(error) => rejection = rejection.or(Some(error)),
        }
    }
    if paths.is_empty() {
        return Err(rejection.unwrap_or_else(|| "DXF: нет поддерживаемых контуров. Блоки INSERT нужно разложить в CAD; поддержаны LINE, POLYLINE, LWPOLYLINE и SPLINE с контрольными точками.".into()));
    }
    let extent = |points: &[Point]| {
        let x = points.iter().map(|p| p.x);
        x.clone().fold(f64::NEG_INFINITY, f64::max) - x.fold(f64::INFINITY, f64::min)
    };
    let size = paths
        .iter()
        .map(|p| extent(p))
        .fold(0.0, f64::max)
        .max(1e-12);
    let near = |a: Point, b: Point| (a.x - b.x).hypot(a.y - b.y) <= size * 1e-7;
    // CAD often stores upper/lower surfaces as separate splines, or exports
    // an unordered set of LINEs. Join only endpoints, never arbitrary points.
    let mut loops = Vec::new();
    while let Some(mut path) = paths.pop() {
        loop {
            if path.len() > 2 && near(path[0], *path.last().unwrap()) {
                break;
            }
            let end = *path.last().unwrap();
            let matches: Vec<_> = paths
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    if near(end, p[0]) {
                        Some((i, false))
                    } else if near(end, *p.last().unwrap()) {
                        Some((i, true))
                    } else {
                        None
                    }
                })
                .collect();
            if matches.len() != 1 {
                break;
            }
            let (index, reverse) = matches[0];
            let mut next = paths.swap_remove(index);
            if reverse {
                next.reverse();
            }
            path.extend(next.into_iter().skip(1));
        }
        // Require a plausible section rather than silently selecting a border.
        if path.len() >= 12
            && let Ok((a, b)) = split_loop(&path)
        {
            let width = extent(&path);
            let height = path.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max)
                - path.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
            if extent(&a) > 0.95 * width
                && extent(&b) > 0.95 * width
                && height > width * 1e-5
                && height < width * 0.6
            {
                loops.push(path);
            }
        }
    }
    loops.sort_by(|a, b| extent(b).total_cmp(&extent(a)));
    if loops.len() > 1 && extent(&loops[1]) > extent(&loops[0]) * 0.75 {
        return Err("DXF: несколько сопоставимых профилей. Экспортируйте нужный профиль отдельно, чтобы не выбрать чужую геометрию.".into());
    }
    loops.into_iter().next().ok_or_else(|| rejection.unwrap_or_else(|| "DXF: не удалось собрать профиль. Нужен плоский контур в XY, носок слева, хорда вдоль X; разомкнутые или разветвлённые цепочки исправьте в CAD.".into()))
}

/// Homogeneous de Boor preserves rational CAD geometry instead of discarding weights.
fn rational_de_boor(
    degree: usize,
    knots: &[f64],
    control: &[Point],
    weights: &[f64],
    t: f64,
) -> Point {
    let span = (degree..control.len())
        .rev()
        .find(|&i| t >= knots[i])
        .unwrap_or(degree);
    let mut row: Vec<[f64; 3]> = (0..=degree)
        .map(|j| {
            let i = j + span - degree;
            [
                control[i].x * weights[i],
                control[i].y * weights[i],
                weights[i],
            ]
        })
        .collect();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = j + span - degree;
            let width = knots[i + degree + 1 - r] - knots[i];
            let a = if width.abs() < 1e-12 {
                0.0
            } else {
                (t - knots[i]) / width
            };
            row[j] = std::array::from_fn(|k| (1.0 - a) * row[j - 1][k] + a * row[j][k]);
        }
    }
    Point::new(
        row[degree][0] / row[degree][2],
        row[degree][1] / row[degree][2],
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    fn squiggle() -> Vec<Point> {
        [
            (0.0, 0.0),
            (0.5, 2.0),
            (1.5, -1.0),
            (2.5, 1.5),
            (3.5, 0.0),
            (4.0, 2.5),
            (5.0, 1.0),
            (6.0, -0.5),
            (7.0, 1.0),
            (8.0, 0.0),
            (8.5, 1.5),
            (9.0, 0.5),
            (9.5, 2.0),
            (10.0, 0.0),
        ]
        .map(|(x, y)| Point::new(x, y))
        .to_vec()
    }
    fn clamped(count: usize, degree: usize) -> Vec<f64> {
        let interior = count - degree;
        (0..count + degree + 1)
            .map(|i| i.saturating_sub(degree).min(interior) as f64)
            .collect()
    }

    #[test]
    fn de_boor_matches_an_independent_basis_evaluation() {
        // Reference values come from a direct Cox–de Boor basis-function sum
        // over the same clamped uniform knot vector, not from this recurrence.
        let control = squiggle();
        let knots = clamped(control.len(), 3);
        for (t, x, y) in [
            (0.00, 0.0, 0.0),
            (0.50, 0.739583333333, 0.958333333333),
            (1.00, 1.416666666667, 0.166666666667),
            (2.75, 3.214843750000, 0.645833333333),
            (5.50, 5.5, 0.3125),
            (7.25, 7.248697916667, 0.580729166667),
            (9.00, 8.5, 1.083333333333),
            (10.30, 9.246041666667, 1.286541666667),
            (11.00, 10.0, 0.0),
        ] {
            let point = de_boor(3, &knots, &control, t);
            assert!(
                (point.x - x).abs() < 1e-9 && (point.y - y).abs() < 1e-9,
                "t {t}: got ({}, {}), expected ({x}, {y})",
                point.x,
                point.y
            );
        }
    }

    #[test]
    fn de_boor_is_clamped_and_degree_one_is_the_control_polygon() {
        let control = squiggle();
        for degree in 1..=5 {
            let knots = clamped(control.len(), degree);
            let start = de_boor(degree, &knots, &control, knots[degree]);
            let end = de_boor(degree, &knots, &control, knots[control.len()]);
            assert!((start.x - control[0].x).abs() < 1e-12);
            assert!((start.y - control[0].y).abs() < 1e-12);
            assert!((end.x - control[control.len() - 1].x).abs() < 1e-12);
            assert!((end.y - control[control.len() - 1].y).abs() < 1e-12);
        }
        // Degree one interpolates its control points: a knot lands on a vertex.
        let knots = clamped(control.len(), 1);
        for (i, expected) in control.iter().enumerate() {
            let point = de_boor(1, &knots, &control, knots[1] + i as f64);
            assert!((point.x - expected.x).abs() < 1e-12, "vertex {i}");
            assert!((point.y - expected.y).abs() < 1e-12, "vertex {i}");
        }
    }

    #[test]
    fn bulge_arcs_land_on_the_circle_the_dxf_bulge_defines() {
        let (a, b) = (Point::new(-1.0, 0.5), Point::new(2.0, 1.25));
        let chord = (b.x - a.x).hypot(b.y - a.y);
        for bulge in [0.25, 0.6, 1.0, -0.25, -1.0] {
            let points = arc_points(a, b, bulge);
            assert!(points.len() >= 2, "bulge {bulge}");
            let theta = 4.0 * bulge.atan();
            let radius = (chord / (2.0 * (theta * 0.5).sin())).abs();
            // Every interior point sits at the arc radius from one common
            // centre, recovered from the first point and the two ends.
            let centre = {
                let mid = Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
                let apothem = chord / (2.0 * (theta * 0.5).sin()) * (theta * 0.5).cos();
                Point::new(
                    mid.x - (b.y - a.y) / chord * apothem,
                    mid.y + (b.x - a.x) / chord * apothem,
                )
            };
            for end in [a, b] {
                assert!(
                    ((end.x - centre.x).hypot(end.y - centre.y) - radius).abs() < 1e-9,
                    "bulge {bulge}: end point off the circle"
                );
            }
            for point in &points {
                assert!(
                    ((point.x - centre.x).hypot(point.y - centre.y) - radius).abs() < 1e-9,
                    "bulge {bulge}: arc point off the circle"
                );
            }
            // A unit bulge is a semicircle, so its apex stands a radius off the
            // chord, on the side the sign selects.
            if bulge.abs() == 1.0 {
                assert!((radius - chord * 0.5).abs() < 1e-12);
                let apex = points[points.len() / 2];
                let side = (b.x - a.x) * (apex.y - a.y) - (b.y - a.y) * (apex.x - a.x);
                assert!(side * bulge < 0.0, "semicircle bulges to the wrong side");
            }
        }
        assert!(arc_points(a, b, 0.0).is_empty(), "straight segment");
    }

    #[test]
    fn bulge_zero_and_degenerate_spline_are_handled() {
        let single = vec![Point::new(0.0, 0.0), Point::new(1.0, 0.0)];
        let outline = Outline {
            kind: "SPLINE".into(),
            points: single,
            degree: 3,
            ..Default::default()
        };
        // Fewer control points than the degree cannot define a curve.
        assert!(outline.sampled_spline().is_err());
        assert!(outline.contour().is_err());
    }
}

#[cfg(test)]
mod rational_tests {
    use super::*;
    #[test]
    fn rational_quarter_circle_retains_cad_weights() {
        let control = [
            Point::new(1.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(0.0, 1.0),
        ];
        for i in 0..=100 {
            let p = rational_de_boor(
                2,
                &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                &control,
                &[1.0, 0.5_f64.sqrt(), 1.0],
                i as f64 / 100.0,
            );
            assert!((p.x * p.x + p.y * p.y - 1.0).abs() < 1e-12);
        }
    }
    #[test]
    fn malformed_knots_are_not_replaced_by_invented_geometry() {
        let outline = Outline {
            kind: "SPLINE".into(),
            points: vec![Point::new(0.0, 0.0); 4],
            degree: 3,
            knots: vec![0.0; 3],
            ..Default::default()
        };
        assert!(outline.sampled_spline().is_err());
    }
}
