use glam::DVec3;
use std::{
    fs,
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
        let step = (self.max - self.min).min_element() / 180.0;
        let mut seeds = Vec::new();
        for z in 1..8 {
            for y in 1..12 {
                seeds.push(
                    self.min
                        + (self.max - self.min) * DVec3::new(0.01, y as f64 / 12.0, z as f64 / 8.0),
                );
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
            .map(|v| v.length())
            .fold(0.0, f64::max);
        for (i, v) in self.velocity.iter().enumerate() {
            if v.is_some_and(|v| v.x < -0.03 * max_speed) {
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
fn latest(dir: &Path) -> Result<(f64, PathBuf), String> {
    fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let t = e.file_name().to_str()?.parse::<f64>().ok()?;
            (t.is_finite() && e.path().is_dir()).then_some((t, e.path()))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
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
        let data = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        for line in data
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        {
            let v: Vec<f64> = line
                .split_whitespace()
                .map(|s| {
                    s.parse()
                        .map_err(|_| format!("Повреждённая запись CFD: {}", path.display()))
                })
                .collect::<Result<_, _>>()?;
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
pub(crate) fn loads(case: &Path) -> Result<(f64, DVec3), String> {
    let (_, dir) = latest(&case.join("postProcessing/loads"))?;
    let modern = dir.join("force.dat");
    let is_modern = modern.exists();
    let path = if is_modern {
        modern
    } else {
        dir.join("forces.dat")
    };
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    for line in text
        .lines()
        .rev()
        .filter(|l| !l.trim_start().starts_with('#'))
    {
        let clean = line.replace(['(', ')'], " ");
        let v: Vec<f64> = clean
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        if v.len() >= 10 && v.iter().all(|v| v.is_finite()) {
            let force = if is_modern {
                DVec3::new(v[1], v[2], v[3])
            } else {
                DVec3::new(v[1] + v[4] + v[7], v[2] + v[5] + v[8], v[3] + v[6] + v[9])
            };
            return Ok((v[0], force));
        }
    }
    Err("Нет конечных сил из расчёта OpenFOAM".into())
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(loads(&root).unwrap(), (100.0, DVec3::new(3.0, 5.0, 7.0)));
        fs::remove_file(dir.join("force.dat")).unwrap();
        fs::write(
            dir.join("forces.dat"),
            "100 ((1 2 3) (2 3 4) (0 0 0)) ((0 0 0) (0 0 0) (0 0 0))\n",
        )
        .unwrap();
        assert_eq!(loads(&root).unwrap(), (100.0, DVec3::new(3.0, 5.0, 7.0)));
        fs::remove_dir_all(root).unwrap();
    }
}
