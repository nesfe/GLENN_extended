//! ASCII and binary STL. Binary detection uses the record length, not `solid`:
//! real binary headers often begin with that word. No unit is encoded in STL.
use glam::DVec3;
use glenn_core::mesh::SolidMesh;
use std::{io::Read, path::Path};
pub const MAX_STL_BYTES: u64 = 64 * 1024 * 1024;
pub fn read(path: &Path) -> Result<SolidMesh, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_STL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    parse(&bytes)
}
pub fn parse(bytes: &[u8]) -> Result<SolidMesh, String> {
    if bytes.len() as u64 > MAX_STL_BYTES {
        return Err("STL превышает 64 МБ".into());
    }
    if bytes.len() >= 84 {
        let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        if count <= 1_000_000 && 84 + 50 * count == bytes.len() {
            let mut triangles = Vec::with_capacity(count);
            for face in bytes[84..].chunks_exact(50) {
                let mut points = [DVec3::ZERO; 3];
                for (i, p) in points.iter_mut().enumerate() {
                    let k = 12 + i * 12;
                    *p = DVec3::from_array(std::array::from_fn(|axis| {
                        f32::from_le_bytes(face[k + axis * 4..k + axis * 4 + 4].try_into().unwrap())
                            as f64
                    }));
                }
                triangles.push(points);
            }
            return SolidMesh::from_triangles(triangles);
        }
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "STL: повреждён бинарный файл (длина не соответствует числу граней)")?;
    let mut words = text.split_whitespace();
    if words.next() != Some("solid") {
        return Err("STL: нет заголовка solid или корректной бинарной записи".into());
    }
    // Solid names occupy the rest of the header line and are not geometry.
    let mut faces = Vec::new();
    let mut points = Vec::new();
    let mut in_facet = false;
    for line in text.lines().skip(1) {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("facet") => {
                if in_facet {
                    return Err("STL: незавершённая грань".into());
                }
                in_facet = true;
                points.clear();
            }
            Some("vertex") => {
                if !in_facet {
                    return Err("STL: вершина вне грани".into());
                }
                let mut p = [0.0; 3];
                for v in &mut p {
                    *v = parts
                        .next()
                        .ok_or("STL: неполная вершина")?
                        .parse::<f64>()
                        .map_err(|_| "STL: неверная координата")?;
                }
                points.push(DVec3::from_array(p));
                if points.len() > 3 {
                    return Err("STL: в грани больше трёх вершин".into());
                }
            }
            Some("endfacet") => {
                if !in_facet || points.len() != 3 {
                    return Err("STL: неполная грань".into());
                }
                faces.push([points[0], points[1], points[2]]);
                in_facet = false;
            }
            _ => {}
        }
    }
    if in_facet {
        return Err("STL: оборванный файл".into());
    }
    SolidMesh::from_triangles(faces)
}
