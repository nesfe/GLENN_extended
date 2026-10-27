use glam::DVec3;
use glenn_extended::{geometry::Airfoil, interchange, mesh::SolidMesh, model::Simulation};
#[test]
fn ascii_and_binary_stl_share_units_geometry_and_topology() {
    let ascii = include_bytes!("fixtures/cube.stl");
    let mut mesh = interchange::stl::parse(ascii).unwrap();
    let report = mesh.validate_and_orient().unwrap();
    assert_eq!(mesh.triangles.len(), 12);
    assert_eq!(report.components, 1);
    assert!((report.volume - 1.0).abs() < 1e-12);
    assert!((report.area - 6.0).abs() < 1e-12);
    let mut binary = vec![0u8; 80];
    binary[..5].copy_from_slice(b"solid");
    binary.extend(12u32.to_le_bytes());
    for t in &mesh.triangles {
        binary.extend([0u8; 12]);
        for i in t {
            for v in mesh.vertices[*i].to_array() {
                binary.extend((v as f32).to_le_bytes());
            }
        }
        binary.extend([0, 0]);
    }
    let mut read = interchange::stl::parse(&binary)
        .unwrap()
        .scaled(0.001)
        .unwrap();
    assert!((read.validate_and_orient().unwrap().volume - 1e-9).abs() < 1e-20);
    binary.pop();
    assert!(interchange::stl::parse(&binary).is_err());
    let mut open = mesh.clone();
    open.triangles.pop();
    assert!(open.validate_and_orient().unwrap_err().contains("открытых"));
    mesh.triangles[0].swap(1, 2);
    assert!(mesh.validate_and_orient().is_err());
}
#[test]
fn stl_rejects_degenerate_and_nonfinite_vertices() {
    assert!(SolidMesh::from_triangles(vec![[DVec3::ZERO; 3]]).is_err());
    assert!(SolidMesh::from_triangles(vec![[DVec3::NAN, DVec3::X, DVec3::Y]]).is_err());
}
#[test]
fn dxf_reassembles_unordered_line_entities() {
    let state = Simulation::default();
    let contour = Airfoil::generate(&state).contour();
    let mut segments: Vec<_> = (0..contour.len())
        .map(|i| {
            let a = contour[i];
            let b = contour[(i + 1) % contour.len()];
            format!(
                "0\nLINE\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n",
                a.x, a.y, b.x, b.y
            )
        })
        .collect();
    segments.reverse();
    segments.rotate_left(31);
    let text = format!(
        "\u{feff}0\nSECTION\n2\nENTITIES\n{}0\nENDSEC\n0\nEOF\n",
        segments.join("")
    );
    let imported = interchange::import_airfoil(&text, "lines.dxf", &state).unwrap();
    let result = glenn_extended::physics::aerodynamics::Analysis::new(imported).unwrap();
    let reference = glenn_extended::physics::aerodynamics::Analysis::new(state).unwrap();
    assert!((result.current.cl - reference.current.cl).abs() < 0.01);
}
#[test]
fn sampled_streams_cannot_cross_thin_surfaces_between_grid_points() {
    let mesh = interchange::stl::parse(include_bytes!("fixtures/cube.stl")).unwrap();
    let surface = glenn_extended::mesh::SurfaceIndex::new(&mesh);
    assert!(surface.crosses(DVec3::new(-1.0, 0.5, 0.5), DVec3::new(2.0, 0.5, 0.5)));
    assert!(surface.crosses(DVec3::new(2.0, 0.5, 0.5), DVec3::new(-1.0, 0.5, 0.5)));
    assert!(!surface.crosses(DVec3::new(-1.0, 2.0, 0.5), DVec3::new(2.0, 2.0, 0.5)));
}
