//! The public error type.

use std::time::Duration;

use autumn_web::AutumnError;
use http::StatusCode;

use crate::api::{ApiError, FailureInfo};
use crate::config::ConfigError;
use crate::literal::ParamError;
use crate::value::DecodeError;

/// An error from the plugin.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum AthenaError {
    /// The configuration is not valid.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// A parameter is not valid.
    #[error(transparent)]
    Param(#[from] ParamError),
    /// The number of parameters is not the number of `?` placeholders.
    #[error("the SQL has {placeholders} placeholders, but the query has {parameters} parameters")]
    #[non_exhaustive]
    ParameterCount {
        /// The `?` placeholders in the SQL.
        placeholders: usize,
        /// The bound parameters.
        parameters: usize,
    },
    /// A call to Athena failed.
    #[error(transparent)]
    Api(#[from] ApiError),
    /// Athena ran the query, and the query failed.
    ///
    /// The message does not show `reason`, because it can repeat parameter values.
    #[error("query {query_id} failed{}", category_text(failure.as_ref()))]
    #[non_exhaustive]
    Failed {
        /// The query ID.
        query_id: String,
        /// The Athena reason for the failure.
        reason: Option<String>,
        /// The Athena error details.
        failure: Option<FailureInfo>,
    },
    /// Someone stopped the query.
    #[error("query {query_id} was cancelled")]
    #[non_exhaustive]
    Cancelled {
        /// The query ID.
        query_id: String,
    },
    /// The query did not complete in time. The plugin stopped it.
    #[error("the query did not complete in {timeout:?}")]
    #[non_exhaustive]
    Timeout {
        /// The query ID. It is `None` if the start did not complete in time.
        query_id: Option<String>,
        /// The timeout.
        timeout: Duration,
    },
    /// The result has more rows than the limit.
    #[error("query {query_id} returned more than {limit} rows")]
    #[non_exhaustive]
    TooManyRows {
        /// The query ID.
        query_id: String,
        /// The row limit.
        limit: usize,
    },
    /// The values of the result have more bytes than the limit.
    #[error("query {query_id} returned more than {limit_bytes} bytes")]
    #[non_exhaustive]
    ResultTooLarge {
        /// The query ID.
        query_id: String,
        /// The byte limit.
        limit_bytes: usize,
    },
    /// The app shuts down. The plugin starts no new queries.
    #[error("the app shuts down: the Athena plugin starts no new queries")]
    ShuttingDown,
    /// A result value does not decode.
    #[error(transparent)]
    Decode(#[from] DecodeError),
    /// The app does not have the plugin.
    #[error("the Athena plugin is not installed: add `AthenaPlugin` to the app")]
    NotInstalled,
}

/// The Athena error codes of a failure, for the message.
fn category_text(failure: Option<&FailureInfo>) -> String {
    match failure.map(|f| (f.category, f.error_type)) {
        Some((Some(category), Some(error_type))) => {
            format!(" (Athena error category {category}, type {error_type})")
        }
        Some((Some(category), None)) => format!(" (Athena error category {category})"),
        _ => String::new(),
    }
}

impl AthenaError {
    /// The query ID, if Athena started the query.
    #[must_use]
    pub fn query_id(&self) -> Option<&str> {
        match self {
            Self::Failed { query_id, .. }
            | Self::Cancelled { query_id }
            | Self::TooManyRows { query_id, .. }
            | Self::ResultTooLarge { query_id, .. } => Some(query_id),
            Self::Timeout { query_id, .. } => query_id.as_deref(),
            _ => None,
        }
    }

    /// Returns `true` if a retry of the same query can succeed.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Api(err) => err.retryable,
            Self::Timeout { .. } => true,
            Self::Failed { failure, .. } => failure.as_ref().is_some_and(|f| f.retryable),
            _ => false,
        }
    }

    /// The HTTP status for this error.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Timeout { .. } => StatusCode::GATEWAY_TIMEOUT,
            Self::Api(err) if err.retryable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Cancelled { .. } | Self::ShuttingDown => StatusCode::SERVICE_UNAVAILABLE,
            Self::Failed { .. } if self.is_retryable() => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Converts to an [`AutumnError`] with [`status`](Self::status).
    ///
    /// The `?` operator also converts, but always gives status 500.
    /// Autumn shows server error details only in development.
    #[must_use]
    pub fn into_autumn(self) -> AutumnError {
        let status = self.status();
        AutumnError::internal_server_error(self).with_status(status)
    }
}

#[cfg(test)]
mod tests;

/// Adds [`or_http`](AthenaResultExt::or_http) to `Result<T, AthenaError>`.
pub trait AthenaResultExt<T> {
    /// Converts the error with [`AthenaError::into_autumn`].
    ///
    /// # Errors
    ///
    /// Returns the converted error.
    fn or_http(self) -> Result<T, AutumnError>;
}

impl<T> AthenaResultExt<T> for Result<T, AthenaError> {
    fn or_http(self) -> Result<T, AutumnError> {
        self.map_err(AthenaError::into_autumn)
    }
}
