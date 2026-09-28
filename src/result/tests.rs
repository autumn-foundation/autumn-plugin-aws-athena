use std::sync::Arc;

use super::*;
use crate::value::Value;

fn columns() -> Arc<[Column]> {
    vec![Column::new("id", "bigint"), Column::new("name", "varchar")].into()
}

fn raw(values: &[Option<&str>]) -> Vec<Option<String>> {
    values.iter().map(|v| v.map(str::to_owned)).collect()
}

#[test]
fn a_dml_row_with_the_labels_is_a_header() {
    assert!(is_header(
        StatementType::Dml,
        &columns(),
        &raw(&[Some("id"), Some("name")])
    ));
}

#[test]
fn a_row_with_other_values_is_data() {
    assert!(!is_header(
        StatementType::Dml,
        &columns(),
        &raw(&[Some("1"), Some("name")])
    ));
    assert!(!is_header(
        StatementType::Dml,
        &columns(),
        &raw(&[Some("id"), None])
    ));
    assert!(!is_header(
        StatementType::Dml,
        &columns(),
        &raw(&[Some("id")])
    ));
}

#[test]
fn only_dml_results_have_a_header() {
    let labels = raw(&[Some("id"), Some("name")]);
    assert!(!is_header(StatementType::Utility, &columns(), &labels));
    assert!(!is_header(StatementType::Ddl, &columns(), &labels));
}

#[test]
fn an_unknown_statement_with_the_labels_is_a_header() {
    // Fail safe: a missing type must not add a label row to the data.
    let labels = raw(&[Some("id"), Some("name")]);
    assert!(is_header(StatementType::Unknown, &columns(), &labels));
}

#[test]
fn decodes_each_row() {
    let rows = decode_rows(
        &columns(),
        vec![raw(&[Some("1"), Some("a")]), raw(&[Some("2"), None])],
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].values(), &[Value::Int(2), Value::Null]);
}

#[test]
fn a_row_with_the_wrong_width_fails() {
    assert!(decode_rows(&columns(), vec![raw(&[Some("1")])]).is_err());
}

#[test]
fn a_bad_value_fails() {
    assert!(decode_rows(&columns(), vec![raw(&[Some("x"), Some("a")])]).is_err());
}
