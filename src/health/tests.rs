use std::sync::Arc;
use std::time::Duration;

use autumn_web::actuator::{HealthIndicator, HealthStatus};

use super::*;
use crate::client::Athena;
use crate::config::AthenaConfig;
use crate::testing::FakeAthena;

fn check_for(fake: &FakeAthena, workgroup: &str) -> WorkgroupCheck {
    let shared = Arc::new(Shared::default());
    let config = AthenaConfig {
        workgroup: workgroup.to_owned(),
        ..AthenaConfig::default()
    };
    let athena = Athena::with_parts(Arc::new(fake.clone()), config, Arc::default()).unwrap();
    assert!(shared.handle.set(athena).is_ok());
    WorkgroupCheck::new(shared)
}

#[tokio::test]
async fn a_check_before_startup_is_down() {
    let check = WorkgroupCheck::new(Arc::new(Shared::default()));
    let output = check.check().await;
    assert_eq!(output.status, HealthStatus::Down);
    assert_eq!(output.details["state"], "not started");
}

#[tokio::test]
async fn the_check_reads_the_configured_workgroup() {
    let fake = FakeAthena::new();
    let output = check_for(&fake, "reports").check().await;
    assert_eq!(output.status, HealthStatus::Up);
    assert_eq!(fake.checked_workgroups(), vec!["reports".to_owned()]);
}

#[tokio::test(start_paused = true)]
async fn the_check_keeps_a_result_for_a_short_time() {
    let fake = FakeAthena::new();
    let check = check_for(&fake, "primary");
    check.check().await;
    check.check().await;
    assert_eq!(fake.checked_workgroups().len(), 1);
    tokio::time::sleep(Duration::from_secs(16)).await;
    fake.fail_workgroup_check("down");
    assert_eq!(check.check().await.status, HealthStatus::Down);
    assert_eq!(fake.checked_workgroups().len(), 2);
}

#[test]
fn the_check_allows_time_for_sdk_retries() {
    let check = WorkgroupCheck::new(Arc::new(Shared::default()));
    assert!(check.timeout_ms() >= 5000);
}
