use std::sync::{Mutex, OnceLock};

#[derive(Debug, Default, Clone)]
pub struct PerfCounters {
    pub browse_cache_hits: u64,
    pub browse_cache_misses: u64,
    pub browse_summary_cache_hits: u64,
    pub browse_summary_cache_misses: u64,
}

static PERF_COUNTERS: OnceLock<Mutex<PerfCounters>> = OnceLock::new();

fn counters() -> &'static Mutex<PerfCounters> {
    PERF_COUNTERS.get_or_init(|| Mutex::new(PerfCounters::default()))
}

pub fn snapshot() -> PerfCounters {
    counters()
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default()
}

pub fn record_browse_cache_hit() {
    if let Ok(mut value) = counters().lock() {
        value.browse_cache_hits += 1;
    }
}

pub fn record_browse_cache_miss() {
    if let Ok(mut value) = counters().lock() {
        value.browse_cache_misses += 1;
    }
}

pub fn record_browse_summary_cache_hit() {
    if let Ok(mut value) = counters().lock() {
        value.browse_summary_cache_hits += 1;
    }
}

pub fn record_browse_summary_cache_miss() {
    if let Ok(mut value) = counters().lock() {
        value.browse_summary_cache_misses += 1;
    }
}
