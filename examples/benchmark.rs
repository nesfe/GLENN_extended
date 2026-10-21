//! Reproducible diagnostic, not a CI timing threshold.
use glenn_extended::{model::Simulation, physics::aerodynamics::Analysis};
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
}
