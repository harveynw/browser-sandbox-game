//! Fixed-memory timing distributions. Quantiles are conservative 25 us bucket upper bounds.
use serde_json::{Value, json};
use std::time::Duration;
const BUCKETS: usize = 4096;
/// Tracks timing samples in fixed-size buckets without retaining individual samples.
pub struct Histogram {
    /// Counts of timing samples in 25-microsecond buckets.
    bins: Vec<u64>,
    /// Total number of timing samples.
    count: u64,
    /// Sum of sample durations in microseconds.
    sum: u128,
    /// Shortest observed sample in microseconds.
    min: u64,
    /// Longest observed sample in microseconds.
    max: u64,
}
impl Default for Histogram {
    fn default() -> Self {
        Self {
            bins: vec![0; BUCKETS],
            count: 0,
            sum: 0,
            min: u64::MAX,
            max: 0,
        }
    }
}
impl Histogram {
    /// Adds a duration sample to the fixed timing buckets.
    pub fn observe(&mut self, time: Duration) {
        let us = time.as_micros().min(u64::MAX as u128) as u64;
        self.bins[(us / 25).min((BUCKETS - 1) as u64) as usize] += 1;
        self.count += 1;
        self.sum += u128::from(us);
        self.min = self.min.min(us);
        self.max = self.max.max(us);
    }
    /// Estimates a timing percentile in milliseconds from the stored buckets.
    fn quantile(&self, p: f64) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        let rank = (self.count as f64 * p).ceil() as u64;
        let mut sum = 0;
        for (i, n) in self.bins.iter().enumerate() {
            sum += n;
            if sum >= rank {
                return if i == BUCKETS - 1 {
                    self.max as f64 / 1000.0
                } else {
                    ((i + 1) * 25) as f64 / 1000.0
                };
            }
        }
        self.max as f64 / 1000.0
    }
    /// Returns collected measurements as a JSON report.
    pub fn report(&self) -> Value {
        json!({"samples":self.count,"p50_ms":self.quantile(0.5),"p95_ms":self.quantile(0.95),"p99_ms":self.quantile(0.99),"min_ms":if self.count==0{0.0}else{self.min as f64/1000.0},"max_ms":self.max as f64/1000.0,"mean_ms":if self.count==0{0.0}else{self.sum as f64/self.count as f64/1000.0}})
    }
}
/// Tracks elapsed work for each major tick phase.
#[derive(Default, bevy_ecs::prelude::Resource)]
pub struct WorldTimings {
    /// Timing distribution for client admission and packet handling.
    pub admission: Histogram,
    /// Timing distribution for AI planning.
    pub planning: Histogram,
    /// Timing distribution for actor movement.
    pub motors: Histogram,
    /// Timing distribution for gameplay rules after movement.
    pub gameplay: Histogram,
    /// Timing distribution for state publication.
    pub replication: Histogram,
    /// Timing distribution for network output submission.
    pub flush: Histogram,
}
impl WorldTimings {
    /// Returns collected measurements as a JSON report.
    pub fn report(&self) -> Value {
        json!({"admission":self.admission.report(),"planning":self.planning.report(),"motors":self.motors.report(),"gameplay":self.gameplay.report(),"replication":self.replication.report(),"flush":self.flush.report()})
    }
}
/// Verifies that bounded histogram tracks tails and overflow without keeping raw samples.
#[test]
fn bounded_histogram_tracks_tails_and_overflow_without_keeping_raw_samples() {
    let mut h = Histogram::default();
    for i in 0..100 {
        h.observe(Duration::from_millis(i));
    }
    h.observe(Duration::from_secs(1));
    assert_eq!(h.bins.len(), BUCKETS);
    let r = h.report();
    assert_eq!(r["samples"], 101);
    assert_eq!(r["p50_ms"], 50.025);
    assert_eq!(r["p99_ms"], 99.025);
    assert_eq!(r["max_ms"], 1000.0);
}
