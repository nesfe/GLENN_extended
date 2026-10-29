//! One persistent worker, bounded snapshots, latest-request-wins publication.
use crate::{Config, Snapshot, Solver};
use std::sync::{Arc, Condvar, Mutex};
type Output = Option<(u64, Result<Arc<Snapshot>, String>)>;
#[derive(Default)]
struct Control {
    config: Option<Config>,
    generation: u64,
    paused: bool,
    stop: bool,
}
pub struct Worker {
    control: Arc<(Mutex<Control>, Condvar)>,
    output: Arc<Mutex<Output>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Default for Worker {
    fn default() -> Self {
        let control = Arc::new((Mutex::new(Control::default()), Condvar::new()));
        let output = Arc::new(Mutex::new(None));
        let shared = control.clone();
        let published = output.clone();
        let thread = std::thread::spawn(move || {
            let mut generation = 0;
            let mut solver = None;
            loop {
                let (lock, wake) = &*shared;
                let mut c = lock.lock().unwrap();
                while !c.stop && (c.config.is_none() || (c.paused && c.generation == generation)) {
                    c = wake.wait(c).unwrap();
                }
                if c.stop {
                    break;
                }
                let requested = if generation != c.generation {
                    generation = c.generation;
                    c.config.clone()
                } else {
                    None
                };
                drop(c);
                // Rasterizing the body must never hold the UI command lock.
                if let Some(config) = requested {
                    solver = Some(Solver::new(config));
                }
                let result = match solver.as_mut().unwrap() {
                    Ok(s) => s.advance(24).map(|()| Arc::new(s.snapshot())),
                    Err(e) => Err(e.clone()),
                };
                let mut c = lock.lock().unwrap();
                if c.generation == generation {
                    if result.is_err() {
                        c.paused = true;
                    }
                    *published.lock().unwrap() = Some((generation, result));
                }
            }
        });
        Self {
            control,
            output,
            thread: Some(thread),
        }
    }
}
impl Worker {
    pub fn submit(&self, config: Config) -> u64 {
        let (lock, wake) = &*self.control;
        let mut c = lock.lock().unwrap();
        c.generation += 1;
        c.config = Some(config);
        c.paused = false;
        *self.output.lock().unwrap() = None;
        wake.notify_one();
        c.generation
    }
    pub fn pause(&self, paused: bool) {
        let (lock, wake) = &*self.control;
        lock.lock().unwrap().paused = paused;
        wake.notify_one();
    }
    pub fn poll(&self, generation: u64) -> Option<Result<Arc<Snapshot>, String>> {
        self.output
            .lock()
            .unwrap()
            .take()
            .and_then(|(id, r)| (id == generation).then_some(r))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.control;
        lock.lock().unwrap().stop = true;
        wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_new_generation_never_publishes_an_old_field() {
        let worker = Worker::default();
        let mut config = Config {
            chord_cells: 24,
            reynolds: 100.0,
            alpha: 0.0,
            contour: vec![],
            mach: 0.0,
            gamma: 1.4,
        };
        worker.submit(config.clone());
        config.alpha = 30.0;
        let latest = worker.submit(config);
        let start = std::time::Instant::now();
        loop {
            if let Some(result) = worker.poll(latest) {
                assert!(result.unwrap().step >= 24);
                break;
            }
            assert!(start.elapsed().as_secs() < 10);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        worker.pause(true);
    }
}
