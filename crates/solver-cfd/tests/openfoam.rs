//! Real integration test, never silently substituted with a fixture solver.
//! OPENFOAM_BASHRC=/path/etc/bashrc cargo test -p glenn-solver-cfd --test openfoam -- --ignored --nocapture
use glenn_engine::Worker;
use glenn_solver_cfd::{Backend, Config, FlowModel, run};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[test]
#[ignore = "requires an installed OpenCFD OpenFOAM 2412 toolchain"]
fn real_mesh_solver_and_postprocessing_for_all_three_models() {
    let bashrc =
        std::env::var("OPENFOAM_BASHRC").expect("Set OPENFOAM_BASHRC to the real etc/bashrc");
    let root = std::env::temp_dir().join(format!(
        "glenn-openfoam-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    eprintln!("Cases and logs: {}", root.display());
    for (name, model) in [
        ("laminar", FlowModel::Laminar),
        ("rans", FlowModel::SteadyRans),
        ("urans", FlowModel::UnsteadyRans),
    ] {
        let mesh =
            glenn_io::stl::parse(include_bytes!("../../../tests/fixtures/cube.stl")).unwrap();
        let cfg = Config {
            model,
            speed: 1.0,
            kinematic_viscosity: 0.01,
            iterations: 100,
            transit_times: 0.1,
            ..Config::default()
        };
        let backend = Backend {
            bashrc: bashrc.clone(),
            wsl: false,
        };
        let case = root.join(name);
        let worker = Worker::new(move |(): (), cancel| {
            run(
                &mesh,
                &cfg,
                &backend,
                &case,
                cancel,
                &Arc::new(Mutex::new(String::new())),
            )
        });
        worker.submit(());
        let until = Instant::now() + Duration::from_secs(300);
        loop {
            if let Some((_, result)) = worker.poll() {
                let result = result.unwrap();
                assert!(result.force.is_finite() && result.force.x > 0.0);
                assert!(result.field.velocity.iter().flatten().count() > 1000);
                assert!(!result.streamlines.is_empty());
                let min_ux = result
                    .field
                    .velocity
                    .iter()
                    .flatten()
                    .map(|v| v.x)
                    .fold(f64::INFINITY, f64::min);
                if model == FlowModel::Laminar {
                    assert!(
                        min_ux < -0.01,
                        "Expected resolved backflow behind cube at Re=100"
                    );
                }
                eprintln!(
                    "{name}: t={}, Fx={} N, min Ux={min_ux}, converged={}",
                    result.field.time, result.force.x, result.converged
                );
                break;
            }
            assert!(
                Instant::now() < until,
                "OpenFOAM exceeded timeout; worker cancellation stops the process"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[test]
#[ignore = "requires an installed OpenCFD OpenFOAM 2412 toolchain"]
fn low_reynolds_case_converges_and_writes_a_coherent_final_time() {
    let backend = Backend {
        bashrc: std::env::var("OPENFOAM_BASHRC").expect("Set OPENFOAM_BASHRC"),
        wsl: false,
    };
    let case = std::env::temp_dir().join(format!(
        "glenn-low-re-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    eprintln!("Case: {}", case.display());
    let mesh = glenn_io::stl::parse(include_bytes!("../../../tests/fixtures/cube.stl")).unwrap();
    let cfg = Config {
        model: FlowModel::Laminar,
        speed: 0.1,
        kinematic_viscosity: 0.1,
        iterations: 500,
        ..Config::default()
    };
    let worker = Worker::new(move |(): (), cancel| {
        run(
            &mesh,
            &cfg,
            &backend,
            &case,
            cancel,
            &Arc::new(Mutex::new(String::new())),
        )
    });
    worker.submit(());
    let until = Instant::now() + Duration::from_secs(300);
    loop {
        if let Some((_, result)) = worker.poll() {
            let r = result.unwrap();
            assert!(r.converged && r.field.time < 500.0);
            assert!(
                r.force.x > 0.0
                    && r.force.y.abs() < 0.01 * r.force.x
                    && r.force.z.abs() < 0.01 * r.force.x
            );
            eprintln!(
                "Converged at {}, force {:?}",
                r.field.time,
                r.force.to_array()
            );
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(100));
    }
}
