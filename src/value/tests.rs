use std::sync::Arc;

use proptest::prelude::*;
use serde::Deserialize;

use super::*;

fn col(name: &str, type_name: &str) -> Column {
    Column::new(name, type_name)
}

fn value(type_name: &str, raw: &str) -> Value {
    parse(&col("c", type_name), Some(raw)).unwrap()
}

fn row(columns: &[(&str, &str)], raw: &[Option<&str>]) -> Row {
    let columns: Arc<[Column]> = columns.iter().map(|(n, t)| col(n, t)).collect();
    let values = columns
        .iter()
        .zip(raw)
        .map(|(c, r)| parse(c, *r).unwrap())
        .collect();
    Row::new(columns, values)
}

#[test]
fn missing_text_is_null() {
    assert_eq!(parse(&col("c", "varchar"), None).unwrap(), Value::Null);
    assert_eq!(parse(&col("c", "bigint"), None).unwrap(), Value::Null);
}

#[test]
fn empty_varchar_is_an_empty_string() {
    assert_eq!(value("varchar", ""), Value::Text(String::new()));
}

#[test]
fn parses_integer_types() {
    for t in ["tinyint", "smallint", "integer", "int", "bigint", "BIGINT"] {
        assert_eq!(value(t, "-12"), Value::Int(-12), "{t}");
    }
}

#[test]
fn parses_floating_types() {
    assert_eq!(value("double", "1.5E10"), Value::Double(1.5e10));
    assert_eq!(value("real", "0.25"), Value::Double(0.25));
    assert!(matches!(value("double", "NaN"), Value::Double(v) if v.is_nan()));
    assert_eq!(value("double", "Infinity"), Value::Double(f64::INFINITY));
    assert_eq!(
        value("double", "-Infinity"),
        Value::Double(f64::NEG_INFINITY)
    );
}

#[test]
fn parses_other_scalar_types() {
    assert_eq!(value("boolean", "true"), Value::Bool(true));
    assert_eq!(value("boolean", "false"), Value::Bool(false));
    assert_eq!(
        value("decimal(10,2)", "12.50"),
        Value::Decimal("12.50".into())
    );
    assert_eq!(value("decimal", "12.50"), Value::Decimal("12.50".into()));
    assert_eq!(
        value("date", "2024-01-02"),
        Value::Date("2024-01-02".into())
    );
    assert_eq!(
        value("timestamp", "2024-01-02 03:04:05.000"),
        Value::Timestamp("2024-01-02 03:04:05.000".into())
    );
    assert_eq!(
        value("timestamp with time zone", "2024-01-02 03:04:05.000 UTC"),
        Value::Timestamp("2024-01-02 03:04:05.000 UTC".into())
    );
}

#[test]
fn parses_varbinary_hex_pairs() {
    assert_eq!(
        value("varbinary", "68 65 0a"),
        Value::Binary(vec![0x68, 0x65, 0x0a])
    );
    assert_eq!(value("varbinary", ""), Value::Binary(Vec::new()));
}

#[test]
fn keeps_complex_types_as_text() {
    assert_eq!(
        value("array(integer)", "[1, 2]"),
        Value::Text("[1, 2]".into())
    );
    assert_eq!(
        value("map(varchar,integer)", "{a=1}"),
        Value::Text("{a=1}".into())
    );
    assert_eq!(value("json", "{\"a\":1}"), Value::Text("{\"a\":1}".into()));
}

#[test]
fn bad_text_gives_an_error_that_names_the_column() {
    for (t, raw) in [
        ("bigint", "x"),
        ("integer", "1.5"),
        ("double", "one"),
        ("boolean", "yes"),
        ("varbinary", "6"),
        ("varbinary", "zz"),
    ] {
        let err = parse(&col("price", t), Some(raw)).unwrap_err();
        assert_eq!(err.column(), "price", "{t} {raw}");
    }
}

#[test]
fn get_finds_a_value_by_label() {
    let row = row(
        &[("id", "bigint"), ("name", "varchar")],
        &[Some("7"), Some("x")],
    );
    assert_eq!(row.get("name"), Some(&Value::Text("x".into())));
    assert_eq!(row.get("id"), Some(&Value::Int(7)));
    assert_eq!(row.get("missing"), None);
}

#[derive(Debug, Deserialize, PartialEq)]
struct Order {
    id: i64,
    customer: String,
    total: f64,
    paid: bool,
    note: Option<String>,
    qty: u16,
}

#[test]
fn deserializes_a_struct_by_label() {
    let row = row(
        &[
            ("id", "bigint"),
            ("customer", "varchar"),
            ("total", "decimal(10,2)"),
            ("paid", "boolean"),
            ("note", "varchar"),
            ("qty", "integer"),
        ],
        &[
            Some("1"),
            Some("c-1"),
            Some("9.95"),
            Some("true"),
            None,
            Some("3"),
        ],
    );
    assert_eq!(
        row.deserialize::<Order>().unwrap(),
        Order {
            id: 1,
            customer: "c-1".into(),
            total: 9.95,
            paid: true,
            note: None,
            qty: 3
        }
    );
}

#[test]
fn deserializes_a_tuple_by_position() {
    let row = row(
        &[("a", "bigint"), ("b", "varchar")],
        &[Some("2"), Some("x")],
    );
    assert_eq!(
        row.deserialize::<(i64, String)>().unwrap(),
        (2, "x".to_owned())
    );
}

#[test]
fn deserializes_numbers_into_strings() {
    #[derive(Deserialize)]
    struct Id {
        id: String,
    }
    let row = row(&[("id", "bigint")], &[Some("42")]);
    assert_eq!(row.deserialize::<Id>().unwrap().id, "42");
}

#[test]
fn deserializes_binary_into_bytes() {
    #[derive(Deserialize)]
    struct Blob {
        data: Vec<u8>,
    }
    let row = row(&[("data", "varbinary")], &[Some("01 02")]);
    assert_eq!(row.deserialize::<Blob>().unwrap().data, vec![1, 2]);
}

#[test]
fn deserialize_errors_are_decode_errors() {
    #[derive(Debug, Deserialize)]
    struct Id {
        #[allow(dead_code)]
        id: i64,
    }
    let row = row(&[("id", "varchar")], &[Some("abc")]);
    assert!(row.deserialize::<Id>().is_err());
    let row = row_null_id();
    assert!(row.deserialize::<Id>().is_err());
}

fn row_null_id() -> Row {
    row(&[("id", "bigint")], &[None])
}

#[test]
fn integer_out_of_range_fails() {
    #[derive(Debug, Deserialize)]
    struct Small {
        #[allow(dead_code)]
        v: u8,
    }
    let row = row(&[("v", "integer")], &[Some("300")]);
    assert!(row.deserialize::<Small>().is_err());
}

proptest! {
    #[test]
    fn any_bigint_text_parses(v in any::<i64>()) {
        prop_assert_eq!(value("bigint", &v.to_string()), Value::Int(v));
    }

    #[test]
    fn any_varchar_is_kept(s in any::<String>()) {
        prop_assert_eq!(value("varchar", &s), Value::Text(s));
    }

    #[test]
    fn any_bytes_round_trip(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        let raw = bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
        prop_assert_eq!(value("varbinary", &raw), Value::Binary(bytes));
    }
}

#[test]
fn a_signed_hex_pair_fails() {
    assert!(parse(&col("c", "varbinary"), Some("+1 +f")).is_err());
    assert!(parse(&col("c", "varbinary"), Some("-1")).is_err());
    assert_eq!(value("varbinary", "AB cd"), Value::Binary(vec![0xab, 0xcd]));
}

#[derive(Debug, Deserialize, PartialEq)]
enum Status {
    #[serde(rename = "open")]
    Open,
}

#[test]
fn deserializes_hints_from_text_and_numbers() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Hints {
        status: Status,
        ratio: f32,
        big: u64,
        wide: u128,
        count_as_float: f64,
        maybe: Option<u64>,
    }
    let row = row(
        &[
            ("status", "varchar"),
            ("ratio", "decimal(4,2)"),
            ("big", "decimal(20,0)"),
            ("wide", "decimal(38,0)"),
            ("count_as_float", "bigint"),
            ("maybe", "decimal(20,0)"),
        ],
        &[
            Some("open"),
            Some("0.25"),
            Some("18446744073709551615"),
            Some("99999999999999999999999999999999999999"),
            Some("7"),
            Some("5"),
        ],
    );
    assert_eq!(
        row.deserialize::<Hints>().unwrap(),
        Hints {
            status: Status::Open,
            ratio: 0.25,
            big: u64::MAX,
            wide: 99_999_999_999_999_999_999_999_999_999_999_999_999,
            count_as_float: 7.0,
            maybe: Some(5),
        }
    );
}

#[test]
fn hints_that_do_not_fit_fail() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct AsEnum {
        status: Status,
    }
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct AsInt {
        v: i64,
    }
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct AsFloat {
        v: f64,
    }
    assert!(row(&[("status", "bigint")], &[Some("1")]).deserialize::<AsEnum>().is_err());
    assert!(row(&[("v", "decimal(4,2)")], &[Some("12.00")]).deserialize::<AsInt>().is_err());
    // 2^53 + 1 has no exact f64.
    assert!(row(&[("v", "bigint")], &[Some("9007199254740993")]).deserialize::<AsFloat>().is_err());
}

#[test]
fn a_tuple_must_match_the_row_width() {
    let row = row(&[("a", "bigint"), ("b", "bigint")], &[Some("1"), Some("2")]);
    assert!(row.deserialize::<(i64,)>().is_err());
    assert!(row.deserialize::<(i64, i64, i64)>().is_err());
}

#[test]
fn duplicate_labels_fail_for_a_struct() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Id {
        id: i64,
    }
    let row = row(&[("id", "bigint"), ("id", "bigint")], &[Some("1"), Some("2")]);
    let err = row.deserialize::<Id>().unwrap_err();
    assert_eq!(err.column(), "");
    assert!(err.to_string().contains("duplicate field"), "{err}");
}

#[test]
fn an_optional_row_struct_is_some() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Id {
        id: i64,
    }
    let row = row(&[("id", "bigint")], &[Some("1")]);
    assert_eq!(row.deserialize::<Option<Id>>().unwrap(), Some(Id { id: 1 }));
}

#[test]
fn a_decode_error_names_the_field() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Id {
        id: i64,
    }
    let err = row(&[("id", "varchar")], &[Some("abc")]).deserialize::<Id>().unwrap_err();
    assert!(err.to_string().contains("abc"), "{err}");
}
