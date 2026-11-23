mod burner;
mod stats;

use anyhow::{Result, bail};
use burner::Burner;
use clap::Parser;
use log::{debug, info, warn};
use stats::Usage;
use std::thread;
use std::time::{Duration, Instant};

const CORRECTION_GAIN: f64 = 0.3;
const MAX_CORRECTION: f64 = 0.1;

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Keeps total CPU utilization at or above a floor by burning only the missing amount"
)]
struct Args {
    #[arg(
        long,
        env = "CPU_FLOOR_TARGET",
        default_value_t = 25.0,
        help = "Minimum total CPU utilization to maintain, in percent"
    )]
    target: f64,

    #[arg(
        long,
        env = "CPU_FLOOR_INTERVAL_MS",
        default_value_t = 1000,
        help = "How often to measure and adjust, in ms"
    )]
    interval_ms: u64,

    #[arg(
        long,
        env = "CPU_FLOOR_PERIOD_MS",
        default_value_t = 100,
        help = "Burn/sleep cycle length per worker, in ms"
    )]
    period_ms: u64,

    #[arg(
        long,
        env = "CPU_FLOOR_THREADS",
        help = "Number of burner threads [default: one per CPU]"
    )]
    threads: Option<usize>,

    #[arg(
        long,
        env = "CPU_FLOOR_REPORT_SECS",
        default_value_t = 60,
        help = "Seconds between status log lines (0 disables)"
    )]
    report_secs: u64,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();
    let args = Args::parse();
    if !(args.target > 0.0 && args.target <= 100.0) {
        bail!("--target must be in (0, 100]");
    }
    if args.interval_ms == 0 || args.period_ms == 0 {
        bail!("--interval-ms and --period-ms must be positive");
    }

    lower_priority();

    let cpus = stats::cpu_count()?;
    let threads = args.threads.unwrap_or(cpus).max(1);
    let target = args.target / 100.0;
    let capacity = threads.min(cpus) as f64 / cpus as f64;
    info!(
        "target {:.1}% across {cpus} CPUs with {threads} burner threads",
        args.target
    );

    let burner = Burner::spawn(threads, Duration::from_millis(args.period_ms));
    let mut controller = Controller::new(target, capacity);
    let mut report = Report::default();
    let interval = Duration::from_millis(args.interval_ms);
    let report_every = Duration::from_secs(args.report_secs);
    let mut prev = stats::sample()?;

    loop {
        thread::sleep(interval);
        let cur = stats::sample()?;
        let Some(usage) = Usage::between(&prev, &cur) else {
            continue;
        };
        prev = cur;

        let duty = controller.next_duty(&usage);
        burner.set_duty(duty);
        debug!(
            "system {:5.1}%  others {:5.1}%  own {:5.1}%  duty {:5.1}%",
            usage.system * 100.0,
            usage.others() * 100.0,
            usage.own * 100.0,
            burner.duty() * 100.0
        );

        report.record(&usage, target);
        if !report_every.is_zero() && report.started.elapsed() >= report_every {
            report.log(burner.duty());
            report = Report::default();
        }
    }
}

/// Feed-forward on the load from other processes, plus a bounded integral term that
/// absorbs the gap between the requested duty and the CPU time the burners actually get.
struct Controller {
    target: f64,
    capacity: f64,
    correction: f64,
}

impl Controller {
    fn new(target: f64, capacity: f64) -> Self {
        Self {
            target,
            capacity,
            correction: 0.0,
        }
    }

    fn next_duty(&mut self, usage: &Usage) -> f64 {
        let deficit = self.target - usage.others();
        if deficit > 0.0 {
            let error = self.target - usage.system;
            self.correction =
                (self.correction + CORRECTION_GAIN * error).clamp(-MAX_CORRECTION, MAX_CORRECTION);
        }
        ((deficit + self.correction).max(0.0) / self.capacity).clamp(0.0, 1.0)
    }
}

struct Report {
    started: Instant,
    ticks: u32,
    below: u32,
    min_system: f64,
    sum_system: f64,
    sum_own: f64,
}

impl Default for Report {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            ticks: 0,
            below: 0,
            min_system: f64::INFINITY,
            sum_system: 0.0,
            sum_own: 0.0,
        }
    }
}

impl Report {
    fn record(&mut self, usage: &Usage, target: f64) {
        self.ticks += 1;
        self.below += u32::from(usage.system < target);
        self.min_system = self.min_system.min(usage.system);
        self.sum_system += usage.system;
        self.sum_own += usage.own;
    }

    fn log(&self, duty: f64) {
        if self.ticks == 0 {
            return;
        }
        let n = f64::from(self.ticks);
        info!(
            "avg system {:.1}% (min {:.1}%)  avg own {:.1}%  duty {:.1}%  below target {}/{} ticks",
            self.sum_system / n * 100.0,
            self.min_system * 100.0,
            self.sum_own / n * 100.0,
            duty * 100.0,
            self.below,
            self.ticks
        );
    }
}

/// Runs the whole process under SCHED_IDLE so any real workload preempts the burners.
/// Uses raw syscalls because musl's sched_* wrappers always fail with ENOSYS.
#[cfg(target_os = "linux")]
fn lower_priority() {
    let param: libc::sched_param = unsafe { std::mem::zeroed() };
    let current = unsafe { libc::syscall(libc::SYS_sched_getscheduler, 0) };
    if current != libc::c_long::from(libc::SCHED_IDLE)
        && unsafe {
            libc::syscall(
                libc::SYS_sched_setscheduler,
                0,
                libc::SCHED_IDLE,
                &raw const param,
            )
        } != 0
    {
        warn!(
            "could not switch to SCHED_IDLE: {}",
            std::io::Error::last_os_error()
        );
    }
    if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 19) } != 0 {
        warn!("could not set nice 19: {}", std::io::Error::last_os_error());
    }
}

#[cfg(not(target_os = "linux"))]
fn lower_priority() {
    warn!("not running on Linux; priority unchanged");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(system: f64, own: f64) -> Usage {
        Usage { system, own }
    }

    #[test]
    fn fills_only_the_missing_load() {
        let mut c = Controller::new(0.25, 1.0);
        let duty = c.next_duty(&usage(0.10, 0.0));
        assert!(duty > 0.15 && duty < 0.25, "duty {duty}");
    }

    #[test]
    fn idles_when_others_cover_the_target() {
        let mut c = Controller::new(0.25, 1.0);
        for _ in 0..50 {
            assert_eq!(c.next_duty(&usage(0.60, 0.0)), 0.0);
        }
        assert_eq!(c.correction, 0.0);
    }

    #[test]
    fn scales_by_burner_capacity() {
        let mut c = Controller::new(0.25, 0.5);
        let duty = c.next_duty(&usage(0.25, 0.25));
        assert!((duty - 0.5).abs() < 1e-9, "duty {duty}");
    }

    #[test]
    fn correction_is_bounded() {
        let mut c = Controller::new(0.25, 1.0);
        for _ in 0..100 {
            c.next_duty(&usage(0.0, 0.0));
        }
        assert!((c.correction - MAX_CORRECTION).abs() < 1e-9);
    }
}
