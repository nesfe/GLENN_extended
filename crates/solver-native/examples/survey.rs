use glenn_core::{geometry::Airfoil, model::Simulation};
use glenn_solver_native::{Config, Solver};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let alpha = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(24.0);
    let re = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1e6);
    let cells = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(40);
    let state = Simulation {
        camber: 0.0,
        ..Simulation::default()
    };
    let mut solver = Solver::new(Config::gas_section(
        &Airfoil::generate(&state),
        alpha,
        re,
        cells,
        args.get(5).and_then(|v| v.parse().ok()).unwrap_or(0.0),
        1.4,
    ))
    .unwrap();
    let start = std::time::Instant::now();
    let batches = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(12);
    for _ in 0..batches {
        solver.advance(500).unwrap();
        let s = solver.snapshot();
        println!(
            "step={} t={:.2} seconds={:.3} minUx={:.3} cl={:.3} cd={:.3} rho={:.5}",
            s.step,
            s.time,
            start.elapsed().as_secs_f64(),
            s.min_ux,
            s.lift,
            s.drag,
            s.mean_density
        );
    }
}
