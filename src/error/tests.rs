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
        query_id: Some("t".into()),
        timeout: Duration::from_secs(1),
    };
    assert_eq!(timeout.query_id(), Some("t"));
    let early = AthenaError::Timeout {
        query_id: None,
        timeout: Duration::from_secs(1),
    };
    assert_eq!(early.query_id(), None);
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
        query_id: None,
        timeout: Duration::from_secs(1),
    };
    assert_eq!(timeout.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(
        AthenaError::ShuttingDown.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let cancelled = AthenaError::Cancelled {
        query_id: "c".into(),
    };
    assert_eq!(cancelled.status(), StatusCode::SERVICE_UNAVAILABLE);
    let denied = AthenaError::Api(ApiError::permanent("StartQueryExecution", "AccessDenied"));
    assert_eq!(denied.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!denied.is_retryable());
    assert_eq!(failed(true).status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(failed(false).status(), StatusCode::INTERNAL_SERVER_ERROR);
    let api = AthenaError::Api(ApiError::new("StartQueryExecution", "down"));
    assert_eq!(api.into_autumn().status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[test]
fn the_failure_message_does_not_show_the_reason() {
    // The reason can repeat parameter values.
    let text = failed(false).to_string();
    assert!(!text.contains("boom"), "{text}");
    assert!(text.contains("query q failed"), "{text}");
}

#[test]
fn or_http_uses_the_error_status() {
    let result: Result<(), AthenaError> = Err(AthenaError::ShuttingDown);
    assert_eq!(
        result.or_http().unwrap_err().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}
