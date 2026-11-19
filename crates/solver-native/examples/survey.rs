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
    // Optional common convective end time for comparing different cell sizes.
    if let Some(end) = args.get(6).and_then(|s| s.parse::<f64>().ok()) {
        assert!(end.is_finite() && end > 0.0);
        let mut previous = 0.0_f64;
        let mut integral = [0.0; 2];
        let mut weight = 0.0;
        loop {
            solver.advance(5).unwrap();
            let f = solver.snapshot();
            let dt = (f.time.min(end) - previous.max(end * 0.75)).max(0.0);
            weight += dt;
            integral[0] += f64::from(f.lift) * dt;
            integral[1] += f64::from(f.drag) * dt;
            if f.time >= end {
                println!(
                    "N={cells} end={end} step={} cl_mean={:.6} cd_mean={:.6} minUx={:.5}",
                    f.step,
                    integral[0] / weight,
                    integral[1] / weight,
                    f.min_ux
                );
                break;
            }
            previous = f.time;
        }
        return;
    }
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
