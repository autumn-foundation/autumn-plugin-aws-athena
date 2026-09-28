# autumn-plugin-aws-athena

An [Autumn](https://github.com/autumn-foundation/autumn) plugin for [Amazon Athena](https://aws.amazon.com/athena/). Handlers run SQL queries with bound parameters and get typed rows.

- Bound parameters: each value becomes a safe SQL literal.
- Typed rows: `serde` decodes rows into your structs.
- Limits: a query timeout and a row limit. The plugin stops timed-out and dropped queries in Athena.
- Operations: a readiness check and Prometheus metrics.
- Tests: `FakeAthena` runs your handlers with no AWS account.

## Install

```toml
[dependencies]
autumn-plugin-aws-athena = "0.1"
```

```rust,ignore
use autumn_plugin_aws_athena::{Athena, AthenaError, AthenaPlugin};
use autumn_web::prelude::*;

#[derive(serde::Deserialize, serde::Serialize)]
struct Order {
    id: i64,
    total: f64,
}

#[get("/customers/{id}/orders")]
async fn orders(athena: Athena, Path(id): Path<String>) -> AutumnResult<Json<Vec<Order>>> {
    let rows = athena
        .query("SELECT id, total FROM orders WHERE customer_id = ?")
        .bind(id)
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
```

## Configuration

The plugin reads `[athena]` in `autumn.toml`. Profile sections and profile files override it. `AUTUMN_ATHENA__<KEY>` variables override all files. For example, `AUTUMN_ATHENA__POLL__MAX_MS` sets `poll.max_ms`.

```toml
[athena]
region = "eu-west-1"                     # Default: the AWS default chain.
endpoint_url = "http://localhost:4566"   # Optional. For a local emulator.
workgroup = "primary"
catalog = "AwsDataCatalog"               # Optional.
database = "sales"                       # Optional.
output_location = "s3://my-bucket/athena/" # Optional if the workgroup sets it.
timeout_ms = 300000                      # Includes the result read.
max_rows = 10000                         # More rows give an error.
page_size = 1000                         # 1 to 1000.
reuse_max_age_minutes = 0                # 0 disables result reuse. Maximum 10080.
health_check = true                      # Readiness check with GetWorkGroup.
cancel_on_drop = true                    # Stop a dropped query in Athena.

[athena.poll]
initial_ms = 200
max_ms = 2000
multiplier = 2.0
```

Credentials come from the AWS default chain. Set code values with `AthenaPlugin::configure`, or give the full configuration with `AthenaPlugin::config`.

## Queries

| Method | Result |
|--------|--------|
| `execute()` | Waits for the query. Reads no rows. Use it for DDL and `INSERT`. |
| `fetch()` | All rows as `Row` values, with the columns and statistics. |
| `fetch_as::<T>()` | All rows as `T`. A struct reads by label. A tuple reads by position. |

Each query can override the config: `database`, `catalog`, `workgroup`, `timeout`, `max_rows` and `reuse_results`.

### Parameters

Use `?` in the SQL and `bind` for each value. The plugin encodes each value as an Athena SQL literal:

| Rust value | SQL literal |
|------------|-------------|
| `&str`, `String` | `'it''s'` |
| integers | `42` |
| `f32`, `f64` | `1.5e0`, `nan()`, `infinity()` |
| `bool` | `true` |
| `None` | `NULL` |
| `Vec<u8>` | `X'0AFF'` |
| `Param::decimal("12.50")?` | `DECIMAL '12.50'` |
| `Param::date("2024-01-31")?` | `DATE '2024-01-31'` |
| `Param::timestamp("2024-01-31 12:00:00")?` | `TIMESTAMP '2024-01-31 12:00:00'` |

The plugin counts the `?` placeholders before the call. A mismatch gives `AthenaError::ParameterCount`. For a dynamic table name, use `literal::quote_identifier`.

### Values

| Athena type | `Value` |
|-------------|---------|
| `tinyint`, `smallint`, `integer`, `bigint` | `Int` |
| `real`, `double` | `Double` |
| `boolean` | `Bool` |
| `decimal` | `Decimal` (text) |
| `date` | `Date` (text) |
| `timestamp` | `Timestamp` (text) |
| `varbinary` | `Binary` |
| all other types | `Text` |

Athena gives `array`, `map` and `row` values in a text format that is not JSON. To get JSON, use `CAST(x AS JSON)` and parse the text.

## Errors

`AthenaError::into_autumn` gives an `AutumnError` with the correct status. The `?` operator also converts, but always gives status 500.

| Error | Status |
|-------|--------|
| `Timeout` | 504 |
| `Api`, `Cancelled`, retryable `Failed` | 503 |
| all other errors | 500 |

Autumn shows server error details only in development.

## Operations

- Readiness: the `athena` indicator calls `GetWorkGroup`. The output does not show AWS error details. The log has them.
- Metrics: `athena_queries_started_total`, `athena_queries_total{outcome}`, `athena_data_scanned_bytes_total` and `athena_queries_open`.
- Shutdown: the plugin stops each open query.
- Logs: the plugin logs query IDs. It does not log SQL text or parameter values.

## IAM permissions

The plugin calls `athena:StartQueryExecution`, `athena:GetQueryExecution`, `athena:GetQueryResults`, `athena:StopQueryExecution` and `athena:GetWorkGroup`. Athena also needs access to the S3 data, the S3 result location and the AWS Glue catalog.

## Tests

Turn on the `test-support` feature. Give `FakeAthena` to the plugin:

```rust,ignore
use autumn_plugin_aws_athena::testing::{FakeAthena, FakeQuery};

let fake = FakeAthena::new();
fake.push(
    FakeQuery::succeeded()
        .columns(&[("id", "bigint"), ("total", "double")])
        .row(&[Some("1"), Some("9.5")]),
);
let client = autumn_web::test::TestApp::new()
    .routes(routes![orders])
    .plugin(AthenaPlugin::new().config(AthenaConfig::default()).api(fake.clone()))
    .build();
client.get("/customers/c-1/orders").send().await.assert_ok();
assert_eq!(fake.started()[0].parameters, vec!["'c-1'".to_owned()]);
```

## Compatibility

- `autumn-web` 0.7.
- Rust 1.91 or later.

## License

Apache-2.0.
