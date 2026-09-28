//! Query counters and the Autumn metrics source.

use std::sync::atomic::{AtomicU64, Ordering};

use autumn_web::actuator::{MetricFamily, MetricKind, MetricSample, MetricsSource};

/// How a query ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

/// Counters for all queries of one app.
#[derive(Debug, Default)]
pub(crate) struct Metrics {
    started: AtomicU64,
    succeeded: AtomicU64,
    failed: AtomicU64,
    cancelled: AtomicU64,
    timed_out: AtomicU64,
    scanned_bytes: AtomicU64,
    open: AtomicU64,
}

impl Metrics {
    pub(crate) fn started(&self) {
        self.started.fetch_add(1, Ordering::Relaxed);
        self.open.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn ended(&self, outcome: Outcome, scanned_bytes: u64) {
        let counter = match outcome {
            Outcome::Succeeded => &self.succeeded,
            Outcome::Failed => &self.failed,
            Outcome::Cancelled => &self.cancelled,
            Outcome::TimedOut => &self.timed_out,
        };
        counter.fetch_add(1, Ordering::Relaxed);
        self.scanned_bytes.fetch_add(scanned_bytes, Ordering::Relaxed);
        let _ = self
            .open
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| Some(v.saturating_sub(1)));
    }

    fn load(counter: &AtomicU64) -> f64 {
        #[allow(clippy::cast_precision_loss, reason = "Prometheus values are f64")]
        let value = counter.load(Ordering::Relaxed) as f64;
        value
    }
}

impl MetricsSource for Metrics {
    fn collect(&self) -> Vec<MetricFamily> {
        let outcome = |name: &str, counter: &AtomicU64| MetricSample {
            labels: vec![("outcome".to_owned(), name.to_owned())],
            value: Self::load(counter),
        };
        vec![
            MetricFamily {
                name: "athena_queries_started_total".to_owned(),
                help: "Athena queries that the plugin started.".to_owned(),
                kind: MetricKind::Counter,
                samples: vec![MetricSample { labels: Vec::new(), value: Self::load(&self.started) }],
            },
            MetricFamily {
                name: "athena_queries_total".to_owned(),
                help: "Athena queries that ended, by outcome.".to_owned(),
                kind: MetricKind::Counter,
                samples: vec![
                    outcome("succeeded", &self.succeeded),
                    outcome("failed", &self.failed),
                    outcome("cancelled", &self.cancelled),
                    outcome("timed_out", &self.timed_out),
                ],
            },
            MetricFamily {
                name: "athena_data_scanned_bytes_total".to_owned(),
                help: "Bytes that Athena scanned for the queries.".to_owned(),
                kind: MetricKind::Counter,
                samples: vec![MetricSample { labels: Vec::new(), value: Self::load(&self.scanned_bytes) }],
            },
            MetricFamily {
                name: "athena_queries_open".to_owned(),
                help: "Athena queries that did not end yet.".to_owned(),
                kind: MetricKind::Gauge,
                samples: vec![MetricSample { labels: Vec::new(), value: Self::load(&self.open) }],
            },
        ]
    }
}

#[cfg(test)]
mod tests;
