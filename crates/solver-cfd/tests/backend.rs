use glenn_solver_cfd::{Config, FlowModel, write_case};
use std::time::{SystemTime, UNIX_EPOCH};
fn cube() -> glenn_core::mesh::SolidMesh {
    glenn_io::stl::parse(include_bytes!("../../../tests/fixtures/cube.stl")).unwrap()
}
#[test]
fn cases_preserve_units_and_do_not_overwrite_user_files() {
    let root = std::env::temp_dir().join(format!(
        "glenn-case-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for (i, model) in [
        FlowModel::Laminar,
        FlowModel::SteadyRans,
        FlowModel::UnsteadyRans,
    ]
    .into_iter()
    .enumerate()
    {
        let cfg = Config {
            model,
            speed: 7.0,
            density: 998.0,
            kinematic_viscosity: 1e-6,
            ..Config::default()
        };
        let path = root.join(i.to_string());
        write_case(&path, &cube(), &cfg).unwrap();
        let text = std::fs::read_to_string(path.join("system/controlDict")).unwrap();
        assert!(text.contains("rhoInf 998"));
        assert!(text.contains(if model == FlowModel::UnsteadyRans {
            "pimpleFoam"
        } else {
            "simpleFoam"
        }));
        assert!(
            write_case(&path, &cube(), &cfg)
                .unwrap_err()
                .contains("существует")
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn nonfinite_or_unsupported_inputs_are_rejected_before_meshing() {
    for speed in [f64::NAN, -1.0, 0.0, 500.0] {
        let c = Config {
            speed,
            ..Config::default()
        };
        assert!(c.validate().is_err());
    }
}

#[test]
fn imported_pressure_has_physical_units_and_fields_cannot_mix_times() {
    let root = std::env::temp_dir().join(format!(
        "glenn-fields-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("postProcessing/sample/10")).unwrap();
    std::fs::create_dir_all(root.join("postProcessing/loads/0")).unwrap();
    std::fs::create_dir_all(root.join("constant/polyMesh")).unwrap();
    std::fs::write(
        root.join("constant/polyMesh/boundary"),
        "1\n(\nbody\n{\nnFaces 6;\n}\n)\n",
    )
    .unwrap();
    std::fs::write(
        root.join("glenn-case.json"),
        serde_json::to_string(&Config {
            density: 2.0,
            ..Config::default()
        })
        .unwrap(),
    )
    .unwrap();
    std::fs::write(root.join("sample-grid.json"), "[[5,5,5],[0,0,0],[4,4,4]]").unwrap();
    let mut cloud = String::new();
    for z in 0..5 {
        for y in 0..5 {
            for x in 0..5 {
                cloud.push_str(&format!("{x} {y} {z} 3 1 0 0 0 0 0\n"));
            }
        }
    }
    let field = root.join("postProcessing/sample/10/volume_p_U_vorticity.xy");
    std::fs::write(&field, &cloud).unwrap();
    std::fs::write(
        root.join("log.simpleFoam"),
        "SIMPLE solution converged in 10 iterations",
    )
    .unwrap();
    let force = root.join("postProcessing/loads/0/force.dat");
    std::fs::write(&force, "9 3 0 0 1 0 0 2 0 0\n").unwrap();
    assert!(
        glenn_solver_cfd::load_result(&root)
            .err()
            .unwrap()
            .contains("разным моментам")
    );
    std::fs::write(&force, "10 3 0 0 1 0 0 2 0 0\n").unwrap();
    let result = glenn_solver_cfd::load_result(&root).unwrap();
    assert_eq!(result.field.pressure[0], Some(6.0));
    assert_eq!(result.force.x, 3.0);
    assert!(result.converged);
    std::fs::write(&field, cloud.replacen("3 1", "broken 1", 1)).unwrap();
    assert!(glenn_solver_cfd::load_result(&root).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn uploaded_tsagi_is_imported_and_its_thickness_controls_refinement() {
    let model =
        glenn_io::stl::parse(include_bytes!("../../../ЦАГИ 846-14 (без прорезей).STL")).unwrap();
    assert_eq!(model.triangles.len(), 9408);
    let (lo, hi) = model.bounds();
    assert!(((hi - lo).z - 35.0).abs() < 1e-6);
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ЦАГИ расчёт");
    write_case(&path, &model, &Config::default()).unwrap();
    let snappy = std::fs::read_to_string(path.join("system/snappyHexMeshDict")).unwrap();
    assert!(
        snappy.contains("level (7 7)"),
        "thin wing must not disappear in span-sized cells"
    );
    let (dims, a, b): ([usize; 3], [f64; 3], [f64; 3]) =
        serde_json::from_str(&std::fs::read_to_string(path.join("sample-grid.json")).unwrap())
            .unwrap();
    assert!(
        (b[1] - a[1]) / ((dims[1] - 1) as f64) < 0.2,
        "display grid must sample near the wing"
    );
}

#[test]
fn a_previously_saved_empty_tunnel_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("constant/polyMesh")).unwrap();
    for text in [
        "inlet\n{\nnFaces 400;\n}\n",
        "body\n{\nnFaces 0;\n}\nother\n{\nnFaces 100;\n}\n",
    ] {
        std::fs::write(root.path().join("constant/polyMesh/boundary"), text).unwrap();
        let error = glenn_solver_cfd::load_result(root.path()).err().unwrap();
        assert!(error.contains("body: 0 граней"), "{error}");
    }
}
