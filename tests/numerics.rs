use glenn_extended::{
    geometry::{Airfoil, WingMesh},
    interchange,
    model::{Planet, Profile, Simulation, presets},
    physics::{
        aerodynamics::Analysis, atmosphere, flight, panel::PanelModel, probe::total_pressure_ratio,
    },
};
fn symmetric() -> Simulation {
    Simulation {
        camber: 0.0,
        twist: 0.0,
        sweep: 0.0,
        alpha: 0.0,
        ..Simulation::default()
    }
}
#[test]
fn isa_reference_values_and_monotone_pressure() {
    let mut state = Simulation {
        altitude: 0.0,
        ..Simulation::default()
    };
    let sea = atmosphere::calculate(&state);
    assert!((sea.density - 1.225).abs() < 0.001);
    for (h, p) in [(11000.0, 22632.0), (20000.0, 5474.9), (32000.0, 868.0)] {
        state.altitude = h;
        assert!((atmosphere::calculate(&state).pressure - p).abs() < 1.0);
    }
    let mut previous = sea.pressure;
    for h in (100..=80000).step_by(100) {
        state.altitude = h as f64;
        let pressure = atmosphere::calculate(&state).pressure;
        assert!(pressure < previous);
        previous = pressure;
    }
}
#[test]
fn panel_symmetry_lift_slope_and_kutta() {
    let foil = Airfoil::generate(&symmetric());
    let solver = PanelModel::prepare(&foil).unwrap();
    let zero = solver.evaluate(0.0, 0.0);
    assert!(zero.lift.abs() < 1e-9, "{}", zero.lift);
    for (a, b) in zero.upper.iter().zip(&zero.lower) {
        assert!((a.y - b.y).abs() < 1e-8);
    }
    let positive = solver.evaluate(5.0, 0.0);
    let negative = solver.evaluate(-5.0, 0.0);
    assert!((positive.lift + negative.lift).abs() < 1e-8);
    assert!(
        (0.48..0.72).contains(&positive.lift),
        "lift {}",
        positive.lift
    );
    assert!((positive.upper.last().unwrap().y - positive.lower.last().unwrap().y).abs() < 1e-9);
}
#[test]
fn cambered_profile_generates_lift_at_zero_incidence() {
    let state = Simulation {
        alpha: 0.0,
        twist: 0.0,
        ..Simulation::default()
    };
    let solver = PanelModel::prepare(&Airfoil::generate(&state)).unwrap();
    let result = solver.evaluate(0.0, 0.0);
    assert!(result.lift > 0.1);
    assert!(result.moment < 0.0);
}
#[test]
fn every_preset_and_environment_is_finite() {
    for preset in presets() {
        preset.state.validate().unwrap();
        for planet in Planet::ALL {
            let mut state = preset.state.clone();
            state.planet = *planet;
            let result = Analysis::new(state).unwrap();
            let r = &result.current;
            assert!(
                r.cl.is_finite() && r.cd.is_finite() && r.lift.is_finite() && r.cd > 0.0,
                "{}",
                preset.name
            );
        }
    }
}
#[test]
fn span_loading_integrates_to_signed_lift() {
    for alpha in [-12.0, 8.0] {
        let state = Simulation {
            alpha,
            ..symmetric()
        };
        let result = Analysis::new(state).unwrap();
        let points = &result.current.span_loading;
        let integral: f64 = points
            .windows(2)
            .map(|p| (p[1].x - p[0].x) * (p[0].y + p[1].y) * 0.5)
            .sum();
        assert!((integral - result.current.lift).abs() < 1e-7);
    }
}
#[test]
fn zero_speed_has_no_forces_and_stationary_cylinder_has_drag() {
    let still = Analysis::new(Simulation {
        velocity: 0.0,
        ..symmetric()
    })
    .unwrap();
    assert_eq!(still.current.lift, 0.0);
    assert_eq!(still.current.drag, 0.0);
    let cylinder = Analysis::new(Simulation {
        profile_type: Profile::Cylinder,
        ..symmetric()
    })
    .unwrap();
    assert!(cylinder.current.cd_profile >= 1.0);
}
#[test]
fn geometry_is_cached_for_incidence_and_environment() {
    let mut analysis = Analysis::new(Simulation::default()).unwrap();
    let original_mesh = analysis.mesh.vertices.clone();
    let mut state = analysis.state.clone();
    state.alpha += 3.0;
    analysis.update(&state).unwrap();
    state.planet = Planet::Mars;
    analysis.update(&state).unwrap();
    assert_eq!(analysis.mesh.vertices, original_mesh);
    assert_eq!(analysis.geometry_rebuilds, 1);
    state.thickness += 2.0;
    analysis.update(&state).unwrap();
    assert_eq!(analysis.geometry_rebuilds, 2);
}
#[test]
fn pitot_pressure_is_continuous_and_matches_mach_two_reference() {
    let left = total_pressure_ratio(1.0 - 1e-7, 1.4);
    let right = total_pressure_ratio(1.0 + 1e-7, 1.4);
    assert!((left - right).abs() < 1e-6);
    assert!((total_pressure_ratio(2.0, 1.4) - 5.64044).abs() < 0.0001);
}
#[test]
fn stall_speed_does_not_depend_on_current_angle() {
    let mut analysis = Analysis::new(Simulation::default()).unwrap();
    let first = flight::calculate(&analysis).stall_speed;
    let mut state = analysis.state.clone();
    state.alpha = 0.0;
    analysis.update(&state).unwrap();
    assert_eq!(first, flight::calculate(&analysis).stall_speed);
}
#[test]
fn project_roundtrip_and_invalid_input() {
    let state = Simulation::default();
    assert_eq!(
        interchange::load_project(&interchange::save_project(&state).unwrap())
            .unwrap()
            .0,
        state
    );
    assert!(interchange::load_project(r#"{"chord":0}"#).is_err());
    assert!(interchange::load_project(r#"{"velocity":-20}"#).is_err());
    assert!(interchange::load_project(r#"{"format_version":99,"simulation":{}}"#).is_err());
}
#[test]
fn selig_roundtrip_and_counted_lednicer_with_unequal_surfaces() {
    let analysis = Analysis::new(Simulation::default()).unwrap();
    let exported = interchange::export_dat(&analysis);
    let imported = interchange::import_airfoil(&exported, "wing.dat", &analysis.state).unwrap();
    Analysis::new(imported).unwrap();
    let mut lednicer = String::from("Test\n7 6\n");
    for i in 0..7 {
        let x = i as f64 / 6.0;
        lednicer.push_str(&format!("{x} {}\n", 0.1 * (std::f64::consts::PI * x).sin()));
    }
    for i in 0..6 {
        let x = i as f64 / 5.0;
        lednicer.push_str(&format!(
            "{x} {}\n",
            -0.05 * (std::f64::consts::PI * x).sin()
        ));
    }
    let state = interchange::import_airfoil(&lednicer, "unequal.dat", &analysis.state).unwrap();
    Analysis::new(state).unwrap();
    assert!(
        interchange::import_airfoil("Invalid\nNaN 0\n0 0", "bad.dat", &analysis.state).is_err()
    );
}
#[test]
fn exported_mesh_is_closed_with_consistent_winding() {
    let state = Simulation::default();
    let mesh = WingMesh::generate(&state, &Airfoil::generate(&state));
    let mut edges = std::collections::HashMap::<(usize, usize), (usize, i32)>::new();
    for [a, b, c] in &mesh.triangles {
        for (u, v) in [(*a, *b), (*b, *c), (*c, *a)] {
            let entry = edges.entry((u.min(v), u.max(v))).or_default();
            entry.0 += 1;
            entry.1 += if u < v { 1 } else { -1 };
        }
    }
    assert!(
        edges
            .values()
            .all(|&(count, direction)| count == 2 && direction == 0)
    );
    assert!(interchange::export_stl(&mesh).contains("facet normal"));
}

#[test]
fn mesh_normals_point_outward() {
    let state = Simulation::default();
    let mesh = WingMesh::generate(&state, &Airfoil::generate(&state));
    let signed_volume: f64 = mesh
        .triangles
        .iter()
        .map(|indices| {
            let [a, b, c] = indices.map(|index| mesh.vertices[index]);
            (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum();
    assert!(
        signed_volume > 0.0,
        "STL must describe an outward-oriented solid"
    );
}

#[test]
fn dxf_polyline_roundtrip_and_rejected_arc() {
    let state = Simulation::default();
    let foil = Airfoil::generate(&state);
    let mut text = String::from("0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n");
    for point in foil.contour() {
        text.push_str(&format!("10\n{}\n20\n{}\n", point.x, point.y));
    }
    text.push_str("0\nENDSEC\n0\nEOF\n");
    let imported = interchange::import_airfoil(&text, "profile.dxf", &state).unwrap();
    Analysis::new(imported).unwrap();
    let unsupported = text.replace("0\nLWPOLYLINE\n", "0\nARC\n");
    assert!(interchange::import_airfoil(&unsupported, "arc.dxf", &state).is_err());
}

#[test]
fn legacy_tsfc_is_migrated_with_notice() {
    let (state, notice) =
        interchange::load_project(r#"{"tsfc":0.65,"fuelMass":1500,"aircraftMass":1150}"#).unwrap();
    assert!((state.tsfc - 0.65 / (9.80665 * 3600.0)).abs() < 1e-12);
    assert!(state.fuel_mass < state.aircraft_mass);
    assert!(notice.unwrap().contains("TSFC"));
}

#[test]
fn rejected_update_leaves_last_valid_analysis_intact() {
    let mut analysis = Analysis::new(Simulation::default()).unwrap();
    let original = analysis.state.clone();
    let mut invalid = original.clone();
    invalid.chord = f64::NAN;
    assert!(analysis.update(&invalid).is_err());
    assert_eq!(analysis.state, original);
}
