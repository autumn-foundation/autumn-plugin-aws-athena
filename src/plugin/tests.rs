use std::time::Duration;

use super::*;
use crate::testing::{FakeAthena, FakeQuery};

#[test]
fn build_declares_the_config_section() {
    let app = autumn_web::app().plugin(AthenaPlugin::new().config_section("reports"));
    assert!(app.has_config_section("reports"));
    assert!(!app.has_config_section("athena"));
}

#[test]
fn an_explicit_config_declares_no_section() {
    let app = autumn_web::app().plugin(AthenaPlugin::new().config(AthenaConfig::default()));
    assert!(!app.has_config_section("athena"));
}

#[tokio::test(start_paused = true)]
async fn shutdown_stops_open_queries() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::pending());
    let shared = Shared::default();
    let athena = Athena::with_parts(
        Arc::new(fake.clone()),
        AthenaConfig::default(),
        Arc::clone(&shared.metrics),
    )
    .unwrap();
    assert!(shared.handle.set(athena.clone()).is_ok());
    let task = tokio::spawn(async move { athena.query("SELECT 1").execute().await });
    tokio::time::sleep(Duration::from_secs(1)).await;
    shared.shutdown().await;
    assert_eq!(fake.stopped(), vec!["fake-1".to_owned()]);
    assert!(task.await.unwrap().is_err());
}

#[tokio::test]
async fn shutdown_before_startup_does_nothing() {
    Shared::default().shutdown().await;
}
