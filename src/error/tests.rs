use std::time::Duration;

use http::StatusCode;

use super::*;

fn failed(retryable: bool) -> AthenaError {
    AthenaError::Failed {
        query_id: "q".into(),
        reason: Some("boom".into()),
        failure: Some(FailureInfo {
            retryable,
            ..FailureInfo::default()
        }),
    }
}

#[test]
fn query_errors_carry_the_query_id() {
    assert_eq!(failed(false).query_id(), Some("q"));
    let timeout = AthenaError::Timeout {
        query_id: "t".into(),
        timeout: Duration::from_secs(1),
    };
    assert_eq!(timeout.query_id(), Some("t"));
    assert_eq!(AthenaError::NotInstalled.query_id(), None);
}

#[test]
fn transient_errors_are_retryable() {
    assert!(AthenaError::Api(ApiError::new("GetQueryExecution", "throttled")).is_retryable());
    assert!(failed(true).is_retryable());
    assert!(!failed(false).is_retryable());
    assert!(!AthenaError::NotInstalled.is_retryable());
}

#[test]
fn errors_map_to_http_statuses() {
    let timeout = AthenaError::Timeout {
        query_id: "t".into(),
        timeout: Duration::from_secs(1),
    };
    assert_eq!(timeout.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(failed(true).status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(failed(false).status(), StatusCode::INTERNAL_SERVER_ERROR);
    let api = AthenaError::Api(ApiError::new("StartQueryExecution", "down"));
    assert_eq!(api.into_autumn().status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[test]
fn the_failure_message_has_the_reason() {
    assert_eq!(failed(false).to_string(), "query q failed: boom");
}
