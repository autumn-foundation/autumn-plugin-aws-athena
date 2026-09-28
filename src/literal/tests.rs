use proptest::prelude::*;

use super::*;

/// Independent oracle: decodes one SQL string literal, or returns `None`.
fn decode_string_literal(sql: &str) -> Option<String> {
    let inner = sql.strip_prefix('\'')?.strip_suffix('\'')?;
    let mut out = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            // Inside a literal, a quote must be doubled.
            if chars.next() != Some('\'') {
                return None;
            }
        }
        out.push(c);
    }
    Some(out)
}

#[test]
fn null_and_bool_encode_as_keywords() {
    assert_eq!(Param::Null.to_sql(), "NULL");
    assert_eq!(Param::Bool(true).to_sql(), "true");
    assert_eq!(Param::Bool(false).to_sql(), "false");
}

#[test]
fn integers_encode_as_digits() {
    assert_eq!(Param::Int(0).to_sql(), "0");
    assert_eq!(Param::Int(-42).to_sql(), "-42");
    assert_eq!(Param::Int(i64::MAX).to_sql(), "9223372036854775807");
}

#[test]
fn smallest_integer_uses_a_typed_literal() {
    // `-9223372036854775808` is unary minus on a value out of bigint range.
    assert_eq!(
        Param::Int(i64::MIN).to_sql(),
        "BIGINT '-9223372036854775808'"
    );
}

#[test]
fn doubles_encode_with_an_exponent() {
    assert_eq!(Param::Double(1.5).to_sql(), "1.5e0");
    assert_eq!(Param::Double(-0.001).to_sql(), "-1e-3");
    assert_eq!(Param::Double(f64::NAN).to_sql(), "nan()");
    assert_eq!(Param::Double(f64::INFINITY).to_sql(), "infinity()");
    assert_eq!(Param::Double(f64::NEG_INFINITY).to_sql(), "-infinity()");
}

#[test]
fn text_doubles_single_quotes() {
    assert_eq!(Param::Text("abc".into()).to_sql(), "'abc'");
    assert_eq!(Param::Text("it's".into()).to_sql(), "'it''s'");
    assert_eq!(Param::Text(String::new()).to_sql(), "''");
    assert_eq!(
        Param::Text("x' OR '1'='1".into()).to_sql(),
        "'x'' OR ''1''=''1'"
    );
}

#[test]
fn binary_encodes_as_hex() {
    assert_eq!(Param::Binary(vec![0x0a, 0xff]).to_sql(), "X'0AFF'");
    assert_eq!(Param::Binary(Vec::new()).to_sql(), "X''");
}

#[test]
fn decimal_accepts_numbers_only() {
    assert_eq!(Param::decimal("12.50").unwrap().to_sql(), "DECIMAL '12.50'");
    assert_eq!(Param::decimal("-7").unwrap().to_sql(), "DECIMAL '-7'");
    for bad in ["", "-", "1.", ".5", "1e3", "1'", "1.2.3", " 1"] {
        assert!(Param::decimal(bad).is_err(), "{bad:?} must fail");
    }
}

#[test]
fn decimal_rejects_more_than_38_digits() {
    assert!(Param::decimal("1".repeat(38)).is_ok());
    assert!(Param::decimal("1".repeat(39)).is_err());
}

#[test]
fn date_accepts_iso_dates_only() {
    assert_eq!(
        Param::date("2024-02-29").unwrap().to_sql(),
        "DATE '2024-02-29'"
    );
    for bad in ["2024-2-29", "2024-13-01", "2024-00-10", "2024-01-32", "20240101", "2024-01-01'"] {
        assert!(Param::date(bad).is_err(), "{bad:?} must fail");
    }
}

#[test]
fn timestamp_accepts_iso_timestamps_only() {
    assert_eq!(
        Param::timestamp("2024-01-02 03:04:05").unwrap().to_sql(),
        "TIMESTAMP '2024-01-02 03:04:05'"
    );
    assert_eq!(
        Param::timestamp("2024-01-02 03:04:05.123456").unwrap().to_sql(),
        "TIMESTAMP '2024-01-02 03:04:05.123456'"
    );
    for bad in [
        "2024-01-02",
        "2024-01-02T03:04:05",
        "2024-01-02 24:00:00",
        "2024-01-02 03:60:00",
        "2024-01-02 03:04:60",
        "2024-01-02 03:04:05.",
        "2024-01-02 03:04:05.1234567890123",
        "2024-01-02 03:04:05Z",
    ] {
        assert!(Param::timestamp(bad).is_err(), "{bad:?} must fail");
    }
}

#[test]
fn conversions_pick_the_matching_kind() {
    assert_eq!(Param::from(7_i32), Param::Int(7));
    assert_eq!(Param::from(7_u32), Param::Int(7));
    assert_eq!(Param::from(true), Param::Bool(true));
    assert_eq!(Param::from(2.5_f64), Param::Double(2.5));
    assert_eq!(Param::from("a"), Param::Text("a".into()));
    assert_eq!(Param::from(String::from("a")), Param::Text("a".into()));
    assert_eq!(Param::from(None::<i64>), Param::Null);
    assert_eq!(Param::from(Some(3_i64)), Param::Int(3));
    assert_eq!(Param::from(vec![1_u8]), Param::Binary(vec![1]));
}

#[test]
fn large_u64_becomes_a_decimal() {
    assert_eq!(Param::from(5_u64), Param::Int(5));
    assert_eq!(
        Param::from(u64::MAX).to_sql(),
        "DECIMAL '18446744073709551615'"
    );
}

#[test]
fn identifiers_are_double_quoted() {
    assert_eq!(quote_identifier("orders").unwrap(), "\"orders\"");
    assert_eq!(quote_identifier("a\"b").unwrap(), "\"a\"\"b\"");
    assert!(quote_identifier("").is_err());
}

proptest! {
    #[test]
    fn any_text_is_one_literal_that_decodes_to_the_input(s in any::<String>()) {
        let sql = Param::Text(s.clone()).to_sql();
        prop_assert_eq!(decode_string_literal(&sql), Some(s));
    }

    #[test]
    fn any_finite_double_round_trips(v in any::<f64>().prop_filter("finite", |v| v.is_finite())) {
        let sql = Param::Double(v).to_sql();
        let parsed: f64 = sql.parse().unwrap();
        prop_assert_eq!(parsed.to_bits(), v.to_bits());
    }

    #[test]
    fn any_integer_round_trips(v in (i64::MIN + 1)..=i64::MAX) {
        prop_assert_eq!(Param::Int(v).to_sql().parse::<i64>().unwrap(), v);
    }

    #[test]
    fn valid_decimal_text_is_accepted(int in "-?[0-9]{1,20}", frac in proptest::option::of("[0-9]{1,10}")) {
        let text = frac.map_or_else(|| int.clone(), |f| format!("{int}.{f}"));
        let sql = Param::decimal(text.clone()).unwrap().to_sql();
        prop_assert_eq!(sql, format!("DECIMAL '{text}'"));
    }

    #[test]
    fn decimal_never_accepts_a_quote(s in ".*'.*") {
        prop_assert!(Param::decimal(s).is_err());
    }

    #[test]
    fn any_identifier_round_trips(s in ".+") {
        let quoted = quote_identifier(&s).unwrap();
        let inner = quoted.strip_prefix('"').unwrap().strip_suffix('"').unwrap();
        prop_assert_eq!(inner.replace("\"\"", "\""), s);
    }
}
