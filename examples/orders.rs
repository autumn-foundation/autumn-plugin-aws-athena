//! A small app that reads orders from Athena.
//!
//! Set `AUTUMN_ATHENA__DATABASE` and `AUTUMN_ATHENA__OUTPUT_LOCATION`, then run:
//!
//! ```sh
//! cargo run --example orders
//! ```

use autumn_plugin_aws_athena::{Athena, AthenaError, AthenaPlugin};
use autumn_web::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Order {
    id: i64,
    customer: String,
    total: f64,
}

#[get("/customers/{customer}/orders")]
async fn orders(athena: Athena, Path(customer): Path<String>) -> AutumnResult<Json<Vec<Order>>> {
    let rows = athena
        .query("SELECT id, customer, total FROM orders WHERE customer = ? LIMIT 100")
        .bind(customer)
        .fetch_as::<Order>()
        .await
        .map_err(AthenaError::into_autumn)?;
    Ok(Json(rows))
}

#[autumn_web::main]
async fn main() {
    autumn_web::app()
        .plugin(AthenaPlugin::new())
        .routes(routes![orders])
        .run()
        .await;
}
