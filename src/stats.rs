use anyhow::{Context, Result, bail};
use std::fs;

/// Cumulative CPU counters in USER_HZ ticks, summed over all CPUs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub total: u64,
    pub busy: u64,
    pub own: u64,
}

/// CPU utilization over an interval, as fractions (0.0..=1.0) of the whole machine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Usage {
    pub system: f64,
    pub own: f64,
}

impl Usage {
    pub fn others(&self) -> f64 {
        (self.system - self.own).max(0.0)
    }

    /// Returns `None` when no time has elapsed between the samples.
    pub fn between(prev: &Sample, cur: &Sample) -> Option<Self> {
        let total = cur.total.checked_sub(prev.total).filter(|&t| t > 0)? as f64;
        let busy = cur.busy.saturating_sub(prev.busy) as f64;
        let own = cur.own.saturating_sub(prev.own) as f64;
        let system = (busy / total).clamp(0.0, 1.0);
        Some(Self {
            system,
            own: (own / total).clamp(0.0, system),
        })
    }
}

pub fn sample() -> Result<Sample> {
    let stat = fs::read_to_string("/proc/stat").context("reading /proc/stat")?;
    let own = fs::read_to_string("/proc/self/stat").context("reading /proc/self/stat")?;
    let (total, busy) = parse_system(&stat)?;
    Ok(Sample {
        total,
        busy,
        own: parse_own(&own)?,
    })
}

pub fn cpu_count() -> Result<usize> {
    let stat = fs::read_to_string("/proc/stat").context("reading /proc/stat")?;
    Ok(parse_cpu_count(&stat))
}

/// Steal time counts toward the total but not toward busy, so utilization is never overestimated.
fn parse_system(stat: &str) -> Result<(u64, u64)> {
    let line = stat
        .lines()
        .find(|l| l.starts_with("cpu "))
        .context("no aggregate cpu line in /proc/stat")?;
    let fields = line
        .split_whitespace()
        .skip(1)
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .context("malformed cpu line in /proc/stat")?;
    let [user, nice, system, idle, iowait, irq, softirq, steal, ..] = fields[..] else {
        bail!(
            "expected at least 8 fields in /proc/stat cpu line, got {}",
            fields.len()
        );
    };
    let busy = user + nice + system + irq + softirq;
    Ok((busy + idle + iowait + steal, busy))
}

/// Returns utime + stime of the current process.
fn parse_own(stat: &str) -> Result<u64> {
    let after_comm = stat
        .rsplit_once(')')
        .context("malformed /proc/self/stat")?
        .1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    let tick = |i: usize| -> Result<u64> {
        fields
            .get(i)
            .context("truncated /proc/self/stat")?
            .parse()
            .context("malformed /proc/self/stat")
    };
    Ok(tick(11)? + tick(12)?)
}

fn parse_cpu_count(stat: &str) -> usize {
    stat.lines()
        .filter(|l| {
            l.strip_prefix("cpu")
                .and_then(|rest| rest.split(char::is_whitespace).next())
                .is_some_and(|id| id.parse::<u32>().is_ok())
        })
        .count()
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "\
cpu  100 10 50 800 20 5 5 10 0 0
cpu0 50 5 25 400 10 3 2 5 0 0
cpu1 50 5 25 400 10 2 3 5 0 0
intr 12345
ctxt 6789
";

    #[test]
    fn parses_system_counters() {
        assert_eq!(parse_system(STAT).unwrap(), (1000, 170));
    }

    #[test]
    fn counts_cpus() {
        assert_eq!(parse_cpu_count(STAT), 2);
    }

    #[test]
    fn parses_own_counters_with_spaces_in_comm() {
        let stat = "1234 (cpu floor) S 1 1234 1234 0 -1 4194560 100 0 0 0 42 8 0 0 39 19 3 0";
        assert_eq!(parse_own(stat).unwrap(), 50);
    }

    #[test]
    fn computes_usage_between_samples() {
        let prev = Sample {
            total: 1000,
            busy: 100,
            own: 10,
        };
        let cur = Sample {
            total: 2000,
            busy: 400,
            own: 210,
        };
        let usage = Usage::between(&prev, &cur).unwrap();
        assert!((usage.system - 0.3).abs() < 1e-9);
        assert!((usage.own - 0.2).abs() < 1e-9);
        assert!((usage.others() - 0.1).abs() < 1e-9);
    }

    #[test]
    fn no_usage_without_elapsed_time() {
        let s = Sample {
            total: 1000,
            busy: 100,
            own: 10,
        };
        assert_eq!(Usage::between(&s, &s), None);
    }
}
