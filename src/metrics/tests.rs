use autumn_web::actuator::MetricsSource;

use super::*;

fn value(metrics: &Metrics, name: &str, label: Option<&str>) -> f64 {
    let families = metrics.collect();
    let family = families.iter().find(|f| f.name == name).unwrap();
    family
        .samples
        .iter()
        .find(|s| label.is_none_or(|l| s.labels.iter().any(|(_, v)| v == l)))
        .unwrap()
        .value
}

#[test]
fn counts_each_outcome_and_the_open_queries() {
    let metrics = Metrics::default();
    metrics.started();
    metrics.started();
    assert!((value(&metrics, "athena_queries_open", None) - 2.0).abs() < f64::EPSILON);
    metrics.ended(Outcome::Succeeded, 100);
    metrics.ended(Outcome::TimedOut, 5);
    assert!((value(&metrics, "athena_queries_started_total", None) - 2.0).abs() < f64::EPSILON);
    assert!((value(&metrics, "athena_queries_total", Some("succeeded")) - 1.0).abs() < f64::EPSILON);
    assert!((value(&metrics, "athena_queries_total", Some("timed_out")) - 1.0).abs() < f64::EPSILON);
    assert!((value(&metrics, "athena_queries_total", Some("failed"))).abs() < f64::EPSILON);
    assert!((value(&metrics, "athena_data_scanned_bytes_total", None) - 105.0).abs() < f64::EPSILON);
    assert!((value(&metrics, "athena_queries_open", None)).abs() < f64::EPSILON);
}

#[test]
fn the_open_gauge_never_goes_below_zero() {
    let metrics = Metrics::default();
    metrics.ended(Outcome::Failed, 0);
    assert!((value(&metrics, "athena_queries_open", None)).abs() < f64::EPSILON);
}

#[test]
fn no_metric_name_uses_the_autumn_prefix() {
    for family in Metrics::default().collect() {
        assert!(!family.name.starts_with("autumn_"), "{}", family.name);
    }
}
