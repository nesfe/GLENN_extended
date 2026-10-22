//! Physical invariants of the field actually consumed by the renderer and probe.
use glam::DVec3;
use glenn_extended::{
    geometry::{Airfoil, SectionFrame},
    model::{Point, Simulation},
    physics::{
        aerodynamics::Analysis,
        flow::{FlowField, SectionGrid},
        panel::PanelModel,
        probe,
    },
};
fn symmetric(alpha: f64) -> Simulation {
    Simulation {
        alpha,
        camber: 0.0,
        twist: 0.0,
        sweep: 0.0,
        dihedral: 0.0,
        ..Simulation::default()
    }
}

#[test]
fn panel_velocity_obeys_no_penetration_and_matches_displayed_pressure() {
    let foil = Airfoil::generate(&symmetric(8.0));
    let panel = PanelModel::prepare(&foil).unwrap();
    let contour = foil.contour();
    let pressure = panel.evaluate(8.0, 0.0);
    for i in 0..contour.len() {
        let a = contour[i];
        let b = contour[(i + 1) % contour.len()];
        let length = (b.x - a.x).hypot(b.y - a.y);
        let normal = Point::new((b.y - a.y) / length, -(b.x - a.x) / length);
        let center = Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
        let velocity = panel.velocity_at(
            Point::new(center.x + normal.x * 1e-7, center.y + normal.y * 1e-7),
            8.0,
        );
        let penetration = velocity.x * normal.x + velocity.y * normal.y;
        assert!(
            penetration.abs() < 0.002,
            "panel {i}: normal speed {penetration}"
        );
        let surface = if i < foil.upper.len() - 1 {
            &pressure.upper
        } else {
            &pressure.lower
        };
        let cp = surface
            .iter()
            .min_by(|a, b| (a.x - center.x).abs().total_cmp(&(b.x - center.x).abs()))
            .unwrap()
            .y;
        let actual = 1.0 - velocity.x.powi(2) - velocity.y.powi(2);
        assert!(
            (actual - cp).abs() < 0.005,
            "panel {i}: Cp {actual} vs {cp}"
        );
    }
}
#[test]
fn incidence_changes_fixed_frame_flow_and_reverses_trailing_vorticity() {
    let mut analysis = Analysis::new(symmetric(8.0)).unwrap();
    let grid = SectionGrid::new(&analysis.foil, &analysis.panel);
    let point = DVec3::new(0.2, 0.3, 0.0) * analysis.state.chord;
    let positive = FlowField {
        analysis: &analysis,
        grid: &grid,
    };
    let velocity = positive.velocity(point);
    let circulation = positive.tip_circulation();
    let tip_offset = DVec3::new(2.0, 0.3, 0.0) * analysis.state.chord;
    let swirl = positive.velocity(positive.tip_origin(1.0) + tip_offset).z;
    let line = positive.trace(DVec3::new(-2.2, 0.3, 0.0) * analysis.state.chord, false);
    analysis.update(&symmetric(-8.0)).unwrap();
    let negative = FlowField {
        analysis: &analysis,
        grid: &grid,
    };
    assert!(velocity.distance(negative.velocity(point)) > 0.15);
    assert!((circulation + negative.tip_circulation()).abs() < 1e-6);
    let reverse_swirl = negative.velocity(negative.tip_origin(1.0) + tip_offset).z;
    assert!(
        swirl * reverse_swirl < 0.0,
        "actual trailing velocity must reverse"
    );
    let other = negative.trace(DVec3::new(-2.2, 0.3, 0.0) * analysis.state.chord, false);
    assert!(
        line.samples[80]
            .position
            .distance(other.samples[80].position)
            > 0.03 * analysis.state.chord
    );
    let far = negative.velocity(DVec3::new(-10000.0, 500.0, 0.0));
    assert!(
        far.distance(DVec3::X) < 0.001,
        "upstream direction must stay fixed: {far}"
    );
}
#[test]
fn streamlines_do_not_cross_wing_and_probe_uses_the_same_velocity() {
    let analysis = Analysis::new(symmetric(12.0)).unwrap();
    let grid = SectionGrid::new(&analysis.foil, &analysis.panel);
    let field = FlowField {
        analysis: &analysis,
        grid: &grid,
    };
    let paths = field.streamlines(1);
    assert!(paths.len() > 25);
    for path in &paths {
        for pair in path.samples.windows(2) {
            for subdivision in 0..=32 {
                let t = subdivision as f64 / 32.0;
                assert!(!field.contains_solid(pair[0].position.lerp(pair[1].position, t)));
            }
            assert!(pair[1].travel_time > pair[0].travel_time);
        }
    }
    let position = [0.1, 0.4, 0.0];
    let reading = probe::sample(&field, position).unwrap();
    let speed = field
        .velocity(DVec3::from_array(position) * analysis.state.chord)
        .length()
        * analysis.state.velocity;
    assert!((reading.speed - speed).abs() < 1e-10);
    let solid = SectionFrame::at(&analysis.state, 0.0).to_world(Point::new(0.3, 0.0))
        / analysis.state.chord;
    assert!(probe::sample(&field, solid.to_array()).is_none());
}
#[test]
fn wake_follows_actual_tip_geometry_and_vanishes_at_zero_lift() {
    let mut analysis = Analysis::new(symmetric(0.0)).unwrap();
    let grid = SectionGrid::new(&analysis.foil, &analysis.panel);
    let field = FlowField {
        analysis: &analysis,
        grid: &grid,
    };
    assert!(field.tip_circulation().abs() < 1e-8);
    assert!(field.streamlines(1).iter().all(|line| !line.vortex));
    let original = field.tip_origin(1.0);
    let mut changed = symmetric(9.0);
    changed.sweep = 30.0;
    changed.dihedral = 8.0;
    changed.twist = -5.0;
    changed.has_winglets = true;
    analysis.update(&changed).unwrap();
    let field = FlowField {
        analysis: &analysis,
        grid: &grid,
    };
    let tip = field.tip_origin(1.0);
    assert!(tip.distance(original) > changed.chord);
    let expected = SectionFrame::at(&changed, changed.span * 0.5)
        .to_world(*analysis.foil.upper.last().unwrap());
    assert!(tip.distance(expected) < 1e-12);
    changed.velocity = 0.0;
    analysis.update(&changed).unwrap();
    let field = FlowField {
        analysis: &analysis,
        grid: &grid,
    };
    assert_eq!(field.tip_circulation(), 0.0);
    assert_eq!(probe::sample(&field, [-2.0, 1.0, 0.0]).unwrap().speed, 0.0);
}

#[test]
fn rotating_cylinder_pressure_and_flow_share_magnus_circulation() {
    use glenn_extended::{model::Profile, physics::panel::cylinder_velocity};
    let foil = Airfoil::generate(&Simulation {
        profile_type: Profile::Cylinder,
        ..symmetric(0.0)
    });
    let panel = PanelModel::prepare(&foil).unwrap();
    for spin in [-1.0, 0.0, 1.0] {
        let pressure = panel.cylinder_pressure(0.0, spin);
        assert!((pressure.lift - std::f64::consts::TAU * spin).abs() < 0.02);
        let top = cylinder_velocity(Point::new(0.5, 0.5), 0.0, spin);
        assert!(top.y.abs() < 1e-10);
        assert!((top.x - (2.0 + spin)).abs() < 1e-10);
    }
}

#[test]
fn profile_thickness_and_flap_change_the_computed_flow() {
    let base = Analysis::new(symmetric(6.0)).unwrap();
    let base_grid = SectionGrid::new(&base.foil, &base.panel);
    let position = DVec3::new(0.2, 0.25, 0.0) * base.state.chord;
    let reference = FlowField {
        analysis: &base,
        grid: &base_grid,
    }
    .velocity(position);
    for change in 0..3 {
        let mut state = base.state.clone();
        match change {
            0 => state.thickness = 22.0,
            1 => state.camber = 6.0,
            _ => state.flap_angle = 20.0,
        }
        let analysis = Analysis::new(state).unwrap();
        let grid = SectionGrid::new(&analysis.foil, &analysis.panel);
        let field = FlowField {
            analysis: &analysis,
            grid: &grid,
        };
        assert!(
            field.velocity(position).distance(reference) > 0.02,
            "geometry case {change}"
        );
    }
}
