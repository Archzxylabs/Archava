//! Process-local Prometheus metrics for the HTTP surface.
//!
//! The counters are deliberately small and label-free: the host's risk is
//! "nobody can see that it is failing", not "the dashboard needs a route
//! dimension". A per-route label set would also let an unauthenticated caller
//! create cardinality by requesting arbitrary paths; the matched path is not
//! used here for the same reason.
//!
//! `render` is the only place that formats the exposition text, so the
//! `/metrics` handler cannot accidentally emit a metric the counters do not
//! maintain.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;

/// Counters the process maintains for its whole lifetime.
#[derive(Debug)]
pub(crate) struct Metrics {
    started_unix: u64,
    requests: AtomicU64,
    server_errors: AtomicU64,
    latency_micros: AtomicU64,
}

impl Metrics {
    pub(crate) fn new() -> Self {
        let started_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        Self {
            started_unix,
            requests: AtomicU64::new(0),
            server_errors: AtomicU64::new(0),
            latency_micros: AtomicU64::new(0),
        }
    }

    /// Records one completed request. `Relaxed` is enough: the counters are
    /// independent statistics, not a synchronization edge.
    pub(crate) fn observe(&self, status: StatusCode, elapsed: Duration) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        if status.is_server_error() {
            self.server_errors.fetch_add(1, Ordering::Relaxed);
        }
        self.latency_micros.fetch_add(
            elapsed.as_micros().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
    }

    /// Renders the Prometheus text exposition. `db_ready` is sampled by the
    /// caller immediately before the scrape so a stale value cannot outlive a
    /// database restart.
    pub(crate) fn render(&self, db_ready: bool) -> String {
        let requests = self.requests.load(Ordering::Relaxed);
        let errors = self.server_errors.load(Ordering::Relaxed);
        let latency = self.latency_micros.load(Ordering::Relaxed) as f64 / 1_000_000.0;
        format!(
            "# HELP archava_http_requests_total Total HTTP requests served.\n\
             # TYPE archava_http_requests_total counter\n\
             archava_http_requests_total {requests}\n\
             # HELP archava_http_requests_5xx_total Total HTTP requests answered 5xx.\n\
             # TYPE archava_http_requests_5xx_total counter\n\
             archava_http_requests_5xx_total {errors}\n\
             # HELP archava_http_request_duration_seconds_sum Cumulative request latency.\n\
             # TYPE archava_http_request_duration_seconds summary\n\
             archava_http_request_duration_seconds_sum {latency}\n\
             archava_http_request_duration_seconds_count {requests}\n\
             # HELP archava_process_start_time_seconds Process start time, Unix seconds.\n\
             # TYPE archava_process_start_time_seconds gauge\n\
             archava_process_start_time_seconds {}\n\
             # HELP archava_db_ready Whether a database probe succeeded on this scrape.\n\
             # TYPE archava_db_ready gauge\n\
             archava_db_ready {}\n",
            self.started_unix,
            u8::from(db_ready),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_rendered_after_a_request() {
        let metrics = Metrics::new();
        metrics.observe(StatusCode::OK, Duration::from_millis(25));
        metrics.observe(StatusCode::INTERNAL_SERVER_ERROR, Duration::from_millis(75));
        let text = metrics.render(true);
        assert!(text.contains("archava_http_requests_total 2"));
        assert!(text.contains("archava_http_requests_5xx_total 1"));
        assert!(text.contains("archava_http_request_duration_seconds_sum 0.1"));
        assert!(text.contains("archava_http_request_duration_seconds_count 2"));
        assert!(text.contains("archava_db_ready 1"));
    }

    #[test]
    fn a_cold_process_renders_zeroed_counters() {
        let text = Metrics::new().render(false);
        assert!(text.contains("archava_http_requests_total 0"));
        assert!(text.contains("archava_http_requests_5xx_total 0"));
        assert!(text.contains("archava_db_ready 0"));
    }
}
