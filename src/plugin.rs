//! [`AthenaPlugin`]: installs an [`Athena`] handle in an Autumn app.
//!
//! # Contract
//!
//! - `build` reads the configuration. A bad configuration stops the boot in the startup hook.
//! - The startup hook makes the SDK client and puts the handle in the app state.
//! - The shutdown hook stops each open query.
//! - The readiness check and the metrics source use the same handle.

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use autumn_web::app::AppBuilder;
use autumn_web::plugin::Plugin;
use autumn_web::{AppState, AutumnError};

use crate::api::AthenaApi;
use crate::client::Athena;
use crate::config::{AthenaConfig, ConfigError, DEFAULT_SECTION};
use crate::error::AthenaError;
use crate::health::WorkgroupCheck;
use crate::metrics::Metrics;
use crate::sdk::SdkAthena;

/// The plugin name in Autumn diagnostics.
pub const PLUGIN_NAME: &str = "autumn-plugin-aws-athena";

/// State that the plugin hooks share.
#[derive(Default)]
pub(crate) struct Shared {
    pub(crate) handle: OnceLock<Athena>,
    pub(crate) metrics: Arc<Metrics>,
}

impl Shared {
    pub(crate) async fn shutdown(&self) {
        if let Some(athena) = self.handle.get() {
            athena.stop_all().await;
        }
    }
}

enum Source {
    Section(String),
    Explicit(Box<AthenaConfig>),
}

type Change = Box<dyn FnOnce(&mut AthenaConfig) + Send>;

/// Installs an [`Athena`] handle in an Autumn app.
///
/// ```rust,no_run
/// use autumn_plugin_aws_athena::AthenaPlugin;
///
/// # async fn run() {
/// autumn_web::app().plugin(AthenaPlugin::new()).run().await;
/// # }
/// ```
#[must_use]
pub struct AthenaPlugin {
    source: Source,
    changes: Vec<Change>,
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
        Self {
            source: Source::Section(DEFAULT_SECTION.to_owned()),
            changes: Vec::new(),
            api: None,
        }
    }

    /// Reads `[section]` instead of `[athena]`.
    pub fn config_section(mut self, section: impl Into<String>) -> Self {
        self.source = Source::Section(section.into());
        self
    }

    /// Uses `config` and reads no files or variables.
    pub fn config(mut self, config: AthenaConfig) -> Self {
        self.source = Source::Explicit(Box::new(config));
        self
    }

    /// Changes the configuration after the plugin reads it.
    pub fn configure(mut self, change: impl FnOnce(&mut AthenaConfig) + Send + 'static) -> Self {
        self.changes.push(Box::new(change));
        self
    }

    /// Uses `api` instead of an AWS SDK client, for example a test fake.
    pub fn api(mut self, api: impl AthenaApi) -> Self {
        self.api = Some(Arc::new(api));
        self
    }

    fn resolve(source: &Source, changes: Vec<Change>) -> Result<AthenaConfig, ConfigError> {
        let mut config = match source {
            Source::Section(section) => AthenaConfig::resolve(section)?,
            Source::Explicit(config) => (**config).clone(),
        };
        for change in changes {
            change(&mut config);
        }
        config.validate()?;
        Ok(config)
    }
}

impl Plugin for AthenaPlugin {
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed(PLUGIN_NAME)
    }

    fn build(self, app: AppBuilder) -> AppBuilder {
        let Self {
            source,
            changes,
            api,
        } = self;
        let mut app = app;
        if let Source::Section(section) = &source {
            app = app.config_section(section.clone());
        }
        let resolved = Self::resolve(&source, changes);
        let shared = Arc::new(Shared::default());
        app = app.metrics_source("athena", Arc::clone(&shared.metrics) as _);
        if resolved.as_ref().is_ok_and(|config| config.health_check) {
            let check = WorkgroupCheck {
                shared: Arc::clone(&shared),
            };
            app = app.health_indicator("athena", Arc::new(check));
        }
        let on_start = Arc::clone(&shared);
        let resolved = Arc::new(resolved);
        app.on_startup(move |state| {
            let shared = Arc::clone(&on_start);
            let resolved = Arc::clone(&resolved);
            let api = api.clone();
            async move {
                let config = resolved
                    .as_ref()
                    .clone()
                    .map_err(|err| AutumnError::internal_server_error(AthenaError::Config(err)))?;
                let api: Arc<dyn AthenaApi> = match api {
                    Some(api) => api,
                    None => Arc::new(SdkAthena::from_config(&config).await),
                };
                let athena = Athena::with_parts(api, config, Arc::clone(&shared.metrics))
                    .map_err(AutumnError::internal_server_error)?;
                state.insert_extension(athena.clone());
                let _ = shared.handle.set(athena);
                tracing::info!("the Athena plugin is ready");
                Ok(())
            }
        })
        .on_shutdown(move || {
            let shared = Arc::clone(&shared);
            async move { shared.shutdown().await }
        })
    }
}

impl Athena {
    /// Gets the handle from the app state, for example in a job or a task.
    #[must_use]
    pub fn from_state(state: &AppState) -> Option<Self> {
        state.extension::<Self>().map(|athena| (*athena).clone())
    }
}

impl axum::extract::FromRequestParts<AppState> for Athena {
    type Rejection = AutumnError;

    async fn from_request_parts(
        _parts: &mut http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Self::from_state(state).ok_or_else(|| AthenaError::NotInstalled.into_autumn())
    }
}

#[cfg(test)]
mod tests;
