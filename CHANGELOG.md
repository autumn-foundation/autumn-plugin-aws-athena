# Changelog

## [Unreleased]

## [0.2.0] - 2026-10-31

### Changed

- Support `autumn-web` 0.9. The plugin now needs `autumn-web` 0.9.

## 0.1.0

- `AthenaPlugin` and the `Athena` extractor.
- `AthenaQuery` with bound parameters, `execute`, `fetch` and `fetch_as`.
- A timeout, a row limit, a byte limit and a concurrency limit for each query.
- The plugin stops timed-out, dropped and open-at-shutdown queries in Athena.
- A readiness check and Prometheus metrics.
- `FakeAthena` for tests (feature `test-support`).
