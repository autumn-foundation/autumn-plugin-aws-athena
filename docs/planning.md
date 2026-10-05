# Planning

This document records the plan for `autumn-plugin-aws-athena`. It uses three methods: brainstorming, reverse brainstorming and six thinking hats. The last section gives the decisions and the TDD plan.

## Goal

An Autumn app runs Amazon Athena SQL queries from a handler. The app does one install step. The plugin keeps cost, time and memory in limits.

## 1. Brainstorming

Write all ideas first. Do not judge them in this step.

1. A handler extractor `Athena` that gives a client.
2. A fluent query builder: `athena.query(sql).bind(v).fetch_as::<T>()`.
3. Bind parameters with Athena `ExecutionParameters` and `?` placeholders.
4. Decode rows into `serde` structs by column name.
5. Typed values from the Athena column type (`bigint`, `double`, `boolean`).
6. Poll the query state with a capped exponential backoff.
7. A query timeout that stops the query in Athena.
8. Stop the query when the caller drops the future.
9. Stop all open queries at app shutdown.
10. A row limit that stops unbounded result reads.
11. Read the `[athena]` section of `autumn.toml`, with profiles and `AUTUMN_ATHENA__*` variables.
12. A readiness health indicator that reads the workgroup.
13. Prometheus metrics: queries by outcome, bytes scanned, open queries.
14. Result reuse (Athena caches results for a maximum age).
15. A test fake of the Athena API for users of the plugin.
16. A count check of `?` placeholders before a call to AWS.
17. Helpers that quote identifiers and literals.
18. Named parameters (`:name`).
19. A streaming row API.
20. Read results directly from the S3 CSV file.
21. Prepared statements (`PREPARE` and `EXECUTE`).
22. Map Athena errors to HTTP status codes.
23. Custom endpoint URL for LocalStack.
24. Query cost estimates from bytes scanned.

## 2. Reverse brainstorming

Question: "How can we make this plugin fail?" Each answer gives a countermeasure.

| How to make it fail | Countermeasure |
|---------------------|----------------|
| Put user input into the SQL text. | Bind values as encoded literals. Do not format SQL. |
| Encode a string literal badly, so that a quote ends it. | Double each `'`. Property tests prove one literal token. |
| Send the wrong number of parameters. | Count `?` outside strings and comments. Fail before the call. |
| Poll without end when Athena is slow. | Use a deadline. At the deadline, stop the query. |
| Poll too fast and cause throttling. | Use backoff with a cap. The SDK retries throttles. |
| Leave a query running after a client disconnects. | A drop guard stops the query. |
| Leave queries running at shutdown. | A shutdown hook stops all open queries. |
| Read a very large result into memory. | A row limit. Too many rows gives an error, not a partial result. |
| Treat the header row as data. | Skip row one on page one only for DML or unknown statements with matching labels. |
| Treat `NULL` as an empty string. | A missing `VarCharValue` is `NULL`. An empty value is `""`. |
| Log secrets or personal data. | Do not log SQL text or parameter values. Log the query ID. |
| Send internal Athena messages to HTTP clients. | The HTTP error body is generic. The log has the details. |
| Panic in a request path. | No `unwrap`, `expect` or `panic` in library code. Clippy denies them. |
| Start with a bad configuration and fail later. | Validate at startup. A bad value stops the boot. |
| Show "up" when AWS access is broken. | The readiness check calls `GetWorkGroup`. |
| Make tests depend on AWS. | A trait seam, a fake and SDK mocks. No test calls AWS. |

## 3. Six thinking hats

### White hat (facts)

- Autumn 0.8 gives `Plugin`, `on_startup`, `on_shutdown`, `health_indicator`, `metrics_source` and `config_section`.
- `Plugin::build` is synchronous. AWS config loads asynchronously, so the client starts in `on_startup`.
- Athena states are `QUEUED`, `RUNNING`, `SUCCEEDED`, `FAILED` and `CANCELLED`.
- `GetQueryResults` returns 1000 rows or fewer on each page. All values are text.
- Execution parameters are SQL literals. A string needs quotes.
- The SDK fills `ClientRequestToken`, so a retried start is idempotent.
- `aws-smithy-mocks` mocks the SDK client with no network.

### Red hat (feelings)

- Users want one line of setup and one line for a query.
- A surprise AWS bill is the worst result. Cost control must be on by default.
- Silent truncation of results feels unsafe.

### Black hat (risks)

- AWS SDK crates raise their minimum Rust version often. Pin the versions in a lockfile. Test with the lockfile.
- A placeholder counter can disagree with the Athena parser. Keep the lexer small and documented.
- The Athena text format of `array`, `map` and `row` is not JSON. We keep these as text.
- A drop guard needs a Tokio runtime. Without one, it only logs.

### Yellow hat (benefits)

- The fake API lets users test handlers with no AWS account.
- Encoded parameters remove the main SQL injection path.
- Metrics and health checks make the plugin safe to operate.

### Green hat (new ideas)

- A `testing` module with a scripted `FakeAthena`.
- `Param` values with validation for `DATE`, `TIMESTAMP` and `DECIMAL`.
- A serde deserializer over `Value`, so `NaN` and big decimals survive.

### Blue hat (process)

- Pure modules first. Each gets a `# Contract` section and property tests.
- Glue modules next. Each gets tests with the fake or with SDK mocks.
- Each cycle is red, then green, then refactor. Each phase gets a commit.
- At the end, review agents check the code from different angles.

## 4. Decisions

### In scope

Ideas 1 to 17, 22 and 23.

### Out of scope

| Idea | Reason |
|------|--------|
| 18. Named parameters | Athena supports only `?`. |
| 19. Streaming rows | The row limit covers the main need. Add later if users ask. |
| 20. Read CSV from S3 | It needs S3 access and a CSV parser. |
| 21. Prepared statements | Execution parameters give the same safety. |
| 24. Cost estimates | Prices change by region. Bytes scanned is in the metrics. |

## 5. Architecture

| Module | Kind | Job |
|--------|------|-----|
| `literal` | pure | Encodes `Param` values as Athena SQL literals. Quotes identifiers. |
| `placeholder` | pure | Counts `?` placeholders outside strings and comments. |
| `backoff` | pure | Gives the poll delay for each attempt. |
| `value` | pure | Parses Athena text into `Value`. Deserializes rows with serde. |
| `result` | pure | Skips the header row and builds `Row` values. |
| `config` | pure | `AthenaConfig`, layering and validation. |
| `api` | seam | The `AthenaApi` trait and its data types. |
| `sdk` | glue | `AthenaApi` on the AWS SDK client. |
| `client` | glue | `Athena` and `Query`: start, poll, stop, fetch. |
| `plugin` | glue | `AthenaPlugin` builder and `Plugin::build`. |
| `health` | glue | The readiness indicator. |
| `metrics` | glue | Counters and the metrics source. |
| `error` | data | `AthenaError` and the HTTP mapping. |
| `testing` | public | `FakeAthena` for tests (feature `test-support`). |

## 6. TDD plan

Each item is one cycle. Red: write a test that fails. Green: write the minimum code. Refactor: clean up with all tests green.

1. `literal`: each `Param` kind encodes. Property: any string gives one literal that decodes to the input.
2. `placeholder`: counts in plain SQL, strings, identifiers and comments. Property: `?` in literals never counts.
3. `backoff`: the first delay, growth and cap. Property: monotonic and in bounds.
4. `api`: each Athena state maps to `QueryState`. Unknown states are not terminal.
5. `value`: each column type parses. Invalid numbers give an error.
6. `result`: header skip rules. Row width checks.
7. `config`: defaults, TOML, profile layers, environment variables and validation.
8. `sdk`: each call maps to and from SDK types with `aws-smithy-mocks`.
9. `client`: success, failure, cancel, timeout, row limit and pages, all on the fake.
10. `client`: the drop guard and the shutdown stop.
11. `plugin`: the extractor, health, metrics and boot errors, in `TestApp`.

## 7. Review

Five review agents read the code. Each agent had one angle:

| Angle | Main findings |
|-------|---------------|
| Correctness and concurrency | Autumn can end the process before the shutdown hooks. A huge timeout panics. Stop calls have no limit. A start timeout loses the query ID. |
| Security | Unchecked typed variants skip validation. A negative number after `-` makes a comment. Debug output shows SQL. |
| Athena semantics and cost | Result reuse can ignore parameter values. A throttled poll stops a paid query. Parameters over 1024 characters fail late. |
| API and documentation | `Query` clashes with the Autumn extractor. `ApiError` is always retryable. Some docs do not match the code. |
| Test quality | No test reaches the deadline paths. Generators miss quotes and comments. Some assertions are weak. |

Each finding got a red test first and then a fix. These items stay out of scope for 0.1:

- `UpdateCount` for DML results.
- `EncryptionConfiguration` for a client-side result location. S3 default encryption covers most cases.
- More than one plugin in one app.
