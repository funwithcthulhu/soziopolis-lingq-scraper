use std::time::Duration;

use std::sync::{Mutex, OnceLock};

#[derive(Debug, Default, Clone)]
pub struct PerfCounters {
    pub browse_cache_hits: u64,
    pub browse_cache_misses: u64,
    pub browse_summary_cache_hits: u64,
    pub browse_summary_cache_misses: u64,
    pub library_page_queries: u64,
    pub library_page_query_time_ms_total: u64,
    pub content_refreshes: u64,
    pub content_refresh_time_ms_total: u64,
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

pub fn record_library_page_query(duration: Duration) {
    if let Ok(mut value) = counters().lock() {
        value.library_page_queries += 1;
        value.library_page_query_time_ms_total += duration.as_millis() as u64;
    }
}

pub fn record_content_refresh(duration: Duration) {
    if let Ok(mut value) = counters().lock() {
        value.content_refreshes += 1;
        value.content_refresh_time_ms_total += duration.as_millis() as u64;
    }
}
