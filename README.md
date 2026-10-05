# autumn-plugin-aws-athena

An [Autumn](https://github.com/autumn-foundation/autumn) plugin for [Amazon Athena](https://aws.amazon.com/athena/). Handlers run SQL queries with bound parameters and get typed rows.

- Bound parameters: each value becomes a safe SQL literal.
- Typed rows: `serde` decodes rows into your structs.
- Limits: a timeout, a row limit, a byte limit and a concurrency limit for each query. The plugin stops timed-out and dropped queries in Athena.
- Operations: a readiness check and Prometheus metrics.
- Tests: `FakeAthena` replaces Athena in tests. The tests need no AWS account.

## Install

```toml
[dependencies]
autumn-plugin-aws-athena = "0.1"
```

```rust,ignore
use autumn_plugin_aws_athena::{Athena, AthenaPlugin, AthenaResultExt as _};
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
```

An app can have one Athena plugin only.

## Configuration

The plugin reads `[athena]` in `autumn.toml`. Profile sections and profile files override it. `AUTUMN_ATHENA__<KEY>` variables override all files. For example, `AUTUMN_ATHENA__POLL__MAX_MS` sets `poll.max_ms`.

```toml
[athena]
region = "eu-west-1"                     # Default: the AWS default chain.
endpoint_url = "http://localhost:4566"   # Optional. Plain HTTP is for the local host only.
workgroup = "primary"
catalog = "AwsDataCatalog"               # Optional.
database = "sales"                       # Optional.
output_location = "s3://my-bucket/athena/" # Optional if the workgroup sets it.
expected_bucket_owner = "123456789012"   # Optional. The account that must own the bucket.
timeout_ms = 300000                      # Includes the wait for a slot and the result read.
max_rows = 10000                         # More rows give an error.
max_result_bytes = 67108864              # More bytes of values give an error.
max_concurrent_queries = 16              # For each process. 0 removes the limit.
page_size = 1000                         # 1 to 1000.
reuse_max_age_minutes = 0                # 0 disables result reuse. Maximum 10080.
health_check = true                      # Readiness check with GetWorkGroup.
cancel_on_drop = true                    # Stop a dropped query in Athena.

[athena.poll]
initial_ms = 200
max_ms = 2000
multiplier = 2.0
```

Credentials come from the AWS default chain. Set code values with `AthenaPlugin::configure`. Give the full configuration with `AthenaPlugin::config`.

Set `BytesScannedCutoffPerQuery` on the workgroup. This limits the cost of one query. The plugin cannot do this.

## Queries

| Method | Result |
|--------|--------|
| `execute()` | Waits for the query. Reads no rows. Use it for DDL and `INSERT`. |
| `fetch()` | All rows as `Row` values, with the columns and statistics. |
| `fetch_as::<T>()` | All rows as `T`. A struct reads by label. A tuple reads by position. |

Each query can override the config: `database`, `catalog`, `workgroup`, `timeout`, `max_rows` and `reuse_results`.

A query with parameters never uses result reuse. Athena can match a cached result for other values.

### Parameters

Use `?` in the SQL and `bind` for each value. The plugin encodes each value as an Athena SQL literal:

| Rust value | SQL literal |
|------------|-------------|
| `&str`, `String` | `'it''s'` |
| integers | `42`, `(-5)` |
| `f32`, `f64` | `1.5e0`, `(-1e-3)`, `nan()`, `infinity()` |
| `bool` | `true` |
| `None` | `NULL` |
| `Vec<u8>` | `X'0AFF'` |
| `Param::decimal("12.50")?` | `DECIMAL '12.50'` |
| `Param::date("2024-01-31")?` | `DATE '2024-01-31'` |
| `Param::timestamp("2024-01-31 12:00:00")?` | `TIMESTAMP '2024-01-31 12:00:00'` |

The plugin counts the `?` placeholders before the call. A mismatch gives `AthenaError::ParameterCount`. An encoded value can have 1024 characters or fewer. For a dynamic table name in DML, use `literal::quote_identifier`.

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

Athena gives `array`, `map` and `row` values in a text format that is not JSON. Use `CAST(x AS JSON)` in the SQL. Then parse the text as JSON. A `char` value has spaces at the end.

## Errors

`or_http()` (from `AthenaResultExt`) and `AthenaError::into_autumn` give an `AutumnError` with the status in this table. The `?` operator also converts, but it always gives status 500.

| Error | Status |
|-------|--------|
| `Timeout` | 504 |
| retryable `Api`, `Cancelled`, `ShuttingDown`, retryable `Failed` | 503 |
| all other errors | 500 |

Autumn shows server error details only in development. The message of `Failed` does not show the Athena reason, because the reason can repeat parameter values. The `reason` field has it.

## Operations

- Readiness: the `athena` indicator calls `GetWorkGroup`. It keeps each result for 15 seconds. A disabled workgroup is down. The output does not show AWS error details. The log has them.
- Metrics: `athena_queries_started_total`, `athena_queries_total{outcome}`, `athena_data_scanned_bytes_total` and `athena_queries_open`.
- Shutdown: when Autumn marks the shutdown, the plugin stops each open query and refuses new queries.
- Logs: the plugin logs query IDs. It does not log SQL text or parameter values.

## IAM permissions

The plugin calls `athena:StartQueryExecution`, `athena:GetQueryExecution`, `athena:GetQueryResults`, `athena:StopQueryExecution` and `athena:GetWorkGroup`. Athena also needs access to the S3 data, the S3 result location and the AWS Glue catalog.

## Tests

Turn on the `test-support` feature for your tests:

```toml
[dev-dependencies]
autumn-plugin-aws-athena = { version = "0.1", features = ["test-support"] }
```

Give `FakeAthena` to the plugin:

```rust,ignore
use autumn_plugin_aws_athena::testing::{FakeAthena, FakeQuery};
use autumn_plugin_aws_athena::{AthenaConfig, AthenaPlugin};

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

- `autumn-web` 0.8.
- Rust 1.91 or later.

## License

Apache-2.0.
