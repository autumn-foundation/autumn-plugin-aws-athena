//! [`AthenaPlugin`]: installs an [`Athena`] handle in an Autumn app.

use std::sync::Arc;

use autumn_web::AppState;
use autumn_web::app::AppBuilder;
use autumn_web::plugin::Plugin;

use crate::api::AthenaApi;
use crate::client::Athena;
use crate::config::AthenaConfig;

/// State that the plugin hooks share.
#[derive(Default)]
struct Shared {
    handle: std::sync::OnceLock<Athena>,
    metrics: Arc<crate::metrics::Metrics>,
}

impl Shared {
    async fn shutdown(&self) {
        todo!()
    }
}

/// Installs an [`Athena`] handle in an Autumn app.
#[must_use]
pub struct AthenaPlugin {
    api: Option<Arc<dyn AthenaApi>>,
}

impl Default for AthenaPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl AthenaPlugin {
    /// Makes a plugin that reads `[athena]`.
    pub fn new() -> Self {
        todo!()
    }

    /// Reads `[section]` instead of `[athena]`.
    pub fn config_section(self, section: impl Into<String>) -> Self {
        let _ = section.into();
        todo!()
    }

    /// Uses `config` and reads no files or variables.
    pub fn config(self, config: AthenaConfig) -> Self {
        let _ = config;
        todo!()
    }

    /// Changes the configuration after the plugin reads it.
    pub fn configure(self, change: impl FnOnce(&mut AthenaConfig) + Send + 'static) -> Self {
        let _ = change;
        todo!()
    }

    /// Uses `api` instead of an AWS SDK client, for example a test fake.
    pub fn api(self, api: impl AthenaApi) -> Self {
        let _ = api;
        todo!()
    }
}

impl Plugin for AthenaPlugin {
    fn build(self, app: AppBuilder) -> AppBuilder {
        let _ = self.api;
        app
    }
}

impl Athena {
    /// Gets the handle from the app state.
    #[must_use]
    pub fn from_state(state: &AppState) -> Option<Self> {
        let _ = state;
        todo!()
    }
}

impl axum::extract::FromRequestParts<AppState> for Athena {
    type Rejection = autumn_web::AutumnError;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let _ = (parts, state);
        todo!()
    }
}

#[cfg(test)]
mod tests;
