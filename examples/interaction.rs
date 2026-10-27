//! Diagnostic for request submission under a slider burst; not a frame-rate test.
use glenn_extended::{
    engine::Worker,
    model::Simulation,
    physics::{
        aerodynamics::Analysis,
        flow::{FlowField, SectionGrid},
    },
};
use std::time::{Duration, Instant};
fn main() {
    let mut analysis = Analysis::new(Simulation::default()).unwrap();
    let grid = SectionGrid::new(&analysis.foil, &analysis.panel);
    let worker = Worker::new(move |state: Simulation, token| {
        analysis.update(&state)?;
        let paths = FlowField {
            analysis: &analysis,
            grid: &grid,
        }
        .streamlines_cancellable(3, &|| token.is_cancelled())
        .ok_or("Cancelled")?;
        Ok((
            analysis.state.alpha,
            paths.iter().map(|p| p.samples.len()).sum::<usize>(),
        ))
    });
    let mut worst = Duration::ZERO;
    let started = Instant::now();
    for i in 0..200 {
        let state = Simulation {
            alpha: (i % 20) as f64,
            ..Simulation::default()
        };
        let tick = Instant::now();
        worker.submit(state);
        worst = worst.max(tick.elapsed());
    }
    let submitted = started.elapsed();
    loop {
        if let Some((_, result)) = worker.poll() {
            let (alpha, samples) = result.unwrap();
            assert_eq!(alpha, 19.0);
            println!(
                "200 submissions: {:.3} ms; worst submission: {:.3} ms; latest result: {:.3} ms; {samples} samples",
                submitted.as_secs_f64() * 1000.0,
                worst.as_secs_f64() * 1000.0,
                started.elapsed().as_secs_f64() * 1000.0
            );
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(1));
    }
}
