use std::path::Path;
use std::time::Duration;

use autumn_web::config::MockEnv;

use super::*;

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

fn env_for(dir: &Path) -> MockEnv {
    MockEnv::new().with("AUTUMN_MANIFEST_DIR", dir.to_str().unwrap())
}

#[test]
fn defaults_are_safe() {
    let config = AthenaConfig::default();
    assert_eq!(config.workgroup, "primary");
    assert_eq!(config.timeout(), Duration::from_secs(300));
    assert_eq!(config.max_rows, 10_000);
    assert_eq!(config.page_size, 1000);
    assert_eq!(config.reuse_max_age_minutes, 0);
    assert!(config.health_check);
    assert!(config.cancel_on_drop);
    assert_eq!(config.poll.initial_ms, 200);
    assert_eq!(config.poll.max_ms, 2000);
    assert!((config.poll.multiplier - 2.0).abs() < f64::EPSILON);
    config.validate().unwrap();
}

#[test]
fn backoff_uses_the_poll_settings() {
    let backoff = AthenaConfig::default().backoff();
    assert_eq!(backoff.delay(0), Duration::from_millis(200));
    assert_eq!(backoff.delay(10), Duration::from_millis(2000));
}

#[test]
fn no_file_gives_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let config = AthenaConfig::resolve_with_env("athena", &env_for(dir.path())).unwrap();
    assert_eq!(config, AthenaConfig::default());
}

#[test]
fn reads_the_section_from_autumn_toml() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "autumn.toml",
        r#"
[athena]
region = "eu-west-1"
database = "sales"
output_location = "s3://results/athena/"
max_rows = 50

[athena.poll]
initial_ms = 100
"#,
    );
    let config = AthenaConfig::resolve_with_env("athena", &env_for(dir.path())).unwrap();
    assert_eq!(config.region.as_deref(), Some("eu-west-1"));
    assert_eq!(config.database.as_deref(), Some("sales"));
    assert_eq!(
        config.output_location.as_deref(),
        Some("s3://results/athena/")
    );
    assert_eq!(config.max_rows, 50);
    assert_eq!(config.poll.initial_ms, 100);
    assert_eq!(config.poll.max_ms, 2000);
}

#[test]
fn reads_a_custom_section() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "autumn.toml",
        "[reports]\nworkgroup = \"reports\"\n",
    );
    let config = AthenaConfig::resolve_with_env("reports", &env_for(dir.path())).unwrap();
    assert_eq!(config.workgroup, "reports");
}

#[test]
fn inline_profile_overrides_the_base() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "autumn.toml",
        r#"
[athena]
database = "dev_db"
max_rows = 5

[profile.prod.athena]
database = "prod_db"
"#,
    );
    let env = env_for(dir.path()).with("AUTUMN_ENV", "production");
    let config = AthenaConfig::resolve_with_env("athena", &env).unwrap();
    assert_eq!(config.database.as_deref(), Some("prod_db"));
    assert_eq!(config.max_rows, 5);
}

#[test]
fn profile_file_overrides_the_inline_profile() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "autumn.toml",
        "[athena]\ndatabase = \"base\"\n[profile.staging.athena]\ndatabase = \"inline\"\n",
    );
    write(
        dir.path(),
        "autumn-staging.toml",
        "[athena]\ndatabase = \"file\"\n",
    );
    let env = env_for(dir.path()).with("AUTUMN_PROFILE", "staging");
    let config = AthenaConfig::resolve_with_env("athena", &env).unwrap();
    assert_eq!(config.database.as_deref(), Some("file"));
}

#[test]
fn environment_overrides_the_files() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "autumn.toml",
        "[athena]\ndatabase = \"base\"\nmax_rows = 5\n",
    );
    let env = env_for(dir.path())
        .with("AUTUMN_ATHENA__DATABASE", "from_env")
        .with("AUTUMN_ATHENA__MAX_ROWS", "7")
        .with("AUTUMN_ATHENA__HEALTH_CHECK", "false")
        .with("AUTUMN_ATHENA__POLL__MULTIPLIER", "1.5")
        .with("AUTUMN_ATHENA__OUTPUT_LOCATION", "s3://env/");
    let config = AthenaConfig::resolve_with_env("athena", &env).unwrap();
    assert_eq!(config.database.as_deref(), Some("from_env"));
    assert_eq!(config.max_rows, 7);
    assert!(!config.health_check);
    assert!((config.poll.multiplier - 1.5).abs() < f64::EPSILON);
    assert_eq!(config.output_location.as_deref(), Some("s3://env/"));
}

#[test]
fn a_numeric_database_name_from_the_environment_stays_text() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_for(dir.path()).with("AUTUMN_ATHENA__DATABASE", "2024");
    let config = AthenaConfig::resolve_with_env("athena", &env).unwrap();
    assert_eq!(config.database.as_deref(), Some("2024"));
}

#[test]
fn a_bad_environment_value_names_the_variable() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_for(dir.path()).with("AUTUMN_ATHENA__MAX_ROWS", "many");
    let err = AthenaConfig::resolve_with_env("athena", &env).unwrap_err();
    assert!(err.to_string().contains("AUTUMN_ATHENA__MAX_ROWS"), "{err}");
}

#[test]
fn unknown_keys_fail() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "autumn.toml", "[athena]\nwork_group = \"x\"\n");
    assert!(AthenaConfig::resolve_with_env("athena", &env_for(dir.path())).is_err());
}

#[test]
fn a_section_that_is_not_a_table_fails() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "autumn.toml", "athena = 5\n");
    assert!(AthenaConfig::resolve_with_env("athena", &env_for(dir.path())).is_err());
}

#[test]
fn bad_toml_fails() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "autumn.toml", "[athena\n");
    assert!(AthenaConfig::resolve_with_env("athena", &env_for(dir.path())).is_err());
}

#[test]
fn resolve_validates_the_result() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "autumn.toml", "[athena]\npage_size = 0\n");
    let err = AthenaConfig::resolve_with_env("athena", &env_for(dir.path())).unwrap_err();
    assert!(err.to_string().contains("page_size"), "{err}");
}

fn invalid(change: impl FnOnce(&mut AthenaConfig)) -> String {
    let mut config = AthenaConfig::default();
    change(&mut config);
    config.validate().unwrap_err().to_string()
}

#[test]
fn validation_names_the_bad_key() {
    assert!(invalid(|c| c.workgroup = String::new()).contains("workgroup"));
    assert!(invalid(|c| c.workgroup = "a b".into()).contains("workgroup"));
    assert!(invalid(|c| c.workgroup = "w".repeat(129)).contains("workgroup"));
    assert!(invalid(|c| c.output_location = Some("results/".into())).contains("output_location"));
    assert!(invalid(|c| c.output_location = Some("s3://".into())).contains("output_location"));
    assert!(invalid(|c| c.region = Some(" ".into())).contains("region"));
    assert!(invalid(|c| c.region = Some("eu west".into())).contains("region"));
    assert!(invalid(|c| c.endpoint_url = Some("localhost:4566".into())).contains("endpoint_url"));
    assert!(invalid(|c| c.catalog = Some(String::new())).contains("catalog"));
    assert!(invalid(|c| c.database = Some(" ".into())).contains("database"));
    assert!(invalid(|c| c.timeout_ms = 0).contains("timeout_ms"));
    assert!(invalid(|c| c.max_rows = 0).contains("max_rows"));
    assert!(invalid(|c| c.page_size = 0).contains("page_size"));
    assert!(invalid(|c| c.page_size = 1001).contains("page_size"));
    assert!(invalid(|c| c.reuse_max_age_minutes = 10_081).contains("reuse_max_age_minutes"));
    assert!(invalid(|c| c.poll.initial_ms = 0).contains("poll.initial_ms"));
    assert!(invalid(|c| c.poll.max_ms = 100).contains("poll.max_ms"));
    assert!(invalid(|c| c.poll.multiplier = 0.5).contains("poll.multiplier"));
    assert!(invalid(|c| c.poll.multiplier = f64::NAN).contains("poll.multiplier"));
}

#[test]
fn valid_optional_values_pass() {
    let config = AthenaConfig {
        region: Some("us-east-1".into()),
        endpoint_url: Some("http://localhost:4566".into()),
        catalog: Some("AwsDataCatalog".into()),
        database: Some("sales".into()),
        output_location: Some("s3://bucket".into()),
        reuse_max_age_minutes: 10_080,
        ..AthenaConfig::default()
    };
    config.validate().unwrap();
}

fn leaf_paths(prefix: &str, table: &toml::Table, out: &mut Vec<String>) {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::Table(inner) => leaf_paths(&path, inner, out),
            _ => out.push(path),
        }
    }
}

#[test]
fn each_field_has_an_environment_variable() {
    let full = AthenaConfig {
        region: Some(String::new()),
        endpoint_url: Some(String::new()),
        catalog: Some(String::new()),
        database: Some(String::new()),
        output_location: Some(String::new()),
        expected_bucket_owner: Some(String::new()),
        ..AthenaConfig::default()
    };
    let toml::Value::Table(table) = toml::Value::try_from(&full).unwrap() else {
        panic!("a config must serialize as a table");
    };
    let mut fields = Vec::new();
    leaf_paths("", &table, &mut fields);
    fields.sort();
    let mut leaves: Vec<String> = LEAVES.iter().map(|(path, _)| (*path).to_owned()).collect();
    leaves.sort();
    assert_eq!(fields, leaves);
}

#[test]
fn plain_http_is_for_a_local_endpoint_only() {
    assert!(invalid(|c| c.endpoint_url = Some("http://athena.example.com".into())).contains("endpoint_url"));
    for local in ["http://localhost:4566", "http://127.0.0.1:4566", "http://[::1]:4566"] {
        let mut config = AthenaConfig::default();
        config.endpoint_url = Some(local.into());
        config.validate().unwrap();
    }
    let mut config = AthenaConfig::default();
    config.endpoint_url = Some("https://athena.example.com".into());
    config.validate().unwrap();
}

#[test]
fn the_bucket_owner_is_an_account_id() {
    assert!(invalid(|c| c.expected_bucket_owner = Some("12345".into())).contains("expected_bucket_owner"));
    assert!(invalid(|c| c.expected_bucket_owner = Some("12345678901x".into())).contains("expected_bucket_owner"));
    let mut config = AthenaConfig::default();
    config.expected_bucket_owner = Some("123456789012".into());
    config.validate().unwrap();
}

#[test]
fn the_byte_limit_must_be_positive() {
    assert!(invalid(|c| c.max_result_bytes = 0).contains("max_result_bytes"));
}

#[test]
fn validation_boundaries_pass() {
    let mut config = AthenaConfig::default();
    config.workgroup = "w".repeat(128);
    config.poll.max_ms = config.poll.initial_ms;
    config.poll.multiplier = 1.0;
    config.page_size = 1;
    config.max_concurrent_queries = 0;
    config.validate().unwrap();
    assert!(invalid(|c| c.poll.multiplier = f64::INFINITY).contains("poll.multiplier"));
}

#[test]
fn errors_name_a_custom_section() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "autumn.toml", "[reports]\npage_size = 0\n");
    let err = AthenaConfig::resolve_with_env("reports", &env_for(dir.path())).unwrap_err();
    assert!(err.to_string().contains("reports.page_size"), "{err}");
}

#[test]
fn a_section_name_with_a_dash_gives_a_valid_variable() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_for(dir.path()).with("AUTUMN_ATHENA_REPORTS__DATABASE", "r");
    let config = AthenaConfig::resolve_with_env("athena-reports", &env).unwrap();
    assert_eq!(config.database.as_deref(), Some("r"));
}

#[test]
fn the_canonical_inline_profile_wins_over_its_alias() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "autumn.toml",
        "[profile.production.athena]\ndatabase = \"alias\"\n[profile.prod.athena]\ndatabase = \"canonical\"\n",
    );
    let env = env_for(dir.path()).with("AUTUMN_ENV", "prod");
    let config = AthenaConfig::resolve_with_env("athena", &env).unwrap();
    assert_eq!(config.database.as_deref(), Some("canonical"));
}

#[test]
fn a_release_build_uses_the_prod_profile() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "autumn.toml", "[profile.prod.athena]\ndatabase = \"p\"\n");
    let env = env_for(dir.path()).with("AUTUMN_IS_DEBUG", "0");
    let config = AthenaConfig::resolve_with_env("athena", &env).unwrap();
    assert_eq!(config.database.as_deref(), Some("p"));
}

#[test]
fn only_the_first_profile_file_is_read() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "autumn-prod.toml", "[athena]\ndatabase = \"prod\"\n");
    write(dir.path(), "autumn-production.toml", "[athena]\ndatabase = \"production\"\nmax_rows = 3\n");
    let env = env_for(dir.path()).with("AUTUMN_ENV", "prod");
    let config = AthenaConfig::resolve_with_env("athena", &env).unwrap();
    assert_eq!(config.database.as_deref(), Some("prod"));
    assert_eq!(config.max_rows, 10_000);
}

#[test]
fn environment_values_parse_by_type() {
    let dir = tempfile::tempdir().unwrap();
    let base = env_for(dir.path());
    let config = AthenaConfig::resolve_with_env(
        "athena",
        &base.clone().with("AUTUMN_ATHENA__HEALTH_CHECK", "0").with("AUTUMN_ATHENA__CANCEL_ON_DROP", "1"),
    )
    .unwrap();
    assert!(!config.health_check);
    assert!(config.cancel_on_drop);
    for (key, value) in [
        ("AUTUMN_ATHENA__HEALTH_CHECK", "yes"),
        ("AUTUMN_ATHENA__POLL__MULTIPLIER", "fast"),
        ("AUTUMN_ATHENA__MAX_ROWS", "-1"),
    ] {
        let err = AthenaConfig::resolve_with_env("athena", &base.clone().with(key, value)).unwrap_err();
        assert!(err.to_string().contains(key), "{key}: {err}");
    }
}

#[test]
fn an_environment_path_through_a_value_fails() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "autumn.toml", "[athena]\npoll = 5\n");
    let env = env_for(dir.path()).with("AUTUMN_ATHENA__POLL__MAX_MS", "10");
    assert!(AthenaConfig::resolve_with_env("athena", &env).is_err());
}

#[test]
fn a_config_path_that_is_a_directory_fails() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("autumn.toml")).unwrap();
    assert!(AthenaConfig::resolve_with_env("athena", &env_for(dir.path())).is_err());
}
