use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Worker threads that each spin for `duty * period` and sleep for the rest of every period.
pub struct Burner {
    duty: Arc<AtomicU64>,
}

impl Burner {
    pub fn spawn(threads: usize, period: Duration) -> Self {
        let duty = Arc::new(AtomicU64::new(0f64.to_bits()));
        for i in 0..threads {
            let duty = Arc::clone(&duty);
            thread::Builder::new()
                .name(format!("burn-{i}"))
                .spawn(move || run(&duty, period))
                .expect("failed to spawn burner thread");
        }
        Self { duty }
    }

    pub fn set_duty(&self, duty: f64) {
        self.duty
            .store(duty.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn duty(&self) -> f64 {
        f64::from_bits(self.duty.load(Ordering::Relaxed))
    }
}

fn run(duty: &AtomicU64, period: Duration) {
    let mut x = 1.000_001_f64;
    loop {
        let start = Instant::now();
        let busy = period.mul_f64(f64::from_bits(duty.load(Ordering::Relaxed)));
        while start.elapsed() < busy {
            for _ in 0..256 {
                x = black_box(x.mul_add(1.000_001, 1e-9).sqrt() + 0.5);
            }
        }
        thread::sleep(period.saturating_sub(start.elapsed()));
    }
}
