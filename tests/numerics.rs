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
fn center_of_pressure_is_a_section_property() {
    // x_cp follows from the pressure distribution the section view draws.
    // Changing the planform changes the wing's lift coefficient but cannot move
    // where that section's own resultant acts, so the marker must hold still.
    let state = Simulation {
        alpha: 5.0,
        twist: 0.0,
        sweep: 0.0,
        ..Simulation::default()
    };
    let reference = Analysis::new(state.clone()).unwrap().current;
    for (name, change) in [
        (
            "span",
            (|s: &mut Simulation| s.span = 34.0) as fn(&mut Simulation),
        ),
        ("taper", |s| s.taper = 0.22),
        ("winglets", |s| s.has_winglets = true),
    ] {
        let mut sample = state.clone();
        change(&mut sample);
        let moved = Analysis::new(sample).unwrap().current;
        assert!(
            (moved.cl - reference.cl).abs() > 1e-4,
            "{name} should change the wing lift coefficient"
        );
        assert!(
            (moved.center_of_pressure - reference.center_of_pressure).abs() < 1e-12,
            "{name} moved x_cp from {} to {}",
            reference.center_of_pressure,
            moved.center_of_pressure
        );
    }
    // A cambered section at positive incidence carries its load behind the
    // leading edge and ahead of mid-chord.
    assert!(
        (0.25..0.5).contains(&reference.center_of_pressure),
        "x_cp {}",
        reference.center_of_pressure
    );
    // Symmetric and unloaded: the formula must not divide by a vanishing lift.
    let unloaded = Analysis::new(symmetric()).unwrap().current;
    assert_eq!(unloaded.center_of_pressure, 0.25);
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

/// Thickness and camber of an imported profile, sampled on a common grid.
fn shape(state: &Simulation) -> Vec<(f64, f64)> {
    let foil = Airfoil::generate(state);
    (0..=40)
        .map(|i| {
            let x = i as f64 / 40.0;
            let upper = glenn_extended::geometry::interpolate(&foil.upper, x);
            let lower = glenn_extended::geometry::interpolate(&foil.lower, x);
            (upper - lower, (upper + lower) * 0.5)
        })
        .collect()
}

#[test]
fn dxf_import_handles_the_shapes_real_cad_exports() {
    let state = Simulation::default();
    let contour = Airfoil::generate(&state).contour();
    // Reference is the same contour taken through the DAT path. Both routes
    // resample onto the solver's cosine stations, so any difference left is
    // the DXF parser's own and not an artefact of resampling.
    let reference = shape(
        &interchange::import_airfoil(
            &contour
                .iter()
                .map(|p| format!("{} {}\n", p.x, p.y))
                .collect::<String>(),
            "reference.dat",
            &state,
        )
        .unwrap(),
    );
    let vertices = |points: &[glenn_extended::model::Point]| {
        points
            .iter()
            .map(|p| format!("10\n{}\n20\n{}\n30\n0.0\n", p.x, p.y))
            .collect::<String>()
    };
    let section = |body: String| format!("0\nSECTION\n2\nENTITIES\n{body}0\nENDSEC\n0\nEOF\n");
    let lwpolyline = |points: &[glenn_extended::model::Point]| {
        format!(
            "0\nLWPOLYLINE\n5\n1F2\n100\nAcDbEntity\n8\nairfoil\n100\nAcDbPolyline\n\
             90\n{}\n70\n1\n43\n0.0\n{}210\n0.0\n220\n0.0\n230\n1.0\n",
            points.len(),
            vertices(points)
        )
    };
    let reversed: Vec<_> = contour.iter().rev().copied().collect();
    let rotated: Vec<_> = contour[contour.len() / 3..]
        .iter()
        .chain(&contour[..contour.len() / 3])
        .copied()
        .collect();
    // Millimetre drawing offset away from the origin, as a CAD sheet would be.
    let scaled: Vec<_> = contour
        .iter()
        .map(|p| glenn_extended::model::Point::new(p.x * 250.0 + 1200.0, p.y * 250.0 - 430.0))
        .collect();
    let old_style = format!(
        "0\nPOLYLINE\n8\n0\n66\n1\n70\n1\n10\n0.0\n20\n0.0\n30\n0.0\n{}0\nSEQEND\n8\n0\n",
        contour
            .iter()
            .map(|p| format!("0\nVERTEX\n8\n0\n10\n{}\n20\n{}\n30\n0.0\n", p.x, p.y))
            .collect::<String>()
    );
    // A SPLINE through the contour as fit points: what most CAD writes.
    let spline = format!(
        "0\nSPLINE\n8\n0\n70\n8\n71\n3\n74\n{}\n{}",
        contour.len(),
        contour
            .iter()
            .map(|p| format!("11\n{}\n21\n{}\n31\n0.0\n", p.x, p.y))
            .collect::<String>()
    );
    // Clamped uniform degree-3 spline given by control points only.
    let control = format!(
        "0\nSPLINE\n8\n0\n70\n8\n71\n3\n73\n{}\n{}",
        contour.len(),
        vertices(&contour)
    );
    let noise = "0\nLINE\n8\nconstruction\n10\n0.0\n20\n0.0\n11\n1.0\n21\n0.0\n\
                 0\nTEXT\n8\nnotes\n10\n0.2\n20\n0.3\n40\n0.05\n1\nNACA 2412\n\
                 0\nPOINT\n8\n0\n10\n0.5\n20\n0.0\n30\n0.0\n";
    // A small closed box must not win over the profile contour.
    let flap = "0\nLWPOLYLINE\n90\n4\n70\n1\n10\n1.1\n20\n0.0\n10\n1.4\n20\n0.02\n\
                10\n1.4\n20\n-0.02\n10\n1.1\n20\n-0.01\n";

    for (name, text, tolerance) in [
        ("modern lwpolyline", section(lwpolyline(&contour)), 1e-9),
        ("reversed winding", section(lwpolyline(&reversed)), 1e-9),
        ("rotated start point", section(lwpolyline(&rotated)), 1e-9),
        ("millimetres, offset", section(lwpolyline(&scaled)), 1e-9),
        ("r12 polyline/vertex", section(old_style), 1e-9),
        ("spline fit points", section(spline), 1e-9),
        ("spline control points", section(control), 0.004),
        (
            "drawing with construction geometry",
            section(lwpolyline(&contour) + noise),
            1e-9,
        ),
        (
            "two contours, profile is the wider",
            section(lwpolyline(&contour) + flap),
            1e-9,
        ),
        (
            "blank padding and CRLF",
            format!(
                "\n{}\n",
                section(lwpolyline(&contour)).replace('\n', "\r\n")
            ),
            1e-9,
        ),
    ] {
        let imported = interchange::import_airfoil(&text, "profile.dxf", &state)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(imported.profile_type, Profile::Custom, "{name}");
        let analysis = Analysis::new(imported.clone())
            .unwrap_or_else(|error| panic!("{name} solver: {error}"));
        assert!(analysis.current.cl.is_finite(), "{name}");
        for (station, (actual, expected)) in shape(&imported).iter().zip(&reference).enumerate() {
            assert!(
                (actual.0 - expected.0).abs() < tolerance
                    && (actual.1 - expected.1).abs() < tolerance,
                "{name} at station {station}: thickness {} vs {}, camber {} vs {}",
                actual.0,
                expected.0,
                actual.1,
                expected.1
            );
        }
    }

    // A drawing with no contour at all must say so instead of inventing one.
    assert!(interchange::import_airfoil(&section(noise.into()), "lines.dxf", &state).is_err());
    assert!(
        interchange::import_airfoil("AutoCAD Binary DXF\0\0", "bin.dxf", &state)
            .is_err_and(|error| error.contains("двоичном"))
    );
    assert!(
        interchange::import_airfoil("0\nSECTION\nnot-a-code\n7\n", "bad.dxf", &state)
            .is_err_and(|error| error.contains("группового кода"))
    );
}

#[test]
fn import_is_invariant_to_drawing_scale_and_position() {
    // A CAD sheet puts the profile wherever it likes, in whatever unit. Chord
    // and camber are measured from the leading edge, so the imported shape
    // must not depend on where the drawing sits.
    let state = Simulation::default();
    let contour = Airfoil::generate(&state).contour();
    let selig = |points: &[glenn_extended::model::Point]| {
        points
            .iter()
            .map(|p| format!("{} {}\n", p.x, p.y))
            .collect::<String>()
    };
    let reference = interchange::import_airfoil(&selig(&contour), "ref.dat", &state).unwrap();
    for (scale, dx, dy) in [
        (1.0, 0.0, 0.0),
        (250.0, 1200.0, -430.0),
        (0.04, -9.5, 61.0),
        (1000.0, 0.0, 250_000.0),
    ] {
        let moved: Vec<_> = contour
            .iter()
            .map(|p| glenn_extended::model::Point::new(p.x * scale + dx, p.y * scale + dy))
            .collect();
        let imported = interchange::import_airfoil(&selig(&moved), "moved.dat", &state)
            .unwrap_or_else(|error| panic!("scale {scale} offset ({dx}, {dy}): {error}"));
        for (surface, expected) in [
            (&imported.custom_upper, &reference.custom_upper),
            (&imported.custom_lower, &reference.custom_lower),
        ] {
            let surface = surface.as_ref().unwrap();
            let expected = expected.as_ref().unwrap();
            assert_eq!(surface.len(), expected.len(), "scale {scale}");
            for (actual, expected) in surface.iter().zip(expected) {
                assert!(
                    (actual.x - expected.x).abs() < 1e-9 && (actual.y - expected.y).abs() < 1e-9,
                    "scale {scale} offset ({dx}, {dy}): ({}, {}) vs ({}, {})",
                    actual.x,
                    actual.y,
                    expected.x,
                    expected.y
                );
            }
        }
    }
}

#[test]
fn both_imported_surfaces_span_the_whole_chord() {
    // The trailing edge belongs to both surfaces whatever point the file
    // starts from, or one surface ends short and its chord-end pressure comes
    // from a clamped extrapolation instead of the trailing edge itself.
    let state = Simulation::default();
    let contour = Airfoil::generate(&state).contour();
    for start in [
        0,
        1,
        7,
        contour.len() / 4,
        contour.len() / 2,
        contour.len() - 1,
    ] {
        for reverse in [false, true] {
            let mut points: Vec<_> = contour[start..]
                .iter()
                .chain(&contour[..start])
                .copied()
                .collect();
            if reverse {
                points.reverse();
            }
            let text: String = points
                .iter()
                .map(|p| format!("{} {}\n", p.x, p.y))
                .collect();
            let imported = interchange::import_airfoil(&text, "start.dat", &state)
                .unwrap_or_else(|error| panic!("start {start} reverse {reverse}: {error}"));
            for surface in [&imported.custom_upper, &imported.custom_lower] {
                let surface = surface.as_ref().unwrap();
                assert!(
                    surface[0].x.abs() < 1e-9,
                    "start {start} reverse {reverse}: begins at {}",
                    surface[0].x
                );
                assert!(
                    (surface.last().unwrap().x - 1.0).abs() < 1e-9,
                    "start {start} reverse {reverse}: ends at {}",
                    surface.last().unwrap().x
                );
            }
        }
    }
}

#[test]
fn dxf_bulge_arcs_are_expanded_into_the_contour() {
    // A rounded leading edge drawn as a bulge segment must become geometry,
    // not a rejection: the chord and thickness have to survive the arc.
    let state = Simulation::default();
    let contour = Airfoil::generate(&state).contour();
    let nose = contour
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.x.total_cmp(&b.x))
        .unwrap()
        .0;
    let mut body = format!("0\nLWPOLYLINE\n90\n{}\n70\n1\n", contour.len());
    for (i, point) in contour.iter().enumerate() {
        body.push_str(&format!("10\n{}\n20\n{}\n", point.x, point.y));
        // Replace the two segments across the nose with a single arc.
        if i == nose {
            body.push_str("42\n-0.42\n");
        }
    }
    let text = format!("0\nSECTION\n2\nENTITIES\n{body}0\nENDSEC\n0\nEOF\n");
    let imported = interchange::import_airfoil(&text, "bulge.dxf", &state).unwrap();
    let upper = imported.custom_upper.as_ref().unwrap();
    let lower = imported.custom_lower.as_ref().unwrap();
    assert!(upper.len() > contour.len() / 2, "arc produced no points");
    assert!((upper[0].x - 0.0).abs() < 1e-12 && (upper.last().unwrap().x - 1.0).abs() < 1e-12);
    let thickness = upper
        .iter()
        .zip(lower)
        .map(|(u, l)| u.y - l.y)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (thickness - state.thickness / 100.0).abs() < 0.01,
        "thickness {thickness}"
    );
    Analysis::new(imported).unwrap();
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
