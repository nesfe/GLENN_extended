//! Cartesian control volumes clipped by the actual section polygon.
//! All geometry here uses cell widths as units.
use crate::{Config, inside};

pub(super) struct Wall {
    pub cell: usize,
    pub normal: [f64; 2], // unit normal pointing from fluid into the body
    pub length: f64,
    pub distance: f64, // normal distance from the merged fluid centroid
    midpoint: [f64; 2],
}

pub(super) struct CutCells {
    pub open: Vec<[f64; 2]>, // aperture of the positive x and y faces
    pub volume: Vec<f64>,
    pub sample_solid: Vec<bool>,
    pub walls: Vec<Wall>,
    groups: Vec<(Vec<usize>, f64)>,
}

fn crossing(a: [f64; 2], b: [f64; 2], axis: usize, level: f64) -> Option<f64> {
    let delta = b[axis] - a[axis];
    if delta.abs() < 1e-13 {
        return None;
    }
    let t = (level - a[axis]) / delta;
    (0.0..=1.0)
        .contains(&t)
        .then_some(a[1 - axis] + t * (b[1 - axis] - a[1 - axis]))
}

fn root(parents: &mut [usize], mut i: usize) -> usize {
    while parents[i] != i {
        parents[i] = parents[parents[i]];
        i = parents[i];
    }
    i
}

impl CutCells {
    pub fn new(cfg: &Config, width: usize, height: usize) -> Result<Self, String> {
        let count = width * height;
        let mut mesh = Self {
            open: vec![[1.0; 2]; count],
            volume: vec![1.0; count],
            sample_solid: vec![false; count],
            walls: Vec::new(),
            groups: Vec::new(),
        };
        if cfg.contour.is_empty() {
            return Ok(mesh);
        }
        let n = cfg.chord_cells as f64;
        let (s, c) = (-cfg.alpha.to_radians()).sin_cos();
        let mut poly: Vec<_> = cfg
            .contour
            .iter()
            .map(|p| {
                [
                    (1.75 + (p.x - 0.25) * c - p.y * s) * n,
                    (2.0 + (p.x - 0.25) * s + p.y * c) * n,
                ]
            })
            .collect();
        let signed_area: f64 = poly
            .iter()
            .zip(poly.iter().cycle().skip(1))
            .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
            .sum();
        if signed_area < 0.0 {
            poly.reverse();
        }
        let dims = [width, height];
        let mut ranges = [0..0, 0..0];
        for axis in 0..2 {
            let lo = poly.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min);
            let hi = poly
                .iter()
                .map(|p| p[axis])
                .fold(f64::NEG_INFINITY, f64::max);
            if lo < 1.0 || hi > (dims[axis] - 1) as f64 {
                return Err("Контур выходит за границу расчётной области".into());
            }
            ranges[axis] = lo.floor() as usize - 1..hi.ceil() as usize + 1;
        }
        // Split each grid line at its polygon intersections. An interval on
        // the wall is closed to transport even for an exactly grid-aligned edge.
        for axis in 0..2 {
            for line in ranges[axis].clone() {
                let level = (line + 1) as f64;
                let mut cuts = Vec::new();
                let mut aligned = Vec::new();
                for (&a, &b) in poly.iter().zip(poly.iter().cycle().skip(1)) {
                    if let Some(at) = crossing(a, b, axis, level) {
                        cuts.push(at);
                    } else if (a[axis] - level).abs() < 1e-12 && (b[axis] - level).abs() < 1e-12 {
                        let (p, q) = (a[1 - axis], b[1 - axis]);
                        aligned.push(p.min(q)..=p.max(q));
                        cuts.extend([p, q]);
                    }
                }
                cuts.sort_by(f64::total_cmp);
                for pair in cuts.windows(2) {
                    let mid = (pair[0] + pair[1]) * 0.5;
                    let p = if axis == 0 {
                        [level, mid]
                    } else {
                        [mid, level]
                    };
                    if !(inside(p, &poly) || aligned.iter().any(|r| r.contains(&mid))) {
                        continue;
                    }
                    for row in pair[0].floor() as usize..pair[1].ceil() as usize {
                        let i = if axis == 0 {
                            row * width + line
                        } else {
                            line * width + row
                        };
                        mesh.open[i][axis] -=
                            pair[1].min((row + 1) as f64) - pair[0].max(row as f64);
                    }
                }
            }
        }
        // Integrals over the east/north faces and the clipped wall give the
        // fluid area and both first moments via the divergence theorem.
        let mut moments = vec![[0.5; 2]; count];
        for y in ranges[1].clone() {
            for x in ranges[0].clone() {
                let i = y * width + x;
                for a in &mut mesh.open[i] {
                    *a = a.clamp(0.0, 1.0);
                }
                mesh.sample_solid[i] = inside([x as f64 + 0.5, y as f64 + 0.5], &poly);
                mesh.volume[i] = mesh.open[i][0];
                moments[i] = [0.5 * mesh.open[i][0], 0.5 * mesh.open[i][1]];
            }
        }
        for (&a, &b) in poly.iter().zip(poly.iter().cycle().skip(1)) {
            let d = [b[0] - a[0], b[1] - a[1]];
            let length = d[0].hypot(d[1]);
            if length < 1e-12 {
                continue;
            }
            let normal = [-d[1] / length, d[0] / length];
            let mut ts = vec![0.0, 1.0];
            for axis in 0..2 {
                for line in
                    a[axis].min(b[axis]).ceil() as usize..=a[axis].max(b[axis]).floor() as usize
                {
                    if d[axis].abs() > 1e-12 {
                        let t = (line as f64 - a[axis]) / d[axis];
                        if t > 0.0 && t < 1.0 {
                            ts.push(t);
                        }
                    }
                }
            }
            ts.sort_by(f64::total_cmp);
            for t in ts.windows(2) {
                let span = length * (t[1] - t[0]);
                if span < 1e-11 {
                    continue;
                }
                let midpoint = std::array::from_fn(|k| a[k] + 0.5 * (t[1] + t[0]) * d[k]);
                let origin: [f64; 2] =
                    std::array::from_fn(|k| (midpoint[k] - 1e-10 * normal[k]).floor());
                let i = origin[1] as usize * width + origin[0] as usize;
                mesh.volume[i] += span * normal[0] * (midpoint[0] - origin[0]);
                for k in 0..2 {
                    let m = midpoint[k] - origin[k];
                    let delta = (t[1] - t[0]) * d[k];
                    moments[i][k] += 0.5 * span * normal[k] * (m * m + delta * delta / 12.0);
                }
                mesh.walls.push(Wall {
                    cell: i,
                    normal,
                    length: span,
                    distance: 0.0,
                    midpoint,
                });
            }
        }
        let mut parents: Vec<_> = (0..count).collect();
        for y in ranges[1].clone() {
            for x in ranges[0].clone() {
                let i = y * width + x;
                let v = mesh.volume[i];
                if !(-1e-7..=1.0000001).contains(&v) {
                    return Err("Ошибка площади обрезанной ячейки".into());
                }
                mesh.volume[i] = if v < 1e-10 { 0.0 } else { v.min(1.0) };
            }
        }
        // Merge small cut cells across an open face with the largest fluid
        // neighbour. Every shared face flux cancels inside the union.
        for y in ranges[1].clone() {
            for x in ranges[0].clone() {
                let i = y * width + x;
                let v = mesh.volume[i];
                if v <= 0.0 || v >= 0.5 {
                    continue;
                }
                let neighbors = [
                    (i + 1, mesh.open[i][0]),
                    (i - 1, mesh.open[i - 1][0]),
                    (i + width, mesh.open[i][1]),
                    (i - width, mesh.open[i - width][1]),
                ];
                if let Some((j, _)) = neighbors
                    .into_iter()
                    .filter(|&(j, op)| op > 1e-10 && mesh.volume[j] > 0.0)
                    .max_by(|a, b| (a.1 * mesh.volume[a.0]).total_cmp(&(b.1 * mesh.volume[b.0])))
                {
                    let from = root(&mut parents, i);
                    let to = root(&mut parents, j);
                    parents[from] = to;
                }
            }
        }
        let mut groups = std::collections::BTreeMap::<usize, Vec<usize>>::new();
        for i in 0..count {
            let j = root(&mut parents, i);
            if j != i {
                groups.entry(j).or_insert_with(|| vec![j]).push(i);
            } else if mesh.volume[i] > 0.0 && mesh.volume[i] < 1.0 - 1e-12 {
                groups.entry(j).or_insert_with(|| vec![j]);
            }
        }
        for (j, group) in groups {
            let area: f64 = group.iter().map(|&i| mesh.volume[i]).sum();
            let centroid = std::array::from_fn::<_, 2, _>(|k| {
                group
                    .iter()
                    .map(|&i| {
                        let offset = if k == 0 { i % width } else { i / width };
                        moments[i][k] + offset as f64 * mesh.volume[i]
                    })
                    .sum::<f64>()
                    / area
            });
            for wall in mesh.walls.iter_mut().filter(|w| parents[w.cell] == j) {
                wall.distance = ((wall.midpoint[0] - centroid[0]) * wall.normal[0]
                    + (wall.midpoint[1] - centroid[1]) * wall.normal[1])
                    .abs();
            }
            mesh.groups.push((group, area));
        }
        for wall in &mut mesh.walls {
            if wall.distance == 0.0 {
                let x = (wall.cell % width) as f64 + 0.5;
                let y = (wall.cell / width) as f64 + 0.5;
                wall.distance = ((wall.midpoint[0] - x) * wall.normal[0]
                    + (wall.midpoint[1] - y) * wall.normal[1])
                    .abs();
            }
        }
        Ok(mesh)
    }
    pub fn distribute(&self, rhs: &mut [[f64; 4]]) {
        for (group, volume) in &self.groups {
            let rate =
                std::array::from_fn(|k| group.iter().map(|&i| rhs[i][k]).sum::<f64>() / volume);
            for &i in group {
                rhs[i] = rate;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glenn_core::model::Point;
    #[test]
    fn cut_volumes_and_wall_fluxes_close_at_every_angle() {
        let n = 32;
        for alpha in [0.0, 12.0, 45.0, 90.0] {
            let cfg = Config {
                contour: vec![
                    Point::new(0.0, 0.0),
                    Point::new(1.0, 0.0),
                    Point::new(0.0, 0.25),
                ],
                alpha,
                chord_cells: n,
                reynolds: 1000.0,
                mach: 0.5,
                gamma: 1.4,
            };
            let width = 7 * n;
            let grid = CutCells::new(&cfg, width, 4 * n).unwrap();
            let area: f64 = grid.volume.iter().map(|v| 1.0 - v).sum();
            assert!(
                (area - 0.125 * (n * n) as f64).abs() < 1e-9,
                "{alpha}: {area}"
            );
            let mut normal = vec![[0.0; 2]; grid.volume.len()];
            for w in &grid.walls {
                assert!(w.distance > 0.0 && w.distance.is_finite());
                for (value, component) in normal[w.cell].iter_mut().zip(w.normal) {
                    *value += component * w.length;
                }
            }
            for (i, wall_normal) in normal.iter().enumerate().skip(width + 1) {
                for (k, step) in [1, width].into_iter().enumerate() {
                    let closure = grid.open[i][k] - grid.open[i - step][k] + wall_normal[k];
                    assert!(
                        closure.abs() < 1e-9,
                        "angle {alpha}, cell {i}, axis {k}: {closure}"
                    );
                }
            }
        }
    }
}
