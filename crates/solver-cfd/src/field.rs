use glam::DVec3;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};
#[derive(Clone, Debug)]
pub struct VolumeField {
    pub dims: [usize; 3],
    pub min: DVec3,
    pub max: DVec3,
    pub velocity: Vec<Option<DVec3>>,
    /// Gauge pressure in Pa (OpenFOAM's incompressible p is multiplied by rho).
    pub pressure: Vec<Option<f64>>,
    pub vorticity: Vec<Option<DVec3>>,
    pub time: f64,
}
impl VolumeField {
    /// Index and speed of the fastest sample opposing the unit inlet direction.
    pub fn strongest_backflow(&self, wind: DVec3) -> Option<(usize, f64)> {
        self.velocity
            .iter()
            .enumerate()
            .filter_map(|(index, sample)| {
                let reverse = -sample.as_ref()?.dot(wind);
                (reverse > 0.0).then_some((index, reverse))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }

    pub fn position(&self, i: usize) -> DVec3 {
        let x = i % self.dims[0];
        let y = i / self.dims[0] % self.dims[1];
        let z = i / (self.dims[0] * self.dims[1]);
        self.min
            + (self.max - self.min)
                * DVec3::new(
                    x as f64 / (self.dims[0] - 1) as f64,
                    y as f64 / (self.dims[1] - 1) as f64,
                    z as f64 / (self.dims[2] - 1) as f64,
                )
    }
    fn index_at(&self, p: DVec3) -> Option<usize> {
        if !p.is_finite() {
            return None;
        }
        let v = (p - self.min) / (self.max - self.min)
            * DVec3::new(
                (self.dims[0] - 1) as f64,
                (self.dims[1] - 1) as f64,
                (self.dims[2] - 1) as f64,
            );
        let i = v.round();
        if (v - i).abs().max_element() > 1e-3
            || i.min_element() < 0.0
            || i.x >= self.dims[0] as f64
            || i.y >= self.dims[1] as f64
            || i.z >= self.dims[2] as f64
        {
            return None;
        }
        Some((i.z as usize * self.dims[1] + i.y as usize) * self.dims[0] + i.x as usize)
    }
    /// No extrapolation through a solid: all eight fluid samples must exist.
    pub fn sample_velocity(&self, p: DVec3) -> Option<DVec3> {
        if !p.is_finite() || p.cmplt(self.min).any() || p.cmpge(self.max).any() {
            return None;
        }
        let v = (p - self.min) / (self.max - self.min)
            * DVec3::new(
                (self.dims[0] - 1) as f64,
                (self.dims[1] - 1) as f64,
                (self.dims[2] - 1) as f64,
            );
        let b = v.floor().as_uvec3();
        let f = v - v.floor();
        let mut out = DVec3::ZERO;
        for dz in 0..2 {
            for dy in 0..2 {
                for dx in 0..2 {
                    let i = (((b.z as usize + dz) * self.dims[1] + b.y as usize + dy)
                        * self.dims[0])
                        + b.x as usize
                        + dx;
                    let w = if dx == 0 { 1.0 - f.x } else { f.x }
                        * if dy == 0 { 1.0 - f.y } else { f.y }
                        * if dz == 0 { 1.0 - f.z } else { f.z };
                    out += self.velocity.get(i).copied().flatten()? * w;
                }
            }
        }
        Some(out)
    }
    /// Instantaneous streamlines, not particle trajectories through time.
    pub fn streamlines(&self) -> Vec<Vec<DVec3>> {
        self.streamlines_around(None)
    }
    pub fn streamlines_around(&self, body: Option<(DVec3, DVec3)>) -> Vec<Vec<DVec3>> {
        self.streamlines_in_wind(body, DVec3::X)
    }
    pub(crate) fn streamlines_in_wind(
        &self,
        body: Option<(DVec3, DVec3)>,
        wind: DVec3,
    ) -> Vec<Vec<DVec3>> {
        let step = (self.max - self.min).min_element() / 180.0;
        let mut seeds = Vec::new();
        let (lo, hi) = body.unwrap_or((self.min, self.max));
        let center = 0.5 * (lo + hi);
        let extent = hi - lo;
        let across = DVec3::Z.cross(wind);
        let thickness = extent.dot(across.abs());
        let padding = (thickness * 0.25).max(extent.dot(wind.abs()) * 0.08);
        let inside_min = self.min + DVec3::splat(step);
        let inside_max = self.max - DVec3::splat(step);
        for z in 0..7 {
            for lane in 0..7 {
                let offset = (thickness + 2.0 * padding) * (lane as f64 / 6.0 - 0.5);
                let mut target = center + offset * across;
                target.z =
                    (lo.z + extent.z * (z as f64 + 0.5) / 7.0).clamp(inside_min.z, inside_max.z);
                if let Some(seed) = upstream_entry(target, wind, inside_min, inside_max) {
                    seeds.push(seed);
                }
            }
        }
        // Inlet-only seeds cannot enter a closed recirculation bubble. Seed
        // additional lines at computed backflow samples, without prescribing
        // their trajectory or adding any velocity to the solved field.
        let mut wake_seeds = Vec::new();
        let max_speed = self
            .velocity
            .iter()
            .flatten()
            .map(|v| v.length_squared())
            .fold(0.0, f64::max)
            .sqrt();
        for (i, v) in self.velocity.iter().enumerate() {
            if v.is_some_and(|v| v.dot(wind) < -0.03 * max_speed) {
                let p = self.position(i);
                if wake_seeds
                    .iter()
                    .all(|s: &DVec3| s.distance(p) > step * 18.0)
                {
                    wake_seeds.push(p);
                }
                if wake_seeds.len() >= 32 {
                    break;
                }
            }
        }
        seeds.extend(wake_seeds);
        let mut lines = Vec::new();
        for mut p in seeds {
            let mut path = Vec::new();
            for _ in 0..900 {
                let Some(v) = self.sample_velocity(p) else {
                    break;
                };
                if v.length() < 1e-8 {
                    break;
                }
                path.push(p);
                let dt = step / v.length();
                let Some(mid) = self.sample_velocity(p + 0.5 * dt * v) else {
                    break;
                };
                p += dt * mid;
            }
            if path.len() > 2 {
                lines.push(path);
            }
        }
        lines
    }
}
/// First intersection of a wind-aligned line with the sampled fluid box.
fn upstream_entry(target: DVec3, wind: DVec3, min: DVec3, max: DVec3) -> Option<DVec3> {
    let mut entry = f64::NEG_INFINITY;
    let mut exit = f64::INFINITY;
    for axis in 0..2 {
        if wind[axis].abs() < 1e-12 {
            if !(min[axis]..=max[axis]).contains(&target[axis]) {
                return None;
            }
        } else {
            let a = (min[axis] - target[axis]) / wind[axis];
            let b = (max[axis] - target[axis]) / wind[axis];
            entry = entry.max(a.min(b));
            exit = exit.min(a.max(b));
        }
    }
    (entry <= exit).then(|| target + entry * wind)
}

fn time_directories(dir: &Path) -> Result<Vec<(f64, PathBuf)>, String> {
    let mut times: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let t = e.file_name().to_str()?.parse::<f64>().ok()?;
            (t.is_finite() && e.path().is_dir()).then_some((t, e.path()))
        })
        .collect();
    times.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(times)
}

fn latest(dir: &Path) -> Result<(f64, PathBuf), String> {
    time_directories(dir)?
        .pop()
        .ok_or("Нет результатов выборки поля OpenFOAM".into())
}
pub fn read_field(case: &Path, density: f64) -> Result<VolumeField, String> {
    let (dims, min, max): ([usize; 3], [f64; 3], [f64; 3]) = serde_json::from_str(
        &fs::read_to_string(case.join("sample-grid.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if dims.iter().any(|n| !(2..=512).contains(n))
        || dims.iter().product::<usize>() > 2_000_000
        || !density.is_finite()
        || density <= 0.0
        || min.iter().chain(&max).any(|v| !v.is_finite())
        || (0..3).any(|i| max[i] <= min[i])
    {
        return Err("Некорректная сетка выборки CFD".into());
    }
    let (time, dir) = latest(&case.join("postProcessing/sample"))?;
    let n = dims.iter().product();
    let mut result = VolumeField {
        dims,
        min: DVec3::from_array(min),
        max: DVec3::from_array(max),
        velocity: vec![None; n],
        pressure: vec![None; n],
        vorticity: vec![None; n],
        time,
    };
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let Some(fields) = name.strip_prefix("volume_") else {
            continue;
        };
        let names: Vec<_> = fields.split('_').collect();
        let file = fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut reader = BufReader::with_capacity(64 * 1024, file);
        let mut line = String::new();
        let mut v: Vec<f64> = Vec::with_capacity(3 + names.len() * 3);
        loop {
            line.clear();
            if reader
                .read_line(&mut line)
                .map_err(|e| format!("{}: {e}", path.display()))?
                == 0
            {
                break;
            }
            let row = line.trim();
            if row.is_empty() || row.starts_with('#') {
                continue;
            }
            v.clear();
            for token in row.split_whitespace() {
                v.push(
                    token
                        .parse()
                        .map_err(|_| format!("Повреждённая запись CFD: {}", path.display()))?,
                );
            }
            if v.len() < 4 {
                continue;
            }
            let Some(index) = result.index_at(DVec3::new(v[0], v[1], v[2])) else {
                continue;
            };
            let mut col = 3;
            for name in &names {
                let count = if *name == "p" { 1 } else { 3 };
                if col + count > v.len() {
                    return Err("Неполная запись поля CFD".into());
                }
                let valid = v[col..col + count]
                    .iter()
                    .all(|x| x.is_finite() && x.abs() < 1e20);
                if valid {
                    match *name {
                        "U" => {
                            result.velocity[index] =
                                Some(DVec3::new(v[col], v[col + 1], v[col + 2]))
                        }
                        "p" => result.pressure[index] = Some(v[col] * density),
                        "vorticity" => {
                            result.vorticity[index] =
                                Some(DVec3::new(v[col], v[col + 1], v[col + 2]))
                        }
                        _ => {}
                    }
                }
                col += count;
            }
        }
    }
    if result.velocity.iter().flatten().count() < 100
        || result.pressure.iter().flatten().count() < 100
        || result.vorticity.iter().flatten().count() < 100
    {
        return Err("OpenFOAM не вернул достаточное поле U/p".into());
    }
    Ok(result)
}
/// Pressure at the centres of the CFD wall faces, optional in older cases.
pub(crate) fn wall_pressure(
    case: &Path,
    time: f64,
    cfg: &crate::Config,
) -> Result<Vec<(DVec3, f64)>, String> {
    let root = case.join("postProcessing/wallPressure");
    if !root.exists() {
        return Ok(Vec::new());
    }
    let (t, dir) = latest(&root)?;
    if (t - time).abs() > 1e-5 * time.abs().max(1.0) {
        return Err("Давление на стенке и поле относятся к разным шагам".into());
    }
    let data = fs::read_to_string(dir.join("p_body.raw")).map_err(|e| e.to_string())?;
    data.lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|l| {
            let mut words = l.split_whitespace();
            let mut number = || {
                words
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|x| x.is_finite())
                    .ok_or_else(|| "Повреждённое давление стенки".to_owned())
            };
            let pos = DVec3::new(number()?, number()?, number()?);
            let gauge = cfg.gauge_pressure(number()?);
            Ok((pos, gauge))
        })
        .collect()
}

/// OpenFOAM records min, max and area average for each patch.
pub(crate) fn wall_y_plus(case: &Path, time: f64) -> Option<[f64; 3]> {
    let (_, dir) = latest(&case.join("postProcessing/yPlus")).ok()?;
    let records = fs::read_to_string(dir.join("yPlus.dat")).ok()?;
    records.lines().rev().find_map(|l| {
        let mut row = l.split_whitespace();
        let t = row.next()?.parse::<f64>().ok()?;
        if (t - time).abs() > 1e-5 * time.abs().max(1.0) || row.next()? != "body" {
            return None;
        }
        let values: [f64; 3] = [
            row.next()?.parse().ok()?,
            row.next()?.parse().ok()?,
            row.next()?.parse().ok()?,
        ];
        values
            .iter()
            .all(|v| v.is_finite() && *v >= 0.)
            .then_some(values)
    })
}

pub(crate) fn loads(case: &Path) -> Result<Vec<(f64, DVec3)>, String> {
    let mut history = Vec::new();
    for (restart, dir) in time_directories(&case.join("postProcessing/loads"))? {
        let records = load_segment(&dir)?;
        // The state at restart is shared; later iterations from the old run
        // were superseded even if the new run ended sooner.
        history.truncate(history.partition_point(|&(t, _)| t <= restart && t < records[0].0));
        history.extend(records);
    }
    if history.is_empty() {
        return Err("Нет конечных сил из расчёта OpenFOAM".into());
    }
    Ok(history)
}

fn load_segment(dir: &Path) -> Result<Vec<(f64, DVec3)>, String> {
    let modern = dir.join("force.dat");
    let is_modern = modern.exists();
    let path = if is_modern {
        modern
    } else {
        dir.join("forces.dat")
    };
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut history = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let clean = line.replace(['(', ')'], " ");
        let v: Vec<f64> = clean
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|_| "Повреждённая запись сил OpenFOAM")?;
        if v.len() < 10 || !v.iter().all(|n| n.is_finite()) {
            return Err("Некорректная запись сил OpenFOAM".into());
        }
        let force = if is_modern {
            DVec3::new(v[1], v[2], v[3])
        } else {
            DVec3::new(v[1] + v[4] + v[7], v[2] + v[5] + v[8], v[3] + v[6] + v[9])
        };
        if history.last().is_some_and(|&(time, _)| v[0] <= time) {
            return Err("Нарушен порядок отсчётов сил OpenFOAM".into());
        }
        history.push((v[0], force));
    }
    if history.is_empty() {
        return Err("Нет конечных сил из расчёта OpenFOAM".into());
    }
    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tilted_wind_sets_the_inlet_and_the_sign_of_backflow() {
        let mut f = VolumeField {
            dims: [5; 3],
            min: DVec3::ZERO,
            max: DVec3::splat(4.0),
            velocity: vec![Some(DVec3::Y); 125],
            pressure: vec![None; 125],
            vorticity: vec![None; 125],
            time: 1.0,
        };
        let body = Some((DVec3::ONE, DVec3::splat(3.)));
        let lines = f.streamlines_in_wind(body, DVec3::Y);
        assert_eq!(lines.len(), 49);
        assert!(
            lines
                .iter()
                .all(|line| line[0].y < 0.1 && line.last().unwrap().y > 3.8)
        );
        f.velocity[0] = None;
        f.velocity[30] = Some(DVec3::new(-25., 1., 0.));
        f.velocity[42] = Some(DVec3::new(3., -7., 1.));
        assert_eq!(f.strongest_backflow(DVec3::Y), Some((42, 7.)));
        assert_eq!(f.strongest_backflow(DVec3::X), Some((30, 25.)));
        assert_eq!(f.strongest_backflow(DVec3::Z), None);
        let seed = upstream_entry(DVec3::splat(2.), -DVec3::Y, f.min, f.max).unwrap();
        assert_eq!(seed.y, f.max.y);
    }
    #[test]
    fn trilinear_interpolation_reproduces_affine_field_and_rejects_solid() {
        let mut f = VolumeField {
            dims: [3, 3, 3],
            min: DVec3::ZERO,
            max: DVec3::splat(2.0),
            velocity: vec![None; 27],
            pressure: vec![None; 27],
            vorticity: vec![None; 27],
            time: 1.0,
        };
        for i in 0..27 {
            let p = f.position(i);
            f.velocity[i] = Some(DVec3::new(1.0 + 2.0 * p.x, p.y - 3.0, p.z + 1.0));
        }
        let p = DVec3::new(0.23, 1.19, 0.42);
        assert!(
            f.sample_velocity(p)
                .unwrap()
                .distance(DVec3::new(1.46, -1.81, 1.42))
                < 1e-12
        );
        assert!(f.sample_velocity(DVec3::splat(-1.0)).is_none());
        f.velocity[3] = None;
        assert!(f.sample_velocity(p).is_none());
    }
}

#[cfg(test)]
mod load_tests {
    use super::*;
    #[test]
    fn resumed_loads_keep_the_prefix_and_replace_abandoned_iterations() {
        let root = tempfile::tempdir().unwrap();
        for (start, rows) in [
            ("0", vec![(1, 10), (2, 20), (3, 30), (4, 40), (5, 50)]),
            ("2", vec![(3, 31), (4, 41)]),
            ("3", vec![(3, 32), (4, 42)]),
        ] {
            let dir = root.path().join("postProcessing/loads").join(start);
            fs::create_dir_all(&dir).unwrap();
            let contents: String = rows
                .into_iter()
                .map(|(t, f)| format!("{t} {f} 0 0 0 0 0 0 0 0\n"))
                .collect();
            fs::write(dir.join("force.dat"), contents).unwrap();
        }
        let forces: Vec<_> = loads(root.path())
            .unwrap()
            .into_iter()
            .map(|(t, f)| (t, f.x))
            .collect();
        assert_eq!(forces, [(1., 10.), (2., 20.), (3., 32.), (4., 42.)]);
    }
    #[test]
    fn modern_total_force_is_not_added_to_its_components_again() {
        let root = std::env::temp_dir().join(format!(
            "glenn-force-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let dir = root.join("postProcessing/loads/0");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("force.dat"),
            "# Time total pressure viscous\n100 3 5 7 1 2 3 2 3 4\n",
        )
        .unwrap();
        assert_eq!(loads(&root).unwrap()[0], (100.0, DVec3::new(3.0, 5.0, 7.0)));
        fs::remove_file(dir.join("force.dat")).unwrap();
        fs::write(
            dir.join("forces.dat"),
            "100 ((1 2 3) (2 3 4) (0 0 0)) ((0 0 0) (0 0 0) (0 0 0))\n",
        )
        .unwrap();
        assert_eq!(loads(&root).unwrap()[0], (100.0, DVec3::new(3.0, 5.0, 7.0)));
        fs::remove_dir_all(root).unwrap();
    }
}
