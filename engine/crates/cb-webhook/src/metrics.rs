//! Metrics module for webhook observability.
//!
//! This module provides Prometheus-compatible metrics for monitoring
//! the webhook server's performance and behavior.
//!
//! ## Metrics Exposed
//!
//! - `cb_webhook_events_received_total` - Total events received by source and type
//! - `cb_webhook_events_processed_total` - Events processed by status
//! - `cb_webhook_trigger_matches_total` - Trigger matches by trigger name
//! - `cb_webhook_auth_failures_total` - Authentication failures by endpoint
//! - `cb_webhook_payload_bytes` - Payload size histogram
//! - `cb_webhook_processing_duration_seconds` - Processing time histogram
//! - `cb_webhook_endpoints_active` - Number of active endpoints

use metrics::{counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram};
use std::time::Instant;

/// Metric names as constants.
pub mod names {
    /// Total events received.
    pub const EVENTS_RECEIVED: &str = "cb_webhook_events_received_total";
    /// Events processed by status.
    pub const EVENTS_PROCESSED: &str = "cb_webhook_events_processed_total";
    /// Trigger matches.
    pub const TRIGGER_MATCHES: &str = "cb_webhook_trigger_matches_total";
    /// Authentication failures.
    pub const AUTH_FAILURES: &str = "cb_webhook_auth_failures_total";
    /// Payload size in bytes.
    pub const PAYLOAD_BYTES: &str = "cb_webhook_payload_bytes";
    /// Processing duration in seconds.
    pub const PROCESSING_DURATION: &str = "cb_webhook_processing_duration_seconds";
    /// Active endpoints gauge.
    pub const ENDPOINTS_ACTIVE: &str = "cb_webhook_endpoints_active";
    /// NATS publish success.
    pub const NATS_PUBLISH_SUCCESS: &str = "cb_webhook_nats_publish_success_total";
    /// NATS publish failures.
    pub const NATS_PUBLISH_FAILURES: &str = "cb_webhook_nats_publish_failures_total";
    /// Rate limit rejections.
    pub const RATE_LIMIT_REJECTIONS: &str = "cb_webhook_rate_limit_rejections_total";
    /// IP rejections.
    pub const IP_REJECTIONS: &str = "cb_webhook_ip_rejections_total";
}

/// Initialize metric descriptions.
pub fn init_metrics() {
    // Counters
    describe_counter!(
        names::EVENTS_RECEIVED,
        "Total number of webhook events received"
    );
    describe_counter!(
        names::EVENTS_PROCESSED,
        "Total number of webhook events processed by status"
    );
    describe_counter!(names::TRIGGER_MATCHES, "Total number of trigger matches");
    describe_counter!(
        names::AUTH_FAILURES,
        "Total number of authentication failures"
    );
    describe_counter!(
        names::NATS_PUBLISH_SUCCESS,
        "Total number of successful NATS publishes"
    );
    describe_counter!(
        names::NATS_PUBLISH_FAILURES,
        "Total number of failed NATS publishes"
    );
    describe_counter!(
        names::RATE_LIMIT_REJECTIONS,
        "Total number of rate limit rejections"
    );
    describe_counter!(names::IP_REJECTIONS, "Total number of IP-based rejections");

    // Histograms
    describe_histogram!(
        names::PAYLOAD_BYTES,
        "Histogram of webhook payload sizes in bytes"
    );
    describe_histogram!(
        names::PROCESSING_DURATION,
        "Histogram of webhook processing duration in seconds"
    );

    // Gauges
    describe_gauge!(
        names::ENDPOINTS_ACTIVE,
        "Number of currently active webhook endpoints"
    );
}

/// Record a received event.
pub fn record_event_received(source: &str, event_type: &str, endpoint: &str) {
    counter!(
        names::EVENTS_RECEIVED,
        "source" => source.to_string(),
        "event_type" => event_type.to_string(),
        "endpoint" => endpoint.to_string()
    )
    .increment(1);
}

/// Record a processed event.
pub fn record_event_processed(status: &str, endpoint: &str) {
    counter!(
        names::EVENTS_PROCESSED,
        "status" => status.to_string(),
        "endpoint" => endpoint.to_string()
    )
    .increment(1);
}

/// Record a trigger match.
pub fn record_trigger_match(trigger_name: &str, workflow: &str, endpoint: &str) {
    counter!(
        names::TRIGGER_MATCHES,
        "trigger" => trigger_name.to_string(),
        "workflow" => workflow.to_string(),
        "endpoint" => endpoint.to_string()
    )
    .increment(1);
}

/// Record an authentication failure.
pub fn record_auth_failure(endpoint: &str, reason: &str) {
    counter!(
        names::AUTH_FAILURES,
        "endpoint" => endpoint.to_string(),
        "reason" => reason.to_string()
    )
    .increment(1);
}

/// Record payload size.
pub fn record_payload_size(bytes: usize, endpoint: &str) {
    histogram!(
        names::PAYLOAD_BYTES,
        "endpoint" => endpoint.to_string()
    )
    .record(bytes as f64);
}

/// Record processing duration.
pub fn record_processing_duration(duration_secs: f64, endpoint: &str, status: &str) {
    histogram!(
        names::PROCESSING_DURATION,
        "endpoint" => endpoint.to_string(),
        "status" => status.to_string()
    )
    .record(duration_secs);
}

/// Update the active endpoints gauge.
pub fn set_active_endpoints(count: usize) {
    gauge!(names::ENDPOINTS_ACTIVE).set(count as f64);
}

/// Record a successful NATS publish.
pub fn record_nats_publish_success(endpoint: &str, workflow: &str) {
    counter!(
        names::NATS_PUBLISH_SUCCESS,
        "endpoint" => endpoint.to_string(),
        "workflow" => workflow.to_string()
    )
    .increment(1);
}

/// Record a failed NATS publish.
pub fn record_nats_publish_failure(endpoint: &str, reason: &str) {
    counter!(
        names::NATS_PUBLISH_FAILURES,
        "endpoint" => endpoint.to_string(),
        "reason" => reason.to_string()
    )
    .increment(1);
}

/// Record a rate limit rejection.
pub fn record_rate_limit_rejection(endpoint: &str, key: &str) {
    counter!(
        names::RATE_LIMIT_REJECTIONS,
        "endpoint" => endpoint.to_string(),
        "key" => key.to_string()
    )
    .increment(1);
}

/// Record an IP rejection.
pub fn record_ip_rejection(endpoint: &str, ip: &str) {
    counter!(
        names::IP_REJECTIONS,
        "endpoint" => endpoint.to_string(),
        "ip" => ip.to_string()
    )
    .increment(1);
}

/// A timer for measuring processing duration.
pub struct ProcessingTimer {
    start: Instant,
    endpoint: String,
    status: Option<String>,
}

impl ProcessingTimer {
    /// Start a new processing timer.
    pub fn start(endpoint: impl Into<String>) -> Self {
        Self {
            start: Instant::now(),
            endpoint: endpoint.into(),
            status: None,
        }
    }

    /// Set the processing status.
    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = Some(status.into());
    }

    /// Complete the timer and record the duration.
    pub fn complete(mut self) {
        let duration = self.start.elapsed();
        let status = self.status.take().unwrap_or_else(|| "unknown".to_string());
        record_processing_duration(duration.as_secs_f64(), &self.endpoint, &status);
        // Mark as completed so Drop doesn't record again
        self.status = Some("__completed__".to_string());
    }

    /// Complete with a specific status.
    pub fn complete_with_status(mut self, status: impl Into<String>) {
        self.status = Some(status.into());
        self.complete();
    }
}

impl Drop for ProcessingTimer {
    fn drop(&mut self) {
        // If the timer wasn't explicitly completed, record it anyway
        // This handles cases where processing panics or errors out
        if self.status.as_deref() != Some("__completed__") {
            let duration = self.start.elapsed();
            let status = self.status.as_deref().unwrap_or("dropped");
            record_processing_duration(duration.as_secs_f64(), &self.endpoint, status);
        }
    }
}

/// Metrics recorder that batches updates.
#[derive(Default)]
pub struct MetricsRecorder {
    events_received: u64,
    events_processed: u64,
    trigger_matches: u64,
    auth_failures: u64,
}

impl MetricsRecorder {
    /// Create a new metrics recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment events received.
    pub fn inc_events_received(&mut self) {
        self.events_received += 1;
    }

    /// Increment events processed.
    pub fn inc_events_processed(&mut self) {
        self.events_processed += 1;
    }

    /// Increment trigger matches.
    pub fn inc_trigger_matches(&mut self) {
        self.trigger_matches += 1;
    }

    /// Increment auth failures.
    pub fn inc_auth_failures(&mut self) {
        self.auth_failures += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_processing_timer() {
        let timer = ProcessingTimer::start("test-endpoint");
        std::thread::sleep(std::time::Duration::from_millis(10));
        timer.complete_with_status("success");
    }

    #[test]
    fn test_metrics_recorder() {
        let mut recorder = MetricsRecorder::new();
        recorder.inc_events_received();
        recorder.inc_events_received();
        recorder.inc_trigger_matches();

        assert_eq!(recorder.events_received, 2);
        assert_eq!(recorder.trigger_matches, 1);
        assert_eq!(recorder.auth_failures, 0);
    }

    #[test]
    fn test_metric_names() {
        assert!(names::EVENTS_RECEIVED.starts_with("cb_webhook_"));
        assert!(names::PROCESSING_DURATION.contains("duration"));
    }
}
