//! A small app that reads orders from Athena.
//!
//! Set `AUTUMN_ATHENA__DATABASE` and `AUTUMN_ATHENA__OUTPUT_LOCATION`. Then run the command below.
//!
//! ```sh
//! cargo run --example orders
//! ```

use autumn_plugin_aws_athena::{Athena, AthenaPlugin, AthenaResultExt as _};
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
        .or_http()?;
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
