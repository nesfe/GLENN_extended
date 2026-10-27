//! UI-independent, bounded latest-request runtime. One running and one pending
//! request; new input cancels old work, and stale results cannot be published.
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[derive(Clone)]
pub struct Cancellation {
    generation: Arc<AtomicU64>,
    expected: u64,
    shutdown: Arc<AtomicBool>,
}
impl Cancellation {
    pub fn is_cancelled(&self) -> bool {
        self.shutdown.load(Ordering::Relaxed)
            || self.generation.load(Ordering::Acquire) != self.expected
    }
    pub fn check(&self) -> Result<(), String> {
        if self.is_cancelled() {
            Err("Расчёт отменён".into())
        } else {
            Ok(())
        }
    }
}
struct State<I, O> {
    pending: Option<(u64, I)>,
    completed: Option<(u64, Result<O, String>)>,
}
/// Dropping the UI handle signals cancellation without joining on the UI thread.
/// A backend must check its token between bounded units of work.
pub struct Worker<I, O> {
    shared: Arc<(Mutex<State<I, O>>, Condvar)>,
    generation: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
}
impl<I: Send + 'static, O: Send + 'static> Worker<I, O> {
    pub fn new(
        mut compute: impl FnMut(I, &Cancellation) -> Result<O, String> + Send + 'static,
    ) -> Self {
        let shared = Arc::new((
            Mutex::new(State {
                pending: None,
                completed: None,
            }),
            Condvar::new(),
        ));
        let generation = Arc::new(AtomicU64::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));
        let (thread_shared, thread_generation, thread_shutdown) =
            (shared.clone(), generation.clone(), shutdown.clone());
        std::thread::spawn(move || {
            loop {
                let (lock, wake) = &*thread_shared;
                let mut state = lock.lock().unwrap();
                while state.pending.is_none() && !thread_shutdown.load(Ordering::Relaxed) {
                    state = wake.wait(state).unwrap();
                }
                if thread_shutdown.load(Ordering::Relaxed) {
                    break;
                }
                let (id, input) = state.pending.take().unwrap();
                drop(state);
                let token = Cancellation {
                    generation: thread_generation.clone(),
                    expected: id,
                    shutdown: thread_shutdown.clone(),
                };
                let result = compute(input, &token);
                let mut state = lock.lock().unwrap();
                if !token.is_cancelled() {
                    state.completed = Some((id, result));
                }
            }
        });
        Self {
            shared,
            generation,
            shutdown,
        }
    }
    pub fn submit(&self, input: I) -> u64 {
        let (lock, wake) = &*self.shared;
        let mut state = lock.lock().unwrap();
        let id = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        state.pending = Some((id, input));
        state.completed = None;
        wake.notify_one();
        id
    }
    pub fn poll(&self) -> Option<(u64, Result<O, String>)> {
        self.shared.0.lock().unwrap().completed.take()
    }
    pub fn cancel(&self) {
        let mut state = self.shared.0.lock().unwrap();
        self.generation.fetch_add(1, Ordering::AcqRel);
        state.pending = None;
        state.completed = None;
    }
}
impl<I, O> Drop for Worker<I, O> {
    fn drop(&mut self) {
        // Lock pairs with the worker's condvar wait to avoid a lost wakeup.
        let _state = self.shared.0.lock().unwrap();
        self.shutdown.store(true, Ordering::Release);
        self.shared.1.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    #[test]
    fn stale_work_is_cancelled_and_only_latest_request_runs() {
        let (started, wait_started) = mpsc::channel();
        let (release, wait_release) = mpsc::channel();
        let worker = Worker::new(move |input: u32, token| {
            started.send(input).unwrap();
            if input == 1 {
                wait_release.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            token.check()?;
            Ok(input)
        });
        worker.submit(1);
        assert_eq!(
            wait_started.recv_timeout(Duration::from_secs(5)).unwrap(),
            1
        );
        worker.submit(2);
        worker.submit(3);
        release.send(()).unwrap();
        assert_eq!(
            wait_started.recv_timeout(Duration::from_secs(5)).unwrap(),
            3
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some((id, result)) = worker.poll() {
                assert_eq!((id, result.unwrap()), (3, 3));
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(wait_started.try_recv().is_err());
    }
}
