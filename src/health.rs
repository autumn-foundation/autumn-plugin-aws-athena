//! The readiness check.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use autumn_web::actuator::{HealthCheckOutput, HealthIndicator};
use tokio::time::Instant;

use crate::api::BoxFuture;
use crate::plugin::Shared;

/// The time that the check keeps a result. Many probes then make few AWS calls.
const CACHE_FOR: Duration = Duration::from_secs(15);

/// The time limit of one check. It allows time for SDK retries.
const TIMEOUT_MS: u64 = 5000;

/// Reads the workgroup.
///
/// The check is down before the plugin starts. It is also down when the call fails.
/// The output does not show the AWS error, because it can have account details. The log has it.
pub(crate) struct WorkgroupCheck {
    shared: Arc<Shared>,
    last: Mutex<Option<(Instant, HealthCheckOutput)>>,
}

impl WorkgroupCheck {
    pub(crate) const fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            last: Mutex::new(None),
        }
    }

    fn cached(&self) -> Option<HealthCheckOutput> {
        let last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        last.as_ref()
            .filter(|(at, _)| at.elapsed() < CACHE_FOR)
            .map(|(_, output)| output.clone())
    }

    fn keep(&self, output: &HealthCheckOutput) {
        *self.last.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((Instant::now(), output.clone()));
    }
}

impl HealthIndicator for WorkgroupCheck {
    fn check(&self) -> BoxFuture<'_, HealthCheckOutput> {
        Box::pin(async move {
            let Some(athena) = self.shared.handle.get() else {
                return HealthCheckOutput::down().with_details(detail("state", "not started"));
            };
            if let Some(output) = self.cached() {
                return output;
            }
            let workgroup = &athena.config().workgroup;
            let output = match athena.api().check_workgroup(workgroup).await {
                Ok(()) => HealthCheckOutput::up().with_details(detail("workgroup", workgroup)),
                Err(err) => {
                    tracing::warn!(error = %err, "the Athena readiness check failed");
                    HealthCheckOutput::down().with_details(detail("workgroup", workgroup))
                }
            };
            self.keep(&output);
            output
        })
    }

    fn timeout_ms(&self) -> u64 {
        TIMEOUT_MS
    }
}

fn detail(key: &str, value: &str) -> HashMap<String, serde_json::Value> {
    HashMap::from([(key.to_owned(), serde_json::Value::from(value))])
}

#[cfg(test)]
mod tests;
