//! Autumn plugin for Amazon Athena.
//!
//! Add [`AthenaPlugin`] to the app. Then use the [`Athena`] extractor in a handler.
//!
//! ```rust,no_run
//! use autumn_plugin_aws_athena::{Athena, AthenaError, AthenaPlugin};
//! use autumn_web::prelude::*;
//!
//! #[derive(serde::Deserialize, serde::Serialize)]
//! struct Order {
//!     id: i64,
//!     total: f64,
//! }
//!
//! #[get("/customers/{id}/orders")]
//! async fn orders(athena: Athena, Path(id): Path<String>) -> AutumnResult<Json<Vec<Order>>> {
//!     let rows = athena
//!         .query("SELECT id, total FROM orders WHERE customer_id = ?")
//!         .bind(id)
//!         .fetch_as::<Order>()
//!         .await
//!         .map_err(AthenaError::into_autumn)?;
//!     Ok(Json(rows))
//! }
//!
//! # async fn run() {
//! autumn_web::app()
//!     .plugin(AthenaPlugin::new())
//!     .routes(routes![orders])
//!     .run()
//!     .await;
//! # }
//! ```
//!
//! The plugin reads `[athena]` in `autumn.toml`. See [`config`] for the keys.
//!
//! # Safety rules
//!
//! - Bind each value with [`Query::bind`]. Do not put values into the SQL text.
//! - Each query has a timeout and a row limit. The plugin stops a timed-out query in Athena.
//! - Logs have query IDs. Logs do not have SQL text or parameter values.

pub mod api;
mod backoff;
mod client;
pub mod config;
mod error;
mod health;
pub mod literal;
mod metrics;
mod placeholder;
mod plugin;
mod result;
pub mod sdk;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub mod value;

pub use api::{Column, StatementType, Statistics};
pub use client::{Athena, Execution, Query, QueryOutput};
pub use error::AthenaError;
pub use literal::Param;
pub use plugin::{AthenaPlugin, PLUGIN_NAME};
pub use sdk::SdkAthena;
pub use value::{Row, Value};
