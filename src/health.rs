//! The readiness check.

use std::collections::HashMap;
use std::sync::Arc;

use crate::api::BoxFuture;
use autumn_web::actuator::{HealthCheckOutput, HealthIndicator};

use crate::plugin::Shared;

/// Reads the workgroup. Down until the plugin starts or when the call fails.
///
/// The output does not show the AWS error, because it can have account details. The log has it.
pub(crate) struct WorkgroupCheck {
    pub(crate) shared: Arc<Shared>,
}

impl WorkgroupCheck {
    pub(crate) const fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

impl HealthIndicator for WorkgroupCheck {
    fn check(&self) -> BoxFuture<'_, HealthCheckOutput> {
        Box::pin(async move {
            let Some(athena) = self.shared.handle.get() else {
                return HealthCheckOutput::down().with_details(detail("state", "not started"));
            };
            let workgroup = &athena.config().workgroup;
            match athena.api().check_workgroup(workgroup).await {
                Ok(()) => HealthCheckOutput::up().with_details(detail("workgroup", workgroup)),
                Err(err) => {
                    tracing::warn!(error = %err, "the Athena readiness check failed");
                    HealthCheckOutput::down().with_details(detail("workgroup", workgroup))
                }
            }
        })
    }
}

fn detail(key: &str, value: &str) -> HashMap<String, serde_json::Value> {
    HashMap::from([(key.to_owned(), serde_json::Value::from(value))])
}

#[cfg(test)]
mod tests;
