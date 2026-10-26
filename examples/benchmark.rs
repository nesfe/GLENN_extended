//! Reproducible diagnostic, not a CI timing threshold.
use glenn_extended::{
    model::Simulation,
    physics::{
        aerodynamics::Analysis,
        flow::{FlowField, SectionGrid},
    },
};
use std::{hint::black_box, time::Instant};
fn main() {
    let state = Simulation::default();
    let started = Instant::now();
    for _ in 0..100 {
        black_box(Analysis::new(state.clone()).unwrap());
    }
    let cold = started.elapsed().as_secs_f64() * 10.0;
    let mut analysis = Analysis::new(state.clone()).unwrap();
    let started = Instant::now();
    for step in 0..1000 {
        let mut sample = state.clone();
        sample.alpha = (step % 75) as f64 - 30.0;
        analysis.update(&sample).unwrap();
        black_box(&analysis.current);
    }
    println!("Full geometry + 76-angle polar: {cold:.3} ms/iteration");
    println!(
        "Cached incidence update: {:.3} ms/iteration",
        started.elapsed().as_secs_f64()
    );
    println!(
        "Geometry factorizations across 1000 updates: {}",
        analysis.geometry_rebuilds
    );

    // Tunnel stages. The basis grid is rebuilt only when the section shape
    // changes; tracing runs whenever an input the field reads moves. Both are
    // spread across cores, so these numbers depend on the machine.
    let started = Instant::now();
    for _ in 0..5 {
        black_box(SectionGrid::new(&analysis.foil, &analysis.panel));
    }
    println!(
        "Velocity basis grid, per section change: {:.3} ms",
        started.elapsed().as_secs_f64() * 200.0
    );
    let grid = SectionGrid::new(&analysis.foil, &analysis.panel);
    for density in 1..=5 {
        let started = Instant::now();
        let mut lines = 0;
        for _ in 0..5 {
            let paths = FlowField {
                analysis: &analysis,
                grid: &grid,
            }
            .streamlines(density);
            lines = paths.len();
            black_box(paths);
        }
        println!(
            "Streamlines at density {density}: {:.3} ms, {lines} lines",
            started.elapsed().as_secs_f64() * 200.0
        );
    }
}
