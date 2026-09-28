# CLAUDE.md

Guidance for agents that work on this crate.

## What this crate is

`autumn-plugin-aws-athena` is an Autumn plugin. Autumn is `autumn-web` 0.7. Handlers run Amazon Athena queries through the `Athena` extractor. Read `docs/planning.md` before a design change.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo check --locked --lib
cargo test --locked --all-targets --all-features
cargo +1.91.0 check --locked --all-targets --all-features
cargo test --locked --doc --all-features
RUSTDOCFLAGS=-D\ warnings cargo doc --locked --no-deps --all-features
cargo llvm-cov --locked --all-features --ignore-filename-regex '/tests\.rs$' --fail-under-lines 90
```

The MSRV is 1.91. The lockfile holds AWS crates that support 1.91. Do not run `cargo update --ignore-rust-version` on the committed lockfile.

## Architecture

| Module | Kind | Job |
|--------|------|-----|
| `literal` | pure | Encodes `Param` values as SQL literals. Quotes identifiers. |
| `placeholder` | pure | Counts `?` outside literals and comments. |
| `backoff` | pure | The poll delay. |
| `value` | pure | Parses Athena text into `Value`. Serde row decoding. |
| `result` | pure | The header-row rule and the row width check. |
| `config` | pure | `AthenaConfig`, layering and validation. |
| `api` | seam | The `AthenaApi` trait and its data types. |
| `sdk` | glue | `AthenaApi` on `aws-sdk-athena`. |
| `client` | glue | `Athena` and `AthenaQuery`: start, poll, stop, read. |
| `plugin` | glue | `AthenaPlugin` and the extractor. |
| `health` | glue | The workgroup readiness check, with a 15-second cache. |
| `metrics` | glue | Counters and the metrics source. |
| `error` | data | `AthenaError` and the HTTP status map. |
| `testing` | public | `FakeAthena` (feature `test-support`). |

Each pure module has a `# Contract` doc section. Change the contract first. Then change the tests. Then change the code.

## Rules

- Work red, green, refactor. Write the failing test first.
- Put unit tests in `src/<module>/tests.rs`. The coverage gate ignores these files.
- Production code has no `unwrap`, `expect` or `panic`. Clippy denies them outside tests.
- Never put a value into SQL text. Bind it with `Param`.
- Never log SQL text or parameter values. Log the query ID.
- Never show AWS error text in health output. It can have account details.
- Each call to Athena has a time limit. Query calls use the query deadline. Stop calls have their own limit.
- A query with parameters never sends result reuse.
- A permanent API error stops the poll at once. A retryable error gets three polls.
- Metric names must not start with `autumn_`.
- No test calls AWS. Use `FakeAthena` for the client and `aws-smithy-mocks` for `sdk`.

## Test notes

- Client tests use `#[tokio::test(start_paused = true)]`. The clock is paused. Poll sleeps take no real time.
- `FakeQuery` has delays and errors for each call. Use them to test the deadline paths.
- `TestApp` runs startup hooks but not shutdown hooks. `plugin::tests` tests the shutdown hook.
- `AppState::begin_shutdown_for_test` marks the shutdown. The shutdown watch then stops the open queries.
- A dev-dependency on this crate turns on `test-support` for the integration tests.

## Documentation style

Write docs and comments in ASD-STE100 style: short sentences, active voice, simple present tense, one instruction per sentence. Keep instructions at 20 words or fewer and descriptions at 25 words or fewer.
