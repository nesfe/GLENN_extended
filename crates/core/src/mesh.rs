//! Closed triangulated solids in metres, independent of any file format.
use glam::DVec3;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct SolidMesh {
    pub vertices: Vec<DVec3>,
    pub triangles: Vec<[usize; 3]>,
}
#[derive(Clone, Debug)]
pub struct MeshReport {
    pub min: DVec3,
    pub max: DVec3,
    pub area: f64,
    pub volume: f64,
    pub components: usize,
}
impl SolidMesh {
    pub fn from_triangles(faces: Vec<[DVec3; 3]>) -> Result<Self, String> {
        if faces.is_empty() || faces.len() > 1_000_000 {
            return Err("STL: нужно 1…1 000 000 треугольников".into());
        }
        let mut lookup = HashMap::new();
        let mut vertices = Vec::new();
        let mut triangles = Vec::with_capacity(faces.len());
        for face in faces {
            let mut indices = [0; 3];
            for (i, p) in face.into_iter().enumerate() {
                if !p.is_finite() {
                    return Err("STL: координаты содержат NaN или бесконечность".into());
                }
                let key = p.to_array().map(|v| if v == 0.0 { 0 } else { v.to_bits() });
                indices[i] = *lookup.entry(key).or_insert_with(|| {
                    vertices.push(p);
                    vertices.len() - 1
                });
            }
            triangles.push(indices);
        }
        let mut mesh = Self {
            vertices,
            triangles,
        };
        // Repair a consistently inward oriented component; mixed winding and
        // open edges are errors, not silently patched holes.
        mesh.validate_and_orient()?;
        Ok(mesh)
    }
    pub fn bounds(&self) -> (DVec3, DVec3) {
        self.vertices.iter().fold(
            (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        )
    }
    pub fn scaled(&self, scale: f64) -> Result<Self, String> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err("Масштаб STL должен быть положительным".into());
        }
        Self::from_triangles(
            self.triangles
                .iter()
                .map(|t| t.map(|i| self.vertices[i] * scale))
                .collect(),
        )
    }
    pub fn validate_and_orient(&mut self) -> Result<MeshReport, String> {
        if self.vertices.is_empty()
            || self.triangles.is_empty()
            || self.vertices.iter().any(|p| !p.is_finite())
        {
            return Err("Пустая или некорректная сетка".into());
        }
        let (min, max) = self.bounds();
        let length = (max - min).max_element();
        if length <= 0.0 || !length.is_finite() {
            return Err("Нулевая геометрия".into());
        }
        let mut edges: HashMap<(usize, usize), Vec<(usize, bool)>> = HashMap::new();
        let mut area = 0.0;
        for (face, t) in self.triangles.iter().enumerate() {
            if t.iter().any(|i| *i >= self.vertices.len()) {
                return Err("Индекс вершины вне сетки".into());
            }
            let [a, b, c] = t.map(|i| self.vertices[i]);
            let twice_area = (b - a).cross(c - a).length();
            if twice_area <= length * length * 1e-14 {
                return Err(format!(
                    "Вырожденный треугольник {face}; исправьте сетку в CAD"
                ));
            }
            area += twice_area * 0.5;
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                edges
                    .entry((a.min(b), a.max(b)))
                    .or_default()
                    .push((face, a < b));
            }
        }
        let open = edges.values().filter(|e| e.len() == 1).count();
        let nonmanifold = edges.values().filter(|e| e.len() > 2).count();
        if open > 0 || nonmanifold > 0 {
            return Err(format!(
                "STL не замкнут: открытых рёбер {open}, неманифолдных {nonmanifold}. Для объёмной продувки нужен замкнутый объект."
            ));
        }
        let mut neighbors = vec![Vec::new(); self.triangles.len()];
        for edge in edges.values() {
            if edge[0].1 == edge[1].1 {
                return Err(
                    "Несогласованное направление граней STL; исправьте нормали в CAD".into(),
                );
            }
            neighbors[edge[0].0].push(edge[1].0);
            neighbors[edge[1].0].push(edge[0].0);
        }
        let mut seen = vec![false; self.triangles.len()];
        let mut volume = 0.0;
        let mut components = 0;
        for seed in 0..self.triangles.len() {
            if seen[seed] {
                continue;
            }
            components += 1;
            let mut stack = vec![seed];
            let mut group = Vec::new();
            let mut signed = 0.0;
            seen[seed] = true;
            while let Some(f) = stack.pop() {
                group.push(f);
                let [a, b, c] = self.triangles[f].map(|i| self.vertices[i] - min);
                signed += a.dot(b.cross(c)) / 6.0;
                for &n in &neighbors[f] {
                    if !seen[n] {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
            if signed.abs() <= length.powi(3) * 1e-14 {
                return Err("STL: компонент имеет нулевой объём".into());
            }
            if signed < 0.0 {
                for f in group {
                    self.triangles[f].swap(1, 2);
                }
            }
            volume += signed.abs();
        }
        Ok(MeshReport {
            min,
            max,
            area,
            volume,
            components,
        })
    }
    pub fn ascii_stl(&self) -> String {
        use std::fmt::Write;
        let mut out = String::from("solid body\n");
        for t in &self.triangles {
            let [a, b, c] = t.map(|i| self.vertices[i]);
            let n = (b - a).cross(c - a).normalize();
            writeln!(out, "facet normal {} {} {}\nouter loop", n.x, n.y, n.z).unwrap();
            for v in [a, b, c] {
                writeln!(out, "vertex {:.12e} {:.12e} {:.12e}", v.x, v.y, v.z).unwrap();
            }
            out.push_str("endloop\nendfacet\n");
        }
        out.push_str("endsolid body\n");
        out
    }
}

/// Read-only BVH for segment/surface queries, built once for a displayed solid.
/// It prevents interpolation of a sampled volume field from drawing through a
/// thin wall that happens to fall between sample points.
pub struct SurfaceIndex<'a> {
    mesh: &'a SolidMesh,
    root: Node,
}
struct Node {
    min: DVec3,
    max: DVec3,
    faces: Vec<usize>,
    children: Option<[Box<Node>; 2]>,
}
impl Node {
    fn build(mesh: &SolidMesh, mut faces: Vec<usize>) -> Self {
        let mut min = DVec3::splat(f64::INFINITY);
        let mut max = DVec3::splat(f64::NEG_INFINITY);
        for f in &faces {
            for i in mesh.triangles[*f] {
                min = min.min(mesh.vertices[i]);
                max = max.max(mesh.vertices[i]);
            }
        }
        if faces.len() <= 8 {
            return Self {
                min,
                max,
                faces,
                children: None,
            };
        }
        let extent = max - min;
        let axis = if extent.x >= extent.y && extent.x >= extent.z {
            0
        } else if extent.y >= extent.z {
            1
        } else {
            2
        };
        let center = |f: usize| {
            mesh.triangles[f]
                .iter()
                .map(|i| mesh.vertices[*i][axis])
                .sum::<f64>()
        };
        let middle = faces.len() / 2;
        faces.select_nth_unstable_by(middle, |a, b| center(*a).total_cmp(&center(*b)));
        let right = faces.split_off(middle);
        Self {
            min,
            max,
            faces: Vec::new(),
            children: Some([
                Box::new(Self::build(mesh, faces)),
                Box::new(Self::build(mesh, right)),
            ]),
        }
    }
    fn intersects(&self, mesh: &SolidMesh, a: DVec3, d: DVec3) -> bool {
        let (mut low, mut high) = (0.0_f64, 1.0_f64);
        for k in 0..3 {
            if d[k].abs() < 1e-20 {
                if a[k] < self.min[k] || a[k] > self.max[k] {
                    return false;
                }
            } else {
                let t1 = (self.min[k] - a[k]) / d[k];
                let t2 = (self.max[k] - a[k]) / d[k];
                low = low.max(t1.min(t2));
                high = high.min(t1.max(t2));
                if low > high {
                    return false;
                }
            }
        }
        if let Some(children) = &self.children {
            return children.iter().any(|n| n.intersects(mesh, a, d));
        }
        self.faces.iter().any(|f| {
            let [v0, v1, v2] = mesh.triangles[*f].map(|i| mesh.vertices[i]);
            let e1 = v1 - v0;
            let e2 = v2 - v0;
            let p = d.cross(e2);
            let determinant = e1.dot(p);
            if determinant.abs() <= 1e-14 * e1.length() * e2.length() * d.length() {
                return false;
            }
            let inverse = 1.0 / determinant;
            let t = a - v0;
            let u = t.dot(p) * inverse;
            if !(-1e-10..=1.0 + 1e-10).contains(&u) {
                return false;
            }
            let q = t.cross(e1);
            let v = d.dot(q) * inverse;
            if v < -1e-10 || u + v > 1.0 + 1e-10 {
                return false;
            }
            let distance = e2.dot(q) * inverse;
            (-1e-10..=1.0 + 1e-10).contains(&distance)
        })
    }
}
impl<'a> SurfaceIndex<'a> {
    pub fn new(mesh: &'a SolidMesh) -> Self {
        Self {
            mesh,
            root: Node::build(mesh, (0..mesh.triangles.len()).collect()),
        }
    }
    pub fn crosses(&self, a: DVec3, b: DVec3) -> bool {
        self.root.intersects(self.mesh, a, b - a)
    }
}
