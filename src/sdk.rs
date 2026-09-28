//! [`AthenaApi`] on the AWS SDK client.

use aws_sdk_athena::Client;

use crate::api::{ApiError, AthenaApi, BoxFuture, Page, StartRequest, Status};
use crate::config::AthenaConfig;

/// The production [`AthenaApi`]. It uses `aws-sdk-athena`.
#[derive(Debug, Clone)]
pub struct SdkAthena {
    client: Client,
}

impl SdkAthena {
    /// Uses an SDK client that the app made.
    #[must_use]
    pub const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Makes an SDK client from the AWS default chain and `config`.
    pub async fn from_config(config: &AthenaConfig) -> Self {
        let _ = config;
        todo!()
    }

    /// The SDK client.
    #[must_use]
    pub const fn client(&self) -> &Client {
        &self.client
    }
}

impl AthenaApi for SdkAthena {
    fn start(&self, request: StartRequest) -> BoxFuture<'_, Result<String, ApiError>> {
        let _ = request;
        todo!()
    }

    fn status<'a>(&'a self, query_id: &'a str) -> BoxFuture<'a, Result<Status, ApiError>> {
        let _ = query_id;
        todo!()
    }

    fn results<'a>(
        &'a self,
        query_id: &'a str,
        next_token: Option<String>,
        max_results: i32,
    ) -> BoxFuture<'a, Result<Page, ApiError>> {
        let _ = (query_id, next_token, max_results);
        todo!()
    }

    fn stop<'a>(&'a self, query_id: &'a str) -> BoxFuture<'a, Result<(), ApiError>> {
        let _ = query_id;
        todo!()
    }

    fn check_workgroup<'a>(&'a self, workgroup: &'a str) -> BoxFuture<'a, Result<(), ApiError>> {
        let _ = workgroup;
        todo!()
    }
}

#[cfg(test)]
mod tests;
